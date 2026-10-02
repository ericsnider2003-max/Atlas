//! What Atlas can do, in one place you can ask.
//!
//! There are now over a hundred modules, and "what can you do?" was becoming a
//! question only the source could answer. That's a bad sign — a system you
//! can't get an honest inventory of is one you stop trusting the edges of.
//!
//! So every capability is registered with what it needs, what state it's in,
//! and what it costs. The state is the useful part: **built but never run** is
//! a different thing from **working**, and pretending otherwise is how you
//! find out at the worst moment.

use crate::portable::{self, How, Needs, Platform};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Not written yet.
    Planned,
    /// Written and tested, but has never run on real hardware.
    Untested,
    /// Written, but something it needs isn't installed.
    Blocked,
    /// Works, but switched off.
    Off,
    /// Works and is on.
    Working,
}

impl State {
    pub fn plain(&self) -> &'static str {
        match self {
            State::Planned => "not built yet",
            State::Untested => "built, never run for real",
            State::Blocked => "waiting on something",
            State::Off => "switched off",
            State::Working => "working",
        }
    }
    /// Can you rely on it today?
    pub fn usable(&self) -> bool {
        *self == State::Working
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Area {
    Hearing,
    Speaking,
    Windows,
    Files,
    Web,
    Writing,
    Money,
    Email,
    Thinking,
    Itself,
    Seeing,
    /// Keeping things safe, and having them on every device you use.
    ///
    /// Added 19 Sep 2026. The vault, the recovery key, the folder two
    /// devices meet in and the page you open from your phone were all built,
    /// wired and tested, and none of them were in this list — so "what can
    /// you do?" answered without mentioning the part that holds the
    /// passwords. A catalogue with a hole that size is worse than none,
    /// because you stop checking it.
    Keeping,
    /// Your time: the calendar, what's coming up, times with other people.
    ///
    /// Added 20 Sep 2026 with the built-in calendar. A personal assistant that
    /// couldn't tell you what was on your day had a hole where the most
    /// ordinary question of all should be.
    Time,
}

impl Area {
    pub fn plain(&self) -> &'static str {
        match self {
            Area::Hearing => "hearing you",
            Area::Speaking => "talking",
            Area::Windows => "your windows",
            Area::Files => "your files",
            Area::Web => "the web",
            Area::Writing => "writing",
            Area::Money => "money",
            Area::Email => "email",
            Area::Thinking => "working things out",
            Area::Itself => "looking after itself",
            Area::Seeing => "looking at your screen and at the room",
            Area::Keeping => "keeping your things safe, and on every device",
            Area::Time => "your calendar and what's coming up",
        }
    }
}

// Serialize only. A `Capability` is written in the source and read out; there
// is no direction in which one arrives from outside, and `Deserialize` would
// mean owned copies of strings that are already in the binary.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Capability {
    pub id: &'static str,
    /// What it does, in the words you'd use asking for it.
    pub what: &'static str,
    pub area: Area,
    pub state: State,
    /// What it's waiting on, when it's waiting.
    pub needs: Option<&'static str>,
    /// Works with the network unplugged.
    pub offline: bool,
    /// Roughly when it arrived, so "what's new" means something.
    pub added: u32,
    /// What it asks of the machine.
    ///
    /// This is the whole of what decides where it runs. Nothing here says
    /// "works on Android" — `runs_on` reads these against `portable::how`, so
    /// a platform answer can never be typed in by hand and then be wrong.
    pub runs: &'static [crate::portable::Needs],
    /// The source it actually lives in.
    ///
    /// Usually one file named after the id. Written down so the catalogue can
    /// be checked against the tree rather than believed: `tests/catalogue.rs`
    /// fails if a name here matches no file, and counts the modules no
    /// capability claims.
    pub modules: &'static [&'static str],
}

/// Everything, in the order it was built.
///
/// `Planned` with `needs: "wiring in"` means the code exists, compiles and is
/// tested, and nothing in the running program can call it. That is not the
/// same as working, and this list saying otherwise was the single most
/// damaging piece of drift in the system — everything else being wrong costs
/// you a feature; this being wrong costs you the ability to tell.
///
/// `tests/capability_honesty.rs` binds this list to `tests/wiring.rs` so it
/// cannot drift again.
///
/// # The drift that ran the other way (19 Sep 2026)
///
/// It did drift again, in the direction nothing was looking. Both honesty
/// tests read **unwired ⇒ must be Planned**. Neither read the reverse, so
/// `Planned` became a state a capability could be *left* in after it was
/// wired, and 27 of the 29 entries marked `Planned` had real production
/// callers. Twenty-six of them also carried `needs: "wiring in"` -- the
/// phrase that means "nothing can reach it" -- next to eight, nine, fifteen
/// call sites.
///
/// So `atlas catalog`, `docs/CAPABILITIES.md` and the answer to "what can you
/// do?" were **under-reporting by 27**, which is the same defect as claiming
/// too much and is worse in one way: overclaiming gets found the first time
/// somebody tries it, and underclaiming means nobody tries it at all.
///
/// The 27 were re-stated from evidence -- cross-module call sites counted the
/// way `tests/wiring.rs` counts them, with `pub x: crate::foo::FooConfig`
/// lines excluded as the config camouflage that file already warns about,
/// plus each module's own `enabled` default and what its `runs` asks of the
/// machine:
///
/// * **Working** (13): `certainty`, `wanted`, `interrupt`, `recall`, `draft`,
///   `stance`, `ledger`, `triage`, `route`, `learned`, `knowhow`, `timebox`,
///   `chain`, `person`, `fit`. No switch, nothing but thinking or files
///   asked of the machine, and reached every turn from the daemon.
/// * **Off** (7): `system`, `research`, `prose`, `mail`, `unsub`, `selfwork`,
///   `strategy`. Built and reached, shipping with `enabled: false`.
///   `strategy` is Off because its only caller is `selfwork`, which is.
/// * **Untested** (2): `overlay`, `improve`. Reached, and neither has met a
///   real screen.
/// * **Blocked** (3): `endpoint` and `dictate` on audio, `ocr` on tesseract.
///   `ocr` already said `needs: "tesseract, which nothing fetches"` while
///   claiming `Planned` -- the entry contradicted itself in one line.
///
/// `recall` was narrowed rather than promoted: word search works and needs no
/// model, and until 22 Sep meaning search had no source of vectors at all —
/// `set_embedding` sat on `dead_methods.rs`'s list. `meaning.rs` is now the
/// encoder seam (the same external-program shape as `speaker.rs`), the daemon
/// embeds notes off the tick and the question at ask time, and the `what`
/// says both halves honestly: words always, meaning once a model is
/// installed.
///
/// `mesh` is the one that stays `Planned`, and it is named on
/// `CAPABILITY_UNWIRED` with a reason. (`plainchange` was the other until it
/// was wired on 21 Sep — the self-fix reply now leads with its behaviour view,
/// so it is `Untested`, not `Planned`.) That is the rule now:
/// `a_planned_capability_says_why_it_is_planned` fails the build for any
/// `Planned` entry that no list and no written reason accounts for.
pub fn all() -> Vec<Capability> {
    use Area::*;
    use State::*;
    vec![
        Capability { id: "wake", what: "hear a wake word and listen", area: Hearing, state: Blocked, needs: Some("whisper"), offline: true, added: 1, runs: &[Needs::WakeWord, Needs::Audio], modules: &["hearing", "voice", "utterance", "parakeet", "kws"] },
        Capability { id: "endpoint", what: "stop listening when you stop talking", area: Hearing, state: Blocked, needs: Some("ffmpeg, for audio in"), offline: true, added: 12, runs: &[Needs::Audio], modules: &["endpoint"] },
        Capability { id: "dictate", what: "type what you say into a window", area: Hearing, state: Blocked, needs: Some("whisper"), offline: true, added: 12, runs: &[Needs::Audio, Needs::ActInApps], modules: &["dictate"] },
        Capability { id: "accents", what: "notice when it's mishearing you and offer a better model", area: Hearing, state: Blocked, needs: Some("whisper"), offline: true, added: 14, runs: &[Needs::Audio], modules: &["language"] },
        Capability { id: "translate", what: "understand and translate other languages", area: Hearing, state: Blocked, needs: Some("the multilingual model"), offline: true, added: 14, runs: &[Needs::Audio], modules: &["language"] },

        Capability { id: "speak", what: "talk back, in a voice you choose by asking", area: Speaking, state: Blocked, needs: Some("piper"), offline: true, added: 2, runs: &[Needs::Audio], modules: &["tts", "speech", "spoken_form", "speaking", "speakthread", "voicepick"] },
        Capability { id: "persona", what: "have opinions and disagree with you", area: Speaking, state: Working, needs: None, offline: true, added: 3, runs: &[Needs::JustThinking], modules: &["persona"] },
        Capability { id: "certainty", what: "say it doesn't know rather than guessing", area: Speaking, state: Working, needs: None, offline: true, added: 11, runs: &[Needs::JustThinking], modules: &["certainty"] },
        Capability { id: "wanted", what: "work out whether you want solutions or to be heard", area: Speaking, state: Working, needs: None, offline: true, added: 18, runs: &[Needs::JustThinking], modules: &["wanted"] },
        Capability { id: "worklog", what: "keep where your time went — which app, for how long, your longest stretch of focus, and what you were in the middle of when you come back from a break — on this machine only", area: Time, state: Untested, needs: Some("Windows, for keyboard and mouse timing"), offline: true, added: 35, runs: &[Needs::ReadScreen, Needs::Background], modules: &["worklog", "awareness"] },
        Capability { id: "when", what: "read a time the way you'd say it — \"in 20 minutes\", \"next Tuesday afternoon\", \"the 14th at noon\", \"Oct 3 from 2 to 4pm\" — and ask when the words could mean two things", area: Time, state: Working, needs: None, offline: true, added: 35, runs: &[Needs::JustThinking], modules: &["when", "calendar"] },
        Capability { id: "interrupt", what: "stay quiet unless it actually matters", area: Speaking, state: Working, needs: None, offline: true, added: 20, runs: &[Needs::JustThinking], modules: &["interrupt"] },
        Capability { id: "cutin", what: "stop talking the moment you start — cut in by voice while it speaks, your voice told apart from its own coming back through the speakers — and hear nothing at all while paused", area: Hearing, state: Off, needs: None, offline: true, added: 39, runs: &[Needs::Audio], modules: &["micthread"] },
        Capability { id: "kokoro", what: "speak in a Kokoro voice made on this machine — 54 voices across several languages — with the next sentence made while this one plays", area: Speaking, state: Blocked, needs: Some("the Kokoro download, from the Sound & voice page"), offline: true, added: 39, runs: &[Needs::Audio], modules: &["kokoro"] },
        Capability { id: "background", what: "run in the background from the moment you sign in with nothing open — an icon by the clock to open it, open the hub, pause or quit — and start itself again after a crash or an update", area: Itself, state: Untested, needs: Some("your machine"), offline: true, added: 39, runs: &[Needs::Background], modules: &["notifyicon", "startup", "crash", "goodbye", "onlyone"] },

        Capability { id: "layout", what: "arrange your windows", area: Windows, state: Untested, needs: Some("your machine"), offline: true, added: 2, runs: &[Needs::Windows_], modules: &["layout"] },
        Capability { id: "apps", what: "open things", area: Windows, state: Untested, needs: Some("your machine"), offline: true, added: 2, runs: &[Needs::LaunchApps], modules: &["platform", "system"] },
        Capability { id: "panels", what: "put a panel on screen when you ask", area: Windows, state: Untested, needs: Some("your machine"), offline: true, added: 13, runs: &[Needs::Windows_], modules: &["panel", "window"] },
        Capability { id: "overlay", what: "type on your desktop with no window", area: Windows, state: Untested, needs: Some("your machine"), offline: true, added: 13, runs: &[Needs::Windows_], modules: &["overlay", "overlaywin"] },

        Capability { id: "index", what: "know where your files are", area: Files, state: Working, needs: None, offline: true, added: 4, runs: &[Needs::Files], modules: &["index", "chunker", "bm25"] },
        Capability { id: "recall", what: "find something you wrote — by a word that was in it, and by what it was about once an embedding model is installed", area: Files, state: Working, needs: None, offline: true, added: 16, runs: &[Needs::Files], modules: &["recall", "meaning", "stemmer", "bm25", "asking"] },
        Capability { id: "remember", what: "remember what you tell it and what it researches, learn a whole document at once, and answer 'what do you know about X' — from one indexed fact book that recalls associatively, strengthens with repetition, corrects on restatement, and stays fast and bounded as it grows; and 'get to know me' fills it in six questions", area: Files, state: Working, needs: None, offline: true, added: 30, runs: &[Needs::JustThinking], modules: &["facts", "freshness", "consolidate", "contents", "getknow"] },
        Capability { id: "system", what: "move files, set a wallpaper, tidy the desktop", area: Files, state: Off, needs: None, offline: true, added: 10, runs: &[Needs::Files], modules: &["system"] },
        Capability { id: "tune", what: "find what's slowing the machine down", area: Files, state: Off, needs: None, offline: true, added: 10, runs: &[Needs::Files, Needs::Background], modules: &["tune", "checks"] },
        Capability { id: "backup", what: "back itself up", area: Files, state: Working, needs: None, offline: true, added: 5, runs: &[Needs::Files], modules: &["store", "safety"] },
        // One way, by decision: personal never reaches a business space, and
        // a business reaching his own Atlas is not policed at all.
        Capability { id: "firewall", what: "keep your own work out of anything you share", area: Files, state: Untested, needs: None, offline: true, added: 26, runs: &[Needs::Files], modules: &["firewall"] },

        Capability { id: "research", what: "look something up, and save the write-up as a Word or PDF file with its sources as links", area: Web, state: Untested, needs: None, offline: false, added: 4, runs: &[Needs::JustThinking], modules: &["research", "readable", "report"] },
        Capability { id: "delegate_online", what: "hand heavy background work to an online worker and check what comes back", area: Web, state: Blocked, needs: Some("a Cloudflare account and token"), offline: false, added: 20, runs: &[Needs::Background, Needs::JustThinking], modules: &["online"] },
        Capability { id: "build_it", what: "write code from your description and check it against the compiler and tests before trusting it -- in Rust, Python, Go, JavaScript, TypeScript or C++, with the checkers Atlas downloads itself", area: Thinking, state: Untested, needs: Some("a model to draft with"), offline: true, added: 20, runs: &[Needs::Background, Needs::JustThinking], modules: &["build_it", "craft", "goal", "codetools"] },
        Capability { id: "design", what: "review a page's design against a house style — spacing, colour tokens, accessibility — and say what's off, honestly not claiming to judge whether it looks good; also gate a page it builds against the same rules", area: Thinking, state: Working, needs: None, offline: true, added: 30, runs: &[Needs::JustThinking], modules: &["taste"] },
        Capability { id: "animate", what: "draw a self-contained SVG animation from your description, check it renders and moves to the size and length you asked for, and iterate until it does", area: Thinking, state: Untested, needs: Some("a model to draft with"), offline: true, added: 30, runs: &[Needs::Background, Needs::JustThinking], modules: &["motion"] },
        Capability { id: "explain", what: "explain code in plain English — a recent build, a change waiting to be implemented, a file or a paste — at the depth you ask for, honest that it can't prove it's right", area: Thinking, state: Untested, needs: Some("a model to draft with"), offline: true, added: 30, runs: &[Needs::JustThinking], modules: &["explain"] },
        Capability { id: "workshop", what: "keep a per-project queue of proposed changes you review and implement when ready", area: Files, state: Working, needs: None, offline: true, added: 20, runs: &[Needs::Files, Needs::JustThinking], modules: &["workshop"] },
        Capability { id: "calendar", what: "keep your own calendar, and tell you what's on", area: Time, state: Working, needs: None, offline: true, added: 20, runs: &[Needs::Files, Needs::JustThinking], modules: &["calendar", "recur", "civil", "when", "keeping"] },
        Capability { id: "weather", what: "say the weather now or tomorrow, here or in a town you name, from Open-Meteo (free, no account)", area: Time, state: Untested, needs: None, offline: false, added: 42, runs: &[Needs::JustThinking], modules: &["weather"] },
        // 23 Sep 2026, the GitHub ports: an .ics invite from anyone, "the last
        // Friday of every month", a repeat that can be written to a file.
        Capability { id: "vformat", what: "read an .ics invite or calendar from anyone into yours, and write yours out as .ics", area: Time, state: Untested, needs: None, offline: true, added: 31, runs: &[Needs::Files], modules: &["vformat", "calendar"] },
        Capability { id: "clients", what: "keep your client list — bring it in from your contacts (.vcf), write it back out, and point out anyone on it twice without merging them", area: Email, state: Untested, needs: None, offline: true, added: 31, runs: &[Needs::Files], modules: &["clients", "linkage", "vformat"] },
        Capability { id: "urgency", what: "keep your tasks, and say which to do first and why", area: Time, state: Untested, needs: None, offline: true, added: 31, runs: &[Needs::Files], modules: &["shared_task", "urgency"] },
        Capability { id: "automation", what: "watch what you name — the disk over 90% for ten minutes, a server down — and say so when your rule comes true; reminders on weekdays or 'the last Friday of the month'", area: Itself, state: Untested, needs: None, offline: true, added: 31, runs: &[Needs::Background], modules: &["automation", "cronspec", "scheduler"] },
        Capability { id: "palette", what: "reach any page or action by typing, and ask 'did you mean' when a word is one typo off", area: Itself, state: Untested, needs: None, offline: true, added: 31, runs: &[Needs::JustThinking], modules: &["palette", "typos"] },
        Capability { id: "sealedlog", what: "keep a sealed record of what it did, and prove nobody has changed it since", area: Keeping, state: Untested, needs: None, offline: true, added: 31, runs: &[Needs::Files], modules: &["activity", "sealedlog"] },
        // 23 Sep 2026, late — the round-3 ports: time zones, a passphrase
        // judged by how guessable it is, what reaches an online model,
        // lookalike senders, shared pages, sealed files, and the rest.
        Capability { id: "tz", what: "keep time on your clock — \"at 7\" is 7 where you are, a weekly meeting stays at 9 through the clock change, and an invite from another time zone lands at the right hour", area: Time, state: Untested, needs: Some("your time zone chosen in Settings"), offline: true, added: 32, runs: &[Needs::JustThinking], modules: &["tz", "calendar", "vformat", "scheduler"] },
        Capability { id: "guessable", what: "tell a guessable passphrase from a strong one before the vault takes it, and say why", area: Keeping, state: Untested, needs: None, offline: true, added: 32, runs: &[Needs::JustThinking], modules: &["guessable", "vault"] },
        Capability { id: "redact", what: "keep passwords, keys, card and account numbers out of anything sent to an online model, and put them back into the answer here", area: Keeping, state: Untested, needs: Some("an online model set up"), offline: true, added: 32, runs: &[Needs::JustThinking], modules: &["redact", "brain"] },
        Capability { id: "lookalike", what: "warn about mail from an address made to look like someone you deal with, or that its own server marked as forged — and draft nothing to it", area: Email, state: Untested, needs: Some("a mail account"), offline: false, added: 32, runs: &[Needs::JustThinking], modules: &["lookalike", "imap"] },
        Capability { id: "diff", what: "show what changed between two versions of a file, line by line", area: Files, state: Untested, needs: None, offline: true, added: 32, runs: &[Needs::Files], modules: &["diff", "selfwork"] },
        Capability { id: "drain", what: "read its own log as the handful of things that happened, with counts", area: Itself, state: Untested, needs: None, offline: true, added: 32, runs: &[Needs::Files], modules: &["drain", "log"] },
        Capability { id: "bandit", what: "learn which of the things it offers you welcome, and still try the others now and then — and hold an offer for a natural break in your work (never longer than twenty minutes, and never over a presentation)", area: Itself, state: Untested, needs: None, offline: true, added: 32, runs: &[Needs::Background], modules: &["bandit", "proactive"] },
        Capability { id: "yata", what: "keep a page both of your machines can edit while apart, merged without a clash to settle", area: Keeping, state: Untested, needs: Some("a second device"), offline: true, added: 32, runs: &[Needs::Files], modules: &["yata", "sync"] },
        Capability { id: "vad", what: "hear that you've stopped talking even with a fan or air conditioner running", area: Hearing, state: Blocked, needs: Some("ffmpeg, for audio in"), offline: true, added: 32, runs: &[Needs::Audio], modules: &["vad", "endpoint", "voice"] },
        Capability { id: "diarize", what: "turn a recording into who-said-what notes, with your lines marked as yours", area: Hearing, state: Untested, needs: Some("speech-to-text installed for the words; voices are told apart by Atlas's own encoder"), offline: true, added: 32, runs: &[Needs::Files], modules: &["diarize", "speaker", "voiceid", "speakernet"] },
        Capability { id: "gmm", what: "tell your voice from others with no model to download — Atlas's own encoder, learned from the ordinary talk it hears", area: Hearing, state: Untested, needs: Some("30 clips of ordinary talk heard first (use it, or bring recordings in on the hub)"), offline: true, added: 33, runs: &[Needs::Audio], modules: &["speaker", "mfcc", "gmm"] },
        Capability { id: "wakeword", what: "wake on your own phrase, taught from three recordings, without speech-to-text running all day", area: Hearing, state: Untested, needs: Some("three takes of your phrase"), offline: true, added: 33, runs: &[Needs::WakeWord, Needs::Audio], modules: &["wakeword", "mfcc"] },
        Capability { id: "vadcal", what: "tune when it hears you stop talking to your own room, from a recording of the room and one of you", area: Hearing, state: Untested, needs: Some("two recordings on your microphone"), offline: true, added: 33, runs: &[Needs::Files], modules: &["vadcal", "vad"] },
        Capability { id: "hotkey", what: "hold a key in any window to talk; a quick tap still reaches the app", area: Hearing, state: Untested, needs: Some("on Linux, your user in the input group"), offline: true, added: 33, runs: &[Needs::Audio], modules: &["hotkey", "input"] },
        Capability { id: "inhibit", what: "keep the machine from sleeping while the night's work runs, and let it sleep the moment it ends", area: Itself, state: Untested, needs: None, offline: true, added: 33, runs: &[Needs::Background], modules: &["inhibit", "awake"] },
        Capability { id: "loginseal", what: "let scheduled mail checks open the vault while you're signed in to Windows — logins and API keys only", area: Keeping, state: Off, needs: Some("vault.open_on_this_login switched on; Windows"), offline: true, added: 33, runs: &[Needs::RealEncryption], modules: &["loginseal", "vault"] },
        Capability { id: "toast", what: "put alerts in the Windows Action Center so they're still there when you get back", area: Windows, state: Untested, needs: Some("Windows"), offline: true, added: 33, runs: &[Needs::Background], modules: &["toast", "notify"] },
        Capability { id: "spoken_numbers", what: "say money, times, dates and numbers the way a person would", area: Speaking, state: Untested, needs: None, offline: true, added: 33, runs: &[Needs::JustThinking], modules: &["spoken_numbers", "pronounce"] },
        Capability { id: "fixloop", what: "work a failing test with the model until it passes, in a copy of the folder, and show the tested change before anything lands", area: Thinking, state: Untested, needs: Some("a model set up"), offline: true, added: 33, runs: &[Needs::Files], modules: &["fixloop", "strategy", "consult", "handoff"] },
        Capability { id: "cutcheck", what: "find a video's cuts and say which ones make the viewer's eye jump across the frame", area: Files, state: Untested, needs: Some("ffmpeg"), offline: true, added: 33, runs: &[Needs::Files], modules: &["cutcheck", "editcraft"] },
        Capability { id: "filmstrip", what: "play an animation and save it as a GIF you can send anywhere — and an MP4 when ffmpeg is there — checked frame by frame that it really moves", area: Thinking, state: Untested, needs: Some("Edge or Chrome (Windows has Edge); ffmpeg for the MP4"), offline: true, added: 34, runs: &[Needs::Files], modules: &["filmstrip", "gifenc", "pngcodec", "motion"] },
        Capability { id: "scene3d", what: "draw a 3-D scene you describe — shapes or your own model files (OBJ, STL, glTF), glass, glowing lights and patterns; still, or moving (keyframes, spin, a travelling camera) as a GIF and an MP4 — with nothing installed, and in Blender too when it's there", area: Thinking, state: Untested, needs: Some("a model to draft the scene; Blender optional"), offline: true, added: 34, runs: &[Needs::Files], modules: &["scene3d", "meshio", "gifenc", "pngcodec"] },
        Capability { id: "agefile", what: "seal a file so only the people you name can open it — with their Atlas or the standard age tool", area: Keeping, state: Untested, needs: None, offline: true, added: 32, runs: &[Needs::Files, Needs::RealEncryption], modules: &["agefile"] },
        // Gaps the tree had written down itself (GAPS.md §B, dead_config).
        Capability { id: "pronounce", what: "say names, currency pairs, acronyms and symbols the way they sound — \"euro dollar\", \"V P S\" — with your own list on top", area: Speaking, state: Blocked, needs: Some("a speech engine installed"), offline: true, added: 32, runs: &[Needs::Audio], modules: &["pronounce", "voice"] },
        Capability { id: "zipread", what: "find what you're looking for inside .zip archives, and refuse one that would unpack to fill the disk", area: Files, state: Untested, needs: None, offline: true, added: 32, runs: &[Needs::Files], modules: &["zipread", "index", "files"] },
        Capability { id: "browser", what: "drive a browser", area: Web, state: Untested, needs: Some("your machine"), offline: false, added: 6, runs: &[Needs::ActInApps], modules: &["browser", "cdp"] },
        Capability { id: "publish", what: "post something, with approval", area: Web, state: Untested, needs: Some("a browser signed in where it posts"), offline: false, added: 6, runs: &[Needs::JustThinking], modules: &["publish", "publishing", "delivery"] },

        Capability { id: "draft", what: "write something and be honest about it", area: Writing, state: Working, needs: None, offline: true, added: 17, runs: &[Needs::JustThinking], modules: &["draft"] },
        Capability { id: "stance", what: "judge whether writing actually says anything", area: Writing, state: Working, needs: None, offline: true, added: 18, runs: &[Needs::JustThinking], modules: &["stance"] },
        Capability { id: "prose", what: "fix your typing as you go", area: Writing, state: Untested, needs: Some("a live run on the unlocked laptop"), offline: true, added: 20, runs: &[Needs::ActInApps], modules: &["prose"] },

        Capability { id: "ledger", what: "read your statements", area: Money, state: Working, needs: None, offline: true, added: 17, runs: &[Needs::Files], modules: &["ledger"] },
        Capability { id: "tax", what: "know the rules that catch traders out", area: Money, state: Working, needs: None, offline: true, added: 17, runs: &[Needs::JustThinking], modules: &["ledger", "money"] },
        // Arithmetic over candles, not a view on them. `market` answers
        // questions of fact about bars that have closed; `levels` says where
        // a stop and a target fall for a direction somebody else chose. What
        // neither does, and must not be listed as doing, is decide.
        Capability { id: "market", what: "read what the market is doing from the bars", area: Money, state: Untested, needs: Some("a file of bars"), offline: true, added: 26, runs: &[Needs::Files], modules: &["market", "structure", "regime", "multiframe"] },
        Capability { id: "levels", what: "say where a stop and a target go, and why", area: Money, state: Untested, needs: Some("a file of bars"), offline: true, added: 26, runs: &[Needs::Files], modules: &["levels"] },
        // Reading as it happens, and keeping score of itself. `live` is the
        // only one of these that needs a feed.
        Capability { id: "live", what: "read a market while it's still moving", area: Money, state: Untested, needs: Some("a feed"), offline: true, added: 26, runs: &[Needs::JustThinking], modules: &["live"] },

        // Online, and the only capability in this list that genuinely cannot
        // work unplugged -- the messages are on somebody else's server.
        // `Off` rather than `Working`: it ships disabled and needs a bot
        // token you make yourself.
        //
        // Telegram alone, deliberately. `Platform::what_it_permits` has said
        // since it was written that two of the six can never work for a
        // personal account, and building a reader for one must not imply the
        // others -- `what_you_asked_for` still says which are closed.
        Capability { id: "telegram", what: "read what's been sent to a chat bot you own, and sort it like the inbox", area: Email, state: Off, needs: None, offline: false, added: 29, runs: &[Needs::Files], modules: &["telegram", "messaging"] },
        Capability { id: "triage", what: "sort an inbox by what it asks of you", area: Email, state: Working, needs: None, offline: true, added: 19, runs: &[Needs::JustThinking], modules: &["triage"] },
        Capability { id: "mail", what: "reach your mailbox, whoever provides it", area: Email, state: Off, needs: None, offline: false, added: 20, runs: &[Needs::JustThinking], modules: &["mail", "imap", "smtp", "mailthread", "ratelimit", "himalaya", "msoauth"] },
        Capability { id: "unsub", what: "clear out what you never read, safely", area: Email, state: Off, needs: None, offline: false, added: 20, runs: &[Needs::JustThinking], modules: &["unsub"] },

        Capability { id: "route", what: "find another way when one is closed", area: Thinking, state: Working, needs: None, offline: true, added: 18, runs: &[Needs::JustThinking], modules: &["route"] },
        Capability { id: "strategy", what: "try twelve different angles on a hard problem", area: Thinking, state: Off, needs: None, offline: true, added: 9, runs: &[Needs::JustThinking], modules: &["strategy"] },
        Capability { id: "learned", what: "remember what didn't work", area: Thinking, state: Working, needs: None, offline: true, added: 17, runs: &[Needs::JustThinking], modules: &["learned"] },
        Capability { id: "knowhow", what: "know how to do things without asking anyone", area: Thinking, state: Working, needs: None, offline: true, added: 19, runs: &[Needs::JustThinking], modules: &["knowhow"] },
        Capability { id: "timebox", what: "stop before you have to ask what's taking so long", area: Thinking, state: Working, needs: None, offline: true, added: 17, runs: &[Needs::JustThinking], modules: &["timebox"] },
        Capability { id: "chain", what: "do something that crosses several apps", area: Thinking, state: Untested, needs: Some("a live run on the unlocked laptop"), offline: true, added: 19, runs: &[Needs::ActInApps], modules: &["chain"] },
        Capability { id: "person", what: "learn how you work", area: Thinking, state: Working, needs: None, offline: true, added: 18, runs: &[Needs::JustThinking], modules: &["person"] },
        Capability { id: "reason", what: "reason properly rather than following rules", area: Thinking, state: Blocked, needs: Some("a language model"), offline: true, added: 8, runs: &[Needs::JustThinking], modules: &["brain", "infer", "models", "deepbrain", "freeonline"] },

        Capability { id: "selfwork", what: "change its own code and test it", area: Itself, state: Off, needs: None, offline: true, added: 16, runs: &[Needs::Files], modules: &["selfwork", "pipeline", "sandbox", "mend", "selfgrant"] },
        Capability { id: "plainchange", what: "explain a change it staged as behaviour, not code — what will now happen and what it no longer promises, read from the tests it adds and drops", area: Itself, state: Untested, needs: None, offline: true, added: 17, runs: &[Needs::JustThinking], modules: &["plainchange"] },
        Capability { id: "fit", what: "fit itself to whatever machine it's on", area: Itself, state: Working, needs: None, offline: true, added: 19, runs: &[Needs::JustThinking], modules: &["fit", "adapt"] },
        Capability { id: "improve", what: "get better at things it does often", area: Itself, state: Untested, needs: Some("your machine"), offline: true, added: 19, runs: &[Needs::JustThinking], modules: &["improve"] },
        Capability { id: "digest", what: "fingerprint a file or a download so a changed byte is noticed", area: Itself, state: Untested, needs: None, offline: true, added: 27, runs: &[Needs::JustThinking], modules: &["digest"] },
        Capability { id: "claims", what: "check a claim about the market against the bars", area: Money, state: Untested, needs: Some("a file of bars"), offline: true, added: 27, runs: &[Needs::Files], modules: &["claims"] },
        Capability { id: "timeframe", what: "know what a bar is worth in hours rather than counting bars", area: Money, state: Untested, needs: None, offline: true, added: 28, runs: &[Needs::JustThinking], modules: &["timeframe"] },
        Capability { id: "events", what: "know when the central banks speak, out to 2027", area: Money, state: Untested, needs: None, offline: true, added: 28, runs: &[Needs::JustThinking], modules: &["events"] },
        Capability { id: "session", what: "know which desks are open, daylight saving included", area: Money, state: Untested, needs: None, offline: true, added: 28, runs: &[Needs::JustThinking], modules: &["session"] },
        Capability { id: "fxday", what: "know where the trading day ends, and read yesterday's levels off it", area: Money, state: Untested, needs: Some("bars with timestamps"), offline: true, added: 28, runs: &[Needs::Files], modules: &["fxday"] },
        Capability { id: "stale", what: "notice a trade that isn't working and just hasn't lost yet", area: Money, state: Untested, needs: Some("an open trade and the bars since"), offline: true, added: 28, runs: &[Needs::JustThinking], modules: &["stale"] },
        Capability { id: "refusals", what: "say why it isn't trading, and whether that's care or a setting", area: Money, state: Untested, needs: None, offline: true, added: 28, runs: &[Needs::JustThinking], modules: &["refusals"] },
        Capability { id: "rollover", what: "know that Wednesday night costs three, and that the 17:00 turn is the worst minute to be filled in", area: Money, state: Untested, needs: None, offline: true, added: 28, runs: &[Needs::JustThinking], modules: &["rollover"] },
        Capability { id: "asia", what: "read the overnight range, and say whether it was tight for this market rather than tight in pips", area: Money, state: Untested, needs: Some("bars with timestamps"), offline: true, added: 28, runs: &[Needs::Files], modules: &["asia"] },
        Capability { id: "standdown", what: "have no view at all when a release is in front of it, or inside the bar it's reading", area: Money, state: Untested, needs: Some("bars with timestamps"), offline: true, added: 28, runs: &[Needs::JustThinking], modules: &["standdown"] },
        Capability { id: "feed", what: "refuse bars that would make every number wrong", area: Money, state: Untested, needs: Some("a file of bars"), offline: true, added: 28, runs: &[Needs::Files], modules: &["feed"] },
        Capability { id: "together", what: "notice when several trades are really one bet", area: Money, state: Untested, needs: Some("open positions"), offline: true, added: 27, runs: &[Needs::JustThinking], modules: &["together"] },
        Capability { id: "untrusted", what: "read what it's handed without being told what to do", area: Itself, state: Untested, needs: None, offline: true, added: 26, runs: &[Needs::JustThinking], modules: &["untrusted"] },
        Capability { id: "doctor", what: "check itself and say what's wrong", area: Itself, state: Working, needs: None, offline: true, added: 3, runs: &[Needs::JustThinking], modules: &["doctor", "diagnose"] },

        Capability { id: "ocr", what: "read text off an image with an outside program", area: Seeing, state: Blocked, needs: Some("tesseract, which nothing fetches"), offline: true, added: 7, runs: &[Needs::ReadScreen], modules: &["ocr"] },
        Capability { id: "words", what: "read the words on your screen", area: Seeing, state: Untested, needs: Some("the two reading models"), offline: true, added: 27, runs: &[Needs::ReadScreen], modules: &["words"] },
        Capability { id: "watch", what: "notice what window you're in", area: Seeing, state: Untested, needs: Some("your machine"), offline: true, added: 5, runs: &[Needs::ReadScreen], modules: &["watch", "watching"] },
        // Was listed as `Planned, needs: "a GPU"`. Both halves were wrong once
        // it existed: the models run on the processor, and what they are
        // actually waiting on is a download. `Untested` rather than `Off`
        // because nothing here has yet run on Eric's machine, and saying
        // otherwise is the one kind of drift that costs you the ability to
        // tell.
        Capability { id: "vision", what: "name what's in front of the camera, and tell faces apart", area: Seeing, state: Untested, needs: Some("the seeing models"), offline: true, added: 26, runs: &[Needs::Camera], modules: &["vision", "frames", "camera_ask"] },
        Capability { id: "callnotes", what: "notice a call, note your side, record the others only after they say yes, and write up who said what", area: Hearing, state: Untested, needs: Some("a call on this laptop"), offline: true, added: 31, runs: &[Needs::Audio], modules: &["callnotes", "callrec", "callwatch", "consent"] },
        Capability { id: "picture_talk", what: "say what a chart, your screen or a photo shows, with a model on this laptop", area: Seeing, state: Untested, needs: Some("the picture reader, which setup fetches"), offline: true, added: 31, runs: &[Needs::Files], modules: &["picture_talk"] },
        Capability { id: "vault", what: "keep a password, and hand it back when you ask", area: Keeping, state: Working, needs: None, offline: true, added: 29, runs: &[Needs::Files, Needs::RealEncryption], modules: &["vault", "credentials"] },
        Capability { id: "recovery", what: "get you back in when you've lost the way in", area: Keeping, state: Working, needs: None, offline: true, added: 29, runs: &[Needs::Files], modules: &["recovery", "codes"] },
        Capability { id: "sync", what: "carry what you've done to your other devices", area: Keeping, state: Untested, needs: Some("a second device"), offline: true, added: 29, runs: &[Needs::Files], modules: &["sync", "cloudsync", "courier", "hlc", "transport"] },
        Capability { id: "household", what: "let the people you live with use it, without seeing your things", area: Keeping, state: Untested, needs: Some("a second device"), offline: true, added: 29, runs: &[Needs::Files], modules: &["household", "crew", "roster", "profiles"] },
        Capability { id: "hub", what: "a page you can open from your phone, on your own network", area: Keeping, state: Untested, needs: Some("your machine"), offline: true, added: 29, runs: &[Needs::Background, Needs::Files], modules: &["hub", "hublive", "hubjobs", "server", "hubpages", "hubvault"] },
        Capability { id: "goingaway", what: "get your accounts ready before you go somewhere your texts won't arrive", area: Keeping, state: Off, needs: None, offline: true, added: 29, runs: &[Needs::Files], modules: &["goingaway", "codes", "accounts"] },
        Capability { id: "signin", what: "sign you into a site you've given it, and never into a lookalike", area: Keeping, state: Untested, needs: Some("a live run against your real accounts"), offline: false, added: 29, runs: &[Needs::Files, Needs::RealEncryption], modules: &["signin"] },
        Capability { id: "confirmed", what: "say a security change back to you before it happens", area: Keeping, state: Untested, needs: Some("a live run against your real accounts"), offline: true, added: 29, runs: &[Needs::JustThinking], modules: &["confirmed", "walkthrough"] },
        Capability { id: "afterme", what: "make sure the people who'd need the envelope know where it is", area: Keeping, state: Untested, needs: None, offline: true, added: 29, runs: &[Needs::Files], modules: &["afterme"] },
        Capability { id: "reclaim", what: "find space on your disk without being why you lost something", area: Keeping, state: Untested, needs: Some("your machine"), offline: true, added: 29, runs: &[Needs::Files], modules: &["reclaim", "retention"] },
        // `Planned`, and the word is doing work. `atlas mesh` exists and says
        // true things -- which kind you chose, what it costs, what you would
        // do yourself -- but the capability named here is reaching the other
        // device, and `mesh::choose` picks between four routes of which only
        // the cloud folder is built. Calling this `Off` or `Untested` would
        // make the catalogue agree with the command's tone instead of with
        // the tree. See `CAPABILITY_UNWIRED` in `tests/capability_wiring.rs`.
        Capability { id: "mesh", what: "reach your laptop directly from another network, not by leaving it a note", area: Keeping, state: Planned, needs: Some("a private network, and a transport this build doesn't have"), offline: false, added: 29, runs: &[Needs::Background, Needs::Files], modules: &["mesh"] },

        Capability { id: "handloop", what: "follow your hand and move things with it", area: Seeing, state: Untested, needs: Some("the hand models"), offline: true, added: 26, runs: &[Needs::Camera], modules: &["handloop", "handtrack", "handshape", "frames"] },
        Capability { id: "delegate", what: "draft a reply in the window in front, or carry the conversation on while you're away — typing only in gaps in yours, and never sending a reply that didn't land as written", area: Windows, state: Untested, needs: Some("a live run on the unlocked laptop"), offline: true, added: 32, runs: &[Needs::ActInApps, Needs::Background], modules: &["delegate", "idle"] },
        Capability { id: "resume", what: "pick back up what a restart cut off — windows it was working, research and council redone once, project work carried on from its last finished phase — and say what it didn't redo", area: Itself, state: Working, needs: None, offline: true, added: 32, runs: &[Needs::Background], modules: &["resume", "phases"] },
        Capability { id: "twofactor", what: "type a two-factor code you read out or that's in your email or texts, sign you in and make accounts in its own browser, and turn two-factor on or off after reading it back", area: Keeping, state: Untested, needs: Some("a live run against your real accounts"), offline: false, added: 33, runs: &[Needs::ActInApps, Needs::RealEncryption], modules: &["twofactor", "webrun"] },
        Capability { id: "astype", what: "fix a typo in place as you type in your other apps, leave a fix you changed back alone in that text, and learn from what you keep and what you change", area: Windows, state: Untested, needs: Some("a live run on the unlocked laptop"), offline: true, added: 33, runs: &[Needs::ActInApps, Needs::Background], modules: &["astype"] },
        // Eric's H rulings of 25 Sep 2026.
        Capability { id: "pdftext", what: "read a PDF, a Word file or a scanned page, and unzip, with a virus scan before anything is opened", area: Files, state: Untested, needs: Some("your machine"), offline: true, added: 34, runs: &[Needs::Files], modules: &["pdftext", "unpack"] },
        Capability { id: "hotkeys", what: "hold a key anywhere in Windows to talk, or press one to type, alongside the wake word", area: Hearing, state: Untested, needs: Some("your machine"), offline: true, added: 34, runs: &[Needs::Audio], modules: &["hotkeys", "typebox", "quickinput"] },
        Capability { id: "next_up", what: "say what needs you first when you come back, rather than what happened first", area: Itself, state: Working, needs: None, offline: true, added: 32, runs: &[Needs::JustThinking], modules: &["next_up"] },
        Capability { id: "which_errand", what: "pause one errand without losing what it has done, and work out which one you meant when several are going", area: Itself, state: Working, needs: None, offline: true, added: 30, runs: &[Needs::Background, Needs::JustThinking], modules: &["which_errand", "attention"] },
        Capability { id: "wireguard", what: "reach your own server over WireGuard, fenced so your devices reach its model server and nothing else", area: Keeping, state: Untested, needs: Some("your own server"), offline: false, added: 30, runs: &[Needs::Background, Needs::Files], modules: &["wireguard"] },
        Capability { id: "firstlaunch", what: "set itself up from one double-click: its own home, its shortcuts, its voice pieces fetched and checked, and its own window to say how it went", area: Itself, state: Untested, needs: None, offline: false, added: 30, runs: &[Needs::Files, Needs::Background], modules: &["firstlaunch", "getpieces", "setupwin", "settingswin", "hubwin", "webview2_loader", "localclock", "doorrule", "firstrun", "install"] },
        Capability { id: "groups", what: "keep group chats you own -- you decide who's in each and who may post, and every member's Atlas checks your signature on it", area: Keeping, state: Untested, needs: Some("a second device"), offline: true, added: 31, runs: &[Needs::Files], modules: &["groups", "peerkey"] },
        Capability { id: "texting", what: "write a text to someone whose number you've given, ready on your phone to send with one tap -- Atlas can't send or read texts itself", area: Email, state: Untested, needs: Some("your phone"), offline: true, added: 43, runs: &[Needs::Files], modules: &["texting"] },
        Capability { id: "friends", what: "add a friend with one link they open -- no codes sent back and forth, nobody waiting to confirm -- or with a request through a group you share", area: Keeping, state: Untested, needs: None, offline: false, added: 31, runs: &[Needs::Files], modules: &["friends"] },
        Capability { id: "wire", what: "reach a friend's Atlas from anywhere with nothing in the middle -- through Tor, fetched by setup and started by Atlas itself, everything sealed with the keys you swapped; switching to Tor's own bridges when a network blocks it; each friend's connection kept open between messages; straight across when you're on the same wifi; and when their Atlas is off, yours keeps the message and sends it when theirs is back", area: Keeping, state: Untested, needs: Some("two Atlases on two real networks: proven so far on Tor's own test network and against real tor"), offline: false, added: 31, runs: &[Needs::Background], modules: &["wire", "onion", "kin"] },
        Capability { id: "update_courier", what: "hear about new versions in your release channel, check them against your release key, fetch the file in checked pieces from whoever announced it or any friend who already has it, install it (by itself at a quiet moment on your own devices, asking on friends'), go back a version when you say so, let friends tell you when something's wrong and hear your answer -- all of it by voice too -- and keep your own edits through every update", area: Itself, state: Untested, needs: Some("the release key, made with your Apple and Windows signing setup"), offline: true, added: 31, runs: &[Needs::Files], modules: &["update_courier", "update_apply", "feedback", "release", "upgrade", "yourchanges"] },
        Capability { id: "plugins", what: "take add-ons you or a friend made -- new things to say that put together what it already does -- and let each do only what you approved, checked at every step", area: Itself, state: Untested, needs: None, offline: true, added: 31, runs: &[Needs::Files], modules: &["plugins"] },
        // Round 11 (25 Sep): the seventeen everyday tools, joined by `workday`.
        Capability { id: "cliphist", what: "keep what you copied today, if you turn it on -- in memory only, never a copy a password manager marks private or that looks like a key -- and put any of it back", area: Windows, state: Off, needs: Some("turning on (\"turn on clipboard history\"); Windows says when the clipboard changed"), offline: true, added: 36, runs: &[Needs::Background], modules: &["cliphist", "workday"] },
        Capability { id: "screentext", what: "copy the text off the window in front, read on this machine by Windows' own recognizer, and warn you if it holds something secret", area: Seeing, state: Untested, needs: Some("Windows, with a language pack's text recognition"), offline: true, added: 36, runs: &[Needs::ReadScreen], modules: &["screentext"] },
        Capability { id: "marketdays", what: "know the US market's calendar -- holidays, early closes, CPI days and the big releases -- as a schedule, never a lean", area: Money, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::JustThinking], modules: &["marketdays"] },
        Capability { id: "waitingfor", what: "keep who owes you a reply and what you promised, read from the mail you sent, and learn from what you say was never one", area: Email, state: Untested, needs: Some("a mail account set up"), offline: true, added: 36, runs: &[Needs::Files], modules: &["waitingfor", "mailbook"] },
        Capability { id: "capture", what: "catch a thought in one step -- said, or a key chord on whatever's selected -- dated when it said a time so it comes up that day, and gone back over once a week", area: Thinking, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["capture"] },
        Capability { id: "launcher", what: "open any app or Start-menu shortcut by part of its name, learning which you mean", area: Windows, state: Untested, needs: Some("Windows' Start menu"), offline: true, added: 36, runs: &[Needs::LaunchApps], modules: &["launcher"] },
        Capability { id: "tradeday", what: "ask a short check-in before the open and a journal after the close, about you and your process -- never a trade -- and count it back", area: Money, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["tradeday"] },
        Capability { id: "meetprep", what: "a quarter of an hour before a meeting, who's in it, what you last wrote each other, and what's still open", area: Time, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["meetprep"] },
        Capability { id: "snippets", what: "keep text you type often and type it for you, by voice or a key chord, without ever watching your typing", area: Writing, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::ActInApps], modules: &["snippets", "chords"] },
        Capability { id: "findfile", what: "find a file by part of its name, its kind and roughly when -- the closest names offered when nothing matches, never opened unasked", area: Files, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["findfile"] },
        Capability { id: "pdfkit", what: "merge, split and sign PDFs, read and written here, a new file beside the old -- never over it", area: Files, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["pdfkit"] },
        Capability { id: "people", what: "remember what you'd want to about the people you deal with, who you meant to keep in touch with, and their birthdays", area: Thinking, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["people"] },
        Capability { id: "feeds", what: "follow sites by their feeds, list what's new, read one here or keep it for later, trackers taken off every link", area: Web, state: Untested, needs: None, offline: false, added: 36, runs: &[Needs::Background], modules: &["feeds"] },
        Capability { id: "receipts", what: "keep a receipt off the screen or the clipboard -- merchant, date, total, asked about when unsure -- and say what you spent where", area: Money, state: Untested, needs: Some("Windows' text recognition, for one on screen"), offline: true, added: 36, runs: &[Needs::ReadScreen], modules: &["receipts"] },
        Capability { id: "habits", what: "count habits by strength rather than streaks, with pauses, and never bring up a body number", area: Time, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["habits"] },
        Capability { id: "srs", what: "flashcards that come back just before you'd forget them (FSRS)", area: Thinking, state: Untested, needs: None, offline: true, added: 36, runs: &[Needs::Files], modules: &["srs"] },
        Capability { id: "translation", what: "translate written text on this machine with your own model, checking that every number, link and address came through", area: Writing, state: Untested, needs: Some("a local model"), offline: true, added: 36, runs: &[Needs::JustThinking], modules: &["translation"] },
        // 28 Sep 2026: Atlas as a Model Context Protocol client. Tested
        // against a stand-in server; no real one (Filesystem, Playwright,
        // Terminator) has run on Eric's laptop, hence `Untested`.
        Capability { id: "mcp", what: "use tools from other programs you connect -- your folders, a separate browser, Windows apps -- found by what you ask, and asked about before each use; and `atlas mcp` lets your other AI tools see what Atlas is doing and ask it things, never approve", area: Thinking, state: Untested, needs: Some("a program that offers tools, installed and turned on in the settings"), offline: true, added: 38, runs: &[Needs::Background], modules: &["mcp", "mcpserve"] },
        Capability { id: "phonemodel", what: "think on the phone itself: a small language model inside the phone app (the right size for the phone's memory), fetched when you ask, with nothing you say leaving the phone", area: Thinking, state: Untested, needs: Some("a real phone, to measure its speed and battery"), offline: true, added: 37, runs: &[Needs::Files], modules: &["phonemodel"] },
        Capability { id: "phonelink", what: "put Atlas on your phone with a code to scan, as an app on your home screen", area: Keeping, state: Untested, needs: Some("Tailscale on the laptop and the phone"), offline: false, added: 30, runs: &[Needs::Background], modules: &["phonelink", "phoneadd", "ota"] },
        // 28 Sep 2026, the whole tree accounted for. Eric asked for the full
        // list of what Atlas can do, and 159 of 410 modules were claimed by
        // nothing -- among them the video editor, the morning brief, the
        // overnight run, the council, undo, the phone pieces and the hub's own
        // settings. Every unclaimed module was read and either claimed below
        // (or added to an existing entry's `modules`) or named in `PLUMBING`
        // with its reason; `tests/catalogue.rs` now fails for a module that is
        // neither. States were read from the running program: `Working` only
        // where the daemon reaches it with nothing but thinking or files
        // asked of the machine; `Off` where the shipped `tools.yaml` switch
        // that gates it is off (not merely where a struct default says
        // `false` and nothing reads it); `Untested` where it reaches out to
        // the machine, an account or another device and has not met one.
        Capability { id: "capability", what: "say what it can do, what's waiting on what, and what would work on another device -- counted by area, or item by item when you ask", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["capability", "portable"] },
        Capability { id: "register", what: "read the room -- a work question and a chat about a film answered in different voices, and no formal notice dropped into a call with friends", area: Speaking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["register"] },
        Capability { id: "thread", what: "carry one conversation on for good -- no session to start, the older part folded into a summary that keeps what mattered", area: Speaking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["thread"] },
        Capability { id: "understood", what: "check with you before acting on a guess at what you meant, and ask which one when it could be two things", area: Speaking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["understood", "whichone"] },
        Capability { id: "answering", what: "take an answer to its questions however you can give it -- a word, a typed yes, a key -- when speaking isn't an option", area: Speaking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["answering"] },
        Capability { id: "audio", what: "pick the right microphone and speakers, and keep a Bluetooth headset sounding right by not opening its microphone for nothing", area: Hearing, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Audio], modules: &["audio", "playout", "leveller", "miclevel"] },
        Capability { id: "addressing", what: "tell whether what it heard was meant for it -- a \"stop\" to it stops it, a voice on your call doesn't", area: Hearing, state: Blocked, needs: Some("whisper"), offline: true, added: 40, runs: &[Needs::Audio], modules: &["addressing"] },
        Capability { id: "references", what: "work out what \"it\", \"that\" and \"this one\" mean from what just happened -- \"move it to the other screen\"", area: Windows, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["references"] },
        Capability { id: "clipboard", what: "explain or answer about whatever you copied -- and when nothing is copied, work out what \"this\" is from what you were looking at", area: Windows, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::ReadScreen], modules: &["clipboard", "subject"] },
        Capability { id: "uia", what: "read an open window's controls and text without a screenshot -- even one that isn't in front -- and press a button you name", area: Seeing, state: Untested, needs: Some("Windows"), offline: true, added: 40, runs: &[Needs::ReadScreen, Needs::ActInApps], modules: &["uia"] },
        Capability { id: "probe", what: "go and look at another window to answer you, then put your focus back where it was", area: Windows, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Windows_, Needs::ReadScreen], modules: &["probe"] },
        Capability { id: "workspace", what: "open your workspace or a named mode -- trading, writing, a call -- each app on its screen, and close it all again", area: Windows, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Windows_, Needs::LaunchApps], modules: &["workspace", "modes"] },
        Capability { id: "rehearse", what: "show you what it would do -- every window, file and message -- without doing any of it", area: Windows, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["rehearse"] },
        Capability { id: "undo", what: "tell you what it did, across files, settings, mail and posts, and take it back where it can", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["undo"] },
        Capability { id: "mind", what: "tell you what it's working on right now, show how it got to an answer, and say why it did what it did", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["mind", "why"] },
        Capability { id: "trace", what: "keep each turn's timing and a record of every model call -- what was asked, how long it took -- and say which part was slow", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["trace", "timing"] },
        Capability { id: "revise", what: "turn a correction you give it into a change to what it keeps, so you don't have to make it twice", area: Thinking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["revise"] },
        Capability { id: "hollow", what: "notice when its own answer says nothing -- a zero it never measured, a sentence about nothing -- and say so", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["hollow"] },
        Capability { id: "hollowcode", what: "read code for the hollow kind -- compiles, passes, does nothing -- and for dependencies that don't exist", area: Thinking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["hollowcode"] },
        Capability { id: "selfaudit", what: "look at its own record and say what it should fix about itself, and what it's missing on this machine", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["selfaudit", "signals", "wants", "used"] },
        Capability { id: "integrations", what: "keep working with the network unplugged, know which of its connections are working right now, and say which one broke", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["integrations", "connectivity"] },
        Capability { id: "health", what: "watch the machine -- a filling disk, memory running short, a backup that stopped, a battery going", area: Itself, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Background], modules: &["health"] },
        Capability { id: "lanes", what: "be busy without making you wait -- work that needs your windows waits for a gap, and a long job says how it's going", area: Itself, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Background], modules: &["lanes", "channel"] },
        Capability { id: "earned", what: "earn the right to act on its own, one kind of work at a time, business and personal kept apart", area: Thinking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["earned"] },
        Capability { id: "policy", what: "ask before anything that changes your things or leaves this machine, and before using an app it doesn't know -- and remember when you say it may", area: Keeping, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["policy", "categories", "grants"] },
        Capability { id: "brief", what: "give you a morning brief -- what came in overnight against your day, drafts ready, and the short list only you can decide", area: Time, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["brief"] },
        Capability { id: "daily", what: "run the day as a day -- today's list closes at midnight and you choose what carries over", area: Time, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["daily"] },
        Capability { id: "returning", what: "when you come back, say what happened while you were away -- the part that matters, not all of it", area: Time, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["returning"] },
        Capability { id: "workspace_view", what: "show what's outstanding, grouped the way you work, and look back at any day", area: Time, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["workspace_view"] },
        Capability { id: "later", what: "keep a list for later -- anything it just said -- read back when you ask and raised once a week", area: Time, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["later"] },
        Capability { id: "backlog", what: "keep what it couldn't do and why, and offer it again once whatever stopped it has cleared", area: Itself, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["backlog"] },
        Capability { id: "nudge", what: "keep your goals and nudge you toward one you've gone quiet on, once you let it speak first", area: Time, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::Background], modules: &["nudge"] },
        Capability { id: "anticipate", what: "do the work before you ask, so the answer is ready when you come back", area: Thinking, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::Background], modules: &["anticipate"] },
        Capability { id: "routine", what: "notice something you do the same way every time and offer to do it for you", area: Thinking, state: Untested, needs: Some("a few weeks of you doing the same things"), offline: true, added: 40, runs: &[Needs::Background], modules: &["routine"] },
        Capability { id: "overnight", what: "work through what it couldn't finish while you sleep, and in the morning say what actually happened, not what was meant to", area: Itself, state: Untested, needs: Some("a night with the laptop left on"), offline: true, added: 40, runs: &[Needs::Background], modules: &["overnight", "faithful"] },
        Capability { id: "flow", what: "carry out a job of several steps on its own -- each result handed to the next, surviving a failure, pausing for your yes midway", area: Thinking, state: Untested, needs: Some("a live run on the unlocked laptop"), offline: true, added: 40, runs: &[Needs::Files, Needs::Background], modules: &["flow"] },
        Capability { id: "council", what: "put a question to a room of seats that don't agree, and bring back the disagreement", area: Thinking, state: Blocked, needs: Some("a language model"), offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["council"] },
        Capability { id: "decide", what: "work a decision through with you -- the question underneath, the options, and the strongest case against what you've chosen", area: Thinking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["decide", "otherside"] },
        Capability { id: "opportunity", what: "weigh something that might be worth doing on five counts, and say which ones it can't judge yet", area: Thinking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["opportunity"] },
        Capability { id: "reference", what: "know exact things offline -- a contract's tick size, the wash sale window -- from reference shelves you choose", area: Files, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["reference"] },
        Capability { id: "filing", what: "suggest where your files should live, and move them only when you say", area: Files, state: Untested, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["filing"] },
        Capability { id: "orders", what: "know what you ordered and where it is, read off the retailer's own emails", area: Email, state: Untested, needs: Some("a mail account set up"), offline: false, added: 40, runs: &[Needs::JustThinking], modules: &["orders"] },
        Capability { id: "outbox", what: "draft replies to clients and brands and hold them for you, or send them on your standing approval -- and cold-email only people you've named", area: Email, state: Untested, needs: Some("a mail account set up"), offline: false, added: 40, runs: &[Needs::JustThinking], modules: &["outbox", "outreach"] },
        Capability { id: "finance", what: "audit your spending from your bank's exports -- read-only by construction, with no way to move money", area: Money, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["finance"] },
        Capability { id: "budget", what: "keep a hosted model cheap -- the smallest one that will do, a yes before a big job, never a surprise bill", area: Money, state: Off, needs: None, offline: false, added: 40, runs: &[Needs::JustThinking], modules: &["budget"] },
        Capability { id: "booking", what: "work through times other people propose against your calendar, and write one in only when you accept", area: Time, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["booking"] },
        Capability { id: "content", what: "run your content -- why a post worked, a signal told from a fluke, the mistake you're about to repeat", area: Writing, state: Untested, needs: Some("your posts' numbers"), offline: true, added: 40, runs: &[Needs::Files], modules: &["content", "reach"] },
        Capability { id: "opsec", what: "check a post before it goes out for what's visible in the frame -- a patch, a tail number -- until the date the rules stop applying to you", area: Writing, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::Files], modules: &["opsec"] },
        Capability { id: "edit", what: "edit a video from what you describe -- the plan first, then the cut -- with ffmpeg or an editor you already own", area: Files, state: Untested, needs: Some("ffmpeg, which setup fetches"), offline: true, added: 40, runs: &[Needs::Files], modules: &["edit", "editors"] },
        // 29 Sep 2026: photos, by voice or typing, on a new copy. Run for
        // real here with ffmpeg 9.0 and the two cut-out models in tract;
        // never yet on Eric's laptop, hence untested.
        Capability { id: "photo", what: "edit a photo or a folder of them on a new copy -- brighter, fixed colours, straightened (offered, never forced), cropped for Instagram or a YouTube thumbnail, background blurred or removed -- and take it back", area: Files, state: Untested, needs: Some("ffmpeg, which setup fetches; the cut-out models for backgrounds"), offline: true, added: 41, runs: &[Needs::Files], modules: &["photo", "straighten", "cutout"] },
        Capability { id: "imagemake", what: "make a new picture from a description, on this machine -- nothing uploaded, saved in your Pictures folder", area: Files, state: Blocked, needs: Some("the picture maker: a one-off 6.5 GB download (say \"get the picture maker\")"), offline: true, added: 43, runs: &[Needs::Files], modules: &["imagemake"] },
        Capability { id: "selftest", what: "try every command on this machine, safely, and report what works, what's off, what needs installing and what's broken", area: Itself, state: Untested, needs: None, offline: true, added: 43, runs: &[Needs::Files, Needs::ReadScreen], modules: &["selftest", "regressions", "mutation", "coverage"] },
        Capability { id: "operate", what: "do things in your apps -- click through them, fill in boxes, choose options, use their menus -- a step at a time, asking before anything that can't be taken back", area: Windows, state: Untested, needs: Some("the language model; apps that show Windows their controls work best"), offline: true, added: 43, runs: &[Needs::ActInApps, Needs::ReadScreen], modules: &["operate"] },
        Capability { id: "grade", what: "measure a clip's loudness, dialogue and colour, say what a viewer will notice first -- in your words, not the jargon -- and fix it", area: Files, state: Untested, needs: Some("ffmpeg, which setup fetches"), offline: true, added: 40, runs: &[Needs::Files], modules: &["grade", "measure", "plainly"] },
        Capability { id: "voiceover", what: "lay your script over your footage -- where each line lands, the gaps, the music ducked under your voice", area: Files, state: Blocked, needs: Some("piper"), offline: true, added: 40, runs: &[Needs::Files], modules: &["voiceover"] },
        Capability { id: "viewing", what: "watch a video rather than only hear it -- the frames that matter read alongside what's said", area: Seeing, state: Untested, needs: Some("ffmpeg and the picture reader, which setup fetches"), offline: true, added: 40, runs: &[Needs::Files], modules: &["viewing"] },
        Capability { id: "grading", what: "walk you through a colour grade in the fixed order a colourist uses", area: Thinking, state: Working, needs: None, offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["grading"] },
        Capability { id: "chat", what: "talk with the people you work with inside the hub -- one-to-one or in groups, with no company in the middle", area: Keeping, state: Untested, needs: Some("a friend's Atlas to talk to"), offline: false, added: 40, runs: &[Needs::Files], modules: &["chat"] },
        Capability { id: "elsewhere", what: "ask your other Atlas about something, finding it on your network without typing an address", area: Keeping, state: Untested, needs: Some("a second device"), offline: false, added: 40, runs: &[Needs::JustThinking], modules: &["elsewhere", "nearby"] },
        Capability { id: "handover", what: "hand your laptop to someone else and take it back -- your things out of their reach until you type your passphrase", area: Keeping, state: Untested, needs: None, offline: true, added: 40, runs: &[Needs::Files, Needs::RealEncryption], modules: &["handover", "typed"] },
        Capability { id: "identity", what: "ask you to prove it's you only for the things you choose, and not again for a while after", area: Keeping, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::RealEncryption], modules: &["identity"] },
        Capability { id: "enrol", what: "sign you up for a site -- only where your own rules allow, and only after you say yes", area: Keeping, state: Untested, needs: Some("a live run against your real accounts"), offline: false, added: 40, runs: &[Needs::Files], modules: &["enrol"] },
        Capability { id: "tray", what: "take a link, a photo or a file you hand it from your phone, and have it read and waiting when you sit down", area: Keeping, state: Untested, needs: Some("your phone on the hub"), offline: true, added: 40, runs: &[Needs::Files], modules: &["tray"] },
        Capability { id: "phone", what: "reach you on your phone when you're away from the laptop and something can't wait", area: Keeping, state: Off, needs: None, offline: false, added: 40, runs: &[Needs::Background], modules: &["phone"] },
        Capability { id: "companion", what: "show a glance on your phone -- what it's doing, what's next, how many things wait on you -- with no secrets on the lock screen", area: Keeping, state: Untested, needs: Some("the phone app"), offline: true, added: 40, runs: &[Needs::Background], modules: &["companion", "glance"] },
        Capability { id: "remote", what: "take a job from your phone for the laptop at home, and carry a task's files to the phone so you can keep going without signal", area: Keeping, state: Untested, needs: Some("a second device"), offline: false, added: 40, runs: &[Needs::Files], modules: &["remote", "workingset"] },
        Capability { id: "mobile", what: "run on a phone as itself -- the same Atlas standing alone -- and say what an iPhone and an Android phone will and won't allow", area: Keeping, state: Untested, needs: Some("the phone app built and on a phone"), offline: true, added: 40, runs: &[Needs::JustThinking], modules: &["mobile", "ios", "android"] },
        Capability { id: "presence", what: "notice whether you're at the desk, and read a nod, a thumbs-up or where you're looking off the camera", area: Seeing, state: Off, needs: None, offline: true, added: 40, runs: &[Needs::Camera], modules: &["presence", "gaze"] },
        Capability { id: "settings", what: "keep every switch on one page, and keep what you change -- sound and voice too: when it speaks, how loud, when it may pop up", area: Itself, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Files], modules: &["settings", "preferences", "sound"] },
        Capability { id: "dash", what: "arrange the hub's home the way you want it -- which parts, in what order, how big", area: Itself, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Files], modules: &["dash", "layout_prefs"] },
        Capability { id: "social", what: "keep your own accounts' numbers as a daily record -- from each platform's own export (X, TikTok, Instagram, LinkedIn, YouTube Studio) and the free APIs (YouTube, Instagram, Threads, a Facebook Page, TikTok, Bluesky) -- and say how your last video did, what worked this month and why, when to post, and your followers over time, naming what each platform doesn't give", area: Writing, state: Untested, needs: Some("a platform's export, or its key in the vault"), offline: true, added: 41, runs: &[Needs::Files], modules: &["social"] },
        Capability { id: "watchlist", what: "watch the channels, hashtags and topics you name through their public feeds -- YouTube, Mastodon, Bluesky, Hacker News, Product Hunt, Google trends, Reddit -- and say what's working for them; TikTok, Instagram and X one page when you ask, never on a schedule", area: Web, state: Untested, needs: None, offline: false, added: 41, runs: &[Needs::Background], modules: &["social"] },
        Capability { id: "appearance", what: "look the way you choose -- light, dark, or following this computer's own settings -- everywhere at once", area: Itself, state: Untested, needs: Some("your machine"), offline: true, added: 40, runs: &[Needs::Files], modules: &["appearance", "oslook"] },
        // 29 Sep 2026. Off as shipped: it reaches public sites once a day, so
        // it waits for you to turn it on. Never applies, replies or spends.
        Capability { id: "hunt", what: "look once a day for gigs, jobs, grants, contracts and niches -- Hacker News hiring threads, Grants.gov, SAM.gov, Reddit, Product Hunt, the App Store charts, GitHub, your feeds and searches, and job alerts in your mail -- and bring the best few with why, to read more, drop or save; it never applies or replies", area: Web, state: Off, needs: None, offline: false, added: 41, runs: &[Needs::Background], modules: &["hunt", "hunting"] },
        // 29 Sep 2026, Eric: "Can we give Atlas the ability to be a smart ass".
        Capability { id: "wit", what: "be as much of a smart-ass as you like -- off, dry or full, changed in settings or by saying \"tone it down\" -- after the answer, never about errors, money, health, security or bad news, and never in anything written for someone else", area: Speaking, state: Working, needs: None, offline: true, added: 41, runs: &[Needs::JustThinking], modules: &["wit", "talkback"] },
        // 30 Sep 2026: Eric's "doesn't know what it's supposed to be doing ...
        // can't use multiple streams of thought ... not completing a task".
        Capability { id: "router", what: "offer the language model only the few tools a sentence needs, so a small model answers fast and picks the right one", area: Thinking, state: Untested, needs: Some("a language model"), offline: true, added: 42, runs: &[Needs::JustThinking], modules: &["router", "meaningroute", "meaningnative"] },
        Capability { id: "taskloop", what: "work through a request of several steps -- a plan, each step's result looked at, then the next -- and say when it's finished or what it's waiting on", area: Thinking, state: Untested, needs: Some("a language model"), offline: true, added: 42, runs: &[Needs::JustThinking], modules: &["taskloop"] },
        Capability { id: "streams", what: "do several things at once -- the parts of a request that don't depend on each other side by side -- and say what's still running when asked what it's working on", area: Thinking, state: Untested, needs: Some("a language model"), offline: true, added: 42, runs: &[Needs::JustThinking], modules: &["streams"] },
        Capability { id: "backed", what: "never say it's on something unless it really started it", area: Speaking, state: Working, needs: None, offline: true, added: 42, runs: &[Needs::JustThinking], modules: &["backed"] },
    ]
}

/// Everything in one area.
pub fn in_area(area: Area) -> Vec<Capability> {
    all().into_iter().filter(|c| c.area == area).collect()
}

/// What you can rely on today.
pub fn working() -> Vec<Capability> {
    all().into_iter().filter(|c| c.state.usable()).collect()
}

/// What's waiting on something, and on what.
pub fn blocked() -> Vec<(Capability, &'static str)> {
    all()
        .into_iter()
        .filter_map(|c| c.needs.map(|n| (c.clone(), n)).filter(|_| c.state == State::Blocked))
        .collect()
}

/// Grouped by what each is waiting for, because six things blocked on one
/// install is one job rather than six.
pub fn what_would_unblock_most() -> Vec<(&'static str, usize)> {
    let mut counts: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for (_, need) in blocked() {
        *counts.entry(need).or_insert(0) += 1;
    }
    let mut v: Vec<(&'static str, usize)> = counts.into_iter().collect();
    v.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    v
}

/// Who can finish an unfinished capability — the distinction a self-driven
/// completion needs, because "what's left" is three different jobs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Finisher {
    /// Atlas could carry this the rest of the way itself — it's a wiring job on
    /// code that already exists, the kind of thing the self-fix loop drafts.
    Mine,
    /// Built, wired and tested; it only needs a real run on the machine before
    /// it can be trusted. Atlas can't do that in a cloud container — it's a run,
    /// not a change.
    NeedsARun,
    /// Waiting on something only you can provide — an install, an account, a
    /// model, a piece of hardware.
    NeedsYou,
}

/// One thing that isn't finished, and who can finish it.
#[derive(Debug, Clone)]
pub struct Unfinished {
    pub what: &'static str,
    pub finisher: Finisher,
    pub waiting_on: Option<&'static str>,
}

/// The self-finishing backlog: everything not yet working, ordered by how close
/// it is to Atlas's own power to finish — what it could wire itself first, then
/// what only needs a run, then what waits on you.
///
/// This is the honest completion picture. Deliberately *not* the fine-grained
/// wiring backlog (the dead-method ratchet in the tests), which is a scan of
/// the source the running daemon can't do; this is the capability-level view,
/// which is what a person means by "what's left to finish".
pub fn to_finish() -> Vec<Unfinished> {
    let mut out: Vec<Unfinished> = all()
        .into_iter()
        .filter_map(|c| {
            let finisher = match c.state {
                // Not written yet: Atlas's own to build only when it's a wiring
                // job on existing code; if it names something external, it's
                // waiting on that.
                State::Planned => match c.needs {
                    None => Finisher::Mine,
                    Some(n) if n.contains("wir") => Finisher::Mine,
                    Some(_) => Finisher::NeedsYou,
                },
                State::Untested => Finisher::NeedsARun,
                State::Blocked => Finisher::NeedsYou,
                // Working or deliberately Off: not unfinished.
                State::Working | State::Off => return None,
            };
            Some(Unfinished { what: c.what, finisher, waiting_on: c.needs })
        })
        .collect();
    out.sort_by_key(|u| match u.finisher {
        Finisher::Mine => 0u8,
        Finisher::NeedsARun => 1,
        Finisher::NeedsYou => 2,
    });
    out
}

/// The self-finishing backlog said in plain words — the honest split of what's
/// left and who finishes each part.
pub fn to_finish_report() -> String {
    let items = to_finish();
    if items.is_empty() {
        return "Everything built is wired and running — there's nothing left to finish.".into();
    }
    let count = |f: Finisher| items.iter().filter(|u| u.finisher == f).count();
    let mine = count(Finisher::Mine);
    let run = count(Finisher::NeedsARun);
    let yours = count(Finisher::NeedsYou);

    let mut s = String::from("What's left to finish: ");
    let mut parts = Vec::new();
    if mine > 0 {
        parts.push(format!("{mine} I could wire myself"));
    }
    if run > 0 {
        parts.push(format!("{run} that are built but have never run on your machine"));
    }
    if yours > 0 {
        parts.push(format!("{yours} waiting on you"));
    }
    s.push_str(&parts.join(", "));
    s.push('.');

    if mine > 0 {
        let named: Vec<&str> =
            items.iter().filter(|u| u.finisher == Finisher::Mine).map(|u| u.what).collect();
        s.push_str(&format!(" I'd start on: {}.", named.join("; ")));
    }
    if yours > 0 {
        if let Some((need, n)) = what_would_unblock_most().first() {
            s.push_str(&format!(" The one thing that unlocks the most is {need} ({n} of them)."));
        }
    }
    if run > 0 {
        s.push_str(
            " The rest is built and tested — what it really needs is a run on real hardware, \
             which I can't do from here. ",
        );
        // But most of that run is mine to do, not yours to sit through.
        s.push_str(&commissioning_report());
    }
    s
}

/// How a built-but-never-run capability gets verified on real hardware — and,
/// the part that actually costs your time, whether you have to be there for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verify {
    /// Atlas runs it against your real data or model and checks the result
    /// itself. No hands, no watching. Most of the tree is this: the market
    /// reads, the trade logic, the code explainer — they compute an answer that
    /// is either right or wrong on its own terms.
    Computes,
    /// Atlas does the thing and then reads the machine back to confirm it
    /// happened — the window really moved, the app really opened, the page it
    /// asked for really loaded, the right text came off the screen. Still no
    /// watching: Atlas is its own witness through the same sensors it acts with.
    ReadBack,
    /// Only you can say whether it got it right — it named the thing in front of
    /// the camera, it followed your hand. The handful that genuinely need your
    /// eyes.
    YourEyes,
}

/// How a capability would be verified, read from what it needs to run.
///
/// The strongest requirement wins: anything through the camera needs your eyes;
/// anything that moves a window, opens an app, acts in one, or reads the screen
/// is checked by reading the result back; everything else just computes and
/// checks itself.
pub fn how_verified(c: &Capability) -> Verify {
    let has = |n: Needs| c.runs.contains(&n);
    if has(Needs::Camera) || has(Needs::Audio) || has(Needs::WakeWord) {
        Verify::YourEyes
    } else if has(Needs::Windows_)
        || has(Needs::LaunchApps)
        || has(Needs::ActInApps)
        || has(Needs::ReadScreen)
    {
        Verify::ReadBack
    } else {
        Verify::Computes
    }
}

/// The commissioning split of everything built-but-never-run: how many Atlas
/// verifies by computing, how many by acting and reading the result back, and
/// the few that need your eyes — named, because that short list is the only
/// part of setup that actually costs your time.
pub fn commissioning() -> (usize, usize, usize, Vec<&'static str>) {
    let (mut computes, mut read_back) = (0usize, 0usize);
    let mut your_eyes: Vec<&'static str> = Vec::new();
    for c in all() {
        if c.state != State::Untested {
            continue;
        }
        match how_verified(&c) {
            Verify::Computes => computes += 1,
            Verify::ReadBack => read_back += 1,
            Verify::YourEyes => your_eyes.push(c.what),
        }
    }
    (computes, read_back, your_eyes.len(), your_eyes)
}

/// The commissioning split said in plain words — the honest answer to "how much
/// of setting this up do I have to sit through?"
pub fn commissioning_report() -> String {
    let (computes, read_back, eyes_n, eyes) = commissioning();
    let total = computes + read_back + eyes_n;
    if total == 0 {
        return "Nothing is waiting on a first run — it's all been exercised.".into();
    }
    let auto = computes + read_back;
    let mut s = format!(
        "Of {total} things never run on this machine, I can verify {auto} myself — \
         {computes} by running them on your real data and checking the result, and \
         {read_back} by doing them and reading the machine back to confirm it happened."
    );
    if eyes_n == 0 {
        s.push_str(" None need you to watch.");
    } else {
        s.push_str(&format!(
            " Only {eyes_n} need your eyes: {}. That short list is your part — not the whole set.",
            eyes.join("; ")
        ));
    }
    s
}

/// What Atlas knows about itself, for the model, picked for one question.
///
/// Eric, 27 Sep 2026: asked "how do I add my phone" and "where is Atlas
/// doctor", Atlas knew nothing -- the model was never told what Atlas can do
/// or where anything is. This is the hub's own map (the search palette's
/// entries, with their pages) and the capability list, the ones whose words
/// meet the question first, so a small model answers from facts rather than
/// making something up.
pub fn about_atlas(said: &str, most: usize) -> String {
    about_atlas_lines(said, most)
}

/// Is this a question about Atlas itself -- what it can do, its setup, where
/// something is in it? Only then do `about_atlas`'s lines go in the prompt.
///
/// They went in on every turn, headed "answer only from these lines ... say
/// you're not sure rather than guess", and a small model read that as
/// applying to everything: "what's the capital of France" got "I'm not sure"
/// (27 Sep 2026).
pub fn is_about_atlas(said: &str) -> bool {
    let t: String = said.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    let mut w: Vec<&str> = t.split_whitespace().collect();
    // "Atlas, ..." is calling it by name, not asking about it (29 Sep 2026:
    // nearly everything Eric said began "Atlas" -- or "At this", the name
    // misheard -- and every one of them carried the whole about-Atlas block,
    // two thousand characters, into the prompt).
    for lead in [&["hey", "atlas"][..], &["ok", "atlas"], &["okay", "atlas"], &["at", "this"], &["atlas"]] {
        if w.len() > lead.len() && w[..lead.len()] == *lead {
            w.drain(..lead.len());
            break;
        }
    }
    if w.last() == Some(&"atlas") && w.len() > 1 {
        w.pop();
    }
    let t = format!(" {} ", w.join(" "));
    if crate::register::read(said, &Default::default()) == crate::register::Register::AboutAtlas {
        return true;
    }
    const ABOUT: &[&str] = &[
        " atlas ", " the hub ", " hub page", " your settings", " settings ", " setup ", " set up ", " set you up",
        " can you ", " could you ", " are you able", " do you have ", " your memory", " you store", " you remember",
        " where do i ", " where is the ", " where are my ", " how do i turn ", " how do i change ", " switch on ",
        " switch off ", " turn on ", " turn off ", " your model", " you running", " your voice", " about you",
        " yourself ", " what are you ", " who are you ", " who made you", " work offline", " without internet",
    ];
    ABOUT.iter().any(|a| t.contains(a))
}

fn about_atlas_lines(said: &str, most: usize) -> String {
    let words: Vec<String> = said
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2 && !["the", "and", "you", "can", "how", "what", "where", "atlas", "for", "does", "with", "your", "are"].contains(w))
        .map(str::to_string)
        .collect();
    let score = |text: &str| -> usize {
        let t = text.to_lowercase();
        words.iter().filter(|w| t.contains(w.as_str())).count()
    };
    let mut pages: Vec<(usize, String)> = crate::palette::catalogue()
        .iter()
        .filter_map(|e| {
            let crate::palette::Does::Go(href) = e.does else { return None };
            let n = score(&format!("{} {} {}", e.label, e.hint, e.also.join(" ")));
            (n > 0).then(|| (n, format!("- {}: {} (hub page {href})", e.label, e.hint)))
        })
        .collect();
    pages.sort_by(|a, b| b.0.cmp(&a.0));
    let mut caps: Vec<(usize, String)> = all()
        .iter()
        .filter_map(|c| {
            let n = score(&format!("{} {}", c.id, c.what));
            (n > 0).then(|| {
                let needs = c.needs.map(|n| format!(", needs {n}")).unwrap_or_default();
                (n, format!("- {}: {}{needs}", c.what, c.state.plain()))
            })
        })
        .collect();
    caps.sort_by(|a, b| b.0.cmp(&a.0));
    let mut out = String::from(
        "About Atlas (you): you are Atlas. When asked about yourself, your setup or where something is, answer only \
         from these lines and name the hub page; if the answer isn't here, say you're not sure rather than guess. \
         Never tell the person to type a command; point them to a hub page.\n",
    );
    for (_, l) in pages.iter().take(most) {
        out.push_str(l);
        out.push('\n');
    }
    for (_, l) in caps.iter().take(most) {
        out.push_str(l);
        out.push('\n');
    }
    out
}


// ---------------------------------------------------------------------------
// Knowing its own abilities (30 Sep 2026).
//
// Eric: "I have a long list of capabilities Atlas is supposed to perform and
// it doesn't know how to perform them or even know that it has them." On his
// laptop Atlas said "I don't have a camera", "I'm not supposed to" and "I
// don't have a research mode" -- while this catalogue lists looking through
// the camera and research. "Can you X" matched the first entry sharing a
// five-letter word with X ("I don't have anything for that" otherwise). Now
// a question about an ability is searched for (BM25 over what each entry is
// for, with the everyday words people use for it), and answered with its
// state and what turns it on.
// ---------------------------------------------------------------------------

/// Everyday words for an ability that its own description doesn't carry.
const ABILITY_WORDS: &[(&str, &str)] = &[
    ("vision", "camera webcam see me look at me face faces who's here room"),
    ("presence", "camera webcam see me desk watching"),
    ("research", "research internet web online browse search look up find out study"),
    ("speak", "voice talk speak out loud say"),
    ("wake", "hear listen microphone mic wake word"),
    ("mail", "email emails inbox"),
    ("calendar", "calendar schedule agenda events appointments meetings"),
    ("ocr", "screen read text"),
];

/// What turns an ability on, or that it's already there, in a clause.
fn state_said(c: &Capability) -> String {
    match c.state {
        State::Working => "works now".to_string(),
        State::Off => "built, switched off -- Settings turns it on".to_string(),
        State::Blocked => format!("built, waiting on {} -- setup fetches it", c.needs.unwrap_or("a piece that isn't installed")),
        State::Untested => match c.needs {
            Some(n) => format!("built, not tried on this machine yet (it uses {n}) -- ask and it will try"),
            None => "built, not tried on this machine yet -- ask and it will try".to_string(),
        },
        State::Planned => "not built yet".to_string(),
    }
}

/// The catalogue as it stands on this machine: the entries whose state
/// depends on a setting read from it (`research_on`: web research turned
/// on in Settings).
fn as_set_up(research_on: bool) -> Vec<Capability> {
    let mut all = all();
    for c in all.iter_mut() {
        if c.id == "research" {
            c.state = if research_on { State::Working } else { State::Off };
        }
    }
    all
}

/// The abilities a question is about, best first, at most `most`.
pub fn find_abilities(said: &str, research_on: bool, most: usize) -> Vec<Capability> {
    let all = as_set_up(research_on);
    let mut index = crate::bm25::Index::default();
    for (i, c) in all.iter().enumerate() {
        let extra: Vec<&str> = ABILITY_WORDS.iter().filter(|(id, _)| *id == c.id).map(|(_, w)| *w).collect();
        // The everyday words count as much as its own description does.
        index.add(i as u64, &format!("{} {}", c.what, extra.join(" ")), &format!("{} {}", c.id, c.area.plain()));
    }
    let q = crate::router::request_words(said).join(" ");
    if q.trim().is_empty() {
        return Vec::new();
    }
    let hits = index.search(&q, most);
    let best = hits.first().map(|h| h.1).unwrap_or(0.0);
    hits.into_iter()
        .filter(|(_, s)| *s >= ABILITY_FLOOR && *s >= best * 0.5)
        .filter_map(|(i, _)| all.get(i as usize).cloned())
        .collect()
}

/// Below this BM25 score an entry isn't what was asked about.
const ABILITY_FLOOR: f64 = 3.0;

/// "Can you X", answered from the catalogue: the ability, its state, and
/// what turns it on. `None` when nothing in the catalogue is about X.
pub fn answer_can(what: &str, research_on: bool) -> Option<String> {
    let found = find_abilities(what, research_on, 2);
    let first = found.first()?;
    let mut s = format!("Yes -- I can {}: {}.", first.what, state_said(first));
    if let Some(second) = found.get(1) {
        s.push_str(&format!(" Also: {} ({}).", second.what, state_said(second)));
    }
    Some(s)
}

/// The truth about one ability a reply said Atlas lacks
/// (`backed::denies_an_ability`): the catalogue entry that is that ability,
/// said with its state. `topic` is the denial's word ("camera", "research",
/// "screen"), or anything else to be searched for.
pub fn truth_about(topic: &str, research_on: bool) -> Option<String> {
    if topic == "grow" {
        return Some(crate::growth::CAN_GROW.to_string());
    }
    let id = match topic {
        "camera" => Some("vision"),
        "research" => Some("research"),
        "screen" => Some("picture_talk"),
        "files" => Some("findfile"),
        _ => None,
    };
    let c = match id {
        Some(id) => as_set_up(research_on).into_iter().find(|c| c.id == id)?,
        None => find_abilities(topic, research_on, 1).into_iter().next()?,
    };
    Some(format!("Actually, I can {}: {}.", c.what, state_said(&c)))
}

/// What Atlas can do that bears on a question about itself, for the model:
/// a few catalogue lines with their states, and the rule that it never says
/// it lacks one of them.
pub fn abilities_for_prompt(said: &str, research_on: bool, most: usize) -> String {
    // At most `most` lines in all, pages first: each line is paid for on
    // every turn that carries it (the prompt diet, 30 Sep 2026).
    let pages = hub_pages_for(said, most);
    let found = find_abilities(said, research_on, most.saturating_sub(pages.len()).max(1));
    let mut out = format!(
        "About Atlas (you) -- true, so never say you lack one of these; for a setting, name its hub page. Web research: {}.\n",
        if research_on { "on" } else { "off -- Settings turns it on" }
    );
    for p in &pages {
        out.push_str(p);
        out.push('\n');
    }
    for c in &found {
        out.push_str(&format!("- {}: {}\n", c.what, state_said(c)));
    }
    if found.is_empty() && pages.is_empty() {
        out.push_str("For anything else you might do, call the capabilities tool rather than guess.\n");
    }
    out
}

/// The hub's pages a question is about (the search palette's entries), as
/// lines for the model, best first, at most `most`.
fn hub_pages_for(said: &str, most: usize) -> Vec<String> {
    // Only for a question about where something is or how to change it:
    // "can you see me" isn't asking for a page.
    let t = format!(" {} ", said.to_lowercase());
    let about_where = [" where ", " setting", " change ", " turn on", " turn off", " switch ", " set up", " setup", " page", " how do i "]
        .iter()
        .any(|w| t.contains(w));
    if !about_where {
        return Vec::new();
    }
    let words: Vec<String> = crate::router::request_words(said).into_iter().filter(|w| w.len() > 2).collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut pages: Vec<(usize, String)> = crate::palette::catalogue()
        .iter()
        .filter_map(|e| {
            let crate::palette::Does::Go(href) = e.does else { return None };
            let t = format!("{} {} {}", e.label, e.hint, e.also.join(" ")).to_lowercase();
            let n = words.iter().filter(|w| t.contains(w.as_str())).count();
            (n > 0).then(|| (n, format!("- {}: {} (hub page {href})", e.label, crate::router::clip_words(e.hint, 90))))
        })
        .collect();
    pages.sort_by(|a, b| b.0.cmp(&a.0));
    let best = pages.first().map(|p| p.0).unwrap_or(0);
    pages.into_iter().filter(|p| p.0 == best).take(most).map(|p| p.1).collect()
}

/// What Atlas says to "what can you do?"
///
/// By area, and honest about the difference between working and built.
pub fn summary() -> String {
    let all = all();
    let working = all.iter().filter(|c| c.state.usable()).count();
    let untested = all.iter().filter(|c| c.state == State::Untested).count();
    let blocked = all.iter().filter(|c| c.state == State::Blocked).count();
    let off = all.iter().filter(|c| c.state == State::Off).count();

    // By area, the three biggest, counted (28 Sep 2026). With over two hundred
    // entries a bare "73 things work" says nothing about *what*, and a list of
    // seventy is not an answer anyone can hear. Three areas with their counts
    // is the shape of it in one breath; "the full list" is still there for
    // the rest.
    let mut by_area: Vec<(Area, usize)> = EVERY_AREA
        .iter()
        .map(|a| (*a, all.iter().filter(|c| c.area == *a && c.state.usable()).count()))
        .filter(|(_, n)| *n > 0)
        .collect();
    by_area.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let top: Vec<String> = by_area.iter().take(3).map(|(a, n)| format!("{} {n}", a.plain())).collect();
    let mut s = format!("{working} things work right now");
    match top.len() {
        0 => s.push('.'),
        1 => s.push_str(&format!(" -- {}.", top[0])),
        _ => s.push_str(&format!(
            " -- the most in {} and {}.",
            top[..top.len() - 1].join(", "),
            top[top.len() - 1]
        )),
    }
    if off > 0 {
        s.push_str(&format!(" {off} more are built and switched off."));
    }
    if blocked > 0 {
        if let Some((need, n)) = what_would_unblock_most().first() {
            s.push_str(&format!(" {blocked} are waiting on something — {n} of them on {need}."));
        }
    }
    if untested > 0 {
        s.push_str(&format!(" {untested} have never run on your machine."));
    }
    s.push_str(" Ask for the full list to hear them by area.");
    s
}

/// Everything, for reading.
pub fn full() -> String {
    let mut s = String::new();
    for area in EVERY_AREA {
        let items = in_area(*area);
        if items.is_empty() {
            continue;
        }
        s.push_str(&format!("\n{}\n", area.plain().to_uppercase()));
        for c in items {
            let mark = match c.state {
                State::Working => "  ",
                State::Off => "· ",
                State::Blocked => "! ",
                State::Untested => "? ",
                State::Planned => "  ",
            };
            s.push_str(&format!("{mark}{:<52} {}", c.what, c.state.plain()));
            if let Some(n) = c.needs {
                if c.state != State::Working {
                    s.push_str(&format!(" ({n})"));
                }
            }
            s.push('\n');
        }
    }
    s.push_str("\n  works   · switched off   ! waiting   ? never run for real\n");
    s
}

/// What arrived recently, so "what's new" is answerable.
pub fn since(version: u32) -> Vec<Capability> {
    let mut v: Vec<Capability> = all().into_iter().filter(|c| c.added > version).collect();
    v.sort_by_key(|c| std::cmp::Reverse(c.added));
    v
}

/// Can Atlas do this, with what's actually installed?
pub fn can(id: &str) -> Option<(bool, String)> {
    let c = all().into_iter().find(|c| c.id == id)?;
    Some(match c.state {
        State::Working => (true, format!("yes — {}", c.what)),
        State::Off => (false, format!("built, but switched off. Turn on \"{}\" in settings.", c.id)),
        State::Blocked => (
            false,
            format!("not until {} is installed.", c.needs.unwrap_or("something")),
        ),
        State::Untested => (
            false,
            format!("built, but it has never run on a real machine — so I can't promise it works."),
        ),
        State::Planned => (false, format!("not built yet.")),
    })
}

/// What works with the network unplugged.
pub fn offline_count() -> (usize, usize) {
    let all = all();
    (all.iter().filter(|c| c.offline).count(), all.len())
}

// ===================== where each of these runs =====================
//
// Atlas is meant to run on Windows, a Mac, Linux, an iPhone and an Android
// phone. Until now the catalogue said what Atlas can do and `portable` said
// what a platform allows, and nothing joined the two — so "can it do this on
// my phone?" was a question you answered by reading both and doing it in your
// head.
//
// The join is computed, never typed. A capability states what it asks of the
// machine (`runs`); `portable::how` states what a platform grants; the answer
// is the worst of them. There is deliberately no field anywhere saying
// "works on Android", because a field like that is a claim somebody has to
// remember to update, and the whole reason this module exists is that people
// don't.

/// How badly a `How` stops you. Ordered so the worst wins a comparison.
fn severity(h: How) -> u8 {
    match h {
        How::Built => 0,
        How::Possible => 1,
        How::Awkward => 2,
        How::Never => 3,
    }
}

/// What this capability does on that platform — the worst of what it asks for.
///
/// A capability is no better than the hardest thing it needs. `dictate` asks
/// for audio *and* the ability to type into another app; on an iPhone the
/// second is a wall, so dictate is a wall, even though the microphone is fine.
pub fn runs_on(c: &Capability, p: Platform) -> How {
    c.runs
        .iter()
        .map(|n| portable::how(p, *n))
        .max_by_key(|h| severity(*h))
        .unwrap_or(How::Built)
}

/// The one thing standing in the way, when something is.
///
/// "Not on your phone" is an answer that ends the conversation. "Not on your
/// phone because it would have to read your screen" is one you can argue with.
pub fn blocked_by(c: &Capability, p: Platform) -> Option<Needs> {
    c.runs
        .iter()
        .copied()
        .filter(|n| portable::how(p, *n) == How::Never)
        .max_by_key(|n| severity(portable::how(p, *n)))
}

/// One line about one capability on one platform.
///
/// Two facts, kept apart on purpose. `state` says whether Atlas has written
/// it; `how` says whether the platform would allow it if Atlas had. Reporting
/// only the second is how "say it doesn't know rather than guessing" — which
/// is not wired in — came out as "works" on an iPhone.
pub fn says(c: &Capability, p: Platform) -> String {
    // "waiting on something" is the shape of an answer rather than one. The
    // list already records what it is waiting on, so say it.
    let state = match (c.state, c.needs) {
        (State::Blocked, Some(n)) => format!("waiting on {n}"),
        _ => c.state.plain().to_string(),
    };

    match runs_on(c, p) {
        How::Never => {
            let n = blocked_by(c, p).expect("Never means something is walled");
            let mut s = format!("never on {} — it would have to be {}", p.name(), n.plain());
            if let Some(why) = portable::because(p, n) {
                s.push_str(&format!(", and {why}"));
            }
            s
        }
        How::Awkward => {
            let why = c
                .runs
                .iter()
                .copied()
                .find(|n| portable::how(p, *n) == How::Awkward)
                .and_then(|n| portable::because(p, n));
            match why {
                Some(w) => format!("{state} — on {}, {w}", p.name()),
                None => format!("{state} — with a catch on {}", p.name()),
            }
        }
        How::Possible => format!("{state} — and the {} part of it isn't written", p.bare()),
        How::Built => state,
    }
}

/// Everything that is possible at all on a platform.
pub fn on(p: Platform) -> Vec<Capability> {
    all()
        .into_iter()
        .filter(|c| runs_on(c, p).is_effort_not_a_wall())
        .collect()
}

/// Everything that platform will never allow, and the reason each time.
pub fn walled_on(p: Platform) -> Vec<(Capability, Needs)> {
    all()
        .into_iter()
        .filter_map(|c| blocked_by(&c, p).map(|n| (c, n)))
        .collect()
}

/// What to say when someone asks whether it works on their phone.
pub fn on_platform_summary(p: Platform) -> String {
    let all = all();
    let total = all.len();
    let walls: Vec<(Capability, Needs)> = walled_on(p);
    // Counted rather than subtracted. `on` and `walled_on` filter on opposite
    // sides of the same test, so if they ever stop being opposites this
    // number goes wrong visibly instead of quietly being right.
    let possible = on(p).len();
    debug_assert_eq!(possible + walls.len(), total);

    let mut s = format!("On {}, {possible} of {total} are possible.", p.name());
    if walls.is_empty() {
        s.push_str(" Nothing is off the table — the rest is time.");
        return s;
    }

    // Grouped by the reason, because six things blocked by one platform rule
    // is one rule rather than six.
    let mut by_reason: std::collections::BTreeMap<&'static str, usize> = Default::default();
    for (_, n) in &walls {
        *by_reason.entry(n.plain()).or_insert(0) += 1;
    }
    let mut reasons: Vec<(&str, usize)> = by_reason.into_iter().collect();
    reasons.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    let said: Vec<String> = reasons
        .iter()
        .map(|(r, n)| format!("{n} {} {r}", if *n == 1 { "needs" } else { "need" }))
        .collect();
    s.push_str(&format!(" The other {} can't: {}.", walls.len(), said.join(", ")));
    s
}

/// The whole catalogue for one platform, for reading.
pub fn on_platform_full(p: Platform) -> String {
    let mut s = format!("WHAT ATLAS DOES ON {}\n", p.bare().to_uppercase());
    s.push_str(&format!("{}\n", on_platform_summary(p)));
    // Said once, at the top, because every line below reads as a promise
    // otherwise. There is one build of Atlas and it is the one you are
    // running; the rest of this is what would happen if there were another.
    if p != crate::platform::what_am_i() {
        s.push_str(&format!("There is no {} build yet. What one would get: {}\n", p.bare(), portable::for_a_friend(p)));
    }
    for area in EVERY_AREA {
        let items: Vec<Capability> = in_area(*area);
        if items.is_empty() {
            continue;
        }
        s.push_str(&format!("\n{}\n", area.plain().to_uppercase()));
        for c in items {
            s.push_str(&format!("  {:<52} {}\n", c.what, says(&c, p)));
        }
    }
    s
}

/// Every area, once, so nothing is left out of a listing by being forgotten.
pub const EVERY_AREA: &[Area] = &[
    Area::Hearing, Area::Speaking, Area::Windows, Area::Files, Area::Web,
    Area::Writing, Area::Money, Area::Email, Area::Thinking, Area::Itself,
    Area::Seeing, Area::Keeping, Area::Time,
];

/// The catalogue as the document in `docs/CAPABILITIES.md`.
///
/// Generated rather than written. The hand-kept version of that file said
/// "say it doesn't know rather than guessing — working" for a capability
/// nothing could reach, and had said so since at least 12 Sep 2026. A
/// document about what a system does, maintained separately from the system,
/// becomes a document about what somebody once meant to build.
///
/// `tests/catalogue.rs` compares the file on disk with this, so the two
/// cannot part company without a test saying so.
pub fn as_markdown() -> String {
    let mut s = String::new();
    s.push_str("# What Atlas can do\n\n");
    s.push_str(
        "Generated by `atlas catalog --markdown`. Do not edit by hand — a hand-kept copy of this \
         drifted from the code for a week, and a document about what a system does is worth \
         nothing the moment it stops being the system's own answer.\n\n",
    );
    s.push_str(
        "Ask it directly — *\"what can you do?\"*, *\"what's new?\"*, *\"what are you waiting \
         on?\"*, *\"can you read my email?\"* — or read this.\n\n",
    );
    s.push_str(
        "**State** is what Atlas has written. The platform columns are what the platform would \
         allow if it had. The two are separate on purpose: something can be impossible on an \
         iPhone and unbuilt everywhere, and collapsing them hides which.\n\n",
    );
    s.push_str("| state | means |\n|---|---|\n");
    for st in [State::Working, State::Off, State::Blocked, State::Untested, State::Planned] {
        s.push_str(&format!("| **{}** | ", st.plain()));
        s.push_str(match st {
            State::Working => "you can rely on it today",
            State::Off => "built and tested, waiting for you to turn it on",
            State::Blocked => "needs something installed",
            State::Untested => "compiles and passes tests, has never touched real hardware",
            State::Planned => "nothing in the running program can reach it",
        });
        s.push_str(" |\n");
    }
    // Deliberately not `How::plain`, which is written for a sentence about
    // one thing on one platform ("works"). In a column beside a state of
    // "not built yet", "works" reads as a contradiction rather than as the
    // separate fact it is.
    s.push_str("\n| platform | means |\n|---|---|\n");
    s.push_str("| ready | the platform allows it, and Atlas's layer for that platform is written |\n");
    s.push_str("| would | the platform allows it; that layer isn't written yet |\n");
    s.push_str("| catch | allowed, with a compromise worth knowing — listed at the bottom |\n");
    s.push_str("| **no** | the platform forbids it, however much gets written |\n");
    s.push_str(
        "\nThere is one build of Atlas, and it is the Windows one. Every other column is what \
         that platform permits, not something you can install today.\n",
    );

    let platforms = [
        Platform::Windows, Platform::Mac, Platform::Linux,
        Platform::Ios, Platform::Android, Platform::Web,
    ];

    for area in EVERY_AREA {
        let items = in_area(*area);
        if items.is_empty() {
            continue;
        }
        let title = area.plain();
        let mut chars = title.chars();
        let title = match chars.next() {
            Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            None => title.to_string(),
        };
        s.push_str(&format!("\n## {title}\n\n"));
        s.push_str("| | state | lives in |");
        for p in platforms {
            s.push_str(&format!(" {} |", p.bare()));
        }
        s.push_str("\n|---|---|---|");
        s.push_str(&"---|".repeat(platforms.len()));
        s.push('\n');
        for c in items {
            let state = match (c.state, c.needs) {
                (State::Blocked, Some(n)) => format!("waiting on {n}"),
                _ => c.state.plain().to_string(),
            };
            let lives = c
                .modules
                .iter()
                .map(|m| format!("`{m}`"))
                .collect::<Vec<_>>()
                .join(" ");
            s.push_str(&format!("| {} | {state} | {lives} |", c.what));
            for p in platforms {
                s.push_str(&format!(" {} |", mark(runs_on(&c, p))));
            }
            s.push('\n');
        }
    }

    s.push_str("\n## What a platform will never allow\n\n");
    for p in platforms {
        let walls = walled_on(p);
        if walls.is_empty() {
            s.push_str(&format!("**{}** — nothing. The rest is time.\n\n", p.bare()));
            continue;
        }
        s.push_str(&format!("**{}**\n\n", p.bare()));
        let mut by_reason: std::collections::BTreeMap<Needs, Vec<&'static str>> = Default::default();
        for (c, n) in &walls {
            by_reason.entry(*n).or_default().push(c.what);
        }
        for (n, what) in by_reason {
            s.push_str(&format!("- **{}**", n.plain()));
            if let Some(why) = portable::because(p, n) {
                s.push_str(&format!(" — {why}"));
            }
            s.push_str(&format!(". So: {}.\n", what.join("; ")));
        }
        s.push('\n');
    }

    let (claimed, modules) = module_coverage();
    s.push_str(&format!(
        "---\n\n{} things, across {claimed} of {modules} source files. The other {} are \
         plumbing — config, storage, the platform layer — each named in `capability::PLUMBING` \
         with the reason it is not a capability. `tests/catalogue.rs` fails for a module that is \
         neither, so nothing built can go unlisted here.\n",
        all().len(),
        PLUMBING.len()
    ));
    s
}

fn mark(h: How) -> &'static str {
    match h {
        How::Built => "ready",
        How::Possible => "would",
        How::Awkward => "catch",
        How::Never => "**no**",
    }
}

/// What to say about this machine before somebody runs into it.
///
/// `portable.warn_up_front` is the setting — "say what won't work here before
/// it's needed rather than after" — and until 19 Sep 2026 there was no
/// `portable:` section for it to arrive in and nothing that would have read
/// one. The catalogue is what made it answerable: a wall is only worth
/// warning about if you can say which of the things Atlas does hits it.
///
/// `None` on a platform with no walls. A line saying "everything here is
/// possible" is the kind of reassurance that teaches people to skip the
/// section it is printed in, and then to miss the one that matters.
pub fn heads_up(p: Platform, cfg: &crate::portable::PortableConfig) -> Option<String> {
    if !cfg.warn_up_front {
        return None;
    }
    let walls = walled_on(p);
    if walls.is_empty() {
        return None;
    }
    let mut by_reason: std::collections::BTreeMap<Needs, Vec<&'static str>> = Default::default();
    for (c, n) in &walls {
        by_reason.entry(*n).or_default().push(c.what);
    }
    let said: Vec<String> = by_reason
        .into_iter()
        .map(|(n, what)| format!("{} — so no {}", n.plain(), what.join(", no ")))
        .collect();
    Some(format!(
        "On {}, {} of {} can't work at all: {}.",
        p.name(),
        walls.len(),
        all().len(),
        said.join("; ")
    ))
}

/// Every module any capability claims, deduplicated.
///
/// The catalogue's own view of the source tree. `tests/catalogue.rs` checks
/// each of these is a real file and counts the ones nothing claims.
pub fn claimed_modules() -> std::collections::BTreeSet<&'static str> {
    all().into_iter().flat_map(|c| c.modules.iter().copied()).collect()
}

/// How many source files there are.
///
/// Pinned rather than counted, because a running Atlas has a binary and not a
/// source tree — counting at runtime would give zero on the machine where the
/// number matters. `tests/catalogue.rs` counts the tree for real and fails if
/// this has drifted, so the pin is checked rather than trusted.
// 277 -> 281 (19 Sep 2026). Four modules, all from the same afternoon and
// all wired in the change that added them: `whichone` (choosing between
// competing readings of a sentence, which the daemon used to do with ordered
// `contains` arms), `nearby` (finding your other Atlas on the local network,
// which is what `elsewhere.known`'s hand-typed address was standing in for),
// `telegram` (the message reader `messaging.rs` never had), and `judgment`
// (graded judgments that say how sure they are, which a dozen modules had
// each grown their own version of).
// 294 -> 310 (23 Sep 2026, the GitHub ports). Sixteen modules, each wired in
// the change that added it and claimed by a capability at birth: `recur`,
// `civil`, `cronspec`, `stemmer`, `bm25`, `chunker`, `typos`, `linkage`,
// `mailthread`, `vformat`, `hlc`, `automation`, `urgency`, `ratelimit`,
// `readable`, `sealedlog`. (`sha256` was dropped before landing: `digest`
// already had one.)
// 334 -> 339 (24 Sep 2026, rounds 3-5 merged onto the durable base): the
// base's own five (appearance, firstlaunch, getpieces, phonelink, setupwin).
// 339 -> 343 (24 Sep 2026, round 6): `pngcodec`, `gifenc`, `filmstrip`,
// `scene3d` — the rendering arms, each claimed by `filmstrip` or `scene3d`.
// 343 -> 344 (24 Sep, round 6 merge): `transport`, from the main chat's
// same-network sync (7ec88ee).
// 344 -> 345 (24 Sep, round 8): `meshio` — OBJ/STL/glTF readers and a BVH,
// claimed by `scene3d`.
// 345 -> 346 (25 Sep, merging the main chat's b70be92): `platform::mobile`,
// the phone's platform layer (plumbing beside posix/win/mock, unclaimed).
// 346 -> 348 (25 Sep, round 9): `worklog` (where the time went) and `when`
// (times in words), each claimed by its own capability.
// 348 -> 349 (25 Sep, merging the main chat's 0436ddf/d369458): `release`,
// the ed25519 release-signature root of the update courier (plumbing until
// its capability is declared).
// 349 -> 350 (25 Sep, merging the main chat's 5dda9b2): `yourchanges`,
// hand edits to shipped config kept through an update (courier plumbing).
// 350 -> 354 (25 Sep, merging the main chat's 715b53c/6b7e938): `groups`,
// `peerkey`, `plugins` and `update_courier` (courier steps 4-5).
// 354 -> 355 (25 Sep, merging the main chat's e6975c4): `friends`.
// 355 -> 374 (25 Sep, round 11): the seventeen everyday tools --
// `marketdays`, `cliphist`, `screentext`, `mailbook`, `waitingfor`,
// `launcher`, `tradeday`, `meetprep`, `snippets`, `findfile`, `pdfkit`,
// `people`, `feeds`, `receipts`, `habits`, `srs`, `translation` -- the key
// chords (`chords`) and their wiring (`workday`), each claimed.
// 374 -> 377 (26 Sep, merging the main chat's 24bf4b4): `wire`, `portmap`
// and `mailbox`, claimed by `wire`.
// 377 -> 376 (26 Sep, merging the main chat's 03d7e7e): `portmap` and
// `mailbox` removed (friends no longer open router doors or hold each
// other's mail), `onion` added (friends reach each other through Tor),
// claimed by `wire`.
// 376 -> 393 (26 Sep, merging the third chat's line, 23e -> 25h): `hubwin`,
// `settingswin`, `overlaywin`, `webview2_loader` (the hub and settings in
// Atlas's own window), `speaking`, `callwatch`, `callrec`, `callnotes`,
// `localclock`, `picture_talk`, `next_up`, `phases`, `resume`, `twofactor`,
// `webrun`, `astype` and `platform::idle`, each claimed where it was built.
// 393 -> 400 (26 Sep, completing the three-chat merge): the third chat's
// 25i/25j (`hotkeys`, `later`, `pdftext`, `typebox`, `unpack`) and the main
// chat's update-courier step 2 (`update_apply`, `feedback`).
// 404 -> 407 (28 Sep): the constant was already one behind on merge-0928
// (405 modules in the tree), plus `mcp` (other programs' tools) and
// `himalaya` (reading mail through the Himalaya program).
// 411 -> 410 (28 Sep): `ladder` left personal Atlas; it was not general
// trading knowledge (tests/personal_atlas_is_its_own.rs).
// What is counted (29 Sep 2026, when daemon.rs and main.rs were split into
// `src/daemon/*.rs` and `src/main/*.rs`): module names as a capability names
// them -- every `.rs` under `src/` by its stem, a folder's `mod.rs` as the
// folder, `lib` and `main` left out, and the files inside a split module's
// folder (`src/<m>/` beside `src/<m>.rs`) counted as `<m>`, not as modules of
// their own. So the split changed nothing here: `daemon` is still one module
// (on PLUMBING), and `src/main/*.rs` are still the binary.
// 410 -> 413 (29 Sep): `photo`, `straighten` and `cutout` (photo editing).
// 413 -> 414 (29 Sep): `social` (its `src/social/*.rs` fold into it).
// 414 -> 418 (29 Sep): `hunt`, `hunting` (opportunity hunting), `wit`, `talkback`.
// 418 -> 420 (29 Sep): `doing`, `repeating` (Eric's evening on the laptop; plumbing).
// 420 -> 421 (29 Sep): `utterance` (the wake word and the request in one breath; claimed by `wake`).
// 421 -> 423 (30 Sep, merging the other chat's 29 Sep work): `playout` (the
// voice played inside Atlas, through the speaker it chose -- part of `audio`)
// and `winpark` (plumbing).
// 423 -> 426 (30 Sep 2026): `leveller` and `miclevel` (a quiet voice heard
// without shouting -- part of `audio`) and `camera_ask` ("can you see me?" --
// part of `vision`).
// 427 -> 430 on merging with r8-brain (router, taskloop, backed, streams).
// 430 -> 431 (30 Sep 2026): `deepbrain` (the deep model beside the talking
// one -- part of `reason`).
// 431 -> 434 (30 Sep 2026): `meaningroute` and `meaningnative` (tools by
// meaning, part of `router`), and `used` (what gets used, part of
// `selfaudit`). 434 -> 435: `imagemake` (pictures made on this machine). 435 -> 436: `selftest`. 436 -> 437: `operate`.
// Merged 30 Sep 2026 with the other chat's 30 Sep work: `freeonline`
// (the free online models, second to this machine's -- part of `reason`),
// `talkbench` (Atlas's conversation timed against a model), `parakeet`
// (hearing through sherpa-onnx with NVIDIA's Parakeet -- part of `wake`),
// `keeping` (reminders, timers, events moved -- part of `calendar`) and
// `weather` (Open-Meteo). 437 -> 442. 442 -> 443: `texting`. 443 -> 444: `speakernet` (part of `diarize`).
pub const MODULES_IN_TREE: usize = 453;

/// Every module no capability claims, and why it is not one.
///
/// Added 28 Sep 2026. Until then the tree had three kinds of module: claimed
/// by a capability, plumbing, and "built and not yet written down" -- and
/// nothing told the second from the third, so the third grew to about 130
/// modules (the video editor, the morning brief, undo, the overnight run, the
/// phone pieces) while the catalogue answered "what can you do?" without them.
/// A ratchet on the count held the number and said nothing about what was in
/// it.
///
/// Now there are two kinds and no third. `tests/catalogue.rs` fails for a
/// module that is neither claimed nor listed here, for a module that is both,
/// and for a name here that is not a module. Being listed costs a reason, so
/// a feature cannot be filed as plumbing without somebody writing down why it
/// is not one.
pub const PLUMBING: &[(&str, &str)] = &[
    ("cpuuse", "measures Atlas's own CPU while idle and where the loop's time goes, for the log and for self-repair"),
    ("b64", "base64 encoding for pictures and keys handed to other programs"),
    ("winpark", "keeps Atlas's hidden helper windows (the overlay, the typing box) from costing anything while hidden"),
    ("talkbench", "times Atlas's own conversation against a model (`atlas talk-bench`), for choosing which model this machine runs"),
    ("backends", "picks which mechanism touches a window per request; the window capabilities are what it serves"),
    ("bars", "the price-bar type and the view that cannot see the future, which every market reading is built on"),
    ("checkup", "the fast on-device self-check doctor and setup run, not something you ask for by itself"),
    ("cli", "reads flags off the command line for the terminal commands"),
    ("config", "loads the YAML every setting lives in"),
    ("daemon", "the always-on core every capability runs inside"),
    ("error", "the one error type every module returns"),
    ("fixtures", "synthetic price series the market self-check measures itself against"),
    ("gguf", "reads a model file's header so the model loader knows what it is loading"),
    ("http", "a minimal loopback HTTP client for the browser debugger and local helpers"),
    ("intent", "turns a phrase into an intent for every capability; the understanding layer, not a thing you ask for"),
    ("doing", "reads a request that doesn't start with a command's phrase as the command it means, and whether the screen was mentioned; part of the understanding layer"),
    ("repeating", "keeps a reply from saying again what Atlas already said; part of how every reply is spoken, not a thing you ask for"),
    ("judgment", "the shared scale graded judgments are expressed on, borrowed by a dozen capabilities"),
    ("lifecycle", "starts heavyweight helpers when needed and reaps them when idle"),
    ("look", "the palette and catenary constants the native windows are painted from"),
    ("look_paint", "paints Atlas's own windows natively from that palette"),
    ("mark", "Atlas's logo, drawn in code"),
    ("memory", "the five separate stores the approval gate and preferences read"),
    ("metrics", "counts tests and modules for the documents, a developer's tool"),
    ("mock", "a fake operating system for tests and dry runs; rehearse is the capability that uses it"),
    ("params", "every frozen tuning number in the market module, with its reason"),
    ("perf", "how hard the background loop works when idle or on battery, a property of every capability"),
    ("posix", "the Mac and Linux platform layer"),
    ("roots", "where this install lives on disk"),
    ("shakedown", "the commissioning pass that walks this catalogue itself and checks what it can"),
    ("tier", "decides how much work a turn gets before any capability runs"),
    ("time", "civil dates from a timestamp, with no calendar crate"),
    ("tools", "runs the external programs heavy capabilities shell out to"),
    ("unwaited", "reaps child processes nobody is waiting for"),
    ("verify", "the market module's offline self-check report, a developer's demonstration no running path calls"),
    ("win", "the Windows platform layer"),
    ("ws", "a minimal WebSocket client for the browser debugger"),
];

/// How much of the tree the catalogue accounts for.
///
/// Claimed modules against the whole tree. The difference is exactly
/// `PLUMBING`, which `tests/catalogue.rs` holds to: no module is left over.
pub fn module_coverage() -> (usize, usize) {
    (claimed_modules().len(), MODULES_IN_TREE)
}

/// Which capability a module belongs to, if any.
///
/// The direction Atlas needs when it is working on itself: it has a file open
/// and wants to know what breaks if it gets this wrong.
pub fn what_uses(module: &str) -> Vec<Capability> {
    all().into_iter().filter(|c| c.modules.contains(&module)).collect()
}
