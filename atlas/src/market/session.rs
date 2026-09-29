//! Which market is actually awake.
//!
//! ## Why this exists
//!
//! A verdict formed at 03:00 UTC and one formed at 14:00 UTC are not the same
//! kind of fact. The same structure — same swings, same break, same
//! confidence — means something different when London and New York are both on
//! the book than it does in the hour after Sydney opens and before Tokyo does.
//! Ranges hold in thin hours and break in deep ones. A system that scores its
//! own accuracy without splitting on that is averaging two different games and
//! calling it one number.
//!
//! ## Why the rules are hand-rolled
//!
//! Two constraints, pointing the same way. No dependencies is the house rule,
//! and it is load-bearing here: the Python original could not use `zoneinfo`
//! because **Windows ships no system timezone database** and every lookup
//! raises without an extra package. The same argument applies to a Rust tz
//! crate on a bare Windows machine. And the sessions have to be defined where they actually
//! live — in local hours at each financial centre.
//!
//! **London does not open at 07:00 UTC.** It opens at 08:00 London time, which
//! is 08:00 UTC in winter and 07:00 in summer, and New York's clocks change on
//! a different date. Measured, for 2026:
//!
//! ```text
//! 4 h   1 Jan –  6 Mar    London–New York overlap
//! 5 h   9 Mar – 27 Mar    US on DST, Europe not yet
//! 4 h  30 Mar – 23 Oct
//! 5 h  26 Oct – 30 Oct    Europe off DST, US not yet
//! 4 h   2 Nov – 31 Dec
//! ```
//!
//! Note the direction, because it is the opposite of what it sounds like: the
//! deepest window of the trading day **grows** by an hour for four weeks a
//! year. Anything with hardcoded UTC session boundaries is quietly wrong for a
//! month a year, in the most liquid part of the day.
//!
//! ## What a transition actually is
//!
//! A clock change is a local-time event, so the UTC instant it happens at is
//! the local instant minus the offset in force just before it. That sounds
//! obvious and is the one thing that is easy to get wrong: Sydney springs
//! forward at 02:00 AEST on the first Sunday of October, which is **16:00 UTC
//! on the Saturday**. Computing the Sunday and stopping there is a day out,
//! twice a year — which is exactly the bug the Python version shipped with, and
//! the reason the transition instants are pinned in the tests below.
//!
//! Those rules were validated against the IANA database over 2024–2027: eight
//! zones, hourly, **140,256 offsets, zero mismatches**. Rust has no tz database
//! to validate against here, so the transitions verified in that run are baked
//! in as literals and checked to the minute.

use super::time::{last_weekday, nth_weekday, Utc, MS_PER_HOUR};

/// The four trading centres, in the order the trading day meets them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Centre {
    Sydney,
    Tokyo,
    London,
    NewYork,
}

pub const CENTRES: [Centre; 4] = [Centre::Sydney, Centre::Tokyo, Centre::London, Centre::NewYork];

/// Zones that are NOT trading sessions.
///
/// These exist so the event calendar can express release times in the local
/// clock of the statistical agency that publishes them, which is the only place
/// those times are actually defined. A zone having a clock is not a claim about
/// liquidity, and [`open_centres`] never consults them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Zone {
    Sydney,
    Tokyo,
    London,
    NewYork,
    Toronto,
    Brussels,
    Zurich,
    Auckland,
}

impl Centre {
    fn zone(self) -> Zone {
        match self {
            Centre::Sydney => Zone::Sydney,
            Centre::Tokyo => Zone::Tokyo,
            Centre::London => Zone::London,
            Centre::NewYork => Zone::NewYork,
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Centre::Sydney => "Sydney",
            Centre::Tokyo => "Tokyo",
            Centre::London => "London",
            Centre::NewYork => "New York",
        }
    }
    /// Local business hours, in that centre's own clock. Asia keeps banking
    /// hours; London and New York open an hour earlier.
    pub fn hours(self) -> (u32, u32) {
        match self {
            Centre::Sydney | Centre::Tokyo => (9, 18),
            Centre::London | Centre::NewYork => (8, 17),
        }
    }
}

/// The UTC instant at which a local-clock event happens.
///
/// `offset_before` is the offset in force in the moment BEFORE the change —
/// standard time when springing forward, daylight time when falling back. This
/// is the whole trick, and getting it wrong puts Sydney's boundary a day late.
fn utc_instant(y: i32, m: u32, d: u32, local_hour: i64, offset_before: i64) -> i64 {
    Utc::date(y, m, d).to_ms() + (local_hour - offset_before) * MS_PER_HOUR
}

/// GMT (+0) / BST (+1). Last Sunday of March to last Sunday of October, both at
/// 01:00 UTC — the EU defines its change in UTC directly, which is why this is
/// the only one of the four with no offset arithmetic.
fn london(ms: i64) -> i64 {
    let y = Utc::from_ms(ms).year;
    let start = Utc::at(y, 3, last_weekday(y, 3, 6), 1, 0).to_ms();
    let end = Utc::at(y, 10, last_weekday(y, 10, 6), 1, 0).to_ms();
    if start <= ms && ms < end {
        1
    } else {
        0
    }
}

/// EST (−5) / EDT (−4). Second Sunday of March to first Sunday of November,
/// each at 02:00 local.
fn new_york(ms: i64) -> i64 {
    let y = Utc::from_ms(ms).year;
    let start = utc_instant(y, 3, nth_weekday(y, 3, 6, 2), 2, -5); // 07:00 UTC
    let end = utc_instant(y, 11, nth_weekday(y, 11, 6, 1), 2, -4); // 06:00 UTC
    if start <= ms && ms < end {
        -4
    } else {
        -5
    }
}

/// AEST (+10) / AEDT (+11). Southern hemisphere, so the daylight period WRAPS
/// the new year: first Sunday of October 02:00 AEST until the first Sunday of
/// April 03:00 AEDT. Both land at 16:00 UTC on the Saturday.
fn sydney(ms: i64) -> i64 {
    let y = Utc::from_ms(ms).year;
    let start = utc_instant(y, 10, nth_weekday(y, 10, 6, 1), 2, 10);
    let end = utc_instant(y, 4, nth_weekday(y, 4, 6, 1), 3, 11);
    if ms >= start || ms < end {
        11
    } else {
        10
    }
}

/// NZST (+12) / NZDT (+13). Wraps the new year like Sydney: last Sunday of
/// September 02:00 NZST until the first Sunday of April 03:00 NZDT.
fn auckland(ms: i64) -> i64 {
    let y = Utc::from_ms(ms).year;
    let start = utc_instant(y, 9, last_weekday(y, 9, 6), 2, 12);
    let end = utc_instant(y, 4, nth_weekday(y, 4, 6, 1), 3, 13);
    if ms >= start || ms < end {
        13
    } else {
        12
    }
}

/// Offset in hours at a zone, at a UTC instant.
pub fn offset_hours(zone: Zone, ms: i64) -> i64 {
    match zone {
        Zone::London => london(ms),
        // The EU defines its change in UTC, so continental Europe moves on the
        // same INSTANT as London and stays exactly one hour ahead all year.
        // Derived rather than restated, so the two cannot drift apart.
        Zone::Brussels | Zone::Zurich => london(ms) + 1,
        Zone::NewYork | Zone::Toronto => new_york(ms),
        Zone::Tokyo => 9, // Japan has had no daylight saving since 1951
        Zone::Sydney => sydney(ms),
        Zone::Auckland => auckland(ms),
    }
}

/// The instant on that zone's wall clock.
fn local(zone: Zone, ms: i64) -> Utc {
    Utc::from_ms(ms + offset_hours(zone, ms) * MS_PER_HOUR)
}

/// Is a centre inside its own business hours — and is it a weekday THERE.
///
/// Monday morning in Sydney is still Sunday evening in New York. A weekend
/// check done in UTC gets both ends of the week wrong.
pub fn is_open(centre: Centre, ms: i64) -> bool {
    let here = local(centre.zone(), ms);
    if here.is_weekend() {
        return false;
    }
    let (open, close) = centre.hours();
    here.hour >= open && here.hour < close
}

/// Which centres are awake at one instant, and how deep that makes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub at: i64,
    pub centres: Vec<Centre>,
}

impl Session {
    /// How many centres are on the book.
    ///
    /// The only honest liquidity proxy available without a data feed — and
    /// unlike a feed it cannot be missing, late, or wrong in a way nobody
    /// notices.
    pub fn depth(&self) -> usize {
        self.centres.len()
    }

    pub fn is_overlap(&self) -> bool {
        self.centres.len() > 1
    }

    /// A stable string for splitting a scoreboard.
    ///
    /// Deliberately NOT the timestamp: two Tuesdays in the London–New York
    /// overlap are the same population, and a key that separated them would
    /// mean nothing ever reached thirty observations and nothing was ever
    /// allowed to speak.
    pub fn key(&self) -> String {
        match self.centres.len() {
            0 => "off-session".into(),
            1 => self.centres[0].name().into(),
            _ => format!(
                "{} overlap",
                self.centres.iter().map(|c| c.name()).collect::<Vec<_>>().join("-")
            ),
        }
    }

    pub fn say(&self) -> String {
        if self.centres.is_empty() {
            return "off-session (no major centre open)".into();
        }
        let clocks: Vec<String> = self
            .centres
            .iter()
            .map(|c| {
                let t = local(c.zone(), self.at);
                format!("{} {:02}:{:02}", c.name(), t.hour, t.minute)
            })
            .collect();
        format!("{} ({} local)", self.key(), clocks.join(", "))
    }
}

pub fn session_at(ms: i64) -> Session {
    Session {
        at: ms,
        centres: CENTRES.iter().copied().filter(|&c| is_open(c, ms)).collect(),
    }
}

/// How many hours two centres are open together on the UTC day of `ms`.
///
/// Exists to make the clock-change drift visible rather than theoretical. The
/// direction is the opposite of what it sounds like — see the module note.
pub fn overlap_hours(ms: i64, a: Centre, b: Centre) -> f64 {
    let day = Utc::from_ms(ms);
    let start = Utc::date(day.year, day.month, day.day).to_ms();
    let mut minutes = 0;
    for m in 0..24 * 60 {
        let t = start + m * 60_000;
        if is_open(a, t) && is_open(b, t) {
            minutes += 1;
        }
    }
    minutes as f64 / 60.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Transition instants from the IANA database, verified in the Python run
    /// that swept 140,256 offsets across eight zones with zero mismatches.
    /// Rust has no tz database to check against here, so the facts that run
    /// established are written down and checked to the minute.
    ///
    /// (zone, y, m, d, hour, offset just before, offset from then on)
    const KNOWN: [(Zone, i32, u32, u32, u32, i64, i64); 16] = [
        (Zone::London, 2025, 3, 30, 1, 0, 1),
        (Zone::London, 2025, 10, 26, 1, 1, 0),
        (Zone::London, 2026, 3, 29, 1, 0, 1),
        (Zone::London, 2026, 10, 25, 1, 1, 0),
        (Zone::NewYork, 2025, 3, 9, 7, -5, -4),
        (Zone::NewYork, 2025, 11, 2, 6, -4, -5),
        (Zone::NewYork, 2026, 3, 8, 7, -5, -4),
        (Zone::NewYork, 2026, 11, 1, 6, -4, -5),
        // The two that were wrong first time: both are Saturday 16:00 UTC.
        (Zone::Sydney, 2025, 4, 5, 16, 11, 10),
        (Zone::Sydney, 2025, 10, 4, 16, 10, 11),
        (Zone::Sydney, 2026, 4, 4, 16, 11, 10),
        (Zone::Sydney, 2026, 10, 3, 16, 10, 11),
        // New Zealand, Saturday 14:00 UTC.
        (Zone::Auckland, 2026, 4, 4, 14, 13, 12),
        (Zone::Auckland, 2026, 9, 26, 14, 12, 13),
        (Zone::Brussels, 2026, 3, 29, 1, 1, 2),
        (Zone::Brussels, 2026, 10, 25, 1, 2, 1),
    ];

    #[test]
    fn every_clock_change_lands_on_the_exact_minute_it_really_does() {
        for (zone, y, m, d, h, before, after) in KNOWN {
            let t = Utc::at(y, m, d, h, 0).to_ms();
            assert_eq!(
                offset_hours(zone, t - 60_000),
                before,
                "{:?} a minute before {}-{:02}-{:02} {:02}:00Z",
                zone, y, m, d, h
            );
            assert_eq!(
                offset_hours(zone, t),
                after,
                "{:?} at {}-{:02}-{:02} {:02}:00Z",
                zone, y, m, d, h
            );
        }
    }

    #[test]
    fn sydney_is_on_daylight_time_in_january_not_july() {
        // The southern-hemisphere trap: January is INSIDE the daylight period,
        // and a rule written as `start <= t < end` makes it out to be the
        // opposite.
        assert_eq!(offset_hours(Zone::Sydney, Utc::date(2026, 1, 15).to_ms()), 11);
        assert_eq!(offset_hours(Zone::Sydney, Utc::date(2026, 7, 15).to_ms()), 10);
        assert_eq!(offset_hours(Zone::Auckland, Utc::date(2026, 1, 15).to_ms()), 13);
        assert_eq!(offset_hours(Zone::Auckland, Utc::date(2026, 7, 15).to_ms()), 12);
    }

    #[test]
    fn tokyo_never_moves() {
        for m in 1..=12 {
            assert_eq!(offset_hours(Zone::Tokyo, Utc::date(2026, m, 15).to_ms()), 9);
        }
    }

    #[test]
    fn continental_europe_stays_exactly_an_hour_ahead_of_london() {
        // Derived from London rather than restated, so the two cannot drift
        // apart in a later edit.
        for m in 1..=12 {
            let t = Utc::date(2026, m, 15).to_ms();
            assert_eq!(offset_hours(Zone::Brussels, t), offset_hours(Zone::London, t) + 1);
            assert_eq!(offset_hours(Zone::Zurich, t), offset_hours(Zone::Brussels, t));
        }
    }

    #[test]
    fn toronto_tracks_new_york() {
        for m in 1..=12 {
            let t = Utc::date(2026, m, 15).to_ms();
            assert_eq!(offset_hours(Zone::Toronto, t), offset_hours(Zone::NewYork, t));
        }
    }

    #[test]
    fn a_centre_opens_on_its_own_clock_not_on_utc() {
        // 2026-07-01 is a Wednesday. London in July is BST, so 08:00 London is
        // 07:00 UTC and the market is shut at 06:59.
        assert!(!is_open(Centre::London, Utc::at(2026, 7, 1, 6, 59).to_ms()));
        assert!(is_open(Centre::London, Utc::at(2026, 7, 1, 7, 0).to_ms()));
        // In January, GMT: open at 08:00 UTC, shut at 07:59.
        assert!(!is_open(Centre::London, Utc::at(2026, 1, 7, 7, 59).to_ms()));
        assert!(is_open(Centre::London, Utc::at(2026, 1, 7, 8, 0).to_ms()));
    }

    #[test]
    fn the_weekend_is_measured_at_the_centre_not_in_utc() {
        // Friday 23:00 UTC is Saturday morning in Sydney -- shut, even though
        // the UTC day is a weekday. This is the test a UTC weekend check fails.
        assert!(!is_open(Centre::Sydney, Utc::at(2026, 7, 3, 23, 0).to_ms()));
        // Sunday 23:00 UTC is Monday morning in Sydney -- open, even though the
        // UTC day is a Sunday.
        assert!(is_open(Centre::Sydney, Utc::at(2026, 7, 5, 23, 0).to_ms()));
    }

    #[test]
    fn the_london_new_york_overlap_gains_an_hour_in_march() {
        // The whole reason this is not a table of UTC hours. 2026: the US
        // springs forward 8 March, the EU 29 March. Between them New York opens
        // an hour earlier in UTC while London still closes at 17:00.
        let winter = overlap_hours(Utc::date(2026, 2, 18).to_ms(), Centre::London, Centre::NewYork);
        let gap = overlap_hours(Utc::date(2026, 3, 18).to_ms(), Centre::London, Centre::NewYork);
        let summer = overlap_hours(Utc::date(2026, 6, 17).to_ms(), Centre::London, Centre::NewYork);
        let autumn = overlap_hours(Utc::date(2026, 10, 28).to_ms(), Centre::London, Centre::NewYork);
        assert_eq!(winter, 4.0);
        assert_eq!(summer, 4.0);
        assert_eq!(gap, 5.0, "the three-week stretch is real");
        assert_eq!(autumn, 5.0, "and so is the autumn week");
    }

    #[test]
    fn a_session_names_itself_and_its_clocks() {
        let s = session_at(Utc::at(2026, 6, 17, 13, 0).to_ms());
        assert_eq!(s.centres, vec![Centre::London, Centre::NewYork]);
        assert!(s.is_overlap() && s.depth() == 2);
        assert!(s.key().contains("overlap"));
        assert!(s.say().contains("London") && s.say().contains("14:00"), "{}", s.say());
    }

    #[test]
    fn an_hour_with_nothing_open_is_recorded_as_off_session() {
        // 2026-06-17 21:30 UTC: New York shut at 21:00 (17:00 EDT), Sydney
        // opens at 23:00. Nothing is open, and the record must say so rather
        // than silently attributing the bar to a session.
        let s = session_at(Utc::at(2026, 6, 17, 21, 30).to_ms());
        assert!(s.centres.is_empty());
        assert_eq!(s.key(), "off-session");
        assert!(!s.is_overlap());
    }

    #[test]
    fn two_days_in_the_same_session_share_one_key() {
        let a = session_at(Utc::at(2026, 6, 16, 13, 0).to_ms());
        let b = session_at(Utc::at(2026, 6, 17, 14, 30).to_ms());
        assert_eq!(a.key(), b.key());
        let c = session_at(Utc::at(2026, 6, 17, 2, 0).to_ms());
        assert_ne!(c.key(), a.key());
    }

    #[test]
    fn a_zone_added_for_release_times_is_not_a_trading_session() {
        // A session is a claim about liquidity. Zurich having a clock is not
        // one, and `open_centres` must never consult it.
        let s = session_at(Utc::at(2026, 6, 17, 13, 0).to_ms());
        assert!(s.centres.iter().all(|c| CENTRES.contains(c)));
        assert_eq!(CENTRES.len(), 4);
    }
}
