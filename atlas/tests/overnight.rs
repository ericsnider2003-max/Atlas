use atlas::handoff::Problem;
use atlas::daily::Whereabouts;
use atlas::overnight::{
    morning_brief, morning_detail, worth_doing_overnight, Brain, Outcome, OvernightConfig, Result_,
    Session, Step,
};

fn cfg() -> OvernightConfig {
    OvernightConfig { enabled: true, ..Default::default() }
}

fn result(problem: &str, outcome: Outcome) -> Result_ {
    Result_ {
        problem: problem.into(),
        outcome,
        attempts: 2,
        sandbox_path: (outcome == Outcome::Solved).then(|| "data/sandbox/fix-1".into()),
        tests_passed: (outcome == Outcome::Solved).then_some(907),
        note: "found the cached handle".into(),
        dollars: 0.03,
    }
}

// ================= nothing is applied while you sleep =================

#[test]
fn nothing_is_ever_applied_unattended_and_that_is_not_configurable() {
    // The whole arrangement rests on this. Waking up to changed files you
    // never saw is the thing that would make overnight work unusable.
    assert!(!OvernightConfig::default().apply_while_asleep);
    let yaml = "enabled: true\napply_while_asleep: true\n";
    let parsed: OvernightConfig = serde_yaml::from_str(yaml).unwrap();
    assert!(!parsed.apply_while_asleep, "it cannot be switched on from config");
}

#[test]
fn the_morning_brief_says_plainly_that_nothing_changed() {
    let mut s = Session::start(0);
    s.record(result("wake word after a device change", Outcome::Solved));
    let said = morning_brief(&s, Whereabouts::Asleep);
    assert!(said.contains("Nothing's been applied"), "got: {said}");
    assert!(morning_detail(&s).contains("Nothing has been applied to your machine."));
}

// ================= where the answers come from doesn't matter =================

#[test]
fn a_session_works_the_same_whichever_brain_it_is_given() {
    // Swapping local for hosted, or hosted for "leave it for me", changes one
    // line of config and nothing else.
    for brain in [Brain::Local, Brain::Hosted, Brain::AskYouLater] {
        let c = OvernightConfig { brain, ..cfg() };
        let s = Session::start(0);
        assert_eq!(s.next(&["a problem".into()], &c, Whereabouts::Asleep, 10.0), Step::Work("a problem".into()));
    }
}

#[test]
fn only_a_paid_brain_can_run_out_of_budget() {
    let s = Session::start(0);
    let free = OvernightConfig { brain: Brain::Local, ..cfg() };
    assert_eq!(s.next(&["x".into()], &free, Whereabouts::Asleep, 0.0), Step::Work("x".into()));

    let paid = OvernightConfig { brain: Brain::Hosted, ..cfg() };
    assert!(matches!(s.next(&["x".into()], &paid, Whereabouts::Asleep, 0.0), Step::Finish(_)));
}

// ================= knowing when to stop =================

#[test]
fn work_happens_while_you_are_gone_rather_than_between_two_hours() {
    // `in_window(23)` / `in_window(9)` was the gate and is gone. A clock
    // cannot tell you asleep from you at a desk at two in the morning, and
    // it has no idea you left for work at eight -- so the night ran while
    // you were up and refused to run on the Saturday you were out all day.
    //
    // Both kinds of gone mean Atlas may get on with something; they are kept
    // apart because `Out` can end at any moment and `Asleep` cannot.
    assert!(Whereabouts::Asleep.free_to_work());
    assert!(Whereabouts::Out.free_to_work());
    assert!(!Whereabouts::Here.free_to_work());
}

#[test]
fn it_stops_when_you_come_back() {
    // Not "when the window closes". The step that finds you at the machine
    // is the one that ends the stretch, which is also why `next` takes the
    // same answer the caller used to start it -- an hour here and a
    // `Whereabouts` there is how two halves of one decision disagree.
    let s = Session::start(0);
    match s.next(&["x".into()], &cfg(), Whereabouts::Here, 10.0) {
        Step::Finish(why) => assert!(why.contains("came back"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn several_failures_in_a_row_stop_the_night_rather_than_burning_it() {
    // Three in a row means the setup is wrong, not that the problems are
    // hard. Carrying on wastes the night and the money.
    let mut s = Session::start(0);
    for i in 0..3 {
        s.record(result(&format!("problem {i}"), Outcome::Stuck));
    }
    match s.next(&["another".into()], &cfg(), Whereabouts::Asleep, 10.0) {
        Step::Abandon(why) => assert!(why.contains("3 in a row"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_success_resets_the_patience() {
    let mut s = Session::start(0);
    s.record(result("a", Outcome::Stuck));
    s.record(result("b", Outcome::Stuck));
    s.record(result("c", Outcome::Solved));
    s.record(result("d", Outcome::Stuck));
    assert!(matches!(s.next(&["e".into()], &cfg(), Whereabouts::Asleep, 10.0), Step::Work(_)), "not abandoned");
}

#[test]
fn it_does_not_work_through_the_same_problem_twice() {
    let mut s = Session::start(0);
    s.record(result("first", Outcome::Solved));
    assert_eq!(
        s.next(&["first".into(), "second".into()], &cfg(), Whereabouts::Asleep, 10.0),
        Step::Work("second".into())
    );
}

#[test]
fn an_empty_queue_finishes_rather_than_inventing_work() {
    let s = Session::start(0);
    match s.next(&[], &cfg(), Whereabouts::Asleep, 10.0) {
        Step::Finish(why) => assert!(why.contains("worked through everything")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn there_is_a_ceiling_on_how_much_it_takes_on_in_one_night() {
    let c = OvernightConfig { max_problems: 2, ..cfg() };
    let mut s = Session::start(0);
    s.record(result("a", Outcome::Solved));
    s.record(result("b", Outcome::Solved));
    assert!(matches!(s.next(&["c".into()], &c, Whereabouts::Asleep, 10.0), Step::Finish(_)));
}

// ================= what it takes on =================

#[test]
fn anything_needing_a_judgement_call_is_left_for_you() {
    for goal in [
        "decide which layout is better",
        "should I use the smaller model",
        "delete the old captures",
        "post the draft to LinkedIn",
    ] {
        let p = Problem { goal: goal.into(), ..Default::default() };
        assert!(!worth_doing_overnight(&p), "{goal:?} needs you");
    }
}

#[test]
fn a_plain_broken_thing_is_fair_game() {
    let p = Problem {
        goal: "the wake word stops working after the headset reconnects".into(),
        ..Default::default()
    };
    assert!(worth_doing_overnight(&p));
}

// ================= the morning =================

#[test]
fn the_brief_leads_with_what_needs_a_decision() {
    let mut s = Session::start(0);
    s.record(result("a fix", Outcome::Solved));
    s.record(result("a choice", Outcome::NeedsYou));
    let said = morning_brief(&s, Whereabouts::Asleep);
    assert!(said.contains("needs a decision"));
    assert!(said.contains("a choice"));
}

#[test]
fn a_pile_of_fixes_is_summarised_rather_than_listed() {
    let mut s = Session::start(0);
    for i in 0..8 {
        s.record(result(&format!("fix {i}"), Outcome::Solved));
    }
    let said = morning_brief(&s, Whereabouts::Asleep);
    assert!(said.contains("8 fixed"));
    assert!(said.contains("and 5 more"), "got: {said}");
    assert!(said.len() < 260, "still speakable: {said}");
}

#[test]
fn what_it_cost_is_in_the_brief_when_it_cost_anything() {
    let mut s = Session::start(0);
    s.record(result("a", Outcome::Solved));
    assert!(morning_brief(&s, Whereabouts::Asleep).contains("cost $0.03"));

    let mut free = Session::start(0);
    free.record(Result_ { dollars: 0.0, ..result("b", Outcome::Solved) });
    assert!(!morning_brief(&free, Whereabouts::Asleep).contains("cost"), "nothing to mention when it's free");
}

#[test]
fn a_quiet_night_says_so() {
    assert!(morning_brief(&Session::start(0), Whereabouts::Asleep).contains("didn't get to anything"));
}

#[test]
fn the_detailed_account_says_where_each_change_is_waiting() {
    let mut s = Session::start(0);
    s.record(result("the wake word", Outcome::Solved));
    let d = morning_detail(&s);
    assert!(d.contains("[fixed] the wake word"));
    assert!(d.contains("907 tests pass"));
    assert!(d.contains("waiting in data/sandbox/fix-1"));
}

#[test]
fn it_is_on_and_defaults_to_the_free_brain() {
    // Off until 18 Sep 2026, when the night was actually wired to the tick.
    // "Off by default" was the right answer while nothing ran -- shipping a
    // switch that does nothing is worse than shipping no switch. Now that it
    // runs, what matters is what it costs, and under `ask_you_later` a night
    // spends nothing, reaches nothing outside the machine, and cannot apply
    // anything: `apply_while_asleep` is wired shut rather than defaulted off.
    //
    // The brain is the part still worth pinning. Any other value spends money
    // or drives another application, and neither should arrive by default.
    let c = OvernightConfig::default();
    assert!(c.enabled, "the night was wired and should ship on");
    assert_eq!(c.brain, Brain::AskYouLater, "costs nothing unless you choose otherwise");
    assert!(!c.apply_while_asleep, "a night must never apply anything");
}

// ================= delegating the conversation =================

fn delegating() -> OvernightConfig {
    OvernightConfig {
        brain: Brain::Delegate,
        delegate_window: "Claude — Atlas Project".into(),
        ..cfg()
    }
}

#[test]
fn a_delegated_continuation_is_bounded_by_turns_not_by_your_presence() {
    // Same mechanism as stepping out of the room mid-conversation. The turn
    // budget is the bound.
    let s = Session::start(0);
    let d = s.delegation_for("the wake word drops after a reconnect", &delegating()).unwrap();
    assert_eq!(d.app, "Claude — Atlas Project");
    assert_eq!(d.max_turns, 6);
    assert!(!d.stop_when.is_empty(), "it stops when the answer arrives");
}

#[test]
fn the_nights_total_turn_budget_is_enforced_across_problems() {
    let c = OvernightConfig { delegate_turns_total: 10, ..delegating() };
    let mut s = Session::start(0);
    s.spend_turns(6);
    let d = s.delegation_for("another problem", &c).unwrap();
    assert_eq!(d.max_turns, 4, "only what's left of the night");

    s.spend_turns(4);
    assert!(s.delegation_for("a third", &c).is_none(), "budget gone");
}

#[test]
fn running_out_of_turns_finishes_the_night_cleanly() {
    let c = OvernightConfig { delegate_turns_total: 5, ..delegating() };
    let mut s = Session::start(0);
    s.spend_turns(5);
    match s.next(&["x".into()], &c, Whereabouts::Asleep, 0.0) {
        Step::Finish(why) => assert!(why.contains("5 turns you allowed"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn with_no_window_named_it_stops_rather_than_finding_one() {
    // It continues a conversation that already exists. It does not go looking
    // for somewhere to start one.
    let c = OvernightConfig { delegate_window: String::new(), ..delegating() };
    let s = Session::start(0);
    assert!(matches!(s.next(&["x".into()], &c, Whereabouts::Asleep, 0.0), Step::Abandon(_)));
    assert!(s.delegation_for("x", &c).is_none());
}

#[test]
fn delegating_costs_nothing_and_needs_no_budget() {
    let s = Session::start(0);
    assert_eq!(s.next(&["x".into()], &delegating(), Whereabouts::Asleep, 0.0), Step::Work("x".into()));
}

#[test]
fn a_delegated_night_still_applies_nothing_while_you_sleep() {
    assert!(!delegating().apply_while_asleep);
}
