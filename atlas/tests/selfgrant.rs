use atlas::selfgrant::{
    asking_for, may_land, reach_of, reach_of_change, Granted, Reach, SelfGrantConfig, Verdict,
};


fn granted(up_to: Reach) -> Granted {
    Granted { up_to, at: 0 }
}

#[test]
fn asking_before_every_change_is_not_safety_it_is_a_stop() {
    // A system that needs permission to fix a typo never fixes one. With
    // nothing granted, even the smallest change stops — which is the failure
    // this exists to avoid, demonstrated.
    let nothing = may_land(
        &["config/commands.yaml".into()],
        true,
        true,
        None,
        false,
        &SelfGrantConfig::default(),
    );
    assert!(matches!(nothing, Verdict::AskFirst(_)), "a typo fix, stopped");

    let granted_says = may_land(
        &["config/commands.yaml".into()],
        true,
        true,
        Some(&granted(Reach::WhatItSays)),
        false,
        &SelfGrantConfig::default(),
    );
    assert!(matches!(granted_says, Verdict::GoAhead { .. }), "same change, now it lands");
}

#[test]
fn permission_is_by_what_a_change_touches_not_by_how_sure_atlas_is() {
    // Being sure is something Atlas assesses about itself, and a mistake it's
    // confident about is exactly the one that gets through. Nothing in the
    // decision takes a confidence value — the same inputs always give the
    // same answer.
    let once = may_land(
        &["src/vault.rs".into()],
        true,
        true,
        Some(&granted(Reach::HowItDecides)),
        false,
        &SelfGrantConfig::default(),
    );
    let again = may_land(
        &["src/vault.rs".into()],
        true,
        true,
        Some(&granted(Reach::HowItDecides)),
        false,
        &SelfGrantConfig::default(),
    );
    assert_eq!(once, again);
    assert!(matches!(once, Verdict::AskFirst(_)), "however sure it might be");
}

#[test]
fn atlas_can_never_widen_its_own_limits_however_this_is_set() {
    assert!(!Reach::ItsOwnLimits.ever_grantable());
    assert_eq!(reach_of("src/policy.rs"), Reach::ItsOwnLimits);
    assert_eq!(reach_of("Cargo.toml"), Reach::ItsOwnLimits);
    assert_eq!(reach_of("src/selfgrant.rs"), Reach::ItsOwnLimits, "including this file");

    let v = may_land(
        &["src/policy.rs".into()],
        true,
        true,
        Some(&granted(Reach::HowItDecides)),
        false,
        &SelfGrantConfig::default(),
    );
    match v {
        Verdict::Never(why) => assert!(why.contains("doesn't have any")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn writing_a_bigger_grant_in_the_config_does_not_make_it_true() {
    let over: SelfGrantConfig =
        serde_yaml::from_str("may_change: its_own_limits\n").unwrap();
    assert!(over.granted().is_none());

    let touches: SelfGrantConfig =
        serde_yaml::from_str("may_change: what_it_touches\n").unwrap();
    assert!(touches.granted().is_none(), "your machine and accounts always need you");
}

#[test]
fn the_grantable_levels_are_the_two_where_being_wrong_is_recoverable() {
    let says: SelfGrantConfig = serde_yaml::from_str("may_change: what_it_says\n").unwrap();
    assert_eq!(says.granted(), Some(Reach::WhatItSays));
    let decides: SelfGrantConfig = serde_yaml::from_str("may_change: how_it_decides\n").unwrap();
    assert_eq!(decides.granted(), Some(Reach::HowItDecides));
}

#[test]
fn one_file_at_a_higher_reach_makes_the_whole_change_that_reach() {
    // Not the average, and not what it's mostly about.
    let mixed = vec!["config/commands.yaml".into(), "src/vault.rs".into()];
    assert_eq!(reach_of_change(&mixed), Reach::WhatItTouches);
}

#[test]
fn anything_unrecognised_defaults_to_the_middle_rather_than_the_bottom() {
    // A new file should not be free to change by being new.
    assert_eq!(reach_of("src/something-new.rs"), Reach::HowItDecides);
}

#[test]
fn with_a_grant_a_wording_fix_lands_on_its_own() {
    let v = may_land(
        &["config/commands.yaml".into()],
        true,
        true,
        Some(&granted(Reach::WhatItSays)),
        false,
        &SelfGrantConfig::default(),
    );
    match v {
        Verdict::GoAhead { say_after } => {
            // Never silent, even when it didn't ask — the sentence names what
            // it changed rather than being a fixed acknowledgement.
            assert!(say_after.contains(Reach::WhatItSays.plain()));
            assert_ne!(say_after.len(), 0);
            assert!(say_after.len() > Reach::WhatItSays.plain().len() + 10);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_change_beyond_what_you_granted_asks_and_says_which_line_it_crossed() {
    let v = may_land(
        &["src/route.rs".into()],
        true,
        true,
        Some(&granted(Reach::WhatItSays)),
        false,
        &SelfGrantConfig::default(),
    );
    match v {
        Verdict::AskFirst(why) => {
            // Both sides of the line, so you know how far to widen it.
            assert!(why.contains(Reach::HowItDecides.plain()));
            assert!(why.contains(Reach::WhatItSays.plain()));
            assert_ne!(Reach::HowItDecides.plain(), Reach::WhatItSays.plain());
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn something_that_could_not_be_undone_is_shown_to_you_whatever_is_granted() {
    let v = may_land(
        &["config/commands.yaml".into()],
        true,
        false,
        Some(&granted(Reach::HowItDecides)),
        false,
        &SelfGrantConfig::default(),
    );
    match v {
        Verdict::AskFirst(why) => assert!(why.contains("couldn't undo")),
        o => panic!("{o:?}"),
    }
    // And the same change, reversible, does land — so it's the reversibility
    // deciding it and not the path.
    let reversible = may_land(
        &["config/commands.yaml".into()],
        true,
        true,
        Some(&granted(Reach::HowItDecides)),
        false,
        &SelfGrantConfig::default(),
    );
    assert!(matches!(reversible, Verdict::GoAhead { .. }));
}

#[test]
fn failing_tests_stop_it_before_the_grant_is_even_considered() {
    let v = may_land(
        &["config/commands.yaml".into()],
        false,
        true,
        Some(&granted(Reach::HowItDecides)),
        false,
        &SelfGrantConfig::default(),
    );
    assert!(matches!(v, Verdict::AskFirst(_)));
}

#[test]
fn a_grant_never_lapses_on_its_own() {
    // Eric, 25 Sep 2026 (B3): he steps away for long stretches, and Atlas
    // mustn't break in that time. A grant stands until he takes it back.
    let g = Granted { up_to: Reach::HowItDecides, at: 0 };
    let v = may_land(
        &["config/commands.yaml".into()],
        true,
        true,
        Some(&g),
        false,
        &SelfGrantConfig::default(),
    );
    assert!(matches!(v, Verdict::GoAhead { .. }), "{v:?}");
    let asked = asking_for(Reach::HowItDecides, &SelfGrantConfig::default());
    assert!(asked.contains("until you take it back"), "{asked}");
}

#[test]
fn the_default_lets_it_fix_how_it_decides_things() {
    // The old default of nothing was the wrong way round: a self-improving
    // system that couldn't improve anything.
    assert_eq!(SelfGrantConfig::default().granted(), Some(Reach::HowItDecides));
}

#[test]
fn with_no_grant_at_all_it_still_asks() {
    let v = may_land(
        &["config/commands.yaml".into()],
        true,
        true,
        None,
        false,
        &SelfGrantConfig::default(),
    );
    match v {
        Verdict::AskFirst(why) => assert!(why.contains("you haven't said")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn asking_for_a_grant_says_what_goes_wrong_rather_than_what_it_would_gain() {
    // That's the thing you're actually deciding about.
    let cfg = SelfGrantConfig::default();
    let decides = asking_for(Reach::HowItDecides, &cfg);
    let says = asking_for(Reach::WhatItSays, &cfg);

    // The consequence named is the one for that level, not a generic warning.
    assert!(decides.contains(Reach::HowItDecides.if_wrong()));
    assert!(says.contains(Reach::WhatItSays.if_wrong()));
    assert_ne!(decides, says);
}

#[test]
fn everything_done_alone_stays_undoable() {
    assert!(SelfGrantConfig::default().always_reversible);
    let parsed: SelfGrantConfig =
        serde_yaml::from_str("may_change: what_it_says\nalways_reversible: false\n").unwrap();
    assert!(parsed.always_reversible);
}

// ================= a new module is its own decision =================

#[test]
fn adding_a_whole_new_module_always_comes_to_you() {
    // Additive and reversible, but it's a new thing in the system rather than
    // a change to an existing one.
    let v = may_land(
        &["src/something.rs".into()],
        true,
        true,
        Some(&granted(Reach::HowItDecides)),
        true,
        &SelfGrantConfig::default(),
    );
    match v {
        Verdict::AskFirst(why) => {
            assert!(why.contains(Reach::SomethingNew.plain()));
            assert_ne!(Reach::SomethingNew.plain(), Reach::HowItDecides.plain());
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_same_change_without_a_new_module_lands() {
    // So it's the newness deciding it, not the file.
    let v = may_land(
        &["src/something.rs".into()],
        true,
        true,
        Some(&granted(Reach::HowItDecides)),
        false,
        &SelfGrantConfig::default(),
    );
    assert!(matches!(v, Verdict::GoAhead { .. }));
}

#[test]
fn a_new_module_can_never_be_granted_in_advance() {
    let cfg: SelfGrantConfig = serde_yaml::from_str("may_change: something_new\n").unwrap();
    assert!(cfg.granted().is_none());
}

// ================= how it raises things =================

#[test]
fn it_raises_a_recommendation_without_making_it_a_demand() {
    // A system that interrupts with "I need a decision" about its own
    // internals is one you learn to dread.
    use atlas::selfgrant::raise_it;
    let decides = raise_it("the route picker keeps choosing a dead route", Reach::HowItDecides);
    let new = raise_it("a module for reading receipts", Reach::SomethingNew);

    // It names what the change would touch, so the two differ.
    assert!(decides.contains(Reach::HowItDecides.plain()));
    assert!(new.contains(Reach::SomethingNew.plain()));
    assert_ne!(decides, new);
    // And it's an offer rather than a demand.
    assert!(!decides.contains("need"), "not a demand: {decides}");
}

#[test]
fn something_unanswered_is_mentioned_once_more_and_then_left_alone() {
    // Asking twice is a reminder. Three times is nagging about something that
    // was never urgent.
    use atlas::selfgrant::raised_again;
    assert!(raised_again("the route picker", 0).is_some());
    assert!(raised_again("the route picker", 1).is_some());
    assert!(raised_again("the route picker", 2).is_none());
}

// ================= the side doors =================
//
// An outside audit found these and it was right. An unknown file used to fall
// through to HowItDecides, which meant the ratchet that catches unreachable
// code, the audit that catches bad tests, and the policy config itself were
// all editable under an ordinary grant.
//
// None of those would have caused a mistake directly. Each of them hides the
// next one, which is worse.

use atlas::selfgrant::LIMIT_MODULES;

#[test]
fn atlas_cannot_edit_the_ratchet_that_catches_unreachable_code() {
    assert_eq!(reach_of("tests/wiring.rs"), Reach::ItsOwnLimits);
}

#[test]
fn atlas_cannot_edit_the_audit_that_catches_bad_tests() {
    assert_eq!(reach_of("tests/retrospective.rs"), Reach::ItsOwnLimits);
}

#[test]
fn atlas_cannot_edit_the_policy_config_file() {
    // The one I'd have been most annoyed to miss — the rules themselves, in
    // a file that looked like ordinary config.
    assert_eq!(reach_of("config/policy.yaml"), Reach::ItsOwnLimits);
}

#[test]
fn a_test_not_known_to_be_ordinary_is_treated_as_a_guard() {
    // Fails closed. The failure directions aren't symmetric: over-restricting
    // costs a question, under-restricting costs the limit.
    assert_eq!(reach_of("tests/something-new.rs"), Reach::ItsOwnLimits);
    assert_eq!(reach_of("tests/prose.rs"), Reach::WhatItTouches, "an ordinary one is still shown to you");
}

#[test]
fn an_ordinary_test_is_never_one_that_guards_a_limit() {
    // A guard can't be allowlisted by accident.
    for t in ["prose.rs", "look.rs", "grading.rs", "layout.rs", "metrics.rs",
              "language.rs", "voiceover.rs", "plainchange.rs"] {
        let stem = t.trim_end_matches(".rs");
        assert!(
            !LIMIT_MODULES.contains(&stem),
            "{t} is on the ordinary list and guards a limit"
        );
    }
}

#[test]
fn a_build_script_or_ci_config_counts_as_a_limit() {
    // These can make the whole suite vacuous without touching a test.
    assert_eq!(reach_of("build.rs"), Reach::ItsOwnLimits);
    assert_eq!(reach_of(".cargo/config.toml"), Reach::ItsOwnLimits);
    assert_eq!(reach_of(".github/workflows/ci.yml"), Reach::ItsOwnLimits);
}

#[test]
fn a_windows_style_path_is_matched_the_same_as_a_unix_one() {
    // Otherwise the whole thing is bypassed by a backslash on the machine it
    // actually runs on.
    assert_eq!(reach_of("tests\\wiring.rs"), Reach::ItsOwnLimits);
    assert_eq!(reach_of("config\\policy.yaml"), Reach::ItsOwnLimits);
}

#[test]
fn ordinary_config_is_still_something_you_see() {
    // It parameterises behaviour rather than being a limit, but it's still a
    // change to what Atlas does on your machine.
    assert_eq!(reach_of("config/apps.yaml"), Reach::WhatItTouches);
    assert_eq!(reach_of("config/layouts.yaml"), Reach::WhatItTouches);
}

#[test]
fn the_config_file_that_holds_its_own_limits_is_not_ordinary_config() {
    // tools.yaml contains `self_grant:` and `financial_domains:`. It looks
    // like ordinary config and isn't — editing it under a standing grant
    // would let Atlas widen the grant.
    assert_eq!(reach_of("config/tools.yaml"), Reach::ItsOwnLimits);
    // And the struct those settings parse into: a new field with a permissive
    // default changes a limit without touching the YAML at all.
    assert_eq!(reach_of("src/voice.rs"), Reach::ItsOwnLimits);
}

#[test]
fn the_self_work_machinery_is_not_editable_under_an_ordinary_grant() {
    // `selfwork.rs` holds `may_edit` and the shipped never-touch list: editing
    // it is widening what self-work may edit, so it's its own limits (1 Oct
    // 2026, with `mend.rs`, `sandbox.rs` and `selftest.rs`).
    assert_eq!(reach_of("src/selfwork.rs"), Reach::ItsOwnLimits);
    assert_eq!(reach_of("src/mend.rs"), Reach::ItsOwnLimits);
    assert_eq!(reach_of("src/pipeline.rs"), Reach::ItsOwnLimits);
    assert_eq!(reach_of("src/selfaudit.rs"), Reach::WhatItTouches);
}
