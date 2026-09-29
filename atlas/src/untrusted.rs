//! Everything Atlas reads, and the one thing none of it may do.
//!
//! ## The rule, already established, now made general
//!
//! Handing Atlas a link says **look at this**. It never says **do what this
//! says**. That rule was written for the tray, because a fetched web page
//! that could be parsed into an intent would let any page issue Atlas
//! instructions in Eric's name.
//!
//! The same rule has to hold for everything handed over — records, past
//! verdicts, closed trades, notes, a config someone edited, a file dropped in
//! a folder. Atlas is a consultant reading a client's material. A
//! consultant reads the brief; it does not take orders from the stationery.
//!
//! ## The protection is structural, and it is not the detector
//!
//! `looks_like_orders` exists and finds the obvious attempts, and it is **not
//! what keeps this safe**. A detector can be got around by anyone who thinks
//! about it for a minute, and a system that relies on one has a security
//! boundary made of a word list.
//!
//! What actually keeps it safe is that a `Read` has no route into the parser.
//! It carries text, it renders that text as a quotation, and there is no
//! method on it that produces an intent. A source-reading guard holds that
//! line, because the failure mode is a line of code that does not exist yet.
//!
//! The detector's job is different and still worth having: it says **when
//! somebody tried**, which is a thing Eric would want to know.

use serde::{Deserialize, Serialize};

/// Something Atlas read, from somewhere that is not Atlas.
///
/// Deliberately a plain carrier. There is no `as_intent`, no `parse`, no
/// `execute` — not because they were left out, but because their absence is
/// the guarantee.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Read {
    /// Where it came from, so a quotation can say whose words these are.
    pub from: String,
    pub text: String,
    pub at: u64,
}

impl Read {
    pub fn new(from: &str, text: &str, at: u64) -> Read {
        Read { from: from.trim().to_string(), text: text.to_string(), at }
    }

    /// Rendered so it cannot be mistaken for Atlas's own words.
    ///
    /// Every line marked, and the source named above it. This matters more
    /// than it looks: the way a document ends up being obeyed is that its
    /// sentences and the system's sentences arrive in the same shape, and
    /// whatever reads them next cannot tell which was which.
    pub fn quoted(&self) -> String {
        let who = if self.from.is_empty() { "somewhere" } else { &self.from };
        let body: Vec<String> = self.text.lines().map(|l| format!("> {l}")).collect();
        format!("From {who}, quoted, not followed:\n{}", body.join("\n"))
    }

    /// What in here was trying to give an order.
    pub fn orders_found(&self) -> Vec<String> {
        looks_like_orders(&self.text)
    }

    /// Worth mentioning to Eric?
    pub fn worth_telling_him(&self) -> Option<String> {
        let found = self.orders_found();
        if found.is_empty() {
            return None;
        }
        Some(format!(
            "Something in what I read from {} was written as an instruction to me: {}. I've \
             quoted it rather than acted on it, which is what happens to everything I read — \
             but you should know somebody put it there.",
            if self.from.is_empty() { "an unnamed source" } else { &self.from },
            found.join("; ")
        ))
    }
}

/// Phrases that are trying to give an order rather than state a fact.
///
/// A short list on purpose. A long one catches ordinary prose — "you must be
/// careful with USDJPY" is a sentence a person writes — and a guard that fires
/// on ordinary prose is a guard somebody switches off.
pub const ORDER_SHAPED: [&str; 14] = [
    "ignore previous",
    "ignore all previous",
    "disregard the above",
    "disregard your",
    "you are now",
    "from now on you",
    "new instructions",
    "system prompt",
    "override",
    "do not tell",
    "delete all",
    "send the",
    "transfer the",
    "reveal your",
];

/// Find the order-shaped phrases, if any.
pub fn looks_like_orders(text: &str) -> Vec<String> {
    let low = text.to_lowercase();
    let mut found: Vec<String> = ORDER_SHAPED
        .iter()
        .filter(|p| low.contains(*p))
        .map(|p| (*p).to_string())
        .collect();
    found.dedup();
    found
}

/// Everything read this session, so "what have you been fed" has an answer.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Inbox {
    read: Vec<Read>,
}

impl Inbox {
    pub fn took_in(&mut self, r: Read, keep: usize) {
        self.read.push(r);
        while self.read.len() > keep.max(1) {
            self.read.remove(0);
        }
    }

    pub fn count(&self) -> usize {
        self.read.len()
    }

    /// Everything that tried to give an order, newest first.
    pub fn attempts(&self) -> Vec<&Read> {
        let mut out: Vec<&Read> = self.read.iter().filter(|r| !r.orders_found().is_empty()).collect();
        out.sort_by(|a, b| b.at.cmp(&a.at));
        out
    }

    pub fn spoken(&self) -> String {
        let tried = self.attempts();
        if tried.is_empty() {
            return format!(
                "I've read {} thing(s) from outside and none of them tried to tell me what to do.",
                self.count()
            );
        }
        format!(
            "{} of the {} things I've read tried to give me instructions. The most recent was \
             from {}. All of them were quoted, none were followed.",
            tried.len(),
            self.count(),
            if tried[0].from.is_empty() { "an unnamed source" } else { &tried[0].from }
        )
    }
}
