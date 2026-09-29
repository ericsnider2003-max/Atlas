//! Civil dates from a Unix timestamp, with no calendar crate.
//!
//! `chrono` and `time` are excellent and neither is here, for the same reason
//! nothing else in this module tree is: Atlas has to build and run on a bare
//! Windows machine offline, and every dependency is a thing that can fail to build on the
//! machine that matters. The conversion is two dozen lines of integer
//! arithmetic and it is exactly right, so it is owned.
//!
//! The algorithm is Howard Hinnant's `days_from_civil` / `civil_from_days`,
//! which is branch-free, exact for the whole proleptic Gregorian range, and has
//! been in the C++ standard library's lineage for a decade. It is not clever
//! and it is not mine; it is simply correct, which is what a date conversion
//! underneath a trading calendar needs to be.
//!
//! Everything here is UTC. Local time is a separate problem and lives in
//! `session.rs`, where the daylight-saving rules are — deliberately not mixed
//! in, because a conversion that quietly applied an offset would be the single
//! easiest way to make every session boundary an hour wrong for half the year.

pub const MS_PER_MIN: i64 = 60_000;
pub const MS_PER_HOUR: i64 = 3_600_000;
pub const MS_PER_DAY: i64 = 86_400_000;

/// A calendar date and time of day, in UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Utc {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub hour: u32,
    pub minute: u32,
}

/// Days since 1970-01-01 for a civil date. Hinnant's algorithm.
pub fn days_from_civil(y: i32, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = (y - era * 400) as i64; // [0, 399]
    let mp = ((m as i64) + 9) % 12; // Mar=0
    let doy = (153 * mp + 2) / 5 + (d as i64) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    (era as i64) * 146_097 + doe - 719_468
}

/// The inverse.
pub fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    ((if m <= 2 { y + 1 } else { y }) as i32, m, d)
}

impl Utc {
    pub fn from_ms(ms: i64) -> Utc {
        let days = ms.div_euclid(MS_PER_DAY);
        let rem = ms.rem_euclid(MS_PER_DAY);
        let (year, month, day) = civil_from_days(days);
        Utc {
            year,
            month,
            day,
            hour: (rem / MS_PER_HOUR) as u32,
            minute: ((rem % MS_PER_HOUR) / MS_PER_MIN) as u32,
        }
    }

    pub fn to_ms(self) -> i64 {
        days_from_civil(self.year, self.month, self.day) * MS_PER_DAY
            + self.hour as i64 * MS_PER_HOUR
            + self.minute as i64 * MS_PER_MIN
    }

    pub fn at(year: i32, month: u32, day: u32, hour: u32, minute: u32) -> Utc {
        Utc { year, month, day, hour, minute }
    }

    pub fn date(year: i32, month: u32, day: u32) -> Utc {
        Utc::at(year, month, day, 0, 0)
    }

    /// Monday = 0 .. Sunday = 6.
    pub fn weekday(self) -> u32 {
        let d = days_from_civil(self.year, self.month, self.day);
        // 1970-01-01 was a Thursday (weekday 3 on this numbering).
        (d + 3).rem_euclid(7) as u32
    }

    pub fn is_weekend(self) -> bool {
        self.weekday() >= 5
    }

    pub fn say(self) -> String {
        format!(
            "{:04}-{:02}-{:02} {:02}:{:02}Z",
            self.year, self.month, self.day, self.hour, self.minute
        )
    }

    pub fn say_date(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Day of the month of the `n`th given weekday. `n = 1` is the first.
pub fn nth_weekday(year: i32, month: u32, weekday: u32, n: u32) -> u32 {
    let first = Utc::date(year, month, 1).weekday();
    let day = 1 + (weekday + 7 - first) % 7;
    day + 7 * (n - 1)
}

/// Day of the month of the last given weekday.
pub fn last_weekday(year: i32, month: u32, weekday: u32) -> u32 {
    let (ny, nm) = if month == 12 { (year + 1, 1) } else { (year, month + 1) };
    let last_day = civil_from_days(days_from_civil(ny, nm, 1) - 1).2;
    let wd = Utc::date(year, month, last_day).weekday();
    last_day - (wd + 7 - weekday) % 7
}

/// The nth business day of a month, skipping weekends and any listed holidays.
pub fn nth_business_day(year: i32, month: u32, n: u32, holidays: &[(i32, u32, u32)]) -> u32 {
    let mut day = 1u32;
    let mut seen = 0u32;
    loop {
        let d = Utc::date(year, month, day);
        if !d.is_weekend() && !holidays.contains(&(year, month, day)) {
            seen += 1;
            if seen == n {
                return day;
            }
        }
        day += 1;
        if day > 31 {
            return 28; // unreachable for n <= 20; never panics on a caller's typo
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_epoch_is_where_it_should_be() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(Utc::from_ms(0), Utc::at(1970, 1, 1, 0, 0));
    }

    #[test]
    fn a_round_trip_is_the_identity_across_a_century() {
        // Every day from 1970 to 2070. If the conversion is wrong anywhere --
        // a leap year, a century boundary -- this finds it.
        for d in 0..36_600i64 {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d, "{:?}", (y, m, dd));
        }
    }

    #[test]
    fn leap_years_are_right_including_the_century_rules() {
        assert_eq!(civil_from_days(days_from_civil(2024, 2, 29)), (2024, 2, 29));
        // 2000 was a leap year, 1900 and 2100 were not.
        assert_eq!(civil_from_days(days_from_civil(2000, 2, 29)), (2000, 2, 29));
        assert_eq!(days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28), 1);
        assert_eq!(days_from_civil(2100, 3, 1) - days_from_civil(2100, 2, 28), 1);
        assert_eq!(days_from_civil(2024, 3, 1) - days_from_civil(2024, 2, 28), 2);
    }

    #[test]
    fn weekdays_are_right_against_known_dates() {
        // 2026-09-11 is a Friday; 2026-10-02 is a Friday; 2026-02-11 a Wednesday.
        assert_eq!(Utc::date(2026, 9, 11).weekday(), 4);
        assert_eq!(Utc::date(2026, 10, 2).weekday(), 4);
        assert_eq!(Utc::date(2026, 2, 11).weekday(), 2);
        assert_eq!(Utc::date(1970, 1, 1).weekday(), 3, "the epoch was a Thursday");
    }

    #[test]
    fn the_weekend_is_saturday_and_sunday() {
        assert!(Utc::date(2026, 9, 12).is_weekend());
        assert!(Utc::date(2026, 9, 13).is_weekend());
        assert!(!Utc::date(2026, 9, 14).is_weekend());
    }

    #[test]
    fn time_of_day_survives_the_round_trip() {
        let t = Utc::at(2026, 10, 2, 12, 30);
        assert_eq!(Utc::from_ms(t.to_ms()), t);
        assert_eq!(t.say(), "2026-10-02 12:30Z");
    }

    #[test]
    fn negative_timestamps_do_not_break_the_arithmetic() {
        // rem_euclid rather than %, so a pre-1970 instant lands on the right
        // day rather than an hour before it.
        let t = Utc::at(1965, 6, 15, 9, 45);
        assert_eq!(Utc::from_ms(t.to_ms()), t);
        assert!(t.to_ms() < 0);
    }

    #[test]
    fn nth_and_last_weekday_land_on_the_right_days() {
        // March 2026: 1st is a Sunday. 2nd Sunday is the 8th, last is the 29th.
        assert_eq!(nth_weekday(2026, 3, 6, 2), 8);
        assert_eq!(last_weekday(2026, 3, 6), 29);
        assert_eq!(last_weekday(2026, 10, 6), 25);
        assert_eq!(nth_weekday(2026, 11, 6, 1), 1);
        assert_eq!(last_weekday(2024, 12, 6), 29);
    }

    #[test]
    fn business_days_skip_weekends_and_holidays() {
        // 2026-08-01 is a Saturday, so the first business day is Monday the 3rd.
        assert_eq!(nth_business_day(2026, 8, 1, &[]), 3);
        assert_eq!(nth_business_day(2026, 8, 3, &[]), 5);
        // With the 3rd a holiday it shifts to the 4th.
        assert_eq!(nth_business_day(2026, 8, 1, &[(2026, 8, 3)]), 4);
    }
}
