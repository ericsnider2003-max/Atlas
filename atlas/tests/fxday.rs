//! Where the trading day ends: 17:00 New York.
//!
//! Every instant asserted here came out of `docs/fxday_reference.py`, which was
//! run. None of it is a value I expected and wrote down — and one of them is a
//! value that contradicted what I expected, which is the only reason it is
//! right.

use atlas::fxday::{close_on, day_of, day_start, days, levels, prior_day, prior_week, spoken,
                   week_of, week_start, DayShape, CLOSE_HOUR};
use atlas::market::bars::Bars;
use atlas::market::time::{days_from_civil, MS_PER_DAY, MS_PER_HOUR};

fn at(y: i32, m: u32, d: u32, hour: i64) -> i64 {
    days_from_civil(y, m, d) * MS_PER_DAY + hour * MS_PER_HOUR
}

// ---------------------------------------------------------------------------
// The boundary itself
// ---------------------------------------------------------------------------

#[test]
fn the_boundary_follows_new_yorks_clock_and_not_a_fixed_utc_hour() {
    // Hard-coding either one puts every prior-day high and low an hour out for
    // roughly half the year, silently — the levels stay plausible and stay
    // near price, they are just about a different day.
    assert_eq!(close_on(2026, 1, 15), at(2026, 1, 15, 22), "winter: EST, UTC-5");
    assert_eq!(close_on(2026, 7, 15), at(2026, 7, 15, 21), "summer: EDT, UTC-4");
    assert_eq!(CLOSE_HOUR, 17);
}

#[test]
fn it_moves_on_the_right_day_at_both_ends_of_the_year() {
    // 2026: spring forward is Sunday 8 March, fall back is Sunday 1 November.
    assert_eq!(close_on(2026, 3, 7), at(2026, 3, 7, 22), "the Saturday before is still EST");
    assert_eq!(close_on(2026, 3, 9), at(2026, 3, 9, 21), "the Monday after is EDT");
    assert_eq!(close_on(2026, 10, 30), at(2026, 10, 30, 21), "the Friday before is still EDT");
    assert_eq!(close_on(2026, 11, 2), at(2026, 11, 2, 22), "the Monday after is EST");
}

#[test]
fn a_moment_lands_in_the_day_that_contains_it() {
    // Summer, so the boundary is 21:00 UTC.
    let (opens, closes) = day_of(at(2026, 7, 15, 12));
    assert_eq!(opens, at(2026, 7, 14, 21));
    assert_eq!(closes, at(2026, 7, 15, 21));

    // One minute after a close belongs to the NEXT day, not the one that just
    // ended. Off by one here shifts every level by a day.
    let (opens, _) = day_of(at(2026, 7, 15, 21) + 60_000);
    assert_eq!(opens, at(2026, 7, 15, 21));

    // And the close instant itself opens the new day.
    assert_eq!(day_start(at(2026, 7, 15, 21)), at(2026, 7, 15, 21));
}

#[test]
fn every_trading_day_is_exactly_twenty_four_hours() {
    // I assumed one would be twenty-three, on the weekend the clocks go
    // forward. It never is — the change happens at 02:00 on a Sunday and the
    // market is shut from 17:00 Friday to 17:00 Sunday, so it always lands
    // inside the weekend. The reference run swept 2024-2027 and said so; this
    // pins it.
    let week = week_start(at(2026, 3, 11, 12)); // the spring-forward week
    let mut cursor = week;
    for day in 0..5 {
        let (opens, closes) = day_of(cursor + 1);
        assert_eq!(
            closes - opens,
            MS_PER_DAY,
            "day {day} of the spring-forward week is not 24 hours"
        );
        cursor = closes;
    }

    let week = week_start(at(2026, 11, 4, 12)); // the fall-back week
    let mut cursor = week;
    for day in 0..5 {
        let (opens, closes) = day_of(cursor + 1);
        assert_eq!(closes - opens, MS_PER_DAY, "day {day} of the fall-back week");
        cursor = closes;
    }
}

#[test]
fn the_week_opens_sunday_evening_and_runs_five_days() {
    let (opens, closes) = week_of(at(2026, 7, 15, 12));
    assert_eq!(opens, at(2026, 7, 12, 21), "Sunday 17:00 New York");
    assert_eq!(closes, at(2026, 7, 17, 21), "Friday 17:00 New York");
    assert_eq!(closes - opens, 5 * MS_PER_DAY);
}

#[test]
fn the_weekend_is_forty_seven_or_forty_nine_hours_twice_a_year() {
    // What actually moves when the clocks do. It matters for anything deciding
    // whether a gap in the bars is a weekend or a hole in the feed.
    let ordinary = week_start(at(2026, 7, 15, 12)) - close_on(2026, 7, 10);
    assert_eq!(ordinary / MS_PER_HOUR, 48);

    let spring = week_start(at(2026, 3, 11, 12)) - close_on(2026, 3, 6);
    assert_eq!(spring / MS_PER_HOUR, 47, "an hour short when the clocks go forward");

    let autumn = week_start(at(2026, 11, 4, 12)) - close_on(2026, 10, 30);
    assert_eq!(autumn / MS_PER_HOUR, 49, "an hour long when they go back");
}

#[test]
fn midnight_utc_would_have_split_the_opening_session() {
    // The reason for the whole convention. A midnight-UTC reader turns the
    // Sunday evening session into a three-hour stub and a short Monday — six
    // candles a week where every broker chart shows five.
    let opens = week_start(at(2026, 7, 15, 12));
    let midnight_after = (opens / MS_PER_DAY + 1) * MS_PER_DAY;
    assert_eq!((midnight_after - opens) / MS_PER_HOUR, 3);
}

// ---------------------------------------------------------------------------
// Days made of bars
// ---------------------------------------------------------------------------

/// Hourly bars across `hours`, starting at `from`, each one a small range.
fn hourly(from: i64, hours: usize) -> Bars {
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    let mut p = 1.1000;
    for i in 0..hours {
        o.push(p);
        h.push(p + 0.0008);
        l.push(p - 0.0006);
        c.push(p + 0.0002);
        t.push(from + i as i64 * MS_PER_HOUR);
        p += 0.0002;
    }
    Bars::new(o, h, l, c, t).unwrap()
}

#[test]
fn bars_group_into_the_days_they_fall_in() {
    // Start at the Monday open, summer, and run three days.
    let start = at(2026, 7, 13, 21);
    let bars = hourly(start, 72);
    let view = bars.latest().unwrap();
    let found = days(&view);
    assert_eq!(found.len(), 3, "{:#?}", found.iter().map(|d| d.bars).collect::<Vec<_>>());
    for d in &found[..2] {
        assert_eq!(d.bars, 24, "a full day is twenty-four hourly bars");
    }
}

#[test]
fn the_day_in_progress_is_never_the_prior_day() {
    // A high that can still move is not a level. `prior_day` returns the last
    // COMPLETED day, and with only one day of bars there is no such thing.
    let one_day = hourly(at(2026, 7, 13, 21), 20);
    let view = one_day.latest().unwrap();
    assert_eq!(days(&view).len(), 1);
    assert_eq!(prior_day(&view), None, "the day still running is not a prior day");
    assert!(spoken(&view).contains("haven't got a completed day"), "{}", spoken(&view));

    let two_days = hourly(at(2026, 7, 13, 21), 30);
    let view = two_days.latest().unwrap();
    let prior = prior_day(&view).unwrap();
    assert_eq!(prior.bars, 24);
    assert_eq!(prior.opens, at(2026, 7, 13, 21));
    assert_eq!(prior.closes, at(2026, 7, 14, 21));
}

#[test]
fn a_days_high_and_low_are_the_extremes_of_its_bars() {
    let bars = hourly(at(2026, 7, 13, 21), 30);
    let view = bars.latest().unwrap();
    let d = prior_day(&view).unwrap();
    let (h, l) = (view.high(), view.low());
    let want_high = h[..24].iter().copied().fold(f64::MIN, f64::max);
    let want_low = l[..24].iter().copied().fold(f64::MAX, f64::min);
    assert!((d.high - want_high).abs() < 1e-12);
    assert!((d.low - want_low).abs() < 1e-12);
    assert!(d.range() > 0.0);
    assert_eq!(d.position_of(d.low), Some(0.0));
    assert_eq!(d.position_of(d.high), Some(1.0));
}

#[test]
fn a_weekend_disappears_without_this_file_knowing_what_one_is() {
    // The calendar says where boundaries are; the bars say which days exist.
    // Friday's session, then nothing, then Sunday evening. Two days, not four.
    let mut o = vec![];
    let mut h = vec![];
    let mut l = vec![];
    let mut c = vec![];
    let mut t = vec![];
    // Friday: 17:00 Thu NY -> 17:00 Fri NY, so bars from 21:00 Thu.
    for i in 0..24i64 {
        let p = 1.1000 + i as f64 * 0.0001;
        o.push(p);
        h.push(p + 0.0005);
        l.push(p - 0.0005);
        c.push(p);
        t.push(at(2026, 7, 16, 21) + i * MS_PER_HOUR);
    }
    // Then Sunday evening onward — the whole weekend missing.
    for i in 0..6i64 {
        let p = 1.2000 + i as f64 * 0.0001;
        o.push(p);
        h.push(p + 0.0005);
        l.push(p - 0.0005);
        c.push(p);
        t.push(at(2026, 7, 19, 21) + i * MS_PER_HOUR);
    }
    let bars = Bars::new(o, h, l, c, t).unwrap();
    let view = bars.latest().unwrap();
    let found = days(&view);
    assert_eq!(found.len(), 2, "two days with bars, and the weekend is simply absent");
    assert_eq!(found[0].bars, 24);
    assert_eq!(found[1].bars, 6);
    // The prior day is Friday, not an empty Saturday.
    let prior = prior_day(&view).unwrap();
    assert_eq!(prior.bars, 24);
}

#[test]
fn a_short_day_is_reported_as_short_rather_than_treated_as_a_level() {
    // Three bars is a public holiday or a hole in the feed, and its high is not
    // a line anybody was watching.
    let mut o = vec![];
    let mut h = vec![];
    let mut l = vec![];
    let mut c = vec![];
    let mut t = vec![];
    for i in 0..3i64 {
        o.push(1.1);
        h.push(1.1005);
        l.push(1.0995);
        c.push(1.1);
        t.push(at(2026, 12, 24, 22) + i * MS_PER_HOUR);
    }
    for i in 0..5i64 {
        o.push(1.1);
        h.push(1.1005);
        l.push(1.0995);
        c.push(1.1);
        t.push(at(2026, 12, 25, 22) + i * MS_PER_HOUR);
    }
    let bars = Bars::new(o, h, l, c, t).unwrap();
    let view = bars.latest().unwrap();
    let prior = prior_day(&view).unwrap();
    assert_eq!(prior.bars, 3);
    let said = spoken(&view);
    assert!(said.contains("holiday or a hole"), "{said}");
}

#[test]
fn a_series_with_no_clock_gets_no_days_rather_than_invented_ones() {
    // A prior day worked out from bar counts would be a different day on every
    // timeframe, which is worse than having none.
    let closes: Vec<f64> = (0..200).map(|i| 1.1 + i as f64 * 0.0001).collect();
    let no_clock = Bars::from_closes(&closes, 0.0002).unwrap();
    let view = no_clock.latest().unwrap();
    assert!(days(&view).is_empty());
    assert_eq!(prior_day(&view), None);
    assert_eq!(prior_week(&view), None);
    assert!(levels(&view).is_empty());
}

// ---------------------------------------------------------------------------
// The levels themselves
// ---------------------------------------------------------------------------

#[test]
fn the_levels_are_named_and_sorted_by_how_near_they_are() {
    let bars = hourly(at(2026, 7, 13, 21), 30);
    let view = bars.latest().unwrap();
    let found = levels(&view);
    assert!(!found.is_empty());
    let names: Vec<&str> = found.iter().map(|(_, n)| *n).collect();
    assert!(names.contains(&"yesterday's high"), "{names:?}");
    assert!(names.contains(&"yesterday's low"), "{names:?}");
    assert!(names.contains(&"yesterday's close"), "{names:?}");

    let now = view.now();
    let mut last = 0.0f64;
    for (price, _) in &found {
        let d = (price - now).abs();
        assert!(d >= last - 1e-12, "levels are not sorted by distance: {found:?}");
        last = d;
    }
}

#[test]
fn a_prior_week_needs_a_completed_week() {
    let one = hourly(at(2026, 7, 13, 21), 100);
    let view = one.latest().unwrap();
    assert_eq!(prior_week(&view), None, "one week in progress is not a prior week");
}

#[test]
fn a_shape_with_no_range_has_no_position_in_it() {
    // A day that never moved is a day, and dividing by its range is not a
    // measurement. `None` rather than nought, which would read as "at the low".
    let flat = DayShape {
        opens: 0,
        closes: MS_PER_DAY,
        high: 1.1,
        low: 1.1,
        open: 1.1,
        close: 1.1,
        bars: 24,
    };
    assert_eq!(flat.range(), 0.0);
    assert_eq!(flat.position_of(1.1), None);
}
