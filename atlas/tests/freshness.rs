//! Stale knowledge, said as stale.
//!
//! The failure this prevents: a fact that was true in March, stored next to a
//! fact that is true forever, read back in the same tone in September.

use atlas::freshness::*;

const HOUR: u64 = 3600;
const DAY: u64 = 24 * HOUR;

fn page(claim: &str, shelf: Shelf, as_of: u64) -> Known {
    Known::new(claim, shelf, Checkable::Page("https://x".into()), as_of)
}

// --- decay ------------------------------------------------------------------

#[test]
fn settled_facts_never_decay() {
    let k = page("TCP retransmits on timeout", Shelf::Settled, 0);
    assert_eq!(k.weight(50 * 365 * DAY), 1.0);
    assert_eq!(k.state(50 * 365 * DAY), State::Fresh);
}

#[test]
fn your_own_preferences_do_not_expire_on_a_clock() {
    // A preference from two years ago is still your preference until you say
    // otherwise. Ageing it would make Atlas re-ask things you already settled.
    let k = Known::new("prefers Chrome on the left", Shelf::Yours, Checkable::YouSaid, 0);
    assert_eq!(k.state(700 * DAY), State::Fresh);
    assert!(!k.should_recheck(700 * DAY));
}

#[test]
fn a_version_number_goes_stale_in_weeks_not_years() {
    let k = page("the latest release is 1.4.2", Shelf::Quick, 0);
    assert_eq!(k.state(2 * DAY), State::Fresh);
    assert_eq!(k.state(20 * DAY), State::Ageing);
    assert_eq!(k.state(60 * DAY), State::Stale);
}

#[test]
fn a_price_is_stale_the_same_day() {
    let k = page("it costs $40", Shelf::Volatile, 0);
    assert_eq!(k.state(HOUR), State::Fresh);
    assert_eq!(k.state(2 * DAY), State::Stale);
}

#[test]
fn confidence_halves_rather_than_falling_off_a_cliff() {
    // A 29-day-old fact treated as certain and a 31-day-old one as worthless
    // is the behaviour a hard expiry gives you.
    let k = page("x", Shelf::Quick, 0);
    let at_one_half_life = k.weight(14 * DAY);
    assert!((at_one_half_life - 0.5).abs() < 0.05, "got {at_one_half_life}");
    let at_two = k.weight(28 * DAY);
    assert!((at_two - 0.25).abs() < 0.05, "got {at_two}");
}

#[test]
fn nothing_ever_reaches_zero() {
    // A two-year-old version number is still a better starting point than
    // nothing. It just must not be said as though it were current.
    let k = page("x", Shelf::Volatile, 0);
    assert!(k.weight(3650 * DAY) > 0.0);
}

// --- what it says -----------------------------------------------------------

#[test]
fn a_fresh_fact_is_said_plainly() {
    let k = page("the office is on Mill Street", Shelf::Slow, 0);
    assert_eq!(k.spoken(DAY), "the office is on Mill Street");
}

#[test]
fn an_ageing_fact_carries_its_age() {
    let k = page("the latest release is 1.4.2", Shelf::Quick, 0);
    let said = k.spoken(20 * DAY);
    assert!(said.contains("1.4.2"));
    assert!(said.contains("days ago") || said.contains("weeks ago"), "got: {said}");
}

#[test]
fn a_stale_fact_is_offered_with_a_recheck_rather_than_withheld() {
    // "I don't know" is rarely more useful than "here's what was true in
    // March, want me to check?"
    let k = page("the latest release is 1.4.2", Shelf::Quick, 0);
    let said = k.spoken(90 * DAY);
    assert!(said.contains("1.4.2"), "the claim was withheld: {said}");
    assert!(said.contains("look again"), "no offer to check: {said}");
}

#[test]
fn a_stale_fact_it_cannot_check_says_so_instead_of_offering() {
    let m = Known::new("they moved to Leeds", Shelf::Slow, Checkable::ModelAlone, 0);
    let said = m.spoken(900 * DAY);
    assert!(said.contains("they moved to Leeds"), "the claim was withheld: {said}");
    assert!(said.contains("can't check it myself"), "got: {said}");

    // The shelf decides decay, not the source: a Slow claim ages even when
    // you were the one who said it. Only Shelf::Yours is exempt.
    let from_you = Known::new("they moved to Leeds", Shelf::Slow, Checkable::YouSaid, 0);
    assert_eq!(from_you.state(900 * DAY), State::Stale);
}

#[test]
fn ages_read_the_way_a_person_says_them() {
    assert_eq!(ago(30), "just now");
    assert!(ago(5 * HOUR).contains("hours"));
    assert!(ago(5 * DAY).contains("days"));
    assert!(ago(30 * DAY).contains("weeks"));
    assert!(ago(200 * DAY).contains("months"));
    assert!(ago(1000 * DAY).contains("years"));
}

// --- rechecking -------------------------------------------------------------

#[test]
fn only_things_it_can_check_alone_are_offered_for_rechecking() {
    let now = 90 * DAY;
    assert!(page("x", Shelf::Quick, 0).should_recheck(now));
    assert!(Known::new("x", Shelf::Quick, Checkable::File("/a".into()), 0).should_recheck(now));
    assert!(Known::new("x", Shelf::Quick, Checkable::Ran("ver".into()), 0).should_recheck(now));
    assert!(!Known::new("x", Shelf::Quick, Checkable::ModelAlone, 0).should_recheck(now));
    assert!(!Known::new("x", Shelf::Quick, Checkable::YouSaid, 0).should_recheck(now));
}

#[test]
fn a_settled_fact_is_never_worth_rechecking() {
    assert!(!page("why the sky is blue", Shelf::Settled, 0).should_recheck(9999 * DAY));
}

// --- classification ---------------------------------------------------------

#[test]
fn wording_places_a_claim_on_the_right_shelf() {
    assert_eq!(shelf_for("the price is $12"), Shelf::Volatile);
    assert_eq!(shelf_for("the latest version is 3.1"), Shelf::Quick);
    assert_eq!(shelf_for("the CEO is Jane Doe"), Shelf::Slow);
    assert_eq!(shelf_for("HTTP stands for hypertext transfer protocol"), Shelf::Settled);
}

#[test]
fn an_unclassifiable_claim_gets_a_shelf_life_not_immortality() {
    // Treated as settled, an unclassified claim is one Atlas is still
    // asserting in a year.
    assert_eq!(shelf_for("the thing does the thing"), Shelf::Slow);
    assert_ne!(shelf_for("some arbitrary sentence"), Shelf::Settled);
}

#[test]
fn the_more_perishable_reading_wins_when_a_claim_matches_two() {
    // "the current price the CEO paid" is both Volatile and Slow. Being wrong
    // toward perishable costs a sentence; the other way costs a wrong action.
    assert_eq!(shelf_for("the price the CEO paid"), Shelf::Volatile);
    assert_eq!(shelf_for("the latest law on this"), Shelf::Quick);
}

#[test]
fn a_model_only_claim_is_marked_as_such() {
    // Once written down it looks identical to a sourced one. Marking it is
    // the only way that survives storage.
    assert!(!Checkable::ModelAlone.can_recheck_alone());
    assert!(Checkable::ModelAlone.plain().contains("nothing behind it"));
}

// --- ranking ----------------------------------------------------------------

#[test]
fn freshness_breaks_ties_without_overruling_relevance() {
    let fresh = page("x", Shelf::Quick, 0);
    let stale = page("x", Shelf::Quick, 0);
    let now = 200 * DAY;
    let hi = ranking_multiplier(&fresh, 0);
    let lo = ranking_multiplier(&stale, now);
    assert!(hi > lo);
    // A stale exact match must still be able to beat a fresh vague one, so
    // the penalty is bounded.
    assert!(lo > 0.5, "stale content was effectively deleted: {lo}");
    assert!(hi <= 1.0);
}

#[test]
fn a_settled_fact_takes_no_ranking_penalty_ever() {
    let k = page("how TCP works", Shelf::Settled, 0);
    assert_eq!(ranking_multiplier(&k, 0), ranking_multiplier(&k, 9999 * DAY));
}

// --- how it changes what Atlas says -----------------------------------------

use atlas::certainty::{aged, Confidence};

#[test]
fn a_fresh_basis_leaves_confidence_alone() {
    let basis = [page("x", Shelf::Quick, 0)];
    let (level, note) = aged(Confidence::Fine, &basis, DAY);
    assert_eq!(level, Confidence::Fine);
    assert!(note.is_none());
}

#[test]
fn an_old_basis_forces_a_qualifier_onto_a_confident_answer() {
    // The failure being prevented: an answer that looks well-formed, drawn
    // from a note written in March, spoken in September in the same tone.
    let basis = [page("the latest release is 1.4.2", Shelf::Quick, 0)];
    let (level, note) = aged(Confidence::Fine, &basis, 120 * DAY);
    assert_eq!(level, Confidence::Qualify);
    assert!(note.unwrap().contains("months ago"));
}

#[test]
fn the_weakest_source_decides_not_the_average() {
    // Nine fresh notes must not hide one that is two years out of date.
    let mut basis: Vec<Known> = (0..9).map(|_| page("x", Shelf::Settled, 0)).collect();
    basis.push(page("the latest version is 2.0", Shelf::Quick, 0));
    let (level, _) = aged(Confidence::Fine, &basis, 300 * DAY);
    assert_eq!(level, Confidence::Qualify);
}

#[test]
fn age_can_never_raise_confidence() {
    // A stale fact stated crisply is still stale.
    let basis = [page("x", Shelf::Settled, 0)];
    let (level, _) = aged(Confidence::Withhold, &basis, 0);
    assert_eq!(level, Confidence::Withhold);
}

#[test]
fn no_basis_means_no_opinion_about_age() {
    let (level, note) = aged(Confidence::Fine, &[], 999 * DAY);
    assert_eq!(level, Confidence::Fine);
    assert!(note.is_none());
}
