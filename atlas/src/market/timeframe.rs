//! What a bar is worth.
//!
//! ## The problem, stated plainly
//!
//! Every lookback in this module started life as a BAR COUNT — twenty for the
//! efficiency ratio, two for the swing detector, twenty for a break. Each was
//! chosen once, against one mental picture of a chart, and then applied to
//! every timeframe as if a bar were a unit of anything.
//!
//! It is not. Twenty bars is:
//!
//! ```text
//! M1     20 minutes      -- inside a single news window
//! M5     1 hour 40 min   -- part of one session
//! M15    5 hours         -- most of a session
//! H1     20 hours        -- nearly a full day, across three sessions
//! H4     3 days 8 hours  -- a week of trading
//! ```
//!
//! Five different questions. And it failed SILENTLY: the number came back, it
//! looked plausible, and nobody was told the question had changed.
//!
//! So lookbacks are wall-clock horizons and the bar count is derived. Ask for
//! "the last day of price action" and get 24 bars on H1 and 96 on M15 — the
//! same question, asked correctly on each.
//!
//! ## Three things that follow
//!
//! **A bar is a span, not a moment.** The H4 bar closing at 16:00 covers
//! 12:00–16:00, so it CONTAINS a 12:30 payrolls print, its window and the whole
//! recovery — while its close sits three and a half hours clear and a
//! close-only check calls it clean. It is the least clean bar of the week.
//!
//! **A sample size is a length of time.** Thirty observations is half an hour
//! on M1 and a full trading week on H4. That turns "we need more data" into a
//! date.
//!
//! **A window can be too short to ask.** On H4 a day is six bars, and six bars
//! cannot support the measures: the efficiency ratio's random-walk baseline at
//! n=6 is 0.41, so the gap between a trend and noise has closed, and the 95%
//! critical R² at n=5 is 0.77, which almost nothing clears. Converting the
//! horizon correctly and then handing over a sample too small to answer it is a
//! different way of being wrong, not a fix — hence [`MIN_BARS`], and
//! [`stretched`] to say when it bit.

use super::bars::{refuse, Answer, AsOf};
use super::time::MS_PER_MIN;

/// The standard chart timeframes, named the way every platform names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tf {
    M1,
    M5,
    M15,
    M30,
    H1,
    H4,
    D,
}

pub const ALL: [Tf; 7] = [Tf::M1, Tf::M5, Tf::M15, Tf::M30, Tf::H1, Tf::H4, Tf::D];

/// FX trades about 120 hours a week — Sunday evening to Friday evening, five
/// days and not seven. Using 168 would promise data a third sooner than it can
/// arrive.
pub const TRADING_HOURS_PER_WEEK: f64 = 120.0;

/// Below this many bars the measures stop meaning anything, whatever the clock
/// says. See the module note.
pub const MIN_BARS: usize = 20;

/// A named horizon, so that "20" cannot mean five different things in five
/// files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Horizon {
    /// Roughly a day of price action — the span a trader means by "recently".
    Recent,
    /// About a trading week. Enough for the level scan to find anything.
    Structure,
    /// The least any reader needs before it will speak at all.
    Minimum,
}

impl Horizon {
    pub fn minutes(self) -> f64 {
        match self {
            Horizon::Recent => 24.0 * 60.0,
            Horizon::Structure => 5.0 * 24.0 * 60.0,
            Horizon::Minimum => 10.0 * 60.0,
        }
    }
}

impl Tf {
    pub fn minutes(self) -> i64 {
        match self {
            Tf::M1 => 1,
            Tf::M5 => 5,
            Tf::M15 => 15,
            Tf::M30 => 30,
            Tf::H1 => 60,
            Tf::H4 => 240,
            Tf::D => 1440,
        }
    }

    pub fn hours(self) -> f64 {
        self.minutes() as f64 / 60.0
    }

    pub fn name(self) -> &'static str {
        match self {
            Tf::M1 => "M1",
            Tf::M5 => "M5",
            Tf::M15 => "M15",
            Tf::M30 => "M30",
            Tf::H1 => "H1",
            Tf::H4 => "H4",
            Tf::D => "D",
        }
    }

    /// By name, refusing anything that is not a standard timeframe.
    ///
    /// Inventing one would give every bar-count conversion a made-up scale, and
    /// a made-up scale is worse than a missing one because it answers.
    pub fn parse(name: &str) -> Answer<Tf> {
        ALL.iter()
            .find(|t| t.name() == name)
            .copied()
            .map(Ok)
            .unwrap_or_else(|| {
                refuse(format!(
                    "{:?} is not a timeframe Atlas knows; it knows {:?}. \
                     Inventing one would give every conversion a made-up scale",
                    name,
                    ALL.map(|t| t.name())
                ))
            })
    }

    /// How many bars cover a wall-clock horizon, rounded UP.
    ///
    /// Up rather than to nearest, deliberately: asking for a day of history and
    /// being handed 23 hours of it is the sort of quiet shortfall that makes
    /// two timeframes disagree for reasons nobody can find.
    pub fn bars_for_minutes(self, minutes: f64) -> usize {
        if minutes <= 0.0 {
            return 1;
        }
        let n = (minutes / self.minutes() as f64).ceil() as usize;
        n.max(1)
    }

    /// The bar count for a named horizon, floored at [`MIN_BARS`].
    pub fn bars_for(self, h: Horizon) -> usize {
        self.bars_for_minutes(h.minutes()).max(MIN_BARS)
    }

    /// The raw conversion, before the floor. Exposed so both the intent and the
    /// override are visible rather than one hiding the other.
    pub fn raw_bars_for(self, h: Horizon) -> usize {
        self.bars_for_minutes(h.minutes())
    }

    /// Milliseconds spanned by `bars` of this timeframe.
    pub fn span_ms(self, bars: usize) -> i64 {
        bars as i64 * self.minutes() * MS_PER_MIN
    }

    /// `(opened, closes)` for the bar that **opened** at `open_ms`.
    ///
    /// It took a CLOSE until 17 Sep 2026 — `(close - span, close)` — and its
    /// only caller, `standdown::spanning`, handed it a stamp. The stamps in
    /// this project are opens (see `AsOf::opened_at`, which now says so and
    /// gives the reasons), so the window returned was the *previous* bar's and
    /// the news blackout was a bar early in both directions.
    ///
    /// Taking the open is also the honest shape: a bar's stamp is the only
    /// time either side of it that a caller actually has.
    pub fn bar_window(self, open_ms: i64) -> (i64, i64) {
        (open_ms, open_ms + self.span_ms(1))
    }

    /// Does this bar overlap a window at all — not merely close inside it.
    ///
    /// The only correct question above M5, and the one a close-only check gets
    /// wrong on precisely the bar that matters most.
    pub fn spans(self, open_ms: i64, from_ms: i64, to_ms: i64) -> bool {
        let (opened, closes) = self.bar_window(open_ms);
        opened < to_ms && from_ms < closes
    }

    /// How long after a swing happens before it can be known.
    ///
    /// Returned in milliseconds and rendered in hours by [`say_span`], because
    /// "two bars" sounds like a detail and "eight hours" sounds like what it is.
    pub fn confirmation_delay_ms(self, k: usize) -> i64 {
        self.span_ms(k)
    }

    /// How long `bars` takes to ARRIVE, at 120 trading hours a week.
    ///
    /// Not the same as [`span_ms`]. Thirty H4 bars is 120 hours of market time
    /// — a full trading week of waiting — and R9 wants thirty observations
    /// before anything speaks.
    pub fn calendar_ms(self, bars: usize) -> i64 {
        let market_hours = bars as f64 * self.hours();
        let weeks = market_hours / TRADING_HOURS_PER_WEEK;
        (weeks * 7.0 * 24.0 * 3_600_000.0) as i64
    }

}

/// The smallest range worth trading, in pips, from the round-trip cost.
///
/// The same figure on every timeframe, on purpose: cost is paid per trade and
/// does not shrink with the bar, while the range available does — the
/// arithmetic reason the fast timeframes are hard, and not psychological. So
/// it is a free function, not a `Tf` method: it never depended on the
/// timeframe. `cost_pips` is the round-trip cost, `2*(spread+slippage)`, which
/// is exactly what `regime::Regime` stores. `regime::tradeable` used to inline
/// `COST_MULTIPLE * cost_pips` — an identical second copy of this threshold —
/// and now calls this, so the rule lives in one place.
pub fn viable_range_pips(cost_pips: f64, multiple: f64) -> f64 {
    multiple * cost_pips
}

pub fn say_span(ms: i64) -> String {
    let h = ms as f64 / 3_600_000.0;
    if h < 1.0 {
        format!("{:.0} min", ms as f64 / 60_000.0)
    } else if h < 48.0 {
        format!("{:.1} h", h)
    } else {
        format!("{:.1} days", h / 24.0)
    }
}

/// How much longer the window is than the horizon asked for, if the floor bit.
///
/// `None` when the horizon was honoured. A caller that believes it is looking
/// at a day of price action when it is looking at three is wrong about the one
/// thing it was trying to control.
pub fn stretched(tf: Tf, h: Horizon) -> Option<i64> {
    let asked = tf.raw_bars_for(h);
    let got = asked.max(MIN_BARS);
    if got == asked {
        None
    } else {
        Some(tf.span_ms(got) - tf.span_ms(asked))
    }
}

/// The timeframe of a view that carries timestamps, or `None`.
///
/// Uses the MEDIAN gap, not the mean. A weekend is a 65-hour hole in an H1
/// series and a mean would land between two timeframes and name the wrong one —
/// which is worse than naming none, because every conversion downstream would
/// then be confidently wrong.
pub fn infer(view: &AsOf<'_>) -> Option<Tf> {
    let t = view.time();
    if t.len() < 3 {
        return None;
    }
    let mut gaps: Vec<f64> = t
        .windows(2)
        .take(200)
        .map(|w| (w[1] - w[0]) as f64 / MS_PER_MIN as f64)
        .collect();
    gaps.sort_by(|a, b| a.total_cmp(&b));
    let median = gaps[gaps.len() / 2];
    ALL.iter()
        .find(|tf| {
            let m = tf.minutes() as f64;
            (median - m).abs() < (0.5f64).max(m * 0.05)
        })
        .copied()
}

/// The bar count for a horizon, worked out from the bars themselves.
///
/// The seam that makes the readers timeframe-aware without any of them having
/// to be told which timeframe they are on. Untimestamped bars keep the
/// caller's default — a series with no timestamps has no timeframe, and
/// inventing one makes every conversion downstream confidently wrong.
pub fn window(view: &AsOf<'_>, h: Horizon, default: usize) -> usize {
    match infer(view) {
        Some(tf) => tf.bars_for(h),
        None => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::bars::Bars;
    use super::super::time::{Utc, MS_PER_HOUR};

    fn stamped(n: usize, every_min: i64) -> Bars {
        let closes: Vec<f64> = (0..n).map(|i| 1.1 + (i % 7) as f64 * 1e-4).collect();
        let start = Utc::at(2026, 6, 1, 0, 0).to_ms();
        let time: Vec<i64> = (0..n).map(|i| start + i as i64 * every_min * MS_PER_MIN).collect();
        let b = Bars::from_closes(&closes, 0.0006).unwrap();
        let v = b.latest().unwrap();
        Bars::new(
            v.open().to_vec(),
            v.high().to_vec(),
            v.low().to_vec(),
            v.close().to_vec(),
            time,
        )
        .unwrap()
    }

    #[test]
    fn twenty_bars_means_five_different_things() {
        // The whole reason this module exists, asserted rather than described.
        let m1 = Tf::M1.span_ms(20) as f64 / MS_PER_HOUR as f64;
        let h4 = Tf::H4.span_ms(20) as f64 / MS_PER_HOUR as f64;
        assert!(m1 < 0.5);
        assert!(h4 > 70.0);
        assert_eq!((h4 / m1).round() as i64, 240);
    }

    #[test]
    fn a_named_horizon_is_the_same_length_of_time_everywhere_it_fits() {
        for tf in [Tf::M1, Tf::M5, Tf::M15, Tf::M30, Tf::H1] {
            let n = tf.bars_for(Horizon::Recent);
            let hours = tf.span_ms(n) as f64 / MS_PER_HOUR as f64;
            assert!((23.9..=24.1 + tf.hours()).contains(&hours), "{} {}", tf.name(), hours);
            assert!(stretched(tf, Horizon::Recent).is_none(), "{}", tf.name());
        }
    }

    #[test]
    fn the_bar_count_scales_the_right_way() {
        assert_eq!(Tf::H1.bars_for(Horizon::Recent), 24);
        assert_eq!(Tf::M15.bars_for(Horizon::Recent), 96);
        // H4 would be 6 by the clock; the floor holds it at 20. Both the raw
        // conversion and the override are pinned, so neither hides the other.
        assert_eq!(Tf::H4.raw_bars_for(Horizon::Recent), 6);
        assert_eq!(Tf::H4.bars_for(Horizon::Recent), MIN_BARS);
    }

    #[test]
    fn no_horizon_produces_a_window_too_small_to_ask() {
        for tf in ALL {
            assert!(tf.bars_for(Horizon::Recent) >= MIN_BARS, "{}", tf.name());
        }
    }

    #[test]
    fn a_stretched_window_says_by_how_much() {
        let by = stretched(Tf::H4, Horizon::Recent).expect("H4 must stretch");
        assert!(by > 2 * 24 * MS_PER_HOUR);
        assert!(stretched(Tf::H1, Horizon::Recent).is_none());
    }

    #[test]
    fn a_horizon_is_rounded_up_never_down() {
        assert_eq!(Tf::H4.bars_for_minutes(600.0), 3);
        assert_eq!(Tf::H4.span_ms(3) / MS_PER_HOUR, 12);
    }

    #[test]
    fn an_h4_bar_swallows_a_news_release_whole() {
        // Payrolls lands 12:30. The H4 bar that OPENS at 12:00 covers
        // 12:00-16:00 -- the release, the window and the recovery. Its own
        // stamp is half an hour clear of the release, so a stamp-only check
        // calls it clean. It is not.
        //
        // Stated in opens since 17 Sep, because that is what the stamps are.
        // It read `close = 16:00` before, which described the same bar and
        // arrived at the right answer only because `bar_window` subtracted a
        // span; on real open-stamped data the pair was a bar out.
        //
        // A release at 14:00 rather than 12:30, deliberately. The whole point
        // is a bar whose OWN STAMP is clear of the window while the bar
        // contains it, and payrolls at 12:30 gives a window starting at 12:00
        // — exactly the stamp of the bar that holds it, so a stamp-only check
        // would catch that one by luck and prove nothing. ISM Services at
        // 14:00 sits in the middle of the same bar, which is the honest case
        // and the commoner one.
        let release = Utc::at(2026, 10, 2, 14, 0).to_ms();
        let (from, to) = (release - 30 * MS_PER_MIN, release + 120 * MS_PER_MIN);
        let opens = Utc::at(2026, 10, 2, 12, 0).to_ms();
        assert!(Tf::H4.spans(opens, from, to), "the H4 bar containing the release was missed");
        assert!(!(from..=to).contains(&opens), "and a stamp-only check would call it clean");
        // The M5 bar opening at the same instant is over by 12:05, two and a
        // half hours before the window opens.
        assert!(!Tf::M5.spans(opens, from, to), "on M5 that same stamp really is clear");
        // And the H4 bar that opens at 16:00 starts after the window closes.
        assert!(
            !Tf::H4.spans(Utc::at(2026, 10, 2, 16, 0).to_ms(), from, to),
            "the bar that opens at 16:00 is genuinely past it"
        );
    }

    #[test]
    fn the_confirmation_delay_is_a_real_duration() {
        assert_eq!(Tf::H4.confirmation_delay_ms(2), 8 * MS_PER_HOUR);
        assert_eq!(Tf::M5.confirmation_delay_ms(2), 10 * MS_PER_MIN);
        assert_eq!(say_span(Tf::H4.confirmation_delay_ms(2)), "8.0 h");
    }

    #[test]
    fn thirty_observations_is_a_date_not_a_number() {
        assert!(Tf::H4.calendar_ms(30) > 6 * 24 * MS_PER_HOUR);
        assert!(Tf::M1.calendar_ms(30) < MS_PER_HOUR);
    }

    #[test]
    fn the_wait_for_data_allows_for_the_market_being_shut() {
        // 120 trading hours a week, not 168. Using 168 would promise data a
        // third sooner than it can arrive.
        let week = Tf::H1.calendar_ms(120) as f64 / (24.0 * MS_PER_HOUR as f64);
        assert!((6.5..7.5).contains(&week), "{}", week);
    }

    #[test]
    fn the_cost_floor_does_not_shrink_with_the_bar() {
        // Now a free function of the round-trip cost, not a Tf method — so its
        // independence from the timeframe is structural rather than asserted.
        // Round-trip cost 2*(0.8+0.3) = 2.2 pips, times the 8x multiple = 17.6.
        let floor = super::viable_range_pips(2.2, 8.0);
        assert!((floor - 17.6).abs() < 1e-9, "{floor}");
        assert!(floor > 15.0);
    }

    #[test]
    fn a_timeframe_that_is_not_standard_is_refused() {
        let e = Tf::parse("H3");
        assert!(e.is_err());
        assert!(e.unwrap_err().0.contains("made-up scale"));
        assert_eq!(Tf::parse("H4").unwrap(), Tf::H4);
    }

    #[test]
    fn the_timeframe_is_read_off_timestamped_bars() {
        let b = stamped(100, 15);
        assert_eq!(infer(&b.latest().unwrap()), Some(Tf::M15));
        assert_eq!(infer(&stamped(100, 240).latest().unwrap()), Some(Tf::H4));
    }

    #[test]
    fn a_weekend_gap_does_not_rename_the_timeframe() {
        // The median, not the mean. A weekend is a 65-hour hole in an H1
        // series; a mean would land between two timeframes and name the wrong
        // one confidently.
        let n = 120;
        let closes: Vec<f64> = (0..n).map(|i| 1.1 + (i % 5) as f64 * 1e-4).collect();
        let start = Utc::at(2026, 6, 1, 0, 0).to_ms();
        let time: Vec<i64> = (0..n)
            .map(|i| {
                let base = start + i as i64 * MS_PER_HOUR;
                if i >= 60 { base + 65 * MS_PER_HOUR } else { base }
            })
            .collect();
        let b0 = Bars::from_closes(&closes, 0.0006).unwrap();
        let v = b0.latest().unwrap();
        let b = Bars::new(
            v.open().to_vec(),
            v.high().to_vec(),
            v.low().to_vec(),
            v.close().to_vec(),
            time,
        )
        .unwrap();
        assert_eq!(infer(&b.latest().unwrap()), Some(Tf::H1));
    }

    #[test]
    fn bars_with_no_timestamps_report_no_timeframe() {
        let b = super::super::fixtures::walk(100, 3);
        assert_eq!(infer(&b.latest().unwrap()), None);
        assert_eq!(window(&b.latest().unwrap(), Horizon::Recent, 20), 20);
    }

    #[test]
    fn a_readers_lookback_is_read_off_the_bars() {
        let b = stamped(300, 15);
        assert_eq!(window(&b.latest().unwrap(), Horizon::Recent, 20), 96);
    }
}
