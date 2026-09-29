//! Calendar arithmetic shared by `recur` and `cronspec`.
//!
//! Howard Hinnant's days-from-civil / civil-from-days (public domain, see
//! <https://howardhinnant.github.io/date_algorithms.html>). Written out here
//! rather than taken from `triage.rs` or `digest.rs` so this crate builds
//! alone; the merge should collapse the three copies into one.
//!
//! Every time in this crate is **local seconds**: unix seconds plus the
//! caller's UTC offset. That is honest about one limit — a fixed offset does
//! not know about daylight-saving changes. See `HANDOFF.md`, gap G1.

/// Days since 1970-01-01 for a proleptic Gregorian date.
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m as i64 + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// (year, month 1-12, day 1-31) for days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 0 = Monday … 6 = Sunday (ISO order, which is what RRULE's WKST assumes).
pub fn weekday(days: i64) -> u32 {
    // 1970-01-01 was a Thursday (ISO 3).
    ((days + 3).rem_euclid(7)) as u32
}

pub fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

pub fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        _ => 28,
    }
}

/// A broken-down local time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Civil {
    pub year: i64,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
    pub second: u32,
}

impl Civil {
    pub fn from_local(secs: i64) -> Civil {
        let days = secs.div_euclid(86_400);
        let rem = secs.rem_euclid(86_400) as u32;
        let (year, month, day) = civil_from_days(days);
        Civil { year, month, day, hour: rem / 3600, minute: rem % 3600 / 60, second: rem % 60 }
    }
    pub fn to_local(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * 86_400
            + (self.hour * 3600 + self.minute * 60 + self.second) as i64
    }
    pub fn days(&self) -> i64 {
        days_from_civil(self.year, self.month, self.day)
    }
    pub fn weekday(&self) -> u32 {
        weekday(self.days())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn round_trips_across_four_centuries() {
        for z in (-200_000..200_000).step_by(37) {
            let (y, m, d) = civil_from_days(z);
            assert_eq!(days_from_civil(y, m, d), z);
        }
    }
    #[test]
    fn known_dates() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(weekday(0), 3); // Thursday
        // 2026-09-23 is a Wednesday.
        assert_eq!(weekday(days_from_civil(2026, 9, 23)), 2);
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2100, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
    }
}
