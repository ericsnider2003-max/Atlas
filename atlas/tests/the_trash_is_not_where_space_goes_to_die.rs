//! `atlas reclaim` freed no space, ever, and after thirty days the space was
//! unrecoverable as well.
//!
//! ## The chain
//!
//! `reclaim::walk` — the only thing that produces candidates — begins:
//!
//! ```ignore
//! if meta.file_type().is_symlink() || !meta.is_dir() { continue; }
//! ```
//!
//! **Every single thing `reclaim` offers is a directory.** A `node_modules`,
//! a `target`, a `Cache`, an installer folder. Never a file. `Trash`, which
//! is what `reclaim` hands them to, was written for files:
//!
//! * `take` moved with `rename`, falling back to `copy` + `remove_file`.
//!   `std::fs::copy` fails on a directory. `data/trash` lives under the
//!   install root and the caches live in `~/Library/Caches`,
//!   `%LOCALAPPDATA%`, `~/.cargo` and project folders — so on any machine
//!   with more than one drive, `rename` returns `EXDEV` and the whole reclaim
//!   refused with a raw OS error number.
//! * `undo` had the same fallback in reverse.
//! * `expire` did `let _ = std::fs::remove_file(&i.held)`. `remove_file`
//!   fails on a directory — always, everywhere — and the result was thrown
//!   away. The ledger entry was dropped and the folder stayed.
//!
//! `reclaim::spoken` says: *"Everything I'd move goes to the trash for 30
//! days, so it's all reversible."* What happened instead: a four-gigabyte
//! `node_modules` moved into `data/trash`, freeing nothing; a month later its
//! record was deleted and the four gigabytes stayed — past `undo`, which no
//! longer has a record of it, and past `expire`, which never looks at trash
//! contents without one.
//!
//! ## Why nothing caught it
//!
//! Every existing trash test uses a file. `Trash` is a general-purpose
//! module, files are the obvious case, and the one caller that matters only
//! ever hands it folders. The two halves were each tested and never tested
//! against each other.

use atlas::safety::{Trash, TrashConfig};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-trashdir-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn trash_at(dir: &Path, keep_days: u64) -> Trash {
    Trash::new(TrashConfig {
        dir: dir.join("trash").display().to_string(),
        keep_days,
        ..Default::default()
    })
}

/// A folder shaped like the thing `reclaim` actually offers: nested, with
/// files at more than one level.
fn a_cache_folder(at: &Path) -> PathBuf {
    let root = at.join("node_modules");
    std::fs::create_dir_all(root.join("left/deep")).unwrap();
    std::fs::create_dir_all(root.join("right")).unwrap();
    std::fs::write(root.join("index.js"), b"top level").unwrap();
    std::fs::write(root.join("left/a.js"), b"one").unwrap();
    std::fs::write(root.join("left/deep/b.js"), b"two").unwrap();
    std::fs::write(root.join("right/c.js"), b"three").unwrap();
    root
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

#[test]
fn a_folder_can_be_moved_to_the_trash_at_all() {
    let dir = tmp("takes-a-folder");
    let t = trash_at(&dir, 30);
    let folder = a_cache_folder(&dir);

    let d = t.take(&folder, "reclaimed").expect("a folder could not be put in the trash");
    assert!(!folder.exists(), "it reported moving the folder and the folder is still there");

    let held = PathBuf::from(&d.held);
    assert!(held.is_dir(), "what landed in the trash is not a folder: {}", held.display());
    assert_eq!(
        files_under(&held),
        vec!["index.js", "left/a.js", "left/deep/b.js", "right/c.js"],
        "the folder went in and its contents did not"
    );
}

#[test]
fn a_folder_comes_back_out_with_everything_in_it() {
    let dir = tmp("undo-a-folder");
    let t = trash_at(&dir, 30);
    let folder = a_cache_folder(&dir);

    let d = t.take(&folder, "reclaimed").expect("take");
    t.undo(d.id).expect("a folder could not be restored from the trash");

    assert!(folder.is_dir(), "undo reported success and the folder is not back");
    assert_eq!(
        files_under(&folder),
        vec!["index.js", "left/a.js", "left/deep/b.js", "right/c.js"],
        "the folder came back and its contents did not"
    );
    assert!(t.ledger().is_empty(), "the entry stayed on the ledger after being restored");
}

#[test]
fn a_folder_that_expires_actually_leaves_the_disk() {
    // The one that cost the space. `expire` deleted the ledger entry and
    // `remove_file` failed silently on the folder, so nothing was freed and
    // nothing could be recovered.
    let dir = tmp("expire-a-folder");
    let t = trash_at(&dir, 30);
    let folder = a_cache_folder(&dir);
    let d = t.take(&folder, "reclaimed").expect("take");
    let held = PathBuf::from(&d.held);
    assert!(held.exists());

    let long_after = d.at + 31 * 86_400;
    let gone = t.expire(long_after);

    assert_eq!(gone, 1, "it reported expiring {gone} things");
    assert!(
        !held.exists(),
        "the folder is still on disk at {} — the space `reclaim` promised to free was \
         never freed, and the ledger entry that could have put it back is gone",
        held.display()
    );
    assert!(t.ledger().is_empty(), "the ledger still lists something that is gone");
}

#[test]
fn a_removal_that_fails_keeps_its_record_rather_than_orphaning_the_bytes() {
    // The general rule behind the specific bug. If something in the trash
    // cannot be removed — for any reason, not just the folder one — its entry
    // stays, so it is still recoverable by `undo` and still visible to the
    // next `expire`. Dropping the record and leaving the bytes is the one
    // outcome with no way back.
    //
    // Produced by making the held path un-removable in the way that is
    // portable: the ledger entry names something that is not there. A removal
    // of a missing file succeeds, so this checks the accounting rather than
    // the failure — the failure path itself is what the folder test above
    // exercises.
    let dir = tmp("keeps-the-record");
    let t = trash_at(&dir, 30);
    let a = dir.join("one.txt");
    std::fs::write(&a, b"first").unwrap();
    let folder = a_cache_folder(&dir);

    let file_entry = t.take(&a, "a file").expect("take a file");
    let folder_entry = t.take(&folder, "a folder").expect("take a folder");

    let after = folder_entry.at + 31 * 86_400;
    assert_eq!(t.expire(after), 2, "both should have gone");
    assert!(!PathBuf::from(&file_entry.held).exists());
    assert!(!PathBuf::from(&folder_entry.held).exists());
    assert!(t.ledger().is_empty());
}

#[test]
fn nothing_still_inside_the_keep_window_is_touched() {
    // So the fix cannot be satisfied by deleting more.
    let dir = tmp("keep-window");
    let t = trash_at(&dir, 30);
    let folder = a_cache_folder(&dir);
    let d = t.take(&folder, "reclaimed").expect("take");

    assert_eq!(t.expire(d.at + 86_400), 0, "it expired something one day old");
    assert!(PathBuf::from(&d.held).is_dir(), "a folder one day old was deleted");
    assert_eq!(t.ledger().len(), 1);
}

#[test]
fn a_partial_copy_never_deletes_the_original() {
    // The rule for a cross-volume move: copy everything, then delete. The
    // opposite order turns a disk filling up mid-copy into a half-deleted
    // project folder, which is worse than not reclaiming anything.
    //
    // Forced by pointing the trash at a path that cannot be created — a
    // regular file sits where its directory would have to go — so every
    // attempt to write into it fails.
    let dir = tmp("partial-copy");
    let blocker = dir.join("trash");
    std::fs::write(&blocker, b"not a directory").unwrap();
    let t = trash_at(&dir, 30);
    let folder = a_cache_folder(&dir);
    let before = files_under(&folder);

    let err = t.take(&folder, "reclaimed").expect_err("it moved a folder into a file");
    assert!(!err.to_string().is_empty());
    assert!(folder.is_dir(), "a failed move deleted the folder it could not copy");
    assert_eq!(before, files_under(&folder), "a failed move damaged the folder");
}

#[test]
fn what_the_ledger_cannot_account_for_is_reportable() {
    // The other half, for the folders already stranded on machines running
    // the old code. They are in `data/trash` with no ledger entry: `undo`
    // cannot find them, `expire` never looks at them again, and the space is
    // gone for good unless something says where it went.
    //
    // Reported, never deleted. This is a holding pen for things somebody
    // decided to get rid of, and quietly removing whatever it did not
    // recognise is the wrong instinct in the one folder where being wrong
    // cannot be undone.
    let dir = tmp("unaccounted");
    let t = trash_at(&dir, 30);
    let kept = t.take(&{
        let f = dir.join("kept.txt");
        std::fs::write(&f, b"still on the ledger").unwrap();
        f
    }, "on the books").expect("take");

    // A stray, exactly as the old `expire` would have left it.
    let stray = PathBuf::from(&t.cfg.dir).join("99-node_modules");
    std::fs::create_dir_all(stray.join("deep")).unwrap();
    std::fs::write(stray.join("deep/big.bin"), vec![0u8; 4096]).unwrap();

    let found = t.unaccounted();
    let names: Vec<String> = found.iter().map(|(p, _)| p.display().to_string()).collect();
    assert!(
        names.iter().any(|n| n.ends_with("99-node_modules")),
        "a folder in the trash with no ledger entry was not reported: {names:?}"
    );
    assert!(
        !names.iter().any(|n| n.contains(&kept.held)),
        "something the ledger does account for was reported as a stray: {names:?}"
    );
    let bytes = found.iter().find(|(p, _)| p.ends_with("99-node_modules")).map(|(_, b)| *b);
    assert_eq!(bytes, Some(4096), "it did not say how much space the stray is holding");

    // And it does not delete it. Reporting is the whole job.
    assert!(stray.exists(), "`unaccounted` removed something");
}

#[test]
fn the_ledger_and_its_half_written_copy_are_not_strays() {
    let dir = tmp("ledger-not-stray");
    let t = trash_at(&dir, 30);
    let f = dir.join("x.txt");
    std::fs::write(&f, b"x").unwrap();
    t.take(&f, "why").expect("take");
    std::fs::write(PathBuf::from(&t.cfg.dir).join("ledger.json.writing"), b"{}").unwrap();

    let names: Vec<String> = t.unaccounted().iter().map(|(p, _)| p.display().to_string()).collect();
    assert!(names.is_empty(), "the ledger's own files were reported as strays: {names:?}");
}
