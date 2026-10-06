//! Words to a time: "tomorrow at 3", "in 20 minutes", "next Tuesday
//! afternoon", "the 14th at noon", "Oct 3 from 2 to 4pm", "end of day".
//!
//! **Sources:** the shape is chrono's (`wanasit/chrono`, MIT; its README and
//! parser list read as the reference, clean-room): small parsers, each for one
//! kind of expression — a day, a date, a clock time, an offset, a length — and
//! a refining step that puts what they found together. Its refinement for a
//! bare hour ("at 3" means the afternoon) is followed, and said.
//!
//! **Honest about doubt.** Every result says whether it is `sure`. It is not
//! sure — and says why — when the words allow two readings a person could
//! mean: "next Monday" said on a Sunday (tomorrow, or a week tomorrow?), a
//! numeric date like 3/4 (March or April?), "a few days", "next week"
//! with no day. A calendar asks rather than guesses when it isn't sure; a
//! time that was filled in ("tonight" with no hour is 8 pm, a bare "at 3" is
//! 3 pm) is marked `guessed` so it can be said back.
//!
//! Every time here is **local seconds** (Unix seconds plus the local offset),
//! the convention `calendar` and `civil` use.

use crate::civil::{days_from_civil, days_in_month, Civil};

const DAY: u64 = 86_400;

/// What the words said.
#[derive(Debug, Clone, PartialEq)]
pub struct Parsed {
    /// Local seconds.
    pub start: u64,
    /// No clock time: the whole day.
    pub all_day: bool,
    /// A length, when one was given ("for an hour", "from 2 to 4").
    pub mins: Option<u64>,
    /// False when the words allow two readings; `why` says which.
    pub sure: bool,
    pub why: Option<String>,
    /// A time or a meridiem filled in rather than said.
    pub guessed: bool,
    /// A day was said (or implied by an offset); false for a bare "at 5pm".
    pub day_said: bool,
}

const MONTHS: [(&str, u32); 23] = [
    ("january", 1), ("february", 2), ("march", 3), ("april", 4), ("may", 5), ("june", 6), ("july", 7),
    ("august", 8), ("september", 9), ("october", 10), ("november", 11), ("december", 12),
    ("jan", 1), ("feb", 2), ("mar", 3), ("apr", 4), ("jun", 6), ("jul", 7), ("aug", 8), ("sep", 9), ("sept", 9),
    ("oct", 10), ("nov", 11),
];
const DEC: (&str, u32) = ("dec", 12);
const WEEKDAYS: [(&str, u32); 14] = [
    ("monday", 0), ("tuesday", 1), ("wednesday", 2), ("thursday", 3), ("friday", 4), ("saturday", 5), ("sunday", 6),
    ("mon", 0), ("tue", 1), ("tues", 1), ("wed", 2), ("thu", 3), ("thurs", 3), ("fri", 4),
];
const WEEKEND_SHORT: [(&str, u32); 2] = [("sat", 5), ("sun", 6)];

/// The part of the day the words set, which decides a bare hour's half.
#[derive(Clone, Copy, PartialEq)]
enum Ctx {
    Morning,
    /// Afternoon or evening: 1 to 11 is pm.
    Pm,
    /// "Tonight": 5 to 11 is pm, 12 is midnight, 1 to 4 is the small hours
    /// that follow (moved onto the next day once the day is known).
    Night,
}

fn month_of(w: &str) -> Option<u32> {
    MONTHS.iter().chain(std::iter::once(&DEC)).find(|(n, _)| *n == w).map(|(_, m)| *m)
}

fn weekday_of(w: &str) -> Option<u32> {
    WEEKDAYS.iter().chain(WEEKEND_SHORT.iter()).find(|(n, _)| *n == w).map(|(_, d)| *d)
}

/// Small numbers in words and digits; "a"/"an" is one.
fn number(w: &str) -> Option<f64> {
    let words = [
        ("a", 1.0), ("an", 1.0), ("one", 1.0), ("two", 2.0), ("three", 3.0), ("four", 4.0), ("five", 5.0),
        ("six", 6.0), ("seven", 7.0), ("eight", 8.0), ("nine", 9.0), ("ten", 10.0), ("eleven", 11.0),
        ("twelve", 12.0), ("fifteen", 15.0), ("twenty", 20.0), ("thirty", 30.0), ("forty", 40.0),
        ("forty-five", 45.0), ("fifty", 50.0), ("ninety", 90.0), ("couple", 2.0),
    ];
    if let Some((_, n)) = words.iter().find(|(x, _)| *x == w) {
        return Some(*n);
    }
    w.parse::<f64>().ok().filter(|n| n.is_finite() && *n >= 0.0)
}

fn unit_secs(w: &str) -> Option<u64> {
    let w = w.trim_end_matches('s');
    Some(match w {
        "second" | "sec" => 1,
        "minute" | "min" | "mn" => 60,
        "hour" | "hr" | "h" => 3600,
        "day" => DAY,
        "week" | "wk" => 7 * DAY,
        "fortnight" => 14 * DAY,
        "month" => 30 * DAY,
        _ => return None,
    })
}

/// Words, lower-cased, with punctuation that carries no time dropped.
fn words(text: &str) -> Vec<String> {
    // A full stop between digits is a decimal point ("1.5 hours"); anywhere
    // else it ends a sentence.
    let lower = text.to_lowercase();
    let b = lower.as_bytes();
    let kept: String = lower
        .char_indices()
        .map(|(i, c)| {
            let decimal = c == '.' && i > 0 && b[i - 1].is_ascii_digit() && b.get(i + 1).map(|x| x.is_ascii_digit()).unwrap_or(false);
            if matches!(c, '.' | ',' | ';' | '!' | '?' | '(' | ')' | '"') && !decimal { ' ' } else { c }
        })
        .collect();
    let t = kept;
    let t = t.replace("o'clock", " oclock ").replace("o clock", " oclock ");
    let mut out = Vec::new();
    let t = t.replace(['\u{2013}', '\u{2014}'], "-");
    for whole in t.split_whitespace() {
        let whole = whole.trim_matches('\'');
        // "2pm-4pm", "2:00pm-3:30pm": a range whose parts each carry a
        // meridiem splits on the dash first, so each part reads as a time.
        let parts: Vec<&str> = whole.split('-').collect();
        let each_a_time = parts.len() == 2
            && parts.iter().all(|p| {
                let core = p.trim_end_matches("pm").trim_end_matches("am");
                !core.is_empty() && core.chars().all(|c| c.is_ascii_digit() || c == ':')
            })
            && parts.iter().any(|p| p.ends_with("am") || p.ends_with("pm"));
        let pieces: Vec<&str> = if each_a_time { parts } else { vec![whole] };
        for (k, w) in pieces.iter().enumerate() {
        if k > 0 {
            out.push("to".to_string());
        }
        let w: &str = w;
        // "3pm", "10am", "3:30pm", "2-4pm": split the meridiem off.
        let (core, mer) = if let Some(c) = w.strip_suffix("pm").or_else(|| w.strip_suffix("p")).filter(|c| c.chars().any(|ch| ch.is_ascii_digit()) && c.chars().last().map(|ch| ch.is_ascii_digit()).unwrap_or(false)) {
            (c.to_string(), Some("pm"))
        } else if let Some(c) = w.strip_suffix("am").or_else(|| w.strip_suffix("a")).filter(|c| c.chars().any(|ch| ch.is_ascii_digit()) && c.chars().last().map(|ch| ch.is_ascii_digit()).unwrap_or(false)) {
            (c.to_string(), Some("am"))
        } else {
            (w.to_string(), None)
        };
        // "2-4" (a range) splits into "2", "to", "4"; ISO dates keep theirs.
        if core.contains('-') && !is_iso(&core) && core.split('-').all(|p| p.chars().all(|c| c.is_ascii_digit() || c == ':') && !p.is_empty()) {
            let parts: Vec<&str> = core.split('-').collect();
            for (i, p) in parts.iter().enumerate() {
                if i > 0 {
                    out.push("to".to_string());
                }
                out.push(p.to_string());
            }
        } else {
            out.push(core);
        }
        if let Some(m) = mer {
            out.push(m.to_string());
        }
        }
    }
    out
}

fn is_iso(w: &str) -> bool {
    let p: Vec<&str> = w.split('-').collect();
    p.len() == 3 && p[0].len() == 4 && p.iter().all(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()))
}

/// "14th" → 14.
fn ordinal(w: &str) -> Option<u32> {
    let digits: String = w.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &w[digits.len()..];
    if digits.is_empty() || !(rest.is_empty() || ["st", "nd", "rd", "th"].contains(&rest)) {
        return None;
    }
    digits.parse().ok().filter(|d| (1..=31).contains(d))
}

/// A clock time at `i`: returns (minute of day, words used, guessed).
fn clock_at(w: &[String], i: usize, context_pm: Option<Ctx>) -> Option<(u32, usize, bool)> {
    let tok = w.get(i)?;
    let next = w.get(i + 1).map(|s| s.as_str());
    match tok.as_str() {
        "noon" | "midday" => return Some((12 * 60, 1, false)),
        // Midnight is the end of the day it's said with: "tonight at
        // midnight" is the start of tomorrow.
        "midnight" => return Some((24 * 60, 1, false)),
        "half" | "quarter" if next == Some("past") || next == Some("to") => {
            let (h, used) = hour_word(w.get(i + 2)?)?;
            let mins = if tok == "half" { 30 } else { 15 };
            // The hour's half first ("quarter to one" is 12:45, not 00:45),
            // then the quarter before or after it.
            let (hh, guessed) = meridiem(h, w.get(i + 2 + used).map(|s| s.as_str()), context_pm);
            let at = if next == Some("past") { hh * 60 + mins } else { (hh * 60 + 24 * 60 - mins) % (24 * 60) };
            let extra = if matches!(w.get(i + 2 + used).map(|s| s.as_str()), Some("am" | "pm")) { 1 } else { 0 };
            return Some((at, 2 + used + extra, guessed));
        }
        _ => {}
    }
    // 15:30, 3:30, 1530? (no: four bare digits are a year)
    let (h, m, had_colon) = if let Some((a, b)) = tok.split_once(':') {
        (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?, true)
    } else {
        (tok.parse::<u32>().ok()?, 0, false)
    };
    if m > 59 || h > 23 {
        return None;
    }
    let mut used = 1;
    if next == Some("oclock") {
        used += 1;
    }
    match w.get(i + used).map(|s| s.as_str()) {
        Some("pm") => return Some((((h % 12) + 12) * 60 + m, used + 1, false)),
        Some("am") => return Some(((h % 12) * 60 + m, used + 1, false)),
        _ => {}
    }
    if had_colon && h >= 13 || h == 0 && had_colon {
        return Some((h * 60 + m, used, false));
    }
    let after_word = w.get(i.wrapping_sub(1)).map(|p| matches!(p.as_str(), "at" | "by" | "from" | "until" | "till" | "between" | "around" | "about")).unwrap_or(false);
    // "2-4pm": a bare number that starts a range ending in am/pm.
    let starts_range = next == Some("to")
        && w.get(i + 2).map(|n| n.parse::<u32>().is_ok() || n.contains(':')).unwrap_or(false)
        && matches!(w.get(i + 3).map(|s| s.as_str()), Some("am" | "pm"));
    if !had_colon && used == 1 && !after_word && !starts_range {
        // A bare number that isn't after "at": not a time.
        return None;
    }
    let (hh, guessed) = meridiem(h, None, context_pm);
    Some((hh * 60 + m, used, guessed))
}

fn hour_word(w: &str) -> Option<(u32, usize)> {
    let n = number(w)? as u32;
    (1..=12).contains(&n).then_some((n, 1))
}

/// A 12-hour reading of `h` with no am/pm: the day's context if there is
/// one ("tonight at 8"), else chrono's rule — 1 to 6 is the afternoon —
/// and 7 to 11 the morning. Marked guessed.
fn meridiem(h: u32, said: Option<&str>, context_pm: Option<Ctx>) -> (u32, bool) {
    match said {
        Some("pm") => return ((h % 12) + 12, false),
        Some("am") => return (h % 12, false),
        _ => {}
    }
    if h == 0 || h > 12 {
        return (h, false);
    }
    match context_pm {
        // "tonight at 12" is midnight; "tonight at 1" is the small hours.
        Some(Ctx::Night) if h == 12 => (0, false),
        Some(Ctx::Night) if h <= 4 => (h, false),
        Some(Ctx::Night) => (h + 12, false),
        _ if h == 12 => (12, false),
        Some(Ctx::Pm) => (h + 12, false),
        Some(Ctx::Morning) => (h, false),
        None if (1..=6).contains(&h) => (h + 12, true),
        None => (h, true),
    }
}

/// Parse `text` against `now` (local seconds).
pub fn parse(text: &str, now: u64) -> Option<Parsed> {
    let w = words(text);
    let today = now - now % DAY;
    let nowc = Civil::from_local(now as i64);
    let wd_today = nowc.weekday();

    let mut day: Option<u64> = None;
    let mut minute: Option<u32> = None;
    let mut mins: Option<u64> = None;
    let mut sure = true;
    let mut why: Option<String> = None;
    let mut guessed = false;
    let mut offset: Option<u64> = None;
    let unsure = |s: &mut bool, why: &mut Option<String>, reason: String| {
        *s = false;
        why.get_or_insert(reason);
    };

    let has = |x: &str| w.iter().any(|t| t == x);
    let phrase = |p: &str| {
        let pw: Vec<&str> = p.split(' ').collect();
        w.windows(pw.len()).any(|win| win.iter().zip(&pw).all(|(a, b)| a == b))
    };

    // Part of the day, which also decides a bare hour's am/pm.
    let context_pm = if has("tonight") {
        Some(Ctx::Night)
    } else if has("evening") || has("afternoon") || phrase("this evening") {
        Some(Ctx::Pm)
    } else if has("morning") {
        Some(Ctx::Morning)
    } else {
        None
    };

    let mut i = 0;
    while i < w.len() {
        let t = w[i].as_str();
        let prev = if i > 0 { w[i - 1].as_str() } else { "" };
        let next = w.get(i + 1).map(|s| s.as_str()).unwrap_or("");

        // ---- offsets: "in 20 minutes", "in half an hour", "2 days from now"
        if t == "in" || t == "within" {
            let mut j = i + 1;
            let mut n = None;
            if w.get(j).map(|s| s == "half").unwrap_or(false) && w.get(j + 1).map(|s| s == "an" || s == "a").unwrap_or(false) {
                n = Some(0.5);
                j += 2;
            } else if let Some(x) = w.get(j).and_then(|s| number(s)) {
                n = Some(x);
                j += 1;
                if w.get(j).map(|s| s == "couple").unwrap_or(false) {
                    n = Some(2.0);
                    j += 1;
                }
                if w.get(j).map(|s| s == "of").unwrap_or(false) {
                    j += 1;
                }
                if w.get(j).map(|s| s == "and" || s == "&").unwrap_or(false) && w.get(j + 1).map(|s| s == "a").unwrap_or(false) && w.get(j + 2).map(|s| s == "half").unwrap_or(false) {
                    n = n.map(|x| x + 0.5);
                    j += 3;
                }
            } else if w.get(j).map(|s| s == "few").unwrap_or(false) || (w.get(j).map(|s| s == "a").unwrap_or(false) && w.get(j + 1).map(|s| s == "few").unwrap_or(false)) {
                j += if w[j] == "a" { 2 } else { 1 };
                n = Some(3.0);
                unsure(&mut sure, &mut why, "\"a few\" isn't a number — how many?".into());
            }
            if let (Some(mut n), Some(u)) = (n, w.get(j).and_then(|s| unit_secs(s))) {
                // "an hour and a half"
                if phrase_at(&w, j + 1, "and a half") {
                    n += 0.5;
                    j += 3;
                }
                let secs = (n * u as f64).round() as u64;
                offset = Some(secs);
                if u >= DAY {
                    // "in 3 days": that day; a clock time said with it ("in 3
                    // days at 5") refines it.
                    day = Some(if u == 30 * DAY { months_on(today, n) } else { (now + secs) - (now + secs) % DAY });
                }
                i = j + 1;
                continue;
            }
        }
        if let (Some(n), Some(u)) = (number(t), unit_secs(next)) {
            let after = w.get(i + 2).map(|s| s.as_str());
            let after2 = w.get(i + 3).map(|s| s.as_str());
            if after == Some("from") && matches!(after2, Some("now" | "today")) || after == Some("later") {
                let secs = (n * u as f64).round() as u64;
                offset = Some(secs);
                if u >= DAY {
                    day = Some(if u == 30 * DAY { months_on(today, n) } else { (now + secs) - (now + secs) % DAY });
                }
                // Past "from now"/"from today" too, so "today" isn't read again.
                i += if after == Some("from") { 4 } else { 3 };
                continue;
            }
        }
        if t == "fortnight" && prev == "a" && next == "from" {
            day = Some(today + 14 * DAY);
            i += 2;
            continue;
        }

        // ---- days
        match t {
            "today" => day = Some(today),
            "tonight" => {
                day = Some(today);
            }
            "tomorrow" | "tmrw" | "tmr" => {
                if prev == "after" && i >= 2 && w[i - 2] == "day" {
                    day = Some(today + 2 * DAY);
                } else {
                    day = Some(today + DAY);
                    if nowc.hour < 4 {
                        unsure(&mut sure, &mut why, "it's past midnight -- \"tomorrow\" as in later today, or the day after?".into());
                    }
                }
            }
            "yesterday" => day = Some(today.saturating_sub(DAY)),
            "weekend" if matches!(prev, "this" | "the" | "next" | "at") => {
                let to_sat = (5 + 7 - wd_today) % 7;
                let mut d = today + to_sat as u64 * DAY;
                if prev == "next" {
                    if to_sat <= 1 || wd_today == 6 {
                        d += 7 * DAY;
                    }
                    unsure(&mut sure, &mut why, "\"next weekend\" — this coming one or the one after?".into());
                }
                day = Some(d);
            }
            "week" if prev == "next" && !has("in") => {
                let to_mon = (7 - wd_today) % 7;
                day = Some(today + if to_mon == 0 { 7 } else { to_mon } as u64 * DAY);
                unsure(&mut sure, &mut why, "next week — which day?".into());
            }
            "eod" => {
                day.get_or_insert(today);
                minute.get_or_insert(17 * 60);
                guessed = true;
            }
            _ => {}
        }
        if phrase_at(&w, i, "end of day") || phrase_at(&w, i, "end of the day") || phrase_at(&w, i, "close of business") {
            day.get_or_insert(today);
            minute.get_or_insert(17 * 60);
            guessed = true;
        }
        if phrase_at(&w, i, "end of the week") || phrase_at(&w, i, "end of week") {
            let to_fri = (4 + 7 - wd_today) % 7;
            day = Some(today + to_fri as u64 * DAY);
        }
        if phrase_at(&w, i, "end of the month") || phrase_at(&w, i, "end of month") {
            let last = days_in_month(nowc.year, nowc.month);
            day = Some(days_from_civil(nowc.year, nowc.month, last) as u64 * DAY);
        }
        if phrase_at(&w, i, "first thing") {
            minute.get_or_insert(9 * 60);
            guessed = true;
        }

        // ---- weekdays
        let short_ok = t.len() > 4 || matches!(prev, "on" | "this" | "next" | "last" | "by" | "until" | "till" | "from");
        if let Some(target) = weekday_of(t).filter(|_| short_ok) {
            // "sat"/"sun" only as a day when nothing else reads them.
            let ahead = ((target + 7 - wd_today) % 7) as u64;
            let d = match prev {
                "last" => {
                    let back = if ahead == 0 { 7 } else { 7 - ahead };
                    today.saturating_sub(back * DAY)
                }
                "this" => today + ahead * DAY,
                "next" => {
                    let a = if ahead == 0 { 7 } else { ahead };
                    if a <= 2 {
                        unsure(&mut sure, &mut why, format!("\"next {t}\" — the one in {a} day{}, or the week after?", if a == 1 { "" } else { "s" }));
                    }
                    today + a * DAY
                }
                _ => {
                    // "on Monday" said on a Monday means next week's.
                    today + if ahead == 0 { 7 } else { ahead } * DAY
                }
            };
            day = Some(d);
        }

        // ---- dates: "14th", "the 3rd", "oct 3", "3 october", "october 3rd 2026"
        if let Some(m) = month_of(t) {
            let (dnum, year) = if let Some(d) = w.get(i + 1).and_then(|s| ordinal(s)) {
                (Some(d), w.get(i + 2).and_then(|s| s.parse::<i64>().ok()).filter(|y| (1970..2200).contains(y)))
            } else if let Some(d) = (if prev == "of" { w.get(i.wrapping_sub(2)) } else { Some(&w[i.saturating_sub(1)]) }).filter(|_| i > 0).and_then(|s| ordinal(s)) {
                (Some(d), w.get(i + 1).and_then(|s| s.parse::<i64>().ok()).filter(|y| (1970..2200).contains(y)))
            } else {
                (None, None)
            };
            if let Some(d) = dnum {
                if let Some(v) = date_day(nowc.year, m, d, year, today) {
                    day = Some(v);
                }
            }
        } else if let Some(d) = ordinal(t).filter(|_| {
            // "the 14th", "on the 3rd", "14th" — but not "2 hours" or a time.
            let tail = t.chars().any(|c| c.is_alphabetic());
            (tail || prev == "the") && !w.get(i + 1).map(|n| month_of(n).is_some() || unit_secs(n).is_some()).unwrap_or(false)
                && !w.get(i.wrapping_sub(1)).map(|p| month_of(p).is_some() || p == "of").unwrap_or(false)
        }) {
            if !(w.get(i + 1).map(|s| s == "of").unwrap_or(false) && w.get(i + 2).map(|s| month_of(s).is_some()).unwrap_or(false)) {
                // The day of this month, or next month's if it has passed.
                let (mut y, mut m) = (nowc.year, nowc.month);
                if d < nowc.day {
                    m += 1;
                    if m > 12 {
                        m = 1;
                        y += 1;
                    }
                }
                if d <= days_in_month(y, m) {
                    day = Some(days_from_civil(y, m, d) as u64 * DAY);
                }
            }
        }
        if is_iso(t) {
            let p: Vec<u32> = t.split('-').filter_map(|x| x.parse().ok()).collect();
            if p.len() == 3 && (1..=12).contains(&p[1]) && p[2] >= 1 && p[2] <= days_in_month(p[0] as i64, p[1]) {
                day = Some(days_from_civil(p[0] as i64, p[1], p[2]) as u64 * DAY);
            }
        } else if t.contains('/') {
            let p: Vec<u32> = t.split('/').filter_map(|x| x.parse().ok()).collect();
            if p.len() >= 2 && t.split('/').count() == p.len() {
                // US order (month/day), as this machine's owner writes it;
                // not sure when both readings are real dates.
                let (m, d) = (p[0], p[1]);
                let year = p.get(2).map(|y| if *y < 100 { 2000 + *y as i64 } else { *y as i64 });
                if (1..=12).contains(&m) && (1..=31).contains(&d) {
                    if let Some(v) = date_day(nowc.year, m, d, year, today) {
                        day = Some(v);
                        if d <= 12 && d != m {
                            unsure(&mut sure, &mut why, format!("is {t} month/day or day/month?"));
                        }
                    }
                }
            }
        }

        // ---- clock times
        if minute.is_none() || (w.get(i).map(|s| s == "noon" || s == "midnight").unwrap_or(false)) {
            if let Some((mm, used, g)) = clock_at(&w, i, context_pm) {
                // A range: "from 2 to 4pm", "2-4pm", "between 2 and 3".
                let to = w.get(i + used).map(|s| s.as_str());
                if matches!(to, Some("to" | "until" | "till" | "and")) {
                    let end_pm = w.get(i + used + 2).map(|s| s.as_str());
                    if let Some((end, used2, _)) = clock_at_bare(&w, i + used + 1, context_pm) {
                        let mut start = mm;
                        // "2 to 4pm": the start takes the end's half of the day
                        // -- unless that would put it after the end ("11 to
                        // 1pm" is 11 in the morning, not 11 at night).
                        if g && end_pm == Some("pm") && start < 12 * 60 && start + 12 * 60 < end {
                            start += 12 * 60;
                        }
                        if end > start {
                            mins = Some((end - start) as u64);
                        }
                        minute = Some(start);
                        guessed |= g && end_pm.is_none();
                        i += used + 1 + used2;
                        continue;
                    }
                }
                minute = Some(mm);
                guessed |= g;
                i += used;
                continue;
            }
        }

        // ---- lengths: "for 30 minutes", "for an hour", "for 1.5 hours"
        if t == "for" {
            if let (Some(n), Some(u)) = (w.get(i + 1).and_then(|s| number(s)), w.get(i + 2).and_then(|s| unit_secs(s))) {
                if u < DAY {
                    mins = Some(((n * u as f64) / 60.0).round() as u64);
                    i += 3;
                    continue;
                }
            }
            if w.get(i + 1).map(|s| s == "half").unwrap_or(false) && w.get(i + 3).map(|s| unit_secs(s) == Some(3600)).unwrap_or(false) {
                mins = Some(30);
                i += 4;
                continue;
            }
        }
        i += 1;
    }

    // Parts of the day as times, when no clock time was said.
    if minute.is_none() {
        let part = if has("tonight") {
            Some(20 * 60)
        } else if has("morning") {
            Some(9 * 60)
        } else if has("afternoon") {
            Some(14 * 60)
        } else if has("evening") {
            Some(18 * 60)
        } else if has("lunch") || has("lunchtime") {
            Some(12 * 60)
        } else {
            None
        };
        if let Some(p) = part {
            minute = Some(p);
            guessed = true;
        }
    }

    // "tonight at 1", "quarter to one tonight": the small hours after tonight.
    if has("tonight") {
        if let Some(m) = minute.filter(|m| *m < 5 * 60) {
            minute = Some(m + 24 * 60);
        }
    }

    if let Some(off) = offset {
        if day.is_none() || minute.is_none() {
            // Offsets of minutes and hours are a moment; of days, a day.
            if off < DAY {
                return Some(Parsed { start: now + off, all_day: false, mins, sure, why, guessed, day_said: true });
            }
        }
    }

    match (day, minute) {
        (None, None) => None,
        (Some(d), None) => Some(Parsed { start: d, all_day: true, mins, sure, why, guessed, day_said: true }),
        (Some(d), Some(m)) => {
            let start = d + m as u64 * 60;
            // A moment that has already gone ("today at 9am" said at 10:15)
            // is asked about, not booked -- unless the words look back.
            if start + mins.unwrap_or(0) * 60 <= now && !(has("yesterday") || has("last") || has("ago")) {
                unsure(&mut sure, &mut why, "that time has already passed today -- did you mean another day?".into());
            }
            Some(Parsed { start, all_day: false, mins, sure, why, guessed, day_said: true })
        }
        (None, Some(m)) => {
            // Only a time: today if it's still ahead, else tomorrow.
            let mut s = today + m as u64 * 60;
            if s <= now {
                s += DAY;
            }
            Some(Parsed { start: s, all_day: false, mins, sure, why, guessed, day_said: false })
        }
    }
}

/// The local midnight `n` calendar months after `today`, the day clamped to
/// the month's length (Jan 31 + 1 month is Feb 28, not Mar 2).
fn months_on(today: u64, n: f64) -> u64 {
    let c = Civil::from_local(today as i64);
    let total = c.year * 12 + (c.month as i64 - 1) + n.round() as i64;
    let (y, m) = (total.div_euclid(12), (total.rem_euclid(12) + 1) as u32);
    let d = c.day.min(days_in_month(y, m));
    days_from_civil(y, m, d) as u64 * DAY
}

fn clock_at_bare(w: &[String], i: usize, context_pm: Option<Ctx>) -> Option<(u32, usize, bool)> {
    // The end of a range needs no "at" in front of it.
    let tok = w.get(i)?;
    if tok.contains(':') || tok.parse::<u32>().is_ok() {
        let mut v: Vec<String> = w.to_vec();
        v.insert(i, "at".into());
        return clock_at(&v, i + 1, context_pm);
    }
    clock_at(w, i, context_pm)
}

fn phrase_at(w: &[String], i: usize, p: &str) -> bool {
    let pw: Vec<&str> = p.split(' ').collect();
    w.len() >= i + pw.len() && w[i..i + pw.len()].iter().zip(&pw).all(|(a, b)| a == b)
}

/// The local midnight of month/day, this year or `year`; a date already past
/// with no year is next year's.
fn date_day(this_year: i64, m: u32, d: u32, year: Option<i64>, today: u64) -> Option<u64> {
    let y = year.unwrap_or(this_year);
    if d > days_in_month(y, m) {
        return None;
    }
    let mut v = days_from_civil(y, m, d) as u64 * DAY;
    if year.is_none() && v < today {
        if d > days_in_month(y + 1, m) {
            return None;
        }
        v = days_from_civil(y + 1, m, d) as u64 * DAY;
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wednesday 23 September 2026, 10:15 local.
    fn now() -> u64 {
        days_from_civil(2026, 9, 23) as u64 * DAY + 10 * 3600 + 15 * 60
    }

    fn at(y: i64, m: u32, d: u32, h: u32, min: u32) -> u64 {
        days_from_civil(y, m, d) as u64 * DAY + (h * 3600 + min * 60) as u64
    }

    #[test]
    fn the_everyday_ones() {
        assert_eq!(parse("tomorrow at 3pm", now()).unwrap().start, at(2026, 9, 24, 15, 0));
        assert_eq!(parse("in 20 minutes", now()).unwrap().start, now() + 1200);
        let p = parse("at 3", now()).unwrap();
        assert_eq!(p.start, at(2026, 9, 23, 15, 0));
        assert!(p.guessed);
        let n = parse("next monday", now()).unwrap();
        assert_eq!(n.start, at(2026, 9, 28, 0, 0));
        assert!(n.sure);
        assert!(!parse("3/4 at noon", now()).unwrap().sure);
    }
}
