//! What real bars have to satisfy before any reader is allowed to see them.
//!
//! ## Why this is built with no real bars to run it on
//!
//! Atlas has no bar feed of its own: bars arrive from whatever you point it at.
//! But the day real bars arrive is exactly the wrong day to start thinking
//! about what a bad bar looks like: everything will run, numbers will come out,
//! and nobody will know which of them were computed across a weekend hole or a
//! duplicated timestamp.
//!
//! **Every defect below is silent.** A frame with a hole in it does not raise —
//! the readers happily compute an efficiency ratio across a 65-hour weekend as
//! though it were one bar of movement and report a trend. A duplicated bar does
//! not raise either; it just makes a swing look like a double top. None of
//! these produce an error. They produce a number. That is the whole argument
//! for checking at the door rather than hoping to notice later.
//!
//! ## The one that is not like the others
//!
//! A bar stamped in the future is not a data-quality problem, it is **lookahead
//! arriving through the front door**. If a feed hands over a bar dated after
//! the decision instant — clock skew, a broker's server timezone, an off-by-one
//! in a replay driver — then every reader downstream is legitimately reading a
//! bar that has not happened.
//!
//! No amount of care inside the readers can catch that. `AsOf` makes it
//! impossible for a reader to look past the end of what it was given; it cannot
//! make the thing it was given honest. This is the door the type system cannot
//! guard, and it is why [`no_future_bars`] is checked against the decision
//! instant rather than against the data.
//!
//! ## Refused versus noted
//!
//! Refusing a whole session because one candle has a high below its low would
//! be its own kind of failure. The split is whether a reader could produce a
//! **wrong** answer or merely a **weak** one.

use super::bars::{refuse, Answer, Bars};
use super::time::{Utc, MS_PER_MIN};
use super::timeframe::{self, Tf};

/// A gap longer than this many bar-lengths is a hole worth reporting. Two is
/// deliberate: one missing bar is a quiet market, several is a feed problem or
/// a session boundary, and the two need telling apart.
pub const GAP_BARS: f64 = 2.0;

/// FX shuts about 22:00 Friday UTC and reopens about 22:00 Sunday, so a hole
/// this size is the market being shut rather than a fault.
pub const WEEKEND_HOURS: (f64, f64) = (40.0, 80.0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteKind {
    Weekend,
    Gap,
    Flat,
    Spike,
}

/// Something true about the feed that a reader downstream should know.
#[derive(Debug, Clone, PartialEq)]
pub struct Note {
    pub kind: NoteKind,
    pub at: usize,
    pub detail: String,
}

impl Note {
    pub fn say(&self) -> String {
        format!("{:?} at bar {}: {}", self.kind, self.at, self.detail)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub bars: usize,
    pub notes: Vec<Note>,
    pub timeframe: Option<Tf>,
}

impl Report {
    pub fn clean(&self) -> bool {
        self.notes.is_empty()
    }
    pub fn of_kind(&self, k: NoteKind) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.kind == k).collect()
    }
    pub fn say(&self) -> String {
        if self.clean() {
            return format!("{} bars, no defects found", self.bars);
        }
        let mut counts: Vec<(NoteKind, usize)> = Vec::new();
        for n in &self.notes {
            match counts.iter_mut().find(|(k, _)| *k == n.kind) {
                Some((_, c)) => *c += 1,
                None => counts.push((n.kind, 1)),
            }
        }
        let bits: Vec<String> = counts
            .iter()
            .map(|(k, c)| {
                // Spikes are the one kind whose note count is capped, so
                // reporting the notes would report the cap. Every other kind
                // keeps one note per occurrence.
                let n = if *k == NoteKind::Spike { spikes_in(self).max(*c) } else { *c };
                format!("{n} {k:?}")
            })
            .collect();
        format!("{} bars: {}", self.bars, bits.join(", "))
    }
}

/// Every bar's high is the highest thing in it and its low the lowest.
///
/// A bar whose high is below its open is not slightly wrong — it makes the true
/// range negative, which makes the ATR wrong, which makes **every magnitude in
/// every verdict** wrong, quietly, for as long as that bar is in the window.
fn check_ohlc(bars: &Bars) -> Answer<()> {
    let (o, h, l, c) = (bars.all_open(), bars.all_high(), bars.all_low(), bars.all_close());
    for i in 0..c.len() {
        if h[i] < l[i] || h[i] < o[i] || h[i] < c[i] || l[i] > o[i] || l[i] > c[i] {
            return refuse(format!(
                "bar {} has an impossible OHLC: o={:.5} h={:.5} l={:.5} c={:.5}. \
                 This makes the true range negative and every magnitude computed \
                 from it wrong -- quietly, and only while that bar is in the window",
                i, o[i], h[i], l[i], c[i]
            ));
        }
        for (name, v) in [("open", o[i]), ("high", h[i]), ("low", l[i]), ("close", c[i])] {
            if !v.is_finite() {
                return refuse(format!("bar {} has a non-finite {}", i, name));
            }
            if v <= 0.0 {
                return refuse(format!("bar {} has a {} at or below zero", i, name));
            }
        }
    }
    Ok(())
}

/// Timestamps strictly increasing, if there are timestamps at all.
///
/// Unsorted bars are not cosmetic. Every reader takes the LAST n rows and calls
/// them the most recent; if the order is wrong those are simply the wrong bars,
/// and nothing anywhere would say so.
fn check_order(bars: &Bars) -> Answer<()> {
    let t = bars.all_time();
    for i in 1..t.len() {
        if t[i] < t[i - 1] {
            return refuse(format!(
                "bars are not in time order -- bar {} ({}) comes before bar {} ({}). \
                 Every reader takes the last n rows and calls them recent, so \
                 out-of-order bars are silently the wrong bars",
                i,
                Utc::from_ms(t[i]).say(),
                i - 1,
                Utc::from_ms(t[i - 1]).say()
            ));
        }
        if t[i] == t[i - 1] {
            return refuse(format!(
                "duplicate timestamp at bar {}: {} appears twice. A repeated bar \
                 makes a swing look like a double top and doubles its weight in \
                 every average",
                i,
                Utc::from_ms(t[i]).say()
            ));
        }
    }
    Ok(())
}

/// No bar may be stamped after the instant being decided at.
///
/// **The door the type system cannot guard.** `AsOf` stops a reader looking
/// past the end of what it was handed; it cannot make what it was handed
/// honest. A bar dated after the decision instant passes every check downstream
/// because from the readers' side the frame looks completely ordinary.
///
/// Refused rather than trimmed. Silently trimming would hide a broken feed, and
/// a feed handing over future bars is wrong about something else too.
pub fn no_future_bars(bars: &Bars, now_ms: i64) -> Answer<()> {
    let t = bars.all_time();
    if let Some((i, &stamp)) = t.iter().enumerate().find(|(_, &x)| x > now_ms) {
        let ahead = t.iter().filter(|&&x| x > now_ms).count();
        return refuse(format!(
            "{} bar(s) are stamped after the decision instant {}, first at bar \
             {} ({}). That is lookahead arriving through the front door: every \
             reader would pass it, because from their side the frame looks \
             ordinary",
            ahead,
            Utc::from_ms(now_ms).say(),
            i,
            Utc::from_ms(stamp).say()
        ));
    }
    Ok(())
}

/// Holes in the series, with the weekend told apart from a fault.
fn find_gaps(bars: &Bars) -> Vec<Note> {
    let t = bars.all_time();
    if t.len() < 3 {
        return Vec::new();
    }
    let mut mins: Vec<f64> = t.windows(2).map(|w| (w[1] - w[0]) as f64 / MS_PER_MIN as f64).collect();
    let mut sorted = mins.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let step = sorted[sorted.len() / 2];
    if step <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    for (i, gap) in mins.drain(..).enumerate() {
        if gap <= step * GAP_BARS {
            continue;
        }
        let hours = gap / 60.0;
        let weekend = (WEEKEND_HOURS.0..=WEEKEND_HOURS.1).contains(&hours);
        out.push(Note {
            kind: if weekend { NoteKind::Weekend } else { NoteKind::Gap },
            at: i + 1,
            detail: format!(
                "{:.1} h between bars where {:.0} min is normal{}",
                hours,
                step,
                if weekend {
                    ""
                } else {
                    " -- readings spanning this are measuring a hole as though it were movement"
                }
            ),
        });
    }
    out
}

/// Bars with no range at all.
///
/// Usually a feed stall rather than a market that did not move, and it drags
/// the ATR down — which tightens every magnitude and every tolerance derived
/// from it, on readings taken nearby.
fn find_flat(bars: &Bars) -> Vec<Note> {
    let (h, l) = (bars.all_high(), bars.all_low());
    let flat: Vec<usize> = (0..h.len()).filter(|&i| h[i] == l[i]).collect();
    if flat.is_empty() {
        return Vec::new();
    }
    vec![Note {
        kind: NoteKind::Flat,
        at: flat[0],
        detail: format!(
            "{} bar(s) have zero range; these pull the ATR down and tighten \
             every magnitude derived from it",
            flat.len()
        ),
    }]
}

/// Single bars whose range dwarfs their neighbours'.
///
/// Not refused: a real release does this, and refusing it would throw away the
/// most informative bar of the week. Noted, because a spike also drags an ATR
/// up for the next fourteen bars, so every magnitude measured just after one is
/// smaller than it looks.
fn find_spikes(bars: &Bars, times: f64) -> Vec<Note> {
    let (h, l) = (bars.all_high(), bars.all_low());
    if h.len() < 30 {
        return Vec::new();
    }
    let mut rng: Vec<f64> = (0..h.len()).map(|i| h[i] - l[i]).collect();
    let mut sorted = rng.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let typical = sorted[sorted.len() / 2];
    if typical <= 0.0 {
        return Vec::new();
    }
    let found: Vec<(usize, f64)> = rng
        .drain(..)
        .enumerate()
        .filter(|(_, r)| *r > typical * times)
        .collect();
    let total = found.len();

    // Capped at five NOTES, never at five spikes — and every one of them says
    // how many there really are.
    //
    // The cap was here before and the count was not, so `Report::say()` read
    // "5 Spike" on a file with ninety of them. That is the shape this codebase
    // keeps finding: a number that looks like a measurement and is actually a
    // limit. It matters here rather than being merely untidy — each spike
    // drags the ATR up for fourteen bars, and anything sizing risk off the ATR
    // is wrong for that whole stretch. Ninety of them is about 1% of a
    // twenty-year H1 series; five would be nothing.
    found
        .into_iter()
        .take(5)
        .map(|(i, r)| Note {
            kind: NoteKind::Spike,
            at: i,
            detail: format!(
                "range {:.0}x the median; the ATR stays inflated for the next 14 bars \
                 ({total} bars like this in the series)",
                r / typical
            ),
        })
        .collect()
}

/// How many bars in the series spike, however many notes were kept.
///
/// Read off the notes rather than recomputed, so the two can never disagree
/// about the same series.
pub fn spikes_in(report: &Report) -> usize {
    report
        .of_kind(NoteKind::Spike)
        .first()
        .and_then(|n| n.detail.rsplit_once("("))
        .and_then(|(_, tail)| tail.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

/// Check a feed at the door. Refuses anything that makes a reader wrong.
pub fn accept(bars: &Bars, now_ms: Option<i64>) -> Answer<Report> {
    check_ohlc(bars)?;
    check_order(bars)?;
    if let Some(now) = now_ms {
        no_future_bars(bars, now)?;
    }
    let tf = bars.latest().ok().and_then(|v| timeframe::infer(&v));
    let mut notes = find_gaps(bars);
    notes.extend(find_flat(bars));
    notes.extend(find_spikes(bars, 12.0));
    Ok(Report { bars: bars.len(), notes, timeframe: tf })
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fixtures::walk;
    use super::super::time::MS_PER_HOUR;

    fn good(n: usize, every_min: i64) -> Bars {
        let b = walk(n, 9);
        let v = b.latest().unwrap();
        let start = Utc::at(2026, 6, 1, 0, 0).to_ms();
        let time: Vec<i64> = (0..n).map(|i| start + i as i64 * every_min * MS_PER_MIN).collect();
        Bars::new(v.open().to_vec(), v.high().to_vec(), v.low().to_vec(), v.close().to_vec(), time)
            .unwrap()
    }

    fn rebuild(b: &Bars, mutate: impl Fn(&mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<f64>, &mut Vec<i64>)) -> Bars {
        let (mut o, mut h, mut l, mut c, mut t) = (
            b.all_open().to_vec(), b.all_high().to_vec(),
            b.all_low().to_vec(), b.all_close().to_vec(), b.all_time().to_vec(),
        );
        mutate(&mut o, &mut h, &mut l, &mut c, &mut t);
        Bars::new(o, h, l, c, t).unwrap()
    }

    const LATER: i64 = 1_800_000_000_000; // well after every fixture

    #[test]
    fn a_clean_feed_passes_and_reports_its_timeframe() {
        let r = accept(&good(300, 60), Some(LATER)).unwrap();
        assert!(r.clean(), "{}", r.say());
        assert_eq!(r.timeframe, Some(Tf::H1));
        assert_eq!(r.bars, 300);
        assert!(r.say().contains("no defects"));
    }

    #[test]
    fn an_impossible_bar_is_refused() {
        // Fatal because it makes the true range negative, which makes the ATR
        // wrong, which makes every magnitude in every verdict wrong -- and
        // nothing anywhere raises.
        let b = rebuild(&good(300, 60), |_, h, l, _, _| h[50] = l[50] - 0.001);
        let e = accept(&b, Some(LATER));
        assert!(e.is_err());
        let msg = e.unwrap_err().0;
        assert!(msg.contains("impossible OHLC") && msg.contains("50"), "{}", msg);
    }

    #[test]
    fn a_non_finite_price_is_refused() {
        // This used to build the bad series with `rebuild` and hand it to
        // `accept`, proving that `check_ohlc` caught it. That was true and
        // not enough: `accept` is one way in, and a `Bars` built any other
        // way carried NaN straight into `partial_cmp().unwrap()` in
        // `market/levels.rs` and ended the process.
        //
        // The refusal moved to `Bars::new`, the one gate every series passes
        // through -- so it is asserted there now, which is a stronger claim
        // than the original made. `rebuild` itself would panic on this input
        // today, which is the check working.
        let clean = good(300, 60);
        let mut c = clean.all_close().to_vec();
        c[10] = f64::NAN;
        let r = Bars::new(
            clean.all_open().to_vec(),
            clean.all_high().to_vec(),
            clean.all_low().to_vec(),
            c,
            clean.all_time().to_vec(),
        );
        assert!(r.is_err(), "a NaN price was accepted into a series");
        let why = format!("{:?}", r.err().unwrap());
        assert!(why.contains("not a number"), "the refusal does not say what is wrong: {why}");
        assert!(why.contains("close"), "the refusal does not say which column: {why}");
        // And which bar. Both halves matter: the column alone does not tell
        // you where to look in a 300-bar series.
        assert!(why.contains("bar 10"), "the refusal does not say which bar: {why}");

        // And a clean series still passes the feed checks, so the new gate
        // has not made everything unusable.
        assert!(accept(&clean, Some(LATER)).is_ok());
    }

    #[test]
    fn out_of_order_bars_are_refused() {
        let b = rebuild(&good(300, 60), |_, _, _, _, t| t.swap(100, 101));
        let e = accept(&b, Some(LATER));
        assert!(e.unwrap_err().0.contains("not in time order"));
    }

    #[test]
    fn a_duplicate_timestamp_is_refused() {
        let b = rebuild(&good(300, 60), |_, _, _, _, t| t[100] = t[99]);
        let e = accept(&b, Some(LATER));
        assert!(e.unwrap_err().0.contains("duplicate timestamp"));
    }

    #[test]
    fn a_bar_from_the_future_is_refused() {
        // The door the type system cannot guard. `AsOf` stops a reader looking
        // past what it was given; it cannot make what it was given honest.
        let b = good(300, 60);
        let mid = b.all_time()[150];
        let e = accept(&b, Some(mid));
        let msg = e.unwrap_err().0;
        assert!(msg.contains("front door") && msg.contains("lookahead"), "{}", msg);
    }

    #[test]
    fn no_future_bars_names_how_many_and_where_the_first_is() {
        // Called directly, as a walk-forward caller does at each decision
        // instant: the whole series passes at its last bar, and at bar 150
        // it says exactly how many bars are ahead and which comes first.
        let b = good(300, 60);
        assert!(no_future_bars(&b, b.all_time()[299]).is_ok());
        let msg = no_future_bars(&b, b.all_time()[150]).unwrap_err().0;
        assert!(msg.starts_with("149 bar(s)") && msg.contains("first at bar 151"), "{msg}");
    }

    #[test]
    fn a_future_bar_is_refused_not_quietly_trimmed() {
        // Trimming would hide a broken feed, and a feed handing over future
        // bars is wrong about something else too. Checked by behaviour: the
        // same series must still be refused, never shortened and accepted.
        let b = good(300, 60);
        for cut in [10usize, 150, 298] {
            assert!(accept(&b, Some(b.all_time()[cut])).is_err(), "cut {}", cut);
        }
        assert!(accept(&b, Some(b.all_time()[299])).is_ok(), "the last bar is not future");
    }

    #[test]
    fn untimestamped_bars_cannot_be_checked_for_the_future_and_are_not() {
        let b = walk(100, 1);
        let r = accept(&b, Some(0)).unwrap();
        assert_eq!(r.timeframe, None);
    }

    #[test]
    fn a_weekend_is_recognised_as_a_weekend_not_a_fault() {
        let b = rebuild(&good(200, 60), |_, _, _, _, t| {
            for i in 100..200 {
                t[i] += 48 * MS_PER_HOUR;
            }
        });
        let r = accept(&b, Some(LATER)).unwrap();
        assert!(!r.of_kind(NoteKind::Weekend).is_empty(), "{}", r.say());
        assert!(r.of_kind(NoteKind::Gap).is_empty(), "a weekend is not a fault");
    }

    #[test]
    fn a_hole_is_noted_and_says_what_it_does_to_a_reading() {
        let b = rebuild(&good(200, 60), |_, _, _, _, t| {
            for i in 100..200 {
                t[i] += 6 * MS_PER_HOUR;
            }
        });
        let r = accept(&b, Some(LATER)).unwrap();
        let gaps = r.of_kind(NoteKind::Gap);
        assert!(!gaps.is_empty(), "{}", r.say());
        assert!(gaps[0].detail.contains("measuring a hole"));
        // Noted, NOT refused. Refusing a session for one hole would be its own
        // failure; the distinction is wrong versus weak.
        assert_eq!(r.bars, 200);
    }

    #[test]
    fn zero_range_bars_are_noted_with_what_they_do_to_the_atr() {
        let b = rebuild(&good(300, 60), |o, h, l, c, _| {
            for i in 40..43 {
                h[i] = l[i];
                o[i] = l[i];
                c[i] = l[i];
            }
        });
        let r = accept(&b, Some(LATER)).unwrap();
        let flat = r.of_kind(NoteKind::Flat);
        assert!(!flat.is_empty(), "{}", r.say());
        assert!(flat[0].detail.contains("ATR"));
    }

    #[test]
    fn a_news_spike_is_noted_not_refused() {
        // A real release does this. Refusing it would throw away the most
        // informative bar of the week.
        let b = rebuild(&good(300, 60), |_, h, _, _, _| h[120] += 0.05);
        let r = accept(&b, Some(LATER)).unwrap();
        let spikes = r.of_kind(NoteKind::Spike);
        assert!(!spikes.is_empty(), "{}", r.say());
        assert!(spikes[0].detail.contains("inflated"));
    }

    #[test]
    fn the_report_summarises_what_it_found() {
        let b = rebuild(&good(200, 60), |_, _, _, _, t| {
            for i in 100..200 {
                t[i] += 6 * MS_PER_HOUR;
            }
        });
        let s = accept(&b, Some(LATER)).unwrap().say();
        assert!(s.contains("200 bars") && s.contains("Gap"), "{}", s);
    }
}
