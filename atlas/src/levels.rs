//! Where a stop and a target go, and why.
//!
//! ## Best, and what that word is allowed to mean
//!
//! It names a best target **by this analysis** — the nearest level in the way
//! that clears the minimum reward, because the nearest level is the one price
//! is most likely to actually reach — and it lists the runners-up with what
//! each would be worth. That is a defensible claim about a reading of the
//! chart.
//!
//! What it does not do is claim the analysis is the truth. **An unscored
//! analysis is worth nothing**: a call made here is only as good as the
//! record of checking calls like it against what actually happened.
//!
//! It also will not tell you which way to trade. That belongs to whatever is
//! arguing the direction out.
//!
//! ## What it does instead
//!
//! Given a direction someone else has decided, it answers three questions of
//! arithmetic:
//!
//! 1. **Where is this idea wrong?** A stop goes beyond the structure whose
//!    breaking would mean the reason for the trade is gone — not at a round
//!    number, and not at a fixed pip distance.
//! 2. **How far is the next thing in the way?** That is the target, because
//!    it is where price has something to do other than continue.
//! 3. **Does the arithmetic survive the cost?** The spread is charged twice
//!    and compared to the reward being aimed at. This is the check that makes
//!    a small target at a small timeframe *arithmetically impossible* rather
//!    than merely unwise: a spread charged on both legs is a small share of a
//!    one-R move on an hourly chart and a large one on a one-minute chart, and
//!    a target a small fraction of the stop away can need a win rate above
//!    100% just to pay for itself.
//!
//! Every answer carries the reason for it and the price at which the idea is
//! wrong. A level with no stated invalidation is not a level, it is a hope.
//!
//! ## Refusing is a result
//!
//! `propose` returns a refusal far more often than it returns an idea, and
//! each refusal names its own cause and the number behind it. That is the
//! point. A system that always has an answer is a system whose answer means
//! nothing, and this codebase has been burned specifically by things that
//! produced confident output from nothing at all.

use crate::market::bars::AsOf;
use crate::market::levels as sr;
use crate::market::structure::{self, Trend};
use serde::{Deserialize, Serialize};

/// Which way the trade goes. Decided elsewhere, never here.
///
/// `market::claims::Side` says `Long`/`Short` for the same idea. Both spellings
/// are kept — an order is bought or sold, a claim argues long or short — but
/// the conversion lives here rather than being written out at each boundary,
/// because a conversion written four times is a conversion that is wrong in one
/// of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Buy,
    Sell,
}

impl From<Side> for crate::market::claims::Side {
    fn from(s: Side) -> crate::market::claims::Side {
        match s {
            Side::Buy => crate::market::claims::Side::Long,
            Side::Sell => crate::market::claims::Side::Short,
        }
    }
}

impl Side {
    /// The same direction in the claim vocabulary.
    pub fn as_claim(self) -> crate::market::claims::Side {
        self.into()
    }

    pub fn plain(self) -> &'static str {
        match self {
            Side::Buy => "buy",
            Side::Sell => "sell",
        }
    }

    fn agrees_with(self, trend: Trend) -> bool {
        matches!((self, trend), (Side::Buy, Trend::Up) | (Side::Sell, Trend::Down))
    }
}

/// What the account can lose, and what a price move is worth in it.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Purse {
    /// The balance, in the account's own currency.
    pub balance: f64,
    /// What one unit of the instrument gains for a one-unit move in price.
    ///
    /// Supplied rather than worked out here, because it depends on the
    /// instrument, the account currency and in some cases a second exchange
    /// rate — and a position size computed from a guessed conversion is worse
    /// than no position size at all.
    pub value_per_point: f64,
}

/// The limits a trade has to fit inside.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct Rules {
    /// How much of the balance may be at risk on one trade. 0.01 is one per
    /// cent.
    pub risk_fraction: f64,
    /// How far beyond the structure the stop goes, in average ranges. A stop
    /// exactly at the level gets taken by the wick that tests it.
    pub buffer_ranges: f64,
    /// The narrowest a stop may be, in average ranges. A stop closer than the
    /// market's ordinary movement is not a stop, it is a coin toss with a
    /// fee.
    pub min_stop_ranges: f64,
    /// The most of the reward the spread may eat before the trade is refused.
    pub max_cost_share: f64,
    /// The least reward, in multiples of the risk, worth taking.
    pub min_reward: f64,
    /// Bars behind the average range.
    ///
    /// A bar count, and `market::timeframe` is right that this is the wrong
    /// unit — fourteen bars is fourteen minutes on M1 and two and a half days
    /// on H4. Kept as a count for now, and named here as the next thing to
    /// convert rather than left to be discovered.
    pub range_window: usize,
    /// Bars either side of a turning point before it counts as one.
    pub pivot_reach: usize,
    /// How far from price to look for levels, in pips.
    pub level_span_pips: f64,
    /// Bars of structure behind the trend this idea is measured against.
    pub structure_bars: usize,
    /// Refuse to propose anything inside a scheduled release's window.
    ///
    /// On, and it should stay on. Off is for reading history that predates the
    /// calendar, and it is the one switch here that turns a rule into a
    /// suggestion.
    pub mind_the_calendar: bool,
}

impl Default for Rules {
    fn default() -> Self {
        Rules {
            risk_fraction: 0.01,
            buffer_ranges: 0.25,
            min_stop_ranges: 1.0,
            max_cost_share: 0.10,
            min_reward: 1.0,
            range_window: 14,
            pivot_reach: 2,
            level_span_pips: 50.0,
            structure_bars: 120,
            mind_the_calendar: true,
        }
    }
}

/// A trade, with its reason and its invalidation.
#[derive(Debug, Clone, PartialEq)]
pub struct Idea {
    pub side: Side,
    pub entry: f64,
    pub stop: f64,
    pub target: f64,
    /// How many times the risk the target is worth.
    pub reward: f64,
    /// How much of the balance is at risk, in its own currency.
    pub risking: f64,
    /// How many units that is.
    pub size: f64,
    /// What the spread costs as a share of the reward being aimed at.
    pub cost_share: f64,
    /// Why these levels, in words.
    pub why: String,
    /// What would have to happen for the reason to be gone.
    pub wrong_if: String,
    /// The other levels that could have been the target, and what each
    /// would have been worth. Shown rather than discarded: the runner-up is
    /// the first thing anybody argues about, and hiding it makes the chosen
    /// one look like the only possibility.
    pub instead: Vec<(f64, f64, String)>,
    /// True when the direction runs against the structure. Not a refusal —
    /// the direction is the caller's to decide, and this module is in no
    /// position to overrule it — but it is said out loud rather than passed
    /// over in silence.
    pub against_the_structure: bool,
}

impl Idea {
    /// The runners-up, in words.
    pub fn other_targets(&self) -> String {
        if self.instead.is_empty() {
            return "There's nothing else in the way worth aiming at.".into();
        }
        let listed: Vec<String> = self
            .instead
            .iter()
            .map(|(at, reward, why)| format!("{at:.5} ({reward:.1}x, {why})"))
            .collect();
        format!("Also in the way: {}.", listed.join("; "))
    }

    /// Said out loud, in the order a person would ask.
    pub fn spoken(&self) -> String {
        let mut said = format!(
            "{} at {:.5}, out at {:.5}, aiming for {:.5} — {:.1} times the risk. {}",
            self.side.plain(),
            self.entry,
            self.stop,
            self.target,
            self.reward,
            self.why
        );
        said.push_str(&format!(" Wrong if {}.", self.wrong_if));
        if self.against_the_structure {
            said.push_str(" Worth saying: that's against the way the structure is pointing.");
        }
        said
    }
}

/// Why there is no trade.
///
/// An enum rather than a string, so a caller can count the reasons over a
/// month and find out which limit is actually doing the refusing — which is
/// a far more useful thing to know than any single refusal.
#[derive(Debug, Clone, PartialEq)]
pub enum NoTrade {
    /// Nothing to hang a stop on.
    NoStructure(String),
    /// The market could not be read at all.
    Unreadable(String),
    /// The stop would sit inside the ordinary movement of the market.
    InsideTheNoise(String),
    /// The spread eats too much of what the trade is aiming at.
    CostsTooMuch(String),
    /// The next thing in the way is too close to be worth the risk.
    NotEnoughRoom(String),
    /// The numbers given do not describe an account.
    NoMoney(String),
    /// There is a scheduled release in front of this, or inside the bar.
    ///
    /// Its own variant rather than folded into `Unreadable`, because the two
    /// mean opposite things about the analysis. Unreadable is "I could not
    /// work this out". This is "I worked it out and I am not acting on it",
    /// and only one of those should count against a reading when the record is
    /// scored.
    StandingDown(String),
    /// The moment itself is a bad one to get filled in — the rollover, or a
    /// book with nobody on it.
    ///
    /// Separate from `StandingDown` because the fix is different and the
    /// caller can act on the difference: standing down means *not this trade*,
    /// and a thin book means *not this minute*. Twenty minutes later the same
    /// idea may be perfectly fine.
    ThinBook(String),
}

impl NoTrade {
    pub fn plain(&self) -> &str {
        match self {
            NoTrade::NoStructure(s)
            | NoTrade::StandingDown(s)
            | NoTrade::Unreadable(s)
            | NoTrade::InsideTheNoise(s)
            | NoTrade::CostsTooMuch(s)
            | NoTrade::NotEnoughRoom(s)
            | NoTrade::ThinBook(s)
            | NoTrade::NoMoney(s) => s,
        }
    }

    /// A short, stable name for counting them.
    pub fn label(&self) -> &'static str {
        match self {
            NoTrade::NoStructure(_) => "no structure",
            NoTrade::Unreadable(_) => "unreadable",
            NoTrade::InsideTheNoise(_) => "inside the noise",
            NoTrade::CostsTooMuch(_) => "costs too much",
            NoTrade::NotEnoughRoom(_) => "not enough room",
            NoTrade::NoMoney(_) => "no money",
            NoTrade::StandingDown(_) => "standing down",
            NoTrade::ThinBook(_) => "thin book",
        }
    }
}

/// The gaps price left behind, as (low, high) pairs, still unfilled.
///
/// Their reader has no fair-value-gap finder, so this is the one piece of my
/// old `market` module that survives the merge — moved here, beside its only
/// caller, rather than kept alive as a module of its own.
///
/// A gap is three bars where the middle one moved far enough that the first
/// and third do not overlap. Price returning to fill one is ordinary, which is
/// exactly why an unfilled one is somewhere price has something to do other
/// than continue — and a target set past it is a target behind the thing that
/// actually stops the move.
pub fn gaps(view: &AsOf<'_>) -> Vec<(f64, f64)> {
    let (high, low) = (view.high(), view.low());
    let n = view.len();
    let mut open = Vec::new();
    for i in 2..n {
        let (lo, hi) = if low[i] > high[i - 2] {
            (high[i - 2], low[i])
        } else if high[i] < low[i - 2] {
            (high[i], low[i - 2])
        } else {
            continue;
        };
        // Filled if anything since has traded back through it. Checked against
        // the bars after the gap only — a gap is not filled by the bar that
        // made it.
        let filled = (i + 1..n).any(|j| low[j] <= hi && high[j] >= lo);
        if !filled {
            open.push((lo, hi));
        }
    }
    open
}

/// Is a stop sitting inside the market's ordinary movement?
///
/// Returns how many average ranges away it is. `None` when there is no
/// average range to compare against — which is not "the stop is fine", and
/// callers have to say which of the two they mean.
///
/// Pulled out as its own function so that every place asking "is this stop in
/// the noise?" — deciding whether to widen a stop on entry, or reading one on
/// a trade already open — gets the same answer from the same arithmetic,
/// rather than an opinion with a name and nothing behind it.
///
/// The reasoning states in a line: a stop closer than one ordinary bar range
/// is not a stop, it is a coin toss on the next bar. Over the several bars a
/// trade needs to reach its target, being clipped by ordinary movement stops
/// being a risk and becomes the expected outcome.
pub fn how_many_ranges_out(stop_distance: f64, average_range: Option<f64>) -> Option<f64> {
    let range = average_range?;
    if !range.is_finite() || range <= 0.0 || !stop_distance.is_finite() || stop_distance < 0.0 {
        return None;
    }
    Some(stop_distance / range)
}

/// The same question, answered against the rules.
pub fn stop_is_in_noise(
    stop_distance: f64,
    average_range: Option<f64>,
    rules: &Rules,
) -> Option<bool> {
    Some(how_many_ranges_out(stop_distance, average_range)? < rules.min_stop_ranges)
}

/// What the spread costs as a share of the reward being aimed at.
///
/// Charged on both legs, because it is. This single number is why a tiny
/// target on a small timeframe is not a strategy: when the round-trip spread
/// is a large share of the reward, the win rate needed to break even can pass
/// 100%.
/// How far this market actually travelled across the last `bars`.
///
/// High-water to low-water over the window, which is the plainest available
/// answer to "has price shown it can get there". Deliberately not an average
/// range times a multiplier — that is two invented numbers where one measured
/// one will do.
///
/// `None` when there is nothing to measure. A caller must treat that as "I
/// cannot tell" rather than as nought, because nought refuses everything.
pub fn how_far_it_travels(view: &AsOf<'_>, bars: usize) -> Option<f64> {
    let n = view.len();
    if n == 0 || bars == 0 {
        return None;
    }
    let from = n.saturating_sub(bars);
    let high = view.high()[from..n].iter().copied().fold(f64::NEG_INFINITY, f64::max);
    let low = view.low()[from..n].iter().copied().fold(f64::INFINITY, f64::min);
    (high.is_finite() && low.is_finite() && high >= low).then_some(high - low)
}

pub fn cost_share(stop_distance: f64, spread: f64, reward: f64) -> Option<f64> {
    if stop_distance <= 0.0 || reward <= 0.0 || !spread.is_finite() || spread < 0.0 {
        return None;
    }
    let share = (spread * 2.0) / (stop_distance * reward);
    share.is_finite().then_some(share)
}

/// Where the stop and target go for a direction somebody else chose.
///
/// `price` is where the trade would go on; `spread` is the current one, in
/// price, not in pips — pips differ by instrument and a unit confusion here
/// costs real money.
/// Where a stop and a target go for a given direction, and what size pays for
/// it.
///
/// ## Why this reads a view rather than a market summary
///
/// It used to take a `Market` — a struct of everything my old reader had
/// worked out, computed once from the whole series. That is a convenient shape
/// and it has one property that matters: a summary computed over the whole
/// series says nothing about *when* each part of it became knowable. A swing
/// high needs bars on both sides to be confirmed, so a summary that reports one
/// is reporting something that was not true yet at the bar it is attached to.
///
/// `AsOf` is a borrow bounded at a bar. Everything below is read through it,
/// so nothing here can be told about a level that had not formed.
///
/// Prices, stops and sizes are exactly what a person trading by hand asks for,
/// so this reads the bars it is given and answers.
pub fn propose(
    view: &AsOf<'_>,
    instrument: &str,
    side: Side,
    price: f64,
    spread: f64,
    purse: &Purse,
    rules: &Rules,
) -> Result<Idea, NoTrade> {
    if !price.is_finite() || price <= 0.0 {
        return Err(NoTrade::Unreadable("that isn't a price".into()));
    }

    // First, before any of the arithmetic. A trade proposed into a release is
    // the failure the calendar exists to stop, and working out a beautiful
    // stop and target for one first would only make it more persuasive.
    //
    // The instrument is why this needs a name rather than just bars: a US
    // release moves AUD/JPY with no dollar in it at all, because the carry
    // cross-section unwinds.
    if rules.mind_the_calendar {
        match crate::standdown::standing_down(
            view,
            instrument,
            &crate::standdown::StanddownConfig::default(),
        ) {
            Ok(Some(why)) => return Err(NoTrade::StandingDown(why.plain())),
            // No clock is not the same as clear — but it is not standing down
            // either. Standing down is a judgement about the market; this is
            // the calendar being unable to answer because the bars arrived
            // without times on them. That is a defect in what was handed in,
            // so it goes back as unreadable and is scored as one.
            Err(no) => return Err(NoTrade::Unreadable(no)),
            Ok(None) => {}
        }

        // And the other moment the fill goes wrong for reasons that have
        // nothing to do with the reading: the 17:00 New York rollover, when
        // every desk squares at once and the quoted spread is several times
        // its normal width. Same switch, because it is the same class of
        // thing — a moment to have no view in — and because a caller reading
        // history with no clock should not be nagged about either.
        if let Some(now) = view.opened_at() {
            if let Some(t) = crate::rollover::thin(now) {
                return Err(NoTrade::ThinBook(t.plain()));
            }
        }
    }
    if !purse.balance.is_finite()
        || purse.balance <= 0.0
        || !purse.value_per_point.is_finite()
        || purse.value_per_point <= 0.0
    {
        return Err(NoTrade::NoMoney(
            "I don't have a balance and a value per point I can size against — a size worked \
             out from a guess is worse than no size"
                .into(),
        ));
    }
    // Wilder's average true range, from their `bars`. The window is a bar
    // count here rather than a length of time, which `market::timeframe` shows
    // is the wrong unit — twenty bars is twenty minutes on M1 and a week on
    // H4. Named as the next thing to fix rather than quietly left.
    let range = view.atr(rules.range_window);
    if !range.is_finite() || range <= 0.0 {
        return Err(NoTrade::Unreadable(
            "the market's average range is nothing, which is not a market".into(),
        ));
    }

    // The structure this idea rests on, and the thing in the way of it.
    //
    // `sr::nearest` gives the closest level on each side, and it includes the
    // round numbers — which my old reader did not have at all. Stops cluster
    // at the figure, so a target one pip past it is a target that does not get
    // filled and a stop just inside it is a stop that gets taken.
    let long = side == Side::Buy;
    let below = sr::nearest(view, true, rules.pivot_reach).ok().flatten().map(|l| l.price);
    let above = sr::nearest(view, false, rules.pivot_reach).ok().flatten().map(|l| l.price);
    let (behind, ahead) = if long { (below, above) } else { (above, below) };
    let Some(behind) = behind else {
        return Err(NoTrade::NoStructure(format!(
            "there's no {} to put a stop beyond",
            match side {
                Side::Buy => "low",
                Side::Sell => "high",
            }
        )));
    };
    let Some(ahead) = ahead else {
        return Err(NoTrade::NoStructure(
            "there's nothing in the way to aim at, so I'd be picking a number".into(),
        ));
    };

    let buffer = range * rules.buffer_ranges;
    let raw_stop = match side {
        Side::Buy => behind - buffer,
        Side::Sell => behind + buffer,
    };
    let mut stop = raw_stop;
    let mut widened = false;
    let distance = (price - stop).abs();
    let floor_distance = range * rules.min_stop_ranges;
    // Through the shared function, so there is only ever one definition of
    // "in the noise".
    if stop_is_in_noise(distance, Some(range), rules).unwrap_or(false) {
        // Widened rather than refused: the structure is still the reason, the
        // market's own movement is just bigger than it today. Said out loud
        // in `why`, because a stop that is not where the structure is has
        // stopped meaning what it looks like it means.
        stop = match side {
            Side::Buy => price - floor_distance,
            Side::Sell => price + floor_distance,
        };
        widened = true;
    }
    let risk_distance = (price - stop).abs();
    if risk_distance <= 0.0 {
        return Err(NoTrade::InsideTheNoise(
            "the stop works out at the entry price, which is not a stop".into(),
        ));
    }

    // Everything price would have to get through, not just the far wall.
    // A pool of resting orders and an unfilled gap are both places price has
    // something to do other than continue, and leaving them out is how a
    // target ends up behind the thing that actually stops the move.
    let mut candidates: Vec<(f64, String)> = vec![(ahead, "the next level in the way".to_string())];
    if let Ok(found) = sr::levels(view, rules.pivot_reach, rules.level_span_pips) {
        for level in found {
            let ahead_of_price =
                (long && level.price > price) || (!long && level.price < price);
            if !ahead_of_price {
                continue;
            }
            candidates.push((
                level.price,
                if level.on_figure {
                    format!("a round number, {} touch(es)", level.touches)
                } else {
                    format!("orders resting at {} touch(es)", level.touches)
                },
            ));
        }
    }
    // Yesterday's high and low, and last week's. After the figure these are
    // the two most-watched lines on an FX chart — and that is the whole
    // argument for them: price stops there because everyone is looking at
    // them, not because of anything the bars know.
    //
    // They need timestamps. A series with none gets nothing rather than a
    // guess, because a "prior day" invented from bar counts would be a
    // different day on every timeframe.
    for (at, what) in crate::fxday::levels(view) {
        if (long && at > price) || (!long && at < price) {
            candidates.push((at, what.to_string()));
        }
    }

    // And last night's two lines, for the same reason and with the same
    // caveat: they are watched because everybody can see them, not because of
    // anything the bars know. Empty while Asia is still trading, because a
    // high that can still move is not a line anybody is defending.
    for (at, what) in crate::asia::levels(view) {
        if (long && at > price) || (!long && at < price) {
            candidates.push((at, what.to_string()));
        }
    }

    for void in gaps(view) {
        let edge = if long { void.0 } else { void.1 };
        if (long && edge > price) || (!long && edge < price) {
            candidates.push((edge, "a gap price hasn't come back through".to_string()));
        }
    }

    // Nothing further than this market has actually gone.
    //
    // The round-number grid does not care what the chart is doing: it offers
    // the next figure whether or not price has ever been near it. On a market
    // sitting in a four-pip box that puts a target fifty pips away on the
    // list, and it wins on reward every time — seven times the risk, off a
    // range that has not moved four pips in the whole window. It is not a
    // target, it is a wish with a price on it.
    //
    // The cap is measured rather than chosen: how far this market travelled
    // across the window the structure was read over. A quiet market gets a
    // near cap and a moving one gets a far cap, without anybody picking a
    // number. When the span cannot be measured, nothing is filtered — a cap
    // of nought would refuse every trade and look like a rule.
    let reach = how_far_it_travels(view, rules.structure_bars);
    if let Some(reach) = reach.filter(|r| *r > 0.0) {
        let before = candidates.len();
        candidates.retain(|(at, _)| (at - price).abs() <= reach);
        if candidates.is_empty() {
            return Err(NoTrade::NotEnoughRoom(format!(
                "every level worth aiming at is further off than this market has moved in the \
                 last {} bars ({:.5}), so there was nothing to aim at that it has shown it can \
                 reach — {} candidate(s) dropped",
                rules.structure_bars, reach, before
            )));
        }
    }

    // Nearest first. The nearest level that pays enough is the best target
    // *by this analysis*, on the stated grounds that the nearest one is the
    // likeliest to actually be reached — not on the grounds that anybody
    // knows where price is going.
    candidates.sort_by(|a, b| {
        (a.0 - price)
            .abs()
            .partial_cmp(&(b.0 - price).abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let scored: Vec<(f64, f64, String)> = candidates
        .into_iter()
        .map(|(at, why)| ((at - price).abs() / risk_distance, at, why))
        .map(|(reward, at, why)| (at, reward, why))
        .collect();
    let Some(chosen) = scored.iter().find(|(_, reward, _)| *reward >= rules.min_reward).cloned()
    else {
        let best = scored
            .iter()
            .map(|(_, reward, _)| *reward)
            .fold(0.0f64, f64::max);
        return Err(NoTrade::NotEnoughRoom(format!(
            "the nearest level worth aiming at is {:.1} times the risk away and I want at least \
             {:.1}",
            best, rules.min_reward
        )));
    };
    let (target, reward, target_why) = chosen;
    let instead: Vec<(f64, f64, String)> = scored
        .into_iter()
        .filter(|(at, _, _)| (*at - target).abs() > f64::EPSILON)
        .take(3)
        .collect();
    let Some(share) = cost_share(risk_distance, spread, reward) else {
        return Err(NoTrade::CostsTooMuch("I can't work out what the spread costs".into()));
    };
    if share > rules.max_cost_share {
        return Err(NoTrade::CostsTooMuch(format!(
            "the spread costs {:.0}% of what this trade is aiming at, and my limit is {:.0}% — \
             a wider stop or a further target would fix it, a smaller one never will",
            share * 100.0,
            rules.max_cost_share * 100.0
        )));
    }

    let risking = purse.balance * rules.risk_fraction;
    let size = risking / (risk_distance * purse.value_per_point);
    if !size.is_finite() || size <= 0.0 {
        return Err(NoTrade::NoMoney(
            "the size works out at nothing, so there is no trade to place".into(),
        ));
    }

    let why = if widened {
        format!(
            "the stop sits a full {:.1} average ranges out because the structure at {:.5} is \
             closer than the market's ordinary movement; the target is {} at {:.5}",
            rules.min_stop_ranges, behind, target_why, target
        )
    } else {
        format!(
            "the stop sits {:.2} beyond the {} at {:.5}; the target is the nearest thing in the \
             way that pays enough — {} at {:.5}",
            buffer,
            match side {
                Side::Buy => "low",
                Side::Sell => "high",
            },
            behind,
            target_why,
            target
        )
    };
    let wrong_if = format!(
        "price closes {} {:.5}",
        match side {
            Side::Buy => "below",
            Side::Sell => "above",
        },
        behind
    );

    Ok(Idea {
        side,
        entry: price,
        stop,
        target,
        reward,
        risking,
        size,
        cost_share: share,
        why,
        wrong_if,
        instead,
        against_the_structure: !side
            .agrees_with(structure::recent(view, rules.structure_bars, rules.pivot_reach).trend()),
    })
}
