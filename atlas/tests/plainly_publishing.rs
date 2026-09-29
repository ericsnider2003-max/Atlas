use atlas::accounts::{
    asked_to_weaken, audit, instead, spoken as acc_spoken, stakes_for, Account, SecondFactor,
    Stakes, WONT_WEAKEN,
};
use atlas::plainly::{confirm, didnt_understand, result, understand};
use atlas::publishing::{
    description_from, export_args, export_for, format_of, ready_to_post, tags,
    where_to_get_music, Format, MusicSource, Platform, IN_APP_TRADE,
};

// ================= your words, not the jargon =================

#[test]
fn saying_you_are_not_loud_enough_is_enough() {
    // You can hear that something's wrong. Knowing the word for it is Atlas's
    // job.
    let r = understand("I'm not loud enough in this one").unwrap();
    assert!(r.target.contains("-14 LUFS"));
    assert!(r.fix.contains("duck the music"));
}

#[test]
fn something_that_could_mean_two_opposite_things_is_measured_not_guessed() {
    // "The music's too loud" and "I'm too quiet" sound identical and need
    // opposite fixes.
    let quiet = understand("I can't hear me over the music").unwrap();
    assert!(quiet.or_maybe.unwrap().contains("music being too loud"));

    let loud = understand("the music is too loud").unwrap();
    assert!(loud.or_maybe.unwrap().contains("your voice being quiet"));
    assert!(loud.or_maybe.unwrap().contains("opposite fix"));
}

#[test]
fn atlas_repeats_what_it_thinks_you_meant_before_doing_anything() {
    // Getting that wrong wastes the whole edit.
    let said = confirm(&understand("sounds muffled").unwrap());
    assert!(said.starts_with("I'll check"));
    assert!(said.contains("Should be"));
    assert!(said.contains("It might be"));
}

#[test]
fn picture_complaints_work_the_same_way() {
    assert!(understand("it's too dark").unwrap().fix.contains("lift the exposure"));
    assert!(understand("the colour looks off").unwrap().target.contains("4800"));
    assert!(understand("it looks washed out").unwrap().fix.contains("rather than by crushing"));
    assert!(understand("looks fake and oversaturated").unwrap().target.contains("1.1x"));
}

#[test]
fn something_no_edit_can_fix_says_so_rather_than_pretending() {
    assert!(understand("it's jerky").unwrap().fix.contains("nothing in the edit fixes this"));
    assert!(understand("there's an echo").unwrap().fix.contains("room problem"));
}

#[test]
fn complaining_it_drags_is_understood_as_pacing() {
    let r = understand("it drags a bit").unwrap();
    assert!(r.target.contains("inside 3 seconds"));
    assert!(r.fix.contains("15 to 20%"));
}

#[test]
fn the_number_comes_at_the_end_if_you_want_it_not_in_the_way() {
    let said = result("I'm too quiet", "you were at -21 LUFS, now -14", true);
    assert!(said.starts_with("Fixed. You were right"));
}

#[test]
fn atlas_pushing_back_when_it_measures_fine_is_a_question_not_a_refusal() {
    let said = result("too dark", "brightness is at 52%", false);
    assert!(said.contains("looks fine, actually"));
    assert!(said.contains("Want me to change it anyway?"));
}

#[test]
fn something_it_cannot_place_asks_which_part_you_mean() {
    assert!(didnt_understand("it's just off").contains("sound, the picture, or the pace"));
}

// ================= knowing where it's going =================

#[test]
fn each_platform_has_its_own_settings_and_its_own_quirk() {
    assert_eq!(export_for(Platform::TikTok).width, 1080);
    assert!(export_for(Platform::TikTok).note.contains("desktop uploader compresses harder"));
    assert!(export_for(Platform::Reels).note.contains("bottom 25%"));
    assert!(export_for(Platform::Shorts).note.contains("still a search engine"));
    assert!(export_for(Platform::XTwitter).note.contains("burn them in"));
    assert_eq!(export_for(Platform::LinkedIn).height, 1350, "4:5, not 9:16");
}

#[test]
fn the_length_that_performs_is_told_apart_from_the_length_allowed() {
    let t = export_for(Platform::TikTok);
    assert_eq!(t.max_secs, 600);
    assert!(t.sweet_spot_secs.1 < 60, "what works is nowhere near the limit");
}

#[test]
fn the_export_settings_include_the_two_things_people_get_wrong() {
    let args = export_args(&export_for(Platform::TikTok), "in.mp4", "out.mp4", true);
    assert!(args.contains(&"yuv420p".to_string()), "or it won't play on some phones");
    assert!(args.contains(&"+faststart".to_string()), "or it won't start until fully downloaded");
}

#[test]
fn stripping_metadata_is_what_the_export_actually_does() {
    // `opsec::Risk::Metadata::fix` says "strip it on export, which I do by
    // default". It didn't: these arguments carried no `-map_metadata`, so
    // ffmpeg copied the input's GPS, camera serial and timestamps into the
    // file about to be posted publicly. The setting that was supposed to
    // decide this shipped `true` and was read by nothing.
    let stripped = export_args(&export_for(Platform::TikTok), "in.mp4", "out.mp4", true);
    let at = stripped
        .iter()
        .position(|a| a == "-map_metadata")
        .expect("the export keeps the original's metadata");
    assert_eq!(stripped[at + 1], "-1", "-map_metadata needs the -1 to mean anything");
    assert_eq!(
        stripped.last().map(String::as_str),
        Some("out.mp4"),
        "the output file has to stay last or ffmpeg reads it as another flag"
    );

    // And off, for anyone who has a reason to keep it.
    let kept = export_args(&export_for(Platform::TikTok), "in.mp4", "out.mp4", false);
    assert!(!kept.contains(&"-map_metadata".to_string()));
    assert_eq!(kept.last().map(String::as_str), Some("out.mp4"));
}

#[test]
fn atlas_says_when_a_piece_is_the_wrong_length_for_where_it_is_going() {
    let said = ready_to_post(Platform::Reels, 75.0, Format::ShortForm);
    assert!(said.contains("75s"));
    assert!(said.contains("15 to 30s"));
    // Behaviour, not just wording: a well-fitted piece draws no length
    // advisory, so the too-long one's output carries extra guidance.
    let fitted = ready_to_post(Platform::Reels, 20.0, Format::ShortForm);
    assert!(fitted.len() < said.len(), "a 75s piece was not flagged as longer than a 20s one");
}

// ================= it knows what kind of thing you made =================

#[test]
fn the_formats_are_told_apart() {
    assert_eq!(format_of(30.0, true, false, false), Format::TalkingHead);
    assert_eq!(format_of(600.0, true, false, false), Format::LongForm);
    assert_eq!(format_of(45.0, true, false, true), Format::ProductReview);
    assert_eq!(format_of(60.0, false, true, false), Format::ScreenRecording);
}

#[test]
fn each_format_has_rules_that_actually_differ() {
    let review = Format::ProductReview.rules();
    assert!(review.iter().any(|r| r.contains("a review with no criticism reads as an ad")));
    assert!(review.iter().any(|r| r.contains("Everyone is waiting for the price")));

    let screen = Format::ScreenRecording.rules();
    assert!(screen.iter().any(|r| r.contains("nobody can read your full screen on a phone")));

    let long = Format::LongForm.rules();
    assert!(long.iter().any(|r| r.contains("the pace can drop")));
}

#[test]
fn a_talking_head_wants_a_different_length_from_a_tutorial() {
    assert!(Format::TalkingHead.length().1 < Format::Tutorial.length().1);
    assert!(Format::LongForm.length().0 > 400);
}

// ================= music that won't cost you the video =================

#[test]
fn the_in_app_library_is_not_safe_for_a_business_account() {
    // The thing that quietly gets people muted.
    assert!(MusicSource::InApp.safe(false));
    assert!(!MusicSource::InApp.safe(true));
    assert!(MusicSource::InApp.why(true).contains("you'd get muted"));
}

#[test]
fn a_real_song_is_never_safe_and_says_what_happens() {
    assert!(!MusicSource::Commercial.safe(false));
    assert!(MusicSource::Commercial.why(false).contains("muted, demonetised, or taken down"));
}

#[test]
fn there_are_free_sources_that_work_anywhere() {
    let sources = where_to_get_music();
    assert!(sources.iter().any(|(_, s, _)| *s == MusicSource::PublicDomain));
    assert!(sources.iter().any(|(n, _, _)| n.contains("YouTube Audio Library")));
    assert!(sources.iter().any(|(_, _, why)| why.contains("no attribution")));
}

#[test]
fn the_trade_off_of_using_the_apps_own_sound_is_stated() {
    assert!(IN_APP_TRADE.contains("helps reach"));
    assert!(IN_APP_TRADE.contains("repost it elsewhere and the audio comes off"));
}

// ================= what goes with the post =================

#[test]
fn the_description_is_built_from_what_you_actually_said() {
    // A description that doesn't match teaches the platform to show it to the
    // wrong people.
    let transcript = "Stop paying tier two fees. Mine went from forty basis points to twelve \
                      by asking one question.";
    let d = description_from(transcript, Platform::TikTok, "broker fees");
    assert!(d.contains("Stop paying tier two fees"));
    assert!(d.len() <= 92, "front-loaded — only the first line shows");
}

#[test]
fn youtube_gets_more_because_it_is_a_search_engine() {
    let d = description_from("Stop paying tier two fees. It works.", Platform::Shorts, "broker fees");
    assert!(d.contains("broker fees"));
    assert!(d.len() > description_from("Stop paying tier two fees. It works.", Platform::TikTok, "broker fees").len());
}

#[test]
fn hashtags_are_few_or_none_because_thirty_reads_as_spam() {
    assert!(tags("broker fees", Platform::TikTok).len() <= 3);
    assert!(tags("broker fees", Platform::YouTube).is_empty(), "they do nothing there");
}

// ================= where your accounts stand =================

fn accounts() -> Vec<Account> {
    vec![
        Account { site: "Gmail".into(), second_factor: SecondFactor::Sms, stakes: Stakes::Keystone,
                  reused_password: false, has_recovery_codes: false,
                  settings_url: Some("https://myaccount.google.com/security".into()) },
        Account { site: "Chase Bank".into(), second_factor: SecondFactor::App, stakes: Stakes::High,
                  reused_password: false, has_recovery_codes: false, settings_url: None },
        Account { site: "TikTok".into(), second_factor: SecondFactor::None, stakes: Stakes::High,
                  reused_password: true, has_recovery_codes: false, settings_url: None },
        Account { site: "Spotify".into(), second_factor: SecondFactor::None, stakes: Stakes::Medium,
                  reused_password: false, has_recovery_codes: false, settings_url: None },
    ]
}

#[test]
fn email_is_the_account_that_owns_all_the_others() {
    // Every other account resets through it, whatever people think of it.
    assert_eq!(stakes_for("gmail.com"), Stakes::Keystone);
    assert_eq!(stakes_for("Chase Bank"), Stakes::High);
    assert_eq!(stakes_for("Spotify"), Stakes::Medium);
}

#[test]
fn text_message_codes_are_named_as_the_weakest_kind_with_the_reason() {
    assert!(SecondFactor::Sms.concern().unwrap().contains("SIM swap"));
    assert!(SecondFactor::Sms.concern().unwrap().contains("without touching your phone"));
    assert!(SecondFactor::Email.concern().unwrap().contains("really one factor"));
    assert!(SecondFactor::App.concern().is_none());
}

#[test]
fn nothing_at_all_on_a_high_stakes_account_comes_first() {
    let a = audit(&accounts());
    assert!(a[0].what.contains("turn on two-factor") || a[0].site == "Gmail");
    assert!(a.iter().any(|x| x.site == "TikTok" && x.what.contains("turn on two-factor")));
}

#[test]
fn strong_two_factor_with_no_recovery_codes_is_flagged() {
    // How a lost phone becomes a lost account.
    let a = audit(&accounts());
    let rec = a.iter().find(|x| x.what.contains("recovery codes")).unwrap();
    assert!(rec.why.contains("lost phone becomes a lost account"));
}

#[test]
fn atlas_names_the_keystone_because_most_people_have_not_thought_about_it() {
    let accs = accounts();
    let said = acc_spoken(&accs, &audit(&accs));
    assert!(said.contains("Gmail is the one that matters most"));
    assert!(said.contains("every other account resets through it"));
}

#[test]
fn atlas_opens_the_page_and_you_make_the_change() {
    let accs = accounts();
    let said = acc_spoken(&accs, &audit(&accs));
    assert!(said.contains("I can open the page; you make the change"));
}

#[test]
fn being_asked_to_turn_two_factor_off_is_recognised() {
    assert!(asked_to_weaken("turn off two factor on my instagram"));
    assert!(asked_to_weaken("disable 2FA everywhere"));
    assert!(!asked_to_weaken("is two factor on for my bank?"));
}

#[test]
fn it_refuses_with_the_reason_rather_than_a_policy() {
    // And the reason isn't legality — they're his accounts.
    assert!(WONT_WEAKEN.contains("isn't about it being your account"));
    assert!(WONT_WEAKEN.contains("one bad instruction away"));
    assert!(WONT_WEAKEN.contains("a confirmation doesn't fix that"));
    assert!(WONT_WEAKEN.contains("Same reason I don't touch the firewall"), "consistent");
    assert!(WONT_WEAKEN.contains("I'll open any of those settings pages"), "and still helps");
}

#[test]
fn what_it_offers_instead_is_most_of_what_was_wanted() {
    let said = instead(&accounts());
    assert!(said.contains("where two-factor is on and where it isn't"));
    assert!(said.contains("3 of your 4"), "with the count: {said}");
}

#[test]
fn a_well_protected_set_of_accounts_is_left_alone() {
    let good = vec![Account {
        site: "Gmail".into(), second_factor: SecondFactor::Passkey, stakes: Stakes::Keystone,
        reused_password: false, has_recovery_codes: true, settings_url: None,
    }];
    // Behaviour, not just wording: "left alone" means the audit produced no
    // advice at all, not that the summary happens to read reassuringly.
    assert!(audit(&good).is_empty(), "a well-protected account still drew advice");
    assert!(acc_spoken(&good, &audit(&good)).contains("all reasonably protected"));
}
