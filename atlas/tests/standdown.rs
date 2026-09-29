//! Having a calendar and not using it.
//!
//! `market::events` knew when every central bank speaks, out to 2027, and
//! nothing ever asked it. These tests are about the asking — and in particular
//! about the one bar a close-only check gets wrong.

use atlas::market::bars::Bars;
use atlas::market::events::{month, Purpose};
use atlas::market::timeframe::Tf;
use atlas::standdown::{spanning, standing_down, Blackout, StanddownConfig};

/// Non-farm payrolls for the May 2026 reference month: released Friday
/// 5 June 2026, 08:30 New York. June is EDT, so that is 12:30 UTC.
///
/// Hard-coded on purpose and then checked against the module's own arithmetic
/// below, so a test built on a date I invented cannot pass quietly.
const NFP_JUNE_2026: i64 = 1_780_662_600_000;

/// **12:00 UTC the same day: the OPEN of the H4 bar covering 12:00–16:00**,
/// which therefore contains the print, its window and the whole recovery. Its
/// close, 16:00, is three and a half hours clear of everything.
///
/// Stated as an open since 17 Sep 2026. It was `H4_OPEN_CONTAINING = 16:00`, and
/// the fixture below built bars stamped with their closes to match — so the
/// fixture and `standdown` agreed with each other and both disagreed with the
/// real files, which are open-stamped (`AsOf::opened_at` has the evidence).
/// Every property these tests assert is unchanged; only the convention the
/// fixture speaks in has been corrected.
const H4_OPEN_CONTAINING: i64 = 1_780_675_200_000 - 4 * 60 * 60 * 1000;

/// The H4 bar OPENING 12:00 UTC on Thursday 4 June 2026 — the day before
/// payrolls, and nothing scheduled inside it.
///
/// Found by sweeping the calendar rather than picked by eye. My first choice
/// was the Wednesday, and the Wednesday's 12:00–16:00 bar contains ISM Services
/// at 14:00: a "quiet" fixture that was not quiet, which would have made the
/// test below assert the opposite of what it says.
const QUIET: i64 = 1_780_588_800_000 - 4 * 60 * 60 * 1000;

#[test]
fn the_instant_these_tests_are_built_on_is_the_one_the_calendar_derives() {
    let ev = month(2026, 6).expect("June 2026");
    let nfp = ev
        .iter()
        .find(|e| e.name.starts_with("Non-Farm"))
        .expect("payrolls in June 2026");
    assert_eq!(
        nfp.at, NFP_JUNE_2026,
        "the constant these tests hang off is not the instant the calendar derives"
    );
}

// ---------------------------------------------------------------------------
// The bar a close-only check calls clean
// ---------------------------------------------------------------------------

#[test]
fn the_h4_bar_that_closes_clear_of_payrolls_still_contains_them() {
    // The whole reason this module asks an interval question. The close sits
    // three and a half hours past the print — every window in `Purpose` has
    // shut by then — so an instant check at the close finds nothing.
    let (inside, _) =
        atlas::market::events::inside_blackout(H4_OPEN_CONTAINING, "EURUSD", Purpose::AdverseSelection)
            .expect("calendar");
    assert!(!inside, "the close really is clear — that is the trap");

    let found = spanning(Tf::H4, H4_OPEN_CONTAINING, "EURUSD", Purpose::AdverseSelection)
        .expect("calendar")
        .expect("the bar contains payrolls");
    assert!(found.iter().any(|e| e.name.starts_with("Non-Farm")));
}

#[test]
fn on_m1_the_span_question_and_the_instant_question_agree() {
    // Not a redundant check. It is what says the span test is measuring the
    // bar's width rather than simply being more willing to refuse: on a
    // one-minute bar there is almost no width for it to find.
    // 16:00 — the close of the H4 bar above, and genuinely clear: every
    // window around the 12:30 print has shut by 14:30. Asked as an M1 bar
    // OPENING then, so it covers 16:00-16:01 and nothing else.
    //
    // It used to ask about an M1 bar at `H4_CLOSE_AFTER`, which was the same
    // instant under the old close-stamped constant. Under open-stamping the
    // containing bar's stamp is 12:00, which is the exact start of the
    // AdverseSelection window (release minus thirty minutes) — so asking
    // there finds something, correctly, and would have made this test assert
    // the opposite of what it says.
    let clear_instant = H4_OPEN_CONTAINING + 4 * 60 * 60 * 1000;
    let clear = spanning(Tf::M1, clear_instant, "EURUSD", Purpose::AdverseSelection)
        .expect("calendar");
    assert!(clear.is_none(), "an M1 bar opening at 16:00 holds nothing");

    let inside = spanning(
        Tf::M1,
        NFP_JUNE_2026 + 60_000,
        "EURUSD",
        Purpose::AdverseSelection,
    )
    .expect("calendar");
    assert!(inside.is_some(), "the minute after the print is not clear");
}

#[test]
fn the_instrument_decides_and_a_us_print_reaches_a_pair_with_no_dollar_in_it() {
    let euro = spanning(Tf::H4, H4_OPEN_CONTAINING, "EURUSD", Purpose::AdverseSelection).unwrap();
    let carry = spanning(Tf::H4, H4_OPEN_CONTAINING, "AUDJPY", Purpose::AdverseSelection).unwrap();
    let neither = spanning(Tf::H4, H4_OPEN_CONTAINING, "EURGBP", Purpose::AdverseSelection).unwrap();
    assert!(euro.is_some());
    assert!(carry.is_some(), "the carry cross-section unwinds on a US print");
    assert!(neither.is_none(), "but it must not reach everything");
}

// ---------------------------------------------------------------------------
// Through a view
// ---------------------------------------------------------------------------

/// `n` H4 bars ending with one that **opens** at `last_open`.
fn h4_ending(last_open: i64, n: usize) -> Bars {
    let step = 4 * 60 * 60 * 1000;
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    for i in 0..n {
        let px = 1.1000 + (i % 5) as f64 * 1e-4;
        o.push(px);
        h.push(px + 5e-4);
        l.push(px - 5e-4);
        c.push(px);
        t.push(last_open - ((n - 1 - i) as i64) * step);
    }
    Bars::new(o, h, l, c, t).expect("bars")
}

#[test]
fn a_view_ending_on_that_bar_stands_down_even_though_its_close_is_clear() {
    let bars = h4_ending(H4_OPEN_CONTAINING, 40);
    let view = bars.latest().unwrap();
    let why = standing_down(&view, "EURUSD", &StanddownConfig::default())
        .expect("calendar")
        .expect("standing down");
    assert!(
        matches!(why, Blackout::InsideTheBar(_)),
        "it should be the bar that refuses it, not the instant"
    );
    assert!(why.plain().contains("Non-Farm"));
}

#[test]
fn checking_only_the_close_lets_that_same_bar_through() {
    // The configuration that models the mistake, so the difference between the
    // two is a measured thing rather than a claim in a doc comment.
    let bars = h4_ending(H4_OPEN_CONTAINING, 40);
    let view = bars.latest().unwrap();
    let close_only = StanddownConfig { check_the_bar: false, ..StanddownConfig::default() };
    assert_eq!(standing_down(&view, "EURUSD", &close_only).expect("calendar"), None);
}

#[test]
fn a_quiet_bar_is_left_alone() {
    let bars = h4_ending(QUIET, 40);
    let view = bars.latest().unwrap();
    assert_eq!(
        standing_down(&view, "EURUSD", &StanddownConfig::default()).expect("calendar"),
        None,
        "a rule that refuses everything is not a rule"
    );
    assert!(atlas::standdown::spoken(&view, "EURUSD", &StanddownConfig::default())
        .contains("read it normally"));
}

#[test]
fn standing_in_front_of_the_print_is_the_instant_case_not_the_bar_case() {
    // A bar CLOSING five minutes after the release: the decision instant is
    // inside the window, which is the simpler question and the one asked
    // first.
    //
    // The fixture takes an open, so the open is four hours before that close.
    // Spelled out rather than folded into a constant, because the whole point
    // of this test is which end of the bar is being asked about.
    const H4: i64 = 4 * 60 * 60 * 1000;
    let closes_just_after = NFP_JUNE_2026 + 5 * 60_000;
    let bars = h4_ending(closes_just_after - H4, 40);
    let view = bars.latest().unwrap();
    let why = standing_down(&view, "EURUSD", &StanddownConfig::default())
        .unwrap()
        .expect("standing down");
    assert!(matches!(why, Blackout::Now(_)));
}

// ---------------------------------------------------------------------------
// No clock
// ---------------------------------------------------------------------------

#[test]
fn a_series_with_no_timestamps_is_refused_rather_than_waved_through() {
    let closes: Vec<f64> = (0..40).map(|i| 1.1 + (i % 5) as f64 * 1e-4).collect();
    let bars = Bars::new(
        closes.clone(),
        closes.iter().map(|c| c + 5e-4).collect(),
        closes.iter().map(|c| c - 5e-4).collect(),
        closes,
        Vec::new(),
    )
    .unwrap();
    let view = bars.latest().unwrap();
    let e = standing_down(&view, "EURUSD", &StanddownConfig::default())
        .expect_err("no clock is not the same as clear");
    assert!(e.contains("timestamps"));

    // And it is a default, not a law — but the default is the safe one.
    let reckless = StanddownConfig { no_clock_is_clear: true, ..StanddownConfig::default() };
    assert_eq!(standing_down(&view, "EURUSD", &reckless).unwrap(), None);
    assert!(!StanddownConfig::default().no_clock_is_clear);
}

// ---------------------------------------------------------------------------
// Wired, not merely built
// ---------------------------------------------------------------------------

#[test]
fn the_window_is_the_one_the_spread_measurement_supports() {
    // Not `Execution`. The spread is what takes the trade out, and liquidity
    // withdrawal begins thirty minutes BEFORE the print — which the narrow
    // window does not cover at all.
    assert_eq!(StanddownConfig::default().purpose, Purpose::AdverseSelection);
    let (before, _) = Purpose::AdverseSelection.window();
    assert_eq!(before, 30);

    let ahead = spanning(
        Tf::M15,
        NFP_JUNE_2026 - 20 * 60_000,
        "EURUSD",
        Purpose::AdverseSelection,
    )
    .unwrap();
    assert!(ahead.is_some(), "twenty minutes ahead of payrolls is not clear");
    let narrow =
        spanning(Tf::M15, NFP_JUNE_2026 - 20 * 60_000, "EURUSD", Purpose::Execution).unwrap();
    assert!(narrow.is_none(), "and the narrow window is exactly what misses it");
}

#[test]
fn proposing_a_trade_into_a_release_is_refused_before_any_arithmetic_happens() {
    // The wiring that matters. `levels::propose` asks this first, so a reading
    // taken inside a window never becomes an idea with a stop on it.
    let bars = h4_ending(H4_OPEN_CONTAINING, 200);
    let view = bars.latest().unwrap();
    let purse = atlas::levels::Purse { balance: 10_000.0, value_per_point: 1.0 };
    let rules = atlas::levels::Rules::default();
    assert!(rules.mind_the_calendar, "on by default, or it is not wired at all");
    let out = atlas::levels::propose(
        &view,
        "EURUSD",
        atlas::levels::Side::Buy,
        view.now(),
        0.000_15,
        &purse,
        &rules,
    );
    assert!(
        matches!(out, Err(atlas::levels::NoTrade::StandingDown(_))),
        "got {out:?}"
    );
}

#[test]
fn refusing_to_act_and_failing_to_read_are_different_answers() {
    // They sound alike and they mean opposite things. "I could not work this
    // out" is a gap in the reading and should count against Atlas's marks;
    // "I worked it out and will not act" is the system working. One variant
    // each, so nothing downstream has to guess from the wording.
    let bars = h4_ending(H4_OPEN_CONTAINING, 200);
    let view = bars.latest().unwrap();
    let purse = atlas::levels::Purse { balance: 10_000.0, value_per_point: 1.0 };
    let off = atlas::levels::Rules { mind_the_calendar: false, ..atlas::levels::Rules::default() };
    let out = atlas::levels::propose(
        &view,
        "EURUSD",
        atlas::levels::Side::Buy,
        view.now(),
        0.000_15,
        &purse,
        &off,
    );
    assert!(
        !matches!(out, Err(atlas::levels::NoTrade::StandingDown(_))),
        "with the calendar off, nothing may refuse for calendar reasons"
    );
}

#[test]
fn bars_with_no_clock_come_back_unreadable_rather_than_standing_down() {
    // The third case, and the one it was tempting to fold into the second.
    // A missing clock is not a judgement about the market — it is the bars
    // arriving without times on them. Calling that "standing down" would
    // quietly exempt a data defect from ever counting against the record.
    let closes: Vec<f64> = (0..200).map(|i| 1.1 + (i % 5) as f64 * 1e-4).collect();
    let bars = Bars::new(
        closes.clone(),
        closes.iter().map(|c| c + 5e-4).collect(),
        closes.iter().map(|c| c - 5e-4).collect(),
        closes,
        Vec::new(),
    )
    .unwrap();
    let view = bars.latest().unwrap();
    let purse = atlas::levels::Purse { balance: 10_000.0, value_per_point: 1.0 };
    let out = atlas::levels::propose(
        &view,
        "EURUSD",
        atlas::levels::Side::Buy,
        view.now(),
        0.000_15,
        &purse,
        &atlas::levels::Rules::default(),
    );
    assert!(matches!(out, Err(atlas::levels::NoTrade::Unreadable(_))), "got {out:?}");
}
