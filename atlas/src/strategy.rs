//! Trying different things, not the same thing repeatedly.
//!
//! "Three attempts then give up" was a bad rule, and you were right to push on
//! it. Three attempts is only meaningful if they're three *different*
//! attempts — and left to itself a model will happily retry the same idea
//! with the wording changed and call it a second try.
//!
//! So Atlas works through a ladder of genuinely distinct approaches. Each one
//! looks at the problem from a different angle, each is only used once, and it
//! stops when the ladder runs out rather than after an arbitrary count. That
//! means a hard problem gets ten real attempts instead of three lazy ones, and
//! an impossible one still terminates.

use serde::{Deserialize, Serialize};

/// A distinct way of attacking a stuck problem.
///
/// Ordered roughly by cost: cheap and often-right first, expensive and
/// last-resort at the end.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Angle {
    /// Read the actual error text again, carefully. Surprisingly often the
    /// answer is in the message and was skimmed past.
    ReadTheError,
    /// Check the thing being blamed is actually the thing that changed.
    CheckWhatChanged,
    /// Test the assumption the first theory rested on, rather than the theory.
    TestTheAssumption,
    /// Find somewhere else in the codebase that does this correctly and
    /// compare.
    FindAWorkingExample,
    /// Cut the problem in half — does the smaller version fail too?
    Bisect,
    /// Strip it to the smallest thing that still fails.
    Minimise,
    /// Add logging and run it again to see what's actually happening rather
    /// than what's assumed.
    Instrument,
    /// Undo the last change entirely and start from the working state.
    RevertAndRethink,
    /// Question whether the test is right, rather than the code.
    QuestionTheTest,
    /// Look at what calls this, rather than at this.
    LookUpstream,
    /// Look at what this calls.
    LookDownstream,
    /// Try the obvious brute-force version to see whether the clever one was
    /// the problem.
    TryTheSimpleWay,
}

impl Angle {
    /// The whole ladder, in order.
    pub const ALL: &'static [Angle] = &[
        Angle::ReadTheError,
        Angle::CheckWhatChanged,
        Angle::TestTheAssumption,
        Angle::FindAWorkingExample,
        Angle::Instrument,
        Angle::Minimise,
        Angle::Bisect,
        Angle::LookUpstream,
        Angle::LookDownstream,
        Angle::QuestionTheTest,
        Angle::TryTheSimpleWay,
        Angle::RevertAndRethink,
    ];

    /// The instruction handed to whichever brain is working on it.
    pub fn instruction(&self) -> &'static str {
        match self {
            Angle::ReadTheError =>
                "Read the error text again, word for word. What does it literally say is wrong? \
                 Do not reason from what you expect it to mean.",
            Angle::CheckWhatChanged =>
                "What actually changed most recently? Check that the thing being blamed is the \
                 thing that changed, rather than the thing that looks suspicious.",
            Angle::TestTheAssumption =>
                "The previous theory rested on an assumption. Name it, then test that assumption \
                 directly rather than testing the theory again.",
            Angle::FindAWorkingExample =>
                "Find somewhere else in this codebase that does the same kind of thing and works. \
                 Compare the two and list every difference.",
            Angle::Bisect =>
                "Cut the failing path in half. Does the first half alone fail? Narrow it down \
                 rather than guessing at the whole.",
            Angle::Minimise =>
                "Reduce this to the smallest thing that still fails. Remove everything that \
                 isn't needed to reproduce it.",
            Angle::Instrument =>
                "Stop reasoning and look. Add logging at each step and run it again, then report \
                 what actually happened rather than what should have.",
            Angle::RevertAndRethink =>
                "Undo every change so far and go back to the last state that worked. Then \
                 approach it differently from there.",
            Angle::QuestionTheTest =>
                "Consider that the test may be wrong rather than the code. What exactly is it \
                 asserting, and is that assertion correct?",
            Angle::LookUpstream =>
                "Look at what calls this. Is it being given what it expects?",
            Angle::LookDownstream =>
                "Look at what this calls. Is one of those doing something unexpected?",
            Angle::TryTheSimpleWay =>
                "Write the obvious brute-force version. If that works, the cleverness was the \
                 problem.",
        }
    }

    /// A short label for the log and the morning brief.
    pub fn label(&self) -> &'static str {
        match self {
            Angle::ReadTheError => "reread the error",
            Angle::CheckWhatChanged => "checked what changed",
            Angle::TestTheAssumption => "tested the assumption",
            Angle::FindAWorkingExample => "compared with a working example",
            Angle::Bisect => "bisected it",
            Angle::Minimise => "cut it down",
            Angle::Instrument => "added logging",
            Angle::RevertAndRethink => "reverted and started again",
            Angle::QuestionTheTest => "questioned the test",
            Angle::LookUpstream => "looked at the caller",
            Angle::LookDownstream => "looked at what it calls",
            Angle::TryTheSimpleWay => "tried the simple way",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct StrategyConfig {
    /// How many distinct angles to work through before giving up.
    ///
    /// The default is the whole ladder. There is no reason to stop at three
    /// when the remaining approaches are genuinely different from the ones
    /// already tried.
    pub max_angles: usize,
    /// Stop early if this many in a row produce the identical error — the
    /// angles are no longer telling us anything new.
    pub stop_after_identical: u32,
    /// Angles to skip.
    pub skip: Vec<Angle>,
}

impl Default for StrategyConfig {
    fn default() -> Self {
        StrategyConfig {
            max_angles: Angle::ALL.len(),
            stop_after_identical: 3,
            skip: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Effort {
    pub angle: Angle,
    /// What it learned, in a line.
    pub learned: String,
    /// The error afterwards, for comparing against the last one.
    pub error: String,
    pub solved: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Campaign {
    pub problem: String,
    pub efforts: Vec<Effort>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Try this angle.
    Try { angle: Angle, instruction: &'static str },
    /// Solved.
    Done,
    /// Out of angles. Time to write it up.
    Exhausted(String),
}

impl Campaign {
    pub fn new(problem: &str) -> Campaign {
        Campaign { problem: problem.to_string(), efforts: Vec::new() }
    }

    pub fn record(&mut self, e: Effort) {
        self.efforts.push(e);
    }

    fn used(&self) -> Vec<Angle> {
        self.efforts.iter().map(|e| e.angle).collect()
    }

    /// How many attempts in a row produced exactly the same error.
    fn identical_run(&self) -> u32 {
        let mut n = 0;
        let mut last: Option<&String> = None;
        for e in self.efforts.iter().rev() {
            match last {
                None => {
                    last = Some(&e.error);
                    n = 1;
                }
                Some(prev) if *prev == e.error && !e.error.is_empty() => n += 1,
                _ => break,
            }
        }
        n
    }

    /// What to do next.
    pub fn next(&self, cfg: &StrategyConfig) -> Next {
        if self.efforts.last().map(|e| e.solved).unwrap_or(false) {
            return Next::Done;
        }
        // The same error several times running means the angles aren't
        // reaching it. Continuing down the ladder won't help.
        if self.identical_run() >= cfg.stop_after_identical {
            return Next::Exhausted(format!(
                "the last {} attempts hit exactly the same error, so I stopped",
                self.identical_run()
            ));
        }
        if self.efforts.len() >= cfg.max_angles {
            return Next::Exhausted(format!("worked through {} approaches", self.efforts.len()));
        }
        let used = self.used();
        match Angle::ALL
            .iter()
            .find(|a| !used.contains(a) && !cfg.skip.contains(a))
        {
            Some(a) => Next::Try { angle: *a, instruction: a.instruction() },
            None => Next::Exhausted("tried every angle I have".into()),
        }
    }

    /// Everything learned along the way — the useful half of a failed
    /// campaign, and what makes the handoff brief worth reading.
    pub fn what_was_learned(&self) -> Vec<String> {
        self.efforts
            .iter()
            .filter(|e| !e.learned.trim().is_empty())
            .map(|e| format!("{}: {}", e.angle.label(), e.learned))
            .collect()
    }

    pub fn summary(&self) -> String {
        if self.efforts.is_empty() {
            return "Haven't started on it yet.".into();
        }
        if self.efforts.last().map(|e| e.solved).unwrap_or(false) {
            let last = self.efforts.last().unwrap();
            return format!(
                "Fixed it on the {} attempt — {}.",
                ordinal(self.efforts.len()),
                last.angle.label()
            );
        }
        format!(
            "{} approaches, none worked. Last was {}.",
            self.efforts.len(),
            self.efforts.last().unwrap().angle.label()
        )
    }
}

fn ordinal(n: usize) -> String {
    match n {
        1 => "first".into(),
        2 => "second".into(),
        3 => "third".into(),
        n => format!("{n}th"),
    }
}
