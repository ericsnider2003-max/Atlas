//! Files another device reads are never seen half-written (28 Sep 2026).
//!
//! The sync bundle, the key handoff and the household invitation were each
//! `fs::write` straight onto the name the other side looks for, in a folder a
//! cloud client watches. `fs::write` truncates first and fills in after, so
//! while it runs the name holds half a file: the other Atlas's sync pass read
//! it, couldn't open it, and reported the bundle unreadable; a cloud client
//! could carry the half across. `sync::write_whole` writes beside the name and
//! renames into place.

use atlas::sync::write_whole;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-whole-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_file_is_written_and_replaced_whole_with_nothing_left_beside_it() {
    let d = tmp("replace");
    let at = d.join("laptop.bundle");
    write_whole(&at, b"first").unwrap();
    write_whole(&at, b"second, longer").unwrap();
    assert_eq!(std::fs::read(&at).unwrap(), b"second, longer");
    let names: Vec<String> =
        std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
    assert_eq!(names, vec!["laptop.bundle".to_string()], "something was left in the shared folder");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_write_that_fails_leaves_the_last_whole_file_where_it_was() {
    let d = tmp("fails");
    let at = d.join("laptop.bundle");
    write_whole(&at, b"the whole of the last one").unwrap();
    // Somewhere the new one can't be written: a folder where this process's
    // own temporary name would go (`store::write_whole`).
    std::fs::create_dir_all(d.join(format!("laptop.bundle.{}.writing", std::process::id()))).unwrap();
    assert!(write_whole(&at, b"the new one").is_err(), "a failed write was reported as written");
    assert_eq!(std::fs::read(&at).unwrap(), b"the whole of the last one", "the other device now reads a broken file");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn what_is_being_written_is_under_a_name_no_reader_takes() {
    // Every reader picks files by extension: .bundle, .keyhandoff, .invite.
    let d = tmp("names");
    let at = d.join("laptop.bundle");
    let writing = d.join(format!("laptop.bundle.{}.writing", std::process::id()));
    std::fs::create_dir_all(&writing).unwrap();
    let _ = write_whole(&at, b"x");
    let ext = writing.extension().and_then(|x| x.to_str()).unwrap_or("");
    assert!(!["bundle", "keyhandoff", "invite"].contains(&ext));
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn the_files_other_devices_read_are_all_written_whole() {
    let daemon = crate::common::source_of("daemon");
    assert!(
        !daemon.contains("std::fs::write(&out,"),
        "the sync bundle is written straight onto its name again"
    );
    assert!(daemon.matches("crate::sync::write_whole(&out,").count() >= 2, "both bundle writes (sealed and plain) go through write_whole");
    let sync = crate::common::source_of("sync");
    let household = crate::common::source_of("household");
    for (name, src, f) in [("sync", &sync, "pub fn leave_handoff("), ("household", &household, "pub fn leave_invitation(")] {
        let at = src.find(f).unwrap_or_else(|| panic!("{f} in {name}"));
        let body = &src[at..];
        let body = &body[..body.find("\n}\n").unwrap_or(body.len())];
        assert!(body.contains("write_whole("), "{name}: {f} writes straight onto the name the other device reads");
        assert!(!body.contains("std::fs::write("), "{name}: {f} still uses fs::write");
    }
}

// ------------- a rename that meets a passing lock (5 Oct 2026 audit, Q11)

fn locked() -> std::io::Error {
    std::io::Error::from(std::io::ErrorKind::PermissionDenied)
}

#[test]
fn a_rename_that_meets_a_passing_lock_is_tried_again_until_it_goes() {
    let mut tries = 0;
    let r = atlas::store::rename_patiently_with(
        std::path::Path::new("a"),
        std::path::Path::new("b"),
        &[0, 0, 0, 0],
        |e| e.kind() == std::io::ErrorKind::PermissionDenied,
        |_, _| {
            tries += 1;
            if tries < 3 { Err(locked()) } else { Ok(()) }
        },
    );
    assert!(r.is_ok(), "a scanner holding the file for a moment lost the save");
    assert_eq!(tries, 3);
}

#[test]
fn a_lock_that_outlasts_the_waits_is_reported_not_hidden() {
    let mut tries = 0;
    let r = atlas::store::rename_patiently_with(
        std::path::Path::new("a"),
        std::path::Path::new("b"),
        &[0, 0],
        |e| e.kind() == std::io::ErrorKind::PermissionDenied,
        |_, _| {
            tries += 1;
            Err(locked())
        },
    );
    assert_eq!(r.unwrap_err().kind(), std::io::ErrorKind::PermissionDenied);
    assert_eq!(tries, 3, "the first try and one per wait");
}

#[test]
fn any_other_failure_is_not_waited_on() {
    let mut tries = 0;
    let r = atlas::store::rename_patiently_with(
        std::path::Path::new("a"),
        std::path::Path::new("b"),
        &[0, 0, 0],
        |e| e.kind() == std::io::ErrorKind::PermissionDenied,
        |_, _| {
            tries += 1;
            Err(std::io::Error::from(std::io::ErrorKind::NotFound))
        },
    );
    assert!(r.is_err());
    assert_eq!(tries, 1);
}

#[test]
fn every_save_that_renames_into_place_is_patient() {
    // The store and the small state files kept beside it: the places a lost
    // rename meant a lost save.
    for name in ["store", "preferences", "trace", "speaking", "whystopped", "phases", "safety", "server"] {
        let src = crate::common::source_of(name);
        assert!(src.contains("rename_patiently("), "{name}: a save renames into place without waiting out a passing lock");
    }
    for name in ["apns", "webpush", "applewx", "peerkey", "sync", "yourchanges"] {
        let src = crate::common::source_of(name);
        // `store::write_json` is `write_whole` for a JSON value (audit Q3:
        // apns and webpush had the same five lines); checked below.
        assert!(
            src.contains("store::write_whole(") || src.contains("store::write_json("),
            "{name}: a state file is written straight onto its name"
        );
    }
    let store = crate::common::source_of("store");
    let json = &store[store.find("pub fn write_json").expect("store::write_json")..];
    let body = &json[..json.find("\n}").unwrap_or(json.len())];
    assert!(body.contains("write_whole("), "store::write_json must write whole");
}
