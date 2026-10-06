//! Stopping before you have to ask what's taking so long.
//!
//! Phase 3. An assistant that works on something indefinitely is worse than
//! one that gives up, because you can't tell the difference between thinking
//! and stuck.
//!
//! So every piece of work gets a budget, and the budget is spent out loud: it
//! says how long it expects to take, tells you when it's over, and stops with
//! what it has rather than with nothing.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    /// Seconds. A lookup, a command.
    Quick,
    /// A minute or two. Reading something, a small job.
    Small,
    /// Ten minutes. Research, a batch of files.
    Long,
    /// You've said it can take as long as it takes.
    Open,
}

impl Size {
    fn budget_secs(&self) -> u64 {
        match self {
            Size::Quick => 20,
            Size::Small => 120,
            Size::Long => 600,
            Size::Open => 6 * 3600,
        }
    }

    /// Say up front how long this will take, when it's not obviously quick.
    pub fn warn_up_front(&self) -> Option<&'static str> {
        match self {
            Size::Quick => None,
            Size::Small => Some("a minute or two"),
            Size::Long => Some("about ten minutes"),
            Size::Open => Some("as long as it takes — I'll tell you when I'm done"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    Running,
    /// Over budget but making progress — worth asking rather than killing.
    Overrunning { by_secs: u64 },
    /// Stopped, with whatever it had.
    Stopped(Stopped),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stopped {
    Finished,
    /// Out of time, and here's what there is.
    OutOfTime { got: String },
    /// Not getting anywhere — stopping early is better than spending the rest
    /// of the budget proving it.
    Stalled { since_secs: u64 },
    YouStopped,
}

#[derive(Debug, Clone)]
pub struct Box_ {
    pub what: String,
    pub size: Size,
    started: u64,
    /// The last time something actually happened.
    last_progress: u64,
    /// What it has so far, so stopping still returns something.
    pub partial: String,
    pub state: State,
    /// Asked once whether to keep going.
    asked: bool,
}

/// Silence only means stuck relative to how long the job should take. Forty
/// seconds of nothing is alarming in a twenty-second job and unremarkable in
/// a ten-minute one, so the window scales with the budget.
fn stalled_after(size: Size) -> u64 {
    (size.budget_secs() / 2).max(15)
}

impl Box_ {
    pub fn start(what: &str, size: Size, now: u64) -> Box_ {
        Box_ {
            what: what.into(),
            size,
            started: now,
            last_progress: now,
            partial: String::new(),
            state: State::Running,
            asked: false,
        }
    }

    /// Something happened.
    pub fn progress(&mut self, got: &str, now: u64) {
        self.last_progress = now;
        if !got.is_empty() {
            self.partial = got.to_string();
        }
    }

    pub fn elapsed(&self, now: u64) -> u64 {
        now.saturating_sub(self.started)
    }

    /// Where things stand.
    pub fn check(&mut self, now: u64) -> &State {
        if matches!(self.state, State::Stopped(_)) {
            return &self.state;
        }
        let elapsed = self.elapsed(now);
        let quiet = now.saturating_sub(self.last_progress);

        // Stuck is different from slow, and worth catching earlier.
        if quiet >= stalled_after(self.size) && self.size != Size::Open {
            self.state = State::Stopped(Stopped::Stalled { since_secs: quiet });
            return &self.state;
        }
        if elapsed > self.size.budget_secs() {
            // Over budget once: ask. Over budget having already asked: stop.
            if self.asked {
                self.state = State::Stopped(Stopped::OutOfTime { got: self.partial.clone() });
            } else {
                self.state = State::Overrunning { by_secs: elapsed - self.size.budget_secs() };
            }
        }
        &self.state
    }

    pub fn finish(&mut self, got: &str) {
        self.partial = got.to_string();
        self.state = State::Stopped(Stopped::Finished);
    }

    pub fn stop(&mut self) {
        self.state = State::Stopped(Stopped::YouStopped);
    }

    /// What Atlas says about where it is.
    pub fn spoken(&self, now: u64) -> String {
        match &self.state {
            State::Running => format!("{} — {}s in.", self.what, self.elapsed(now)),
            State::Overrunning { by_secs } => format!(
                "{} is taking longer than I expected — {by_secs}s over. Keep going?",
                self.what
            ),
            State::Stopped(Stopped::Finished) => {
                let mut c = self.what.chars();
                let name = match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => String::new(),
                };
                format!("{name} done.")
            }
            State::Stopped(Stopped::OutOfTime { got }) => {
                if got.is_empty() {
                    format!("Stopped on {} — out of time and I had nothing to show.", self.what)
                } else {
                    // Something beats nothing, and you can decide whether it's
                    // enough.
                    format!("Stopped on {} — out of time. What I had: {got}", self.what)
                }
            }
            State::Stopped(Stopped::Stalled { since_secs }) => format!(
                "{} stopped moving {since_secs}s ago, so I stopped rather than wait it out.",
                self.what
            ),
            State::Stopped(Stopped::YouStopped) => String::new(),
        }
    }
}

/// Guess how big a job is from what was asked.
pub fn size_of(request: &str) -> Size {
    let t = request.to_lowercase();
    if ["research", "look into", "go through all", "everything", "compare all", "dig into"]
        .iter()
        .any(|w| t.contains(w))
    {
        return Size::Long;
    }
    if ["read", "summarise", "summarize", "check", "find", "look for", "tidy"]
        .iter()
        .any(|w| t.contains(w))
    {
        return Size::Small;
    }
    if ["overnight", "while i sleep", "take your time", "no rush"].iter().any(|w| t.contains(w)) {
        return Size::Open;
    }
    Size::Quick
}

/// How long a kind of work usually takes, from how long it took before:
/// the middle of the last few runs. `None` with fewer than two to go on.
pub fn usual_secs(past: &[u64]) -> Option<u64> {
    if past.len() < 2 {
        return None;
    }
    let mut v: Vec<u64> = past.iter().rev().take(5).copied().collect();
    v.sort_unstable();
    Some(v[v.len() / 2])
}

/// The up-front estimate, when it's worth saying (Eric, 25 Sep 2026, F6:
/// "yes but not annoyingly"). Only for work that usually takes five minutes
/// or more (quick things get no warning), and not again for the same kind
/// within two hours of the last time it was said.
pub fn estimate_worth_saying(usual: Option<u64>, open_ended: bool, said_last: Option<u64>, now: u64) -> Option<String> {
    if said_last.is_some_and(|at| now.saturating_sub(at) < 2 * 3600) {
        return None;
    }
    if open_ended {
        return Size::Open.warn_up_front().map(|s| format!("This one takes {s}."));
    }
    let secs = usual?;
    if secs < 300 {
        return None;
    }
    let mins = ((secs + 30) / 60).max(5);
    Some(if mins >= 90 {
        format!("This usually takes about {} hours.", (mins + 30) / 60)
    } else {
        format!("This usually takes about {mins} minutes.")
    })
}
