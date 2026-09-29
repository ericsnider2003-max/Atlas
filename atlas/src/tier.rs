//! How much work an answer needs.
//!
//! `route` picks *how* to do a task once Atlas has decided to do it. This
//! decides something earlier and cheaper: whether the task needs doing at all.
//!
//! Three tiers, and the middle one is the point.
//!
//! * **Run a named thing.** You said the name of something Atlas already
//!   knows how to do. There is nothing to work out — execute it and say when
//!   it is done.
//! * **Read from what's already there.** You asked about something Atlas
//!   worked out this morning. The answer is sitting in a report; going away to
//!   think about it again would be slower and no better.
//! * **Actually think.** Everything else. Slow because it should be.
//!
//! Most questions are tier two and get treated as tier three, which is why a
//! local assistant can feel sluggish on questions it already knows the answer
//! to. Waking a model to re-derive something computed an hour ago is the
//! single most common way a voice assistant wastes your time.
//!
//! **The fall-through only goes one way.** Guessing too low means answering
//! from something stale and being confidently out of date. Guessing too high
//! means being slower than necessary. So anything uncertain goes up, and a
//! report that cannot be shown to be current is not used at all.

use serde::{Deserialize, Serialize};

/// Where an answer should come from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Tier {
    /// A named capability. Execute it.
    RunNamed(String),
    /// Something already worked out.
    FromReport {
        report: String,
        /// How old it is, in words, so the answer can carry it.
        as_of: String,
    },
    /// Needs the model.
    Think {
        /// Why this could not be answered more cheaply. Recorded because a
        /// tier-three answer that should have been tier two is the thing
        /// worth noticing, and it is invisible unless the reason is kept.
        why: String,
    },
}

impl Tier {
    /// Roughly how long this will take, for deciding whether to say
    /// "working on it" first.
    pub fn slow(&self) -> bool {
        matches!(self, Tier::Think { .. })
    }
}

/// Something Atlas produced earlier that can answer questions.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Report {
    pub name: String,
    /// What it covers, in the words you would use asking about it.
    pub covers: Vec<String>,
    /// When it was produced.
    pub made_at: u64,
    /// How long its contents stay true.
    pub shelf: crate::freshness::Shelf,
}

impl Report {
    /// Is this still worth answering from?
    ///
    /// Uses the same decay as everything else, so a report about prices ages
    /// in hours while one about how something works does not age at all.
    pub fn still_good(&self, now: u64) -> bool {
        let k = crate::freshness::Known::new(
            &self.name,
            self.shelf,
            crate::freshness::Checkable::File(self.name.clone()),
            self.made_at,
        );
        k.state(now) == crate::freshness::State::Fresh
    }

    fn age(&self, now: u64) -> String {
        crate::freshness::ago(now.saturating_sub(self.made_at))
    }

    fn mentions(&self, words: &[String]) -> bool {
        self.covers
            .iter()
            .any(|c| words.iter().any(|w| c.to_lowercase().contains(w.as_str())))
    }
}

/// Words that mean "right now", which no stored report can answer.
///
/// Checked before anything else. A report made this morning is a perfectly
/// good answer to "what's on today" and a terrible one to "is it raining",
/// and the difference is in the question rather than in the report.
const ASKS_ABOUT_NOW: &[&str] = &[
    "right now", "currently", "at the moment", "just now", "still",
    "as of now", "this second", "live", "latest",
];

fn words_of(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(|w| w.to_string())
        .collect()
}

/// Decide where an answer should come from.
///
/// `named` is what Atlas can run by name. `reports` is what it has already
/// produced. Neither needs a model, so this costs nothing and runs before any
/// decision about whether to spend anything.
pub fn tier_for(said: &str, named: &[String], reports: &[Report], now: u64) -> Tier {
    let lower = said.to_lowercase();

    // 1. Did you name something?
    //
    // Longest match first, so "run the morning report" picks the morning
    // report rather than a shorter capability whose name is inside it.
    let mut matches: Vec<&String> = named
        .iter()
        .filter(|n| lower.contains(&n.to_lowercase()))
        .collect();
    matches.sort_by_key(|n| std::cmp::Reverse(n.len()));
    if let Some(n) = matches.first() {
        return Tier::RunNamed((*n).clone());
    }

    // 2. Are you asking about this moment? No stored answer can serve.
    if let Some(w) = ASKS_ABOUT_NOW.iter().find(|w| lower.contains(*w)) {
        return Tier::Think { why: format!("you asked about {w}, so nothing stored will do") };
    }

    // 3. Is it covered by something already worked out and still current?
    let words = words_of(said);
    let covering: Vec<&Report> = reports.iter().filter(|r| r.mentions(&words)).collect();
    if let Some(r) = covering.iter().find(|r| r.still_good(now)) {
        return Tier::FromReport { report: r.name.clone(), as_of: r.age(now) };
    }
    if let Some(stale) = covering.first() {
        // Covered, but not currently. Named rather than silently upgraded,
        // because "I have this but it's old" is a different fact from "I
        // don't have this".
        return Tier::Think {
            why: format!(
                "{} covers it but it's from {}, so I'd rather look again",
                stale.name,
                stale.age(now)
            ),
        };
    }

    Tier::Think { why: "nothing I've already worked out covers this".into() }
}

/// A tally of where answers came from.
///
/// Kept because the useful question is not whether tiering works but whether
/// it is being *used*: a run where everything is tier three means the reports
/// are not covering what you actually ask.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Mix {
    pub named: u32,
    pub from_report: u32,
    pub thought: u32,
}

impl Mix {
    pub fn note(&mut self, t: &Tier) {
        match t {
            Tier::RunNamed(_) => self.named += 1,
            Tier::FromReport { .. } => self.from_report += 1,
            Tier::Think { .. } => self.thought += 1,
        }
    }

    pub fn total(&self) -> u32 {
        self.named + self.from_report + self.thought
    }

    /// Is the middle tier earning its place?
    ///
    /// Returns a line only when it is not, so an ordinary day says nothing.
    pub fn worth_saying(&self) -> Option<String> {
        if self.total() < 10 {
            return None;
        }
        if self.from_report == 0 {
            return Some(
                "Nothing I've been asked lately was answerable from a report I'd already \
                 made. Either the reports aren't covering what you ask, or they're going \
                 stale before you ask."
                    .into(),
            );
        }
        None
    }
}
