//! Atlas keeps saying it is alive, in every state it can be in.
//!
//! ## The bug
//!
//! `onlyone.rs` stops a second Atlas starting beside a live one. Its whole
//! mechanism is a timestamp the running process rewrites every tick: fresh
//! means someone is there, older than `GONE_AFTER_SECS` (150s) means whoever
//! held it is gone and the lock can be taken over.
//!
//! The beat was called 440 lines into `Daemon::tick`, **behind two early
//! returns** — the pause check, and `if !self.modes.may_interrupt(false)`.
//! Both are reachable from shipped state: `modes::suggested()` ships a focus
//! mode with `Interruptions::Urgent` and an on-a-call mode with
//! `Interruptions::Silent`, and `may_interrupt(false)` is false for both.
//!
//! So saying "focus mode", or pausing Atlas, stopped the heartbeat while
//! Atlas was still running. Two and a half minutes later the lock read
//! `Abandoned`, and the next launch — the shortcut, `ATLAS.bat`, the logon
//! task — took it and ran a **second daemon beside the live one**. Both then
//! load `data/state`, change it in memory and write the whole thing back, so
//! the second write erases what the first learned.
//!
//! `onlyone.rs`'s own module doc describes that outcome: *"The damage is
//! quiet... You notice weeks later that something you told it didn't stick."*
//! The guard was built correctly and then placed where the two commonest
//! quiet states switched it off.
//!
//! ## Why these tests are behavioural
//!
//! Asserting "the beat is the first statement in `tick`" would pass for a
//! beat that never runs. These drive a real `Daemon` through a real tick in
//! each state and read the lock file off disk afterwards, which is the thing
//! that actually has to be true.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::onlyone::{Found, OnlyOne, GONE_AFTER_SECS};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-beat-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

/// The lock a daemon rooted at `dir` would beat.
///
/// `Store::new(root)` is rooted at the state dir in the tests' shape, and
/// `data_dir()` is what the daemon passes to `OnlyOne::at`, so the lock is
/// asked for the same way the daemon writes it rather than by guessing a path.
fn lock_for(dir: &Path) -> OnlyOne {
    OnlyOne::at(&Store::new(dir).data_dir())
}

/// The lock holds `moment pid` since 29 Sep 2026: read as Atlas reads it.
fn beat_at(dir: &Path) -> Option<u64> {
    std::fs::read_to_string(lock_for(dir).path())
        .ok()
        .and_then(|s| atlas::onlyone::moment_in(&s))
}

#[test]
fn a_plain_tick_beats() {
    // The control. If this ever fails the other three prove nothing.
    let (c, p) = (cfg(), plat());
    let dir = tmp("plain");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));
    d.tick(1_000_000);
    assert_eq!(
        beat_at(&dir),
        Some(1_000_000),
        "an ordinary tick did not write the heartbeat at all"
    );
}

#[test]
fn a_paused_atlas_still_beats() {
    // Pausing means "no jobs, no posts, no offers. Only listening." It does
    // not mean the process has stopped existing, and the lock is about
    // existing.
    let (c, p) = (cfg(), plat());
    let dir = tmp("paused");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    d.turn("pause", 1_000_000);
    d.tick(1_000_500);

    assert_eq!(
        beat_at(&dir),
        Some(1_000_500),
        "a paused Atlas stopped beating, so after {GONE_AFTER_SECS}s a second \
         instance would be let in beside it"
    );
}

#[test]
fn an_atlas_that_may_not_interrupt_still_beats() {
    // The second early return, and the one reachable from a shipped mode
    // rather than from an explicit pause: `Interruptions::Silent` makes
    // `may_interrupt(false)` false, which used to skip the beat.
    let (c, p) = (cfg(), plat());
    let dir = tmp("silent");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    // Entered through `Modes` directly rather than by saying a trigger
    // phrase, and that matters: the first version of this test said
    // "{name} mode" and passed against the broken code, because a fresh
    // store has NO modes in it, `active()` was None, `may_interrupt` returned
    // true and the gate it is about was never reached. A test that passes for
    // the wrong reason is the thing this tree has been bitten by seven times.
    let silent = atlas::modes::suggested()
        .into_iter()
        .find(|m| !m.may_interrupt(false))
        .expect("a shipped mode that suppresses interruptions");
    let name = silent.name.clone();
    d.modes.add(silent);
    d.modes.enter(&name, &[]);

    // Prove the gate is actually shut before asserting anything about it.
    assert!(
        !d.modes.may_interrupt(false),
        "mode {name:?} did not suppress interruptions, so this test is not \
         exercising the early return it is named after"
    );

    d.tick(2_000_500);

    assert_eq!(
        beat_at(&dir),
        Some(2_000_500),
        "a mode that suppresses interruptions also suppressed the heartbeat -- \
         mode {:?} would let a second Atlas in after {GONE_AFTER_SECS}s",
        name
    );
}

#[test]
fn a_beating_atlas_cannot_be_taken_over_however_quiet_it_is() {
    // The end-to-end claim, stated the way the damage happens: the thing a
    // second launch actually asks is `can_take()`, so that is what is
    // asserted rather than the file's contents.
    let (c, p) = (cfg(), plat());
    let dir = tmp("takeover");
    let mut d = Daemon::new(&c, &p, None, Store::new(&dir), Proactive::new(ProactiveConfig::default()));

    d.turn("pause", 3_000_000);

    // Well past the staleness window, beating throughout.
    let mut now = 3_000_000;
    for _ in 0..8 {
        now += GONE_AFTER_SECS / 2;
        d.tick(now);
        let found = lock_for(&dir).look(now);
        assert!(
            !found.can_take(),
            "at t={now} a second Atlas could have taken the lock from a running \
             one: {found:?}"
        );
    }

    // And the sanity half: it DOES go stale once the beating stops, or the
    // test above would pass on a lock that can never be taken over at all.
    let later = now + GONE_AFTER_SECS + 1;
    let found = lock_for(&dir).look(later);
    assert!(
        matches!(found, Found::Abandoned { .. }),
        "the lock never goes stale, so `can_take` being false above proves nothing: {found:?}"
    );
}
