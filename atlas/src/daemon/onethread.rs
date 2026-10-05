//! One conversation on every device (item 16, P3; Eric's yes, 1 Oct 2026:
//! "conversation and memory may travel to the phone; the vault never would").
//!
//! What a turn adds -- the exchange itself, a fact learned or restated, an
//! item put on the later list, a reminder set -- is carried to your other
//! devices as sync events in your sealed bundles, and what arrives is made
//! true here. So you talk to the laptop at your desk, open the phone in the
//! car, and it knows what you just asked, what it knows about you and
//! what's on your list.
//!
//! What never travels: the vault and credentials (they're never in these
//! stores), anything said while Atlas is handed over to somebody else (their
//! words are not yours), and any exchange or fact with something private in
//! it -- a key, a card or account number (`brain::holds_something_private`,
//! the rule that keeps them from an online model).
//!
//! Only your own devices take any of it: a bundle not sealed with your
//! household key is never read for these. What arrived by sync isn't sent
//! back out.

use super::*;

/// The sync id prefixes.
pub const FACT_PREFIX: &str = "fact:";
pub const LATER_PREFIX: &str = "later:";
pub const REMIND_PREFIX: &str = "remind:";

/// What a turn could add, as it stood before the turn.
pub(crate) struct Before {
    thread_len: usize,
    facts: std::collections::HashMap<String, u64>,
    later: Vec<String>,
    reminders: Vec<u64>,
}

fn reminder_ids(s: &crate::scheduler::Scheduler) -> Vec<u64> {
    s.active().into_iter().filter(|j| j.command.starts_with("reminder ") && j.every.is_none() && j.on.is_none()).map(|j| j.id).collect()
}

impl Daemon<'_> {
    pub(super) fn before_the_turn(&self) -> Before {
        let later: crate::later::Later = self.store.load(crate::later::RECORD);
        Before {
            thread_len: self.thread.len(),
            facts: self.facts.facts.iter().map(|f| (f.name.clone(), f.as_of)).collect(),
            later: later.items.into_iter().map(|i| i.what).collect(),
            reminders: reminder_ids(&self.scheduler),
        }
    }

    /// After a turn: carry what it added to your other devices.
    pub(super) fn carry_what_the_turn_added(&mut self, before: Before, t: u64) {
        if self.handover().stance.handed_over() || !self.tools_cfg().sync.enabled {
            return;
        }
        let device = self.synclog.device.clone();
        let mut events: Vec<crate::sync::What> = Vec::new();
        // The exchanges this turn added (folding may have moved older ones
        // out of `recent`, never these).
        let added = self.thread.len().saturating_sub(before.thread_len).min(self.thread.recent.len());
        for x in &self.thread.recent[self.thread.recent.len() - added..] {
            if x.said.trim().is_empty() || crate::brain::holds_something_private(&x.said) || crate::brain::holds_something_private(&x.reply) {
                continue;
            }
            let key = (x.at, x.said.clone());
            if self.arrived_by_sync.thread.contains(&key) {
                continue;
            }
            let text = serde_json::json!({ "at": x.at, "said": x.said, "reply": x.reply, "about": x.about }).to_string();
            events.push(crate::sync::What::Said { text, from_you: true });
        }
        // Facts learned or restated.
        for f in &self.facts.facts {
            if before.facts.get(&f.name) == Some(&f.as_of) || self.arrived_by_sync.facts.contains(&(f.name.clone(), f.as_of)) {
                continue;
            }
            if crate::brain::holds_something_private(&f.body) || crate::brain::holds_something_private(&f.summary) {
                continue;
            }
            if let Ok(to) = serde_json::to_string(f) {
                events.push(crate::sync::What::Changed { id: format!("{FACT_PREFIX}{}", f.name), field: "fact".into(), to });
            }
        }
        // The later list: added and taken off.
        let later: crate::later::Later = self.store.load(crate::later::RECORD);
        for i in &later.items {
            if !before.later.contains(&i.what) && !self.arrived_by_sync.later.contains(&i.what) {
                events.push(crate::sync::What::Changed { id: format!("{LATER_PREFIX}{}", i.what), field: "later".into(), to: i.added.to_string() });
            }
        }
        for w in &before.later {
            if !later.items.iter().any(|i| &i.what == w) && !self.arrived_by_sync.later.contains(w) {
                events.push(crate::sync::What::Changed { id: format!("{LATER_PREFIX}{w}"), field: "later".into(), to: String::new() });
            }
        }
        // Reminders set (one-off ones: a repeating job is the laptop's).
        for j in self.scheduler.active() {
            if j.command.starts_with("reminder ") && j.every.is_none() && j.on.is_none() && !before.reminders.contains(&j.id) && !self.arrived_by_sync.reminders.contains(&j.id) {
                let to = serde_json::json!({ "command": j.command, "due": j.due }).to_string();
                events.push(crate::sync::What::Changed { id: format!("{REMIND_PREFIX}{device}:{}", j.id), field: "reminder".into(), to });
            }
        }
        if events.is_empty() {
            return;
        }
        for e in events {
            self.synclog.append(e, t);
        }
        // Saved on the next tick, not inside the turn: the log grows with
        // every exchange, and writing it out is time the reply would wait.
        self.synclog_unsaved = true;
    }

    /// The sync log, written out once a tick when a turn added to it.
    pub(super) fn save_the_synclog_if_changed(&mut self) {
        if std::mem::take(&mut self.synclog_unsaved) {
            let _ = self.store.save("synclog", &Some(self.synclog.clone()));
        }
    }

    /// An exchange from your other device, into this one's thread.
    pub(super) fn take_an_exchange(&mut self, text: &str, sealed: bool) {
        if !sealed {
            return;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return };
        let (Some(said), Some(reply)) = (v.get("said").and_then(|s| s.as_str()), v.get("reply").and_then(|s| s.as_str())) else { return };
        let at = v.get("at").and_then(|a| a.as_u64()).unwrap_or(0);
        if self.thread.recent.iter().any(|x| x.at == at && x.said == said) {
            return;
        }
        let about = v.get("about").and_then(|a| a.as_str()).map(str::to_string);
        // In time order, not arrival order: an exchange from while the
        // phone was offline goes where it happened.
        let i = self.thread.recent.iter().position(|x| x.at > at).unwrap_or(self.thread.recent.len());
        self.thread.recent.insert(i, crate::thread::Exchange { at, said: said.into(), reply: reply.into(), about });
        self.thread.last_active = self.thread.last_active.max(at);
        self.arrived_by_sync.thread.insert((at, said.into()));
        let _ = self.thread.save(&self.store);
    }

    /// A fact from your other device: kept when it's newer than ours.
    pub(super) fn take_a_fact(&mut self, to: &str, sealed: bool) {
        if !sealed {
            return;
        }
        let Ok(f) = serde_json::from_str::<crate::facts::Fact>(to) else { return };
        if self.facts.facts.iter().any(|x| x.name == f.name && x.as_of >= f.as_of) {
            return;
        }
        self.arrived_by_sync.facts.insert((f.name.clone(), f.as_of));
        self.facts.put(f);
        let _ = self.facts.save(&self.store);
    }

    /// The later list, from your other device.
    pub(super) fn take_a_later_item(&mut self, id: &str, to: &str, sealed: bool) {
        let Some(what) = id.strip_prefix(LATER_PREFIX).filter(|_| sealed) else { return };
        let mut later: crate::later::Later = self.store.load(crate::later::RECORD);
        let changed = if to.is_empty() {
            let n = later.items.len();
            later.items.retain(|i| i.what != what);
            later.items.len() != n
        } else {
            later.add(what, to.parse().unwrap_or(0))
        };
        if changed {
            self.arrived_by_sync.later.insert(what.to_string());
            let _ = self.store.save(crate::later::RECORD, &later);
        }
    }

    /// A reminder set on your other device, set here too, once.
    pub(super) fn take_a_reminder(&mut self, id: &str, to: &str, sealed: bool) {
        if !sealed || !id.starts_with(REMIND_PREFIX) {
            return;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(to) else { return };
        let (Some(command), Some(due)) = (v.get("command").and_then(|c| c.as_str()), v.get("due").and_then(|d| d.as_u64())) else { return };
        if !command.starts_with("reminder ") || due <= crate::store::now() {
            return;
        }
        let mut taken: Vec<String> = self.store.load("reminders_from_your_devices");
        if taken.iter().any(|x| x == id) {
            return;
        }
        let job = self.scheduler.at(command, due);
        self.arrived_by_sync.reminders.insert(job);
        let _ = self.scheduler.save(&self.store);
        taken.push(id.to_string());
        let _ = self.store.save("reminders_from_your_devices", &taken);
    }
}

/// What arrived by sync, so it isn't carried straight back out.
#[derive(Debug, Default)]
pub struct ArrivedBySync {
    pub thread: std::collections::HashSet<(u64, String)>,
    pub facts: std::collections::HashSet<(String, u64)>,
    pub later: std::collections::HashSet<String>,
    pub reminders: std::collections::HashSet<u64>,
}
