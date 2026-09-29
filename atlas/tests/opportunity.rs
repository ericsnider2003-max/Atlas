//! Weighing an opportunity on five axes.

use atlas::opportunity::*;

fn scored(n: u8, on: &str) -> Finding {
    Finding::Scored { out_of_ten: n, rests_on: vec![on.into()] }
}

fn full() -> Opportunity {
    let mut o = Opportunity::new("a weekend tool for traders", "a problem I hit myself");
    o.note(Axis::Work, scored(7, "about 40 hours over three weekends"));
    o.note(Axis::Authenticity, scored(8, "three people asked me for it unprompted"));
    o.note(Axis::Roadmap, scored(6, "first step is a prototype; unclear after that"));
    o.note(Axis::Money, scored(5, "no cost to start, unknown return"));
    o.note(Axis::Fit, scored(9, "it is the thing I already do"));
    o
}

// --- the weakest axis is the point ------------------------------------------

#[test]
fn a_fatal_axis_sinks_it_regardless_of_the_others() {
    // Four eights and a one averages to a respectable six and a half, and the
    // one is the thing that actually happens.
    let mut o = full();
    o.note(Axis::Roadmap, scored(1, "no route from a prototype to anyone paying"));
    match o.verdict() {
        Verdict::SetAside { axis, why } => {
            assert_eq!(axis, Axis::Roadmap);
            assert!(why.contains("no route"));
        }
        other => panic!("it survived a fatal axis: {other:?}"),
    }
}

#[test]
fn the_weakest_axis_is_what_gets_presented_first() {
    // The strong parts are why you want to do it and you will find them
    // yourself. The weak one is what you hit in week three.
    match full().verdict() {
        Verdict::Worth { weakest, .. } => assert_eq!(weakest, Axis::Money),
        other => panic!("{other:?}"),
    }
    assert!(full().presented().contains("Weakest part is Money"));
}

// --- nothing is scored on a hunch -------------------------------------------

#[test]
fn a_score_with_nothing_behind_it_is_not_a_score() {
    let mut o = full();
    o.note(Axis::Fit, Finding::Scored { out_of_ten: 9, rests_on: vec![] });
    assert!(matches!(o.verdict(), Verdict::NeedFirst(_)));
}

#[test]
fn an_unexamined_axis_counts_as_unknown_not_neutral() {
    // Neutral is the dangerous default: an opportunity nobody looked at would
    // score the same as one examined and found middling.
    let mut o = Opportunity::new("something", "a video");
    o.note(Axis::Work, scored(8, "a weekend"));
    assert_eq!(o.unlooked().len(), 4);
    assert!(matches!(o.verdict(), Verdict::NeedFirst(_)));
}

#[test]
fn it_names_the_one_thing_it_needs_rather_than_listing_everything() {
    let o = Opportunity::new("something", "a video");
    match o.verdict() {
        Verdict::NeedFirst(q) => assert!(q.ends_with('?'), "got: {q}"),
        other => panic!("{other:?}"),
    }
}

// --- money is honest about not knowing --------------------------------------

#[test]
fn money_says_it_cannot_judge_yet_rather_than_guessing() {
    let mut o = full();
    o.note(Axis::Money, money_is_not_auditable_yet());
    match o.verdict() {
        Verdict::NeedFirst(why) => assert!(why.contains("actual accounts"), "got: {why}"),
        other => panic!("it scored money anyway: {other:?}"),
    }
}

#[test]
fn atlas_says_which_axes_it_can_judge_on_its_own() {
    // An assistant that answers all five equally confidently is hiding which
    // ones it guessed.
    assert!(Axis::Work.atlas_can_judge());
    assert!(Axis::Roadmap.atlas_can_judge());
    assert!(Axis::Authenticity.atlas_can_judge());
    assert!(!Axis::Money.atlas_can_judge());
    assert!(!Axis::Fit.atlas_can_judge());
}

#[test]
fn a_blocked_axis_is_reported_with_what_would_unblock_it() {
    let mut o = full();
    o.note(Axis::Money, money_is_not_auditable_yet());
    assert_eq!(o.blocked().len(), 1);
    assert_eq!(o.blocked()[0].0, Axis::Money);
}

// --- ordering ---------------------------------------------------------------

#[test]
fn a_fatal_axis_is_reported_before_a_missing_one() {
    // "This will never work and here is why" beats "I need three more answers".
    let mut o = Opportunity::new("x", "a video");
    o.note(Axis::Authenticity, scored(1, "nobody has ever asked for this"));
    assert!(matches!(o.verdict(), Verdict::SetAside { axis: Axis::Authenticity, .. }));
}

// --- what it says -----------------------------------------------------------

#[test]
fn where_it_came_from_is_kept() {
    // A pitch you saw and a problem you hit are not the same evidence.
    match full().verdict() {
        Verdict::Worth { because, .. } => {
            assert!(because.iter().any(|b| b.contains("a problem I hit myself")));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn every_axis_asks_something_answerable() {
    for a in Axis::all() {
        let q = a.asks();
        assert!(q.ends_with('?'), "{a:?}: {q}");
        assert!(q.len() > 25, "{a:?} is too terse");
    }
}

#[test]
fn noting_an_axis_twice_replaces_rather_than_duplicates() {
    let mut o = full();
    o.note(Axis::Work, scored(2, "much worse than I thought"));
    assert_eq!(o.looks.len(), 5);
    assert_eq!(o.finding(Axis::Work).and_then(|f| f.score()), Some(2));
}
