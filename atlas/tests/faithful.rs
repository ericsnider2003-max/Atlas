//! A report has to be true, not just fluent.

use atlas::faithful::*;

fn ran(steps: Vec<Step>, draft: &str) -> Vec<Fault> {
    check(&steps, draft)
}

// --- burying a failure ------------------------------------------------------

#[test]
fn a_failure_mentioned_at_the_end_is_still_buried() {
    // Position, not presence. A failure in the last sentence of a cheerful
    // paragraph has been disclosed and not communicated.
    let faults = ran(
        vec![Step::did("tidied the desktop"), Step::failed("backup", "disk full")],
        "Tidied the desktop and everything looks good. The backup didn't work.",
    );
    assert!(matches!(faults[0], Fault::BuriedTheProblem { .. }));
}

#[test]
fn naming_the_failed_step_first_is_enough() {
    let faults = ran(
        vec![Step::did("tidied the desktop"), Step::failed("backup", "disk full")],
        "The backup couldn't run, disk is full. I tidied the desktop.",
    );
    assert!(faults.is_empty(), "{faults:?}");
}

#[test]
fn a_skipped_step_counts_as_something_gone_wrong() {
    let faults = ran(
        vec![Step::skipped("update", "you were mid-call")],
        "All quiet. Nothing needed doing.",
    );
    assert!(!faults.is_empty());
}

#[test]
fn a_clean_run_needs_no_apology() {
    assert!(ran(vec![Step::did("backed up"), Step::did("tidied")], "Backed up and tidied.").is_empty());
}

// --- claiming more than happened -------------------------------------------

#[test]
fn saying_saved_when_nothing_saved_is_a_fault() {
    let faults = ran(
        vec![Step::failed("write the note", "read-only folder")],
        "Saved your note.",
    );
    assert!(faults
        .iter()
        .any(|f| matches!(f, Fault::ClaimedWithoutEvidence { .. })));
}

#[test]
fn ran_but_unchecked_does_not_back_a_claim() {
    // The dangerous outcome: from inside the run it feels identical to
    // success. A file written and never read back is not a file saved.
    let faults = ran(vec![Step::not_checked("wrote the file")], "Saved it.");
    assert!(faults
        .iter()
        .any(|f| matches!(f, Fault::ClaimedWithoutEvidence { .. })));
}

#[test]
fn hedging_honestly_is_not_a_fault() {
    // "I tried to save it" is not a claim that it saved. Without this the
    // module would push Atlas toward saying less rather than saying true
    // things.
    assert!(ran(
        vec![Step::failed("write the note", "read-only folder")],
        "I couldn't save your note — the folder is read-only."
    )
    .iter()
    .all(|f| !matches!(f, Fault::ClaimedWithoutEvidence { .. })));
}

#[test]
fn claims_are_matched_as_whole_words() {
    // "done" is inside "abandoned"; "sent" is inside "presented".
    let steps = vec![Step::not_checked("something")];
    assert!(ran(steps.clone(), "The old approach was abandoned.").is_empty());
    assert!(ran(steps, "The options were presented.").is_empty());
}

// --- claiming verification --------------------------------------------------

#[test]
fn saying_verified_when_nothing_was_checked() {
    let faults = ran(
        vec![Step::not_checked("copied the folder")],
        "Copied and verified the folder.",
    );
    assert!(faults
        .iter()
        .any(|f| matches!(f, Fault::SaidCheckedWhenItWasNot { .. })));
}

#[test]
fn verified_is_fine_when_something_was_actually_observed() {
    assert!(ran(vec![Step::did("read the file back")], "Checked it — the file reads back fine.").is_empty());
}

// --- everything failed ------------------------------------------------------

#[test]
fn a_wholly_failed_run_cannot_read_as_a_success() {
    let faults = ran(
        vec![
            Step::failed("backup", "disk full"),
            Step::failed("tidy", "disk full"),
        ],
        "The disk is full so nothing ran. All set for tomorrow.",
    );
    assert!(!faults.is_empty());
}

// --- rewriting --------------------------------------------------------------

#[test]
fn the_rewrite_puts_the_problem_first_and_keeps_the_rest() {
    // A rewrite rather than a refusal: refusing would leave you with nothing,
    // and the draft is usually right about everything except the order.
    let steps = vec![Step::did("tidied"), Step::failed("backup", "disk full")];
    let fixed = lead_with_the_problem(&steps, "Tidied the desktop.");
    assert!(fixed.starts_with("backup"));
    assert!(fixed.contains("disk full"));
    assert!(fixed.contains("Tidied the desktop."));
    assert!(check(&steps, &fixed).is_empty(), "the rewrite still fails its own check");
}

#[test]
fn a_clean_draft_is_returned_untouched() {
    let steps = vec![Step::did("backed up")];
    assert_eq!(lead_with_the_problem(&steps, "Backed up."), "Backed up.");
}

#[test]
fn several_failures_are_counted_rather_than_listed_one_by_one() {
    let steps = vec![
        Step::failed("backup", "disk full"),
        Step::failed("tidy", "disk full"),
        Step::skipped("update", "you were mid-call"),
    ];
    let fixed = lead_with_the_problem(&steps, "Otherwise quiet.");
    assert!(fixed.starts_with("3 things didn't work"));
}

// --- reporting from scratch -------------------------------------------------

#[test]
fn an_empty_run_says_so() {
    assert!(check(&[], "anything at all").is_empty());
}

// --- the outcomes themselves ------------------------------------------------

#[test]
fn only_an_observed_result_backs_a_claim() {
    assert!(Outcome::Did.backs_a_claim());
    assert!(!Outcome::NotChecked.backs_a_claim());
    assert!(!Outcome::Failed("x".into()).backs_a_claim());
    assert!(!Outcome::Skipped("x".into()).backs_a_claim());
}

#[test]
fn unchecked_is_not_counted_as_gone_wrong_either() {
    // It is its own thing. Treating it as a failure would make Atlas
    // apologetic about work that probably succeeded.
    assert!(!Outcome::NotChecked.went_wrong());
    assert!(Outcome::Failed("x".into()).went_wrong());
    assert!(Outcome::Skipped("x".into()).went_wrong());
}

#[test]
fn every_fault_explains_itself_in_words() {
    for f in [
        Fault::BuriedTheProblem { step: "backup".into() },
        Fault::ClaimedWithoutEvidence { word: "saved".into() },
        Fault::SaidCheckedWhenItWasNot { word: "verified".into() },
        Fault::NothingWorkedButItReadsFine,
    ] {
        assert!(f.plain().len() > 20, "{f:?} has no useful explanation");
    }
}

#[test]
fn a_denied_claim_is_not_a_claim() {
    // "nothing checked" contains "checked" and asserts the opposite. The
    // module flagged its own output before this existed.
    let steps = vec![Step::not_checked("copied the folder")];
    for honest in [
        "Copied it, but nothing checked the result.",
        "Not saved yet.",
        "I couldn't verify that.",
        "Nothing was confirmed.",
        "Never got it sent.",
    ] {
        assert!(
            check(&steps, honest).is_empty(),
            "honest wording was flagged: {honest:?}"
        );
    }
}

#[test]
fn negation_two_words_back_still_counts() {
    let steps = vec![Step::not_checked("x")];
    assert!(check(&steps, "Nothing was checked.").is_empty());
    assert!(check(&steps, "Not yet saved.").is_empty());
}

#[test]
fn an_actual_claim_is_still_caught_after_the_negation_fix() {
    // The fix must not turn the whole check off.
    let steps = vec![Step::not_checked("x")];
    assert!(!check(&steps, "Saved and verified.").is_empty());
}
