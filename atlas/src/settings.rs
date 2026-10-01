//! Every switch in one place.
//!
//! Atlas has accumulated a lot of behaviour that can be turned on and off, and
//! until now the only way to change any of it was to edit YAML. That's a gap:
//! a setting nobody can find is a setting that doesn't exist.
//!
//! This is the registry the hub renders. Each entry knows what it is, what it
//! does, what happens if you change it, and — importantly — **what it costs**.
//! A toggle that turns on a camera should not look identical to one that
//! changes how many sentences Atlas speaks.

use serde::{Deserialize, Serialize};

/// What changing this actually means.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weight {
    /// Taste. Change it freely.
    Preference,
    /// Costs memory, battery or speed.
    Resource,
    /// Turns on a sensor, or lets Atlas reach outside the machine.
    Sensitive,
    /// Changes what Atlas may do without asking.
    Permission,
}

impl Weight {
    pub fn label(&self) -> &'static str {
        match self {
            // Plain descriptions of what turning the setting on means (29
            // Sep 2026): "changes permissions" read as an instruction, and
            // Eric went looking for a permissions screen that doesn't exist.
            Weight::Preference => "your preference",
            Weight::Resource => "uses memory or battery",
            Weight::Sensitive => "uses the mic, camera or internet",
            Weight::Permission => "lets Atlas act without asking",
        }
    }
    /// Should the hub make you confirm?
    pub fn needs_confirming(&self) -> bool {
        matches!(self, Weight::Sensitive | Weight::Permission)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Value {
    Toggle(bool),
    Number { value: f64, min: f64, max: f64 },
    Text(String),
    Choice { value: String, options: Vec<String> },
    List(Vec<String>),
}

impl Value {
    pub fn as_display(&self) -> String {
        match self {
            Value::Toggle(b) => if *b { "on".into() } else { "off".into() },
            Value::Number { value, .. } => format!("{value}"),
            Value::Text(s) | Value::Choice { value: s, .. } => s.clone(),
            Value::List(v) => {
                if v.is_empty() { "none".into() } else { v.join(", ") }
            }
        }
    }

    /// Apply a string from a form field, rejecting anything nonsensical.
    fn set_from(&mut self, raw: &str) -> Result<(), String> {
        match self {
            Value::Toggle(b) => {
                *b = matches!(raw.trim().to_lowercase().as_str(), "on" | "true" | "yes" | "1");
                Ok(())
            }
            Value::Number { value, min, max } => {
                let n: f64 = raw.trim().parse().map_err(|_| format!("{raw} isn't a number"))?;
                if n < *min || n > *max {
                    return Err(format!("must be between {min} and {max}"));
                }
                *value = n;
                Ok(())
            }
            Value::Text(s) => {
                *s = raw.trim().to_string();
                Ok(())
            }
            Value::Choice { value, options } => {
                let r = raw.trim();
                if !options.iter().any(|o| o == r) {
                    return Err(format!("must be one of: {}", options.join(", ")));
                }
                *value = r.to_string();
                Ok(())
            }
            Value::List(v) => {
                *v = raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Setting {
    /// Dotted path into the config, e.g. "presence.enabled".
    pub key: String,
    /// What it's called in the hub.
    pub name: String,
    /// One sentence. What it does, in plain language.
    pub what: String,
    /// The honest cost or consequence. Empty when there isn't one.
    #[serde(default)]
    pub cost: String,
    pub value: Value,
    pub default: Value,
    pub weight: Weight,
    /// Which page it appears under.
    pub group: String,
}

impl Setting {
    /// Has this been changed from the default?
    ///
    /// Numbers are compared with a tolerance. An `f32` config value widened to
    /// `f64` is 0.3499999940395355, which is not equal to 0.35 — and without
    /// this the hub reports settings you never touched as changed.
    pub fn changed(&self) -> bool {
        match (&self.value, &self.default) {
            (Value::Number { value: a, .. }, Value::Number { value: b, .. }) => {
                (a - b).abs() > 1e-6
            }
            (a, b) => a != b,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Settings {
    pub items: Vec<Setting>,
}

/// The categories, in order, each with the sentence that says what it covers.
///
/// A heading with nothing under it makes you open every section to find out
/// which one holds the thing you came for.
pub const GROUP_ORDER: &[(&str, &str)] = &[
    ("Talking to it", "How you get a request in — voice, typing, and which languages."),
    ("Keys", "The keys that reach Atlas from any app: push-to-talk and the typing box, set by pressing them."),
    ("How it talks back", "Length, tone, and whether it admits when it isn't sure."),
    ("Sound", "When Atlas speaks out loud, how loud, quiet hours, and when it may pop up."),
    ("When it speaks first", "Whether Atlas starts a conversation, and when it holds off."),
    ("What it can see", "The screen, the camera, the clipboard, and what it notices about how you work."),
    ("What it may touch", "Changes it can make to this machine and to its own work, without asking each time."),
    ("Your accounts and secrets", "The vault, signing in, and how carefully it treats what it holds."),
    ("Reaching outside this machine", "Anything that leaves the laptop: the web, your phone, your mail, a paid model."),
    ("Social", "Your social accounts' numbers and the people you watch: which of them Atlas reads on its own."),
    ("Looking for opportunities", "Gigs, grants and niches found once a day: where from, how many requests, and what makes the brief."),
    ("Your devices", "Where your devices meet to carry things between them, and what this one is called. Set on the Sync page."),
];

/// The settings that only take effect when Atlas starts, because what they
/// switch is set up once, at the start: the listening loop and its voice,
/// the wake-word listener, the typing key, the phone's listener, the model
/// budget the helpers are sized to, and the identity check the door in front
/// of the daemon was handed.
///
/// Everything else is read at the moment it's used, so a change applies the
/// next time Atlas looks — within a few seconds, with no restart (Eric, 24 Sep
/// 2026: settings that apply live). The list is short on purpose: a setting
/// put here needlessly costs a restart, one left out wrongly says "done" when
/// it isn't, so each one was checked against where it is read.
pub const NEEDS_A_RESTART: &[&str] = &[
    "voice.enabled",
    // 30 Sep 2026: `wake.enabled` was listed here though `pick_up_settings`
    // switches the wake word on and off at once; the phrase is the part the
    // listener keeps from the start.
    "wake.phrase",
    "quick_input.enabled",
    // The keys are handed to Windows once, when Atlas starts.
    "quick_input.hotkey",
    "push_to_talk.enabled",
    "push_to_talk.key",
    "server.enabled",
    "companion.enabled",
    "voice_settings.voice",
    "voice_settings.speed",
    // The voice is built once, when Atlas starts, with its engine in it.
    "tts_engine.engine",
    "endpoint.enabled",
    "models.memory_budget_mb",
    // Both are flags on the model server's command line, read when it starts.
    "models.draft",
    "models.speculate",
    "identity.enabled",
    "identity.trusted_devices",
    // The icon is put up once, when the background Atlas starts.
    "desktop.tray_icon",
];

/// Does a change to this setting wait for Atlas to start again?
pub fn needs_a_restart(key: &str) -> bool {
    NEEDS_A_RESTART.contains(&key)
}

impl Settings {
    /// The settings whose values differ between two readings of the list,
    /// by key. How a running Atlas says what it just picked up.
    pub fn differences(&self, newer: &Settings) -> Vec<Setting> {
        newer
            .items
            .iter()
            .filter(|n| match self.get(&n.key) {
                Some(o) => {
                    let mut probe: Setting = (*n).clone();
                    probe.default = o.value.clone();
                    probe.changed()
                }
                None => true,
            })
            .cloned()
            .collect()
    }

    pub fn get(&self, key: &str) -> Option<&Setting> {
        self.items.iter().find(|s| s.key == key)
    }

    /// Validate a change, then write it to your preferences in `dir`. What
    /// to say either way: the confirmation, why it wasn't taken, or that it
    /// couldn't be kept. The one path the hub's settings forms use, with
    /// Atlas running or not, so a switch can't report success and write
    /// nothing (27 Sep 2026: the settings-only window did exactly that).
    pub fn set_and_keep(&mut self, key: &str, raw: &str, dir: &std::path::Path) -> String {
        match self.set(key, raw) {
            Err(e) => e,
            Ok(confirmation) => {
                // Checked, not defaulted (30 Sep 2026): a settings file that
                // won't read loaded as "nothing chosen", and saving that put
                // every other setting back while this page said "it's on".
                let mut prefs = match crate::preferences::Preferences::load_checked(dir) {
                    Ok(p) => p,
                    Err(e) => return format!("I haven't changed anything: {e}. Fix that file or delete it, then try again."),
                };
                prefs.set(key, raw);
                match prefs.save(dir) {
                    Ok(()) => confirmation,
                    // Said, not swallowed. "It is now on" when the file could
                    // not be written is the same lie in a new place.
                    Err(e) => format!(
                        "I couldn't keep that change: {e}. It will go back to \
                         what it was when I next start."
                    ),
                }
            }
        }
    }

    pub fn set(&mut self, key: &str, raw: &str) -> Result<String, String> {
        let s = self
            .items
            .iter_mut()
            .find(|s| s.key == key)
            .ok_or_else(|| format!("no setting called {key}"))?;
        // A key that can't work is refused here, before it's kept, rather
        // than found not to work at the next start.
        if matches!(key, "push_to_talk.key" | "quick_input.hotkey") {
            crate::hotkeys::check_setting(key, raw.trim())?;
        }
        s.value.set_from(raw)?;
        Ok(format!("{} is now {}", s.name, s.value.as_display()))
    }

    pub fn reset(&mut self, key: &str) -> Result<String, String> {
        let s = self
            .items
            .iter_mut()
            .find(|s| s.key == key)
            .ok_or_else(|| format!("no setting called {key}"))?;
        s.value = s.default.clone();
        Ok(format!("{} back to {}", s.name, s.value.as_display()))
    }

    /// The categories, in the order a person would want them.
    ///
    /// Deliberately not alphabetical. Sorting put "Acting" first because of
    /// its A, and "Acting" was itself a bucket holding twenty of the forty-odd
    /// settings — which is not a category, it is where things went when nobody
    /// decided. The order below runs from what you change often to what you
    /// change once, and anything not named falls to the end rather than
    /// disappearing.
    pub fn groups(&self) -> Vec<String> {
        let mut g: Vec<String> = self.items.iter().map(|s| s.group.clone()).collect();
        g.sort();
        g.dedup();
        g.sort_by_key(|name| {
            GROUP_ORDER
                .iter()
                .position(|(g, _)| *g == name)
                .unwrap_or(GROUP_ORDER.len())
        });
        g
    }

    /// The one-line explanation of a category, when there is one.
    pub fn group_note(group: &str) -> Option<&'static str> {
        GROUP_ORDER.iter().find(|(g, _)| *g == group).map(|(_, n)| *n)
    }

    pub fn in_group(&self, group: &str) -> Vec<&Setting> {
        self.items.iter().filter(|s| s.group == group).collect()
    }

    /// Everything you've changed from the default. The answer to "what have I
    /// actually turned on?"
    pub fn changed(&self) -> Vec<&Setting> {
        self.items.iter().filter(|s| s.changed()).collect()
    }

    /// Everything that lets Atlas reach a sensor, the network, or act without
    /// asking. The page worth reading once a month.
    pub fn consequential(&self) -> Vec<&Setting> {
        let mut v: Vec<&Setting> =
            self.items.iter().filter(|s| s.weight.needs_confirming()).collect();
        v.sort_by(|a, b| b.weight.cmp(&a.weight));
        v
    }

    /// Anything on that isn't doing anything useful, so the hub can suggest
    /// turning it off rather than leaving it costing you battery.
    pub fn idle_but_on(&self, unused_keys: &[String]) -> Vec<&Setting> {
        self.items
            .iter()
            .filter(|s| matches!(s.value, Value::Toggle(true)))
            .filter(|s| unused_keys.iter().any(|k| *k == s.key))
            .collect()
    }
}

/// No `default_on` parameter, on purpose.
///
/// It used to take one, written by hand beside the live value, and that
/// second hand-written declaration is the whole bug this shape removes:
/// `registry` derives the default by building the list twice, so there is
/// nowhere left to write a default that disagrees with the code.
fn toggle(
    key: &str,
    name: &str,
    what: &str,
    cost: &str,
    on: bool,
    weight: Weight,
    group: &str,
) -> Setting {
    Setting {
        key: key.into(),
        name: name.into(),
        what: what.into(),
        cost: cost.into(),
        value: Value::Toggle(on),
        // A placeholder. `registry` overwrites it with the value this same
        // entry takes against `ToolsConfig::default()`.
        default: Value::Toggle(on),
        weight,
        group: group.into(),
    }
}

/// The registry, built from the live config.
///
/// Deliberately hand-written rather than derived: the point is the plain
/// explanation and the honest cost beside each one, and neither can be
/// generated from a struct field.
/// Every setting the hub shows, with its current value *and* the value it
/// would have on a fresh install.
///
/// The default is not written down here. It is obtained by running this
/// same list against `ToolsConfig::default()` and taking each item's value —
/// so "the default" is, by construction, whatever the feature's own code
/// says it is, and the two cannot disagree.
///
/// They used to. Every entry below carried a hand-written default beside its
/// live value, and nothing checked the two against each other. Three had
/// drifted apart: `voice.enabled` and `research.enabled` were written `true`
/// here against `Default` impls that say false, and `models.memory_budget_mb`
/// was written 3500 against a struct default of 6000. The hub's "changed
/// from default" marker was therefore lying about three settings, and — more
/// to the point — a person reading the hub and a person reading the code got
/// different answers to the same question. Deriving it removes the second
/// answer rather than adding a test that the two answers match.
pub fn registry(t: &crate::voice::ToolsConfig) -> Settings {
    let mut live = build(t);
    let fresh = build(&crate::voice::ToolsConfig::default());
    for (item, def) in live.items.iter_mut().zip(fresh.items.iter()) {
        debug_assert_eq!(item.key, def.key, "the two builds must produce the same list");
        item.default = def.value.clone();
    }
    live
}

/// The list itself. Each item's `default` is a placeholder equal to its own
/// value; `registry` replaces it with the real one. Nothing outside this
/// module may call this — a `Settings` whose defaults are placeholders is
/// exactly the wrong thing to hand to a hub page.
fn build(t: &crate::voice::ToolsConfig) -> Settings {
    use Weight::*;
    let mut items = vec![
        // --- listening and speaking ---
        toggle("voice.enabled", "Voice", "Hear you and speak back.",
            "Needs whisper and piper installed.", t.enabled, Preference, "Talking to it"),
        toggle("wake.enabled", "Wake word", "Listen for its name so you don't press anything.",
            "Without a detector binary this transcribes constantly and keeps a CPU core busy.",
            t.wake.as_ref().map(|w| w.enabled).unwrap_or(false), Resource, "Talking to it"),
        toggle("barge_in.enabled", "Voice cut-in",
            "Talk over Atlas while it's speaking and it stops to listen to you.",
            "Best with a headset: through speakers the microphone hears Atlas too, and it may stop itself.",
            t.barge_in.enabled, Preference, "Sound"),
        toggle("voice_id.enabled", "Voice recognition",
            "Ignore voices that aren't yours, so the TV can't drive your workspace.",
            "Never used as permission — a recording of you sounds like you.",
            t.voice_id.enabled, Sensitive, "Talking to it"),
        toggle("quick_input.enabled", "Quick input",
            "A key that opens a box to type a command from any app.",
            "", t.quick_input.enabled, Preference, "Keys"),

        // --- seeing ---
        toggle("presence.enabled", "Desk presence",
            "Uses the camera to tell whether you're there, so it doesn't talk to an empty room.",
            "Turns on your camera at a low sample rate. Frames never leave the machine.",
            t.presence.enabled, Sensitive, "What it can see"),
        toggle("picture_talk.enabled", "Reading pictures",
            "Say what a chart, your screen or a photo shows when you ask, as in \"what does this chart show?\" or \"look at my screen\".",
            "Needs a 3 GB download, which Atlas's setup fetches. About 3 GB of memory while it answers, none otherwise. Nothing leaves the laptop.",
            t.picture_talk.enabled, Preference, "What it can see"),
        toggle("ocr.enabled", "Screen reading",
            "Read text that no app will hand over, and text in photographs.",
            "Needs Tesseract installed. Fast, CPU only.",
            t.ocr.enabled, Preference, "What it can see"),

        // --- acting ---
        toggle("proactive.enabled", "Speaking first",
            "Offer help when it notices something worth mentioning.",
            "Interrupts you. Capped per hour, and it learns what you decline.",
            t.proactive.enabled, Permission, "When it speaks first"),
        toggle("identity.enabled", "Identity checks",
            "Windows Hello before the few actions that can't be undone.",
            "Only the listed actions, and not again for four hours.",
            t.identity.enabled, Permission, "Your accounts and secrets"),
        toggle("server.enabled", "Phone access",
            "A local endpoint so you can reach Atlas from another device.",
            "Loopback only, token required. Reaching it from your phone needs a VPN.",
            t.server.enabled, Sensitive, "Reaching outside this machine"),
        toggle("research.enabled", "Web research",
            "Look things up and write them into a note.",
            "The only thing here that leaves your machine.",
            t.research.enabled, Sensitive, "Reaching outside this machine"),
        // Social (29 Sep 2026): the two parts that fetch on their own. Asking
        // works with both off; these are only the schedules.
        toggle("workday.social.own_refresh", "Your accounts' numbers",
            "Once a day, read your own YouTube, Instagram, Threads, Facebook Page, TikTok and Bluesky numbers from their free official APIs (the ones you set up on the Social page) and keep them, so the history outlives the platforms' 28 to 90 days.",
            "Uses your keys from the vault. About four of YouTube's 10,000 daily units, and 27 Instagram calls.",
            t.workday.social.own_refresh, Sensitive, "Social"),
        toggle("workday.social.scan", "Watching others",
            "Read the channels, hashtags and topics you watch from their public feeds, and say what's working for them.",
            "One read of a source an hour at most, spaced out per site. Never TikTok, Instagram or X -- those only when you ask, one page.",
            t.workday.social.scan, Sensitive, "Social"),
        // Which of your own accounts the refresh reads (29 Sep 2026), set
        // from the Social page. Each needs its key in the vault; one that's
        // on without it is named as not set up.
        toggle("workday.social.instagram", "Instagram numbers",
            "Your Instagram's followers and each post's views, reach, saves and shares, through the Instagram API.",
            "Needs a Professional account and a token in the vault (the Social page says how).",
            t.workday.social.instagram, Sensitive, "Social"),
        toggle("workday.social.threads", "Threads numbers",
            "Your Threads followers and each post's views, likes, replies, reposts, quotes and shares.",
            "Needs a token from your own Meta app in the vault.",
            t.workday.social.threads, Sensitive, "Social"),
        toggle("workday.social.facebook_page", "Facebook Page numbers",
            "Your Page's followers and each post's views, reactions, comments and shares.",
            "Needs a Page token in the vault. A personal profile has no numbers to read.",
            t.workday.social.facebook_page, Sensitive, "Social"),
        toggle("workday.social.tiktok", "TikTok numbers",
            "Your TikTok followers and each video's views, likes, comments and shares, through TikTok's Display API.",
            "Needs your own TikTok app's Sandbox and a sign-in from the Social page. TikTok gives no watch time.",
            t.workday.social.tiktok, Sensitive, "Social"),
        toggle("workday.social.youtube_analytics", "YouTube retention",
            "How much of each video was watched, and watch time, from the YouTube Analytics API.",
            "Needs a Google sign-in from the Social page.",
            t.workday.social.youtube_analytics, Sensitive, "Social"),
        toggle("workday.social.google_app_in_testing", "Google app testing",
            "On while your Google app is in Testing: Google ends its sign-ins after seven days, and I warn you a day before.",
            "",
            t.workday.social.google_app_in_testing, Preference, "Social"),
        // The switch that used to be a line in tools.yaml and two terminal
        // commands. It is here because the person it is for does not type
        // commands, and a protection nobody can find is not a protection.
        //
        // `Sensitive` rather than `Permission`: it does not let Atlas do
        // anything new, it changes what a folder someone else holds a copy of
        // can be read as.
        toggle("sync.encrypt_bundles", "Sealed bundles",
            "Bundles in your sync folder are scrambled, so a cloud provider holds \
             something unreadable rather than your notes.",
            "I make the key myself and keep it on each device. If every copy is ever \
             lost, one button makes a new one and nothing is lost — a bundle is a \
             courier, not where your notes live.",
            t.sync.encrypt_bundles, Sensitive, "Reaching outside this machine"),

        // --- looking after itself ---
        toggle("finance.enabled", "Finances",
            "Read statements and account pages, and flag anything odd.",
            "Read-only by construction: it cannot submit a form or click anything that moves money.",
            t.finance.enabled, Sensitive, "Reaching outside this machine"),
        toggle("call_notes.enabled", "Call notes",
            "When a call starts, note your side; when it ends, write up what was said and decided into your notes.",
            "Nobody else is recorded until you ask them and tell me they said yes; one no means your side only. Audio is deleted after a week, the notes stay. Transcribed on this laptop.",
            t.call_notes.enabled, Permission, "Reaching outside this machine"),
        toggle("delegate.enabled", "Working your apps",
            "When you ask, draft a reply in the window in front, or carry the conversation on while you're away.",
            "Writes as you. Types only in a gap in your own typing, never sends in apps you've set to confirm every message, and \"stop\" pauses it.",
            t.delegate.enabled, Permission, "Reaching outside this machine"),
        toggle("trace.keep_examples", "Keep graded examples",
            "Keep the words of a model call when it was graded — a reply rewritten, a council seat, a turn you corrected — so a new model can be tested against them.",
            "Stays on this laptop and in your backups. Emails, phone numbers and long numbers are taken out first; names are not.",
            t.trace.keep_examples, Sensitive, "Your accounts and secrets"),
        toggle("answering.accept_gestures", "Gestures",
            "A thumbs up or down on camera answers a question when you can't speak.",
            "Needs the camera on. Can decline anything, but never approves something irreversible.",
            t.answering.accept_gestures, Sensitive, "Talking to it"),
        toggle("clipboard.enabled", "Clipboard",
            "Copy something and say \"explain this\".",
            "Only read when you ask — never watched in the background.",
            t.clipboard.enabled, Preference, "What it can see"),
        toggle("system.enabled", "System changes",
            "Set a wallpaper, tidy files into folders, adjust appearance settings.",
            "Never touches security, accounts or networking. Files go through the trash.",
            t.system.enabled, Permission, "What it may touch"),
        toggle("overlay.enabled", "Desktop captions",
            "While it speaks, show what it's saying over whatever you're looking at.",
            "Clicks pass straight through it, and it's gone a few seconds after the voice stops. Windows only.",
            t.overlay.enabled, Preference, "How it talks back"),
        toggle("desktop.tray_icon", "Taskbar icon",
            "While Atlas runs in the background, show its icon by the clock: open it, open the hub in your browser, pause it, or quit it from there.",
            "Without it, the only sign Atlas is running with no window open is that it answers. Windows only.",
            t.desktop.tray_icon, Preference, "How it talks back"),
        toggle("certainty.enabled", "Stated confidence",
            "Hold back an answer it isn't confident in, and say what would settle it.",
            "Occasionally holds back something it would have got right.",
            t.certainty.enabled, Preference, "How it talks back"),
        toggle("tune.enabled", "Laptop upkeep",
            "Find what is holding memory, slowing startup, or filling the disk.",
            "Only ever turns off startup items and clears temporary files — never drivers or services.",
            t.tune.enabled, Sensitive, "What it may touch"),
        toggle("budget.enabled", "Paid model",
            "Let Atlas reach for a hosted model for work the local one cannot do.",
            "The only thing that costs money, and the only thing that leaves your machine. Hard capped.",
            t.budget.enabled, Sensitive, "Reaching outside this machine"),
        toggle("overnight.enabled", "Overnight work",
            "Take on problems it could not finish, in the sandbox, while you sleep.",
            "Nothing is ever applied unattended — you accept the changes in the morning.",
            t.overnight.enabled, Permission, "What it may touch"),
        toggle("language.multilingual", "Other languages",
            "Hear and translate languages other than English, in calls or around you.",
            "Needs the multilingual speech model — same size, slightly less accurate on English.",
            t.language.multilingual, Preference, "Talking to it"),
        toggle("language.translate_others", "Translation",
            "Bring anything that is not your language back in English.",
            "The original is always kept alongside — a translation is an interpretation.",
            t.language.translate_others, Preference, "Talking to it"),
        toggle("endpoint.enabled", "End of speech",
            "End the recording on silence rather than after a fixed eight seconds.",
            "Much faster, and transcribes far less audio.",
            t.endpoint.enabled, Preference, "Talking to it"),
        toggle("dictate.enabled", "Dictation",
            "Speak and the words go into the window you are looking at.",
            "Never types into chat apps — a misheard sentence there is public.",
            t.dictate.enabled, Permission, "Talking to it"),
        toggle("watching.enabled", "Finish alerts",
            "Watch a render or a build and report the outcome.",
            "Only mentions things that ran long enough to be worth it. Failures always.",
            t.watching.enabled, Preference, "When it speaks first"),
        toggle("recall.semantic", "Meaning search",
            "Find something by what it was about, not just the words in it.",
            "Needs a small embedding model. Searching by words works without it.",
            t.recall.semantic, Preference, "What it can see"),
        toggle("self_work.enabled", "Self-repair",
            "Change its own source in the sandbox, run its own tests, and show you the diff.",
            "It cannot edit the files that decide what it is allowed to do, and nothing lands without you.",
            t.self_work.enabled, Permission, "What it may touch"),
        toggle("draft.revise", "Draft revision",
            "Read what it wrote, say what is missing, and rewrite it.",
            "Never rewrites when there is nothing to fix — that makes writing worse, not better.",
            t.draft.revise, Preference, "What it may touch"),
        toggle("person.notice_patterns", "Work patterns",
            "Say something when you have been at it unusually late, or a project has stalled.",
            "Only things checkable against a clock. It never guesses at how you feel.",
            t.person.notice_patterns, Preference, "What it can see"),
        toggle("wanted.ask_when_unclear", "Clarifying questions",
            "When it cannot tell whether you want ideas or just to be heard, ask.",
            "One short question. It learns your answer per topic and stops asking.",
            t.wanted.ask_when_unclear, Preference, "How it talks back"),
        toggle("prose.enabled", "Typing correction",
            "Fix dropped apostrophes and typos in place as you type in your other apps, and learn the fixes you make yourself.",
            "Types into your text box when you pause after a word. Change a fix back and it's left alone in that text. Never in code, terminals or password boxes.",
            t.prose.enabled, Permission, "What it may touch"),
        toggle("unsub.enabled", "Inbox clearing",
            "Unsubscribe from what you never read, block what has no safe way out.",
            "Never clicks unsubscribe in spam — that confirms your address is real.",
            t.unsub.enabled, Permission, "Reaching outside this machine"),
        toggle("mail.enabled", "Email",
            "Sort your inbox into categories, in your mailbox rather than in Atlas.",
            "Nothing is ever deleted. Labels show on your phone and stay if you stop using Atlas.",
            t.mail.enabled, Sensitive, "Reaching outside this machine"),
        toggle("interrupt.respect_focus", "Focus time",
            "Say nothing while you are head down, and hold it until you stop.",
            "Urgent things still get through.",
            t.interrupt.respect_focus, Preference, "When it speaks first"),
        toggle("returning.offer_first", "Brief on return",
            "When you come back, say there are updates instead of reciting them.",
            "Anything wrong or waiting is still said straight away.",
            t.returning.offer_first, Preference, "When it speaks first"),
        toggle("routine.enabled", "Routines",
            "Spot sequences you repeat and offer to do them.",
            "Asks after the third time. Never automates anything that sends.",
            t.routine.enabled, Permission, "What it can see"),
        toggle("accounts.enabled", "Account security",
            "Find where two-factor is on, where it is weak, and what is worth fixing first.",
            "Atlas never changes a security setting — it opens the page and tells you where the switch is.",
            t.accounts.enabled, Sensitive, "Your accounts and secrets"),
        toggle("going_away.enabled", "Travel prep",
            "Check which accounts would lock you out with no signal, and what to do first.",
            "Never changes a security setting. It opens the pages and tells you what to print.",
            t.going_away.enabled, Preference, "Reaching outside this machine"),
        // "The vault" toggle removed 28 Sep 2026: `vault.enabled` was read by
        // nothing, so the switch changed nothing while the vault kept working.
        // The vault is always there; it is locked until you give the passphrase.
        toggle("signin.enabled", "Site sign-in",
            "Fill your login on sites you have granted, from the locked vault.",
            "Only on the exact domain, never on a lookalike. Take access away any time at Access.",
            t.signin.enabled, Sensitive, "Your accounts and secrets"),
        toggle("enrol.enabled", "Make accounts",
            "Sign you up on a site you name: fill the form, make the password, keep it in the vault.",
            "Agrees to the site's terms as you. Stops for good at payment or ID, and hands a robot check or a code to you.",
            t.enrol.enabled, Permission, "Your accounts and secrets"),
        toggle("walkthrough.atlas_clicks", "Security switches",
            "After reading a security change back and hearing your yes, Atlas presses the switch itself instead of leaving it to you.",
            "Only when exactly one control on the page is labelled for that change. It never types on a security page — if it has to sign in first, that's Site sign-in.",
            t.walkthrough.atlas_clicks, Permission, "Your accounts and secrets"),
        toggle("confirmed.enabled", "Security changes",
            "Do what you ask on a security page, after reading it back and waiting for a yes.",
            "Only with you at the machine, one change per yes, and every one is recorded.",
            t.confirmed.enabled, Sensitive, "Your accounts and secrets"),
        toggle("opsec.enabled", "Outgoing checks",
            "Look for a patch, a gate sign, a tail number, or a date you are moving.",
            "Stops entirely on the date you set. It checks the frame, never your opinions.",
            t.opsec.enabled, Preference, "Your accounts and secrets"),
        toggle("companion.enabled", "Phone companion",
            "See what Atlas knows and capture things, with the laptop off.",
            "A window and a notebook. Nothing that opens something else ever leaves the laptop.",
            t.companion.enabled, Preference, "Reaching outside this machine"),
        toggle("backup.enabled", "Backups",
            "A daily copy of everything it has learned.",
            "A few megabytes. Seven kept.",
            t.backup.enabled, Preference, "What it may touch"),
    ];

    // Where your devices meet, and what this one is called: set from the Sync
    // page's first step (27 Sep 2026), which is the only way a person who
    // never edits tools.yaml could set them. Text, kept in settings.yaml like
    // every other setting and read at the moment of use.
    items.push(Setting {
        key: "sync.folder".into(),
        name: "Sync folder".into(),
        what: "The folder your devices meet in — one your cloud drive keeps in step is best.".into(),
        cost: "Whoever can read that folder can read what I leave there, unless sealing is on.".into(),
        value: Value::Text(t.sync.folder.clone()),
        default: Value::Text(t.sync.folder.clone()),
        weight: Sensitive,
        group: "Your devices".into(),
    });
    // Your own handles, for the Social page's refresh (29 Sep 2026).
    items.push(Setting {
        key: "workday.social.youtube_channel".into(),
        name: "Your YouTube channel".into(),
        what: "Your channel's @handle or its UC... id, for reading your own numbers.".into(),
        cost: "".into(),
        value: Value::Text(t.workday.social.youtube_channel.clone()),
        default: Value::Text(String::new()),
        weight: Preference,
        group: "Social".into(),
    });
    items.push(Setting {
        key: "workday.social.bluesky_handle".into(),
        name: "Your Bluesky handle".into(),
        what: "Like name.bsky.social, for reading your own followers and posts.".into(),
        cost: "".into(),
        value: Value::Text(t.workday.social.bluesky_handle.clone()),
        default: Value::Text(String::new()),
        weight: Preference,
        group: "Social".into(),
    });
    // Your own SearXNG, for research to search with (28 Sep 2026). Read at
    // the moment research runs, so a change applies to the next one.
    items.push(Setting {
        key: "research.searxng_url".into(),
        name: "Own search engine".into(),
        what: "The address of a SearXNG you run, to search with instead of DuckDuckGo. Empty uses DuckDuckGo.".into(),
        cost: "SearXNG asks several search engines for you and tells none of them who asked. Its JSON results must be turned on.".into(),
        value: Value::Text(t.research.searxng_url.clone()),
        default: Value::Text(String::new()),
        weight: Sensitive,
        group: "Reaching outside this machine".into(),
    });
    // Opportunity hunting (29 Sep 2026). Off as shipped: it reaches public
    // sites, so turning it on is yours.
    items.push(toggle("hunt.enabled", "Opportunity hunting",
        "Once a day, look for gigs, jobs, grants, contracts and niches in the sources below, and bring the best few with why.",
        "Reads public listings (and job alerts already in your mail). Never applies, replies, contacts anyone or spends.",
        t.hunt.enabled, Sensitive, "Looking for opportunities"));
    let text = |key: &str, name: &str, what: &str, cost: &str, v: &str| Setting {
        key: key.into(),
        name: name.into(),
        what: what.into(),
        cost: cost.into(),
        value: Value::Text(v.to_string()),
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Text(v.to_string()),
        weight: Preference,
        group: "Looking for opportunities".into(),
    };
    items.push(text("hunt.sources", "Opportunity sources",
        "Comma-separated: hn, grants, sam, reddit, producthunt, appstore, github, mail, feeds, search.",
        "SAM.gov needs a free key in the vault under the name below.", &t.hunt.sources));
    items.push(text("hunt.subreddits", "Subreddits to read", "Without the r/, comma-separated.", "", &t.hunt.subreddits));
    items.push(text("hunt.feeds", "Opportunity feeds", "Any RSS or Atom addresses to read too, comma-separated.", "", &t.hunt.feeds));
    items.push(text("hunt.searches", "Opportunity searches", "Searches to run each day, comma-separated.",
        "Needs a SearXNG address set above.", &t.hunt.searches));
    items.push(text("hunt.keywords", "Grant keywords", "What Grants.gov and SAM.gov are asked for.", "", &t.hunt.keywords));
    items.push(text("hunt.sam_key_vault", "SAM.gov key name", "The vault entry holding your SAM.gov key.",
        "The key never leaves the vault except in the one request to SAM.gov.", &t.hunt.sam_key_vault));
    items.push(Setting {
        key: "hunt.top_n".into(),
        name: "Opportunities per brief".into(),
        what: "How many make the morning brief.".into(),
        cost: String::new(),
        value: Value::Number { value: t.hunt.top_n as f64, min: 1.0, max: 10.0 },
        default: Value::Number { value: t.hunt.top_n as f64, min: 1.0, max: 10.0 },
        weight: Preference,
        group: "Looking for opportunities".into(),
    });
    items.push(Setting {
        key: "hunt.max_requests_per_day".into(),
        name: "Daily request limit".into(),
        what: "All sources together. A source that would go past it waits for tomorrow.".into(),
        cost: format!("Never above {}, whatever this says.", crate::hunt::HARD_CEILING),
        value: Value::Number { value: t.hunt.max_requests_per_day as f64, min: 1.0, max: crate::hunt::HARD_CEILING as f64 },
        default: Value::Number { value: t.hunt.max_requests_per_day as f64, min: 1.0, max: crate::hunt::HARD_CEILING as f64 },
        weight: Preference,
        group: "Looking for opportunities".into(),
    });
    items.push(Setting {
        key: "household.device_name".into(),
        name: "This device's name".into(),
        what: "What to call this machine to your other devices.".into(),
        cost: "".into(),
        value: Value::Text(t.household.device_name.clone()),
        default: Value::Text(t.household.device_name.clone()),
        weight: Preference,
        group: "Your devices".into(),
    });

    items.push(Setting {
        key: "persona.tone".into(),
        name: "Tone".into(),
        what: "How it talks to you.".into(),
        cost: String::new(),
        value: Value::Choice {
            value: format!("{:?}", t.persona.tone).to_lowercase(),
            options: vec!["dry".into(), "plain".into(), "warm".into()],
        },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Choice {
            value: format!("{:?}", t.persona.tone).to_lowercase(),
            options: vec!["dry".into(), "plain".into(), "warm".into()],
        },
        weight: Weight::Preference,
        group: "How it talks back".into(),
    });

    // The voice, presented the way the configured engine actually works.
    //
    // This was always a fixed list of seven *piper* voices. Under an engine
    // that copies a voice from a recording, a voice is the filename of a clip
    // you provide — so the dropdown offered seven things that do not exist and
    // hid the only thing that does.
    let cloning = t.tts_engine.engine.can_clone();
    let kokoro = t.tts_engine.engine == crate::tts::Engine::Kokoro;

    // Which engine speaks (28 Sep 2026): piper, which ships, or Kokoro,
    // spoken inside Atlas once it's downloaded (`kokoro`). Chatterbox is
    // only offered when it's already what your config names: it needs a
    // program you write yourself.
    let mut engines = vec!["piper".to_string(), "kokoro".to_string()];
    let engine_now = format!("{:?}", t.tts_engine.engine).to_lowercase();
    if !engines.contains(&engine_now) {
        engines.push(engine_now.clone());
    }
    items.push(Setting {
        key: "tts_engine.engine".into(),
        name: "Voice engine".into(),
        what: "piper is small and plain. Kokoro sounds much more natural and still runs on the processor alone.".into(),
        cost: format!(
            "Kokoro is a {} MB download, from Sound & voice. Until it's there Atlas speaks in piper and says so.",
            crate::kokoro::download_mb()
        ),
        value: Value::Choice { value: engine_now.clone(), options: engines.clone() },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Choice { value: engine_now, options: engines },
        weight: Weight::Preference,
        group: "Talking to it".into(),
    });

    let (voice_value, voice_default) = if cloning {
        (
            Value::Text(t.voice_settings.voice.clone()),
            Value::Text(crate::tts::VoiceSettings::default().voice),
        )
    } else {
        // Kokoro's own English voices under Kokoro; piper's catalogue
        // otherwise. The piper list under Kokoro offered seven voices it
        // doesn't have, and refused every one it does.
        let options: Vec<String> = if kokoro {
            crate::kokoro::english_voices().into_iter().map(String::from).collect()
        } else {
            crate::tts::catalogue().into_iter().map(|v| v.id).collect()
        };
        (
            Value::Choice { value: t.voice_settings.voice.clone(), options: options.clone() },
            Value::Choice { value: crate::tts::VoiceSettings::default().voice, options },
        )
    };
    items.push(Setting {
        key: "voice_settings.voice".into(),
        name: "Speaking voice".into(),
        what: if cloning {
            "Whose voice Atlas speaks in. It copies the voice in a recording you provide."
                .into()
        } else {
            "Which voice Atlas speaks in when it reads a reply out loud.".to_string()
        },
        cost: if cloning {
            "A ten-to-twenty second wav in models/voices. Name it here without the extension."
                .into()
        } else {
            "Every voice is a free download. Or just say: use a British voice.".to_string()
        },
        value: voice_value,
        default: voice_default,
        weight: Weight::Preference,
        group: "Talking to it".into(),
    });

    // The keys (Eric, H1, and 26 Sep 2026: "it needs to be customizable. I
    // don't have an Alt button"). Set in the settings window by pressing the
    // key you want, or by name.
    items.push(toggle("push_to_talk.enabled", "Push-to-talk",
        "Hold a key anywhere in Windows and speak; let go and Atlas hears it. The wake word still works.",
        "A quick tap still reaches the app you're in.",
        t.push_to_talk.enabled, Preference, "Keys"));
    items.push(Setting {
        key: "push_to_talk.key".into(),
        name: "Push-to-talk key".into(),
        what: "The key you hold to talk: one key, such as Tab, Caps Lock, Right Ctrl or an F-key.".into(),
        cost: "Press \"Set by pressing\" and then the key, or type its name.".into(),
        value: Value::Text(t.push_to_talk.key.clone()),
        default: Value::Text(t.push_to_talk.key.clone()),
        weight: Preference,
        group: "Keys".into(),
    });
    items.push(Setting {
        key: "quick_input.hotkey".into(),
        name: "Typing box key".into(),
        what: "The key that opens a one-line box to type to Atlas from any app.".into(),
        cost: "Ctrl, Shift or Win with a key — no Alt needed — or on its own a key nobody types with (an F-key, Insert, Pause). Press \"Set by pressing\" and then the keys.".into(),
        value: Value::Text(t.quick_input.hotkey.clone()),
        default: Value::Text(t.quick_input.hotkey.clone()),
        weight: Preference,
        group: "Keys".into(),
    });

    items.push(Setting {
        key: "voice_settings.speed".into(),
        name: "Speaking pace".into(),
        what: "How quickly it talks.".into(),
        cost: "Below 1 is faster. Or just say: a bit slower.".into(),
        value: Value::Number { value: t.voice_settings.speed as f64, min: 0.6, max: 1.6 },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Number { value: t.voice_settings.speed as f64, min: 0.6, max: 1.6 },
        weight: Weight::Preference,
        group: "Talking to it".into(),
    });

    // Sound & voice (the design's page of that name) and the locked
    // interrupt rule. Read by `sound`, from `Daemon::say`, `reach_you` and
    // `Voice::speak`.
    items.push(Setting {
        key: "sound.speak_replies".into(),
        name: "Speak replies aloud".into(),
        what: "Always, only when you spoke to it (hands-free), or never.".into(),
        cost: "Never still shows every reply.".into(),
        value: Value::Choice { value: t.sound.speak_replies.clone(), options: crate::sound::SPEAK_REPLIES.iter().map(|s| s.to_string()).collect() },
        default: Value::Choice { value: "hands_free".into(), options: crate::sound::SPEAK_REPLIES.iter().map(|s| s.to_string()).collect() },
        weight: Weight::Preference,
        group: "Sound".into(),
    });
    items.push(Setting {
        key: "sound.volume".into(),
        name: "Speaking volume".into(),
        what: "How loud Atlas's voice is, from 0 to 100.".into(),
        cost: "Applied to the voice itself, so the rest of your sound is untouched.".into(),
        value: Value::Number { value: t.sound.volume as f64, min: 0.0, max: 100.0 },
        default: Value::Number { value: 100.0, min: 0.0, max: 100.0 },
        weight: Weight::Preference,
        group: "Sound".into(),
    });
    items.push(toggle("sound.muted", "Mute Atlas",
        "Nothing is said out loud. Everything still works and is shown.",
        "Replies and notes appear on screen instead.",
        t.sound.muted, Weight::Preference, "Sound"));
    items.push(toggle("sound.quiet_hours", "Quiet hours",
        "Atlas won't speak or chime in the window below. It still works, silently.",
        "Anything it wanted to say waits in your brief.",
        t.sound.quiet_hours, Weight::Preference, "Sound"));
    items.push(Setting {
        key: "sound.quiet_from".into(),
        name: "Quiet from".into(),
        what: "When quiet hours start, on your clock (22:00).".into(),
        cost: "".into(),
        value: Value::Text(t.sound.quiet_from.clone()),
        default: Value::Text("22:00".into()),
        weight: Weight::Preference,
        group: "Sound".into(),
    });
    items.push(Setting {
        key: "sound.quiet_to".into(),
        name: "Quiet until".into(),
        what: "When quiet hours end (07:00).".into(),
        cost: "".into(),
        value: Value::Text(t.sound.quiet_to.clone()),
        default: Value::Text("07:00".into()),
        weight: Weight::Preference,
        group: "Sound".into(),
    });
    items.push(Setting {
        key: "sound.popups".into(),
        name: "Pop-ups".into(),
        what: "Only when you ask, when it's urgent, or anything ready. Otherwise it waits in the hub and your brief.".into(),
        cost: "Urgent means time-sensitive, or something you told Atlas is worth interrupting for.".into(),
        value: Value::Choice { value: t.sound.popups.clone(), options: crate::sound::POPUPS.iter().map(|s| s.to_string()).collect() },
        default: Value::Choice { value: "urgent".into(), options: crate::sound::POPUPS.iter().map(|s| s.to_string()).collect() },
        weight: Weight::Preference,
        group: "Sound".into(),
    });
    let wake = t.wake.clone().unwrap_or(crate::voice::WakeConfig { enabled: false, phrase: "atlas".into(), clip_seconds: 3, detector: None, listen_first: false });
    items.push(Setting {
        key: "wake.phrase".into(),
        name: "Wake phrase".into(),
        what: "What you say to get its attention.".into(),
        cost: "Matched loosely: speech-to-text spells it differently every time.".into(),
        value: Value::Text(wake.phrase.clone()),
        default: Value::Text("atlas".into()),
        weight: Weight::Preference,
        group: "Talking to it".into(),
    });

    // A choice of three since 29 Sep 2026 (was a 0-to-1 number): off, dry,
    // or full -- "a smart-ass", Eric's word. `wit.rs` holds the fence.
    let wit_options: Vec<String> = crate::wit::LEVELS.iter().map(|s| s.to_string()).collect();
    items.push(Setting {
        key: "persona.wit".into(),
        name: "Wit".into(),
        what: "Off, dry (the odd aside in conversation), or full: a smart-ass, after the answer.".into(),
        cost: "Wording only, never the facts. Never when something went wrong or you're fed up, never about money, health, security or bad news, and never in anything written for someone else.".into(),
        value: Value::Choice { value: t.persona.wit.word().into(), options: wit_options.clone() },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Choice { value: t.persona.wit.word().into(), options: wit_options },
        weight: Weight::Preference,
        group: "How it talks back".into(),
    });

    items.push(toggle("gaze.enabled", "Watching the room",
        "Use the camera to tell whether you're there, whether you're looking, and read a thumbs up or down.",
        "The picture is deleted the moment it's read. Recognising your face never approves anything.",
        t.gaze.enabled, Weight::Sensitive, "What it can see"));

    items.push(toggle("vision.enabled", "Recognising things",
        "Name the things in front of the camera, tell faces apart, and learn anything you show it and name.",
        "Runs on this machine — no picture is sent anywhere. Needs a one-off download (the seeing models).",
        t.vision.enabled, Weight::Sensitive, "What it can see"));

    items.push(toggle("viewing.enabled", "Watching video",
        "Read what's on screen in a video you send, not just what's said in it.",
        "Pulls a frame only where the picture changed, reads it, deletes it.",
        t.viewing.enabled, Weight::Preference, "What it can see"));

    items.push(toggle("persona.argues", "Pushback",
        "Say when it disagrees, with the reason, instead of going along with you.",
        "", t.persona.argues, Weight::Preference, "How it talks back"));

    items.push(toggle("persona.converses", "Open conversation",
        "Hold a conversation that isn't about work.",
        "", t.persona.converses, Weight::Preference, "How it talks back"));

    items.push(Setting {
        key: "persona.max_spoken_sentences".into(),
        name: "Reply length".into(),
        what: "Maximum sentences in a spoken reply.".into(),
        cost: "Longer replies are harder to listen to than to read.".into(),
        value: Value::Number { value: t.persona.max_spoken_sentences as f64, min: 1.0, max: 8.0 },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Number { value: t.persona.max_spoken_sentences as f64, min: 1.0, max: 8.0 },
        weight: Weight::Preference,
        group: "How it talks back".into(),
    });

    items.push(Setting {
        key: "models.memory_budget_mb".into(),
        name: "Model memory".into(),
        what: "How much RAM Atlas may use for its reasoning model, in MB. 0 means Atlas measures \
               what this machine can spare."
            .into(),
        cost: "Too high and it fights Windows for memory and everything stutters.".into(),
        // 0 is allowed, and shipped: it is "measure it" (`fit.rs`). The range
        // used to start at 512, so the shipped value was outside its own
        // range — nothing could ever set it back to "measure", and a native
        // control clamping to the range would have saved 512 on first touch.
        value: Value::Number { value: t.models.memory_budget_mb as f64, min: 0.0, max: 64000.0 },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Number { value: t.models.memory_budget_mb as f64, min: 0.0, max: 64000.0 },
        weight: Weight::Resource,
        group: "What it may touch".into(),
    });

    // Speculative decoding (28 Sep 2026): off unless a draft file is named and
    // there, or a no-model kind is chosen. Flags on the model server.
    items.push(Setting {
        key: "models.draft".into(),
        name: "Helper model".into(),
        what: "A small model of the same family, in the models folder, that guesses a few words ahead for the main \
               one to check. Empty: none."
            .into(),
        cost: "About 640 MB more memory for Qwen3-0.6B. Faster only when its guesses are often right; not measured \
               on this laptop yet."
            .into(),
        value: Value::Text(t.models.draft.clone()),
        default: Value::Text(String::new()),
        weight: Weight::Resource,
        group: "What it may touch".into(),
    });
    items.push(Setting {
        key: "models.speculate".into(),
        name: "Guessing ahead".into(),
        what: "Lets the model reuse words already in the conversation to answer faster, with no second model.".into(),
        cost: "Almost no memory. Helps summaries and anything that repeats what it read; ordinary chat, little.".into(),
        value: Value::Choice {
            value: if t.models.speculate.trim().is_empty() { "off".into() } else { t.models.speculate.clone() },
            options: std::iter::once("off".to_string()).chain(crate::models::NGRAM_KINDS.iter().map(|k| k.to_string())).collect(),
        },
        default: Value::Choice {
            value: "off".into(),
            options: std::iter::once("off".to_string()).chain(crate::models::NGRAM_KINDS.iter().map(|k| k.to_string())).collect(),
        },
        weight: Weight::Resource,
        group: "What it may touch".into(),
    });

    // Which model talks with you (30 Sep 2026, `deepbrain`): switched live --
    // the talking model's server is started again on the other one.
    items.push(Setting {
        key: "models.talk".into(),
        name: "Better answers".into(),
        what: "Which model talks with you: faster (Qwen3-VL 4B) or better (Qwen3.5 4B, more natural replies). Pictures are read by the Qwen3-VL model either way."
            .into(),
        cost: "Better reads a prompt at about half the speed: 2 to 5 seconds a reply on an ordinary laptop where faster takes 1 to 3. The same memory. Better needs its 2.8 GB file (the Connections page fetches it)."
            .into(),
        value: Value::Choice {
            value: if crate::models::talks_better(&t.models) { "better".into() } else { "faster".into() },
            options: vec!["faster".into(), "better".into()],
        },
        default: Value::Choice { value: "faster".into(), options: vec!["faster".into(), "better".into()] },
        weight: Weight::Resource,
        group: "How it talks back".into(),
    });

    items.push(Setting {
        key: "crew.keep_free_mb".into(),
        name: "Memory kept free".into(),
        what: "Background work that thinks waits rather than start with less memory free than this.".into(),
        cost: "Higher keeps the laptop responsive, and background work waits more often.".into(),
        value: Value::Number { value: t.crew.keep_free_mb as f64, min: 0.0, max: 16384.0 },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Number { value: t.crew.keep_free_mb as f64, min: 0.0, max: 16384.0 },
        weight: Weight::Resource,
        group: "What it may touch".into(),
    });

    items.push(Setting {
        key: "crew.battery_floor_percent".into(),
        name: "Battery floor".into(),
        what: "On battery below this, heavy work Atlas chose itself waits for the charger.".into(),
        cost: "Anything you asked for still runs. Higher saves charge; lower gets chores done unplugged.".into(),
        value: Value::Number { value: t.crew.battery_floor_percent as f64, min: 0.0, max: 100.0 },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Number { value: t.crew.battery_floor_percent as f64, min: 0.0, max: 100.0 },
        weight: Weight::Resource,
        group: "What it may touch".into(),
    });

    items.push(Setting {
        key: "identity.trusted_devices".into(),
        name: "Trusted devices".into(),
        what: "Devices whose own unlock counts as proof it's you.".into(),
        cost: "Anyone holding an unlocked one is treated as you.".into(),
        value: Value::List(t.identity.trusted_devices.clone()),
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::List(t.identity.trusted_devices.clone()),
        weight: Weight::Permission,
        group: "Your accounts and secrets".into(),
    });

    // Picked from a list rather than typed: the person this is for clicks.
    // "Automatic" is the unset value: this computer's own clock (`tz::home`).
    // It was UTC until the 26 Sep merge, when the third chat's reading of the
    // machine's clock and this setting became one home zone.
    let zones: Vec<String> = ["Automatic".to_string(), "UTC".to_string()]
        .into_iter()
        .chain(crate::tz::names().into_iter().map(String::from))
        .collect();
    let zone_now = if t.time_zone.trim().is_empty() { "Automatic".to_string() } else { t.time_zone.trim().to_string() };
    items.push(Setting {
        key: "time_zone".into(),
        name: "Time zone".into(),
        what: "Your clock: what \"at 7\" means, and the time a repeating event keeps.".into(),
        cost: "Automatic follows this computer's clock, daylight saving included. Pick a zone only if the computer's is wrong or you want another.".into(),
        value: Value::Choice { value: zone_now.clone(), options: zones.clone() },
        // Placeholder; `registry` derives the real default. See `build`.
        default: Value::Choice { value: zone_now, options: zones },
        weight: Weight::Preference,
        group: "How it talks back".into(),
    });

    Settings { items }
}
