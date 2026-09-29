//! The market's own calendar: when New York is shut, when it shuts early,
//! and which scheduled releases land on a day -- said as a schedule, never
//! as a view on what the market will do.
//!
//! **Sources:** NYSE's published holidays and early closes for 2026–2028
//! (nyse.com/trade/hours-calendars, read 25 Sep 2026) are the check the rules
//! below were written against; the rules are the NYSE's own (Rule 7.2): New
//! Year's Day (a Saturday New Year is not moved back into the old year),
//! Martin Luther King Jr. Day, Washington's Birthday, Good Friday, Memorial
//! Day, Juneteenth, Independence Day, Labor Day, Thanksgiving and Christmas,
//! each moved to the Friday before when it falls on a Saturday and the Monday
//! after on a Sunday; 1:00 pm closes on the day after Thanksgiving, on
//! Christmas Eve and on July 3 when those are ordinary weekdays. Good Friday
//! is Easter less two days, and Easter is the Gregorian computus (the
//! anonymous "Meeus/Jones/Butcher" algorithm, public domain). BLS's CPI
//! release dates for 2026 (bls.gov/schedule/news_release/cpi.htm, as carried
//! by two independent listings that agree date for date) are a hand-entered
//! table with an expiry -- the same rule `market::events` keeps: a missing
//! date makes the calendar silent, a wrong one makes it confidently wrong.
//! Central banks and payrolls come from `market::events`, which already holds
//! them to that rule.
//!
//! **What this is not.** It says when; it never says what to do about it. No
//! line here reads a market, and none is allowed to (`says_nothing_about_direction`).

use crate::market::events;
use crate::market::time::{days_from_civil, last_weekday, nth_weekday, Utc};

/// What kind of day it is on the New York exchanges.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Day {
    Open,
    /// Weekend.
    Weekend,
    /// A full holiday, named.
    Closed(&'static str),
    /// Closes at 1:00 pm New York time.
    EarlyClose(&'static str),
}

/// Checked against NYSE's own published list for these years; past them the
/// same rules still answer, and say they are unchecked.
pub const CHECKED_THROUGH: i32 = 2028;

/// Easter Sunday (Gregorian), as (month, day).
fn easter(y: i32) -> (u32, u32) {
    let a = y % 19;
    let b = y / 100;
    let c = y % 100;
    let d = b / 4;
    let e = b % 4;
    let f = (b + 8) / 25;
    let g = (b - f + 1) / 3;
    let h = (19 * a + b - d - g + 15) % 30;
    let i = c / 4;
    let k = c % 4;
    let l = (32 + 2 * e + 2 * i - h - k) % 7;
    let m = (a + 11 * h + 22 * l) / 451;
    let month = (h + l - 7 * m + 114) / 31;
    let day = (h + l - 7 * m + 114) % 31 + 1;
    (month as u32, day as u32)
}

fn weekday(y: i32, m: u32, d: u32) -> u32 {
    Utc::date(y, m, d).weekday()
}

/// A fixed-date holiday moved off the weekend: Saturday to Friday, Sunday
/// to Monday. Returns the (y, m, d) it is observed on.
fn observed(y: i32, m: u32, d: u32) -> (i32, u32, u32) {
    let days = days_from_civil(y, m, d);
    let shifted = match weekday(y, m, d) {
        5 => days - 1,
        6 => days + 1,
        _ => days,
    };
    let (yy, mm, dd) = crate::market::time::civil_from_days(shifted);
    (yy, mm, dd)
}

/// Every full NYSE holiday observed in year `y`.
pub fn holidays(y: i32) -> Vec<((i32, u32, u32), &'static str)> {
    let mut out = Vec::new();
    // New Year's Day: a Saturday New Year is simply not observed (NYSE does
    // not close on the last trading day of the old year).
    if weekday(y, 1, 1) != 5 {
        out.push((observed(y, 1, 1), "New Year's Day"));
    }
    out.push(((y, 1, nth_weekday(y, 1, 0, 3)), "Martin Luther King Jr. Day"));
    out.push(((y, 2, nth_weekday(y, 2, 0, 3)), "Washington's Birthday"));
    let (em, ed) = easter(y);
    let gf = crate::market::time::civil_from_days(days_from_civil(y, em, ed) - 2);
    out.push((gf, "Good Friday"));
    out.push(((y, 5, last_weekday(y, 5, 0)), "Memorial Day"));
    if y >= 2022 {
        out.push((observed(y, 6, 19), "Juneteenth"));
    }
    out.push((observed(y, 7, 4), "Independence Day"));
    out.push(((y, 9, nth_weekday(y, 9, 0, 1)), "Labor Day"));
    out.push(((y, 11, nth_weekday(y, 11, 3, 4)), "Thanksgiving"));
    out.push((observed(y, 12, 25), "Christmas"));
    // A holiday observed in another year (New Year's on a Sunday stays in the
    // year; the Saturday case is dropped above) never crosses, but keep the
    // list honest if a rule ever does.
    out.retain(|((yy, _, _), _)| *yy == y);
    out
}

/// What kind of day (y, m, d) is.
pub fn day(y: i32, m: u32, d: u32) -> Day {
    if weekday(y, m, d) >= 5 {
        return Day::Weekend;
    }
    if let Some((_, name)) = holidays(y).into_iter().find(|(date, _)| *date == (y, m, d)) {
        return Day::Closed(name);
    }
    let thanksgiving = nth_weekday(y, 11, 3, 4);
    if (m, d) == (11, thanksgiving + 1) {
        return Day::EarlyClose("the day after Thanksgiving");
    }
    if (m, d) == (12, 24) {
        return Day::EarlyClose("Christmas Eve");
    }
    // July 3 closes early when it is Monday to Thursday; when it's a Friday
    // it is the observed Independence Day and closed outright (above).
    if (m, d) == (7, 3) && weekday(y, 7, 3) <= 3 {
        return Day::EarlyClose("the day before Independence Day");
    }
    Day::Open
}

/// CPI release dates, 8:30 am New York: hand-entered, with an expiry.
const CPI: &[(i32, u32, u32)] = &[
    (2026, 1, 13), (2026, 2, 13), (2026, 3, 11), (2026, 4, 10), (2026, 5, 12), (2026, 6, 10),
    (2026, 7, 14), (2026, 8, 12), (2026, 9, 11), (2026, 10, 14), (2026, 11, 10), (2026, 12, 10),
];
/// The last date the CPI table covers. Past it, CPI is reported as a gap.
pub const CPI_GOOD_UNTIL: (i32, u32) = (2026, 12);

/// One thing on the market's calendar for a day.
#[derive(Debug, Clone, PartialEq)]
pub struct Mark {
    /// UTC seconds; for a whole-day mark, New York midnight.
    pub at: i64,
    pub what: String,
    pub whole_day: bool,
}

fn ny() -> crate::tz::Zone {
    crate::tz::Zone::named("America/New_York").unwrap_or_else(crate::tz::Zone::utc)
}

/// UTC seconds of a New York wall-clock time.
fn ny_at(y: i32, m: u32, d: u32, h: u32, min: u32) -> i64 {
    let local = days_from_civil(y, m, d) * 86_400 + (h * 3600 + min * 60) as i64;
    ny().to_utc(local)
}

/// What's on the market's calendar for the New York day (y, m, d): the
/// exchange's day, CPI, and the highest-impact releases and central-bank
/// decisions `market::events` holds (impact 3 only -- "most of a standard
/// economic calendar is noise", that module's own first finding). Plus the
/// gaps: a table that has run out is said, not skipped.
pub fn market_marks(y: i32, m: u32, d: u32) -> (Vec<Mark>, Vec<String>) {
    let mut out = Vec::new();
    let mut gaps = Vec::new();
    match day(y, m, d) {
        Day::Closed(name) => out.push(Mark { at: ny_at(y, m, d, 0, 0), what: format!("US stock markets closed ({name})"), whole_day: true }),
        Day::EarlyClose(why) => out.push(Mark { at: ny_at(y, m, d, 13, 0), what: format!("US stock markets close early, 1:00 pm New York ({why})"), whole_day: false }),
        _ => {}
    }
    if y > CHECKED_THROUGH {
        gaps.push(format!("NYSE's holidays for {y} are worked out by its rules, not checked against its published list"));
    }
    if (y, m) > CPI_GOOD_UNTIL {
        gaps.push(format!("CPI dates after {}-{:02} aren't in yet", CPI_GOOD_UNTIL.0, CPI_GOOD_UNTIL.1));
    } else if CPI.contains(&(y, m, d)) {
        out.push(Mark { at: ny_at(y, m, d, 8, 30), what: "US CPI release".into(), whole_day: false });
    }
    match events::month(y, m) {
        Ok(list) => {
            for e in list.into_iter().filter(|e| e.impact >= 3) {
                let at = e.at.div_euclid(1000);
                let local = ny().to_local(at);
                let day_of = crate::market::time::civil_from_days(local.div_euclid(86_400));
                if day_of == (y, m, d) {
                    out.push(Mark { at, what: e.name.clone(), whole_day: false });
                }
            }
        }
        Err(why) => gaps.push(why.0),
    }
    out.sort_by_key(|k| (!k.whole_day, k.at));
    (out, gaps)
}

/// The day's marks said on your clock: "US CPI release at 13:30 (8:30 New
/// York)". Whole-day marks carry no time.
pub fn marks_spoken(marks: &[Mark], home: &crate::tz::Zone) -> Vec<String> {
    marks
        .iter()
        .map(|k| {
            if k.whole_day {
                return k.what.clone();
            }
            let mine = home.to_local(k.at).rem_euclid(86_400);
            let theirs = ny().to_local(k.at).rem_euclid(86_400);
            if mine == theirs {
                format!("{} at {:02}:{:02}", k.what, mine / 3600, mine % 3600 / 60)
            } else {
                format!("{} at {:02}:{:02} ({}:{:02} New York)", k.what, mine / 3600, mine % 3600 / 60, theirs / 3600, theirs % 3600 / 60)
            }
        })
        .collect()
}

/// For the morning brief and "what's on the market today": today's marks and
/// tomorrow's, as sentences, on your clock. `now` is UTC seconds. Empty when
/// nothing is on; a gap is said once at the end.
pub fn today_and_tomorrow(now: i64, home: &crate::tz::Zone) -> Vec<String> {
    let local_ny = ny().to_local(now).div_euclid(86_400);
    let mut lines = Vec::new();
    let mut gaps: Vec<String> = Vec::new();
    for (k, label) in [(0, "Today"), (1, "Tomorrow")] {
        let (y, m, d) = crate::market::time::civil_from_days(local_ny + k);
        let (marks, g) = market_marks(y, m, d);
        for gap in g {
            if !gaps.contains(&gap) {
                gaps.push(gap);
            }
        }
        let said = marks_spoken(&marks, home);
        if !said.is_empty() {
            lines.push(format!("{label}: {}.", said.join("; ")));
        }
    }
    if !gaps.is_empty() {
        lines.push(format!("(Not covered: {}.)", gaps.join("; ")));
    }
    lines
}

/// The next time New York's stock market is closed a full day or closes
/// early, from UTC `now`, within a year: (date, kind).
pub fn next_closure(now: i64) -> Option<((i32, u32, u32), Day)> {
    let start = ny().to_local(now).div_euclid(86_400);
    (0..370).map(|k| crate::market::time::civil_from_days(start + k)).find_map(|(y, m, d)| match day(y, m, d) {
        k @ (Day::Closed(_) | Day::EarlyClose(_)) => Some(((y, m, d), k)),
        _ => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn easter_is_the_computus() {
        assert_eq!(easter(2026), (4, 5));
        assert_eq!(easter(2027), (3, 28));
        assert_eq!(easter(2028), (4, 16));
        assert_eq!(easter(2019), (4, 21));
    }

    #[test]
    fn a_saturday_new_year_is_not_moved_back() {
        assert!(!holidays(2028).iter().any(|(_, n)| *n == "New Year's Day"));
        assert_eq!(day(2027, 12, 31), Day::Open);
    }
}
