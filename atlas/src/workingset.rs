//! Taking the work with you.
//!
//! You start something on the laptop, pick your phone up, walk out, and lose
//! signal. If the files the task needs are still on the laptop, Atlas on the
//! phone can talk about the task and do nothing with it — which is the
//! difference between continuing and merely remembering.
//!
//! So a task carries its files. Not your whole drive — the handful of things
//! *this* piece of work touches, worked out from what you've actually opened
//! and referred to, packed small enough to live on a phone.

use serde::{Deserialize, Serialize};

/// A file that travels with a task.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Carried {
    pub path: String,
    /// What you'd call it.
    pub name: String,
    pub bytes: u64,
    /// Why it came along.
    pub because: Why,
    /// Can it be shrunk for the trip?
    pub shrinkable: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// You had it open when the task started.
    YouHadItOpen,
    /// You mentioned it out loud.
    YouMentionedIt,
    /// The task can't be done without it.
    Needed,
    /// It's in the same folder as something that is.
    NearSomethingNeeded,
    /// Atlas produced it as part of the work.
    Made,
}

impl Why {
    /// How strongly it earns a place, when space is short.
    pub fn weight(&self) -> f32 {
        match self {
            Why::Needed => 1.0,
            Why::Made => 0.9,
            Why::YouHadItOpen => 0.8,
            Why::YouMentionedIt => 0.7,
            Why::NearSomethingNeeded => 0.3,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WorkingSetConfig {
    pub enabled: bool,
    /// Most a task's files may weigh on the phone, in megabytes.
    pub budget_mb: u64,
    /// Shrink images and video for the trip rather than dropping them.
    pub shrink_rather_than_drop: bool,
    /// Keep them after the task is done, this many days.
    pub keep_days: u32,
    /// Pack the current task automatically whenever devices meet.
    pub always_carry_current: bool,
}

impl Default for WorkingSetConfig {
    fn default() -> Self {
        WorkingSetConfig {
            enabled: true,
            // Enough for documents and a few images; not enough to fill a
            // phone by accident.
            budget_mb: 200,
            shrink_rather_than_drop: true,
            keep_days: 14,
            always_carry_current: true,
        }
    }
}

/// What travels, and what didn't fit.
#[derive(Debug, Clone, PartialEq)]
pub struct Packed {
    pub taking: Vec<Carried>,
    /// Left behind, with why.
    pub leaving: Vec<(String, String)>,
    pub total_mb: u64,
    /// Shrunk to fit rather than dropped.
    pub shrunk: Vec<String>,
}

/// Decide what comes along.
///
/// Most valuable first, and when something doesn't fit it's shrunk before
/// it's dropped — a smaller version of the document you need beats the
/// absence of it.
pub fn pack(files: &[Carried], cfg: &WorkingSetConfig) -> Packed {
    let mut sorted: Vec<&Carried> = files.iter().collect();
    sorted.sort_by(|a, b| {
        b.because
            .weight()
            .partial_cmp(&a.because.weight())
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.bytes.cmp(&b.bytes))
    });

    let budget = cfg.budget_mb * 1024 * 1024;
    let mut used: u64 = 0;
    let mut taking = Vec::new();
    let mut leaving = Vec::new();
    let mut shrunk = Vec::new();

    for f in sorted {
        if used + f.bytes <= budget {
            used += f.bytes;
            taking.push(f.clone());
            continue;
        }
        // Doesn't fit as-is.
        if cfg.shrink_rather_than_drop && f.shrinkable {
            // A tenth is a fair rule of thumb for an image or a video at
            // phone-screen quality.
            let smaller = f.bytes / 10;
            if used + smaller <= budget {
                used += smaller;
                taking.push(Carried { bytes: smaller, ..f.clone() });
                shrunk.push(f.name.clone());
                continue;
            }
        }
        leaving.push((
            f.name.clone(),
            if f.because == Why::NearSomethingNeeded {
                "only nearby, not needed".into()
            } else {
                format!("{}MB, no room", f.bytes / 1_048_576)
            },
        ));
    }

    Packed { taking, leaving, total_mb: used / 1_048_576, shrunk }
}

/// What Atlas says as you pick your phone up.
pub fn spoken(p: &Packed) -> String {
    if p.taking.is_empty() {
        return "Nothing to carry.".into();
    }
    let mut s = format!("{} files with you, {}MB.", p.taking.len(), p.total_mb.max(1));
    if !p.shrunk.is_empty() {
        s.push_str(&format!(" {} shrunk for the trip.", p.shrunk.len()));
    }
    if !p.leaving.is_empty() {
        // Naming what didn't come is the part that saves you finding out at
        // the wrong moment.
        s.push_str(&format!(
            " Left behind: {}.",
            p.leaving.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    s
}

/// You're offline and reached for something that didn't come.
pub fn not_here(name: &str, why: &str) -> String {
    format!(
        "{name} didn't come with us — {why}. I've noted that you wanted it, and it'll be here \
         next time the laptop's in reach."
    )
}

/// Changes made on the phone that have to go home.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Changed {
    pub path: String,
    pub at: u64,
    /// The whole file, since these are small by design.
    pub bytes: u64,
}

/// What happens when you get back.
///
/// The file on the laptop may also have moved on. Same rule as everywhere
/// else: if only one side changed, it just lands.
pub fn returning(changed: &[Changed], also_changed_at_home: &[String]) -> (usize, Vec<String>) {
    let clashes: Vec<String> = changed
        .iter()
        .filter(|c| also_changed_at_home.iter().any(|h| h == &c.path))
        .map(|c| c.path.clone())
        .collect();
    (changed.len() - clashes.len(), clashes)
}
