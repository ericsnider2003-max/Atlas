//! Doing the work before you ask for it.
//!
//! The second thing that makes Jarvis feel like Jarvis: the answer is usually
//! already there. *"The simulation completed while you were out."*
//!
//! Every part needed for this already existed — the scheduler, the background
//! lane, research, the journal. What was missing was the layer that decides
//! *when* something is worth doing unasked. That is this.
//!
//! One rule throughout: anticipated work only ever runs in the background lane
//! and only ever prepares. It never takes your screen and never sends
//! anything. Preparation is safe; acting unasked is not.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Trigger {
    /// Every day at this hour and minute, on these weekdays (0 = Monday).
    Daily { hour: u32, minute: u32, days: Vec<u32> },
    /// A file matching this appeared under a watched root.
    FileAppears { pattern: String },
    /// A calendar event starts within this many minutes.
    BeforeEvent { minutes: u64 },
    /// You just came back to the desk.
    OnReturn,
    /// The machine has been quiet this long — a good moment for slow work.
    WhenIdle { secs: u64 },
    /// You said something matching this, and it tends to be followed by more.
    AfterTopic { keyword: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Rule {
    pub name: String,
    pub trigger: Trigger,
    /// Run in the background and hold the result.
    pub command: String,
    /// Tell you when it's ready, rather than waiting to be asked.
    #[serde(default)]
    pub announce: bool,
    /// Don't repeat within this many seconds.
    #[serde(default = "d_cool")]
    pub cooldown_secs: u64,
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub last_fired: u64,
}
fn d_cool() -> u64 {
    3600
}

/// What is true right now, for deciding what is worth preparing.
#[derive(Debug, Clone, Default)]
pub struct Moment {
    /// Minutes since midnight, local.
    pub minutes_of_day: u32,
    /// 0 = Monday.
    pub weekday: u32,
    /// Files that appeared since the last check.
    pub new_files: Vec<String>,
    /// Minutes until the next calendar event, if any.
    pub minutes_to_event: Option<u64>,
    /// You just came back.
    pub returned: bool,
    pub idle_secs: u64,
    /// The last thing you said.
    pub last_said: String,
    /// Whether the machine can afford slow work right now.
    pub can_afford_work: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Anticipator {
    pub rules: Vec<Rule>,
}

impl Anticipator {
    pub fn load(store: &Store) -> Anticipator {
        store.load("anticipate")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("anticipate", self)
    }

    pub fn add(&mut self, r: Rule) {
        self.rules.retain(|x| x.name != r.name);
        self.rules.push(r);
    }

    /// Rules that should fire now.
    ///
    /// Nothing fires when the machine cannot afford it — anticipation that
    /// makes your laptop stutter is worse than no anticipation.
    pub fn due(&mut self, m: &Moment, t: u64) -> Vec<Rule> {
        if !m.can_afford_work {
            return Vec::new();
        }
        let mut out = Vec::new();
        for r in self.rules.iter_mut() {
            if !r.enabled {
                continue;
            }
            // last_fired == 0 means never, not "fired in 1970". Without this
            // a fresh install sits dormant for a full cooldown.
            if r.last_fired != 0 && t.saturating_sub(r.last_fired) < r.cooldown_secs {
                continue;
            }
            if fires(&r.trigger, m) {
                r.last_fired = t;
                out.push(r.clone());
            }
        }
        out
    }

    pub fn enable(&mut self, name: &str, on: bool) -> bool {
        match self.rules.iter_mut().find(|r| r.name == name) {
            Some(r) => {
                r.enabled = on;
                true
            }
            None => false,
        }
    }

    pub fn active(&self) -> Vec<&Rule> {
        self.rules.iter().filter(|r| r.enabled).collect()
    }
}

fn fires(trigger: &Trigger, m: &Moment) -> bool {
    match trigger {
        Trigger::Daily { hour, minute, days } => {
            let target = hour * 60 + minute;
            let day_ok = days.is_empty() || days.contains(&m.weekday);
            // A window, not an instant — the tick does not land on the second.
            day_ok && m.minutes_of_day >= target && m.minutes_of_day < target + 5
        }
        Trigger::FileAppears { pattern } => {
            let p = pattern.to_lowercase();
            m.new_files.iter().any(|f| matches(&f.to_lowercase(), &p))
        }
        Trigger::BeforeEvent { minutes } => {
            m.minutes_to_event.map(|mins| mins <= *minutes).unwrap_or(false)
        }
        Trigger::OnReturn => m.returned,
        Trigger::WhenIdle { secs } => m.idle_secs >= *secs,
        Trigger::AfterTopic { keyword } => {
            m.last_said.to_lowercase().contains(&keyword.to_lowercase())
        }
    }
}

/// Glob-lite: `*` matches any run of characters. Enough for "*.pdf" and
/// "invoice*", which is what file rules actually need.
pub fn matches(text: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return text.contains(pattern);
    }
    let mut pos = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        match text[pos..].find(part) {
            Some(found) => {
                // A leading segment must anchor at the start.
                if i == 0 && found != 0 {
                    return false;
                }
                pos += found + part.len();
            }
            None => return false,
        }
    }
    // A trailing segment must anchor at the end.
    if let Some(last) = parts.last() {
        if !last.is_empty() && !text.ends_with(last) {
            return false;
        }
    }
    true
}

/// A starting set. All off until you turn them on — an assistant that begins
/// doing things unasked on day one is alarming, not helpful.
pub fn suggested() -> Vec<Rule> {
    vec![
        Rule {
            name: "morning brief".into(),
            trigger: Trigger::Daily { hour: 7, minute: 0, days: vec![0, 1, 2, 3, 4] },
            command: "research overnight news".into(),
            announce: true,
            cooldown_secs: 20 * 3600,
            enabled: false,
            last_fired: 0,
        },
        Rule {
            name: "before a meeting".into(),
            trigger: Trigger::BeforeEvent { minutes: 10 },
            command: "summarise the last thread with these people".into(),
            announce: true,
            cooldown_secs: 600,
            enabled: false,
            last_fired: 0,
        },
        Rule {
            name: "new document landed".into(),
            trigger: Trigger::FileAppears { pattern: "*.pdf".into() },
            command: "summarise the new document".into(),
            announce: false,
            cooldown_secs: 300,
            enabled: false,
            last_fired: 0,
        },
        Rule {
            name: "welcome back".into(),
            trigger: Trigger::OnReturn,
            command: "what happened while I was away".into(),
            announce: true,
            cooldown_secs: 900,
            enabled: false,
            last_fired: 0,
        },
        Rule {
            name: "index while idle".into(),
            trigger: Trigger::WhenIdle { secs: 600 },
            command: "index refresh".into(),
            announce: false,
            cooldown_secs: 3600,
            enabled: false,
            last_fired: 0,
        },
    ]
}

/// How Atlas offers something it prepared. Never "I did X" — it prepared X,
/// and you decide whether you want it.
pub fn ready_line(rule: &Rule) -> String {
    format!("{} is ready when you want it.", rule.name)
}
