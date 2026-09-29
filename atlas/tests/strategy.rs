use atlas::handoff::HandoffConfig;
use atlas::strategy::{Angle, Campaign, Effort, Next, StrategyConfig};

fn cfg() -> StrategyConfig {
    StrategyConfig::default()
}

fn effort(angle: Angle, error: &str, solved: bool) -> Effort {
    Effort { angle, learned: format!("nothing new from {}", angle.label()), error: error.into(), solved }
}

#[test]
fn atlas_gets_twelve_real_attempts_not_three() {
    // Three attempts is only meaningful if they're three different attempts.
    assert!(Angle::ALL.len() >= 12);
    assert_eq!(cfg().max_angles, Angle::ALL.len());
    assert!(HandoffConfig::default().attempts_before_asking >= 12);
}

#[test]
fn every_angle_is_genuinely_different_from_the_others() {
    // Left alone, a model retries the same idea with the wording changed and
    // calls it a second try. Each of these attacks the problem differently.
    let mut seen = std::collections::BTreeSet::new();
    for a in Angle::ALL {
        assert!(seen.insert(a.instruction()), "{:?} duplicates another angle", a);
        assert!(a.instruction().len() > 40, "{:?} is too vague to be a real approach", a);
    }
}

#[test]
fn no_angle_is_used_twice() {
    let mut c = Campaign::new("the wake word fails");
    for _ in 0..Angle::ALL.len() {
        match c.next(&cfg()) {
            Next::Try { angle, .. } => c.record(effort(angle, "same error", false)),
            _ => break,
        }
    }
    let used: Vec<Angle> = c.efforts.iter().map(|e| e.angle).collect();
    let mut unique = used.clone();
    unique.sort_by_key(|a| format!("{a:?}"));
    unique.dedup();
    assert_eq!(used.len(), unique.len(), "an angle was repeated");
}

#[test]
fn the_cheap_obvious_angles_come_first() {
    let c = Campaign::new("x");
    match c.next(&cfg()) {
        Next::Try { angle, .. } => assert_eq!(angle, Angle::ReadTheError,
            "the answer is often in the message and was skimmed past"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn reverting_and_starting_again_is_the_last_resort_not_the_first() {
    let last = Angle::ALL.last().unwrap();
    assert_eq!(*last, Angle::RevertAndRethink);
}

#[test]
fn solving_it_ends_the_campaign() {
    let mut c = Campaign::new("x");
    c.record(effort(Angle::ReadTheError, "", true));
    assert_eq!(c.next(&cfg()), Next::Done);
    assert!(c.summary().contains("Fixed it on the first attempt"));
}

#[test]
fn the_same_error_three_times_running_stops_it_early() {
    // The angles aren't reaching the problem. Continuing down the ladder
    // won't help, and burning eight more attempts proves nothing.
    let mut c = Campaign::new("x");
    for a in [Angle::ReadTheError, Angle::CheckWhatChanged, Angle::TestTheAssumption] {
        c.record(effort(a, "error: identical every time", false));
    }
    // Behaviour, not just wording: three identical errors stop the campaign
    // early, where three distinct errors (the sibling test) keep it going.
    assert!(matches!(c.next(&cfg()), Next::Exhausted(_)), "three identical errors did not stop it");
    match c.next(&cfg()) {
        Next::Exhausted(why) => assert!(why.contains("same error"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn different_errors_each_time_means_it_is_still_learning_something() {
    let mut c = Campaign::new("x");
    for (i, a) in [Angle::ReadTheError, Angle::CheckWhatChanged, Angle::TestTheAssumption]
        .iter()
        .enumerate()
    {
        c.record(effort(*a, &format!("error number {i}"), false));
    }
    assert!(matches!(c.next(&cfg()), Next::Try { .. }), "progress, so keep going");
}

#[test]
fn running_out_of_angles_is_a_clean_stop_not_an_endless_loop() {
    let mut c = Campaign::new("x");
    for (i, a) in Angle::ALL.iter().enumerate() {
        c.record(effort(*a, &format!("distinct error {i}"), false));
    }
    match c.next(&cfg()) {
        Next::Exhausted(why) => assert!(why.contains("12 approaches"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn you_can_shorten_the_ladder_or_skip_angles_you_do_not_want() {
    let short = StrategyConfig { max_angles: 2, ..cfg() };
    let mut c = Campaign::new("x");
    c.record(effort(Angle::ReadTheError, "a", false));
    c.record(effort(Angle::CheckWhatChanged, "b", false));
    assert!(matches!(c.next(&short), Next::Exhausted(_)));

    let skip = StrategyConfig { skip: vec![Angle::ReadTheError], ..cfg() };
    match Campaign::new("x").next(&skip) {
        Next::Try { angle, .. } => assert_ne!(angle, Angle::ReadTheError),
        o => panic!("{o:?}"),
    }
}

#[test]
fn what_it_learned_survives_even_when_it_failed() {
    // The useful half of a failed campaign, and what makes the handoff brief
    // worth reading rather than a list of shrugs.
    let mut c = Campaign::new("x");
    c.record(Effort {
        angle: Angle::Instrument,
        learned: "the device handle is cached in the config struct".into(),
        error: "still fails".into(),
        solved: false,
    });
    let learned = c.what_was_learned();
    assert_eq!(learned.len(), 1);
    assert!(learned[0].starts_with("added logging:"));
    assert!(learned[0].contains("cached in the config struct"));
}

#[test]
fn a_campaign_can_say_where_it_got_to_at_any_point() {
    let mut c = Campaign::new("x");
    assert!(c.summary().contains("Haven't started"));
    c.record(effort(Angle::Bisect, "e", false));
    assert!(c.summary().contains("1 approaches, none worked"));
    assert!(c.summary().contains("bisected it"));
}
