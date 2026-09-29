//! Fixing without papering over, and asking someone who doesn't read code.

use atlas::mend::*;

fn change(theory: &str, added: &[&str], removed: &[&str]) -> Proposed {
    Proposed {
        theory: theory.into(),
        added: added.iter().map(|s| s.to_string()).collect(),
        removed: removed.iter().map(|s| s.to_string()).collect(),
    }
}

// --- refusing the cheap way out ---------------------------------------------

#[test]
fn deleting_the_failing_test_is_not_a_fix() {
    // It removes the thing that noticed, not the thing that was wrong.
    // The attribute is assembled rather than written literally: a test file
    // containing it in a string truncates its own body for any scanner reading
    // these files, including tests/retrospective.rs.
    let attr = concat!("#[", "test]");
    let p = change("the test is wrong", &[], &[attr, "assert_eq!(a, b);"]);
    assert_eq!(paper_overs(&p), vec![Cheat::DeletedTheTest]);
}

#[test]
fn moving_a_test_is_not_deleting_it() {
    // A change that removes and re-adds a test is a refactor, not a cheat.
    let attr = concat!("#[", "test]");
    let p = change("moved it", &[attr, "assert_eq!(a, b);"], &[attr, "assert_eq!(a, b);"]);
    assert!(!paper_overs(&p).contains(&Cheat::DeletedTheTest));
}

#[test]
fn silencing_the_warning_is_not_a_fix() {
    for line in [concat!("#[", "allow(dead_code)]"), "# type: ignore", "x = 1  # noqa"] {
        let p = change("t", &[line], &[]);
        assert!(
            paper_overs(&p).contains(&Cheat::SilencedIt),
            "{line} got through"
        );
    }
}

#[test]
fn widening_a_type_until_it_stops_complaining_is_not_a_fix() {
    // A type that accepts anything stops the compiler helping anywhere it's
    // used, which is a cost paid far from where the shortcut was taken.
    let p = change("t", &["def handle(x: Any) -> Any:"], &[]);
    assert!(paper_overs(&p).contains(&Cheat::MadeItAnything));
}

#[test]
fn swallowing_the_error_is_not_a_fix() {
    for line in ["except: pass", "except Exception: pass", "let _ = risky();"] {
        assert!(
            paper_overs(&change("t", &[line], &[])).contains(&Cheat::SwallowedIt),
            "{line} got through"
        );
    }
}

#[test]
fn turning_a_failure_into_a_zero_is_not_a_fix() {
    // The whole hollow-answer problem, arriving as a proposed fix.
    let p = change("t", &["let n = read().unwrap_or(0);"], &[]);
    assert!(paper_overs(&p).contains(&Cheat::DefaultedIt));
}

#[test]
fn skipping_the_test_is_not_a_fix() {
    for line in [concat!("#[", "ignore]"), "@pytest.mark.skip", "@unittest.skip"] {
        assert!(paper_overs(&change("t", &[line], &[])).contains(&Cheat::SkippedIt));
    }
}

#[test]
fn a_real_fix_is_allowed_through() {
    // The check must not block ordinary work, or it gets switched off.
    let p = change(
        "the index was off by one",
        &["for i in 0..items.len() {", "    total += items[i].size;"],
        &["for i in 0..=items.len() {"],
    );
    assert!(paper_overs(&p).is_empty(), "{:?}", paper_overs(&p));
}

#[test]
fn every_shortcut_can_say_why_it_is_not_a_fix() {
    for c in [
        Cheat::DeletedTheTest,
        Cheat::SilencedIt,
        Cheat::MadeItAnything,
        Cheat::SwallowedIt,
        Cheat::DefaultedIt,
        Cheat::SkippedIt,
    ] {
        assert!(c.why_not().len() > 25, "{c:?} explains nothing");
    }
}

#[test]
fn a_refusal_carries_the_reason_belonging_to_the_shortcut_it_found() {
    // Computed rather than compared against fixed wording: the property worth
    // holding is that the explanation matches the shortcut, not that a
    // particular sentence appears.
    for c in [Cheat::DeletedTheTest, Cheat::SilencedIt, Cheat::DefaultedIt] {
        let said = refusal(&[c]);
        assert!(
            said.contains(c.why_not()),
            "{c:?} was refused with someone else's reason: {said}"
        );
        assert!(
            !said.contains("Cheat") && !said.contains("::"),
            "a type name reached you: {said}"
        );
    }
}

#[test]
fn several_shortcuts_are_counted_rather_than_listed_one_by_one() {
    let two = refusal(&[Cheat::SilencedIt, Cheat::DefaultedIt]);
    let three = refusal(&[Cheat::SilencedIt, Cheat::DefaultedIt, Cheat::SkippedIt]);
    assert!(two.contains('2') && three.contains('3'));
    assert!(refusal(&[]).is_empty(), "nothing wrong produced a refusal");
}

// --- asking someone who doesn't read code -----------------------------------

fn good_question() -> Question {
    Question {
        doing: "I'm sorting your notes by when you last changed them.".into(),
        asks: "Some of them don't have a date I can read. What should happen to those?".into(),
        options: vec![
            "put them at the end, and tell you which ones".into(),
            "leave them out until you look at them".into(),
        ],
        set_aside: "the sorting".into(),
    }
}

#[test]
fn a_good_question_needs_no_code_to_answer() {
    // The rule the whole module turns on.
    assert!(good_question().answerable_without_code().is_ok());
}

#[test]
fn a_question_full_of_jargon_is_sent_back() {
    let q = Question {
        doing: "The parse function returns None".into(),
        asks: "Should the return value be an Option or should it panic?".into(),
        options: vec!["return None".into(), "raise an exception".into()],
        set_aside: "the parsing".into(),
    };
    let missed = q.answerable_without_code().unwrap_err();
    assert!(!missed.is_empty(), "jargon got through");
    assert!(missed.iter().any(|w| w == "function" || w == "return value" || w == "exception"));
}

#[test]
fn file_paths_and_error_codes_count_as_code() {
    let q = Question {
        doing: "src/index.rs is failing".into(),
        asks: "what should it do".into(),
        options: vec!["a".into(), "b".into()],
        set_aside: "that file".into(),
    };
    assert!(q.answerable_without_code().is_err());
}

#[test]
fn one_option_is_an_announcement_not_a_question() {
    let mut q = good_question();
    q.options = vec!["put them at the end".into()];
    assert!(!q.is_a_real_choice());
}

#[test]
fn four_options_is_asking_you_to_design_it() {
    let mut q = good_question();
    q.options = vec!["a".into(), "b".into(), "c".into(), "d".into()];
    assert!(!q.is_a_real_choice());
}

#[test]
fn two_or_three_options_is_a_decision_you_can_make() {
    let mut q = good_question();
    assert!(q.is_a_real_choice());
    q.options.push("ask you each time".into());
    assert!(q.is_a_real_choice());
}

#[test]
fn nothing_is_guessed_when_you_dont_answer() {
    // An earlier version had a default action, and that was wrong: a design
    // decision guessed at is a confident wrong answer with everything else
    // built on top of it. The whole point of asking is that Atlas doesn't know.
    let q = good_question();
    assert!(!q.set_aside.trim().is_empty(), "nothing was named as set aside");
    let said = q.spoken("Eric");
    assert!(said.contains("carry on with the rest"), "got: {said}");
    assert!(
        !said.contains("I'll put") && !said.contains("by default"),
        "it still proposes a default: {said}"
    );
}

#[test]
fn it_says_who_it_is_talking_to_and_why_it_stopped() {
    // Leading with the question sounds like curiosity. Leading with "I found
    // something and I can't choose" says why you're being interrupted.
    let said = good_question().spoken("Eric");
    assert!(said.starts_with("Eric,"), "got: {said}");
    assert!(said.contains("found a problem"));
    assert!(said.contains("decide"));
}

#[test]
fn only_one_thing_is_set_aside_and_the_rest_carries_on() {
    // Silence costs one parked task, not a night.
    let said = good_question().spoken("Eric");
    assert!(said.contains("leave the sorting"));
    assert!(said.contains("rest"));
}

#[test]
fn the_question_becomes_an_item_on_your_list() {
    use atlas::backlog::Blocker;
    match good_question().parked() {
        Blocker::NeedsYourDecision { question, set_aside } => {
            assert!(question.contains("What should happen"));
            assert_eq!(set_aside, "the sorting");
        }
        other => panic!("it didn't park: {other:?}"),
    }
}

#[test]
fn being_back_at_the_machine_is_not_a_decision() {
    // The difference between this and needing approval. Approval clears when
    // you turn up; this clears only when you choose.
    use atlas::backlog::{Backlog, Blocker, Conditions, Item};
    let b = Backlog::default();
    let item = Item {
        id: 1,
        request: "sort the notes".into(),
        blocker: good_question().parked(),
        first_seen: 0,
        last_offered: 0,
        offers: 0,
        dismissed: false,
        done: false,
    };
    let here = Conditions { online: true, screen_free: true, you_are_here: true, tools: vec![] };
    assert!(b.is_blocked(&item, &here), "your presence was read as an answer");
    assert!(!matches!(item.blocker, Blocker::NeedsApproval));
}

#[test]
fn a_parked_decision_reads_as_work_waiting_not_as_a_quiz() {
    // A list of these should read as a list of work.
    let e = good_question().parked().explain();
    assert!(e.starts_with("the sorting"), "got: {e}");
    assert!(e.contains("waiting on you"));
}

// --- when to ask at all -----------------------------------------------------

#[test]
fn mechanical_failures_are_never_put_to_you() {
    // Asking about every missing import is how an assistant becomes something
    // you stop reading.
    assert!(!should_ask(Kind::Mechanical, 0));
    assert!(!should_ask(Kind::Mechanical, 9));
    assert!(!should_ask(Kind::Behavioural, 5));
}

#[test]
fn a_real_choice_is_put_to_you_immediately() {
    assert!(should_ask(Kind::Undecided, 0));
    assert!(should_ask(Kind::Environment, 0));
}

#[test]
fn not_understanding_it_earns_two_attempts_first() {
    // Plenty of unclear failures become clear once something has been tried.
    // Eight more attempts at something Atlas doesn't understand produces eight
    // more variations of not understanding it.
    assert!(!should_ask(Kind::Unclear, 0));
    assert!(!should_ask(Kind::Unclear, 1));
    assert!(should_ask(Kind::Unclear, 2));
}

#[test]
fn atlas_only_claims_to_fix_what_it_can() {
    assert!(Kind::Mechanical.atlas_can_fix());
    assert!(Kind::Behavioural.atlas_can_fix());
    assert!(!Kind::Undecided.atlas_can_fix());
    assert!(!Kind::Environment.atlas_can_fix());
    assert!(!Kind::Unclear.atlas_can_fix());
}

#[test]
fn every_kind_that_cannot_be_fixed_alone_needs_you() {
    for k in [Kind::Undecided, Kind::Environment, Kind::Unclear] {
        assert!(k.needs_you(), "{k:?} would loop forever");
        assert!(!k.atlas_can_fix());
    }
}
