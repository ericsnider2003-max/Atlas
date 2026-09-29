//! Doing something that crosses several apps.
//!
//! Phase 3. "Take the numbers out of the spreadsheet, put them in the report,
//! and send it to Marta" is three apps and four failure points, and the
//! interesting part is not the happy path — it's what happens when step three
//! fails after step two already changed something.
//!
//! Two rules make this safe enough to be worth having. Nothing irreversible
//! happens until every reversible step has succeeded, so a chain that's going
//! to fail fails before it sends anything. And a step Atlas isn't sure about
//! stops the chain rather than guessing, because a wrong guess halfway through
//! a chain is much worse than a wrong guess on its own.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// What it does, in your words.
    pub what: String,
    /// Which app.
    pub app: String,
    /// Can this be undone?
    pub reversible: bool,
    /// It leaves the machine — sending, posting, paying.
    pub goes_out: bool,
    /// What it needs from an earlier step.
    pub needs: Option<String>,
    /// What it produces for a later one.
    pub produces: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepState {
    Waiting,
    Running,
    Done,
    Failed,
    /// Undone after a later failure.
    RolledBack,
    /// Never reached.
    Skipped,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Chain {
    pub goal: String,
    pub steps: Vec<Step>,
    pub states: Vec<StepState>,
    /// What each step produced.
    pub outputs: Vec<(String, String)>,
    pub at: usize,
}

/// What should happen next.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Do this one.
    Run { index: usize, what: String },
    /// This one leaves the machine. Ask first, with everything that led here.
    Confirm { index: usize, what: String, context: String },
    /// Something's missing or ambiguous — ask rather than guess.
    Ask { index: usize, question: String },
    Finished(String),
    /// Stopped, and what was undone.
    Stopped { why: String, undone: Vec<String> },
}

impl Chain {
    pub fn new(goal: &str, steps: Vec<Step>) -> Chain {
        let states = vec![StepState::Waiting; steps.len()];
        Chain { goal: goal.into(), steps, states, outputs: Vec::new(), at: 0 }
    }

    /// Check the whole thing before starting any of it.
    ///
    /// A chain missing something at step four should not run steps one to
    /// three first.
    pub fn check(&self) -> Result<(), String> {
        let mut available: Vec<&str> = Vec::new();
        for s in &self.steps {
            if let Some(need) = &s.needs {
                if !available.contains(&need.as_str()) {
                    return Err(format!("\"{}\" needs {need}, and nothing before it makes one", s.what));
                }
            }
            if let Some(p) = &s.produces {
                available.push(p);
            }
        }
        // Sending before the thing being sent exists is the classic version of
        // this, and it's worth catching by name.
        if let Some(bad) = self.steps.iter().position(|s| s.goes_out) {
            if self.steps[bad + 1..].iter().any(|s| !s.reversible) {
                return Err("something irreversible happens after the send, which can't be undone".into());
            }
        }
        Ok(())
    }

    pub fn next(&self) -> Next {
        if self.at >= self.steps.len() {
            return Next::Finished(format!("{} — all {} steps.", self.goal, self.steps.len()));
        }
        let s = &self.steps[self.at];

        if let Some(need) = &s.needs {
            if !self.outputs.iter().any(|(k, _)| k == need) {
                return Next::Ask {
                    index: self.at,
                    question: format!("I need {need} for \"{}\" and haven't got one. Where from?", s.what),
                };
            }
        }

        if s.goes_out {
            // Everything that led here, so you're approving the result rather
            // than the instruction.
            let context = self
                .outputs
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join("; ");
            return Next::Confirm { index: self.at, what: s.what.clone(), context };
        }

        Next::Run { index: self.at, what: s.what.clone() }
    }

    pub fn done(&mut self, produced: Option<(String, String)>) {
        self.states[self.at] = StepState::Done;
        if let Some(p) = produced {
            self.outputs.push(p);
        }
        self.at += 1;
    }

    /// A step failed. Undo what can be undone, in reverse.
    pub fn failed(&mut self, why: &str) -> Next {
        self.states[self.at] = StepState::Failed;
        let mut undone = Vec::new();

        for i in (0..self.at).rev() {
            if self.states[i] == StepState::Done && self.steps[i].reversible {
                self.states[i] = StepState::RolledBack;
                undone.push(self.steps[i].what.clone());
            }
        }
        for i in self.at + 1..self.steps.len() {
            self.states[i] = StepState::Skipped;
        }
        Next::Stopped { why: format!("{} failed: {why}", self.steps[self.at].what), undone }
    }

    /// How far it got, for the morning brief.
    pub fn progress(&self) -> (usize, usize) {
        (self.states.iter().filter(|s| **s == StepState::Done).count(), self.steps.len())
    }

    /// What Atlas says as it goes. Named steps, not percentages.
    pub fn spoken(&self) -> String {
        let (done, total) = self.progress();
        if done == total {
            return format!("{} — done.", self.goal);
        }
        match self.steps.get(self.at) {
            Some(s) => format!("{}: {} — {} of {total}.", self.goal, s.what, done + 1),
            None => format!("{} — {done} of {total}.", self.goal),
        }
    }
}

/// A chain that couldn't be undone is worth saying so about, plainly.
pub fn what_stands(c: &Chain) -> Vec<&str> {
    c.steps
        .iter()
        .zip(&c.states)
        .filter(|(s, st)| **st == StepState::Done && !s.reversible)
        .map(|(s, _)| s.what.as_str())
        .collect()
}
