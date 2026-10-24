//! A market "what if", read-only (Plan F, 24 Oct 2026, as a proof slice).
//!
//! Given a hypothetical position -- in at 100, stop 95, target 110, over 20
//! bars -- replay every past stretch of that length in a bars file you point
//! at, scaled to the entry, and count how often the target came first, the
//! stop came first, or neither. That is the exact limit of a Monte Carlo
//! over history: every window once, so the same file always gives the same
//! odds.
//!
//! Analysis only, by construction. This module reads numbers it is handed
//! and returns words; it has no way to reach a broker, a network, or the
//! screen, and `tests/what_if_is_read_only.rs` holds it to that. Charter s8:
//! no trades, ever. Every answer says it is a simulation over past prices,
//! not a forecast or advice, and names the dates the history covers.

use crate::market::bars::{AsOf, Bars};

/// Fewer windows than this and the odds are noise; refuse rather than print
/// a confident-looking percentage.
const MIN_WINDOWS: usize = 30;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scenario {
    pub entry: f64,
    pub stop: f64,
    pub target: f64,
    /// How many bars to give it.
    pub horizon: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Odds {
    pub windows: usize,
    pub target_first: usize,
    pub stop_first: usize,
    pub neither: usize,
    /// Windows where one bar reached both; counted as the stop.
    pub same_bar: usize,
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,
}

/// Replay every past window of `s.horizon` bars from `view`.
pub fn replay(view: &AsOf, s: &Scenario) -> Result<Odds, String> {
    let long = s.stop < s.entry && s.entry < s.target;
    let short = s.target < s.entry && s.entry < s.stop;
    if !long && !short {
        return Err("The stop and target need to sit either side of the entry.".into());
    }
    if s.horizon == 0 || s.entry <= 0.0 {
        return Err("I need a positive entry and at least one bar.".into());
    }
    let (high, low, close) = (view.high(), view.low(), view.close());
    let n = close.len();
    let windows = n.saturating_sub(s.horizon);
    if windows < MIN_WINDOWS {
        return Err(format!(
            "That history has {n} bars -- {windows} windows of {} bars, and I want at least {MIN_WINDOWS} before giving odds.",
            s.horizon
        ));
    }
    let mut o = Odds {
        windows,
        target_first: 0,
        stop_first: 0,
        neither: 0,
        same_bar: 0,
        from_ms: view.time().first().copied(),
        to_ms: view.time().last().copied(),
    };
    for start in 0..windows {
        let k = s.entry / close[start];
        let mut outcome = None;
        for j in start + 1..=start + s.horizon {
            let (hi, lo) = (high[j] * k, low[j] * k);
            let (stopped, reached) = if long {
                (lo <= s.stop, hi >= s.target)
            } else {
                (hi >= s.stop, lo <= s.target)
            };
            if stopped && reached {
                o.same_bar += 1;
            }
            if stopped {
                outcome = Some(false);
                break;
            }
            if reached {
                outcome = Some(true);
                break;
            }
        }
        match outcome {
            Some(true) => o.target_first += 1,
            Some(false) => o.stop_first += 1,
            None => o.neither += 1,
        }
    }
    Ok(o)
}

fn date(ms: i64) -> String {
    let (y, m, d) =
        crate::market::time::civil_from_days(ms.div_euclid(crate::market::time::MS_PER_DAY));
    format!("{y}-{m:02}-{d:02}")
}

/// The odds in words, labelled for what they are.
pub fn said(o: &Odds, s: &Scenario) -> String {
    let pct = |k: usize| (k as f64 * 100.0 / o.windows as f64).round() as u32;
    let span = match (o.from_ms, o.to_ms) {
        (Some(a), Some(b)) => format!(", {} to {}", date(a), date(b)),
        _ => " (the file has no dates, so I can't say how recent it is)".into(),
    };
    let mut out = format!(
        "Simulated, not a forecast or advice -- nothing is bought or sold. Replaying all {} past {}-bar stretches of that history{span}, from {}: target {} first in {}%, stop {} first in {}%, neither in {}%.",
        o.windows,
        s.horizon,
        s.entry,
        s.target,
        pct(o.target_first),
        s.stop,
        pct(o.stop_first),
        pct(o.neither)
    );
    if o.same_bar > 0 {
        out.push_str(&format!(
            " In {} of them one bar reached both; I counted the stop.",
            o.same_bar
        ));
    }
    out.push_str(" Past price paths only: news, gaps between bars and costs aren't in it.");
    out
}

/// Bars from a CSV: `time_ms,open,high,low,close` or `open,high,low,close`
/// per line. A header line is skipped; any other unreadable line is refused,
/// not dropped, since a silently shortened history gives different odds.
pub fn bars_from_csv(text: &str) -> Result<Bars, String> {
    let (mut t, mut o, mut h, mut l, mut c) = (vec![], vec![], vec![], vec![], vec![]);
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let nums: Result<Vec<f64>, _> = line.split(',').map(|f| f.trim().parse::<f64>()).collect();
        let nums = match nums {
            Ok(v) => v,
            Err(_) if i == 0 => continue,
            Err(_) => return Err(format!("Line {} isn't numbers: {line}", i + 1)),
        };
        match nums.as_slice() {
            [tm, op, hi, lo, cl] => {
                t.push(*tm as i64);
                o.push(*op);
                h.push(*hi);
                l.push(*lo);
                c.push(*cl);
            }
            [op, hi, lo, cl] => {
                o.push(*op);
                h.push(*hi);
                l.push(*lo);
                c.push(*cl);
            }
            _ => return Err(format!("Line {} needs 4 or 5 columns: {line}", i + 1)),
        }
    }
    if !t.is_empty() && t.len() != c.len() {
        return Err("Some lines have a time and some don't.".into());
    }
    Bars::new(o, h, l, c, t).map_err(|r| r.0)
}

/// "what if ... at 100 ... stop 95 ... target 110 [over 20 bars] ... <file>.csv"
pub fn parse(said: &str) -> Option<(Scenario, String)> {
    let lower = said.trim().to_lowercase();
    if !lower.starts_with("what if") || !lower.contains("stop") || !lower.contains("target") {
        return None;
    }
    let end = lower.find(".csv")? + 4;
    let words: Vec<&str> = lower[..end].split_whitespace().collect();
    let path_start = words
        .iter()
        .rposition(|w| *w == "using" || *w == "on" || *w == "in" || *w == "from")?;
    let path = said.trim()[..end]
        .split_whitespace()
        .skip(path_start + 1)
        .collect::<Vec<_>>()
        .join(" ")
        .trim_matches('"')
        .to_string();
    let after = |key: &str| -> Option<f64> {
        let i = words[..path_start].iter().position(|w| *w == key)?;
        words.get(i + 1)?.trim_end_matches([',', '.']).parse().ok()
    };
    let entry = after("at").or_else(|| after("entry"))?;
    let stop = after("stop")?;
    let target = after("target")?;
    let horizon = after("over").map(|h| h as usize).unwrap_or(20);
    Some((
        Scenario {
            entry,
            stop,
            target,
            horizon,
        },
        path,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::market::fixtures::{ramp, walk};

    #[test]
    fn a_rising_history_reaches_a_long_target_and_never_the_stop() {
        let b = ramp(200, 0.01);
        let s = Scenario {
            entry: 100.0,
            stop: 95.0,
            target: 101.0,
            horizon: 20,
        };
        let o = replay(&b.latest().unwrap(), &s).unwrap();
        assert_eq!(o.windows, 180);
        assert_eq!(o.stop_first, 0, "{o:?}");
        assert!(o.target_first > 0, "{o:?}");
        assert_eq!(o.target_first + o.stop_first + o.neither, o.windows);
    }

    #[test]
    fn the_same_history_always_gives_the_same_odds_and_a_short_mirrors_a_long() {
        let b = walk(600, 7);
        let long = Scenario {
            entry: 100.0,
            stop: 99.0,
            target: 101.0,
            horizon: 30,
        };
        let a = replay(&b.latest().unwrap(), &long).unwrap();
        assert_eq!(a, replay(&b.latest().unwrap(), &long).unwrap());
        let short = Scenario {
            entry: 100.0,
            stop: 101.0,
            target: 99.0,
            horizon: 30,
        };
        let s = replay(&b.latest().unwrap(), &short).unwrap();
        assert_eq!(s.windows, a.windows);
    }

    #[test]
    fn too_little_history_or_a_muddled_scenario_is_refused() {
        let b = ramp(40, 0.01);
        let s = Scenario {
            entry: 100.0,
            stop: 95.0,
            target: 110.0,
            horizon: 20,
        };
        assert!(replay(&b.latest().unwrap(), &s)
            .unwrap_err()
            .contains("at least 30"));
        let muddled = Scenario {
            entry: 100.0,
            stop: 105.0,
            target: 110.0,
            horizon: 5,
        };
        assert!(replay(&ramp(200, 0.01).latest().unwrap(), &muddled).is_err());
    }

    #[test]
    fn every_answer_is_labelled_a_simulation_and_dated() {
        let o = Odds {
            windows: 100,
            target_first: 30,
            stop_first: 50,
            neither: 20,
            same_bar: 2,
            from_ms: Some(0),
            to_ms: Some(86_400_000 * 365),
        };
        let s = Scenario {
            entry: 100.0,
            stop: 95.0,
            target: 110.0,
            horizon: 20,
        };
        let w = said(&o, &s);
        assert!(
            w.starts_with("Simulated, not a forecast or advice -- nothing is bought or sold."),
            "{w}"
        );
        assert!(w.contains("1970-01-01 to 1971-01-01"), "{w}");
        assert!(
            w.contains("target 110 first in 30%, stop 95 first in 50%, neither in 20%"),
            "{w}"
        );
        assert!(w.contains("I counted the stop"), "{w}");
    }

    #[test]
    fn a_csv_is_read_with_or_without_times_and_a_bad_line_is_refused() {
        let b =
            bars_from_csv("time,open,high,low,close\n0,1,2,0.5,1.5\n60000,1.5,2,1,1.8\n").unwrap();
        assert_eq!(b.len(), 2);
        assert_eq!(bars_from_csv("1,2,0.5,1.5\n1.5,2,1,1.8").unwrap().len(), 2);
        assert!(bars_from_csv("1,2,0.5,1.5\nx,2,1,1.8")
            .unwrap_err()
            .contains("Line 2"));
    }

    #[test]
    fn the_question_is_read_from_plain_words() {
        let (s, path) = parse("What if I'm in at 100 with stop 95 and target 110 over 15 bars using C:\\data\\eurusd.csv").unwrap();
        assert_eq!(
            s,
            Scenario {
                entry: 100.0,
                stop: 95.0,
                target: 110.0,
                horizon: 15
            }
        );
        assert_eq!(path, "C:\\data\\eurusd.csv");
        assert!(parse("what if it rains tomorrow").is_none());
        assert!(
            parse("what if at 100 stop 95 target 110").is_none(),
            "no file, no answer"
        );
    }
}
