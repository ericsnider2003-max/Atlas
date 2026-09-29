//! The rollover: the worst minutes of the day to get filled in, and the one
//! night in five that costs three.

use atlas::fxday::close_on;
use atlas::market::time::{MS_PER_DAY, MS_PER_HOUR, MS_PER_MIN};
use atlas::rollover::{held_through, rollover_ending, thin, Thin, THIN_MINUTES};

/// 17:00 New York on a given New York date, which is what a rollover is.
fn roll(y: i32, m: u32, d: u32) -> i64 {
    close_on(y, m, d)
}

// June 2026: the 8th is a Monday, so the 10th is a Wednesday and the 12th a
// Friday. Checked below rather than trusted.
const MON: (i32, u32, u32) = (2026, 6, 8);
const TUE: (i32, u32, u32) = (2026, 6, 9);
const WED: (i32, u32, u32) = (2026, 6, 10);
const THU: (i32, u32, u32) = (2026, 6, 11);
const FRI: (i32, u32, u32) = (2026, 6, 12);
const SAT: (i32, u32, u32) = (2026, 6, 13);

fn weekday_of(ms: i64) -> i64 {
    (ms.div_euclid(MS_PER_DAY) + 4).rem_euclid(7)
}

#[test]
fn the_dates_these_tests_name_are_the_days_they_say_they_are() {
    // Every other test here leans on June 2026 being an ordinary week. If that
    // is wrong, everything below is wrong in a way that still looks green.
    for (want, (y, m, d)) in [(1, MON), (2, TUE), (3, WED), (4, THU), (5, FRI), (6, SAT)] {
        let noon = atlas::market::time::Utc::date(y, m, d).to_ms() + 12 * MS_PER_HOUR;
        assert_eq!(weekday_of(noon), want, "{y}-{m:02}-{d:02}");
    }
}

// ---------------------------------------------------------------------------
// Which nights are charged, and which are charged three times
// ---------------------------------------------------------------------------

#[test]
fn wednesdays_rollover_charges_three_nights_and_no_other_does() {
    // Value date rolls Friday to Monday, so one night on the chart is three
    // nights of interest. It happens every week and it lands on the one night
    // nobody remembers.
    for (day, triple) in [(MON, false), (TUE, false), (WED, true), (THU, false)] {
        let (y, m, d) = day;
        // An instant inside the FX day that CLOSES on this date: an hour
        // before its own 17:00.
        let inside = roll(y, m, d) - MS_PER_HOUR;
        let r = rollover_ending(inside).unwrap_or_else(|| panic!("no rollover ending {y}-{m}-{d}"));
        assert_eq!(r.at, roll(y, m, d), "{y}-{m:02}-{d:02}");
        assert_eq!(r.triple, triple, "{y}-{m:02}-{d:02}");
        assert_eq!(r.nights(), if triple { 3 } else { 1 });
    }
}

#[test]
fn fridays_close_and_the_weekend_have_no_rollover_to_be_held_through() {
    // Friday's 17:00 closes the week rather than rolling a position into
    // another session, and the weekend nights are the ones Wednesday paid for.
    let (y, m, d) = FRI;
    assert_eq!(rollover_ending(roll(y, m, d) - MS_PER_HOUR), None);
    let (y, m, d) = SAT;
    assert_eq!(rollover_ending(roll(y, m, d) - MS_PER_HOUR), None);
}

#[test]
fn a_trade_opened_wednesday_morning_is_a_different_trade_from_the_same_one_on_tuesday() {
    // The whole reason this module exists. Two identical setups, one night
    // each, and one of them costs three times as much to hold. Nothing on the
    // chart distinguishes them.
    let (y, m, d) = WED;
    let wednesday_morning = roll(y, m, d) - 8 * MS_PER_HOUR;
    let over_wednesday = held_through(wednesday_morning, roll(y, m, d) + MS_PER_HOUR);
    let (ty, tm, td) = TUE;
    let tuesday_morning = roll(ty, tm, td) - 8 * MS_PER_HOUR;
    let over_tuesday = held_through(tuesday_morning, roll(ty, tm, td) + MS_PER_HOUR);

    assert_eq!(over_wednesday.nights(), 1);
    assert_eq!(over_tuesday.nights(), 1);
    assert_eq!(over_wednesday.charged(), 3);
    assert_eq!(over_tuesday.charged(), 1);
    assert!(over_wednesday.crosses_a_triple() && !over_tuesday.crosses_a_triple());
    assert!(over_wednesday.say().contains("three nights"));
    // And the cost follows the charge, with the rate supplied rather than
    // invented — a swap rate is a broker fact that moves with policy rates.
    assert!((over_wednesday.cost(-2.5) - -7.5).abs() < 1e-9);
    assert!((over_tuesday.cost(-2.5) - -2.5).abs() < 1e-9);
}

#[test]
fn a_trade_closed_in_the_same_session_pays_nothing() {
    let (y, m, d) = TUE;
    let held = held_through(roll(y, m, d) - 6 * MS_PER_HOUR, roll(y, m, d) - MS_PER_HOUR);
    assert_eq!(held.nights(), 0);
    assert_eq!(held.charged(), 0);
    assert!(held.say().contains("no swap"));
}

#[test]
fn a_week_held_end_to_end_counts_four_nights_and_charges_six() {
    // Monday morning to Friday afternoon: Monday, Tuesday, Wednesday and
    // Thursday rollovers, with Wednesday's counting three.
    let (y, m, d) = MON;
    let (fy, fm, fd) = FRI;
    let held = held_through(roll(y, m, d) - 8 * MS_PER_HOUR, roll(fy, fm, fd) - MS_PER_HOUR);
    assert_eq!(held.nights(), 4, "{:?}", held.rollovers);
    assert_eq!(held.charged(), 6);
}

#[test]
fn nothing_is_counted_twice_and_a_backwards_span_counts_nothing() {
    let (y, m, d) = MON;
    let (fy, fm, fd) = FRI;
    let a = roll(y, m, d) - 8 * MS_PER_HOUR;
    let b = roll(fy, fm, fd) - MS_PER_HOUR;
    let held = held_through(a, b);
    let mut ats: Vec<i64> = held.rollovers.iter().map(|r| r.at).collect();
    let before = ats.len();
    ats.dedup();
    assert_eq!(ats.len(), before, "a rollover was counted twice");
    assert!(ats.windows(2).all(|w| w[0] < w[1]), "not in order");
    assert_eq!(held_through(b, a).nights(), 0);
}

// ---------------------------------------------------------------------------
// The thin book
// ---------------------------------------------------------------------------

#[test]
fn the_minutes_around_the_turn_are_named_as_thin_and_the_rest_of_the_day_is_not() {
    let (y, m, d) = TUE;
    let at = roll(y, m, d);
    for away in [-THIN_MINUTES + 1, -5, 0, 5, THIN_MINUTES - 1] {
        assert!(
            matches!(thin(at + away * MS_PER_MIN), Some(Thin::Rollover { .. })),
            "{away} minutes from the turn was not called thin"
        );
    }
    // Mid-afternoon London, with New York open too: the deepest book there is.
    let busy = at - 5 * MS_PER_HOUR;
    assert_eq!(thin(busy), None, "the London-New York overlap was called thin");
}

#[test]
fn the_thin_window_says_when_it_is_wednesdays_turn() {
    let (y, m, d) = WED;
    let at = roll(y, m, d);
    match thin(at - 2 * MS_PER_MIN) {
        Some(Thin::Rollover { triple, .. }) => assert!(triple),
        other => panic!("{other:?}"),
    }
    assert!(thin(at - 2 * MS_PER_MIN).unwrap().plain().contains("three nights"));
}

#[test]
fn an_hour_with_nobody_open_is_named_rather_than_passed_as_normal() {
    // Saturday. No desk anywhere, and whatever is quoted is a price nobody is
    // really making.
    let (y, m, d) = SAT;
    let noon = atlas::market::time::Utc::date(y, m, d).to_ms() + 12 * MS_PER_HOUR;
    assert_eq!(thin(noon), Some(Thin::NobodyOpen));
}

#[test]
fn the_window_width_is_declared_as_a_choice_and_not_dressed_up_as_a_finding() {
    // Atlas has no tick data and cannot measure a spread it never sees. The
    // number is a judgement; what matters is that the module says so.
    assert_eq!(THIN_MINUTES, 30);
    let src = include_str!("../src/rollover.rs");
    assert!(
        src.contains("**chosen** number, not a measured one"),
        "THIN_MINUTES stopped admitting it was chosen"
    );
}

// ---------------------------------------------------------------------------
// Wired
// ---------------------------------------------------------------------------

#[test]
fn proposing_a_trade_in_the_rollover_window_is_refused_as_a_bad_minute_not_a_bad_trade() {
    // And the difference is the useful part: standing down means not this
    // trade, a thin book means not this minute.
    let (y, m, d) = TUE;
    let at = roll(y, m, d) - 2 * MS_PER_MIN;
    let step = 4 * MS_PER_HOUR;
    let n = 200usize;
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    for i in 0..n {
        let px = 1.1000 + (i % 40) as f64 * 5e-4;
        o.push(px);
        h.push(px + 1e-3);
        l.push(px - 1e-3);
        c.push(px);
        t.push(at - ((n - 1 - i) as i64) * step);
    }
    let bars = atlas::market::bars::Bars::new(o, h, l, c, t).unwrap();
    let view = bars.latest().unwrap();
    let out = atlas::levels::propose(
        &view,
        "EURUSD",
        atlas::levels::Side::Buy,
        view.now(),
        0.000_15,
        &atlas::levels::Purse { balance: 10_000.0, value_per_point: 1.0 },
        &atlas::levels::Rules::default(),
    );
    assert!(matches!(out, Err(atlas::levels::NoTrade::ThinBook(_))), "got {out:?}");
    assert_eq!(out.unwrap_err().label(), "thin book");
}
