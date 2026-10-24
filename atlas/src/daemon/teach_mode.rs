//! Teaching a mode by voice (Plan B proof slice, 24 Oct 2026): the
//! read-back, then the yes. See `crate::teachmode`.

use super::*;

/// What was taught, waiting for your yes, and when it was said.
const PENDING: &str = "teach_mode_pending";
/// How long a read-back waits for the yes.
const YES_WITHIN_SECS: u64 = 5 * 60;

impl Daemon<'_> {
    /// `None` unless the words teach a mode, or answer a read-back.
    pub(super) fn teach_mode_turn(&mut self, said: &str, t: u64) -> Option<String> {
        let pending: Option<(u64, crate::modes::Mode)> = self.store.load(PENDING);
        if let Some((when, mode)) =
            pending.filter(|(when, _)| t.saturating_sub(*when) <= YES_WITHIN_SECS)
        {
            let _ = when;
            // Only the very next thing said answers the read-back: moving on
            // drops it, so a later yes to something else is never taken.
            let answer = crate::teachmode::answer(said);
            if answer.is_none() {
                let _ = self.store.save(PENDING, &None::<(u64, crate::modes::Mode)>);
            }
            if let Some(yes) = answer {
                let _ = self.store.save(PENDING, &None::<(u64, crate::modes::Mode)>);
                if !yes {
                    return Some(format!("Left it -- no {} mode.", mode.name));
                }
                let name = mode.name.clone();
                self.modes.add(mode);
                return Some(match self.modes.save(&self.store) {
                    Ok(()) => format!("Done. Say \"{name} mode\" whenever you want it."),
                    Err(e) => format!(
                        "I couldn't keep the {name} mode ({e}), so it's gone after a restart."
                    ),
                });
            }
        }
        match crate::teachmode::parse(said)? {
            Err(why) => Some(why),
            Ok(mode) => {
                let replacing = self.modes.get(&mode.name).is_some();
                let back = crate::teachmode::read_back(&mode);
                if let Err(e) = self.store.save(PENDING, &Some((t, mode))) {
                    return Some(format!("I couldn't hold that while you decide ({e})."));
                }
                Some(if replacing {
                    format!("That replaces the mode you have with that name. {back}")
                } else {
                    back
                })
            }
        }
    }
}
