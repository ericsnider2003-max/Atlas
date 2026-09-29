//! Knowing when a long job is finished.
//!
//! `overnight` decides what is *safe* to run unattended. Nothing decides what
//! *finished* means, so a job that runs for six hours has no way to tell
//! whether it got there — and neither does the report in the morning.
//!
//! A long-horizon task is three things: a trigger, the work, and a way to
//! check. The third is the one that gets skipped, and skipping it is what
//! turns an overnight run into a pile of activity you have to read.
//!
//! The rule that makes it work: **the check has to be something a machine can
//! run.** "Make the report better" cannot be evaluated at 3am by the thing
//! that wrote it. "Every section has at least three sources and the build
//! passes" can. The more of the criterion that rests on judgement, the closer
//! the loop gets to a thing that runs forever and grades its own homework.
//!
//! So this refuses subjective criteria rather than accepting them and hoping.
//! A job that cannot say what done looks like is a job that should wait for
//! you, and saying so at the start costs a question. Saying nothing costs a
//! night.

use serde::{Deserialize, Serialize};

/// One thing that must be true before the job counts as finished.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Check {
    /// A command that must exit zero.
    CommandPasses(String),
    /// A file that must exist.
    FileExists(String),
    /// A file that must contain this.
    FileContains { path: String, text: String },
    /// A count that must reach a number — sources gathered, tests added.
    AtLeast { what: String, n: u32 },
    /// Something only a person can judge.
    ///
    /// Kept as a variant rather than rejected outright, because plenty of real
    /// goals are genuinely subjective. It just cannot be what a loop checks
    /// itself against, so a job carrying one of these waits for you instead of
    /// running unattended.
    YouDecide(String),
}

impl Check {
    /// Can this be settled without a person?
    fn machine_checkable(&self) -> bool {
        !matches!(self, Check::YouDecide(_))
    }

    pub fn plain(&self) -> String {
        match self {
            Check::CommandPasses(c) => format!("`{c}` exits clean"),
            Check::FileExists(p) => format!("{p} exists"),
            Check::FileContains { path, text } => format!("{path} contains \"{text}\""),
            Check::AtLeast { what, n } => format!("at least {n} {what}"),
            Check::YouDecide(w) => format!("you decide whether {w}"),
        }
    }
}

/// What a long job is trying to reach.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Goal {
    pub what: String,
    pub checks: Vec<Check>,
    /// Stop after this many attempts however it is going.
    ///
    /// Not optional. A loop with no cap is a loop that burns a night, or a
    /// bill, discovering that its criterion was unreachable.
    pub give_up_after: u32,
}

/// Why a goal cannot be run unattended.
#[derive(Debug, Clone, PartialEq)]
pub enum NotRunnable {
    /// No way to tell whether it worked.
    NoCheck,
    /// Every check needs a person.
    OnlyYouCanTell(Vec<String>),
    /// It would run forever.
    NoLimit,
}

impl NotRunnable {
    pub fn plain(&self) -> String {
        match self {
            NotRunnable::NoCheck => {
                "I don't know what finished looks like for this, so I'd be working \
                 without being able to tell you if it worked."
                    .into()
            }
            NotRunnable::OnlyYouCanTell(w) => format!(
                "The only way to know if this worked is for you to look at it ({}). \
                 I'd rather do it while you're around.",
                w.join("; ")
            ),
            NotRunnable::NoLimit => {
                "There's no point at which I'd stop, so this could run all night \
                 getting nowhere.".into()
            }
        }
    }
}

/// How a single attempt went.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Attempt {
    pub n: u32,
    /// Which checks passed this time.
    pub passed: Vec<Check>,
    pub failed: Vec<Check>,
    /// What was different from last time. Empty means it did the same thing
    /// again, which is the signal that looping further is pointless.
    pub changed: String,
}

impl Attempt {
    pub fn met_everything(&self) -> bool {
        self.failed.is_empty() && !self.passed.is_empty()
    }
}

/// Where a run stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Standing {
    /// Every check passed.
    Done,
    /// Keep going.
    TryAgain { attempts_left: u32 },
    /// Attempts ran out.
    GaveUp { after: u32, still_failing: Vec<String> },
    /// It is repeating itself without getting closer.
    ///
    /// Worth its own answer. Running the same attempt eight more times costs a
    /// night to learn what two attempts already showed.
    GoingInCircles { after: u32 },
}

impl Goal {
    pub fn new(what: &str, give_up_after: u32) -> Goal {
        Goal { what: what.to_string(), checks: Vec::new(), give_up_after }
    }

    pub fn checking(mut self, c: Check) -> Goal {
        self.checks.push(c);
        self
    }

    /// Can Atlas run this while you sleep?
    pub fn runnable_unattended(&self) -> Result<(), NotRunnable> {
        if self.checks.is_empty() {
            return Err(NotRunnable::NoCheck);
        }
        if self.give_up_after == 0 {
            return Err(NotRunnable::NoLimit);
        }
        if !self.checks.iter().any(|c| c.machine_checkable()) {
            return Err(NotRunnable::OnlyYouCanTell(
                self.checks.iter().map(|c| c.plain()).collect(),
            ));
        }
        Ok(())
    }

    /// Only the checks a loop can settle for itself.
    pub fn machine_checks(&self) -> Vec<&Check> {
        self.checks.iter().filter(|c| c.machine_checkable()).collect()
    }

    /// Where the run stands after these attempts.
    ///
    /// Order matters: finished beats out of attempts, and out of attempts
    /// beats going in circles — because a run that met its checks on the last
    /// attempt is done, not exhausted.
    pub fn standing(&self, attempts: &[Attempt]) -> Standing {
        let Some(last) = attempts.last() else {
            return Standing::TryAgain { attempts_left: self.give_up_after };
        };
        if last.met_everything() {
            return Standing::Done;
        }
        let used = attempts.len() as u32;
        if used >= self.give_up_after {
            return Standing::GaveUp {
                after: used,
                still_failing: last.failed.iter().map(|c| c.plain()).collect(),
            };
        }
        // Two attempts that changed nothing means the next eight will not
        // either.
        let stuck = attempts.len() >= 2
            && attempts.iter().rev().take(2).all(|a| a.changed.trim().is_empty());
        if stuck {
            return Standing::GoingInCircles { after: used };
        }
        Standing::TryAgain { attempts_left: self.give_up_after - used }
    }

    /// What to say about it in the morning.
    ///
    /// Leads with the standing rather than the effort. How many attempts it
    /// made is not the answer to whether it worked.
    pub fn spoken(&self, attempts: &[Attempt]) -> String {
        match self.standing(attempts) {
            Standing::Done => format!("{} — done, and it checks out.", self.what),
            Standing::GaveUp { after, still_failing } => format!(
                "{} — gave up after {after}. Still failing: {}.",
                self.what,
                still_failing.join("; ")
            ),
            Standing::GoingInCircles { after } => format!(
                "{} I stopped after {after} tries: it was repeating itself rather than getting \
                 closer, and carrying on would have burned the night.",
                crate::route::stuck_on(&self.what, "the same failure coming back each time")
            ),
            Standing::TryAgain { attempts_left } => {
                format!("{} — still going, {attempts_left} attempts left.", self.what)
            }
        }
    }
}

/// How long a job may run on its own (Eric, 25 Sep 2026, E3: "yes, don't
/// make the limit ridiculously small or it's pointless").
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LongJobConfig {
    /// Attempts before it stops however it's going.
    pub max_attempts: u32,
    /// Hours before it stops however it's going.
    pub max_hours: u32,
}

impl Default for LongJobConfig {
    fn default() -> Self {
        // Fifty tries or a working night, whichever comes first. Big enough
        // to be worth leaving; the "going in circles" check stops it far
        // sooner when it has stopped getting anywhere.
        LongJobConfig { max_attempts: 50, max_hours: 8 }
    }
}

/// Why a long run stopped, beyond what `Standing` says.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    Standing(Standing),
    OutOfTime { after: u32, hours: u32 },
    Stopped { after: u32 },
}

/// Run attempts until the goal is met, the attempts or hours run out, it goes
/// in circles, or you stop it. `try_once(n)` makes one attempt and says how it
/// went; `stop()` is asked between attempts.
pub fn keep_at_it(
    goal: &Goal,
    limits: &LongJobConfig,
    mut try_once: impl FnMut(u32) -> Attempt,
    mut stop: impl FnMut() -> bool,
    started: std::time::Instant,
) -> (Ended, Vec<Attempt>) {
    let mut attempts: Vec<Attempt> = Vec::new();
    let max_time = std::time::Duration::from_secs(limits.max_hours as u64 * 3600);
    loop {
        match goal.standing(&attempts) {
            Standing::TryAgain { .. } => {}
            s => return (Ended::Standing(s), attempts),
        }
        if stop() {
            let after = attempts.len() as u32;
            return (Ended::Stopped { after }, attempts);
        }
        if started.elapsed() >= max_time {
            let after = attempts.len() as u32;
            return (Ended::OutOfTime { after, hours: limits.max_hours }, attempts);
        }
        let n = attempts.len() as u32 + 1;
        attempts.push(try_once(n));
    }
}

/// What to say when a long run ends.
pub fn ended_spoken(goal: &Goal, ended: &Ended, attempts: &[Attempt]) -> String {
    match ended {
        Ended::Standing(_) => goal.spoken(attempts),
        Ended::OutOfTime { after, hours } => format!(
            "{} — stopped at the {hours}-hour limit after {after} attempts. Still failing: {}.",
            goal.what,
            attempts.last().map(|a| a.failed.iter().map(|c| c.plain()).collect::<Vec<_>>().join("; ")).unwrap_or_default()
        ),
        Ended::Stopped { after } => format!("{} — stopped when you asked, after {after} attempts.", goal.what),
    }
}
