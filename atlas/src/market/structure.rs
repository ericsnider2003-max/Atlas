//! Where structure is, and whether it has turned.
//!
//! ## The gap this closes
//!
//! The first version of this reading answered two questions: are the highs
//! rising, are the lows rising. Both returned "unknown" the moment a sequence
//! stopped being monotonic — **which is precisely the moment a trend turns**.
//! So a reversal and a patch of meaningless noise produced the identical
//! reading, and the one thing most worth being told was the one thing the
//! structure could not say.
//!
//! It was worse than a reporting gap. There was no way to ARGUE it either: the
//! trend reading asks "is structure trending my way", a question about the last
//! pair of swings, so a market that has fallen for a week and just put in one
//! higher low gets FALSE — correctly, because it is not trending up yet. A side
//! could watch a reversal happen and have nothing to stake.
//!
//! ## The rule
//!
//! > **A lower high warns. A lower low confirms.**
//!
//! Price making a lower high means buyers failed at a level. That is a warning
//! and nothing more — the trend is intact until the last higher low gives way.
//! When it does, the sequence of higher highs and higher lows is broken in both
//! halves at once and the turn is real. Mirrored for a downtrend turning up.
//!
//! **A lower high on its own is not a reversal.** Every range on every chart is
//! a sequence of lower highs that came to nothing, and a detector that calls the
//! first one a turn is wrong early, repeatedly, against a trend that is still
//! running — which is the most expensive way to be wrong there is.

use super::bars::AsOf;

/// How one pivot compares to the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Label {
    /// The first in the sequence: nothing to compare against.
    First,
    Higher,
    Lower,
    /// Exactly equal.
    ///
    /// Not folded into `Lower`. A double top is a real and common thing, and
    /// calling it a lower high because the comparison had to return something
    /// is a fabricated reading.
    Equal,
}

impl Label {
    fn say_high(self) -> &'static str {
        match self {
            Label::First => "",
            Label::Higher => "HH",
            Label::Lower => "LH",
            Label::Equal => "EQ",
        }
    }
    fn say_low(self) -> &'static str {
        match self {
            Label::First => "",
            Label::Higher => "HL",
            Label::Lower => "LL",
            Label::Equal => "EQ",
        }
    }
}

/// What the structure is doing, from the LAST pair of pivots.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trend {
    Up,
    Down,
    /// Highs and lows disagree.
    Range,
    /// Not enough pivots to say. Never reported as `Range` — "unclear" and
    /// "sideways" are different facts.
    Unknown,
}

impl Trend {
    pub fn say(self) -> &'static str {
        match self {
            Trend::Up => "UP",
            Trend::Down => "DOWN",
            Trend::Range => "RANGE",
            Trend::Unknown => "UNKNOWN",
        }
    }
}

/// A change of direction, and whether it is finished.
///
/// Two states, recorded separately. `confirmed` is true only when the swing
/// that was supposed to hold has actually broken; before that it is a warning
/// and it says so.
#[derive(Debug, Clone, PartialEq)]
pub struct Turn {
    pub direction: Trend,
    pub confirmed: bool,
    /// The bar of the swing that broke, when one did.
    pub at: Option<usize>,
    pub why: String,
}

impl Turn {
    pub fn say(&self) -> String {
        let head = if self.confirmed { "turned" } else { "MAY be turning" };
        let where_ = match self.at {
            Some(i) => format!(" at bar {}", i),
            None => String::new(),
        };
        format!("{} {}{} -- {}", head, self.direction.say(), where_, self.why)
    }
}

/// One pivot: where it happened, at what price, and when it became knowable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pivot {
    pub at: usize,
    pub price: f64,
    /// `at + k`. A swing needs k bars either side, so it cannot be confirmed
    /// until k bars later — and acting on it at `at` is the commonest way a
    /// structure backtest lies to itself. Not that the data is wrong: it had
    /// not happened yet.
    pub confirmed_at: usize,
}

impl Pivot {
    pub fn known_by(&self, bar: usize) -> bool {
        bar >= self.confirmed_at
    }
}

/// The last few swing highs and lows, and what they say.
///
/// There is deliberately no composite `strength` score. Osler (2000) had six
/// professional desks publish strength ratings for their own levels and found
/// those ratings had **no meaningful correlation** with how often price
/// actually bounced. A score here would be rebuilding something already
/// measured and found worthless.
#[derive(Debug, Clone, PartialEq)]
pub struct Structure {
    pub highs: Vec<Pivot>,
    pub lows: Vec<Pivot>,
}

fn label(prices: &[f64]) -> Vec<Label> {
    let mut out = Vec::with_capacity(prices.len());
    for (i, &p) in prices.iter().enumerate() {
        out.push(if i == 0 {
            Label::First
        } else if p > prices[i - 1] {
            Label::Higher
        } else if p < prices[i - 1] {
            Label::Lower
        } else {
            Label::Equal
        });
    }
    out
}

/// All rising, all falling, or neither.
///
/// Returns `None` for neither AND for too-few-to-say, because both are honestly
/// "the sequence does not answer this". The labels and [`Structure::reversal`]
/// are what tell a turn from noise.
fn stepping(prices: &[f64], up: bool) -> Option<bool> {
    if prices.len() < 2 {
        return None;
    }
    let all_up = prices.windows(2).all(|w| w[1] > w[0]);
    let all_down = prices.windows(2).all(|w| w[1] < w[0]);
    if up {
        if all_up {
            Some(true)
        } else if all_down {
            Some(false)
        } else {
            None
        }
    } else if all_down {
        Some(true)
    } else if all_up {
        Some(false)
    } else {
        None
    }
}

impl Structure {
    fn high_prices(&self) -> Vec<f64> {
        self.highs.iter().map(|p| p.price).collect()
    }
    pub fn low_prices(&self) -> Vec<f64> {
        self.lows.iter().map(|p| p.price).collect()
    }

    pub fn high_labels(&self) -> Vec<Label> {
        label(&self.high_prices())
    }
    pub fn low_labels(&self) -> Vec<Label> {
        label(&self.low_prices())
    }

    /// Every high above the one before it. `None` when neither, or too few.
    pub fn higher_highs(&self) -> Option<bool> {
        stepping(&self.high_prices(), true)
    }
    pub fn higher_lows(&self) -> Option<bool> {
        stepping(&self.low_prices(), true)
    }

    /// Every high BELOW the one before it.
    ///
    /// **Not `!higher_highs`.** A sequence that rose, turned and fell is
    /// neither, and reading "not rising" as "falling" is how a system talks
    /// itself into a short in the middle of an uptrend's pullback. Both can be
    /// `None` at once, and that is a real answer.
    pub fn lower_highs(&self) -> Option<bool> {
        stepping(&self.high_prices(), false)
    }
    pub fn lower_lows(&self) -> Option<bool> {
        stepping(&self.low_prices(), false)
    }

    fn last_high(&self) -> Option<Pivot> {
        self.highs.last().copied()
    }
    fn last_low(&self) -> Option<Pivot> {
        self.lows.last().copied()
    }

    /// From the LAST pair of each, deliberately.
    ///
    /// A trend that put in one deep pullback five swings ago is still a trend,
    /// and a definition needing all five to step the same way calls almost
    /// every real chart unclear — which is the behaviour that hid reversals in
    /// the first place.
    pub fn trend(&self) -> Trend {
        let (hl, ll) = (self.high_labels(), self.low_labels());
        let h = hl.last().copied().unwrap_or(Label::First);
        let l = ll.last().copied().unwrap_or(Label::First);
        match (h, l) {
            (Label::First, _) | (_, Label::First) => Trend::Unknown,
            (Label::Higher, Label::Higher) => Trend::Up,
            (Label::Lower, Label::Lower) => Trend::Down,
            _ => Trend::Range,
        }
    }

    /// What the structure was doing BEFORE the most recent pair of pivots.
    ///
    /// A turn is defined against something, and that something has to exclude
    /// the pivots being judged — otherwise the evidence for the turn is also
    /// the evidence for the trend it supposedly turned, and the test is
    /// circular.
    pub fn prior_trend(&self) -> Trend {
        let (hl, ll) = (self.high_labels(), self.low_labels());
        if hl.len() < 2 || ll.len() < 2 {
            return Trend::Unknown;
        }
        let hs = &hl[..hl.len() - 1];
        let ls = &ll[..ll.len() - 1];
        for (h, l) in hs.iter().rev().zip(ls.iter().rev()) {
            let up = *h == Label::Higher || *l == Label::Higher;
            let down = *h == Label::Lower || *l == Label::Lower;
            if up && !down {
                return Trend::Up;
            }
            if down && !up {
                return Trend::Down;
            }
        }
        Trend::Unknown
    }

    /// Whether the structure has turned, and whether it is finished.
    ///
    /// `None` when nothing has changed direction — a clean trend and a formless
    /// chart both return `None`, and [`Structure::trend`] tells them apart.
    pub fn reversal(&self) -> Option<Turn> {
        let (hl, ll) = (self.high_labels(), self.low_labels());
        let (h, l) = (hl.last().copied()?, ll.last().copied()?);
        let was = self.prior_trend();
        if was != Trend::Up && was != Trend::Down {
            // Nothing to turn from. Without this the first two pivots of a
            // fresh series read as a reversal of a trend that never existed.
            return None;
        }
        let hp = self.last_high()?.price;
        let lp = self.last_low()?.price;

        // Both halves broken: the sequence has given way in highs AND lows at
        // once. This is the turn.
        if was == Trend::Up && h == Label::Lower && l == Label::Lower {
            return Some(Turn {
                direction: Trend::Down,
                confirmed: true,
                at: Some(self.lows.last()?.at),
                why: format!(
                    "lower high at {:.5} and the prior low broken at {:.5}",
                    hp, lp
                ),
            });
        }
        if was == Trend::Down && h == Label::Higher && l == Label::Higher {
            return Some(Turn {
                direction: Trend::Up,
                confirmed: true,
                at: Some(self.highs.last()?.at),
                why: format!(
                    "higher low at {:.5} and the prior high taken at {:.5}",
                    lp, hp
                ),
            });
        }

        // One half broken. A warning, explicitly not a turn. Which half broke
        // changes what it means, so each gets its own words rather than sharing
        // one vague sentence.
        let (dir, why) = match (was, h, l) {
            (Trend::Up, Label::Lower, _) => (
                Trend::Down,
                format!(
                    "lower high at {:.5}, but the low at {:.5} is still higher \
                     -- a pullback until that gives way",
                    hp, lp
                ),
            ),
            (Trend::Up, _, Label::Lower) => (
                Trend::Down,
                format!(
                    "the last higher low broke at {:.5}, but the high at {:.5} \
                     still held -- a widening range, not yet a turn",
                    lp, hp
                ),
            ),
            (Trend::Down, Label::Higher, _) => (
                Trend::Up,
                format!(
                    "higher high at {:.5}, but the low at {:.5} is still lower \
                     -- a bounce until that high is held",
                    hp, lp
                ),
            ),
            (Trend::Down, _, Label::Higher) => (
                Trend::Up,
                format!(
                    "the last lower low failed at {:.5}, but the high at {:.5} \
                     is still lower -- a base forming, not yet a turn",
                    lp, hp
                ),
            ),
            _ => return None,
        };
        Some(Turn { direction: dir, confirmed: false, at: None, why })
    }

    /// One line, for the record and for an advocate to read.
    pub fn say(&self) -> String {
        if self.highs.is_empty() && self.lows.is_empty() {
            return "no structure has formed yet".into();
        }
        let run = |pts: &[Pivot], labels: &[Label], high: bool| -> String {
            if pts.is_empty() {
                return format!("no {}", if high { "highs" } else { "lows" });
            }
            let body: Vec<String> = pts
                .iter()
                .zip(labels)
                .map(|(p, lab)| {
                    let tag = if high { lab.say_high() } else { lab.say_low() };
                    if tag.is_empty() {
                        format!("{:.5}", p.price)
                    } else {
                        format!("{}:{:.5}", tag, p.price)
                    }
                })
                .collect();
            format!(
                "{} {}: {}",
                pts.len(),
                if high { "highs" } else { "lows" },
                body.join(" ")
            )
        };
        let mut parts = vec![
            format!("trend {}", self.trend().say()),
            run(&self.highs, &self.high_labels(), true),
            run(&self.lows, &self.low_labels(), false),
        ];
        // Whether the WHOLE run steps one way, or only the last pair does.
        // `trend` reads the last pair by design (see its note); these read
        // every swing. A trend that rests on one pair after a deep pullback
        // and a clean run that never pulled back are different facts, and the
        // one worth acting on is the clean one -- so the reading says which it
        // is rather than leaving the last-pair label to stand for both.
        let run_quality = match self.trend() {
            Trend::Up => Some(
                if self.higher_highs() == Some(true) && self.higher_lows() == Some(true) {
                    "clean uptrend -- every high and every low stepping up"
                } else {
                    "trend rests on the last pair -- earlier swings do not all step up"
                },
            ),
            Trend::Down => Some(
                if self.lower_highs() == Some(true) && self.lower_lows() == Some(true) {
                    "clean downtrend -- every high and every low stepping down"
                } else {
                    "trend rests on the last pair -- earlier swings do not all step down"
                },
            ),
            Trend::Range | Trend::Unknown => None,
        };
        if let Some(q) = run_quality {
            parts.push(q.to_string());
        }
        if let Some(t) = self.reversal() {
            parts.push(t.say());
        }
        parts.join("; ")
    }
}

/// The last `n` swing highs and the last `n` swing lows visible in this view.
///
/// Free after the first call: the view has already worked the pivots out and
/// cached them, keyed by its own bound.
pub fn recent(view: &AsOf<'_>, n: usize, k: usize) -> Structure {
    let (hi, lo) = view.swings(k);
    let (h, l) = (view.high(), view.low());
    let take = |ix: &[usize], src: &[f64]| -> Vec<Pivot> {
        let start = ix.len().saturating_sub(n);
        ix[start..]
            .iter()
            .map(|&i| Pivot { at: i, price: src[i], confirmed_at: i + k })
            .collect()
    };
    Structure { highs: take(&hi, h), lows: take(&lo, l) }
}

/// Every pivot, optionally filtered to what was KNOWABLE at a bar.
///
/// Without the filter you get every pivot, including ones the market has not
/// finished proving. That difference is the whole of the confirmation-delay
/// problem, and it is why `confirmed_at` exists on [`Pivot`].
pub fn confirmed_swings(view: &AsOf<'_>, k: usize, as_at: Option<usize>) -> Vec<Pivot> {
    let (hi, lo) = view.swings(k);
    let (h, l) = (view.high(), view.low());
    let mut out: Vec<Pivot> = hi
        .iter()
        .map(|&i| Pivot { at: i, price: h[i], confirmed_at: i + k })
        .chain(lo.iter().map(|&i| Pivot { at: i, price: l[i], confirmed_at: i + k }))
        .collect();
    out.sort_by_key(|p| p.at);
    if let Some(bar) = as_at {
        out.retain(|p| p.known_by(bar));
    }
    out
}

/// Pivots that exist in the data but were NOT yet knowable at `at`.
///
/// Exists so the size of the problem can be looked at rather than assumed: on
/// any frame this returns the last pivot or two, every time. There is always
/// something in the data that had not happened yet from the point of view of
/// the bar being judged.
pub fn unknowable(view: &AsOf<'_>, at: usize, k: usize) -> Vec<Pivot> {
    confirmed_swings(view, k, None)
        .into_iter()
        .filter(|p| !p.known_by(at))
        .collect()
}

/// What the structure looked like just BEFORE the break it now reports.
///
/// `AsOf::back_to`'s own note names this as the shape every honest use has: "a
/// break reader asking what structure looked like before the move it is
/// judging". The earlier view is bounded at the bar before the swing that
/// broke, so only pivots confirmed by then are in it — the reading is what
/// could actually have been known, not a hindsight redraw.
///
/// Descriptive only. It shows the before and leaves the after to the reader;
/// it decides nothing. `None` when nothing has turned, or when the turn has no
/// bar yet (an unconfirmed warning).
pub fn before_the_turn(view: &AsOf<'_>, n: usize, k: usize) -> Option<String> {
    let now = recent(view, n, k);
    let turn = now.reversal()?;
    let at = turn.at?;
    let earlier = view.back_to(at.checked_sub(1)?).ok()?;
    let then = recent(&earlier, n, k);
    Some(format!(
        "before the break at bar {at}, as it could be read then: {}",
        then.say()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::bars::Bars;
    use super::super::fixtures::path;

    const UP_THEN_PULLBACK: [f64; 10] = [
        1.0000, 1.0100, 1.0050, 1.0200, 1.0150, 1.0300, 1.0250, 1.0280, 1.0260, 1.0290,
    ];
    /// Identical, except the last dip goes THROUGH the prior low.
    const UP_THEN_REVERSAL: [f64; 10] = [
        1.0000, 1.0100, 1.0050, 1.0200, 1.0150, 1.0300, 1.0250, 1.0280, 1.0200, 1.0230,
    ];
    const DOWN_THEN_REVERSAL: [f64; 10] = [
        1.0300, 1.0200, 1.0250, 1.0100, 1.0150, 1.0000, 1.0050, 1.0020, 1.0100, 1.0070,
    ];

    fn fixture(points: &[f64]) -> Bars {
        Bars::from_closes(&path(points, 0.0002), 0.0006).unwrap()
    }

    fn structure_of(points: &[f64]) -> Structure {
        let b = fixture(points);
        let v = b.latest().unwrap();
        recent(&v, 5, 2)
    }

    fn clean_uptrend() -> Bars {
        let mut c = vec![1.0];
        let mut p = 1.0;
        for _ in 0..9 {
            for _ in 0..6 {
                p += 0.0015;
                c.push(p);
            }
            for _ in 0..3 {
                p -= 0.0008;
                c.push(p);
            }
        }
        Bars::from_closes(&c, 0.0006).unwrap()
    }

    #[test]
    fn a_falling_structure_reports_lower_highs_and_lower_lows() {
        let s = structure_of(&[1.03, 1.02, 1.025, 1.01, 1.015, 1.00, 1.005]);
        assert_eq!(s.lower_highs(), Some(true), "{}", s.say());
        assert_eq!(s.lower_lows(), Some(true));
        assert_eq!(s.higher_highs(), Some(false));
    }

    #[test]
    fn not_rising_is_not_the_same_as_falling() {
        // The reason `lower_highs` is its own reading and not `!higher_highs`:
        // a structure that rose, turned and fell is neither, and folding them
        // together argues for a short inside an uptrend.
        let s = structure_of(&UP_THEN_REVERSAL);
        assert_eq!(s.higher_highs(), None, "mixed, so unknown");
        assert_eq!(s.lower_highs(), None, "and equally not falling");
    }

    #[test]
    fn every_pivot_is_labelled() {
        let s = structure_of(&UP_THEN_REVERSAL);
        let hl = s.high_labels();
        assert_eq!(hl[0], Label::First, "the first has nothing to compare to");
        assert_eq!(&hl[1..], &[Label::Higher, Label::Higher, Label::Lower]);
        assert_eq!(hl.len(), s.highs.len());
        let ll = s.low_labels();
        assert_eq!(&ll[1..], &[Label::Higher, Label::Higher, Label::Lower]);
    }

    #[test]
    fn an_equal_high_is_not_a_lower_one() {
        // A double top is real and common. Forcing it into one of the two
        // because the comparison had to return something is fabricated.
        assert_eq!(label(&[1.0, 1.0]), vec![Label::First, Label::Equal]);
    }

    #[test]
    fn a_clear_reversal_is_identified_as_a_reversal() {
        // The gap this closes. Both monotonic readings go None the moment a
        // sequence stops being monotonic -- exactly when a trend turns -- so a
        // reversal and noise gave the identical reading.
        let s = structure_of(&UP_THEN_REVERSAL);
        assert_eq!(s.higher_highs(), None);
        assert_eq!(s.lower_highs(), None);
        let t = s.reversal().expect("the structure must still identify it");
        assert_eq!(t.direction, Trend::Down);
        assert!(t.confirmed);
        assert!(t.at.is_some());
        assert_eq!(s.trend(), Trend::Down, "{}", s.say());
    }

    #[test]
    fn a_downtrend_turning_up_is_identified_too() {
        let s = structure_of(&DOWN_THEN_REVERSAL);
        let t = s.reversal().expect(&s.say());
        assert_eq!(t.direction, Trend::Up);
        assert!(t.confirmed);
        assert_eq!(s.trend(), Trend::Up);
    }

    #[test]
    fn a_lower_high_alone_is_not_a_reversal() {
        // The expensive mistake this rule prevents. Every range is a sequence
        // of lower highs that came to nothing; calling the first one a turn is
        // wrong early, repeatedly, against a trend still running.
        let s = structure_of(&UP_THEN_PULLBACK);
        let t = s.reversal().expect("it must still be reported");
        assert_eq!(t.direction, Trend::Down);
        assert!(!t.confirmed, "but as a warning, not a turn");
        assert!(t.at.is_none(), "nothing broke, so no bar where it did");
        assert!(t.why.contains("pullback"), "{}", t.why);
    }

    #[test]
    fn the_only_difference_is_whether_the_low_broke() {
        // The two fixtures are the same path except the depth of the last dip.
        // If anything else drove the verdict this would pass by accident and
        // keep passing when it broke.
        let held = structure_of(&UP_THEN_PULLBACK);
        let broke = structure_of(&UP_THEN_REVERSAL);
        assert_eq!(held.high_labels(), broke.high_labels(), "highs are identical");
        let (hl, bl) = (held.low_labels(), broke.low_labels());
        assert_eq!(hl[..hl.len() - 1], bl[..bl.len() - 1]);
        assert_eq!(*hl.last().unwrap(), Label::Higher);
        assert_eq!(*bl.last().unwrap(), Label::Lower);
        assert!(!held.reversal().unwrap().confirmed);
        assert!(broke.reversal().unwrap().confirmed);
    }

    #[test]
    fn a_trend_that_never_turned_reports_no_reversal() {
        let b = clean_uptrend();
        let s = recent(&b.latest().unwrap(), 5, 2);
        assert_eq!(s.trend(), Trend::Up, "{}", s.say());
        assert!(s.reversal().is_none(), "{}", s.say());
    }

    #[test]
    fn a_turn_needs_something_to_turn_from() {
        // Two pivots into a fresh series there is no prior trend, and calling
        // that a reversal invents the thing it claims to have reversed.
        let s = structure_of(&[1.0000, 1.0100, 1.0050, 1.0080]);
        assert!(s.prior_trend() == Trend::Unknown || s.reversal().is_none(), "{}", s.say());
    }

    #[test]
    fn the_summary_names_the_turn_and_the_labels() {
        let line = structure_of(&UP_THEN_REVERSAL).say();
        assert!(line.contains("trend DOWN"), "{}", line);
        assert!(line.contains("turned DOWN"), "{}", line);
        assert!(line.contains("LH:") && line.contains("LL:"), "{}", line);
    }

    #[test]
    fn the_summary_reads_a_clean_run_off_the_stepping_predicates() {
        // `say` now consumes `lower_highs`/`lower_lows` (and their rising
        // twins): a trend where every swing steps the same way reads as clean,
        // and one that only qualifies on the last pair says so. These are the
        // whole-run readings, distinct from `trend`'s deliberate last-pair one.

        // A clean downtrend: this exact series is proven all-lower elsewhere.
        let down = structure_of(&[1.03, 1.02, 1.025, 1.01, 1.015, 1.00, 1.005]);
        assert_eq!(down.lower_highs(), Some(true), "{}", down.say());
        assert_eq!(down.lower_lows(), Some(true), "{}", down.say());
        assert!(down.say().contains("clean downtrend"), "{}", down.say());

        // Its mirror is a clean uptrend, exercising the rising predicates.
        let up = structure_of(&[0.97, 0.98, 0.975, 0.99, 0.985, 1.00, 0.995]);
        assert_eq!(up.trend(), Trend::Up, "{}", up.say());
        assert_eq!(up.higher_highs(), Some(true), "{}", up.say());
        assert_eq!(up.higher_lows(), Some(true), "{}", up.say());
        assert!(up.say().contains("clean uptrend"), "{}", up.say());

        // A trend that only holds on the last pair is named as such rather than
        // borrowing the clean label: here the run turned, so no predicate is
        // all-true and the note points at the last pair.
        let mixed = structure_of(&UP_THEN_REVERSAL);
        assert_ne!(mixed.lower_highs(), Some(true), "{}", mixed.say());
        assert!(mixed.say().contains("rests on the last pair"), "{}", mixed.say());
    }

    #[test]
    fn an_empty_structure_says_so_rather_than_pretending() {
        let s = Structure { highs: vec![], lows: vec![] };
        assert_eq!(s.trend(), Trend::Unknown);
        assert!(s.reversal().is_none());
        assert!(s.say().contains("no structure"));
    }

    // ---- the confirmation delay ----------------------------------------

    #[test]
    fn a_swing_is_not_knowable_on_the_bar_it_happened() {
        let b = super::super::fixtures::walk(400, 5);
        let v = b.latest().unwrap();
        let ps = confirmed_swings(&v, 2, None);
        assert!(!ps.is_empty());
        assert!(ps.iter().all(|p| p.confirmed_at == p.at + 2));
        let p = ps.last().unwrap();
        assert!(!p.known_by(p.at), "it takes k more bars to prove nothing higher followed");
        assert!(p.known_by(p.confirmed_at));
    }

    #[test]
    fn the_delay_is_exactly_the_detectors_own_k() {
        let b = super::super::fixtures::walk(400, 9);
        let v = b.latest().unwrap();
        for k in [1usize, 2, 4] {
            assert!(confirmed_swings(&v, k, None).iter().all(|p| p.confirmed_at == p.at + k));
        }
    }

    #[test]
    fn a_pivot_is_unknowable_right_up_to_the_bar_that_confirms_it() {
        // Stated exactly rather than probabilistically. The first version of
        // this asked whether ANYTHING was unknowable one bar from the end,
        // which is only true when a pivot happens to sit there -- a test that
        // passes on most seeds and fails on some is not a test, it is a
        // coin toss with an assertion attached.
        let b = super::super::fixtures::walk(400, 13);
        let v = b.latest().unwrap();
        let last = *confirmed_swings(&v, 2, None).last().unwrap();
        assert!(
            unknowable(&v, last.confirmed_at - 1, 2).contains(&last),
            "{:?} was treated as known the bar before it could be",
            last
        );
        assert!(!unknowable(&v, last.confirmed_at, 2).contains(&last));
    }

    #[test]
    fn asking_what_was_known_returns_only_what_was_known() {
        let b = super::super::fixtures::walk(400, 17);
        let v = b.latest().unwrap();
        let all = confirmed_swings(&v, 2, None);
        let known = confirmed_swings(&v, 2, Some(200));
        assert!(known.len() < all.len());
        assert!(known.iter().all(|p| p.confirmed_at <= 200));
    }

    #[test]
    fn the_structure_on_the_record_carries_its_delay() {
        let b = super::super::fixtures::walk(300, 21);
        let s = recent(&b.latest().unwrap(), 5, 2);
        assert!(s.highs.iter().all(|p| p.confirmed_at == p.at + 2));
        assert!(s.lows.iter().all(|p| p.confirmed_at > p.at));
    }
}
