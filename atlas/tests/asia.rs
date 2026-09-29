//! The overnight range.
//!
//! The boundary is not asserted from a clock constant. `market::session` is
//! the thing that knows which desks are awake, and these tests check that this
//! module asks it rather than keeping its own copy of the answer — which is
//! the version that goes wrong twice a year and looks fine in between.

use atlas::asia::{against_a_normal_day, last_night, levels, overnight, spoken};
use atlas::market::bars::Bars;
use atlas::market::session::{is_open, Centre};
use atlas::market::time::{MS_PER_DAY, MS_PER_HOUR};

/// Monday 8 June 2026, 00:00 UTC. A week with nothing unusual in it.
const MONDAY: i64 = 1_780_876_800_000;

/// Hourly bars from `from`, with the high and low of each bar set by `shape`.
fn hourly(from: i64, hours: usize, shape: impl Fn(usize) -> (f64, f64)) -> Bars {
    let (mut o, mut h, mut l, mut c, mut t) = (vec![], vec![], vec![], vec![], vec![]);
    for i in 0..hours {
        let (hi, lo) = shape(i);
        let mid = (hi + lo) / 2.0;
        o.push(mid);
        h.push(hi);
        l.push(lo);
        c.push(mid);
        t.push(from + i as i64 * MS_PER_HOUR);
    }
    Bars::new(o, h, l, c, t).expect("bars")
}

// ---------------------------------------------------------------------------
// The boundary
// ---------------------------------------------------------------------------

#[test]
fn overnight_means_asia_awake_and_london_shut_and_asks_session_to_find_out() {
    // Checked against `session` itself rather than against hours written down
    // here. A second copy of the answer is the thing that drifts in March.
    let mut nights = 0;
    for hour in 0..(24 * 7) {
        let at = MONDAY + hour * MS_PER_HOUR;
        let asia = is_open(Centre::Tokyo, at) || is_open(Centre::Sydney, at);
        let london = is_open(Centre::London, at);
        assert_eq!(overnight(at), asia && !london, "hour {hour}");
        if overnight(at) {
            nights += 1;
        }
    }
    assert!(nights > 0, "a week with no overnight hours in it is not a week");
}

#[test]
fn the_boundary_moves_with_the_clocks_rather_than_staying_put_in_utc() {
    // London's change is the last Sunday in March; Tokyo never moves. So the
    // UTC hour at which the overnight session ends is not the same in January
    // and July — and a module holding its own constant would have it wrong for
    // seven months of the year while looking perfectly reasonable.
    let end_of_night = |day_start: i64| -> Option<i64> {
        (0..24).find(|h| !overnight(day_start + h * MS_PER_HOUR) && overnight(day_start + (h - 1).max(0) * MS_PER_HOUR))
    };
    // Wednesday 14 January 2026 and Wednesday 15 July 2026, both 00:00 UTC.
    let winter = end_of_night(1_768_348_800_000);
    let summer = end_of_night(1_784_246_400_000);
    assert!(winter.is_some() && summer.is_some());
    assert_ne!(winter, summer, "the overnight session ends at the same UTC hour all year");
}

// ---------------------------------------------------------------------------
// The range
// ---------------------------------------------------------------------------

/// The bar indices of the last completed overnight run in a series of `hours`
/// hourly bars starting at `from`.
///
/// Worked out here rather than written down, because the answer moves: the
/// session boundary follows London's clock, and `is_open` also knows the
/// market is shut at the weekend — which is what made the first version of
/// this fixture box a Friday night and then read a Saturday.
fn last_completed_run(from: i64, hours: usize) -> (usize, usize) {
    let at = |i: usize| from + i as i64 * MS_PER_HOUR;
    let mut end = hours;
    while end > 0 && overnight(at(end - 1)) {
        end -= 1; // skip a session still in progress
    }
    while end > 0 && !overnight(at(end - 1)) {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && overnight(at(start - 1)) {
        start -= 1;
    }
    (start, end)
}

#[test]
fn a_completed_night_gives_its_high_and_its_low() {
    // Six days of hourly bars, flat at 1.1000 except the last completed
    // overnight run, which is given a known 20-pip box. Building it this way
    // means the answer is not a number I read off a run — it is the box.
    let hours = 24 * 6;
    let start = MONDAY;
    let (from, to) = last_completed_run(start, hours);
    assert!(to > from, "no completed overnight session in six days of bars");
    let bars = hourly(start, hours, |i| {
        if i >= from && i < to {
            (1.1010, 1.0990)
        } else {
            (1.1001, 1.0999)
        }
    });
    let view = bars.latest().unwrap();
    let night = last_night(&view).expect("a night in six days of bars");
    assert!(!night.still_forming, "the last bar of this series is not overnight");
    assert!((night.high - 1.1010).abs() < 1e-9, "high {:.5}", night.high);
    assert!((night.low - 1.0990).abs() < 1e-9, "low {:.5}", night.low);
    assert!(night.bars >= 4, "only {} bars", night.bars);
}

#[test]
fn a_night_still_in_progress_offers_no_levels_at_all() {
    // The rule `fxday::prior_day` follows, for the same reason: both ends can
    // still move, so neither is a line anybody is defending yet.
    let start = MONDAY;
    // Walk forward to a bar that IS overnight and end the series there.
    let end = (1..(24 * 6)).find(|h| overnight(start + h * MS_PER_HOUR)).unwrap();
    let bars = hourly(start, end as usize + 1, |_| (1.1010, 1.0990));
    let view = bars.latest().unwrap();
    let night = last_night(&view).expect("the session in progress");
    assert!(night.still_forming);
    assert!(levels(&view).is_empty(), "a range that can still move was offered as a level");
    assert!(spoken(&view).contains("still trading"));
}

#[test]
fn bars_with_no_times_get_nothing_rather_than_a_guess() {
    let closes: Vec<f64> = (0..100).map(|i| 1.1 + (i % 7) as f64 * 1e-4).collect();
    let bars = Bars::new(
        closes.clone(),
        closes.iter().map(|c| c + 1e-4).collect(),
        closes.iter().map(|c| c - 1e-4).collect(),
        closes,
        Vec::new(),
    )
    .unwrap();
    let view = bars.latest().unwrap();
    assert_eq!(last_night(&view), None);
    assert!(levels(&view).is_empty());
    assert!(spoken(&view).contains("no times"));
}

// ---------------------------------------------------------------------------
// Tight or wide, measured rather than chosen
// ---------------------------------------------------------------------------

#[test]
fn how_wide_the_night_was_is_a_fraction_of_this_markets_own_days() {
    // The whole reason there is no pip threshold in the module. Twenty pips is
    // a coiled spring on EUR/CHF and a quiet hour on GBP/JPY, and one constant
    // cannot be both.
    let start = MONDAY - 10 * MS_PER_DAY;
    let hours = 24 * 16;
    // Days of roughly 100 pips; nights of roughly 20.
    let bars = hourly(start, hours, |i| {
        let at = start + i as i64 * MS_PER_HOUR;
        if overnight(at) {
            (1.1010, 1.0990)
        } else {
            (1.1050, 1.0950)
        }
    });
    let view = bars.latest().unwrap();
    let share = against_a_normal_day(&view).expect("enough complete days to compare against");
    assert!(share > 0.0 && share < 0.5, "night was {share:.2} of a day");
    assert!(spoken(&view).contains("of a normal day"));
}

#[test]
fn too_little_history_to_compare_against_says_so_instead_of_guessing() {
    // One night and no completed day behind it. The honest answer is that it
    // cannot be called tight or wide, not a number with nothing under it.
    let start = MONDAY;
    let end = (1..48).find(|h| !overnight(start + h * MS_PER_HOUR) && overnight(start + (h - 1) * MS_PER_HOUR));
    let Some(end) = end else { return };
    let bars = hourly(start, end as usize + 1, |_| (1.1010, 1.0990));
    let view = bars.latest().unwrap();
    assert_eq!(against_a_normal_day(&view), None);
    assert!(spoken(&view).contains("not going to guess"));
}

// ---------------------------------------------------------------------------
// Wired
// ---------------------------------------------------------------------------

#[test]
fn the_overnight_lines_reach_the_place_targets_are_chosen() {
    // The check that stops this being another correct, complete, uncalled
    // module. `levels::propose` has to be able to name one of these lines as
    // the thing it is aiming at.
    let start = MONDAY - 10 * MS_PER_DAY;
    let hours = 24 * 16;
    let bars = hourly(start, hours, |i| {
        let at = start + i as i64 * MS_PER_HOUR;
        if overnight(at) {
            (1.1010, 1.0990)
        } else {
            (1.1050, 1.0950)
        }
    });
    let view = bars.latest().unwrap();
    assert!(!levels(&view).is_empty(), "the fixture must end outside the overnight session");
    let wired = include_str!("../src/levels.rs");
    assert!(
        wired.contains("crate::asia::levels(view)"),
        "propose never asks for the overnight lines"
    );
}
