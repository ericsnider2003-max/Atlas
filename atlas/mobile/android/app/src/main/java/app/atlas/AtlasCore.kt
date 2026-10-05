package app.atlas

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.Uri
import org.json.JSONObject
import java.io.File
import java.net.HttpURLConnection
import java.net.URL

/** Atlas itself, running on this phone: the same Rust core as the laptop. */
object AtlasCore {
    init { System.loadLibrary("atlas_jni") }

    @JvmStatic private external fun start(home: String, port: Int): Int
    @JvmStatic private external fun url(): String?
    /** 0 not started, 1 starting, 2 running, -1 couldn't start. */
    @JvmStatic private external fun state(): Int
    @JvmStatic private external fun stopCore()
    /** Wifi or another network that isn't metered: the phone's own model downloads by itself only then. */
    @JvmStatic private external fun network(unmetered: Boolean)

    @Volatile var hub: Uri? = null
        private set

    private var watching = false

    /**
     * Start once and wait for the hub. Call OFF the main thread (see
     * [ensureAsync]): 28 Sep 2026, this ran in onCreate and the core waited
     * up to 20 s for the hub, freezing the app; a slow start then began a
     * second Atlas. The core now returns at once (2: still starting) and never
     * starts two; this polls for the address.
     */
    @Synchronized fun ensure(ctx: Context): Uri? {
        hub?.let { return it }
        val home = File(ctx.filesDir, "atlas")
        val cfg = File(home, "config")
        if (!cfg.exists()) {
            copyAssets(ctx, "config", cfg)
        }
        val rc = start(home.absolutePath, 0)
        if (rc < 0) return null
        // Up to a minute: a first start on an old phone reads a lot.
        for (i in 0 until 600) {
            url()?.let { hub = Uri.parse(it) }
            if (hub != null || state() < 0) break
            Thread.sleep(100)
        }
        if (hub != null && !watching) { watching = true; watchNetwork(ctx) }
        return hub
    }

    /** [ensure] on a background thread; [done] runs on the main thread. */
    fun ensureAsync(ctx: Context, done: (Uri?) -> Unit) {
        val app = ctx.applicationContext
        val main = android.os.Handler(android.os.Looper.getMainLooper())
        Thread({ val u = runCatching { ensure(app) }.getOrNull(); main.post { done(u) } }, "atlas-start").start()
    }

    /** Stop Atlas, and forget its address: a stopped hub isn't pointed at again (28 Sep 2026). */
    @Synchronized fun stop() {
        hub = null
        stopCore()
    }

    /** Tell Atlas whenever the phone is on wifi (unmetered) or not. */
    private fun watchNetwork(ctx: Context) {
        val cm = ctx.getSystemService(ConnectivityManager::class.java) ?: return
        runCatching {
            cm.registerDefaultNetworkCallback(object : ConnectivityManager.NetworkCallback() {
                override fun onCapabilitiesChanged(n: Network, caps: NetworkCapabilities) {
                    network(caps.hasCapability(NetworkCapabilities.NET_CAPABILITY_NOT_METERED))
                }
                override fun onLost(n: Network) = network(false)
            })
        }
    }

    /** Copy an asset folder, subfolders included (config/guessable/…). */
    private fun copyAssets(ctx: Context, from: String, to: File) {
        val children = ctx.assets.list(from).orEmpty()
        if (children.isEmpty()) {
            // A file: `list` is empty for files, and for empty folders, which
            // `open` refuses — so an empty folder is simply made.
            runCatching { ctx.assets.open(from) }.getOrNull()?.use { i ->
                to.parentFile?.mkdirs()
                to.outputStream().use { i.copyTo(it) }
            } ?: to.mkdirs()
            return
        }
        to.mkdirs()
        for (name in children) copyAssets(ctx, "$from/$name", File(to, name))
    }

    val token: String? get() = hub?.getQueryParameter("t")

    /** An address on the hub, e.g. "/hub/give?text=…". */
    fun at(path: String): String? = hub?.let { "${it.scheme}://${it.encodedAuthority}$path" }

    /** What Atlas is doing and what's ready: /hub/live.json, off the main thread. */
    fun live(): JSONObject? = get("/hub/live.json")

    /** The home-screen widget's glance: /hub/glance.json (glance.rs). */
    fun glance(): JSONObject? = get("/hub/glance.json")

    /** POST a JSON body to the hub; true when it was taken (2xx). */
    fun post(path: String, body: JSONObject): Boolean {
        val u = at(path) ?: return false
        val t = token ?: return false
        return runCatching {
            val c = URL(u).openConnection() as HttpURLConnection
            c.requestMethod = "POST"
            c.doOutput = true
            c.setRequestProperty("Authorization", "Bearer $t")
            c.setRequestProperty("Content-Type", "application/json")
            c.connectTimeout = 2000; c.readTimeout = 4000
            c.outputStream.use { it.write(body.toString().toByteArray()) }
            c.responseCode in 200..299
        }.getOrDefault(false)
    }

    private fun get(path: String): JSONObject? {
        val u = at(path) ?: return null
        val t = token ?: return null
        return runCatching {
            val c = URL(u).openConnection() as HttpURLConnection
            c.setRequestProperty("Authorization", "Bearer $t")
            c.connectTimeout = 2000; c.readTimeout = 4000
            c.inputStream.bufferedReader().use { JSONObject(it.readText()) }
        }.getOrNull()
    }
}
