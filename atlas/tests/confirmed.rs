use atlas::confirmed::{
    answer, before_a_run, consequence, how_to_undo, read_back, record, Asked, Change, ConfirmConfig,
    Run, Step,
};

fn cfg() -> ConfirmConfig {
    ConfirmConfig { enabled: true, ..Default::default() }
}

fn asked(site: &str, change: Change) -> Asked {
    Asked { site: site.into(), account: "eric".into(), change }
}

#[test]
fn atlas_repeats_it_back_in_your_terms_and_waits() {
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    match read_back(&a, true, true, &cfg()) {
        Step::ReadBack { say } => {
            assert!(say.starts_with("So: turn two-factor off for Instagram on eric."));
            assert!(say.ends_with("Yes or no?"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_read_back_carries_the_consequence_you_might_not_have_in_mind() {
    // "Turn it off on Instagram" also turns it off on Facebook.
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    match read_back(&a, true, true, &cfg()) {
        Step::ReadBack { say } => assert!(say.contains("comes off both")),
        o => panic!("{o:?}"),
    }
    assert!(consequence("GitHub", &Change::TurnOffTwoFactor).unwrap().contains("SSH keys"));
    assert!(consequence("Gmail", &Change::TurnOffTwoFactor).unwrap().contains("reset through"));
}

#[test]
fn generating_new_codes_says_the_old_ones_stop_working() {
    let c = consequence("Gmail", &Change::GenerateRecoveryCodes).unwrap();
    assert!(c.contains("stop working the moment these are made"));
}

#[test]
fn strengthening_something_is_read_back_too() {
    // "Turn it on" misheard as "turn it off" is the failure this catches, and
    // it goes both ways.
    let a = asked("TikTok", Change::TurnOnTwoFactor);
    assert!(matches!(read_back(&a, true, true, &cfg()), Step::ReadBack { .. }));
    assert!(cfg().read_back_everything);
}

#[test]
fn nothing_happens_unless_you_are_at_the_machine() {
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    match read_back(&a, false, true, &cfg()) {
        Step::Cannot(why) => assert!(why.contains("the whole reason it's allowed at all")),
        o => panic!("{o:?}"),
    }
    // And the same request with you there is not refused, which is what
    // makes the refusal above about your presence rather than about
    // everything.
    assert!(matches!(read_back(&a, true, true, &cfg()), Step::ReadBack { .. }));
}

#[test]
fn being_present_is_not_configurable_away() {
    // This asserted `parsed.requires_you_present` -- a `#[serde(skip)]` bool
    // pinned true that nothing read. A bool nobody reads is not a boundary;
    // the refusal above is. Deleted 19 Sep 2026 when the module was wired,
    // and what is checked instead is that a config file claiming otherwise
    // changes the refusal not at all.
    let parsed: ConfirmConfig =
        serde_yaml::from_str("enabled: true\nrequires_you_present: false\n")
            .expect("an unknown key must not stop the section parsing");
    assert!(parsed.enabled, "the rest of the section still parsed");
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    assert!(
        matches!(read_back(&a, false, true, &parsed), Step::Cannot(_)),
        "a config file talked its way past being at the machine"
    );
}

#[test]
fn a_locked_vault_stops_it_because_it_would_have_to_sign_in_first() {
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    assert!(matches!(read_back(&a, true, false, &cfg()), Step::Cannot(_)));
}

#[test]
fn yes_means_yes_and_no_means_no() {
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    assert!(matches!(answer("yes", &a), Step::Go { .. }));
    assert!(matches!(answer("go ahead", &a), Step::Go { .. }));
    assert!(matches!(answer("no", &a), Step::Dropped));
    assert!(matches!(answer("wait", &a), Step::Dropped));
}

#[test]
fn anything_ambiguous_is_treated_as_not_yet_rather_than_as_yes() {
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    match answer("hmm, maybe", &a) {
        Step::Unclear { say } => assert!(say.contains("yes or a no")),
        o => panic!("{o:?}"),
    }
    assert!(!matches!(answer("mm", &a), Step::Go { .. }));
}

#[test]
fn a_sentence_containing_no_is_a_no_even_if_it_also_contains_yes() {
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    assert!(matches!(answer("no, not that one", &a), Step::Dropped));
}

// ================= several of them =================

fn three() -> Vec<Asked> {
    vec![
        asked("Instagram", Change::TurnOffTwoFactor),
        asked("TikTok", Change::TurnOffTwoFactor),
        asked("GitHub", Change::GenerateRecoveryCodes),
    ]
}

#[test]
fn you_say_yes_once_per_change_not_once_for_the_batch() {
    // A single yes covering six security changes is how the wrong one gets
    // made, and you'd have no way to tell which.
    //
    // This asserted a `#[serde(skip)]` bool pinned true that nothing read.
    // Deleted 19 Sep 2026: what actually holds the guarantee is that `Run`
    // cannot express a batch. `current` hands back one `Asked`, `record` and
    // `skip` each advance by one, and there is no way in to answer for
    // several -- so a batch is not something a caller asks for and is
    // refused, it is something no caller can say.
    let mut r = Run::new(three());
    assert_eq!(r.current().map(|a| a.site.clone()), Some("Instagram".to_string()));
    r.record(true);
    assert_eq!(r.current().map(|a| a.site.clone()), Some("TikTok".to_string()));
    assert!(!r.finished(), "one yes finished a run of three");
    r.skip();
    r.record(true);
    assert!(r.finished());
    // Two answered, and the one skipped is not counted as done.
    assert_eq!(r.done.len(), 2);

    // A config file that claims otherwise changes nothing, because there is
    // nothing for it to change.
    let parsed: ConfirmConfig =
        serde_yaml::from_str("enabled: true\none_at_a_time: false\n")
            .expect("an unknown key must not stop the section parsing");
    assert!(parsed.enabled);
    let mut still = Run::new(three());
    still.record(true);
    assert!(!still.finished(), "a config file got a batch through");
}

#[test]
fn a_run_says_up_front_how_many_leave_you_less_protected() {
    let said = before_a_run(&three());
    assert!(said.contains("3 changes, one at a time"));
    assert!(said.contains("2 of them leave the account less protected"));
}

#[test]
fn each_one_is_read_back_separately_as_the_run_goes() {
    let mut r = Run::new(three());
    assert_eq!(r.current().unwrap().site, "Instagram");
    r.record(true);
    assert_eq!(r.current().unwrap().site, "TikTok");
}

#[test]
fn you_can_skip_one_mid_run() {
    let mut r = Run::new(three());
    r.record(true);
    r.skip();
    r.record(true);
    assert!(r.finished());
    assert!(r.summary().contains("1 skipped"));
}

#[test]
fn the_summary_names_what_did_not_work_rather_than_counting_it() {
    let mut r = Run::new(three());
    r.record(true);
    r.record(false);
    r.record(true);
    let s = r.summary();
    assert!(s.contains("2 done"));
    assert!(s.contains("TikTok"), "named: {s}");
}

#[test]
fn there_is_a_record_of_what_was_changed_and_what_you_said() {
    // Security changes are the ones you most want a trail of, and the ones
    // people least often have one for.
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    let r = record(&a, true, "yes", 1000);
    assert_eq!(r.what, "turn two-factor off");
    assert_eq!(r.you_said, "yes");
    assert!(r.worked);
}

#[test]
fn how_to_undo_it_is_answered_because_you_will_ask_and_will_not_remember() {
    assert!(how_to_undo(&Change::TurnOffTwoFactor).contains("need whatever method you pick to hand"));
    assert!(how_to_undo(&Change::GenerateRecoveryCodes).contains("The old set is gone"));
}

#[test]
fn it_is_off_until_you_turn_it_on() {
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    assert!(matches!(read_back(&a, true, true, &ConfirmConfig::default()), Step::Cannot(_)));
}
