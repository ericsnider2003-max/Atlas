//! Two folders the backup walked straight past, and a failed backup that
//! evicted the good ones.
//!
//! ## What was skipped
//!
//! `back_up` was one `read_dir` over the state folder with
//! `if !p.is_file() { continue }`. The state folder is mostly flat JSON, so
//! that looked right. Two real directories live in it:
//!
//! * **`handoffs/`** — the files other people have actually sent you.
//!   `household::ReceivedFile::stored_at` points into it from the inbox, so a
//!   backup carried the inbox entries naming those files and none of the
//!   files. Restoring gave you an inbox full of items whose attachments were
//!   gone.
//! * **`landed-over/`** — the previous version of every file Atlas landed on
//!   its own. `selfwork::land`'s doc calls this the thing that makes a
//!   self-landed fix *"something you can put back without asking it"*.
//!
//! Nothing reported either, because `files: Some(n)` counted the flat files,
//! `n > 0`, and `Backup::trustworthy()` therefore said yes.
//!
//! ## The failed backup that looked real
//!
//! `back_up` copies to a temp name and renames into place. A failure part-way
//! left the partial `state-<t>` directory on disk, and `list_backups` counted
//! **every** entry in it at one level — including the temp file of the copy
//! that had just failed. So a backup that died on its first file reported
//! `files: Some(1)`:
//!
//! * `Backup::trustworthy()` → true, so `prune_backups` treated it as a real
//!   backup and deleted a genuine one underneath it to stay under the quota.
//! * `due_for_backup` saw a recent non-empty backup and stopped trying for
//!   another `every_secs`.
//!
//! That is precisely the failure `prune_backups`' own doc describes at
//! length and believes it fixed — arriving through `back_up`'s own temp
//! files rather than through an empty directory.

use atlas::safety::{back_up, list_backups, prune_backups, restore, due_for_backup};
use atlas::safety::{BackupConfig, Trash, TrashConfig};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-bk-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// A state folder shaped like a real one: flat records, plus the two
/// subfolders that actually exist under it.
fn a_state_folder(at: &Path) -> PathBuf {
    let state = at.join("state");
    std::fs::create_dir_all(state.join("handoffs")).unwrap();
    std::fs::create_dir_all(state.join("landed-over")).unwrap();
    std::fs::write(state.join("thread.json"), b"{\"turns\":[]}").unwrap();
    std::fs::write(state.join("memory.json"), b"{\"notes\":[]}").unwrap();
    // What someone sent you, which the inbox record names by this path.
    std::fs::write(state.join("handoffs/1700-contract.pdf"), b"the actual file").unwrap();
    // What Atlas replaced when it landed a fix on its own.
    std::fs::write(state.join("landed-over/panel.rs.before"), b"the version you had").unwrap();
    state
}

fn cfg_at(dir: &Path, keep: usize) -> BackupConfig {
    BackupConfig {
        enabled: true,
        dir: dir.join("backups").display().to_string(),
        keep,
        every_secs: 86_400,
        ..Default::default()
    }
}

fn files_under(p: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![p.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                // Forward slashes on every system, so the expected lists hold on Windows too.
                out.push(path.strip_prefix(p).unwrap_or(&path).display().to_string().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

fn trash_at(dir: &Path) -> Trash {
    Trash::new(TrashConfig {
        dir: dir.join("trash").display().to_string(),
        keep_days: 30,
        ..Default::default()
    })
}

#[test]
fn a_backup_holds_the_files_people_sent_you() {
    let dir = tmp("handoffs");
    let state = a_state_folder(&dir);
    let cfg = cfg_at(&dir, 7);

    let b = back_up(&state, &cfg, 1000).expect("back up");

    let held = files_under(&b.path);
    assert!(
        held.iter().any(|f| f.ends_with("contract.pdf")),
        "the backup does not contain the file someone sent you, only the inbox entry \
         naming it: {held:?}"
    );
    assert!(
        held.iter().any(|f| f.ends_with("panel.rs.before")),
        "the backup does not contain what Atlas replaced when it landed a fix — the \
         thing that makes a self-landed change undoable: {held:?}"
    );
    assert_eq!(b.files, Some(4), "it counted {:?} files in a four-file state folder", b.files);
}

#[test]
fn restoring_puts_the_subfolders_back_too() {
    let dir = tmp("restore-subfolders");
    let state = a_state_folder(&dir);
    let cfg = cfg_at(&dir, 7);
    let b = back_up(&state, &cfg, 1000).expect("back up");

    // Everything goes.
    std::fs::remove_dir_all(&state).unwrap();

    let n = restore(&b.path, &state, &trash_at(&dir), &Default::default()).expect("restore");
    assert_eq!(n, 4, "it restored {n} of four files");
    assert_eq!(
        files_under(&state),
        vec![
            "handoffs/1700-contract.pdf",
            "landed-over/panel.rs.before",
            "memory.json",
            "thread.json",
        ],
        "the restore did not put the folders back"
    );
    assert_eq!(
        std::fs::read_to_string(state.join("handoffs/1700-contract.pdf")).unwrap(),
        "the actual file"
    );
}

#[test]
fn a_backup_that_failed_part_way_leaves_nothing_behind() {
    // A partial `state-<t>` with a recent timestamp is worse than no backup:
    // `due_for_backup` stops trying and `prune_backups` keeps it over a real
    // one.
    //
    // Forced portably: a regular FILE sitting where the backup's `handoffs/`
    // subfolder has to be created. The flat records copy across first, then
    // `create_dir_all` for the subfolder fails on it. No permissions tricks,
    // and it works the same as root.
    let dir = tmp("partial-gone");
    let state = a_state_folder(&dir);
    let cfg = cfg_at(&dir, 7);
    let dest = PathBuf::from(&cfg.dir).join("state-1000");
    std::fs::create_dir_all(&dest).unwrap();
    std::fs::write(dest.join("handoffs"), b"a file where a folder has to go").unwrap();

    let err = back_up(&state, &cfg, 1000).expect_err("it reported a backup it could not finish");
    assert!(!err.to_string().is_empty());
    assert!(
        !dest.exists(),
        "a failed backup left {} on disk, with a recent timestamp — which stops the \
         next attempt and outranks a real backup when pruning",
        dest.display()
    );
    assert!(
        list_backups(&cfg).is_empty(),
        "a backup that failed is being listed as a backup: {:?}",
        list_backups(&cfg).iter().map(|b| (b.at, b.files)).collect::<Vec<_>>()
    );
}

#[test]
fn a_leftover_temp_file_is_not_counted_as_a_backed_up_file() {
    // The other half, for the case where nothing gets to clean up — a power
    // cut, a kill. `list_backups` counted every entry at one level, so a
    // directory holding one abandoned `.part-` file reported
    // `files: Some(1)`, which is `trustworthy()`.
    let dir = tmp("temp-not-counted");
    let cfg = cfg_at(&dir, 7);
    let junk = PathBuf::from(&cfg.dir).join("state-2000");
    std::fs::create_dir_all(&junk).unwrap();
    std::fs::write(junk.join(".part-thread.json"), b"half a file").unwrap();

    let listed = list_backups(&cfg);
    let junk_entry = listed.iter().find(|b| b.at == 2000).expect("the junk directory");
    assert_eq!(
        junk_entry.files,
        Some(0),
        "a directory holding only a half-written temp file was counted as a backup \
         with {:?} files in it",
        junk_entry.files
    );
    assert!(
        !junk_entry.trustworthy(),
        "a backup consisting of one abandoned temp file reports that it is safe to \
         restore from"
    );
}

#[test]
fn a_subfolder_counts_as_what_is_in_it_and_not_as_one_file() {
    // `read_dir(&p).count()` counted `handoffs/` as a single file however
    // much was in it, so a backup's size was whatever its top level happened
    // to look like.
    let dir = tmp("counts-deep");
    let state = a_state_folder(&dir);
    std::fs::write(state.join("handoffs/1701-second.pdf"), b"another").unwrap();
    let cfg = cfg_at(&dir, 7);
    let b = back_up(&state, &cfg, 1000).expect("back up");

    assert_eq!(b.files, Some(5));
    assert_eq!(
        list_backups(&cfg).first().and_then(|x| x.files),
        Some(5),
        "listing a backup counts its subfolders as one entry each"
    );
}

#[test]
fn a_run_of_failed_backups_cannot_evict_the_real_ones() {
    // The consequence, end to end. Seven junk directories with recent
    // timestamps, one real backup underneath, `keep: 7`.
    let dir = tmp("no-eviction");
    let state = a_state_folder(&dir);
    let cfg = cfg_at(&dir, 7);
    let real = back_up(&state, &cfg, 1000).expect("back up");

    for t in 2000..2008u64 {
        let junk = PathBuf::from(&cfg.dir).join(format!("state-{t}"));
        std::fs::create_dir_all(&junk).unwrap();
        std::fs::write(junk.join(".part-thread.json"), b"half a file").unwrap();
    }

    prune_backups(&cfg);

    assert!(
        real.path.exists(),
        "eight failed backups evicted the only real one — restoring from any survivor \
         would copy nothing"
    );
    let left = list_backups(&cfg);
    assert!(
        left.iter().any(|b| b.at == 1000 && b.trustworthy()),
        "the surviving backups hold nothing: {:?}",
        left.iter().map(|b| (b.at, b.files)).collect::<Vec<_>>()
    );
}

#[test]
fn a_backup_that_is_only_temp_files_does_not_stop_the_next_attempt() {
    // `due_for_backup` looks for the newest backup that holds something. A
    // junk directory full of temp files reported holding something, so
    // backups stayed "not due" for as long as they kept failing — the one
    // situation where trying again matters most.
    let dir = tmp("still-due");
    let cfg = cfg_at(&dir, 7);
    let junk = PathBuf::from(&cfg.dir).join("state-5000");
    std::fs::create_dir_all(&junk).unwrap();
    std::fs::write(junk.join(".part-memory.json"), b"half").unwrap();

    assert!(
        due_for_backup(&cfg, 5060),
        "a directory containing one abandoned temp file suppressed the next backup"
    );
}

#[test]
fn a_restore_it_cannot_finish_leaves_your_files_alone() {
    // It used to trash each existing file and copy the new one in the same
    // pass, so a failure part-way left the state folder neither the old one
    // nor the new one — with the replaced half already in the trash and an
    // error carrying a count of nothing.
    //
    // Everything is staged first now, so a backup that cannot be read in full
    // touches nothing.
    let dir = tmp("restore-refuses");
    let state = a_state_folder(&dir);
    let before = files_under(&state);

    // A "backup" whose second file cannot be copied: a directory sitting
    // where `gather` expects to be able to read a file is fine, but an
    // entry that vanishes between the listing and the copy is not. Simulated
    // by making the source unreadable on platforms that allow it, and
    // otherwise by pointing at a backup that is not a directory at all.
    let not_a_backup = dir.join("state-9999");
    std::fs::write(&not_a_backup, b"not a directory").unwrap();

    let err = restore(&not_a_backup, &state, &trash_at(&dir), &Default::default())
        .expect_err("it restored from something that is not a backup");
    assert!(err.to_string().contains("no such backup"), "got: {err}");
    assert_eq!(before, files_under(&state), "a refused restore changed the state folder");
}

#[test]
fn a_restore_does_not_bring_back_a_half_written_file() {
    // `.part-` files are copies that never finished. Restoring one would put
    // a truncated record into the state folder under its real name.
    let dir = tmp("no-part-files");
    let state = a_state_folder(&dir);
    let cfg = cfg_at(&dir, 7);
    let b = back_up(&state, &cfg, 1000).expect("back up");
    std::fs::write(b.path.join(".part-thread.json"), b"HALF").unwrap();

    std::fs::remove_dir_all(&state).unwrap();
    restore(&b.path, &state, &trash_at(&dir), &Default::default()).expect("restore");

    assert_eq!(
        std::fs::read_to_string(state.join("thread.json")).unwrap(),
        "{\"turns\":[]}",
        "a half-written copy was restored over the real record"
    );
    assert!(
        !state.join(".part-thread.json").exists(),
        "the temp file itself was restored into the state folder"
    );
}

#[test]
fn restoring_still_replaces_what_is_there_and_keeps_the_old_copy() {
    // So none of the above is satisfied by refusing to restore. The old file
    // goes to the trash first, which is what makes restoring the wrong backup
    // itself undoable.
    let dir = tmp("replaces");
    let state = a_state_folder(&dir);
    let cfg = cfg_at(&dir, 7);
    let b = back_up(&state, &cfg, 1000).expect("back up");

    std::fs::write(state.join("thread.json"), b"what I have now").unwrap();
    let trash = trash_at(&dir);
    restore(&b.path, &state, &trash, &Default::default()).expect("restore");

    assert_eq!(
        std::fs::read_to_string(state.join("thread.json")).unwrap(),
        "{\"turns\":[]}",
        "the backup's version did not land"
    );
    let held = trash.ledger();
    assert!(
        held.iter().any(|d| d.original.ends_with("thread.json")),
        "the version it replaced was not kept, so restoring the wrong backup is not \
         undoable: {held:?}"
    );
}

#[test]
fn a_backup_does_not_copy_itself_into_itself() {
    // The destination can sit INSIDE the source. `BackupConfig::resolved`
    // derives `data/backups` from `install_root()`, and `Store::new` of a
    // root that does not end in `data/state` returns that root unchanged as
    // the install root — so on a custom `ATLAS_HOME`, a profile directory, or
    // the temporary root a test uses, `<root>/data/backups` is under
    // `<root>`.
    //
    // The old one-level loop never noticed, because it skipped every
    // subdirectory and so never saw `data/`. Recursing turns that layout into
    // a backup copying itself until the path runs out — which is how
    // `tests/off_the_tick.rs` found it, as a scheduled backup reporting
    // `Outcome::Failed`.
    let dir = tmp("inside-itself");
    let state = dir.join("root");
    std::fs::create_dir_all(state.join("handoffs")).unwrap();
    std::fs::write(state.join("thread.json"), b"{}").unwrap();
    std::fs::write(state.join("handoffs/a.pdf"), b"sent to me").unwrap();

    // Backups inside the thing being backed up.
    let cfg = BackupConfig {
        enabled: true,
        dir: state.join("data/backups").display().to_string(),
        keep: 7,
        every_secs: 86_400,
        ..Default::default()
    };

    let b = back_up(&state, &cfg, 1000).expect("a backup whose destination is inside its source");
    let held = files_under(&b.path);
    assert_eq!(
        held,
        vec!["handoffs/a.pdf", "thread.json"],
        "it either failed, or copied its own output back in: {held:?}"
    );

    // And a second backup does not pick up the first.
    let b2 = back_up(&state, &cfg, 2000).expect("a second backup");
    assert_eq!(
        files_under(&b2.path),
        vec!["handoffs/a.pdf", "thread.json"],
        "the second backup swallowed the first one"
    );
}

#[test]
fn a_file_that_vanishes_mid_walk_does_not_fail_the_whole_backup() {
    // The scheduled backup runs on the crew while the tick keeps persisting,
    // so a record appearing or disappearing between the listing and the read
    // is the normal state of affairs. The old loop's `if !p.is_file()
    // { continue }` treated a vanished path as "not a file" and moved on;
    // propagating instead turns that race into a failed backup — and a failed
    // backup is now deleted outright, so the schedule would retry for ever
    // and never keep one.
    //
    // Checked through the public behaviour: a dangling symlink is a path that
    // lists and cannot be read, which is the same shape.
    let dir = tmp("vanishes");
    let state = a_state_folder(&dir);
    #[cfg(unix)]
    std::os::unix::fs::symlink(dir.join("nothing-here"), state.join("gone.json")).unwrap();

    let b = back_up(&state, &cfg_at(&dir, 7), 1000)
        .expect("one unreadable entry failed the whole backup");
    assert_eq!(b.files, Some(4), "the real files did not all make it: {:?}", b.files);
}
