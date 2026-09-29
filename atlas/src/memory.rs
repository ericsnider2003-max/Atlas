//! Memory: five separate stores, per the spec. Not one generic bucket.
//!
//! The separation earns its keep because the stores have different lifetimes
//! and different consequences. A wrong preference is annoying; a wrong
//! approval-history entry makes Atlas act without asking. Keeping them apart
//! means one can be cleared without touching the others.

use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// How you like things done: layout, naming, formatting, confirmations.
pub type Preferences = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct WorkflowMemo {
    /// The request that started it, normalized.
    pub trigger: String,
    /// Actions taken, in order.
    pub steps: Vec<String>,
    pub times_used: u32,
    pub last_used: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Project {
    pub name: String,
    pub folders: Vec<String>,
    pub conventions: BTreeMap<String, String>,
    pub last_touched: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct StyleMemo {
    pub tag: String,
    pub accepted: Vec<String>,
    pub rejected: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ApprovalRecord {
    /// Action kind, e.g. "close_app" — not the full arguments.
    pub kind: String,
    pub approved: bool,
    pub at: u64,
    #[serde(default)]
    pub correction: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Memory {
    pub preferences: Preferences,
    pub workflows: Vec<WorkflowMemo>,
    pub projects: BTreeMap<String, Project>,
    pub styles: Vec<StyleMemo>,
    pub approvals: Vec<ApprovalRecord>,
    /// Compacted approval history: kind -> (total, approved). Keeps the
    /// statistics after the individual records are dropped.
    #[serde(default)]
    pub summaries: BTreeMap<String, (u32, u32)>,
}

/// Keeps the approval log from growing without bound on a laptop.
const MAX_APPROVALS: usize = 500;

impl Memory {
    pub fn load(store: &Store) -> Memory {
        store.load("memory")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("memory", self)
    }

    pub fn prefer(&mut self, key: &str, value: &str) {
        self.preferences.insert(key.into(), value.into());
    }
    pub fn preference(&self, key: &str) -> Option<&str> {
        self.preferences.get(key).map(String::as_str)
    }

    /// Record a completed sequence. Repeats increment rather than duplicate,
    /// which is what makes "you always do X after Y" detectable later.
    pub fn record_workflow(&mut self, trigger: &str, steps: Vec<String>) {
        let t = trigger.trim().to_lowercase();
        if let Some(w) = self.workflows.iter_mut().find(|w| w.trigger == t) {
            w.times_used += 1;
            w.last_used = now();
            w.steps = steps;
        } else {
            self.workflows.push(WorkflowMemo { trigger: t, steps, times_used: 1, last_used: now() });
        }
    }

    /// Sequences seen enough times to be treated as a habit.
    pub fn habits(&self, min_uses: u32) -> Vec<&WorkflowMemo> {
        let mut v: Vec<&WorkflowMemo> =
            self.workflows.iter().filter(|w| w.times_used >= min_uses).collect();
        v.sort_by(|a, b| b.times_used.cmp(&a.times_used));
        v
    }

    pub fn touch_project(&mut self, name: &str, folder: Option<&str>) {
        let p = self.projects.entry(name.to_string()).or_insert_with(|| Project {
            name: name.to_string(),
            ..Default::default()
        });
        p.last_touched = now();
        if let Some(f) = folder {
            if !p.folders.iter().any(|x| x == f) {
                p.folders.push(f.to_string());
            }
        }
    }

    pub fn record_approval(&mut self, kind: &str, approved: bool, correction: Option<String>) {
        self.approvals.push(ApprovalRecord {
            kind: kind.to_string(),
            approved,
            at: now(),
            correction,
        });
        if self.approvals.len() > MAX_APPROVALS {
            let drop = self.approvals.len() - MAX_APPROVALS;
            self.approvals.drain(0..drop);
        }
    }

    /// Fraction of past approvals granted for this action kind.
    /// `None` when there is no history — which must not be read as "risky",
    /// only as "unknown".
    pub fn approval_rate(&self, kind: &str) -> Option<f32> {
        let (mut n, mut yes) = self.summaries.get(kind).copied().unwrap_or((0, 0));
        for a in self.approvals.iter().filter(|a| a.kind == kind) {
            n += 1;
            if a.approved {
                yes += 1;
            }
        }
        (n > 0).then(|| yes as f32 / n as f32)
    }

    /// Collapse old approval records into running totals.
    ///
    /// The individual records stop being useful once there are hundreds — all
    /// that is actually consulted is the ratio. So the oldest ones become a
    /// single summary entry, preserving the statistics while dropping the
    /// per-event detail. Nothing that changes Atlas's behaviour is lost; the
    /// space is.
    pub fn compact_approvals(&mut self, keep_detailed: usize) -> usize {
        if self.approvals.len() <= keep_detailed {
            return 0;
        }
        let split = self.approvals.len() - keep_detailed;
        let old: Vec<ApprovalRecord> = self.approvals.drain(0..split).collect();

        let mut totals: BTreeMap<String, (u32, u32)> = BTreeMap::new();
        for a in &old {
            let e = totals.entry(a.kind.clone()).or_insert((0, 0));
            e.0 += 1;
            if a.approved {
                e.1 += 1;
            }
        }
        for (kind, (n, yes)) in totals {
            self.summaries
                .entry(kind)
                .and_modify(|s| {
                    s.0 += n;
                    s.1 += yes;
                })
                .or_insert((n, yes));
        }
        old.len()
    }

    pub fn times_seen(&self, kind: &str) -> usize {
        let compacted = self.summaries.get(kind).map(|s| s.0 as usize).unwrap_or(0);
        compacted + self.approvals.iter().filter(|a| a.kind == kind).count()
    }
}
