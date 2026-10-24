//! A market "what if" over a bars file (Plan F proof slice, 24 Oct 2026).
//! Reads the file, replays it, says the odds. Nothing else.

use super::*;

impl Daemon<'_> {
    /// `None` unless the words ask a what-if with a stop, a target and a file.
    pub(super) fn whatif_turn(&mut self, said: &str) -> Option<String> {
        let (s, path) = crate::whatif::parse(said)?;
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => return Some(format!("I couldn't read {path}: {e}")),
        };
        let bars = match crate::whatif::bars_from_csv(&text) {
            Ok(b) => b,
            Err(why) => return Some(format!("I couldn't use {path}: {why}")),
        };
        let view = match bars.latest() {
            Ok(v) => v,
            Err(r) => return Some(format!("I couldn't use {path}: {}", r.0)),
        };
        Some(match crate::whatif::replay(&view, &s) {
            Ok(o) => crate::whatif::said(&o, &s),
            Err(why) => why,
        })
    }
}
