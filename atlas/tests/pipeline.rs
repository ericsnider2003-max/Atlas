use atlas::pipeline::{
    refinement_is_warranted, review, Build, Concern, Next, PipelineConfig, Refinement, Stage,
    Thought, Work, WHY_THE_STAGES,
};

fn good_thought() -> Thought {
    Thought {
        symptom: "screen capture fails on a fresh install".into(),
        cause: "the working folder is created by listen and by nothing else".into(),
        where_: "src/voice.rs".into(),
        proof: "capture_works_on_a_fresh_install".into(),
        proof_fails_now: true,
        not_doing: vec!["the capture tool itself".into()],
    }
}

fn good_build() -> Build {
    Build {
        touched: vec!["src/voice.rs".into()],
        tests_before: 2100,
        tests_after: 2101,
        proof_passes: true,
        nothing_else_broke: true,
    }
}

// ================= the thinking has to have happened =================

#[test]
fn a_cause_that_restates_the_symptom_is_not_a_diagnosis() {
    // The commonest failure: "it's slow" / "because it's slow".
    let lazy = Thought {
        symptom: "the panel takes too long to open".into(),
        cause: "the panel is too slow to open".into(),
        ..good_thought()
    };
    let e = lazy.is_thought_through().unwrap_err();
    assert!(e.contains("restatement of the symptom"));
    assert!(e.contains("Why does it happen?"));
}

#[test]
fn a_proving_test_that_already_passes_is_testing_something_else() {
    // The single most valuable check in the loop.
    let unproven = Thought { proof_fails_now: false, ..good_thought() };
    let e = unproven.is_thought_through().unwrap_err();
    assert!(e.contains("passes already"));
    assert!(e.contains("fails first"));
}

#[test]
fn nothing_named_as_proof_stops_it_before_any_code() {
    let vague = Thought { proof: String::new(), ..good_thought() };
    assert!(vague.is_thought_through().unwrap_err().contains("prove it fixed"));
}

#[test]
fn a_real_diagnosis_passes() {
    assert!(good_thought().is_thought_through().is_ok());
}

#[test]
fn you_cannot_build_before_the_thought_holds_up() {
    let mut w = Work::new("fix capture");
    w.thought = Some(Thought { proof_fails_now: false, ..good_thought() });
    assert!(matches!(w.what_next(&PipelineConfig::default()), Next::Blocked(_)));

    w.thought = Some(good_thought());
    assert_eq!(w.what_next(&PipelineConfig::default()), Next::Do(Stage::Build));
}

// ================= did it fix the cause =================

#[test]
fn a_change_somewhere_other_than_the_cause_is_the_thing_this_catches() {
    // A fix elsewhere may still make the test pass, and that's exactly what
    // patching a symptom looks like from the inside.
    let elsewhere = Build { touched: vec!["src/daemon.rs".into()], ..good_build() };
    let r = review(&good_thought(), &elsewhere, &[]);
    assert!(!r.clean());
    let note = &r.blockers()[0];
    assert_eq!(note.kind, Concern::NotWhereTheCauseWas);
    assert!(note.what.contains("passing for a different reason"));
}

#[test]
fn a_change_where_the_cause_was_reviews_clean() {
    let r = review(&good_thought(), &good_build(), &[]);
    assert!(r.clean());
    assert_eq!(r.notes[0].kind, Concern::Fine);
}

#[test]
fn editing_tests_while_losing_tests_is_caught() {
    // The oldest way to make a suite green and meaningless.
    let weakened = Build { tests_after: 2095, ..good_build() };
    let r = review(&good_thought(), &weakened, &["tests/voice_and_tools.rs".into()]);
    assert!(!r.clean());
    assert!(r.blockers().iter().any(|n| n.kind == Concern::TestWeakened));
}

#[test]
fn a_sprawling_change_is_mentioned_but_does_not_block() {
    let sprawl = Build {
        touched: (0..7).map(|i| format!("src/voice.rs{i}")).collect(),
        ..good_build()
    };
    let r = review(&good_thought(), &sprawl, &[]);
    assert!(r.notes.iter().any(|n| n.kind == Concern::Scope));
    assert!(r.clean(), "worth saying, not worth stopping for");
}

// ================= refining, not wandering =================

#[test]
fn a_refinement_that_answers_nothing_is_scope_creep_wearing_a_hat() {
    let r = review(&good_thought(), &good_build(), &[]);
    let unrelated = Refinement {
        answers: Concern::HardToFollow,
        what_changed: "renamed some things while I was in there".into(),
    };
    let e = refinement_is_warranted(&unrelated, &r).unwrap_err();
    assert!(e.contains("new change rather than a refinement"));
}

#[test]
fn a_refinement_answering_a_real_note_is_allowed() {
    let elsewhere = Build { touched: vec!["src/daemon.rs".into()], ..good_build() };
    let r = review(&good_thought(), &elsewhere, &[]);
    let fix = Refinement {
        answers: Concern::NotWhereTheCauseWas,
        what_changed: "moved the fix into voice.rs where the folder is made".into(),
    };
    assert!(refinement_is_warranted(&fix, &r).is_ok());
}

#[test]
fn going_round_too_many_times_means_the_thought_was_wrong_not_the_code() {
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    w.build = Some(Build { touched: vec!["src/daemon.rs".into()], ..good_build() });
    w.review = Some(review(&good_thought(), w.build.as_ref().unwrap(), &[]));
    w.stage = Stage::Review;
    w.rounds = 3;

    match w.what_next(&PipelineConfig::default()) {
        Next::HandOver(why) => {
            assert!(why.contains("the thought was wrong rather than the code"));
        }
        o => panic!("{o:?}"),
    }
}

// ================= nothing lands with a stage missing =================

#[test]
fn nothing_lands_without_all_five_and_that_is_not_configurable() {
    // Skipping the thinking is the failure this exists to stop.
    //
    // This asserted `all_stages_required`, a `#[serde(skip)]` bool pinned
    // true that nothing read -- `what_next` takes the config and never
    // consults it. Deleted 19 Sep 2026: the guarantee is the state machine.
    // Each stage yields `Next::Blocked` rather than advancing when its
    // artefact is missing or its proof fails, so there is no configuration in
    // which a stage is skipped.
    let cfg = PipelineConfig::default();

    // Nothing thought through: it asks for the thought rather than the build.
    let empty = Work::new("fix capture");
    assert!(matches!(empty.what_next(&cfg), Next::Do(Stage::Thought)));

    // Thought done, build missing: it asks for the build, not the landing.
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    w.stage = Stage::Build;
    assert!(matches!(w.what_next(&cfg), Next::Do(Stage::Build)));

    // A build whose proof does not pass is blocked with the reason, whatever
    // the config says.
    let mut failing = w.clone();
    let mut b = good_build();
    b.proof_passes = false;
    failing.build = Some(b);
    match failing.what_next(&cfg) {
        Next::Blocked(why) => assert!(why.contains("proving test"), "got: {why}"),
        other => panic!("a failing proof must not advance: {other:?}"),
    }

    // And an old config naming the removed key still loads.
    let parsed: PipelineConfig =
        serde_yaml::from_str("enabled: true\nall_stages_required: false\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn a_build_with_no_thought_behind_it_cannot_land() {
    let mut w = Work::new("fix capture");
    w.build = Some(good_build());
    let e = w.may_land().unwrap_err();
    assert!(e.contains("nothing said what was actually wrong"));
}

#[test]
fn an_unreviewed_change_cannot_land() {
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    w.build = Some(good_build());
    assert!(w.may_land().unwrap_err().contains("not reviewed"));
}

#[test]
fn a_change_with_a_blocking_review_note_cannot_land() {
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    let elsewhere = Build { touched: vec!["src/daemon.rs".into()], ..good_build() };
    w.review = Some(review(&good_thought(), &elsewhere, &[]));
    w.build = Some(elsewhere);
    assert!(w.may_land().is_err());
}

#[test]
fn a_complete_piece_of_work_lands() {
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    w.build = Some(good_build());
    w.review = Some(review(&good_thought(), &good_build(), &[]));
    assert!(w.may_land().is_ok());
}

#[test]
fn a_build_whose_proof_still_fails_does_not_reach_review() {
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    w.build = Some(Build { proof_passes: false, ..good_build() });
    w.stage = Stage::Build;
    match w.what_next(&PipelineConfig::default()) {
        Next::Blocked(why) => assert!(why.contains("proving test still fails")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn something_that_broke_other_tests_does_not_reach_review() {
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    w.build = Some(Build { nothing_else_broke: false, ..good_build() });
    w.stage = Stage::Build;
    assert!(matches!(w.what_next(&PipelineConfig::default()), Next::Blocked(_)));
}

// ================= what reaches you =================

#[test]
fn what_you_are_told_is_behaviour_and_includes_what_it_left_alone() {
    let mut w = Work::new("fix capture");
    w.thought = Some(good_thought());
    let said = w.in_behaviour();
    assert!(said.contains("screen capture fails on a fresh install"));
    assert!(said.contains("it was the working folder is created by listen"));
    assert!(said.contains("Not touching: the capture tool itself"));
    assert!(!said.contains("src/"), "no filenames: {said}");
}

#[test]
fn the_reason_for_the_stages_is_written_down() {
    assert!(WHY_THE_STAGES.contains("Green tests tell you nothing about whether you fixed the right thing"));
    assert!(WHY_THE_STAGES.contains("comes back in three weeks"));
}

#[test]
fn the_stages_only_go_forwards() {
    assert_eq!(Stage::Thought.next(), Stage::Build);
    assert_eq!(Stage::Build.next(), Stage::Review);
    assert_eq!(Stage::Review.next(), Stage::Refine);
    assert_eq!(Stage::Refine.next(), Stage::Implement);
    assert_eq!(Stage::Done.next(), Stage::Done);
}

// ================= the machinery is gated by the discipline =================

use atlas::selfwork::Session;

#[test]
fn a_self_work_session_cannot_start_without_a_diagnosis() {
    // This is the half that was missing. It went straight from a goal to an
    // attempt.
    let s = Session::new("make the panel faster", 2100);
    let e = s.may_start().unwrap_err();
    assert!(e.contains("what's actually wrong"));
    assert!(e.contains("what would prove it fixed"));
}

#[test]
fn a_session_with_a_restated_symptom_still_cannot_start() {
    let mut s = Session::new("make the panel faster", 2100);
    s.work.thought = Some(Thought {
        symptom: "the panel takes too long to open".into(),
        cause: "the panel is too slow to open".into(),
        ..good_thought()
    });
    assert!(s.may_start().is_err());
}

#[test]
fn a_session_with_a_real_diagnosis_starts() {
    let mut s = Session::new("fix capture", 2100);
    s.work.thought = Some(good_thought());
    assert!(s.may_start().is_ok());
}

#[test]
fn a_session_cannot_land_what_it_never_reviewed() {
    let mut s = Session::new("fix capture", 2100);
    s.work.thought = Some(good_thought());
    s.work.build = Some(good_build());
    assert!(s.may_land().unwrap_err().contains("not reviewed"));
}

// ================= getting stuck is not the end =================

#[test]
fn out_of_ideas_goes_to_a_conversation_rather_than_stopping() {
    // The difference between a system that improves and one that stalls at
    // the edge of what it already knew.
    let mut s = Session::new("fix capture", 2100);
    s.work.thought = Some(good_thought());
    assert!(s.take_it_elsewhere().is_some());
}

#[test]
fn what_it_takes_with_it_is_the_diagnosis_not_the_symptom() {
    // Asking someone "this is broken, help" wastes the thinking that already
    // happened.
    let mut s = Session::new("fix capture", 2100);
    s.work.thought = Some(good_thought());
    let c = s.take_it_elsewhere().unwrap();
    assert!(c.problem.contains("the working folder is created by listen"), "the cause");
    assert!(c.problem.contains("src/voice.rs"), "and where");
    assert!(c.problem.contains("capture_works_on_a_fresh_install"), "and the proof");
    assert!(c.problem.contains("tried everything I know"));
    // Not just the symptom, which is what makes it worth someone reading.
    assert!(c.problem.len() > good_thought().symptom.len() * 3);
}

#[test]
fn with_no_diagnosis_there_is_nothing_worth_taking_anywhere() {
    let s = Session::new("something is wrong", 2100);
    assert!(s.take_it_elsewhere().is_none());
}
