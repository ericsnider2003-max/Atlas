//! Recording other people on a call: ask, and wait for their yes.
//!
//! Eric, 24 Sep 2026: "call: yes notes, voices ask then record". Your own
//! side is noted without anyone's permission, since it captures only you.
//! Anyone else is asked first, and only a yes starts recording them. No
//! answer, or one no, means only your side.

use atlas::consent::{explain, ConsentConfig, Recorder, Scope, State, Step, QUESTION};

fn rec() -> Recorder {
    Recorder::new(ConsentConfig { enabled: true, ask_every_call: false, ..Default::default() })
}

#[test]
fn asking_is_what_ships() {
    let c = ConsentConfig::default();
    assert!(c.ask_the_others);
    assert_eq!(c.question, QUESTION);
    assert!(QUESTION.ends_with("until you've said yes."));
}

#[test]
fn the_call_is_asked_and_nobody_else_is_recorded_until_they_say_yes() {
    let mut c = rec();
    assert_eq!(c.call_started(Scope::Everyone), Step::Announce(QUESTION.to_string()));
    // The question landed: your side is noted while they answer.
    assert_eq!(c.announcement_delivered(), Step::Start(Scope::YouOnly));
    assert_eq!(c.state, State::Asking);
    assert!(!c.capturing_others());
    assert_eq!(c.they_agreed(), Step::Start(Scope::Everyone));
    assert!(c.capturing_others());
    assert!(!c.recorded_others_without_announcing());
}

#[test]
fn no_answer_is_not_a_yes() {
    let mut c = rec();
    c.call_started(Scope::Everyone);
    c.announcement_delivered();
    assert!(matches!(c.no_answer(), Step::AskYou(_)));
    assert_eq!(c.scope, Scope::YouOnly);
    assert!(!c.capturing_others());
    // A late "yes" after giving up doesn't quietly start recording them.
    assert_eq!(c.they_agreed(), Step::Nothing);
    assert!(!c.capturing_others());
}

#[test]
fn one_no_while_asking_means_only_your_side() {
    let mut c = rec();
    c.call_started(Scope::Everyone);
    c.announcement_delivered();
    assert!(matches!(c.someone_objected("Sam"), Step::AskYou(_)));
    assert_eq!(c.they_agreed(), Step::Nothing, "a no can't be outvoted");
    assert!(!c.capturing_others());
}

#[test]
fn a_yes_before_the_question_landed_counts_for_nothing() {
    let mut c = rec();
    c.call_started(Scope::Everyone);
    assert_eq!(c.they_agreed(), Step::Nothing);
    assert!(!c.capturing_others());
}

#[test]
fn a_call_ending_while_asking_keeps_your_side() {
    let mut c = rec();
    c.call_started(Scope::Everyone);
    c.announcement_delivered();
    assert_eq!(c.call_ended(), Step::StopAndKeep);
}

#[test]
fn it_says_it_asks() {
    let said = explain(&ConsentConfig { enabled: true, default_scope: Scope::Everyone, ..Default::default() });
    assert_eq!(said.matches("wait for a yes").count(), 1, "{said}");
    assert!(!said.contains("I tell everyone first"), "{said}");
}
