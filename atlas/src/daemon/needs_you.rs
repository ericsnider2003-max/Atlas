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
