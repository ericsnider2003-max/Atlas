//! Repeating events the way every calendar on earth writes them: RFC 5545 RRULE.
//!
//! **Source:** the iCalendar spec (RFC 5545 §3.3.10), with `fmeringdal/rust-rrule`
//! (MIT OR Apache-2.0) and python-dateutil's `rrule` read as references for
//! behaviour. Clean-room: no code copied, the expansion order below is the
//! spec's table.
//!
//! **Why Atlas wants it.** `calendar::Repeat` is deliberately four patterns
//! (once / daily / weekdays / weekly). That covers a standup and the gym. It
//! does not cover "the second Tuesday", "the last Friday of the month",
//! "every two weeks on Monday and Thursday" — and it cannot read the RRULE line
//! inside any `.ics` a business partner sends (see `vformat`). This is the
//! general rule, with a plain-English reader for the common spoken forms and a
//! describer so Atlas can say back what it understood.
//!
//! Supported: FREQ (DAILY/WEEKLY/MONTHLY/YEARLY), INTERVAL, COUNT, UNTIL,
//! BYDAY (with ordinals in MONTHLY/YEARLY), BYMONTHDAY (negative = from the
//! end), BYMONTH, BYSETPOS, WKST, plus EXDATE on the series. Not supported,
//! refused rather than ignored: HOURLY/MINUTELY/SECONDLY, BYWEEKNO, BYYEARDAY,
//! BYHOUR/BYMINUTE/BYSECOND. A rule part this module does not understand is an
//! error, never silently dropped — a dropped BYSETPOS would put a meeting on
//! the wrong day and nothing would look broken.

use crate::civil::{days_in_month, weekday, Civil};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub freq: Freq,
    pub interval: u32,
    pub count: Option<u32>,
    /// Inclusive, local seconds.
    pub until: Option<i64>,
    /// (ordinal, weekday 0=Mon). Ordinal `Some(2)` = second, `Some(-1)` = last.
    pub by_day: Vec<(Option<i32>, u32)>,
    pub by_month_day: Vec<i32>,
    pub by_month: Vec<u32>,
    pub by_set_pos: Vec<i32>,
    /// Week start, 0=Mon. RFC default is Monday.
    pub wkst: u32,
}

const DAYS: [&str; 7] = ["MO", "TU", "WE", "TH", "FR", "SA", "SU"];
const DAY_NAMES: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];
const MONTH_NAMES: [&str; 12] = [
    "January", "February", "March", "April", "May", "June", "July", "August", "September", "October",
    "November", "December",
];

/// A hard ceiling on periods walked, so a rule that can never match (BYMONTHDAY=31
/// with BYMONTH=2) ends with "no more" instead of spinning. rust-rrule uses the
/// same idea with a 100,000 iteration cap.
pub const MAX_PERIODS: u32 = 100_000;

impl Rule {
    pub fn new(freq: Freq) -> Rule {
        Rule {
            freq,
            interval: 1,
            count: None,
            until: None,
            by_day: vec![],
            by_month_day: vec![],
            by_month: vec![],
            by_set_pos: vec![],
            wkst: 0,
        }
    }

    /// Parse `FREQ=MONTHLY;BYDAY=2TU;COUNT=10` (an optional leading `RRULE:` is fine).
    pub fn parse(s: &str) -> Result<Rule, String> {
        let s = s.trim();
        let s = s.strip_prefix("RRULE:").unwrap_or(s);
        let mut freq = None;
        let mut r = Rule::new(Freq::Daily);
        for part in s.split(';').filter(|p| !p.is_empty()) {
            let (k, v) = part.split_once('=').ok_or_else(|| format!("'{part}' is not NAME=VALUE"))?;
            match k.to_ascii_uppercase().as_str() {
                "FREQ" => {
                    freq = Some(match v.to_ascii_uppercase().as_str() {
                        "DAILY" => Freq::Daily,
                        "WEEKLY" => Freq::Weekly,
                        "MONTHLY" => Freq::Monthly,
                        "YEARLY" => Freq::Yearly,
                        other => return Err(format!("FREQ={other} is not supported (daily/weekly/monthly/yearly only)")),
                    })
                }
                "INTERVAL" => {
                    r.interval = v.parse().map_err(|_| format!("INTERVAL={v} is not a number"))?;
                    if r.interval == 0 {
                        return Err("INTERVAL must be at least 1".into());
                    }
                    // A calendar from anywhere can say INTERVAL=4294967295;
                    // `period * interval` then overflowed (a panic in a debug
                    // build, wrong dates in release). Nothing real repeats
                    // less often than every thousand periods (Q20).
                    if r.interval > 1000 {
                        return Err(format!("INTERVAL={v} is too large to be a real repeat"));
                    }
                }
                "COUNT" => r.count = Some(v.parse().map_err(|_| format!("COUNT={v} is not a number"))?),
                "UNTIL" => r.until = Some(parse_ical_time(v)?),
                "BYDAY" => {
                    for d in v.split(',') {
                        r.by_day.push(parse_byday(d)?);
                    }
                }
                "BYMONTHDAY" => {
                    for d in v.split(',') {
                        let n: i32 = d.parse().map_err(|_| format!("BYMONTHDAY {d} is not a number"))?;
                        if n == 0 || n.abs() > 31 {
                            return Err(format!("BYMONTHDAY {n} is out of range"));
                        }
                        r.by_month_day.push(n);
                    }
                }
                "BYMONTH" => {
                    for d in v.split(',') {
                        let n: u32 = d.parse().map_err(|_| format!("BYMONTH {d} is not a number"))?;
                        if !(1..=12).contains(&n) {
                            return Err(format!("BYMONTH {n} is out of range"));
                        }
                        r.by_month.push(n);
                    }
                }
                "BYSETPOS" => {
                    for d in v.split(',') {
                        let n: i32 = d.parse().map_err(|_| format!("BYSETPOS {d} is not a number"))?;
                        if n == 0 || n.abs() > 366 {
                            return Err(format!("BYSETPOS {n} is out of range"));
                        }
                        r.by_set_pos.push(n);
                    }
                }
                "WKST" => {
                    r.wkst = DAYS
                        .iter()
                        .position(|d| d.eq_ignore_ascii_case(v))
                        .ok_or_else(|| format!("WKST={v} is not a weekday"))? as u32
                }
                other => return Err(format!("{other} is not supported — refusing rather than ignoring it")),
            }
        }
        if r.count.is_some() && r.until.is_some() {
            return Err("COUNT and UNTIL together are not allowed (RFC 5545)".into());
        }
        r.freq = freq.ok_or("FREQ is required")?;
        if matches!(r.freq, Freq::Daily | Freq::Weekly) && r.by_day.iter().any(|(o, _)| o.is_some()) {
            return Err("an ordinal BYDAY (like 2TU) only means something MONTHLY or YEARLY".into());
        }
        Ok(r)
    }

    /// Back to the wire form. `parse(to_rrule(r)) == r`.
    pub fn to_rrule(&self) -> String {
        let mut out = vec![format!(
            "FREQ={}",
            match self.freq {
                Freq::Daily => "DAILY",
                Freq::Weekly => "WEEKLY",
                Freq::Monthly => "MONTHLY",
                Freq::Yearly => "YEARLY",
            }
        )];
        if self.interval != 1 {
            out.push(format!("INTERVAL={}", self.interval));
        }
        if let Some(c) = self.count {
            out.push(format!("COUNT={c}"));
        }
        if let Some(u) = self.until {
            out.push(format!("UNTIL={}", format_ical_time(u)));
        }
        if !self.by_day.is_empty() {
            let v: Vec<String> = self
                .by_day
                .iter()
                .map(|(o, d)| format!("{}{}", o.map(|o| o.to_string()).unwrap_or_default(), DAYS[*d as usize]))
                .collect();
            out.push(format!("BYDAY={}", v.join(",")));
        }
        let join = |v: &[i32]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>().join(",");
        if !self.by_month_day.is_empty() {
            out.push(format!("BYMONTHDAY={}", join(&self.by_month_day)));
        }
        if !self.by_month.is_empty() {
            let v: Vec<i32> = self.by_month.iter().map(|m| *m as i32).collect();
            out.push(format!("BYMONTH={}", join(&v)));
        }
        if !self.by_set_pos.is_empty() {
            out.push(format!("BYSETPOS={}", join(&self.by_set_pos)));
        }
        if self.wkst != 0 {
            out.push(format!("WKST={}", DAYS[self.wkst as usize]));
        }
        out.join(";")
    }

    /// Say the rule back in words, so a misheard rule is caught before it
    /// lands on the calendar fifty times.
    pub fn describe(&self) -> String {
        let unit = match self.freq {
            Freq::Daily => "day",
            Freq::Weekly => "week",
            Freq::Monthly => "month",
            Freq::Yearly => "year",
        };
        let mut s = if self.interval == 1 {
            format!("every {unit}")
        } else {
            format!("every {} {unit}s", self.interval)
        };
        if !self.by_day.is_empty() {
            let days: Vec<String> = self
                .by_day
                .iter()
                .map(|(o, d)| match o {
                    None => DAY_NAMES[*d as usize].to_string(),
                    Some(n) => format!("{} {}", ordinal_word(*n), DAY_NAMES[*d as usize]),
                })
                .collect();
            s.push_str(&format!(" on {}", list_words(&days)));
        }
        if !self.by_month_day.is_empty() {
            let v: Vec<String> = self
                .by_month_day
                .iter()
                .map(|n| if *n < 0 { format!("{} day", ordinal_word(*n)) } else { format!("the {}", nth(*n as u32)) })
                .collect();
            s.push_str(&format!(" on {}", list_words(&v)));
        }
        if !self.by_month.is_empty() {
            let v: Vec<String> = self.by_month.iter().map(|m| MONTH_NAMES[*m as usize - 1].to_string()).collect();
            s.push_str(&format!(" in {}", list_words(&v)));
        }
        if !self.by_set_pos.is_empty() {
            let v: Vec<String> = self.by_set_pos.iter().map(|p| ordinal_word(*p)).collect();
            s.push_str(&format!(", taking only the {} of those", list_words(&v)));
        }
        if let Some(c) = self.count {
            s.push_str(&format!(", {c} times"));
        }
        if let Some(u) = self.until {
            let c = Civil::from_local(u);
            s.push_str(&format!(", until {}-{:02}-{:02}", c.year, c.month, c.day));
        }
        s
    }

    /// The first time the rule itself produces at or after `anchor`, at the
    /// anchor's time of day. Unlike a `Series`, the anchor is not an
    /// occurrence unless the rule generates it — which is what "remind me the
    /// last Friday of every month", said on a Wednesday, needs: the first one
    /// is the Friday, not today.
    pub fn first_at_or_after(&self, anchor: i64) -> Option<i64> {
        let start = Civil::from_local(anchor);
        let tod = anchor.rem_euclid(86_400);
        for period in 0..MAX_PERIODS as i64 {
            for d in self.expand(&start, period) {
                let t = d * 86_400 + tod;
                if t >= anchor {
                    return match self.until {
                        Some(u) if t > u => None,
                        _ => Some(t),
                    };
                }
            }
        }
        None
    }

    /// The days (days-since-epoch) this rule yields inside one period.
    fn expand(&self, start: &Civil, period: i64) -> Vec<i64> {
        let mut days: Vec<i64> = match self.freq {
            Freq::Daily => {
                let d = start.days() + period * self.interval as i64;
                let c = Civil::from_local(d * 86_400);
                let ok = (self.by_month.is_empty() || self.by_month.contains(&c.month))
                    && (self.by_month_day.is_empty() || month_day_matches(&self.by_month_day, &c))
                    && (self.by_day.is_empty() || self.by_day.iter().any(|(_, w)| *w == c.weekday()));
                if ok { vec![d] } else { vec![] }
            }
            Freq::Weekly => {
                let offset = (start.weekday() + 7 - self.wkst) % 7;
                let week0 = start.days() - offset as i64 + period * 7 * self.interval as i64;
                let wanted: Vec<u32> =
                    if self.by_day.is_empty() { vec![start.weekday()] } else { self.by_day.iter().map(|(_, w)| *w).collect() };
                (0..7)
                    .map(|i| week0 + i)
                    .filter(|d| wanted.contains(&weekday(*d)))
                    .filter(|d| self.by_month.is_empty() || self.by_month.contains(&Civil::from_local(d * 86_400).month))
                    .collect()
            }
            Freq::Monthly => {
                let months = start.year * 12 + (start.month as i64 - 1) + period * self.interval as i64;
                let (y, m) = (months.div_euclid(12), (months.rem_euclid(12) + 1) as u32);
                if !self.by_month.is_empty() && !self.by_month.contains(&m) {
                    vec![]
                } else {
                    self.month_days(y, m, start)
                }
            }
            Freq::Yearly => {
                let y = start.year + period * self.interval as i64;
                if self.by_month.is_empty() && !self.by_day.is_empty() && self.by_month_day.is_empty() {
                    // BYDAY across the whole year: ordinals count within the year.
                    let first = crate::civil::days_from_civil(y, 1, 1);
                    let len = if crate::civil::is_leap(y) { 366 } else { 365 };
                    by_day_in_range(&self.by_day, first, len)
                } else {
                    let months = if self.by_month.is_empty() { vec![start.month] } else { self.by_month.clone() };
                    let mut out = vec![];
                    for m in months {
                        out.extend(self.month_days(y, m, start));
                    }
                    out
                }
            }
        };
        days.sort();
        days.dedup();
        if !self.by_set_pos.is_empty() {
            let n = days.len() as i32;
            let mut picked: Vec<i64> = self
                .by_set_pos
                .iter()
                .filter_map(|p| {
                    let i = if *p > 0 { p - 1 } else { n + p };
                    (0..n).contains(&i).then(|| days[i as usize])
                })
                .collect();
            picked.sort();
            picked.dedup();
            days = picked;
        }
        days
    }

    fn month_days(&self, y: i64, m: u32, start: &Civil) -> Vec<i64> {
        let first = crate::civil::days_from_civil(y, m, 1);
        let len = days_in_month(y, m) as i64;
        let from_md: Option<Vec<i64>> = (!self.by_month_day.is_empty()).then(|| {
            self.by_month_day
                .iter()
                .filter_map(|n| {
                    let d = if *n > 0 { *n as i64 } else { len + 1 + *n as i64 };
                    (1..=len).contains(&d).then(|| first + d - 1)
                })
                .collect()
        });
        let from_bd: Option<Vec<i64>> = (!self.by_day.is_empty()).then(|| by_day_in_range(&self.by_day, first, len));
        match (from_md, from_bd) {
            (Some(a), Some(b)) => a.into_iter().filter(|d| b.contains(d)).collect(),
            (Some(a), None) => a,
            (None, Some(b)) => b,
            // Neither given: the start's own day of month. A 31st skips short
            // months (RFC behaviour) rather than sliding to the 30th.
            (None, None) => {
                if start.day as i64 <= len {
                    vec![first + start.day as i64 - 1]
                } else {
                    vec![]
                }
            }
        }
    }
}

fn month_day_matches(mds: &[i32], c: &Civil) -> bool {
    let len = days_in_month(c.year, c.month) as i32;
    mds.iter().any(|n| if *n > 0 { *n == c.day as i32 } else { len + 1 + n == c.day as i32 })
}

fn by_day_in_range(by_day: &[(Option<i32>, u32)], first: i64, len: i64) -> Vec<i64> {
    let mut out = vec![];
    for (ord, wd) in by_day {
        let matches: Vec<i64> = (0..len).map(|i| first + i).filter(|d| weekday(*d) == *wd).collect();
        match ord {
            None => out.extend(matches),
            Some(n) => {
                let k = matches.len() as i32;
                let i = if *n > 0 { n - 1 } else { k + n };
                if (0..k).contains(&i) {
                    out.push(matches[i as usize]);
                }
            }
        }
    }
    out
}

fn parse_byday(d: &str) -> Result<(Option<i32>, u32), String> {
    let d = d.trim();
    // The weekday is the last two bytes; a non-ASCII character there would
    // split it (a calendar from anywhere can send one: found by
    // tests/fuzzing_the_readers.rs, which crashed here).
    if d.len() < 2 || !d.is_char_boundary(d.len() - 2) {
        return Err(format!("BYDAY {d} is not a weekday"));
    }
    let (num, day) = d.split_at(d.len() - 2);
    let w = DAYS.iter().position(|x| x.eq_ignore_ascii_case(day)).ok_or_else(|| format!("BYDAY {d} is not a weekday"))?;
    let ord = if num.is_empty() {
        None
    } else {
        let n: i32 = num.trim_start_matches('+').parse().map_err(|_| format!("BYDAY {d} has a bad ordinal"))?;
        if n == 0 || n.abs() > 53 {
            return Err(format!("BYDAY {d} ordinal out of range"));
        }
        Some(n)
    };
    Ok((ord, w as u32))
}

/// `20260923`, `20260923T090000`, or with a trailing `Z`. Read as local seconds.
pub fn parse_ical_time(v: &str) -> Result<i64, String> {
    let v = v.trim().trim_end_matches('Z');
    let bad = || format!("'{v}' is not an iCalendar date");
    // ASCII first: a date is digits and a `T`, and the byte slices below would
    // split a character otherwise (fuzzing_the_readers found one that did).
    if !v.is_ascii() || v.len() < 8 || !v[..8].bytes().all(|b| b.is_ascii_digit()) {
        return Err(bad());
    }
    let y: i64 = v[0..4].parse().map_err(|_| bad())?;
    let m: u32 = v[4..6].parse().map_err(|_| bad())?;
    let d: u32 = v[6..8].parse().map_err(|_| bad())?;
    if !(1..=12).contains(&m) || d == 0 || d > days_in_month(y, m) {
        return Err(bad());
    }
    let (mut hh, mut mm, mut ss) = (23, 59, 59); // a date-only UNTIL covers that whole day
    if v.len() > 8 {
        let t = v[8..].strip_prefix('T').ok_or_else(bad)?;
        if t.len() != 6 {
            return Err(bad());
        }
        hh = t[0..2].parse().map_err(|_| bad())?;
        mm = t[2..4].parse().map_err(|_| bad())?;
        ss = t[4..6].parse().map_err(|_| bad())?;
    }
    Ok(Civil { year: y, month: m, day: d, hour: hh, minute: mm, second: ss }.to_local())
}

pub fn format_ical_time(t: i64) -> String {
    let c = Civil::from_local(t);
    format!("{:04}{:02}{:02}T{:02}{:02}{:02}", c.year, c.month, c.day, c.hour, c.minute, c.second)
}

fn ordinal_word(n: i32) -> String {
    match n {
        -1 => "last".into(),
        -2 => "second-to-last".into(),
        1 => "first".into(),
        2 => "second".into(),
        3 => "third".into(),
        4 => "fourth".into(),
        5 => "fifth".into(),
        n if n < 0 => format!("{}-from-last", nth((-n) as u32)),
        n => nth(n as u32),
    }
}

fn nth(n: u32) -> String {
    let suf = match (n % 10, n % 100) {
        (1, x) if x != 11 => "st",
        (2, x) if x != 12 => "nd",
        (3, x) if x != 13 => "rd",
        _ => "th",
    };
    format!("{n}{suf}")
}

fn list_words(v: &[String]) -> String {
    match v.len() {
        0 => String::new(),
        1 => v[0].clone(),
        _ => format!("{} and {}", v[..v.len() - 1].join(", "), v[v.len() - 1]),
    }
}

/// A rule anchored to a first occurrence, with exceptions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Series {
    /// Local seconds of the first occurrence (DTSTART). Its time of day is
    /// the time of day of every occurrence.
    pub start: i64,
    pub rule: Rule,
    /// Occurrences cancelled one at a time (EXDATE), local seconds.
    pub exdates: Vec<i64>,
}

impl Series {
    pub fn new(start: i64, rule: Rule) -> Series {
        Series { start, rule, exdates: vec![] }
    }

    /// Every occurrence in `[from, to)`, at most `limit`.
    ///
    /// DTSTART is always the first occurrence (RFC 5545: it "always counts as
    /// the first occurrence" for COUNT), even if the rule would not produce
    /// it. EXDATEs are removed *after* counting, as the RFC orders it, so
    /// cancelling one meeting of a COUNT=10 series leaves nine, not ten.
    pub fn between(&self, from: i64, to: i64, limit: usize) -> Vec<i64> {
        let start = Civil::from_local(self.start);
        let tod = self.start.rem_euclid(86_400);
        let mut out = vec![];
        let mut emitted: u32 = 0;
        let push = |t: i64, out: &mut Vec<i64>, emitted: &mut u32| -> bool {
            if let Some(c) = self.rule.count {
                if *emitted >= c {
                    return false;
                }
            }
            if let Some(u) = self.rule.until {
                if t > u {
                    return false;
                }
            }
            *emitted += 1;
            if t >= from && t < to && !self.exdates.contains(&t) {
                out.push(t);
            }
            true
        };
        if !push(self.start, &mut out, &mut emitted) {
            return out;
        }
        'periods: for period in 0..MAX_PERIODS as i64 {
            for d in self.rule.expand(&start, period) {
                let t = d * 86_400 + tod;
                if t <= self.start {
                    continue;
                }
                if t >= to || out.len() >= limit {
                    break 'periods;
                }
                if !push(t, &mut out, &mut emitted) {
                    break 'periods;
                }
            }
        }
        out.truncate(limit);
        out
    }

}

/// Read the spoken forms people actually use. Returns `None` rather than a
/// guess — the caller asks, per the tree's "not acting on a guess" rule.
///
/// Handles: "every day", "every weekday", "every 2 weeks on monday and
/// thursday", "every second tuesday [of the month]", "the last friday of
/// every month", "on the 15th of every month", "the last weekday of the
/// month", "every year".
pub fn from_plain(text: &str) -> Option<Rule> {
    let t = text.to_lowercase().replace(',', " ");
    let words: Vec<&str> = t.split_whitespace().collect();
    let has = |w: &str| words.contains(&w);
    let day_of = |w: &str| -> Option<u32> {
        let w = w.trim_end_matches('s');
        ["monday", "tuesday", "wednesday", "thursday", "friday", "saturday", "sunday"]
            .iter()
            .position(|d| *d == w || (w.len() >= 3 && d.starts_with(w)))
            .map(|i| i as u32)
    };
    let ord_of = |w: &str| -> Option<i32> {
        Some(match w {
            "first" | "1st" => 1,
            "second" | "2nd" | "other" => 2,
            "third" | "3rd" => 3,
            "fourth" | "4th" => 4,
            "fifth" | "5th" => 5,
            "last" => -1,
            _ => return None,
        })
    };
    let num_of = |w: &str| -> Option<u32> {
        w.parse().ok().or(match w {
            "two" => Some(2),
            "three" => Some(3),
            "four" => Some(4),
            "six" => Some(6),
            _ => None,
        })
    };
    let month_context = has("month") || has("monthly");

    // "last weekday of the month" / "first weekday of the month"
    if month_context && has("weekday") {
        if let Some(o) = words.iter().find_map(|w| ord_of(w)) {
            let mut r = Rule::new(Freq::Monthly);
            r.by_day = (0..5).map(|d| (None, d)).collect();
            r.by_set_pos = vec![o];
            return Some(r);
        }
    }
    // "every weekday"
    if has("weekday") || has("weekdays") {
        let mut r = Rule::new(Freq::Weekly);
        r.by_day = (0..5).map(|d| (None, d)).collect();
        return Some(r);
    }
    // "the 15th of every month"
    if month_context {
        // "15th", "1st", "22nd" — an ordinal suffix, so "4pm" is a time and
        // not the 4th of the month.
        if let Some(n) = words.iter().find_map(|w| {
            let d = w.trim_end_matches(|c: char| c.is_ascii_alphabetic());
            let suffix = &w[d.len()..];
            ["st", "nd", "rd", "th"].contains(&suffix).then(|| d.parse::<i32>().ok()).flatten()
        }) {
            if (1..=31).contains(&n) {
                let mut r = Rule::new(Freq::Monthly);
                r.by_month_day = vec![n];
                return Some(r);
            }
        }
        // "the last friday of every month", "second tuesday of the month"
        for pair in words.windows(2) {
            if let (Some(o), Some(d)) = (ord_of(pair[0]), day_of(pair[1])) {
                let mut r = Rule::new(Freq::Monthly);
                r.by_day = vec![(Some(o), d)];
                return Some(r);
            }
        }
        if has("every") {
            return Some(Rule::new(Freq::Monthly));
        }
    }
    // "every other week" / "every 2 weeks on monday and thursday"
    let mut interval = 1;
    for pair in words.windows(2) {
        if pair[0] == "every" {
            if pair[1] == "other" {
                interval = 2;
            } else if let Some(n) = num_of(pair[1]) {
                interval = n;
            }
        }
    }
    // "every second tuesday" with no month is "every other tuesday".
    let unit_given = words.iter().any(|w| matches!(*w, "day" | "days" | "week" | "weeks" | "month" | "months" | "year" | "years"));
    let days: Vec<u32> = words.iter().filter_map(|w| day_of(w)).collect();
    if !days.is_empty() && !unit_given && has("every") {
        if let Some(o) = words.iter().find_map(|w| ord_of(w)) {
            if o == 2 {
                interval = 2;
            }
        }
    }
    if !days.is_empty() {
        let mut r = Rule::new(Freq::Weekly);
        r.interval = interval;
        let mut d = days;
        d.sort();
        d.dedup();
        r.by_day = d.into_iter().map(|w| (None, w)).collect();
        return Some(r);
    }
    let freq = if has("day") || has("days") || has("daily") {
        Freq::Daily
    } else if has("week") || has("weeks") || has("weekly") {
        Freq::Weekly
    } else if has("year") || has("years") || has("yearly") || has("annually") {
        Freq::Yearly
    } else {
        return None;
    };
    let mut r = Rule::new(freq);
    r.interval = interval;
    Some(r)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::civil::Civil;

    fn at(y: i64, m: u32, d: u32, h: u32) -> i64 {
        Civil { year: y, month: m, day: d, hour: h, minute: 0, second: 0 }.to_local()
    }
    fn ymd(t: i64) -> (i64, u32, u32) {
        let c = Civil::from_local(t);
        (c.year, c.month, c.day)
    }
    fn run(start: i64, rule: &str, n: usize) -> Vec<(i64, u32, u32)> {
        Series::new(start, Rule::parse(rule).unwrap()).between(i64::MIN, i64::MAX, n).into_iter().map(ymd).collect()
    }

    #[test]
    fn second_tuesday_monthly_rfc_example_shape() {
        // 2026-09-08 is the second Tuesday of September 2026.
        let got = run(at(2026, 9, 8, 9), "FREQ=MONTHLY;BYDAY=2TU;COUNT=4", 10);
        assert_eq!(got, vec![(2026, 9, 8), (2026, 10, 13), (2026, 11, 10), (2026, 12, 8)]);
    }

    #[test]
    fn last_friday() {
        let got = run(at(2026, 9, 25, 16), "FREQ=MONTHLY;BYDAY=-1FR;COUNT=3", 10);
        assert_eq!(got, vec![(2026, 9, 25), (2026, 10, 30), (2026, 11, 27)]);
    }

    #[test]
    fn last_weekday_of_month_via_bysetpos() {
        // Oct 2026 ends on a Saturday, so the last weekday is Fri the 30th.
        let got = run(at(2026, 9, 30, 9), "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1;COUNT=3", 10);
        assert_eq!(got, vec![(2026, 9, 30), (2026, 10, 30), (2026, 11, 30)]);
    }

    #[test]
    fn biweekly_on_two_days() {
        // Mon 2026-09-21 start; every 2 weeks on Mon and Thu.
        let got = run(at(2026, 9, 21, 10), "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH;COUNT=5", 10);
        assert_eq!(got, vec![(2026, 9, 21), (2026, 9, 24), (2026, 10, 5), (2026, 10, 8), (2026, 10, 19)]);
    }

    #[test]
    fn thirty_first_skips_short_months_not_slides() {
        let got = run(at(2026, 1, 31, 9), "FREQ=MONTHLY;COUNT=4", 10);
        assert_eq!(got, vec![(2026, 1, 31), (2026, 3, 31), (2026, 5, 31), (2026, 7, 31)]);
    }

    #[test]
    fn negative_monthday_is_from_the_end() {
        let got = run(at(2026, 1, 31, 9), "FREQ=MONTHLY;BYMONTHDAY=-1;COUNT=3", 10);
        assert_eq!(got, vec![(2026, 1, 31), (2026, 2, 28), (2026, 3, 31)]);
    }

    #[test]
    fn until_is_inclusive_and_date_only_covers_the_day() {
        let got = run(at(2026, 9, 21, 18), "FREQ=DAILY;UNTIL=20260923", 10);
        assert_eq!(got, vec![(2026, 9, 21), (2026, 9, 22), (2026, 9, 23)]);
    }

    #[test]
    fn exdate_removed_after_counting() {
        let start = at(2026, 9, 21, 9);
        let mut s = Series::new(start, Rule::parse("FREQ=DAILY;COUNT=3").unwrap());
        s.exdates.push(at(2026, 9, 22, 9));
        let got: Vec<_> = s.between(i64::MIN, i64::MAX, 10).into_iter().map(ymd).collect();
        assert_eq!(got, vec![(2026, 9, 21), (2026, 9, 23)]);
    }

    #[test]
    fn yearly_leap_day_skips_non_leap_years() {
        let got = run(at(2024, 2, 29, 9), "FREQ=YEARLY;COUNT=3", 10);
        assert_eq!(got, vec![(2024, 2, 29), (2028, 2, 29), (2032, 2, 29)]);
    }

    #[test]
    fn thanksgiving() {
        let got = run(at(2026, 11, 26, 12), "FREQ=YEARLY;BYMONTH=11;BYDAY=4TH;COUNT=3", 10);
        assert_eq!(got, vec![(2026, 11, 26), (2027, 11, 25), (2028, 11, 23)]);
    }

    #[test]
    fn first_at_or_after_does_not_count_the_anchor() {
        let r = Rule::parse("FREQ=MONTHLY;BYDAY=-1FR").unwrap();
        // said on Wed 2026-09-23 at 16:00 -> Fri 25th 16:00
        assert_eq!(ymd(r.first_at_or_after(at(2026, 9, 23, 16)).unwrap()), (2026, 9, 25));
        // said on the Friday itself, before 16:00 -> that day
        assert_eq!(ymd(r.first_at_or_after(at(2026, 9, 25, 16)).unwrap()), (2026, 9, 25));
        assert_eq!(Rule::parse("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30").unwrap().first_at_or_after(0), None);
    }

    #[test]
    fn impossible_rule_ends_instead_of_spinning() {
        let s = Series::new(at(2026, 1, 1, 9), Rule::parse("FREQ=YEARLY;BYMONTH=2;BYMONTHDAY=30").unwrap());
        assert_eq!(s.between(at(2026, 1, 2, 0), i64::MAX, 5), Vec::<i64>::new());
    }

    #[test]
    fn next_after_and_window() {
        let s = Series::new(at(2026, 9, 21, 9), Rule::parse("FREQ=WEEKLY;BYDAY=MO,WE").unwrap());
        assert_eq!(s.between(at(2026, 9, 21, 9) + 1, i64::MAX, 1).into_iter().map(ymd).next(), Some((2026, 9, 23)));
        assert_eq!(s.between(at(2026, 10, 1, 0), at(2026, 10, 8, 0), 10).len(), 2);
    }

    #[test]
    fn unknown_parts_are_refused_not_ignored() {
        assert!(Rule::parse("FREQ=WEEKLY;BYWEEKNO=20").is_err());
        assert!(Rule::parse("FREQ=HOURLY").is_err());
        assert!(Rule::parse("FREQ=DAILY;COUNT=2;UNTIL=20260101").is_err());
        assert!(Rule::parse("FREQ=WEEKLY;BYDAY=2TU").is_err());
        assert!(Rule::parse("INTERVAL=2").is_err());
    }

    #[test]
    fn round_trip() {
        for s in [
            "FREQ=MONTHLY;BYDAY=2TU;COUNT=10",
            "FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH",
            "FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1",
            "FREQ=YEARLY;BYMONTHDAY=-1;BYMONTH=2;WKST=SU",
            "FREQ=DAILY;UNTIL=20261231T235959",
        ] {
            let r = Rule::parse(s).unwrap();
            assert_eq!(Rule::parse(&r.to_rrule()).unwrap(), r, "{s}");
        }
    }

    #[test]
    fn describes_what_it_understood() {
        assert_eq!(Rule::parse("FREQ=MONTHLY;BYDAY=2TU").unwrap().describe(), "every month on second Tuesday");
        assert_eq!(
            Rule::parse("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH;COUNT=6").unwrap().describe(),
            "every 2 weeks on Monday and Thursday, 6 times"
        );
    }

    #[test]
    fn plain_english() {
        let p = |s| from_plain(s).map(|r| r.to_rrule());
        assert_eq!(p("every weekday").as_deref(), Some("FREQ=WEEKLY;BYDAY=MO,TU,WE,TH,FR"));
        assert_eq!(p("the last friday of every month").as_deref(), Some("FREQ=MONTHLY;BYDAY=-1FR"));
        assert_eq!(p("second tuesday of the month").as_deref(), Some("FREQ=MONTHLY;BYDAY=2TU"));
        assert_eq!(p("every 2 weeks on monday and thursday").as_deref(), Some("FREQ=WEEKLY;INTERVAL=2;BYDAY=MO,TH"));
        assert_eq!(p("every other tuesday").as_deref(), Some("FREQ=WEEKLY;INTERVAL=2;BYDAY=TU"));
        assert_eq!(p("on the 15th of every month").as_deref(), Some("FREQ=MONTHLY;BYMONTHDAY=15"));
        assert_eq!(p("last weekday of the month").as_deref(), Some("FREQ=MONTHLY;BYDAY=MO,TU,WE,TH,FR;BYSETPOS=-1"));
        assert_eq!(p("every year").as_deref(), Some("FREQ=YEARLY"));
        // a clock time is not a day of the month
        assert_eq!(p("the last friday of every month at 4pm").as_deref(), Some("FREQ=MONTHLY;BYDAY=-1FR"));
        assert_eq!(p("sometime soon"), None);
    }
}
