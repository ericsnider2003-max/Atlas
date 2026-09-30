//! Scheduler: one-time, delayed, and recurring jobs, plus jobs parked waiting
//! for approval.
//!
//! Time is passed in rather than read from the clock, so schedule behaviour is
//! testable without sleeping.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Pending,
    AwaitingApproval,
    Done,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    /// The command text, run through the same parser as speech.
    pub command: String,
    pub due: u64,
    /// Seconds between runs. None = one-shot.
    #[serde(default)]
    pub every: Option<u64>,
    pub state: JobState,
    #[serde(default)]
    pub last_result: Option<String>,
    #[serde(default)]
    pub runs: u32,
    /// A calendar-shaped schedule instead of a fixed interval: `cron:<expr>`
    /// ("0 9 * * MON-FRI") or `rule:<RRULE>` ("FREQ=MONTHLY;BYDAY=-1FR").
    /// Weekdays-only and "the last Friday of the month" are not a number of
    /// seconds apart, so `every` cannot hold them.
    #[serde(default)]
    pub on: Option<String>,
    /// You approved this specific job. It no longer re-asks every time it
    /// fires — which is the point of scheduling something in the first place.
    #[serde(default)]
    pub approved: bool,
    /// Runs in a row that failed. A repeating job is tried again at its next
    /// time; only `FAILS_BEFORE_STOPPING` in a row stop it.
    #[serde(default)]
    pub fails_in_a_row: u32,
}

/// Failed runs in a row after which a repeating job stops (29 Sep 2026: one
/// failure stopped it for good, and nothing said so).
pub const FAILS_BEFORE_STOPPING: u32 = 3;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Scheduler {
    pub jobs: Vec<Job>,
    next_id: u64,
}

impl Scheduler {
    pub fn load(store: &Store) -> Scheduler {
        store.load("schedule")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("schedule", self)
    }

    pub fn at(&mut self, command: &str, due: u64) -> u64 {
        self.push(command, due, None)
    }

    pub fn every(&mut self, command: &str, secs: u64, first_due: u64) -> u64 {
        self.push(command, first_due, Some(secs))
    }

    /// Recurring on a cron expression. Refused, with the reason, if the
    /// expression does not parse or can never fire.
    pub fn on_cron(&mut self, command: &str, expr: &str, now: u64, zone: &crate::tz::Zone) -> std::result::Result<u64, String> {
        crate::cronspec::Cron::parse(expr)?;
        let on = with_zone("cron", zone, expr);
        let first = next_on(&on, now).ok_or_else(|| format!("'{expr}' never fires"))?;
        let id = self.push(command, first, None);
        if let Some(j) = self.jobs.last_mut() {
            j.on = Some(on);
        }
        Ok(id)
    }

    /// Recurring on an RFC 5545 rule, first run at `first`, on `zone`'s wall
    /// clock.
    pub fn on_rule(&mut self, command: &str, rule: &crate::recur::Rule, first: u64, zone: &crate::tz::Zone) -> u64 {
        let id = self.push(command, first, None);
        if let Some(j) = self.jobs.last_mut() {
            j.on = Some(with_zone("rule", zone, &rule.to_rrule()));
        }
        id
    }

    fn push(&mut self, command: &str, due: u64, every: Option<u64>) -> u64 {
        self.next_id += 1;
        let id = self.next_id;
        self.jobs.push(Job {
            id,
            command: command.to_string(),
            due,
            every,
            on: None,
            state: JobState::Pending,
            last_result: None,
            runs: 0,
            approved: false,
            fails_in_a_row: 0,
        });
        id
    }

    /// Jobs that should run at `t`. Awaiting-approval jobs are never returned
    /// — a job does not become approved by getting old.
    pub fn due(&self, t: u64) -> Vec<u64> {
        self.jobs
            .iter()
            .filter(|j| j.state == JobState::Pending && j.due <= t)
            .map(|j| j.id)
            .collect()
    }

    /// Mark a run finished. Recurring jobs are rescheduled from `t`, not from
    /// their old due time, so a laptop asleep for a day does not wake up owing
    /// twenty-four runs of an hourly job.
    pub fn complete(&mut self, id: u64, t: u64, result: &str, ok: bool) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.runs += 1;
            j.last_result = Some(result.to_string());
            j.fails_in_a_row = if ok { 0 } else { j.fails_in_a_row + 1 };
            let keep_going = ok || j.fails_in_a_row < FAILS_BEFORE_STOPPING;
            let next_on = j.on.as_deref().and_then(|on| next_on(on, j.due.max(t)));
            match (j.every, next_on) {
                (Some(every), _) if keep_going => {
                    j.due = t + every;
                    j.state = JobState::Pending;
                }
                // A calendar-shaped job runs again at its next slot; one whose
                // rule has run out (COUNT, UNTIL) is done.
                (None, Some(next)) if keep_going => {
                    j.due = next;
                    j.state = JobState::Pending;
                }
                _ => j.state = if ok { JobState::Done } else { JobState::Failed },
            }
        }
    }

    pub fn park_for_approval(&mut self, id: u64) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.state = JobState::AwaitingApproval;
        }
    }

    pub fn approve(&mut self, id: u64) {
        if let Some(j) = self.jobs.iter_mut().find(|j| j.id == id) {
            j.approved = true;
            if j.state == JobState::AwaitingApproval {
                j.state = JobState::Pending;
            }
        }
    }

    pub fn cancel(&mut self, id: u64) -> bool {
        match self.jobs.iter_mut().find(|j| j.id == id) {
            Some(j) => {
                j.state = JobState::Cancelled;
                true
            }
            None => false,
        }
    }

    pub fn active(&self) -> Vec<&Job> {
        self.jobs
            .iter()
            .filter(|j| matches!(j.state, JobState::Pending | JobState::AwaitingApproval))
            .collect()
    }

    /// Drop finished jobs so the file doesn't grow forever.
    pub fn prune(&mut self) {
        self.jobs.retain(|j| {
            matches!(j.state, JobState::Pending | JobState::AwaitingApproval)
                || j.every.is_some()
                || j.on.is_some()
        });
    }
}

/// The slot after `after` for a `cron:`/`rule:` schedule. Skips forward, not
/// back: a laptop asleep over three slots runs once on waking, the same
/// promise `every` makes.
/// `cron:<expr>` / `rule:<RRULE>` run on UTC, as every job did before zones;
/// `cron[<zone>]:<expr>` runs on that zone's wall clock, so "every weekday at
/// 7" stays 7 through the daylight-saving change.
fn with_zone(kind: &str, zone: &crate::tz::Zone, body: &str) -> String {
    if zone.is_utc() {
        format!("{kind}:{body}")
    } else {
        format!("{kind}[{}]:{body}", zone.id())
    }
}

fn next_on(on: &str, after: u64) -> Option<u64> {
    // A zone given as a POSIX rule can hold ':' ("<+0530>-5:30"), so the
    // bracket closes the head, not the first colon.
    let (head, body) = match on.find("]:") {
        Some(i) if on[..i].contains('[') => (&on[..=i], &on[i + 2..]),
        _ => on.split_once(':')?,
    };
    let (kind, zone) = match head.split_once('[') {
        Some((k, z)) => (k, crate::tz::Zone::named(z.strip_suffix(']')?)?),
        None => (head, crate::tz::Zone::utc()),
    };
    let local_after = zone.to_local(after as i64);
    let local_next = match kind {
        "cron" => crate::cronspec::Cron::parse(body).ok()?.next_after(local_after)?,
        "rule" => crate::recur::Rule::parse(body).ok()?.first_at_or_after(local_after + 1)?,
        _ => return None,
    };
    let utc = zone.to_utc(local_next);
    // A wall-clock time inside the spring gap maps to just after it; never
    // hand back a time at or before `after`, which would fire twice.
    Some(utc.max(after as i64 + 1) as u64)
}
