//! "What can this computer run?" (Plan C proof slice, 24 Oct 2026): the
//! tiers from `crate::preflight` against this machine as measured now.
//! Reading only.

use super::*;

impl Daemon<'_> {
    /// `None` unless the words ask what this machine can run.
    pub(super) fn preflight_turn(&mut self, said: &str) -> Option<String> {
        crate::preflight::asks(said).then(|| crate::preflight::said(&crate::fit::measure(), &crate::preflight::ladder()))
    }
}
