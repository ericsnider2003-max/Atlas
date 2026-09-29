use atlas::accounts::{Account, SecondFactor, Stakes};
use atlas::goingaway::{
    if_you_do_one_thing, periodic_nudge, plan, spoken as away_spoken, would_lock_you_out,
    AwayConfig, Survives, WHY_NOT_ATLAS_HOLDS_IT, WHY_NOT_OFF,
};
use atlas::voiceover::{
    break_into_lines, duck_filter, fits, how_long, lay_out, music_ducking, snapped_count,
    spoken as vo_spoken, Beat, Fit, VoiceoverConfig,
};

// ================= being away is the problem, not two-factor =================

fn accounts() -> Vec<Account> {
    vec![
        Account { site: "Gmail".into(), second_factor: SecondFactor::Sms, stakes: Stakes::Keystone,
                  reused_password: false, has_recovery_codes: false,
                  settings_url: Some("https://myaccount.google.com/security".into()) },
        Account { site: "Chase Bank".into(), second_factor: SecondFactor::Sms, stakes: Stakes::High,
                  reused_password: false, has_recovery_codes: false, settings_url: None },
        Account { site: "GitHub".into(), second_factor: SecondFactor::App, stakes: Stakes::High,
                  reused_password: false, has_recovery_codes: true, settings_url: None },
    ]
}

#[test]
fn what_breaks_when_you_travel_is_sms_not_two_factor() {
    // A text needs your number, your carrier and a signal. An app code needs
    // none of those.
    assert!(Survives::of(SecondFactor::Sms).breaks_when_away());
    assert!(Survives::of(SecondFactor::Email).breaks_when_away());
    assert!(!Survives::of(SecondFactor::App).breaks_when_away());
    assert!(!Survives::of(SecondFactor::Key).breaks_when_away());
}

#[test]
fn an_app_code_is_explained_as_working_with_no_signal_at_all() {
    assert!(Survives::of(SecondFactor::App).plain().contains("no signal"));
    assert!(Survives::of(SecondFactor::Sms).plain().contains("this is what breaks"));
}

#[test]
fn atlas_names_exactly_which_accounts_would_lock_you_out() {
    let accs = accounts();
    let stuck = would_lock_you_out(&accs);
    assert_eq!(stuck.len(), 2);
    assert!(stuck.iter().any(|a| a.site == "Gmail"));
    assert!(!stuck.iter().any(|a| a.site == "GitHub"), "app code with recovery is fine");
}

#[test]
fn the_fix_offered_is_better_protection_not_less() {
    let p = plan(&accounts());
    assert!(p.iter().any(|x| x.what.contains("authenticator app")));
    assert!(p.iter().any(|x| x.what.contains("print the recovery codes")));
    assert!(!p.iter().any(|x| x.what.to_lowercase().contains("turn off")));
}

#[test]
fn recovery_codes_are_named_as_the_thing_that_actually_stops_a_lockout() {
    let p = plan(&accounts());
    let codes = p.iter().find(|x| x.what.contains("recovery codes")).unwrap();
    assert!(codes.why.contains("no phone, no signal and no battery"));
    assert!(codes.why.contains("single thing that stops a lockout"));
}

#[test]
fn a_hardware_key_is_raised_as_built_for_this_situation() {
    let p = plan(&accounts());
    let key = p.iter().find(|x| x.what.contains("hardware key")).unwrap();
    assert!(key.why.contains("no battery"));
    assert!(key.why.contains("spare kept at home"), "and the obvious objection: {}", key.why);
}

#[test]
fn everything_is_marked_as_something_to_do_before_you_leave() {
    // Recovery codes have to be printed and a key has to arrive.
    assert!(plan(&accounts()).iter().all(|p| p.before_you_go));
}

#[test]
fn atlas_leads_with_the_count_and_the_two_fixes() {
    let said = away_spoken(&accounts());
    assert!(said.contains("2 would lock you out"));
    assert!(said.contains("Gmail"));
    assert!(said.contains("authenticator app"));
    assert!(said.contains("print the recovery codes"));
    assert!(said.contains("better protected than turning it off"));
}

#[test]
fn the_reason_not_to_turn_it_off_is_specific_to_being_away() {
    // Not a repeated policy.
    assert!(WHY_NOT_OFF.contains("worst time to have it off, not the best"));
    assert!(WHY_NOT_OFF.contains("least able to notice something's wrong"));
    assert!(WHY_NOT_OFF.contains("it works on a plane"));
    assert!(WHY_NOT_OFF.contains("I'll set both up with you"));
}

#[test]
fn whether_atlas_could_just_hold_the_codes_is_answered_honestly() {
    // The technical answer is yes, and the reason not to isn't obvious.
    assert!(WHY_NOT_ATLAS_HOLDS_IT.starts_with("Technically yes"));
    assert!(WHY_NOT_ATLAS_HOLDS_IT.contains("one factor wearing two hats"));
    assert!(WHY_NOT_ATLAS_HOLDS_IT.contains("I'll do everything around that"));
}

#[test]
fn if_you_only_do_one_thing_it_is_the_account_everything_resets_through() {
    let said = if_you_do_one_thing(&accounts()).unwrap();
    assert!(said.contains("Gmail"));
    assert!(said.contains("Every other account resets through it"));
}

#[test]
fn it_checks_periodically_because_dates_move_and_preparation_takes_days() {
    let cfg = AwayConfig { enabled: true, ..Default::default() };
    assert!(periodic_nudge(&accounts(), 30, &cfg).is_none(), "not yet");
    let nudge = periodic_nudge(&accounts(), 120, &cfg).unwrap();
    assert!(nudge.contains("if you had to leave tomorrow"));
    assert!(nudge.contains("ten minutes"));
}

#[test]
fn accounts_that_would_all_survive_get_nothing() {
    let ready = vec![Account {
        site: "Gmail".into(), second_factor: SecondFactor::App, stakes: Stakes::Keystone,
        reused_password: false, has_recovery_codes: true, settings_url: None,
    }];
    // Behaviour, not just wording: "Nothing to do" is only honest if no
    // account would actually lock you out.
    assert!(would_lock_you_out(&ready).is_empty(), "an account that would survive was flagged");
    assert!(away_spoken(&ready).contains("Nothing to do"));
}

// ================= voiceovers =================

fn cfg() -> VoiceoverConfig {
    VoiceoverConfig { enabled: true, ..Default::default() }
}

#[test]
fn how_long_a_line_takes_counts_the_punctuation() {
    // Ignoring it is why generated timing runs short and then overlaps.
    let plain = how_long("this is a line of about eight words here", 1.0);
    let punctuated = how_long("this is a line. Of about eight words. Here.", 1.0);
    assert!(punctuated > plain);
}

#[test]
fn a_faster_pace_takes_less_time() {
    assert!(how_long("a line of words", 1.4) < how_long("a line of words", 1.0));
}

#[test]
fn lines_land_on_cuts_rather_than_just_after_them() {
    // A line half a second late sounds wrong in a way people notice without
    // knowing why.
    let script = vec!["First line here.".to_string(), "Second line here.".to_string()];
    let beats = vec![Beat { at: 0.0, strong: true }, Beat { at: 2.4, strong: true }];
    let lines = lay_out(&script, &beats, 12.0, &cfg());
    assert!(snapped_count(&lines, &beats) >= 1);
}

#[test]
fn a_script_longer_than_the_footage_says_how_many_words_to_lose() {
    // You cut words, not seconds.
    let script: Vec<String> = (0..8).map(|i| format!("This is line number {i} of the script.")).collect();
    let lines = lay_out(&script, &[], 10.0, &cfg());
    match fits(&lines, 10.0, &cfg()) {
        Fit::TooLong { cut_words, .. } => assert!(cut_words > 5, "got {cut_words}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn a_script_far_too_short_says_the_picture_will_sit_in_silence() {
    let script = vec!["Short.".to_string()];
    let lines = lay_out(&script, &[], 40.0, &cfg());
    match fits(&lines, 40.0, &cfg()) {
        Fit::TooShort { silence_secs } => assert!(silence_secs > 30.0),
        o => panic!("{o:?}"),
    }
}

#[test]
fn something_that_fits_says_how_much_room_is_left() {
    let script = vec!["A reasonable line of about ten words in it.".to_string()];
    let lines = lay_out(&script, &[], 6.0, &cfg());
    assert!(matches!(fits(&lines, 6.0, &cfg()), Fit::Fits { .. }));
}

#[test]
fn the_music_starts_dropping_before_the_voice_arrives() {
    // Otherwise it dips underneath the first word instead of ahead of it.
    let lines = lay_out(&["A line.".to_string()], &[], 10.0, &cfg());
    let ducks = music_ducking(&lines, &cfg());
    assert_eq!(ducks.len(), 1);
    assert!(ducks[0].0 < lines[0].at || lines[0].at == 0.0);
    assert!(ducks[0].2 < 0.0, "it goes down, not up");
}

#[test]
fn the_music_comes_back_after_the_line_rather_than_on_it() {
    let lines = lay_out(&["A line.".to_string()], &[], 10.0, &cfg());
    let ducks = music_ducking(&lines, &cfg());
    assert!(ducks[0].1 > lines[0].ends());
}

#[test]
fn ducking_is_one_filter_rather_than_a_hand_built_timeline() {
    let f = duck_filter(&cfg());
    assert!(f.contains("sidechaincompress"));
    assert!(f.contains("[ducked]"));
}

#[test]
fn a_long_sentence_is_given_a_breath_where_the_writer_put_a_comma() {
    // A wall of text read straight through is what makes a voiceover sound
    // generated, whatever voice reads it.
    let long = "Stop paying tier two fees because the difference between tier two and tier one \
                is forty basis points, which on your volume is real money every single month.";
    let lines = break_into_lines(long);
    assert!(lines.len() > 1, "it should breathe: {lines:?}");
}

#[test]
fn short_sentences_are_left_as_they_are() {
    let lines = break_into_lines("Stop paying tier two fees. It works.");
    assert_eq!(lines.len(), 2);
}

#[test]
fn what_atlas_says_covers_the_fit_and_the_cuts() {
    let script = vec!["One.".to_string(), "Two.".to_string()];
    let beats = vec![Beat { at: 0.0, strong: true }];
    let lines = lay_out(&script, &beats, 10.0, &cfg());
    let said = vo_spoken(&lines, &fits(&lines, 10.0, &cfg()), snapped_count(&lines, &beats));
    assert!(said.contains("2 lines"));
    assert!(said.contains("moved onto cuts") || said.contains("Fits"));
}

#[test]
fn voiceovers_are_off_until_you_turn_them_on() {
    assert!(!VoiceoverConfig::default().enabled);
}
