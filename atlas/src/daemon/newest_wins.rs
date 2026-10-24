//! Newest wins, and nothing is lost (24 Oct 2026, Dispatch for Eric).
//!
//! When the same thing was changed on two of your devices, each kind of
//! synced record already keeps the newer change (by its sync version). What
//! was missing: Atlas asked "Which?" and nothing ever took the answer, and
//! the version that lost was gone. Now the loser is kept in a short list
//! (`sync::REPLACED_KEY`), shown on the Sync page, and "put back what sync
//! replaced" makes it the newest change again -- here and, through sync, on
//! your other devices.

use super::*;

impl Daemon<'_> {
    /// Keep what a merge replaced, newest first, at most `REPLACED_KEPT`.
    pub(super) fn keep_replaced(&mut self, merged: &crate::sync::Merged, from: &str, now: u64) {
        let mut list: Vec<crate::sync::Replaced> = self.store.load(crate::sync::REPLACED_KEY);
        for r in crate::sync::replaced_by(merged, from, now) {
            list.insert(0, r);
        }
        list.truncate(crate::sync::REPLACED_KEPT);
        if let Err(e) = self.store.save(crate::sync::REPLACED_KEY, &list) {
            self.log
                .warn(&format!("what sync replaced couldn't be kept: {e}"));
        }
    }

    /// "What did sync replace?" and "put back what sync replaced". `None`
    /// when the words are neither.
    pub(super) fn sync_replaced_turn(&mut self, said: &str, t: u64) -> Option<String> {
        if crate::sync::asks_what_was_replaced(said) {
            let list: Vec<crate::sync::Replaced> = self.store.load(crate::sync::REPLACED_KEY);
            return Some(match list.first() {
                None => "Sync hasn't replaced anything.".to_string(),
                Some(r) => format!(
                    "Most recently, {}: kept \"{}\" (from {}), replaced \"{}\". {} in all.",
                    r.subject,
                    r.kept,
                    r.kept_from,
                    r.replaced,
                    list.len()
                ),
            });
        }
        if !crate::sync::asks_to_put_back(said) {
            return None;
        }
        let mut list: Vec<crate::sync::Replaced> = self.store.load(crate::sync::REPLACED_KEY);
        let Some(r) = list.first().cloned() else {
            return Some("There's nothing sync replaced to put back.".to_string());
        };
        let what = if r.replaced == "removed" {
            crate::sync::What::Removed {
                id: r.subject.clone(),
            }
        } else {
            crate::sync::What::Changed {
                id: r.subject.clone(),
                field: r.field.clone(),
                to: r.replaced.clone(),
            }
        };
        let previous = self.synclog.clone();
        self.synclog.append(what, t);
        let Some(event) = self.synclog.events.last().cloned() else {
            return Some("I couldn't put it back.".to_string());
        };
        if let Err(e) = self.store.save("synclog", &Some(self.synclog.clone())) {
            self.synclog = previous;
            return Some(format!(
                "I couldn't put it back: the change couldn't be saved ({e})."
            ));
        }
        // Made true here as your other device's changes are, then carried.
        if let Err(e) = self.take_in_synced(&[event], true, "you") {
            return Some(format!(
                "I couldn't put it back here ({e}). It goes to your other devices."
            ));
        }
        list.remove(0);
        if let Err(e) = self.store.save(crate::sync::REPLACED_KEY, &list) {
            self.log
                .warn(&format!("what sync replaced couldn't be updated: {e}"));
        }
        // unheard-ok: returns `String`, not a Result
        let _ = self.carry_to_your_other_devices(t);
        Some(if r.replaced == "removed" {
            format!(
                "Removed {} again, as it was before sync replaced it.",
                r.subject
            )
        } else {
            format!(
                "Put back {}: \"{}\" again, newest now on all your devices.",
                r.subject, r.replaced
            )
        })
    }
}
