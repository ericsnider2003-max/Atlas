//! A fast, on-device self-check.
//!
//! The test suite — thousands of tests across hundreds of files — is a
//! *development* gate. It runs on a build machine before a release, proving the
//! code is sound; it is not something a person runs on their computer to use
//! Atlas, and nobody should have to sit through it. What a fresh install
//! actually needs is a different, much smaller question: does Atlas *work on
//! this machine*, right now?
//!
//! This is that check — a handful of fast probes of the parts everything else
//! rests on (its store, its memory, the screen, the model), plus an honest
//! count of what's ready here. Seconds, offline, and safe to run any time: it
//! writes only to its own probe key and opens nothing, sends nothing, changes
//! nothing of yours.

/// How one probe came out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// It works.
    Pass,
    /// It doesn't, and why.
    Fail(String),
    /// Not a pass/fail — a fact worth reporting (a count, the model's endpoint).
    Note(String),
}

/// One probe and its result.
#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub outcome: Outcome,
}

impl Check {
    pub fn pass(name: &'static str) -> Check {
        Check { name, outcome: Outcome::Pass }
    }
    pub fn fail(name: &'static str, why: impl Into<String>) -> Check {
        Check { name, outcome: Outcome::Fail(why.into()) }
    }
    pub fn note(name: &'static str, what: impl Into<String>) -> Check {
        Check { name, outcome: Outcome::Note(what.into()) }
    }
    pub fn failed(&self) -> bool {
        matches!(self.outcome, Outcome::Fail(_))
    }
}

/// Did the whole check pass — nothing actually failed?
pub fn all_clear(checks: &[Check]) -> bool {
    !checks.iter().any(|c| c.failed())
}

/// The self-check said in plain words, leading with the verdict so the answer
/// is the first thing read, not the last.
pub fn report(checks: &[Check]) -> String {
    let failures = checks.iter().filter(|c| c.failed()).count();
    let mut s = if failures == 0 {
        String::from("Self-check passed — the core is working on this machine.")
    } else {
        format!(
            "Self-check found {failures} problem{}:",
            if failures == 1 { "" } else { "s" }
        )
    };
    for c in checks {
        match &c.outcome {
            Outcome::Pass => s.push_str(&format!("\n  [ok] {}", c.name)),
            Outcome::Fail(why) => s.push_str(&format!("\n  [!!] {} — {why}", c.name)),
            Outcome::Note(what) => s.push_str(&format!("\n  [--] {}: {what}", c.name)),
        }
    }
    s
}
