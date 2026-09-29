//! How long a thing stays true.
//!
//! `certainty` asks whether an answer looks invented. This asks a different
//! question that nothing was asking: *is what we stored still the case?*
//!
//! A note saying "TCP retransmits on timeout" and a note saying "the latest
//! release is 1.4.2" were stored identically — same struct, same timestamp,
//! same confidence when read back. One is true forever and one was true for a
//! fortnight. Recall ranked them the same and Atlas said them in the same
//! tone, which is the failure that makes a knowledge store worse than
//! nothing: a stale fact delivered confidently costs more than an absent one,
//! because you act on it.
//!
//! The whole thing is arithmetic on a timestamp. No model, no index, no
//! background pass — it costs nothing to run, which matters on a machine that
//! is also doing your actual work.
//!
//! What it does **not** do is decide truth. It decides how loudly to say
//! something, and when to offer to go and look again.

use serde::{Deserialize, Serialize};

/// How long this kind of claim stays true.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Shelf {
    /// True regardless of when you ask. How a protocol works, what a word
    /// means, why a thing is done that way.
    Settled,
    /// Yours. It expires when you change it, not on a clock — a preference
    /// from two years ago is still your preference until you say otherwise.
    Yours,
    /// Changes over months. Someone's job, a company's product line, a law.
    Slow,
    /// Changes over weeks. Software versions, prices of durable things,
    /// what's installed on a machine.
    Quick,
    /// Changes hourly or faster. Quotes, weather, whether something is up.
    /// Storing these is nearly pointless; saying them without the timestamp
    /// is worse than pointless.
    Volatile,
}

impl Shelf {
    /// Seconds after which the claim is worth half what it was.
    ///
    /// A half-life rather than an expiry date, because facts don't stop being
    /// true at midnight — confidence in them decays, and a cliff edge would
    /// make Atlas treat a 29-day-old fact as certain and a 31-day-old one as
    /// worthless.
    fn half_life_secs(&self) -> Option<u64> {
        match self {
            Shelf::Settled | Shelf::Yours => None,
            Shelf::Slow => Some(60 * 60 * 24 * 120),
            Shelf::Quick => Some(60 * 60 * 24 * 14),
            Shelf::Volatile => Some(60 * 60 * 6),
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Shelf::Settled => "doesn't change",
            Shelf::Yours => "yours until you change it",
            Shelf::Slow => "changes over months",
            Shelf::Quick => "changes over weeks",
            Shelf::Volatile => "changes constantly",
        }
    }

    /// Is it worth going back to the source for this?
    pub fn worth_rechecking(&self) -> bool {
        !matches!(self, Shelf::Settled | Shelf::Yours)
    }
}

/// Where a claim came from, kept so it can be gone back to.
///
/// A fact without a route back to its source can only be believed or
/// discarded. With one it can be checked, which is the difference between a
/// knowledge store and a pile of assertions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Checkable {
    /// A page. Re-fetch it.
    Page(String),
    /// A file on this machine. Re-read it.
    File(String),
    /// A command whose output was the fact.
    Ran(String),
    /// You said it. Only you can update it.
    YouSaid,
    /// A model produced it with nothing behind it. The weakest kind, and the
    /// one most worth marking, because it looks identical to the others once
    /// it's written down.
    ModelAlone,
}

impl Checkable {
    pub fn can_recheck_alone(&self) -> bool {
        matches!(self, Checkable::Page(_) | Checkable::File(_) | Checkable::Ran(_))
    }

    pub fn plain(&self) -> String {
        match self {
            Checkable::Page(u) => format!("from {u}"),
            Checkable::File(p) => format!("from {p}"),
            Checkable::Ran(c) => format!("from running {c}"),
            Checkable::YouSaid => "you told me".into(),
            Checkable::ModelAlone => "the model said so, with nothing behind it".into(),
        }
    }
}

/// A stored claim and everything needed to judge it later.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Known {
    pub claim: String,
    pub shelf: Shelf,
    pub source: Checkable,
    /// When it was last known to be true — not when the row was written.
    pub as_of: u64,
    /// Times it has been re-checked and still held. Raises confidence in the
    /// shelf classification, not in the claim.
    #[serde(default)]
    pub confirmed_times: u32,
}

/// How much of the original confidence survives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum State {
    /// Say it plainly.
    Fresh,
    /// Say it with its age attached.
    Ageing,
    /// Say it as something that was true, and offer to look again.
    Stale,
}

impl Known {
    pub fn new(claim: &str, shelf: Shelf, source: Checkable, as_of: u64) -> Self {
        Known {
            claim: claim.to_string(),
            shelf,
            source,
            as_of,
            confirmed_times: 0,
        }
    }

    pub fn age_secs(&self, now: u64) -> u64 {
        now.saturating_sub(self.as_of)
    }

    /// Remaining confidence, 0.0 to 1.0.
    ///
    /// Halves every half-life. Never reaches zero, because a two-year-old
    /// version number is still a better starting point than nothing — it just
    /// must not be said as though it were current.
    pub fn weight(&self, now: u64) -> f32 {
        let Some(half) = self.shelf.half_life_secs() else {
            return 1.0;
        };
        if half == 0 {
            return 1.0;
        }
        // Each time it's been confirmed and still held, the half-life stretches:
        // something re-checked four times is four times as slow to fade as
        // something seen once. This is spaced repetition — the mechanism by
        // which a memory that keeps proving true becomes hard to lose. A fact
        // confirmed zero times (the default, and everything before this) decays
        // exactly as before.
        let half = half.saturating_mul(1 + self.confirmed_times as u64);
        let periods = self.age_secs(now) as f32 / half as f32;
        0.5f32.powf(periods).clamp(0.02, 1.0)
    }

    pub fn state(&self, now: u64) -> State {
        match self.weight(now) {
            w if w >= 0.75 => State::Fresh,
            w if w >= 0.35 => State::Ageing,
            _ => State::Stale,
        }
    }

    /// Should Atlas go and look again before answering?
    pub fn should_recheck(&self, now: u64) -> bool {
        self.shelf.worth_rechecking()
            && self.source.can_recheck_alone()
            && self.state(now) != State::Fresh
    }

    /// The claim, said at the volume it has earned.
    ///
    /// The point of the whole module. A stale claim is not withheld — it is
    /// said differently, because "I don't know" is rarely more useful than
    /// "here's what was true in March, want me to check?"
    pub fn spoken(&self, now: u64) -> String {
        match self.state(now) {
            State::Fresh => self.claim.clone(),
            State::Ageing => format!("{} — that's from {}", self.claim, ago(self.age_secs(now))),
            State::Stale if self.source.can_recheck_alone() => format!(
                "{} — but that was {} and it's the kind of thing that changes. Want me to look again?",
                self.claim,
                ago(self.age_secs(now))
            ),
            State::Stale => format!(
                "{} — {} though, and I can't check it myself.",
                self.claim,
                ago(self.age_secs(now))
            ),
        }
    }
}

/// Rough, readable age. Deliberately imprecise: "three weeks ago" is what you
/// need, "19 days and 4 hours" is what a machine would say.
pub fn ago(secs: u64) -> String {
    const MIN: u64 = 60;
    const HOUR: u64 = 60 * MIN;
    const DAY: u64 = 24 * HOUR;
    match secs {
        s if s < 2 * MIN => "just now".into(),
        s if s < HOUR => format!("{} minutes ago", s / MIN),
        s if s < 2 * DAY => format!("{} hours ago", s / HOUR),
        s if s < 14 * DAY => format!("{} days ago", s / DAY),
        s if s < 60 * DAY => format!("{} weeks ago", s / (7 * DAY)),
        s if s < 730 * DAY => format!("{} months ago", s / (30 * DAY)),
        s => format!("{} years ago", s / (365 * DAY)),
    }
}

// ---------------------------------------------------------------------------
// Guessing the shelf
// ---------------------------------------------------------------------------

const VOLATILE_WORDS: &[&str] = &[
    "price", "quote", "cost right now", "weather", "temperature", "is up",
    "is down", "online", "offline", "traffic", "queue", "balance",
    "open now", "closed now", "trending", "score",
];

const QUICK_WORDS: &[&str] = &[
    "version", "latest", "current release", "installed", "update", "patch",
    "build", "changelog", "release notes", "deprecated", "beta", "as of today",
    "this week", "available now",
];

const SLOW_WORDS: &[&str] = &[
    "ceo", "president", "prime minister", "works at", "employed", "owns",
    "headquarters", "law", "regulation", "rate", "policy", "acquired",
    "merged", "renamed", "discontinued",
];

const SETTLED_WORDS: &[&str] = &[
    "how it works", "why", "means", "definition", "protocol", "algorithm",
    "theorem", "born in", "invented", "founded in", "was released in",
    "history of", "derives from", "stands for",
];

/// Best guess at how long a claim stays true, from its wording.
///
/// Keyword matching, and it will be wrong sometimes. It is ordered so that
/// being wrong is cheap: the check runs most-perishable first, so an ambiguous
/// claim gets the shorter shelf. Over-flagging costs you a sentence about
/// dates. Under-flagging costs you acting on something false.
pub fn shelf_for(claim: &str) -> Shelf {
    let c = claim.to_lowercase();
    if VOLATILE_WORDS.iter().any(|w| c.contains(w)) {
        return Shelf::Volatile;
    }
    if QUICK_WORDS.iter().any(|w| c.contains(w)) {
        return Shelf::Quick;
    }
    if SLOW_WORDS.iter().any(|w| c.contains(w)) {
        return Shelf::Slow;
    }
    if SETTLED_WORDS.iter().any(|w| c.contains(w)) {
        return Shelf::Settled;
    }
    // The default is the middle, not the bottom. An unclassified claim
    // treated as settled is one Atlas will still be asserting in a year.
    Shelf::Slow
}

/// Multiplier for a recall score, so fresh beats stale at equal relevance.
///
/// Bounded below so a stale-but-exact match still outranks a fresh-but-vague
/// one. Freshness breaks ties; it does not overrule relevance.
pub fn ranking_multiplier(k: &Known, now: u64) -> f32 {
    0.55 + 0.45 * k.weight(now)
}
