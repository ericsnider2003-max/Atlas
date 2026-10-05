//! "Ask the laptop" from the phone, and hear back (item 24, P6).
//!
//! On the phone: "ask the laptop to find the contract", "on my laptop, check
//! the render finished". The phone says at once that it's gone to the
//! laptop, queues it as a sync event -- carried in your sealed bundles, so
//! only your own laptop takes it -- and tries to carry it straight away.
//!
//! On the laptop: a request from one of your own devices is run as if you'd
//! said it there, when it only asks to be told something
//! (`remote::reading_or_change`). Anything that might change something waits
//! for a yes, which you can give from the phone ("go ahead on the laptop").
//! The answer goes back the same way, and to the phone's notifications.
//!
//! Said on the laptop itself, "ask the laptop to X" is just X.

use super::*;

/// A request from one of your phones, as the laptop holds it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FromThePhone {
    /// The phone's device id and its number for the request.
    pub device: String,
    pub n: u64,
    pub what: String,
    pub at: u64,
    /// Waiting for a yes because it might change something.
    pub held: bool,
}

pub(crate) const ASKED_KEY: &str = "asked_by_your_phone";
const MY_ASKS_KEY: &str = "asked_of_the_laptop";

impl Daemon<'_> {
    /// The phone's half: a request for the laptop, queued and carried. And
    /// "go ahead on the laptop" for the last one it held. `None` when the
    /// words aren't either.
    pub(super) fn ask_the_laptop_turn(&mut self, said: &str, t: u64) -> Option<String> {
        if crate::remote::go_ahead_on_the_laptop(said) {
            let mut q: crate::remote::Queue = self.store.load(MY_ASKS_KEY);
            let last = q.requests.iter().rev().find(|r| r.state != crate::remote::State::Done)?.clone();
            q.set(last.id, crate::remote::State::Waiting);
            let _ = self.store.save(MY_ASKS_KEY, &q);
            let (id, field, to) = crate::remote::yes_to_carry(&self.synclog.device.clone(), last.id);
            self.synclog.append(crate::sync::What::Changed { id, field, to }, t);
            let _ = self.store.save("synclog", &Some(self.synclog.clone()));
            let _ = self.carry_to_your_other_devices(t);
            return Some(format!("Told the laptop to go ahead with \"{}\".", last.what));
        }
        let what = crate::remote::handed_over(said)?;
        if self.plat.device_kind() == crate::sync::Kind::Full {
            // This is the laptop: just do it.
            return Some(self.turn_from(&what, t, Arrival::Directed));
        }
        let mut q: crate::remote::Queue = self.store.load(MY_ASKS_KEY);
        let n = q.ask(&what, crate::remote::Needs::TheLaptop, crate::remote::How::Quietly, t);
        let _ = self.store.save(MY_ASKS_KEY, &q);
        let (id, field, to) = crate::remote::ask_to_carry(&self.synclog.device.clone(), n, &what, t);
        self.synclog.append(crate::sync::What::Changed { id, field, to }, t);
        let _ = self.store.save("synclog", &Some(self.synclog.clone()));
        // Carried now where the laptop can be reached; otherwise on the next
        // sync, and the request waits rather than disappearing.
        let _ = self.carry_to_your_other_devices(t);
        Some(crate::remote::sent_to_the_laptop(&what))
    }

    /// The laptop's half, from sync: a request from one of your own devices
    /// (sealed), kept until it's answered.
    pub(super) fn take_a_request_from_the_phone(&mut self, id: &str, to: &str, sealed: bool) {
        if !sealed || self.plat.device_kind() != crate::sync::Kind::Full {
            return;
        }
        let Some((device, n, what, at)) = crate::remote::read_ask(id, to) else { return };
        let mut asked: Vec<FromThePhone> = self.store.load(ASKED_KEY);
        if asked.iter().any(|a| a.device == device && a.n == n) {
            return;
        }
        asked.push(FromThePhone { device, n, what, at, held: false });
        let _ = self.store.save(ASKED_KEY, &asked);
    }

    /// The laptop's half, from sync: your yes to a held request.
    pub(super) fn take_a_yes_from_the_phone(&mut self, id: &str, sealed: bool, t: u64) -> Vec<String> {
        if !sealed {
            return Vec::new();
        }
        let Some((device, n)) = crate::remote::read_yes(id) else { return Vec::new() };
        let mut asked: Vec<FromThePhone> = self.store.load(ASKED_KEY);
        let Some(i) = asked.iter().position(|a| a.device == device && a.n == n && a.held) else { return Vec::new() };
        let a = asked.remove(i);
        let _ = self.store.save(ASKED_KEY, &asked);
        let reply = self.turn_from(&a.what, t, Arrival::Directed);
        self.answer_the_phone(&a, &reply, true, t);
        Vec::new()
    }

    /// Run what your phone asked for, on each tick: readings now, anything
    /// that might change something held for your yes. Each answer goes back.
    pub(super) fn answer_requests_from_the_phone(&mut self, t: u64) {
        let asked: Vec<FromThePhone> = self.store.load(ASKED_KEY);
        if asked.iter().all(|a| a.held) {
            return;
        }
        let cfg = self.tools_cfg().remote.clone();
        let mut keep = Vec::new();
        for a in asked {
            if a.held {
                keep.push(a);
                continue;
            }
            if crate::remote::needs_your_yes(&a.what, &cfg) {
                let line = crate::remote::held_for_your_yes(&a.what);
                self.answer_the_phone(&a, &line, false, t);
                keep.push(FromThePhone { held: true, ..a });
                continue;
            }
            let reply = self.turn_from(&a.what, t, Arrival::Directed);
            self.answer_the_phone(&a, &reply, true, t);
        }
        let _ = self.store.save(ASKED_KEY, &keep);
    }

    /// The answer, back to the phone that asked: a sync event for its
    /// thread, and a knock on its lock screen (titles only, as every push).
    fn answer_the_phone(&mut self, a: &FromThePhone, text: &str, done: bool, t: u64) {
        let (id, field, to) = crate::remote::answer_to_carry(&a.device, a.n, &a.what, text, done);
        self.synclog.append(crate::sync::What::Changed { id, field, to }, t);
        let _ = self.store.save("synclog", &Some(self.synclog.clone()));
        self.log.info(&format!("answered your phone's \"{}\"", a.what));
        let phone = self.tools_cfg().phone.clone();
        if crate::phone::configured(&phone).is_ok() {
            let note = crate::notify::Note::new(
                if done { "Your laptop answered" } else { "Your laptop is waiting for a yes" },
                text,
                crate::notify::Urgency::Routine,
                t,
            );
            if crate::phone::send(&note, &phone).is_err() {
                // It's in the thread when the phone next syncs either way.
            }
        }
    }

    /// The phone's half, from sync: the laptop's answer to one of ours.
    pub(super) fn take_an_answer_from_the_laptop(&mut self, id: &str, to: &str, sealed: bool) -> Option<String> {
        if !sealed {
            return None;
        }
        let (device, n, what, text, done) = crate::remote::read_answer(id, to)?;
        if device != self.synclog.device {
            return None;
        }
        let mut q: crate::remote::Queue = self.store.load(MY_ASKS_KEY);
        let state = if done { crate::remote::State::Done } else { crate::remote::State::Running };
        if !q.set(n, state) {
            return None;
        }
        let _ = self.store.save(MY_ASKS_KEY, &q);
        Some(format!("From your laptop, on \"{what}\": {text}"))
    }
}
