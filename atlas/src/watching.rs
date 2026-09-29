//! Watching something long finish.
//!
//! You start a render, a build, a large download, and walk away. The useful
//! thing is not a progress bar you have to look at — it's being told the
//! outcome when it happens.
//!
//! The judgement here is about when to speak. Something that finishes in
//! twenty seconds does not need announcing; you were still sitting there.
//! Something that took ten minutes does, even if it succeeded.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Finished,
    Failed,
    /// Still going.
    Running,
    /// The process vanished without a result.
    Vanished,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    /// What you'd call it.
    pub name: String,
    /// The process, so it can be found again.
    pub process: String,
    pub started: u64,
    pub finished: Option<u64>,
    pub outcome: Outcome,
    /// The last line of output, which is usually the useful one.
    #[serde(default)]
    pub last_line: String,
    /// Told you about it.
    #[serde(default)]
    pub reported: bool,
}

impl Job {
    pub fn ran_for(&self, now: u64) -> u64 {
        self.finished.unwrap_or(now).saturating_sub(self.started)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct WatchConfig {
    pub enabled: bool,
    /// Anything shorter than this isn't worth mentioning — you were still
    /// sitting there when it finished.
    pub worth_saying_after_secs: u64,
    /// Failures are always worth saying, however quick.
    pub always_report_failures: bool,
    /// Give up watching after this.
    pub give_up_after_secs: u64,
}

impl Default for WatchConfig {
    fn default() -> Self {
        WatchConfig {
            enabled: true,
            worth_saying_after_secs: 45,
            always_report_failures: true,
            give_up_after_secs: 6 * 3600,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Watcher {
    pub jobs: Vec<Job>,
    next_id: u64,
}

impl Watcher {
    pub fn load(store: &crate::store::Store) -> Watcher {
        store.load("watching")
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("watching", self)
    }

    pub fn watch(&mut self, name: &str, process: &str, t: u64) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.jobs.push(Job {
            id,
            name: name.into(),
            process: process.into(),
            started: t,
            finished: None,
            outcome: Outcome::Running,
            last_line: String::new(),
            reported: false,
        });
        id
    }

    pub fn update(&mut self, id: u64, outcome: Outcome, last_line: &str, t: u64) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.outcome = outcome;
            j.last_line = last_line.to_string();
            if outcome != Outcome::Running && j.finished.is_none() {
                j.finished = Some(t);
            }
        }
    }

    pub fn running(&self) -> Vec<&Job> {
        self.jobs.iter().filter(|j| j.outcome == Outcome::Running).collect()
    }

    /// What's worth telling you about, now.
    pub fn to_report(&mut self, cfg: &WatchConfig, t: u64) -> Vec<String> {
        if !cfg.enabled {
            return Vec::new();
        }
        let mut out = Vec::new();
        for j in self.jobs.iter_mut() {
            if j.reported || j.outcome == Outcome::Running {
                continue;
            }
            let long_enough = j.ran_for(t) >= cfg.worth_saying_after_secs;
            let is_failure = matches!(j.outcome, Outcome::Failed | Outcome::Vanished);
            if !long_enough && !(is_failure && cfg.always_report_failures) {
                // Finished while you were still sitting there. Not news.
                j.reported = true;
                continue;
            }
            j.reported = true;
            out.push(describe(j, t));
        }
        out
    }

    /// Stop watching things that will never finish.
    pub fn prune(&mut self, cfg: &WatchConfig, t: u64) -> usize {
        let before = self.jobs.len();
        self.jobs.retain(|j| {
            j.outcome == Outcome::Running && j.ran_for(t) < cfg.give_up_after_secs
                || j.outcome != Outcome::Running && j.ran_for(t) < cfg.give_up_after_secs
        });
        before - self.jobs.len()
    }
}

/// One spoken line about a finished job.
pub fn describe(j: &Job, t: u64) -> String {
    let mins = j.ran_for(t) / 60;
    let took = if mins >= 1 {
        format!("{mins} minute{}", if mins == 1 { "" } else { "s" })
    } else {
        format!("{} seconds", j.ran_for(t))
    };
    match j.outcome {
        Outcome::Finished => format!("{} finished. Took {took}.", j.name),
        // The last line of output is nearly always the useful one.
        Outcome::Failed => {
            let tail = j.last_line.trim();
            if tail.is_empty() {
                format!("{} failed after {took}.", j.name)
            } else {
                format!("{} failed after {took}. {tail}", j.name)
            }
        }
        Outcome::Vanished => format!("{} disappeared without finishing.", j.name),
        Outcome::Running => format!("{} is still going, {took} in.", j.name),
    }
}
