//! Changing your machine.
//!
//! Arranging the desktop, moving files where they belong, setting a wallpaper,
//! changing a setting when you ask or when Atlas needs it to work.
//!
//! The thing that makes this safe isn't a list of allowed actions — it's
//! sorting every change by **how hard it is to undo**. A wallpaper is nothing:
//! Atlas records the old one and can put it back. A moved file is undoable,
//! because it goes through the trash. A network or account setting is not, and
//! Atlas doesn't touch those at all.
//!
//! Reversibility decides the gate, not how impressive the action sounds.

use crate::policy::Decision;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Change {
    /// Trivially reversible — the old one is remembered.
    Wallpaper { path: String },
    Theme { dark: bool },
    /// Where icons sit, which monitor is primary for the taskbar.
    DesktopLayout { description: String },
    /// Moving a file goes through the trash, so it can be put back.
    MoveFile { from: String, to: String },
    RenameFile { from: String, to: String },
    CreateFolder { path: String },
    /// Overwriting something that already exists.
    Replace { path: String },
    Delete { path: String },
    /// Emptying the recycle bin, or Atlas's own trash.
    PurgeTrash,
    /// A Windows setting Atlas is allowed to touch.
    Setting { name: String, value: String },
    /// Something Atlas has decided it needs — a folder for its own files, a
    /// firewall rule for its own port.
    ForItself { what: String, reversible: bool },
}

/// How hard it is to take back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Undo {
    /// Atlas remembers the old value and can restore it instantly.
    Instant,
    /// Recoverable from the trash.
    FromTrash,
    /// Possible, but you'd have to do it by hand.
    ByHand,
    /// Gone.
    Never,
}

/// Settings Atlas will never change, whatever you say.
///
/// Not because they're difficult — because a voice assistant that can turn off
/// your firewall is a worse thing to own than one that can't. Say the word and
/// Atlas will open the settings page for you instead.
pub const NEVER: &[&str] = &[
    "firewall", "defender", "antivirus", "smart app control", "uac",
    "user account control", "bitlocker", "encryption", "windows update",
    "account", "password", "pin", "sign-in", "signin", "family", "parental",
    "network adapter", "vpn", "proxy", "dns", "registry", "group policy",
    "administrator", "elevation", "driver", "bios", "secure boot",
];

/// Settings that are safe, obvious and easy to put back.
pub const SAFE_SETTINGS: &[&str] = &[
    "wallpaper", "background", "theme", "dark mode", "light mode", "accent colour",
    "accent color", "night light", "volume", "brightness", "notification",
    "focus assist", "do not disturb", "taskbar", "mouse speed", "scroll",
];

pub fn reversibility(c: &Change) -> Undo {
    match c {
        Change::Wallpaper { .. } | Change::Theme { .. } | Change::DesktopLayout { .. } => {
            Undo::Instant
        }
        Change::MoveFile { .. } | Change::RenameFile { .. } | Change::Replace { .. } => {
            Undo::FromTrash
        }
        Change::CreateFolder { .. } => Undo::ByHand,
        Change::Delete { .. } => Undo::FromTrash,
        Change::PurgeTrash => Undo::Never,
        Change::Setting { .. } => Undo::ByHand,
        Change::ForItself { reversible, .. } => {
            if *reversible {
                Undo::Instant
            } else {
                Undo::ByHand
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Do it, and say what you did.
    Go { decision: Decision, undo: Undo },
    /// Not this one, with the reason and what to do instead.
    Refuse(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SystemConfig {
    pub enabled: bool,
    /// Folders Atlas may move files into or out of.
    pub file_roots: Vec<String>,
    /// Let Atlas set up things it needs for itself without asking.
    pub may_prepare_itself: bool,
}

impl Default for SystemConfig {
    fn default() -> Self {
        SystemConfig {
            enabled: false,
            file_roots: vec![
                "%USERPROFILE%/Desktop".into(),
                "%USERPROFILE%/Documents".into(),
                "%USERPROFILE%/Downloads".into(),
                "%USERPROFILE%/Pictures".into(),
            ],
            may_prepare_itself: true,
        }
    }
}

/// The gate.
pub fn judge(c: &Change, cfg: &SystemConfig) -> Verdict {
    if !cfg.enabled {
        // Names the switch. Every refusal in this file should leave you able
        // to act on it -- "I can't" with no route forward is what makes a
        // careful assistant read as a broken one.
        return Verdict::Refuse(
            "changing things on the machine is switched off. Set `system.enabled: true` in \
             config/tools.yaml if you want me moving and renaming files."
                .into(),
        );
    }

    // The permanent no-list first, before anything else is considered.
    if let Change::Setting { name, .. } = c {
        let n = name.to_lowercase();
        if let Some(word) = NEVER.iter().find(|w| n.contains(**w)) {
            return Verdict::Refuse(format!(
                "I don't touch anything to do with {word}. I'll open the settings page for you."
            ));
        }
        if !SAFE_SETTINGS.iter().any(|s| n.contains(s)) {
            // Fails closed. An unrecognised setting could be anything.
            return Verdict::Refuse(format!(
                "I only change appearance and notification settings, not \"{name}\"."
            ));
        }
    }

    // Files have to be somewhere Atlas is allowed to work.
    for path in paths_touched(c) {
        if !within_roots(&path, &cfg.file_roots) {
            // Names the fix, not just the fault. "I can't" without "and here
            // is how you'd let me" is a dead end that makes Atlas look broken
            // when it is actually being careful.
            return Verdict::Refuse(format!(
                "{path} is outside the folders I work in. I only touch what's listed under \
                 `system.file_roots` in config/tools.yaml — add that folder there if you want \
                 me working in it."
            ));
        }
    }

    let undo = reversibility(c);
    let decision = match (c, undo) {
        // Trivially reversible: do it and mention it.
        (_, Undo::Instant) => Decision::ProceedAndReport,
        // Recoverable, but it moved your things. Say so.
        (_, Undo::FromTrash) => Decision::ProceedAndReport,
        (Change::CreateFolder { .. }, _) => Decision::ProceedAndReport,
        // A setting you'd have to undo by hand is worth a yes first.
        (Change::Setting { .. }, _) => Decision::RequireApproval,
        (Change::ForItself { .. }, _) if cfg.may_prepare_itself => Decision::ProceedAndReport,
        (_, Undo::Never) => Decision::RequireApproval,
        _ => Decision::RequireApproval,
    };
    Verdict::Go { decision, undo }
}

fn paths_touched(c: &Change) -> Vec<String> {
    match c {
        Change::MoveFile { from, to } | Change::RenameFile { from, to } => {
            vec![from.clone(), to.clone()]
        }
        Change::CreateFolder { path } | Change::Delete { path } | Change::Replace { path } => {
            vec![path.clone()]
        }
        // A wallpaper is read, not written, so it may come from anywhere.
        _ => Vec::new(),
    }
}

fn within_roots(path: &str, roots: &[String]) -> bool {
    let p = crate::doctor::expand_env(path).replace('\\', "/").to_lowercase();
    let profile = crate::doctor::lookup_env("USERPROFILE")
        .or_else(|| crate::doctor::lookup_env("HOME"))
        .map(|h| h.replace('\\', "/").to_lowercase().trim_end_matches('/').to_string())
        .unwrap_or_default();
    roots.iter().any(|r| {
        // `%USERPROFILE%` where Windows' own name for home isn't set (the
        // same folder, `HOME`, on the other systems).
        let r = crate::doctor::expand_env(r).replace('\\', "/").to_lowercase().replace("%userprofile%", &profile);
        if r.is_empty() {
            return false;
        }
        // Windows moves Desktop, Documents and Pictures into OneDrive when it
        // backs them up: "%USERPROFILE%/Desktop" is then really
        // "%USERPROFILE%/OneDrive/Desktop" (1 Oct 2026: on Eric's laptop every
        // file on the desktop was refused as outside the folders Atlas may
        // work in, so "organize my desktop" moved nothing).
        let onedrive = (!profile.is_empty() && r.starts_with(&profile))
            .then(|| format!("{profile}/onedrive{}", &r[profile.len()..]));
        // And wherever this machine really keeps that folder (2 Oct 2026):
        // Windows lets Downloads or Documents be moved to another drive, and
        // Linux names them in the person's own language, so
        // "%USERPROFILE%/Downloads" means the Downloads Windows (or XDG)
        // says, not only the one under home (`organize::user_folder`).
        let known = (!profile.is_empty())
            .then(|| r.trim_end_matches('/').strip_prefix(&format!("{profile}/")).map(str::to_string))
            .flatten()
            .filter(|rest| ["desktop", "documents", "downloads", "pictures", "videos", "music"].contains(&rest.as_str()))
            .and_then(|which| crate::organize::user_folder(&which))
            .map(|k| k.display().to_string().replace('\\', "/").to_lowercase().trim_end_matches('/').to_string());
        p.starts_with(&r) || onedrive.is_some_and(|o| p.starts_with(&o)) || known.is_some_and(|k| p == k || p.starts_with(&format!("{k}/")))
    })
}

/// What Atlas says as it does it. Names the change and how to undo it, because
/// "done" on its own leaves you wondering what changed.
pub fn describe(c: &Change, undo: Undo) -> String {
    let what = match c {
        Change::Wallpaper { path } => format!(
            "wallpaper set to {}",
            std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone())
        ),
        Change::Theme { dark } => format!("switched to {} mode", if *dark { "dark" } else { "light" }),
        Change::DesktopLayout { description } => description.clone(),
        Change::MoveFile { from, to } => format!("moved {} to {}", file_of(from), folder_of(to)),
        Change::RenameFile { from, to } => format!("renamed {} to {}", file_of(from), file_of(to)),
        Change::CreateFolder { path } => format!("made a folder, {}", file_of(path)),
        Change::Replace { path } => format!("replaced {}", file_of(path)),
        Change::Delete { path } => format!("moved {} to the trash", file_of(path)),
        Change::PurgeTrash => "emptied the trash".into(),
        Change::Setting { name, value } => format!("{name} set to {value}"),
        Change::ForItself { what, .. } => what.clone(),
    };
    match undo {
        Undo::Instant => format!("{what}. Say undo and it goes back."),
        Undo::FromTrash => format!("{what}. It's recoverable if that's wrong."),
        Undo::ByHand => format!("{what}. You'd have to change that back yourself."),
        Undo::Never => format!("{what}. That one can't be undone."),
    }
}

fn file_of(p: &str) -> String {
    std::path::Path::new(p)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}

fn folder_of(p: &str) -> String {
    let path = std::path::Path::new(p);
    let dir = if path.extension().is_some() { path.parent() } else { Some(path) };
    dir.and_then(|d| d.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| p.to_string())
}
