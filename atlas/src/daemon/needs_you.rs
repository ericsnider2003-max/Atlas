//! "What needs me?" and "not now" (Plan A proof slice, 24 Oct 2026): the
//! ranked short list from `crate::needs_you`, drawn from the same work
//! record every Hub page reads and the next hour of the calendar. Reading
//! only: the one thing it writes is your own "not now".

use super::*;

impl Daemon<'_> {
    /// `None` unless the words ask what needs you, or put off what was just
    /// listed.
    pub(super) fn needs_you_turn(&mut self, said: &str, t: u64) -> Option<String> {
        if crate::needs_you::asks(said) {
            let all = self.needs_you_now(t);
            let shown: Vec<String> = all
                .iter()
                .take(crate::needs_you::CAP)
                .map(|n| n.key.clone())
                .collect();
            if let Err(e) = self.store.save(crate::needs_you::LAST_KEY, &(t, shown)) {
                self.log
                    .warn(&format!("what needs you couldn't be kept: {e}"));
            }
            return Some(crate::needs_you::said(&all));
        }
        if crate::needs_you::puts_off(said) {
            // Only right after the list: "not now" to something else is
            // somebody else's answer.
            let (when, keys): (u64, Vec<String>) = self.store.load(crate::needs_you::LAST_KEY);
            if keys.is_empty() || t.saturating_sub(when) > 10 * 60 {
                return None;
            }
            let mut sn: crate::needs_you::Snoozed = self.store.load(crate::needs_you::SNOOZED_KEY);
            sn.put_off(&keys, t);
            if let Err(e) = self.store.save(crate::needs_you::SNOOZED_KEY, &sn) {
                return Some(format!(
                    "I couldn't put those off ({e}). I'll mention them again."
                ));
            }
            let _ = self
                .store
                .save(crate::needs_you::LAST_KEY, &(0u64, Vec::<String>::new()));
            return Some(format!(
                "Put off until {}. Anything new still comes through.",
                crate::localclock::hhmm_here(t + crate::needs_you::SNOOZE_SECS)
            ));
        }
        None
    }

    /// A timed spoken check-in (Plan A, 24 Oct 2026): what needs you and
    /// hasn't been said, at most every `CHECK_IN_EVERY_SECS`, once per item.
    /// `may_speak` is the same gate as every other unprompted line
    /// (`Proactive::may_interrupt`: conversation, idle, presenting, "do not
    /// disturb" from the OS); on top of it, Atlas's modes, a pause, mute and
    /// quiet hours. Nothing is marked said unless it is saved, so a failing
    /// store makes it silent rather than repetitive.
    pub(super) fn needs_you_check_in(&mut self, t: u64, may_speak: bool) -> Option<String> {
        if !may_speak || self.attention.is_paused() || !self.modes.may_interrupt(false) {
            return None;
        }
        let minute = ((t as i64 + crate::localclock::offset_secs()).rem_euclid(86_400) / 60) as u32;
        let sound = &self.tools_cfg().sound;
        if sound.muted || sound.in_quiet_hours(minute) {
            return None;
        }
        // Looked at most once a minute: ranking reads the work record.
        if t.saturating_sub(self.needs_you_looked) < 60 {
            return None;
        }
        self.needs_you_looked = t;
        let mut surfaced: crate::needs_you::Surfaced = self.store.load(crate::needs_you::SURFACED_KEY);
        if !surfaced.due(t) {
            return None;
        }
        let all = self.needs_you_now(t);
        let fresh = surfaced.fresh(&all);
        if fresh.is_empty() {
            return None;
        }
        surfaced.mark(&fresh, t);
        if let Err(e) = self.store.save(crate::needs_you::SURFACED_KEY, &surfaced) {
            self.log
                .warn(&format!("what was said about what needs you couldn't be kept: {e}"));
            return None;
        }
        // "Not now" right after refers to what was just said.
        let keys: Vec<String> = fresh.iter().map(|n| n.key.clone()).collect();
        let _ = self.store.save(crate::needs_you::LAST_KEY, &(t, keys));
        Some(crate::needs_you::said(&fresh))
    }

    /// Everything that needs you now, best first, minus what you put off.
    pub(crate) fn needs_you_now(&self, t: u64) -> Vec<crate::needs_you::Need> {
        let open = self.open_work(t);
        let soon: Vec<crate::needs_you::Soon> = self
            .calendar
            .occurrences_between(t, t + crate::needs_you::SOON_SECS)
            .into_iter()
            .filter(|e| !e.all_day)
            .map(|e| (e.id, e.title, e.start))
            .collect();
        let sn: crate::needs_you::Snoozed = self.store.load(crate::needs_you::SNOOZED_KEY);
        crate::needs_you::rank(&open, &soon, &sn, t)
    }
}

/// 24 Oct 2026: the timed check-in says each thing once, across restarts,
/// and keeps quiet in quiet hours.
#[cfg(test)]
mod check_in_tests {
    use super::*;
    use crate::config::Config;
    use crate::platform::mock::MockPlatform;
    use crate::proactive::{Proactive, ProactiveConfig};
    use crate::store::Store;

    fn root(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("atlas-checkin-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, root: &std::path::Path) -> Daemon<'a> {
        Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
    }

    /// The approvals waiting, as a whole set (that is how they are synced).
    fn ask(store: &Store, whats: &[&str]) {
        let set: Vec<(String, String)> = whats
            .iter()
            .map(|w| (crate::job::approval_key("chat", w), w.to_string()))
            .collect();
        crate::job::sync_approvals(store, &set).unwrap();
    }

    const T: u64 = 1_800_000_000;
    const HALF_HOUR: u64 = crate::needs_you::CHECK_IN_EVERY_SECS;

    #[test]
    fn an_item_is_said_once_not_again_after_a_restart_and_a_new_one_is() {
        let c = Config::load(std::path::Path::new("config")).unwrap();
        let p = MockPlatform::new(Vec::new());
        let r = root("once");
        let mut d = daemon(&c, &p, &r);
        ask(&d.store, &["Send the invoice to Harbor Lane?"]);

        let said = d.needs_you_check_in(T, true).expect("something new needs you");
        assert!(said.contains("Send the invoice"), "{said}");
        // Said once: not at the next look, nor the one after the interval.
        assert_eq!(d.needs_you_check_in(T + 120, true), None);
        assert_eq!(d.needs_you_check_in(T + HALF_HOUR + 120, true), None);

        // A restart: a new daemon on the same store says nothing more.
        drop(d);
        let mut d = daemon(&c, &p, &r);
        assert_eq!(d.needs_you_check_in(T + 2 * HALF_HOUR, true), None);

        // Something new is said, alone, once the interval since the last
        // check-in has passed (that was at T).
        ask(&d.store, &["Send the invoice to Harbor Lane?", "Close the old Chrome windows?"]);
        let said = d.needs_you_check_in(T + 3 * HALF_HOUR, true).expect("a new item");
        assert!(said.contains("Close the old Chrome"), "{said}");
        assert!(!said.contains("Send the invoice"), "{said}");
        assert!(said.starts_with("One thing needs you."), "{said}");
        // And the interval holds between two check-ins.
        ask(&d.store, &["Send the invoice to Harbor Lane?", "Close the old Chrome windows?", "Reply to Dana?"]);
        assert_eq!(d.needs_you_check_in(T + 3 * HALF_HOUR + 120, true), None, "inside the half hour");
        assert!(d.needs_you_check_in(T + 4 * HALF_HOUR + 120, true).is_some());
    }

    #[test]
    fn old_entries_expire_and_the_file_stays_bounded() {
        let mut s = crate::needs_you::Surfaced::default();
        let n = |k: &str| crate::needs_you::Need {
            key: k.into(),
            what: k.into(),
            why_now: String::new(),
            why: crate::needs_you::Why::YourMove,
            at: 0,
        };
        s.mark(&[n("ask:old")], T);
        s.mark(&[n("ask:new")], T + crate::needs_you::SURFACED_KEEP_SECS + 1);
        assert!(!s.at.contains_key("ask:old"));
        assert!(s.at.contains_key("ask:new"));
        let many: Vec<_> = (0..300).map(|i| n(&format!("ask:{i}"))).collect();
        s.mark(&many, T + crate::needs_you::SURFACED_KEEP_SECS + 2);
        assert!(s.at.len() <= crate::needs_you::SURFACED_MAX);
    }

    #[test]
    fn quiet_hours_a_mute_and_the_speaking_gate_hold_it_back_and_it_is_said_after() {
        let mut c = Config::load(std::path::Path::new("config")).unwrap();
        {
            let sound = &mut c.tools.get_or_insert_with(Default::default).sound;
            sound.quiet_hours = true;
            sound.quiet_from = crate::localclock::hhmm_here(T - 3600);
            sound.quiet_to = crate::localclock::hhmm_here(T + 3600);
        }
        let p = MockPlatform::new(Vec::new());
        let r = root("quiet");
        let mut d = daemon(&c, &p, &r);
        ask(&d.store, &["Send the invoice to Harbor Lane?"]);
        assert_eq!(d.needs_you_check_in(T, true), None, "quiet hours");
        // Nothing was marked said while it was held back.
        let held: crate::needs_you::Surfaced = d.store.load(crate::needs_you::SURFACED_KEY);
        assert!(held.at.is_empty());
        // The same moment, outside quiet hours, but Atlas may not speak.
        let mut c2 = Config::load(std::path::Path::new("config")).unwrap();
        c2.tools.get_or_insert_with(Default::default).sound.quiet_hours = false;
        let mut d2 = daemon(&c2, &p, &r);
        assert_eq!(d2.needs_you_check_in(T, false), None, "the interrupt gate says no");
        // Once it may speak, it says it.
        assert!(d2.needs_you_check_in(T + 120, true).is_some());
    }
}
