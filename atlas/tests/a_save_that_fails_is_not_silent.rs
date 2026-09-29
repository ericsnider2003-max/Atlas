//! When Atlas cannot write to disk, it says so.
//!
//! ## What was happening
//!
//! `Daemon::persist` saved sixteen subsystems after every turn and discarded
//! every result — sixteen `let _ = ...` lines. So on a full disk, a file
//! locked by antivirus or a sync client mid-write, or permissions changed by
//! an update, Atlas answered "noted", "got it", "I'll remember that" and
//! remembered nothing, with no indication anywhere.
//!
//! `persist`'s own neighbouring comments record being bitten by this class
//! twice already — the outbox that "quietly emptied while doctor kept saying
//! nothing was lost", and `selfwork` restarting from the beginning each run.
//! Both were fixed by adding a save. Neither fixed the part where a save that
//! fails says nothing.
//!
//! ## How the failure is produced here
//!
//! By pointing the store at a path that cannot be written: a **file** where
//! the state directory should be. `create_dir_all` fails on it, so every
//! `save` fails, on every platform, with no permissions tricks and no root.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// A store root that cannot be created, because a regular file is sitting
/// exactly where the directory would have to go.
fn unwritable_root(tag: &str) -> PathBuf {
    let base = std::env::temp_dir().join(format!("atlas-nosave-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_file(&base);
    std::fs::write(&base, b"not a directory").expect("make the blocking file");
    base.join("state")
}

fn writable_root(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-cansave-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_store_that_cannot_be_written_is_recorded_rather_than_ignored() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = unwritable_root("recorded");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));

    d.persist();

    assert!(
        !d.persist_failures.is_empty(),
        "every save failed and persist recorded none of them"
    );
    // And it names what could not be saved, so the report can be specific.
    let names: Vec<&str> = d.persist_failures.iter().map(|(w, _)| *w).collect();
    assert!(names.contains(&"thread"), "the thread is not among them: {names:?}");
    assert!(names.len() >= 10, "only {} of sixteen were checked: {names:?}", names.len());
}

#[test]
fn the_person_is_told_once_and_not_every_tick() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = unwritable_root("told");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));

    // ONE TICK OF LAG, by design and worth stating. The check sits at the
    // very top of `tick`, above the two early returns, so that a paused or
    // focused Atlas still reports it -- which means it reads the state left
    // by the previous `persist` rather than one that has not run yet. The
    // lag is bounded by the tick interval, i.e. seconds.
    // `my own state folder`, not the bare word "write". An unwritable root also
    // stops the instance lock being refreshed, and `tick` reports THAT after
    // three missed beats in a sentence containing "write over each other's
    // memory". Matching on "write" made this test pass or fail on which of
    // two different true statements happened to land on which tick.
    // `the_lock_it_cannot_refresh_is_also_said_once` below is that other
    // message, tested on its own terms.
    let told_about_saving = |said: &[String]| said.iter().any(|s| s.contains("my own state folder"));

    let first: Vec<String> = d.tick(1000);
    assert!(!told_about_saving(&first), "reported before any save had been attempted: {first:?}");

    let second: Vec<String> = d.tick(2000);
    assert!(
        told_about_saving(&second),
        "a tick after every save failed said nothing about it: {second:?}"
    );

    // And not again after that, or a full disk becomes a message every few
    // seconds, which is its own kind of broken.
    let third: Vec<String> = d.tick(3000);
    assert!(
        !told_about_saving(&third),
        "it complained again on the very next tick: {third:?}"
    );
}

#[test]
fn the_lock_it_cannot_refresh_is_also_said_once() {
    // The same root cause with a different consequence, and the one that
    // loses memory rather than just failing to add to it: a store that cannot
    // be written cannot refresh `running.lock` either, so the lock ages past
    // `GONE_AFTER_SECS` while Atlas is running perfectly well. The next start
    // reads `Abandoned`, takes the lock, and two instances write the same
    // state folder from their own memory.
    //
    // Counted rather than reported on the first failure: one missed write is
    // a blip and the staleness window is five beats wide. Three in a row is a
    // pattern with two beats of margin left — and it is said **once**, for
    // the same reason the save message is.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = unwritable_root("lock-once");
    let mut d =
        Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));

    let about_the_lock =
        |said: &[String]| said.iter().any(|s| s.contains("instance lock"));

    let mut when: Vec<usize> = Vec::new();
    for (i, t) in [1000u64, 2000, 3000, 4000, 5000, 6000].iter().enumerate() {
        if about_the_lock(&d.tick(*t)) {
            when.push(i + 1);
        }
    }
    assert_eq!(
        when.len(),
        1,
        "the lock warning was said on ticks {when:?} — once is the contract, or a full \
         disk becomes a message every few seconds"
    );
    assert_eq!(
        when[0], 3,
        "it fired on tick {} — one missed beat is a blip, and waiting longer than three \
         spends the margin the staleness window leaves",
        when[0]
    );
}

#[test]
fn what_it_says_is_useful_enough_to_act_on() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = unwritable_root("useful");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));

    d.tick(1000); // the persist that fails
    let said = d.tick(2000).join(" "); // the tick that reports it
    // The consequence, stated plainly -- this is the part that matters.
    assert!(
        said.contains("gone when I restart") || said.contains("won't"),
        "it does not say what the consequence is: {said:?}"
    );
    // And where to look.
    assert!(
        said.contains(&root.display().to_string()),
        "it does not say which folder it could not write to: {said:?}"
    );
}

#[test]
fn a_healthy_store_says_nothing_at_all() {
    // The control. If this ever fails, the complaint is firing on a working
    // machine and everything above proves nothing.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = writable_root("healthy");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));

    d.persist();
    assert!(
        d.persist_failures.is_empty(),
        "a writable store reported failures: {:?}",
        d.persist_failures
    );
    let out = d.tick(1000);
    assert!(
        !out.iter().any(|s| s.contains("can't write")),
        "a working Atlas complained about writing: {out:?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}
