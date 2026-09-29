//! Showing you what it would do, without doing it.
//!
//! Idea #4. Before letting Atlas near a real workflow you watch the whole
//! thing play out harmlessly: every window it would move, every file it would
//! touch, every message it would send.
//!
//! The machinery already existed — the fake operating system that the test
//! suite runs against. Nothing exposed it to you. This does.
//!
//! The guarantee is structural: a rehearsal runs against the mock platform, so
//! there is no code path from a rehearsal to your actual windows.

use crate::platform::mock::Action;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Beat {
    /// What it would do, in plain language.
    pub what: String,
    /// Whether this one would have been irreversible.
    pub consequential: bool,
    /// What would have stopped it, if anything.
    pub gated_by: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct Rehearsal {
    pub command: String,
    pub beats: Vec<Beat>,
    /// Things it would have asked you about.
    pub questions: Vec<String>,
    /// Things it couldn't do at all.
    pub blocked: Vec<String>,
}

impl Rehearsal {
    pub fn new(command: &str) -> Rehearsal {
        Rehearsal { command: command.to_string(), ..Default::default() }
    }

    pub fn touches_anything_irreversible(&self) -> bool {
        self.beats.iter().any(|b| b.consequential)
    }

    /// One spoken line.
    pub fn summary(&self) -> String {
        if self.beats.is_empty() && self.blocked.is_empty() {
            return "Nothing would happen.".into();
        }
        let mut s = format!(
            "{} step{}",
            self.beats.len(),
            if self.beats.len() == 1 { "" } else { "s" }
        );
        if !self.questions.is_empty() {
            s.push_str(&format!(", {} thing{} I'd ask about", self.questions.len(),
                if self.questions.len() == 1 { "" } else { "s" }));
        }
        if !self.blocked.is_empty() {
            s.push_str(&format!(", {} I couldn't do", self.blocked.len()));
        }
        s.push('.');
        s
    }

    /// The full walk-through, for reading.
    pub fn detail(&self) -> String {
        let mut s = format!("If you said \"{}\":\n", self.command);
        for (i, b) in self.beats.iter().enumerate() {
            let mark = if b.consequential { "!" } else { " " };
            s.push_str(&format!("  {}{}. {}\n", mark, i + 1, b.what));
            if let Some(g) = &b.gated_by {
                s.push_str(&format!("      (I'd ask first: {g})\n"));
            }
        }
        for q in &self.questions {
            s.push_str(&format!("  ? {q}\n"));
        }
        for b in &self.blocked {
            s.push_str(&format!("  x {b}\n"));
        }
        s
    }
}

/// Turn what the fake OS recorded into something readable.
pub fn from_actions(command: &str, actions: &[Action]) -> Rehearsal {
    let mut r = Rehearsal::new(command);
    for a in actions {
        let (what, consequential) = match a {
            Action::Launch(app) => (format!("open {}", tidy_app(app)), false),
            Action::Place(app, rect) => (
                format!(
                    "move {} to {}x{} at {},{}",
                    tidy_app(app),
                    rect.width,
                    rect.height,
                    rect.x,
                    rect.y
                ),
                false,
            ),
            Action::Focus(app) => (format!("bring {} forward", tidy_app(app)), false),
            // The one that can lose you work.
            Action::Close(app) => (format!("close {}", tidy_app(app)), true),
            Action::Type(text) => (format!("type \"{}\"", short(text)), true),
            Action::Press(k) => (format!("press {k}"), true),
            Action::Click(x, y, _) => (format!("click at {x},{y}"), true),
            Action::Scroll(_, dy) => (format!("scroll {dy}"), false),
            // Waiting isn't worth a line in a walk-through.
            Action::Sleep(_) => continue,
        };
        r.beats.push(Beat { what, consequential, gated_by: None });
    }
    r
}

fn tidy_app(name: &str) -> String {
    name.trim_end_matches(".exe").to_string()
}

fn short(s: &str) -> String {
    if s.chars().count() <= 40 {
        return s.to_string();
    }
    format!("{}…", s.chars().take(40).collect::<String>())
}

/// The line Atlas says before a rehearsal, so it's never mistaken for the real
/// thing.
pub fn preamble(command: &str) -> String {
    format!("Rehearsing \"{command}\" — nothing here actually happens.")
}
