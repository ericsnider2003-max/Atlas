//! The conversation that `Intent::WorkOnYourself` could never have.
//!
//! `Session::may_start` refuses while `work.thought` is `None`, and nothing
//! in `src/` ever set `work.thought`. So this command answered *"Before I
//! touch anything: no diagnosis — what's actually wrong, and what would prove
//! it fixed?"* to every input, every time, and there was no way to answer the
//! question. `tests/a_stage_that_cannot_be_reached.rs` holds the machinery
//! side of that finding; this file is the part a person would see.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-selfwork-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The real config with the pipeline switched on.
///
/// `config/tools.yaml` ships `pipeline.enabled: false`, which is the right
/// default and is also a second way for this command to do nothing. Tested
/// on its own below, so that "it's switched off" cannot quietly stand in for
/// "it doesn't work".
fn cfg_on() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let mut tools = c.tools.clone().unwrap();
    tools.pipeline.enabled = true;
    // A test command that returns immediately and prints nothing.
    //
    // `run_the_proof` genuinely runs `self_work.test_command` with the named
    // test as a filter, which is the point of it — `proof_fails_now` is a
    // measurement, not an answer. Left as the shipped `cargo test`, these
    // tests would compile the whole tree from inside the test process, once
    // per proof, and each one would sit on `PROOF_BUDGET_SECS` before giving
    // up. The first version of this file did exactly that and did not finish.
    //
    // Empty output is read as `NotWrittenYet` — no `test result:` line means
    // nothing ran — which is the same answer as a filter matching no test,
    // and is the path these tests want. The parsing itself is covered
    // directly, against real `cargo test` output, in
    // `tests/a_stage_that_cannot_be_reached.rs`.
    tools.self_work.test_command =
        if cfg!(windows) { "cmd /c rem".into() } else { "/bin/true".into() };
    c.tools = Some(tools);
    c
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn switched_off_says_so_and_nothing_else() {
    // Eric turned the pipeline on in the shipped config on 25 Sep 2026 (E2),
    // so "off" is now a choice you make rather than the default. Still has
    // to say so, and nothing else, when you make it.
    let mut c = Config::load(Path::new("config")).unwrap();
    c.tools.as_mut().unwrap().pipeline.enabled = false;
    let p = plat();
    let mut d = daemon(&c, &p, "off");
    let said = d.execute(&Intent::WorkOnYourself("the panel is slow".into()));
    assert!(said.contains("switched off"), "got: {said}");
}

#[test]
fn a_new_goal_is_answered_with_a_question_rather_than_a_refusal() {
    // What this used to say, for every input: "Before I touch anything: no
    // diagnosis — what's actually wrong, and what would prove it fixed?"
    // True, unanswerable, and the end of the conversation.
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "first-question");
    let said = d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));

    assert!(
        !said.contains("no diagnosis"),
        "it still refuses with the sentence nobody could answer: {said}"
    );
    assert!(said.contains("the settings panel is slow"), "it dropped the goal: {said}");
    assert!(said.contains('?'), "it did not ask for anything: {said}");
    // And what it asks for is the CAUSE. The goal is the symptom: asking
    // "what did you notice?" of someone who has just told you is a question
    // they have answered, and it cost a whole turn to ask it.
    assert!(
        said.to_lowercase().contains("why does it happen"),
        "it asked for the symptom again instead of the cause: {said}"
    );
}

#[test]
fn each_answer_is_taken_as_an_answer_and_not_as_a_new_goal() {
    // The specific way a turn-by-turn flow fails: the first answer is read as
    // a fresh goal, the session restarts, and the same question comes back
    // for ever. `Session::new` being called on every `WorkOnYourself` is
    // exactly why the old code could never leave the Thought stage, recorded
    // in `Session`'s own doc comment.
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "answers");

    let q1 = d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));
    let q2 = d.execute(&Intent::WorkOnYourself(
        "every row re-reads the whole config file from disk".into(),
    ));
    assert_ne!(q1, q2, "the same question came back, so the answer was taken as a new goal");
    assert!(q2.to_lowercase().contains("which part"), "it did not ask where: {q2}");

    let q3 = d.execute(&Intent::WorkOnYourself("src/panel.rs".into()));
    assert_ne!(q2, q3, "it asked the same question twice");
    assert!(
        q3.to_lowercase().contains("prove"),
        "it did not ask what would prove it fixed: {q3}"
    );
}

#[test]
fn a_cause_that_restates_the_symptom_is_sent_back_rather_than_accepted() {
    // The commonest failure in a diagnosis, and the reason `Thought` has a
    // separate `cause` field at all: "it's slow" / "because it's slow".
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "restated");

    d.execute(&Intent::WorkOnYourself("the settings panel is too slow to open".into()));
    d.execute(&Intent::WorkOnYourself("the settings panel takes too long to open".into()));
    d.execute(&Intent::WorkOnYourself("src/panel.rs".into()));
    let said = d.execute(&Intent::WorkOnYourself("a_test_that_does_not_exist_anywhere".into()));
    let said = if said.to_lowercase().contains("prove") {
        // If it is still collecting, one more turn finishes it. Written this
        // way so the test does not silently pass by never reaching the check.
        d.execute(&Intent::WorkOnYourself("a_test_that_does_not_exist_anywhere".into()))
    } else {
        said
    };

    assert!(
        said.contains("restatement") || said.contains("doesn't hold up"),
        "a restated symptom was accepted as a diagnosis: {said}"
    );
}

#[test]
fn the_proving_test_is_actually_run_rather_than_taken_on_trust() {
    // The single most valuable check in the loop. `proof_fails_now` is a
    // measurement, so `Diagnosing` has no slot for it — it comes from
    // running the named test against the tree as it is.
    //
    // The test named here does not exist, so the run matches nothing, and
    // Atlas has to say that rather than reading `cargo`'s exit status (which
    // is **zero** for a filter that matched nothing) as "it passes already".
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "proof-run");

    d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));
    d.execute(&Intent::WorkOnYourself("every row re-reads the config from disk".into()));
    d.execute(&Intent::WorkOnYourself("src/panel.rs".into()));
    let said = d.execute(&Intent::WorkOnYourself(
        "a_proving_test_nobody_has_written_yet_xyzzy".into(),
    ));

    assert!(
        said.contains("a_proving_test_nobody_has_written_yet_xyzzy"),
        "it did not say anything about the test it was given: {said}"
    );
    assert!(
        !said.contains("passes already"),
        "a filter that matched nothing was read as a test that passes: {said}"
    );
}

#[test]
fn at_the_build_stage_it_reaches_for_the_fix_rather_than_punting() {
    // This used to be where the loop stopped: the Build stage answered with
    // its own name ("building it next") or admitted "nothing here writes a
    // candidate fix". Closing the loop (`Daemon::attempt_own_fix`) changed
    // that — Atlas now finds the file the diagnosis points at and drafts a
    // fix. With no model configured (this daemon has none), the honest thing
    // it's waiting for is a model, not a missing piece of itself.
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "honest-build");

    for answer in [
        "the settings panel is slow",
        "every row re-reads the config from disk",
        "src/panel.rs",
        "a_proving_test_nobody_has_written_yet_xyzzy",
    ] {
        d.execute(&Intent::WorkOnYourself(answer.into()));
    }
    let said = d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));

    // It got as far as the file and now wants a model to draft with — the loop
    // is closed, so it no longer punts with "nothing writes a candidate fix".
    assert!(
        said.contains("Working on a fix") && said.contains("background") && said.contains("nothing lands without your yes"),
        "Build must start the bounded local-recipe path and retain approval before changes: {said}"
    );
    assert!(
        !said.contains("writes a candidate fix"),
        "the old dead-end answer is gone now the loop is closed: {said}"
    );
    assert!(
        !said.starts_with("building it next"),
        "the stage must not answer with its own name: {said}"
    );
}

#[test]
fn a_piece_of_work_can_be_dropped_and_stays_dropped() {
    // Without this, a session sits in the store asking the same question at
    // every `WorkOnYourself` for the life of the install — and a person who
    // has changed their mind is answering the next question of a
    // conversation they have already left.
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "dropped");

    d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));
    let dropped = d.execute(&Intent::WorkOnYourself("never mind".into()));
    assert!(dropped.contains("Dropped"), "got: {dropped}");
    assert!(dropped.contains("Nothing was changed"), "it did not say nothing happened: {dropped}");

    // And it does not come back with its next question.
    let after = d.execute(&Intent::WorkOnYourself("stop".into()));
    assert!(after.contains("wasn't working on anything"), "it was still holding it: {after}");
}

#[test]
fn the_same_goal_after_dropping_it_starts_over_rather_than_resuming() {
    // Asking again for something you dropped is a new decision. Resuming
    // would put it back at whatever stage it was dropped in, where
    // `still_needs` is silent — so the diagnosis could never be finished and
    // the goal would be permanently unaskable.
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "dropped-then-asked-again");

    let first = d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));
    d.execute(&Intent::WorkOnYourself("never mind".into()));
    let again = d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));

    assert_eq!(again, first, "asking again for a dropped goal did not start it over: {again}");
}

#[test]
fn dropping_something_that_was_never_started_says_so() {
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "drop-nothing");
    let said = d.execute(&Intent::WorkOnYourself("forget it".into()));
    assert!(said.contains("wasn't working on anything"), "got: {said}");
}

#[test]
fn a_blank_turn_does_not_answer_a_question() {
    // `is_thought_through` checks for empty strings, and a required part
    // filled with `""` would either fail with a confusing reason or — for
    // `where_` — pass, and make the review compare the change against an
    // empty location.
    let c = cfg_on();
    let p = plat();
    let mut d = daemon(&c, &p, "blank");

    let q1 = d.execute(&Intent::WorkOnYourself("the settings panel is slow".into()));
    let q2 = d.execute(&Intent::WorkOnYourself("   ".into()));
    assert!(
        q2.to_lowercase().contains("why does it happen"),
        "a blank turn moved the conversation on, or was routed somewhere else \
         entirely:\n  asked: {q1}\n  then:  {q2}"
    );

    // And the real answer still lands afterwards.
    let q3 = d.execute(&Intent::WorkOnYourself("it re-reads the config on every row".into()));
    assert!(q3.to_lowercase().contains("which part"), "the flow did not recover: {q3}");
}

/// A self-audit that has measurably slowed says so, in plain words.
///
/// The bug this proves gone: asking "anything to look at?" ran
/// `refresh_signals`, which rebuilds `self.signals` from undo,
/// misunderstandings and unused capabilities alone -- overwriting the
/// `GotSlower` signal the hourly tidy had pushed. So a machine whose turns had
/// visibly slowed answered without a word about speed, the one thing a person
/// actually feels. The on-demand self-audit now asks the timing window itself
/// and speaks `timing::why_slow` when `got_slower` reports a real regression.
#[test]
fn a_self_audit_that_has_slowed_down_says_why() {
    use atlas::timing::{Stage, Turn};

    let mut c = Config::load(Path::new("config")).unwrap();
    let mut tools = c.tools.clone().unwrap();
    tools.self_audit.enabled = true;
    c.tools = Some(tools);

    let p = plat();
    let mut d = daemon(&c, &p, "slowed-down");

    // A window that has clearly got slower: six quick turns, then six that
    // cross the "feels slow" line (2000ms of Atlas's own time). This makes
    // `got_slower` fire (later half's median is well over half again the
    // earlier half's) and `slow_ones` non-empty.
    for _ in 0..6 {
        let mut t = Turn { about: "a quick one".into(), ..Default::default() };
        t.note(Stage::Doing, 1000);
        d.timing.add(t);
    }
    for _ in 0..6 {
        let mut t = Turn { about: "a slow one".into(), ..Default::default() };
        t.note(Stage::Doing, 3000);
        d.timing.add(t);
    }

    let said = d.execute(&Intent::WorkOnYourself("anything to look at".into()));

    assert!(
        said.contains("turns were slow"),
        "the self-audit said nothing about the slowdown: {said}"
    );
    assert!(
        said.contains("doing it"),
        "it did not name the stage the time went to: {said}"
    );
}

/// And a self-audit on a machine that has NOT slowed stays quiet about speed.
///
/// The gate matters as much as the wiring: `why_slow` returns a cheerful
/// "nothing's been slow" line, and appending that to every self-audit would be
/// noise. It speaks only on a real regression.
#[test]
fn a_self_audit_that_is_fine_does_not_volunteer_a_speed_line() {
    use atlas::timing::{Stage, Turn};

    let mut c = Config::load(Path::new("config")).unwrap();
    let mut tools = c.tools.clone().unwrap();
    tools.self_audit.enabled = true;
    c.tools = Some(tools);

    let p = plat();
    let mut d = daemon(&c, &p, "steady");

    // Twelve steady, quick turns: nothing has slowed, so `got_slower` is None.
    for _ in 0..12 {
        let mut t = Turn { about: "a quick one".into(), ..Default::default() };
        t.note(Stage::Doing, 1000);
        d.timing.add(t);
    }

    let said = d.execute(&Intent::WorkOnYourself("anything to look at".into()));

    assert!(
        !said.contains("turns were slow") && !said.contains("Nothing's been slow"),
        "it volunteered a speed line with no regression to report: {said}"
    );
}
