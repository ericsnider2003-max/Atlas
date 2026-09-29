//! Remembering what didn't work.
//!
//! Phase 3, and the thing that stops Atlas being annoying over months rather
//! than over one session. Without it, every failure is new: the same approach
//! gets tried again in a fortnight, the same dead end gets walked into, and
//! you have to say "we tried that" yourself.
//!
//! Two rules keep this from becoming a system that refuses to do anything.
//! What failed is recorded with **why**, so a failure caused by something
//! since fixed doesn't count forever. And a lesson expires — the world moves,
//! and a site that blocked automation in March may not in July.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cause {
    /// Something about the world: a site changed, a service was down.
    Outside,
    /// Atlas did it wrong.
    MyFault,
    /// It was never going to work — a wrong approach.
    WrongIdea,
    /// You stopped it.
    YouSaidNo,
    /// It ran out of time or budget.
    RanOut,
}

impl Cause {
    /// How long the lesson is worth keeping, in days.
    ///
    /// A wrong idea stays wrong. The world moves on.
    fn keep_days(&self) -> u64 {
        match self {
            Cause::WrongIdea => 3650,
            Cause::YouSaidNo => 365,
            Cause::MyFault => 60,
            // A site that blocked automation in March may not in July.
            Cause::Outside => 21,
            Cause::RanOut => 30,
        }
    }

    /// Is it worth trying again on its own, or does something have to change
    /// first?
    fn retry_alone(&self) -> bool {
        matches!(self, Cause::Outside | Cause::RanOut)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lesson {
    /// What was attempted, in the terms Atlas would use next time.
    pub approach: String,
    /// Where — the site, the app, the task.
    pub context: String,
    pub cause: Cause,
    /// What actually happened.
    pub what_happened: String,
    pub at: u64,
    /// How many times this has now failed.
    pub times: u32,
}

impl Lesson {
    pub fn stale(&self, now: u64) -> bool {
        let age_days = now.saturating_sub(self.at) / 86_400;
        age_days > self.cause.keep_days()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Learned {
    pub lessons: Vec<Lesson>,
}

/// What to do with an approach you're about to try.
#[derive(Debug, Clone, PartialEq)]
pub enum Advice {
    /// No history. Go ahead.
    Fresh,
    /// Failed before, but worth another go.
    TryAgain { because: String },
    /// Failed repeatedly for a reason that hasn't changed.
    Dont { because: String, suggest: Option<String> },
}

impl Learned {
    pub fn record(&mut self, approach: &str, context: &str, cause: Cause, what: &str, at: u64) {
        if let Some(existing) = self
            .lessons
            .iter_mut()
            .find(|l| l.approach == approach && l.context == context)
        {
            existing.times += 1;
            existing.at = at;
            existing.cause = cause;
            existing.what_happened = what.into();
            return;
        }
        self.lessons.push(Lesson {
            approach: approach.into(),
            context: context.into(),
            cause,
            what_happened: what.into(),
            at,
            times: 1,
        });
    }

    /// Drop lessons that have aged out.
    pub fn forget_stale(&mut self, now: u64) -> usize {
        let before = self.lessons.len();
        self.lessons.retain(|l| !l.stale(now));
        before - self.lessons.len()
    }

    /// Should Atlas try this?
    pub fn advise(&self, approach: &str, context: &str, now: u64) -> Advice {
        let Some(l) = self
            .lessons
            .iter()
            .find(|l| l.approach == approach && l.context == context && !l.stale(now))
        else {
            return Advice::Fresh;
        };

        // Once is bad luck, especially when it was the world's fault.
        if l.times == 1 && l.cause.retry_alone() {
            return Advice::TryAgain {
                because: format!("it failed once — {} — but that may have changed", l.what_happened),
            };
        }
        if l.cause == Cause::WrongIdea {
            return Advice::Dont {
                because: format!("{} — that approach doesn't work here", l.what_happened),
                suggest: self.alternative(context, approach),
            };
        }
        if l.times >= 2 {
            return Advice::Dont {
                because: format!(
                    "tried {} times, most recently {}",
                    l.times, l.what_happened
                ),
                suggest: self.alternative(context, approach),
            };
        }
        Advice::TryAgain { because: format!("only failed once, {}", l.what_happened) }
    }

    /// Something that worked in the same place, if there is one.
    fn alternative(&self, context: &str, avoid: &str) -> Option<String> {
        self.lessons
            .iter()
            .find(|l| l.context == context && l.approach != avoid && l.cause == Cause::Outside)
            .map(|l| l.approach.clone())
    }

    /// Everything known about one place, for "what have we tried here?"
    pub fn about(&self, context: &str, now: u64) -> Vec<&Lesson> {
        self.lessons.iter().filter(|l| l.context == context && !l.stale(now)).collect()
    }
}

/// What Atlas says instead of trying again.
///
/// Naming the count matters — "we've tried that twice" is an argument, "that
/// won't work" is an assertion.
pub fn spoken(a: &Advice) -> String {
    match a {
        Advice::Fresh => String::new(),
        Advice::TryAgain { because } => format!("Worth another go — {because}."),
        Advice::Dont { because, suggest } => match suggest {
            Some(alt) => format!("I'd rather not — {because}. {alt} worked before."),
            None => format!("I'd rather not — {because}."),
        },
    }
}
