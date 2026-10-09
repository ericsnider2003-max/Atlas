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
    #[serde(default)]
    pub phase: PhonePhase,
    #[serde(default)]
    pub watch: Option<u64>,
    #[serde(default)]
    pub worker: Option<u64>,
    #[serde(default)]
    pub reply: Option<String>,
    #[serde(default)]
    pub reply_sent: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhonePhase { #[default] Waiting, Held, Authorized, Pending, Running, Finished, Failed, Uncertain, #[serde(other)] Unknown }

pub(crate) const ASKED_KEY: &str = "asked_by_your_phone";
const MY_ASKS_KEY: &str = "asked_of_the_laptop";

impl Daemon<'_> {
    /// The phone's half: a request for the laptop, queued and carried. And
    /// "go ahead on the laptop" for the last one it held. `None` when the
    /// words aren't either.
    pub(super) fn ask_the_laptop_turn(&mut self, said: &str, t: u64) -> Option<String> {
        if crate::remote::go_ahead_on_the_laptop(said) {
            let guard = match self.store.transaction() { Ok(guard) => guard, Err(error) => return Some(format!("Your approval wasn't sent: storage is busy ({error}).")) };
            let mut q: crate::remote::Queue = match self.store.load_checked(MY_ASKS_KEY) { Ok(value) => value.unwrap_or_default(), Err(error) => return Some(format!("Your approval wasn't sent: request coverage is unavailable ({error}).")) };
            let last = q.requests.iter().rev().find(|r| r.state != crate::remote::State::Done)?.clone();
            if last.receipt_status.as_deref() != Some("held") && !(last.receipt_status.is_none() && last.state == crate::remote::State::Running) { return Some("That laptop request is not confirmed to be waiting for an approval. Check its current result before asking it to repeat anything.".into()); }
            q.set(last.id, crate::remote::State::Waiting);
            if let Err(error) = self.store.save(MY_ASKS_KEY, &q) { return Some(format!("Your approval wasn't sent: its queue couldn't be saved ({error}).")); }
            let (id, field, to) = crate::remote::yes_to_carry(&self.synclog.device.clone(), last.id);
            let previous = self.synclog.clone(); self.synclog.append(crate::sync::What::Changed { id, field, to }, t);
            if let Err(error) = self.store.save("synclog", &Some(self.synclog.clone())) { self.synclog = previous; return Some(format!("Your approval wasn't sent: its sync event couldn't be saved ({error}).")); }
            drop(guard);
            // unheard-ok: returns `String`, not a Result
            let _ = self.carry_to_your_other_devices(t);
            return Some(format!("Told the laptop to go ahead with \"{}\".", last.what));
        }
        let what = crate::remote::handed_over(said)?;
        if self.plat.device_kind() == crate::sync::Kind::Full {
            // This is the laptop: just do it.
            return Some(self.turn_from(&what, t, Arrival::Directed));
        }
        let guard = match self.store.transaction() { Ok(guard) => guard, Err(error) => return Some(format!("Your request wasn't sent: storage is busy ({error}).")) };
        let mut q: crate::remote::Queue = match self.store.load_checked(MY_ASKS_KEY) { Ok(value) => value.unwrap_or_default(), Err(error) => return Some(format!("Your request wasn't sent: request coverage is unavailable ({error}).")) };
        let n = q.ask(&what, crate::remote::Needs::TheLaptop, crate::remote::How::Quietly, t);
        if let Err(error) = self.store.save(MY_ASKS_KEY, &q) { return Some(format!("Your request wasn't sent: its queue couldn't be saved ({error}).")); }
        let (id, field, to) = crate::remote::ask_to_carry(&self.synclog.device.clone(), n, &what, t);
        let previous = self.synclog.clone(); self.synclog.append(crate::sync::What::Changed { id, field, to }, t);
        if let Err(error) = self.store.save("synclog", &Some(self.synclog.clone())) { self.synclog = previous; return Some(format!("Your request is kept locally, but wasn't sent: its sync event couldn't be saved ({error}).")); }
        drop(guard);
        // Carried now where the laptop can be reached; otherwise on the next
        // sync, and the request waits rather than disappearing.
        // unheard-ok: returns `String`, not a Result
        let _ = self.carry_to_your_other_devices(t);
        Some(crate::remote::sent_to_the_laptop(&what))
    }

    /// The laptop's half, from sync: a request from one of your own devices
    /// (sealed), kept until it's answered.
    pub(super) fn take_a_request_from_the_phone(&mut self, id: &str, to: &str, sealed: bool) -> crate::error::Result<()> {
        if !sealed || self.plat.device_kind() != crate::sync::Kind::Full {
            return Ok(());
        }
        let Some((device, n, what, at)) = crate::remote::read_ask(id, to) else { return Err(crate::error::AtlasError::Platform("malformed phone request; not accepted".into())) };
        let _guard = self.store.transaction()?;
        let mut asked: Vec<FromThePhone> = self.store.load_checked(ASKED_KEY)?.unwrap_or_default();
        if asked.iter().any(|a| a.device == device && a.n == n) {
            return Ok(());
        }
        asked.push(FromThePhone { device, n, what, at, held: false, phase: PhonePhase::Waiting, watch: None, worker: None, reply: None, reply_sent: false });
        self.store.save(ASKED_KEY, &asked)
    }

    /// The laptop's half, from sync: your yes to a held request.
    pub(super) fn take_a_yes_from_the_phone(&mut self, id: &str, sealed: bool, _t: u64) -> crate::error::Result<Vec<String>> {
        if !sealed {
            return Ok(Vec::new());
        }
        let Some((device, n)) = crate::remote::read_yes(id) else { return Ok(Vec::new()) };
        let _guard = self.store.transaction()?;
        let mut asked: Vec<FromThePhone> = self.store.load_checked(ASKED_KEY)?.unwrap_or_default();
        let Some(a) = asked.iter_mut().find(|a| a.device == device && a.n == n && (a.phase == PhonePhase::Held || a.phase == PhonePhase::Waiting && a.held)) else { return Ok(Vec::new()) };
        a.phase = PhonePhase::Authorized; a.held = true; a.reply = None; a.reply_sent = false;
        self.store.save(ASKED_KEY, &asked)?;
        Ok(Vec::new())
    }

    /// Run what your phone asked for, on each tick: readings now, anything
    /// that might change something held for your yes. Each answer goes back.
    pub(super) fn answer_requests_from_the_phone(&mut self, t: u64) {
        let asked: Vec<FromThePhone> = match self.store.load_checked(ASKED_KEY) { Ok(value) => value.unwrap_or_default(), Err(_) => return };
        let cfg = self.tools_cfg().remote.clone();
        for a in asked {
            let mut next = a.clone();
            match a.phase {
                PhonePhase::Waiting if a.held => { next.phase = PhonePhase::Held; }
                PhonePhase::Waiting if crate::remote::needs_your_yes(&a.what, &cfg) => {
                    next.held = true; next.phase = PhonePhase::Held; next.reply = Some(crate::remote::held_for_your_yes(&a.what)); next.reply_sent = false;
                }
                PhonePhase::Waiting | PhonePhase::Authorized => {
                    // Old builds also see held=true and therefore cannot replay an attempted action.
                    next.held = true; next.phase = PhonePhase::Pending; next.reply = None; next.reply_sent = false;
                    if self.save_phone_request(&a, &next).is_err() { continue; }
                    self.last_crew_handoff = None;
                    let reply = self.turn_from(&a.what, t, Arrival::Directed);
                    let watch = self.last_crew_handoff.and_then(|id| self.crew_links.get(&id).map(|link| link.watch_id));
                    let pending = next.clone();
                    next.watch = watch;
                    next.worker = self.last_crew_handoff;
                    next.phase = if watch.is_some() { PhonePhase::Running } else if self.operating.is_some() || self.active_file_move_id().is_some() || self.working_through_steps() || !matches!(self.session.pending, crate::session::Pending::Nothing) || crate::remote::reading_or_change(&a.what) != crate::remote::Looks::LikeAReading { PhonePhase::Uncertain } else { PhonePhase::Finished };
                    next.reply = Some(if next.phase == PhonePhase::Uncertain { format!("Laptop reply; completion is unconfirmed and the request is held for review: {reply}") } else { reply });
                    if self.save_phone_request(&pending, &next).is_err() { continue; }
                    if self.answer_the_phone(&next, t).is_ok() { let mut sent = next.clone(); sent.reply_sent = true; if let Err(error) = self.save_phone_request(&next, &sent) { self.log.warn(&format!("phone reply delivery marker was not saved ({error}); the durable request remains fenced and its reply will be retried")); } }
                    continue;
                }
                PhonePhase::Pending | PhonePhase::Unknown => {
                    next.phase = PhonePhase::Uncertain; next.held = true; next.reply_sent = false;
                    next.reply = Some("The laptop's earlier attempt has no durable completion receipt. Check what happened before requesting it again; it will not be replayed automatically.".into());
                }
                PhonePhase::Running => {
                    let job = a.watch.and_then(|id| self.long_work.jobs.iter().find(|job| job.id == id));
                    if let Some(job) = job.filter(|job| job.outcome != crate::watching::Outcome::Running) {
                        next.phase = PhonePhase::Uncertain;
                        next.reply = Some(if job.last_line.is_empty() { "The linked laptop worker ended without a usable answer. Check its result before repeating.".into() } else { job.last_line.clone() });
                        if job.last_line.is_empty() { next.phase = PhonePhase::Uncertain; }
                        next.reply_sent = false;
                    } else if !a.watch.is_some_and(|watch| self.crew_links.values().any(|link| link.watch_id == watch)) {
                        next.phase = PhonePhase::Uncertain; next.reply_sent = false;
                        next.reply = Some("The linked laptop worker is no longer running and has no completion receipt. The request is held for review and will not be repeated automatically.".into());
                    }
                }
                _ => {}
            }
            if next != a && self.save_phone_request(&a, &next).is_err() { continue; }
            if next.reply.is_some() && !next.reply_sent && self.answer_the_phone(&next, t).is_ok() {
                let mut sent = next.clone(); sent.reply_sent = true; if let Err(error) = self.save_phone_request(&next, &sent) { self.log.warn(&format!("phone reply delivery marker was not saved ({error}); the durable request remains fenced and its reply will be retried")); }
            }
        }
    }

    fn save_phone_request(&self, expected: &FromThePhone, next: &FromThePhone) -> crate::error::Result<()> {
        let _guard = self.store.transaction()?;
        let mut asked: Vec<FromThePhone> = self.store.load_checked(ASKED_KEY)?.unwrap_or_default();
        let Some(current) = asked.iter_mut().find(|a| a.device == expected.device && a.n == expected.n && **a == *expected) else { return Err(crate::error::AtlasError::Platform("phone request changed or disappeared".into())); };
        *current = next.clone(); self.store.save(ASKED_KEY, &asked)
    }

    pub(super) fn finish_phone_worker(&mut self, worker: u64, outcome: &crate::taskloop::Outcome, t: u64) {
        use crate::taskloop::Outcome;
        if matches!(outcome, Outcome::Started(_)) { return; }
        let asked: Vec<FromThePhone> = match self.store.load_checked(ASKED_KEY) { Ok(value) => value.unwrap_or_default(), Err(_) => return };
        for a in asked.into_iter().filter(|a| a.phase == PhonePhase::Running && a.worker == Some(worker)) {
            let mut next = a.clone(); next.phase = if matches!(outcome, Outcome::Done(_)) { PhonePhase::Finished } else if matches!(outcome, Outcome::Failed(_)) { PhonePhase::Failed } else { PhonePhase::Uncertain };
            next.reply = Some(outcome.text().to_string()); next.reply_sent = false;
            if self.save_phone_request(&a, &next).is_ok() && self.answer_the_phone(&next, t).is_ok() { let mut sent = next.clone(); sent.reply_sent = true; if let Err(error) = self.save_phone_request(&next, &sent) { self.log.warn(&format!("phone reply delivery marker was not saved ({error}); the durable request remains fenced and its reply will be retried")); } }
        }
    }

    /// The answer, back to the phone that asked: a sync event for its
    /// thread, and a knock on its lock screen (titles only, as every push).
    fn answer_the_phone(&mut self, a: &FromThePhone, t: u64) -> crate::error::Result<()> {
        let text = a.reply.as_deref().unwrap_or("");
        let done = a.phase == PhonePhase::Finished;
        let (id, field, to) = crate::remote::answer_to_carry(&a.device, a.n, &a.what, text, done);
        let mut receipt: serde_json::Value = serde_json::from_str(&to).map_err(|e| crate::error::AtlasError::Platform(e.to_string()))?;
        receipt["status"] = serde_json::to_value(a.phase).map_err(|e| crate::error::AtlasError::Platform(e.to_string()))?;
        let to = receipt.to_string();
        let _guard = self.store.transaction()?;
        let previous = self.synclog.clone();
        self.synclog.append(crate::sync::What::Changed { id, field, to }, t);
        if let Err(error) = self.store.save("synclog", &Some(self.synclog.clone())) { self.synclog = previous; return Err(error); }
        drop(_guard);
        self.log.info(&format!("answered your phone's \"{}\"", a.what));
        let phone = self.tools_cfg().phone.clone();
        if crate::phone::configured(&phone).is_ok() {
            let note = crate::notify::Note::new(
                if done { "Your laptop answered" } else if a.phase == PhonePhase::Held { "Your laptop is waiting for a yes" } else if a.phase == PhonePhase::Running { "Your laptop is working" } else { "Your laptop needs a result check" },
                text,
                crate::notify::Urgency::Routine,
                t,
            );
            if crate::phone::send(&note, &phone).is_err() {
                // It's in the thread when the phone next syncs either way.
            }
        }
        Ok(())
    }

    /// The phone's half, from sync: the laptop's answer to one of ours.
    pub(super) fn take_an_answer_from_the_laptop(&mut self, id: &str, to: &str, sealed: bool) -> crate::error::Result<Option<String>> {
        if !sealed {
            return Ok(None);
        }
        let Some((device, n, what, text, done)) = crate::remote::read_answer(id, to) else { return Err(crate::error::AtlasError::Platform("malformed laptop receipt; completion was not accepted".into())) };
        if device != self.synclog.device {
            return Ok(None);
        }
        let value: serde_json::Value = serde_json::from_str(to).map_err(|e| crate::error::AtlasError::Platform(e.to_string()))?;
        let state = match value.get("status").and_then(|v| v.as_str()) {
            Some("finished") if done => crate::remote::State::Done,
            Some("running") if !done => crate::remote::State::Running,
            Some("failed") if !done => crate::remote::State::Failed,
            Some("held" | "uncertain") if !done => crate::remote::State::Waiting,
            None if value.get("status").is_none() => if done { crate::remote::State::Done } else { crate::remote::State::Waiting },
            _ => return Err(crate::error::AtlasError::Platform("invalid laptop answer status; completion was not accepted".into())),
        };
        let _guard = self.store.transaction()?;
        let mut q: crate::remote::Queue = self.store.load_checked(MY_ASKS_KEY)?.unwrap_or_default();
        if let Some(current) = q.requests.iter().find(|request| request.id == n) {
            if current.what != what { return Err(crate::error::AtlasError::Platform("laptop receipt does not match the original request".into())); }
            if current.state == crate::remote::State::Done && state != crate::remote::State::Done { return Ok(None); }
            if current.receipt_status.as_deref() == Some("running") && value.get("status").and_then(|v| v.as_str()) == Some("held") { return Ok(None); }
        }
        if !q.set(n, state) {
            return Ok(None);
        }
        if let Some(request) = q.requests.iter_mut().find(|request| request.id == n) { request.receipt_status = value.get("status").and_then(|v| v.as_str()).map(str::to_string); }
        self.store.save(MY_ASKS_KEY, &q)?;
        Ok(Some(format!("From your laptop, on \"{what}\": {text}")))
    }
}

#[cfg(test)]
mod durable_phone_requests {
    use super::*;
    fn fixture(run: impl FnOnce(&mut Daemon<'_>, &crate::store::Store)) {
        let root = std::env::temp_dir().join(format!("atlas-phone-fence-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = crate::store::Store::new(root);
        let mut cfg = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        cfg.tools.as_mut().unwrap().phone = Default::default();
        let platform = crate::platform::mock::MockPlatform::new(vec![]);
        let mut daemon = Daemon::new(&cfg, &platform, None, store.clone(), crate::proactive::Proactive::new(Default::default()));
        run(&mut daemon, &store);
    }
    fn request(phase: PhonePhase) -> FromThePhone {
        FromThePhone { device: "fixture-phone".into(), n: 1, what: "check what time it is".into(), at: 100, held: phase != PhonePhase::Waiting, phase, watch: None, worker: None, reply: None, reply_sent: false }
    }
    #[test]
    fn busy_storage_blocks_incoming_ack_and_action() {
        fixture(|daemon, store| {
            store.save(ASKED_KEY, &vec![request(PhonePhase::Waiting)]).unwrap();
            let root = store.root().to_path_buf(); let (ready_tx, ready_rx) = std::sync::mpsc::channel(); let (release_tx, release_rx) = std::sync::mpsc::channel();
            let held = std::thread::spawn(move || { let _guard = crate::store::state_transaction(&root).unwrap(); ready_tx.send(()).unwrap(); release_rx.recv().unwrap(); }); ready_rx.recv().unwrap();
            let (id, _, to) = crate::remote::ask_to_carry("fixture-phone", 2, "check the clock", 100);
            let accepted = daemon.take_a_request_from_the_phone(&id, &to, true);
            let events = daemon.synclog.events.len(); let turns = daemon.session.recorded;
            daemon.answer_requests_from_the_phone(101);
            let unchanged = daemon.session.recorded == turns && daemon.synclog.events.len() == events;
            release_tx.send(()).unwrap(); held.join().unwrap();
            assert!(accepted.is_err(), "caller must withhold ACK when acceptance is not durable"); assert!(unchanged, "no action or answer event may start before the fence saves");
            let asked: Vec<FromThePhone> = store.load_checked(ASKED_KEY).unwrap().unwrap(); assert_eq!(asked.len(), 1); assert_eq!(asked[0].phase, PhonePhase::Waiting);
        });
    }
    #[test]
    fn restarted_pending_and_orphaned_running_requests_never_replay() {
        for phase in [PhonePhase::Pending, PhonePhase::Running] {
            fixture(|daemon, store| {
                let a = request(phase); store.save(ASKED_KEY, &vec![a.clone()]).unwrap();
                let mut restarted = Daemon::new(daemon.cfg, daemon.plat, None, store.clone(), crate::proactive::Proactive::new(Default::default()));
                let daemon = &mut restarted;
                let before = daemon.session.recorded; daemon.answer_requests_from_the_phone(101); daemon.answer_requests_from_the_phone(102);
                let asked: Vec<FromThePhone> = store.load_checked(ASKED_KEY).unwrap().unwrap();
                assert_eq!(daemon.session.recorded, before); assert_eq!(asked[0].phase, PhonePhase::Uncertain); assert!(asked[0].held);
                let (id, _, to) = crate::remote::ask_to_carry(&a.device, a.n, &a.what, 100); daemon.take_a_request_from_the_phone(&id, &to, true).unwrap();
                assert_eq!(store.load_checked::<Vec<FromThePhone>>(ASKED_KEY).unwrap().unwrap().len(), 1);
            });
        }
    }
    #[test]
    fn started_is_not_done_and_only_the_exact_worker_can_finish() {
        fixture(|daemon, store| {
            let mut a = request(PhonePhase::Running); a.worker = Some(42); store.save(ASKED_KEY, &vec![a]).unwrap();
            let events = daemon.synclog.events.len();
            daemon.finish_phone_worker(42, &crate::taskloop::Outcome::Started("Reading now".into()), 101);
            daemon.finish_phone_worker(43, &crate::taskloop::Outcome::Done("wrong worker".into()), 102);
            assert_eq!(store.load_checked::<Vec<FromThePhone>>(ASKED_KEY).unwrap().unwrap()[0].phase, PhonePhase::Running);
            assert_eq!(daemon.synclog.events.len(), events);
            daemon.finish_phone_worker(42, &crate::taskloop::Outcome::Done("the exact answer".into()), 103);
            let asked: Vec<FromThePhone> = store.load_checked(ASKED_KEY).unwrap().unwrap(); assert_eq!(asked[0].phase, PhonePhase::Finished);
            assert_eq!(asked[0].reply.as_deref(), Some("the exact answer"));
        });
    }
    #[test]
    fn malformed_or_mismatched_answers_never_claim_completion() {
        fixture(|daemon, store| {
            let mut queue = crate::remote::Queue::default(); let n = queue.ask("check the clock", crate::remote::Needs::TheLaptop, crate::remote::How::Quietly, 100); store.save(MY_ASKS_KEY, &queue).unwrap();
            let (id, _, _) = crate::remote::answer_to_carry(&daemon.synclog.device, n, "check the clock", "done", true);
            assert!(daemon.take_an_answer_from_the_laptop(&id, r#"{"what":"check the clock","text":"done"}"#, true).is_err());
            let (_, _, to) = crate::remote::answer_to_carry(&daemon.synclog.device, n, "different request", "done", true);
            assert!(daemon.take_an_answer_from_the_laptop(&id, &to, true).is_err());
            assert_eq!(store.load_checked::<crate::remote::Queue>(MY_ASKS_KEY).unwrap().unwrap().requests[0].state, crate::remote::State::Waiting);
        });
    }
}
