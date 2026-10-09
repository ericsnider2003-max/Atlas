//! Failures nobody was waiting for (5 Oct 2026, audit Q1).
//!
//! The clean-pass audit counted 1,146 `let _ = ...` in `src/`. Each threw a
//! `Result` away, so a failed write, a failed delete, a failed process start
//! left no trace: Atlas said "done" and nothing told anyone it wasn't. The
//! saves among them were made loud first (`store::record_failed_save`, 4 Oct);
//! this does the same for everything else, through the same door.
//!
//! Two macros, chosen by what a failure costs:
//!
//! * [`kept!`] for writing something that is meant to last: a file, a setting,
//!   a copy. A failure is DATA NOT KEPT. `Daemon::persist` takes these with the
//!   failed saves and tells the person once, exactly as for its own records.
//! * [`heard!`] for everything else worth knowing about: a delete, a folder
//!   made, a helper program started. A failure is written to `atlas.log` once
//!   (by `persist`), never spoken.
//!
//! A file that is already gone is not a failed delete, so `NotFound` from an
//! `io::Error` is not recorded by either macro.
//!
//! What stays `let _ =` on purpose, and is listed in the guard test
//! `tests/no_silent_discards.rs`: a send whose reader has stopped (the other
//! side is shutting down), killing or waiting on a process that has already
//! ended, socket shutdown and timeouts, Win32 UI calls whose failure has no
//! remedy, values bound only to be dropped, and `Store::save`, which already
//! reports its own failures. Anything else must use one of these macros, and
//! the guard test fails when a new silent discard appears.
//!
//! Bounded: a disk that stays full can't grow this without limit. Keyed by
//! where in the code, so a loop failing a thousand times is one entry with a
//! count, not a thousand.

use std::fmt::Display;
use std::sync::Mutex;

/// How bad a failure is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cost {
    /// Something meant to be kept wasn't (`kept!`): the person is told.
    NotKept,
    /// Worth a line in the log (`heard!`).
    Logged,
}

/// One place in the code that failed, since it was last taken.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unheard {
    pub cost: Cost,
    /// `module_path!()` of the call, e.g. `atlas::yourchanges`.
    pub module: &'static str,
    pub line: u32,
    /// What was being done, in the code's own words (the expression).
    pub doing: &'static str,
    /// The most recent error.
    pub error: String,
    /// How many times it failed since last taken.
    pub times: u32,
}

impl Unheard {
    /// The module's own name, `yourchanges` for `atlas::yourchanges`: short
    /// enough to put in a sentence to the person.
    pub fn part(&self) -> &'static str {
        self.module.rsplit("::").next().unwrap_or(self.module)
    }

    /// One line for the log.
    pub fn line(&self) -> String {
        let times = if self.times > 1 { format!(" ({} times)", self.times) } else { String::new() };
        format!("{}:{} {} failed{}: {}", self.part(), self.line, self.doing, times, self.error)
    }
}

const MOST_KEPT: usize = 128;
static FAILURES: Mutex<Vec<Unheard>> = Mutex::new(Vec::new());

/// Record a failure reported by this module's result adapter.
fn record(cost: Cost, module: &'static str, line: u32, doing: &'static str, error: &str) {
    let Ok(mut v) = FAILURES.lock().or_else(crate::crash::unpoison) else { return };
    if let Some(u) = v.iter_mut().find(|u| u.module == module && u.line == line) {
        u.times = u.times.saturating_add(1);
        u.error = error.to_string();
        return;
    }
    if v.len() >= MOST_KEPT {
        // The oldest log-only entry goes first: a lost write is never pushed
        // out by noise.
        let at = v.iter().position(|u| u.cost == Cost::Logged).unwrap_or(0);
        v.remove(at);
    }
    v.push(Unheard { cost, module, line, doing, error: error.to_string(), times: 1 });
}

/// Every failure since the last time this was asked. Taken, so each is
/// reported once.
pub fn take() -> Vec<Unheard> {
    match FAILURES.lock().or_else(crate::crash::unpoison) {
        Ok(mut v) => std::mem::take(&mut *v),
        Err(_) => Vec::new(),
    }
}

/// True when `e` is worth recording: everything except an `io::Error` saying
/// the thing is already gone (a delete of a file that isn't there succeeded).
fn worth_hearing<E: Display + 'static>(e: &E) -> bool {
    match (e as &dyn std::any::Any).downcast_ref::<std::io::Error>() {
        Some(io) => io.kind() != std::io::ErrorKind::NotFound,
        None => true,
    }
}

/// What the macros call. Returns the value on success, `None` on failure.
#[doc(hidden)]
pub fn hear<T, E: Display + 'static>(
    r: Result<T, E>,
    cost: Cost,
    module: &'static str,
    line: u32,
    doing: &'static str,
) -> Option<T> {
    match r {
        Ok(v) => Some(v),
        Err(e) => {
            if worth_hearing(&e) {
                record(cost, module, line, doing, &e.to_string());
            }
            None
        }
    }
}

/// Write something meant to last; if it fails, the person is told (see the
/// module docs). Evaluates to `Option<T>`.
#[macro_export]
macro_rules! kept {
    ($e:expr) => {
        $crate::unheard::hear($e, $crate::unheard::Cost::NotKept, module_path!(), line!(), stringify!($e))
    };
}

/// Do something whose failure belongs in the log (see the module docs).
/// Evaluates to `Option<T>`.
#[macro_export]
macro_rules! heard {
    ($e:expr) => {
        $crate::unheard::hear($e, $crate::unheard::Cost::Logged, module_path!(), line!(), stringify!($e))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    // Q8 mutation baseline (6 Oct 2026): the log line could be blank, or
    // count a single failure as "(1 times)", without a test noticing.
    #[test]
    fn the_log_line_names_the_place_and_counts_only_repeats() {
        let mut u = Unheard {
            cost: Cost::Logged,
            module: "atlas::yourchanges",
            line: 42,
            doing: "std::fs::write(&p, b)",
            error: "denied".into(),
            times: 1,
        };
        assert_eq!(u.line(), "yourchanges:42 std::fs::write(&p, b) failed: denied");
        u.times = 2;
        assert_eq!(u.line(), "yourchanges:42 std::fs::write(&p, b) failed (2 times): denied");
    }

    // One test touches the shared list, so parallel tests can't race on it.
    #[test]
    fn failures_are_kept_counted_and_taken_once() {
        let _ = take();
        let gone: std::io::Result<()> = Err(std::io::Error::from(std::io::ErrorKind::NotFound));
        assert_eq!(heard!(gone), None);
        assert!(take().is_empty(), "a file already gone is not a failure");

        for _ in 0..3 {
            let denied: std::io::Result<()> = Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            assert_eq!(kept!(denied), None);
        }
        let ok: Result<u8, String> = Ok(7);
        assert_eq!(heard!(ok), Some(7));
        let other: Result<(), String> = Err("no route".into());
        let _ = heard!(other);

        let got = take();
        assert_eq!(got.len(), 2, "{got:?}");
        let lost = got.iter().find(|u| u.cost == Cost::NotKept).unwrap();
        assert_eq!(lost.times, 3);
        assert_eq!(lost.part(), "tests");
        assert!(lost.doing.contains("denied"));
        assert!(got.iter().any(|u| u.cost == Cost::Logged && u.error == "no route"));
        assert!(take().is_empty(), "taken once");

        for i in 0..(MOST_KEPT as u32 + 10) {
            record(Cost::Logged, "m", i, "x", "e");
        }
        record(Cost::NotKept, "m", 9999, "write", "full");
        for i in 0..20 {
            record(Cost::Logged, "n", i, "x", "e");
        }
        let got = take();
        assert!(got.len() <= MOST_KEPT);
        assert!(got.iter().any(|u| u.cost == Cost::NotKept), "a lost write is never pushed out by noise");
    }
}
