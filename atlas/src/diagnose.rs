//! Atlas checking on itself.
//!
//! An assistant that quietly half-works is worse than one that plainly
//! doesn't — you keep asking, it keeps not quite delivering, and you never
//! find out why. So Atlas runs its own checks, and reports in three
//! categories: what it fixed, what it wants your permission to fix, and what
//! it can't fix at all.
//!
//! The line it will not cross: **it only repairs things it created.** Scratch
//! folders, its own caches, its own state files. Anything belonging to you is
//! a recommendation, never an action.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Impact {
    /// Works, but worse than it should.
    Degraded,
    /// One capability is unavailable.
    Broken,
    /// Atlas can't run properly at all.
    Fatal,
}

/// What can be done about it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Remedy {
    /// Atlas can fix this itself, safely and reversibly, without asking.
    /// Only ever applies to things Atlas created.
    Self_ { action: String },
    /// Atlas knows the fix but it touches your machine. Needs a yes.
    Offer { action: String, what_changes: String },
    /// You have to do it — an install, a setting, a decision.
    Yours { action: String },
    /// Not understood well enough to suggest anything.
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Symptom {
    /// Stable id, so a recurring problem isn't reported as a new one.
    pub id: String,
    pub impact: Impact,
    /// One plain sentence. This gets read aloud.
    pub what: String,
    pub remedy: Remedy,
}

/// What the checks are given to look at.
#[derive(Debug, Clone, Default)]
pub struct Vitals {
    pub config_loaded: bool,
    pub config_error: Option<String>,
    pub state_writable: bool,
    /// Tools named in config that aren't on disk or PATH.
    pub missing_tools: Vec<String>,
    /// Model files named in config that aren't there.
    pub missing_models: Vec<String>,
    pub scratch_dir_missing: bool,
    /// State files that had to be set aside as unreadable.
    pub preserved_files: usize,
    pub disk_free_gb: f32,
    pub ram_used_fraction: f32,
    /// Turns that ended in an error, out of the last hundred.
    pub recent_failures: u32,
    /// Slowest stage of a turn, and how long it took.
    pub slowest_stage: Option<(String, u64)>,
    pub backups_ever: usize,
}

pub fn diagnose(v: &Vitals) -> Vec<Symptom> {
    let mut out = Vec::new();

    if !v.config_loaded {
        out.push(Symptom {
            id: "config".into(),
            impact: Impact::Fatal,
            what: v
                .config_error
                .clone()
                .unwrap_or_else(|| "My configuration won't load.".into()),
            remedy: Remedy::Yours { action: "fix the config file named in the error".into() },
        });
    }

    if !v.state_writable {
        out.push(Symptom {
            id: "state".into(),
            impact: Impact::Fatal,
            what: "I can't write to my own state folder, so nothing I learn will survive.".into(),
            remedy: Remedy::Yours {
                action: "check the Atlas folder isn't read-only or blocked by antivirus".into(),
            },
        });
    }

    // Something Atlas made and can remake. No permission needed.
    if v.scratch_dir_missing {
        out.push(Symptom {
            id: "scratch".into(),
            impact: Impact::Degraded,
            what: "My scratch folder was missing.".into(),
            remedy: Remedy::Self_ { action: "recreate it".into() },
        });
    }

    if v.preserved_files > 0 {
        out.push(Symptom {
            id: "preserved".into(),
            impact: Impact::Degraded,
            what: format!(
                "{} of my state files couldn't be read and were set aside. I've started those fresh.",
                v.preserved_files
            ),
            remedy: Remedy::Self_ { action: "carry on with defaults; the old files are kept".into() },
        });
    }

    for t in &v.missing_tools {
        out.push(Symptom {
            id: format!("tool:{t}"),
            impact: Impact::Broken,
            what: format!("{t} isn't installed, so anything needing it won't work."),
            remedy: Remedy::Yours { action: format!("run setup/install-voice.bat, or install {t}") },
        });
    }

    for m in &v.missing_models {
        out.push(Symptom {
            id: format!("model:{m}"),
            impact: Impact::Broken,
            what: format!("The model {m} isn't downloaded."),
            remedy: Remedy::Yours { action: "run setup/install-voice.bat".into() },
        });
    }

    if v.backups_ever == 0 {
        out.push(Symptom {
            id: "nobackup".into(),
            impact: Impact::Degraded,
            what: "Nothing I've learned has ever been backed up.".into(),
            remedy: Remedy::Offer {
                action: "take one now".into(),
                what_changes: "copies my state folder into data/backups".into(),
            },
        });
    }

    if v.disk_free_gb > 0.0 && v.disk_free_gb < 2.0 {
        out.push(Symptom {
            id: "disk".into(),
            impact: Impact::Fatal,
            what: "There's almost no disk left — I can't even save what I learn.".into(),
            remedy: Remedy::Offer {
                action: "clear my own captures and scratch files".into(),
                what_changes: "deletes screenshots and temporary audio, nothing of yours".into(),
            },
        });
    }

    if v.ram_used_fraction > 0.93 {
        out.push(Symptom {
            id: "ram".into(),
            impact: Impact::Degraded,
            what: "Memory is nearly full, so I'll be slow and may stutter.".into(),
            remedy: Remedy::Offer {
                action: "shut down my background helpers".into(),
                what_changes: "closes the headless browser and any idle model process".into(),
            },
        });
    }

    if v.recent_failures >= 10 {
        out.push(Symptom {
            id: "failures".into(),
            impact: Impact::Broken,
            what: format!("{} of my last hundred turns ended in an error.", v.recent_failures),
            remedy: Remedy::Unknown,
        });
    }

    if let Some((stage, ms)) = &v.slowest_stage {
        if *ms > 4000 {
            out.push(Symptom {
                id: format!("slow:{stage}"),
                impact: Impact::Degraded,
                what: format!("{stage} is taking {:.1} seconds, which is slow enough to notice.", *ms as f32 / 1000.0),
                remedy: Remedy::Unknown,
            });
        }
    }

    out.sort_by(|a, b| b.impact.cmp(&a.impact));
    out
}

/// The ones Atlas may act on unasked.
pub fn self_fixable(symptoms: &[Symptom]) -> Vec<&Symptom> {
    symptoms.iter().filter(|s| matches!(s.remedy, Remedy::Self_ { .. })).collect()
}

/// The ones it wants permission for.
pub fn needs_permission(symptoms: &[Symptom]) -> Vec<&Symptom> {
    symptoms.iter().filter(|s| matches!(s.remedy, Remedy::Offer { .. })).collect()
}

/// The ones only you can resolve.
pub fn yours(symptoms: &[Symptom]) -> Vec<&Symptom> {
    symptoms.iter().filter(|s| matches!(s.remedy, Remedy::Yours { .. })).collect()
}

/// What Atlas says when asked how it's doing.
///
/// Leads with anything fatal, mentions at most one thing per category, and
/// says "I'm fine" plainly when it is — a health report that always has
/// something to say gets ignored.
pub fn report(symptoms: &[Symptom]) -> String {
    if symptoms.is_empty() {
        return "I'm running properly.".into();
    }
    let mut parts: Vec<String> = Vec::new();

    if let Some(f) = symptoms.iter().find(|s| s.impact == Impact::Fatal) {
        parts.push(f.what.clone());
        if let Remedy::Yours { action } = &f.remedy {
            parts.push(format!("You'd need to {action}."));
        }
        return parts.join(" ");
    }

    let broken: Vec<&Symptom> = symptoms.iter().filter(|s| s.impact == Impact::Broken).collect();
    if !broken.is_empty() {
        parts.push(broken[0].what.clone());
        if broken.len() > 1 {
            parts.push(format!("{} other things are affected.", broken.len() - 1));
        }
    }

    let offers = needs_permission(symptoms);
    if let Some(o) = offers.first() {
        if let Remedy::Offer { action, .. } = &o.remedy {
            parts.push(format!("I could {action} if you want."));
        }
    }

    if parts.is_empty() {
        let d = &symptoms[0];
        parts.push(d.what.clone());
    }
    parts.join(" ")
}

/// A fuller answer, for when you ask what's wrong in detail.
pub fn detail(symptoms: &[Symptom]) -> String {
    if symptoms.is_empty() {
        return "Nothing to report.".into();
    }
    let mut s = String::new();
    for sym in symptoms {
        let tag = match sym.impact {
            Impact::Fatal => "STOPS ME",
            Impact::Broken => "broken",
            Impact::Degraded => "degraded",
        };
        s.push_str(&format!("[{tag}] {}\n", sym.what));
        match &sym.remedy {
            Remedy::Self_ { action } => s.push_str(&format!("    I handled it: {action}\n")),
            Remedy::Offer { action, what_changes } => {
                s.push_str(&format!("    I can {action} — {what_changes}\n"))
            }
            Remedy::Yours { action } => s.push_str(&format!("    You'd need to {action}\n")),
            Remedy::Unknown => s.push_str("    I don't know what's causing this\n"),
        }
    }
    s
}
