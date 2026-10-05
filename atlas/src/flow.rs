//! The workflow engine: multi-step work Atlas carries out on its own.
//!
//! A single command is one action. Real work is a chain — research a topic,
//! write it up, save it, open it. This runs those chains, passes each step's
//! output to the next, survives a failure partway through, and can pause for
//! approval in the middle without losing its place.
//!
//! Chains are recorded from what you actually did, so a sequence you repeat
//! becomes something you can name.

use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum OnFail {
    /// Stop the chain. The default: later steps usually assume earlier ones
    /// worked.
    #[default]
    Stop,
    /// Carry on — this step was optional.
    Continue,
    /// Try again, up to n times.
    Retry(u32),
}


#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Step {
    /// A command in the same language you would speak.
    pub command: String,
    #[serde(default)]
    pub on_fail: OnFail,
    /// Name for this step's output, referenced later as {name}.
    #[serde(default)]
    pub produces: Option<String>,
}

impl Step {
    pub fn new(command: &str) -> Step {
        Step { command: command.into(), on_fail: OnFail::Stop, produces: None }
    }
    pub fn optional(mut self) -> Step {
        self.on_fail = OnFail::Continue;
        self
    }
    pub fn retrying(mut self, n: u32) -> Step {
        self.on_fail = OnFail::Retry(n);
        self
    }
    pub fn producing(mut self, name: &str) -> Step {
        self.produces = Some(name.into());
        self
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Workflow {
    pub name: String,
    /// What you say to start it.
    #[serde(default)]
    pub triggers: Vec<String>,
    pub steps: Vec<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Running,
    /// Stopped mid-chain waiting for your yes. Keeps its position.
    AwaitingApproval,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub workflow: String,
    pub steps: Vec<Step>,
    pub position: usize,
    pub state: RunState,
    /// Named outputs, available to later steps as {name}.
    pub outputs: BTreeMap<String, String>,
    pub log: Vec<String>,
    attempts: u32,
    pub started: u64,
    /// The add-on this run belongs to, if it is one. Every step of such a run
    /// is checked by `plugins::may_run` before it executes -- what the step
    /// turned out to be, against what you still allow that add-on.
    #[serde(default)]
    pub plugin: Option<String>,
}

/// What the caller should do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Execute this command, then call `report`.
    Run(String),
    /// Ask this, then call `approve` or `deny`.
    Approve(String),
    Finished,
    Stopped(String),
}

impl Run {
    pub fn start(w: &Workflow) -> Run {
        Run {
            workflow: w.name.clone(),
            steps: w.steps.clone(),
            position: 0,
            state: RunState::Running,
            outputs: BTreeMap::new(),
            log: Vec::new(),
            attempts: 0,
            started: now(),
            plugin: None,
        }
    }

    /// A run of an add-on's sequence. See `plugins`.
    pub fn start_for_plugin(w: &Workflow, id: &str) -> Run {
        Run { plugin: Some(id.to_string()), ..Run::start(w) }
    }

    /// Stop the whole run here, whatever the step says about failing. For a
    /// step that was refused rather than one that failed: an optional step
    /// that was not *allowed* must not let the rest carry on.
    pub fn halt(&mut self, why: &str) {
        self.log.push(why.to_string());
        self.state = RunState::Failed;
    }

    pub fn current(&self) -> Option<&Step> {
        self.steps.get(self.position)
    }

    /// The next command, with earlier outputs substituted in. This is the
    /// content-staging piece: step three can use what step one produced.
    pub fn next(&self) -> Next {
        match self.state {
            RunState::Done => return Next::Finished,
            RunState::Failed => return Next::Stopped(self.log.last().cloned().unwrap_or_default()),
            RunState::AwaitingApproval => {
                let s = self.current().map(|s| s.command.clone()).unwrap_or_default();
                return Next::Approve(expand(&s, &self.outputs));
            }
            RunState::Running => {}
        }
        match self.current() {
            Some(s) => Next::Run(expand(&s.command, &self.outputs)),
            None => Next::Finished,
        }
    }

    /// Pause here until the user says yes.
    pub fn needs_approval(&mut self) {
        if self.state == RunState::Running {
            self.state = RunState::AwaitingApproval;
        }
    }

    pub fn approve(&mut self) {
        if self.state == RunState::AwaitingApproval {
            self.state = RunState::Running;
        }
    }

    /// Refused mid-chain. The rest is abandoned rather than half-done.
    pub fn deny(&mut self) {
        if self.state == RunState::AwaitingApproval {
            self.log.push(format!("step {} declined", self.position + 1));
            self.state = RunState::Failed;
        }
    }

    /// Report the outcome of the current step and move on.
    pub fn report(&mut self, output: &str, ok: bool) {
        if self.state != RunState::Running {
            return;
        }
        let Some(step) = self.steps.get(self.position).cloned() else {
            self.state = RunState::Done;
            return;
        };

        if ok {
            if let Some(name) = &step.produces {
                self.outputs.insert(name.clone(), output.to_string());
            }
            self.log.push(format!("{} -> {output}", step.command));
            self.attempts = 0;
            self.position += 1;
            if self.position >= self.steps.len() {
                self.state = RunState::Done;
            }
            return;
        }

        self.log.push(format!("{} FAILED: {output}", step.command));
        match step.on_fail {
            OnFail::Continue => {
                self.attempts = 0;
                self.position += 1;
                if self.position >= self.steps.len() {
                    self.state = RunState::Done;
                }
            }
            OnFail::Retry(n) => {
                self.attempts += 1;
                if self.attempts > n {
                    self.state = RunState::Failed;
                }
            }
            OnFail::Stop => self.state = RunState::Failed,
        }
    }

    pub fn finished(&self) -> bool {
        matches!(self.state, RunState::Done | RunState::Failed)
    }

    /// One line, for speaking. Long transcripts are unusable out loud.
    pub fn summary(&self) -> String {
        match self.state {
            RunState::Done => format!("{}: all {} steps done.", self.workflow, self.steps.len()),
            RunState::Failed => format!(
                "{}: stopped at step {} of {}.",
                self.workflow,
                self.position + 1,
                self.steps.len()
            ),
            RunState::AwaitingApproval => format!("{}: waiting on you.", self.workflow),
            RunState::Running => {
                format!("{}: step {} of {}.", self.workflow, self.position + 1, self.steps.len())
            }
        }
    }
}

/// `{name}` from a previous step's output. Unknown names are left alone so a
/// typo shows up rather than silently becoming empty.
pub fn expand(template: &str, outputs: &BTreeMap<String, String>) -> String {
    let mut out = String::with_capacity(template.len());
    let chars: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '{' {
            if let Some(close) = chars[i + 1..].iter().position(|c| *c == '}') {
                let key: String = chars[i + 1..i + 1 + close].iter().collect();
                if let Some(v) = outputs.get(&key) {
                    out.push_str(v);
                    i += close + 2;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    pub workflows: Vec<Workflow>,
}

impl Library {
    pub fn load(store: &Store) -> Library {
        store.load("workflows")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("workflows", self)
    }

    pub fn add(&mut self, w: Workflow) {
        self.workflows.retain(|x| x.name != w.name);
        self.workflows.push(w);
    }

    pub fn get(&self, name: &str) -> Option<&Workflow> {
        self.workflows.iter().find(|w| w.name.eq_ignore_ascii_case(name))
    }

    /// Match spoken text to a saved chain. Longest trigger wins, so a specific
    /// phrase is not shadowed by a general one.
    pub fn match_trigger(&self, said: &str) -> Option<&Workflow> {
        let t = said.trim().to_lowercase();
        let mut best: Option<(&Workflow, usize)> = None;
        for w in &self.workflows {
            for trig in &w.triggers {
                let g = trig.trim().to_lowercase();
                if g.is_empty() || !t.contains(&g) {
                    continue;
                }
                if best.map(|(_, l)| g.len() > l).unwrap_or(true) {
                    best = Some((w, g.len()));
                }
            }
        }
        best.map(|(w, _)| w)
    }

    /// Turn a sequence you just performed into a reusable chain.
    pub fn record(&mut self, name: &str, commands: &[String], trigger: Option<&str>) -> Workflow {
        let w = Workflow {
            name: name.to_string(),
            triggers: trigger.map(|t| vec![t.to_string()]).unwrap_or_default(),
            steps: commands.iter().map(|c| Step::new(c)).collect(),
        };
        self.add(w.clone());
        w
    }
}
