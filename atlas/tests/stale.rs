//! The third thing that happens to a trade: nothing.

use atlas::levels::Side;
use atlas::market::bars::Bars;
use atlas::market::time::MS_PER_HOUR;
use atlas::stale::{how_its_going, implied_bars, spoken, Going, Open, StaleConfig};

const START: i64 = 1_780_876_800_000; // Monday 8 June 2026, 00:00 UTC

/// Hourly bars whose high and low come from `shape`.
fn hourly(n: usize, shape: impl Fn(usize) -> (f64, f64)) -> Bars {
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    for i in 0..n {
        let (hi, lo) = shape(i);
        o.push((hi + lo) / 2.0);
        h.push(hi);
        l.push(lo);
        c.push((hi + lo) / 2.0);
        t.push(START + i as i64 * MS_PER_HOUR);
    }
    Bars::new(o, h, l, c, t).expect("bars")
}

fn long_trade() -> Open {
    Open { side: Side::Buy, entry: 1.1000, stop: 1.0980, target: 1.1040, opened_at: START }
}

// ---------------------------------------------------------------------------
// The pace, which is measured
// ---------------------------------------------------------------------------

#[test]
fn how_long_it_ought_to_take_comes_off_the_bars_and_not_out_of_the_air() {
    // Forty pips to target, ten pips of ordinary movement a bar: four bars if
    // price went in a straight line, which it never does. That is why it is
    // called an implied pace and not a prediction.
    let t = long_trade();
    let near = |a: Option<f64>, b: f64| a.map(|x| (x - b).abs() < 1e-9) == Some(true);
    assert!(near(implied_bars(&t, Some(0.0010)), 4.0), "{:?}", implied_bars(&t, Some(0.0010)));
    // Half the movement, twice as long.
    assert!(near(implied_bars(&t, Some(0.0005)), 8.0));
    // And no answer at all when there is nothing to divide by, rather than a
    // number that would make every trade instantly stale.
    assert_eq!(implied_bars(&t, None), None);
    assert_eq!(implied_bars(&t, Some(0.0)), None);
}

#[test]
fn with_no_measurable_pace_it_refuses_rather_than_calling_the_trade_stale() {
    let bars = hourly(50, |_| (1.1001, 1.0999));
    let view = bars.latest().unwrap();
    let e = how_its_going(&long_trade(), &view, None, &StaleConfig::default())
        .expect_err("no pace is not the same as no progress");
    assert!(e.contains("just an opinion"));
}

// ---------------------------------------------------------------------------
// The three endings
// ---------------------------------------------------------------------------

#[test]
fn a_trade_that_goes_nowhere_for_long_enough_is_named_as_going_nowhere() {
    // Forty bars of a two-pip box on a trade that needed four. Nothing hits,
    // nothing moves, and "it hasn't lost yet" is not the same as "it's working".
    let bars = hourly(40, |_| (1.1001, 1.0999));
    let view = bars.latest().unwrap();
    let v = how_its_going(&long_trade(), &view, Some(0.0010), &StaleConfig::default()).unwrap();
    match v {
        Going::Stale { bars, implied, best } => {
            assert_eq!(bars, 40);
            assert!((implied - 4.0).abs() < 1e-9);
            assert!(best < 0.25, "best was {best:.2}");
        }
        other => panic!("{other:?}"),
    }
    assert!(v.is_stale());
    assert!(spoken(&long_trade(), &view, Some(0.0010), &StaleConfig::default())
        .contains("choices rather than findings"));
}

#[test]
fn the_stop_and_the_target_settle_it_and_neither_is_ever_called_stale() {
    let stopped = hourly(40, |i| if i < 3 { (1.1001, 1.0999) } else { (1.1001, 1.0970) });
    let reached = hourly(40, |i| if i < 3 { (1.1001, 1.0999) } else { (1.1050, 1.0999) });
    let cfg = StaleConfig::default();
    assert_eq!(
        how_its_going(&long_trade(), &stopped.latest().unwrap(), Some(0.0010), &cfg).unwrap(),
        Going::Stopped
    );
    assert_eq!(
        how_its_going(&long_trade(), &reached.latest().unwrap(), Some(0.0010), &cfg).unwrap(),
        Going::Reached
    );
}

#[test]
fn a_bar_that_spans_both_is_read_as_the_stop() {
    // There is no way to know from a bar which side it touched first, and
    // assuming the good one is how a backtest invents money.
    let both = hourly(40, |i| if i < 3 { (1.1001, 1.0999) } else { (1.1050, 1.0970) });
    assert_eq!(
        how_its_going(&long_trade(), &both.latest().unwrap(), Some(0.0010), &StaleConfig::default())
            .unwrap(),
        Going::Stopped
    );
}

#[test]
fn a_trade_still_inside_its_implied_pace_is_left_alone() {
    // A rule that can fire on the second bar is a rule that will.
    let bars = hourly(3, |_| (1.1001, 1.0999));
    let view = bars.latest().unwrap();
    let v = how_its_going(&long_trade(), &view, Some(0.0010), &StaleConfig::default()).unwrap();
    assert!(matches!(v, Going::TooEarly { .. }), "{v:?}");
    assert!(!v.is_stale());
}

#[test]
fn slow_but_moving_is_working_rather_than_stale() {
    // Thirty pips of the forty, in forty bars. Late, and going the right way.
    // A rule that cuts this is a rule that cuts winners, which never shows up
    // in a record as a loss — which is exactly why it has to be got right.
    let bars = hourly(40, |i| {
        let up = 1.1000 + (i as f64 * 0.000_08).min(0.0030);
        (up, up - 0.0002)
    });
    let view = bars.latest().unwrap();
    let v = how_its_going(&long_trade(), &view, Some(0.0010), &StaleConfig::default()).unwrap();
    match v {
        Going::Working { best, .. } => assert!(best > 0.25, "best {best:.2}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_best_it_has_been_is_what_counts_not_where_it_sits_now() {
    // A trade that reached most of its target and came back is a different
    // animal from one that never moved, and only one of them is stale.
    let bars = hourly(40, |i| {
        if (10..14).contains(&i) {
            (1.1035, 1.1025)
        } else {
            (1.1001, 1.0999)
        }
    });
    let view = bars.latest().unwrap();
    let v = how_its_going(&long_trade(), &view, Some(0.0010), &StaleConfig::default()).unwrap();
    assert!(!v.is_stale(), "a trade that got 87% of the way was called stale: {v:?}");
}

// ---------------------------------------------------------------------------
// Shorts, and the awkward inputs
// ---------------------------------------------------------------------------

#[test]
fn a_short_is_read_the_same_way_upside_down() {
    let short = Open { side: Side::Sell, entry: 1.1000, stop: 1.1020, target: 1.0960, opened_at: START };
    let nowhere = hourly(40, |_| (1.1001, 1.0999));
    let cfg = StaleConfig::default();
    assert!(how_its_going(&short, &nowhere.latest().unwrap(), Some(0.0010), &cfg)
        .unwrap()
        .is_stale());
    let good = hourly(40, |i| if i < 3 { (1.1001, 1.0999) } else { (1.1001, 1.0950) });
    assert_eq!(
        how_its_going(&short, &good.latest().unwrap(), Some(0.0010), &cfg).unwrap(),
        Going::Reached
    );
}

#[test]
fn a_window_that_starts_after_the_trade_did_is_refused_rather_than_flattered() {
    // Reading progress off a window that starts mid-trade reports the best of
    // the last few bars as the best of the trade.
    let bars = hourly(40, |_| (1.1001, 1.0999));
    let view = bars.latest().unwrap();
    let late = Open { opened_at: START - 10 * MS_PER_HOUR, ..long_trade() };
    let e = how_its_going(&late, &view, Some(0.0010), &StaleConfig::default()).unwrap_err();
    assert!(e.contains("flatters"));
}

#[test]
fn bars_with_no_times_cannot_answer_this_at_all() {
    let closes: Vec<f64> = (0..40).map(|_| 1.1000).collect();
    let bars = Bars::new(
        closes.clone(),
        closes.iter().map(|c| c + 1e-4).collect(),
        closes.iter().map(|c| c - 1e-4).collect(),
        closes,
        Vec::new(),
    )
    .unwrap();
    let view = bars.latest().unwrap();
    assert!(how_its_going(&long_trade(), &view, Some(0.0010), &StaleConfig::default()).is_err());
}

#[test]
fn the_two_judgements_are_reachable_and_named_as_judgements() {
    let cfg = StaleConfig::default();
    assert_eq!(cfg.patience, 3.0);
    assert_eq!(cfg.progress_floor, 0.25);
    let src = include_str!("../src/stale.rs");
    assert_eq!(
        src.matches("**Chosen, not measured.**").count(),
        2,
        "a tunable stopped saying where its number came from"
    );
    // And patience is a dial, not a law: a caller that wants a longer leash
    // gets one, and the same fixture stops being stale.
    let bars = hourly(40, |_| (1.1001, 1.0999));
    let view = bars.latest().unwrap();
    let patient = StaleConfig { patience: 50.0, ..cfg };
    assert!(!how_its_going(&long_trade(), &view, Some(0.0010), &patient).unwrap().is_stale());
}

// ---------------------------------------------------------------------------
// Not a stop move
// ---------------------------------------------------------------------------

#[test]
fn nothing_here_can_be_mistaken_for_moving_a_stop() {
    // A time exit is "the trade is nothing". A stop is "the trade is wrong".
    // Letting the first travel as the second would make a time exit look like
    // a stop being moved, which it is not.
    let src = include_str!("../src/stale.rs");
    let fields = "pub stop: f64";
    assert_eq!(src.matches(fields).count(), 1, "the stop is read, never written");
}
