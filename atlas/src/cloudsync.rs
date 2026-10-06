//! Setting up the folder two devices meet in.
//!
//! The cloud path is the workhorse: it doesn't need both devices on the same
//! network, or even on at the same time. One writes a bundle, the other picks
//! it up whenever it next runs.
//!
//! Atlas can do nearly all of this itself. What it can't do is sign you into
//! anything — so the split is: you install the app and sign in once, and Atlas
//! finds it, makes its folder, checks the sync is actually working, and never
//! bothers you about it again.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OneDrive,
    ICloud,
    Dropbox,
    GoogleDrive,
    /// Any folder that syncs.
    Whatever,
}

impl Provider {
    pub fn name(&self) -> &'static str {
        match self {
            Provider::OneDrive => "OneDrive",
            Provider::ICloud => "iCloud Drive",
            Provider::Dropbox => "Dropbox",
            Provider::GoogleDrive => "Google Drive",
            Provider::Whatever => "a synced folder",
        }
    }

    /// Free space you get without paying.
    pub fn free_gb(&self) -> u32 {
        match self {
            Provider::OneDrive => 5,
            Provider::ICloud => 5,
            Provider::Dropbox => 2,
            Provider::GoogleDrive => 15,
            Provider::Whatever => 0,
        }
    }

    /// Where Windows keeps it, as an environment variable or a path.
    pub fn windows_hint(&self) -> &'static str {
        match self {
            // Set by the OneDrive client itself, which is why this is
            // reliable rather than a guess at a folder name.
            Provider::OneDrive => "%OneDrive%",
            Provider::ICloud => "%USERPROFILE%/iCloudDrive",
            Provider::Dropbox => "%USERPROFILE%/Dropbox",
            Provider::GoogleDrive => "%USERPROFILE%/Google Drive",
            Provider::Whatever => "",
        }
    }

    /// How well it behaves on Windows, which is where you are.
    pub fn on_windows(&self) -> &'static str {
        match self {
            Provider::OneDrive => "built in — nothing to install, and it's already signed in if \
                                   you use a Microsoft account",
            Provider::ICloud => "works, but the Windows client is the weakest of the four",
            Provider::Dropbox => "reliable, and the quickest to actually push a change",
            Provider::GoogleDrive => "fine, though it mounts as a drive rather than a folder",
            Provider::Whatever => "whatever you point me at",
        }
    }

    /// And on the phone.
    pub fn on_ios(&self) -> &'static str {
        match self {
            Provider::OneDrive => "the app, and it shows up in Files — that's what Atlas uses",
            Provider::ICloud => "already there, nothing to install",
            Provider::Dropbox => "the app, and it appears in Files",
            Provider::GoogleDrive => "the app, and it appears in Files",
            Provider::Whatever => "it has to appear in Files",
        }
    }
}

/// Roughly what Atlas puts through it.
///
/// Worth knowing before worrying about space: the bundles are event logs,
/// which are text and tiny.
pub fn space_needed_mb(events_per_day: u32, days_kept: u32, carrying_files: bool) -> u64 {
    // An event is a couple of hundred bytes.
    let logs = (events_per_day as u64 * days_kept as u64 * 250) / 1_048_576;
    // Working-set files are the only thing with real size, and only if you
    // route them this way.
    let files = if carrying_files { 400 } else { 0 };
    logs.max(1) + files
}

/// Will the free tier do?
pub fn free_tier_is_enough(p: Provider, needed_mb: u64) -> (bool, String) {
    let free_mb = p.free_gb() as u64 * 1024;
    if needed_mb < free_mb / 4 {
        (
            true,
            format!(
                // The sentence used to stop at "and won't". Nothing called
                // this function, so nobody ever read the half of it that was
                // missing — the exact failure being unwired hides.
                "{}MB against {}GB free — this costs you nothing, and won't grow \
                 into something that does",
                needed_mb,
                p.free_gb()
            ),
        )
    } else if needed_mb < free_mb {
        (true, format!("{}MB against {}GB free — fine, but keep an eye on it", needed_mb, p.free_gb()))
    } else {
        (false, format!("{}MB needs more than the {}GB free tier", needed_mb, p.free_gb()))
    }
}

/// A step Atlas can do, or one it needs you for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    pub what: String,
    /// Atlas does it.
    pub automatic: bool,
    /// If not, why not.
    pub why_you: Option<String>,
}

/// Setting it up on the laptop.
///
/// Nearly all of it is Atlas's job. Signing in isn't, and never will be.
pub fn laptop_steps(p: Provider, already_installed: bool) -> Vec<Step> {
    let mut steps = Vec::new();

    if !already_installed {
        steps.push(Step {
            what: format!("install {} and sign in", p.name()),
            automatic: false,
            // The OneDrive aside used to be unconditional, so setting up
            // Dropbox told you about OneDrive. Invisible until something
            // actually printed these steps for a provider that isn't
            // OneDrive — and the only test passes `already_installed: true`,
            // which skips this step entirely.
            why_you: Some(match p {
                Provider::OneDrive => "I don't sign into anything — and for OneDrive there's \
                                       usually nothing to install, it's already there"
                    .to_string(),
                _ => "I don't sign into anything".to_string(),
            }),
        });
    }

    steps.push(Step {
        what: format!("find the {} folder", p.name()),
        automatic: true,
        why_you: None,
    });
    steps.push(Step {
        what: "make an Atlas folder inside it".into(),
        automatic: true,
        why_you: None,
    });
    steps.push(Step {
        what: "write a test file and watch it sync".into(),
        automatic: true,
        why_you: None,
    });
    steps.push(Step {
        what: "mark the folder as always-keep rather than online-only".into(),
        automatic: true,
        // The failure people actually hit: the folder is there, the file
        // isn't, because it was evicted to save space.
        why_you: None,
    });
    steps
}

/// And on the phone.
pub fn phone_steps(p: Provider) -> Vec<Step> {
    vec![
        Step {
            what: format!("install {} and sign in", p.name()),
            automatic: false,
            why_you: Some("signing in is yours".into()),
        },
        Step {
            what: "turn it on in Files so Atlas can see it".into(),
            automatic: false,
            why_you: Some("iOS only lets the app itself do that".into()),
        },
        Step {
            what: "point Atlas at the folder".into(),
            automatic: true,
            why_you: None,
        },
        Step {
            what: "send a test bundle and confirm it lands".into(),
            automatic: true,
            why_you: None,
        },
    ]
}

/// Where Atlas looks on Windows, in order.
///
/// The environment variable first, because the client sets it and it's right
/// even when the folder has been moved or renamed.
pub fn where_to_look(p: Provider) -> Vec<&'static str> {
    match p {
        Provider::OneDrive => vec![
            "%OneDrive%",
            "%OneDriveConsumer%",
            "%OneDriveCommercial%",
            "%USERPROFILE%/OneDrive",
        ],
        Provider::ICloud => vec!["%USERPROFILE%/iCloudDrive", "%USERPROFILE%/iCloud Drive"],
        Provider::Dropbox => vec!["%USERPROFILE%/Dropbox"],
        Provider::GoogleDrive => vec!["%USERPROFILE%/Google Drive", "G:/My Drive"],
        Provider::Whatever => vec![],
    }
}

/// Things that go wrong, and what they look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trouble {
    /// The folder exists but the file never appears on the other side.
    NotActuallySyncing,
    /// Files present as placeholders and vanish when offline.
    OnlineOnly,
    /// Full.
    OutOfSpace,
    /// Signed out.
    SignedOut,
    /// The provider renamed or moved the folder.
    Moved,
}

impl Trouble {
    pub fn looks_like(&self) -> &'static str {
        match self {
            Trouble::NotActuallySyncing => "the test file is still only on this machine",
            Trouble::OnlineOnly => "the file is there but it's a placeholder — nothing in it",
            Trouble::OutOfSpace => "writes are failing",
            Trouble::SignedOut => "the folder is there but nothing moves",
            Trouble::Moved => "the folder isn't where it was",
        }
    }

    pub fn fix(&self) -> &'static str {
        match self {
            Trouble::NotActuallySyncing => "check the client is running — it's the tray icon",
            // This one catches people constantly: the file is "there" and
            // useless offline, which is exactly when you need it.
            Trouble::OnlineOnly => "I'll mark the folder always-keep, which fixes it for good",
            Trouble::OutOfSpace => "clear some space, or point me at a different folder",
            Trouble::SignedOut => "sign back in — that one's yours",
            Trouble::Moved => "I'll find it again",
        }
    }

    pub fn atlas_can_fix(&self) -> bool {
        matches!(self, Trouble::OnlineOnly | Trouble::Moved)
    }
}

/// Every trouble, in the order worth reading them: the common ones first.
const TROUBLES: [Trouble; 5] = [
    Trouble::NotActuallySyncing,
    Trouble::OnlineOnly,
    Trouble::OutOfSpace,
    Trouble::SignedOut,
    Trouble::Moved,
];

/// The real providers, for a side-by-side. `Whatever` is left out on purpose:
/// it is the "point me at any synced folder" escape hatch, not a thing to
/// compare a free tier or a Windows path against.
const PROVIDERS: [Provider; 4] = [
    Provider::OneDrive,
    Provider::ICloud,
    Provider::Dropbox,
    Provider::GoogleDrive,
];

/// Which side of the pair you are setting up.
///
/// The laptop and the phone are genuinely different jobs — one has a Windows
/// path and a folder Atlas can find; the other only lets the app itself turn
/// sync on — so the guidance forks on this rather than trying to say both at
/// once.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Setup {
    Laptop,
    Phone,
}

/// The four providers side by side: free space, how each behaves on Windows,
/// where Windows keeps it, and what the phone needs.
///
/// The comparison a person asks for before they pick one, built from the same
/// per-provider readings the setup path uses rather than a second opinion.
pub fn compare_providers() -> String {
    let mut out = String::from("How the sync providers compare\n");
    for p in PROVIDERS {
        out.push_str(&format!("\n{} ({}GB free)\n", p.name(), p.free_gb()));
        out.push_str(&format!("  Windows: {}\n", p.on_windows()));
        let hint = p.windows_hint();
        if !hint.is_empty() {
            out.push_str(&format!("  Windows path: {}\n", hint));
        }
        out.push_str(&format!("  Phone: {}\n", p.on_ios()));
    }
    out
}

/// How to set sync up on one provider, for the laptop or the phone.
///
/// Reads the same per-provider notes as [`compare_providers`], adds the steps
/// (marking which are Atlas's and which are yours), and ends with the
/// troubles worth knowing about — each with what it looks like, the fix, and
/// whether Atlas can do that fix itself.
pub fn setup_guidance(p: Provider, device: Setup) -> String {
    let mut out = format!("Setting up sync through {} ({}GB free)\n", p.name(), p.free_gb());
    match device {
        Setup::Laptop => {
            out.push_str(&format!("  On Windows: {}\n", p.on_windows()));
            let hint = p.windows_hint();
            if !hint.is_empty() {
                out.push_str(&format!("  I look for it at {} first.\n", hint));
            }
            out.push_str("  Steps:\n");
            for s in laptop_steps(p, false) {
                let who = if s.automatic { "me" } else { "you" };
                out.push_str(&format!("    [{who}] {}\n", s.what));
            }
        }
        Setup::Phone => {
            out.push_str(&format!("  On the phone: {}\n", p.on_ios()));
            out.push_str("  Steps:\n");
            for s in phone_steps(p) {
                let who = if s.automatic { "me" } else { "you" };
                out.push_str(&format!("    [{who}] {}\n", s.what));
            }
        }
    }
    out.push_str("  If it stops working:\n");
    for t in TROUBLES {
        let who = if t.atlas_can_fix() { "I fix this" } else { "needs you" };
        out.push_str(&format!("    {} — {} ({who})\n", t.looks_like(), t.fix()));
    }
    out
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CloudConfig {
    pub enabled: bool,
    pub provider: String,
    /// Found or set at setup.
    pub folder: String,
    /// Check it's really syncing this often.
    pub check_every_hours: u32,
    /// Send working-set files through it as well as event logs.
    pub carry_files: bool,
    // `encrypt_before_writing` was here until 18 Sep 2026: `#[serde(skip)]`,
    // pinned true, read by nothing, and stating a guarantee the tree did not
    // keep -- bundles were written in the clear. It is gone rather than
    // wired, because the real switch belongs where the writing happens:
    // `sync.encrypt_bundles`, which ships off and says what it costs. A
    // second switch here would be two answers to one question.
}

impl Default for CloudConfig {
    fn default() -> Self {
        CloudConfig {
            enabled: false,
            provider: "onedrive".into(),
            folder: String::new(),
            check_every_hours: 12,
            carry_files: true,
        }
    }
}

/// Is the sync folder still doing its job? Checked every
/// `check_every_hours` by the running Atlas (it used to be checked once, at
/// setup, and never again).
///
/// What can be seen from this side: the folder is still there; a file can
/// still be written into it; and — the one that catches a signed-out or
/// stopped client — whether anything new has arrived from your other devices
/// lately. `mine` is this device's own bundle name, which doesn't count;
/// `expect_others` is whether any other device has ever been seen (a folder
/// with one device in it has nothing to arrive).
pub fn still_syncing(folder: &std::path::Path, mine: &str, expect_others: bool, now: u64, cfg: &CloudConfig) -> Option<(Trouble, String)> {
    if !folder.is_dir() {
        return Some((Trouble::Moved, format!("the sync folder {} isn't there any more", folder.display())));
    }
    let probe = folder.join(".atlas-can-write");
    if std::fs::write(&probe, now.to_string()).is_err() {
        return Some((Trouble::OutOfSpace, format!("I can't write into {} — full, or its permissions changed", folder.display())));
    }
    crate::heard!(std::fs::remove_file(&probe));
    if !expect_others {
        return None;
    }
    let newest = std::fs::read_dir(folder)
        .ok()?
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "bundle") && e.file_name().to_string_lossy() != mine)
        .filter_map(|e| e.metadata().ok()?.modified().ok())
        .filter_map(|t| t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs()))
        .max();
    // Twice the check interval, so one quiet stretch on the other machine
    // (asleep, on a plane) isn't reported as a broken folder.
    let window = 2 * cfg.check_every_hours.max(1) as u64 * 3600;
    match newest {
        None => Some((Trouble::NotActuallySyncing, "nothing from your other devices has ever arrived in the folder".into())),
        Some(t) if now.saturating_sub(t) > window => Some((
            Trouble::NotActuallySyncing,
            format!("nothing new from your other devices in {} hours", now.saturating_sub(t) / 3600),
        )),
        _ => None,
    }
}

/// What Atlas says while setting it up.
pub fn setting_up(p: Provider, found_at: Option<&str>) -> String {
    match found_at {
        Some(path) => format!(
            "Found {} at {path}. Making a folder and testing it — give me a minute.",
            p.name()
        ),
        None => format!(
            "I can't find {}. Once it's installed and signed in I'll pick it up by myself — \
             there's nothing for you to point me at.",
            p.name()
        ),
    }
}

/// After the test.
pub fn result(worked: bool, seconds: u64, p: Provider) -> String {
    if worked {
        format!(
            "{} is working — the test file came back in {seconds} seconds. That's the path your \
             phone and laptop will use when they're not on the same wifi.",
            p.name()
        )
    } else {
        format!("The test file didn't come back. {}", Trouble::NotActuallySyncing.fix())
    }
}

/// Why this one rather than the others, for a Windows laptop and an iPhone.
///
/// This used to end "switching later costs nothing because I encrypt before
/// anything is written either way." **That was not true**, and it was the
/// single most consequential false sentence in the tree: sync bundles are
/// written by `Daemon::carry_to_your_other_devices` as
/// `serde_json::to_string_pretty`, in the clear, into a folder that a cloud
/// provider then copies to their servers. Everything a bundle carries —
/// captured notes, things said — sat readable in OneDrive while this page
/// told you it did not.
///
/// `CloudConfig::encrypt_before_writing` is the setting that was supposed to
/// be behind it. It is `#[serde(skip)]`, pinned true, and read by nothing.
/// It stays on `DEAD_IN_WIRED` with that as its reason rather than being
/// deleted like its neighbours, because deleting it would leave nothing in
/// the tree saying this is missing.
///
/// See `BUNDLES_ARE_NOT_ENCRYPTED` below for what is said to the user now,
/// and `tests/settings_that_do_something_now.rs` for the guard that keeps
/// this page and the writing path agreeing.
pub const WHY_ONEDRIVE: &str =
    "For a Windows laptop it's the least work: it's already in Windows, and if you sign into the \
     machine with a Microsoft account it's already signed in. The free 5GB is far more than this \
     needs — the bundles are text. Dropbox pushes changes a bit faster if that ever matters, and \
     switching later costs nothing because a bundle is a plain file either way.";

/// What a bundle is, said at the moment you are choosing where to put them.
///
/// This page used to end by claiming bundles were encrypted. They were not,
/// and until 18 Sep 2026 nothing in the tree could make that true. Sealing
/// exists now and **ships off**, so the honest sentence is neither the old
/// claim nor the flat "not built" that replaced it for an afternoon: it is
/// what the setting does, what it costs, and how to turn it on.
pub const ABOUT_BUNDLES: &str =
    "One thing to know before you point me at a synced folder. A bundle carries what you \
     captured and what was said — never a password and never a vault entry, because the vault \
     does not leave this machine. By default it is plain text, so anyone who can read that \
     folder can read those. Turn on \"Seal what I carry between devices\" in the hub and I \
     make a key myself, keep it on each device, and write it down for you. If every copy of \
     it is ever lost, one button makes a new one and nothing is lost: a bundle is a courier, \
     not where your notes live, and every bundle carries the whole record from the beginning.";

/// The limit of what Atlas will do here.
pub const WHAT_YOU_DO: &str =
    "I can install it and sign you in from the vault, the same as any other site. Everything \
     after that is mine too: finding the folder, making my own inside it, testing that it really \
     syncs, and keeping it always-available so it works when you're offline. The only thing I \
     leave to you is the first time you set the vault passphrase.";
