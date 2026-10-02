//! Reminders that ring on the phone with Atlas closed (item 15, Eric's yes,
//! 1 Oct 2026).
//!
//! iOS stops an app soon after it leaves the screen, and Atlas's own core
//! goes with it: a reminder due in an hour would wait until the app was
//! next opened. So the reminders still to come are listed in `live.json`
//! (`upcoming`), and when the app goes to the background it hands each one
//! to iOS as a local notification, which rings on time whether the app is
//! running or not. Coming back to the foreground takes them back, and Atlas
//! rings its own again -- never both. No server and no Apple key: it's the
//! phone's own scheduler.
//!
//! Android doesn't need this: its foreground service keeps Atlas running.

use crate::scheduler::Scheduler;

/// The most handed to iOS at once (it keeps at most 64 pending per app).
pub const MOST: usize = 48;

/// How far ahead: a week. Further ones are handed over on a later visit.
pub const AHEAD_SECS: u64 = 7 * 24 * 60 * 60;

/// The reminders due after `now` and within `AHEAD_SECS`, soonest first:
/// `{id, due, text}`, `due` in Unix seconds.
pub fn upcoming(s: &Scheduler, now: u64) -> Vec<serde_json::Value> {
    let mut v: Vec<(u64, u64, String)> = s
        .active()
        .into_iter()
        .filter_map(|j| {
            let text = j.command.strip_prefix("reminder ")?.trim_start_matches("Reminder:").trim().to_string();
            (j.due > now && j.due <= now + AHEAD_SECS && !text.is_empty()).then_some((j.due, j.id, text))
        })
        .collect();
    v.sort();
    v.truncate(MOST);
    v.into_iter().map(|(due, id, text)| serde_json::json!({ "id": id, "due": due, "text": text })).collect()
}
