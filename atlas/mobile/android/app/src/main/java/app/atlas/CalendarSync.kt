package app.atlas

import android.Manifest
import android.content.ContentUris
import android.content.ContentValues
import android.content.Context
import android.content.pm.PackageManager
import android.net.Uri
import android.provider.CalendarContract
import org.json.JSONArray
import org.json.JSONObject
import java.net.HttpURLConnection
import java.net.URL
import java.util.TimeZone

/**
 * The phone's calendar and Atlas's, kept together (H7). Reads the phone's own
 * calendars for a window (a week back, five weeks on), sends them to Atlas on
 * this phone (/hub/calendar/phone), and writes Atlas's own events into one
 * calendar of its own, "Atlas", which is never read back. Only with calendar
 * permission; MainActivity asks for it once.
 */
object CalendarSync {
    private const val NAME = "Atlas"
    private var last = 0L

    private fun allowed(ctx: Context) =
        ctx.checkSelfPermission(Manifest.permission.READ_CALENDAR) == PackageManager.PERMISSION_GRANTED &&
            ctx.checkSelfPermission(Manifest.permission.WRITE_CALENDAR) == PackageManager.PERMISSION_GRANTED

    /** At most every ten minutes. Runs on the caller's (background) thread. */
    fun maybeSync(ctx: Context) {
        val now = System.currentTimeMillis()
        if (now - last < 10 * 60 * 1000 || !allowed(ctx)) return
        last = now
        runCatching { sync(ctx) }
    }

    private fun asAdapter(uri: Uri): Uri = uri.buildUpon()
        .appendQueryParameter(CalendarContract.CALLER_IS_SYNCADAPTER, "true")
        .appendQueryParameter(CalendarContract.Calendars.ACCOUNT_NAME, NAME)
        .appendQueryParameter(CalendarContract.Calendars.ACCOUNT_TYPE, CalendarContract.ACCOUNT_TYPE_LOCAL)
        .build()

    /** Atlas's own calendar on the phone, made the first time. */
    private fun atlasCalendar(ctx: Context): Long? {
        val cr = ctx.contentResolver
        cr.query(CalendarContract.Calendars.CONTENT_URI, arrayOf(CalendarContract.Calendars._ID),
            "${CalendarContract.Calendars.ACCOUNT_NAME}=? AND ${CalendarContract.Calendars.ACCOUNT_TYPE}=?",
            arrayOf(NAME, CalendarContract.ACCOUNT_TYPE_LOCAL), null)?.use { if (it.moveToFirst()) return it.getLong(0) }
        val v = ContentValues().apply {
            put(CalendarContract.Calendars.ACCOUNT_NAME, NAME)
            put(CalendarContract.Calendars.ACCOUNT_TYPE, CalendarContract.ACCOUNT_TYPE_LOCAL)
            put(CalendarContract.Calendars.NAME, NAME)
            put(CalendarContract.Calendars.CALENDAR_DISPLAY_NAME, NAME)
            put(CalendarContract.Calendars.CALENDAR_COLOR, 0xFFD9730D.toInt())
            put(CalendarContract.Calendars.CALENDAR_ACCESS_LEVEL, CalendarContract.Calendars.CAL_ACCESS_OWNER)
            put(CalendarContract.Calendars.OWNER_ACCOUNT, NAME)
            put(CalendarContract.Calendars.VISIBLE, 1)
            put(CalendarContract.Calendars.SYNC_EVENTS, 1)
        }
        return cr.insert(asAdapter(CalendarContract.Calendars.CONTENT_URI), v)?.let { ContentUris.parseId(it) }
    }

    private fun sync(ctx: Context) {
        val cr = ctx.contentResolver
        val mine = atlasCalendar(ctx)
        val now = System.currentTimeMillis()
        val from = now - 7L * 86_400_000
        val to = now + 35L * 86_400_000
        val events = JSONArray()
        val uri = CalendarContract.Instances.CONTENT_URI.buildUpon().also {
            ContentUris.appendId(it, from); ContentUris.appendId(it, to)
        }.build()
        cr.query(uri, arrayOf(
            CalendarContract.Instances._ID, CalendarContract.Instances.TITLE, CalendarContract.Instances.BEGIN,
            CalendarContract.Instances.END, CalendarContract.Instances.ALL_DAY, CalendarContract.Instances.EVENT_LOCATION,
            CalendarContract.Instances.CALENDAR_ID), null, null, null)?.use { c ->
            while (c.moveToNext()) {
                if (mine != null && c.getLong(6) == mine) continue
                events.put(JSONObject()
                    .put("key", c.getLong(0).toString())
                    .put("title", c.getString(1) ?: "")
                    .put("start", c.getLong(2) / 1000)
                    .put("end", c.getLong(3) / 1000)
                    .put("all_day", c.getInt(4) == 1)
                    .put("place", c.getString(5) ?: ""))
            }
        }
        val body = JSONObject().put("from", from / 1000).put("to", to / 1000).put("events", events).toString()
        val u = AtlasCore.at("/hub/calendar/phone") ?: return
        val t = AtlasCore.token ?: return
        val reply = (URL(u).openConnection() as HttpURLConnection).run {
            requestMethod = "POST"; doOutput = true
            setRequestProperty("Authorization", "Bearer $t")
            setRequestProperty("Content-Type", "application/json")
            connectTimeout = 3000; readTimeout = 10000
            outputStream.use { it.write(body.toByteArray()) }
            inputStream.bufferedReader().use { JSONObject(it.readText()) }
        }
        val atlas = reply.optJSONArray("atlas") ?: return
        val cal = mine ?: return
        val prefs = ctx.getSharedPreferences("calendar", Context.MODE_PRIVATE)
        val written = JSONObject(prefs.getString("written", "{}")!!)
        val seen = HashSet<String>()
        for (i in 0 until atlas.length()) {
            val a = atlas.getJSONObject(i)
            val id = a.optString("id"); if (id.isEmpty()) continue
            seen.add(id)
            val v = ContentValues().apply {
                put(CalendarContract.Events.CALENDAR_ID, cal)
                put(CalendarContract.Events.TITLE, a.optString("title"))
                put(CalendarContract.Events.DTSTART, a.optLong("start") * 1000)
                put(CalendarContract.Events.DTEND, a.optLong("end") * 1000)
                put(CalendarContract.Events.ALL_DAY, if (a.optBoolean("all_day")) 1 else 0)
                put(CalendarContract.Events.EVENT_LOCATION, a.optString("place").takeIf { it != "null" } ?: "")
                put(CalendarContract.Events.EVENT_TIMEZONE, if (a.optBoolean("all_day")) "UTC" else TimeZone.getDefault().id)
            }
            val existing = written.optLong(id, -1)
            val updated = existing >= 0 && cr.update(asAdapter(ContentUris.withAppendedId(CalendarContract.Events.CONTENT_URI, existing)), v, null, null) > 0
            if (!updated) cr.insert(asAdapter(CalendarContract.Events.CONTENT_URI), v)?.let { written.put(id, ContentUris.parseId(it)) }
        }
        // Gone from Atlas: gone from the phone.
        for (id in written.keys().asSequence().toList()) {
            if (id !in seen) {
                cr.delete(asAdapter(ContentUris.withAppendedId(CalendarContract.Events.CONTENT_URI, written.getLong(id))), null, null)
                written.remove(id)
            }
        }
        prefs.edit().putString("written", written.toString()).apply()
    }
}
