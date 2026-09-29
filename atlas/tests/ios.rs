use atlas::ios::{
    abilities, cannot, first_run, laptop_is_better_at, phone_is_better_at, ways_to_start, Can,
    IosConfig, THE_BIG_ONE, WHAT_IOS_DOES_BETTER,
};

#[test]
fn the_biggest_difference_is_that_it_cannot_wait_for_a_wake_word() {
    // Everything else is closer than you'd expect; this one isn't.
    let wake = abilities()
        .into_iter()
        .find(|a| a.what.contains("wake on a word"))
        .unwrap();
    assert_eq!(wake.can, Can::Never);
    assert!(wake.detail.contains("biggest single difference from the laptop"));
    assert!(THE_BIG_ONE.contains("no app gets around that"));
}

#[test]
fn it_can_hear_you_think_and_talk_back_entirely_offline() {
    for what in ["hear what you say", "talk back", "think with a local model", "remember everything"] {
        let a = abilities().into_iter().find(|a| a.what.contains(what)).unwrap();
        assert!(a.can <= Can::Limited, "{what} is {:?}", a.can);
    }
}

#[test]
fn seeing_your_screen_and_touching_other_apps_are_off_the_table() {
    let never = cannot();
    assert!(never.iter().any(|a| a.what.contains("see your screen")));
    assert!(never.iter().any(|a| a.what.contains("control other apps")));
    assert!(never.iter().any(|a| a.what.contains("read your other apps' data")));
}

#[test]
fn nothing_runs_all_the_time_and_that_is_said_rather_than_glossed() {
    let bg = abilities().into_iter().find(|a| a.what.contains("run all the time")).unwrap();
    assert_eq!(bg.can, Can::Never);
    assert!(bg.detail.contains("takes them away if you waste them"));

    let bursts = abilities().into_iter().find(|a| a.what.contains("background bursts")).unwrap();
    assert_eq!(bursts.can, Can::Limited);
    assert!(bursts.detail.contains("enough to sync"));
    assert!(bursts.detail.contains("not enough to think hard"));
}

#[test]
fn replacing_siri_is_named_as_impossible_rather_than_hard() {
    let siri = abilities().into_iter().find(|a| a.what.contains("replace Siri")).unwrap();
    assert_eq!(siri.can, Can::Never);
    assert!(siri.detail.contains("\"Hey Siri, ask Atlas\" works through Shortcuts"));
}

#[test]
fn the_scanner_is_where_ios_beats_anything_atlas_would_build() {
    let scan = abilities().into_iter().find(|a| a.what.contains("scan a document")).unwrap();
    assert_eq!(scan.can, Can::Yes);
    assert!(scan.detail.contains("better than anything I'd write"));
    assert!(WHAT_IOS_DOES_BETTER.contains("the one thing the phone does better than the laptop"));
}

#[test]
fn sending_a_message_composes_it_and_you_tap_send() {
    let send = abilities().into_iter().find(|a| a.what.contains("send an email")).unwrap();
    assert_eq!(send.can, Can::Limited);
    assert!(send.detail.contains("you tap send"));
    assert!(send.detail.contains("I wouldn't want it to"));
}

#[test]
fn all_four_ways_of_reaching_the_laptop_work_from_the_phone() {
    for what in ["same wifi", "cloud folder", "over a cable", "AirDrop"] {
        let a = abilities().into_iter().find(|a| a.what.contains(what)).unwrap();
        assert_eq!(a.can, Can::Yes, "{what}");
    }
}

#[test]
fn airdrop_to_the_windows_laptop_is_called_out_as_not_possible() {
    let ad = abilities().into_iter().find(|a| a.what.contains("AirDrop")).unwrap();
    assert!(ad.detail.contains("Not to the laptop though"));
}

#[test]
fn asking_the_laptop_queues_rather_than_failing() {
    let ask = abilities().into_iter().find(|a| a.what.contains("ask the laptop")).unwrap();
    assert!(ask.detail.contains("queues if the laptop is off"));
}

#[test]
fn credentials_staying_off_the_phone_is_a_choice_not_a_limit() {
    let creds = abilities().into_iter().find(|a| a.what.contains("hold your credentials")).unwrap();
    assert!(creds.detail.contains("by choice rather than by iOS"));
    assert!(!IosConfig::default().holds_credentials);

    let parsed: IosConfig = serde_yaml::from_str("enabled: true\nholds_credentials: true\n").unwrap();
    assert!(!parsed.holds_credentials);
}

#[test]
fn there_are_several_ways_to_start_it_and_the_cheapest_is_first() {
    let ways = ways_to_start();
    assert!(ways[0].0.contains("Action Button"));
    assert!(ways.iter().any(|(w, _)| w.contains("Hey Siri")));
    assert!(ways.iter().any(|(w, _)| w.contains("back tap")));
    assert!(ways.last().unwrap().1.contains("always works"));
}

#[test]
fn each_device_is_honest_about_what_the_other_does_better() {
    assert!(phone_is_better_at().iter().any(|s| s.contains("scanning anything")));
    assert!(phone_is_better_at().iter().any(|s| s.contains("the moment you have it")));
    assert!(laptop_is_better_at().iter().any(|s| s.contains("needs your files")));
    assert!(laptop_is_better_at().iter().any(|s| s.contains("with no button")));
}

#[test]
fn permissions_are_asked_for_when_needed_not_all_at_launch() {
    // Asking for everything at launch is how you get told no to all of it.
    assert!(IosConfig::default().ask_permissions_when_needed);
}

#[test]
fn the_first_run_leads_with_the_two_differences_that_matter() {
    let said = first_run();
    assert!(said.contains("can't listen until you start me"));
    assert!(said.contains("can't see your screen"));
    assert!(said.contains("Everything else works"));
}

#[test]
fn every_ability_says_why_or_what_the_catch_is() {
    for a in abilities() {
        assert!(!a.detail.is_empty(), "{} has no detail", a.what);
        if a.can == Can::Never {
            assert!(a.detail.len() > 20, "{} doesn't explain itself", a.what);
        }
    }
}
