//! Trending, ranging, or choppy — and the honest reasons those are hard.
//!
//! ## Four things the research changed about how this is built
//!
//! **1. Every published threshold in this subject is a convention, and some are
//! decorative.** The Choppiness Index thresholds everyone uses, 61.8 and 38.2,
//! are Fibonacci ratios — TradingView says so in its own documentation.
//! Wilder's ADX 20/25 comes from a 1978 book on daily commodity bars and has no
//! derivation at all. Nothing here takes a number like that on trust: each
//! measure is read against its own random-walk baseline, and several of those
//! baselines are derivable in closed form.
//!
//! **2. The efficiency ratio's baseline is 1/√n, not zero.** For a random walk
//! E[net move]/E[path] = √n/n. So at n=10 the baseline is 0.316 — and the
//! universally quoted "ER above 0.30 means trending" is a threshold **below
//! chance**. At n=30 the baseline is 0.183 and 0.30 is genuinely strong.
//!
//! This matters most for FX. Measured 22-day ER: S&P 0.30, Bitcoin 0.28,
//! **EURUSD 0.18** — against a 1/√22 = 0.213 baseline. EURUSD sits *below* its
//! own random-walk baseline. FX at these horizons is mildly mean-reverting, and
//! a threshold ported from equity writing essentially never fires.
//!
//! **3. ADX is the worst tool here, not the default one.** Its lag is 2(n−1)
//! bars, not (n−1), because Wilder smoothing is applied twice; at n=14 that is
//! 26 bars — six and a half hours on M15. It also ignores closes entirely, so a
//! run of bars with long upper wicks and falling closes generates positive +DM.
//! In the one systematic multi-parameter test found it ranked **last of
//! thirteen** regime filters. It is not implemented, and that is a decision
//! rather than an omission.
//!
//! **4. Hurst and variance-ratio tests do not work at this window length.**
//! Detecting first-order autocorrelation needs |ρ| > 1.96/√T: at 500 bars that
//! is 0.088, and real FX intraday autocorrelation is 0.01–0.05. Detecting
//! ρ=0.05 at 80% power needs about 3,100 bars — 32 days of M15, by which time
//! the regime has changed many times. [`hurst_needs`] returns that number so
//! nobody adds one to the live classifier.
//!
//! ## And the one that decides whether any of it pays
//!
//! "Ranging" versus "choppy" has no accepted quantitative definition — it is a
//! practitioner distinction, and every source that draws it does so in prose.
//! The honest operationalisation is that it is not purely a property of the
//! price series at all. It is **cost-relative**:
//!
//! > A 10-pip box on EURUSD M15, at 0.8 pip spread and 0.3 pip slippage, has
//! > 22% of its width eaten before you are right. That is not a range. The same
//! > statistical structure on H4 with a 60-pip box is a range.
//!
//! So [`Regime::tradeable`] gates on room against cost, and a structure that
//! passes every statistical test but fails that gate is reported CHOPPY. It is
//! the most load-bearing line in the file.

use super::bars::{pip_size, Answer, AsOf};
use super::timeframe::{window, Horizon};

/// Costs are per side, in pips. Typical retail EURUSD; a real deployment passes
/// its own. They are the reason the trichotomy is not purely a price question.
pub const DEFAULT_SPREAD_PIPS: f64 = 0.8;
pub const DEFAULT_SLIPPAGE_PIPS: f64 = 0.3;

/// How many round trips of cost a range must be worth. The SHAPE of the rule is
/// arithmetic; the 8 is a choice, and is registered as one in `params`.
pub const COST_MULTIPLE: f64 = 8.0;

/// Hysteresis, in multiples of the random-walk efficiency ratio.
///
/// A single threshold makes the label flip every time the measure grazes it.
/// ADX's conventional 20/25 pair and the Fractal Dimension Index's 1.4/1.6 pair
/// are both already hysteresis bands that most implementations collapse into
/// one number and then wonder why the state chatters.
pub const ENTER_TREND: f64 = 1.30;
pub const EXIT_TREND: f64 = 1.10;

/// A regime may not be left until held this long. Set to exceed the lag of the
/// measure driving it, or the classifier reacts to its own smoothing.
pub const MIN_DWELL: usize = 5;

/// Kaufman's Efficiency Ratio: net displacement over path length.
///
/// Zero when price ends where it started however far it travelled; one when
/// every bar went the same way. Read against [`er_baseline`] — never against a
/// fixed number.
pub fn efficiency_ratio(view: &AsOf<'_>, n: usize) -> Answer<f64> {
    view.need(n + 1)?;
    let c = view.close();
    let w = &c[c.len() - (n + 1)..];
    let path: f64 = w.windows(2).map(|x| (x[1] - x[0]).abs()).sum();
    if path <= 0.0 {
        return Ok(0.0);
    }
    Ok((w[w.len() - 1] - w[0]).abs() / path)
}

/// What the efficiency ratio reads on a random walk: 1/√n.
///
/// E[|net|] = σ√n·√(2/π) and E[path] = nσ·√(2/π), so the ratio is √n/n. This is
/// why one ER threshold cannot be right across lookbacks.
pub fn er_baseline(n: usize) -> f64 {
    1.0 / (n as f64).sqrt()
}

/// The efficiency ratio as a multiple of its own random-walk baseline.
///
/// 1.0 means indistinguishable from a coin flip. This is what everything here
/// thresholds on, because it is comparable across lookbacks, instruments and
/// timeframes in a way a raw ER is not.
pub fn trend_strength(view: &AsOf<'_>, n: usize) -> Answer<f64> {
    Ok(efficiency_ratio(view, n)? / er_baseline(n))
}

/// (R², slope per bar) of a straight line through the last `n` closes.
///
/// The only measure in this subject with published critical values behind it
/// rather than round numbers, and it gives direction for free — which ADX does
/// not. The caveat is real and is not in the tables: those values assume
/// independent residuals, and financial residuals are autocorrelated, so the
/// true critical value is HIGHER. [`r2_critical`] is a floor, not a test.
pub fn r_squared(view: &AsOf<'_>, n: usize) -> Answer<(f64, f64)> {
    view.need(n)?;
    let c = view.close();
    let y = &c[c.len() - n..];
    let xm = (n as f64 - 1.0) / 2.0;
    let ym: f64 = y.iter().sum::<f64>() / n as f64;
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for (i, &v) in y.iter().enumerate() {
        let dx = i as f64 - xm;
        let dy = v - ym;
        sxx += dx * dx;
        sxy += dx * dy;
        syy += dy * dy;
    }
    if sxx <= 0.0 || syy <= 0.0 {
        return Ok((0.0, 0.0));
    }
    Ok((sxy * sxy / (sxx * syy), sxy / sxx))
}

/// Chande & Kroll's 95% critical R² by sample size, interpolated.
pub fn r2_critical(n: usize) -> f64 {
    const TABLE: [(usize, f64); 9] = [
        (5, 0.77), (10, 0.40), (14, 0.27), (20, 0.20), (25, 0.16),
        (30, 0.13), (50, 0.08), (60, 0.06), (120, 0.03),
    ];
    if n <= TABLE[0].0 {
        return TABLE[0].1;
    }
    if n >= TABLE[TABLE.len() - 1].0 {
        return TABLE[TABLE.len() - 1].1;
    }
    for w in TABLE.windows(2) {
        let ((n0, v0), (n1, v1)) = (w[0], w[1]);
        if n <= n1 {
            let t = (n - n0) as f64 / (n1 - n0) as f64;
            return v0 + (v1 - v0) * t;
        }
    }
    TABLE[TABLE.len() - 1].1
}

/// Are the Bollinger bands inside the Keltner channels — volatility coiled.
///
/// The one rule here that needs no calibration at all, which is why it is the
/// default for the energy axis. It compares a standard deviation to an ATR,
/// both in price units, so it is self-normalising: no pip threshold, nothing
/// that breaks when the instrument changes. Reduces to sd/ATR < 0.75.
pub fn squeeze(view: &AsOf<'_>, n: usize, mult: f64) -> Answer<bool> {
    view.need(n + 1)?;
    let c = view.close();
    let w = &c[c.len() - n..];
    let mean: f64 = w.iter().sum::<f64>() / n as f64;
    let var: f64 = w.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / n as f64;
    Ok(2.0 * var.sqrt() < mult * view.atr(n))
}

/// The Choppiness Index. Never read without [`chop_baseline`].
///
/// Its neutral point is NOT 50 and NOT the Fibonacci pair everyone quotes — it
/// depends on n and on k = E[TR]/E[|Δclose|], the instrument's own intrabar
/// multiplier. On an instrument where k is small, 61.8 essentially never
/// triggers, which is the usual reason people find this indicator "does not
/// work on higher timeframes".
pub fn choppiness(view: &AsOf<'_>, n: usize) -> Answer<f64> {
    view.need(n + 1)?;
    let (h, l, c) = (view.high(), view.low(), view.close());
    let len = c.len();
    let mut sum_tr = 0.0;
    for i in len - n..len {
        let prev = c[i - 1];
        sum_tr += (h[i] - l[i]).max((h[i] - prev).abs()).max((l[i] - prev).abs());
    }
    let hi = h[len - n..].iter().cloned().fold(f64::MIN, f64::max);
    let lo = l[len - n..].iter().cloned().fold(f64::MAX, f64::min);
    let rng = hi - lo;
    if rng <= 0.0 || sum_tr <= 0.0 {
        return Ok(100.0);
    }
    // A gap can put the oldest bar's TR partly outside the window's range, so
    // the ratio can exceed n. Clamped rather than allowed past 100.
    Ok((100.0 * (sum_tr / rng).log10() / (n as f64).log10()).clamp(0.0, 100.0))
}

/// What the Choppiness Index reads on a random walk with THESE bars' own
/// intrabar multiplier. Derived, not quoted.
pub fn chop_baseline(view: &AsOf<'_>, n: usize) -> Answer<f64> {
    view.need(n + 1)?;
    let (h, l, c) = (view.high(), view.low(), view.close());
    let mut tr_sum = 0.0;
    for i in 1..c.len() {
        let prev = c[i - 1];
        tr_sum += (h[i] - l[i]).max((h[i] - prev).abs()).max((l[i] - prev).abs());
    }
    let tr_mean = tr_sum / (c.len() - 1) as f64;
    let dc_mean: f64 =
        c.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (c.len() - 1) as f64;
    if dc_mean <= 0.0 {
        return Ok(50.0);
    }
    let k = tr_mean / dc_mean;
    Ok(100.0 * (k.log10() + 0.5 * (n as f64).log10() - 2.0f64.log10()) / (n as f64).log10())
}

/// How many bars a Hurst or variance-ratio test WOULD need to see `rho`.
///
/// Exists so nobody adds one to the live classifier. The answer for realistic
/// FX intraday autocorrelation is thousands of bars — by which point the regime
/// being classified has changed many times.
pub fn hurst_needs(rho: f64, power: f64) -> usize {
    let z = if power >= 0.95 {
        1.64
    } else if power >= 0.90 {
        1.28
    } else {
        0.84
    };
    (((1.96 + z) / rho.abs().max(1e-9)).powi(2)).ceil() as usize
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Flat,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Energy {
    Coiled,
    Normal,
    Expanding,
}

/// What kind of market this is, on two axes rather than one.
///
/// Forcing a three-way label onto a single indicator is the root cause of most
/// threshold instability in this subject, so direction and energy are kept
/// separate and [`Regime::label`] combines them at the end.
#[derive(Debug, Clone, PartialEq)]
pub struct Regime {
    pub direction: Direction,
    pub energy: Energy,
    /// ER as a multiple of its random-walk baseline.
    pub strength: f64,
    pub r2: f64,
    pub chop: f64,
    pub chop_vs_random: f64,
    pub width_pips: f64,
    pub cost_pips: f64,
    pub bars: usize,
}

impl Regime {
    /// Is there enough room in it to cover the cost of being in it.
    ///
    /// The line that separates a range from chop, and the one no price-series
    /// definition can give you, because it depends on what you pay to trade.
    pub fn tradeable(&self) -> bool {
        // One definition of the threshold, in `timeframe::viable_range_pips`.
        // This used to inline `COST_MULTIPLE * self.cost_pips`, a second copy
        // of the same rule; it now calls the shared function so the two can
        // never drift.
        self.width_pips >= crate::market::timeframe::viable_range_pips(self.cost_pips, COST_MULTIPLE)
    }

    pub fn label(&self) -> String {
        match self.direction {
            Direction::Up => "TRENDING UP".into(),
            Direction::Down => "TRENDING DOWN".into(),
            Direction::Flat => {
                if self.tradeable() {
                    "RANGING".into()
                } else {
                    "CHOPPY".into()
                }
            }
        }
    }

    pub fn say(&self) -> String {
        format!(
            "{} (energy {:?}; trend strength {:.2}x random walk over {} bars; \
             R2 {:.2}; chop {:.0} vs {:.0} random; {:.1} pips of room against \
             {:.1} pips of cost)",
            self.label(),
            self.energy,
            self.strength,
            self.bars,
            self.r2,
            self.chop,
            self.chop_vs_random,
            self.width_pips,
            self.cost_pips
        )
    }
}

/// Classify the market, with hysteresis against the previous reading.
///
/// `previous` and `held_for` make the classification sticky. Without them a
/// measure grazing its threshold relabels the market every bar, and a label
/// that flips bar to bar is not a regime — it is the measure's own noise
/// wearing a name.
///
/// `n = None` reads the lookback off the bars: the horizon is a length of time,
/// not a bar count, so it converts for whatever timeframe the view is on. A
/// series with no timestamps keeps the default rather than having one invented.
pub fn read(
    view: &AsOf<'_>,
    n: Option<usize>,
    spread_pips: f64,
    slippage_pips: f64,
    previous: Option<&Regime>,
    held_for: usize,
) -> Answer<Regime> {
    let n = n.unwrap_or_else(|| window(view, Horizon::Recent, 20));
    view.need(n.max(25) + 1)?;

    let strength = trend_strength(view, n)?;
    let (r2, slope) = r_squared(view, n)?;
    let chop = choppiness(view, 14)?;
    let chop_rw = chop_baseline(view, 14)?;
    let coiled = squeeze(view, 20, 1.5)?;

    let was_trending = previous.map(|p| p.direction != Direction::Flat).unwrap_or(false);
    let gate = if was_trending { EXIT_TREND } else { ENTER_TREND };

    let direction = if let Some(p) = previous.filter(|_| held_for < MIN_DWELL) {
        p.direction // too soon to change its mind
    } else if strength >= gate && r2 >= r2_critical(n) {
        if slope > 0.0 { Direction::Up } else { Direction::Down }
    } else {
        Direction::Flat
    };

    let c = view.close();
    let recent = &c[c.len().saturating_sub(3 * n)..];
    let mean_step: f64 = if recent.len() > 1 {
        recent.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f64>() / (recent.len() - 1) as f64
    } else {
        0.0
    };
    let energy = if coiled {
        Energy::Coiled
    } else if view.atr(14) > mean_step * 3.0 {
        Energy::Expanding
    } else {
        Energy::Normal
    };

    let (h, l) = (view.high(), view.low());
    let hi = h[h.len() - n..].iter().cloned().fold(f64::MIN, f64::max);
    let lo = l[l.len() - n..].iter().cloned().fold(f64::MAX, f64::min);
    let pip = pip_size(view.now());

    Ok(Regime {
        direction,
        energy,
        strength,
        r2,
        chop,
        chop_vs_random: chop_rw,
        width_pips: (hi - lo) / pip,
        cost_pips: 2.0 * (spread_pips + slippage_pips),
        bars: n,
    })
}

/// The common case: default costs, no previous reading.
pub fn read_plain(view: &AsOf<'_>) -> Answer<Regime> {
    read(view, None, DEFAULT_SPREAD_PIPS, DEFAULT_SLIPPAGE_PIPS, None, 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::fixtures::{box_range, ramp, walk, walk_with};

    #[test]
    fn the_er_baseline_is_one_over_root_n() {
        assert!((er_baseline(10) - 0.3162).abs() < 1e-3);
        assert!((er_baseline(30) - 0.1826).abs() < 1e-3);
    }

    #[test]
    fn the_quoted_threshold_is_below_chance_at_ten() {
        // The correction that matters most. "ER > 0.30 means trending" is the
        // most quoted rule in the subject and at n=10 it is BELOW the
        // random-walk baseline.
        assert!(0.30 < er_baseline(10));
        assert!(0.30 > er_baseline(30));
    }

    #[test]
    fn a_random_walk_scores_about_one() {
        let mut vals = Vec::new();
        for seed in 1u64..9 {
            vals.push(trend_strength(&walk(300, seed).latest().unwrap(), 20).unwrap());
        }
        let m: f64 = vals.iter().sum::<f64>() / vals.len() as f64;
        assert!((0.6..1.5).contains(&m), "random walk scored {:.2}x its baseline", m);
    }

    #[test]
    fn a_clean_trend_scores_far_above_one() {
        let b = ramp(120, 0.0008);
        assert!(trend_strength(&b.latest().unwrap(), 20).unwrap() > 3.0);
    }

    #[test]
    fn the_regression_gives_direction_as_well_as_fit() {
        let up = r_squared(&ramp(120, 0.0008).latest().unwrap(), 20).unwrap();
        let closes: Vec<f64> = (0..120).map(|i| 1.2 - i as f64 * 0.0008).collect();
        let dn_bars = super::super::bars::Bars::from_closes(&closes, 0.0006).unwrap();
        let dn = r_squared(&dn_bars.latest().unwrap(), 20).unwrap();
        assert!(up.0 > 0.95 && up.1 > 0.0, "{:?}", up);
        assert!(dn.0 > 0.95 && dn.1 < 0.0, "{:?}", dn);
    }

    #[test]
    fn the_critical_values_get_easier_with_more_bars() {
        assert!(r2_critical(5) > r2_critical(20));
        assert!(r2_critical(20) > r2_critical(120));
        assert!((r2_critical(14) - 0.27).abs() < 1e-9);
    }

    #[test]
    fn the_squeeze_needs_no_calibration_to_fire() {
        let closes: Vec<f64> = (0..60).map(|i| 1.1 + (i % 2) as f64 * 1e-5).collect();
        let b = super::super::bars::Bars::from_closes(&closes, 1e-5).unwrap();
        assert!(squeeze(&b.latest().unwrap(), 20, 1.5).unwrap());
    }

    #[test]
    fn choppiness_is_read_against_its_own_baseline() {
        let b = walk(300, 5);
        let v = b.latest().unwrap();
        let c = choppiness(&v, 14).unwrap();
        let base = chop_baseline(&v, 14).unwrap();
        assert!((0.0..=100.0).contains(&c));
        assert!((0.0..100.0).contains(&base));
        assert!((c - base).abs() < 25.0, "{} vs {}", c, base);
    }

    #[test]
    fn a_trend_reads_less_choppy_than_a_random_walk() {
        let t = choppiness(&ramp(120, 0.0008).latest().unwrap(), 14).unwrap();
        let w = choppiness(&walk(300, 5).latest().unwrap(), 14).unwrap();
        assert!(t < w, "{} vs {}", t, w);
    }

    #[test]
    fn the_module_says_how_many_bars_a_hurst_test_would_need() {
        // The point of this function is to stop anyone putting one in the live
        // classifier.
        assert!(hurst_needs(0.05, 0.80) > 3000);
        assert!(hurst_needs(0.30, 0.80) < 200);
    }

    #[test]
    fn a_clean_uptrend_is_labelled_a_trend() {
        let b = ramp(200, 0.0008);
        let r = read_plain(&b.latest().unwrap()).unwrap();
        assert_eq!(r.direction, Direction::Up, "{}", r.say());
        assert!(r.label().contains("TRENDING UP"));
    }

    #[test]
    fn a_flat_market_with_no_room_is_choppy_not_ranging() {
        let b = walk_with(200, 4, 0.00004);
        let r = read_plain(&b.latest().unwrap()).unwrap();
        assert_eq!(r.direction, Direction::Flat, "{}", r.say());
        assert!(!r.tradeable(), "{}", r.say());
        assert_eq!(r.label(), "CHOPPY", "{}", r.say());
    }

    #[test]
    fn the_same_structure_with_room_in_it_is_a_range() {
        let b = box_range(200, 1.0960, 1.1040, 20);
        let r = read_plain(&b.latest().unwrap()).unwrap();
        assert_eq!(r.direction, Direction::Flat, "{}", r.say());
        assert!(r.tradeable(), "{}", r.say());
        assert_eq!(r.label(), "RANGING");
    }

    #[test]
    fn raising_the_spread_turns_the_same_range_into_chop() {
        // The price series is identical; only the cost of trading it moved.
        let b = box_range(200, 1.0960, 1.1040, 20);
        let v = b.latest().unwrap();
        let cheap = read(&v, None, 0.8, 0.3, None, 0).unwrap();
        let dear = read(&v, None, 8.0, 0.3, None, 0).unwrap();
        assert_eq!(cheap.label(), "RANGING", "{}", cheap.say());
        assert_eq!(dear.label(), "CHOPPY", "{}", dear.say());
        assert_eq!(cheap.width_pips, dear.width_pips);
    }

    #[test]
    fn a_regime_cannot_be_left_before_its_dwell_time() {
        let trend = read_plain(&ramp(200, 0.0008).latest().unwrap()).unwrap();
        let flat_bars = box_range(200, 1.0980, 1.1020, 14);
        let flat = flat_bars.latest().unwrap();
        let held = read(&flat, None, 0.8, 0.3, Some(&trend), 1).unwrap();
        assert_eq!(
            held.direction, trend.direction,
            "a label that changes within the dwell time is noise wearing a name"
        );
        let free = read(&flat, None, 0.8, 0.3, Some(&trend), MIN_DWELL + 1).unwrap();
        assert_eq!(free.direction, Direction::Flat, "{}", free.say());
    }

    #[test]
    fn leaving_a_trend_is_easier_than_entering_one() {
        assert!(EXIT_TREND < ENTER_TREND);
    }

    #[test]
    fn how_often_noise_looks_like_a_trend_is_measured_not_assumed() {
        // The calibration nobody publishes, and the reason FX is hard. A
        // twenty-bar window of pure noise reads as a trend some of the time,
        // and knowing HOW often is the difference between a filter and a
        // superstition.
        let mut fired = 0;
        let trials = 120u64;
        for seed in 0..trials {
            let b = walk(300, seed + 1);
            if read_plain(&b.latest().unwrap()).unwrap().direction != Direction::Flat {
                fired += 1;
            }
        }
        let rate = fired as f64 / trials as f64;
        assert!(rate < 0.35, "noise was called a trend {:.0}% of the time", rate * 100.0);
        assert!(rate > 0.005, "a gate that never fires on noise never fires at all");
    }

    #[test]
    fn a_short_view_is_refused_rather_than_answered() {
        let b = walk(20, 1);
        assert!(read_plain(&b.latest().unwrap()).is_err());
    }
}
