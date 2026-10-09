//! Deletion cannot leave the tree it was pointed at.
//!
//! `apply` is the one irreversible operation in `retention`, and it used to
//! delete whatever path a `Plan::Delete` carried. That was safe only because
//! the single caller happened to build its plans from a survey of `data`.
//!
//! `Plan` is public, derives Serialize/Deserialize, and reaches disk. "The
//! only caller is careful" is a property of today, not of the code, and the
//! point of no return is exactly where the check belongs.

use atlas::retention::{ out_of_bounds, Plan};
use std::fs;
use std::path::{Path, PathBuf};

/// A throwaway tree: `<tmp>/root/keep.txt` plus `<tmp>/outside.txt`.
struct Tree {
    base: PathBuf,
}

impl Tree {
    fn new(tag: &str) -> Tree {
        let base = std::env::temp_dir().join(format!("atlas-retention-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("root/nested")).unwrap();
        fs::write(base.join("root/keep.txt"), "aaaa").unwrap();
        fs::write(base.join("root/nested/deep.txt"), "bbbb").unwrap();
        fs::write(base.join("outside.txt"), "cccc").unwrap();
        Tree { base }
    }
    fn root(&self) -> PathBuf {
        self.base.join("root")
    }
    fn del(&self, rel: &str) -> Plan {
        Plan::Delete {
            path: self.base.join(rel),
            why: "test".into(),
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

#[test]
fn a_file_inside_the_tree_is_deleted() {
    let t = Tree::new("inside");
    let freed = atlas::retention::apply_until(&[t.del("root/keep.txt")], &t.root(), &|| false).unwrap();
    assert_eq!(freed, 4);
    assert!(!t.base.join("root/keep.txt").exists());
}

#[test]
fn nested_files_are_still_reachable() {
    let t = Tree::new("nested");
    assert_eq!(atlas::retention::apply_until(&[t.del("root/nested/deep.txt")], &t.root(), &|| false).unwrap(), 4);
}

#[test]
fn a_sibling_of_the_tree_is_not_touched() {
    let t = Tree::new("sibling");
    let freed = atlas::retention::apply_until(&[t.del("outside.txt")], &t.root(), &|| false).unwrap();
    assert_eq!(freed, 0, "it deleted a file outside the tree");
    assert!(t.base.join("outside.txt").exists());
}

#[test]
fn dot_dot_does_not_climb_out() {
    // A string prefix check would accept this: the path literally starts with
    // the root. Canonicalising first is what makes the check mean anything.
    let t = Tree::new("dotdot");
    let plan = Plan::Delete {
        path: t.root().join("../outside.txt"),
        why: "test".into(),
    };
    assert_eq!(atlas::retention::apply_until(&[plan], &t.root(), &|| false).unwrap(), 0, "`..` climbed out of the tree");
    assert!(t.base.join("outside.txt").exists());
}

#[test]
fn one_stray_path_does_not_stop_the_rest() {
    // Refusing the whole batch would mean a single bad plan blocks
    // housekeeping forever, and the disk fills up instead.
    let t = Tree::new("mixed");
    let freed = atlas::retention::apply_until(
        &[t.del("outside.txt"), t.del("root/keep.txt")],
        &t.root(), &|| false).unwrap();
    assert_eq!(freed, 4);
    assert!(t.base.join("outside.txt").exists());
    assert!(!t.base.join("root/keep.txt").exists());
}

#[test]
fn a_directory_is_refused_rather_than_silently_failing() {
    let t = Tree::new("dir");
    assert_eq!(atlas::retention::apply_until(&[t.del("root/nested")], &t.root(), &|| false).unwrap(), 0);
    assert!(t.base.join("root/nested").exists());
}

#[test]
fn a_path_that_no_longer_exists_frees_nothing() {
    let t = Tree::new("gone");
    assert_eq!(atlas::retention::apply_until(&[t.del("root/never-existed.txt")], &t.root(), &|| false).unwrap(), 0);
}

#[test]
fn keep_is_not_a_deletion() {
    let t = Tree::new("keep");
    assert_eq!(atlas::retention::apply_until(&[Plan::Keep], &t.root(), &|| false).unwrap(), 0);
    assert!(t.base.join("root/keep.txt").exists());
}

#[test]
fn strays_are_reported_rather_than_swallowed() {
    // A plan naming a path outside the tree is a bug upstream. Skipping it
    // silently means never finding out.
    let t = Tree::new("report");
    let plans = [t.del("outside.txt"), t.del("root/keep.txt")];
    let strays = out_of_bounds(&plans, &t.root());
    assert_eq!(strays.len(), 1);
    assert!(strays[0].ends_with("outside.txt"));
}

#[test]
fn nothing_is_out_of_bounds_when_everything_is_inside() {
    let t = Tree::new("clean");
    assert!(out_of_bounds(&[t.del("root/keep.txt")], &t.root()).is_empty());
}

#[test]
fn an_unreadable_root_deletes_nothing() {
    // If the root cannot be canonicalised there is nothing to be inside of,
    // and the safe answer is to do nothing rather than to fall back to
    // comparing strings.
    let t = Tree::new("noroot");
    assert_eq!(
        atlas::retention::apply_until(&[t.del("root/keep.txt")], Path::new("/definitely/not/here"), &|| false).unwrap(),
        0
    );
    assert!(t.base.join("root/keep.txt").exists());
}

// ===========================================================================
// An unreadable date must not mark a file for deletion
// ===========================================================================

use atlas::retention::{plan, Class, Item, RetentionConfig, Usage};

#[test]
fn a_file_whose_age_is_unknown_is_never_proposed_for_deletion() {
    // `unwrap_or(0)` on the timestamp dated undatable files to 1970, which is
    // exactly what marks something as old enough to remove. A file Atlas could
    // not read the age of became the first thing it proposed deleting.
    let items = vec![Item {
        class: Class::Unknown,
        path: PathBuf::from("/tmp/undatable"),
        bytes: 100,
        modified: 0,
    }];
    let plans = plan(&items, &RetentionConfig::default(), 9_999_999_999);
    assert!(
        !plans.iter().any(|p| matches!(p, Plan::Delete { .. })),
        "it proposed deleting a file it couldn't date"
    );
}

#[test]
fn an_old_scratch_file_is_still_proposed() {
    // The fix must not stop housekeeping working.
    let items = vec![Item {
        class: Class::Scratch,
        path: PathBuf::from("/tmp/old.wav"),
        bytes: 100,
        modified: 0,
    }];
    let plans = plan(&items, &RetentionConfig::default(), 9_999_999_999);
    assert!(plans.iter().any(|p| matches!(p, Plan::Delete { .. })));
}

#[test]
fn undatable_bytes_are_counted_but_not_attributed() {
    let mut u = Usage::default();
    u.add(Class::Unknown, 500);
    u.add(Class::Scratch, 100);
    assert_eq!(u.unknown, 500);
    assert_eq!(u.total(), 600, "undatable bytes vanished from the total");
    assert_eq!(u.scratch, 100);
}
