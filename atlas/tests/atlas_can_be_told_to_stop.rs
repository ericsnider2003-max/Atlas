//! Asking Atlas to stop, and Atlas stopping properly.
//!
//! ## What there was before
//!
//! `Daemon::run` was `loop { .. }` with no `break`, and nothing installed a
//! signal handler. So the only way Atlas ever ended was being killed —
//! Ctrl-C, the console window closed, a logoff, Task Manager. Every one of
//! those is a kill, and a killed process does not unwind, which meant three
//! pieces of code written for the way out had never once run in production:
//!
//! * **`OnlyOne::release`** — a caller in the tests, none in `src`. So the
//!   instance lock always outlived the process, and `onlyone.rs` said so in
//!   its own source: *"Nothing calls `release()` — `Daemon::run` is an
//!   infinite loop with no shutdown path, so the lock always outlives a clean
//!   exit."* Change a config line and restart, and Atlas refused for up to
//!   150 seconds, blaming a process that no longer existed.
//! * **`Helpers::stop_all`** — documented *"everything down, on the way
//!   out."* Also no caller in `src`.
//! * **The final `persist`** — reachable only through `Drop for Daemon`,
//!   which needs the value to go out of scope. `run` never returned, so it
//!   never did.
//!
//! That is the shape this tree keeps producing and the reason the guards
//! exist: code that compiles, is tested, is documented as doing exactly the
//! right thing, and is reached by nothing.
//!
//! ## What is being pinned
//!
//! Two halves. The behavioural half calls `shut_down` and checks that the
//! lock is gone, the state is on disk, and a second Atlas can start at once.
//! The structural half reads the source, because the *reason* none of this
//! ran was not a wrong value anywhere — it was a missing `break` and a
//! missing handler, and neither of those is visible to a test that only calls
//! functions.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::backlog::{Backlog, Blocker};
use atlas::onlyone::{Found, OnlyOne};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-goodbye-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("data")).unwrap();
    d
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, dir: &Path) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()))
}

fn conf() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// The ask-to-stop flag is process-wide — it has to be, because a signal
/// handler cannot be handed a context. `cargo test` runs a file's tests on
/// threads in the same process, so any test that touches the flag holds this.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

// ---------------------------------------------------------------- behaviour

#[test]
fn the_lock_is_let_go_of_on_the_way_out() {
    let (c, p) = (conf(), plat());
    let dir = tmp("lock");
    let mut d = daemon(&c, &p, &dir);

    let lock = OnlyOne::at(&d.store.data_dir());
    lock.take(1_000).expect("a fresh folder's lock is free");
    assert!(lock.path().exists(), "the lock was not taken, so this proves nothing");

    d.shut_down();

    assert!(
        !lock.path().exists(),
        "the lock file is still there after a clean stop, at {}. Every restart \
         inside the staleness window will be refused and blamed on a process \
         that is gone.",
        lock.path().display()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_second_atlas_can_start_the_moment_the_first_one_stops() {
    // The same fact from the person's side, which is the side it was felt
    // from: edit a config line, restart, and wait two and a half minutes.
    let (c, p) = (conf(), plat());
    let dir = tmp("restart");
    let mut d = daemon(&c, &p, &dir);

    let lock = OnlyOne::at(&d.store.data_dir());
    lock.take(1_000).expect("free");
    d.shut_down();

    // One second later, which is how long a person takes to press the up
    // arrow and Enter. Well inside GONE_AFTER_SECS, so before this it was
    // refused.
    let found = lock.look(1_001);
    assert_eq!(
        found,
        Found::Free,
        "a second later the lock still reads as {found:?}, so restarting is refused"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn what_changed_since_the_last_save_is_written_down() {
    // State changed outside a turn is the case `Drop for Daemon` was written
    // for and the case a killed process never reached. A backlog entry
    // because it is recorded by machinery rather than by a sentence: exactly
    // the kind of change that happens between ticks.
    let (c, p) = (conf(), plat());
    let dir = tmp("persist");
    let mut d = daemon(&c, &p, &dir);

    d.backlog.record("book the flight", Blocker::Offline, 1_000);
    d.shut_down();

    let reopened = Backlog::load(&Store::new(&dir));
    assert!(
        reopened.items.iter().any(|i| i.request == "book the flight"),
        "the backlog on disk does not have what was filed just before stopping: {:?}",
        reopened.items.iter().map(|i| &i.request).collect::<Vec<_>>()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn it_says_it_is_stopping() {
    // Not decoration. Atlas is voice-first and often has no visible window,
    // so a shutdown that says nothing is indistinguishable from a crash --
    // and this tree has a whole module (`crash`) built because that
    // distinction was impossible to make.
    let (c, p) = (conf(), plat());
    let dir = tmp("said");
    let mut d = daemon(&c, &p, &dir);

    let said = d.shut_down();
    assert!(
        said.iter().any(|s| s.to_lowercase().contains("stopping")),
        "stopping said nothing that names stopping: {said:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn stopping_twice_does_not_stop_twice() {
    // `Drop` still persists, for the paths that do not go through
    // `shut_down`. Without the guard flag an orderly exit would write all
    // sixteen files twice and, on a full disk, report the same failure twice
    // -- which reads as two faults.
    let (c, p) = (conf(), plat());
    let dir = tmp("twice");
    let mut d = daemon(&c, &p, &dir);

    let first = d.shut_down();
    let second = d.shut_down();

    assert!(!first.is_empty(), "the first stop did nothing");
    assert!(
        second.is_empty(),
        "stopping a second time went round again and said it again: {second:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn background_errands_are_called_off_before_the_saving_starts() {
    // Order again, for a different reason. `Drop for Crew` waits up to
    // SHUTDOWN_DEADLINE_SECS for errands to notice. If the asking happened
    // there too, the ten-second wait would begin only once everything else
    // had finished -- and on a Windows console close there are about five
    // seconds in total. Asking first means the errands wind down during the
    // persist.
    let mut crew = atlas::crew::Crew::new(2);
    let stopped = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = stopped.clone();
    crew.hand(
        "a long errand",
        0,
        Box::new(move |asked| {
            for _ in 0..200 {
                if asked.stopping() {
                    flag.store(true, std::sync::atomic::Ordering::SeqCst);
                    return Err("stopped".into());
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Ok("ran the whole way".into())
        }),
    )
    .expect("a free hand");

    // Give the errand a moment to actually be running.
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert_eq!(crew.ask_everyone_to_stop(), 1, "the running errand was not asked");

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !stopped.load(std::sync::atomic::Ordering::SeqCst)
        && std::time::Instant::now() < deadline
    {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        stopped.load(std::sync::atomic::Ordering::SeqCst),
        "the errand was never told to stop, so shutdown would wait out the full \
         deadline for work that could have ended in milliseconds"
    );
}

#[test]
fn the_way_out_calls_the_errands_off_first() {
    let src = crate::common::source_of("daemon");
    let at = src.find("pub fn shut_down(").expect("shut_down is gone");
    let body = &src[at..];
    let end = body.find("\n    }").map(|e| e + 6).unwrap_or(body.len());
    let body = &body[..end];

    let called_off = body
        .find("ask_everyone_to_stop()")
        .expect("shut_down no longer calls off background work");
    let persist = body.find("self.persist()").expect("shut_down no longer saves state");
    assert!(
        called_off < persist,
        "the errands are called off after the state is written, so the ten-second \
         wait in `Drop for Crew` starts at the very end instead of overlapping it"
    );
}

// ------------------------------------------------------------------ the ask

#[test]
fn asking_cannot_be_un_asked() {
    let _g = alone();
    atlas::goodbye::reset_for_test();

    assert!(!atlas::goodbye::asked_to_stop());
    atlas::goodbye::please_stop();
    assert!(atlas::goodbye::asked_to_stop());
    // Deliberately one-way. A shutdown that can be cancelled halfway is a
    // shutdown that leaves half of it done.
    atlas::goodbye::please_stop();
    assert!(atlas::goodbye::asked_to_stop());

    atlas::goodbye::reset_for_test();
}

#[test]
fn a_second_ask_is_noticed() {
    let _g = alone();
    atlas::goodbye::reset_for_test();

    atlas::goodbye::please_stop();
    assert!(
        !atlas::goodbye::asked_twice(),
        "one ask is reading as two, so the first Ctrl-C would skip the goodbye"
    );
    atlas::goodbye::please_stop();
    assert!(
        atlas::goodbye::asked_twice(),
        "a second Ctrl-C is not noticed, so an impatient person gets the same \
         unhurried shutdown they just told you was too slow"
    );

    atlas::goodbye::reset_for_test();
}

#[test]
fn the_wait_between_passes_ends_early_when_asked() {
    // The run loop's idle sleep is up to two seconds. Sleeping through it
    // means Ctrl-C appears to do nothing for that long, which is how long it
    // takes someone to press it again -- and on Windows the second press
    // starts a five-second countdown to being killed regardless.
    let _g = alone();
    atlas::goodbye::reset_for_test();

    let started = std::time::Instant::now();
    atlas::goodbye::nap(2_000);
    let slept = started.elapsed();
    assert!(
        slept >= std::time::Duration::from_millis(1_800),
        "the nap did not actually wait ({slept:?}), so the loop would spin"
    );

    atlas::goodbye::please_stop();
    let started = std::time::Instant::now();
    atlas::goodbye::nap(2_000);
    let after = started.elapsed();
    assert!(
        after < std::time::Duration::from_millis(300),
        "asked to stop and the wait still ran for {after:?} of its two seconds"
    );

    atlas::goodbye::reset_for_test();
}

// ----------------------------------------------------------------- structure

#[test]
fn the_run_loop_has_a_way_out() {
    // The whole defect, in one missing statement. A behavioural test cannot
    // see this: every function involved existed and worked, and nothing
    // reached them because the loop had no `break`.
    let src = crate::common::source_of("daemon");
    let at = src.find("pub fn run(").expect("Daemon::run is gone");
    let body = &src[at..];
    let end = body.find("\n    /// One exchange").unwrap_or(body.len());
    let body = &body[..end];

    assert!(
        body.contains("goodbye::asked_to_stop()"),
        "`Daemon::run` no longer asks whether it has been told to stop, so the \
         loop is infinite again and the only way out is being killed"
    );
    assert!(
        body.contains("break"),
        "`Daemon::run` checks whether it was asked to stop and does not leave the \
         loop when it was"
    );
    assert!(
        body.contains("shut_down()"),
        "`Daemon::run` leaves its loop without shutting anything down, so the lock \
         stays held and the helpers outlive Atlas"
    );
}

#[test]
fn something_actually_listens_for_the_signal() {
    // The other half of the same gap. `goodbye::listen` installs the handler;
    // a `goodbye` module nothing calls is the original defect with a new
    // file in it.
    let main = crate::common::source_of("main");
    assert!(
        main.contains("goodbye::listen()"),
        "nothing installs the signal handler, so Ctrl-C is still a kill and \
         everything else in this file is decoration"
    );
    let at = main.find("goodbye::listen()").unwrap();
    let run_at = main.find("fn run_daemon").expect("run_daemon");
    let loop_at = main[run_at..].find("d.run(").expect("d.run") + run_at;
    assert!(
        at > run_at && at < loop_at,
        "the handler is installed outside the daemon's own startup, or after the \
         loop has already begun"
    );
}

#[test]
fn the_signal_handler_only_stores_a_flag() {
    // Signal safety, as a rule the next person cannot accidentally break. A
    // handler interrupts the process at an arbitrary instruction: allocating,
    // locking, printing or writing a file from inside one is how a shutdown
    // handler becomes the crash it was meant to prevent.
    let src = std::fs::read_to_string("src/goodbye.rs").expect("goodbye.rs");
    let mut bodies = Vec::new();
    for (i, _) in src.match_indices("fn handler(") {
        let open = src[i..].find('{').expect("a handler body") + i;
        let mut depth = 0usize;
        let mut end = open;
        for (j, ch) in src[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        end = open + j;
                        break;
                    }
                }
                _ => {}
            }
        }
        bodies.push(src[open..=end].to_string());
    }
    assert!(
        !bodies.is_empty(),
        "no signal handler found in goodbye.rs -- if it moved, point this test at \
         it rather than deleting it"
    );

    const FORBIDDEN: &[&str] = &[
        "println!", "print!", "eprintln!", "format!", "to_string", "String::",
        "Vec::", "vec!", "std::fs", "lock()", "unwrap()", "log",
    ];
    for body in &bodies {
        // Strip comments: the prose explains what must not happen and would
        // otherwise trip its own rule.
        let code: String = body
            .lines()
            .filter(|l| !l.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        for bad in FORBIDDEN {
            assert!(
                !code.contains(bad),
                "a signal handler does {bad} -- that is not signal-safe, and the \
                 whole point of the atomic flag is that the handler does nothing \
                 else:\n{code}"
            );
        }
    }
}

#[test]
fn the_lock_is_released_after_the_state_is_written() {
    // Order, not presence. While the lock is held a second Atlas refuses to
    // start; release it first and a second Atlas can take it, load the old
    // state and write over what this one is still saving. A stale lock costs
    // a wait. Two Atlases on one folder costs the state.
    let src = crate::common::source_of("daemon");
    let at = src.find("pub fn shut_down(").expect("shut_down is gone");
    let body = &src[at..];
    let end = body.find("\n    }").map(|e| e + 6).unwrap_or(body.len());
    let body = &body[..end];

    let persist = body.find("self.persist()").expect("shut_down no longer saves state");
    let helpers = body.find("stop_all()").expect("shut_down no longer stops the helpers");
    let release = body.find("release()").expect("shut_down no longer releases the lock");

    assert!(
        persist < release,
        "the lock is released before the state is written, so a second Atlas can \
         start and overwrite what this one is still saving"
    );
    assert!(
        helpers < release,
        "the lock is released before the helpers are stopped, so a second Atlas \
         can start while the first one's model server is still holding its memory"
    );
}

// ------------------------------------------------ restarting into an update
//
// 28 Sep 2026. An update that went in by itself restarted Atlas with
// `persist(); relaunch_self(); process::exit(0)`. `exit` runs no way out, so
// the lock was still held -- beaten moments before -- when the new copy asked
// for it, and the new copy was refused ("already running") and ended. The old
// one had gone. With Atlas running in the background, an automatic update
// left nothing running until the next sign-in.

#[test]
fn restarting_for_an_update_lets_go_of_the_lock_before_the_new_copy_starts() {
    let (c, p) = (conf(), plat());
    let dir = tmp("update-restart");
    let mut d = daemon(&c, &p, &dir);

    let lock = OnlyOne::at(&d.store.data_dir());
    let now = atlas::store::now();
    lock.take(now).expect("free");
    lock.beat(now); // as the tick always has, just before an update restarts
    let data = d.store.data_dir();

    let mut new_copy_got_in = None;
    let r = d.restart_as(|| {
        // What the new copy does first: ask for the lock, a moment later.
        new_copy_got_in = Some(OnlyOne::at(&data).take(atlas::store::now() + 1));
        Ok(())
    });
    assert!(r.is_ok());
    let got = new_copy_got_in.expect("the new copy was never started");
    assert!(
        got.is_ok(),
        "the new copy was refused the lock, so after the old one exits no Atlas is running: {got:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_restart_whose_new_copy_cannot_start_leaves_this_one_holding_the_lock() {
    // Rather than stopped, lock-less, with nothing replacing it.
    let (c, p) = (conf(), plat());
    let dir = tmp("update-restart-fails");
    let mut d = daemon(&c, &p, &dir);

    let lock = OnlyOne::at(&d.store.data_dir());
    let now = atlas::store::now();
    lock.take(now).expect("free");

    let r = d.restart_as(|| Err("no program file".into()));
    assert!(r.is_err());
    assert!(
        !lock.look(atlas::store::now()).can_take(),
        "the restart failed and this Atlas carried on without its lock, so a second one could start beside it"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_update_restart_goes_through_the_way_out() {
    // The structural half: the restart in the tick is `restart_as`, and
    // nothing in the daemon exits the process without it.
    let src = crate::common::source_of("daemon");
    let exits = src.matches("std::process::exit(").count();
    assert_eq!(exits, 1, "a new way for the daemon to exit the process without stopping properly");
    let at = src.find("std::process::exit(").unwrap();
    let before = &src[at.saturating_sub(600)..at];
    assert!(before.contains("self.restart_as("), "the process exit isn't behind restart_as: {before}");
}

// ------------------------------------------------ a crash with nothing open
//
// 28 Sep 2026. Atlas runs in the background from sign-in. A panic that got
// past every `crash::caught` ended it with nothing said and nothing to start
// it again until the next sign-in. `run_daemon` now stops properly and starts
// itself again -- a few times, not for ever.

#[test]
fn after_a_crash_atlas_starts_itself_again_but_not_in_a_loop() {
    use atlas::crash::{may_start_again, AGAIN_AT_MOST, AGAIN_WINDOW_SECS};
    let dir = tmp("crash-again");
    let state = dir.join("state");
    let t = 1_000_000;
    for i in 0..AGAIN_AT_MOST as u64 {
        assert!(may_start_again(&state, t + i * 10), "restart {} was refused", i + 1);
    }
    assert!(
        !may_start_again(&state, t + 100),
        "a crash on every start would restart for ever"
    );
    // Once the bad patch has passed, one crash earns one restart again.
    assert!(may_start_again(&state, t + AGAIN_WINDOW_SECS + 60));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_restart_that_cannot_be_counted_is_not_made() {
    use atlas::crash::may_start_again;
    let dir = tmp("crash-uncounted");
    // Where the count would go is a file, so it can never be written.
    std::fs::write(dir.join("blocked"), "a file, not a folder").unwrap();
    assert!(!may_start_again(&dir.join("blocked"), 1_000));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_background_loop_is_caught_and_the_way_out_still_runs() {
    let src = crate::common::source_of("main");
    let at = src.find("fn run_daemon(").expect("run_daemon");
    let body = &src[at..];
    let end = body.find("\nfn ").unwrap_or(body.len());
    let body = &body[..end];
    assert!(body.contains("catch_unwind"), "a panic past the loop still ends Atlas silently");
    assert!(body.contains("d.shut_down()"), "a crash skips the way out, so the lock stays held");
    assert!(body.contains("may_start_again"), "nothing starts Atlas again after a crash");
}

// ------------------------------------------------ signing out of Windows
//
// 28 Sep 2026. Windows ends the background Atlas the moment its icon's
// window has answered the end of the session. Nothing listened, so every
// sign-out was a kill: state since the last save lost and the lock left for
// the next sign-in to wait out. The icon's window now asks Atlas to stop and
// holds Windows a few seconds for the way out.

#[test]
fn the_end_of_a_windows_session_asks_atlas_to_stop_and_waits_for_the_lock_to_go() {
    let _one = alone();
    atlas::goodbye::reset_for_test();
    let dir = tmp("endsession");
    let lock = OnlyOne::at(&dir.join("data"));
    lock.take(1_000).unwrap();
    // The daemon's side: on being asked, it finishes and lets go.
    let path = lock.path().to_path_buf();
    let daemon = std::thread::spawn(move || {
        while !atlas::goodbye::asked_to_stop() {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
        let _ = std::fs::remove_file(path);
    });
    let done = atlas::goodbye::stop_and_wait(lock.path(), std::time::Duration::from_secs(4));
    daemon.join().unwrap();
    assert!(done, "it didn't wait for the way out to finish");
    // And it gives up rather than hanging the sign-out when Atlas can't stop.
    lock.take(2_000).unwrap();
    assert!(!atlas::goodbye::stop_and_wait(lock.path(), std::time::Duration::from_millis(200)));
    atlas::goodbye::reset_for_test();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_icon_by_the_clock_answers_the_end_of_the_session() {
    let src = crate::common::source_of("notifyicon");
    let at = src.find("WM_ENDSESSION if").expect("the icon's window doesn't handle the end of the session");
    let arm = &src[at..(at + 600).min(src.len())];
    assert!(arm.contains("stop_and_wait("), "it answers the end of the session without letting Atlas stop");
}
