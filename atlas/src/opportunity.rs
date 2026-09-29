//! Whether an opportunity is worth your time.
//!
//! `decide` works a choice you are already facing. This is for the other
//! shape: something crossed your feed and looks like it might be worth doing.
//!
//! Five axes, and the reason there are five is that any one of them alone is
//! the way people talk themselves into things. Money alone gets you a plan
//! that pays well and that you will abandon in March. Fit alone gets you the
//! thing you would enjoy that nobody will pay for. The point of the frame is
//! that a weak axis is visible instead of getting averaged away.
//!
//! **Nothing is scored on a hunch.** Every axis carries what it rests on, and
//! an axis with nothing behind it counts as unknown rather than as neutral.
//! Neutral is the more dangerous default: it lets an opportunity nobody has
//! examined score the same as one that was examined and came out middling.
//!
//! Money is the axis that cannot be answered honestly yet, and it says so
//! rather than guessing. Until Atlas can read the actual accounts, a figure
//! here would be a number with the authority of a measurement and the content
//! of a wish.

use serde::{Deserialize, Serialize};

/// The five things worth knowing before you start something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Axis {
    /// What it actually takes. Hours, over how long, and whose.
    Work,
    /// Whether the thing is real. Does the demand exist, or only the pitch?
    Authenticity,
    /// Whether there is a route from here to there, with named steps.
    Roadmap,
    /// What it costs and what it returns, against your real position.
    Money,
    /// Whether it suits you specifically, not a generic person.
    Fit,
}

impl Axis {
    pub fn all() -> [Axis; 5] {
        [Axis::Work, Axis::Authenticity, Axis::Roadmap, Axis::Money, Axis::Fit]
    }

    /// What this axis is asking, in the words you would use.
    pub fn asks(&self) -> &'static str {
        match self {
            Axis::Work => "How many hours, over how long, and are they yours?",
            Axis::Authenticity => "Is there real demand here, or only a good pitch?",
            Axis::Roadmap => "What are the first three steps, and what stops after each?",
            Axis::Money => "What does it cost to start, and what does it return?",
            Axis::Fit => "What about you specifically makes this yours to do?",
        }
    }

    /// Can Atlas answer this on its own?
    ///
    /// Named per-axis rather than left implicit, because an assistant that
    /// answers all five equally confidently is hiding which ones it guessed.
    pub fn atlas_can_judge(&self) -> bool {
        match self {
            // Estimable from a described plan.
            Axis::Work | Axis::Roadmap => true,
            // Needs looking at the world.
            Axis::Authenticity => true,
            // Needs your accounts. Not yet.
            Axis::Money => false,
            // Needs to know you, which it does, but the call is yours.
            Axis::Fit => false,
        }
    }
}

/// How an axis came out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Finding {
    /// Scored, with what the score rests on.
    Scored { out_of_ten: u8, rests_on: Vec<String> },
    /// Looked at and could not be answered, with why.
    Unknown(String),
    /// Cannot be answered until something is true — Atlas being online,
    /// finances connected, a question answered.
    Blocked(String),
}

impl Finding {
    pub fn score(&self) -> Option<u8> {
        match self {
            Finding::Scored { out_of_ten, .. } => Some(*out_of_ten),
            _ => None,
        }
    }

    /// An axis with nothing behind it is not a score.
    fn is_grounded(&self) -> bool {
        matches!(self, Finding::Scored { rests_on, .. } if !rests_on.is_empty())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Look {
    pub axis: Axis,
    pub finding: Finding,
}

/// What to do about it.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Worth starting, with the reason.
    Worth { because: Vec<String>, weakest: Axis },
    /// Not worth it, naming the axis that sank it.
    SetAside { axis: Axis, why: String },
    /// Cannot say yet, and here is the one thing that would settle it.
    NeedFirst(String),
}

/// One opportunity, being weighed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Opportunity {
    pub what: String,
    /// Where it came from. A pitch you saw and a problem you hit are not the
    /// same evidence, and the difference is worth keeping.
    pub found_via: String,
    pub looks: Vec<Look>,
}

/// Below this, an axis sinks the whole thing regardless of the others.
///
/// A hard floor rather than a weighting, because averaging is exactly how a
/// fatal weakness disappears: four eights and a one averages to a respectable
/// six-and-a-half, and the one is the thing that will actually happen.
pub const FATAL_BELOW: u8 = 3;

impl Opportunity {
    pub fn new(what: &str, found_via: &str) -> Opportunity {
        Opportunity {
            what: what.to_string(),
            found_via: found_via.to_string(),
            looks: Vec::new(),
        }
    }

    pub fn note(&mut self, axis: Axis, finding: Finding) {
        match self.looks.iter_mut().find(|l| l.axis == axis) {
            Some(l) => l.finding = finding,
            None => self.looks.push(Look { axis, finding }),
        }
    }

    pub fn finding(&self, axis: Axis) -> Option<&Finding> {
        self.looks.iter().find(|l| l.axis == axis).map(|l| &l.finding)
    }

    /// Axes nobody has looked at yet.
    pub fn unlooked(&self) -> Vec<Axis> {
        Axis::all()
            .into_iter()
            .filter(|a| self.finding(*a).is_none())
            .collect()
    }

    /// Axes that are waiting on something.
    pub fn blocked(&self) -> Vec<(Axis, String)> {
        self.looks
            .iter()
            .filter_map(|l| match &l.finding {
                Finding::Blocked(why) => Some((l.axis, why.clone())),
                _ => None,
            })
            .collect()
    }

    /// The call.
    ///
    /// Order matters. A fatal axis is reported before a missing one, because
    /// "this will never work and here is why" is more use than "I still need
    /// three more answers first".
    pub fn verdict(&self) -> Verdict {
        // Anything already sunk?
        for l in &self.looks {
            if let Some(n) = l.finding.score() {
                if n < FATAL_BELOW {
                    return Verdict::SetAside {
                        axis: l.axis,
                        why: match &l.finding {
                            Finding::Scored { rests_on, .. } => rests_on.join("; "),
                            _ => String::new(),
                        },
                    };
                }
            }
        }
        if let Some(a) = self.unlooked().first() {
            return Verdict::NeedFirst(a.asks().to_string());
        }
        if let Some((_, why)) = self.blocked().first() {
            return Verdict::NeedFirst(why.clone());
        }
        // An ungrounded score is not a score.
        for l in &self.looks {
            if !l.finding.is_grounded() {
                if let Finding::Unknown(why) = &l.finding {
                    return Verdict::NeedFirst(why.clone());
                }
                return Verdict::NeedFirst(l.axis.asks().to_string());
            }
        }

        let weakest = self
            .looks
            .iter()
            .filter(|l| l.finding.score().is_some())
            .min_by_key(|l| l.finding.score().unwrap_or(10))
            .map(|l| l.axis)
            .unwrap_or(Axis::Fit);

        let mut because: Vec<String> = self
            .looks
            .iter()
            .filter_map(|l| match &l.finding {
                Finding::Scored { out_of_ten, rests_on } => Some(format!(
                    "{:?} {}/10 — {}",
                    l.axis,
                    out_of_ten,
                    rests_on.join("; ")
                )),
                _ => None,
            })
            .collect();
        because.push(format!("found via {}", self.found_via));
        Verdict::Worth { because, weakest }
    }

    /// How it reads when presented.
    ///
    /// Weakest axis first. The strong ones are why you would want to do it and
    /// you will find them yourself; the weak one is what you will hit in week
    /// three.
    pub fn presented(&self) -> String {
        match self.verdict() {
            Verdict::NeedFirst(q) => format!("{} — before anything else: {q}", self.what),
            Verdict::SetAside { axis, why } => format!(
                "{} — setting it aside on {axis:?}: {why}",
                self.what
            ),
            Verdict::Worth { because, weakest } => {
                let mut lines = vec![format!("{} — worth a look.", self.what)];
                lines.push(format!("Weakest part is {weakest:?}, so that's what to test first."));
                lines.extend(because);
                lines.join("\n")
            }
        }
    }
}

/// The money axis, until the accounts are readable.
///
/// A single place that says why, so the reason is the same wherever it
/// surfaces and stops being true in one commit rather than several.
pub fn money_is_not_auditable_yet() -> Finding {
    Finding::Blocked(
        "I can't judge the money on this until I can read your actual accounts. \
         Any figure I gave you now would have the authority of a measurement and \
         the content of a guess."
            .into(),
    )
}
