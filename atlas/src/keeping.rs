//! Keeping track of what's already set: reminders listed, cancelled and
//! snoozed, timers, and calendar events cancelled or moved.
//!
//! 30 Sep 2026, from an audit of what a person hits in a day: a reminder
//! could be set and never seen again (`Scheduler::active` and `cancel` had no
//! caller -- the "(#4)" it handed back could be used for nothing), "set a
//! timer for ten minutes" went nowhere, and an event could be booked but not
//! cancelled or moved (`Calendar::remove` had no caller), though booking one
//! promised "say 'cancel that' and I'll take it off".
//!
//! Here: the words. What they're done to is `Daemon::keeping_track`.

/// Which reminder.
#[derive(Debug, Clone, PartialEq)]
pub enum Which {
    All,
    Number(u64),
    /// Words from what it's about.
    About(String),
    /// "the reminder", "that timer": the only one, or the last one set.
    Last,
}

/// What was asked.
#[derive(Debug, Clone, PartialEq)]
pub enum Ask {
    ListReminders,
    CancelReminder(Which),
    /// A timer, in seconds.
    Timer(u64),
    /// Say the last reminder again later: seconds, or the usual ten minutes.
    Snooze(Option<u64>),
    /// Take an event off the calendar: what it is, or when ("my 3pm").
    CancelEvent(String),
    /// Move an event: which, and where to.
    MoveEvent { what: String, to: String },
}

/// Snoozing with no time said.
pub const SNOOZE_SECS: u64 = 600;

fn words(said: &str) -> String {
    let t: String = said
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ':' || c == '#' || c == '\'' { c } else { ' ' })
        .collect();
    t.split_whitespace().collect::<Vec<_>>().join(" ")
}

const NUMBERS: &[(&str, u64)] = &[
    ("a", 1), ("an", 1), ("one", 1), ("two", 2), ("three", 3), ("four", 4), ("five", 5), ("six", 6),
    ("seven", 7), ("eight", 8), ("nine", 9), ("ten", 10), ("eleven", 11), ("twelve", 12), ("fifteen", 15),
    ("twenty", 20), ("thirty", 30), ("forty", 40), ("forty-five", 45), ("fifty", 50), ("sixty", 60), ("ninety", 90),
];

fn number(w: &str) -> Option<u64> {
    w.parse().ok().or_else(|| NUMBERS.iter().find(|(n, _)| *n == w).map(|(_, v)| *v))
}

/// A length of time in words: "10 minutes", "ten minutes", "a minute and a
/// half" (not that), "an hour", "half an hour", "1 hour 30 minutes",
/// "90 seconds", "25 min".
pub fn duration_secs(text: &str) -> Option<u64> {
    let t = words(text).replace("half an hour", "30 minutes").replace("a half hour", "30 minutes");
    let w: Vec<&str> = t.split_whitespace().collect();
    let mut total = 0u64;
    let mut found = false;
    let mut i = 0;
    while i + 1 < w.len() {
        // "20-minute" arrives as "20 minute".
        if let Some(n) = number(w[i]) {
            let unit = w[i + 1];
            let secs = if unit.starts_with("sec") {
                Some(1)
            } else if unit.starts_with("min") {
                Some(60)
            } else if unit.starts_with("hour") || unit == "hr" || unit == "hrs" {
                Some(3600)
            } else {
                None
            };
            if let Some(s) = secs {
                total += n * s;
                found = true;
                i += 2;
                continue;
            }
        }
        i += 1;
    }
    (found && total > 0).then_some(total)
}

fn after<'a>(t: &'a str, marks: &[&str]) -> Option<&'a str> {
    marks.iter().find_map(|m| t.find(m).map(|i| t[i + m.len()..].trim()))
}

/// What's being asked about what's already set, if that's what this is.
pub fn read(said: &str) -> Option<Ask> {
    let t = words(said);
    let padded = format!(" {t} ");
    let about_reminders = padded.contains(" reminder") || padded.contains(" timer");

    // Listing.
    if about_reminders
        && [
            "what reminders", "which reminders", "my reminders", "list reminders", "list my reminders",
            "show my reminders", "show reminders", "any reminders", "reminders do i have", "reminders have i got",
            "my timers", "what timers", "any timers", "timers do i have", "reminders are set",
        ]
        .iter()
        .any(|p| t.contains(p))
        && !t.starts_with("cancel")
        && !t.starts_with("delete")
        && !t.starts_with("remove")
    {
        return Some(Ask::ListReminders);
    }

    let cancelling = ["cancel ", "delete ", "remove ", "clear ", "forget ", "stop ", "turn off ", "get rid of ", "scrap ", "drop "]
        .iter()
        .find(|p| t.starts_with(**p))
        .map(|p| t[p.len()..].trim().to_string());

    // Cancelling a reminder or timer.
    if let Some(rest) = cancelling.as_deref().filter(|_| about_reminders) {
        if rest.starts_with("all") || rest.contains("every") {
            return Some(Ask::CancelReminder(Which::All));
        }
        if let Some(n) = rest
            .split_whitespace()
            .find_map(|w| w.trim_start_matches('#').parse::<u64>().ok())
        {
            return Some(Ask::CancelReminder(Which::Number(n)));
        }
        if let Some(about) = after(rest, &["reminder to ", "reminder about ", "reminder for ", "timer for "]) {
            if !about.is_empty() {
                return Some(Ask::CancelReminder(Which::About(about.to_string())));
            }
        }
        return Some(Ask::CancelReminder(Which::Last));
    }

    // Timers.
    let timer = t.contains("timer")
        && (t.starts_with("set") || t.starts_with("start") || t.starts_with("timer") || t.starts_with("put")
            || t.starts_with("can you set") || t.starts_with("give me") || t.contains(" minute timer")
            || t.contains(" second timer") || t.contains(" hour timer"));
    if timer {
        if let Some(secs) = duration_secs(&t) {
            return Some(Ask::Timer(secs));
        }
    }

    // Snoozing.
    if t.starts_with("snooze") || t.starts_with("remind me again") || t == "later" || t.starts_with("again in") {
        return Some(Ask::Snooze(duration_secs(&t)));
    }

    // Calendar: moving.
    for verb in ["move ", "reschedule ", "push ", "shift ", "change "] {
        if let Some(rest) = t.strip_prefix(verb) {
            if let Some(i) = rest.rfind(" to ") {
                let (what, to) = (rest[..i].trim(), rest[i + 4..].trim());
                let what = strip_owner(what);
                if !what.is_empty() && !to.is_empty() && !what.contains("reminder") && !what.contains("timer") {
                    return Some(Ask::MoveEvent { what: what.to_string(), to: to.to_string() });
                }
            }
        }
    }

    // Calendar: cancelling. Only something that can be an event ("my 3pm",
    // "the dentist", "tomorrow's meeting") -- the daemon checks there is one.
    if let Some(rest) = cancelling {
        let what = strip_owner(&rest);
        let not_an_event = ["that", "it", "this", "the post", "post", "everything", "the download", "download", "the update"];
        if !what.is_empty() && !not_an_event.contains(&what) && !rest.starts_with("that") && !rest.starts_with("it ") {
            return Some(Ask::CancelEvent(what.to_string()));
        }
    }
    None
}

fn strip_owner(s: &str) -> &str {
    let mut s = s.trim();
    for p in ["my ", "the ", "our ", "that ", "this "] {
        if let Some(r) = s.strip_prefix(p) {
            s = r.trim();
        }
    }
    for tail in [" on my calendar", " from my calendar", " off my calendar", " event", " appointment"] {
        if let Some(r) = s.strip_suffix(tail) {
            s = r.trim();
        }
    }
    s
}

/// A clock time in these words ("3pm", "3 pm", "15:00", "3 o'clock"),
/// as (hour 0-23, or both readings for an hour without am/pm, minute).
pub fn clock_in(what: &str) -> Option<(Vec<u32>, u32)> {
    let t = words(what).replace("o'clock", "").replace(" pm", "pm").replace(" am", "am");
    for w in t.split_whitespace() {
        let (body, pm, am) = if let Some(b) = w.strip_suffix("pm") {
            (b, true, false)
        } else if let Some(b) = w.strip_suffix("am") {
            (b, false, true)
        } else {
            (w, false, false)
        };
        let (h, m) = match body.split_once(':') {
            Some((h, m)) => (h.parse::<u32>().ok()?, m.parse::<u32>().ok()?),
            None => match body.parse::<u32>() {
                Ok(h) => (h, 0),
                Err(_) => continue,
            },
        };
        if h > 23 || m > 59 {
            continue;
        }
        let hours = if pm && h < 12 {
            vec![h + 12]
        } else if am && h == 12 {
            vec![0]
        } else if pm || am || h > 12 || body.contains(':') && h >= 13 {
            vec![h]
        } else if h <= 12 {
            // "my 3": 3 in the afternoon more often than 3 in the night; both.
            vec![h % 12 + 12, h % 12]
        } else {
            vec![h]
        };
        return Some((hours, m));
    }
    None
}

/// Does an event (its title, and its start on the local clock) answer to
/// these words -- by its time ("my 3pm") or by what it's called ("the
/// dentist")?
pub fn event_answers_to(title: &str, local_start: u64, what: &str) -> bool {
    if let Some((hours, minute)) = clock_in(what) {
        let c = crate::civil::Civil::from_local(local_start as i64);
        if hours.contains(&(c.hour as u32)) && c.minute as u32 == minute {
            return true;
        }
    }
    let title = words(title);
    let skip = ["my", "the", "a", "an", "on", "at", "today", "tomorrow", "todays", "tomorrows", "today's", "tomorrow's", "meeting", "event", "appointment"];
    let wanted: Vec<&str> = what.split_whitespace().filter(|w| !skip.contains(w) && clock_in(w).is_none()).collect();
    if wanted.is_empty() {
        // "cancel my meeting" with nothing else: a title with "meeting" in it.
        return what.split_whitespace().any(|w| w == "meeting" && title.contains("meeting"));
    }
    wanted.iter().all(|w| title.split_whitespace().any(|t| t == *w || t.starts_with(w)))
}
