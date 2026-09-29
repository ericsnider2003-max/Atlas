//! Building things offline: what to check, in what order, and what to ask
//! before starting.

use atlas::craft::*;
use atlas::goal::Check;

fn ran(cmd: &str, tells: Tells, passed: bool, out: &str) -> Ran {
    Ran { command: cmd.into(), tells, passed, output: out.into() }
}

// --- the ladder is ordered, and the order is the point ----------------------

#[test]
fn the_cheapest_most_precise_check_comes_first() {
    // A type error found by `cargo check` in two seconds is the same error the
    // tests find in forty, except it arrives with a line number.
    let l = ladder(Lang::Rust);
    let sound = l.iter().position(|g| g.tells == Tells::Sound).unwrap();
    let behaviour = l.iter().position(|g| g.tells == Tells::Behaviour).unwrap();
    assert!(sound < behaviour, "tests would run before the compiler");
    assert!(l[sound].seconds < l[behaviour].seconds);
}

#[test]
fn python_gets_a_parse_check_because_it_has_no_compiler() {
    // Without one the first real signal is a test failure, which is the
    // vaguest and slowest — the exact thing the ladder exists to avoid.
    let l = ladder(Lang::Python);
    assert!(l.iter().any(|g| g.tells == Tells::Sound && g.command.contains("compileall")));
    assert!(l.iter().any(|g| g.tells == Tells::Sound && g.command.contains("mypy")));
}

#[test]
fn both_languages_have_every_rung() {
    for lang in [Lang::Rust, Lang::Python] {
        let l = ladder(lang);
        for t in [Tells::Shape, Tells::Sound, Tells::Style, Tells::Behaviour] {
            assert!(l.iter().any(|g| g.tells == t), "{lang:?} has no {t:?} check");
        }
    }
}

#[test]
fn only_a_soundness_failure_makes_the_rest_meaningless() {
    // A style complaint does not stop the tests meaning something. A syntax
    // error does.
    assert!(Tells::Sound.blocks_later());
    assert!(!Tells::Style.blocks_later());
    assert!(!Tells::Shape.blocks_later());
    assert!(!Tells::Behaviour.blocks_later());
}

// --- reading the results ----------------------------------------------------

#[test]
fn a_compile_failure_is_what_gets_reported_not_the_test_noise() {
    // Running tests on code that doesn't compile produces noise, and noise is
    // worse than nothing: it gives the next attempt something confident and
    // wrong to work from.
    let r = vec![
        ran("cargo check --all-targets", Tells::Sound, false, "error[E0308] line 41"),
        ran("cargo test", Tells::Behaviour, false, "42 tests failed"),
    ];
    match read_ladder(Lang::Rust, &r) {
        Next::Fix { gate, output } => {
            assert_eq!(gate.tells, Tells::Sound);
            assert!(output.contains("E0308"), "it reported the test noise instead");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn the_tools_own_words_are_kept_verbatim() {
    // A paraphrased compiler error loses the line number, which is the useful
    // part.
    let r = vec![ran("cargo check --all-targets", Tells::Sound, false, "src/x.rs:41:9 expected u64")];
    match read_ladder(Lang::Rust, &r) {
        Next::Fix { output, .. } => assert!(output.contains("41:9")),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_failing_test_on_compiling_code_is_the_thing_to_fix() {
    let r = vec![
        ran("cargo check --all-targets", Tells::Sound, true, ""),
        ran("cargo test", Tells::Behaviour, false, "assertion failed"),
    ];
    assert!(matches!(read_ladder(Lang::Rust, &r), Next::Fix { .. }));
}

#[test]
fn style_complaints_do_not_block_and_are_reported_as_notes() {
    let r = vec![
        ran("cargo check --all-targets", Tells::Sound, true, ""),
        ran("cargo clippy --all-targets -- -D warnings", Tells::Style, false, "needless clone"),
        ran("cargo test", Tells::Behaviour, true, ""),
    ];
    match read_ladder(Lang::Rust, &r) {
        Next::WorksWithNotes(n) => assert!(n[0].contains("needless clone")),
        other => panic!("a style note stopped the build: {other:?}"),
    }
}

#[test]
fn everything_passing_is_good() {
    let r = vec![
        ran("cargo check --all-targets", Tells::Sound, true, ""),
        ran("cargo clippy --all-targets -- -D warnings", Tells::Style, true, ""),
        ran("cargo test", Tells::Behaviour, true, ""),
    ];
    assert_eq!(read_ladder(Lang::Rust, &r), Next::Good);
}

// --- not running what cannot mean anything ----------------------------------

#[test]
fn tests_are_not_run_on_code_that_does_not_compile() {
    // Saves forty seconds, and stops the noise existing at all.
    let r = vec![ran("cargo check --all-targets", Tells::Sound, false, "error")];
    let left = still_worth_running(Lang::Rust, &r);
    assert!(!left.iter().any(|g| g.tells == Tells::Behaviour), "it would have run the tests");
    assert!(!left.iter().any(|g| g.tells == Tells::Style));
}

#[test]
fn everything_is_worth_running_when_it_compiles() {
    let r = vec![ran("cargo check --all-targets", Tells::Sound, true, "")];
    let left = still_worth_running(Lang::Rust, &r);
    assert!(left.iter().any(|g| g.tells == Tells::Behaviour));
    assert!(!left.iter().any(|g| g.command.contains("cargo check")), "it would rerun");
}

// --- the questions before writing anything ----------------------------------

#[test]
fn it_asks_one_thing_at_a_time() {
    // Five questions at once gets four skimmed answers, and the skimmed ones
    // are the assumptions that bite later.
    let mut s = Spec::new("a thing that watches a folder");
    assert_eq!(s.next_question(), Some(Ask::Shape));
    s.shape = Some("copies new files somewhere".into());
    assert_eq!(s.next_question(), Some(Ask::Inputs));
}

#[test]
fn what_happens_when_the_assumption_is_wrong_is_always_asked() {
    // Unanswered, this defaults to carrying on quietly, which is the worst of
    // the options.
    let mut s = Spec::new("x");
    s.shape = Some("a".into());
    s.inputs = Some("b".into());
    assert_eq!(s.next_question(), Some(Ask::WhenItGoesWrong));
}

#[test]
fn having_no_prohibitions_is_allowed() {
    // Plenty of small things have none, and inventing one to fill the field is
    // worse than leaving it out.
    let mut s = Spec::new("x");
    s.shape = Some("a".into());
    s.inputs = Some("b".into());
    s.when_wrong = Some("c".into());
    s.done_when = vec![Check::CommandPasses("pytest".into())];
    assert!(s.never.is_empty());
    assert!(s.ready(), "an empty never-list blocked it: {:?}", s.next_question());
}

#[test]
fn it_will_not_start_without_a_way_to_tell_it_worked() {
    let mut s = Spec::new("x");
    s.shape = Some("a".into());
    s.inputs = Some("b".into());
    s.when_wrong = Some("c".into());
    assert_eq!(s.next_question(), Some(Ask::DoneWhen));
}

#[test]
fn every_question_says_why_it_is_worth_the_interruption() {
    for a in Ask::all() {
        assert!(a.question().ends_with('?'));
        assert!(a.because().len() > 20, "{a:?} has no reason attached");
    }
}

// --- handing over to the loop -----------------------------------------------

#[test]
fn the_spec_becomes_a_goal_the_toolchain_can_settle() {
    let mut s = Spec::new("a folder watcher");
    s.shape = Some("a".into());
    s.inputs = Some("b".into());
    s.when_wrong = Some("c".into());
    s.done_when = vec![Check::FileExists("watcher.py".into())];

    let g = s.as_goal(Lang::Python, 6);
    assert!(g.runnable_unattended().is_ok(), "it couldn't check itself");
    assert!(g.machine_checks().len() >= 4, "the toolchain checks didn't come across");
}

#[test]
fn formatting_is_not_a_success_criterion() {
    // It gets fixed automatically, so making it a gate would mean a run could
    // fail on whitespace.
    let mut s = Spec::new("x");
    s.done_when = vec![Check::CommandPasses("pytest".into())];
    let g = s.as_goal(Lang::Rust, 3);
    let all = format!("{:?}", g.checks);
    assert!(!all.contains("cargo fmt"), "formatting became a pass/fail gate");
}

#[test]
fn a_path_tells_you_which_ladder_to_use() {
    assert_eq!(Lang::of_path("src/main.rs"), Some(Lang::Rust));
    assert_eq!(Lang::of_path("app/thing.py"), Some(Lang::Python));
    assert_eq!(Lang::of_path("notes.md"), None);
}
