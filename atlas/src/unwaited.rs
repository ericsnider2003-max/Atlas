//! Children nobody is waiting for.
//!
//! ## The name
//!
//! This was called `orphans` for twenty minutes, which is the obvious word
//! and the wrong one. Two reasons, and both are about this tree rather than
//! about English:
//!
//! * **"Orphan" already means something else here.** `dead_methods.rs`,
//!   `dead_capabilities.rs` and `new_capabilities_are_wired.rs` all use it
//!   for a *capability* with no caller — "these orphans now have a caller or
//!   a test". A module named after the tree's word for unwired code, whose
//!   subject is unreaped processes, is two unrelated ideas under one word.
//! * **The guards read words.** `bug_sweep`'s stale-documentation scan looks
//!   for a module name appearing as a whole word near a phrase like "waiting
//!   on", and `HANDOVER_2026-09-14.md` contains *"the nine orphans are named
//!   individually with what each is waiting on"* — ordinary prose about
//!   capabilities, which the guard immediately and correctly reported as a
//!   doc claiming something unbuilt that is now wired. A module whose name is
//!   a common English word in this project's own vocabulary will keep doing
//!   that, and the fix is the name, not a baseline entry.
//!
//! `unwaited` is what these actually are: started on purpose, not waited for
//! on purpose, and collected here so that not waiting does not mean leaking.
//!
//! ## The leak
//!
//! Three places start a process and deliberately do not wait for it, because
//! waiting is the wrong thing to do: a notification panel, an app the person
//! asked for, a browser. Each wrote
//!
//! ```text
//! cmd.spawn().map(|_| ())
//! ```
//!
//! which starts the process and drops the [`Child`] on the same line.
//!
//! On Windows that is fine — dropping the handle is all the cleanup there is.
//! On Linux and macOS it is not. A child that has exited stays in the process
//! table as a zombie until its **parent** collects its exit status, and
//! dropping a `Child` in Rust does not collect it and does not detach it. The
//! documentation is explicit: *"There is no way to detach a child; the
//! `Child` structure does no cleanup on drop."*
//!
//! Atlas is a daemon. It runs for days, and until it exits nothing collects
//! any of them:
//!
//! * `window::open` re-execs Atlas once per panel. Every notification the
//!   person reads and closes leaves a `<defunct>` entry behind for the rest
//!   of the session.
//! * `Platform::launch` leaves one per app opened.
//! * `browser::launch` leaves one per headless browser started.
//!
//! None of it is visible until it is: a zombie holds a process-table slot and
//! a PID, and the per-user limit (`RLIMIT_NPROC`, commonly a few thousand) is
//! shared with everything else the person is running. Atlas is then the
//! program that stopped their machine from starting processes, and a `ps`
//! showing a hundred `atlas <defunct>` lines reads as Atlas leaking real
//! processes rather than exit statuses.
//!
//! ## Why a list and not `SIGCHLD`
//!
//! `signal(SIGCHLD, SIG_IGN)` makes the kernel reap automatically, in one
//! line, everywhere. It is not used here because it also makes `wait` and
//! `waitpid` fail with `ECHILD` for every child in the process — and three
//! things in this tree depend on waiting working: `tools::wait_or_kill` polls
//! `try_wait` to enforce every external tool's timeout, `lifecycle::Helpers`
//! kills and waits for whisper, piper and the model server, and
//! `frames::Rolling` does the same for ffmpeg. A one-line fix that silently
//! breaks the timeout on every voice tool is not a fix.
//!
//! So instead: hand the child here, and it is collected on the tick with
//! everything else.

use std::process::Child;
use std::sync::Mutex;

/// Children waiting to be collected.
///
/// A module-level list rather than a field on the daemon because the three
/// callers cannot reach the daemon: `window::open` is a free function called
/// from a panel path, and the two `launch` implementations are behind the
/// `Platform` trait, which is deliberately a narrow interface over an
/// operating system and not a door back into Atlas.
static LOOSE: Mutex<Vec<Child>> = Mutex::new(Vec::new());

/// Start it, and let it go — but keep the handle so it can be collected.
///
/// The child is not killed, waited for, or interfered with in any way. This
/// is the same fire-and-forget the callers already wanted; the only
/// difference is that its exit status has somewhere to go.
///
/// Reaps before pushing, so a process that runs entirely inside a CLI
/// invocation — where no tick ever comes — still does not accumulate.
pub fn dont_wait(child: Child) {
    let Ok(mut loose) = LOOSE.lock() else {
        // Poisoned: a thread panicked holding it. Losing one exit status is
        // not worth propagating a panic out of a notification.
        return;
    };
    collect(&mut loose);
    loose.push(child);
}

/// Collect whatever has finished. Called once a tick; never blocks.
///
/// Returns how many were collected, so the caller can log a number that
/// ought to be small.
pub fn reap() -> usize {
    let Ok(mut loose) = LOOSE.lock() else {
        return 0;
    };
    collect(&mut loose)
}

/// How many are still going after a reap.
///
/// Read on the way out, where it is the difference between "Atlas is gone"
/// and "Atlas is gone and the browser it opened is still up". Those are the
/// person's own windows and Atlas does not close them — but it should say
/// they are there, because it started them and nothing else will mention it.
pub fn still_running() -> usize {
    LOOSE.lock().map(|l| l.len()).unwrap_or(0)
}

/// `try_wait` on each, dropping the ones that have ended.
///
/// `try_wait` is the whole point: it returns `Ok(None)` for a child still
/// running and never blocks. An `Err` — which in practice means the status
/// was already taken — counts as collected, because retrying it forever is
/// how a list that exists to shrink stops shrinking.
fn collect(loose: &mut Vec<Child>) -> usize {
    let before = loose.len();
    loose.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
    before - loose.len()
}
