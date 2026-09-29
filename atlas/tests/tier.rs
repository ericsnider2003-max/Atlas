//! How much work an answer needs.

use atlas::freshness::Shelf;
use atlas::tier::*;

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

fn named() -> Vec<String> {
    vec!["morning report".into(), "back up".into(), "report".into()]
}

fn reports(made_at: u64) -> Vec<Report> {
    vec![
        Report {
            name: "morning intel".into(),
            covers: vec!["news".into(), "trending".into(), "headlines".into()],
            made_at,
            shelf: Shelf::Quick,
        },
        Report {
            name: "share prices".into(),
            covers: vec!["price".into(), "market".into()],
            made_at,
            shelf: Shelf::Volatile,
        },
    ]
}

// --- naming something ------------------------------------------------------

#[test]
fn saying_the_name_of_a_thing_just_runs_it() {
    let t = tier_for("run the morning report", &named(), &[], 0);
    assert_eq!(t, Tier::RunNamed("morning report".into()));
    assert!(!t.slow());
}

#[test]
fn the_longest_name_wins() {
    // "report" is also a capability. "morning report" is the one you said.
    assert_eq!(
        tier_for("run the morning report", &named(), &[], 0),
        Tier::RunNamed("morning report".into())
    );
}

// --- reading what is already there ------------------------------------------

#[test]
fn a_question_covered_by_a_fresh_report_is_answered_from_it() {
    // The whole point. Waking a model to re-derive something computed an hour
    // ago is the commonest way a voice assistant wastes your time.
    match tier_for("what was the big news", &[], &reports(0), HOUR) {
        Tier::FromReport { report, as_of } => {
            assert_eq!(report, "morning intel");
            assert!(!as_of.is_empty(), "the answer must be able to carry its age");
        }
        other => panic!("it went off to think: {other:?}"),
    }
}

#[test]
fn the_answer_carries_how_old_it_is() {
    match tier_for("any news", &[], &reports(0), 5 * HOUR) {
        Tier::FromReport { as_of, .. } => assert!(as_of.contains("hours")),
        other => panic!("{other:?}"),
    }
}

// --- falling through, and only upward ---------------------------------------

#[test]
fn a_stale_report_is_not_used_and_says_why() {
    // "I have this but it's old" is a different fact from "I don't have this",
    // and the difference is worth saying.
    match tier_for("what was the big news", &[], &reports(0), 90 * DAY) {
        Tier::Think { why } => {
            assert!(why.contains("morning intel"), "got: {why}");
            assert!(why.contains("look again"), "got: {why}");
        }
        other => panic!("it answered from a stale report: {other:?}"),
    }
}

#[test]
fn asking_about_this_moment_never_reads_from_a_report() {
    // A report made this morning answers "what's on today" and cannot answer
    // "is it raining right now". The difference is in the question.
    for q in [
        "what's the price right now",
        "is it still running",
        "what's the latest news",
        "what's happening at the moment",
    ] {
        match tier_for(q, &[], &reports(0), HOUR) {
            Tier::Think { .. } => {}
            other => panic!("{q:?} was answered from storage: {other:?}"),
        }
    }
}

#[test]
fn a_volatile_report_goes_stale_within_the_day() {
    // Prices age in hours. The shelf does the work, not a separate rule.
    match tier_for("what about the market", &[], &reports(0), 2 * DAY) {
        Tier::Think { .. } => {}
        other => panic!("it quoted a day-old price: {other:?}"),
    }
}

#[test]
fn nothing_covering_it_says_so_plainly() {
    match tier_for("how do I fix the boiler", &[], &reports(0), HOUR) {
        Tier::Think { why } => assert!(why.contains("nothing I've already worked out")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn thinking_is_the_slow_one_and_says_so() {
    let t = tier_for("something unfamiliar", &[], &[], 0);
    assert!(t.slow(), "the caller can't know to say 'working on it'");
    assert!(!Tier::RunNamed("x".into()).slow());
    assert!(!Tier::FromReport { report: "r".into(), as_of: "an hour ago".into() }.slow());
}

#[test]
fn every_fall_through_records_why() {
    // A tier-three answer that should have been tier two is the thing worth
    // noticing, and it is invisible unless the reason is kept.
    for q in ["the price right now", "something nobody covered", "the news"] {
        if let Tier::Think { why } = tier_for(q, &[], &reports(0), 900 * DAY) {
            assert!(why.len() > 15, "{q:?} fell through with no reason: {why}");
        }
    }
}

// --- is the middle tier earning its place? ----------------------------------

#[test]
fn an_ordinary_mix_says_nothing() {
    let mut m = Mix::default();
    for _ in 0..8 {
        m.note(&Tier::FromReport { report: "r".into(), as_of: "an hour ago".into() });
    }
    for _ in 0..4 {
        m.note(&Tier::Think { why: "x".into() });
    }
    assert!(m.worth_saying().is_none());
}

#[test]
fn never_using_a_report_is_worth_mentioning() {
    // Either the reports aren't covering what you ask, or they go stale first.
    let mut m = Mix::default();
    for _ in 0..12 {
        m.note(&Tier::Think { why: "x".into() });
    }
    assert!(m.worth_saying().unwrap().contains("stale"));
}

#[test]
fn a_quiet_day_is_not_evidence_of_anything() {
    let mut m = Mix::default();
    m.note(&Tier::Think { why: "x".into() });
    assert!(m.worth_saying().is_none(), "three questions is not a pattern");
    assert_eq!(m.total(), 1);
}
