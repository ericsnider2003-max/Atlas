//! Standing jobs on a clock: five-field cron expressions.
//!
//! **Source:** Vixie cron semantics, with `Hexagon/croner-rust` (MIT) read as
//! the reference for the extensions (`L`, `#`, nicknames) and for the
//! day-of-month/day-of-week rule. Clean-room.
//!
//! **Why Atlas wants it.** Idea #3 on the 22 Sep list — "every morning
//! summarise overnight trades", "hourly server health check" — needs a
//! schedule the user sets and Atlas keeps across restarts. `recur` is for
//! calendar events a person attends; this is for jobs Atlas runs, where
//! "every 15 minutes during market hours on weekdays" is one line.
//!
//! The one trap worth naming: when BOTH day-of-month and day-of-week are
//! restricted, classic cron fires when EITHER matches (`0 9 1 * MON` = the 1st
//! AND every Monday). Most people read it as AND. This keeps Vixie's OR (so a
//! crontab pasted from anywhere means what it meant there), accepts croner's
//! `+` prefix on the weekday field to ask for AND, and `describe()` says which
//! one it is doing.

use crate::civil::{days_in_month, Civil};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cron {
    minutes: [bool; 60],
    hours: [bool; 24],
    /// index 1..=31
    dom: [bool; 32],
    months: [bool; 13],
    /// 0 = Sunday … 6 = Saturday (cron order).
    dow: [bool; 7],
    dom_star: bool,
    dow_star: bool,
    /// `L` in day-of-month.
    last_dom: bool,
    /// `5#2` = second Friday.
    nth_dow: Vec<(u32, u32)>,
    /// `5L` = last Friday.
    last_dow: Vec<u32>,
    and_mode: bool,
    source: String,
}

const MONTHS: [&str; 12] = ["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const DOWS: [&str; 7] = ["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

impl Cron {
    pub fn parse(expr: &str) -> Result<Cron, String> {
        let e = expr.trim();
        let expanded = match e.to_ascii_lowercase().as_str() {
            "@hourly" => "0 * * * *",
            "@daily" | "@midnight" => "0 0 * * *",
            "@weekly" => "0 0 * * 0",
            "@monthly" => "0 0 1 * *",
            "@yearly" | "@annually" => "0 0 1 1 *",
            _ => e,
        };
        let f: Vec<&str> = expanded.split_whitespace().collect();
        if f.len() != 5 {
            return Err(format!("'{expr}' needs 5 fields (minute hour day month weekday), has {}", f.len()));
        }
        let mut c = Cron {
            minutes: [false; 60],
            hours: [false; 24],
            dom: [false; 32],
            months: [false; 13],
            dow: [false; 7],
            dom_star: f[2] == "*" || f[2] == "?",
            dow_star: f[4] == "*" || f[4] == "?",
            last_dom: false,
            nth_dow: vec![],
            last_dow: vec![],
            and_mode: false,
            source: expr.trim().to_string(),
        };
        for v in field(f[0], 0, 59, &[])? {
            c.minutes[v as usize] = true;
        }
        for v in field(f[1], 0, 23, &[])? {
            c.hours[v as usize] = true;
        }
        for part in f[2].split(',') {
            if part.eq_ignore_ascii_case("L") {
                c.last_dom = true;
            } else if part != "?" {
                for v in field(part, 1, 31, &[])? {
                    c.dom[v as usize] = true;
                }
            }
        }
        for v in field(f[3], 1, 12, &MONTHS)? {
            c.months[v as usize] = true;
        }
        let mut dow_field = f[4];
        if let Some(rest) = dow_field.strip_prefix('+') {
            c.and_mode = true;
            dow_field = rest;
            c.dow_star = rest == "*";
        }
        for part in dow_field.split(',') {
            let up = part.to_ascii_uppercase();
            if let Some((d, n)) = up.split_once('#') {
                let d = dow_value(d)?;
                let n: u32 = n.parse().map_err(|_| format!("'{part}': # needs 1-5"))?;
                if !(1..=5).contains(&n) {
                    return Err(format!("'{part}': # needs 1-5"));
                }
                c.nth_dow.push((d, n));
            } else if up.len() > 1 && up.ends_with('L') {
                c.last_dow.push(dow_value(&up[..up.len() - 1])?);
            } else if part != "?" {
                for v in field(part, 0, 7, &DOWS)? {
                    c.dow[(v % 7) as usize] = true; // 7 is Sunday too
                }
            }
        }
        Ok(c)
    }

    fn day_matches(&self, c: &Civil) -> bool {
        if !self.months[c.month as usize] {
            return false;
        }
        let len = days_in_month(c.year, c.month);
        let dom_hit = self.dom[c.day as usize] || (self.last_dom && c.day == len);
        // civil weekday is 0=Mon; cron is 0=Sun
        let wd = (c.weekday() + 1) % 7;
        let nth = (c.day - 1) / 7 + 1;
        let dow_hit = self.dow[wd as usize]
            || self.nth_dow.iter().any(|(d, n)| *d == wd && *n == nth)
            || self.last_dow.iter().any(|d| *d == wd && c.day + 7 > len);
        match (self.dom_star, self.dow_star) {
            (true, true) => true,
            (true, false) => dow_hit,
            (false, true) => dom_hit,
            (false, false) => {
                if self.and_mode {
                    dom_hit && dow_hit
                } else {
                    dom_hit || dow_hit
                }
            }
        }
    }

    /// The first firing strictly after `t` (local seconds), looking at most
    /// ~5 years ahead. `None` means the expression can never fire in that
    /// span (`0 0 30 2 *`) — the caller should say so, not wait forever.
    pub fn next_after(&self, t: i64) -> Option<i64> {
        let start = t.div_euclid(60) * 60 + 60; // next whole minute
        let first_day = start.div_euclid(86_400);
        for d in first_day..first_day + 366 * 5 {
            let c = Civil::from_local(d * 86_400);
            if !self.day_matches(&c) {
                continue;
            }
            for h in 0..24u32 {
                if !self.hours[h as usize] {
                    continue;
                }
                for m in 0..60u32 {
                    if !self.minutes[m as usize] {
                        continue;
                    }
                    let at = d * 86_400 + (h * 3600 + m * 60) as i64;
                    if at >= start {
                        return Some(at);
                    }
                }
            }
        }
        None
    }

    pub fn describe(&self) -> String {
        let mut s = format!("cron '{}'", self.source);
        if !self.dom_star && !self.dow_star {
            s.push_str(if self.and_mode {
                " — fires when the day of the month AND the weekday both match"
            } else {
                " — fires when EITHER the day of the month OR the weekday matches (classic cron)"
            });
        }
        s
    }
}

fn dow_value(s: &str) -> Result<u32, String> {
    if let Some(i) = DOWS.iter().position(|d| d.eq_ignore_ascii_case(s)) {
        return Ok(i as u32);
    }
    let v: u32 = s.parse().map_err(|_| format!("'{s}' is not a weekday"))?;
    if v > 7 {
        return Err(format!("weekday {v} out of range"));
    }
    Ok(v % 7)
}

fn value(s: &str, lo: u32, hi: u32, names: &[&str]) -> Result<u32, String> {
    if let Some(i) = names.iter().position(|n| n.eq_ignore_ascii_case(s)) {
        // month names are 1-based, weekday names 0-based
        return Ok(if lo == 1 { i as u32 + 1 } else { i as u32 });
    }
    let v: u32 = s.parse().map_err(|_| format!("'{s}' is not a number"))?;
    if v < lo || v > hi {
        return Err(format!("{v} is outside {lo}-{hi}"));
    }
    Ok(v)
}

fn field(spec: &str, lo: u32, hi: u32, names: &[&str]) -> Result<Vec<u32>, String> {
    let mut out = vec![];
    for part in spec.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, s)) => {
                let s: u32 = s.parse().map_err(|_| format!("step '{s}' is not a number"))?;
                if s == 0 {
                    return Err("a step of 0 never advances".into());
                }
                (r, s)
            }
            None => (part, 1),
        };
        let (a, b) = if range == "*" || range == "?" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (value(a, lo, hi, names)?, value(b, lo, hi, names)?)
        } else {
            let a = value(range, lo, hi, names)?;
            // "5/15" means 5 through the top, every 15.
            (a, if part.contains('/') { hi } else { a })
        };
        if a > b {
            return Err(format!("range {a}-{b} runs backwards"));
        }
        let mut v = a;
        while v <= b {
            out.push(v);
            v += step;
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn at(y: i64, mo: u32, d: u32, h: u32, mi: u32) -> i64 {
        Civil { year: y, month: mo, day: d, hour: h, minute: mi, second: 0 }.to_local()
    }
    fn show(t: i64) -> (i64, u32, u32, u32, u32) {
        let c = Civil::from_local(t);
        (c.year, c.month, c.day, c.hour, c.minute)
    }

    #[test]
    fn every_fifteen_in_market_hours_on_weekdays() {
        let c = Cron::parse("*/15 9-16 * * MON-FRI").unwrap();
        // Fri 2026-09-25 16:50 -> next is Mon 09:00
        assert_eq!(show(c.next_after(at(2026, 9, 25, 16, 50)).unwrap()), (2026, 9, 28, 9, 0));
        assert_eq!(show(c.next_after(at(2026, 9, 28, 9, 0)).unwrap()), (2026, 9, 28, 9, 15));
    }

    #[test]
    fn vixie_or_versus_plus_and() {
        let or = Cron::parse("0 9 1 * MON").unwrap();
        let and = Cron::parse("0 9 1 * +MON").unwrap();
        // From Wed 2026-09-23: OR fires Monday 28th; AND waits for a Monday the 1st (Feb 1 2027).
        assert_eq!(show(or.next_after(at(2026, 9, 23, 12, 0)).unwrap()), (2026, 9, 28, 9, 0));
        assert_eq!(show(and.next_after(at(2026, 9, 23, 12, 0)).unwrap()), (2027, 2, 1, 9, 0));
        assert!(or.describe().contains("EITHER"));
        assert!(and.describe().contains("AND"));
    }

    #[test]
    fn last_day_and_nth_weekday() {
        let l = Cron::parse("0 18 L * *").unwrap();
        assert_eq!(show(l.next_after(at(2026, 2, 1, 0, 0)).unwrap()), (2026, 2, 28, 18, 0));
        let second_fri = Cron::parse("30 8 * * 5#2").unwrap();
        assert_eq!(show(second_fri.next_after(at(2026, 9, 23, 0, 0)).unwrap()), (2026, 10, 9, 8, 30));
        let last_fri = Cron::parse("0 17 * * FRIL").unwrap();
        assert_eq!(show(last_fri.next_after(at(2026, 9, 1, 0, 0)).unwrap()), (2026, 9, 25, 17, 0));
    }

    #[test]
    fn sunday_is_zero_or_seven_and_nicknames() {
        let a = Cron::parse("0 0 * * 0").unwrap();
        let b = Cron::parse("0 0 * * 7").unwrap();
        let w = Cron::parse("@weekly").unwrap();
        let t = at(2026, 9, 23, 0, 0);
        assert_eq!(a.next_after(t), b.next_after(t));
        assert_eq!(a.next_after(t), w.next_after(t));
        assert_eq!(show(a.next_after(t).unwrap()), (2026, 9, 27, 0, 0));
    }

    #[test]
    fn never_fires_is_none_not_a_hang() {
        let c = Cron::parse("0 0 30 2 *").unwrap();
        assert_eq!(c.next_after(at(2026, 1, 1, 0, 0)), None);
    }

    #[test]
    fn bad_expressions_are_errors() {
        for bad in ["* * * *", "60 * * * *", "*/0 * * * *", "5-1 * * * *", "0 0 * * 8", "0 0 * * FRI#6"] {
            assert!(Cron::parse(bad).is_err(), "{bad}");
        }
    }
}
