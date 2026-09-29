//! The half of the record that nothing kept.

use atlas::refusals::{Refusals, ENOUGH_TO_LOOK, KEEP_EXAMPLES, LOPSIDED};

fn many(r: &mut Refusals, pair: &str, label: &str, n: u32) {
    for i in 0..n {
        r.declined(pair, label, &format!("reason {i}"), 1_000 + i as u64);
    }
}

#[test]
fn a_refusal_is_counted_against_the_pair_and_the_cause() {
    let mut r = Refusals::default();
    r.declined("eurusd", "costs too much", "the spread ate 34%", 10);
    r.declined("EURUSD", "costs too much", "the spread ate 41%", 20);
    r.declined("EURUSD", "inside the noise", "the stop was half a range", 30);
    // The pair is normalised, so `eurusd` and `EURUSD` are one instrument and
    // not two half-populated ones.
    assert_eq!(r.by_pair.len(), 1);
    assert_eq!(r.refusals_for("eurusd"), 3);
    assert_eq!(r.refusals_for("EURUSD"), 3);
    assert_eq!(r.refusals_for("GBPUSD"), 0);
}

#[test]
fn the_ideas_it_did_find_are_kept_too_because_a_count_needs_a_denominator() {
    // "Refused forty times" means nothing on its own. Forty refusals and
    // thirty trades is a busy reader; forty and none is a wall.
    let mut r = Refusals::default();
    many(&mut r, "EURUSD", "costs too much", 40);
    r.proposed("EURUSD");
    r.proposed("eurusd");
    assert_eq!(r.ideas_for("EURUSD"), 2);
    assert_eq!(r.proposed.len(), 1, "the denominator was split across two spellings");
    assert!(r.spoken("EURUSD").contains("2 trade(s)"));
}

#[test]
fn examples_are_kept_but_only_a_few_and_never_the_same_one_twice() {
    // Enough to read a count back into a case; few enough that the file cannot
    // grow without bound on a daemon that runs for months.
    let mut r = Refusals::default();
    for i in 0..50 {
        r.declined("EURUSD", "costs too much", &format!("reason {i}"), i as u64);
    }
    let (_, causes) = &r.by_pair[0];
    assert_eq!(causes[0].times, 50);
    assert_eq!(causes[0].examples.len(), KEEP_EXAMPLES);

    let mut same = Refusals::default();
    for _ in 0..10 {
        same.declined("EURUSD", "costs too much", "the same sentence every time", 1);
    }
    assert_eq!(same.by_pair[0].1[0].examples.len(), 1);
}

// ---------------------------------------------------------------------------
// Reading the shape of it
// ---------------------------------------------------------------------------

#[test]
fn nothing_is_read_into_a_handful_of_refusals() {
    // One cause out of three is not a pattern, and reporting it as one is how
    // a reader talks itself into moving a limit on noise.
    let mut r = Refusals::default();
    many(&mut r, "EURUSD", "costs too much", ENOUGH_TO_LOOK - 1);
    assert_eq!(r.dominant("EURUSD"), None);
    assert_eq!(r.what_that_suggests("EURUSD"), None);
    assert!(r.spoken("EURUSD").contains("not going to read anything into"));

    // One more, and the same evidence is now worth looking at.
    r.declined("EURUSD", "costs too much", "one more", 99);
    assert!(r.dominant("EURUSD").is_some());
}

#[test]
fn one_cause_swamping_the_rest_is_named_as_a_setting_and_not_as_a_market() {
    // This is the finding the whole module exists for. A reader refusing
    // everything on cost is not being careful — it is on a timeframe its own
    // spread limit cannot survive, and no amount of waiting changes that.
    let mut r = Refusals::default();
    many(&mut r, "EURUSD", "costs too much", 45);
    many(&mut r, "EURUSD", "not enough room", 5);
    let (label, share) = r.dominant("EURUSD").unwrap();
    assert_eq!(label, "costs too much");
    assert!(share > LOPSIDED);
    let said = r.what_that_suggests("EURUSD").unwrap();
    assert!(said.contains("timeframe is too small"), "{said}");
    assert!(said.contains("waiting does not fix it"));
}

#[test]
fn a_spread_of_causes_is_what_a_careful_reader_looks_like_and_is_said_so() {
    let mut r = Refusals::default();
    many(&mut r, "EURUSD", "costs too much", 12);
    many(&mut r, "EURUSD", "not enough room", 12);
    many(&mut r, "EURUSD", "inside the noise", 12);
    assert_eq!(r.what_that_suggests("EURUSD"), None, "no cause dominates here");
    assert!(r.spoken("EURUSD").contains("being careful"));
}

#[test]
fn every_refusal_levels_can_produce_has_something_to_say_about_it() {
    // A label with no reading is a label that reports a count and no meaning,
    // which is the state this module was built to end. Every one of
    // `NoTrade`'s own labels is covered.
    let labels = [
        "no structure",
        "unreadable",
        "inside the noise",
        "costs too much",
        "not enough room",
        "no money",
        "standing down",
        "thin book",
    ];
    for label in labels {
        let mut r = Refusals::default();
        many(&mut r, "EURUSD", label, 40);
        assert!(
            r.what_that_suggests("EURUSD").is_some(),
            "nothing to say about a wall of '{label}'"
        );
    }
    // And the list above is the whole list, checked against the source rather
    // than against my memory of it.
    let src = include_str!("../src/levels.rs");
    for label in labels {
        assert!(src.contains(&format!("=> \"{label}\"")), "levels no longer produces '{label}'");
    }
    let produced = src.matches("(_) => \"").count();
    assert_eq!(produced, labels.len(), "levels grew a refusal nothing can read");
}

// ---------------------------------------------------------------------------
// It reports; it does not act
// ---------------------------------------------------------------------------

#[test]
fn nothing_here_can_change_a_limit_it_has_been_complaining_about() {
    // A module that loosened its own rules because it had been refusing a lot
    // is a system that talks itself into trades.
    //
    // Checked against the code rather than the whole file: the module doc says
    // the word "Rules" while explaining that it never touches them, and a
    // check that cannot tell prose from code is a check that fails on its own
    // documentation. (It did, first time it was run.)
    let src = include_str!("../src/refusals.rs");
    let code: String = src
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(!code.contains("Rules"), "refusals reached into the limits it reports on");
    assert!(!code.contains("crate::levels"), "refusals reached into levels");
}

#[test]
fn an_instrument_never_seen_says_so_rather_than_reporting_zeroes() {
    let r = Refusals::default();
    assert!(r.spoken("USDCHF").contains("haven't looked"));
}
