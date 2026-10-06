//! Reminders and wants in words, and the settings' fingerprint.
//!
//! Moved out of `daemon.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.


/// The thing to be reminded of, scheduling words removed. Reminders read
/// "… to <text>", so the text is what follows the last " to ".
pub(super) fn reminder_text(said: &str) -> String {
    if let Some(i) = said.rfind(" to ") {
        return said[i + 4..].trim().to_string();
    }
    let s = said.trim();
    s.strip_prefix("remind me")
        .or_else(|| s.strip_prefix("Remind me"))
        .unwrap_or(s)
        .trim()
        .to_string()
}

/// "in 20 minutes" / "in 2 hours" -> seconds, or None.
pub(super) fn relative_secs(low: &str) -> Option<u64> {
    let idx = low.find(" in ")?;
    let mut w = low[idx + 4..].split_whitespace();
    let n: u64 = w.next()?.parse().ok()?;
    let unit = w.next()?;
    if unit.starts_with("min") {
        Some(n * 60)
    } else if unit.starts_with("hour") || unit.starts_with("hr") {
        Some(n * 3600)
    } else {
        None
    }
}

/// When a reminder should first fire, on the local clock. `Err(Some(why))`
/// when the words name a time that could mean two things (or has passed, or
/// is a day with no hour) -- the caller asks; `Err(None)` when no time was
/// named at all.
pub(super) fn first_occurrence(raw: &str, now: u64) -> std::result::Result<u64, Option<String>> {
    let p = crate::when::parse(raw, now).ok_or(None)?;
    if !p.sure {
        return Err(p.why.or_else(|| Some("I'm not sure which time you mean.".into())));
    }
    if p.all_day {
        let c = crate::civil::Civil::from_local(p.start as i64);
        return Err(Some(format!("what time on {:04}-{:02}-{:02}?", c.year, c.month, c.day)));
    }
    Ok(p.start)
}

/// A local moment said the way a person checks it: "today at 17:00",
/// "tomorrow at 09:00", "on Fri 2026-10-02 at 15:00".
pub(super) fn local_moment(at: u64, now: u64) -> String {
    let c = crate::civil::Civil::from_local(at as i64);
    let days = (at / 86_400) as i64 - (now / 86_400) as i64;
    let day = match days {
        0 => "today".to_string(),
        1 => "tomorrow".to_string(),
        _ => {
            let wd = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"][c.weekday() as usize];
            format!("on {wd} {:04}-{:02}-{:02}", c.year, c.month, c.day)
        }
    };
    format!("{day} at {:02}:{:02}", c.hour, c.minute)
}

pub(super) fn say_duration(secs: u64) -> String {
    if secs >= 3600 && secs.is_multiple_of(3600) {
        let h = secs / 3600;
        format!("{h} hour{}", if h == 1 { "" } else { "s" })
    } else {
        let m = (secs / 60).max(1);
        format!("{m} minute{}", if m == 1 { "" } else { "s" })
    }
}

/// Does this read as a want or an idea Atlas should weigh as an opportunity?
/// Cue-based on purpose: it only fires on a stated wish, never on a question
/// or a command, so it sits last in the Unknown chain and claims nothing that
/// another door would answer.
pub(super) fn reads_as_a_want(said: &str) -> bool {
    let t = said.to_lowercase();
    const CUES: &[&str] = &[
        "i want to",
        "i'd like to",
        "i would like to",
        "i wish",
        "it'd be good if",
        "it would be good if",
        "i've been meaning to",
        "ive been meaning to",
        "i've got an idea",
        "ive got an idea",
        "here's an idea",
        "heres an idea",
        "what if i",
        "i keep meaning to",
        "i should really",
    ];
    CUES.iter().any(|c| t.contains(c))
}

/// What a settings folder's two files hold, as one number: changes when
/// either file's contents do, whatever the file system does with times.
/// The settings files' sizes and modified times, hashed: what
/// `pick_up_settings` checks every tick before reading anything.
pub(super) fn settings_stamp(dir: &std::path::Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for name in ["tools.yaml", "settings.yaml"] {
        match std::fs::metadata(dir.join(name)) {
            Ok(m) => {
                m.len().hash(&mut h);
                m.modified().ok().hash(&mut h);
            }
            Err(_) => 0u8.hash(&mut h),
        }
    }
    h.finish()
}

pub(super) fn settings_fingerprint(dir: &std::path::Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for name in ["tools.yaml", "settings.yaml"] {
        std::fs::read(dir.join(name)).unwrap_or_default().hash(&mut h);
    }
    h.finish()
}
