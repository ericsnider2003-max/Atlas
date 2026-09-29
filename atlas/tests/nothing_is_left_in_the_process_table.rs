//! Processes Atlas starts and does not wait for.
//!
//! ## The leak
//!
//! Three places start a process on purpose and do not wait for it — a
//! notification panel, an app the person asked to open, a headless browser.
//! All three were written the same way:
//!
//! ```text
//! cmd.spawn().map(|_| ())
//! ```
//!
//! which starts it and drops the `Child` on the same line. On Windows that
//! is the whole of the cleanup. On Linux and macOS a child that has exited
//! stays in the process table as a zombie until its **parent** collects its
//! exit status, and Rust's `Child` says so plainly: *"There is no way to
//! detach a child; the `Child` structure does no cleanup on drop."*
//!
//! Atlas is a daemon, so "until the parent exits" means days. One
//! `<defunct>` per notification the person reads and closes, one per app
//! they open. Nothing shows it until the per-user process limit is reached,
//! at which point Atlas is the program that stopped their machine from
//! starting anything — and `ps` showing a hundred `atlas <defunct>` lines
//! reads as Atlas leaking real processes rather than exit statuses.
//!
//! ## Why not `SIGCHLD`
//!
//! `signal(SIGCHLD, SIG_IGN)` fixes it in one line and is not used, because
//! it also makes `wait` fail with `ECHILD` for every child in the process.
//! `tools::wait_or_kill` polls `try_wait` to enforce every external tool's
//! timeout; `lifecycle::Helpers` waits on whisper, piper and the model
//! server; `frames::Rolling` waits on ffmpeg. A one-line fix that silently
//! breaks the timeout on every voice tool is not a fix. The structural test
//! at the bottom is there so that shortcut cannot be taken later without
//! someone reading that reasoning.

use std::process::Command;
use std::sync::{Mutex, MutexGuard};

/// The list is process-wide, because the three callers cannot reach the
/// daemon to be handed one. `cargo test` runs a file's tests on threads, so
/// anything counting what is outstanding holds this — and leaves the count
/// as it found it.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Poll until everything handed over has been collected, or give up.
fn until_collected(down_to: usize) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while std::time::Instant::now() < deadline {
        atlas::unwaited::reap();
        if atlas::unwaited::still_running() <= down_to {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    false
}

#[test]
fn a_child_handed_over_is_collected_once_it_ends() {
    let _g = alone();
    let before = atlas::unwaited::still_running();
    let child = Command::new("true")
        .spawn()
        .or_else(|_| Command::new("cmd").args(["/C", "exit"]).spawn())
        .expect("something that exits immediately");
    atlas::unwaited::dont_wait(child);
    assert!(
        atlas::unwaited::still_running() > before,
        "the child was not kept, so nothing can collect it"
    );

    // It exits immediately; give it a moment and then reap. Polled rather
    // than slept-and-asserted so this does not turn into a flake on a busy
    // machine.
    assert!(
        until_collected(before),
        "a process that has exited is still uncollected, which on unix is a zombie"
    );
}

#[test]
fn reaping_does_not_wait_for_something_still_running() {
    // The other half: `reap` is called once a tick, so it must never block.
    // `wait()` here instead of `try_wait()` would hold the whole daemon for
    // as long as the person leaves a panel open -- which is the difference
    // between housekeeping and a hang.
    let _g = alone();
    let before = atlas::unwaited::still_running();
    let child = Command::new("sleep")
        .arg("2")
        .spawn()
        .or_else(|_| Command::new("timeout").args(["/T", "2"]).spawn());
    let Ok(child) = child else {
        return; // no sleep on this machine; the assertion above still stands
    };
    atlas::unwaited::dont_wait(child);

    let started = std::time::Instant::now();
    atlas::unwaited::reap();
    let took = started.elapsed();
    assert!(
        took < std::time::Duration::from_millis(500),
        "reaping took {took:?} with a child still running -- it is waiting, and \
         that is a tick held hostage by however long the person leaves a panel open"
    );

    // Put the count back, so the other test measuring it is not racing this
    // one through a list they both share.
    assert!(until_collected(before), "the sleeping child was never collected");
}

#[test]
fn every_fire_and_forget_spawn_hands_its_child_over() {
    // The structural half, and the one that matters: this is a defect of
    // omission. Each site *worked* -- the panel opened, the app launched --
    // and the cost was invisible from inside the function.
    //
    // Only the sites that throw the handle away are looked for. A `spawn()`
    // whose `Child` is kept (`let mut child = ..`), returned, or waited for
    // is correct as it is.
    let mut offenders = Vec::new();
    for file in ["src/window.rs", "src/platform/posix.rs", "src/browser.rs", "src/platform/win.rs"] {
        let src = std::fs::read_to_string(file).unwrap_or_default();
        if src.is_empty() {
            continue;
        }
        for (i, line) in src.lines().enumerate() {
            // Comments are where the old shape is quoted, in the note
            // explaining why it went. Reading them as code makes the fix
            // fail its own test.
            if line.trim_start().starts_with("//") {
                continue;
            }
            if line.contains(".map(|_| ())") {
                // Is this a spawn? Look at the few lines above it.
                let start = i.saturating_sub(8);
                let near: String = src.lines().take(i).skip(start).collect::<Vec<_>>().join("\n");
                if near.contains(".spawn()") {
                    offenders.push(format!("{file}:{}  {}", i + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "these start a process and drop the handle on the same line, which on unix \
         leaves a zombie per call. Hand it to `unwaited::dont_wait` instead:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn something_actually_collects_them_on_the_tick() {
    // A list nobody empties is the leak with an extra allocation.
    let src = crate::common::source_of("daemon");
    assert!(
        src.contains("unwaited::reap()"),
        "nothing calls `unwaited::reap`, so the handles are kept and never collected \
         -- which is the original leak plus a growing Vec"
    );
}

#[test]
fn the_sigchld_shortcut_is_not_taken() {
    // `signal(SIGCHLD, SIG_IGN)` would reap automatically and break
    // `try_wait` everywhere, which is what enforces every external tool's
    // timeout. If a future change wants it, it has to delete this test and
    // read why first.
    let mut offenders = Vec::new();
    let mut stack = vec![std::path::PathBuf::from("src")];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in std::fs::read_dir(&p).expect("dir").flatten() {
                stack.push(e.path());
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        for (i, line) in text.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("///") || t.starts_with("//!") {
                continue;
            }
            if line.contains("SIGCHLD") || line.contains("SIG_IGN") {
                offenders.push(format!("{}:{}  {t}", p.display(), i + 1));
            }
        }
    }
    assert!(
        offenders.is_empty(),
        "SIGCHLD is being handled or ignored. That reaps children automatically \
         and makes `wait`/`try_wait` fail with ECHILD for every child in the \
         process -- which silently removes the timeout from every external tool, \
         from the helper shutdown, and from the camera. See `unwaited`:\n  {}",
        offenders.join("\n  ")
    );
}
