//! What runs where, and what doesn't.
//!
//! Written because "it's Rust so it's portable" is the answer that gets people
//! into trouble. The logic is portable — every module that decides something
//! compiles anywhere. What isn't portable is everything that touches a machine:
//! moving a window, reading the screen, hearing you, protecting a secret.
//!
//! The useful question isn't "does it run" but "what does it do once it's
//! running", and the answers differ enough to be worth stating per platform
//! rather than as a single yes.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Windows,
    Mac,
    Linux,
    Ios,
    Android,
    /// A browser, on anything.
    Web,
}

impl Platform {
    pub fn name(&self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Mac => "a Mac",
            Platform::Linux => "Linux",
            Platform::Ios => "an iPhone or iPad",
            Platform::Android => "Android",
            Platform::Web => "a browser",
        }
    }

    /// The name with no article, for the places a sentence needs one word.
    ///
    /// `name` is written to follow "on" — "on an iPhone or iPad" — and reads
    /// wrong anywhere else ("the an iPhone half isn't written").
    pub fn bare(&self) -> &'static str {
        match self {
            Platform::Windows => "Windows",
            Platform::Mac => "macOS",
            Platform::Linux => "Linux",
            Platform::Ios => "iOS",
            Platform::Android => "Android",
            Platform::Web => "the web",
        }
    }
}

/// The things Atlas does that need the machine's cooperation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Move and size windows.
    Windows_,
    /// Read what's on screen.
    ReadScreen,
    /// Act inside another app.
    ActInApps,
    /// Open another app at all.
    ///
    /// Separate from `ActInApps` because the two come apart on a phone: iOS
    /// will let one app bring another to the front and will never let it
    /// touch what's inside. Folding them together would report opening an app
    /// as impossible there, which is the same lie as the other way round.
    LaunchApps,
    /// Hear a wake word while closed.
    WakeWord,
    /// Record and play audio.
    Audio,
    /// Look through a camera.
    Camera,
    /// Protect a secret so a stolen file is useless.
    RealEncryption,
    /// Run while you're not looking at it.
    Background,
    /// Read your files.
    Files,
    /// The parts that are only arithmetic.
    JustThinking,
}

impl Needs {
    /// Every one of them, in one place.
    ///
    /// The coverage count and the catalogue both walk this, so a need that
    /// exists but isn't listed here is a need nothing reports on. `plain`
    /// below is an exhaustive match, so adding a variant stops the crate
    /// compiling until it is named — and `tests/portable.rs` pins the length
    /// so it also has to be added here.
    pub const EVERY: &'static [Needs] = &[
        Needs::Windows_,
        Needs::ReadScreen,
        Needs::ActInApps,
        Needs::LaunchApps,
        Needs::WakeWord,
        Needs::Audio,
        Needs::Camera,
        Needs::RealEncryption,
        Needs::Background,
        Needs::Files,
        Needs::JustThinking,
    ];

    /// What it is, in the words you'd use asking for it.
    pub fn plain(&self) -> &'static str {
        match self {
            Needs::Windows_ => "moving your windows",
            Needs::ReadScreen => "reading your screen",
            Needs::ActInApps => "doing things inside other apps",
            Needs::LaunchApps => "opening your apps",
            Needs::WakeWord => "hearing you without opening it",
            Needs::Audio => "listening and talking",
            Needs::Camera => "looking through a camera",
            Needs::RealEncryption => "keeping a secret a stolen file can't give up",
            Needs::Background => "running while you're not looking",
            Needs::Files => "reading your files",
            Needs::JustThinking => "working things out",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// Works, and is written.
    Built,
    /// The platform allows it; Atlas hasn't written it yet.
    Possible,
    /// The platform allows it only with a compromise worth knowing about.
    Awkward,
    /// The platform doesn't permit it. No amount of work changes this.
    Never,
}

impl How {
    pub fn plain(&self) -> &'static str {
        match self {
            How::Built => "works",
            How::Possible => "would work — not written yet",
            How::Awkward => "possible, with a catch",
            How::Never => "not allowed on this platform",
        }
    }

    /// Is this work, or a wall?
    ///
    /// The distinction that matters when someone asks "can you make it do X" —
    /// on three of these the answer is time, and on one it's no.
    pub fn is_effort_not_a_wall(&self) -> bool {
        !matches!(self, How::Never)
    }
}

/// What each platform allows.
pub fn how(p: Platform, n: Needs) -> How {
    use How::*;
    use Needs::*;
    use Platform::*;

    match (p, n) {
        // Everything that is only arithmetic runs everywhere. That's most of
        // the modules and all of the judgement.
        (_, JustThinking) => Built,

        (Windows, Windows_) => Built,
        (Windows, ReadScreen) => Built,
        (Windows, ActInApps) => Built,
        (Windows, LaunchApps) => Built,
        (Windows, WakeWord) => Built,
        (Windows, Audio) => Built,
        (Windows, Camera) => Built,
        (Windows, RealEncryption) => Built,
        (Windows, Background) => Built,
        (Windows, Files) => Built,

        // A Mac permits all of it. The screen and accessibility parts need
        // permissions the user grants once, and Keychain is the equivalent of
        // DPAPI.
        //
        // Launching, closing and files are Built rather than Possible: they
        // are written, in platform/posix.rs, and work today. Claiming
        // otherwise would be the same understatement as the old code refusing
        // to do them at all.
        (Mac, Files) => Built,
        (Mac, LaunchApps) => Built,
        (Mac, ReadScreen) | (Mac, ActInApps) => Awkward,
        (Mac, Camera) => Awkward,
        (Mac, _) => Possible,

        // Linux is fine except that window management depends on which
        // display server is running, and there are two.
        (Linux, Files) => Built,
        (Linux, LaunchApps) => Built,
        // The camera is opened by an ffmpeg line that lives in the config, so
        // the only per-platform part is which input it names. That makes it
        // written here in the same sense it is written on Windows.
        (Linux, Camera) => Built,
        (Linux, Windows_) | (Linux, ReadScreen) => Awkward,
        (Linux, RealEncryption) => Awkward,
        (Linux, _) => Possible,

        // The ones that are walls rather than work.
        (Ios, WakeWord) => Never,
        (Ios, ReadScreen) => Never,
        (Ios, ActInApps) => Never,
        (Ios, Windows_) => Never,
        (Ios, LaunchApps) => Awkward,
        (Ios, Background) => Awkward,
        (Ios, Files) => Awkward,
        // Allowed, and not written: the camera is reachable only through the
        // platform's own capture API, not by running ffmpeg, which is how
        // every desktop does it here.
        (Ios, Camera) => Possible,
        (Ios, _) => Possible,

        (Android, Windows_) => Never,
        (Android, WakeWord) => Possible,
        (Android, ReadScreen) => Awkward,
        (Android, ActInApps) => Awkward,
        (Android, _) => Possible,

        (Web, Windows_) | (Web, ReadScreen) | (Web, ActInApps) => Never,
        (Web, WakeWord) | (Web, Background) => Never,
        (Web, LaunchApps) => Never,
        (Web, RealEncryption) => Never,
        (Web, Camera) => Awkward,
        (Web, Files) => Awkward,
        (Web, _) => Possible,
    }
}

/// Why, where the answer isn't obvious.
pub fn because(p: Platform, n: Needs) -> Option<&'static str> {
    use Needs::*;
    use Platform::*;
    Some(match (p, n) {
        (Ios, WakeWord) => "an app that isn't in front of you gets no microphone. This is the \
                            platform's decision and it isn't going to change",
        (Ios, ReadScreen) | (Ios, ActInApps) => "one app cannot see or touch another. Shortcuts \
                                                 is the sanctioned way round it and it only does \
                                                 what each app chose to expose",
        (Ios, Background) => "a few minutes after you switch away, then it's suspended",
        (Ios, Windows_) | (Android, Windows_) => "there are no windows to arrange — one app fills \
                                                  the screen, and the system decides when two \
                                                  share it",
        (Ios, Files) => "only what you hand it, or what it wrote itself. There is no walking your \
                         folders",
        (Ios, LaunchApps) => "only apps that published a way in, and only to the front of them — \
                              nothing chosen off a list of what is installed",
        (Mac, ReadScreen) | (Mac, ActInApps) => "needs Screen Recording and Accessibility \
                                                 permission, granted once in System Settings",
        (Mac, Camera) => "camera permission is granted to whatever launched Atlas, so the prompt \
                          names that rather than Atlas",
        (Ios, Camera) => "the camera is there, but only through the platform's capture API — the \
                          ffmpeg line every desktop uses doesn't exist on a phone",
        (Web, Camera) => "works with permission, and only while the page is open",
        (Linux, Windows_) => "X11 and Wayland need different code, and Wayland deliberately \
                              restricts what one app can do to another",
        (Linux, RealEncryption) => "no single equivalent of DPAPI — it depends on which keyring \
                                    is installed",
        (Android, ReadScreen) | (Android, ActInApps) => "an accessibility service, which the \
                                                         user turns on knowing what it means",
        (Web, RealEncryption) => "nothing in a browser can protect a secret from the browser",
        (Web, _) => "a page only sees itself",
        _ => return None,
    })
}

/// How much of Atlas works on a platform, as a fraction.
///
/// Counting what's allowed rather than what's written, since the second is a
/// matter of time and the first isn't.
pub fn coverage(p: Platform) -> (usize, usize) {
    let all = Needs::EVERY;
    let allowed = all.iter().filter(|n| how(p, **n).is_effort_not_a_wall()).count();
    (allowed, all.len())
}

/// What to tell someone before they install it.
pub fn honest_summary(p: Platform) -> String {
    let (allowed, all) = coverage(p);
    // Walked rather than hand-listed: a wall this function doesn't know about
    // is a wall nobody is told about, which is the one failure the whole
    // module exists to prevent.
    let walls: Vec<&str> = Needs::EVERY
        .iter()
        .filter(|n| how(p, **n) == How::Never)
        .map(|n| n.plain())
        .collect();

    let mut out = format!("On {}, {allowed} of {all} work.", p.name());
    if walls.is_empty() {
        out.push_str(" Nothing is off the table — the rest is time.");
    } else {
        out.push_str(&format!(" Not possible at all: {}.", walls.join(", ")));
    }
    out
}

/// The part that is the same everywhere, and it's most of it.
pub const WHAT_TRAVELS: &str =
    "Every module that decides something — what's outstanding, what to say, what a message is \
     asking, what a claim is worth keeping — is arithmetic and runs anywhere. What doesn't travel \
     is the layer that touches a machine, which is about a dozen files. Handing this to someone \
     on a Mac means writing that layer, not rewriting Atlas.";

/// What to hand a friend.
pub fn for_a_friend(p: Platform) -> String {
    match p {
        Platform::Windows => "the same thing you're running.".into(),
        Platform::Mac | Platform::Linux => format!(
            "everything that thinks, and none of the machine control until that layer is \
             written for {}. It's about a dozen files, not a rewrite.",
            p.name()
        ),
        Platform::Ios | Platform::Android | Platform::Web => format!(
            "the phone shape rather than the desktop one — it asks, it captures, it syncs. On \
             {} it can't watch your screen or move your windows, so pretending otherwise would \
             just disappoint them.",
            p.name()
        ),
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PortableConfig {
    /// Say what won't work here before it's needed rather than after.
    pub warn_up_front: bool,
}

impl Default for PortableConfig {
    fn default() -> Self {
        PortableConfig { warn_up_front: true }
    }
}
