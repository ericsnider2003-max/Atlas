//! Answers that are not answers.
//!
//! Two bugs found on the first day Atlas ran on real hardware shared a shape,
//! and neither was caught by 2,500 tests:
//!
//! * `Daemon::readings` returned `Readings::default()`. Every field zero.
//!   `assess` only reports on values above zero, so a machine that read as all
//!   zeros looked like a machine with nothing wrong. **Absence of a finding
//!   was indistinguishable from absence of a problem.**
//! * The typed prompt answered `parsed Outstanding — not wired to an action
//!   yet` for every intent outside six. The sentence was true of the prompt
//!   and false of Atlas — the action existed. **The code announced its own
//!   incompleteness and nothing was listening.**
//!
//! Call these hollow answers. They are worse than errors, because an error
//! stops and a hollow answer proceeds, looking fine, forever.
//!
//! This module is the listener. It is used three ways: by `doctor` on demand,
//! by the nightly self-audit, and by `tests/no_quiet_nothings.rs` as a ratchet
//! over the source so the count can only go down.

use serde::{Deserialize, Serialize};

/// Why an answer is hollow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Why {
    /// Says out loud that it is not finished.
    SaysSoItself,
    /// A measurement that came back as all zeros. Nothing measures zero of
    /// everything, so this is an unread instrument, not a reading.
    NothingMeasured,
    /// Produced no text at all where text was the whole job.
    Silent,
    /// Answered by describing the question rather than answering it.
    EchoedTheQuestion,
    /// Claimed a positive state while every number in it was zero.
    ///
    /// The failure this module was written about, and the one it could not
    /// see. "All fine. 0 gigabytes free, memory at 0 percent." admits nothing,
    /// is not empty, and echoes no type — it passes every other check here
    /// while being the exact readings stub that started all of this. An audit
    /// run against fourteen real answers missed six, and every miss had this
    /// shape.
    ZeroDressedAsFine,
    /// A bare null word where a value was the whole point.
    ///
    /// "Unknown" as an entire answer is not a reading. It is the absence of
    /// one, formatted to look like one.
    NullAsAnAnswer,
}

impl Why {
    pub fn plain(&self) -> &'static str {
        match self {
            Why::SaysSoItself => "says it isn't wired up",
            Why::NothingMeasured => "read zero of everything, which means it read nothing",
            Why::Silent => "answered with nothing at all",
            Why::EchoedTheQuestion => "described the question instead of answering it",
            Why::ZeroDressedAsFine => {
                "said everything was fine while every number in it was zero"
            }
            Why::NullAsAnAnswer => "gave a placeholder where the value was the whole point",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hollow {
    /// What was asked, or which capability was exercised.
    pub what: String,
    pub why: Why,
    /// The answer itself, so a person can judge it.
    pub answer: String,
}

/// Phrases that mean "this is not built".
///
/// Deliberately literal. A cleverer check that guessed at intent would flag
/// prose that merely discusses unfinished work, and a self-audit that cries
/// wolf gets switched off.
pub const ADMITS_INCOMPLETE: &[&str] = &[
    "not wired to an action",
    "isn't built yet",
    "is not built yet",
    "not built yet",
    "not implemented",
    "unimplemented",
    "coming soon",
    "todo:",
    "placeholder",
    "stub",
];

/// Shapes that mean "I am repeating your question back".
///
/// The prompt's catch-all printed the parsed `Intent` in debug form. Anything
/// that leaks an internal type name into a spoken answer is this.
pub const ECHOES_THE_QUESTION: &[&str] = &["parsed ", "Intent::", "Some(", "None)"];

/// Judge one answer.
pub fn judge(what: &str, answer: &str) -> Option<Hollow> {
    let a = answer.trim();
    let hollow = |why| {
        Some(Hollow { what: what.into(), why, answer: answer.trim().to_string() })
    };
    if a.is_empty() {
        return hollow(Why::Silent);
    }
    let lower = a.to_lowercase();
    if ADMITS_INCOMPLETE.iter().any(|p| lower.contains(p)) {
        return hollow(Why::SaysSoItself);
    }
    if ECHOES_THE_QUESTION.iter().any(|p| a.contains(p)) {
        return hollow(Why::EchoedTheQuestion);
    }
    if is_null_word(&lower) {
        return hollow(Why::NullAsAnAnswer);
    }
    if claims_fine_on_nothing(&lower) {
        return hollow(Why::ZeroDressedAsFine);
    }
    None
}

/// Words that claim a good or completed state.
///
/// Only these matter for the zero check. "Nothing outstanding" and "you did
/// nothing today" are *correct* answers whose true value is zero, and flagging
/// them would be the crying-wolf failure this module deliberately avoids. What
/// separates a hollow one is the assertion sitting on top: a claim that
/// something is fine, connected, or was carried out, with nothing behind it.
pub const CLAIMS_A_GOOD_STATE: &[&str] = &[
    "all fine",
    "all good",
    "everything's connected",
    "everything is connected",
    "all working",
    "all healthy",
    "everything's fine",
    "everything is fine",
    "all checks passed",
    "all clear",
];

/// Claims that some work was actually carried out.
const CLAIMS_WORK_HAPPENED: &[&str] = &["ran ", "checked ", "scanned ", "processed ", "sent "];

/// A whole answer that is only a placeholder.
pub const NULL_WORDS: &[&str] =
    &["unknown", "none", "n/a", "na", "null", "nil", "undefined", "-", "?"];

fn is_null_word(lower: &str) -> bool {
    let t = lower.trim_end_matches(['.', '!']).trim();
    NULL_WORDS.contains(&t)
}

/// Every number in the text, so "all fine" can be checked against them.
fn numbers_in(text: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() || (c == '.' && !cur.is_empty()) {
            cur.push(c);
        } else {
            if !cur.is_empty() {
                if let Ok(n) = cur.trim_end_matches('.').parse::<f64>() {
                    out.push(n);
                }
                cur.clear();
            }
        }
    }
    if let Ok(n) = cur.trim_end_matches('.').parse::<f64>() {
        out.push(n);
    }
    out
}

/// Does this assert a positive state with nothing behind it?
fn claims_fine_on_nothing(lower: &str) -> bool {
    let nums = numbers_in(lower);
    let all_zero = !nums.is_empty() && nums.iter().all(|n| *n == 0.0);

    if CLAIMS_A_GOOD_STATE.iter().any(|p| lower.contains(p)) {
        // "All fine, 300 GB free" is a real answer. "All fine, 0 GB free" is
        // an unread instrument wearing one. And a bare "everything's
        // connected" with no numbers at all is the empty-board case.
        return all_zero || nums.is_empty();
    }
    // "Ran 0 checks" claims an action that did not happen.
    if CLAIMS_WORK_HAPPENED.iter().any(|p| lower.contains(p)) && all_zero {
        return true;
    }
    // A line that is nothing but zeros and units, with no claim attached, is
    // still an unread instrument: "Disk: 0 GB free of 0 GB".
    if all_zero && nums.len() >= 2 {
        return true;
    }
    false
}

/// Judge a set of measurements.
///
/// The rule that would have caught the readings stub: **if every number is
/// zero, the instrument was not read.** No real machine has zero bytes of
/// memory and zero bytes of disk and zero days of uptime.
pub fn judge_readings(r: &crate::health::Readings) -> Option<Hollow> {
    // Any unread instrument, not only all of them.
    //
    // This required *every* field to be zero, which made it blind to the case
    // that actually shipped: off Windows, `read_disk` was an empty stub while
    // `read_memory` worked, so disk read zero forever and this returned None.
    // A detector that only fires when everything is broken cannot catch a
    // system that is half broken -- and half broken is the normal way things
    // break.
    let unread = unread_instruments(r);
    if unread.is_empty() {
        return None;
    }
    let all = unread.len() >= 2;
    Some(Hollow {
        what: "machine readings".into(),
        why: Why::NothingMeasured,
        answer: if all {
            "every instrument read zero".into()
        } else {
            format!("{} read zero", unread.join(" and "))
        },
    })
}

/// Which individual instruments came back unread, by name.
///
/// Separate from `judge_readings` on purpose: one unread instrument among
/// several is a smaller problem than all of them, and worth naming precisely
/// rather than lumping together.
pub fn unread_instruments(r: &crate::health::Readings) -> Vec<&'static str> {
    let mut out = Vec::new();
    if r.ram_total_gb <= 0.0 {
        out.push("memory");
    }
    if r.disk_total_gb <= 0.0 {
        out.push("disk");
    }
    out
}

/// Run every cheap self-question Atlas can ask itself, and report the hollow
/// answers.
///
/// `ask` is whatever answers a typed line — in practice `Daemon::execute`
/// behind a closure, so this module needs no knowledge of the daemon and can
/// be tested with a stub.
pub fn audit<F: FnMut(&str) -> String>(mut ask: F) -> Vec<Hollow> {
    SELF_QUESTIONS.iter().filter_map(|q| judge(q, &ask(q))).collect()
}

/// The questions the audit asks. Every one is read-only: nothing here can move
/// a window, send anything, or change a setting, because an audit that has
/// side effects is one nobody dares run.
pub const SELF_QUESTIONS: &[&str] = &[
    "what's outstanding",
    "what's queued",
    "how's the machine",
    "what can you do",
    "what did you do today",
];

/// One line, for a person.
/// What Atlas says about what it found, worst first.
///
/// This took `found.iter().take(3)` and called them `worst`. They were the
/// first three *questions* that came back hollow, in the order
/// `SELF_QUESTIONS` happens to be written -- so a run where the badly broken
/// thing was asked about fifth reported three mild ones and a count.
///
/// The name said "worst" and the code said "first", and nothing was wrong
/// enough to fail. `judgment::worst_first` grades each finding against
/// described bands and sorts, so the word is now true.
pub fn spoken(found: &[Hollow], cfg: &crate::judgment::JudgmentConfig) -> String {
    if found.is_empty() {
        return "Everything I asked myself came back with a real answer.".into();
    }
    let ranked = crate::judgment::worst_first(found, cfg);
    let worst: Vec<String> = ranked
        .iter()
        .take(3)
        .map(|(h, g)| {
            // The band, when the grading was clear about it. On an edge Atlas
            // says what it found and not how bad, because a severity it is
            // not sure of is worse than none -- it gets acted on.
            match g.settled() {
                Some(_) => format!("{} {} ({})", h.what, h.why.plain(), g.plain),
                None => format!("{} {}", h.what, h.why.plain()),
            }
        })
        .collect();
    format!(
        "{} thing{} answered hollow, worst first: {}.",
        found.len(),
        if found.len() == 1 { "" } else { "s" },
        worst.join("; ")
    )
}
