//! Local time, for the first time: a zone's offset at any instant, and a wall
//! clock time turned back into UTC.
//!
//! **Sources:** POSIX.1-2017 §8.3 (the `TZ` variable: `std offset dst
//! [offset],start[/time],end[/time]` with `Jn`, `n` and `Mm.w.d` rules, week 5
//! meaning the last); musl's `src/time/__tz.c` (MIT) read for the rule
//! arithmetic; the POSIX strings are the footer lines of the IANA tz database
//! (public domain) for each zone as of 2026; the Windows names map through
//! CLDR's `windowsZones.xml` (Unicode licence), territory 001. RFC 5545
//! §3.3.5 for the two awkward hours: a time that happens twice means the
//! first, a time that never happens is read with the offset before the gap.
//! Clean-room.
//!
//! **Why Atlas wants it.** Nothing in the tree knew local time. The calendar
//! read an Outlook invite at `TZID=Pacific Standard Time:…T100000` as 10:00
//! UTC — seven hours early. The greeting refused to say "morning" rather than
//! risk saying it at night. Standing watches written "at 7" meant 7 UTC.
//! A rule string is small enough to carry for every zone, needs no tz
//! database on disk (Windows has none), and is right until the rules change —
//! which, for a zone, is a news item years in advance.

use crate::civil::{days_from_civil, days_in_month, weekday};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum When {
    /// Jn: day 1..=365, February 29 never counted.
    Julian1(u32),
    /// n: day 0..=365, February 29 counted.
    Julian0(u32),
    /// Mm.w.d: month, week 1..=5 (5 = last), weekday 0 = Sunday.
    Month(u32, u32, u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Change {
    when: When,
    /// Seconds after local midnight (may be negative or past 24h).
    at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Zone {
    pub name: String,
    /// The POSIX rule this zone was read from, so a zone that came from an
    /// `.ics` file's own VTIMEZONE can be stored and read back.
    spec: String,
    std_abbr: String,
    /// Seconds east of UTC (the opposite sign of the POSIX string).
    std_off: i64,
    dst: Option<(String, i64, Change, Change)>,
}

fn parse_num(s: &mut &str) -> Option<i64> {
    let n = s.chars().take_while(|c| c.is_ascii_digit()).count();
    if n == 0 {
        return None;
    }
    let v = s[..n].parse().ok()?;
    *s = &s[n..];
    Some(v)
}

/// `[+-]hh[:mm[:ss]]` as seconds, sign as written.
fn parse_hms(s: &mut &str) -> Option<i64> {
    let sign = match s.chars().next()? {
        '-' => {
            *s = &s[1..];
            -1
        }
        '+' => {
            *s = &s[1..];
            1
        }
        _ => 1,
    };
    let h = parse_num(s)?;
    let mut t = h * 3600;
    for mul in [60, 1] {
        if let Some(rest) = s.strip_prefix(':') {
            *s = rest;
            t += parse_num(s)? * mul;
        } else {
            break;
        }
    }
    Some(sign * t)
}

fn parse_abbr(s: &mut &str) -> Option<String> {
    if let Some(rest) = s.strip_prefix('<') {
        let end = rest.find('>')?;
        let a = rest[..end].to_string();
        *s = &rest[end + 1..];
        return Some(a);
    }
    let n = s.chars().take_while(|c| c.is_ascii_alphabetic()).count();
    if n < 3 {
        return None;
    }
    let a = s[..n].to_string();
    *s = &s[n..];
    Some(a)
}

fn parse_change(s: &mut &str) -> Option<Change> {
    let when = if let Some(rest) = s.strip_prefix('M') {
        *s = rest;
        let m = parse_num(s)? as u32;
        *s = s.strip_prefix('.')?;
        let w = parse_num(s)? as u32;
        *s = s.strip_prefix('.')?;
        let d = parse_num(s)? as u32;
        if !(1..=12).contains(&m) || !(1..=5).contains(&w) || d > 6 {
            return None;
        }
        When::Month(m, w, d)
    } else if let Some(rest) = s.strip_prefix('J') {
        *s = rest;
        When::Julian1(parse_num(s)? as u32)
    } else {
        When::Julian0(parse_num(s)? as u32)
    };
    let at = if let Some(rest) = s.strip_prefix('/') {
        *s = rest;
        parse_hms(s)?
    } else {
        7200
    };
    Some(Change { when, at })
}

impl Zone {
    pub fn utc() -> Zone {
        Zone { name: "UTC".into(), spec: "UTC0".into(), std_abbr: "UTC".into(), std_off: 0, dst: None }
    }

    /// A fixed offset, `off` seconds east of UTC, with no daylight saving:
    /// what's known when only the current offset can be read. 0 is UTC.
    pub fn fixed(off: i64) -> Zone {
        if off == 0 {
            return Zone::utc();
        }
        let sign = if off > 0 { '+' } else { '-' };
        let a = off.unsigned_abs();
        let abbr = format!("{sign}{:02}{:02}", a / 3600, (a % 3600) / 60);
        // POSIX offsets are west-positive, so the sign flips in the spec.
        let spec = format!("<{abbr}>{}{}:{:02}", if off > 0 { '-' } else { '+' }, a / 3600, (a % 3600) / 60);
        Zone { name: format!("UTC{sign}{:02}:{:02}", a / 3600, (a % 3600) / 60), spec, std_abbr: abbr, std_off: off, dst: None }
    }

    /// A POSIX TZ string: `PST8PDT,M3.2.0,M11.1.0`, `<+0530>-5:30`, `CET-1CEST,M3.5.0,M10.5.0/3`.
    pub fn posix(spec: &str) -> Option<Zone> {
        let mut s = spec.trim();
        let std_abbr = parse_abbr(&mut s)?;
        let std_off = -parse_hms(&mut s)?;
        if s.is_empty() {
            return Some(Zone { name: spec.trim().into(), spec: spec.trim().into(), std_abbr, std_off, dst: None });
        }
        let dst_abbr = parse_abbr(&mut s)?;
        let dst_off = if s.starts_with(',') || s.is_empty() { std_off + 3600 } else { -parse_hms(&mut s)? };
        // No rule given: POSIX leaves it to the implementation; the US rule is
        // what every libc uses.
        let (start, end) = if s.is_empty() {
            (Change { when: When::Month(3, 2, 0), at: 7200 }, Change { when: When::Month(11, 1, 0), at: 7200 })
        } else {
            s = s.strip_prefix(',')?;
            let a = parse_change(&mut s)?;
            s = s.strip_prefix(',')?;
            let b = parse_change(&mut s)?;
            if !s.is_empty() {
                return None;
            }
            (a, b)
        };
        Some(Zone { name: spec.trim().into(), spec: spec.trim().into(), std_abbr, std_off, dst: Some((dst_abbr, dst_off, start, end)) })
    }

    /// An IANA name (`America/Los_Angeles`), a Windows name (`Pacific
    /// Standard Time`), `UTC`/`Z`, or a POSIX string.
    pub fn named(name: &str) -> Option<Zone> {
        let n = name.trim().trim_matches('"');
        if n.eq_ignore_ascii_case("utc") || n.eq_ignore_ascii_case("z") || n.eq_ignore_ascii_case("gmt") || n == "Etc/UTC" {
            return Some(Zone::utc());
        }
        let iana = WINDOWS.iter().find(|(w, _)| w.eq_ignore_ascii_case(n)).map(|(_, i)| *i).unwrap_or(n);
        if let Some((_, p)) = IANA.iter().find(|(i, _)| i.eq_ignore_ascii_case(iana)) {
            let mut z = Zone::posix(p)?;
            z.name = iana.to_string();
            return Some(z);
        }
        // Outlook sometimes writes "(UTC-08:00) Pacific Time (US & Canada)".
        if let Some(i) = WINDOWS_DISPLAY.iter().find(|(d, _)| n.contains(d)).map(|(_, i)| *i) {
            return Zone::named(i);
        }
        Zone::posix(n)
    }

    fn change_utc(&self, year: i64, c: &Change, off_before: i64) -> i64 {
        let day = match c.when {
            When::Julian1(n) => {
                let leap_skip = if crate::civil::is_leap(year) && n >= 60 { 1 } else { 0 };
                days_from_civil(year, 1, 1) + n as i64 - 1 + leap_skip
            }
            When::Julian0(n) => days_from_civil(year, 1, 1) + n as i64,
            When::Month(m, w, d) => {
                let first = days_from_civil(year, m, 1);
                let wd = (weekday(first) as i64 + 1) % 7; // civil is ISO (Monday 0); POSIX counts from Sunday
                let mut day = first + (d as i64 - wd).rem_euclid(7) + 7 * (w as i64 - 1);
                let last = first + days_in_month(year, m) as i64 - 1;
                while day > last {
                    day -= 7;
                }
                day
            }
        };
        day * 86_400 + c.at - off_before
    }

    fn in_dst(&self, utc: i64) -> bool {
        let Some((_, dst_off, start, end)) = &self.dst else { return false };
        let year = crate::civil::Civil::from_local(utc + self.std_off).year;
        let s = self.change_utc(year, start, self.std_off);
        let e = self.change_utc(year, end, *dst_off);
        if s < e {
            s <= utc && utc < e
        } else {
            !(e <= utc && utc < s)
        }
    }

    /// Seconds east of UTC in force at `utc`.
    pub fn offset_at(&self, utc: i64) -> i64 {
        match &self.dst {
            Some((_, off, ..)) if self.in_dst(utc) => *off,
            _ => self.std_off,
        }
    }

    pub fn abbreviation_at(&self, utc: i64) -> &str {
        match &self.dst {
            Some((a, ..)) if self.in_dst(utc) => a,
            _ => &self.std_abbr,
        }
    }

    pub fn to_local(&self, utc: i64) -> i64 {
        utc + self.offset_at(utc)
    }

    /// A wall-clock time in this zone, as UTC. Twice-happening times take the
    /// first; never-happening ones use the offset from before the gap.
    pub fn to_utc(&self, local: i64) -> i64 {
        let Some((_, dst_off, ..)) = &self.dst else { return local - self.std_off };
        let as_std = local - self.std_off;
        let as_dst = local - dst_off;
        let std_ok = self.offset_at(as_std) == self.std_off;
        let dst_ok = self.offset_at(as_dst) == *dst_off;
        match (std_ok, dst_ok) {
            (true, true) => as_std.min(as_dst),
            (true, false) => as_std,
            (false, true) => as_dst,
            (false, false) => local - self.offset_at(local - self.std_off - 86_400),
        }
    }

    /// What to store to get this zone back with `named`: the IANA name when
    /// it is one, otherwise the rule itself.
    pub fn id(&self) -> String {
        match Zone::named(&self.name) {
            Some(z) if z.spec == self.spec => self.name.clone(),
            _ => self.spec.clone(),
        }
    }

    pub fn is_utc(&self) -> bool {
        self.std_off == 0 && self.dst.is_none()
    }

    /// Hour of the day (0–23) on the local clock.
    pub fn hour(&self, utc: i64) -> u32 {
        (self.to_local(utc).rem_euclid(86_400) / 3600) as u32
    }
}

/// Your zone, from the `time_zone` setting. Unset ("Automatic") is this
/// computer's own clock; a name that can't be read is UTC, never a guess.
///
/// Unset used to be UTC. The third chat found what that costs (24 Sep 2026):
/// on Eric's laptop in Pacific time, 5 pm read as midnight and "remind me at
/// 3" went off at 8 in the morning. So unset now means the machine: the zone
/// Windows names (`suggest`, with its daylight-saving rules), or failing
/// that the offset it reports now (`localclock`). `ATLAS_CLOCK_OFFSET`, which
/// the test suite pins to 0, wins over both.
pub fn home(setting: &str) -> Zone {
    let s = setting.trim();
    if s.is_empty() || s.eq_ignore_ascii_case("automatic") {
        return machine();
    }
    Zone::named(s).unwrap_or_else(Zone::utc)
}

/// This computer's zone. See `home`.
pub fn machine() -> Zone {
    if crate::localclock::told_offset().is_some() {
        return Zone::fixed(crate::localclock::machine_offset_secs());
    }
    suggest().unwrap_or_else(|| Zone::fixed(crate::localclock::machine_offset_secs()))
}

/// What the computer itself says its zone is, to offer when none is set:
/// `TZ` in the environment, or on Windows `tzutil /g` (a Windows zone name,
/// which `named` reads). Asked once per run.
pub fn suggest() -> Option<Zone> {
    static ONCE: std::sync::OnceLock<Option<Zone>> = std::sync::OnceLock::new();
    ONCE.get_or_init(|| {
        if let Some(z) = std::env::var("TZ").ok().and_then(|t| Zone::named(t.trim_start_matches(':'))) {
            return Some(z);
        }
        if cfg!(windows) {
            let out = crate::tools::command("tzutil").arg("/g").output().ok()?;
            return Zone::named(String::from_utf8_lossy(&out.stdout).trim());
        }
        None
    })
    .clone()
}

/// Every zone name this module knows, for a list to pick from.
pub fn names() -> Vec<&'static str> {
    IANA.iter().map(|(n, _)| *n).collect()
}

/// IANA zone → POSIX rule (the tz database footer, 2026).
const IANA: &[(&str, &str)] = &[
    ("America/Los_Angeles", "PST8PDT,M3.2.0,M11.1.0"),
    ("America/Vancouver", "PST8PDT,M3.2.0,M11.1.0"),
    ("America/Tijuana", "PST8PDT,M3.2.0,M11.1.0"),
    ("America/Denver", "MST7MDT,M3.2.0,M11.1.0"),
    ("America/Edmonton", "MST7MDT,M3.2.0,M11.1.0"),
    ("America/Boise", "MST7MDT,M3.2.0,M11.1.0"),
    ("America/Phoenix", "MST7"),
    ("America/Chicago", "CST6CDT,M3.2.0,M11.1.0"),
    ("America/Winnipeg", "CST6CDT,M3.2.0,M11.1.0"),
    ("America/Mexico_City", "CST6"),
    ("America/Regina", "CST6"),
    ("America/New_York", "EST5EDT,M3.2.0,M11.1.0"),
    ("America/Toronto", "EST5EDT,M3.2.0,M11.1.0"),
    ("America/Detroit", "EST5EDT,M3.2.0,M11.1.0"),
    ("America/Indiana/Indianapolis", "EST5EDT,M3.2.0,M11.1.0"),
    ("America/Bogota", "<-05>5"),
    ("America/Lima", "<-05>5"),
    ("America/Panama", "EST5"),
    ("America/Halifax", "AST4ADT,M3.2.0,M11.1.0"),
    ("America/Puerto_Rico", "AST4"),
    ("America/Caracas", "<-04>4"),
    ("America/Santiago", "<-04>4<-03>,M9.1.6/24,M4.1.6/24"),
    ("America/St_Johns", "NST3:30NDT,M3.2.0,M11.1.0"),
    ("America/Sao_Paulo", "<-03>3"),
    ("America/Argentina/Buenos_Aires", "<-03>3"),
    ("America/Anchorage", "AKST9AKDT,M3.2.0,M11.1.0"),
    ("Pacific/Honolulu", "HST10"),
    ("Atlantic/Reykjavik", "GMT0"),
    ("Europe/London", "GMT0BST,M3.5.0/1,M10.5.0"),
    ("Europe/Dublin", "IST-1GMT0,M10.5.0,M3.5.0/1"),
    ("Europe/Lisbon", "WET0WEST,M3.5.0/1,M10.5.0"),
    ("Europe/Paris", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Berlin", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Madrid", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Rome", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Amsterdam", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Brussels", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Zurich", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Stockholm", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Warsaw", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Prague", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Vienna", "CET-1CEST,M3.5.0,M10.5.0/3"),
    ("Europe/Athens", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Helsinki", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Kyiv", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Bucharest", "EET-2EEST,M3.5.0/3,M10.5.0/4"),
    ("Europe/Istanbul", "<+03>-3"),
    ("Europe/Moscow", "MSK-3"),
    ("Africa/Cairo", "EET-2EEST,M4.5.5/0,M10.5.4/24"),
    ("Africa/Johannesburg", "SAST-2"),
    ("Africa/Lagos", "WAT-1"),
    ("Africa/Nairobi", "EAT-3"),
    ("Asia/Dubai", "<+04>-4"),
    ("Asia/Karachi", "PKT-5"),
    ("Asia/Kolkata", "IST-5:30"),
    ("Asia/Kathmandu", "<+0545>-5:45"),
    ("Asia/Dhaka", "<+06>-6"),
    ("Asia/Bangkok", "<+07>-7"),
    ("Asia/Jakarta", "WIB-7"),
    ("Asia/Shanghai", "CST-8"),
    ("Asia/Hong_Kong", "HKT-8"),
    ("Asia/Singapore", "<+08>-8"),
    ("Asia/Taipei", "CST-8"),
    ("Asia/Manila", "PST-8"),
    ("Asia/Seoul", "KST-9"),
    ("Asia/Tokyo", "JST-9"),
    ("Australia/Perth", "AWST-8"),
    ("Australia/Adelaide", "ACST-9:30ACDT,M10.1.0,M4.1.0/3"),
    ("Australia/Darwin", "ACST-9:30"),
    ("Australia/Brisbane", "AEST-10"),
    ("Australia/Sydney", "AEST-10AEDT,M10.1.0,M4.1.0/3"),
    ("Australia/Melbourne", "AEST-10AEDT,M10.1.0,M4.1.0/3"),
    ("Pacific/Auckland", "NZST-12NZDT,M9.5.0,M4.1.0/3"),
];

/// Windows zone name → IANA (CLDR windowsZones, territory 001).
const WINDOWS: &[(&str, &str)] = &[
    ("Pacific Standard Time", "America/Los_Angeles"),
    ("Pacific Standard Time (Mexico)", "America/Tijuana"),
    ("Mountain Standard Time", "America/Denver"),
    ("US Mountain Standard Time", "America/Phoenix"),
    ("Central Standard Time", "America/Chicago"),
    ("Central Standard Time (Mexico)", "America/Mexico_City"),
    ("Canada Central Standard Time", "America/Regina"),
    ("Eastern Standard Time", "America/New_York"),
    ("US Eastern Standard Time", "America/Indiana/Indianapolis"),
    ("SA Pacific Standard Time", "America/Bogota"),
    ("Atlantic Standard Time", "America/Halifax"),
    ("Venezuela Standard Time", "America/Caracas"),
    ("Pacific SA Standard Time", "America/Santiago"),
    ("Newfoundland Standard Time", "America/St_Johns"),
    ("E. South America Standard Time", "America/Sao_Paulo"),
    ("Argentina Standard Time", "America/Argentina/Buenos_Aires"),
    ("Alaskan Standard Time", "America/Anchorage"),
    ("Hawaiian Standard Time", "Pacific/Honolulu"),
    ("UTC", "Etc/UTC"),
    ("Greenwich Standard Time", "Atlantic/Reykjavik"),
    ("GMT Standard Time", "Europe/London"),
    ("W. Europe Standard Time", "Europe/Berlin"),
    ("Romance Standard Time", "Europe/Paris"),
    ("Central Europe Standard Time", "Europe/Prague"),
    ("Central European Standard Time", "Europe/Warsaw"),
    ("GTB Standard Time", "Europe/Bucharest"),
    ("FLE Standard Time", "Europe/Kyiv"),
    ("Turkey Standard Time", "Europe/Istanbul"),
    ("Russian Standard Time", "Europe/Moscow"),
    ("Egypt Standard Time", "Africa/Cairo"),
    ("South Africa Standard Time", "Africa/Johannesburg"),
    ("W. Central Africa Standard Time", "Africa/Lagos"),
    ("E. Africa Standard Time", "Africa/Nairobi"),
    ("Arabian Standard Time", "Asia/Dubai"),
    ("Pakistan Standard Time", "Asia/Karachi"),
    ("India Standard Time", "Asia/Kolkata"),
    ("Nepal Standard Time", "Asia/Kathmandu"),
    ("Bangladesh Standard Time", "Asia/Dhaka"),
    ("SE Asia Standard Time", "Asia/Bangkok"),
    ("China Standard Time", "Asia/Shanghai"),
    ("Singapore Standard Time", "Asia/Singapore"),
    ("Taipei Standard Time", "Asia/Taipei"),
    ("Korea Standard Time", "Asia/Seoul"),
    ("Tokyo Standard Time", "Asia/Tokyo"),
    ("W. Australia Standard Time", "Australia/Perth"),
    ("Cen. Australia Standard Time", "Australia/Adelaide"),
    ("AUS Central Standard Time", "Australia/Darwin"),
    ("E. Australia Standard Time", "Australia/Brisbane"),
    ("AUS Eastern Standard Time", "Australia/Sydney"),
    ("New Zealand Standard Time", "Pacific/Auckland"),
];

/// Fragments of Outlook's display names, for files that carry those instead.
const WINDOWS_DISPLAY: &[(&str, &str)] = &[
    ("Pacific Time (US & Canada)", "America/Los_Angeles"),
    ("Mountain Time (US & Canada)", "America/Denver"),
    ("Central Time (US & Canada)", "America/Chicago"),
    ("Eastern Time (US & Canada)", "America/New_York"),
    ("Arizona", "America/Phoenix"),
    ("Dublin, Edinburgh, Lisbon, London", "Europe/London"),
    ("Amsterdam, Berlin, Bern, Rome", "Europe/Berlin"),
];
