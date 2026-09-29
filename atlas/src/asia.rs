//! The overnight range, and why anybody watches it.
//!
//! While Tokyo and Sydney are the only desks on the book, price usually does
//! very little. The high and low it makes in those hours become the two lines
//! London opens into — and they are watched for the same reason yesterday's
//! high is watched: **everybody can see them, and everybody knows everybody
//! can see them.** That is the whole mechanism. It is not a property of the
//! bars.
//!
//! ## Why this is not a breakout strategy
//!
//! It would be very easy to write "buy the break of the Asian high" here, and
//! that is not what this is. This module says where the two lines are and how
//! wide the range they bound is. What to do about it is a decision, and a
//! decision belongs somewhere it can be held to an outcome.
//!
//! ## The part that is measured rather than assumed
//!
//! An Asian range is only interesting when it is **narrow**. A night that
//! already moved a hundred pips has spent the move; the lines are still there
//! and the coiled-spring reading behind them is gone.
//!
//! Every retail version of this hard-codes a pip threshold — twenty pips,
//! thirty pips — which is one number doing two jobs badly: it is far too tight
//! for GBP/JPY and far too loose for EUR/CHF, and it means something different
//! in a quiet month than a violent one. So nothing is hard-coded. The range is
//! reported against the **median of this instrument's own recent complete
//! days**, from the bars in front of it, and the ratio is handed over with the
//! lines. A caller that wants a threshold picks one knowing what it is a
//! fraction of.
//!
//! ## Where the session boundary comes from
//!
//! Not from a clock constant here. `market::session` already knows which
//! centres are awake at an instant, daylight saving and all, so "overnight"
//! means exactly **Tokyo or Sydney open and London shut** — which moves with
//! the clocks in March and October without this file knowing they exist.
//!
//! ## What it refuses
//!
//! A range still forming is not a level. If price is still inside the
//! overnight session, there is no completed high and low yet and this says so
//! rather than handing back a high that can still move — the same rule
//! `fxday::prior_day` follows, for the same reason.

use crate::market::bars::AsOf;
use crate::market::session::{is_open, Centre};

/// Was the market in its overnight hours at this instant?
///
/// Asia awake and London shut. New York is irrelevant to the question by
/// construction: when New York is open, London either is too or has only just
/// closed, and in neither case is this the overnight session.
pub fn overnight(ms: i64) -> bool {
    (is_open(Centre::Tokyo, ms) || is_open(Centre::Sydney, ms)) && !is_open(Centre::London, ms)
}

/// The overnight range of one FX day.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Overnight {
    pub opens: i64,
    pub closes: i64,
    pub high: f64,
    pub low: f64,
    /// Bars inside it. A range built from two bars is a hole in the feed
    /// wearing a range's clothes.
    pub bars: usize,
    /// True while price is still inside these hours, in which case the high
    /// and low can both still move and neither is a level.
    pub still_forming: bool,
}

impl Overnight {
    pub fn range(&self) -> f64 {
        self.high - self.low
    }

    /// Is price above, below, or inside the two lines?
    pub fn where_price_sits(&self, price: f64) -> &'static str {
        if price > self.high {
            "above the overnight high"
        } else if price < self.low {
            "below the overnight low"
        } else {
            "inside the overnight range"
        }
    }

    pub fn say(&self) -> String {
        format!(
            "overnight {:.5} to {:.5} ({:.1} pips over {} bars)",
            self.low,
            self.high,
            self.range() * 10_000.0,
            self.bars
        )
    }
}

/// The most recent **completed** overnight session in the view.
///
/// Walks back from the bound and takes the last contiguous run of overnight
/// bars. The run is contiguous by construction because the session is a block
/// of hours, so a gap inside one is a hole in the feed and shortens the run
/// rather than splicing two nights together.
pub fn last_night(view: &AsOf<'_>) -> Option<Overnight> {
    let time = view.time();
    if time.is_empty() {
        return None;
    }
    let n = view.len().min(time.len());
    if n == 0 {
        return None;
    }
    let (h, l) = (view.high(), view.low());

    // Where the most recent run of overnight bars ends. If the last bar is
    // itself overnight, that run is the one still in progress.
    let mut end = n;
    let still_forming = overnight(time[n - 1]);
    if !still_forming {
        while end > 0 && !overnight(time[end - 1]) {
            end -= 1;
        }
    }
    if end == 0 {
        return None;
    }
    let mut start = end;
    while start > 0 && overnight(time[start - 1]) {
        start -= 1;
    }
    if start == end {
        return None;
    }

    let mut high = f64::NEG_INFINITY;
    let mut low = f64::INFINITY;
    for i in start..end {
        high = high.max(h[i]);
        low = low.min(l[i]);
    }
    (high.is_finite() && low.is_finite()).then_some(Overnight {
        opens: time[start],
        closes: time[end - 1],
        high,
        low,
        bars: end - start,
        still_forming,
    })
}

/// How wide last night was against this instrument's own recent days.
///
/// 1.0 means it covered as much ground as a typical whole day, which is not a
/// coiled spring by any reading. 0.2 is the shape people mean when they talk
/// about the Asian range at all.
///
/// The median is used rather than the mean because one news day doubles a mean
/// and moves a median hardly at all — and a comparison that a single day can
/// move is a comparison that says more about last Thursday than about tonight.
pub fn against_a_normal_day(view: &AsOf<'_>) -> Option<f64> {
    let night = last_night(view)?;
    let mut all = crate::fxday::days(view);
    // The last entry is the day in progress, whose range is still growing.
    // Dropped BEFORE the short-day filter, not after: filtering first can
    // remove the day in progress itself, and then the pop takes a completed
    // day instead — silently, and only on the series where the current day is
    // short, which is most of them.
    all.pop();
    let mut days: Vec<f64> = all
        .iter()
        .filter(|d| d.bars >= 12)
        .map(|d| d.range())
        .collect();
    if days.is_empty() {
        return None;
    }
    days.sort_by(f64::total_cmp);
    let median = days[days.len() / 2];
    (median > 0.0).then(|| night.range() / median)
}

/// Last night's high and low as levels, nearest to price first.
///
/// Empty while the session is still forming: a high that can still move is not
/// a line anybody is defending.
pub fn levels(view: &AsOf<'_>) -> Vec<(f64, &'static str)> {
    let Some(night) = last_night(view) else { return Vec::new() };
    if night.still_forming {
        return Vec::new();
    }
    let mut out = vec![
        (night.high, "the overnight high"),
        (night.low, "the overnight low"),
    ];
    let now = view.now();
    out.sort_by(|a, b| (a.0 - now).abs().total_cmp(&(b.0 - now).abs()));
    out
}

/// Said out loud.
pub fn spoken(view: &AsOf<'_>) -> String {
    let Some(night) = last_night(view) else {
        return "I can't see an overnight session in these bars — either they carry no times, or \
                they don't reach back that far."
            .into();
    };
    if night.still_forming {
        return format!(
            "Asia is still trading — {} so far, and both ends of that can still move. I won't \
             call either one a level until the session is over.",
            night.say()
        );
    }
    let mut said = format!("Last night: {}. Price is {}.", night.say(), night.where_price_sits(view.now()));
    match against_a_normal_day(view) {
        Some(share) if share < 0.35 => said.push_str(&format!(
            " That's {:.0}% of a normal day here, which is the tight overnight range people \
             actually watch.",
            share * 100.0
        )),
        Some(share) if share > 0.8 => said.push_str(&format!(
            " But it covered {:.0}% of a normal day's ground overnight, so the move people wait \
             for has largely happened already — the lines are still there and the reason for \
             watching them isn't.",
            share * 100.0
        )),
        Some(share) => said.push_str(&format!(" That's {:.0}% of a normal day here.", share * 100.0)),
        None => said.push_str(
            " I haven't enough complete days behind it to say whether that's tight or wide, so \
             I'm not going to guess.",
        ),
    }
    if night.bars < 4 {
        said.push_str(&format!(
            " Only {} bars in it, mind — that is a hole in the feed rather than a session.",
            night.bars
        ));
    }
    said
}
