//! What Atlas can do on Android.
//!
//! Worth stating up front because it surprises people: **Android lets Atlas do
//! most of what the iPhone can't.** It can listen for a wake word in the
//! background, be the assistant you get when you hold the home button, read
//! the screen, and act in other apps.
//!
//! That's a genuinely different product on the same idea, and if you're handing
//! this to friends it's worth knowing that the Android ones get the better
//! version.

use serde::{Deserialize, Serialize};

pub use crate::ios::Can;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ability {
    pub what: &'static str,
    pub can: Can,
    pub detail: &'static str,
    /// Needs a permission the user has to grant deliberately, in Settings
    /// rather than a popup.
    pub needs_deliberate_permission: bool,
}

pub fn abilities() -> Vec<Ability> {
    use Can::*;
    vec![
        Ability { what: "wake on a word with the app closed", can: Yes,
            detail: "a foreground service can listen all day. This is the big one iOS won't do — \
                     on Android it works exactly like the laptop",
            needs_deliberate_permission: false },
        Ability { what: "be the assistant on the home button", can: Yes,
            detail: "Android lets you choose your default assistant, so Atlas can replace Google \
                     Assistant outright",
            needs_deliberate_permission: true },
        Ability { what: "hear you and transcribe", can: Yes,
            detail: "whisper on the phone, offline", needs_deliberate_permission: false },
        Ability { what: "talk back", can: Yes,
            detail: "system voices, free", needs_deliberate_permission: false },
        Ability { what: "run continuously", can: Yes,
            detail: "a foreground service with a persistent notification — Android's rule is that \
                     you must be able to see it's running, which is fair",
            needs_deliberate_permission: false },

        Ability { what: "read what's on screen", can: Limited,
            detail: "through the accessibility service. Powerful, and the permission is a serious \
                     one — Android makes you turn it on in Settings and warns you properly",
            needs_deliberate_permission: true },
        Ability { what: "tap and type in other apps", can: Limited,
            detail: "same permission. This is how Atlas could actually drive an app on a phone, \
                     which iOS has no equivalent of at all",
            needs_deliberate_permission: true },
        Ability { what: "read your notifications", can: Limited,
            detail: "with the notification-access permission — so it can tell a brand email from \
                     a group chat without opening either",
            needs_deliberate_permission: true },

        Ability { what: "reach your files properly", can: Yes,
            detail: "a real file system, not a sandbox — the same working-set idea as the laptop",
            needs_deliberate_permission: true },
        Ability { what: "scan a document", can: Yes,
            detail: "ML Kit does the edge detection and text, free and offline. Slightly behind \
                     Apple's but not by much",
            needs_deliberate_permission: false },
        Ability { what: "receive anything shared to it", can: Yes,
            detail: "and register as a target for specific types, so it appears where it should",
            needs_deliberate_permission: false },
        Ability { what: "a quick settings tile", can: Yes,
            detail: "pull down and tap, from any app or the lock screen",
            needs_deliberate_permission: false },
        Ability { what: "home screen widgets", can: Yes,
            detail: "including buttons that do things directly",
            needs_deliberate_permission: false },
        Ability { what: "send a message on your behalf", can: Limited,
            detail: "technically yes, and it still asks — that's a choice, not a limit",
            needs_deliberate_permission: true },
        Ability { what: "sync over wifi, cloud, cable or a private network", can: Yes,
            detail: "all four, same as the laptop", needs_deliberate_permission: false },
        Ability { what: "AirDrop", can: Never,
            detail: "Apple only. Nearby Share is the equivalent and works Android to Android",
            needs_deliberate_permission: false },
        Ability { what: "hold your credentials", can: Never,
            detail: "same choice as on iOS — a phone is what you lose",
            needs_deliberate_permission: false },
    ]
}

/// The permissions worth being careful about.
///
/// Accessibility on Android is close to root for the user interface. It's the
/// right tool for what it does and it deserves to be asked for once, clearly,
/// with what it's for — not buried in an onboarding flow.
pub fn serious_permissions() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "accessibility service",
            "read the screen, and tap and type in other apps",
            "this is the most powerful permission on the phone. Grant it if you want Atlas to \
             actually do things in apps; leave it off and everything else still works",
        ),
        (
            "notification access",
            "see notifications as they arrive",
            "lets Atlas tell a brand email from a group chat without opening either",
        ),
        (
            "all files access",
            "reach your files the way the laptop does",
            "Android will warn you about this one, and it's right to",
        ),
        (
            "battery optimisation off",
            "keep listening without Android killing it",
            "needed for the wake word. It costs battery and Atlas should say how much",
        ),
    ]
}

/// How much it costs to leave the wake word on.
///
/// Worth being honest rather than discovering it as a flat battery.
pub fn wake_word_battery_percent_per_hour() -> f32 {
    // A small always-on model on a modern phone. Real, not nothing.
    1.5
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AndroidConfig {
    pub enabled: bool,
    pub model: String,
    // The four below stopped being settable on 19 Sep 2026, and the reason is
    // not that they are dangerous — it is that **there is no Android build**.
    // `atlas catalog --platform android` says so plainly. A person could set
    // `always_listening: true` in `tools.yaml` and would get nothing, and
    // would keep getting nothing until an app exists.
    //
    // They are not deleted, because each records a decision made before the
    // thing was built and worth keeping: everything powerful is off until
    // chosen, and this is where that was decided. `#[serde(skip)]` is what
    // turns a switch that does nothing into a specification, and
    // `PROMISES_ABOUT_WHAT_IS_NOT_BUILT` in `tests/dead_config.rs` is where
    // the tree records exactly that.
    //
    /// Listen for the wake word all the time.
    #[serde(skip)]
    pub always_listening: bool,
    /// Use the accessibility service.
    #[serde(skip)]
    pub can_act_in_apps: bool,
    /// Read notifications.
    #[serde(skip)]
    pub read_notifications: bool,
    /// Be the assistant on the home button.
    #[serde(skip)]
    pub replace_assistant: bool,
    /// Never holds credentials. Not configurable.
    #[serde(skip, default = "never")]
    pub holds_credentials: bool,
}

fn never() -> bool {
    false
}

impl Default for AndroidConfig {
    fn default() -> Self {
        AndroidConfig {
            enabled: false,
            model: "small".into(),
            // Off by default despite being possible: it costs battery and
            // people should choose it rather than find it.
            always_listening: false,
            can_act_in_apps: false,
            read_notifications: false,
            replace_assistant: false,
            holds_credentials: false,
        }
    }
}

/// The difference from iOS, said in one line.
pub const VERSUS_IOS: &str =
    "On Android, Atlas can listen for a wake word with the app closed, be the assistant on the \
     home button, read the screen and act in other apps. iOS allows none of those. Everything \
     else is about the same — so if a friend has an Android, they get the closer-to-the-laptop \
     version.";

/// What Android asks of you that iOS doesn't.
pub const THE_TRADE: &str =
    "The power comes with permissions that are genuinely powerful. The accessibility service is \
     close to full control of the interface — worth granting deliberately, once, knowing what it \
     is, and worth leaving off if you only want Atlas to talk and remember.";
