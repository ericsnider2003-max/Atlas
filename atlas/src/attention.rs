//! "Pause." / "I'm ready."
//!
//! Pausing must be instant and total: Atlas stops talking mid-sentence, stops
//! the task it is running, and stops offering things.
//!
//! Since 28 Sep 2026 it also stops *listening*: the microphone's thread
//! (`micthread`) records nothing while paused — no wake word, no watching
//! while Atlas speaks. It used to keep listening so that "resume" could be
//! said; there are other ways back now that don't need an open microphone —
//! the icon by the clock, the hub's Carry on, typing it, or holding the talk
//! key (a deliberate press, which still works while paused).
//!
//! Resuming picks up where it stopped rather than starting over.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Heard {
    Pause,
    Resume,
    /// "Stop" — abandon the current task rather than suspend it.
    Cancel,
    /// Everything down, now. Not a pause — an abandon.
    Panic,
    Status,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Mode {
    Active,
    Paused {
        since: u64,
        /// What was interrupted, so resuming can name it.
        doing: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attention {
    pub mode: Mode,
    /// Task ids suspended by the pause, to be released on resume.
    pub suspended: Vec<u64>,
    /// Everything was abandoned rather than suspended.
    #[serde(default)]
    halted: bool,
}

impl Default for Attention {
    fn default() -> Self {
        Attention { mode: Mode::Active, suspended: Vec::new(), halted: false }
    }
}

impl Attention {
    pub fn is_paused(&self) -> bool {
        matches!(self.mode, Mode::Paused { .. })
    }

    pub fn pause(&mut self, doing: Option<String>, t: u64) -> String {
        if self.is_paused() {
            return "Already paused.".into();
        }
        self.mode = Mode::Paused { since: t, doing: doing.clone() };
        match doing {
            Some(d) => format!("Paused. {d} on hold."),
            None => "Paused.".into(),
        }
    }

    pub fn resume(&mut self, t: u64) -> String {
        let Mode::Paused { since, doing } = self.mode.clone() else {
            return "Wasn't paused.".into();
        };
        self.mode = Mode::Active;
        let halted = self.halted;
        self.halted = false;
        if halted {
            return "Starting fresh — I dropped what was running.".into();
        }
        let gap = t.saturating_sub(since);
        match doing {
            Some(d) => format!("Picking {d} back up."),
            None if gap > 3600 => "Back with you.".into(),
            None => "Go ahead.".into(),
        }
    }

    /// Everything down. Queues emptied, work abandoned, nothing resumed on
    /// its own — the difference from a pause is that nothing is waiting for
    /// you to come back.
    pub fn halt(&mut self, t: u64) -> String {
        self.mode = Mode::Paused { since: t, doing: Some("everything".into()) };
        self.suspended.clear();
        self.halted = true;
        "Everything stopped.".into()
    }

    /// Was the last stop a panic rather than a pause? Resuming after one
    /// should not silently restart abandoned work.
    pub fn was_halted(&self) -> bool {
        self.halted
    }

    /// Take back the suspended task ids on resume.
    pub fn release(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.suspended)
    }

    pub fn suspend(&mut self, id: u64) {
        if !self.suspended.contains(&id) {
            self.suspended.push(id);
        }
    }

    /// While paused, only a resume gets through. Everything else waits —
    /// including proactive offers, scheduled work, and speech.
    pub fn allows(&self, heard: Option<Heard>) -> bool {
        if !self.is_paused() {
            return true;
        }
        matches!(heard, Some(Heard::Resume) | Some(Heard::Status))
    }

    /// May Atlas speak right now?
    pub fn may_speak(&self) -> bool {
        !self.is_paused()
    }

    pub fn status(&self, t: u64) -> String {
        match &self.mode {
            Mode::Active => "Running.".into(),
            Mode::Paused { since, doing } => {
                let mins = t.saturating_sub(*since) / 60;
                match doing {
                    Some(d) => format!("Paused on {d}, {mins} minutes ago."),
                    None => format!("Paused {mins} minutes ago."),
                }
            }
        }
    }
}

/// Recognise pause and resume in speech.
///
/// Matched on the whole utterance, not as a substring: "don't pause the video"
/// must not pause Atlas. That is why these are exact phrases rather than
/// keyword hits.
pub fn hear(said: &str) -> Option<Heard> {
    let t: String = said
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    const PAUSE: &[&str] = &[
        "pause", "hold on", "hold up", "one moment", "one second", "wait",
        "give me a minute", "give me a second", "stand by", "hang on",
        "pause atlas", "atlas pause",
    ];
    const RESUME: &[&str] = &[
        "resume", "im ready", "i am ready", "im back", "i am back", "carry on",
        "go ahead", "continue", "keep going", "im ready to continue",
        "i am ready to continue", "ready to continue", "unpause", "atlas resume",
    ];
    const CANCEL: &[&str] = &["stop", "cancel that", "forget it", "never mind", "abort", "cancel"];
    // Deliberately phrases nobody says by accident, and that are easy to
    // reach for when something is going wrong.
    const PANIC: &[&str] = &[
        "atlas stop everything", "stop everything", "halt", "emergency stop",
        "shut it down", "kill it", "atlas halt", "drop everything",
    ];
    const STATUS: &[&str] = &["what are you doing", "are you paused", "status", "where were we"];

    if PAUSE.contains(&t.as_str()) {
        return Some(Heard::Pause);
    }
    if RESUME.contains(&t.as_str()) {
        return Some(Heard::Resume);
    }
    // Checked before Cancel, so "stop everything" isn't read as "stop".
    if PANIC.contains(&t.as_str()) {
        return Some(Heard::Panic);
    }
    if CANCEL.contains(&t.as_str()) {
        return Some(Heard::Cancel);
    }
    if STATUS.contains(&t.as_str()) {
        return Some(Heard::Status);
    }
    None
}
