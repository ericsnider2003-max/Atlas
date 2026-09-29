//! A search that found nothing, and a search that could not look.
//!
//! `walk` had four silent exits: an unreadable directory, an unreadable file
//! type, unreadable metadata, and a depth cap. Each one dropped files out of
//! the index with nothing said, so a later search returned nothing and that
//! nothing read as a definite answer.
//!
//! The fourth was worse than the other three. A file whose timestamp could not
//! be read was still indexed, dated to the epoch — and `recall` ranks by
//! recency. An unreadable date did not hide a file, it buried it.

use atlas::index::{Index, IndexConfig, Missed};
use std::fs;
use std::path::PathBuf;

struct Tree(PathBuf);
impl Tree {
    fn new(tag: &str) -> Tree {
        let d = std::env::temp_dir().join(format!("atlas-idx-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("deep/one/two/three/four/five")).unwrap();
        fs::write(d.join("top.txt"), "hello").unwrap();
        fs::write(d.join("deep/one/two/three/four/five/buried.txt"), "hi").unwrap();
        Tree(d)
    }
    fn cfg(&self, max_depth: u32) -> IndexConfig {
        IndexConfig {
            roots: vec![self.0.to_string_lossy().to_string()],
            exclude_dirs: vec![],
            exclude_exts: vec![],
            max_depth,
            max_enrich_mb: 4,
        }
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn a_clean_scan_reports_nothing_missed() {
    // A clean run must add no noise to an ordinary answer.
    let t = Tree::new("clean");
    let idx = Index::scan(&t.cfg(10));
    assert!(!idx.missed.anything(), "{:?}", idx.missed);
    assert!(idx.missed.caveat().is_none());
}

#[test]
fn hitting_the_depth_cap_is_counted_rather_than_silent() {
    let t = Tree::new("deep");
    let idx = Index::scan(&t.cfg(2));
    assert!(idx.missed.too_deep > 0, "the depth cap dropped folders silently");
    assert!(idx.missed.caveat().unwrap().contains("nested too deep"));
}

#[cfg(unix)]
#[test]
fn a_folder_it_cannot_open_is_counted_not_skipped() {
    // The big one. A permission-denied folder used to take its whole tree out
    // of the index with nothing said.
    use std::os::unix::fs::PermissionsExt;
    let t = Tree::new("perm");
    let locked = t.0.join("locked");
    fs::create_dir_all(&locked).unwrap();
    fs::write(locked.join("secret.txt"), "x").unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();

    let idx = Index::scan(&t.cfg(10));
    let missed = idx.missed.unreadable_dirs;
    let reached_it = idx.entries.keys().any(|k| k.ends_with("secret.txt"));
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o755)).unwrap();

    // Root ignores the mode bits, so on a root shell the folder is readable
    // and there is nothing to miss. Asserting a count here would make the test
    // pass or fail on who is running it rather than on what the code does.
    if reached_it {
        assert_eq!(missed, 0, "it read the folder and still called it unreadable");
        return;
    }
    assert!(missed > 0, "an unreadable folder vanished silently");
    assert!(
        idx.missed.caveat().unwrap().contains("couldn't open"),
        "the caveat didn't mention it"
    );
}

#[test]
fn the_caveat_names_the_scale_and_keeps_a_few_examples() {
    // A tally tells you how bad, examples tell you why. Keeping every path
    // would grow with the size of the disk.
    let mut m = Missed::default();
    m.unreadable_dirs = 3;
    m.undated = 12;
    let c = m.caveat().unwrap();
    assert!(c.contains("3 folders"));
    assert!(c.contains("12"));
    assert!(m.examples.len() <= 8);
}

#[test]
fn an_undated_file_is_flagged_because_it_ranks_as_old() {
    // It is not missing from the index. It is in it, dated to the epoch, and
    // recall sorts by recency.
    let mut m = Missed::default();
    m.undated = 1;
    assert!(m.caveat().unwrap().contains("rank as old"));
}

#[test]
fn every_kind_of_miss_counts_as_something_worth_saying() {
    for m in [
        Missed { unreadable_dirs: 1, ..Missed::default() },
        Missed { unreadable_files: 1, ..Missed::default() },
        Missed { too_deep: 1, ..Missed::default() },
        Missed { undated: 1, ..Missed::default() },
    ] {
        assert!(m.anything());
        assert!(m.caveat().is_some());
    }
    assert!(!Missed::default().anything());
}

#[test]
fn the_scan_still_finds_what_it_can_reach() {
    // Reporting the gaps must not stop it indexing.
    let t = Tree::new("finds");
    let idx = Index::scan(&t.cfg(10));
    assert!(idx.entries.keys().any(|k| k.ends_with("top.txt")));
    assert!(idx.entries.keys().any(|k| k.ends_with("buried.txt")));
}
