//! When Atlas should have no view, whatever the chart says.
//!
//! ## This is a rule, not a reader
//!
//! `market::events` already knows when every central bank speaks, out to 2027,
//! with windows that have measurements behind them rather than a vendor's
//! traffic lights. `inside_blackout` already answers the question.
//!
//! **Nothing called it.** That is the whole of what this file fixes, and it is
//! worth being plain about the size of it: the calendar was built, correct, and
//! inert. A reader that knows when the Fed speaks and trades through it anyway
//! is a reader that has the information and does not use it — which is
//! indistinguishable, from the outside and from the results, from not having
//! the calendar at all.
//!
//! ## Why this earns its place
//!
//! Most of the losses in a retail FX record that look like bad analysis are not
//! bad analysis. They are ordinary setups taken into a release: the spread goes
//! from one pip to eight, the stop is filled four pips past where it sat, and
//! the trade that would have worked is closed at a loss before the move it
//! predicted happens.
//!
//! None of that shows up as a flaw in the reading. The reading was fine. The
//! record just shows a loss, and a system learning from its record learns the
//! wrong lesson — it marks down whichever reading happened to fire that day.
//!
//! ## The bar that gets this wrong
//!
//! > The H4 bar closing at 16:00 covers 12:00–16:00, so it **contains** the
//! > 12:30 payrolls print, its window and the whole recovery. Its close sits
//! > three and a half hours clear, so a close-only news check calls that bar
//! > clean. It is the least clean bar of the week.
//!
//! That is the second chat's finding and it is the reason this checks a bar's
//! **span** and not its close. Asking an interval question about an instant
//! gets the worst bar of the week exactly wrong, and gets it wrong in the
//! reassuring direction.

use crate::market::bars::AsOf;
use crate::market::events::{inside_blackout, Event, Purpose};
use crate::market::timeframe::{infer, Tf};

/// Why Atlas is standing down: a scheduled release, and which side of the bar
/// it is on.
///
/// Named `Blackout` rather than `Why` because three modules already have a
/// `Why` and a fourth is how a reader ends up reasoning about the wrong one.
#[derive(Debug, Clone, PartialEq)]
pub enum Blackout {
    /// Price is inside the window around a scheduled release right now.
    Now(Vec<Event>),
    /// The bar being read *contains* a release, even though it closed clear of
    /// one. The case a close-only check calls clean.
    InsideTheBar(Vec<Event>),
}

impl Blackout {
    pub fn events(&self) -> &[Event] {
        match self {
            Blackout::Now(e) | Blackout::InsideTheBar(e) => e,
        }
    }

    pub fn plain(&self) -> String {
        let named: Vec<String> = self.events().iter().take(3).map(|e| e.say()).collect();
        // A release with no fixed time (the BoJ's hour-wide band) is read as
        // the band, not a point — `Event::is_window`'s own note: "modelling it
        // as a point would be precise and wrong". Said, because "wait for the
        // print" has no meaning until it has actually spoken.
        let banded = self.events().iter().take(3).find(|e| e.is_window()).map(|e| {
            format!(
                ". {} has no fixed time — it lands somewhere in its band, so there's no \
                 'after the print' until it has actually spoken, and a late statement is \
                 itself information",
                e.name
            )
        });
        let banded = banded.unwrap_or_default();
        match self {
            Blackout::Now(_) => format!(
                "there's a release inside the window right now — {}. The spread widens, the \
                 stop gets filled past where it sits, and a reading that was fine takes a loss \
                 that has nothing to do with it{banded}",
                named.join("; ")
            ),
            Blackout::InsideTheBar(_) => format!(
                "the bar I'm reading contains a release — {}. It closed clear of it, so a \
                 check on the close would call this bar clean. It is the least clean bar on \
                 the chart{banded}",
                named.join("; ")
            ),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StanddownConfig {
    /// Which window. `AdverseSelection` is the default rather than
    /// `Execution`, because the spread is what takes the trade out — and it
    /// starts widening thirty minutes *before* the print, which the narrow
    /// window does not cover.
    pub purpose: Purpose,
    /// Check whether the bar being read contains a release, as well as whether
    /// price is inside one now.
    pub check_the_bar: bool,
    /// Treat a series with no timestamps as safe.
    ///
    /// `false`, and it should stay false. A reader with no clock cannot know
    /// whether it is standing in front of a release, and "I cannot tell" is
    /// not "it is fine".
    pub no_clock_is_clear: bool,
}

impl Default for StanddownConfig {
    fn default() -> Self {
        StanddownConfig {
            purpose: Purpose::AdverseSelection,
            check_the_bar: true,
            no_clock_is_clear: false,
        }
    }
}

/// Should Atlas have a view on this instrument, at this moment?
///
/// `None` means go ahead. The instrument matters: a US release moves AUD/JPY
/// with no dollar in it at all, because the carry cross-section unwinds, and
/// `Event::touches` knows that.
pub fn standing_down(
    view: &AsOf<'_>,
    instrument: &str,
    cfg: &StanddownConfig,
) -> Result<Option<Blackout>, String> {
    let Some(opened) = view.opened_at() else {
        // No clock. Not the same as clear, and the default says so.
        return if cfg.no_clock_is_clear {
            Ok(None)
        } else {
            Err("these bars carry no timestamps, so I can't tell whether there's a release in \
                 front of me. That isn't the same as there not being one"
                .into())
        };
    };

    // TWO QUESTIONS, TWO INSTANTS, and conflating them was the bug.
    //
    // "Is price in a blackout right now?" is about the moment the decision is
    // being made, which is when the last bar COMPLETED — you see a finished
    // bar and then act. "Does the bar I am reading contain a release?" is
    // about the bar's whole span, which starts at its OPEN.
    //
    // Both used to be asked with the same number: the bar's stamp, passed to
    // `inside_blackout` as though it were the decision instant and to
    // `Tf::bar_window(close_ms)` as though it were a close. The stamps are
    // opens, so the span question came out a full bar early — the bar
    // containing a payrolls print read clean, and the next one, hours past the
    // release, was refused. See `AsOf::opened_at` for the evidence that the
    // stamps are opens.
    //
    // `closes_at` needs at least two stamps to know how long a bar is. A
    // one-bar series falls back to the open: it is the only instant there is,
    // and the alternative is refusing to answer at all on a series that has a
    // clock.
    let deciding_at = view.closes_at().unwrap_or(opened);

    match inside_blackout(deciding_at, instrument, cfg.purpose) {
        Ok((true, events)) => return Ok(Some(Blackout::Now(events))),
        Ok((false, _)) => {}
        Err(e) => return Err(e.0),
    }

    if !cfg.check_the_bar {
        return Ok(None);
    }

    // The bar as a span. On M1 this is nearly the same question as the
    // instant; on H4 it is a different one, and it is the one that matters.
    let Some(tf) = infer(view) else { return Ok(None) };
    if let Some(found) = spanning(tf, opened, instrument, cfg.purpose)? {
        return Ok(Some(Blackout::InsideTheBar(found)));
    }
    Ok(None)
}

/// Releases inside the bar that **opened** at `open_ms`.
///
/// Separate and public so the awkward case can be tested without assembling a
/// view: this is the one that is easy to get right for the wrong reason, and
/// it was got wrong for exactly that reason until 17 Sep 2026 — it took a
/// close, and every caller handed it an open-stamped bar's stamp. See
/// `AsOf::opened_at`.
pub fn spanning(
    tf: Tf,
    open_ms: i64,
    instrument: &str,
    purpose: Purpose,
) -> Result<Option<Vec<Event>>, String> {
    let (opened, closes) = tf.bar_window(open_ms);
    let found = crate::market::events::overlapping(opened, closes, instrument, purpose)
        .map_err(|e| e.0)?;
    Ok((!found.is_empty()).then_some(found))
}

/// Said out loud.
pub fn spoken(view: &AsOf<'_>, instrument: &str, cfg: &StanddownConfig) -> String {
    match standing_down(view, instrument, cfg) {
        Err(e) => e,
        Ok(None) => "Nothing scheduled that would move this. I'll read it normally.".into(),
        Ok(Some(why)) => format!("I'm standing down on {instrument}: {}.", why.plain()),
    }
}
