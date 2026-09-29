package app.atlas

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Intent
import android.net.Uri
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder

/**
 * Keeps Atlas running on the phone, and is Android's live activity: an
 * ongoing notification saying what Atlas is doing, and an actionable one when
 * something is ready (Open / Later). Status is an icon and a word, never
 * colour alone. Reads /hub/live.json from Atlas on this phone; nothing online.
 */
class AtlasService : Service() {
    private val thread = HandlerThread("atlas-live").apply { start() }
    private val h = Handler(thread.looper)
    private var lastReady = ""
    /** The 5-second loop is posted once: every onStartCommand used to post
     *  another, so each start of the service added a loop (28 Sep 2026). */
    private var polling = false
    private val loop = object : Runnable {
        override fun run() {
            getSystemService(NotificationManager::class.java)?.let { poll(it) }
            h.postDelayed(this, 5000)
        }
    }

    override fun onBind(i: Intent?): IBinder? = null

    override fun onStartCommand(i: Intent?, flags: Int, id: Int): Int {
        val nm = getSystemService(NotificationManager::class.java)
        nm.createNotificationChannel(NotificationChannel("live", "What Atlas is doing", NotificationManager.IMPORTANCE_LOW))
        nm.createNotificationChannel(NotificationChannel("ready", "Ready for you", NotificationManager.IMPORTANCE_DEFAULT))
        startForeground(1, live("Atlas", "Here when you need it", false))
        // Off the main thread, as the app does; the loop copes until it's up.
        AtlasCore.ensureAsync(this) {}
        if (!polling) {
            polling = true
            h.removeCallbacks(loop)
            h.post(loop)
        }
        return START_STICKY
    }

    private fun open(path: String) = PendingIntent.getActivity(
        this, path.hashCode(), Intent(Intent.ACTION_VIEW, Uri.parse("atlas://hub$path"), this, MainActivity::class.java),
        PendingIntent.FLAG_IMMUTABLE)

    private fun live(title: String, text: String, working: Boolean): Notification =
        Notification.Builder(this, "live")
            .setSmallIcon(R.drawable.ic_stat_atlas)
            .setContentTitle(title).setContentText(text)
            .setStyle(Notification.BigTextStyle().bigText(text))
            .setOngoing(true).setContentIntent(open("/now")).build()

    private fun poll(nm: NotificationManager) {
        // The home-screen widget rides the same loop; it redraws only when
        // what it shows has changed (GlanceWidget.push).
        AtlasCore.glance()?.let { GlanceWidget.push(this, it) }
        CalendarSync.maybeSync(this)
        val s = AtlasCore.live() ?: return
        val w = s.optJSONObject("working")
        if (w != null) {
            nm.notify(1, live("Working: " + w.optString("title"), w.optString("step"), true))
        } else {
            nm.notify(1, live("Atlas", s.optString("status", "Here when you need it"), false))
        }
        val ready = s.optJSONArray("ready")?.optJSONObject(0)
        val title = ready?.optString("title").orEmpty()
        if (title.isNotEmpty() && title != lastReady) {
            lastReady = title
            val href = ready!!.optString("href", "/hub/outstanding").removePrefix("/hub")
            nm.notify(2, Notification.Builder(this, "ready")
                .setSmallIcon(R.drawable.ic_stat_atlas)
                .setContentTitle("Ready for you").setContentText(title)
                .setContentIntent(open(href)).setAutoCancel(true)
                .addAction(Notification.Action.Builder(null, "Open", open(href)).build())
                .addAction(Notification.Action.Builder(null, "Later", open("/outstanding")).build())
                .build())
        }
    }

    override fun onDestroy() { h.removeCallbacks(loop); polling = false; thread.quitSafely(); AtlasCore.stop(); super.onDestroy() }
}
