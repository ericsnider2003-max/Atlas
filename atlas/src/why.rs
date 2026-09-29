//! Answering "why did you do that?"
//!
//! Every decision Atlas makes already produces a reason — the layout engine
//! says which monitor and why, the approval gate says which rule applied, the
//! audio layer says which microphone and what it measured. None of it was ever
//! shown to you.
//!
//! That's a shame, because an assistant that can account for itself is one you
//! extend trust to, and one that can't is one you second-guess forever. It is
//! also the thing none of the fictional systems do — they all announce
//! conclusions.

use serde::{Deserialize, Serialize};

/// One thing Atlas decided, and what decided it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub at: u64,
    /// "put Chrome on the left screen", "used the webcam mic".
    pub what: String,
    /// The rule or measurement behind it, in plain language.
    pub because: String,
    /// What it would have done otherwise, when there was a real alternative.
    #[serde(default)]
    pub instead_of: Option<String>,
    /// Where the rule lives, so you can change it.
    #[serde(default)]
    pub set_by: Option<String>,
}

const KEEP: usize = 300;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Record {
    pub decisions: Vec<Decision>,
}

impl Record {
    pub fn note(&mut self, what: &str, because: &str, at: u64) {
        self.decisions.push(Decision {
            at,
            what: what.into(),
            because: because.into(),
            instead_of: None,
            set_by: None,
        });
        self.trim();
    }

    pub fn note_full(&mut self, d: Decision) {
        self.decisions.push(d);
        self.trim();
    }

    fn trim(&mut self) {
        if self.decisions.len() > KEEP {
            let drop = self.decisions.len() - KEEP;
            self.decisions.drain(0..drop);
        }
    }

    /// Find what you're asking about.
    ///
    /// Matched loosely, because you won't phrase it the way Atlas recorded it
    /// — you'll say "why is Chrome over there", and "microphone" where Atlas
    /// wrote "mic".
    pub fn find(&self, question: &str) -> Option<&Decision> {
        let words: Vec<String> = question
            .to_lowercase()
            .split_whitespace()
            .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
            .filter(|w| w.len() > 2 && !IGNORE.contains(&w.as_str()))
            .collect();
        if words.is_empty() {
            return self.decisions.last();
        }
        self.decisions
            .iter()
            .rev()
            .max_by_key(|d| {
                let hay = format!("{} {}", d.what, d.because).to_lowercase();
                words.iter().filter(|w| mentions(&hay, w)).count()
            })
            .filter(|d| {
                let hay = format!("{} {}", d.what, d.because).to_lowercase();
                words.iter().any(|w| mentions(&hay, w))
            })
    }

    pub fn last(&self) -> Option<&Decision> {
        self.decisions.last()
    }
}

/// Does this text mention that word, allowing for a shortened form?
///
/// You say "microphone"; Atlas wrote "mic". Matching only on whole words
/// means the obvious question finds nothing.
fn mentions(haystack: &str, word: &str) -> bool {
    if haystack.contains(word) {
        return true;
    }
    haystack.split(|c: char| !c.is_alphanumeric()).any(|h| {
        h.len() >= 3 && (word.starts_with(h) || h.starts_with(word))
    })
}

/// Words that carry no meaning for matching.
const IGNORE: &[&str] = &[
    "why", "did", "you", "the", "that", "this", "for", "and", "was", "were",
    "how", "come", "what", "made", "your",
];

/// Is this a question about something Atlas did?
pub fn is_asking_why(said: &str) -> bool {
    let t = said.to_lowercase();
    t.starts_with("why")
        || t.starts_with("how come")
        || t.contains("what made you")
        || t.contains("explain that")
        || t.contains("why'd you")
        || t.contains("reason for")
}

/// The answer.
///
/// Leads with the reason rather than restating the question, names the
/// alternative when there was one, and says where the rule lives so you can
/// change it rather than argue with it.
pub fn answer(d: Option<&Decision>) -> String {
    let Some(d) = d else {
        return "I don't have a decision recorded that matches that.".into();
    };
    let mut s = format!("{} because {}", d.what, d.because);
    if !s.ends_with('.') {
        s.push('.');
    }
    if let Some(alt) = &d.instead_of {
        s.push_str(&format!(" Otherwise it would have been {alt}."));
    }
    if let Some(set) = &d.set_by {
        s.push_str(&format!(" That's set in {set}."));
    }
    s
}

/// Several decisions at once, for "why is my workspace like this?"
pub fn account(decisions: &[&Decision]) -> String {
    if decisions.is_empty() {
        return "Nothing to account for yet.".into();
    }
    decisions
        .iter()
        .map(|d| format!("{} — {}", d.what, d.because))
        .collect::<Vec<_>>()
        .join("\n")
}

/// A decision broken into the steps behind it, one per line.
///
/// For Atlas's own window, where you asked to see the thought process rather
/// than hear a summary of it. Spoken, this would be four sentences nobody
/// retains; on screen it is four lines you can point at.
///
/// Only the parts that exist are included — an absent alternative or an
/// unknown source produces no line at all, rather than "Instead of: none",
/// which reads as a finding when it is an absence.
pub fn steps(d: &Decision) -> Vec<String> {
    let mut out = vec![format!("Did: {}", d.what), format!("Because: {}", d.because)];
    if let Some(alt) = &d.instead_of {
        out.push(format!("Rather than: {alt}"));
    }
    if let Some(src) = &d.set_by {
        out.push(format!("Rule lives in: {src}"));
    }
    out
}
