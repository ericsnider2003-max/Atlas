//! Why Atlas isn't trading.
//!
//! Every refusal in [`crate::levels`] is a decision, and until now every one of
//! them was printed once and thrown away. That is half a record. A system that
//! keeps only what it did, and none of what it declined to do, cannot answer
//! the question that matters most when nothing is happening: **is it being
//! careful, or is it broken?**
//!
//! Those two look identical from outside. A reader that refuses everything
//! because the market genuinely offers nothing, and a reader that refuses
//! everything because its cost limit is set for a timeframe it is not on, both
//! produce silence. One is the system working and one is a bug that will never
//! announce itself — nothing errors, nothing looks wrong, and the account just
//! never trades.
//!
//! ## What the shape of the refusals tells you
//!
//! Counted by cause, the answer is usually obvious at a glance:
//!
//! - Nearly all **costs too much** → the timeframe is too small for this
//!   spread. Not a market condition; an arithmetic one, and no amount of
//!   waiting fixes it.
//! - Nearly all **inside the noise** → the stop rule is tighter than the
//!   market's ordinary movement. Same thing from the other side.
//! - Nearly all **standing down** → either the calendar is stuck on, or Atlas
//!   is only ever asked during release windows.
//! - Nearly all **not enough room** → the minimum reward is set above what
//!   this market actually offers.
//! - A **spread** of causes → this is a reader being careful, which is what it
//!   was built for.
//!
//! None of that is derivable from the ideas Atlas *did* produce. It is only
//! visible in what it turned down.
//!
//! ## Deliberately not a judgement
//!
//! This counts and reports. It does not decide that a limit is wrong and
//! change it — nothing here writes to `Rules`. A module that quietly loosened
//! its own limits because it had been refusing a lot would be a system that
//! talks itself into trades, which is the exact failure the refusals exist to
//! prevent.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

/// How many worked examples to keep per cause. Enough to read one and see what
/// the numbers looked like; few enough that the file cannot grow without
/// bound.
pub const KEEP_EXAMPLES: usize = 3;

/// One cause, and how often it came up.
///
/// `Turned` rather than `Cause`: `learned::Cause` already exists.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Turned {
    pub label: String,
    pub times: u32,
    /// The last time this cause came up, in seconds since the epoch.
    pub last: u64,
    /// A few of the reasons in full, so a count can be read back into a case.
    pub examples: Vec<String>,
}

/// What Atlas declined, per instrument.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Refusals {
    /// Keyed by instrument, then cause. Kept as a list rather than a map so
    /// the file reads in a stable order — a diff that reshuffles every save is
    /// a diff nobody reads.
    pub by_pair: Vec<(String, Vec<Turned>)>,
    /// Ideas that were actually produced, for the same instruments. Without
    /// this the counts have no denominator and "refused forty times" means
    /// nothing.
    pub proposed: Vec<(String, u32)>,
}

impl Refusals {
    pub fn load(store: &Store) -> Self {
        store.load("refusals")
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("refusals", self)
    }

    /// Record one refusal.
    pub fn declined(&mut self, pair: &str, label: &str, reason: &str, now: u64) {
        let pair = pair.to_uppercase();
        let causes = match self.by_pair.iter_mut().find(|(p, _)| *p == pair) {
            Some((_, c)) => c,
            None => {
                self.by_pair.push((pair.clone(), Vec::new()));
                &mut self.by_pair.last_mut().expect("just pushed").1
            }
        };
        match causes.iter_mut().find(|c| c.label == label) {
            Some(c) => {
                c.times = c.times.saturating_add(1);
                c.last = now;
                if c.examples.len() < KEEP_EXAMPLES && !c.examples.iter().any(|e| e == reason) {
                    c.examples.push(reason.to_string());
                }
            }
            None => causes.push(Turned {
                label: label.to_string(),
                times: 1,
                last: now,
                examples: vec![reason.to_string()],
            }),
        }
        causes.sort_by(|a, b| b.times.cmp(&a.times).then(a.label.cmp(&b.label)));
    }

    /// Record one idea that survived every check.
    pub fn proposed(&mut self, pair: &str) {
        let pair = pair.to_uppercase();
        match self.proposed.iter_mut().find(|(p, _)| *p == pair) {
            Some((_, n)) => *n = n.saturating_add(1),
            None => self.proposed.push((pair, 1)),
        }
    }

    pub fn refusals_for(&self, pair: &str) -> u32 {
        let pair = pair.to_uppercase();
        self.by_pair
            .iter()
            .find(|(p, _)| *p == pair)
            .map(|(_, c)| c.iter().map(|c| c.times).sum())
            .unwrap_or(0)
    }

    pub fn ideas_for(&self, pair: &str) -> u32 {
        let pair = pair.to_uppercase();
        self.proposed.iter().find(|(p, _)| *p == pair).map(|(_, n)| *n).unwrap_or(0)
    }

    /// The cause that dominates, and its share — but only once there is enough
    /// to look at.
    ///
    /// `None` below [`ENOUGH_TO_LOOK`], because one cause out of three
    /// refusals is not a pattern and reporting it as one is how a reader talks
    /// itself into changing a limit on noise.
    pub fn dominant(&self, pair: &str) -> Option<(String, f64)> {
        let pair = pair.to_uppercase();
        let (_, causes) = self.by_pair.iter().find(|(p, _)| *p == pair)?;
        let total: u32 = causes.iter().map(|c| c.times).sum();
        if total < ENOUGH_TO_LOOK {
            return None;
        }
        let top = causes.iter().max_by_key(|c| c.times)?;
        Some((top.label.clone(), top.times as f64 / total as f64))
    }

    /// What a dominant cause means, when one cause is nearly all of them.
    ///
    /// Says what to look at. Deliberately does not change anything: a module
    /// that loosened its own limits because it had been refusing a lot is a
    /// system that talks itself into trades.
    pub fn what_that_suggests(&self, pair: &str) -> Option<String> {
        let (label, share) = self.dominant(pair)?;
        if share < LOPSIDED {
            return None;
        }
        let said = match label.as_str() {
            "costs too much" => "the timeframe is too small for this spread — that is arithmetic, \
                                 not a market condition, and waiting does not fix it",
            "inside the noise" => "the stop rule is tighter than this market's ordinary movement, \
                                   so every stop it works out is inside the noise",
            "not enough room" => "the minimum reward is set above what this market is offering",
            "standing down" => "either the calendar is effectively always on here, or I'm only \
                                ever being asked during release windows",
            "thin book" => "I'm being asked at the rollover almost every time — twenty minutes \
                            either side would change the answer",
            "no structure" => "there isn't enough structure in what I'm being handed to hang a \
                              stop on; that usually means too few bars",
            "unreadable" => "the bars themselves are the problem, not the market",
            "no money" => "the balance and value-per-point I'm being given don't describe an \
                           account",
            _ => return None,
        };
        Some(format!(
            "{:.0}% of what I turned down on {pair} was for one reason — {said}.",
            share * 100.0
        ))
    }

    /// Said out loud, for one instrument.
    pub fn spoken(&self, pair: &str) -> String {
        let pair_up = pair.to_uppercase();
        let refused = self.refusals_for(&pair_up);
        let ideas = self.ideas_for(&pair_up);
        if refused == 0 && ideas == 0 {
            return format!("I haven't looked at {pair_up} yet.");
        }
        let mut said = format!(
            "On {pair_up} I've found {ideas} trade(s) and turned down {refused}.",
        );
        if let Some((_, causes)) = self.by_pair.iter().find(|(p, _)| *p == pair_up) {
            let top: Vec<String> = causes
                .iter()
                .take(3)
                .map(|c| format!("{} ({})", c.label, c.times))
                .collect();
            if !top.is_empty() {
                said.push_str(&format!(" Mostly: {}.", top.join(", ")));
            }
        }
        match self.what_that_suggests(&pair_up) {
            Some(s) => {
                said.push(' ');
                said.push_str(&s);
            }
            None if refused >= ENOUGH_TO_LOOK => said.push_str(
                " No single cause dominates, which is what a reader being careful looks like.",
            ),
            None => said.push_str(&format!(
                " That is fewer than {ENOUGH_TO_LOOK}, so I'm not going to read anything into the \
                 shape of it yet."
            )),
        }
        said
    }
}

/// How many refusals before the shape of them is worth reading.
///
/// Thirty, matching `judgment::MIN_SAMPLE`, and for the same reason: the number
/// below which a pattern is indistinguishable from noise.
pub const ENOUGH_TO_LOOK: u32 = 30;

/// The share of one cause that counts as lopsided.
///
/// A judgement, and named as one. Two thirds of everything coming back for a
/// single reason is a setting, not a market.
pub const LOPSIDED: f64 = 0.66;
