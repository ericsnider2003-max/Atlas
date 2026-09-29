//! Whether the higher timeframe agrees — reading more than one timeframe
//! for the same instrument at once, instead of reading one in isolation
//! and staying blind to what the others say.
//!
//! General capability, not specific to any one business: any market read
//! benefits from knowing whether a shorter and a longer view of the same
//! instrument point the same way, the same reason a chart reader glances
//! at the daily before trusting the hourly.
//!
//! ## What this is not
//!
//! **Not a claim that agreement predicts anything.** The one measurement
//! run against this question so far (12 Sep 2026, on one set of FX
//! data) found H4 agreement and H4 conflict both landed inside the noise
//! — a real test, not a proven edge. This module exists because Atlas
//! could not previously even *ask* the question — "does the higher
//! timeframe agree" was a capability gap, not a validated signal. Building
//! the ability to ask is not the same as answering it, and nothing here
//! should be read as the latter.
//!
//! **Not a resampler.** This does not build an H4 bar out of four H1 bars.
//! It takes views the caller already has at each timeframe — exactly how
//! the numbers were checked on 12 Sep 2026, where separate H1/H4/D books
//! already exist independently — and reads each one's own structure using
//! [`crate::market::timeframe`]'s own horizon-to-bar-count conversion, so a
//! "day of structure" means the right number of bars on every timeframe
//! rather than one raw count applied everywhere.

use super::bars::AsOf;
use super::structure::{recent, Trend};
use super::timeframe::{self, Horizon};

/// One timeframe's own structural read, labeled so a caller can tell which
/// is which.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameRead {
    pub timeframe: String,
    pub trend: Trend,
}

/// Read one view's own structure, using its own inferred timeframe to
/// decide how many bars count as [`Horizon::Structure`] — a trading
/// week's worth, converted correctly whether this view is M15 or D.
pub fn read_frame(view: &AsOf<'_>, k: usize) -> FrameRead {
    let tf_name = timeframe::infer(view).map(|t| t.name().to_string()).unwrap_or_else(|| "unknown".into());
    let n = timeframe::window(view, Horizon::Structure, 20);
    let structure = recent(view, n, k);
    FrameRead { timeframe: tf_name, trend: structure.trend() }
}

/// Read every given view and return one labeled result per timeframe, in
/// the order given.
pub fn read_all(views: &[(&AsOf<'_>, usize)]) -> Vec<FrameRead> {
    views.iter().map(|(view, k)| read_frame(view, *k)).collect()
}

/// Whether the timeframes checked actually agree on a direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Agreement {
    /// Every timeframe checked had a directional read, and all of them
    /// agreed on which way.
    Agrees(Trend),
    /// At least two timeframes had directional reads that disagreed.
    Conflicts,
    /// Nothing directional to agree or conflict about — every timeframe
    /// checked came back Range or Unknown.
    Inconclusive,
    /// Some timeframes had a directional read and agreed with each other,
    /// but at least one timeframe checked had none. Deliberately not
    /// folded into `Agrees` — a silent abstention is not the same claim
    /// as unanimous agreement, and saying "every timeframe agrees" when
    /// one of them had no opinion at all is exactly the overconfident
    /// statement this module exists to avoid making.
    Mixed(Trend),
}

/// Whether the given reads agree. `Range` and `Unknown` are excluded from
/// the direction *comparison* itself — they're not a third direction to
/// conflict with Up or Down, they're an absence of one — but their
/// presence still changes what can honestly be claimed: full agreement
/// requires every read to have had a directional opinion, not just the
/// ones that did to match each other. Confirmed against a real case where
/// this mattered: H1 read DOWN and H4 read RANGE on the same real bars,
/// and the first version of this function reported "every timeframe
/// agrees: DOWN" — true only of the reads that had an opinion, and false
/// of what was actually asked.
pub fn agreement(reads: &[FrameRead]) -> Agreement {
    let directional: Vec<Trend> =
        reads.iter().map(|r| r.trend).filter(|t| matches!(t, Trend::Up | Trend::Down)).collect();
    let Some(&first) = directional.first() else {
        return Agreement::Inconclusive;
    };
    let all_agree = directional.iter().all(|&t| t == first);
    if !all_agree {
        return Agreement::Conflicts;
    }
    if directional.len() == reads.len() {
        Agreement::Agrees(first)
    } else {
        Agreement::Mixed(first)
    }
}

/// A readable summary: one line per timeframe, then what they add up to.
pub fn spoken(reads: &[FrameRead]) -> String {
    let mut out = String::new();
    for r in reads {
        out.push_str(&format!("  {}: {}\n", r.timeframe, r.trend.say()));
    }
    match agreement(reads) {
        Agreement::Agrees(t) => out.push_str(&format!("Every timeframe checked agrees: {}.", t.say())),
        Agreement::Conflicts => out.push_str("These timeframes do not agree with each other."),
        Agreement::Inconclusive => {
            out.push_str("Nothing directional on any timeframe checked — nothing to agree or conflict about.")
        }
        Agreement::Mixed(t) => out.push_str(&format!(
            "The timeframes with a directional read agree ({}), but at least one had no \
             directional read at all — not the same as every timeframe agreeing.",
            t.say()
        )),
    }
    out
}
