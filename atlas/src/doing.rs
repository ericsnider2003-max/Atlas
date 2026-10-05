//! Asked to do something, not to talk about it.
//!
//! Eric's evening on the laptop (29 Sep 2026): "I guess I want you to
//! organize my desktop", "I asked you to look at my screen and tell me...",
//! "I want you to go and do a diagnosis on yourself", "what still needs to
//! be set up", "try to listen with my webcam mic", "calm down with being our
//! smart apps" (a smart-ass, misheard). Atlas has something for every one of
//! them, and every one went to the model as conversation: the phrase list
//! matches a sentence that *starts* with a phrase, and none of these did.
//!
//! `rescue` reads such a sentence for the one thing it asks, and hands back
//! the plain command it means ("tidy my desktop", "look at my screen", "run
//! a self check"), which the ordinary phrase list then parses -- so what
//! each command does, and every check on it, stays in one place. It reads
//! only what it can read unambiguously: a sentence that asks for two of
//! these, or none, is left alone for the model.
//!
//! Speech-to-text errors are part of the reading: "smart apps", "smart as"
//! and "smart ask" are "smart-ass"; "at this" at the start is "Atlas".
//!
//! `refers_to_screen` is the other half: whether what was said is about
//! what's on the screen at all, which decides whether the window in front
//! goes into the prompt (`Daemon::conversation_turn`).

/// Words, lower case, apostrophes dropped, with the speech-to-text slips
/// this module knows put right.
fn heard_words(said: &str) -> Vec<String> {
    let mut w = crate::repeating::words(said);
    // "At this" opening a sentence is "Atlas" misheard (the evening: "At
    // this. Are you smart?", "At this you're repeating yourself").
    if w.len() >= 2 && w[0] == "at" && w[1] == "this" {
        w.splice(0..2, ["atlas".to_string()]);
    }
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < w.len() {
        // "smart apps" / "smart as" / "smart ask" / "smart arse": smart-ass.
        if w[i] == "smart" && w.get(i + 1).is_some_and(|n| ["apps", "app", "as", "ask", "arse", "ass", "asses", "asks"].contains(&n.as_str())) {
            out.push("smartass".into());
            i += 2;
            continue;
        }
        if w[i] == "smartarse" || w[i] == "smartapps" {
            out.push("smartass".into());
            i += 1;
            continue;
        }
        if w[i] == "mike" {
            out.push("mic".into());
            i += 1;
            continue;
        }
        out.push(w[i].clone());
        i += 1;
    }
    out
}

fn has(w: &[String], word: &str) -> bool {
    w.iter().any(|x| x == word)
}

fn has_any(w: &[String], words: &[&str]) -> bool {
    words.iter().any(|x| has(w, x))
}

fn has_phrase(w: &[String], phrase: &str) -> bool {
    let hay = format!(" {} ", w.join(" "));
    hay.contains(&format!(" {phrase} "))
}

/// Which of the things below `said` asks for, as the plain command that
/// does it. `None` when it asks for none of them, or for more than one.
pub fn rescue(said: &str) -> Option<String> {
    let w = heard_words(said);
    if w.is_empty() {
        return None;
    }
    let found: Vec<String> = [
        wit(&w),
        desktop(&w),
        screen(&w),
        self_check(&w),
        setup_left(&w),
        microphone(&w),
        one_ask(&w).then(|| machine(&w)).flatten(),
        one_ask(&w).then(|| find_a_file(&w)).flatten(),
        one_ask(&w).then(|| new_mail(&w)).flatten(),
        one_ask(&w).then(|| how_a_post_did(&w)).flatten(),
        one_ask(&w).then(|| a_mode(&w)).flatten(),
    ]
        .into_iter()
        .flatten()
        .collect();
    match found.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    }
}

/// "Calm down with being a smart-ass", "tone down the smart-ass", "be more
/// of a smart-ass", "you can tune it down".
fn wit(w: &[String]) -> Option<String> {
    let about_wit = has(w, "smartass") || has_any(w, &["sarcasm", "sarcastic", "wit", "jokes"]);
    let down = has_phrase(w, "calm down")
        || has_phrase(w, "tone it down")
        || has_phrase(w, "tune it down")
        || has_phrase(w, "tone down")
        || has_phrase(w, "tune down")
        || has_phrase(w, "dial it back")
        || has_phrase(w, "stop being")
        || has(w, "less");
    let up = has(w, "more") || has_phrase(w, "be a smartass");
    if about_wit && down && !up {
        return Some("tone it down".into());
    }
    if has(w, "smartass") && up && !down {
        return Some("be more of a smart ass".into());
    }
    // "You can tune it down. It's in the setting." -- the setting that
    // makes Atlas less of a smart-ass, said without naming it. Only with the
    // setting mentioned, so "tone it down in the email" stays the email's.
    if (has_phrase(w, "tune it down") || has_phrase(w, "tone it down")) && has_any(w, &["setting", "settings"]) {
        return Some("tone it down".into());
    }
    None
}

/// "Organize my desktop", "clean up the desktop", "tidy my desktop".
fn desktop(w: &[String]) -> Option<String> {
    let verb = has_any(w, &["organize", "organise", "organizing", "organising", "tidy", "clean", "sort", "arrange", "declutter", "neaten"]);
    (verb && has(w, "desktop")).then(|| "tidy my desktop".into())
}

/// "Look at my screen and tell me...", "what do you see on my screen",
/// "can you read my screen".
fn screen(w: &[String]) -> Option<String> {
    let on_screen = has_phrase(w, "my screen") || has_phrase(w, "the screen") || has_phrase(w, "my display") || has_phrase(w, "my monitor");
    let looking = has_any(w, &["look", "see", "read", "check", "view", "describe", "watch"]) || has_phrase(w, "whats on");
    // Not the settings of the screen ("turn my screen brightness down").
    let settings = has_any(w, &["brightness", "resolution", "wallpaper", "background", "lock", "record", "recording", "share", "sharing"]);
    (on_screen && looking && !settings).then(|| "look at my screen".into())
}

/// "Do a diagnosis on yourself", "generate a report on yourself", "check
/// yourself", "run a diagnostic".
fn self_check(w: &[String]) -> Option<String> {
    let yourself = has(w, "yourself") || has_phrase(w, "your self") || has_phrase(w, "on you") || has_phrase(w, "your own");
    let checking = w.iter().any(|x| x.starts_with("diagnos") || x.starts_with("self"))
        || has_any(w, &["check", "report", "test", "examine", "audit", "checkup", "inspect", "health"]);
    // "Work on yourself" / "fix yourself" / "improve yourself" are other
    // commands (`work_on_yourself`).
    let changing = has_any(w, &["fix", "improve", "change", "rewrite", "code"]);
    (yourself && checking && !changing).then(|| "run a self check".into())
}

/// "What still needs to be set up", "what's left to set up", "is anything
/// not set up yet".
fn setup_left(w: &[String]) -> Option<String> {
    let set_up = has_phrase(w, "set up") || has(w, "setup") || has_phrase(w, "setting up");
    let left = has_any(w, &["still", "left", "missing", "remaining", "outstanding", "yet", "needs", "need", "unfinished"]);
    // "Set up a meeting", "set up my printer": something else being set up.
    let other = has_any(w, &["meeting", "call", "printer", "reminder", "account", "email", "phone", "appointment"]);
    (set_up && left && !other).then(|| "what's left to set up".into())
}

/// One thing asked, not several: "find the tax pdf and read me what it says"
/// is steps, worked through one by one (`taskloop`), not a file search.
fn one_ask(w: &[String]) -> bool {
    !has_any(w, &["and", "then"])
}

/// "My laptop is running slow, what's eating the memory", "how much space
/// have I got left on this thing". The self-test's everyday sentences (4 Oct
/// 2026): each went to the model to be routed, ten to sixteen seconds on the
/// laptop, for a reading the rules can name.
fn machine(w: &[String]) -> Option<String> {
    let slow = has_phrase(w, "running slow") || has_phrase(w, "so slow") || has_phrase(w, "really slow") || has_phrase(w, "is slow");
    let memory = has_any(w, &["memory", "ram"]) && has_any(w, &["eating", "using", "hogging", "taking", "full"]);
    let space = ["how much space", "how much storage", "how much disk", "space left", "storage left", "space have i", "space do i", "free space", "disk space", "how full is"]
        .iter()
        .any(|p| has_phrase(w, p));
    let about_the_machine = has_any(w, &["laptop", "computer", "pc", "machine", "thing", "disk", "drive", "memory", "ram", "space", "storage", "this"]);
    // "Speed it up" / "close what isn't needed" is the tune-up (`pc_tune`);
    // a slow app or website is about that app.
    let acting = has_any(w, &["close", "kill", "speed", "fix", "optimize", "optimise", "clean", "clear", "free"]) && !space;
    let elsewhere = has_any(w, &["website", "site", "internet", "wifi", "connection", "game", "video", "email"]);
    ((slow || memory || space) && about_the_machine && !acting && !elsewhere).then(|| "how's the machine".into())
}

/// Names a mode usually has.
const MODE_NAMES: &[&str] = &[
    "focus", "work", "gaming", "game", "study", "quiet", "call", "meeting", "presentation", "research", "reading",
    "writing", "night", "deep", "editing", "streaming", "recording", "travel", "public", "cafe",
];

/// "Go into focus", "switch to focus mode", "focus mode on": a mode, named
/// with one of the names modes have. "Go into detail" names none, so the
/// sentence carries on to the model (self-test, 4 Oct 2026: "go into focus"
/// went to the model, 6 s, to be told there is no focus mode).
fn a_mode(w: &[String]) -> Option<String> {
    let lead = match (w.first().map(|s| s.as_str()), w.get(1).map(|s| s.as_str())) {
        (Some("go"), Some("into")) | (Some("switch"), Some("to")) | (Some("enter"), _) => true,
        _ => false,
    };
    let named = w.iter().find(|x| MODE_NAMES.contains(&x.as_str()))?;
    let said_mode = has(w, "mode");
    let short = w.len() <= 4;
    ((lead || said_mode) && short).then(|| format!("mode {named}"))
}

/// Kinds of file a person names when looking for one.
const FILE_KINDS: &[&str] = &[
    "pdf", "pdfs", "file", "files", "document", "documents", "doc", "docx", "spreadsheet", "spreadsheets", "xlsx", "csv",
    "photo", "photos", "picture", "pictures", "presentation", "slides", "receipt", "receipts", "invoice", "invoices",
    "contract", "lease", "statement", "screenshot", "screenshots", "folder",
];

/// "Find the tax pdf from last year", "where's that invoice from March",
/// "locate my lease": a file, said without "find the file".
fn find_a_file(w: &[String]) -> Option<String> {
    let start = match w.first().map(|s| s.as_str()) {
        Some("find") | Some("locate") => 1,
        Some("wheres") => 1,
        Some("where") if w.get(1).is_some_and(|x| x == "is" || x == "are") => 2,
        _ => return None,
    };
    if !w.iter().any(|x| FILE_KINDS.contains(&x.as_str())) {
        return None;
    }
    // "Find me a flight", "find out", "find a gig": not a file.
    if has_any(w, &["out", "flight", "flights", "gig", "gigs", "job", "jobs", "restaurant", "hotel", "song"]) {
        return None;
    }
    let what: Vec<&str> = w[start..]
        .iter()
        .map(|s| s.as_str())
        .skip_while(|x| ["the", "my", "that", "a", "me"].contains(x))
        .collect();
    (!what.is_empty()).then(|| format!("find the file {}", what.join(" ")))
}

/// "Anything new come in by email", "any mail today", "did I get an email".
fn new_mail(w: &[String]) -> Option<String> {
    let mail = has_any(w, &["email", "emails", "mail", "inbox", "gmail", "outlook"]);
    let asking = w.first().is_some_and(|x| ["any", "anything", "did", "is", "has", "have", "check", "whats"].contains(&x.as_str()));
    let new = has_any(w, &["new", "received", "unread"]) || has_phrase(w, "come in") || (asking && has_any(w, &["any", "anything", "check", "inbox"]));
    let writing = has_any(w, &["send", "write", "draft", "reply", "forward", "unsubscribe", "address", "account", "set", "add"]);
    (mail && new && !writing).then(|| "check my email".into())
}

/// "How did my last YouTube video do", "how's my latest TikTok doing".
fn how_a_post_did(w: &[String]) -> Option<String> {
    let last = has_any(w, &["last", "latest", "newest", "recent"]);
    let post = has_any(w, &["video", "post", "reel", "short", "tiktok", "upload"]);
    let how = has(w, "how") || has_any(w, &["views", "performing", "perform", "stats", "numbers"]);
    if !(last && post && how) {
        return None;
    }
    let on = ["youtube", "tiktok", "instagram", "linkedin", "bluesky"].into_iter().find(|p| has(w, p));
    Some(match on {
        Some(p) => format!("how did my last video do on {p}"),
        None => "how did my last video do".into(),
    })
}

/// Words a microphone is known by, for "use my webcam mic".
const MIC_KINDS: &[&str] = &["webcam", "camera", "cam", "headset", "headphones", "usb", "laptop", "builtin", "airpods", "bluetooth", "desk", "external", "blue", "yeti", "rode", "shure"];

/// "Use my webcam mic", "listen with the webcam microphone", "switch to the
/// headset mic" -- the kind named (not one after "not": "using my webcam mic
/// right now, not my laptop mic").
fn microphone(w: &[String]) -> Option<String> {
    if !(has(w, "mic") || has(w, "microphone")) {
        return None;
    }
    let wants = has_any(w, &["use", "using", "listen", "switch", "try", "change", "hear"]);
    if !wants {
        return None;
    }
    let mut picked: Vec<&str> = Vec::new();
    for (i, x) in w.iter().enumerate() {
        let Some(kind) = MIC_KINDS.iter().find(|k| **k == x.as_str()) else { continue };
        // Said as the one NOT to use.
        let negated = w[i.saturating_sub(3)..i].iter().any(|p| p == "not" || p == "instead" || p == "than");
        if !negated && !picked.contains(kind) {
            picked.push(kind);
        }
    }
    match picked.as_slice() {
        [one] => Some(format!("switch microphone to {one}")),
        _ => None,
    }
}

/// Does what was said point at the screen, a window, or what's in front of
/// the user? Only then does the window's name go into the prompt: on Eric's
/// laptop it went in on every turn, and a Discord window in front turned
/// half an evening's replies into commentary on a Discord server.
///
/// `app` is the program in front ("Discord"), which counts when named.
pub fn refers_to_screen(said: &str, app: &str) -> bool {
    let w = heard_words(said);
    if w.is_empty() {
        return false;
    }
    const WORDS: &[&str] = &["screen", "window", "windows", "display", "monitor", "tab", "page", "onscreen"];
    if has_any(&w, WORDS) {
        return true;
    }
    const PHRASES: &[&str] = &[
        "this app", "this program", "this chart", "this graph", "this document", "this file", "this site",
        "this website", "this server", "this channel", "this chat", "this video", "this one here", "on here",
        "in front of me", "im looking at", "i am looking at", "what am i looking at", "whats this", "what is this",
        "look at this", "read this", "explain this", "see this", "what do you see", "can you see",
    ];
    if PHRASES.iter().any(|p| has_phrase(&w, p)) {
        return true;
    }
    let app = app.trim().to_lowercase();
    let app = app.trim_end_matches(".exe");
    !app.is_empty() && app.len() >= 3 && has(&w, app)
}

/// Does this read as asking for something to be done? For a sentence the
/// phrases didn't match: the model is told to say what it can do and ask
/// one short question if none of its tools does it, rather than chat.
pub fn looks_like_an_action(said: &str) -> bool {
    crate::thread::is_a_request(said)
}
