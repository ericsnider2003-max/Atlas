//! Deciding whether to say anything at all.
//!
//! Everything Atlas might tell you goes through here. Without one place that
//! decides, every feature politely announces itself and the sum is a system
//! that talks constantly about nothing — which is how people end up ignoring
//! the one message that mattered.
//!
//! The test is not "is this true" or "is this interesting". It is **would you
//! do something differently if you knew**. Almost nothing passes that, which
//! is the point.

use serde::{Deserialize, Serialize};

/// How much it matters that you hear this now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weight {
    /// You'd never act on it. Atlas keeps it and says nothing.
    Never,
    /// Worth knowing when you next stop. Batched.
    WhenYouStop,
    /// Today, but not this minute.
    Today,
    /// Now — something is about to go wrong or has.
    Now,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thing {
    /// What Atlas would say.
    pub say: String,
    pub weight: Weight,
    /// A short tag, so the same kind of thing can be recognised and not
    /// repeated.
    pub about: String,
    /// Something you can do about it. Without one, it is almost never worth
    /// interrupting for.
    pub actionable: bool,
    pub at: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Doing {
    /// Head down in something.
    Focused,
    /// Moving between things.
    Between,
    /// Away from the machine.
    Away,
    /// Talking to Atlas already.
    Talking,
    /// In a call.
    InACall,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct InterruptConfig {
    /// The most Atlas will interrupt you in an hour, unprompted.
    pub per_hour: u32,
    /// Never say the same kind of thing twice inside this many minutes.
    pub repeat_after_mins: u64,
    /// Hold everything below this and batch it.
    pub interrupt_above: Weight,
    /// Say nothing at all while focused, except the urgent.
    pub respect_focus: bool,
    /// Things you've said you don't care about.
    pub muted: Vec<String>,
}

impl Default for InterruptConfig {
    fn default() -> Self {
        InterruptConfig {
            // Deliberately low. Four is already more than most days need.
            per_hour: 4,
            repeat_after_mins: 90,
            interrupt_above: Weight::Today,
            respect_focus: true,
            muted: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Say it now.
    Say(String),
    /// Keep it for when you stop.
    Hold(String),
    /// Say nothing, ever, about this one.
    Drop(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Gate {
    /// What was said and when, so it isn't repeated.
    said: Vec<(String, u64)>,
    /// Waiting for a natural break.
    pub held: Vec<Thing>,
    /// Interruptions this hour.
    recent: Vec<u64>,
}

const MINUTE: u64 = 60;
const HOUR: u64 = 3600;

impl Gate {
    /// The single question everything goes through.
    pub fn consider(&mut self, t: &Thing, doing: Doing, cfg: &InterruptConfig, now: u64) -> Decision {
        // You said you don't care.
        if cfg.muted.iter().any(|m| t.about.contains(m.as_str())) {
            return Decision::Drop("you muted this".into());
        }

        // Nothing you could act on is almost never worth saying out loud.
        if !t.actionable && t.weight < Weight::Now {
            return Decision::Drop("nothing you'd do differently".into());
        }
        if t.weight == Weight::Never {
            return Decision::Drop("not worth saying".into());
        }

        // The same thing again is worse than the first time, not better.
        if let Some((_, when)) = self.said.iter().find(|(a, _)| *a == t.about) {
            if now.saturating_sub(*when) < cfg.repeat_after_mins * MINUTE {
                return Decision::Drop("said something like it recently".into());
            }
        }

        // Urgent goes through everything below.
        if t.weight == Weight::Now {
            self.record(t, now);
            return Decision::Say(t.say.clone());
        }

        if doing == Doing::Away {
            return self.hold(t, "you're not here");
        }
        if doing == Doing::InACall {
            return self.hold(t, "you're in a call");
        }
        if cfg.respect_focus && doing == Doing::Focused {
            return self.hold(t, "you're in the middle of something");
        }

        // Already talking: it isn't an interruption, so the budget doesn't
        // apply.
        if doing == Doing::Talking {
            self.record(t, now);
            return Decision::Say(t.say.clone());
        }

        if t.weight < cfg.interrupt_above {
            return self.hold(t, "it can wait");
        }

        // The hourly budget.
        self.recent.retain(|w| now.saturating_sub(*w) < HOUR);
        if self.recent.len() as u32 >= cfg.per_hour {
            return self.hold(t, "I've said enough this hour");
        }

        self.record(t, now);
        Decision::Say(t.say.clone())
    }

    fn hold(&mut self, t: &Thing, why: &str) -> Decision {
        if !self.held.iter().any(|h| h.about == t.about) {
            self.held.push(t.clone());
        }
        Decision::Hold(why.into())
    }

    fn record(&mut self, t: &Thing, now: u64) {
        self.said.retain(|(a, _)| *a != t.about);
        self.said.push((t.about.clone(), now));
        self.recent.push(now);
        self.held.retain(|h| h.about != t.about);
        if self.said.len() > 200 {
            self.said.remove(0);
        }
    }

    /// You stopped, or asked. Everything held, as one thing rather than six.
    ///
    /// Six separate remarks is six interruptions even when they arrive
    /// together; one sentence naming the important one is not.
    pub fn release(&mut self, now: u64) -> Option<String> {
        if self.held.is_empty() {
            return None;
        }
        let mut held = std::mem::take(&mut self.held);
        held.sort_by(|a, b| b.weight.cmp(&a.weight));
        for h in &held {
            self.said.retain(|(a, _)| *a != h.about);
            self.said.push((h.about.clone(), now));
        }

        let first = &held[0];
        let rest = held.len() - 1;
        Some(if rest == 0 {
            first.say.clone()
        } else {
            format!("{} And {rest} other thing{}.", first.say, if rest == 1 { "" } else { "s" })
        })
    }

    /// Anything waiting?
    pub fn waiting(&self) -> usize {
        self.held.len()
    }

    /// Drop what's gone stale while held. An hour-old observation about a
    /// stalled job isn't worth saying when you sit back down.
    pub fn forget_stale(&mut self, now: u64, older_than_secs: u64) -> usize {
        let before = self.held.len();
        self.held.retain(|h| now.saturating_sub(h.at) < older_than_secs);
        before - self.held.len()
    }
}

/// You told Atlas to stop mentioning something.
/// "Start telling me about the backups again" -> "the backups".
///
/// The other half of `mute_from`, and it exists for the same reason a
/// handover has a way back: a switch you can only flick one way is a trap.
/// Somebody mutes a topic in irritation on a Tuesday and has no idea, three
/// weeks later, why Atlas never mentions their backups.
pub fn unmute_from(said: &str) -> Option<String> {
    let t = said.to_lowercase();
    for lead in [
        "start telling me about",
        "tell me about",
        "unmute",
        "start mentioning",
        "you can mention",
    ] {
        if let Some(i) = t.find(lead) {
            let rest = t[i + lead.len()..]
                .trim()
                .trim_end_matches(['.', '!'])
                .trim_end_matches(" again")
                .trim();
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}

/// Topics muted by saying so, rather than by editing the config.
///
/// Kept in the store rather than written back into `config/tools.yaml`.
/// Nothing in this tree rewrites a person's config file, and starting with
/// this one would mean a spoken aside reformatting a file they hand-edit —
/// comments, ordering and all. `InterruptConfig::muted` stays what you wrote
/// down deliberately; this is what you said out loud, and the gate consults
/// both.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Muted {
    pub topics: Vec<String>,
}

impl Muted {
    pub const FILE: &str = "muted";

    pub fn load(store: &crate::store::Store) -> Muted {
        store.load(Self::FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(Self::FILE, self)
    }

    /// Returns false when it was already muted, so the answer can say so
    /// rather than claiming to have done something.
    pub fn mute(&mut self, topic: &str) -> bool {
        let t = topic.trim().to_lowercase();
        if t.is_empty() || self.topics.iter().any(|m| *m == t) {
            return false;
        }
        self.topics.push(t);
        true
    }

    pub fn unmute(&mut self, topic: &str) -> bool {
        let t = topic.trim().to_lowercase();
        let before = self.topics.len();
        self.topics.retain(|m| *m != t);
        self.topics.len() != before
    }

    pub fn spoken(&self) -> String {
        match self.topics.len() {
            0 => "I'm not keeping quiet about anything.".into(),
            _ => format!("I don't mention: {}.", self.topics.join(", ")),
        }
    }
}

pub fn mute_from(said: &str) -> Option<String> {
    let t = said.to_lowercase();
    for lead in ["stop telling me about", "don't tell me about", "dont tell me about",
                 "stop mentioning", "i don't care about", "i dont care about", "mute"] {
        if let Some(i) = t.find(lead) {
            let rest = t[i + lead.len()..].trim().trim_end_matches(['.', '!']);
            if !rest.is_empty() {
                return Some(rest.to_string());
            }
        }
    }
    None
}
