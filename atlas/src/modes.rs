//! Named workspace modes.
//!
//! "Trading mode." "Writing mode." "Call mode." Each is a set of apps, a
//! layout, a notification policy, and a lighting state. The layout engine
//! already supported all of this; nothing had names.
//!
//! The part that earns its keep is the notification policy. Half the value of
//! saying "call mode" is that Atlas then shuts up.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

/// How much Atlas is allowed to interrupt in this mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Interruptions {
    /// Anything.
    #[default]
    Normal,
    /// Only things that would cost you something to miss.
    Urgent,
    /// Nothing. Atlas still listens and still works — it just doesn't speak.
    Silent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mode {
    pub name: String,
    /// Apps to open, in order. Empty leaves the workspace alone.
    #[serde(default)]
    pub open: Vec<String>,
    /// Apps to close on entering. Deliberately separate from `open` so a mode
    /// can be additive.
    #[serde(default)]
    pub close: Vec<String>,
    #[serde(default)]
    pub interruptions: Interruptions,
    /// Anticipation rules to enable while in this mode.
    #[serde(default)]
    pub rules_on: Vec<String>,
    #[serde(default)]
    pub rules_off: Vec<String>,
    /// Smart-light scene name, when lights exist.
    #[serde(default)]
    pub lights: Option<String>,
    /// Longer replies, or terse ones.
    #[serde(default = "d_detail")]
    pub verbosity: u8,
    /// Phrases that switch to this mode.
    #[serde(default)]
    pub triggers: Vec<String>,
}
fn d_detail() -> u8 {
    2
}


impl Mode {
    /// May Atlas speak up unprompted right now?
    pub fn may_interrupt(&self, urgent: bool) -> bool {
        match self.interruptions {
            Interruptions::Normal => true,
            Interruptions::Urgent => urgent,
            Interruptions::Silent => false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Modes {
    pub modes: Vec<Mode>,
    pub current: Option<String>,
    /// What was open before the current mode, so leaving can restore it.
    #[serde(default)]
    previous_apps: Vec<String>,
}

/// What entering a mode should do.
#[derive(Debug, Clone, PartialEq)]
pub struct Transition {
    pub close: Vec<String>,
    pub open: Vec<String>,
    pub rules_on: Vec<String>,
    pub rules_off: Vec<String>,
    pub lights: Option<String>,
    pub say: String,
}

impl Modes {
    /// Have you told Atlas you are somewhere it should not talk out loud?
    ///
    /// A mode you set, not something sensed. There is no honest way to detect
    /// "in a coffee shop": a camera cannot tell a stranger from a colleague,
    /// and somebody being nearby is always true in public, so guessing either
    /// broadcasts your business to a room or silences Atlas at your own desk.
    /// Headphones handle the common case without this; this is the override
    /// for when you have none.
    pub fn in_public(&self) -> bool {
        self.active()
            .map(|m| {
                let n = m.name.to_lowercase();
                n.contains("public") || n.contains("out") || n.contains("cafe")
            })
            .unwrap_or(false)
    }

    pub fn load(store: &Store) -> Modes {
        store.load("modes")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("modes", self)
    }

    pub fn add(&mut self, m: Mode) {
        self.modes.retain(|x| x.name != m.name);
        self.modes.push(m);
    }

    pub fn get(&self, name: &str) -> Option<&Mode> {
        self.modes.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }

    pub fn active(&self) -> Option<&Mode> {
        self.current.as_ref().and_then(|c| self.get(c))
    }

    /// Match spoken text to a mode. Longest trigger wins, so "call mode" is
    /// not shadowed by "call".
    pub fn match_trigger(&self, said: &str) -> Option<&Mode> {
        let t = said.trim().to_lowercase();
        let mut best: Option<(&Mode, usize)> = None;
        for m in &self.modes {
            for trig in m.triggers.iter().chain(std::iter::once(&m.name)) {
                let g = trig.trim().to_lowercase();
                if g.is_empty() || !t.contains(&g) {
                    continue;
                }
                if best.map(|(_, l)| g.len() > l).unwrap_or(true) {
                    best = Some((m, g.len()));
                }
            }
        }
        best.map(|(m, _)| m)
    }

    /// Enter a mode. `open_now` is what is currently open, so leaving can put
    /// it back.
    pub fn enter(&mut self, name: &str, open_now: &[String]) -> Option<Transition> {
        let m = self.get(name)?.clone();
        if self.current.is_none() {
            self.previous_apps = open_now.to_vec();
        }
        self.current = Some(m.name.clone());
        Some(Transition {
            close: m.close.clone(),
            open: m.open.clone(),
            rules_on: m.rules_on.clone(),
            rules_off: m.rules_off.clone(),
            lights: m.lights.clone(),
            say: format!("{} mode.", m.name),
        })
    }

    /// Leave, restoring what was open before.
    ///
    /// Restoring rather than just closing matters: a mode you cannot get out
    /// of cleanly is a mode you stop using.
    pub fn leave(&mut self) -> Option<Transition> {
        let m = self.active()?.clone();
        let restore = std::mem::take(&mut self.previous_apps);
        self.current = None;
        Some(Transition {
            close: m.open.clone(),
            open: restore,
            rules_on: m.rules_off.clone(),
            rules_off: m.rules_on.clone(),
            lights: Some("default".into()),
            say: format!("Out of {} mode.", m.name),
        })
    }

    pub fn may_interrupt(&self, urgent: bool) -> bool {
        self.active().map(|m| m.may_interrupt(urgent)).unwrap_or(true)
    }

    /// The verbosity a mode actually asked for, or `None` when no mode is on.
    ///
    /// This replaced `verbosity()`, which was the same thing with
    /// `.unwrap_or(2)` on the end, and the default was the bug: 2 reads as 3
    /// sentences through `sentences_for`, and `run_command` applied it as a
    /// `min` over `register.length()`. So on a fresh install -- where no mode
    /// is active -- every conversation was capped at three sentences by a
    /// placeholder standing in for "nobody said", and `Register::Chatting`'s
    /// eight could never apply. A ceiling has to come from a mode somebody
    /// turned on.
    ///
    /// `verbosity()` was deleted rather than kept: with both of its callers
    /// moved here it had no caller left, and its only remaining meaning was
    /// the defaulting that caused the problem. Listing it as dead would have
    /// added something to the backlog that only existed to be wrong.
    pub fn verbosity_if_set(&self) -> Option<u8> {
        self.active().map(|m| m.verbosity)
    }
}

/// A starting set, shaped around how you actually described your workspace.
pub fn suggested() -> Vec<Mode> {
    vec![
        Mode {
            name: "focus".into(),
            open: vec![],
            close: vec!["discord".into()],
            interruptions: Interruptions::Urgent,
            rules_on: vec![],
            rules_off: vec!["new document landed".into()],
            lights: Some("focus".into()),
            verbosity: 1,
            triggers: vec!["focus mode".into(), "heads down".into(), "deep work".into()],
        },
        Mode {
            name: "call".into(),
            open: vec![],
            close: vec![],
            interruptions: Interruptions::Silent,
            rules_on: vec![],
            rules_off: vec![],
            lights: Some("bright".into()),
            verbosity: 1,
            triggers: vec!["call mode".into(), "i'm on a call".into(), "im on a call".into()],
        },
        Mode {
            name: "research".into(),
            open: vec!["chrome".into(), "notepad".into()],
            close: vec![],
            interruptions: Interruptions::Normal,
            rules_on: vec!["new document landed".into()],
            rules_off: vec![],
            lights: None,
            verbosity: 3,
            triggers: vec!["research mode".into()],
        },
    ]
}

// ---------- how much to say ----------

/// Sentence budget for a given level.
pub fn sentences_for(level: u8) -> usize {
    match level {
        0 | 1 => 1,
        2 => 3,
        _ => 6,
    }
}
