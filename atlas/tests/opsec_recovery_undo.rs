use atlas::opsec::{
    check, spoken as op_spoken, OpsecConfig, Risk, NOT_ITS_BUSINESS,
};
use atlas::recovery::{
    gaps, spoken as rec_spoken, suggest, RecoveryConfig, Route, Setup, NOT_ATLAS, TEST_IT,
};
use atlas::undo::{reverse, say, tell, understand, Asking, History, Reversal, Undo};

const DAY: u64 = 86_400;

// ================= what's in the frame, until it isn't your problem =================

fn serving() -> OpsecConfig {
    OpsecConfig { enabled: true, applies_until: "2027-06-14".into(), ..Default::default() }
}

// Both tests below were written before `opsec.rs` split its word lists into
// unambiguous and ambiguous. They used to pass "I deploy next month" and
// "outside the hangar" bare and expect a hit -- which, under the split, is
// exactly the false positive the split exists to stop: "deploy" is what a
// developer does to a server on an ordinary Tuesday, and "hangar" and "base"
// and "post" are ordinary English. `tests/opsec.rs` is where that design is
// specified in full, and it passes; these two were simply never updated with
// it, and sat failing.
//
// Kept rather than deleted, because what each was really testing is still
// worth holding: that movement is caught, that a place is caught, and that
// each risk's `fix()` says the useful thing. Each now corroborates the
// ambiguous term the way a real flagged post would, and each has gained the
// negative half -- the bare sentence finding nothing -- so the split itself
// is guarded here too rather than only in the other file.

#[test]
fn saying_when_you_are_moving_is_the_thing_that_matters_most() {
    // People say it without thinking, because it feels like ordinary
    // conversation. "deploy" is ambiguous on its own -- it needs the rest of
    // the sentence to actually be about a movement before it counts.
    let bare = check("Quick one before I deploy next month", &[], &serving(), "2026-09-06");
    assert!(
        bare.is_empty(),
        "\"deploy\" alone is what a developer says about a server: {bare:?}"
    );

    let found = check(
        "Quick one before I deploy next month -- wheels up on the 3rd",
        &[],
        &serving(),
        "2026-09-06",
    );
    assert_eq!(found[0].risk, Risk::Movement);
    assert!(found[0].detail.contains("when you're moving"));
    assert!(found.iter().any(|f| f.detail.contains("deploy")), "{found:?}");
    assert!(Risk::Movement.fix().contains("say it after, not before"));
}

#[test]
fn something_that_places_you_is_caught() {
    let bare = check("Filmed this outside the hangar", &[], &serving(), "2026-09-06");
    assert!(bare.is_empty(), "a hangar is also where light aircraft live: {bare:?}");

    // In uniform, the same sentence is about where you are stationed.
    let uniformed = OpsecConfig { in_uniform: true, ..serving() };
    let found = check("Filmed this outside the hangar", &[], &uniformed, "2026-09-06");
    assert!(found.iter().any(|f| f.risk == Risk::Location), "{found:?}");
    assert!(Risk::Location.fix().contains("a blur still leaves the shape"));
    assert!(!Risk::Location.fixable_in_edit());
}

#[test]
fn a_patch_in_frame_is_reported_with_where() {
    let visible = vec![(Risk::Insignia, 14.0, "unit patch readable on the left shoulder".to_string())];
    let found = check("Nothing sensitive said", &visible, &serving(), "2026-09-06");
    let said = op_spoken(&found, &serving(), "2026-09-06");
    assert!(said.contains("at 14s"));
    assert!(said.contains("blur it, or reframe above the chest"));
}

#[test]
fn a_badge_or_a_screen_outranks_a_patch() {
    let visible = vec![
        (Risk::Insignia, 3.0, "rank".to_string()),
        (Risk::Credential, 9.0, "ID card on the desk".to_string()),
    ];
    let found = check("", &visible, &serving(), "2026-09-06");
    assert_eq!(found[0].risk, Risk::Credential);
}

#[test]
fn uniform_next_to_a_brand_deal_is_flagged_only_when_you_are_in_uniform() {
    let plain = check("Use my code for 20% off", &[], &serving(), "2026-09-06");
    assert!(!plain.iter().any(|f| f.risk == Risk::UniformContext));

    let uniformed = OpsecConfig { in_uniform: true, ..serving() };
    let found = check("Use my code for 20% off", &[], &uniformed, "2026-09-06");
    assert!(found.iter().any(|f| f.risk == Risk::UniformContext));
}

#[test]
fn location_data_is_stripped_from_every_export_regardless() {
    // Cheap, invisible, and it catches people who did everything else right.
    assert!(OpsecConfig::default().always_strip_metadata);
}

#[test]
fn after_your_date_atlas_says_nothing_about_any_of_it() {
    // A system that keeps applying rules you're out from under is one you
    // learn to ignore.
    let cfg = serving();
    assert!(cfg.still_applies("2026-09-06"));
    assert!(!cfg.still_applies("2027-08-01"));

    let after = check("I deploy next month from the hangar", &[], &cfg, "2027-08-01");
    assert!(after.is_empty());
    assert_eq!(op_spoken(&after, &cfg, "2027-08-01"), "");
}

#[test]
fn your_own_words_are_not_leaks() {
    let mine = OpsecConfig { not_a_leak: vec!["range".into()], ..serving() };
    let found = check("Out at the range today", &[], &mine, "2026-09-06");
    assert!(!found.iter().any(|f| f.risk == Risk::Location));
}

#[test]
fn atlas_does_not_read_your_posts_for_opinions() {
    // This was `atlas_checks_the_frame_not_your_opinions`, and it asserted
    // that `NOT_ITS_BUSINESS` contains "not what you're saying" — which it
    // did, and which was the wrong way round. **Nothing in `src/` produces a
    // `Risk`**, so `check`'s `visible` list is always empty and the frame
    // loop has never run; the only half that runs is the transcript scan,
    // i.e. what you're saying. The sentence disclaimed the one thing it does.
    //
    // The half of the promise that was always true is the one about
    // opinions, and that is what this pins now. See
    // `tests/it_says_which_half_it_looks_at.rs`.
    assert!(
        !NOT_ITS_BUSINESS.contains("not what you're saying"),
        "it is disclaiming the only half it actually does: {NOT_ITS_BUSINESS}"
    );
    assert!(NOT_ITS_BUSINESS.contains("What you think about anything is yours"));
    assert!(
        NOT_ITS_BUSINESS.contains("opinions"),
        "the line that matters was dropped: {NOT_ITS_BUSINESS}"
    );
}

// ================= a way into the vault that isn't you =================

fn rcfg() -> RecoveryConfig {
    RecoveryConfig { enabled: true, ..Default::default() }
}

#[test]
fn no_way_in_but_you_is_the_top_of_the_list() {
    let g = gaps(&[], &rcfg(), 0);
    assert!(g[0].what.contains("no way into the vault but you"));
    assert!(g[0].why.contains("including the recovery codes"));
}

#[test]
fn untested_recovery_is_a_plan_not_recovery() {
    let old = Setup {
        route: Route::SealedEnvelope,
        with: "the envelope at home".into(),
        set_up_at: 0,
        last_checked: None,
        used_at: None,
    };
    let g = gaps(&[old], &rcfg(), 400 * DAY);
    assert!(g.iter().any(|x| x.why.contains("envelopes get thrown out")));
    assert!(TEST_IT.contains("actually use it once"));
}

#[test]
fn a_route_having_been_used_is_the_loudest_thing_there_is() {
    let used = Setup {
        route: Route::SealedEnvelope,
        with: "the envelope".into(),
        set_up_at: 0,
        last_checked: Some(0),
        used_at: Some(500),
    };
    let g = gaps(&[used], &rcfg(), 600);
    assert_eq!(g[0].urgency, 1.0);
    assert!(g[0].why.contains("change the passphrase"));
}

#[test]
fn a_split_route_means_no_single_person_can_open_it() {
    let split = Route::SplitBetweenPeople { pieces: 3, needed: 2 };
    assert!(split.needs_more_than_one_person());
    assert!(!Route::OnePerson.needs_more_than_one_person());
    assert!(split.plain().contains("any 2 of them"));
}

#[test]
fn every_route_states_its_own_weakness() {
    for r in [
        Route::SealedEnvelope,
        Route::SplitBetweenPeople { pieces: 3, needed: 2 },
        Route::OnePerson,
        Route::SecondKeyFile,
    ] {
        assert!(!r.honest_weakness().is_empty());
    }
    assert!(Route::OnePerson.honest_weakness().contains("any time, not only when you'd want"));
    assert!(Route::SplitBetweenPeople { pieces: 3, needed: 2 }
        .honest_weakness()
        .contains("exactly what's hard in an emergency"));
}

#[test]
fn what_is_suggested_depends_on_how_you_actually_live() {
    let away = suggest(true, false);
    assert_eq!(away[0].0, Route::SealedEnvelope);
    assert!(away[0].1.contains("no signal, no phone and no coordination"));

    let reachable = suggest(false, true);
    assert!(reachable.iter().any(|(r, _)| r.needs_more_than_one_person()));
}

#[test]
fn you_would_know_if_the_envelope_had_been_opened() {
    assert!(Route::SealedEnvelope.visible_if_used());
    assert!(!Route::OnePerson.visible_if_used());
}

#[test]
fn atlas_is_never_one_of_the_routes() {
    assert!(!RecoveryConfig::default().atlas_can_use_these);
    assert!(NOT_ATLAS.contains("the passphrase would be decoration"));
}

#[test]
fn two_routes_that_are_both_one_person_is_worth_mentioning() {
    let two = vec![
        Setup { route: Route::OnePerson, with: "brother".into(), set_up_at: 0, last_checked: Some(0), used_at: None },
        Setup { route: Route::OnePerson, with: "mum".into(), set_up_at: 0, last_checked: Some(0), used_at: None },
    ];
    let g = gaps(&two, &rcfg(), 10);
    assert!(g.iter().any(|x| x.what.contains("any one of them can open it alone")));
    // Behaviour, not just wording: the gap is specifically about both being
    // single-person. Make one a split route and it stops firing.
    let mixed = vec![
        Setup { route: Route::OnePerson, with: "brother".into(), set_up_at: 0, last_checked: Some(0), used_at: None },
        Setup { route: Route::SplitBetweenPeople { pieces: 3, needed: 2 }, with: "family".into(), set_up_at: 0, last_checked: Some(0), used_at: None },
    ];
    assert!(
        gaps(&mixed, &rcfg(), 10).len() < g.len(),
        "the single-person gap fired even with a split route present"
    );
}

#[test]
fn everything_in_order_says_so_briefly() {
    let good = vec![
        Setup { route: Route::SealedEnvelope, with: "envelope".into(), set_up_at: 0, last_checked: Some(0), used_at: None },
        Setup { route: Route::SplitBetweenPeople { pieces: 3, needed: 2 }, with: "family".into(), set_up_at: 0, last_checked: Some(0), used_at: None },
    ];
    // Behaviour, not just wording: "all checked" is only honest if nothing is
    // flagged, so the gap list behind the sentence is actually empty.
    assert!(gaps(&good, &rcfg(), 10).is_empty(), "an in-order setup still flagged a gap");
    assert!(rec_spoken(&good, &rcfg(), 10).contains("2 ways in besides you, all checked"));
}

// ================= what did you do, and undo it =================

fn history() -> History {
    let mut h = History::default();
    h.note("moved 12 PDFs into Documents/Finance", "files",
        Undo::Atlas("they're in the trash, I'd put them back".into()), true, 100);
    h.note("set the wallpaper", "settings", Undo::Atlas("the old one is saved".into()), false, 200);
    h.note("archived 40 newsletters", "mail", Undo::Atlas("they're in Archive".into()), true, 300);
    h
}

#[test]
fn you_do_not_have_to_remember_a_phrase() {
    // The moment you need this is the moment you're least inclined to recall
    // the right wording.
    for said in [
        "what did you do", "what have you done", "what changed", "what just happened",
        "did you do something", "what did I miss",
    ] {
        assert!(matches!(understand(said), Asking::WhatDidYouDo { .. }), "{said}");
    }
    for said in ["undo", "put it back", "change it back", "no go back", "that was wrong"] {
        assert!(matches!(understand(said), Asking::UndoLast), "{said}");
    }
}

#[test]
fn naming_a_part_of_your_world_narrows_it() {
    assert_eq!(understand("undo what you did to my files"), Asking::UndoIn("files".into()));
    assert_eq!(understand("put my wallpaper back"), Asking::UndoIn("settings".into()));
    assert_eq!(understand("undo the email thing"), Asking::UndoIn("mail".into()));
}

#[test]
fn how_far_back_you_meant_is_taken_from_how_you_said_it() {
    assert!(matches!(understand("what did you do today"), Asking::WhatDidYouDo { since_mins: 1440 }));
    assert!(matches!(
        understand("what happened while I was out"),
        Asking::WhatDidYouDo { since_mins: 720 }
    ));
}

#[test]
fn a_flat_list_of_forty_moves_is_not_an_answer_so_it_groups_them() {
    let h = history();
    let said = tell(&h.since(0));
    assert!(said.starts_with("Last thing: archived 40 newsletters"));
    assert!(said.contains("to your files"));
    assert!(said.contains("Say undo"));
}

#[test]
fn things_atlas_did_on_its_own_are_counted_separately() {
    let h = history();
    assert!(tell(&h.since(0)).contains("1 of those I did on my own"));
    assert_eq!(h.on_its_own(0).len(), 1);
}

#[test]
fn undo_takes_the_last_thing_and_confirms_before_doing_it() {
    // Undoing the wrong thing is its own mistake, and the last thing Atlas
    // did isn't always the thing you're annoyed about.
    let h = history();
    match reverse(h.last()) {
        Reversal::CanDo { confirm, .. } => {
            assert!(confirm.contains("Undo \"archived 40 newsletters\"?"));
            assert!(confirm.contains("they're in Archive"));
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn naming_an_area_undoes_the_right_thing_rather_than_the_last_thing() {
    let h = history();
    // Behaviour, not just wording: the files entry it reverses is genuinely a
    // different action from the last thing done (the mail archive).
    assert_ne!(
        h.last_in("files").unwrap().id,
        h.last().unwrap().id,
        "naming files undid the last thing, not the files thing"
    );
    match reverse(h.last_in("files")) {
        Reversal::CanDo { what, .. } => assert!(what.contains("12 PDFs")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn something_only_you_can_undo_says_where() {
    let mut h = History::default();
    h.note("turned two-factor off on TikTok", "security",
        Undo::You("on TikTok's own settings page — I'll open it".into()), true, 100);
    // Behaviour, not just wording: it is classified as over-to-you, not as
    // something Atlas offers to do itself.
    assert!(
        matches!(reverse(h.last()), Reversal::OverToYou { .. }),
        "a you-only undo was offered as something Atlas could do"
    );
    let said = say(&reverse(h.last()));
    assert!(said.contains("I can't take back"));
    assert!(said.contains("I'll open it"));
}

#[test]
fn something_gone_says_why_rather_than_just_no() {
    let mut h = History::default();
    h.note("emptied the trash", "files", Undo::Cannot("the files are gone".into()), true, 100);
    assert!(say(&reverse(h.last())).contains("can't be undone: the files are gone"));
}

#[test]
fn something_already_undone_is_not_offered_again() {
    let mut h = history();
    let id = h.last().unwrap().id;
    assert!(h.mark_undone(id));
    assert_ne!(h.last().unwrap().id, id);
}

#[test]
fn nothing_to_undo_says_so() {
    assert_eq!(say(&reverse(None)), "Nothing to undo.");
    assert_eq!(tell(&[]), "Nothing.");
}

#[test]
fn the_history_stays_bounded() {
    let mut h = History::default();
    for i in 0..3000 {
        h.note("x", "files", Undo::Cannot("y".into()), true, i);
    }
    assert!(h.done.len() <= 2000);
}
