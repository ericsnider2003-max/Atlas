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
    // Somewhere the new one can't be written.
    std::fs::create_dir_all(d.join("laptop.bundle.writing")).unwrap();
    assert!(write_whole(&at, b"the new one").is_err(), "a failed write was reported as written");
    assert_eq!(std::fs::read(&at).unwrap(), b"the whole of the last one", "the other device now reads a broken file");
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn what_is_being_written_is_under_a_name_no_reader_takes() {
    // Every reader picks files by extension: .bundle, .keyhandoff, .invite.
    let d = tmp("names");
    let at = d.join("laptop.bundle");
    std::fs::create_dir_all(d.join("laptop.bundle.writing")).unwrap();
    let _ = write_whole(&at, b"x");
    let writing = d.join("laptop.bundle.writing");
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
