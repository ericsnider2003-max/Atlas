//! Where the trading day ends: **17:00 New York.**
//!
//! Eric's ruling, and the convention every retail FX broker runs on. The
//! second development chat named this as a decision rather than guessing at it,
//! which was right — picking the other answer disagrees with every broker chart
//! while looking perfectly reasonable.
//!
//! ## It is a local time, not a UTC hour
//!
//! ```text
//! winter (EST, UTC-5)   17:00 New York = 22:00 UTC
//! summer (EDT, UTC-4)   17:00 New York = 21:00 UTC
//! ```
//!
//! Hard-coding either one puts every prior-day high and low an hour out for
//! roughly half the year — and does it silently. The levels are still levels,
//! still plausible, still near price. Nothing looks broken; the numbers are
//! just about a different day.
//!
//! ## Why this and not midnight UTC
//!
//! Because it gives **five twenty-four-hour days a week instead of six.** The
//! week opens 17:00 Sunday New York and closes 17:00 Friday, so the Sunday
//! evening session belongs to Monday rather than forming a stub of its own. A
//! midnight-UTC reader splits that opening session into a three-hour Sunday bar
//! and a short Monday, and then disagrees with the chart Eric is looking at on
//! every single day.
//!
//! It is also why brokers run their servers on GMT+2 in winter and GMT+3 in
//! summer: both put 17:00 New York at midnight server time, so the daily candle
//! closes on their own midnight all year.
//!
//! ## The thing I got wrong, and the measurement that said so
//!
//! I assumed a trading day would be twenty-three hours once a year, when the
//! clocks go forward. **It never is.** The change happens at 02:00 on a Sunday,
//! and the market is shut from 17:00 Friday to 17:00 Sunday — so it always
//! lands inside the weekend. Swept across 2024–2027 in
//! `docs/fxday_reference.py`: every transition falls in the Saturday-to-Sunday
//! span, and every trading day is exactly twenty-four hours.
//!
//! What *does* move is the weekend: **47 hours in March and 49 in November**,
//! against 48 the rest of the year. That matters for anything deciding whether
//! a gap in the bars is a weekend or a hole.
//!
//! ## Which days exist is decided by the bars, not the calendar
//!
//! The calendar says where the boundaries are. It does not know about
//! Christmas, or a broker's maintenance window, or a pair that simply did not
//! print. So "the prior day" here means the previous span that **has bars in
//! it**, which skips weekends and holidays without needing to know what either
//! one is.

use crate::market::bars::AsOf;
use crate::market::session::offset_hours;
use crate::market::session::Zone;
use crate::market::time::{civil_from_days, days_from_civil, MS_PER_DAY, MS_PER_HOUR};

/// The hour, on a New York wall clock, at which one FX day becomes the next.
pub const CLOSE_HOUR: i64 = 17;

/// The UTC instant of 17:00 New York on a given New York calendar date.
///
/// The offset is read at roughly the instant in question rather than assumed
/// from the date, because a date does not know its own offset. Seventeen
/// hundred is never near a transition — they happen at 02:00 — so one pass
/// settles it.
pub fn close_on(y: i32, m: u32, d: u32) -> i64 {
    let midnight = days_from_civil(y, m, d) * MS_PER_DAY;
    let offset = offset_hours(Zone::NewYork, midnight + CLOSE_HOUR * MS_PER_HOUR);
    midnight + (CLOSE_HOUR - offset) * MS_PER_HOUR
}

/// The most recent 17:00 New York at or before `ms`.
pub fn day_start(ms: i64) -> i64 {
    // At most two days back covers the offset either way. A loop with no bound
    // would be a loop that hangs on a bad timestamp rather than returning a
    // wrong answer, which is worse.
    for back in 0..3 {
        let (y, m, d) = civil_from_days((ms - back * MS_PER_DAY).div_euclid(MS_PER_DAY));
        let c = close_on(y, m, d);
        if c <= ms {
            return c;
        }
    }
    // Unreachable for any sane instant; falling back to the nearest boundary
    // rather than panicking, because a daemon reading a feed should not die on
    // one strange timestamp.
    let (y, m, d) = civil_from_days(ms.div_euclid(MS_PER_DAY));
    close_on(y, m, d) - MS_PER_DAY
}

/// The FX day containing `ms`, as (opens, closes).
pub fn day_of(ms: i64) -> (i64, i64) {
    let start = day_start(ms);
    let (y, m, d) = civil_from_days(start.div_euclid(MS_PER_DAY));
    let (ny, nm, nd) = civil_from_days(days_from_civil(y, m, d) + 1);
    (start, close_on(ny, nm, nd))
}

/// Day of the week of an instant. 0 is Sunday.
fn weekday_of(ms: i64) -> i64 {
    (ms.div_euclid(MS_PER_DAY) + 4).rem_euclid(7)
}

/// The most recent Sunday 17:00 New York at or before `ms` — the FX week open.
pub fn week_start(ms: i64) -> i64 {
    let mut s = day_start(ms);
    // Six steps at most, and bounded for the same reason as above.
    for _ in 0..8 {
        if weekday_of(s) == 0 {
            return s;
        }
        s = day_start(s - 1);
    }
    s
}

/// The FX week containing `ms`, as (opens, closes). Closes 17:00 Friday.
/// Five days exactly, and that is safe for a reason worth writing down: the
/// clock change happens at 02:00 on a Sunday, which is *before* the week opens
/// at 17:00 that evening. So the offset never moves inside a trading week, and
/// adding five twenty-four-hour days lands on Friday's close to the minute.
pub fn week_of(ms: i64) -> (i64, i64) {
    let start = week_start(ms);
    (start, start + 5 * MS_PER_DAY)
}

// ---------------------------------------------------------------------------
// What a day did
// ---------------------------------------------------------------------------

/// One completed day's shape, and the levels a trader reads off it.
///
/// `DayShape` rather than `Shape`: four modules already define a `Shape`, and
/// a fifth is how somebody ends up reading one and reasoning about another.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DayShape {
    pub opens: i64,
    pub closes: i64,
    pub high: f64,
    pub low: f64,
    pub open: f64,
    pub close: f64,
    /// How many bars fell inside it. Kept because a "day" made of three bars
    /// is a public holiday or a feed with a hole in it, and its high is not
    /// the level anybody is watching.
    pub bars: usize,
}

impl DayShape {
    pub fn range(&self) -> f64 {
        self.high - self.low
    }

    /// Where price sits inside this day's range, nought at the low.
    pub fn position_of(&self, price: f64) -> Option<f64> {
        let r = self.range();
        (r > 0.0).then(|| ((price - self.low) / r).clamp(0.0, 1.0))
    }

    pub fn say(&self) -> String {
        format!(
            "high {:.5}, low {:.5}, closed {:.5} ({} bars)",
            self.high, self.low, self.close, self.bars
        )
    }
}

/// Group a view's bars into FX days, oldest first.
///
/// Only days with bars in them appear, which is what makes weekends and
/// holidays disappear without this file having to know what either one is.
///
/// The **last** entry is the day in progress — it has not closed yet, and
/// `prior_day` deliberately does not return it. A high that can still move is
/// not a level.
pub fn days(view: &AsOf<'_>) -> Vec<DayShape> {
    let time = view.time();
    if time.is_empty() {
        return Vec::new();
    }
    let (o, h, l, c) = (view.open(), view.high(), view.low(), view.close());
    let mut out: Vec<DayShape> = Vec::new();
    for i in 0..view.len().min(time.len()) {
        let (opens, closes) = day_of(time[i]);
        match out.last_mut() {
            Some(day) if day.opens == opens => {
                day.high = day.high.max(h[i]);
                day.low = day.low.min(l[i]);
                day.close = c[i];
                day.bars += 1;
            }
            _ => out.push(DayShape {
                opens,
                closes,
                high: h[i],
                low: l[i],
                open: o[i],
                close: c[i],
                bars: 1,
            }),
        }
    }
    out
}

/// The last **completed** day.
///
/// `None` when the view holds only the day in progress, which is the honest
/// answer rather than handing back a high that can still move.
pub fn prior_day(view: &AsOf<'_>) -> Option<DayShape> {
    let all = days(view);
    (all.len() >= 2).then(|| all[all.len() - 2])
}

/// The last **completed** week, the same way.
pub fn prior_week(view: &AsOf<'_>) -> Option<DayShape> {
    let time = view.time();
    if time.is_empty() {
        return None;
    }
    let (o, h, l, c) = (view.open(), view.high(), view.low(), view.close());
    let mut weeks: Vec<DayShape> = Vec::new();
    for i in 0..view.len().min(time.len()) {
        let (opens, closes) = week_of(time[i]);
        match weeks.last_mut() {
            Some(w) if w.opens == opens => {
                w.high = w.high.max(h[i]);
                w.low = w.low.min(l[i]);
                w.close = c[i];
                w.bars += 1;
            }
            _ => weeks.push(DayShape {
                opens,
                closes,
                high: h[i],
                low: l[i],
                open: o[i],
                close: c[i],
                bars: 1,
            }),
        }
    }
    (weeks.len() >= 2).then(|| weeks[weeks.len() - 2])
}

/// Prior day and prior week as plain levels, nearest to price first.
///
/// The prior day's high and low are the two most-watched lines on an FX chart
/// after the figure, which is the whole argument for having them: price stops
/// there because everyone is looking at them, not because of anything the bars
/// know.
pub fn levels(view: &AsOf<'_>) -> Vec<(f64, &'static str)> {
    let mut out = Vec::new();
    if let Some(d) = prior_day(view) {
        out.push((d.high, "yesterday's high"));
        out.push((d.low, "yesterday's low"));
        out.push((d.close, "yesterday's close"));
    }
    if let Some(w) = prior_week(view) {
        out.push((w.high, "last week's high"));
        out.push((w.low, "last week's low"));
    }
    let now = view.now();
    out.sort_by(|a, b| (a.0 - now).abs().total_cmp(&(b.0 - now).abs()));
    out.dedup_by(|a, b| (a.0 - b.0).abs() < f64::EPSILON);
    out
}

/// Said out loud.
pub fn spoken(view: &AsOf<'_>) -> String {
    match (prior_day(view), prior_week(view)) {
        (None, _) => "I haven't got a completed day to read levels off yet.".into(),
        (Some(d), w) => {
            let mut said = format!("Yesterday: {}.", d.say());
            if d.bars < 12 {
                said.push_str(&format!(
                    " Only {} bars in it, so that was a holiday or a hole in the feed rather \
                     than a day — its high isn't a level anybody watched.",
                    d.bars
                ));
            }
            if let Some(w) = w {
                said.push_str(&format!(" Last week: {}.", w.say()));
            }
            said
        }
    }
}
