//! What went wrong once, kept so it's checked every time after.
//!
//! Research report, 30 Sep 2026, Stage 2 item 14. Two things already tell
//! Atlas plainly that it got something wrong -- you, correcting it ("that's
//! wrong" ... "it should have opened Chrome"), and its own self-test finding
//! a command broken -- and neither came back to check. A correction became a
//! lesson in the prompt; a self-test failure became a line in a report. So
//! the same mistake could come back next week with nothing to notice.
//!
//! Now both are cases, kept on disk (`FILE`):
//!
//! * each self-test run says every correction's sentence again, on its copy,
//!   and fails the row if the reply is the one you corrected
//!   (`selftest::run_all`);
//! * a self-test failure is a `SelfTestFails` signal, which self-repair reads
//!   like any other (`signals::from_regressions` → `selfaudit::recommend`).
//!
//! Nothing here stores more than the sentence and the replies already kept
//! in the conversation; no new words are recorded.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    /// You said it was wrong.
    Correction,
    /// The self-test found it broken.
    SelfTest,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Case {
    /// What was said.
    pub said: String,
    /// The reply that was wrong.
    pub wrong: String,
    /// What should have happened, in your words or the self-test's.
    pub wanted: String,
    /// The command, when the self-test knew it.
    #[serde(default)]
    pub command: String,
    pub source: Source,
    pub at: u64,
}

/// Where the cases are kept, in the store.
pub const FILE: &str = "regressions";
/// The most kept; the oldest go first.
pub const MOST: usize = 200;
/// Written by the self-test into its reports folder, and taken into the
/// install's own cases by the process that started it.
pub const FROM_SELFTEST: &str = "failing.json";

/// Add a case, replacing an older one for the same sentence from the same
/// source.
pub fn add(cases: &mut Vec<Case>, case: Case) {
    cases.retain(|c| !(c.source == case.source && c.said.eq_ignore_ascii_case(&case.said)));
    cases.push(case);
    let over = cases.len().saturating_sub(MOST);
    cases.drain(..over);
}

/// A self-test that now passes takes its case off: fixed is fixed.
pub fn settle_selftest(cases: &mut Vec<Case>, failing: &[Case]) {
    cases.retain(|c| c.source != Source::SelfTest);
    for f in failing {
        add(cases, f.clone());
    }
}

/// Did the reply repeat the one you corrected? Compared loosely: case,
/// spacing and trailing punctuation don't make it a different answer.
pub fn repeats_the_mistake(case: &Case, reply: &str) -> bool {
    let norm = |s: &str| s.to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ").trim_end_matches(['.', '!', '?']).to_string();
    case.source == Source::Correction && !case.wrong.trim().is_empty() && norm(&case.wrong) == norm(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(said: &str, source: Source) -> Case {
        Case { said: said.into(), wrong: "Paused.".into(), wanted: "open chrome".into(), command: String::new(), source, at: 1 }
    }

    #[test]
    fn a_sentence_is_kept_once_per_source_and_the_oldest_go_first() {
        let mut v = Vec::new();
        add(&mut v, c("open chrome", Source::Correction));
        add(&mut v, c("Open Chrome", Source::Correction));
        add(&mut v, c("open chrome", Source::SelfTest));
        assert_eq!(v.len(), 2);
        for i in 0..MOST + 5 {
            add(&mut v, c(&format!("s{i}"), Source::Correction));
        }
        assert_eq!(v.len(), MOST);
    }

    #[test]
    fn the_same_wrong_reply_is_the_same_mistake() {
        let k = c("open chrome", Source::Correction);
        assert!(repeats_the_mistake(&k, "paused"));
        assert!(!repeats_the_mistake(&k, "Opening Chrome."));
    }

    #[test]
    fn a_self_test_case_that_passes_now_is_taken_off() {
        let mut v = vec![c("a", Source::SelfTest), c("b", Source::Correction)];
        settle_selftest(&mut v, &[]);
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].source, Source::Correction);
    }
}
