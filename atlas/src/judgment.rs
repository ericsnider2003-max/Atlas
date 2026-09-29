//! Graded judgments, and how sure Atlas is of one.
//!
//! # What this is
//!
//! Atlas makes small judgments in a dozen places and each one grew its own
//! way of doing it. `certainty` grades an answer on three levels with two
//! thresholds. `grade` checks audio and pictures against numbers. `tier`
//! and `levels` band things. `hollow` finds answers
//! that are not answers and returns them **in the order the questions were
//! asked**, then calls the first three "worst".
//!
//! They are all the same two shapes underneath:
//!
//! * **Level** — where something sits on an ordered scale whose bands are
//!   *described*, not just numbered. "Over budget" and "beyond help" are
//!   different bands and the difference is a sentence, not a number.
//! * **Holds** — how strongly the evidence says a condition is true, with
//!   the evidence against it counted rather than ignored.
//!
//! [`crate::whichone`] is the third — picking one of several named readings —
//! and it is where it is because it was built first, for the daemon's intent
//! arms. This module shares its [`crate::whichone::Clarity`] rather than
//! declaring a second word for the same idea: **two ways to say "I am not
//! sure" is how one of them goes unused and then wrong.**
//!
//! # The contract, which is the point
//!
//! Every judgment here returns *how sure* alongside *what*, and the caller
//! is expected to do something different when the answer is `Close`. That is
//! the rule `certainty` and `understood` already keep, made available to
//! everything else rather than reimplemented per module.
//!
//! A judgment that cannot be made comes back as [`Clarity::Nothing`] rather
//! than as a middle band. **Not knowing and being in the middle are different
//! facts**, and collapsing them is how `readings` returned all zeros and
//! looked like a healthy machine — the bug `hollow.rs` was written about.
//!
//! # No model
//!
//! Everything here is arithmetic over evidence the caller already has. It
//! runs on a machine with nothing downloaded, it is testable, and it is the
//! same answer every time. Where a judgment genuinely needs to read prose,
//! `brain.rs` is the model and `certainty`/`understood` are what grade what
//! it says; this is for the far larger number of judgments that never needed
//! one.
//!
//! # And it is wired
//!
//! A substrate is an excellent place for things to be built and never
//! reached — this tree has 372 functions that only tests call, and a general
//! mechanism with no caller would be the 373rd.
//! `tests/a_judgment_says_how_sure_it_is.rs` fails the build if anything
//! here has no production caller.

use crate::whichone::Clarity;
use serde::Deserialize;

/// A piece of evidence that has already been measured.
///
/// Named, because a total with no account of what went into it cannot be
/// argued with — and the whole reason to grade something rather than
/// threshold it is so a person can see why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Signal {
    pub name: &'static str,
    /// How much this counts. Negative is evidence against.
    pub weight: f64,
}

impl Signal {
    pub fn of(name: &'static str, weight: f64) -> Signal {
        Signal { name, weight }
    }
}

/// One band on an ordered scale.
///
/// `plain` is not decoration. A band that can only be printed as a number
/// tells a person that something scored 2.4, which is not information. The
/// whole value of banding is that each band means something sayable.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Band {
    pub id: &'static str,
    /// What being in this band means, in the words you would use saying it.
    pub plain: &'static str,
    /// The score at which this band begins.
    pub from: f64,
}

/// Where something landed, and how near the edge.
#[derive(Debug, Clone, PartialEq)]
pub struct Graded {
    pub band: &'static str,
    pub plain: &'static str,
    pub score: f64,
    /// How far into the band it is, measured from the band's own floor.
    ///
    /// The number that says whether to trust the band: a thing scoring 2.01
    /// against a band starting at 2.0 is in that band and barely.
    pub past_the_edge: f64,
    pub clarity: Clarity,
}

impl Graded {
    /// The band to act on, or `None` when it sat on an edge.
    pub fn settled(&self) -> Option<&'static str> {
        match self.clarity {
            Clarity::Clear => Some(self.band),
            _ => None,
        }
    }
}

/// The fewest observations any count-based finding in Atlas will speak on.
/// Thirty, the usual floor below which a sample mean is too unstable to act
/// on; everything that counts before it speaks reads this one number.
pub const MIN_SAMPLE: usize = 30;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct JudgmentConfig {
    /// How far past a band's floor something must be before the band is
    /// treated as settled.
    ///
    /// In the units of whatever is being scored, so a caller whose signals
    /// are worth 1.0 each gets "a whole signal clear of the edge".
    pub past_the_edge: f64,
    /// Below this much evidence either way, there is no judgment to make.
    ///
    /// Separate from the band floors on purpose. A condition with no evidence
    /// at all is not "unlikely"; it is unmeasured, and reporting it as the
    /// bottom band is the `readings`-returns-zeros bug with better manners.
    pub measured_above: f64,
    /// Fewest observations a numeric judgment will speak on.
    ///
    /// [`MIN_SAMPLE`], the one floor every count-based finding in Atlas uses:
    /// a finding with twelve observations behind it is a story, and a story
    /// that came out of a computer is believed harder than one that did not.
    /// A second number meaning the same thing, invented in a different file,
    /// is how the two come to disagree.
    pub min_seen: usize,
}

impl Default for JudgmentConfig {
    fn default() -> Self {
        JudgmentConfig { past_the_edge: 0.5, measured_above: 0.001, min_seen: MIN_SAMPLE }
    }
}

/// Add up named evidence.
///
/// Trivial, and worth existing so that every caller totals the same way and
/// the signals stay a list rather than becoming a float early.
///
/// # On the names in this module
///
/// `add_up`, `which_band`, `how_strongly` and `severity_of` are deliberately
/// not the obvious `total`, `grade`, `holds` and `how_bad`. Every one of
/// those four already names a function somewhere else in this tree --
/// `retention::total`, `tier::total`, `understood::grade`, `hollowcode::how_bad`,
/// and four separate `holds` -- and the deadness scans read bare names, so
/// each would have made another module's function look reached.
///
/// Four collisions in one new file, on top of five caught over the two days
/// before it. A general-purpose module is the worst place for this, because
/// its vocabulary is *meant* to be the ordinary words. Named distinctly at
/// the point of writing rather than after the guard complained.
pub fn add_up(signals: &[Signal]) -> f64 {
    signals.iter().map(|s| s.weight).sum()
}

/// Which band a score falls in, and whether it is clearly in it.
///
/// `bands` may be in any order — they are sorted here. Depending on the
/// caller to list them low-to-high is exactly the kind of unwritten ordering
/// this module exists to remove.
///
/// Returns [`Clarity::Nothing`] when the score is below the lowest band,
/// rather than clamping to it. Something that scored under every band is not
/// in the bottom one; it is off the scale, and that is a different sentence.
pub fn which_band(score: f64, bands: &[Band], cfg: &JudgmentConfig) -> Graded {
    let mut sorted: Vec<&Band> = bands.iter().collect();
    sorted.sort_by(|a, b| a.from.partial_cmp(&b.from).unwrap_or(std::cmp::Ordering::Equal));

    let found = sorted.iter().rev().find(|b| score >= b.from);
    let Some(b) = found else {
        return Graded {
            band: "",
            plain: "",
            score,
            past_the_edge: 0.0,
            clarity: Clarity::Nothing,
        };
    };
    let past = score - b.from;
    // Near the top edge counts too: a thing just short of the next band is as
    // uncertain as one just past this one's floor, and only checking the
    // floor would call it settled.
    let below_next = sorted
        .iter()
        .find(|n| n.from > b.from)
        .map(|n| n.from - score)
        .unwrap_or(f64::INFINITY);
    let clear = past >= cfg.past_the_edge && below_next >= cfg.past_the_edge;
    Graded {
        band: b.id,
        plain: b.plain,
        score,
        past_the_edge: past,
        clarity: if clear { Clarity::Clear } else { Clarity::Close },
    }
}

/// How strongly the evidence says a condition is true.
#[derive(Debug, Clone, PartialEq)]
pub struct Held {
    /// Zero to one. Half means the evidence is balanced, not that the
    /// condition half-applies.
    pub likely: f64,
    pub clarity: Clarity,
    /// What counted for it, biggest first.
    pub because: Vec<&'static str>,
}

impl Held {
    /// True enough to act on.
    pub fn settled(&self) -> Option<bool> {
        match self.clarity {
            Clarity::Clear => Some(self.likely > 0.5),
            _ => None,
        }
    }
}

/// Weigh a condition, counting what argues against it.
///
/// `likely` is the share of the total evidence that is for the condition, so
/// balanced evidence comes back at 0.5 — **which means the evidence is
/// balanced, not that the condition half-holds.** Those are different facts
/// and `Clarity::Close` is how this one says which it means.
///
/// Evidence weights are magnitudes: a `Signal` with a negative weight in
/// `for_it` counts against, and `against` is a convenience for the caller who
/// has the two lists already apart.
pub fn how_strongly(for_it: &[Signal], against: &[Signal], cfg: &JudgmentConfig) -> Held {
    let f: f64 = for_it.iter().map(|s| s.weight.abs()).sum();
    let a: f64 = against.iter().map(|s| s.weight.abs()).sum();
    let all = f + a;
    if all <= cfg.measured_above {
        // Nothing was weighed. Not "unlikely" -- unmeasured.
        return Held { likely: 0.0, clarity: Clarity::Nothing, because: Vec::new() };
    }
    let likely = f / all;
    let mut because: Vec<(&'static str, f64)> =
        for_it.iter().map(|s| (s.name, s.weight.abs())).collect();
    because.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));

    // A margin, in the same shape the rest of this file uses: how far from
    // balanced. `past_the_edge` is in evidence units elsewhere and in shares
    // here, so it is halved -- a caller asking for "half a signal clear"
    // should not accidentally demand near-certainty.
    let from_balanced = (likely - 0.5).abs() * 2.0;
    let clarity = if from_balanced >= cfg.past_the_edge { Clarity::Clear } else { Clarity::Close };
    Held { likely, clarity, because: because.into_iter().map(|(n, _)| n).collect() }
}

// ===================== numbers =====================
//
// What `which_band` and `how_strongly` could not do, and why it mattered.
//
// Both take a score the caller has already worked out, which is fine when the
// evidence is words -- a matched phrase is worth one, a two-word phrase two,
// and those are comparable because they are the same kind of thing. Numbers
// are not. A £200 rise and a 0.3-ATR move cannot be added, and the useful
// question about either is not "how big" but **"how unusual for this"**.
//
// Two rules:
//
// 1. **Nothing speaks below a sample floor.** `judgment::MIN_SAMPLE` is 30, and
//    its reason is written down: a finding with twelve observations behind it
//    is a story, and a story that came out of a computer is believed harder
//    than one that did not.
// 2. **Compare against a measured baseline, not an invented threshold.**
//    A gate like `straightness >= 0.45` is a number that rests on no
//    measurement. The efficiency ratio's random-walk baseline is
//    1/sqrt(n), so the universally quoted "above 0.30 means trending" is
//    *below chance* at n = 10. A gate that does not know its own baseline is
//    not measuring what it says it is.
//
// So a number arrives with what ordinary looks like and how much it usually
// varies, and comes out as a count of ordinary variations -- which is
// unitless, and therefore addable across dimensions that share nothing else.

/// A number, with what ordinary looks like for it.
///
/// `usually` and `varies_by` are the caller's own baseline, measured from its
/// own history. Nothing here invents them, and `seen` is how many
/// observations they rest on -- without which the other two are a guess with
/// decimal places.
///
/// # Which centre and which spread
///
/// 1. **The median as `usually`.** Real records are skewed, and the skew is
///    not consistent in sign from one record to the next, so it cannot be
///    corrected for with a constant. The mean is pulled by the tail; the
///    median is not.
/// 2. **Mean absolute deviation as `varies_by`.** Heavy-tailed records make
///    the standard deviation noisy at the sample sizes a person actually has
///    (a single outlier moves it); the absolute form does not. For a normal
///    distribution `sd/mad` is about 1.25, which is the conversion if a
///    caller wants to think in standard deviations.
/// 3. **Normalise per record, always.** Two records of the same kind can
///    differ in scale by an order of magnitude; pooled, they describe
///    neither. That is the whole argument for this function existing, and
///    for `money.category_jump`'s single fraction being the wrong shape.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Measured {
    pub name: &'static str,
    pub value: f64,
    /// What this usually is.
    pub usually: f64,
    /// How much it usually moves either side of that.
    pub varies_by: f64,
    /// How many observations `usually` and `varies_by` rest on.
    pub seen: usize,
}

/// How unusual this value is, counted in its own ordinary variations.
///
/// Unitless on purpose: **that is what makes two measurements on different
/// scales comparable at all.** A grocery bill £200 above a £150 spread and an
/// entry 1.3 ATR beyond its mean are both "about 1.3 unusual", and only in
/// that form can they be weighed together.
///
/// `None`, not zero, when there is not enough behind the baseline or the
/// thing does not vary. Zero would read as "perfectly ordinary", which is a
/// claim, and the honest answer is that nothing was measured -- the
/// distinction this whole module turns on.
pub fn how_unusual(m: &Measured, cfg: &JudgmentConfig) -> Option<f64> {
    if m.seen < cfg.min_seen {
        return None;
    }
    if !m.varies_by.is_finite() || m.varies_by <= 0.0 || !m.value.is_finite() {
        return None;
    }
    Some((m.value - m.usually) / m.varies_by)
}

/// The median, and the mean distance from it.
///
/// The baseline a caller should build a [`Measured`] from, in one place so
/// that six callers do not each decide what "usually" means. See
/// [`Measured`]'s own note for why the median and the absolute deviation
/// rather than the mean and the standard deviation.
///
/// `None` on an empty slice, because there is no centre of nothing.
pub fn ordinary_for(values: &[f64]) -> Option<(f64, f64)> {
    let mut v: Vec<f64> = values.iter().copied().filter(|x| x.is_finite()).collect();
    if v.is_empty() {
        return None;
    }
    v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let mid = v.len() / 2;
    let median =
        if v.len() % 2 == 0 { (v[mid - 1] + v[mid]) / 2.0 } else { v[mid] };
    let spread = v.iter().map(|x| (x - median).abs()).sum::<f64>() / v.len() as f64;
    Some((median, spread))
}

/// The same, for things that are **counts of events**.
///
/// A count carries noise of its own that the observed spread can miss
/// entirely. Somebody who worked exactly five times in each of nine hours has
/// an observed spread of nearly zero -- and then an hour with four in it is
/// *infinitely* unusual, which is arithmetic rather than a fact about them.
///
/// Counted events vary by about the square root of their own size even when
/// nothing has changed: five events one week and seven the next is the same
/// rate, not a rise. So the spread is floored at the square root of the
/// middle, which is the sampling noise of a count and not a number chosen
/// here.
///
/// Found by `person::is_late` refusing to drift. Atlas is meant to stop
/// calling 2am late once you have worked it four nights running -- and
/// against a nine-to-five whose hours were all identical, the floorless
/// spread made every hour outside that block permanently extraordinary.
///
/// Use this for hours worked, posts made, trades taken, times a thing
/// happened. Use [`ordinary_for`] for measured quantities -- R multiples,
/// durations, rates -- where the spread you observe is the spread there is.
pub fn ordinary_for_counts(values: &[f64]) -> Option<(f64, f64)> {
    let (middle, spread) = ordinary_for(values)?;
    Some((middle, spread.max(middle.max(0.0).sqrt())))
}

/// Several measurements, weighed together on one scale.
///
/// The composite-scoring shape: each dimension is normalised by its own
/// spread first, so a setup graded on entry, spread, time of day and size is
/// one number rather than four incomparable ones -- and **changing a weight
/// does not require re-measuring anything**, which is the thing you want
/// while tuning.
///
/// Returns the score and the dimensions that had nothing behind them. A
/// caller that ignores the second half is claiming a composite it only partly
/// measured, so the names come back rather than being dropped.
///
/// `None` when nothing could be measured at all. Not zero: a setup where
/// every dimension is short of its sample floor has not scored neutral, it
/// has not been scored.
pub fn weighed_together(
    parts: &[(Measured, f64)],
    cfg: &JudgmentConfig,
) -> (Option<f64>, Vec<&'static str>) {
    let mut sum = 0.0;
    let mut weight = 0.0;
    let mut missing = Vec::new();
    for (m, w) in parts {
        match how_unusual(m, cfg) {
            Some(u) => {
                sum += u * w;
                weight += w.abs();
            }
            None => missing.push(m.name),
        }
    }
    if weight <= 0.0 {
        return (None, missing);
    }
    // Divided by the weight that actually counted, not by the weight asked
    // for. Dividing by the full weight would quietly pull every partly
    // measured composite towards zero -- which reads as "ordinary" and is the
    // `readings`-returns-zeros bug wearing arithmetic.
    (Some(sum / weight), missing)
}

/// What to say about a composite that was only partly measured.
///
/// Said rather than swallowed. A score built from two of five dimensions is
/// not the same claim as one built from five, and the difference is exactly
/// the kind a person acts on without noticing they were not told.
pub fn only_partly_measured(missing: &[&'static str], of: usize) -> Option<String> {
    if missing.is_empty() {
        return None;
    }
    Some(format!(
        "{} of {of} had too little behind them to count: {}.",
        missing.len(),
        missing.join(", ")
    ))
}

// ===================== the bands that are in use =====================

/// How badly a hollow answer matters.
///
/// `hollow::spoken` took the first three findings and called them "worst".
/// They were the first three *questions* that came back hollow, in the order
/// `SELF_QUESTIONS` is written — order posing as judgment, which is the same
/// defect the daemon's `contains` arms had and the same one `whichone`
/// replaced.
///
/// The bands are described rather than numbered because the difference
/// between them is what you do about it, and that is a sentence.
pub const HOW_BAD: &[Band] = &[
    Band {
        id: "noted",
        plain: "worth knowing, not worth stopping for",
        from: 1.0,
    },
    Band {
        id: "misleading",
        plain: "this one would have you believe something untrue",
        from: 2.0,
    },
    Band {
        id: "claims-it-works",
        plain: "it says it is fine and it is not, which is the worst kind",
        from: 3.0,
    },
];

/// What makes one hollow answer worse than another.
///
/// Two things, and the second is why this is not just a ranking of the
/// `Why` variants:
///
/// 1. **What kind of hollow it is.** Saying "not wired up" out loud is honest
///    and merely unfinished. Claiming everything is fine while every number
///    is zero is the failure `hollow.rs` was written about, and it is worse
///    because nothing downstream can tell.
/// 2. **Whether it was silent about it.** An answer with no text at all gives
///    a person nothing to judge, so they cannot catch it themselves.
pub fn severity_of(h: &crate::hollow::Hollow) -> Vec<Signal> {
    use crate::hollow::Why;
    let kind = match h.why {
        // Honest about being unfinished. The least bad thing on this list.
        Why::SaysSoItself => Signal::of("said so itself", 1.0),
        Why::EchoedTheQuestion => Signal::of("described the question", 1.5),
        Why::Silent => Signal::of("said nothing at all", 2.0),
        Why::NullAsAnAnswer => Signal::of("a placeholder where the value was the point", 2.0),
        // An unread instrument reporting as a reading.
        Why::NothingMeasured => Signal::of("read zero of everything", 2.5),
        // The one this module exists for.
        Why::ZeroDressedAsFine => Signal::of("claimed fine while every number was zero", 3.0),
    };
    let mut out = vec![kind];
    if h.answer.trim().is_empty() {
        out.push(Signal::of("left you nothing to judge", 0.5));
    }
    out
}

/// Is what the self-audit found worth interrupting you about?
///
/// The nightly audit raised its recommendations and **never ran the hollow
/// check at all** -- `hollow_answers()` was reached only when you asked. So
/// the one pass that happens without you asking was the one that skipped the
/// bug-detector, which is the same shape as the detector that never ran in
/// the first place.
///
/// A count is not the answer here. Six findings that all say "not wired up"
/// are a to-do list; one that says "claimed fine while every number was zero"
/// is something you want to know tonight. So the evidence is graded and the
/// evidence *against* interrupting is counted too, which is the half a
/// threshold cannot express.
pub fn worth_raising(found: &[crate::hollow::Hollow], cfg: &JudgmentConfig) -> Held {
    if found.is_empty() {
        // Not "unlikely to be worth raising" -- there is nothing to raise.
        return how_strongly(&[], &[], cfg);
    }
    let ranked = worst_first(found, cfg);
    let worst = &ranked[0].1;

    let mut for_it = Vec::new();
    let mut against = Vec::new();

    match worst.band {
        "claims-it-works" => for_it.push(Signal::of("one of them claims to be fine and is not", 3.0)),
        "misleading" => for_it.push(Signal::of("one of them would mislead you", 1.5)),
        // Everything honest about being unfinished. That is a backlog, and a
        // backlog read out at night is how you learn to ignore the voice.
        _ => against.push(Signal::of("all of them are honest about being unfinished", 2.0)),
    }
    if found.len() >= 3 {
        for_it.push(Signal::of("several at once", 1.0));
    } else {
        against.push(Signal::of("only one or two", 0.5));
    }
    // A finding Atlas could not grade clearly is not evidence for waking you.
    if worst.settled().is_none() {
        against.push(Signal::of("how bad it is isn't clear", 1.0));
    }
    how_strongly(&for_it, &against, cfg)
}

/// The findings, worst first, with how sure the grading is.
///
/// Sorted by score rather than by the order the questions happen to be
/// written in. Ties keep their original order, so two findings Atlas cannot
/// tell apart are not reshuffled into a false ranking.
pub fn worst_first(
    found: &[crate::hollow::Hollow],
    cfg: &JudgmentConfig,
) -> Vec<(crate::hollow::Hollow, Graded)> {
    let mut graded: Vec<(crate::hollow::Hollow, Graded)> = found
        .iter()
        .map(|h| {
            let score = add_up(&severity_of(h));
            (h.clone(), which_band(score, HOW_BAD, cfg))
        })
        .collect();
    graded.sort_by(|a, b| b.1.score.partial_cmp(&a.1.score).unwrap_or(std::cmp::Ordering::Equal));
    graded
}
