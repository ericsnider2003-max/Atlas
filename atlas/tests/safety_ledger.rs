//! The trash record, when it can't be read.
//!
//! `ledger()` returned `Vec<Discarded>`, and an unreadable or corrupt file
//! collapsed to an empty list. That is the worst place in Atlas for "nothing
//! found" and "couldn't look" to be the same answer, because `take` derives
//! the next id from the highest already in it.
//!
//! The chain: bad read gives an empty list, so the next id is 1, so the file
//! is written over `1-something` already in the trash, and the ledger is then
//! rewritten holding one entry — erasing the record of everything else held
//! there. A module whose whole purpose is that nothing is gone immediately
//! could lose all of it on one bad read.

use atlas::safety::{LedgerState, Trash, TrashConfig};
use std::fs;
use std::path::PathBuf;

#[path = "file_recovery_transactions.rs"]
mod file_recovery_transactions;

struct Area(PathBuf);
impl Area {
    fn new(tag: &str) -> Area {
        let d = std::env::temp_dir().join(format!("atlas-trash-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("trash")).unwrap();
        Area(d)
    }
    fn trash(&self) -> Trash {
        Trash::new(TrashConfig {
            dir: self.0.join("trash").to_string_lossy().to_string(),
            keep_days: 30,
        })
    }
    fn file(&self, name: &str, body: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, body).unwrap();
        p
    }
    fn ledger_file(&self) -> PathBuf {
        self.0.join("trash/ledger.json")
    }
}
impl Drop for Area {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

// --- the four states are distinguishable -----------------------------------

#[test]
fn a_ledger_never_written_is_genuinely_empty() {
    // The one safe case, and the only one that should read as empty.
    let a = Area::new("fresh");
    assert_eq!(a.trash().read_ledger(), LedgerState::Fresh);
    assert!(a.trash().read_ledger().safe_to_write());
}

#[test]
fn a_corrupt_ledger_is_not_an_empty_one() {
    let a = Area::new("corrupt");
    fs::write(a.ledger_file(), "{ this is not json").unwrap();
    let st = a.trash().read_ledger();
    assert!(matches!(st, LedgerState::Corrupt(_)), "{st:?}");
    assert!(!st.safe_to_write());
    assert!(st.trouble().unwrap().contains("damaged"));
}

#[test]
fn a_ledger_holding_entries_reads_back() {
    let a = Area::new("read");
    let t = a.trash();
    t.take(&a.file("one.txt", "x"), "test").unwrap();
    assert!(matches!(t.read_ledger(), LedgerState::Read(v) if v.len() == 1));
}

// --- the destructive chain is broken ---------------------------------------

#[test]
fn it_refuses_to_trash_anything_when_the_record_is_unreadable() {
    // The whole point. Writing now would overwrite something already held and
    // lose the record of the rest.
    let a = Area::new("refuse");
    let t = a.trash();
    t.take(&a.file("first.txt", "important"), "test").unwrap();
    fs::write(a.ledger_file(), "corrupted halfway through").unwrap();

    let second = a.file("second.txt", "new");
    let r = t.take(&second, "test");
    assert!(r.is_err(), "it trashed something over a broken record");
    assert!(second.exists(), "and it moved the file anyway");
}

#[test]
fn the_refusal_says_nothing_was_touched() {
    // A refusal that leaves you unsure whether it half-happened is not much
    // better than the failure.
    let a = Area::new("says");
    let t = a.trash();
    fs::write(a.ledger_file(), "broken").unwrap();
    let err = t.take(&a.file("x.txt", "y"), "test").unwrap_err().to_string();
    assert!(err.contains("Nothing has been touched"), "got: {err}");
    assert!(err.contains("unreadable") || err.contains("record"), "got: {err}");
}

#[test]
fn what_was_already_held_survives_the_refusal() {
    let a = Area::new("survives");
    let t = a.trash();
    t.take(&a.file("kept.txt", "important"), "test").unwrap();
    let held: Vec<_> = fs::read_dir(a.0.join("trash"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    fs::write(a.ledger_file(), "broken").unwrap();
    let _ = t.take(&a.file("new.txt", "x"), "test");

    for name in held.iter().filter(|n| n.ends_with("kept.txt")) {
        assert!(a.0.join("trash").join(name).exists(), "{name} was overwritten");
    }
}

#[test]
fn ids_keep_climbing_rather_than_restarting() {
    let a = Area::new("ids");
    let t = a.trash();
    let one = t.take(&a.file("a.txt", "1"), "test").unwrap();
    let two = t.take(&a.file("b.txt", "2"), "test").unwrap();
    assert!(two.id > one.id, "ids restarted: {} then {}", one.id, two.id);
}

// --- writing is atomic ------------------------------------------------------

#[test]
fn a_ledger_write_leaves_no_half_file_behind() {
    // Written beside and renamed, so a crash mid-write cannot leave a
    // half-written ledger where a whole one used to be.
    let a = Area::new("atomic");
    let t = a.trash();
    t.take(&a.file("a.txt", "1"), "test").unwrap();
    let leftovers: Vec<_> = fs::read_dir(a.0.join("trash"))
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().contains("writing"))
        .collect();
    assert!(leftovers.is_empty(), "a temp ledger was left behind");
}

// --- backups ----------------------------------------------------------------

#[test]
fn an_unreadable_backup_folder_is_not_an_empty_one() {
    // "You have no backups" is the answer most likely to make someone stop
    // worrying at exactly the wrong moment.
    use atlas::safety::{backups, BackupConfig};
    let cfg = BackupConfig {
        enabled: true,
        dir: "/definitely/not/a/real/path".into(),
        every_secs: 3600,
        keep: 5,
    };
    // A missing folder is genuinely no backups.
    assert!(backups(&cfg).is_ok());
}

#[test]
fn the_list_command_surfaces_an_unreadable_folder_instead_of_saying_none() {
    // The exact contrast `atlas backups list` now depends on: `list_backups`
    // cannot tell an unreadable folder from an empty one, so on its own it
    // would print "No backups yet." over a folder it simply could not read.
    // `backups` refuses that, returning `Err` so the command can say so. A
    // path that is a *file* rather than a directory reads as unreadable on
    // every platform and needs no permission trickery to provoke.
    use atlas::safety::{backups, list_backups, BackupConfig};

    let not_a_dir = std::env::temp_dir().join(format!("atlas-backups-not-a-dir-{}", std::process::id()));
    fs::write(&not_a_dir, b"i am a file, not a backups folder").unwrap();

    let cfg = BackupConfig {
        enabled: true,
        dir: not_a_dir.to_string_lossy().into_owned(),
        every_secs: 3600,
        keep: 5,
    };

    // The old path: silence that reads as reassurance.
    assert!(
        list_backups(&cfg).is_empty(),
        "list_backups collapses an unreadable folder to an empty list -- this \
         is why the command could not use it"
    );
    // The wired path: an error the command can show.
    let err = backups(&cfg).expect_err("an unreadable folder must not read as empty");
    assert!(err.contains("couldn't read"), "the error should say it could not read: {err}");

    let _ = fs::remove_file(&not_a_dir);
}

#[test]
fn a_backup_whose_contents_cannot_be_counted_says_so() {
    use atlas::safety::Backup;
    let unknown = Backup { path: "/x".into(), at: 0, files: None, bytes: 0 };
    let known = Backup { path: "/x".into(), at: 0, files: Some(3), bytes: 0 };
    assert!(unknown.line().contains("couldn't read"));
    assert!(!unknown.trustworthy(), "an uncountable backup looked restorable");
    assert!(known.trustworthy());
    assert!(known.line().contains("3 files"));
}

#[test]
fn an_empty_backup_is_not_trustworthy_either() {
    use atlas::safety::Backup;
    let empty = Backup { path: "/x".into(), at: 0, files: Some(0), bytes: 0 };
    assert!(!empty.trustworthy());
}
