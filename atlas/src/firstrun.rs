//! The first time you run it.
//!
//! The alternative to this is a list of `FAIL` lines, which is what setup has
//! been so far. That's fine for someone who reads build output and useless for
//! anyone else — including you, on a day when you don't feel like it.
//!
//! So Atlas walks through it out loud instead. It finds what it can by itself,
//! asks only about what it genuinely can't work out, and lets you skip
//! anything and come back. Nothing here blocks: if you say "not now" to every
//! question it still ends up working, just with less.

use serde::{Deserialize, Serialize};

/// One thing to sort out.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Say hello and what this is.
    Hello,
    /// Look for the apps named in config.
    FindApps,
    /// Work out which monitor is which.
    Monitors,
    /// Try each microphone and see which hears you.
    Microphones,
    /// Let you hear the voices and pick one.
    Voice,
    /// Anything not installed yet.
    Missing,
    /// What it can do, in one sentence, and how to stop it.
    HowToUse,
    Done,
}

impl Step {
    /// What this step is, in words.
    ///
    /// Added when "finish setting up" became a thing you could say: the
    /// answer has to name what is left, and the alternative was printing a
    /// Rust variant at somebody -- which `tests/hub_is_not_code.rs` exists to
    /// stop and which is the same failure in a smaller hat.
    pub fn plain(&self) -> &'static str {
        match self {
            Step::Hello => "saying hello",
            Step::FindApps => "finding the apps you use",
            Step::Monitors => "working out which monitor is which",
            Step::Microphones => "trying your microphones",
            Step::Voice => "picking a voice",
            Step::Missing => "the things that aren't installed yet",
            Step::HowToUse => "how to use me, and how to stop me",
            Step::Done => "nothing — that's the end of it",
        }
    }

    /// Can Atlas do this without you?
    pub fn automatic(&self) -> bool {
        matches!(self, Step::FindApps | Step::Missing)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Answer {
    pub step: Step,
    /// What you said, or what Atlas found.
    pub value: String,
    pub skipped: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FirstRun {
    pub answers: Vec<Answer>,
    /// Steps you said to come back to later.
    pub deferred: Vec<Step>,
    pub finished: bool,
}

/// What Atlas should do or say next.
#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    /// Say this; nothing is expected back.
    Say(String),
    /// Ask this and wait.
    Ask { step: Step, question: String, examples: Vec<String> },
    /// Go and find something out, then carry on.
    Look { step: Step, what: String },
    /// Play the voices so you can choose.
    Audition,
    Finished(String),
}

const ORDER: &[Step] = &[
    Step::Hello,
    Step::FindApps,
    Step::Monitors,
    Step::Microphones,
    Step::Voice,
    Step::Missing,
    Step::HowToUse,
    Step::Done,
];

impl FirstRun {
    pub fn load(store: &crate::store::Store) -> FirstRun {
        store.load("firstrun")
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("firstrun", self)
    }

    pub fn done_with(&self, step: Step) -> bool {
        self.answers.iter().any(|a| a.step == step)
    }

    pub fn record(&mut self, step: Step, value: &str, skipped: bool) {
        self.answers.retain(|a| a.step != step);
        self.answers.push(Answer { step, value: value.into(), skipped });
        if skipped {
            self.deferred.push(step);
        }
    }

    /// Where we're up to.
    pub fn next(&mut self, found: &Found) -> Move {
        let Some(step) = ORDER.iter().copied().find(|s| !self.done_with(*s)) else {
            self.finished = true;
            return Move::Finished(self.closing());
        };

        match step {
            Step::Hello => Move::Say(
                "I'm Atlas. Give me two minutes and I'll work out where everything is. \
                 Say \"skip\" to any of this and I'll come back to it."
                    .into(),
            ),
            Step::FindApps => Move::Look {
                step,
                what: "looking for the apps you use".into(),
            },
            Step::Monitors => {
                if found.monitors <= 1 {
                    // Nothing to ask about with one screen.
                    self.record(Step::Monitors, "one screen", false);
                    return self.next(found);
                }
                Move::Ask {
                    step,
                    question: format!(
                        "I can see {} screens. Which one do you actually work on?",
                        found.monitors
                    ),
                    examples: vec!["the left one".into(), "the right one".into(), "skip".into()],
                }
            }
            Step::Microphones => {
                if found.microphones.is_empty() {
                    self.record(Step::Microphones, "none found", false);
                    return Move::Say(
                        "I can't find a microphone, so we'll type for now. \
                         Everything works typed."
                            .into(),
                    );
                }
                Move::Look {
                    step,
                    what: format!(
                        "testing {} microphone{} — say something when I ask",
                        found.microphones.len(),
                        if found.microphones.len() == 1 { "" } else { "s" }
                    ),
                }
            }
            Step::Voice => Move::Audition,
            Step::Missing => Move::Look { step, what: "checking what still needs installing".into() },
            Step::HowToUse => Move::Say(
                "That's it. Say \"call me\" and your name if you'd like me to use it, \"what can \
                 you do\" any time, \"stop everything\" if I get something wrong, and open \
                 RUN-SETTINGS if you'd rather change things yourself."
                    .into(),
            ),
            Step::Done => {
                self.finished = true;
                Move::Finished(self.closing())
            }
        }
    }

    /// The last thing it says, which is the only summary you get.
    fn closing(&self) -> String {
        let apps = self.value_of(Step::FindApps);
        let mic = self.value_of(Step::Microphones);
        let missing = self.value_of(Step::Missing);

        let mut parts = Vec::new();
        if !apps.is_empty() {
            parts.push(apps);
        }
        if !mic.is_empty() {
            parts.push(format!("listening on {mic}"));
        }
        let mut s = if parts.is_empty() {
            "Ready.".to_string()
        } else {
            format!("Ready — {}.", parts.join(", "))
        };
        if !missing.is_empty() {
            s.push_str(&format!(" Still to install: {missing}."));
        }
        if !self.deferred.is_empty() {
            s.push_str(&format!(
                " {} thing{} to come back to — say \"finish setting up\" when you want to.",
                self.deferred.len(),
                if self.deferred.len() == 1 { "" } else { "s" }
            ));
        }
        s
    }

    fn value_of(&self, step: Step) -> String {
        self.answers
            .iter()
            .find(|a| a.step == step && !a.skipped)
            .map(|a| a.value.clone())
            .unwrap_or_default()
    }

    /// Come back to what you skipped.
    pub fn resume(&mut self) -> Option<Step> {
        let step = self.deferred.pop()?;
        self.answers.retain(|a| a.step != step);
        self.finished = false;
        Some(step)
    }
}

/// What Atlas managed to work out by itself.
#[derive(Debug, Clone, Default)]
pub struct Found {
    pub apps: Vec<String>,
    pub missing_apps: Vec<String>,
    pub monitors: usize,
    pub microphones: Vec<String>,
    pub missing_tools: Vec<String>,
}

impl Found {
    /// What Atlas says after looking, which should be what it found rather
    /// than a count.
    pub fn report_apps(&self) -> String {
        match (self.apps.len(), self.missing_apps.len()) {
            (0, _) => "I couldn't find any of the apps in your config.".into(),
            (n, 0) => format!("found all {n}"),
            (n, _) => format!(
                "found {n}, but not {}",
                self.missing_apps.iter().take(2).cloned().collect::<Vec<_>>().join(" or ")
            ),
        }
    }

    pub fn report_missing(&self) -> String {
        if self.missing_tools.is_empty() {
            return String::new();
        }
        self.missing_tools.join(", ")
    }
}

/// Answers that mean "not now".
pub fn is_skip(said: &str) -> bool {
    let t = said.trim().to_lowercase();
    ["skip", "not now", "later", "next", "pass", "move on", "don't care", "dont care"]
        .iter()
        .any(|s| t == *s || t.starts_with(s))
}

/// Which screen you meant.
pub fn which_monitor(said: &str) -> Option<&'static str> {
    let t = said.to_lowercase();
    if t.contains("left") {
        Some("left")
    } else if t.contains("right") {
        Some("right")
    } else if t.contains("middle") || t.contains("centre") || t.contains("center") {
        Some("middle")
    } else if t.contains("this") || t.contains("here") {
        // Whichever one the pointer is on, which Atlas can see.
        Some("current")
    } else {
        None
    }
}
