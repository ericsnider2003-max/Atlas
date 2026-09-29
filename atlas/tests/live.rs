//! The bar that has not closed yet.
//!
//! Most of the old file went with the module. It used to check that reading
//! bars one at a time gave the same answer as reading them all at once —
//! which `AsOf` now makes unrepresentable rather than merely wrong, so the
//! check could no longer fail and was removed rather than left to pass
//! forever.
//!
//! What is left tests the one thing their `Bars` does not model.

use atlas::live::{Firmness, Forming, Live};
use atlas::market::structure::Trend;

/// Enough closed bars to read structure over, stepping up.
fn primed(n: usize, step: f64) -> Live {
    let mut live = Live::new("EURUSD", 500, 40, 2);
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    let mut p = 1.1000;
    for i in 0..n {
        o.push(p);
        h.push(p + step.abs());
        l.push(p - step.abs() * 0.5);
        c.push(p + step);
        t.push(1_757_000_000_000 + i as i64 * 900_000);
        p += step;
    }
    live.prime(&o, &h, &l, &c, &t).unwrap();
    live
}

#[test]
fn a_forming_candle_extends_its_own_high_and_low() {
    // A candle that is still being drawn has a real high and low, and they are
    // where price has actually been rather than where it is now. A forming bar
    // that only tracked the last price would report a range of nothing and
    // every magnitude taken from it would be wrong.
    let mut f = Forming::open_at(1.1000, 0);
    f.tick(1.1020);
    f.tick(1.0990);
    f.tick(1.1005);
    assert_eq!(f.open, 1.1000);
    assert!((f.high - 1.1020).abs() < 1e-9);
    assert!((f.low - 1.0990).abs() < 1e-9);
    assert!((f.at - 1.1005).abs() < 1e-9, "where it is now, not where it closed");
}

#[test]
fn nonsense_prices_do_not_move_a_forming_candle() {
    let mut f = Forming::open_at(1.1000, 0);
    f.tick(f64::NAN);
    f.tick(-1.0);
    f.tick(0.0);
    assert!((f.high - 1.1000).abs() < 1e-9);
    assert!((f.low - 1.1000).abs() < 1e-9);
}

#[test]
fn with_nothing_forming_there_is_one_reading_and_it_says_so() {
    let live = primed(120, 0.0004);
    let now = live.now().unwrap();
    assert!(!live.is_forming());
    assert_eq!(now.forming, None, "not a repeat of the settled reading");
    assert!(!now.only_if_it_closes_here);
    assert_eq!(now.at(Firmness::Settled), now.settled);
    assert_eq!(now.at(Firmness::Forming), now.settled, "nothing to add");
}

#[test]
fn a_bar_closing_clears_the_candle_that_was_drawing_it() {
    // Leaving it would count the same price action twice — once as the bar
    // that closed and again as the candle that had been drawing it.
    let mut live = primed(120, 0.0004);
    live.tick(1.1500, 1_757_000_000_000);
    assert!(live.is_forming());
    let before = live.settled_bars();
    live.closed(1.1490, 1.1510, 1.1480, 1.1500, 1_757_000_900_000);
    assert!(!live.is_forming(), "the forming candle became the bar");
    assert_eq!(live.settled_bars(), before + 1);
}

#[test]
fn the_two_readings_are_named_apart_when_they_differ() {
    // The whole point of the module. "The structure has turned" and "the
    // structure will have turned if this candle closes here" are different
    // sentences, and only one of them is a fact.
    let mut live = primed(120, 0.0004); // stepping up
    let settled_before = live.now().unwrap().settled;

    // A candle that collapses far below everything behind it.
    live.tick(1.0500, 1_757_000_000_000);
    let now = live.now().unwrap();

    assert_eq!(now.settled, settled_before, "the closed bars did not change");
    if now.forming != Some(now.settled) && now.forming.is_some() {
        assert!(now.only_if_it_closes_here);
        assert!(now.spoken().contains("isn't a fact yet"), "{}", now.spoken());
        assert_ne!(
            now.at(Firmness::Settled),
            now.at(Firmness::Forming),
            "the caller chooses which one, and neither is substituted for the other"
        );
    }
}

#[test]
fn asking_for_the_settled_reading_gets_the_settled_reading() {
    // Never silently upgraded to the forming one, however dramatic the candle.
    let mut live = primed(120, 0.0004);
    let settled = live.now().unwrap().settled;
    live.tick(0.9000, 1_757_000_000_000);
    let now = live.now().unwrap();
    assert_eq!(now.at(Firmness::Settled), settled);
}

#[test]
fn it_refuses_to_speak_before_it_has_a_window() {
    // Saying something now would be saying it about a window that isn't there.
    let thin = primed(10, 0.0004);
    let not_yet = thin.not_ready().unwrap();
    assert!(not_yet.contains("10 closed bars"), "{not_yet}");
    assert!(not_yet.contains("EURUSD"), "{not_yet}");

    let enough = primed(120, 0.0004);
    assert_eq!(enough.not_ready(), None);
}

#[test]
fn priming_goes_through_the_same_door_as_everything_else() {
    // A second, laxer path into the same data is how one of them ends up being
    // the one that is actually used.
    let mut live = Live::new("EURUSD", 500, 40, 2);
    let ragged = live.prime(&[1.0, 2.0], &[1.0], &[1.0], &[1.0], &[]);
    assert!(ragged.is_err(), "ragged columns are refused here too");
    assert_eq!(live.settled_bars(), 0, "and nothing was kept from the attempt");

    let empty = live.prime(&[], &[], &[], &[], &[]);
    assert!(empty.is_err());
}

#[test]
fn it_keeps_only_what_it_was_told_to() {
    // A live reader that grows without limit is one that eventually stops.
    let mut live = Live::new("EURUSD", 60, 40, 2);
    let n = 300;
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    let mut p = 1.1000;
    for i in 0..n {
        o.push(p);
        h.push(p + 0.0004);
        l.push(p - 0.0002);
        c.push(p + 0.0004);
        t.push(1_757_000_000_000 + i as i64 * 900_000);
        p += 0.0004;
    }
    live.prime(&o, &h, &l, &c, &t).unwrap();
    assert!(live.settled_bars() <= 60.max(60), "{}", live.settled_bars());
    assert!(live.now().is_ok(), "and it can still read what it kept");
}

#[test]
fn the_forming_series_never_leaves_the_module() {
    // The bars-with-the-forming-candle are a hypothetical, and the type cannot
    // say so — they come back as an ordinary `Bars`, indistinguishable from
    // real history. So nothing outside hands one out: `now()` reads it and
    // returns a `Trend`.
    let source = include_str!("../src/live.rs");
    assert!(
        source.contains("fn as_though_closed"),
        "the hypothetical series is built somewhere"
    );
    assert!(
        !source.contains("pub fn as_though_closed"),
        "and it is not public — a caller holding one could not tell it from history"
    );
}

#[test]
fn a_trend_reads_as_something_sayable() {
    let live = primed(120, 0.0004);
    let now = live.now().unwrap();
    assert!(!now.settled.say().is_empty());
    assert!(now.spoken().contains("closed bars"), "{}", now.spoken());
    assert!(matches!(now.settled, Trend::Up | Trend::Down | Trend::Range | Trend::Unknown));
}
