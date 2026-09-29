//! The trade that isn't working. It just hasn't lost yet.
//!
//! A stop answers "was I wrong". A target answers "was I right". Neither
//! answers the third thing that happens to most trades, which is **nothing**:
//! the setup fired, price went nowhere, and the position sits there for two
//! days paying spread and swap while the reason it was opened quietly expires.
//!
//! That trade is not a winner waiting to happen. The reason for it was a
//! reading of the market *at a moment*, and a reading has a shelf life — a
//! break of structure that was going to run has, by and large, run. What is
//! left after it doesn't is an open position with no thesis, held because
//! closing it would make the loss real.
//!
//! ## Why this is not just "close after N bars"
//!
//! Because N is different on every timeframe and in every market, and a fixed
//! N is the same mistake as a fixed pip stop. What this does instead is
//! measure two things off the bars and compare them:
//!
//! - **How long the market's own movement says it should take.** The target is
//!   some distance away and this market covers some distance per bar. Divide.
//!   That is a pace, and it comes from the series rather than from a guess.
//! - **How far it has actually got.** The best the trade has been, as a share
//!   of the distance to target. Best, not current — a trade that reached 80% of
//!   its target and came back is a different animal from one that never moved,
//!   and only one of them is stale.
//!
//! ## What is chosen rather than measured, said plainly
//!
//! Two numbers in [`StaleConfig`] are judgements: how many times the implied
//! pace to allow, and how little progress counts as none. They are named as
//! choices, and the honest way to settle them is the same as everything else
//! here — score closed trades and read the answer off the record. Until that
//! has thirty of them, these are starting points and this module says so.
//!
//! ## What it will not do
//!
//! It will not move the stop. A time exit is a decision to leave at market,
//! and dressing it up as a stop move would confuse two different things: one
//! is "the trade is wrong", the other is "the trade is nothing".
//!
//! It also will not fire on a trade that has never had the chance. Under the
//! implied pace, `Going::TooEarly` — because a rule that can fire on the
//! second bar is a rule that will.

use crate::levels::Side;
use crate::market::bars::AsOf;

/// A position, as much of it as this needs to know.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Open {
    pub side: Side,
    pub entry: f64,
    pub stop: f64,
    pub target: f64,
    /// When it was opened, as a bar timestamp in milliseconds.
    pub opened_at: i64,
}

impl Open {
    fn to_target(&self) -> f64 {
        (self.target - self.entry).abs()
    }

    /// How favourable a price is, as a share of the way to target.
    /// Negative when price is the wrong side of entry.
    fn share_of_the_way(&self, price: f64) -> f64 {
        let distance = self.to_target();
        if distance <= 0.0 {
            return 0.0;
        }
        match self.side {
            Side::Buy => (price - self.entry) / distance,
            Side::Sell => (self.entry - price) / distance,
        }
    }
}

/// The two judgements, kept together and labelled as judgements.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StaleConfig {
    /// How many times the implied pace to wait before asking the question.
    ///
    /// **Chosen, not measured.** Three is generous — it allows a trade three
    /// times as long as the market's own movement says it needs — on the
    /// principle that an exit rule firing early is worse than one firing late,
    /// because the early one cuts winners and never shows up as a loss.
    pub patience: f64,
    /// Below this share of the way to target, the trade has gone nowhere.
    ///
    /// **Chosen, not measured.** A quarter, on the reasoning that a trade that
    /// cannot make a quarter of its target in three times the expected time
    /// was not the move it was opened for.
    pub progress_floor: f64,
}

impl Default for StaleConfig {
    fn default() -> Self {
        StaleConfig { patience: 3.0, progress_floor: 0.25 }
    }
}

/// What has happened to the position.
///
/// `Going` rather than `Verdict`: eleven modules already define a `Verdict`,
/// and a twelfth is how somebody reads one and reasons about another.
#[derive(Debug, Clone, PartialEq)]
pub enum Going {
    /// It hit the stop. Nothing for this module to say.
    Stopped,
    /// It hit the target.
    Reached,
    /// Not yet given a fair run at it.
    TooEarly { bars: usize, implied: f64 },
    /// Running, past the implied pace, and making progress.
    Working { bars: usize, best: f64 },
    /// Running, past the patience, and going nowhere.
    Stale { bars: usize, implied: f64, best: f64 },
}

impl Going {
    pub fn is_stale(&self) -> bool {
        matches!(self, Going::Stale { .. })
    }

    pub fn plain(&self) -> String {
        match self {
            Going::Stopped => "It hit the stop. That's the trade being wrong, which is what a \
                                 stop is for."
                .into(),
            Going::Reached => "It reached the target.".into(),
            Going::TooEarly { bars, implied } => format!(
                "{bars} bars in, and this market's own movement says a move this size takes \
                 about {implied:.0}. Too early to call it anything."
            ),
            Going::Working { bars, best } => format!(
                "{bars} bars in and it's got {:.0}% of the way. Slow, but it is moving in the \
                 direction it was opened for.",
                best * 100.0
            ),
            Going::Stale { bars, implied, best } => format!(
                "{bars} bars in against an implied {implied:.0}, and the best it has managed is \
                 {:.0}% of the way. This isn't a winner in the making — the reading it was \
                 opened on has expired and the position is paying spread and swap to stay in a \
                 trade with no thesis. It hasn't lost. It also isn't working.",
                best * 100.0
            ),
        }
    }
}

/// Read an open position against the bars since it was opened.
///
/// The view must reach back to the bar the trade was opened on. It is an error
/// rather than a guess if it does not: reading progress from a window that
/// starts mid-trade would report the best of the last few bars as the best of
/// the trade, which flatters every stale position there is.
pub fn how_its_going(
    open: &Open,
    view: &AsOf<'_>,
    average_range: Option<f64>,
    cfg: &StaleConfig,
) -> Result<Going, String> {
    let time = view.time();
    if time.is_empty() {
        return Err("these bars carry no timestamps, so I can't tell which of them are since the \
                    trade was opened"
            .into());
    }
    let n = view.len().min(time.len());
    let Some(from) = (0..n).find(|&i| time[i] >= open.opened_at) else {
        return Err("none of these bars are from after the trade was opened".into());
    };
    if from == 0 && time[0] > open.opened_at {
        return Err("these bars start after the trade was opened, so the best it has been isn't \
                    in them — and reading progress off a window that starts mid-trade flatters \
                    every stale position there is"
            .into());
    }

    let (h, l) = (view.high(), view.low());
    let mut best = f64::NEG_INFINITY;
    for i in from..n {
        // The stop and the target settle it, and the stop is checked first:
        // when a single bar spans both, there is no way to know from a bar
        // which came first, and assuming the good one is how a backtest
        // invents money.
        let touched_stop = match open.side {
            Side::Buy => l[i] <= open.stop,
            Side::Sell => h[i] >= open.stop,
        };
        if touched_stop {
            return Ok(Going::Stopped);
        }
        let touched_target = match open.side {
            Side::Buy => h[i] >= open.target,
            Side::Sell => l[i] <= open.target,
        };
        if touched_target {
            return Ok(Going::Reached);
        }
        let favourable = match open.side {
            Side::Buy => h[i],
            Side::Sell => l[i],
        };
        best = best.max(open.share_of_the_way(favourable));
    }

    let bars = n - from;
    let Some(implied) = implied_bars(open, average_range) else {
        return Err("I can't work out how long a move this size ought to take here — without \
                    that, 'it's taking too long' is just an opinion about a number"
            .into());
    };

    if (bars as f64) < implied * cfg.patience {
        if (bars as f64) < implied {
            return Ok(Going::TooEarly { bars, implied });
        }
        return Ok(Going::Working { bars, best });
    }
    if best < cfg.progress_floor {
        Ok(Going::Stale { bars, implied, best })
    } else {
        Ok(Going::Working { bars, best })
    }
}

/// How many bars this market's own movement says a move to target takes.
///
/// Distance over distance-per-bar, and nothing else. It is a pace, not a
/// prediction: price does not travel in one direction, so the real number is
/// always larger — which is exactly what [`StaleConfig::patience`] is for.
pub fn implied_bars(open: &Open, average_range: Option<f64>) -> Option<f64> {
    let range = average_range?;
    if !range.is_finite() || range <= 0.0 {
        return None;
    }
    let distance = open.to_target();
    (distance > 0.0).then(|| distance / range)
}

/// Said out loud, with the standing caveat about where the two numbers came
/// from.
pub fn spoken(
    open: &Open,
    view: &AsOf<'_>,
    average_range: Option<f64>,
    cfg: &StaleConfig,
) -> String {
    match how_its_going(open, view, average_range, cfg) {
        Err(e) => e,
        Ok(v) => {
            let mut said = v.plain();
            if v.is_stale() {
                said.push_str(
                    " Worth knowing where that judgement comes from: waiting three times the \
                     implied pace, and calling a quarter of the way 'nowhere', are both choices \
                     rather than findings. Thirty scored trades would settle them properly.",
                );
            }
            said
        }
    }
}
