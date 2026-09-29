//! Where a stop and a target go — and, far more often, why there is no trade.
//!
//! Most of this file is about refusals. That is deliberate and it is the point
//! of the module: a system that always has an answer is a system whose answer
//! means nothing.
//!
//! ## What changed, and what it cost
//!
//! These tests used to build a `Market` by hand — walls in known places,
//! average range set to a chosen number — so that each one failed for exactly
//! one reason. That property is gone: `propose` now reads a view of real bars,
//! so a change in the structure reader can fail a test about position sizing.
//!
//! It is a real loss and worth naming rather than glossing. What is bought for
//! it is larger: a hand-built market summary can describe a state the bars
//! could never produce, so every one of those tests was checking the arithmetic
//! against a market that may not exist. The fixtures below are bars, and the
//! walls are wherever the reader actually finds them.

use atlas::levels::{cost_share, gaps, how_far_it_travels, how_many_ranges_out, propose,
                    stop_is_in_noise, NoTrade, Purse, Rules, Side};
use atlas::market::bars::Bars;

/// A range with known walls, oscillating between them.
///
/// `box_range` rather than a random walk: on a twenty-bar window a walk reads
/// as a trend often enough that using one to test "flat" would make the test
/// measure the seed.
fn boxed(lo: f64, hi: f64) -> Bars {
    atlas::market::fixtures::box_range(400, lo, hi, 40)
}

/// Rules with the calendar off.
///
/// The fixtures are `box_range`, which carries no timestamps — so the calendar
/// check correctly refuses every one of them, for a reason that has nothing to
/// do with what these tests are about. Turned off here and tested on its own in
/// `tests/standdown.rs`, rather than left on and worked around.
fn no_calendar() -> Rules {
    Rules { mind_the_calendar: false, ..Rules::default() }
}

fn purse() -> Purse {
    Purse { balance: 10_000.0, value_per_point: 1.0 }
}

fn near(a: f64, b: f64, slack: f64) -> bool {
    (a - b).abs() < slack
}

// ---------------------------------------------------------------------------
// The arithmetic that needs no market at all
// ---------------------------------------------------------------------------

#[test]
fn what_the_spread_costs_is_a_share_of_the_reward_and_not_of_the_price() {
    // A spread of one pip against a twenty-pip target, paid on both legs, is a
    // tenth of the trade. The same spread measured against the PRICE is
    // 0.0001 of it, which is the number that makes every trade look free.
    //
    // The reward is in R, so the distance it is aiming at is the stop times
    // the reward: a ten-pip stop at 2R is twenty pips.
    assert!(near(cost_share(0.0010, 0.0001, 2.0).unwrap(), 0.10, 1e-9));
    assert!(near(cost_share(0.0010, 0.000_01, 2.0).unwrap(), 0.01, 1e-9));
    // Nothing to aim at, nothing to take a share of. `None` rather than a
    // number, so a caller cannot treat "unanswerable" as "cheap".
    assert_eq!(cost_share(0.0010, 0.0001, 0.0), None);
    assert_eq!(cost_share(0.0, 0.0001, 2.0), None);
}

#[test]
fn a_stop_inside_one_ordinary_bar_range_is_a_coin_toss_with_a_fee() {
    let rules = no_calendar();
    assert_eq!(how_many_ranges_out(0.0006, Some(0.0012)), Some(0.5));
    assert_eq!(how_many_ranges_out(0.0024, Some(0.0012)), Some(2.0));
    assert_eq!(stop_is_in_noise(0.0006, Some(0.0012), &rules), Some(true));
    assert_eq!(stop_is_in_noise(0.0024, Some(0.0012), &rules), Some(false));
    // Exactly one range is the boundary and is NOT in the noise — the rule is
    // "closer than", and a boundary that flips with rounding is a rule nobody
    // can reason about.
    assert_eq!(stop_is_in_noise(0.0012, Some(0.0012), &rules), Some(false));
}

#[test]
fn no_average_range_is_no_answer_rather_than_a_clean_bill() {
    // "There is no range to compare against" and "the stop is fine" are
    // different facts, and callers have to say which they mean.
    assert_eq!(how_many_ranges_out(0.001, None), None);
    assert_eq!(stop_is_in_noise(0.001, None, &no_calendar()), None);
    assert_eq!(how_many_ranges_out(0.001, Some(0.0)), None);
    assert_eq!(how_many_ranges_out(f64::NAN, Some(0.001)), None);
}

#[test]
fn an_unfilled_gap_is_somewhere_price_has_something_to_do() {
    // Their reader has no fair-value-gap finder, so this is the one piece of
    // my old market module that survived the merge.
    //
    // A run straight up leaves gaps behind it; a range that keeps coming back
    // through its own levels fills them.
    let ramp = atlas::market::fixtures::ramp(80, 0.0030);
    let view = ramp.latest().unwrap();
    let found = gaps(&view);
    for (lo, hi) in &found {
        assert!(hi > lo, "a gap has a low below its high: {lo} {hi}");
    }

    let ranging = boxed(1.0800, 1.0850);
    let flat = ranging.latest().unwrap();
    assert!(
        gaps(&flat).len() <= found.len(),
        "a market that keeps returning through its own levels leaves fewer open"
    );
}

// ---------------------------------------------------------------------------
// Refusals, which is most of what this module does
// ---------------------------------------------------------------------------

#[test]
fn without_a_balance_and_a_value_per_point_there_is_no_size_to_work_out() {
    // A size worked out from a guess is worse than no size.
    let bars = boxed(1.0800, 1.0900);
    let view = bars.latest().unwrap();
    let broke = Purse { balance: 0.0, value_per_point: 1.0 };
    let no = propose(&view, "EURUSD", Side::Buy, view.now(), 0.0001, &broke, &no_calendar());
    assert!(matches!(no, Err(NoTrade::NoMoney(_))), "{no:?}");

    let weightless = Purse { balance: 10_000.0, value_per_point: 0.0 };
    assert!(matches!(
        propose(&view, "EURUSD", Side::Buy, view.now(), 0.0001, &weightless, &no_calendar()),
        Err(NoTrade::NoMoney(_))
    ));
}

#[test]
fn something_that_is_not_a_price_is_refused_before_anything_else() {
    let bars = boxed(1.0800, 1.0900);
    let view = bars.latest().unwrap();
    for bad in [f64::NAN, 0.0, -1.1] {
        assert!(matches!(
            propose(&view, "EURUSD", Side::Buy, bad, 0.0001, &purse(), &no_calendar()),
            Err(NoTrade::Unreadable(_))
        ));
    }
}

#[test]
fn a_spread_that_eats_the_reward_is_refused_and_says_what_it_ate() {
    // The cost that kills a strategy is never the one on the ticket.
    let bars = boxed(1.0800, 1.0900);
    let view = bars.latest().unwrap();
    let huge = 0.0050;
    match propose(&view, "EURUSD", Side::Buy, view.now(), huge, &purse(), &no_calendar()) {
        Err(NoTrade::CostsTooMuch(why)) => {
            assert!(!why.is_empty(), "a refusal has to say what it refused for");
        }
        // A range this tight may refuse for room first, which is also correct —
        // what must never happen is an idea coming back priced as though the
        // spread were free.
        Err(other) => assert!(
            !matches!(other, NoTrade::NoMoney(_)),
            "refused, but for the wrong reason: {other:?}"
        ),
        Ok(idea) => panic!("a fifty-pip spread produced a tradeable idea: {idea:?}"),
    }
}

#[test]
fn every_refusal_carries_a_short_name_and_a_sentence() {
    // The name is for counting them; the sentence is for the person. A module
    // that refuses most of the time has to be able to say why it refused, or
    // the pattern in its refusals can never be seen.
    let bars = boxed(1.0800, 1.0810);
    let view = bars.latest().unwrap();
    if let Err(no) = propose(&view, "EURUSD", Side::Buy, view.now(), 0.0001, &purse(), &no_calendar()) {
        assert!(!no.label().is_empty());
        assert!(no.plain().len() > 20, "{}", no.plain());
        assert!(!no.label().contains(' ') || no.label().len() < 20);
    }
}

// ---------------------------------------------------------------------------
// An idea, when there is one
// ---------------------------------------------------------------------------

#[test]
fn an_idea_risks_what_the_rules_say_and_no_more() {
    let bars = boxed(1.0700, 1.1000);
    let view = bars.latest().unwrap();
    let rules = no_calendar();
    if let Ok(idea) = propose(&view, "EURUSD", Side::Buy, view.now(), 0.0001, &purse(), &rules) {
        let want = purse().balance * rules.risk_fraction;
        assert!(
            near(idea.risking, want, 0.01),
            "risking {} and the rule says {want}",
            idea.risking
        );
        assert!(idea.stop < idea.entry, "a long's stop is below it");
        assert!(idea.target > idea.entry, "and its target above");
        assert!(idea.reward >= rules.min_reward, "{}", idea.reward);
        assert!(idea.size > 0.0);
        assert!(!idea.why.is_empty() && !idea.wrong_if.is_empty());
    }
}

#[test]
fn the_runners_up_are_shown_rather_than_discarded() {
    // The runner-up is the first thing anybody argues about, and hiding it
    // makes the chosen target look like the only possibility.
    let bars = boxed(1.0700, 1.1000);
    let view = bars.latest().unwrap();
    if let Ok(idea) = propose(&view, "EURUSD", Side::Buy, view.now(), 0.0001, &purse(), &no_calendar()) {
        for (at, reward, why) in &idea.instead {
            assert!(*at > 0.0);
            assert!(*reward >= 0.0);
            assert!(!why.is_empty(), "a runner-up says what it was");
        }
        // And they are sayable, not just stored. A runner-up nobody can read
        // is a runner-up that was discarded with extra steps.
        let said = idea.other_targets();
        if !idea.instead.is_empty() {
            assert!(!said.is_empty(), "the runners-up have to be readable");
        }
    }
}

#[test]
fn a_sell_is_the_mirror_of_a_buy_and_not_a_special_case() {
    let bars = boxed(1.0700, 1.1000);
    let view = bars.latest().unwrap();
    if let Ok(idea) = propose(&view, "EURUSD", Side::Sell, view.now(), 0.0001, &purse(), &no_calendar()) {
        assert!(idea.stop > idea.entry, "a short's stop is above it");
        assert!(idea.target < idea.entry, "and its target below");
        assert_eq!(idea.side, Side::Sell);
    }
}

#[test]
fn the_same_range_can_refuse_in_both_directions_and_that_is_an_answer() {
    // Deliberately kept from the old file. A range too tight to pay one R
    // either way is not a failure of the module — it is the module working.
    // What would be wrong is producing an idea anyway.
    let tight = boxed(1.08000, 1.08040);
    let view = tight.latest().unwrap();
    let buy = propose(&view, "EURUSD", Side::Buy, view.now(), 0.0001, &purse(), &no_calendar());
    let sell = propose(&view, "EURUSD", Side::Sell, view.now(), 0.0001, &purse(), &no_calendar());
    assert!(buy.is_err() && sell.is_err(), "buy {buy:?}  sell {sell:?}");
}

#[test]
fn a_target_further_off_than_the_market_has_ever_moved_is_not_a_target() {
    // The failure that test above caught the first time it was actually run.
    //
    // The round-number grid does not look at the chart. On a four-pip box it
    // offered the next figure fifty pips away, which won on reward — seven
    // times the risk — off a market that has not moved four pips in four
    // hundred bars. Nothing in the arithmetic was wrong. The target was a
    // wish, and a wish with a good reward ratio beats every real level.
    let tight = boxed(1.08000, 1.08040);
    let view = tight.latest().unwrap();
    let travelled = how_far_it_travels(&view, no_calendar().structure_bars).unwrap();
    assert!(travelled < 0.0010, "the fixture moves {travelled:.5}, which is the whole point");

    let sell = propose(&view, "EURUSD", Side::Sell, view.now(), 0.0001, &purse(), &no_calendar());
    match sell {
        Err(NoTrade::NotEnoughRoom(_)) | Err(NoTrade::CostsTooMuch(_)) => {}
        other => panic!("a fifty-pip target off a four-pip range came back as {other:?}"),
    }
}

#[test]
fn a_market_that_does_move_keeps_its_targets() {
    // The other half, and the half that stops this being a rule that refuses
    // everything: on a range price has actually walked, the same levels are
    // well inside reach and nothing is dropped.
    let wide = boxed(1.0700, 1.1000);
    let view = wide.latest().unwrap();
    let travelled = how_far_it_travels(&view, no_calendar().structure_bars).unwrap();
    assert!(travelled > 0.0050, "moved {travelled:.5}");
    let buy = propose(&view, "EURUSD", Side::Buy, view.now(), 0.0001, &purse(), &no_calendar());
    let sell = propose(&view, "EURUSD", Side::Sell, view.now(), 0.0001, &purse(), &no_calendar());
    assert!(
        buy.is_ok() || sell.is_ok(),
        "the reach cap refused a market with three hundred pips in it: {buy:?} / {sell:?}"
    );
}

#[test]
fn the_reach_is_measured_off_the_bars_and_says_so_when_it_cannot_be() {
    let bars = boxed(1.0700, 1.1000);
    let view = bars.latest().unwrap();
    // Over the whole window, exactly the high-water to low-water span.
    let all = how_far_it_travels(&view, view.len()).unwrap();
    let highest = view.high().iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let lowest = view.low().iter().copied().fold(f64::INFINITY, f64::min);
    assert!((all - (highest - lowest)).abs() < 1e-12);
    // A shorter window can only be narrower.
    assert!(how_far_it_travels(&view, 20).unwrap() <= all);
    // And nothing to measure is `None`, never nought — nought would refuse
    // every trade and look like a rule.
    assert_eq!(how_far_it_travels(&view, 0), None);
}

#[test]
fn reading_a_view_is_what_makes_these_numbers_mean_anything() {
    // The merge in one test. `propose` takes a borrow bounded at a bar, so it
    // cannot be told about a level that had not formed yet — not because it
    // would be caught, but because the value is not reachable from what it
    // holds.
    let bars = boxed(1.0700, 1.1000);
    let early = bars.as_of(120).unwrap();
    let late = bars.latest().unwrap();
    assert_eq!(early.len(), 121);
    assert!(late.len() > early.len());
    // Both are readable; neither can see past its own bound.
    let a = propose(&early, "EURUSD", Side::Buy, early.now(), 0.0001, &purse(), &no_calendar());
    let b = propose(&late, "EURUSD", Side::Buy, late.now(), 0.0001, &purse(), &no_calendar());
    let _ = (a, b); // either may refuse; what matters is that neither panicked
}

#[test]
fn the_two_spellings_of_a_direction_convert_in_one_place() {
    // `levels::Side` is Buy/Sell because an order is bought or sold;
    // `market::claims::Side` is Long/Short because a claim argues a direction.
    // Both are kept, and the conversion lives in one place rather than being
    // written out at each boundary — a conversion written four times is a
    // conversion that is wrong in one of them.
    use atlas::market::claims::Side as Claiming;
    assert_eq!(Side::Buy.as_claim(), Claiming::Long);
    assert_eq!(Side::Sell.as_claim(), Claiming::Short);
    assert_eq!(Claiming::from(Side::Buy), Claiming::Long);
}

#[test]
fn a_trade_proposal_shows_the_claim_vocabulary_alongside_the_structure() {
    // `propose` reads swing structure; `claims::verify` reads its own
    // vocabulary. Nothing joined the two in one command before 12 Sep 2026
    // -- a proposal and the claims that would back or contradict it were
    // two separate things to ask for. Pinned so the two don't drift back
    // apart into being asked for separately again.
    let main: &str = &crate::common::source_of("main");
    assert!(
        main.contains("market::claims::ALL_KINDS"),
        "a trade proposal no longer checks the claim vocabulary for the side it's proposing"
    );
    assert!(
        main.contains("market::claims::verify("),
        "the claims are named but never actually checked against the bars"
    );
}
