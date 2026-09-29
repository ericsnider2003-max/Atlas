use atlas::firstrun::{is_skip, which_monitor, FirstRun, Found, Move, Step};
use atlas::selfwork::{
    count_passing, may_edit, Edit, SelfWorkConfig, Session, Step as WorkStep, Tried, Verdict,
};
use atlas::strategy::StrategyConfig;

// ================= the first run =================

fn found() -> Found {
    Found {
        apps: vec!["chrome".into(), "discord".into()],
        missing_apps: vec!["claude".into()],
        monitors: 2,
        microphones: vec!["Webcam".into(), "AirPods".into()],
        missing_tools: vec!["whisper".into()],
    }
}

#[test]
fn it_starts_by_saying_what_it_is_and_that_you_can_skip() {
    let mut f = FirstRun::default();
    match f.next(&found()) {
        Move::Say(s) => {
            assert!(s.contains("I'm Atlas"));
            assert!(s.contains("skip"), "you can get out of any of it: {s}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn it_finds_what_it_can_without_asking_you() {
    let mut f = FirstRun::default();
    f.record(Step::Hello, "", false);
    match f.next(&found()) {
        Move::Look { step, .. } => assert_eq!(step, Step::FindApps),
        o => panic!("{o:?}"),
    }
    assert!(Step::FindApps.automatic());
    assert!(!Step::Monitors.automatic(), "which screen you work on is not guessable");
}

#[test]
fn with_one_screen_it_does_not_ask_which_screen() {
    let mut f = FirstRun::default();
    f.record(Step::Hello, "", false);
    f.record(Step::FindApps, "found all 2", false);
    let one = Found { monitors: 1, ..found() };
    match f.next(&one) {
        Move::Look { step, .. } => assert_eq!(step, Step::Microphones, "it skipped straight past"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn with_no_microphone_it_says_we_will_type_rather_than_failing() {
    let mut f = FirstRun::default();
    for s in [Step::Hello, Step::FindApps, Step::Monitors] {
        f.record(s, "", false);
    }
    let deaf = Found { microphones: vec![], ..found() };
    match f.next(&deaf) {
        Move::Say(s) => {
            assert!(s.contains("type for now"));
            assert!(s.contains("Everything works typed"), "not a dead end: {s}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn skipping_is_remembered_so_you_can_come_back() {
    let mut f = FirstRun::default();
    f.record(Step::Voice, "", true);
    f.record(Step::Microphones, "", true);
    assert_eq!(f.deferred.len(), 2);
    assert_eq!(f.resume(), Some(Step::Microphones));
    assert!(!f.done_with(Step::Microphones), "it comes back around");
}

#[test]
fn the_closing_line_says_what_worked_and_what_is_left() {
    let mut f = FirstRun::default();
    f.record(Step::Hello, "", false);
    f.record(Step::FindApps, "found 2, but not claude", false);
    f.record(Step::Monitors, "right", false);
    f.record(Step::Microphones, "the webcam", false);
    f.record(Step::Voice, "", true);
    f.record(Step::Missing, "whisper", false);
    f.record(Step::HowToUse, "", false);
    match f.next(&found()) {
        Move::Finished(s) => {
            assert!(s.contains("listening on the webcam"));
            assert!(s.contains("Still to install: whisper"));
            assert!(s.contains("1 thing to come back to"), "got: {s}");
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn it_reports_what_it_found_rather_than_a_count() {
    let f = found();
    assert!(f.report_apps().contains("not claude"), "got: {}", f.report_apps());
    let all = Found { missing_apps: vec![], ..found() };
    assert_eq!(all.report_apps(), "found all 2");
}

#[test]
fn skip_is_understood_however_you_say_it() {
    for s in ["skip", "not now", "later", "pass", "move on"] {
        assert!(is_skip(s), "{s}");
    }
    assert!(!is_skip("the left one"));
}

#[test]
fn which_screen_you_meant_is_understood_in_plain_words() {
    assert_eq!(which_monitor("the left one"), Some("left"));
    assert_eq!(which_monitor("right please"), Some("right"));
    assert_eq!(which_monitor("this one"), Some("current"));
    assert_eq!(which_monitor("mmm"), None);
}

// ================= Atlas working on Atlas =================

fn cfg() -> SelfWorkConfig {
    SelfWorkConfig { enabled: true, ..Default::default() }
}

#[test]
fn it_cannot_edit_the_files_that_decide_what_it_is_allowed_to_do() {
    // A system that can edit its own permissions has none.
    for f in ["config/policy.yaml", "src/policy.rs", "src/system.rs", "src/finance.rs", "src/consent.rs"] {
        match may_edit(f, &cfg()) {
            Verdict::Refused(why) => assert!(why.contains("what I'm allowed to do"), "{f}: {why}"),
            o => panic!("{f} should be refused, got {o:?}"),
        }
    }
}

#[test]
fn it_can_edit_ordinary_source_and_tests() {
    assert_eq!(may_edit("src/persona.rs", &cfg()), Verdict::Allowed);
    assert_eq!(may_edit("docs/BRIEF.md", &cfg()), Verdict::Allowed);
    // An *ordinary* test — one on `selfgrant::ORDINARY_TESTS`. This line used
    // to be `tests/panels.rs`, which is not on that list, and it was pinning
    // the hole rather than the contract: `may_edit` consulted only
    // `cfg.never_touch` (six paths) and `cfg.may_touch` (which includes
    // `tests/`), so any test file at all was editable — `tests/wiring.rs`,
    // `tests/guards.rs`, `tests/ceiling.rs`, and `config/tools.yaml` where
    // `self_grant:` lives. `may_edit` now asks `selfgrant::reach_of` first.
    // The landing side of the same fix is in
    // `tests/fixing_itself_then_and_there.rs`.
    assert_eq!(may_edit("tests/prose.rs", &cfg()), Verdict::Allowed);
    // And a test that is not known to be ordinary is treated as a guard,
    // which is the direction that costs a limit when it is wrong.
    assert!(matches!(may_edit("tests/wiring.rs", &cfg()), Verdict::Refused(_)));
    assert!(matches!(may_edit("config/tools.yaml", &cfg()), Verdict::Refused(_)));
    assert!(matches!(may_edit("build.rs", &cfg()), Verdict::Refused(_)));
}

#[test]
fn it_cannot_reach_outside_the_project_at_all() {
    assert!(matches!(may_edit("../../etc/passwd", &cfg()), Verdict::Refused(_)));
    assert!(matches!(may_edit("C:/Windows/System32/x.dll", &cfg()), Verdict::Refused(_)));
}

#[test]
fn passing_is_not_enough_if_the_tests_went_missing() {
    // A change that passes because it deleted the failing test has "passed"
    // and is worthless.
    let t = Tried {
        edits: vec![],
        passed: true,
        tests_run: 900,
        problem: None,
        lines_changed: 12,
    };
    let e = t.worth_showing(1158, &cfg()).unwrap_err();
    assert!(e.contains("258 fewer tests"), "got: {e}");
    assert!(e.contains("isn't a fix"));
}

#[test]
fn a_change_that_passes_with_the_tests_intact_is_worth_showing() {
    let t = Tried { edits: vec![], passed: true, tests_run: 1159, problem: None, lines_changed: 12 };
    assert!(t.worth_showing(1158, &cfg()).is_ok());
}

#[test]
fn a_change_that_fails_is_never_offered() {
    let t = Tried {
        edits: vec![],
        passed: false,
        tests_run: 1100,
        problem: Some("assertion failed".into()),
        lines_changed: 4,
    };
    assert!(t.worth_showing(1158, &cfg()).is_err());
}

#[test]
fn a_run_where_almost_nothing_executed_does_not_count_as_passing() {
    let t = Tried { edits: vec![], passed: true, tests_run: 0, problem: None, lines_changed: 3 };
    assert!(t.worth_showing(0, &cfg()).unwrap_err().contains("passing means nothing"));
}

#[test]
fn a_failed_attempt_moves_to_a_different_approach() {
    let mut s = Session::new("fix the wake word", 1158);
    let failed = Tried {
        // `src/endpoint.rs`, not `src/voice.rs`. Both are in the "how it
        // hears and speaks" area, which is what these two tests are about,
        // but `voice.rs` is on `selfgrant::ITS_OWN_LIMITS` — it is the struct
        // the `self_grant:` and `financial_domains:` settings parse into, so
        // a permissive new field there widens a limit without touching the
        // YAML. `may_edit` now consults that list, and using it here made
        // these tests assert that Atlas may edit its own limits.
        edits: vec![Edit {
            path: "src/endpoint.rs".into(),
            content: "x".into(),
            reason: "y".into(),
        }],
        passed: false,
        tests_run: 1157,
        problem: Some("still panics".into()),
        lines_changed: 6,
    };
    match s.after(failed, "the handle is cached", &cfg(), &StrategyConfig::default()) {
        WorkStep::Attempt { angle, .. } => {
            assert_ne!(format!("{angle:?}"), "", "a different angle, not the same one again")
        }
        o => panic!("{o:?}"),
    }
    assert_eq!(s.campaign.efforts.len(), 1);
    assert!(s.campaign.what_was_learned()[0].contains("handle is cached"));
}

#[test]
fn a_working_change_is_proposed_with_what_it_touched_and_how_many_tests_ran() {
    let mut s = Session::new("fix the wake word", 1158);
    let ok = Tried {
        // `src/endpoint.rs`, not `src/voice.rs`. Both are in the "how it
        // hears and speaks" area, which is what these two tests are about,
        // but `voice.rs` is on `selfgrant::ITS_OWN_LIMITS` — it is the struct
        // the `self_grant:` and `financial_domains:` settings parse into, so
        // a permissive new field there widens a limit without touching the
        // YAML. `may_edit` now consults that list, and using it here made
        // these tests assert that Atlas may edit its own limits.
        edits: vec![Edit {
            path: "src/endpoint.rs".into(),
            content: "x".into(),
            reason: "y".into(),
        }],
        passed: true,
        tests_run: 1159,
        problem: None,
        lines_changed: 8,
    };
    match s.after(ok, "the handle was cached", &cfg(), &StrategyConfig::default()) {
        WorkStep::Propose { summary } => {
            // Areas, not filenames — "how it hears and speaks" means
            // something; src/voice.rs does not.
            assert!(summary.contains("how it hears and speaks"), "got: {summary}");
            assert!(!summary.contains("src/"), "no paths in what you hear");
            assert!(summary.contains("1159 tests pass"));
            assert!(summary.contains("1 attempt"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_big_change_says_so_rather_than_slipping_past() {
    let mut s = Session::new("restructure the layout engine", 1158);
    let big = Tried {
        edits: vec![Edit { path: "src/layout.rs".into(), content: "x".into(), reason: "y".into() }],
        passed: true,
        tests_run: 1160,
        problem: None,
        lines_changed: 400,
    };
    match s.after(big, "", &cfg(), &StrategyConfig::default()) {
        WorkStep::Propose { summary } => {
            assert!(summary.contains("big change for one go"), "got: {summary}")
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn touching_a_forbidden_file_stops_the_whole_thing() {
    let mut s = Session::new("relax the approval rules", 1158);
    let sneaky = Tried {
        edits: vec![Edit { path: "src/policy.rs".into(), content: "x".into(), reason: "y".into() }],
        passed: true,
        tests_run: 1158,
        problem: None,
        lines_changed: 2,
    };
    assert!(matches!(
        s.after(sneaky, "", &cfg(), &StrategyConfig::default()),
        WorkStep::Refuse(_)
    ));
}

#[test]
fn the_number_of_tests_is_read_from_the_output() {
    let out = "test result: ok. 44 passed; 0 failed\n\
               test result: ok. 1114 passed; 0 failed";
    assert_eq!(count_passing(out), 1158);
    assert_eq!(count_passing("test result: FAILED. 3 passed; 1 failed"), 0);
}

#[test]
fn it_is_off_until_you_turn_it_on() {
    assert!(!SelfWorkConfig::default().enabled);
}
