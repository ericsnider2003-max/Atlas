//! Reading what was said: cutting in, corrections, refiling, names, keys and gestures.
//!
//! Moved out of `daemon.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

/// The talk key, while a reply is being said: held, the reply stops at once
/// and you're listened to for as long as it's down. A stop or a pause is
/// returned as said; anything else is kept in `cut_in`, to be answered
/// next, and returned as `speech::YOUR_TURN` -- you taking the turn, which
/// is answered, not acknowledged with "Paused." (29 Sep 2026).
pub(super) fn key_cut_in(
    keys: Option<&crate::hotkeys::Hotkeys>,
    ears: &dyn Ears,
    cut_in: &std::cell::RefCell<Option<String>>,
) -> Option<String> {
    let keys = keys?;
    if !keys.held() {
        return None;
    }
    // Stopped now, not at the end of the sentence playing.
    crate::micthread::cut_playback();
    let words = ears.listen_while(&|| keys.held()).ok().flatten().unwrap_or_default();
    if crate::speech::is_interruption(&words) {
        return Some(words);
    }
    *cut_in.borrow_mut() = Some(words);
    Some(crate::speech::YOUR_TURN.to_string())
}

/// A reply being said, as the loop sees it (`speakthread`).
impl crate::speakthread::Host for Daemon<'_> {
    fn line(&mut self, chunk: &str) {
        crate::outln!("{chunk}");
        self.log.info(chunk);
    }
    fn between(&mut self) {
        // The hub while Atlas speaks: every page used to wait for the
        // sentence being said, and before 28 Sep 2026 for the whole reply.
        self.answer_hub_mid_reply();
        // And the icon by the clock: its Pause quiets the reply (`hush`).
        self.answer_tray(crate::store::now());
    }
    fn hush(&mut self) -> Option<String> {
        // Paused on the hub or the icon mid-reply (possible now that the hub
        // is answered while a sentence plays): quiet at once, the rest kept
        // for "carry on".
        self.attention.is_paused().then(|| "pause".to_string())
    }
    fn trouble(&mut self, why: &str) {
        self.log.warn(&format!("couldn't say it out loud: {why}"));
    }
}

/// Whether a queued command cannot run without a connection.
///
/// The same shape as `lanes::lane_for`: a keyword reading of the command's
/// own words, not an inspection of what it will do — which is all that can
/// be known before it runs. Kept deliberately narrow: a false `false` means
/// a task fails with an honest network error when it runs offline, while a
/// false `true` means work silently parked on a machine that could have
/// done it. The second is worse, so only commands that are unambiguously
/// about the network are named.
pub(super) fn command_needs_connection(command: &str) -> bool {
    let c = command.to_lowercase();
    const ONLINE: &[&str] = &[
        "research", "look into", "look up", "search the web", "fetch", "download",
        "check mail", "check my mail", "check the mail", "unsubscribe", "sync",
        "post ", "publish",
    ];
    ONLINE.iter().any(|k| c.contains(k))
}

/// Strip a leading correction lead-in so `facts::triple` reads the statement
/// itself, not the marker: "actually my car is a Toyota" → "my car is a
/// Toyota". Case-insensitive on the prefix, but the original casing of the
/// statement is preserved for the acknowledgement. Only removes a lead-in it
/// finds at the very start, so a mid-sentence "actually" is left alone.
pub(super) fn strip_correction_lead(raw: &str) -> String {
    let mut s = raw.trim();
    const LEADS: &[&str] = &[
        "actually,", "actually", "correction:", "correction,", "correction",
        "i meant", "no,", "scratch that,", "scratch that", "update:", "just so you know,",
        "for the record,",
    ];
    loop {
        let low = s.to_ascii_lowercase();
        let mut cut = None;
        for lead in LEADS {
            if low.starts_with(lead) {
                cut = Some(lead.len());
                break;
            }
        }
        match cut {
            Some(n) => s = s[n..].trim_start(),
            None => break,
        }
    }
    // A trailing "now"/"anymore" is a correction marker, not part of the value.
    let mut out = s.to_string();
    for tail in [" now", " anymore", " these days"] {
        if out.to_lowercase().ends_with(tail) {
            out.truncate(out.len() - tail.len());
        }
    }
    out.trim().to_string()
}

/// The kind a refile correction names, if it names one.
///
/// "that's actually a task", "refile that as an idea" -- the word after the
/// article is what decides. Returns `None` when the correction is about where
/// the note belongs (a handle) rather than what sort of thing it is, which is
/// what `refile_handle_from` then reads. A bare "note" is deliberately not a
/// kind here: "file that under my notes" is a handle, and treating the word as
/// a kind would swallow it.
pub(super) fn refile_kind_from(correction: &str) -> Option<crate::capture::Kind> {
    use crate::capture::Kind;
    let t = correction.to_lowercase();
    if t.contains("task") || t.contains("to do") || t.contains("todo") || t.contains("remind") {
        Some(Kind::Task)
    } else if t.contains("idea") {
        Some(Kind::Idea)
    } else if t.contains("question") {
        Some(Kind::Question)
    } else if t.contains("decision") || t.contains("decided") {
        Some(Kind::Decision)
    } else if t.contains("quote") {
        Some(Kind::Quote)
    } else if t.contains("fact") {
        Some(Kind::Fact)
    } else {
        None
    }
}

/// The handle a refile correction files the note under, if any.
///
/// "file that under the roof job" -> "roof job". The leading "under"/"with"
/// and any article are stripped so the handle is the name you would later
/// reach for it by, not the sentence around it.
pub(super) fn refile_handle_from(correction: &str) -> Option<String> {
    let mut h = correction.trim().to_lowercase();
    for lead in ["under ", "with ", "as "] {
        if let Some(rest) = h.strip_prefix(lead) {
            h = rest.to_string();
            break;
        }
    }
    for article in ["the ", "a ", "an ", "my "] {
        if let Some(rest) = h.strip_prefix(article) {
            h = rest.to_string();
            break;
        }
    }
    let h = h.trim().to_string();
    if h.is_empty() {
        None
    } else {
        Some(h)
    }
}

/// A spoken conversion request turned into what `files::convert` says about
/// it, or `None` when it is not a conversion at all.
///
/// `Some` only when both sides of "... to ..." name a format Atlas
/// recognises. That is what keeps an ordinary search which happens to contain
/// the word " to " -- "the note about how to bake bread" -- out of the
/// conversion path: neither "how" nor "bake bread" is a format, so it declines
/// and the filename search answers it.
pub(super) fn convert_answer(what: &str) -> Option<String> {
    use crate::files::{convert, Convert};

    let lower = what.to_lowercase();
    let (before, after) = lower.split_once(" to ")?;
    let from = sort_named(before)?;
    let to = sort_named(after)?;
    let said = match convert(from, to) {
        Convert::Can { how, loses: None } => format!("Yes -- I'd {how}."),
        Convert::Can { how, loses: Some(l) } => format!("I'd {how}, though you'd lose {l}."),
        Convert::CanWithLoss { how, loses } => format!("I can {how}, but you'd lose {loses}."),
        Convert::Cannot(why) => format!("I can't -- {why}."),
    };
    Some(said)
}

/// The formats people name out loud, each mapped to the `Sort` that carries
/// its conversion rules. Deliberately narrow: a word it does not know returns
/// `None`, so `convert_answer` declines rather than reading a conversion into
/// an ordinary sentence.
pub(super) fn sort_named(text: &str) -> Option<crate::files::Sort> {
    use crate::files::Sort;
    text.split_whitespace().find_map(|w| {
        let w = w.trim_matches(|c: char| !c.is_alphanumeric());
        Some(match w {
            "text" | "txt" | "plain" => Sort::Text,
            "word" | "doc" | "docx" | "document" => Sort::Document,
            "excel" | "spreadsheet" | "sheet" | "xlsx" | "csv" => Sort::Sheet,
            "pdf" => Sort::Pdf,
            "picture" | "image" | "photo" | "png" | "jpg" | "jpeg" => Sort::Picture,
            "audio" | "sound" | "mp3" | "recording" => Sort::Audio,
            "video" | "movie" | "mp4" => Sort::Video,
            _ => return None,
        })
    })
}

/// A short, sayable title for a queued change, from the request. The user
/// refers to it by this — "implement the date parser" — so it is the first
/// meaningful few words, not a hash.
pub(super) fn workshop_title(request: &str) -> String {
    // Drop a leading project/verb phrase so the title is about the change,
    // not the project. "on the atlas project, add a date parser" -> "add a
    // date parser".
    let mut r = request.trim();
    for lead in ["on the ", "on ", "in the ", "in ", "for the ", "for "] {
        if let Some(rest) = r.strip_prefix(lead) {
            // Skip up to the first comma, which usually ends the project clause.
            if let Some(idx) = rest.find(',') {
                r = rest[idx + 1..].trim();
                break;
            }
        }
    }
    let words: Vec<&str> = r.split_whitespace().take(7).collect();
    let title = words.join(" ");
    let title = title.trim_end_matches(['.', ',', '!', '?']).trim();
    if title.is_empty() {
        "the change".to_string()
    } else {
        title.to_string()
    }
}

/// What comes after the first of `phrases` found in `said`, or all of it.
pub(super) fn words_after(said: &str, phrases: &[&str]) -> String {
    for p in phrases {
        if let Some(i) = said.find(p) {
            return said[i + p.len()..].trim().to_string();
        }
    }
    said.trim().to_string()
}

/// "a gesture called thumbs up that opens spotify" → ("thumbs up", "open spotify").
pub fn gesture_asked(said: &str) -> Option<(String, String)> {
    let l = said.to_lowercase();
    let after = ["called ", "named "].iter().find_map(|k| l.find(k).map(|i| &l[i + k.len()..]))?;
    let (name, rest) = [" that ", " to ", " for ", " which "]
        .iter()
        .find_map(|sep| after.split_once(sep))
        .unwrap_or((after, ""));
    let does = rest.trim().trim_end_matches(['.', '!']);
    // "opens spotify" is the command "open spotify".
    let does = match does.split_once(' ') {
        Some((verb, obj)) if verb.ends_with('s') && !verb.ends_with("ss") => format!("{} {obj}", &verb[..verb.len() - 1]),
        _ => does.to_string(),
    };
    let name = name.trim().trim_matches('"').to_string();
    (!name.is_empty()).then_some((name, does))
}

/// "control shift space" → "ctrl+shift+space"; "caps lock" → "capslock".
pub fn key_spoken(words: &str) -> String {
    let w = words.to_lowercase().replace(" plus ", "+").replace(" and ", "+").replace('-', "+");
    let w = w
        .replace("caps lock", "capslock")
        .replace("scroll lock", "scrolllock")
        .replace("right control", "rightctrl")
        .replace("right ctrl", "rightctrl")
        .replace("left control", "leftctrl")
        .replace("page up", "pageup")
        .replace("page down", "pagedown")
        .replace("windows key", "win")
        .replace("control", "ctrl");
    let mut parts: Vec<String> = Vec::new();
    for chunk in w.split('+') {
        for p in chunk.split_whitespace() {
            let p = p.trim_matches(|c: char| c == '.' || c == ',' || c == '"');
            if !p.is_empty() && p != "the" && p != "key" {
                parts.push(p.to_string());
            }
        }
    }
    parts.join("+")
}
