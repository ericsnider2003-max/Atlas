//! Progress is disposable. The result is not.

use atlas::channel::{Leak, Run};
use atlas::faithful::Step;

fn run(updates: &[&str]) -> Run {
    let mut r = Run::default();
    for u in updates {
        r.update(u);
    }
    r
}

#[test]
fn a_result_that_points_back_at_an_update_is_flagged() {
    // You may have heard none of the updates. "As I mentioned" fails for
    // exactly the person who most needed the answer.
    let r = run(&["Checking the disk now."]);
    let leaks = r.check_result("As I mentioned, it is fine.");
    assert!(leaks.iter().any(|l| matches!(l, Leak::RefersToAnUpdate(_))));
}

#[test]
fn a_result_that_opens_mid_thought_is_flagged() {
    let r = run(&["Starting."]);
    assert!(r
        .check_result("So that is everything done.")
        .iter()
        .any(|l| matches!(l, Leak::StartsMidThought(_))));
}

#[test]
fn a_number_only_said_while_working_has_to_be_repeated() {
    let r = run(&["Found 14 stale files."]);
    let leaks = r.check_result("Cleaned them up.");
    assert!(leaks
        .iter()
        .any(|l| matches!(l, Leak::OnlyMentionedInProgress(n) if n == "14")));
}

#[test]
fn repeating_the_number_clears_it() {
    let r = run(&["Found 14 stale files."]);
    assert!(r.check_result("Cleaned up 14 stale files.").is_empty());
}

#[test]
fn progress_itself_is_never_checked() {
    // Updates are allowed to be partial, chatty, and wrong in hindsight. That
    // is what they are for.
    let mut r = Run::default();
    r.update("So, as I said, still going...");
    assert_eq!(r.updates.len(), 1);
}

#[test]
fn ordinary_words_are_not_treated_as_things_you_missed() {
    // Flagging common words would make this noise, and a noisy check gets
    // switched off.
    let r = run(&["Having a look at the folder now."]);
    assert!(r.check_result("All tidy.").is_empty());
}

#[test]
fn closing_puts_a_failure_first_and_then_checks_the_wording() {
    let r = run(&["Backing up."]);
    let steps = vec![Step::did("tidied"), Step::failed("backup", "disk full")];
    let (said, leaks) = r.close(&steps, "Tidied the desktop.");
    assert!(said.starts_with("backup"), "the failure was not put first: {said}");
    assert!(leaks.iter().all(|l| !matches!(l, Leak::RefersToAnUpdate(_))));
}

#[test]
fn every_leak_explains_itself() {
    for l in [
        Leak::RefersToAnUpdate("as i said".into()),
        Leak::OnlyMentionedInProgress("14".into()),
        Leak::StartsMidThought("so".into()),
    ] {
        assert!(l.plain().len() > 20, "{l:?} has no useful explanation");
    }
}
