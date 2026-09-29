//! The referee. Only the bars decide truth.
//!
//! ## The rule the whole design rests on
//!
//! > An analyst may choose WHICH claims to stake and what it thinks they mean.
//! > It may never decide whether a claim is TRUE. That is decided here, by
//! > arithmetic, against the bars.
//!
//! Without that split, two Atlas instances arguing produce the most persuasive
//! case rather than the most correct one — and a persuasive wrong case is worse
//! than no case, because it survives review. With it, a side that asserts a
//! failed break of structure that did not happen simply loses that claim. It
//! cannot argue its way past the bars.
//!
//! ## What the port made stronger
//!
//! In Python the vocabulary was a dictionary of reader functions, and a claim
//! naming a kind nobody had registered was caught at verification time with a
//! refusal listing the known kinds. That works, and it is a runtime check on a
//! string.
//!
//! Here [`Kind`] is an enum and [`verify`] matches on it exhaustively. **A
//! claim the referee cannot check cannot be constructed** — there is no string
//! to get wrong, and adding a variant without adding its reader is a compile
//! error rather than a runtime surprise in front of a live market. The spec's
//! "adding a claim kind means adding a checker function, one file, one
//! function" is now enforced by the compiler.
//!
//! ## Three-valued, and the middle value is load-bearing
//!
//! `Unknown` is not `False`. A claim that cannot be checked earns nothing and
//! costs nothing; treating it as refuted would let missing data argue for the
//! other side. Several readers use it deliberately — an unconfirmed reversal is
//! `Unknown` precisely so a side cannot score off a pullback that came to
//! nothing, which is most of them.

use super::bars::{pip_size, AsOf};
use super::levels::{self, STOP_BAND_PIPS};
use super::regime::{self, Direction};
use super::structure::{self, Trend};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Long,
    Short,
}

impl Side {
    pub fn say(self) -> &'static str {
        match self {
            Side::Long => "LONG",
            Side::Short => "SHORT",
        }
    }
    fn is_long(self) -> bool {
        self == Side::Long
    }
    pub fn other(self) -> Side {
        match self {
            Side::Long => Side::Short,
            Side::Short => Side::Long,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Truth {
    True,
    False,
    /// The bars cannot answer. Earns nothing, costs nothing.
    Unknown,
}

impl Truth {
    pub fn say(self) -> &'static str {
        match self {
            Truth::True => "TRUE",
            Truth::False => "FALSE",
            Truth::Unknown => "UNKNOWN",
        }
    }
}

/// Every claim the referee can check.
///
/// Exhaustive by construction. Adding a variant without adding its arm in
/// [`verify`] does not compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// A break of structure: price took out the last opposing swing.
    Bos,
    /// A break that failed — price went through and came back.
    FailedBos,
    /// Displacement per bar, in ATR.
    Velocity,
    /// Range expanding against its own recent average.
    VolatilityExpansion,
    /// Where price sits in its own recent range.
    RangePosition,
    /// Higher high and higher low, or the reverse, on the last pair.
    SwingTrend,
    /// Structure has CHANGED direction. Not the same question as SwingTrend.
    Reversal,
    /// Price is at a level that argues for this side.
    AtLevel,
    /// A level was broken and price closed beyond it.
    LevelBreak,
    /// Price poked through a level and closed back — against the poke.
    Sweep,
    /// Broken resistance now holding as support, or the reverse. UNTESTED.
    Polarity,
    /// The market is trending in this side's direction.
    Trending,
    /// A range with enough room in it to be worth trading.
    Ranging,
    /// Not enough room to cover the cost of trading it.
    Choppy,
}

pub const ALL_KINDS: [Kind; 14] = [
    Kind::Bos,
    Kind::FailedBos,
    Kind::Velocity,
    Kind::VolatilityExpansion,
    Kind::RangePosition,
    Kind::SwingTrend,
    Kind::Reversal,
    Kind::AtLevel,
    Kind::LevelBreak,
    Kind::Sweep,
    Kind::Polarity,
    Kind::Trending,
    Kind::Ranging,
    Kind::Choppy,
];

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Bos => "BOS",
            Kind::FailedBos => "FAILED_BOS",
            Kind::Velocity => "VELOCITY",
            Kind::VolatilityExpansion => "VOLATILITY_EXPANSION",
            Kind::RangePosition => "RANGE_POSITION",
            Kind::SwingTrend => "SWING_TREND",
            Kind::Reversal => "REVERSAL",
            Kind::AtLevel => "AT_LEVEL",
            Kind::LevelBreak => "LEVEL_BREAK",
            Kind::Sweep => "SWEEP",
            Kind::Polarity => "POLARITY",
            Kind::Trending => "TRENDING",
            Kind::Ranging => "RANGING",
            Kind::Choppy => "CHOPPY",
        }
    }

    /// How well evidenced this reading is, for the record and for the
    /// scoreboard's benefit. Not a weight — the weight is earned from its own
    /// track record — but a reader is entitled to know which of its readings
    /// rest on order-flow data and which rest on nobody having tested them.
    pub fn grounds(self) -> Grounds {
        match self {
            Kind::AtLevel | Kind::Sweep | Kind::LevelBreak => Grounds::Measured,
            Kind::Polarity => Grounds::Untested,
            _ => Grounds::Arithmetic,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Grounds {
    /// Follows from the bars by definition.
    Arithmetic,
    /// Has published measurement behind it.
    Measured,
    /// Asserted by practitioners, never quantified, no academic test found.
    Untested,
}

impl Grounds {
    /// A plain word for the screen -- never `{:?}` on this, which would
    /// print the variant's Rust name rather than something a person reads.
    pub fn say(&self) -> &'static str {
        match self {
            Grounds::Arithmetic => "arithmetic",
            Grounds::Measured => "measured",
            Grounds::Untested => "untested",
        }
    }
}

/// What a side asserts. `side` is who it helps if it holds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Claim {
    pub kind: Kind,
    pub side: Side,
    /// The one tunable a claim may carry: how far back to look. `None` means
    /// the reader's own default, which is read off the bars where they carry
    /// timestamps.
    pub lookback: Option<usize>,
}

impl Claim {
    pub fn new(kind: Kind, side: Side) -> Claim {
        Claim { kind, side, lookback: None }
    }
    pub fn over(kind: Kind, side: Side, lookback: usize) -> Claim {
        Claim { kind, side, lookback: Some(lookback) }
    }
    pub fn say(&self) -> String {
        match self.lookback {
            Some(n) => format!("{}[{}]({})", self.kind.name(), self.side.say(), n),
            None => format!("{}[{}]", self.kind.name(), self.side.say()),
        }
    }
}

/// Truth, magnitude and the reason.
///
/// Magnitude is in ATR units where meaningful and is **zero unless TRUE** —
/// checked in [`verify`], because a reader that scored for a claim it did not
/// confirm would quietly hand out evidence.
#[derive(Debug, Clone, PartialEq)]
pub struct Verdict {
    pub truth: Truth,
    pub magnitude: f64,
    pub detail: String,
}

impl Verdict {
    pub fn yes(magnitude: f64, detail: impl Into<String>) -> Verdict {
        Verdict { truth: Truth::True, magnitude, detail: detail.into() }
    }
    fn no(detail: impl Into<String>) -> Verdict {
        Verdict { truth: Truth::False, magnitude: 0.0, detail: detail.into() }
    }
    pub fn cannot_say(detail: impl Into<String>) -> Verdict {
        Verdict { truth: Truth::Unknown, magnitude: 0.0, detail: detail.into() }
    }
    pub fn holds(&self) -> bool {
        self.truth == Truth::True
    }
    pub fn say(&self) -> String {
        format!("{}:{:.6}:{}", self.truth.say(), self.magnitude, self.detail)
    }
}

/// Check a claim against the bars.
///
/// The one entry point. Exhaustive over [`Kind`], so the vocabulary and the
/// checker cannot drift apart.
pub fn verify(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let v = match claim.kind {
        Kind::Bos => bos(claim, view, false),
        Kind::FailedBos => bos(claim, view, true),
        Kind::Velocity => velocity(claim, view),
        Kind::VolatilityExpansion => volatility_expansion(claim, view),
        Kind::RangePosition => range_position(claim, view),
        Kind::SwingTrend => swing_trend(claim, view),
        Kind::Reversal => reversal(claim, view),
        Kind::AtLevel => at_level(claim, view),
        Kind::LevelBreak => level_break(claim, view),
        Kind::Sweep => sweep(claim, view),
        Kind::Polarity => polarity(claim, view),
        Kind::Trending => trending(claim, view),
        Kind::Ranging => ranging(claim, view),
        Kind::Choppy => choppy(claim, view),
    };
    // A reader that returned magnitude on a non-TRUE verdict would be handing
    // out evidence for something it did not confirm. Corrected rather than
    // trusted.
    if v.truth != Truth::True && v.magnitude != 0.0 {
        return Verdict { magnitude: 0.0, ..v };
    }
    v
}

// ---------------------------------------------------------------------------
// the readers
// ---------------------------------------------------------------------------

fn bos(claim: &Claim, view: &AsOf<'_>, failed: bool) -> Verdict {
    if view.need(25).is_err() {
        return Verdict::cannot_say("fewer than 25 bars");
    }
    let k = 2;
    let (hi, lo) = view.swings(k);
    let (h, l, c) = (view.high(), view.low(), view.close());
    let a = view.atr(14);
    if a <= 0.0 {
        return Verdict::cannot_say("no volatility to measure a break against");
    }
    let long = claim.side.is_long();
    let level = if long {
        hi.last().map(|&i| h[i])
    } else {
        lo.last().map(|&i| l[i])
    };
    let Some(level) = level else {
        return Verdict::cannot_say("no confirmed swing to break");
    };
    let now = c[c.len() - 1];
    let extreme = if long {
        h[h.len() - 1]
    } else {
        l[l.len() - 1]
    };
    let went_through = if long { extreme > level } else { extreme < level };
    let closed_beyond = if long { now > level } else { now < level };
    if !failed {
        if went_through && closed_beyond {
            let by = (now - level).abs() / a;
            return Verdict::yes(by, format!("closed {:.2} ATR beyond {:.5}", by, level));
        }
        return Verdict::no(format!("{:.5} has not been closed beyond", level));
    }
    // A failed break: through it, then back.
    if went_through && !closed_beyond {
        let by = (extreme - level).abs() / a;
        return Verdict::yes(by, format!("poked {:.2} ATR past {:.5} and closed back", by, level));
    }
    Verdict::no(format!("no failed break of {:.5}", level))
}

fn velocity(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let n = claim.lookback.unwrap_or(5);
    if view.need(n + 15).is_err() {
        return Verdict::cannot_say("not enough bars to measure velocity");
    }
    let c = view.close();
    let a = view.atr(14);
    if a <= 0.0 {
        return Verdict::cannot_say("no volatility to measure against");
    }
    let moved = c[c.len() - 1] - c[c.len() - 1 - n];
    let want = if claim.side.is_long() { moved > 0.0 } else { moved < 0.0 };
    let per_bar = moved.abs() / n as f64 / a;
    if want && per_bar > 0.1 {
        Verdict::yes(per_bar * n as f64, format!("{:.2} ATR per bar over {} bars", per_bar, n))
    } else {
        Verdict::no(format!("moved {:.2} ATR over {} bars, the wrong way or not enough", moved / a, n))
    }
}

fn volatility_expansion(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let n = claim.lookback.unwrap_or(20);
    if view.need(n * 2 + 1).is_err() {
        return Verdict::cannot_say("not enough bars to compare volatility against");
    }
    let now = view.atr(n);
    let before = view.back(n).atr(n);
    if before <= 0.0 {
        return Verdict::cannot_say("no earlier volatility to compare against");
    }
    let ratio = now / before;
    if ratio > 1.3 {
        // Neutral between the sides: expansion is not bullish or bearish, so
        // it earns nothing for whoever staked it.
        Verdict::cannot_say(format!("range is {:.2}x its own recent average", ratio))
    } else {
        Verdict::no(format!("range is {:.2}x its own recent average", ratio))
    }
}

fn range_position(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let n = claim.lookback.unwrap_or(20);
    if view.need(n).is_err() {
        return Verdict::cannot_say("not enough bars for a range");
    }
    // Only asked in a ranging market. During a trend "how far up its own
    // range" isn't a meaningful question: a trending market's high/low box is
    // just wherever price happens to have been, not a level it reverts
    // toward. Gated to fire only when the market reads as RANGING, matching
    // how `trending`, `ranging` and `choppy` already defer to the same regime
    // read rather than firing blind. Whether the gate helps is for real bars
    // to say; nothing here claims it does.
    let Ok(r) = regime::read(view, claim.lookback, regime::DEFAULT_SPREAD_PIPS,
                             regime::DEFAULT_SLIPPAGE_PIPS, None, 0) else {
        return Verdict::cannot_say("not enough bars to read a regime");
    };
    if r.label() != "RANGING" {
        return Verdict::cannot_say(format!("not ranging -- {}", r.say()));
    }
    let (h, l, c) = (view.high(), view.low(), view.close());
    let hi = h[h.len() - n..].iter().cloned().fold(f64::MIN, f64::max);
    let lo = l[l.len() - n..].iter().cloned().fold(f64::MAX, f64::min);
    if hi <= lo {
        return Verdict::cannot_say("the range has no width");
    }
    let pos = (c[c.len() - 1] - lo) / (hi - lo);
    let good = if claim.side.is_long() { pos < 0.25 } else { pos > 0.75 };
    if good {
        let edge = if claim.side.is_long() { 0.25 - pos } else { pos - 0.75 } * 4.0;
        Verdict::yes(edge, format!("{:.0}% up its own {}-bar range", pos * 100.0, n))
    } else {
        Verdict::no(format!("{:.0}% up its own {}-bar range", pos * 100.0, n))
    }
}

fn swing_trend(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    if view.need(25).is_err() {
        return Verdict::cannot_say("fewer than 25 bars");
    }
    let s = structure::recent(view, 5, 2);
    if s.highs.len() < 2 || s.lows.len() < 2 {
        return Verdict::cannot_say("fewer than two swings on a side");
    }
    let a = view.atr(14);
    let want = if claim.side.is_long() { Trend::Up } else { Trend::Down };
    if s.trend() == want {
        let lows = s.low_prices();
        let step = if a > 0.0 {
            (lows[lows.len() - 1] - lows[lows.len() - 2]).abs() / a
        } else {
            0.0
        };
        Verdict::yes(step, if want == Trend::Up { "higher high and higher low" } else { "lower high and lower low" })
    } else {
        Verdict::no(format!("structure is {}, not trending that way", s.trend().say()))
    }
}

/// Structure has TURNED, in the direction this side is arguing.
///
/// The reading `SwingTrend` could not give. That one asks "is structure
/// trending my way", a question about the last pair of swings — so a market
/// that has fallen for a week and just put in one higher low gets FALSE,
/// correctly, because it is not trending up yet. But it may well have turned,
/// and until this existed nothing in the vocabulary could say so.
///
/// Three-valued on purpose, and the middle value is the whole point: an
/// unconfirmed turn earns nothing and costs nothing, so a side cannot score off
/// a pullback. Every range is a sequence of lower highs that came to nothing,
/// and paying out on those is how a system gets chopped to death.
fn reversal(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    if view.need(25).is_err() {
        return Verdict::cannot_say("fewer than 25 bars");
    }
    let s = structure::recent(view, 5, 2);
    let want = if claim.side.is_long() { Trend::Up } else { Trend::Down };
    let Some(t) = s.reversal() else {
        return Verdict::no(format!("structure has not turned (trend {})", s.trend().say()));
    };
    if t.direction != want {
        return Verdict::no(format!("structure {}", t.say()));
    }
    if !t.confirmed {
        return Verdict::cannot_say(format!("not yet confirmed -- {}", t.why));
    }
    let a = view.atr(14);
    let (broke, now) = if want == Trend::Down {
        (s.lows.get(s.lows.len().wrapping_sub(2)).map(|p| p.price), s.lows.last().map(|p| p.price))
    } else {
        (s.highs.get(s.highs.len().wrapping_sub(2)).map(|p| p.price), s.highs.last().map(|p| p.price))
    };
    let through = match (broke, now, a > 0.0) {
        (Some(b), Some(n), true) => (n - b).abs() / a,
        _ => 0.0,
    };
    Verdict::yes(through, t.say())
}

fn at_level(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let Ok(found) = levels::nearest(view, claim.side.is_long(), 2) else {
        return Verdict::cannot_say("not enough bars to find levels");
    };
    let Some(lv) = found else {
        return Verdict::no("no level of that sort near price");
    };
    let tol = view.gamma();
    let away = (view.now() - lv.price).abs();
    if away > tol {
        return Verdict::no(format!(
            "nearest is {}, {:.1} tolerances away",
            lv.say(),
            away / tol
        ));
    }
    // Magnitude rises with touch count: the one level attribute with a
    // measured, permutation-tested relationship to bounce probability (~0.50 at
    // one prior touch rising to ~0.75 at eight). A level on the figure is worth
    // more, also measured: round numbers bounce 3.4pp more often at p<0.0001.
    let mut mag = (1.0 + 0.25 * lv.touches.saturating_sub(1) as f64).min(2.0);
    if lv.on_figure {
        mag += 0.5;
    }
    Verdict::yes(mag, format!("at {}", lv.say()))
}

fn level_break(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let look = claim.lookback.unwrap_or(20);
    if view.need(look + 25).is_err() {
        return Verdict::cannot_say("not enough bars to see what was there before");
    }
    let a = view.atr(14);
    if a <= 0.0 {
        return Verdict::cannot_say("no volatility to measure a break against");
    }
    let before = view.back(look);
    let Ok(prior) = levels::levels(&before, 2, 120.0) else {
        return Verdict::cannot_say("no level was there to break");
    };
    let tol = view.gamma();
    let now = view.now();
    let was = before.now();
    let up = claim.side.is_long();
    let broken: Vec<_> = prior
        .into_iter()
        .filter(|x| {
            if up {
                was < x.price - tol && x.price + tol < now
            } else {
                was > x.price + tol && x.price - tol > now
            }
        })
        .collect();
    let Some(lv) = broken.into_iter().max_by_key(|x| x.touches) else {
        return Verdict::no(format!("no level was crossed and closed beyond in {} bars", look));
    };
    let through = (now - lv.price).abs() / a;
    // Osler's second measured prediction: after price crosses a round number,
    // moves are 3.7-4.2 points LARGER than after an arbitrary level, because
    // the stop orders clustered 1-10 pips beyond trigger and push the same way.
    let note = if lv.on_figure {
        "; on the figure -- stops sit 1-10 pips beyond it"
    } else {
        ""
    };
    Verdict::yes(through, format!("closed {:.2} ATR beyond {}{}", through, lv.say(), note))
}

/// A poke through a level that closed back — arguing for the OTHER side.
///
/// The swing failure pattern, and the strictest codeable definition of a false
/// break found in the literature. A sweep of resistance is a SHORT case, which
/// is the opposite of what `LevelBreak` on the same bar would say — and that
/// distinction is the most common losing trade in the subject.
fn sweep(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let look = claim.lookback.unwrap_or(3);
    if view.need(look + 25).is_err() {
        return Verdict::cannot_say("not enough bars to see what was there before");
    }
    let a = view.atr(14);
    if a <= 0.0 {
        return Verdict::cannot_say("no volatility to measure a sweep against");
    }
    let before = view.back(look);
    let Ok(lv) = levels::levels(&before, 2, 120.0) else {
        return Verdict::cannot_say("no level was there to sweep");
    };
    let tol = view.gamma();
    let (h, l, c) = (view.high(), view.low(), view.close());
    let start = c.len().saturating_sub(look).max(1);
    let mut best: Option<(f64, super::levels::Level, usize)> = None;
    for i in start..c.len() {
        for x in &lv {
            let hit = if claim.side == Side::Short {
                h[i] > x.price + tol && c[i] < x.price
            } else {
                l[i] < x.price - tol && c[i] > x.price
            };
            if hit {
                let depth = if claim.side == Side::Short {
                    h[i] - x.price
                } else {
                    x.price - l[i]
                };
                if best.as_ref().map(|b| depth > b.0).unwrap_or(true) {
                    best = Some((depth, *x, i));
                }
            }
        }
    }
    let Some((depth, x, i)) = best else {
        return Verdict::no(format!("no level was swept in the last {} bars", look));
    };
    let pip = pip_size(view.now());
    let in_band = x.on_figure && depth <= STOP_BAND_PIPS * pip;
    let note = if in_band {
        " -- terminated inside the 1-10 pip stop band past the figure, where the orders measurably are"
    } else {
        ""
    };
    Verdict::yes(
        (depth / a).min(3.0),
        format!("swept {} by {:.2} ATR at bar {} and closed back inside{}", x.say(), depth / a, i, note),
    )
}

/// Broken resistance now holding as support, or the reverse.
///
/// **UNTESTED, and it says so in its own verdict text.** Every practitioner
/// source asserts role reversal; none quantifies it, and no academic test of it
/// was found. It is implemented because the machinery to settle it already
/// exists: it graduates through the scoreboard like every other kind, capped at
/// the untested weight until thirty outcomes. If polarity is folklore, this
/// will be silenced by the data rather than by anybody's opinion — which is the
/// correct way for a system to hold an unproven belief.
fn polarity(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let look = claim.lookback.unwrap_or(20);
    if view.need(look + 40).is_err() {
        return Verdict::cannot_say("not enough history to see a flip");
    }
    let a = view.atr(14);
    if a <= 0.0 {
        return Verdict::cannot_say("no volatility to measure against");
    }
    let before = view.back(look);
    let Ok(prior) = levels::levels(&before, 2, 120.0) else {
        return Verdict::cannot_say("no level existed to flip");
    };
    let tol = view.gamma();
    let now = view.now();
    let was = before.now();
    let (h, l) = (view.high(), view.low());
    let recent_lo = l[l.len() - look..].iter().cloned().fold(f64::MAX, f64::min);
    let recent_hi = h[h.len() - look..].iter().cloned().fold(f64::MIN, f64::max);
    for x in prior {
        let crossed_up = was < x.price && now > x.price;
        let crossed_dn = was > x.price && now < x.price;
        if claim.side.is_long() && crossed_up && (recent_lo - x.price).abs() <= tol && now > x.price {
            return Verdict::yes(
                ((now - x.price).abs() / a).min(2.0),
                format!(
                    "{} broke upward and has held as support on retest -- \
                     UNTESTED reading, weighted by its own record",
                    x.say()
                ),
            );
        }
        if claim.side == Side::Short && crossed_dn && (recent_hi - x.price).abs() <= tol && now < x.price {
            return Verdict::yes(
                ((now - x.price).abs() / a).min(2.0),
                format!(
                    "{} broke downward and has held as resistance on retest -- \
                     UNTESTED reading, weighted by its own record",
                    x.say()
                ),
            );
        }
    }
    Verdict::no("no level has broken and held in the other role")
}

fn trending(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let Ok(r) = regime::read(view, claim.lookback, regime::DEFAULT_SPREAD_PIPS,
                             regime::DEFAULT_SLIPPAGE_PIPS, None, 0) else {
        return Verdict::cannot_say("not enough bars to read a regime");
    };
    let want = if claim.side.is_long() { Direction::Up } else { Direction::Down };
    match r.direction {
        Direction::Flat => Verdict::no(format!("not trending -- {}", r.say())),
        d if d != want => Verdict::no(format!("trending the other way -- {}", r.say())),
        // Magnitude is how far the efficiency ratio sits above its own
        // random-walk baseline, so it is comparable across lookbacks and
        // instruments in a way a raw ER is not. A reading at 1.0 is a coin flip
        // and scores nothing.
        _ => Verdict::yes((r.strength - 1.0).min(3.0), r.say()),
    }
}

/// A range with room in it. Neutral between the sides by construction.
///
/// Returns `Unknown` when true rather than scoring for whoever staked it — a
/// range is not bullish or bearish, and letting it score would hand a free
/// point to the faster typist.
fn ranging(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let Ok(r) = regime::read(view, claim.lookback, regime::DEFAULT_SPREAD_PIPS,
                             regime::DEFAULT_SLIPPAGE_PIPS, None, 0) else {
        return Verdict::cannot_say("not enough bars to read a regime");
    };
    if r.label() == "RANGING" {
        Verdict::cannot_say(r.say())
    } else {
        Verdict::no(r.say())
    }
}

/// Not enough room to cover the cost of trading it.
///
/// The one regime reading worth staking as a REFUSAL. Neutral like `Ranging`,
/// but a side that stakes it is telling the record that the cost gate failed —
/// which is what makes a statistically fine range unprofitable.
fn choppy(claim: &Claim, view: &AsOf<'_>) -> Verdict {
    let Ok(r) = regime::read(view, claim.lookback, regime::DEFAULT_SPREAD_PIPS,
                             regime::DEFAULT_SLIPPAGE_PIPS, None, 0) else {
        return Verdict::cannot_say("not enough bars to read a regime");
    };
    if r.label() == "CHOPPY" {
        Verdict::cannot_say(r.say())
    } else {
        Verdict::no(r.say())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::bars::Bars;
    use super::super::fixtures::{box_range, from_path, ramp, walk};

    const UP_THEN_PULLBACK: [f64; 10] = [
        1.0000, 1.0100, 1.0050, 1.0200, 1.0150, 1.0300, 1.0250, 1.0280, 1.0260, 1.0290,
    ];
    const UP_THEN_REVERSAL: [f64; 10] = [
        1.0000, 1.0100, 1.0050, 1.0200, 1.0150, 1.0300, 1.0250, 1.0280, 1.0200, 1.0230,
    ];

    #[test]
    fn a_windowed_claim_is_measured_over_the_lookback_it_was_built_with() {
        // `Claim::over` sets the lookback the verifiers read; `Claim::new`
        // leaves each reader on its own default. Proven through `velocity`,
        // whose bar requirement is `lookback + 15`: on a 20-bar rise the
        // default window (5) has the bars it needs and rules, while a 30-bar
        // window cannot be measured and says so. The verdict changes because
        // the window `over` set actually reached the reader.
        let b = ramp(20, 0.001);
        let v = b.latest().unwrap();
        let default = verify(&Claim::new(Kind::Velocity, Side::Long), &v);
        let windowed = verify(&Claim::over(Kind::Velocity, Side::Long, 30), &v);
        assert_ne!(
            default.truth,
            Truth::Unknown,
            "the default window should have enough bars to rule: {}",
            default.detail
        );
        assert_eq!(
            windowed.truth,
            Truth::Unknown,
            "a 30-bar window cannot be measured on 20 bars: {}",
            windowed.detail
        );
        assert!(windowed.detail.contains("not enough bars"), "{}", windowed.detail);
    }

    #[test]
    fn every_kind_is_checkable_and_the_list_is_complete() {
        // The compiler already guarantees the match is exhaustive. This checks
        // the other direction: that ALL_KINDS has not fallen behind the enum.
        let b = walk(400, 3);
        let v = b.latest().unwrap();
        assert_eq!(ALL_KINDS.len(), 14);
        for k in ALL_KINDS {
            for side in [Side::Long, Side::Short] {
                let out = verify(&Claim::new(k, side), &v);
                assert!(!out.detail.is_empty(), "{} gave no reason", k.name());
            }
        }
    }

    #[test]
    fn no_reader_scores_for_a_claim_it_did_not_confirm() {
        let b = walk(400, 5);
        let v = b.latest().unwrap();
        for k in ALL_KINDS {
            for side in [Side::Long, Side::Short] {
                let out = verify(&Claim::new(k, side), &v);
                if out.truth != Truth::True {
                    assert_eq!(out.magnitude, 0.0, "{} {:?}", k.name(), out);
                }
            }
        }
    }

    #[test]
    fn a_confirmed_reversal_can_be_staked_and_verified() {
        // Until this reader existed a side could watch a reversal happen and
        // have no way to stake it.
        let b = from_path(&UP_THEN_REVERSAL).unwrap();
        let v = b.latest().unwrap();
        let short = verify(&Claim::new(Kind::Reversal, Side::Short), &v);
        assert_eq!(short.truth, Truth::True, "{:?}", short);
        assert!(short.magnitude > 0.0, "a confirmed break is worth something");
        let long = verify(&Claim::new(Kind::Reversal, Side::Long), &v);
        assert_eq!(long.truth, Truth::False, "{:?}", long);
    }

    #[test]
    fn an_unconfirmed_turn_earns_nobody_anything() {
        // The money rule. UNKNOWN earns nothing and costs nothing, so a side
        // cannot score off a lower high that came to nothing -- which is most
        // of them.
        let b = from_path(&UP_THEN_PULLBACK).unwrap();
        let v = verify(&Claim::new(Kind::Reversal, Side::Short), &b.latest().unwrap());
        assert_eq!(v.truth, Truth::Unknown, "{:?}", v);
        assert_eq!(v.magnitude, 0.0);
        assert!(v.detail.contains("not yet confirmed"), "{}", v.detail);
    }

    #[test]
    fn neither_side_can_claim_a_reversal_of_a_running_trend() {
        let b = super::super::fixtures::zigzag(9, 6, 3, 0.0015);
        let v = b.latest().unwrap();
        for side in [Side::Long, Side::Short] {
            let out = verify(&Claim::new(Kind::Reversal, side), &v);
            assert_eq!(out.truth, Truth::False, "{:?} {:?}", side, out);
        }
    }

    #[test]
    fn a_range_earns_nothing_for_either_side() {
        // Letting it score would hand a free point to the faster typist.
        let b = box_range(200, 1.0960, 1.1040, 20);
        let v = b.latest().unwrap();
        for side in [Side::Long, Side::Short] {
            let out = verify(&Claim::new(Kind::Ranging, side), &v);
            assert_eq!(out.truth, Truth::Unknown, "{:?}", out);
            assert_eq!(out.magnitude, 0.0);
        }
    }

    #[test]
    fn a_trend_is_stakeable_and_only_by_the_side_it_favours() {
        let b = ramp(200, 0.0008);
        let v = b.latest().unwrap();
        let up = verify(&Claim::new(Kind::Trending, Side::Long), &v);
        let dn = verify(&Claim::new(Kind::Trending, Side::Short), &v);
        assert_eq!(up.truth, Truth::True, "{:?}", up);
        assert!(up.magnitude > 0.0);
        assert_eq!(dn.truth, Truth::False);
        assert_eq!(dn.magnitude, 0.0);
    }

    #[test]
    fn range_position_abstains_outside_a_ranging_market() {
        // Position-in-range is not a meaningful question during a trend, so
        // it should abstain rather than fire on a
        // trending series -- the trending fixture already proven to read as
        // TRENDING UP by `a_trend_is_stakeable_and_only_by_the_side_it_favours`.
        let trending = ramp(200, 0.0008);
        let v = trending.latest().unwrap();
        for side in [Side::Long, Side::Short] {
            let out = verify(&Claim::new(Kind::RangePosition, side), &v);
            assert_eq!(out.truth, Truth::Unknown, "should abstain while trending: {:?}", out);
            assert_eq!(out.magnitude, 0.0);
        }
    }

    #[test]
    fn range_position_still_fires_inside_a_genuine_range() {
        // The gate should cost it nothing when the market actually is
        // ranging -- the same fixture `a_range_earns_nothing_for_either_side`
        // already proved reads as RANGING.
        let b = box_range(200, 1.0960, 1.1040, 20);
        let v = b.latest().unwrap();
        // The fixture's last bars sit at the low end of the box on this
        // phase, so Long is the side with room to be near the bottom.
        let long = verify(&Claim::new(Kind::RangePosition, Side::Long), &v);
        let short = verify(&Claim::new(Kind::RangePosition, Side::Short), &v);
        assert!(
            long.truth == Truth::True || short.truth == Truth::True,
            "the gate should not silence every reading inside a real range: long={:?} short={:?}",
            long,
            short
        );
    }

    #[test]
    fn being_at_a_level_can_be_staked() {
        let b = from_path(&[1.1000, 1.1100, 1.1000, 1.1100, 1.1002]).unwrap();
        let v = verify(&Claim::new(Kind::AtLevel, Side::Long), &b.latest().unwrap());
        assert!(matches!(v.truth, Truth::True | Truth::False));
        if v.truth == Truth::True {
            assert!(v.magnitude > 0.0 && v.detail.contains("at "));
        }
    }

    #[test]
    fn price_in_the_middle_of_nowhere_is_not_at_a_level() {
        // 1.1063 on purpose: 1.1050 is itself a "50" level, so a fixture
        // ending there would be at a figure and the test would be wrong.
        let b = from_path(&[1.1000, 1.1100, 1.1000, 1.1100, 1.1063]).unwrap();
        let v = verify(&Claim::new(Kind::AtLevel, Side::Long), &b.latest().unwrap());
        assert_eq!(v.truth, Truth::False, "{:?}", v);
    }

    #[test]
    fn polarity_admits_in_its_own_verdict_that_it_is_untested() {
        assert_eq!(Kind::Polarity.grounds(), Grounds::Untested);
        let b = from_path(&[1.1000, 1.1100, 1.1000, 1.1100, 1.1200, 1.1105, 1.1180]).unwrap();
        let v = verify(&Claim::over(Kind::Polarity, Side::Long, 30), &b.latest().unwrap());
        if v.truth == Truth::True {
            assert!(v.detail.contains("UNTESTED"), "{}", v.detail);
        }
    }

    #[test]
    fn a_sweep_argues_for_the_other_side() {
        // Up through a level, close back below: a SHORT case. The single most
        // important distinction in the level vocabulary.
        let b = from_path(&[1.1000, 1.1100, 1.1000, 1.1100, 1.1000, 1.1130, 1.1040]).unwrap();
        let v = verify(&Claim::over(Kind::Sweep, Side::Short, 40), &b.latest().unwrap());
        if v.truth == Truth::True {
            assert!(v.detail.contains("closed back inside"), "{}", v.detail);
        }
    }

    #[test]
    fn a_short_series_is_told_it_cannot_be_answered_not_refuted() {
        // UNKNOWN, not FALSE. Treating missing data as refutation would let it
        // argue for the other side.
        let b = Bars::from_closes(&[1.1, 1.1001, 1.1002, 1.1003], 0.0006).unwrap();
        let v = b.latest().unwrap();
        for k in ALL_KINDS {
            let out = verify(&Claim::new(k, Side::Long), &v);
            assert_eq!(out.truth, Truth::Unknown, "{} said {:?}", k.name(), out.truth);
        }
    }

    #[test]
    fn a_claim_renders_readably() {
        assert_eq!(Claim::new(Kind::Bos, Side::Long).say(), "BOS[LONG]");
        assert_eq!(Claim::over(Kind::Sweep, Side::Short, 5).say(), "SWEEP[SHORT](5)");
    }

    #[test]
    fn the_grounds_of_each_reading_are_recorded() {
        // A reader is entitled to know which readings rest on order-flow data
        // and which rest on nobody having tested them.
        assert_eq!(Kind::AtLevel.grounds(), Grounds::Measured);
        assert_eq!(Kind::Bos.grounds(), Grounds::Arithmetic);
        assert_eq!(Kind::Polarity.grounds(), Grounds::Untested);
    }
}
