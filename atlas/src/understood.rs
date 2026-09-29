//! Acting on a guess.
//!
//! Atlas arrives at what you meant two different ways. A phrase in its own
//! list is matched outright — it cannot have misread that. Anything else is
//! handed to a small local model, which returns an intent in exactly the same
//! confident shape whether it recognised your sentence or invented a reading
//! of it.
//!
//! The policy gate grades *what Atlas is about to do*. Nothing has ever told
//! it *how sure Atlas is that you asked for it*. So a dictation matched from
//! the phrase list and a dictation the model guessed out of a mumble are
//! graded identically, and both simply happen. `brain.rs` asks the model to
//! "use ask rather than guessing" — but that is an instruction to the very
//! model whose confident wrongness `certainty.rs` exists to catch, and
//! nothing checks whether it obeyed.
//!
//! This is the missing half, and it is deliberately the smallest thing that
//! closes the gap: an inferred reading of something that *changes the world*
//! is graded one rung stricter, so Atlas says what it took you to mean and
//! waits for an answer instead of acting in hope. Being told what Atlas
//! assumed costs you a sentence. Undoing what it did on a guess costs
//! whatever it did.
//!
//! Three properties, on purpose:
//!
//! - **It only ever escalates.** Like [`crate::voiceid::handle`], this
//!   function can return a stricter grade and never a looser one, so the
//!   worst case is a question you did not need to be asked.
//! - **Reading is left alone.** An inferred `AutoProceed` stays automatic.
//!   Ordinary conversation reaches the model constantly, and asking "did you
//!   mean that?" before answering a question would make Atlas unusable while
//!   protecting nothing — nothing was going to change.
//! - **Already-asking stays as it is.** `AskClarification` and
//!   `RequireApproval` are the two rungs that already stop and ask. There is
//!   nothing above them to escalate to.

use crate::policy::Decision;
use serde::Deserialize;

/// How Atlas came to believe this is what you asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Understanding {
    /// Matched against Atlas's own phrase list. Deterministic: the same words
    /// always give the same intent, and no model was consulted.
    Matched,
    /// A small local model's reading of a sentence the phrase list did not
    /// know. It may be right. Nothing has checked.
    Inferred,
}

impl Understanding {
    /// Read from what the brain reported about how it got there.
    ///
    /// `Reached::No` — the model was asked and did not answer — counts as
    /// matched rather than inferred, because in that case no model reading
    /// exists to doubt: the reply was produced without one.
    pub fn from_reached(r: crate::brain::Reached) -> Self {
        match r {
            crate::brain::Reached::Yes => Understanding::Inferred,
            _ => Understanding::Matched,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UnderstoodConfig {
    /// On by default, unlike most of `tools.yaml`.
    ///
    /// The usual default is off, so that a missing config means Atlas does
    /// less rather than more. This one inverts that for the same reason:
    /// switching it off does not remove a behaviour, it removes a question —
    /// and what is left is Atlas acting on the model's guess without saying
    /// so. "Less" here means fewer things done unasked.
    pub enabled: bool,
}

impl Default for UnderstoodConfig {
    fn default() -> Self {
        UnderstoodConfig { enabled: true }
    }
}

/// Grade an action by how Atlas came to understand the request.
///
/// `base` is the grade the action already earned on its own merits — pass
/// the intrinsic [`crate::policy::classify`] rather than the
/// history-relaxed `classify_with_policy`, for the same reason `voiceid`
/// does: a standing grant records that *you* approved something like this
/// before, and what is in doubt here is whether you asked for it at all.
pub fn grade(base: Decision, u: Understanding, cfg: &UnderstoodConfig) -> Decision {
    if !cfg.enabled || u == Understanding::Matched {
        return base;
    }
    match base {
        // Reading, listing, answering. Nothing changes, so a wrong reading
        // costs you one irrelevant answer and no more.
        Decision::AutoProceed => Decision::AutoProceed,
        // The one that matters. `ProceedAndReport` is "act, then say what was
        // done" — the world has already changed by the time you hear about
        // it. `Intent::Dictate` is graded here, and its own note in
        // `policy.rs` names the exact failure this prevents: "a misheard
        // sentence landing somewhere public".
        Decision::ProceedAndReport => Decision::AskClarification,
        // Already stops and asks.
        other => other,
    }
}

/// What Atlas says when it stops to check a reading.
///
/// It states the assumption rather than pleading ignorance. "I didn't catch
/// that" throws away work Atlas actually did and makes you start over; naming
/// the reading lets you correct one word instead.
pub fn checking(described: &str) -> String {
    format!("I took that as {described}. Say yes and I'll do it, or put it another way.")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_matched_phrase_is_graded_exactly_as_before() {
        for base in [
            Decision::AutoProceed,
            Decision::ProceedAndReport,
            Decision::AskClarification,
            Decision::RequireApproval,
        ] {
            assert_eq!(grade(base, Understanding::Matched, &UnderstoodConfig::default()), base);
        }
    }

    #[test]
    fn an_inferred_change_to_the_world_becomes_a_question() {
        assert_eq!(
            grade(Decision::ProceedAndReport, Understanding::Inferred, &UnderstoodConfig::default()),
            Decision::AskClarification
        );
    }

    #[test]
    fn an_inferred_reading_is_still_just_answered() {
        // The noise guard. Ordinary conversation is inferred almost every
        // time; if this escalated, Atlas would ask before every answer.
        assert_eq!(
            grade(Decision::AutoProceed, Understanding::Inferred, &UnderstoodConfig::default()),
            Decision::AutoProceed
        );
    }

    #[test]
    fn it_never_relaxes_anything() {
        for base in [
            Decision::AutoProceed,
            Decision::ProceedAndReport,
            Decision::AskClarification,
            Decision::RequireApproval,
        ] {
            for u in [Understanding::Matched, Understanding::Inferred] {
                assert!(
                    grade(base, u, &UnderstoodConfig::default()).severity() >= base.severity(),
                    "{base:?}/{u:?} came back looser"
                );
            }
        }
    }

    #[test]
    fn switched_off_it_changes_nothing() {
        let off = UnderstoodConfig { enabled: false };
        assert_eq!(
            grade(Decision::ProceedAndReport, Understanding::Inferred, &off),
            Decision::ProceedAndReport
        );
    }

    #[test]
    fn the_model_being_unreachable_is_not_a_guess() {
        // There is no model reading to doubt when the model never answered.
        assert_eq!(
            Understanding::from_reached(crate::brain::Reached::No),
            Understanding::Matched
        );
        assert_eq!(
            Understanding::from_reached(crate::brain::Reached::NotNeeded),
            Understanding::Matched
        );
        assert_eq!(
            Understanding::from_reached(crate::brain::Reached::Yes),
            Understanding::Inferred
        );
    }

    #[test]
    fn what_it_says_names_the_reading_rather_than_the_failure() {
        let s = checking("dictating into the window you are looking at");
        assert!(s.contains("dictating into the window"), "{s}");
        assert!(!s.to_lowercase().contains("didn't catch"), "{s}");
    }
}
