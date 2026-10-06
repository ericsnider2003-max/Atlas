//! Saying what happened, not what was meant to happen.
//!
//! `certainty` asks whether Atlas knows a thing. `why` explains a decision
//! after the fact. Neither asks the question underneath both: **does this
//! report match what actually occurred?**
//!
//! That gap is the one this whole codebase keeps falling into from the other
//! side. The vault declared itself encrypted while its cipher cancelled
//! itself. The capability list advertised modules nothing could reach. The
//! no-list refused a command string nobody types. Every time, the account of
//! the system was more careful than the system.
//!
//! A spoken report is the same artefact. "Backed up and cleaned up" is a claim
//! about the world, and if the backup failed and the cleanup ran anyway, the
//! sentence is false in the way that costs most: it is fluent, it is
//! plausible, and it stops you looking.
//!
//! The rule this implements: a claim that something is done, saved, sent, or
//! checked must rest on an outcome observed in the same run. Not on the step
//! having been attempted, and not on it usually working.
//!
//! Pure text and enums. No I/O, nothing to schedule, nothing to slow down.

use serde::{Deserialize, Serialize};

/// What became of one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// It ran and the result was observed. The only outcome that backs a
    /// claim of completion.
    Did,
    /// It ran and did not work.
    Failed(String),
    /// It never ran.
    Skipped(String),
    /// It ran and nothing confirmed the result.
    ///
    /// The most dangerous of the four, because from inside the run it feels
    /// identical to `Did`. A file written but never read back, a message
    /// handed to a queue, a command whose exit code went unchecked.
    NotChecked,
}

impl Outcome {
    pub fn went_wrong(&self) -> bool {
        matches!(self, Outcome::Failed(_) | Outcome::Skipped(_))
    }

    /// Does this outcome support saying the thing is done?
    pub fn backs_a_claim(&self) -> bool {
        matches!(self, Outcome::Did)
    }

    pub fn plain(&self) -> String {
        match self {
            Outcome::Did => "done".into(),
            Outcome::Failed(why) => format!("failed: {why}"),
            Outcome::Skipped(why) => format!("skipped: {why}"),
            Outcome::NotChecked => "ran, but nothing checked it".into(),
        }
    }
}

/// One thing Atlas did, and what came of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    pub what: String,
    pub outcome: Outcome,
}

impl Step {
    pub fn did(what: &str) -> Step {
        Step { what: what.into(), outcome: Outcome::Did }
    }
    pub fn failed(what: &str, why: &str) -> Step {
        Step { what: what.into(), outcome: Outcome::Failed(why.into()) }
    }
    pub fn skipped(what: &str, why: &str) -> Step {
        Step { what: what.into(), outcome: Outcome::Skipped(why.into()) }
    }
    pub fn not_checked(what: &str) -> Step {
        Step { what: what.into(), outcome: Outcome::NotChecked }
    }
}

/// A way a report outruns its evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fault {
    /// Something failed or was skipped and the report does not open with it.
    ///
    /// Position matters rather than mere presence. A failure mentioned in the
    /// last sentence of a cheerful paragraph has been disclosed and not
    /// communicated, and the reader stops before reaching it.
    BuriedTheProblem { step: String },
    /// The report says a thing is done and no step observed it working.
    ClaimedWithoutEvidence { word: String },
    /// The report says a thing was checked and nothing checked it.
    SaidCheckedWhenItWasNot { word: String },
    /// Every step went wrong and the report reads as a success.
    NothingWorkedButItReadsFine,
}

impl Fault {
    pub fn plain(&self) -> String {
        match self {
            Fault::BuriedTheProblem { step } => format!(
                "\"{step}\" went wrong and the report doesn't open with it"
            ),
            Fault::ClaimedWithoutEvidence { word } => format!(
                "says \"{word}\" but nothing in the run observed that working"
            ),
            Fault::SaidCheckedWhenItWasNot { word } => format!(
                "says \"{word}\" but nothing was checked"
            ),
            Fault::NothingWorkedButItReadsFine => {
                "every step went wrong and this reads as a success".into()
            }
        }
    }
}

/// Words that assert a thing is finished.
const CLAIMS_DONE: &[&str] = &[
    "done", "finished", "complete", "completed", "sorted", "handled",
    "saved", "sent", "posted", "published", "delivered", "uploaded",
    "downloaded", "installed", "created", "made", "wrote", "written",
    "deleted", "removed", "cleaned", "updated", "fixed", "repaired",
    "backed up", "moved", "renamed", "closed", "opened", "set up",
    "all set", "ready", "sorted out", "taken care of",
];

/// Words that assert a thing was confirmed.
const CLAIMS_CHECKED: &[&str] = &[
    "verified", "confirmed", "checked", "tested", "validated", "made sure",
    "double-checked", "proven", "certain", "definitely", "guaranteed",
];

/// Words that soften a claim enough that it is no longer an assertion.
///
/// "I tried to save it" is not a claim that it saved. Without this, honest
/// hedging would be reported as a fault and the module would push Atlas
/// towards saying less rather than saying true things.
const HEDGES: &[&str] = &[
    "tried", "attempted", "meant to", "should have", "may have", "might have",
    "couldn't", "could not", "didn't", "did not", "wasn't", "was not",
    "failed to", "unable", "not sure", "think", "believe", "probably",
];

fn words_of(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Whole-word or whole-phrase match.
///
/// Whole words for the same reason `finance::is_safe_field` uses them:
/// `contains` would find "done" inside "abandoned" and "sent" inside
/// "presented".
fn mentions(text: &str, needle: &str) -> bool {
    let lower = text.to_lowercase();
    if needle.contains(' ') {
        return lower.contains(needle);
    }
    words_of(&lower).iter().any(|w| w == needle)
}

/// Words that turn the claim after them into its opposite.
const NEGATORS: &[&str] = &[
    "no", "not", "nothing", "never", "none", "nobody", "without", "un",
    "couldn-t", "didn-t", "wasn-t", "isn-t", "hasn-t", "haven-t", "wouldn-t",
    "cannot", "unable", "failed", "yet",
];

/// Does the text actually *assert* this, rather than deny it?
///
/// "nothing checked" contains the word "checked" and asserts the opposite.
/// Without this the module flagged its own output: a report reading
/// "1 ran but nothing checked" was read by `check` as a claim of checking.
///
/// Negation is scoped to the **clause**, not to a fixed number of words back.
/// A two-word window handled "nothing checked" and missed "never got it sent",
/// where the negator is three words away. Clauses are what negation actually
/// attaches to, so a comma or a full stop ends its reach and nothing before
/// that boundary can wrongly cancel a later claim.
fn asserts(text: &str, needle: &str) -> bool {
    let lower = text.to_lowercase();
    for clause in lower.split(['.', ',', ';', '!', '?']) {
        let has_claim = if needle.contains(' ') {
            clause.contains(needle)
        } else {
            words_of(clause).iter().any(|w| w == needle)
        };
        if !has_claim {
            continue;
        }
        // Where in the clause does the claim start? Only negators before it
        // count — "saved, but nothing checked" must not cancel "saved".
        // Byte offsets come from the string itself rather than being counted
        // up as we go. The old version added `word.len() + 1` per word, which
        // assumes every separator is one byte — an em dash is three, so the
        // running total drifted into the middle of a character and slicing
        // there panicked. Atlas transcribes speech and reads web pages, so
        // dashes, curly quotes and accented names are routine rather than
        // exotic.
        let at = if needle.contains(' ') {
            clause.find(needle).unwrap_or(0)
        } else {
            let mut found = clause.len();
            let mut start: Option<usize> = None;
            for (i, c) in clause.char_indices() {
                let part_of_word = c.is_alphanumeric() || c == '-';
                match (part_of_word, start) {
                    (true, None) => start = Some(i),
                    (false, Some(s)) => {
                        if &clause[s..i] == needle {
                            found = s;
                            break;
                        }
                        start = None;
                    }
                    _ => {}
                }
            }
            // A word running to the end of the clause never hits a separator.
            if found == clause.len() {
                if let Some(s) = start {
                    if &clause[s..] == needle {
                        found = s;
                    }
                }
            }
            found
        };
        // `found` is now always a real char boundary, but the clamp stays:
        // cheap, and it means a future change to the search cannot turn a
        // wrong answer into a crash.
        let before = &clause[..at.min(clause.len())];
        let negated = words_of(before)
            .iter()
            .any(|w| NEGATORS.contains(&w.as_str()));
        if !negated {
            return true;
        }
    }
    false
}

fn first_sentence(text: &str) -> &str {
    let end = text
        .find(['.', '!', '?'])
        .map(|i| i + 1)
        .unwrap_or(text.len());
    text[..end].trim()
}

/// Everything wrong with this report, given what actually happened.
///
/// Returns an empty list when the report is supported. Order is worst first:
/// a buried failure outranks an unsupported word, because the reader can
/// recover from a vague sentence and cannot recover from not being told.
pub fn check(steps: &[Step], draft: &str) -> Vec<Fault> {
    let mut faults = Vec::new();
    if steps.is_empty() {
        return faults;
    }

    let opening = first_sentence(draft);
    let hedged = HEDGES.iter().any(|h| mentions(draft, h));

    // 1. Anything that went wrong has to be in the first sentence.
    for s in steps.iter().filter(|s| s.outcome.went_wrong()) {
        let named = words_of(&s.what)
            .iter()
            .filter(|w| w.len() > 3)
            .any(|w| mentions(opening, w));
        let flagged = ["failed", "couldn't", "could not", "didn't", "did not",
                       "skipped", "wrong", "problem", "error", "stopped"]
            .iter()
            .any(|w| mentions(opening, w));
        if !named && !flagged {
            faults.push(Fault::BuriedTheProblem { step: s.what.clone() });
        }
    }

    let anything_worked = steps.iter().any(|s| s.outcome.backs_a_claim());
    let anything_was_checked = anything_worked;

    // 2. Claiming completion with nothing observed to have worked.
    if !anything_worked && !hedged {
        if let Some(word) = CLAIMS_DONE.iter().find(|w| asserts(draft, w)) {
            faults.push(Fault::ClaimedWithoutEvidence { word: (*word).into() });
        }
    }

    // 3. Claiming verification when nothing verified anything.
    if !anything_was_checked && !hedged {
        if let Some(word) = CLAIMS_CHECKED.iter().find(|w| asserts(draft, w)) {
            faults.push(Fault::SaidCheckedWhenItWasNot { word: (*word).into() });
        }
    }

    // 4. Everything went wrong and it still reads well.
    let all_bad = steps.iter().all(|s| s.outcome.went_wrong());
    if all_bad && faults.is_empty() && CLAIMS_DONE.iter().any(|w| asserts(draft, w)) {
        faults.push(Fault::NothingWorkedButItReadsFine);
    }

    faults
}

/// Put the problem first, keeping the rest of what was written.
///
/// A rewrite rather than a refusal. Refusing to speak would leave you with
/// nothing, and the draft is usually right about everything except the order.
pub fn lead_with_the_problem(steps: &[Step], draft: &str) -> String {
    let wrong: Vec<&Step> = steps.iter().filter(|s| s.outcome.went_wrong()).collect();
    if wrong.is_empty() {
        return draft.to_string();
    }
    let opener = match wrong.as_slice() {
        [one] => format!("{} — {}.", one.what, one.outcome.plain()),
        many => format!(
            "{} things didn't work: {}.",
            many.len(),
            many.iter()
                .map(|s| s.what.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    };
    format!("{opener} {draft}")
}
