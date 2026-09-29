package app.atlas

import android.app.PendingIntent
import android.appwidget.AppWidgetManager
import android.appwidget.AppWidgetProvider
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.widget.RemoteViews
import org.json.JSONObject
import java.text.DateFormat
import java.util.Date

/**
 * Atlas at a glance on the home screen: what it's doing or what's next, how
 * many things wait on you, and a tap into Give. Drawn from the glance Atlas
 * makes safe itself (/hub/glance.json, glance.rs); AtlasService fetches it
 * and calls [push] when what shows has changed. Android has no lock-screen
 * widgets on phones, so this is the home view (`home`), never the lock one.
 */
class GlanceWidget : AppWidgetProvider() {
    override fun onUpdate(ctx: Context, mgr: AppWidgetManager, ids: IntArray) {
        val g = kept(ctx)
        for (id in ids) mgr.updateAppWidget(id, views(ctx, g))
    }

    companion object {
        private const val PREFS = "glance"

        /** The last glance kept, or null before Atlas has run. */
        private fun kept(ctx: Context): JSONObject? =
            ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE).getString("json", null)?.let {
                runCatching { JSONObject(it) }.getOrNull()
            }

        /** Keep a new glance and redraw, only when what shows changed. */
        fun push(ctx: Context, g: JSONObject) {
            val prefs = ctx.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            val shown = g.optString("status") + g.optJSONObject("home")?.toString()
            if (prefs.getString("shown", null) == shown) return
            prefs.edit().putString("json", g.toString()).putString("shown", shown).apply()
            val mgr = AppWidgetManager.getInstance(ctx)
            val ids = mgr.getAppWidgetIds(ComponentName(ctx, GlanceWidget::class.java))
            for (id in ids) mgr.updateAppWidget(id, views(ctx, g))
        }

        private fun open(ctx: Context, path: String) = PendingIntent.getActivity(
            ctx, path.hashCode(),
            Intent(Intent.ACTION_VIEW, Uri.parse("atlas://hub$path"), ctx, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE)

        private fun views(ctx: Context, g: JSONObject?): RemoteViews {
            val v = RemoteViews(ctx.packageName, R.layout.atlas_glance)
            if (g == null) {
                v.setTextViewText(R.id.glance_status, "Atlas")
                v.setTextViewText(R.id.glance_headline, "Open Atlas once and this fills in.")
                v.setTextViewText(R.id.glance_waiting, "")
            } else {
                val home = g.optJSONObject("home") ?: JSONObject()
                val working = home.optString("working", "")
                val next = home.optJSONObject("next")
                val headline = when {
                    working.isNotEmpty() && working != "null" -> "Working: $working"
                    next != null -> listOf(next.optString("at"), next.optString("what")).filter { it.isNotEmpty() }.joinToString(" ")
                    else -> g.optString("status")
                }
                val waiting = home.optInt("waiting", 0)
                val asOf = g.optLong("as_of", 0)
                val stale = asOf > 0 && System.currentTimeMillis() / 1000 - asOf > 15 * 60
                val waitingText = when (waiting) { 0 -> "Nothing waiting"; 1 -> "1 thing waiting"; else -> "$waiting things waiting" }
                v.setTextViewText(R.id.glance_status, g.optString("status", "Atlas"))
                v.setTextViewText(R.id.glance_headline, headline)
                v.setTextViewText(R.id.glance_waiting,
                    if (stale) "$waitingText · as of ${DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(asOf * 1000))}" else waitingText)
            }
            v.setOnClickPendingIntent(R.id.glance_root, open(ctx, "/now"))
            v.setOnClickPendingIntent(R.id.glance_waiting, open(ctx, "/outstanding"))
            v.setOnClickPendingIntent(R.id.glance_capture, open(ctx, "/give"))
            return v
        }
    }
}
