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
//! An invalid state root must refuse startup before reading or changing it.
//! Separately, a directory replacing a record after successful startup
//! exercises once-only reporting and recovery without permission tricks.

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

fn block_record(root: &Path, name: &str) {
    let path = root.join(format!("{name}.json"));
    if path.is_file() { std::fs::remove_file(&path).unwrap(); }
    std::fs::create_dir(&path).unwrap();
}

fn persist_after_transient_contention(d: &mut Daemon) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    loop {
        d.persist();
        if !d.persist_failures.iter().any(|(name, _)| *name == "state snapshot busy") { return; }
        assert!(std::time::Instant::now() < deadline, "state never became writable: {:?}", d.persist_failures);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

#[test]
fn a_store_that_cannot_be_written_is_recorded_rather_than_ignored() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = unwritable_root("recorded");
    let before = std::fs::read(root.parent().unwrap()).unwrap();
    let error = match Daemon::try_new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default())) {
        Ok(_) => panic!("startup accepted a state root that cannot be written"),
        Err(error) => error,
    };
    assert!(!error.to_string().is_empty(), "startup refusal did not explain the failure");
    assert_eq!(std::fs::read(root.parent().unwrap()).unwrap(), before, "startup changed the blocking owner file");
}

#[test]
fn the_person_is_told_once_and_not_every_tick() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = writable_root("told");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));
    block_record(&root, "thread");

    // ONE TICK OF LAG, by design and worth stating. The check sits at the
    // very top of `tick`, above the two early returns, so that a paused or
    // focused Atlas still reports it -- which means it reads the state left
    // by the previous `persist` rather than one that has not run yet. The
    // lag is bounded by the tick interval, i.e. seconds.
    // The blocked thread record is introduced after successful startup;
    // startup refusal must not replace the runtime storage-failure proof.
    let told_about_saving = |said: &[String]| said.iter().any(|s| s.contains("my own state folder"));

    let first: Vec<String> = d.tick(1000);
    assert!(!told_about_saving(&first), "reported before any save had been attempted: {first:?}");
    // A real snapshot worker may briefly hold the barrier. Allow that worker
    // to finish, then require the actual blocked record failure before the
    // next tick's warning; synthetic timestamps do not advance worker time.
    persist_after_transient_contention(&mut d);
    assert!(d.persist_failures.iter().any(|(name, _)| *name == "thread"));

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
    std::fs::remove_dir(root.join("thread.json")).unwrap();
    persist_after_transient_contention(&mut d);
    assert!(d.persist_failures.is_empty(), "repair did not allow the retained state to save: {:?}", d.persist_failures);
    assert!(root.join("thread.json").is_file());
}

#[test]
fn the_lock_it_cannot_refresh_is_also_said_once() {
    // Block only the legacy heartbeat after startup. The native singleton
    // now prevents a second live instance even if this heartbeat cannot save,
    // while the failed heartbeat still needs a once-only truthful report.
    //
    // Counted rather than reported on the first failure: one missed write is
    // a blip and the staleness window is five beats wide. Three in a row is a
    // pattern with two beats of margin left — and it is said **once**, for
    // the same reason the save message is.
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = writable_root("lock-once");
    let mut d =
        Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));
    let heartbeat = atlas::onlyone::OnlyOne::at(&Store::new(&root).data_dir()).path().to_path_buf();
    if heartbeat.is_file() { std::fs::remove_file(&heartbeat).unwrap(); }
    std::fs::create_dir(&heartbeat).unwrap();

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
    let root = writable_root("useful");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));
    block_record(&root, "thread");

    d.tick(1000); // the persist that fails
    persist_after_transient_contention(&mut d);
    assert!(d.persist_failures.iter().any(|(name, _)| *name == "thread"));
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

    persist_after_transient_contention(&mut d);
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

/// 5 Oct 2026: `persist` checked its own sixteen records, but 287 other saves
/// across the daemon are `let _ = store.save(..)` -- connected accounts,
/// calendar links, the vault's install state. One of those failing said
/// nothing. Now every failed save is recorded inside `Store::save`, and
/// `persist` reports the ones that belong to its store.
#[test]
fn a_save_nobody_checked_is_still_reported() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = writable_root("unchecked");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));
    persist_after_transient_contention(&mut d);
    assert!(d.persist_failures.is_empty(), "{:?}", d.persist_failures);

    // A directory sitting where one record's file goes: that record, and only
    // that one, cannot be written.
    std::fs::create_dir_all(root.join("calendar_links_for_test.json")).unwrap();
    let guard = atlas::store::wait_for_state_transaction(&root).unwrap();
    let error = d.store.save("calendar_links_for_test", &vec!["https://example.com/cal.ics".to_string()]).unwrap_err();
    assert!(!matches!(error, atlas::error::AtlasError::Io(ref error) if error.kind() == std::io::ErrorKind::WouldBlock), "the fixture did not reach the real failed record");
    drop(guard);

    d.persist();
    let names: Vec<&str> = d.persist_failures.iter().filter(|(name, _)| *name != "state snapshot busy").map(|(w, _)| *w).collect();
    assert_eq!(names, vec!["calendar_links_for_test"], "an unchecked failed save went unreported: {names:?}");
    let out = d.tick(1000);
    assert!(out.iter().any(|s| s.contains("calendar_links_for_test")), "nobody was told: {out:?}");

    // Reported once: the next persist, with nothing new failing, clears it.
    persist_after_transient_contention(&mut d);
    assert!(d.persist_failures.is_empty(), "{:?}", d.persist_failures);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn snapshot_contention_does_not_hide_an_already_failed_record() {
    let c = Config::load(Path::new("config")).unwrap(); let p = plat();
    let root = writable_root("failed-record-plus-busy");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));
    persist_after_transient_contention(&mut d);
    let guard = atlas::store::wait_for_state_transaction(&root).unwrap();
    block_record(&root, "blocked_owner_record");
    assert!(d.store.save("blocked_owner_record", &"owner update").is_err());
    drop(guard);
    let worker_root = root.clone();
    let (ready_tx, ready_rx) = std::sync::mpsc::channel(); let (release_tx, release_rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || { let _guard = atlas::store::wait_for_state_transaction(&worker_root).unwrap(); ready_tx.send(()).unwrap(); release_rx.recv().unwrap(); });
    ready_rx.recv().unwrap();
    d.persist();
    let names: Vec<_> = d.persist_failures.iter().map(|(name, _)| *name).collect();
    assert!(names.contains(&"blocked_owner_record") && names.contains(&"state snapshot busy"), "snapshot masked the real failed record: {names:?}");
    release_tx.send(()).unwrap(); worker.join().unwrap();
    drop(d); let _ = std::fs::remove_dir_all(root);
}
