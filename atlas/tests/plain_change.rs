//! Explaining a change as behaviour, not code.
//!
//! `plainchange` reads the tests a change adds and drops — sentences about
//! behaviour, in this tree — and says what will be different, with no diff. It
//! can't judge whether the change is right; it names what changes. These drive
//! the diff builder, the behaviour summary, and the daemon path that answers
//! "what will that change do?".

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::plainchange::{ask, diff_of, explain, spoken, written};
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

// A file "before" and "after" a change, as (path, contents). The behaviour is
// carried by the test names, so these are just enough Rust to hold two.
const BEFORE: &str = r#"
#[test]
fn a_running_total_survives_a_restart() { assert!(true); }
"#;

const AFTER: &str = r#"
#[test]
fn a_running_total_survives_a_restart() { assert!(true); }
#[test]
fn a_negative_number_still_adds_correctly() { assert!(true); }
"#;

fn before() -> Vec<(String, String)> {
    vec![("src/total.rs".into(), BEFORE.into())]
}
fn after() -> Vec<(String, String)> {
    vec![("src/total.rs".into(), AFTER.into())]
}

#[test]
fn a_new_test_reads_as_new_behaviour() {
    let d = diff_of(&before(), &after());
    assert_eq!(d.tests_added, vec!["a_negative_number_still_adds_correctly".to_string()]);
    assert!(d.tests_removed.is_empty(), "nothing was removed: {:?}", d.tests_removed);
    assert_eq!(d.files, vec!["src/total.rs".to_string()]);
}

#[test]
fn a_removed_test_is_a_promise_no_longer_kept() {
    // Reverse the change: the after has fewer tests than the before.
    let d = diff_of(&after(), &before());
    assert_eq!(d.tests_removed, vec!["a_negative_number_still_adds_correctly".to_string()]);
    let e = explain(&d, "tidy the totals");
    assert!(
        e.watch_out.iter().any(|w| w.to_lowercase().contains("no longer")),
        "a dropped test must be flagged as a promise withdrawn: {:?}",
        e.watch_out
    );
}

#[test]
fn the_summary_says_what_is_different_without_code() {
    let e = explain(&diff_of(&before(), &after()), "handle negatives");
    let s = spoken(&e);
    // It speaks the behaviour, not the diff — no braces, no fn.
    assert!(s.to_lowercase().contains("negative"), "should name the new behaviour: {s}");
    assert!(!s.contains('{') && !s.contains("fn "), "no code should leak into the summary: {s}");
    // The written form and its question are the fuller pre-decision view.
    let w = written(&e);
    assert!(w.to_lowercase().contains("it will now"), "{w}");
    assert!(!ask(&e).is_empty());
}

#[test]
fn a_change_with_no_test_movement_admits_it_cannot_describe_it() {
    // Same tests before and after: nothing testably different, and it says so
    // rather than inventing a behavioural summary.
    let e = explain(&diff_of(&before(), &before()), "a refactor");
    assert!(!e.certain, "with no test change it must not claim certainty");
    assert!(
        spoken(&e).to_lowercase().contains("can't tell you exactly"),
        "it should admit it can't describe the behaviour: {}",
        spoken(&e)
    );
}

// --- through the daemon ----------------------------------------------------

#[test]
fn asking_what_a_change_does_with_nothing_staged_says_so() {
    let dir = std::env::temp_dir().join(format!("atlas-plainchange-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));

    // A plain string literal so the every-intent-reaches-the-daemon guard can
    // read this branch is driven end to end. Nothing is staged, so it says so
    // rather than describing a change that isn't there.
    let reply = d.turn("what will that change do", 100);
    assert!(reply.to_lowercase().contains("nothing"), "with nothing staged it should say so: {reply}");
    // The nothing-staged answer is its own branch, not the catch-all.
    let junk = d.turn("zzqx frobnicate wibble", 100);
    assert_ne!(reply, junk, "the nothing-staged answer is a branch of its own");
    let _ = std::fs::remove_dir_all(&dir);
}
