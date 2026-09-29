use atlas::otherside::{against, is_asked_for, spoken as os_spoken, written, Angle, Decision};
use atlas::returning::{
    full_brief, how_long, welcome, Address, Gone, Happened, ReturnConfig, Welcome,
};

fn cfg() -> ReturnConfig {
    ReturnConfig { address: "Eric".into(), ..Default::default() }
}

fn happened(what: &str, needs_you: bool, failed: bool) -> Happened {
    Happened { what: what.into(), needs_you, failed, at: 0 }
}

// ================= coming back =================

#[test]
fn five_minutes_is_not_an_absence() {
    assert_eq!(how_long(300), Gone::Moment);
    let w = welcome(300, &[happened("the index finished", false, false)], 10, &cfg());
    assert_eq!(w, Welcome::Nothing, "silence");
}

#[test]
fn how_long_you_were_gone_changes_what_you_get() {
    assert_eq!(how_long(3600), Gone::ShortWhile);
    assert_eq!(how_long(20_000), Gone::HalfDay);
    assert_eq!(how_long(60_000), Gone::Overnight);
}

#[test]
fn something_wrong_or_waiting_is_said_straight_away_not_offered() {
    let w = welcome(
        20_000,
        &[happened("the nine o'clock post missed its window", true, true)],
        9,
        &cfg(),
    );
    match w {
        Welcome::Straight(s) => {
            assert!(s.starts_with("Morning, Eric."));
            assert!(s.contains("missed its window"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn nothing_urgent_is_offered_rather_than_recited() {
    // You may have come back to do something specific, and a briefing is in
    // the way.
    let w = welcome(
        20_000,
        &[happened("the index finished overnight", false, false)],
        14,
        &cfg(),
    );
    match w {
        Welcome::Offer(s) => {
            assert!(s.contains("Afternoon, Eric."));
            assert!(s.contains("when you're ready"));
            assert!(s.contains("the index finished"), "and names it: {s}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_urgent_ones_lead_and_the_rest_are_offered_after() {
    let w = welcome(
        20_000,
        &[
            happened("the post needs a decision", true, false),
            happened("the index finished", false, false),
            happened("the backup ran", false, false),
        ],
        9,
        &cfg(),
    );
    match w {
        Welcome::Straight(s) => {
            assert!(s.contains("needs a decision"));
            assert!(s.contains("2 other things when you want it"));
            assert!(!s.contains("backup"), "the rest aren't recited");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_long_list_of_urgent_things_is_capped_rather_than_read_out() {
    let many: Vec<Happened> =
        (0..6).map(|i| happened(&format!("thing {i} failed"), false, true)).collect();
    match welcome(60_000, &many, 9, &cfg()) {
        Welcome::Straight(s) => assert!(s.contains("and 4 more"), "got: {s}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_quiet_night_gets_one_line_and_a_quiet_hour_gets_silence() {
    assert!(matches!(welcome(60_000, &[], 9, &cfg()), Welcome::Straight(_)));
    assert_eq!(welcome(3600, &[], 14, &cfg()), Welcome::Nothing);
}

#[test]
fn how_atlas_addresses_you_is_yours_including_not_at_all() {
    // A system that calls you "sir" uninvited is doing a bit.
    assert_eq!(Address::default(), Address::None);
    assert_eq!(Address::None.greet(9), "");
    assert_eq!(Address::Name("Eric".into()).greet(9), "Morning, Eric. ");
    assert_eq!(Address::Title("sir".into()).greet(20), "Evening, sir. ");

    let plain = ReturnConfig::default();
    match welcome(60_000, &[happened("x failed", false, true)], 9, &plain) {
        Welcome::Straight(s) => assert!(s.starts_with('x'), "no greeting at all: {s}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn saying_yes_to_the_offer_puts_what_needs_you_first() {
    let b = full_brief(&[
        happened("the backup ran", false, false),
        happened("the post needs a decision", true, false),
    ]);
    let decision = b.find("needs a decision").unwrap();
    let backup = b.find("backup").unwrap();
    assert!(decision < backup);
}

// ================= arguing the other side =================

fn decision() -> Decision {
    Decision {
        what: "moving the website to a new VPS".into(),
        because: "it might as well be done at some point".into(),
        reversible: false,
        costs: Some("about four days and $40 a month".into()),
        depends_on: vec!["the payment provider's IP allowlist".into()],
        previously: Some("you moved it in March and moved it back".into()),
    }
}

#[test]
fn it_is_never_volunteered_only_asked_for() {
    // Arguing against a decision nobody asked about is exhausting.
    assert!(is_asked_for("argue the other side of this"));
    assert!(is_asked_for("talk me out of it"));
    assert!(is_asked_for("poke holes in this"));
    assert!(!is_asked_for("I'm moving the website to a new VPS"));
}

#[test]
fn something_you_cannot_undo_is_the_strongest_objection() {
    let objections = against(&decision());
    assert_eq!(objections[0].angle, Angle::HardToUndo);
    assert!(objections[0].case.contains("can't be undone"));
}

#[test]
fn having_decided_it_before_is_raised_with_what_happened() {
    let o = against(&decision());
    let prev = o.iter().find(|o| o.angle == Angle::YouTriedThis).unwrap();
    assert!(prev.case.contains("moved it back"));
    assert!(prev.what_would_answer_it.contains("what's different this time"));
}

#[test]
fn the_cost_is_weighed_against_something_rather_than_just_stated() {
    let o = against(&decision());
    let cost = o.iter().find(|o| o.angle == Angle::NotWorthIt).unwrap();
    assert!(cost.case.contains("$40 a month"));
    assert!(cost.what_would_answer_it.contains("number on the other side"));
}

#[test]
fn what_it_rests_on_that_you_do_not_control_is_named() {
    let o = against(&decision());
    let dep = o.iter().find(|o| o.angle == Angle::RestsOnSomethingElse).unwrap();
    assert!(dep.case.contains("allowlist"));
}

#[test]
fn the_case_for_doing_nothing_is_always_made() {
    // Nearly always worth asking and nearly never asked.
    let o = against(&decision());
    assert!(o.iter().any(|o| o.angle == Angle::NothingIsFine));
}

#[test]
fn a_reason_that_reads_as_found_afterwards_is_pointed_at_gently() {
    let o = against(&decision());
    let real = o.iter().find(|o| o.angle == Angle::RealReason).unwrap();
    assert!(real.case.contains("doesn't make it wrong"), "not an accusation: {}", real.case);
}

#[test]
fn a_solid_reason_does_not_get_that_objection() {
    let solid = Decision {
        because: "the current box runs out of memory twice a week".into(),
        ..decision()
    };
    assert!(!against(&solid).iter().any(|o| o.angle == Angle::RealReason));
}

#[test]
fn a_reversible_cheap_decision_gets_almost_no_case_against_it() {
    // Six objections to everything is a second opinion you learn to ignore.
    let easy = Decision {
        what: "trying the smaller model for a week".into(),
        because: "the big one is slow".into(),
        reversible: true,
        costs: None,
        depends_on: vec![],
        previously: None,
    };
    let o = against(&easy);
    assert!(o.len() <= 2, "got {} objections", o.len());
}

#[test]
fn it_is_framed_as_the_case_against_not_as_atlas_disagreeing() {
    // It was asked for. Presenting it as a personal view would misrepresent
    // what it is.
    let d = decision();
    let said = os_spoken(&d, &against(&d));
    assert!(said.starts_with("The strongest case against:"));
    assert!(said.contains("What would answer it"));

    let w = written(&d, &against(&d));
    assert!(w.contains("None of that means don't"));
    assert!(w.contains("someone who disagreed would make"));
}

#[test]
fn every_objection_says_what_would_settle_it() {
    // An objection with no answer is just discouragement.
    for o in against(&decision()) {
        assert!(!o.what_would_answer_it.is_empty(), "{:?} has no answer", o.angle);
    }
}
