//! One unreadable ledger must not empty the trash.
//!
//! `safety.rs`'s `read_ledger` doc explains the rule and why it exists:
//!
//! > This used to be `Vec<Discarded>`, with an unreadable or corrupt file
//! > collapsing to an empty list. That is the worst place in Atlas for
//! > "nothing found" and "couldn't look" to be the same answer... A module
//! > whose whole purpose is that nothing is gone immediately could lose all
//! > of it, quietly, on one bad read.
//!
//! And `ledger()` — the convenience that *does* collapse them — says: "Kept
//! for reading and reporting only. **Never use it to decide what to write.**"
//!
//! `expire()` used it to decide what to write. On a corrupt ledger it
//! computed `keep = []`, wrote `[]` over the file, and returned 0 — reporting
//! that nothing had expired while erasing the record of everything held. It
//! runs hourly from the daemon's housekeeping.

use atlas::safety::{Trash, TrashConfig};
use std::path::PathBuf;

fn dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-trash-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn trash(d: &PathBuf) -> Trash {
    Trash::new(TrashConfig { dir: d.display().to_string(), keep_days: 30 })
}

/// Put a real file into the trash so the ledger has something in it.
fn discard_one(t: &Trash, d: &PathBuf, name: &str) {
    let f = d.join(name);
    std::fs::write(&f, b"something worth keeping").unwrap();
    t.take(&f, "a test").expect("take it into the trash");
}

#[test]
fn a_corrupt_ledger_does_not_erase_the_record() {
    let d = dir("corrupt");
    let t = trash(&d);
    discard_one(&t, &d, "important.txt");
    let before = t.ledger().len();
    assert_eq!(before, 1, "setup failed: nothing is in the trash");

    // Corrupt it the way a power cut does — valid file, invalid JSON.
    let ledger = d.join("ledger.json");
    std::fs::write(&ledger, b"[{\"id\": 1, \"orig").unwrap();

    let expired = t.expire(atlas::store::now());

    assert_eq!(expired, 0, "it claimed to expire entries it could not read");
    let after = std::fs::read_to_string(&ledger).unwrap();
    assert_ne!(
        after.trim(),
        "[]",
        "the ledger was overwritten with an empty list on a bad read — everything \
         in the trash is now unrecoverable"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// **This one passed before the fix too**, and that is worth writing down
/// rather than letting it look like proof. A directory where the ledger file
/// should be is unreadable *and* unwritable, so the old code's
/// `let _ = self.write_ledger(&keep)` failed silently and the directory
/// survived by accident. The case that actually demonstrates the defect is
/// `a_corrupt_ledger_does_not_erase_the_record` above — a readable file with
/// invalid JSON, which the old code cheerfully replaced with `[]`.
///
/// Kept because it pins the other half of the contract: unreadable must also
/// mean "expire nothing", and the return value must say 0 rather than lying.
#[test]
fn an_unreadable_ledger_does_not_erase_the_record() {
    let d = dir("unreadable");
    let t = trash(&d);
    discard_one(&t, &d, "also-important.txt");

    // A directory where the ledger file should be: readable as an entry,
    // unreadable as a file, on every platform.
    let ledger = d.join("ledger.json");
    std::fs::remove_file(&ledger).unwrap();
    std::fs::create_dir(&ledger).unwrap();

    let expired = t.expire(atlas::store::now());
    assert_eq!(expired, 0, "it expired entries from a ledger it could not read");
    assert!(ledger.is_dir(), "it wrote over the unreadable ledger anyway");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn expiry_still_works_on_a_healthy_ledger() {
    // The control: the guard must not have bought safety by disabling expiry.
    let d = dir("healthy");
    let t = trash(&d);
    discard_one(&t, &d, "old.txt");
    assert_eq!(t.ledger().len(), 1);

    // Well past keep_days.
    let expired = t.expire(atlas::store::now() + 31 * 86_400);
    assert_eq!(expired, 1, "a genuinely old entry was not expired");
    assert_eq!(t.ledger().len(), 0, "the ledger still lists it");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn nothing_recent_is_expired() {
    let d = dir("recent");
    let t = trash(&d);
    discard_one(&t, &d, "new.txt");
    assert_eq!(t.expire(atlas::store::now()), 0, "a fresh entry was expired");
    assert_eq!(t.ledger().len(), 1, "and it is still listed");
    let _ = std::fs::remove_dir_all(&d);
}

// ================= backups =================
//
// `back_up` creates the destination directory and then copies with `?`, so a
// failure part-way leaves an empty or partial `state-<t>` behind and returns
// `Err` without cleaning up. Two things then went wrong, both of which lose
// the thing backups exist to protect:
//
//   * `prune_backups` counted those junk directories toward `keep` and
//     deleted strictly oldest-first, so a run of failures evicted the real
//     backups underneath it;
//   * `due_for_backup` took the newest directory whatever was in it, so one
//     junk directory with a fresh timestamp suppressed further attempts for
//     a whole interval — exactly when trying again matters most.

use atlas::safety::{due_for_backup, list_backups, prune_backups, BackupConfig};

fn backups_dir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-bk-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn bcfg(d: &PathBuf, keep: usize) -> BackupConfig {
    BackupConfig {
        enabled: true,
        dir: d.display().to_string(),
        every_secs: 86_400,
        keep,
        ..BackupConfig::default()
    }
}

/// A backup directory holding `files` files, stamped `at`.
fn make_backup(d: &PathBuf, at: u64, files: usize) {
    let p = d.join(format!("state-{at}"));
    std::fs::create_dir_all(&p).unwrap();
    for i in 0..files {
        std::fs::write(p.join(format!("f{i}.json")), b"{}").unwrap();
    }
}

#[test]
fn pruning_drops_the_empty_ones_before_the_good_ones() {
    let d = backups_dir("prune");
    // Three real backups, then five failures leaving empty directories.
    make_backup(&d, 1_000, 4);
    make_backup(&d, 2_000, 4);
    make_backup(&d, 3_000, 4);
    for (i, at) in [4_000, 5_000, 6_000, 7_000, 8_000].iter().enumerate() {
        let _ = i;
        make_backup(&d, *at, 0);
    }
    assert_eq!(list_backups(&bcfg(&d, 3)).len(), 8, "setup");

    let removed = prune_backups(&bcfg(&d, 3));
    assert_eq!(removed, 5, "wrong number pruned");

    let left = list_backups(&bcfg(&d, 3));
    assert_eq!(left.len(), 3, "wrong number left: {left:?}");
    for b in &left {
        assert!(
            b.trustworthy(),
            "a restorable backup was deleted in favour of an empty one: {} survived",
            b.path.display()
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn an_empty_backup_does_not_make_atlas_think_it_backed_up() {
    let d = backups_dir("due");
    // A real backup a week ago, and a failure five seconds ago.
    make_backup(&d, 1_000_000, 4);
    make_backup(&d, 1_000_000 + 7 * 86_400, 0);

    let now = 1_000_000 + 7 * 86_400 + 5;
    assert!(
        due_for_backup(&bcfg(&d, 7), now),
        "a failed backup with a fresh timestamp suppressed the next attempt"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_recent_real_backup_does_suppress_the_next_one() {
    // The control: the fix must not make Atlas back up constantly.
    let d = backups_dir("notdue");
    make_backup(&d, 1_000_000, 4);
    assert!(
        !due_for_backup(&bcfg(&d, 7), 1_000_005),
        "a backup taken five seconds ago is being treated as due"
    );
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn pruning_still_drops_the_oldest_when_they_are_all_good() {
    let d = backups_dir("allgood");
    for at in [1_000u64, 2_000, 3_000, 4_000, 5_000] {
        make_backup(&d, at, 4);
    }
    assert_eq!(prune_backups(&bcfg(&d, 3)), 2);
    let left = list_backups(&bcfg(&d, 3));
    let ats: Vec<u64> = left.iter().map(|b| b.at).collect();
    assert_eq!(ats, vec![3_000, 4_000, 5_000], "the wrong ones were kept");
    let _ = std::fs::remove_dir_all(&d);
}
