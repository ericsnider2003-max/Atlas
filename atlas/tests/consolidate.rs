use atlas::consolidate::{
    dropped_note, learn, over_budget_on_purpose, same_claim, size_note, trim, worth_keeping, Claim,
    ConsolidateConfig, Confirmation,
};
use atlas::freshness::Shelf;

const DAY: u64 = 86_400;

fn claim(says: &str, shelf: Shelf, sources: &[&str], first_at: u64, asked: u32) -> Claim {
    Claim {
        says: says.into(),
        shelf,
        confirmations: sources
            .iter()
            .enumerate()
            .map(|(i, s)| Confirmation { source: (*s).into(), at: first_at + i as u64 * DAY, as_worded: says.into() })
            .collect(),
        first_at,
        asked_about: asked,
        corrected: false,
        density: atlas::consolidate::Density::Full,
    }
}

// ================= asking differently costs you nothing =================

#[test]
fn the_same_thing_asked_twice_is_one_claim_not_two() {
    // Two notes means two timestamps, two decay curves, and confidence in a
    // settled fact drifting down because you happened to ask twice.
    assert!(same_claim(
        "a wash sale disallows the loss if you rebuy within 30 days",
        "the wash sale rule disallows losses when rebought inside thirty days"
    ));
}

#[test]
fn two_different_claims_are_not_merged_just_because_they_share_words() {
    // Merging two things that aren't the same claim loses one silently, which
    // is worse than keeping a near-duplicate.
    assert!(!same_claim(
        "a wash sale disallows the loss if you rebuy within 30 days",
        "section 1256 contracts are taxed 60/40 regardless of holding period"
    ));
}

#[test]
fn re_reading_something_does_not_make_atlas_less_sure_of_it() {
    // Decay is measured from when it was last confirmed, not from first
    // sight. Measuring from first_at is how asking again ages a fact.
    let old_but_reconfirmed = claim(
        "the wash sale window is 30 days",
        Shelf::Slow,
        &["irs.gov", "a broker's guide"],
        0,
    // asked twice
        2,
    );
    assert!(old_but_reconfirmed.as_of() > 0, "as_of follows the newest confirmation");
    assert_eq!(old_but_reconfirmed.as_of(), DAY);
}

#[test]
fn two_sources_agreeing_raises_confidence_and_the_same_source_twice_does_not() {
    let one = claim("x", Shelf::Slow, &["a.com"], 0, 0);
    let same_twice = claim("x", Shelf::Slow, &["a.com", "a.com"], 0, 0);
    let two = claim("x", Shelf::Slow, &["a.com", "b.com"], 0, 0);

    assert_eq!(one.standing(), same_twice.standing(), "the same source twice is one source");
    assert!(two.standing() > one.standing());
}

#[test]
fn agreement_caps_so_a_popular_wrong_answer_is_not_unassailable() {
    let three = claim("x", Shelf::Slow, &["a", "b", "c"], 0, 0);
    let ten = claim("x", Shelf::Slow, &["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"], 0, 0);
    assert_eq!(three.standing(), ten.standing());
    assert!(ten.standing() < 1.0);
}

#[test]
fn something_you_corrected_has_no_standing_however_many_sources_agreed() {
    let mut c = claim("x", Shelf::Slow, &["a", "b", "c"], 0, 5);
    c.corrected = true;
    assert_eq!(c.standing(), 0.0);
}

// ================= set in stone stays set =================

#[test]
fn something_settled_never_needs_checking_again_however_old() {
    // That's what settled means. Re-checking it is work that produces
    // nothing.
    let settled = claim("TCP retransmits on timeout", Shelf::Settled, &["a"], 0, 0);
    assert!(!settled.worth_rechecking(3650 * DAY));
}

#[test]
fn something_about_your_own_setup_never_needs_checking_either() {
    let yours = claim("panels go on the right of the primary screen", Shelf::Yours, &["you"], 0, 0);
    assert!(!yours.worth_rechecking(3650 * DAY));
}

#[test]
fn something_that_changes_does_need_checking() {
    let version = claim("the latest release is 1.4.2", Shelf::Quick, &["a"], 0, 0);
    assert!(!version.worth_rechecking(DAY));
    assert!(version.worth_rechecking(200 * DAY));
}

// ================= what gets dropped =================

#[test]
fn a_settled_fact_is_worth_more_to_keep_than_a_stale_quote() {
    let settled = claim("how a protocol works", Shelf::Settled, &["a"], 0, 0);
    let quote = claim("the price was 412", Shelf::Volatile, &["a"], 0, 0);
    assert!(worth_keeping(&settled, 400 * DAY) > worth_keeping(&quote, 400 * DAY) * 3.0);
}

#[test]
fn something_you_keep_asking_about_survives_over_something_you_never_did() {
    let used = claim("a", Shelf::Slow, &["x"], 0, 9);
    let never = claim("b", Shelf::Slow, &["x"], 0, 0);
    assert!(worth_keeping(&used, DAY) > worth_keeping(&never, DAY));
}

#[test]
fn trimming_drops_the_stale_and_unused_first() {
    let mut known = vec![
        claim("a stale quote", Shelf::Volatile, &["x"], 0, 0),
        claim("a settled fact", Shelf::Settled, &["x"], 0, 3),
        claim("an old version number", Shelf::Quick, &["x"], 0, 0),
    ];
    let dropped = trim(&mut known, 1, 400 * DAY);
    assert_eq!(known.len(), 1);
    assert_eq!(known[0].says, "a settled fact");
    assert_eq!(dropped.len(), 2);
}

#[test]
fn nothing_settled_is_dropped_even_when_that_breaks_the_budget() {
    // Sorting by worth would nearly always protect these, and nearly always
    // isn't a guarantee. Losing something that can't be looked up again to
    // satisfy a number is the wrong trade.
    let mut known: Vec<Claim> = (0..20)
        .map(|i| claim(&format!("settled {i}"), Shelf::Settled, &["x"], 0, 0))
        .collect();
    known.push(claim("a stale quote", Shelf::Volatile, &["x"], 0, 0));

    let dropped = trim(&mut known, 5, 400 * DAY);
    assert_eq!(known.len(), 20, "all twenty settled facts survived a budget of five");
    assert_eq!(dropped, vec!["a stale quote"]);
}

#[test]
fn nothing_about_you_is_dropped_either() {
    let mut known: Vec<Claim> = (0..10)
        .map(|i| claim(&format!("your preference {i}"), Shelf::Yours, &["you"], 0, 0))
        .collect();
    let dropped = trim(&mut known, 2, 400 * DAY);
    assert_eq!(known.len(), 10);
    assert!(dropped.is_empty());
}

#[test]
fn a_corrected_claim_loses_its_protection() {
    // Otherwise "settled" becomes a way for a wrong fact to be permanent.
    let mut wrong = claim("something wrong", Shelf::Settled, &["x"], 0, 0);
    wrong.corrected = true;
    let mut known = vec![wrong, claim("right", Shelf::Settled, &["x"], 0, 0)];
    let dropped = trim(&mut known, 1, DAY);
    assert_eq!(dropped, vec!["something wrong"]);
}

#[test]
fn being_over_budget_on_purpose_is_said_rather_than_hidden() {
    // A cap quietly being exceeded is a cap that isn't doing anything.
    let known: Vec<Claim> = (0..20)
        .map(|i| claim(&format!("settled {i}"), Shelf::Settled, &["x"], 0, 0))
        .collect();
    let said = over_budget_on_purpose(&known, 5).unwrap();
    assert!(said.contains("20 things known against a budget of 5"));
    assert!(said.contains("can't be looked up again"));
    assert!(over_budget_on_purpose(&known, 50).is_none());
}

#[test]
fn dropping_things_is_said_rather_than_done_quietly() {
    // A store that silently forgets is one you stop trusting.
    let said = dropped_note(&["a".into(), "b".into()]).unwrap();
    assert!(said.contains("Forgot 2 things"));
    assert!(said.contains("Nothing settled and nothing about your own setup"));
    assert!(dropped_note(&[]).is_none());
}

// ================= it doesn't fill the laptop =================

#[test]
fn the_whole_store_is_a_few_megabytes() {
    // The fear is that it grows without bound. The numbers say otherwise.
    let said = size_note(10_000);
    assert!(said.contains("10000 things known"));
    assert!(said.contains("3MB"), "got: {said}");
    assert!(said.contains("doesn't grow while you're not asking"));
}

#[test]
fn the_protection_is_not_configurable_away() {
    // The old version of this asserted a `#[serde(skip)]` bool pinned true
    // that nothing read. Deleted 19 Sep 2026 -- `trim` does not take the
    // config struct at all, only `keep_at_most`, so there was never anywhere
    // for the flag to be consulted.
    //
    // What protects them is the partition: settled and yours are held out of
    // the trim entirely rather than ranked highly within it, and the budget
    // is allowed to be exceeded rather than drop one.
    let mut known: Vec<Claim> = Vec::new();
    for i in 0..6 {
        known.push(claim(&format!("settled thing {i}"), Shelf::Settled, &["a"], 100, 0));
    }
    for i in 0..6 {
        known.push(claim(&format!("ordinary thing {i}"), Shelf::Slow, &["a"], 100, 0));
    }

    // A budget far below the number of protected claims.
    let dropped = trim(&mut known, 2, 1_000);
    assert!(
        !dropped.iter().any(|d| d.contains("settled thing")),
        "a settled claim was dropped to satisfy a number: {dropped:?}"
    );
    assert_eq!(
        known.iter().filter(|c| c.shelf == Shelf::Settled).count(),
        6,
        "all six survive a budget of two -- the budget gives way, not the claims"
    );

    // And an old config naming the removed key still loads.
    let parsed: ConsolidateConfig =
        serde_yaml::from_str("enabled: true\nprotects_settled_and_yours: false\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn learning_the_same_thing_twice_strengthens_rather_than_duplicates() {
    // The whole point, end to end.
    let mut known: Vec<Claim> = Vec::new();
    assert!(!learn(&mut known, "the wash sale window is 30 days", "irs.gov", 0));
    assert_eq!(known.len(), 1);

    // Same fact, your words, months later, a different source.
    let merged = learn(
        &mut known,
        "a wash sale window of thirty days applies",
        "a broker's guide",
        200 * DAY,
    );
    assert!(merged, "should have strengthened what was already known");
    assert_eq!(known.len(), 1, "one claim, not two");
    assert_eq!(known[0].independent_sources(), 2);

    // And it's now dated from the newer confirmation, not the older one.
    assert_eq!(known[0].as_of(), 200 * DAY);
    assert!(known[0].standing() > 0.7, "two sources agreeing is worth more than one");
}

#[test]
fn a_genuinely_different_fact_is_added_rather_than_merged() {
    let mut known: Vec<Claim> = Vec::new();
    learn(&mut known, "the wash sale window is 30 days", "irs.gov", 0);
    learn(&mut known, "section 1256 contracts are taxed sixty forty", "irs.gov", 0);
    assert_eq!(known.len(), 2);
}

// ================= does compacting lose knowledge =================

use atlas::consolidate::{knew_once, once_knew, tombstone, trim_with_stones, Tombstone};

#[test]
fn dropping_a_claim_loses_the_claim_but_not_that_you_knew_it() {
    // The honest answer is yes, compacting loses knowledge. What it doesn't
    // have to lose is the knowledge that you once knew — much smaller, and
    // the part that actually hurts.
    let mut known = vec![
        claim("the tier two fee is forty basis points", Shelf::Quick, &["the broker's site"], 0, 0),
        claim("a settled fact worth keeping", Shelf::Settled, &["x"], 0, 5),
    ];
    let mut stones: Vec<Tombstone> = Vec::new();
    let dropped = trim_with_stones(&mut known, &mut stones, 1, 400 * DAY);

    assert_eq!(dropped.len(), 1);
    assert_eq!(stones.len(), 1, "it left a marker");
    assert_eq!(stones[0].source, "the broker's site");
}

#[test]
fn a_search_that_would_have_hit_a_dropped_claim_says_so_rather_than_nothing() {
    // Silently returning nothing is the failure that makes you stop trusting
    // the store.
    let c = claim("the tier two fee is forty basis points", Shelf::Quick, &["the broker's site"], 0, 0);
    let stones = vec![tombstone(&c, DAY)];

    let found = once_knew(&stones, "what was the tier two fee").unwrap();
    let said = knew_once(found);
    assert!(said.contains("let it go to save space"));
    // The useful part: it can go and get it again rather than shrugging.
    assert!(said.contains("the broker's site"));
    assert!(said.contains("want me to look again"));
}

#[test]
fn something_it_never_knew_does_not_match_a_tombstone() {
    let c = claim("the tier two fee is forty basis points", Shelf::Quick, &["x"], 0, 0);
    let stones = vec![tombstone(&c, DAY)];
    assert!(once_knew(&stones, "what is the melting point of gallium").is_none());
}

#[test]
fn a_tombstone_is_a_fraction_of_the_size_of_the_claim() {
    let c = claim(
        "the wash sale rule disallows a loss when substantially identical securities are \
         repurchased within thirty days before or after the sale",
        Shelf::Slow,
        &["irs.gov"],
        0,
        0,
    );
    let t = tombstone(&c, DAY);
    assert!(t.about.len() * 3 < c.says.len(), "{} vs {}", t.about.len(), c.says.len());
}

#[test]
fn tombstones_do_not_grow_forever_either() {
    // The whole point is that nothing does.
    let mut known: Vec<Claim> = (0..500)
        .map(|i| claim(&format!("volatile thing {i}"), Shelf::Volatile, &["x"], 0, 0))
        .collect();
    let mut stones: Vec<Tombstone> = Vec::new();
    trim_with_stones(&mut known, &mut stones, 10, 400 * DAY);
    assert!(stones.len() <= 40, "{} tombstones against a cap of 40", stones.len());
}

// ================= squeeze before dropping =================

use atlas::consolidate::{compact, make_room, worth_compacting, Density};

#[test]
fn a_claim_loses_its_elaboration_before_it_loses_its_fact() {
    // Most of what makes a claim long is qualifier and example. Almost none
    // of it is the fact.
    let long = "a wash sale disallows the loss if you rebuy within 30 days, which means the \
                loss is added to the cost basis of the new position instead";
    let short = compact(long);
    assert!(short.contains("rebuy within 30 days"), "the fact survived: {short}");
    assert!(!short.contains("cost basis"), "the elaboration went");
    assert!(short.len() * 2 < long.len());
}

#[test]
fn compacting_stops_at_a_sentence_rather_than_mid_clause() {
    // Dropping a clause mid-sentence is how you get a fact that reads fine
    // and means something else.
    let two = "the deadline is April 15. Filing late costs a penalty.";
    assert_eq!(compact(two), "the deadline is April 15");
}

#[test]
fn something_already_short_is_left_alone() {
    // There's nothing to remove, and squeezing it risks losing a word that
    // mattered.
    assert!(!worth_compacting("the tick is 0.25"));
    assert!(!worth_compacting("the wash sale window is thirty days"));
}

#[test]
fn squeezing_happens_before_anything_is_dropped() {
    // The whole point: compacting the long ones usually frees enough that
    // nothing has to go, and a compacted claim still answers the question.
    let long = "the tier two fee is forty basis points, which applies once you pass the \
                monthly volume threshold and resets at the start of each month";
    let mut known = vec![
        claim(long, Shelf::Slow, &["the broker"], 0, 2),
        claim("a settled fact", Shelf::Settled, &["x"], 0, 5),
    ];
    let mut stones: Vec<Tombstone> = Vec::new();
    let (squeezed, dropped) = make_room(&mut known, &mut stones, 2, DAY);

    assert_eq!(squeezed, 1);
    assert!(dropped.is_empty(), "nothing needed to go");
    let kept = known.iter().find(|c| c.says.contains("forty basis points")).unwrap();
    assert_eq!(kept.density, Density::Compact);
    assert!(kept.says.len() * 2 < long.len());
}

#[test]
fn a_compacted_claim_still_answers_the_question() {
    // The difference from a tombstone, and the reason for the middle tier.
    let long = "the tier two fee is forty basis points, which applies once you pass the \
                monthly volume threshold";
    let short = compact(long);
    assert!(short.contains("forty basis points"), "still an answer, not a pointer");
}

#[test]
fn dropping_only_happens_when_squeezing_was_not_enough() {
    let mut known: Vec<Claim> = (0..10)
        .map(|i| claim(&format!("short volatile {i}"), Shelf::Volatile, &["x"], 0, 0))
        .collect();
    let mut stones: Vec<Tombstone> = Vec::new();
    let (squeezed, dropped) = make_room(&mut known, &mut stones, 3, 400 * DAY);

    assert_eq!(squeezed, 0, "nothing was worth squeezing");
    assert_eq!(dropped.len(), 7);
    assert_eq!(stones.len(), 7, "and each left a pointer");
}

#[test]
fn the_three_states_are_ordered_by_how_much_survives() {
    assert!(Density::Full < Density::Compact);
    assert!(Density::Compact < Density::Stone);
}
