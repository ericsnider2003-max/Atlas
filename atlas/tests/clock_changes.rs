//! A time on another day is read on the clock in force that day, not today's
//! (5 Oct 2026 audit, Q15).
//!
//! `localclock::offset_secs()` is the offset *now*. It was also used for
//! dates on the other side of a clock change -- a task "due on the 10th"
//! entered in October, "which day was this", "when did this start" -- and
//! came out an hour off. New York ends summer time on Sunday 1 November 2026.

use atlas::localclock::{midnight_in, utc_of_wall_in};

fn utc(y: i64, m: u32, d: u32, h: u32) -> u64 {
    (atlas::civil::days_from_civil(y, m, d) * 86_400 + h as i64 * 3600) as u64
}

fn new_york() -> atlas::tz::Zone {
    atlas::tz::Zone::named("America/New_York").expect("a zone Atlas knows")
}

#[test]
fn five_pm_on_a_day_after_the_change_is_five_pm_then() {
    let ny = new_york();
    let day = atlas::civil::days_from_civil(2026, 11, 10);
    assert_eq!(utc_of_wall_in(day, 17 * 3600, &ny), utc(2026, 11, 10, 22), "17:00 EST is 22:00 UTC");
    // What it was: today's (October, EDT) offset applied to November.
    let october_offset = ny.offset_at(utc(2026, 10, 5, 12) as i64);
    let old = (day * 86_400 + 17 * 3600 - october_offset) as u64;
    assert_eq!(old, utc(2026, 11, 10, 21), "the old reading was 16:00 on the wall");
}

#[test]
fn the_day_the_clocks_go_back_starts_and_ends_at_local_midnight() {
    let ny = new_york();
    let afternoon = utc(2026, 11, 1, 20);
    let start = midnight_in(afternoon, &ny);
    assert_eq!(start, utc(2026, 11, 1, 4), "midnight EDT");
    let end = midnight_in(utc(2026, 11, 2, 20), &ny);
    assert_eq!(end, utc(2026, 11, 2, 5), "midnight EST");
    assert_eq!(end - start, 25 * 3600, "a 25-hour day");
}

#[test]
fn times_on_other_days_are_not_read_with_todays_offset() {
    // The places that read a date that isn't now: a task's due day, the day a
    // mail or a work span falls on, the time a step started.
    for (name, bad) in [
        ("workspace_view", "midnight(day, crate::localclock::offset_secs())"),
        ("workspace_view", "start + 86_400"),
        ("daemon", "midnight(created, crate::localclock::offset_secs())"),
        ("daemon", "localclock::midnight(s.start, off)"),
        ("daemon", "localclock::hhmm(at, off)"),
        ("daily", "midnight(shifted, crate::localclock::offset_secs())"),
        ("hublive", "17 * 3600 - crate::localclock::offset_secs()"),
        ("hublive", "localclock::day(d, off)"),
        ("hublive", "localclock::hhmm(e.at, off)"),
        ("hublive", "localclock::hhmm(w.started, off)"),
    ] {
        let src = crate::common::source_of(name);
        assert!(!src.contains(bad), "{name}: `{bad}` reads another day with today's offset");
    }
}
