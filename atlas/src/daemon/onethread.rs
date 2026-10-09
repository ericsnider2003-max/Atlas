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
//!
//! A reminder rings on ONE device: the one it was set on (4 Oct 2026, Eric:
//! a reminder set on the phone rang twice -- the phone's own alarm, then the
//! laptop's push of the copy it had been handed). Your other devices know
//! it -- "what reminders do I have" lists it, and cancelling it there
//! cancels it where it lives -- but they hold it as a note of another
//! device's reminder (`Elsewhere`), never as a job of their own, so it is
//! never fired, pushed, or handed to iOS a second time.

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
    reminders: Vec<(u64, u64)>,
}

/// The one-off reminders this device owns, as (id, due). A repeating one is
/// this device's alone and isn't carried.
fn reminder_ids(s: &crate::scheduler::Scheduler) -> Vec<(u64, u64)> {
    s.active().into_iter().filter(|j| j.command.starts_with("reminder ") && j.every.is_none() && j.on.is_none()).map(|j| (j.id, j.due)).collect()
}

/// Where the notes of your other devices' reminders are kept.
pub const ELSEWHERE_KEY: &str = "reminders_on_your_other_devices";

/// A reminder set on another of your devices: known here, rung there.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Elsewhere {
    /// The sync id, `remind:<device>:<job id>`: whose it is, and which.
    pub key: String,
    /// The words, as the job holds them ("Reminder: stretch").
    pub text: String,
    pub due: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sync_version: Option<crate::sync::Version>,
    #[serde(default)]
    pub cancel_pending: bool,
}

impl Elsewhere {
    /// The words as said back ("stretch").
    pub fn words(&self) -> &str {
        self.text.trim_start_matches("Reminder:").trim()
    }
}

/// The device a reminder's sync id names, and its job number there.
fn owner_of(key: &str) -> Option<(&str, u64)> {
    let (device, id) = key.strip_prefix(REMIND_PREFIX)?.rsplit_once(':')?;
    Some((device, id.parse().ok()?))
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
        // Reminders set, moved or cancelled here (one-off ones: a repeating
        // job is this device's alone). An empty `to` says it's gone.
        let now_set = reminder_ids(&self.scheduler);
        for j in self.scheduler.active() {
            if now_set.contains(&(j.id, j.due)) && !before.reminders.contains(&(j.id, j.due)) {
                let to = serde_json::json!({ "command": j.command, "due": j.due }).to_string();
                events.push(crate::sync::What::Changed { id: format!("{REMIND_PREFIX}{device}:{}", j.id), field: "reminder".into(), to });
            }
        }
        for (id, _) in &before.reminders {
            if !now_set.iter().any(|(n, _)| n == id) {
                events.push(crate::sync::What::Changed { id: format!("{REMIND_PREFIX}{device}:{id}"), field: "reminder".into(), to: String::new() });
            }
        }
        // Another device's reminder cancelled here: told to where it lives.
        for key in std::mem::take(&mut self.reminders_cancelled_elsewhere) {
            events.push(crate::sync::What::Changed { id: key, field: "reminder".into(), to: String::new() });
        }
        if events.is_empty() {
            return;
        }
        let mut later: crate::later::Later = self.store.load(crate::later::RECORD);
        let mut later_version_changed = false;
        for e in events {
            self.synclog.append(e, t);
            if let Some(event) = self.synclog.events.last() {
                later_version_changed |= later.note_version(event);
                self.facts.note_sync_version(event);
                if let crate::sync::What::Changed { id, field, .. } = &event.what {
                    if field == "reminder" {
                        if let Some((device, number)) = owner_of(id) {
                            if device == self.synclog.device {
                                if let Some(job) = self.scheduler.jobs.iter_mut().find(|job| job.id == number) {
                                    job.sync_version = Some(crate::sync::Version::of(event));
                                }
                            }
                        }
                    }
                }
            }
        }
        if later_version_changed {
            let _ = self.store.save(crate::later::RECORD, &later);
        }
        // Saved on the next tick, not inside the turn: the log grows with
        // every exchange, and writing it out is time the reply would wait.
        self.synclog_unsaved = true;
    }

    /// The sync log, written out once a tick when a turn added to it.
    pub(super) fn save_the_synclog_if_changed(&mut self) {
        let Ok(_guard) = self.store.transaction() else { return };
        let pending = self.store.load_checked::<Vec<Elsewhere>>(ELSEWHERE_KEY);
        if let Ok(Some(rows)) = &pending {
            for row in rows.iter().filter(|row| row.cancel_pending) {
                self.synclog_unsaved = true;
                let already = self.synclog.events.iter().any(|e| matches!(&e.what, crate::sync::What::Changed { id, to, field } if id == &row.key && to.is_empty() && field == "reminder") && e.device == self.synclog.device && row.sync_version.as_ref().is_none_or(|old| crate::sync::Version::of(e) >= *old));
                if !already {
                    if let Some(version) = &row.sync_version { self.synclog.observe_version(version, crate::store::now()); }
                    self.synclog.append(crate::sync::What::Changed { id: row.key.clone(), field: "reminder".into(), to: String::new() }, crate::store::now());
                    self.synclog_unsaved = true;
                }
            }
        }
        if self.synclog_unsaved {
            if self.store.save("synclog", &Some(self.synclog.clone())).is_ok() {
                self.synclog_unsaved = false;
                if let Ok(Some(mut rows)) = pending {
                    let mut changed = false;
                    for row in rows.iter_mut().filter(|row| row.cancel_pending) {
                        if let Some(event) = self.synclog.events.iter().rev().find(|e| e.device == self.synclog.device && matches!(&e.what, crate::sync::What::Changed { id, to, field } if id == &row.key && to.is_empty() && field == "reminder")) {
                            if row.sync_version.as_ref().is_none_or(|old| crate::sync::Version::of(event) >= *old) {
                                row.sync_version = Some(crate::sync::Version::of(event)); row.cancel_pending = false; changed = true;
                            }
                        }
                    }
                    if changed { let _ = self.store.save(ELSEWHERE_KEY, &rows); }
                }
            }
        }
    }

    /// An exchange from your other device, into this one's thread.
    pub(super) fn take_an_exchange(&mut self, text: &str, sealed: bool) -> crate::error::Result<()> {
        if !sealed {
            return Ok(());
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else { return Ok(()) };
        let (Some(said), Some(reply)) = (v.get("said").and_then(|s| s.as_str()), v.get("reply").and_then(|s| s.as_str())) else { return Ok(()) };
        let at = v.get("at").and_then(|a| a.as_u64()).unwrap_or(0);
        if self.thread.recent.iter().any(|x| x.at == at && x.said == said) {
            return self.thread.save(&self.store);
        }
        let about = v.get("about").and_then(|a| a.as_str()).map(str::to_string);
        // In time order, not arrival order: an exchange from while the
        // phone was offline goes where it happened.
        let i = self.thread.recent.iter().position(|x| x.at > at).unwrap_or(self.thread.recent.len());
        self.thread.recent.insert(i, crate::thread::Exchange { at, said: said.into(), reply: reply.into(), about });
        self.thread.last_active = self.thread.last_active.max(at);
        self.arrived_by_sync.thread.insert((at, said.into()));
        self.thread.save(&self.store)
    }

    /// A fact from your other device: kept when it's newer than ours.
    pub(super) fn take_a_fact(&mut self, event: &crate::sync::Event, sealed: bool) -> crate::error::Result<()> {
        if !sealed {
            return Ok(());
        }
        for old in &self.synclog.events { self.facts.note_sync_version(old); }
        if self.facts.apply_synced(event) {
            if let crate::sync::What::Changed { to, .. } = &event.what {
                if let Ok(f) = serde_json::from_str::<crate::facts::Fact>(to) {
                    self.arrived_by_sync.facts.insert((f.name, f.as_of));
                }
            }
        }
        self.facts.save(&self.store)
    }

    /// The later list, from your other device.
    pub(super) fn take_a_later_item(&mut self, event: &crate::sync::Event, sealed: bool) -> crate::error::Result<()> {
        if !sealed { return Ok(()); }
        let crate::sync::What::Changed { id, .. } = &event.what else { return Ok(()) };
        let Some(what) = id.strip_prefix(LATER_PREFIX) else { return Ok(()) };
        let mut later: crate::later::Later = self.store.load_checked(crate::later::RECORD)?.unwrap_or_default();
        // Bootstrap versions from the durable event log when upgrading an
        // older list. New lists retain the version with the items themselves.
        for old in &self.synclog.events {
            later.note_version(old);
        }
        if later.apply_synced(event) {
            self.store.save(crate::later::RECORD, &later)?;
            self.arrived_by_sync.later.insert(what.to_string());
        }
        Ok(())
    }

    /// A reminder from your other device: noted here, never rung here (it
    /// rings where it was set). An empty `to` means it was cancelled -- and
    /// when it is one of OURS, cancelled from the other device, it is
    /// cancelled here, where it lives.
    pub(super) fn take_a_reminder(&mut self, event: &crate::sync::Event, sealed: bool) -> crate::error::Result<()> {
        if !sealed {
            return Ok(());
        }
        let crate::sync::What::Changed { id, field, to } = &event.what else { return Ok(()) };
        if field != "reminder" { return Ok(()); }
        let Some((device, job)) = owner_of(id) else { return Ok(()) };
        let version = crate::sync::Version::of(event);
        if device == self.synclog.device {
            let Some(current) = self.scheduler.jobs.iter().find(|j| j.id == job && j.command.starts_with("reminder ") && j.every.is_none() && j.on.is_none()) else { return Ok(()) };
            if current.sync_version.as_ref().is_some_and(|old| old >= &version) { return self.scheduler.save(&self.store); }
            if to.is_empty() { self.cancel_scheduled_job(job); }
            if let Some(current) = self.scheduler.jobs.iter_mut().find(|j| j.id == job) { current.sync_version = Some(version); }
            return self.scheduler.save(&self.store);
        }
        let mut elsewhere: Vec<Elsewhere> = self.store.load_checked(ELSEWHERE_KEY)?.unwrap_or_default();
        if elsewhere.iter().find(|e| e.key == *id).and_then(|e| e.sync_version.as_ref()).is_some_and(|old| old >= &version) { return Ok(()); }
        let (text, due) = if to.is_empty() {
            (String::new(), 0)
        } else {
            let Ok(v) = serde_json::from_str::<serde_json::Value>(to) else { return Ok(()) };
            let (Some(command), Some(due)) = (v.get("command").and_then(|c| c.as_str()), v.get("due").and_then(|d| d.as_u64())) else { return Ok(()) };
            let Some(text) = command.strip_prefix("reminder ") else { return Ok(()) };
            if due <= crate::store::now() {
                (String::new(), 0)
            } else {
                (text.to_string(), due)
            }
        };
        elsewhere.retain(|e| e.key != *id);
        elsewhere.push(Elsewhere { key: id.to_string(), text, due, sync_version: Some(version), cancel_pending: false });
        elsewhere.sort_by(|a, b| a.due.cmp(&b.due).then(a.key.cmp(&b.key)));
        self.store.save(ELSEWHERE_KEY, &elsewhere)
    }

    /// Your other devices' reminders still to come (the ones that have rung
    /// there are dropped).
    pub fn reminders_elsewhere(&self) -> Vec<Elsewhere> {
        let now = crate::store::now();
        let mut v: Vec<Elsewhere> = self.store.load(ELSEWHERE_KEY);
        v.retain(|e| e.due > now);
        v
    }

    /// One of your other devices' reminders, cancelled from here: gone from
    /// this list now, and cancelled where it lives on the next sync.
    pub(super) fn cancel_elsewhere(&mut self, key: &str, t: u64) -> crate::error::Result<()> {
        let _guard = self.store.transaction()?;
        let mut rows: Vec<Elsewhere> = self.store.load_checked(ELSEWHERE_KEY)?.unwrap_or_default();
        let Some(row) = rows.iter_mut().find(|row| row.key == key) else { return Ok(()) };
        let mut log = self.synclog.clone();
        if let Some(version) = &row.sync_version { log.observe_version(version, t); }
        log.append(crate::sync::What::Changed { id: key.into(), field: "reminder".into(), to: String::new() }, t);
        row.sync_version = log.events.last().map(crate::sync::Version::of);
        row.text.clear(); row.due = 0; row.cancel_pending = true;
        self.store.save(ELSEWHERE_KEY, &rows)?;
        self.synclog = log; self.synclog_unsaved = true;
        Ok(())
    }

    /// Which device a reminder from elsewhere rings on, as said.
    pub(super) fn where_it_rings(&self) -> &'static str {
        if self.plat.device_kind() == crate::sync::Kind::Full {
            "on your phone"
        } else {
            "on your laptop"
        }
    }
}

/// What arrived by sync, so it isn't carried straight back out.
#[derive(Debug, Default)]
pub struct ArrivedBySync {
    pub thread: std::collections::HashSet<(u64, String)>,
    pub facts: std::collections::HashSet<(String, u64)>,
    pub later: std::collections::HashSet<String>,
}
