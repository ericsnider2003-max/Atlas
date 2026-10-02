//! What Atlas can actually do on an iPhone or iPad.
//!
//! iOS is a much smaller box than Windows and pretending otherwise leads to a
//! phone app that promises things it can't do. Some of what Atlas does on the
//! laptop is simply unavailable — not hard, unavailable — and some of what
//! iOS gives you for free is better than anything I'd have built.
//!
//! The honest summary: **it can hear you, talk back, think, remember, read
//! documents and cameras, and reach the laptop. It cannot watch your screen,
//! touch other apps, or run all the time.**

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Can {
    /// Works, and works well.
    Yes,
    /// Works, with a real limitation attached.
    Limited,
    /// Only while the app is open in front of you.
    OnlyInFront,
    /// iOS does not permit it. Not a matter of effort.
    Never,
}

impl Can {
    pub fn plain(&self) -> &'static str {
        match self {
            Can::Yes => "yes",
            Can::Limited => "yes, with a catch",
            Can::OnlyInFront => "only with the app open",
            Can::Never => "no — iOS doesn't allow it",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ability {
    pub what: &'static str,
    pub can: Can,
    /// The catch, or the reason.
    pub detail: &'static str,
}

/// Everything, honestly.
pub fn abilities() -> Vec<Ability> {
    use Can::*;
    vec![
        // ---- hearing and speaking ----
        Ability { what: "hear what you say and transcribe it", can: Yes,
            detail: "Apple's own speech recogniser, kept on the phone (on-device only). A little \
                     less accurate than the laptop's Parakeet on hard audio" },
        Ability { what: "talk back", can: Yes,
            detail: "iOS has good voices built in, and they cost nothing" },
        Ability { what: "wake on a word while the app is closed", can: Never,
            detail: "iOS won't let an app listen in the background. Use the Action Button, a \
                     Shortcut, or open it — this is the biggest single difference from the laptop" },
        Ability { what: "talk through AirPods", can: Yes,
            detail: "including while the screen is off, once you've started" },
        Ability { what: "keep listening with the screen off", can: Limited,
            detail: "an audio session holds while a conversation is going, but it ends when you \
                     stop — it can't sit waiting all day" },

        // ---- thinking ----
        Ability { what: "think with a local model", can: Limited,
            detail: "a smaller one than the laptop's. Fine for conversation, notes and questions; \
                     long reasoning is better sent to the laptop" },
        Ability { what: "remember everything", can: Yes,
            detail: "the whole log lives on the phone — your projects, notes and history" },
        Ability { what: "search everything you've written", can: Yes,
            detail: "same search as the laptop, over what has synced" },

        // ---- looking at things ----
        Ability { what: "scan a document with the camera", can: Yes,
            detail: "iOS does this itself — edge detection, perspective correction and OCR, all \
                     free and better than anything I'd write" },
        Ability { what: "read text out of a photo", can: Yes,
            detail: "built in, offline, and quick" },
        Ability { what: "see your screen or other apps", can: Never,
            detail: "no app can. This is why it can't do the window and desktop work" },
        Ability { what: "read your photo library", can: Limited,
            detail: "with permission, and you can grant it just the photos you pick" },

        // ---- other apps ----
        Ability { what: "control other apps", can: Never,
            detail: "iOS has no automation for this. The nearest thing is Shortcuts, and only \
                     where an app has chosen to offer them" },
        Ability { what: "read your other apps' data", can: Never,
            detail: "each app's data is sealed off from every other" },
        Ability { what: "receive things you share to it", can: Yes,
            detail: "anything from any app — a link, a PDF, a photo, a selection of text" },
        Ability { what: "read your calendar, contacts and reminders", can: Yes,
            detail: "with permission, through Apple's own interfaces" },
        Ability { what: "send an email or message on your behalf", can: Limited,
            detail: "it can compose one and hand it to Mail or Messages, but you tap send. iOS \
                     doesn't allow otherwise, and I wouldn't want it to" },

        // ---- being there ----
        Ability { what: "run all the time", can: Never,
            detail: "iOS gives short background windows and takes them away if you waste them. \
                     Nothing runs continuously" },
        Ability { what: "do work in short background bursts", can: Limited,
            detail: "a few minutes at a time, when iOS decides — enough to sync and process a \
                     queue, not enough to think hard" },
        Ability { what: "notify you when something finishes", can: Yes,
            detail: "including work the laptop did" },
        Ability { what: "show things on the lock screen", can: Yes,
            detail: "a widget with what's outstanding, and a live one while something's running" },
        Ability { what: "be on the Action Button", can: Yes,
            detail: "iPhone 15 Pro and up — one press, straight into listening" },
        Ability { what: "replace Siri", can: Never,
            detail: "no app can be the system assistant. \"Hey Siri, ask Atlas\" works through \
                     Shortcuts, and that's the closest it gets" },

        // ---- talking to the laptop ----
        Ability { what: "find the laptop on the same wifi", can: Yes,
            detail: "the same way an AirPlay speaker is found — no account, nothing configured" },
        Ability { what: "sync through the cloud folder", can: Yes,
            detail: "works when the two are never on together" },
        Ability { what: "sync over a cable", can: Yes,
            detail: "no network of any kind needed" },
        Ability { what: "AirDrop to your iPad", can: Yes,
            detail: "one tap, nothing online. Not to the laptop though — that's Apple only" },
        Ability { what: "ask the laptop to do something", can: Yes,
            detail: "and it queues if the laptop is off rather than failing" },

        // ---- the vault ----
        Ability { what: "hold your credentials", can: Never,
            detail: "by choice rather than by iOS — the phone is the device you're most likely \
                     to lose" },
        Ability { what: "unlock with Face ID", can: Yes,
            detail: "for the phone's own things, not for the laptop's vault" },
    ]
}

/// The ones that are genuinely off the table, which is the useful list.
pub fn cannot() -> Vec<&'static Ability> {
    let all: &'static [Ability] = Box::leak(abilities().into_boxed_slice());
    all.iter().filter(|a| a.can == Can::Never).collect()
}

/// What works with no signal at all.
pub fn works_offline(what: &str) -> bool {
    // Everything on the phone is local except reaching the laptop and the
    // cloud folder.
    !matches!(what, w if w.contains("cloud") || w.contains("wifi"))
}

/// How you get Atlas listening, in order of how little it costs you.
pub fn ways_to_start() -> Vec<(&'static str, &'static str)> {
    vec![
        ("the Action Button", "one press, straight into listening. iPhone 15 Pro and up"),
        ("\"Hey Siri, ask Atlas\"", "hands free, and Siri hands it straight over"),
        ("the lock screen widget", "one tap without unlocking"),
        ("the back tap", "double or triple tap the back of the phone, set in Accessibility"),
        ("opening the app", "always works"),
    ]
}

/// The single biggest difference from the laptop, said plainly.
pub const THE_BIG_ONE: &str =
    "On the laptop you say the wake word and it's listening. On the phone you have to start it — \
     a button, a Shortcut, or opening the app. iOS won't let anything listen in the background, \
     and no app gets around that. Everything else is closer than you'd expect; this one isn't.";

/// What iOS gives you that's better than building it.
pub const WHAT_IOS_DOES_BETTER: &str =
    "The document scanner. Edge detection, straightening and reading the text are all built into \
     iOS, they're free, and they're better than what I'd write. Point the camera at a page and \
     you get clean text — that's the one thing the phone does better than the laptop.";

/// Where the phone genuinely wins.
pub fn phone_is_better_at() -> Vec<&'static str> {
    vec![
        "scanning anything — the camera is right there and iOS does the hard part",
        "catching a thought the moment you have it",
        "asking something while you're walking",
        "hearing that the laptop finished",
    ]
}

/// Where the laptop wins.
pub fn laptop_is_better_at() -> Vec<&'static str> {
    vec![
        "anything that needs your files",
        "long reasoning, because the model is bigger",
        "windows, apps and the browser",
        "being available the second you speak, with no button",
        "anything with your credentials in it",
    ]
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct IosConfig {
    pub enabled: bool,
    /// Which model size to run on the phone.
    pub model: String,
    // Both stopped being settable on 19 Sep 2026, and the reason is not that
    // they are dangerous — it is that **there is no iOS build**. `atlas
    // catalog --platform ios` says so plainly. Kept rather than deleted
    // because each records a decision made before the thing was built:
    // asking for everything at launch is how you get told no to all of it,
    // and this is where that was decided. See the note in `android.rs`.
    //
    /// Do short background work when iOS allows it.
    #[serde(skip, default = "always")]
    pub background_work: bool,
    /// Ask for the camera, photos, calendar and contacts only when first
    /// needed, rather than all at once at first launch.
    #[serde(skip, default = "always")]
    pub ask_permissions_when_needed: bool,
    /// Never hold credentials on the phone. Not configurable.
    #[serde(skip, default = "never")]
    pub holds_credentials: bool,
}

fn never() -> bool {
    false
}

fn always() -> bool {
    true
}

impl Default for IosConfig {
    fn default() -> Self {
        IosConfig {
            enabled: false,
            model: "small".into(),
            background_work: true,
            // Asking for everything at launch is how you get told no to all
            // of it.
            ask_permissions_when_needed: true,
            holds_credentials: false,
        }
    }
}

/// What Atlas says the first time you open it on the phone.
pub fn first_run() -> String {
    "Most of what I do on the laptop I do here too — hearing you, talking, remembering, reading \
     documents. Two differences worth knowing: I can't listen until you start me, and I can't \
     see your screen or other apps. Everything else works."
        .into()
}
