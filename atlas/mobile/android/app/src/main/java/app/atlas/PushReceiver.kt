package app.atlas

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import org.json.JSONObject
import org.unifiedpush.android.connector.FailedReason
import org.unifiedpush.android.connector.PushService
import org.unifiedpush.android.connector.UnifiedPush
import org.unifiedpush.android.connector.data.PushEndpoint
import org.unifiedpush.android.connector.data.PushMessage

/**
 * The laptop reaching this phone with Atlas closed (item 15, decision 3: no
 * Google needed). A UnifiedPush distributor -- the ntfy app is the usual one
 * -- gives this app a push address and keys; they go to Atlas on this phone
 * (/hub/web-push-endpoint), which carries them to the laptop as a sync event
 * from your own devices only. The laptop seals each message for this phone
 * (Web Push, RFC 8291); the connector opens it here and it is shown as one
 * of Atlas's own notifications. Titles only unless you allowed detail.
 */
class PushReceiver : PushService() {
    override fun onNewEndpoint(endpoint: PushEndpoint, instance: String) {
        val keys = endpoint.pubKeySet ?: return
        val body = JSONObject()
            .put("endpoint", endpoint.url)
            .put("p256dh", keys.pubKey)
            .put("auth", keys.auth)
        // The core may still be starting; it is asked again on the next
        // registration, which the app makes each time it opens.
        Thread {
            AtlasCore.ensure(this)
            AtlasCore.post("/hub/web-push-endpoint", body)
        }.start()
    }

    override fun onMessage(message: PushMessage, instance: String) {
        val v = runCatching { JSONObject(String(message.content)) }.getOrNull() ?: return
        show(this, v.optString("title", "Atlas"), v.optString("body", ""), v.optBoolean("urgent", false))
    }

    override fun onRegistrationFailed(reason: FailedReason, instance: String) {}

    override fun onUnregistered(instance: String) {}

    companion object {
        /** Register with the distributor already chosen, or the phone's
         *  default one. With none installed, nothing happens: the laptop
         *  says it can't reach the phone, and Atlas's own notifications
         *  still work whenever the app is running. */
        fun register(ctx: Context) {
            UnifiedPush.tryUseCurrentOrDefaultDistributor(ctx) { found ->
                if (found) UnifiedPush.register(ctx)
            }
        }

        fun show(ctx: Context, title: String, text: String, urgent: Boolean) {
            val nm = ctx.getSystemService(NotificationManager::class.java) ?: return
            nm.createNotificationChannel(NotificationChannel("said", "Reminders and news from Atlas", NotificationManager.IMPORTANCE_HIGH))
            val open = PendingIntent.getActivity(
                ctx, 7, Intent(Intent.ACTION_VIEW, Uri.parse("atlas://hub/now"), ctx, MainActivity::class.java),
                PendingIntent.FLAG_IMMUTABLE)
            val n = Notification.Builder(ctx, "said")
                .setSmallIcon(R.drawable.ic_stat_atlas)
                .setContentTitle(title).setContentText(text)
                .setStyle(Notification.BigTextStyle().bigText(text))
                .setCategory(if (urgent) Notification.CATEGORY_ALARM else Notification.CATEGORY_MESSAGE)
                .setAutoCancel(true).setContentIntent(open).build()
            nm.notify((System.currentTimeMillis() % 100000).toInt() + 1000, n)
        }
    }
}
