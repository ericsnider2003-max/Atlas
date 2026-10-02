//! The application tracker (bookshelf/hunt open items, 2 Oct 2026): what
//! you've applied for, said in your own words, and a nudge when one has gone
//! quiet for a week.
//!
//! - "I applied for video editor at Brightside" / "I applied to Brightside"
//! - "Brightside got back to me" / "I have an interview with Brightside"
//! - "Brightside said no" / "Brightside offered me the job"
//! - "my applications" / "where are my applications"
//!
//! Nothing here sends anything. The follow-up nudge only says it's been a
//! week; writing the note is yours.

use serde::{Deserialize, Serialize};

/// The store record.
pub const FILE: &str = "applications";
/// A week with no word, and Atlas mentions it once.
pub const FOLLOW_UP_AFTER_SECS: u64 = 7 * 86_400;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Stage {
    Applied,
    Heard,
    Interview,
    Offer,
    No,
}

impl Stage {
    fn said(self) -> &'static str {
        match self {
            Stage::Applied => "applied, no word yet",
            Stage::Heard => "they got back to you",
            Stage::Interview => "interview",
            Stage::Offer => "offer",
            Stage::No => "they said no",
        }
    }
    fn open(self) -> bool {
        !matches!(self, Stage::Offer | Stage::No)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Application {
    /// The company, as you named it.
    pub to: String,
    /// The role, when you said one.
    #[serde(default)]
    pub role: String,
    pub applied_at: u64,
    pub stage: Stage,
    /// When the stage last changed.
    pub moved_at: u64,
    /// The follow-up nudge has been given.
    #[serde(default)]
    pub nudged: bool,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct Applications {
    pub all: Vec<Application>,
}

/// What was said about an application.
#[derive(Debug, Clone, PartialEq)]
pub enum Heard {
    Applied { to: String, role: String },
    Moved { to: String, stage: Stage },
    List,
}

fn clean(s: &str) -> String {
    s.trim().trim_end_matches(['.', '!', '?']).trim().trim_start_matches("the ").to_string()
}

/// What `said` tells the tracker, if anything.
pub fn heard(said: &str) -> Option<Heard> {
    let raw = said.trim().trim_end_matches(['.', '!', '?']);
    let raw = raw.strip_prefix("Atlas, ").or_else(|| raw.strip_prefix("atlas, ")).unwrap_or(raw);
    let low = raw.to_ascii_lowercase();
    if matches!(
        low.as_str(),
        "my applications" | "where are my applications" | "show my applications" | "list my applications"
            | "what have i applied for" | "how are my applications going" | "job applications"
    ) {
        return Some(Heard::List);
    }
    for lead in ["i applied for ", "i applied to ", "i just applied for ", "i just applied to ", "applied for ", "applied to "] {
        if let Some(rest) = low.strip_prefix(lead) {
            let at = raw.len() - rest.len();
            let rest_raw = &raw[at..];
            if let Some(i) = rest.rfind(" at ") {
                let (role, to) = (clean(&rest_raw[..i]), clean(&rest_raw[i + 4..]));
                let role = role.trim_start_matches("a ").trim_start_matches("an ").trim_start_matches("the ").to_string();
                if !to.is_empty() {
                    return Some(Heard::Applied { to, role });
                }
            }
            let to = clean(rest_raw);
            // "applied to three jobs" isn't a company.
            if to.is_empty() || to.split_whitespace().count() > 5 || rest.ends_with("jobs") {
                return None;
            }
            return Some(Heard::Applied { to, role: String::new() });
        }
    }
    let moved: &[(&str, Stage)] = &[
        (" got back to me", Stage::Heard),
        (" replied", Stage::Heard),
        (" called me back", Stage::Heard),
        (" wants an interview", Stage::Interview),
        (" said no", Stage::No),
        (" turned me down", Stage::No),
        (" rejected me", Stage::No),
        (" offered me the job", Stage::Offer),
        (" made me an offer", Stage::Offer),
    ];
    for (tail, stage) in moved {
        if let Some(head) = low.strip_suffix(tail) {
            let to = clean(&raw[..head.len()]);
            if !to.is_empty() && to.split_whitespace().count() <= 5 && !to.eq_ignore_ascii_case("they") {
                return Some(Heard::Moved { to, stage: *stage });
            }
        }
    }
    for lead in ["i have an interview with ", "i got an interview with ", "i've got an interview with ", "interview with "] {
        if let Some(rest) = low.strip_prefix(lead) {
            let mut to = clean(&raw[raw.len() - rest.len()..]);
            // "… with Acme on Friday": the company, not the day.
            for when in [" on ", " next ", " tomorrow", " today", " this "] {
                if let Some(i) = to.to_ascii_lowercase().find(when) {
                    to.truncate(i);
                }
            }
            if !to.is_empty() {
                return Some(Heard::Moved { to, stage: Stage::Interview });
            }
        }
    }
    None
}

impl Applications {
    fn find(&mut self, to: &str) -> Option<&mut Application> {
        let want = to.to_lowercase();
        self.all.iter_mut().rev().find(|a| a.to.to_lowercase() == want)
    }

    /// Take in what was heard; the reply to say.
    pub fn take(&mut self, h: Heard, now: u64) -> String {
        match h {
            Heard::Applied { to, role } => {
                let what = if role.is_empty() { to.clone() } else { format!("{role} at {to}") };
                self.all.push(Application { to, role, applied_at: now, stage: Stage::Applied, moved_at: now, nudged: false });
                format!("Noted: you applied for {what}. If there's no word in a week I'll mention it.")
            }
            Heard::Moved { to, stage } => match self.find(&to) {
                Some(a) => {
                    a.stage = stage;
                    a.moved_at = now;
                    let name = a.to.clone();
                    match stage {
                        Stage::Offer => format!("An offer from {name} -- well done. Marked."),
                        Stage::No => format!("Sorry about {name}. Marked as closed."),
                        Stage::Interview => format!("An interview with {name}. Marked."),
                        _ => format!("{name} got back to you. Marked."),
                    }
                }
                None => format!(
                    "I don't have an application to {to}. Say \"I applied to {to}\" first if you want me to keep track of it."
                ),
            },
            Heard::List => self.listed(now),
        }
    }

    /// Every application, open ones first, said plainly.
    pub fn listed(&self, now: u64) -> String {
        if self.all.is_empty() {
            return "You haven't told me about any applications. Say \"I applied for <role> at <company>\" and I'll keep track."
                .into();
        }
        let mut open: Vec<&Application> = self.all.iter().filter(|a| a.stage.open()).collect();
        open.sort_by_key(|a| a.applied_at);
        let closed: Vec<&Application> = self.all.iter().filter(|a| !a.stage.open()).collect();
        let mut lines = vec![format!("{} open, {} closed.", open.len(), closed.len())];
        for a in open.iter().chain(closed.iter()) {
            let days = now.saturating_sub(a.applied_at) / 86_400;
            let what = if a.role.is_empty() { a.to.clone() } else { format!("{} at {}", a.role, a.to) };
            let ago = match days {
                0 => "today".to_string(),
                1 => "yesterday".to_string(),
                n => format!("{n} days ago"),
            };
            lines.push(format!("- {what}: {} (applied {ago})", a.stage.said()));
        }
        lines.join("\n")
    }

    /// The ones quiet for a week that haven't been mentioned yet, marked as
    /// mentioned. The lines to say.
    pub fn due_follow_ups(&mut self, now: u64) -> Vec<String> {
        let mut out = Vec::new();
        for a in self.all.iter_mut() {
            if a.stage == Stage::Applied && !a.nudged && now.saturating_sub(a.moved_at) >= FOLLOW_UP_AFTER_SECS {
                a.nudged = true;
                out.push(format!(
                    "No word from {} in a week since you applied. A short follow-up note is normal now.",
                    a.to
                ));
            }
        }
        out
    }
}
