//! Small readings of a sentence: app gates, topics, seconds, o'clock, offsets.
//!
//! Moved out of `daemon.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// Spoken channel name to a Channel.
pub(super) fn channel_named(name: &str) -> Channel {
    match name.trim().to_lowercase().as_str() {
        "x" | "twitter" => Channel::X,
        "linkedin" => Channel::LinkedIn,
        "instagram" | "insta" => Channel::Instagram,
        "facebook" => Channel::Facebook,
        "discord" => Channel::Discord,
        "email" | "mail" => Channel::Email { to: String::new(), subject: String::new() },
        other => Channel::Other(other.to_string()),
    }
}

/// The target an intent is acting on, if it has one.
/// The (app, action) pair an intent acts on, when it is one the permission
/// gate covers. Used to record a grant against the exact action that was
/// asked about, so approving "open Foo" does not silently also allow closing
/// it.
pub(super) fn app_action_of(i: &Intent) -> Option<(String, String)> {
    match i {
        Intent::OpenApp(a) => Some((a.clone(), "open".into())),
        Intent::CloseApp(a) => Some((a.clone(), "close".into())),
        Intent::FocusApp(a) => Some((a.clone(), "focus".into())),
        // "Allow the camera?" -- a yes is kept as the camera's grant
        // (`daemon::camera`, 30 Sep 2026).
        Intent::CaptureWebcam => Some((camera::CAMERA.into(), "look".into())),
        _ => None,
    }
}

pub(super) fn argument_of(i: &Intent) -> Option<String> {
    match i {
        Intent::OpenApp(a)
        | Intent::CloseApp(a)
        | Intent::FocusApp(a)
        | Intent::Research(a)
        | Intent::DraftPost(a)
        | Intent::SetMode(a) => Some(a.clone()),
        _ => None,
    }
}

/// A short label for what a turn was about, so returning to the thread days
/// later can name it.
pub(super) fn topic_of(i: &Intent) -> Option<String> {
    match i {
        Intent::Research(t) => Some(t.clone()),
        Intent::OpenApp(a) | Intent::CloseApp(a) | Intent::FocusApp(a) => Some(a.clone()),
        Intent::DraftPost(c) => Some(format!("the {c} post")),
        Intent::SetMode(m) => Some(format!("{m} mode")),
        Intent::WorkspaceOn | Intent::WorkspaceOff => Some("the workspace".into()),
        _ => None,
    }
}

pub(super) fn report(r: Result<workspace::Report>, ok: &str) -> String {
    match r {
        Ok(r) if r.ok() => ok.to_string(),
        // `Report::plain`, not a list of `failed`. This used to name only the
        // apps that failed, which meant a bring-up that ran out of its
        // wall-clock budget -- and so never even tried the last two apps --
        // reported no failures and came back "Workspace online." `not_reached`
        // and `abandoned` exist for exactly that, and `ok()` now counts them,
        // so this arm has to be able to say what they are.
        Ok(r) => format!("Partly. {}", r.plain()),
        Err(e) => format!("error: {e}"),
    }
}

pub(super) fn act(r: Result<()>, ok: &str) -> String {
    match r {
        Ok(()) => ok.to_string(),
        Err(e) => format!("error: {e}"),
    }
}

/// A name as it comes off `raw_argument` parsing, cleaned up for use as a
/// pairing peer's name.
///
/// `raw_argument` exists so a pairing *code* survives intact -- but `Pair`
/// and `ForgetPeer` take a name, not a code, and the same raw path that
/// protects a code also lets a spoken sentence's trailing "." or "?" ride
/// straight into what gets stored. Trimmed here rather than by widening
/// `raw_argument` itself, which would corrupt a code the same way
/// `normalize()` used to -- see `intent.rs`'s doc comment on why codes need
/// the parser to leave them alone completely. Case is left as given: it's
/// `kin::Pairings`' own case-insensitive comparison (`same_name`) that makes
/// "Sarah" and "sarah" the same pairing, not lowercasing here, so a name
/// typed with its proper capitalization still displays that way.
pub(super) fn spoken_name(raw: &str) -> &str {
    raw.trim().trim_end_matches(['.', '!', '?', ',', ';', ':'])
}

/// Should this go in the history, and how would it be taken back?
///
/// Reading, asking and explaining leave nothing behind. Only things that
/// changed your world are worth being able to undo.
/// Enough of what was asked to recognise a slow turn later.
///
/// Short on purpose: the timing window lives in memory for the life of the
/// process, and keeping whole utterances in it would turn a latency measure
/// into a transcript nobody asked for.///
/// How long the video is, from what ffmpeg said about it.
///
/// Read from the scan Atlas already ran rather than a second `ffprobe` call.
/// Zero when it cannot be found, which `where_to_look` treats as "no gaps to
/// fill" — the cautious direction: fewer frames, never a runaway.
pub(super) fn seconds_of(told: &str) -> f32 {
    told.split("Duration:")
        .nth(1)
        .and_then(|rest| rest.split(',').next())
        .and_then(|stamp| {
            let parts: Vec<&str> = stamp.trim().split(':').collect();
            match parts.as_slice() {
                [h, m, s] => Some(
                    h.parse::<f32>().ok()? * 3600.0
                        + m.parse::<f32>().ok()? * 60.0
                        + s.parse::<f32>().ok()?,
                ),
                _ => None,
            }
        })
        .unwrap_or(0.0)
}

/// Fill `{placeholders}` in a tool's arguments from the config vars.
///
/// The same substitution `ExternalTool` does when it runs something, needed
/// here because the camera arguments are reused rather than run.
pub(super) fn resolved(args: &[String], vars: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    args.iter()
        .map(|a| {
            let mut out = a.clone();
            for (k, v) in vars {
                out = out.replace(&format!("{{{k}}}"), v);
            }
            out
        })
        .collect()
}

/// "Sam: the roof quote came back high" -> ("Sam", "the roof quote...").
///
/// Two shapes, because people say both. A colon is unambiguous and is what
/// anybody typing will use. Without one, the first word is the name -- which
/// is why `commands.yaml` deliberately does not list "tell" as a phrase for
/// this: "tell me what's outstanding" would become a message to somebody
/// called "me".
/// Whether two member lists name the same set of people, case-insensitively.
/// Used to recognise "message Jordan and Maya" as the group you already have
/// with exactly those two, rather than minting a second one each time.
pub(super) fn same_member_set(a: &[String], b: &[String]) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|x| b.iter().any(|y| crate::kin::same_name(x, y)))
}

pub(super) fn split_who_and_what(raw: &str) -> (String, String) {
    let t = raw.trim();
    if let Some((who, body)) = t.split_once(':') {
        return (who.trim().to_string(), body.trim().to_string());
    }
    match t.split_once(char::is_whitespace) {
        Some((who, body)) => {
            let body = body.trim();
            // "Sam that the roof quote came back high" -- the filler word
            // people put in when they are speaking rather than typing.
            let body = body.strip_prefix("that ").unwrap_or(body);
            (who.trim().to_string(), body.trim().to_string())
        }
        None => (t.to_string(), String::new()),
    }
}

/// An hour of the day, the way a person says it.
pub(super) fn oclock(hour: u32) -> String {
    match hour % 24 {
        0 => "midnight".into(),
        12 => "midday".into(),
        h if h < 12 => format!("{h}am"),
        h => format!("{}pm", h - 12),
    }
}

/// This machine's offset from UTC, in minutes.
///
/// Worked out rather than configured: a config field would be a second
/// declaration of a fact the operating system already knows, and it would be
/// wrong twice a year. Falls back to 0 when it cannot be read -- which shows
/// a message in UTC rather than refusing to send it, because the timestamp is
/// a convenience and the message is the point.
///
/// On Windows there is no `date +%z`; this used to fall back to 0 there, so
/// every chat timestamp was shown in UTC and anything scheduled "daily at
/// 08:00" would have fired at 08:00 UTC. Asked of Windows itself now, and
/// the answer is kept for ten minutes rather than asked for on every tick --
/// long enough to cost nothing, short enough to follow a clock change.
pub(crate) fn local_offset_mins() -> i16 {
    // Through `localclock`, which asks Windows directly. This asked
    // `date +%z`, which Windows doesn't have, so on the laptop every
    // message went out stamped as UTC.
    (crate::localclock::offset_secs() / 60) as i16
}

/// `+0530` / `-0500` -> minutes. Separate from the command that produces it
/// so the parsing can be tested without caring what this machine's clock is
/// set to.
pub fn parse_offset(raw: &str) -> i16 {
    let raw = raw.trim();
    if raw.len() < 5 {
        return 0;
    }
    let sign = match raw.as_bytes()[0] {
        b'-' => -1,
        b'+' => 1,
        _ => return 0,
    };
    // All four digits, or none of it. `unwrap_or(0)` per field was the first
    // version and it turned junk into a plausible answer: "++0500" parsed its
    // hours as "+0", failed, took 0 -- and then read "50" as the minutes and
    // returned a real-looking offset of fifty minutes. A parser that cannot
    // fail will always prefer a wrong answer to no answer.
    let Some(digits) = raw.get(1..5) else { return 0 };
    if !digits.bytes().all(|b| b.is_ascii_digit()) {
        return 0;
    }
    let hours: i16 = digits[0..2].parse().unwrap_or(0);
    let mins: i16 = digits[2..4].parse().unwrap_or(0);
    if hours > 14 || mins > 59 {
        // No real zone is further out than 14 hours. Past that it is not an
        // offset, whatever it looks like.
        return 0;
    }
    sign * (hours * 60 + mins)
}

pub(super) fn about_short(said: &str) -> String {
    let trimmed = said.trim();
    // Char boundaries, not byte offsets. Slicing a string by byte index
    // panics on any accented character or em dash, which is routine in
    // anything transcribed or pasted.
    let mut out: String = trimmed.chars().take(48).collect();
    if trimmed.chars().count() > 48 {
        out.push('\u{2026}');
    }
    out
}

pub(super) fn clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub(super) fn worth_recording(intent: &Intent, said: &str) -> Option<(String, &'static str, crate::undo::Undo)> {
    use crate::undo::Undo;
    // A failure didn't change anything, so it isn't undoable.
    if said.starts_with("I can't") || said.contains("didn't work") {
        return None;
    }
    Some(match intent {
        Intent::WorkspaceOn => ("brought the workspace up".into(), "windows",
            Undo::Atlas("take it back down".into())),
        Intent::WorkspaceOff => ("put the workspace away".into(), "windows",
            Undo::Atlas("bring it back".into())),
        Intent::OpenApp(a) => (format!("opened {a}"), "windows", Undo::Atlas(format!("close {a}"))),
        Intent::CloseApp(a) => (format!("closed {a}"), "windows", Undo::Atlas(format!("open {a}"))),
        Intent::SetMode(m) => (format!("switched to {m}"), "settings",
            Undo::Atlas("switch back".into())),
        Intent::DraftPost(_) => ("wrote a draft".into(), "posting",
            Undo::Atlas("throw the draft away".into())),
        Intent::BackUp => ("backed itself up".into(), "atlas",
            Undo::Cannot("a backup isn't something to take back".into())),
        // Everything else only read, asked or explained.
        _ => return None,
    })
}
