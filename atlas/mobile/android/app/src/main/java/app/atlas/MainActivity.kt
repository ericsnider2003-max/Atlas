package app.atlas

import android.Manifest
import android.annotation.SuppressLint
import android.app.Activity
import android.content.Intent
import android.content.pm.PackageManager
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.speech.RecognitionListener
import android.speech.RecognizerIntent
import android.speech.SpeechRecognizer
import android.speech.tts.TextToSpeech
import android.webkit.JavascriptInterface
import android.webkit.WebView
import android.webkit.WebViewClient
import android.widget.TextView
import org.json.JSONArray

/**
 * The hub, full screen. The phone design is the hub's own pages at phone
 * width, so every page the laptop has, the phone has. The page follows the
 * phone's font size (WebView text zoom), dark mode and animation settings.
 */
class MainActivity : Activity() {
    private lateinit var web: WebView
    private var ears: SpeechRecognizer? = null
    private var voice: TextToSpeech? = null

    /** An intent that arrived before the hub was up: handled once it is. */
    private var waiting: Intent? = null

    override fun onCreate(saved: Bundle?) {
        super.onCreate(saved)
        // Started off the main thread (28 Sep 2026: onCreate waited up to
        // 20 s for the hub and the app froze); said meanwhile.
        setContentView(TextView(this).apply {
            text = "Starting Atlas…"
            setPadding(48, 48, 48, 48)
        })
        waiting = intent
        askOnce()
        AtlasCore.ensureAsync(this) { hub ->
            if (isFinishing || isDestroyed) return@ensureAsync
            if (hub == null) {
                setContentView(TextView(this).apply {
                    text = "Atlas couldn't start on this phone. Its data is safe; try opening it again."
                    setPadding(48, 48, 48, 48)
                })
            } else {
                show(hub)
            }
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun show(hub: Uri) {
        startForegroundService(Intent(this, AtlasService::class.java))
        // Ask for a push address each time the app opens (item 15); the
        // distributor hands back the same one unless it changed.
        PushReceiver.register(this)
        web = WebView(this)
        web.settings.javaScriptEnabled = true
        web.settings.domStorageEnabled = true
        // Follow the phone's font size setting (WCAG 1.4.4 / EN 301 549 11.7).
        web.settings.textZoom = (resources.configuration.fontScale * 100).toInt()
        web.webViewClient = object : WebViewClient() {
            // The hub stays in the app; anything else opens in the browser.
            override fun shouldOverrideUrlLoading(v: WebView, r: android.webkit.WebResourceRequest): Boolean {
                if (r.url.host == "127.0.0.1") return false
                startActivity(Intent(Intent.ACTION_VIEW, r.url)); return true
            }
        }
        web.addJavascriptInterface(Shell(), "AtlasShell")
        setContentView(web)
        web.loadUrl(hub.toString())
        waiting?.let { waiting = null; handle(it) }
    }

    /**
     * Asked once, on the first launch, together: the phone's calendar kept
     * with Atlas's (H7), and -- on Android 13 and later -- showing
     * notifications, without which the "what Atlas is doing" and "ready for
     * you" notifications never appear (28 Sep 2026: declared, never asked).
     * A no is kept by Android.
     */
    private fun askOnce() {
        val prefs = getSharedPreferences("asked", MODE_PRIVATE)
        val wanted = mutableListOf<String>()
        if (!prefs.getBoolean("calendar", false)) {
            prefs.edit().putBoolean("calendar", true).apply()
            wanted += listOf(Manifest.permission.READ_CALENDAR, Manifest.permission.WRITE_CALENDAR)
        }
        if (Build.VERSION.SDK_INT >= 33 && !prefs.getBoolean("notifications", false)) {
            prefs.edit().putBoolean("notifications", true).apply()
            if (checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
                wanted += Manifest.permission.POST_NOTIFICATIONS
            }
        }
        if (wanted.isNotEmpty()) requestPermissions(wanted.toTypedArray(), 8)
    }

    override fun onNewIntent(i: Intent) {
        super.onNewIntent(i)
        if (::web.isInitialized) handle(i) else waiting = i
    }

    /**
     * Share → Atlas opens Give with the words in its box; an atlas:// link
     * (a notification tap, the widget) opens its page. Only ever navigation:
     * this activity is open to every app on the phone, so nothing that
     * arrives here may change anything by itself (28 Sep 2026: a link or a
     * share handed text straight to Atlas). You press Give to hand it over.
     */
    private fun handle(i: Intent?) {
        if (i == null || !::web.isInitialized) return
        when (i.action) {
            Intent.ACTION_SEND -> {
                val text = listOfNotNull(i.getStringExtra(Intent.EXTRA_SUBJECT), i.getStringExtra(Intent.EXTRA_TEXT)).joinToString("\n")
                if (text.isNotBlank()) AtlasCore.at("/hub/give?draft=" + Uri.encode(text))?.let(web::loadUrl)
            }
            Intent.ACTION_VIEW -> i.data?.takeIf { it.scheme == "atlas" }?.let { d ->
                // atlas://hub/<page> (or atlas://give): the page, and nothing
                // of the query but words for Give's box.
                val page = if (d.host == "give") "/give" else (d.path ?: "")
                val safe = page.all { it.isLetterOrDigit() || it == '/' || it == '-' || it == '_' }
                var path = ("/hub" + if (safe) page else "").trimEnd('/')
                val words = d.getQueryParameter("text")
                if (path == "/hub/give" && !words.isNullOrBlank()) path += "?draft=" + Uri.encode(words)
                AtlasCore.at(path)?.let(web::loadUrl)
            }
        }
    }

    @Deprecated("Back goes back through the hub first")
    override fun onBackPressed() { if (::web.isInitialized && web.canGoBack()) web.goBack() else super.onBackPressed() }

    override fun onDestroy() { ears?.destroy(); voice?.shutdown(); super.onDestroy() }

    /** window.AtlasShell: what the Talk page's hold-to-talk calls. */
    inner class Shell {
        @JavascriptInterface fun listen() = runOnUiThread { begin() }
        @JavascriptInterface fun stop() = runOnUiThread { ears?.stopListening() }
        @JavascriptInterface fun speak(text: String) = runOnUiThread { say(text) }
    }

    private fun begin() {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) {
            requestPermissions(arrayOf(Manifest.permission.RECORD_AUDIO), 7); return
        }
        // On the phone only: the on-device recogniser where there is one,
        // and offline preferred where there isn't.
        val r = if (Build.VERSION.SDK_INT >= 31 && SpeechRecognizer.isOnDeviceRecognitionAvailable(this))
            SpeechRecognizer.createOnDeviceSpeechRecognizer(this) else SpeechRecognizer.createSpeechRecognizer(this)
        ears?.destroy(); ears = r
        r.setRecognitionListener(object : RecognitionListener {
            override fun onResults(b: Bundle) {
                val said = b.getStringArrayList(SpeechRecognizer.RESULTS_RECOGNITION)?.firstOrNull() ?: return
                web.evaluateJavascript("window.atlasHeard&&window.atlasHeard(${JSONArray(listOf(said))}[0])", null)
            }
            override fun onError(e: Int) {
                web.evaluateJavascript("(function(){var n=document.getElementById('holdnote');if(n){n.setAttribute('role','status');n.textContent=\"Atlas didn't catch that. Try again, or type.\";}})()", null)
            }
            override fun onReadyForSpeech(p: Bundle?) {}
            override fun onBeginningOfSpeech() {}
            override fun onRmsChanged(v: Float) {}
            override fun onBufferReceived(b: ByteArray?) {}
            override fun onEndOfSpeech() {}
            override fun onPartialResults(b: Bundle?) {}
            override fun onEvent(t: Int, b: Bundle?) {}
        })
        r.startListening(Intent(RecognizerIntent.ACTION_RECOGNIZE_SPEECH).apply {
            putExtra(RecognizerIntent.EXTRA_LANGUAGE_MODEL, RecognizerIntent.LANGUAGE_MODEL_FREE_FORM)
            putExtra(RecognizerIntent.EXTRA_PREFER_OFFLINE, true)
        })
    }

    private fun say(text: String) {
        val v = voice
        if (v == null) {
            voice = TextToSpeech(this) { ok -> if (ok == TextToSpeech.SUCCESS) voice?.speak(text, TextToSpeech.QUEUE_FLUSH, null, "atlas") }
        } else v.speak(text, TextToSpeech.QUEUE_FLUSH, null, "atlas")
    }
}
