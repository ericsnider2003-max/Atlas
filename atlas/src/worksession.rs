//! "Let's work" sessions (1 Oct 2026, the "why Atlas feels stale" report,
//! idea 3): "two hours on the edit" -- Atlas holds everything that isn't
//! urgent, checks in once halfway, and at the end says how it went, from
//! the work log, and keeps that in your notes.
//!
//! What it doesn't do: open or close apps on its own (that's a named mode's
//! job, "video mode"), or decide the session went well. The summary is what
//! the record shows -- time on it, the longest stretch, how often you
//! switched -- and what you said you'd do.

use serde::{Deserialize, Serialize};

/// One session.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Session {
    /// What it's for, in your words ("the edit").
    pub what: String,
    pub started: u64,
    pub until: u64,
    /// Whether the halfway check-in has been said.
    #[serde(default)]
    pub checked_in: bool,
}

/// The shortest and longest sessions taken: under ten minutes isn't a
/// session, and past four hours Atlas would be silencing itself for a day.
pub const SHORTEST_SECS: u64 = 10 * 60;
pub const LONGEST_SECS: u64 = 4 * 3600;

/// What was said about a session.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    /// Start one: what for, and how long.
    Start { what: String, secs: u64 },
    /// End it now.
    End,
    /// How long is left?
    HowLong,
}

/// "Let's work on the edit for two hours", "two hours on the edit", "work
/// session on the budget for 45 minutes", "end the session".
pub fn heard(said: &str) -> Option<Said> {
    let t = said.trim().trim_end_matches(['.', '!', '?']).to_ascii_lowercase();
    let t = t.trim_start_matches("atlas, ").trim_start_matches("atlas ").trim().to_string();
    if ["end the session", "end session", "stop the session", "end the work session", "session over", "i'm done with the session"]
        .contains(&t.as_str())
    {
        return Some(Said::End);
    }
    if ["how long is left", "how long left", "how much time is left", "how long left in the session"].contains(&t.as_str()) {
        return Some(Said::HowLong);
    }
    let secs = crate::camwatch::length_in(&t)?;
    // What it's for: after "on"/"for" and without the length.
    let lead = ["let's work on ", "lets work on ", "let's work ", "lets work ", "work session on ", "work session for ", "focus session on ", "focus on "];
    let body = lead.iter().find_map(|l| t.strip_prefix(l)).map(str::to_string).or_else(|| {
        // "two hours on the edit"
        let i = t.find(" on ")?;
        let before = &t[..i];
        crate::camwatch::length_in(before).map(|_| t[i + 4..].to_string())
    })?;
    let what = strip_length(&body);
    if what.is_empty() {
        return None;
    }
    Some(Said::Start { what, secs: secs.clamp(SHORTEST_SECS, LONGEST_SECS) })
}

/// The words of a session's purpose without "for two hours".
fn strip_length(body: &str) -> String {
    let words: Vec<&str> = body.split_whitespace().collect();
    let cut = words
        .iter()
        .position(|w| *w == "for")
        .filter(|i| crate::camwatch::length_in(&words[*i..].join(" ")).is_some())
        .unwrap_or(words.len());
    words[..cut].join(" ")
}

impl Session {
    pub fn new(what: &str, secs: u64, now: u64) -> Session {
        Session { what: what.to_string(), started: now, until: now + secs, checked_in: false }
    }

    pub fn left(&self, now: u64) -> u64 {
        self.until.saturating_sub(now)
    }

    /// The halfway check-in, once.
    pub fn check_in_due(&self, now: u64) -> bool {
        !self.checked_in && now >= self.started + (self.until - self.started) / 2 && now < self.until
    }

    pub fn over(&self, now: u64) -> bool {
        now >= self.until
    }
}

/// What Atlas says on starting.
pub fn started(s: &Session) -> String {
    format!(
        "{} on {}. I'll hold anything that isn't urgent, check in once halfway, and tell you how it went at the end. Say \"end the session\" to stop early.",
        crate::worklog::duration_words(s.until - s.started),
        s.what
    )
}

/// The halfway line.
pub fn halfway(s: &Session, now: u64) -> String {
    format!("Halfway on {} -- {} to go. Still on it?", s.what, crate::worklog::duration_words(s.left(now)))
}

/// How it went: what the work log shows for the session's span.
pub fn how_it_went(s: &Session, ended: u64, log: &crate::worklog::Summary, held: usize) -> String {
    let span = ended.saturating_sub(s.started);
    let mut out = format!("That's the session on {}: {}", s.what, crate::worklog::duration_words(span));
    if ended < s.until {
        out.push_str(" (ended early)");
    }
    out.push('.');
    if log.active > 0 {
        let top: Vec<String> = log.by_category.iter().take(2).map(|(c, n)| format!("{c} {}", crate::worklog::duration_words(*n))).collect();
        out.push_str(&format!(" At the machine {}: {}.", crate::worklog::duration_words(log.active), top.join(", ")));
        match log.blocks.first() {
            Some(b) => out.push_str(&format!(" Longest stretch: {}.", crate::worklog::duration_words(b.end.saturating_sub(b.start)))),
            None => out.push_str(" No stretch of 25 minutes on one thing."),
        }
        let hours = log.active as f64 / 3600.0;
        if log.switches > 0 && hours >= 0.25 {
            out.push_str(&format!(" You switched about {:.0} times an hour.", (log.switches as f64 / hours).max(1.0)));
        }
    }
    if held > 0 {
        out.push_str(&format!(" I held {held} thing{} for you -- say \"what did I miss\" to hear them.", if held == 1 { "" } else { "s" }));
    }
    out
}

/// The line kept in your notes.
pub fn note_line(s: &Session, ended: u64, said: &str, clock: &dyn Fn(u64) -> String) -> String {
    format!("- {} to {}, {}: {}", clock(s.started), clock(ended), s.what, said)
}
