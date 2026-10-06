//! Atlas knowing it has crashed.
//!
//! Before this, it could not. There was no `panic::set_hook`, no
//! `catch_unwind`, no watchdog and no restart anywhere in the tree — the only
//! mention of `catch_unwind` in `src` was in `mend.rs`, listing it as a
//! *cheat to detect*. So a single `unwrap` on a malformed file ended the
//! process, the console window closed, and nothing brought it back. The next
//! time you started Atlas it would greet you as though nothing had happened,
//! because as far as it knew, nothing had.
//!
//! That matters more here than in most programs, for two reasons this project
//! has already written down elsewhere: Atlas is meant to be always-on and
//! handed to friends, so the person in front of it is often not the person
//! who could read a stack trace; and "Atlas works on itself" cannot mean
//! anything while Atlas cannot report its own failures.
//!
//! Three pieces, deliberately small:
//!
//! 1. **A note, written at the moment of the panic.** Not a log line — a
//!    dated file in the store, so it survives the process dying and is still
//!    there next start.
//! 2. **A caught tick.** One bad intent must not end the session. The daemon
//!    wraps the tick body; a panic inside it becomes a sentence and the loop
//!    goes round again.
//! 3. **One sentence, next start.** Said out loud, once, and then cleared —
//!    a crash you are never told about is the same as no crash report at all.
//!
//! What this deliberately does **not** do: restart Atlas, register a service,
//! or retry the thing that panicked. A panic means an assumption in the code
//! was wrong; repeating it immediately is how a crash becomes a loop. The
//! note says what happened and the next tick carries on with the rest.

use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// Set once the hook has been installed, so a second call is a no-op rather
/// than a hook that writes the note twice.
static INSTALLED: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// Set by the hook when it fires, on the thread that panicked.
    ///
    /// ## What was wrong
    ///
    /// This was a `static PANICKED: AtomicBool`, documented as *"`caught`
    /// reads it to tell a real panic apart from an `Err` that merely
    /// unwound"* — and `caught` never read it. It only stored `false`. So
    /// `caught` returned the same sentence either way, and that sentence is
    /// *"I've written down what happened and carried on."*
    ///
    /// Which is a claim about a file. It is false whenever the hook did not
    /// run: an unwind resumed from elsewhere, a panic raised before
    /// `crash::watch` was called, or an abort-on-panic build. The person is
    /// then told there is a crash note to look at and there is not.
    ///
    /// ## Why thread-local rather than a global
    ///
    /// The hook runs on the thread that panicked, and `caught` is called from
    /// the tick thread and from the crew's threads. With one global flag, two
    /// concurrent `caught` calls could have the wrong one consume the other's
    /// flag — so the sentence would be right or wrong depending on timing,
    /// which is the worst way for an honesty check to work. A thread-local is
    /// read by exactly the code that set it.
    static PANICKED: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// What Atlas knows about a crash.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Note {
    /// Unix seconds.
    pub at: u64,
    /// The panic message, as the standard library formatted it.
    pub what: String,
    /// `file:line` where it happened, when the panic carried a location.
    pub where_: String,
    /// What Atlas was doing. Set by `doing`, so a crash in the middle of a
    /// spoken turn does not report itself as "idle".
    pub during: String,
}

impl Note {
    /// One sentence, for saying out loud.
    ///
    /// Deliberately not the panic message verbatim: `called `Option::unwrap()`
    /// on a `None` value` is a sentence for whoever wrote the line, not for
    /// whoever is standing in the kitchen. The detail stays in the file.
    pub fn plain(&self) -> String {
        let doing =
            if self.during.trim().is_empty() { String::new() } else { format!(" while {}", self.during) };
        format!(
            "I stopped unexpectedly{doing} last time. I've written down what happened \
             — the Activity page shows it."
        )
    }

    /// The whole of it, for `atlas crash` and for a bug report.
    pub fn detail(&self) -> String {
        format!(
            "at {}\nduring: {}\nwhere: {}\nwhat: {}",
            self.at,
            if self.during.is_empty() { "(not recorded)" } else { &self.during },
            if self.where_.is_empty() { "(no location)" } else { &self.where_ },
            self.what
        )
    }
}

/// Where the note goes.
///
/// A file of its own rather than a key in the store's JSON, for one reason
/// that decides it: this is written from inside a panic hook, when the
/// program's invariants are already broken and anything clever may itself
/// panic. A single `write` of a small string is about as little as can be
/// asked of a process in that state.
pub fn note_path(store_root: &Path) -> PathBuf {
    store_root.join("last-crash.json")
}

/// What Atlas is doing right now, for the note.
///
/// A plain global rather than something threaded through every call, because
/// the hook runs on whatever thread panicked and has no access to `self`.
static DOING: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

/// Say what is happening, so a crash can say it too.
// Private, and deliberately. `caught` below is the only thing that sets or
// clears this, and `caught` is the wired entry point (`daemon.rs:7557`). It
// shipped `pub` with the 17 Sep merge, which took `dead_capabilities`'
// helper-untested count from 27 to 28 -- a `pub` for no reason anything can
// see, which is what that guard is counting. Narrowing it is the fix; raising
// the ceiling would have been the mistake.
fn doing(what: &str) {
    if let Ok(mut g) = DOING.lock().or_else(crate::crash::unpoison) {
        g.clear();
        g.push_str(what);
    }
}

fn current_doing() -> String {
    DOING.lock().or_else(crate::crash::unpoison).map(|g| g.clone()).unwrap_or_default()
}

/// Install the panic hook. Call once, at startup, before anything else.
///
/// Keeps the default hook as well: the standard message still goes to stderr,
/// because someone running from a terminal should still see it. This adds the
/// note; it does not hide anything.
pub fn watch(store_root: &Path) {
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }
    let path = note_path(store_root);
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        PANICKED.with(|p| p.set(true));
        let note = Note {
            at: crate::store::now(),
            what: message_of(info),
            where_: info.location().map(|l| format!("{}:{}", l.file(), l.line())).unwrap_or_default(),
            during: current_doing(),
        };
        // Best effort, and silent on failure. A panic hook that panics
        // aborts the process, which would turn a recoverable crash into an
        // unrecoverable one — so every step here is allowed to fail.
        if let Some(dir) = path.parent() {
            crate::heard!(std::fs::create_dir_all(dir));
        }
        if let Ok(json) = serde_json::to_string_pretty(&note) {
            // Beside it and rename, not straight over the top. `fs::write`
            // truncates first, so a second failure while the hook is running
            // — which is exactly the situation the hook is for — would leave
            // an empty or half-written note where the previous crash's note
            // used to be, and the previous one is the more useful of the two.
            let tmp = path.with_extension("json.writing");
            if std::fs::write(&tmp, json).is_ok() {
                crate::kept!(std::fs::rename(&tmp, &path));
            }
        }
        previous(info);
    }));
}

fn message_of(info: &std::panic::PanicHookInfo<'_>) -> String {
    // `payload_as_str` is not stable for every payload shape, so both of the
    // usual ones are handled explicitly.
    let p = info.payload();
    if let Some(s) = p.downcast_ref::<&str>() {
        (*s).to_string()
    } else if let Some(s) = p.downcast_ref::<String>() {
        s.clone()
    } else {
        "something went wrong that didn't say what".to_string()
    }
}

/// The note from last time, if there is one.
pub fn last(store: &Store) -> Option<Note> {
    let text = std::fs::read_to_string(note_path(store.root())).ok()?;
    serde_json::from_str(&text).ok()
}

/// Read it and clear it, so it is said once and not every morning.
pub fn take(store: &Store) -> Option<Note> {
    let n = last(store);
    if n.is_some() {
        crate::heard!(std::fs::remove_file(note_path(store.root())));
    }
    n
}

/// A lock whose holder panicked, taken anyway (5 Oct 2026 audit, Q16).
///
/// `lock()` fails once a thread panicked while holding it, and about 170
/// places answered that with `if let Ok(..)` or `.ok()`: the work behind the
/// lock was skipped, silently, for the rest of the run. The panic itself is
/// already caught and said (`caught`, the panic hook); what it leaves behind
/// is ordinary data, and carrying on with it beats a part of Atlas going
/// quietly dead. Used as `m.lock().or_else(crate::crash::unpoison)`, which
/// keeps each call site's shape and is never an `Err`.
pub fn unpoison<G>(p: std::sync::PoisonError<G>) -> std::result::Result<G, std::sync::PoisonError<G>> {
    Ok(p.into_inner())
}
/// Run something, and turn a panic inside it into an error.
///
/// This is what stops one bad intent ending the session. `AssertUnwindSafe`
/// is the honest part: it says "I know state may be left half-changed". That
/// is acceptable *here* and nowhere else, because the alternative is not a
/// consistent Atlas — it is no Atlas at all, with the console window gone.
/// The next tick reloads what it needs from the store, which is the same
/// thing a restart would have done.
pub fn caught<T>(what: &str, f: impl FnOnce() -> T) -> std::result::Result<T, String> {
    doing(what);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    doing("");
    match r {
        Ok(v) => Ok(v),
        Err(_) => {
            // Read, then cleared. The whole point of the flag.
            let note_written = PANICKED.with(|p| p.replace(false));
            Err(if note_written {
                format!(
                    "something went wrong while {what}. I've written down what happened \
                     and carried on."
                )
            } else {
                // Said differently because it IS different, and the
                // difference is whether there is anything for you to look at.
                format!(
                    "something went wrong while {what} and I couldn't write down what — \
                     there's no crash note for this one. I've carried on."
                )
            })
        }
    }
}

/// How many times the background Atlas starts itself again after a crash
/// within [`AGAIN_WINDOW_SECS`] before it stops trying.
pub const AGAIN_AT_MOST: usize = 3;

/// The window those restarts are counted in.
pub const AGAIN_WINDOW_SECS: u64 = 600;

/// Where the recent restarts are written down.
fn restarts_path(state_dir: &Path) -> PathBuf {
    state_dir.join("restarted-after-crash.json")
}

/// Should the background Atlas start itself again after a crash that got
/// past every `caught` (28 Sep 2026)? Yes, and the restart is written down --
/// unless it has already done so [`AGAIN_AT_MOST`] times in the last
/// [`AGAIN_WINDOW_SECS`], which is a crash that happens on every start, and
/// starting again would only be a loop that eats a core.
///
/// Why at all: Atlas now runs with nothing open, started at sign-in. A panic
/// that escaped the loop ended the process, the icon by the clock went, the
/// hub stopped answering, and nothing brought it back until the next sign-in
/// -- while the crash note that would have explained it waited for a start
/// that wasn't coming. Task Scheduler doesn't restart a program that exits
/// with an error.
pub fn may_start_again(state_dir: &Path, now: u64) -> bool {
    let path = restarts_path(state_dir);
    let mut recent: Vec<u64> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    recent.retain(|at| now.saturating_sub(*at) < AGAIN_WINDOW_SECS && *at <= now);
    if recent.len() >= AGAIN_AT_MOST {
        return false;
    }
    recent.push(now);
    crate::heard!(std::fs::create_dir_all(state_dir));
    // A restart that can't be counted isn't made: uncounted, a crash on
    // every start would restart for ever.
    serde_json::to_string(&recent).ok().is_some_and(|json| std::fs::write(&path, json).is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atlas-crash-{tag}-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    }

    #[test]
    fn a_note_reads_as_a_sentence_and_not_as_a_stack_trace() {
        let n = Note {
            at: 100,
            what: "called `Option::unwrap()` on a `None` value".into(),
            where_: "src/gguf.rs:300".into(),
            during: "reading a model file".into(),
        };
        let said = n.plain();
        assert!(said.contains("while reading a model file"));
        assert!(!said.contains("unwrap"), "the panic message reached the spoken line: {said}");
        assert!(!said.contains("gguf.rs"), "a source path reached the spoken line: {said}");
        // The detail is kept, just not said.
        assert!(n.detail().contains("src/gguf.rs:300"));
        assert!(n.detail().contains("unwrap"));
    }

    #[test]
    fn a_note_with_nothing_recorded_still_reads_as_english() {
        let n = Note { at: 1, what: "x".into(), where_: String::new(), during: String::new() };
        assert!(!n.plain().contains("while "), "an empty `during` left a dangling word");
        assert!(n.detail().contains("(no location)"));
    }

    #[test]
    fn catching_turns_a_panic_into_a_sentence() {
        let r: std::result::Result<(), String> = caught("doing the thing", || panic!("boom"));
        let why = r.unwrap_err();
        assert!(why.contains("doing the thing"));
        assert!(!why.contains("boom"), "the panic payload reached the person: {why}");
    }

    #[test]
    fn catching_does_not_change_the_answer_when_nothing_goes_wrong() {
        assert_eq!(caught("counting", || 2 + 2), Ok(4));
    }

    #[test]
    fn what_atlas_was_doing_is_cleared_afterwards_so_the_next_crash_is_not_mislabelled() {
        let _ = caught("answering you", || 1);
        assert_eq!(current_doing(), "", "a finished job is still reported as in progress");
    }

    #[test]
    fn the_note_survives_a_round_trip_through_the_file() {
        let root = temp("roundtrip");
        let store = Store::new(&root);
        let n = Note { at: 7, what: "w".into(), where_: "f:1".into(), during: "d".into() };
        std::fs::write(note_path(store.root()), serde_json::to_string(&n).unwrap()).unwrap();
        assert_eq!(last(&store).unwrap(), n);
        // And taking it clears it, so it is said once.
        assert_eq!(take(&store).unwrap(), n);
        assert!(last(&store).is_none(), "the crash would be reported every start forever");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn no_note_is_not_an_error() {
        let root = temp("empty");
        let store = Store::new(&root);
        assert!(last(&store).is_none());
        assert!(take(&store).is_none());
        let _ = std::fs::remove_dir_all(&root);
    }
}
