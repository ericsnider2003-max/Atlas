//! Landing a change must not overwrite work Atlas did not plan for.
//!
//! `Edit` promises "the whole new file, so what lands is exactly what was
//! tested". That is right for the sandbox and wrong for landing: a whole-file
//! copy silently wins against anything you changed in the meantime, where a
//! patch would have refused.
//!
//! Overnight work makes the gap hours wide. Approving in the morning would
//! have overwritten an evening's editing. The old version going to trash makes
//! it recoverable but not visible, and you would not know to look.
//!
//! Six of the system prompts in the reference collection state this rule
//! outright: never revert changes you did not make.

use atlas::sandbox::{Change, Fingerprint, Sandbox};
use atlas::safety::{Trash, TrashConfig};
use std::fs;
use std::path::PathBuf;

struct Tmp(PathBuf);
impl Tmp {
    fn new(tag: &str) -> Tmp {
        let d = std::env::temp_dir().join(format!("atlas-lost-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(d.join("trash")).unwrap();
        Tmp(d)
    }
    fn file(&self, name: &str, body: &str) -> PathBuf {
        let p = self.0.join(name);
        fs::write(&p, body).unwrap();
        p
    }
    fn trash(&self) -> Trash {
        Trash::new(TrashConfig {
            dir: self.0.join("trash").to_string_lossy().to_string(),
            ..Default::default()
        })
    }
}
impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn change(source: PathBuf, target: PathBuf) -> Change {
    Change {
        bytes: fs::metadata(&source).map(|m| m.len()).unwrap_or(0),
        new_file: !target.exists(),
        target_was: Fingerprint::of(&target),
        target,
        source,
    }
}

#[test]
fn an_untouched_file_is_replaced_as_before() {
    let t = Tmp::new("clean");
    let target = t.file("thing.rs", "old");
    let source = t.file("thing.new", "new");
    let c = change(source, target.clone());
    assert_eq!(Sandbox::promote(&[c], &t.trash(), true).unwrap(), 1);
    assert_eq!(fs::read_to_string(&target).unwrap(), "new");
}

#[test]
fn a_file_you_edited_meanwhile_is_not_overwritten() {
    let t = Tmp::new("edited");
    let target = t.file("thing.rs", "old");
    let source = t.file("thing.new", "atlas version");
    let c = change(source, target.clone());

    // You edit it while the change is waiting for approval.
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&target, "your evening's work").unwrap();

    let r = Sandbox::promote(&[c], &t.trash(), true);
    assert!(r.is_err(), "it overwrote work it did not plan for");
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "your evening's work",
        "your version is gone"
    );
}

#[test]
fn the_refusal_names_the_file_and_says_nothing_was_touched() {
    let t = Tmp::new("named");
    let target = t.file("prose.rs", "old");
    let source = t.file("prose.new", "x");
    let c = change(source, target.clone());
    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&target, "yours").unwrap();

    let err = Sandbox::promote(&[c], &t.trash(), true).unwrap_err().to_string();
    assert!(err.contains("prose.rs"), "got: {err}");
    assert!(err.contains("haven't touched"), "got: {err}");
}

#[test]
fn one_moved_file_stops_the_whole_batch() {
    // A half-applied change set is harder to reason about than one that did
    // not happen, because you have to work out which half.
    let t = Tmp::new("batch");
    let a = t.file("a.rs", "old-a");
    let b = t.file("b.rs", "old-b");
    let sa = t.file("a.new", "new-a");
    let sb = t.file("b.new", "new-b");
    let ca = change(sa, a.clone());
    let cb = change(sb, b.clone());

    std::thread::sleep(std::time::Duration::from_millis(1100));
    fs::write(&b, "you edited b").unwrap();

    assert!(Sandbox::promote(&[ca, cb], &t.trash(), true).is_err());
    assert_eq!(fs::read_to_string(&a).unwrap(), "old-a", "a was landed anyway");
    assert_eq!(fs::read_to_string(&b).unwrap(), "you edited b");
}

#[test]
fn a_file_that_appeared_since_planning_is_not_written_over() {
    // Planned as a new file, but something created it meanwhile. Writing over
    // it is the same loss.
    let t = Tmp::new("appeared");
    let target = t.0.join("fresh.rs");
    let source = t.file("fresh.new", "atlas version");
    let c = change(source, target.clone());
    assert!(c.new_file);

    fs::write(&target, "someone else got there first").unwrap();
    assert!(Sandbox::promote(&[c], &t.trash(), true).is_err());
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "someone else got there first"
    );
}

#[test]
fn approval_is_still_required_first() {
    // The new check must not become a way past the old one.
    let t = Tmp::new("approval");
    let target = t.file("x.rs", "old");
    let source = t.file("x.new", "new");
    let c = change(source, target.clone());
    assert!(Sandbox::promote(&[c], &t.trash(), false).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "old");
}

#[test]
fn a_fingerprint_notices_length_and_time_separately() {
    let t = Tmp::new("fp");
    let p = t.file("f", "hello");
    let before = Fingerprint::of(&p).unwrap();

    fs::write(&p, "hello there").unwrap();
    assert_ne!(Fingerprint::of(&p).unwrap(), before, "a length change was missed");

    let missing = Fingerprint::of(&t.0.join("not-here"));
    assert!(missing.is_none());
}
