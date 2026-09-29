//! Working a decision instead of answering it.

use atlas::decide::*;

/// Words that name a winner. Checked against the laid-out text, because the
/// refusal is the product: an assistant that says "I'd go with the second one"
/// has decided, whatever it adds afterwards about it being your call.
///
/// Lived in `src/decide.rs` as `names_a_winner` until 25 Sep 2026; nothing in
/// production called it, so it moved here, the only place that asks.
fn names_a_winner(text: &str) -> bool {
    const RECOMMENDS: &[&str] = &[
        "i'd go with", "i would go with", "i recommend", "my recommendation",
        "the best option", "the right choice", "you should pick", "you should choose",
        "go with the", "the winner", "clearly better", "obviously better",
        "i'd pick", "i would pick", "the answer is", "definitely the",
    ];
    let lower = text.to_lowercase();
    RECOMMENDS.iter().any(|r| lower.contains(r))
}

fn opt(what: &str, costs: &str, rests: &[&str]) -> Option_ {
    Option_ {
        what: what.into(),
        costs: costs.into(),
        rests_on: rests.iter().map(|s| s.to_string()).collect(),
        set_aside: None,
    }
}

fn worked() -> Decision {
    let mut d = Decision::new("which broker", Weight::WorthWorking);
    d.real_question = Some("what am I actually optimising for".into());
    d.boring = Some(("stay where I am".into(), "fees would have to differ by more than the switch costs".into()));
    d.options = vec![
        opt("stay", "nothing changes", &["current fees hold"]),
        opt("move", "a week of setup", &["the quoted fees are real"]),
    ];
    d.against = vec!["switching costs are always underestimated".into()];
    d.if_wrong = Some("a week lost, reversible".into());
    d.confidence = Some((6, "I haven't read the fee schedule".into()));
    d.open_questions = vec!["what does the transfer actually cost".into()];
    d
}

// --- the refusal ------------------------------------------------------------

#[test]
fn laying_out_the_field_still_names_no_winner() {
    // `laid_out` is the survey. Recommending is a separate, explicit act via
    // `lean`, so the survey itself stays neutral and you can read it without
    // being steered.
    assert!(!names_a_winner(&worked().laid_out()));
}

// --- recommending, with the working shown ----------------------------------

#[test]
fn a_worked_decision_will_recommend() {
    let l = worked().lean().expect("a fully worked decision should lean");
    assert!(!l.option.is_empty());
    assert!(!l.because.is_empty(), "an unexplained lean is the thing being avoided");
    assert!(!l.would_change_it.is_empty());
}

#[test]
fn it_will_not_lean_before_the_options_are_costed() {
    // Otherwise the cheap-looking one wins by not having been examined.
    let mut d = Decision::new("x", Weight::WorthWorking);
    d.real_question = Some("y".into());
    assert!(matches!(d.lean(), Err(CannotLean::StillWorking(_))));
}

#[test]
fn it_will_not_lean_with_nothing_argued_against_it() {
    // A lean nobody attacked is a preference wearing evidence.
    let mut d = worked();
    d.against.clear();
    assert!(matches!(d.lean(), Err(CannotLean::StillWorking(Move::Reversal))));
}

#[test]
fn it_asks_rather_than_guessing_what_you_want() {
    // Reasoning from a guessed preference is confidently wrong in a way you
    // cannot see, because the guess never appears in the working.
    let mut d = worked();
    d.needs_from_you = vec!["how much is a week of your time worth here?".into()];
    match d.lean() {
        Err(CannotLean::NeedsSomethingFromYou(q)) => assert!(q.contains("worth")),
        other => panic!("it guessed instead of asking: {other:?}"),
    }
    // Contrast: nothing missing means it doesn't ask anything at all.
    let complete = worked();
    assert!(complete.needs_from_you.is_empty());
    assert!(complete.lean().is_ok(), "nothing to ask about should just lean");
}

#[test]
fn an_option_that_did_not_hold_up_is_set_aside_not_deleted() {
    // Quietly dropping it means the same idea comes back in a month looking
    // new.
    let mut d = worked();
    d.set_aside("move", "the quoted fees turned out to be introductory");
    assert_eq!(d.standing().len(), 1);
    assert_eq!(d.options.len(), 2, "it was deleted rather than set aside");
    assert!(matches!(d.lean(), Err(CannotLean::NothingLeftToCompare)));
}

#[test]
fn a_lean_carries_what_was_set_aside_and_why() {
    let mut d = worked();
    d.options.push(opt("third way", "unknown", &["nothing"]));
    d.set_aside("third way", "needs an account you don't have");
    let l = d.lean().expect("two still standing");
    assert!(l.because.iter().any(|b| b.contains("set aside")));
}

#[test]
fn every_refusal_to_lean_says_what_it_needs() {
    // Length is the wrong bar here. "Which matters more?" is nineteen
    // characters and a perfectly good question; what matters is that a
    // refusal hands back something answerable rather than a shrug.
    for c in [
        CannotLean::StillWorking(Move::Stakes),
        CannotLean::NothingLeftToCompare,
        CannotLean::NeedsSomethingFromYou("Which matters more?".into()),
    ] {
        let said = c.plain();
        assert!(!said.trim().is_empty(), "{c:?} explains nothing");
        let answerable = said.ends_with('?') || said.contains(' ');
        assert!(answerable, "{c:?} gives you nothing to act on: {said}");
    }
}

// --- order ------------------------------------------------------------------

#[test]
fn framing_comes_before_options() {
    // Most bad decisions are right answers to the wrong question. Comparing
    // first gets you a well-compared answer to it.
    let d = Decision::new("x", Weight::WorthWorking);
    assert_eq!(d.next_move(), Some(Move::Framing));
}

#[test]
fn the_boring_answer_is_asked_for_before_the_clever_ones() {
    let mut d = Decision::new("x", Weight::WorthWorking);
    d.real_question = Some("y".into());
    assert_eq!(d.next_move(), Some(Move::Default));
}

#[test]
fn evidence_is_asked_for_before_the_case_against() {
    let mut d = Decision::new("x", Weight::WorthWorking);
    d.real_question = Some("y".into());
    d.boring = Some(("a".into(), "b".into()));
    d.options = vec![opt("one", "c", &[]), opt("two", "d", &["something"])];
    assert_eq!(d.next_move(), Some(Move::Evidence));
}

#[test]
fn a_worked_decision_has_nothing_left() {
    assert!(worked().complete());
    assert_eq!(worked().next_move(), None);
    assert!(worked().gaps().is_empty());
}

#[test]
fn a_half_worked_decision_reports_every_gap_not_just_the_first() {
    // Otherwise you fix one thing, ask again, and fix one more.
    let mut d = Decision::new("x", Weight::WorthWorking);
    d.real_question = Some("y".into());
    let gaps = d.gaps();
    assert!(gaps.len() >= 5, "only found {gaps:?}");
    assert!(gaps.contains(&Move::Stakes));
    assert!(gaps.contains(&Move::Ownership));
    assert!(!gaps.contains(&Move::Framing));
}

// --- weight -----------------------------------------------------------------

#[test]
fn a_cheap_reversible_decision_is_not_worked_at_all() {
    // Working it costs more than getting it wrong. Saying so is the useful
    // answer.
    let d = Decision::new("which mug", Weight::JustPick);
    assert!(!d.worth_working());
    assert_eq!(d.next_move(), None);
    assert!(d.laid_out().contains("Pick one and move on"));
    assert!(d.gaps().is_empty());
}

#[test]
fn a_one_way_decision_says_so_at_the_end() {
    let mut d = worked();
    d.weight = Weight::OneWay;
    assert!(d.laid_out().contains("can't be undone"));
    assert!(!names_a_winner(&d.laid_out()));
}

// --- what gets said ---------------------------------------------------------

#[test]
fn every_option_carries_its_cost() {
    // An option with no stated cost wins by default against options that were
    // honest about theirs.
    let out = worked().laid_out();
    assert!(out.contains("costs you nothing changes"));
    assert!(out.contains("costs you a week of setup"));
}

#[test]
fn an_option_resting_on_nothing_says_so_rather_than_going_blank() {
    let mut d = worked();
    d.options[0].rests_on.clear();
    assert!(d.laid_out().contains("nothing stated"));
}

#[test]
fn the_reframed_question_is_surfaced_only_when_it_changed() {
    let mut d = worked();
    assert!(d.laid_out().contains("The question underneath"));
    d.real_question = Some(d.about.clone());
    assert!(!d.laid_out().contains("The question underneath"));
}

#[test]
fn confidence_comes_with_what_is_holding_it_down() {
    // A number on its own is not information.
    let out = worked().laid_out();
    assert!(out.contains("6 out of 10"));
    assert!(out.contains("held down by"));
}

#[test]
fn the_open_questions_are_handed_back_to_you() {
    assert!(worked().laid_out().contains("Before you decide"));
}

#[test]
fn every_move_asks_something_a_person_would_say_out_loud() {
    for m in Move::all() {
        let q = m.asks();
        assert!(q.ends_with('?'), "{m:?} is not a question: {q}");
        assert!(q.len() > 25, "{m:?} is too terse to be useful");
    }
}

#[test]
fn the_moves_are_ordered_deliberately() {
    let all = Move::all();
    let pos = |m: Move| all.iter().position(|x| *x == m).unwrap();
    assert!(pos(Move::Framing) < pos(Move::Options));
    assert!(pos(Move::Options) < pos(Move::Evidence));
    assert!(pos(Move::Evidence) < pos(Move::Reversal));
    assert!(pos(Move::Stakes) > pos(Move::Options));
}

// --- voice first, then written ---------------------------------------------

#[test]
fn the_recommendation_is_said_plainly_on_its_own() {
    // A recommendation buried in its own working is one you have to dig for.
    let l = worked().lean().unwrap();
    let said = l.spoken();
    assert!(said.len() < 60, "too long to say: {said}");
    assert!(!said.contains("rests on"), "the reasoning leaked into the answer: {said}");
    assert!(!said.contains('\n'), "more than one line: {said}");
}

#[test]
fn the_reasoning_is_there_when_you_ask_for_it() {
    let l = worked().lean().unwrap();
    assert!(l.can_explain());
    let w = l.written();
    assert!(w.starts_with(&l.spoken()), "the answer should still come first");
    assert!(w.contains("rests on"));
    assert!(w.contains("What would change it"));
}

#[test]
fn a_lean_can_always_explain_itself() {
    // If this ever fails, an unexplained recommendation escaped.
    assert!(worked().lean().unwrap().can_explain());
}
