//! `persist()` put back bytes that were already there, thirteen times a turn.
//!
//! `daemon::persist` saves thirteen subsystems after every turn. On a normal
//! turn almost none of them changed — you asked what was outstanding, and the
//! modes, the flows, the watcher, the anticipator and the rest are exactly as
//! they were. Each save was a write **and** a rename: twenty-six filesystem
//! operations, about **a millisecond of disk per turn**, to produce files
//! byte-identical to the ones already on disk.
//!
//! Measured before and after on an otherwise-empty store: **1032µs → 133µs**.
//! What is left is the serialising, which has to happen to know whether
//! anything changed at all.
//!
//! The saving is content-compared against the file rather than against a
//! remembered hash, and that is deliberate: `Store` is `Clone` and holds
//! nothing but a path, so two stores can point at one directory and a hash
//! remembered in one would be wrong about what the other wrote. The file
//! cannot be wrong about it.
//!
//! These tests exist because "write less" is one keystroke away from "lose
//! data". Every one of them is about something that must still happen.

use atlas::store::Store;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
struct Thing {
    n: u64,
    what: String,
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-onlychanged-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn written_at(dir: &PathBuf, name: &str) -> std::time::SystemTime {
    std::fs::metadata(dir.join(format!("{name}.json"))).unwrap().modified().unwrap()
}

#[test]
fn a_changed_value_is_still_written() {
    let dir = tmp("changed");
    let s = Store::new(dir.clone());
    s.save("thing", &Thing { n: 1, what: "one".into() }).unwrap();
    s.save("thing", &Thing { n: 2, what: "two".into() }).unwrap();

    let back: Thing = s.load("thing");
    assert_eq!(back.n, 2, "the second save was skipped as if nothing changed");
    assert_eq!(back.what, "two");
}

#[test]
fn an_unchanged_value_does_not_touch_the_file() {
    // The whole point. Not "writes the same bytes quickly" — does not write.
    let dir = tmp("unchanged");
    let s = Store::new(dir.clone());
    let t = Thing { n: 7, what: "same".into() };
    s.save("thing", &t).unwrap();
    let first = written_at(&dir, "thing");

    std::thread::sleep(std::time::Duration::from_millis(20));
    s.save("thing", &t).unwrap();

    assert_eq!(written_at(&dir, "thing"), first, "an unchanged value was written again");
}

#[test]
fn a_file_deleted_underneath_is_written_again() {
    // The failure mode a remembered hash would have: the store thinks it
    // already wrote this, and the file is gone.
    let dir = tmp("deleted");
    let s = Store::new(dir.clone());
    let t = Thing { n: 3, what: "three".into() };
    s.save("thing", &t).unwrap();
    std::fs::remove_file(dir.join("thing.json")).unwrap();

    s.save("thing", &t).unwrap();
    assert_eq!(s.load::<Thing>("thing"), t, "the file was not restored");
}

#[test]
fn two_stores_on_one_directory_do_not_confuse_each_other() {
    // `Store` is `Clone` and main.rs makes more than one for the same root.
    // A hash remembered in one would be wrong about what the other wrote.
    let dir = tmp("two");
    let a = Store::new(dir.clone());
    let b = Store::new(dir.clone());

    a.save("thing", &Thing { n: 1, what: "from a".into() }).unwrap();
    b.save("thing", &Thing { n: 2, what: "from b".into() }).unwrap();

    assert_eq!(a.load::<Thing>("thing").what, "from b", "one store shadowed the other");
}

#[test]
fn the_first_save_creates_the_directory() {
    // `create_dir_all` moved below the comparison; a store whose root does not
    // exist yet must still work.
    let base = std::env::temp_dir().join("atlas-onlychanged-mkdir");
    let _ = std::fs::remove_dir_all(&base);
    let deep = base.join("not").join("there").join("yet");

    let s = Store::new(deep.clone());
    s.save("thing", &Thing { n: 9, what: "deep".into() }).unwrap();
    assert_eq!(s.load::<Thing>("thing").n, 9);
}

#[test]
fn the_write_that_does_happen_is_still_atomic() {
    // Temp-then-rename is what makes a crash mid-save leave the previous good
    // copy rather than a truncated one. Skipping unchanged writes must not
    // have touched the path that writes.
    // Read to the END OF THE FUNCTION, not a fixed 2000 characters.
    //
    // It was `&src[at..at + 2000]`, and that broke on 17 Sep for a reason
    // that had nothing to do with the code: a comment explaining why the temp
    // file is now named per-process pushed `std::fs::rename` past character
    // 2000, and the guard reported "the rename is gone — saves are no longer
    // atomic" about a rename sitting four lines below the window. A rule
    // whose answer depends on how much prose is above the line it checks
    // teaches its reader to delete the prose.
    let src = std::fs::read_to_string("src/store.rs").expect("store.rs");
    let at = src.find("pub fn save").expect("save is gone");
    let rest = &src[at..];
    // The next function at the same indentation ends this one.
    let end = rest[1..]
        .find("\n    pub fn ")
        .or_else(|| rest[1..].find("\n    fn "))
        .map(|i| i + 1)
        .unwrap_or(rest.len());
    let body = &rest[..end];

    assert!(body.contains("json.tmp"), "the temp file is gone");
    // `rename_patiently` is `fs::rename`, tried again while Windows reports a
    // passing lock (5 Oct 2026, audit Q11).
    assert!(body.contains("rename_patiently(&tmp"), "the rename is gone — saves are no longer atomic");
    assert!(
        body.find("json.tmp").unwrap() < body.find("rename_patiently(&tmp").unwrap(),
        "the temp file is written after the rename"
    );

    // And the temp path must be unique per process. Two Atlases writing one
    // record at once is a state `main.rs` deliberately allows -- the CLI
    // answers while the daemon runs -- so a shared temp name means two
    // `fs::write`s interleaving and a half-of-each file getting renamed into
    // place. See the note in `save` itself.
    assert!(
        body.contains("std::process::id()"),
        "the temp file is not named per process, so a CLI command and the daemon \
         can write the same `.tmp` at the same time and publish a mixed file"
    );
}

#[test]
fn nothing_is_skipped_when_only_part_of_the_value_changed() {
    let dir = tmp("partial");
    let s = Store::new(dir.clone());
    s.save("thing", &Thing { n: 5, what: "same".into() }).unwrap();
    s.save("thing", &Thing { n: 5, what: "different".into() }).unwrap();
    assert_eq!(s.load::<Thing>("thing").what, "different");
}

#[test]
fn a_real_daemon_still_keeps_everything_across_a_restart() {
    // The end-to-end version, because the thirteen subsystems in `persist` are
    // the reason any of this matters. If skipping unchanged writes lost one of
    // them, this is where it shows.
    use atlas::platform::mock::MockPlatform;
    use atlas::platform::Monitor;
    use atlas::proactive::{Proactive, ProactiveConfig};

    let dir = tmp("daemon");
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let plat =
        MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let now = atlas::store::now();
    {
        let mut d = atlas::daemon::Daemon::new(
            &cfg,
            &plat,
            None,
            Store::new(dir.clone()),
            Proactive::new(ProactiveConfig::default()),
        );
        d.backlog.record("renew the certificate", atlas::backlog::Blocker::NeedsApproval, now);
        d.scheduler.at("back up", now + 3600);
        d.persist();
        // Twice, so the second is the skipping path.
        d.persist();
    }
    let d2 = atlas::daemon::Daemon::new(
        &cfg,
        &plat,
        None,
        Store::new(dir),
        Proactive::new(ProactiveConfig::default()),
    );
    assert_eq!(d2.backlog.outstanding().len(), 1, "the backlog was lost");
    assert_eq!(d2.scheduler.jobs.len(), 1, "the scheduler was lost");
}

// 27 Sep 2026: the comparison is skipped -- a stat instead of a read -- when
// this process wrote exactly these bytes and the file's size and modified
// time have not moved since. Everything that can move them must still win.

#[test]
fn a_hand_edit_between_two_identical_saves_is_put_right() {
    let dir = tmp("hand-edit");
    let s = Store::new(dir.clone());
    let t = Thing { n: 4, what: "kept".into() };
    s.save("thing", &t).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("thing.json"), b"{\"schema\":1,\"data\":{\"n\":4,\"what\":\"edit\"}}").unwrap();
    s.save("thing", &t).unwrap();
    assert_eq!(s.load::<Thing>("thing"), t, "the save was skipped as if the file still held it");
}

#[test]
fn a_kept_record_follows_every_write_to_its_file() {
    // `load_kept`: the hub's appearance and roster, read from memory until
    // the file changes -- from this store, another store, or by hand.
    let dir = tmp("load-kept");
    let a = Store::new(dir.clone());
    let b = Store::new(dir.clone());
    assert_eq!(a.load_kept::<Thing>("thing"), Thing::default(), "a missing file is the default");
    a.save("thing", &Thing { n: 1, what: "one".into() }).unwrap();
    assert_eq!(a.load_kept::<Thing>("thing").n, 1);
    std::thread::sleep(std::time::Duration::from_millis(20));
    b.save("thing", &Thing { n: 2, what: "two".into() }).unwrap();
    assert_eq!(a.load_kept::<Thing>("thing").n, 2, "another store's write was not seen");
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::write(dir.join("thing.json"), b"{\"schema\":1,\"data\":{\"n\":3,\"what\":\"hand\"}}").unwrap();
    assert_eq!(a.load_kept::<Thing>("thing").what, "hand", "a hand edit was not seen");
    std::fs::remove_file(dir.join("thing.json")).unwrap();
    assert_eq!(a.load_kept::<Thing>("thing"), Thing::default(), "a deleted file was still answered from memory");
}
