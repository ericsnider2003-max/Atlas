//! Knowing when a long job is finished.

use atlas::goal::*;

fn attempt(n: u32, passed: Vec<Check>, failed: Vec<Check>, changed: &str) -> Attempt {
    Attempt { n, passed, failed, changed: changed.into() }
}

fn build() -> Check {
    Check::CommandPasses("cargo test".into())
}

fn sources() -> Check {
    Check::AtLeast { what: "sources".into(), n: 3 }
}

// --- it will not run without a definition of done ---------------------------

#[test]
fn a_goal_with_no_check_will_not_run_unattended() {
    // Working all night without being able to say whether it worked is how an
    // overnight run becomes a pile of activity you have to read.
    let g = Goal::new("make the report better", 5);
    assert_eq!(g.runnable_unattended(), Err(NotRunnable::NoCheck));
    assert!(g.runnable_unattended().unwrap_err().plain().contains("finished looks like"));
}

#[test]
fn a_goal_only_you_can_judge_waits_for_you() {
    // "Make it look good" cannot be evaluated at 3am by the thing that wrote
    // it. Keeping it as a goal is fine; running it unattended is not.
    let g = Goal::new("redesign the panel", 5)
        .checking(Check::YouDecide("it looks right".into()));
    let result = g.runnable_unattended();
    // Structural, not wording: exactly one reason, and it is the refusal
    // variant rather than success or some other error shape.
    assert!(result.is_err(), "a subjective check must not be runnable unattended");
    match result {
        Err(NotRunnable::OnlyYouCanTell(w)) => {
            assert_eq!(w.len(), 1);
            assert!(w[0].contains("looks right"));
        }
        other => panic!("it would have run: {other:?}"),
    }
}

#[test]
fn one_machine_check_among_subjective_ones_is_enough_to_start() {
    let g = Goal::new("tidy the module", 5)
        .checking(Check::YouDecide("it reads well".into()))
        .checking(build());
    assert!(g.runnable_unattended().is_ok());
    assert_eq!(g.machine_checks().len(), 1);
}

#[test]
fn a_goal_with_no_limit_will_not_run() {
    // A loop with no cap burns a night discovering its criterion was
    // unreachable.
    let g = Goal::new("x", 0).checking(build());
    assert_eq!(g.runnable_unattended(), Err(NotRunnable::NoLimit));
}

#[test]
fn every_refusal_says_why_in_words_you_would_use() {
    for r in [
        NotRunnable::NoCheck,
        NotRunnable::OnlyYouCanTell(vec!["it looks right".into()]),
        NotRunnable::NoLimit,
    ] {
        assert!(r.plain().len() > 40, "{r:?} explains nothing");
    }
}

// --- where a run stands -----------------------------------------------------

#[test]
fn meeting_every_check_is_done() {
    let g = Goal::new("x", 5).checking(build()).checking(sources());
    let a = vec![attempt(1, vec![build(), sources()], vec![], "wrote the tests")];
    assert_eq!(g.standing(&a), Standing::Done);
    assert!(g.spoken(&a).contains("checks out"));
}

#[test]
fn running_out_of_attempts_names_what_is_still_failing() {
    let g = Goal::new("x", 2).checking(build());
    let a = vec![
        attempt(1, vec![], vec![build()], "tried a"),
        attempt(2, vec![], vec![build()], "tried b"),
    ];
    match g.standing(&a) {
        Standing::GaveUp { after, still_failing } => {
            assert_eq!(after, 2);
            assert!(still_failing[0].contains("cargo test"));
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn repeating_itself_stops_early_rather_than_burning_the_night() {
    // Running the same attempt eight more times costs a night to learn what
    // two attempts already showed.
    let g = Goal::new("x", 20).checking(build());
    let a = vec![
        attempt(1, vec![], vec![build()], "tried something"),
        attempt(2, vec![], vec![build()], ""),
        attempt(3, vec![], vec![build()], ""),
    ];
    assert!(matches!(g.standing(&a), Standing::GoingInCircles { .. }));
    assert!(g.spoken(&a).contains("repeating itself"));
}

#[test]
fn still_changing_things_keeps_going() {
    let g = Goal::new("x", 20).checking(build());
    let a = vec![
        attempt(1, vec![], vec![build()], "tried a"),
        attempt(2, vec![], vec![build()], "tried b"),
    ];
    assert!(matches!(g.standing(&a), Standing::TryAgain { .. }));
}

#[test]
fn finishing_on_the_last_attempt_is_done_not_exhausted() {
    // Order matters. A run that met its checks on the final attempt succeeded.
    let g = Goal::new("x", 2).checking(build());
    let a = vec![
        attempt(1, vec![], vec![build()], "tried a"),
        attempt(2, vec![build()], vec![], "tried b"),
    ];
    assert_eq!(g.standing(&a), Standing::Done);
}

#[test]
fn a_run_that_has_not_started_has_all_its_attempts() {
    let g = Goal::new("x", 5).checking(build());
    assert_eq!(g.standing(&[]), Standing::TryAgain { attempts_left: 5 });
}

#[test]
fn an_attempt_passing_nothing_has_not_met_everything() {
    // An empty check list must not read as success.
    assert!(!attempt(1, vec![], vec![], "").met_everything());
}

// --- the report -------------------------------------------------------------

#[test]
fn the_morning_line_leads_with_where_it_stands_not_how_hard_it_tried() {
    let g = Goal::new("the migration", 3).checking(build());
    let a = vec![
        attempt(1, vec![], vec![build()], "a"),
        attempt(2, vec![], vec![build()], "b"),
        attempt(3, vec![], vec![build()], "c"),
    ];
    let said = g.spoken(&a);
    assert!(said.starts_with("the migration"));
    assert!(said.contains("gave up"), "got: {said}");
}

#[test]
fn every_check_can_describe_itself() {
    for c in [
        Check::CommandPasses("cargo test".into()),
        Check::FileExists("out.md".into()),
        Check::FileContains { path: "a".into(), text: "b".into() },
        Check::AtLeast { what: "sources".into(), n: 3 },
        Check::YouDecide("it reads well".into()),
    ] {
        assert!(!c.plain().is_empty());
    }
}
