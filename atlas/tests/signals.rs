//! The self-audit has data now.
//!
//! `Daemon.signals` was declared, passed to `recommend()`, and never pushed
//! to. Asking Atlas what it should fix returned nothing every time — not
//! because nothing was wrong, but because the vector was empty. A wired
//! consumer with no producer looks identical to a working one from every
//! angle except the answer.

use atlas::selfaudit::{self, Kind};
use atlas::signals;
use atlas::undo::{Did, History, Undo};

fn did(id: u64, what: &str, area: &str, undone: bool, you_asked: bool) -> Did {
    Did {
        id,
        what: what.into(),
        area: area.into(),
        at: 0,
        undo: Undo::Atlas("put it back".into()),
        undone,
        you_asked,
    }
}

fn history(items: Vec<Did>) -> History {
    let mut h = History::default();
    h.done = items;
    h
}

#[test]
fn an_undone_action_becomes_a_signal() {
    let h = history(vec![
        did(1, "moved Chrome left", "windows", true, false),
        did(2, "moved Chrome left", "windows", false, false),
    ]);
    let s = signals::from_undo(&h);
    assert_eq!(s.len(), 1);
    assert_eq!(s[0].kind, Kind::YouKeepCorrecting);
    assert_eq!(s[0].subject, "windows");
    assert_eq!(s[0].seen, 1);
    assert_eq!(s[0].of, 2);
}

#[test]
fn undoing_something_you_asked_for_is_not_a_signal() {
    // That is you changing your mind. It says nothing about Atlas, and
    // counting it would teach the audit to flag your own indecision.
    let h = history(vec![did(1, "closed Discord", "apps", true, true)]);
    assert!(signals::from_undo(&h).is_empty());
}

#[test]
fn nothing_undone_produces_nothing() {
    let h = history(vec![did(1, "opened Chrome", "apps", false, false)]);
    assert!(signals::from_undo(&h).is_empty());
}

#[test]
fn areas_are_counted_separately() {
    let h = history(vec![
        did(1, "a", "windows", true, false),
        did(2, "b", "windows", true, false),
        did(3, "c", "mail", true, false),
        did(4, "d", "mail", false, false),
    ]);
    let mut s = signals::from_undo(&h);
    s.sort_by(|a, b| a.subject.cmp(&b.subject));
    assert_eq!(s.len(), 2);
    assert_eq!((s[0].subject.as_str(), s[0].seen, s[0].of), ("mail", 1, 2));
    assert_eq!((s[1].subject.as_str(), s[1].seen, s[1].of), ("windows", 2, 2));
}

#[test]
fn the_example_is_the_thing_you_took_back() {
    let h = history(vec![
        did(1, "opened YouTube unasked", "apps", true, false),
    ]);
    assert_eq!(signals::from_undo(&h)[0].example, "opened YouTube unasked");
}

#[test]
fn misunderstandings_become_a_rate_not_a_count() {
    let s = signals::from_misunderstandings(7, 100, "put the thing on the thing").unwrap();
    assert_eq!(s.kind, Kind::NotUnderstood);
    assert_eq!((s.seen, s.of), (7, 100));
    assert!(signals::from_misunderstandings(0, 100, "").is_none());
    assert!(signals::from_misunderstandings(5, 0, "").is_none());
}

#[test]
fn never_used_capabilities_are_reported_together() {
    let unused = vec!["ledger".to_string(), "stance".to_string()];
    let s = signals::from_unused(&unused, 46).unwrap();
    assert_eq!(s.kind, Kind::NeverUsed);
    assert_eq!((s.seen, s.of), (2, 46));
    assert!(s.example.contains("ledger"));
    assert!(signals::from_unused(&[], 46).is_none());
}

#[test]
fn gather_produces_something_the_audit_can_act_on() {
    // The end-to-end point: real records in, a recommendation out.
    let h = history(vec![
        did(1, "moved a window", "windows", true, false),
        did(2, "moved a window", "windows", true, false),
        did(3, "moved a window", "windows", true, false),
        did(4, "moved a window", "windows", false, false),
    ]);
    let sigs = signals::gather(&h, 12, 100, "do the thing", &["ledger".into()], 46);
    assert!(!sigs.is_empty(), "nothing was produced");

    let recs = selfaudit::recommend(&sigs, 3);
    assert!(
        !recs.is_empty(),
        "signals were produced but the audit still recommends nothing"
    );
}

#[test]
fn an_empty_history_still_produces_nothing_rather_than_noise() {
    // The fix must not make Atlas complain about a machine that is fine.
    let sigs = signals::gather(&History::default(), 0, 0, "", &[], 46);
    assert!(sigs.is_empty());
}

#[test]
fn the_regret_window_is_not_configurable() {
    // A threshold you can widen is one that gets widened until nothing
    // crosses it.
    assert_eq!(signals::REGRET_WINDOW_SECS, 120);
    let src = std::fs::read_to_string("src/signals.rs").unwrap();
    assert!(
        !src.contains("regret_window") || !src.contains("SignalsConfig"),
        "the regret window has become tuneable"
    );
}

#[test]
fn got_slower_still_has_no_producer_and_that_is_known() {
    // Named rather than quietly absent: nothing in Atlas times a turn, so this
    // signal cannot fire. Kept in the taxonomy because it is the right
    // taxonomy. Delete this test when turn timing lands.
    let h = history(vec![did(1, "x", "y", true, false)]);
    let sigs = signals::gather(&h, 1, 10, "x", &[], 46);
    assert!(
        !sigs.iter().any(|s| s.kind == Kind::GotSlower),
        "GotSlower has a producer now — good, delete this test"
    );
}
