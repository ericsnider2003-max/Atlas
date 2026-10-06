//! Atlas being asked to stop, and stopping properly.
//!
//! ## What there was before this
//!
//! Nothing. `Daemon::run` was `loop { ... }` with no `break` and no signal
//! handler, so the only way Atlas ever ended was being killed — Ctrl-C, the
//! console window closed, a logoff, Task Manager. That meant three things
//! that exist for the way out never ran:
//!
//! * **`OnlyOne::release()`** — the instance lock. It has a caller in the
//!   tests and none in `src`, because there was no way out to call it from.
//!   So the lock always outlived the process, and restarting inside
//!   `GONE_AFTER_SECS` (150s) was refused with *"Atlas is already running…
//!   Close the other one first"* when there was nothing to close. Change a
//!   config line and restart, and you waited two and a half minutes.
//! * **`Helpers::stop_all()`** — documented *"Everything down — on suspend,
//!   on battery, on the way out."* Also no caller. So whisper, piper and the
//!   model server **survived Atlas**, holding their memory, and the next
//!   start spawned more.
//! * **A final `persist()`** — anything changed since the last one was lost.
//!
//! ## Signal safety
//!
//! A signal handler may do almost nothing: it interrupts the process at an
//! arbitrary instruction, so allocating, locking, or writing a file from
//! inside one is how a shutdown handler becomes the crash it was meant to
//! prevent. This one stores `true` in an `AtomicBool` and returns. **All** the
//! real work happens on the main thread, on the next pass of the run loop,
//! where it is ordinary code.
//!
//! ## No new dependency
//!
//! `platform/win.rs` already declares the Win32 functions it needs with
//! `unsafe extern "system"`, so the console handler is declared the same way
//! rather than adding a crate for one function. On unix, `signal` comes from
//! the libc that `std` already links.
//!
//! The `ctrlc` crate would do this in three lines. It is not here because
//! this Cargo.toml is curated — every dependency in it has a comment
//! explaining why it earns its place — and one atomic flag plus two
//! declarations is less to own than a dependency and its tree.

use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Why Atlas was asked to stop, kept for the record of runs (`whystopped`, item
/// 33). The first reason given wins: what started the way out is the answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Why {
    /// Closed on purpose: Atlas's own window, its icon, or Ctrl-C.
    YouClosedIt,
    /// Windows signing out, restarting or shutting down.
    WindowsEnding,
    /// Replaced by an update.
    Updating,
    /// Asked, with no more said.
    Asked,
}

impl Why {
    pub fn plain(&self) -> &'static str {
        match self {
            Why::YouClosedIt => "you closed it",
            Why::WindowsEnding => "Windows was signing out, restarting or shutting down",
            Why::Updating => "it was updating itself",
            Why::Asked => "it was asked to stop",
        }
    }

    fn code(self) -> u8 {
        match self {
            Why::YouClosedIt => 1,
            Why::WindowsEnding => 2,
            Why::Updating => 3,
            Why::Asked => 4,
        }
    }

    fn from_code(c: u8) -> Option<Why> {
        match c {
            1 => Some(Why::YouClosedIt),
            2 => Some(Why::WindowsEnding),
            3 => Some(Why::Updating),
            4 => Some(Why::Asked),
            _ => None,
        }
    }
}

/// The first reason given, 0 for none yet.
static WHY: AtomicU8 = AtomicU8::new(0);

/// Ask Atlas to stop, saying why.
pub fn please_stop_because(why: Why) {
    let _ = WHY.compare_exchange(0, why.code(), Ordering::SeqCst, Ordering::SeqCst);
    please_stop();
}

/// `please_stop_because` without waking anyone: all a Unix signal handler
/// may do is touch atomics (the doorbell takes a lock). A nap on Unix looks
/// at the flag at least every `doorbell::LONGEST_SLEEP_MS` for this reason.
#[cfg(unix)]
fn mark_stop(why: Why) {
    let _ = WHY.compare_exchange(0, why.code(), Ordering::SeqCst, Ordering::SeqCst);
    mark_asked();
}

fn mark_asked() {
    if ASKED.swap(true, Ordering::SeqCst) {
        TIMES.store(true, Ordering::SeqCst);
    }
}

/// Why Atlas is stopping: the first reason given, or `Asked`.
pub fn why() -> Why {
    Why::from_code(WHY.load(Ordering::SeqCst)).unwrap_or(Why::Asked)
}

/// Set by a signal handler, read by the run loop. Never cleared: once Atlas
/// has been asked to stop, asking again cannot un-ask it.
static ASKED: AtomicBool = AtomicBool::new(false);

/// How many times the person has asked. A second Ctrl-C while a shutdown is
/// already running means "I meant it", and should not be swallowed.
static TIMES: AtomicBool = AtomicBool::new(false);

/// Has Atlas been asked to stop?
///
/// Checked once per pass of the run loop. `Relaxed` would be enough for a
/// single flag, but the cost of `SeqCst` on one load per tick is nothing
/// measurable and the reasoning is shorter.
pub fn asked_to_stop() -> bool {
    ASKED.load(Ordering::SeqCst)
}

/// Ask Atlas to stop.
///
/// Public so any other way out and the tests use the same door the signal
/// handler does, rather than a second mechanism that behaves slightly
/// differently.
///
/// Named `please_stop` and not `ask_to_stop`, which is what it was called
/// for about an hour. `Crew::ask_to_stop(id)` already owns that name, and
/// `tests/dead_methods.rs` matches bare names: adding a second `ask_to_stop`
/// made `crew::ask_to_stop` — a genuine orphan, with no production caller —
/// read as newly wired, and the guard duly reported it as progress. The
/// collision is the hazard, not the wording, so the new name moves.
pub fn please_stop() {
    mark_asked();
    // Whatever is napping wakes to see it (`doorbell`).
    crate::doorbell::ring();
}

/// The file Atlas's own window leaves to ask the background Atlas to stop.
///
/// A file in the install's own state folder rather than a network route:
/// only something that can already write to this install can ask, so the
/// phone — which reaches the hub through Tailscale and arrives looking like
/// this machine — can never stop Atlas by accident or on purpose.
pub fn stop_file(state_dir: &std::path::Path) -> std::path::PathBuf {
    state_dir.join("please_stop")
}

/// What a stop request says when a new version is moving in.
pub const UPDATING: &str = "updating";

/// Has Atlas's window asked it to stop? Consumes the request, so the next
/// start isn't stopped by an old one. Checked once a pass of the run loop.
pub fn asked_by_file(state_dir: &std::path::Path) -> bool {
    let f = stop_file(state_dir);
    if f.is_file() {
        let said = std::fs::read_to_string(&f).unwrap_or_default();
        crate::heard!(std::fs::remove_file(&f));
        please_stop_because(if said.trim() == UPDATING { Why::Updating } else { Why::YouClosedIt });
        return true;
    }
    false
}

/// Asked more than once — someone is impatient, and they are entitled to be.
pub fn asked_twice() -> bool {
    TIMES.load(Ordering::SeqCst)
}

/// Start listening for the signals that mean "stop".
///
/// Safe to call more than once; installing a handler twice is harmless.
/// Failure to install is not fatal and is not reported: Atlas without a
/// signal handler behaves exactly as it did before this module existed, which
/// is worse but not broken, and a warning at startup about a thing the person
/// cannot act on is noise.
pub fn listen() {
    #[cfg(unix)]
    unsafe {
        // The two that mean "stop": Ctrl-C at a terminal, and the polite kill
        // that a service manager or `systemctl stop` sends.
        //
        // SIGHUP is deliberately NOT caught. On a real terminal it means the
        // terminal went away, and the default action — terminate — is right;
        // catching it would keep Atlas alive attached to nothing.
        const SIGINT: i32 = 2;
        const SIGTERM: i32 = 15;
        // Declared taking a function pointer rather than the `usize` that
        // `sighandler_t` really is, so the compiler checks the signature of
        // what is handed to it. The sentinel values (SIG_DFL, SIG_IGN) are
        // the reason the real type is an integer, and nothing here passes
        // one. The return is the previous handler, which is not wanted.
        unsafe extern "C" {
            fn signal(sig: i32, handler: extern "C" fn(i32)) -> usize;
        }
        extern "C" fn handler(sig: i32) {
            // Atomics only, as above: SIGINT is a person at a terminal;
            // SIGTERM is the system (a service manager, a shutdown).
            mark_stop(if sig == 2 { Why::YouClosedIt } else { Why::WindowsEnding });
        }
        signal(SIGINT, handler);
        signal(SIGTERM, handler);
    }

    #[cfg(windows)]
    unsafe {
        // `SetConsoleCtrlHandler`, declared here the way `platform/win.rs`
        // declares what it needs.
        //
        // Returning TRUE says "handled", which stops the default
        // termination for CTRL_C and CTRL_BREAK and lets the run loop exit on
        // its own terms. For CTRL_CLOSE, CTRL_LOGOFF and CTRL_SHUTDOWN
        // Windows gives roughly five seconds before killing the process
        // regardless, which is ample for a persist and a lock release but is
        // the reason the shutdown does the important things first.
        #[allow(clippy::upper_case_acronyms, reason = "the Win32 type, by its Win32 name")]
        type BOOL = i32;
        const TRUE: BOOL = 1;
        unsafe extern "system" {
            fn SetConsoleCtrlHandler(handler: Option<unsafe extern "system" fn(u32) -> BOOL>, add: BOOL) -> BOOL;
        }
        unsafe extern "system" fn handler(kind: u32) -> BOOL {
            // CTRL_C 0, CTRL_BREAK 1, CTRL_CLOSE 2: a person. CTRL_LOGOFF 5,
            // CTRL_SHUTDOWN 6: Windows ending. Atomics only.
            please_stop_because(if kind >= 5 { Why::WindowsEnding } else { Why::YouClosedIt });
            TRUE
        }
        SetConsoleCtrlHandler(Some(handler), TRUE);
    }
}

/// Sleep, unless someone asks to stop first.
///
/// The run loop's idle sleep is up to two seconds. Sleeping through it means
/// Ctrl-C appears to do nothing for that long, which is exactly how long it
/// takes someone to press it again — and the second press, on Windows, is
/// answered by the console giving the process about five seconds and then
/// killing it regardless. So the sleep is broken into slices and the flag is
/// checked between them.
///
/// The slice is 50ms because it is short enough to feel immediate and long
/// enough that the checking costs nothing: at the two-second maximum this is
/// forty atomic loads, against a sleep syscall that is already thousands of
/// times more expensive.
pub fn nap(total_ms: u64) {
    let until = std::time::Instant::now() + std::time::Duration::from_millis(total_ms);
    loop {
        let seen = crate::doorbell::rung();
        if asked_to_stop() {
            return;
        }
        let left = until.saturating_duration_since(std::time::Instant::now());
        if left.is_zero() {
            return;
        }
        // `please_stop` rings the doorbell, so this sleeps the whole nap
        // rather than waking every 50 ms to look (audit Q14).
        crate::doorbell::wait_after(seen, left.as_millis().max(1) as u64);
    }
}

/// Only for tests: forget that anyone asked.
///
/// Nothing in `src` calls this, and nothing should. The flag is deliberately
/// one-way in normal operation — a shutdown that can be cancelled halfway is
/// a shutdown that leaves half of it done. The only reason it can be cleared
/// at all is that the flag is process-wide (a signal handler cannot be handed
/// a context), so a test file's tests would otherwise contaminate each other.
///
/// Carries the `_for_test` suffix because that is the one the dead-capability
/// sweeps exempt, per `vault::pre_envelope_for_test` and
/// `profiles::isolated_for_test`. Naming it `reset_for_tests` — the plural —
/// put it on the unwired list, which is the guard working correctly: a
/// test-only capability that does not say so in its name is
/// indistinguishable from one that was built and forgotten.
/// Ask Atlas to stop and wait, up to `wait`, for its way out to finish --
/// which it marks by letting go of its lock, the last thing it does.
/// Returns whether it finished in time.
///
/// For Windows signing you out or shutting down (28 Sep 2026): it ends the
/// background Atlas the moment the icon's window has answered, and nothing
/// was listening, so every sign-out killed Atlas mid-whatever -- state since
/// the last save lost, the model server killed rather than stopped, and the
/// lock left behind for the next sign-in to wait out. Now the icon's window
/// asks, and holds Windows for a few seconds while the way out runs.
pub fn stop_and_wait(lock: &std::path::Path, wait: std::time::Duration) -> bool {
    please_stop_because(Why::WindowsEnding);
    let until = std::time::Instant::now() + wait;
    while lock.exists() {
        if std::time::Instant::now() >= until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    true
}

#[doc(hidden)]
pub fn reset_for_test() {
    ASKED.store(false, Ordering::SeqCst);
    TIMES.store(false, Ordering::SeqCst);
    WHY.store(0, Ordering::SeqCst);
}
