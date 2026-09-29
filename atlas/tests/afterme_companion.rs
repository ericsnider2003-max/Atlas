use atlas::afterme::{
    suggest_for_you, AfterMeConfig, Arrangement, Instruction, Timer, When, Where, NOT_ATLAS,
    THE_SHAPE, USE_THE_PLATFORMS, WHY_PAPER,
};

/// What's missing, asked the way the running program asks it.
///
/// These called the free `afterme::gaps` directly until 19 Sep 2026, when the
/// module got somewhere to keep what you have actually arranged. `gaps` is
/// private now and `Arrangement::gaps` is the way in, which is what `atlas
/// afterme` and the daemon use -- so these tests exercise the reachable path
/// rather than one beside it.
fn gaps(
    kind: Option<Where>,
    instructions: &[Instruction],
    timer: Option<Timer>,
    cfg: &AfterMeConfig,
) -> Vec<atlas::afterme::Gap> {
    Arrangement {
        kind,
        instructions: instructions.to_vec(),
        timer,
        reviewed_at: 0,
    }
    .gaps(cfg)
}
use atlas::companion::{
    how_they_talk, merge, never_travels, on_return, CompanionConfig, Merge, Pending, Phone, Piece,
    WHAT_IT_CANNOT_DO, WHY_NOT_TWO_ATLASES,
};

const DAY: u64 = 86_400;

// ================= the envelope =================

#[test]
fn it_is_paper_and_the_reason_is_that_digital_copies_itself() {
    assert!(WHY_PAPER.contains("Anything digital syncs"));
    assert!(WHY_PAPER.contains("a copy you didn't decide to make"));
    assert!(WHY_PAPER.contains("you can tell if the seal is broken"));
}

#[test]
fn the_shape_keeps_it_yours_first() {
    assert!(THE_SHAPE.contains("in your safe"));
    assert!(THE_SHAPE.contains("only where it is and when to open it"));
    assert!(THE_SHAPE.contains("nobody has been handed anything"));
}

#[test]
fn what_a_person_is_told_contains_no_secret() {
    let i = Instruction {
        person: "brother".into(),
        location: "in the safe, top shelf".into(),
        when: When::OutOfContact { days: 180 },
        then_what: "Open it and follow what's inside.".into(),
        they_know: true,
    };
    let told = i.as_told();
    assert!(told.contains("if I'm out of contact for 180 days"));
    assert!(told.contains("in the safe, top shelf"));

    // This asserted `!i.gives_anything_away_now()`, a method that returned
    // `false` for every instruction ever built — a bool nobody read, which is
    // not a boundary. The guarantee is held by the type instead, and checked
    // where it is enforced: an `Instruction` is serialised whole, and what
    // comes out is a person, a place, a condition and what to do. There is
    // nowhere in it to put a secret.
    let whole = serde_json::to_string(&i).expect("an instruction serialises");
    for field in ["person", "location", "when", "then_what", "they_know"] {
        assert!(whole.contains(field), "{field} is missing from {whole}");
    }
    for secret in ["passphrase", "password", "secret", "recovery", "key"] {
        assert!(
            !whole.contains(secret),
            "an instruction has somewhere to put a {secret}: {whole}"
        );
    }
}

#[test]
fn each_place_states_what_is_wrong_with_it() {
    assert!(Where::YoursAlone.the_catch().contains("useless to anyone if they can't get into your house"));
    assert!(Where::SealedWithSomeone.the_catch().contains("could open it any evening they felt like it"));
    assert!(Where::BankBox.the_catch().contains("usually needs probate, which takes months"));
    assert!(Where::SplitPhysically.the_catch().contains("two of them to be reachable at once"));
}

#[test]
fn keeping_it_yourself_means_nobody_can_open_it_without_you() {
    assert!(!Where::YoursAlone.openable_without_you());
    assert!(Where::SealedWithSomeone.openable_without_you());
}

#[test]
fn no_envelope_at_all_is_the_top_of_the_list() {
    let g = gaps(None, &[], None, &AfterMeConfig::default());
    // Behaviour, not just wording: "the top of the list" means it is ranked at
    // maximum urgency, ahead of every other gap.
    assert_eq!(g[0].urgency, 1.0, "no envelope was not ranked as the most urgent gap");
    assert!(g[0].what.contains("no envelope"));
    assert!(g[0].why.contains("including the recovery codes"));
}

#[test]
fn an_envelope_nobody_can_find_is_the_same_as_no_envelope() {
    let g = gaps(Some(Where::YoursAlone), &[], Some(Timer::ThePlatforms), &AfterMeConfig::default());
    // Behaviour, not just wording: an envelope nobody can find is ranked as
    // nearly as severe as having none (1.0), not quietly treated as sorted.
    assert!(!g.is_empty() && g[0].urgency >= 0.9, "a lost envelope was not treated as a top-tier gap");
    assert!(g.iter().any(|x| x.what.contains("nobody knows where it is")));
}

#[test]
fn an_arrangement_someone_has_not_been_told_about_is_not_one() {
    let untold = Instruction {
        person: "brother".into(),
        location: "the safe".into(),
        when: When::OutOfContact { days: 180 },
        then_what: "".into(),
        they_know: false,
    };
    let g = gaps(Some(Where::YoursAlone), &[untold], Some(Timer::ThePlatforms), &AfterMeConfig::default());
    assert!(g.iter().any(|x| x.what.contains("hasn't been told")));
    // Behaviour, not just wording: telling the person is what removes the gap.
    // The same arrangement with `they_know: true` leaves nothing flagged.
    let told = Instruction {
        person: "brother".into(),
        location: "the safe".into(),
        when: When::OutOfContact { days: 180 },
        then_what: "".into(),
        they_know: true,
    };
    assert!(
        gaps(Some(Where::YoursAlone), &[told], Some(Timer::ThePlatforms), &AfterMeConfig::default()).is_empty(),
        "a told arrangement was still flagged as untold"
    );
}

#[test]
fn atlas_cannot_be_the_timer_and_says_why_rather_than_failing_silently() {
    // The laptop is off for six months, so the count would never run.
    assert!(!Timer::Atlas.survives_your_absence());
    assert!(Timer::Atlas.why().contains("nothing would ever fire"));

    let g = gaps(
        Some(Where::YoursAlone),
        &[Instruction {
            person: "brother".into(), location: "the safe".into(),
            when: When::OutOfContact { days: 180 }, then_what: "".into(), they_know: true,
        }],
        Some(Timer::Atlas),
        &AfterMeConfig::default(),
    );
    assert!(g.iter().any(|x| x.what.contains("count runs on the laptop")));
}

#[test]
fn the_platforms_own_timers_are_the_right_answer_and_cost_nothing() {
    assert!(Timer::ThePlatforms.survives_your_absence());
    assert!(Timer::ThePlatforms.why().contains("keeps counting whether any device of yours is on"));
    assert!(USE_THE_PLATFORMS.contains("Inactive Account Manager"));
    assert!(USE_THE_PLATFORMS.contains("release nothing but a note saying where the envelope is"));
}

#[test]
fn a_bank_box_may_be_slower_than_your_condition_allows() {
    let cfg = AfterMeConfig { after_days: 180, ..Default::default() };
    let g = gaps(
        Some(Where::BankBox),
        &[Instruction {
            person: "brother".into(), location: "the box".into(),
            when: When::OutOfContact { days: 180 }, then_what: "".into(), they_know: true,
        }],
        Some(Timer::ThePlatforms),
        &cfg,
    );
    assert!(g.iter().any(|x| x.why.contains("probate can run months")));
}

#[test]
fn what_is_suggested_keeps_it_in_your_hands_first() {
    let s = suggest_for_you();
    assert_eq!(s[0].0, Where::YoursAlone);
    assert!(s[0].1.contains("nothing changes hands"));
    assert!(s.iter().any(|(w, _)| *w == Where::SplitPhysically));
}

#[test]
fn atlas_never_holds_the_passphrase() {
    // This asserted `!AfterMeConfig::default().atlas_holds_it` -- a
    // `#[serde(skip)]` bool pinned false that nothing read. Deleted 19 Sep
    // 2026 when the module was wired: a bool nobody reads is not a boundary,
    // and nothing would have consulted it before deciding.
    //
    // What holds the guarantee is the shape of the types. Every field of
    // everything this module keeps is named here, and none of them is
    // somewhere a passphrase could go. `tests/the_envelope.rs` checks the
    // stored form of the same thing.
    let fields = format!("{:?}", AfterMeConfig::default());
    for could_hold_one in ["passphrase", "password", "secret", "key"] {
        assert!(!fields.contains(could_hold_one), "{fields}");
    }
    assert!(NOT_ATLAS.contains("the envelope would be pointless"));
}

#[test]
fn the_default_wait_is_longer_than_a_deployment() {
    assert!(AfterMeConfig::default().after_days >= 180);
}

// ================= Atlas without your laptop =================

fn cfg() -> CompanionConfig {
    CompanionConfig { enabled: true, ..Default::default() }
}

#[test]
fn nothing_that_opens_something_else_travels_to_the_phone() {
    // A phone gets left in a taxi.
    assert!(!CompanionConfig::default().mirrors_secrets);
    let never = never_travels();
    assert!(never.iter().any(|(w, _)| *w == "the vault"));
    assert!(never.iter().any(|(w, _)| *w == "your credentials"));
    assert!(never.iter().any(|(w, why)| *w == "the index of your files" && why.contains("a map of everything you have")));
}

#[test]
fn code_counts_stay_on_the_laptop_too() {
    assert!(!Piece::CodeCounts.safe_on_a_phone());
    assert!(Piece::Outstanding.safe_on_a_phone());
}

#[test]
fn capturing_works_with_the_laptop_off_which_is_the_point() {
    let mut p = Phone::default();
    p.capture("chase the certification when I'm back", Piece::Notes, 0);
    p.capture("idea about the fee tiers", Piece::Notes, 100);
    assert_eq!(p.waiting().len(), 2);
    assert!(cfg().queue_while_offline);
}

#[test]
fn the_phone_is_honest_about_how_old_its_picture_is() {
    // Acting on a six-month-old task list is worse than having none.
    let mut p = Phone::default();
    p.mirrored_at = Some(0);
    assert!(p.state(2 * DAY, &cfg()).contains("Last synced 2 days ago"));
    assert!(p.state(2 * DAY, &cfg()).contains("treat this as a snapshot"));
    assert!(p.state(300 * DAY, &cfg()).contains("some of it will be wrong"));
}

#[test]
fn a_fresh_mirror_says_so_briefly() {
    let mut p = Phone::default();
    p.mirrored_at = Some(0);
    assert_eq!(p.state(0, &cfg()), "Up to date.");
}

#[test]
fn notes_always_merge_because_adding_one_cannot_clash() {
    // Which is what avoids the hard problem entirely.
    let pending = vec![
        Pending { id: 1, what: "a note".into(), piece: Piece::Notes, at: 0, landed: false },
        Pending { id: 2, what: "another".into(), piece: Piece::Thread, at: 0, landed: false },
    ];
    assert_eq!(merge(&pending, &["a note".into()]), Merge::TakeItAll { count: 2 });
}

#[test]
fn only_a_real_clash_needs_you() {
    let pending = vec![
        Pending { id: 1, what: "finish the VPS audit".into(), piece: Piece::Outstanding, at: 0, landed: false },
        Pending { id: 2, what: "a note".into(), piece: Piece::Notes, at: 0, landed: false },
    ];
    match merge(&pending, &["finish the VPS audit".into()]) {
        Merge::NeedsYou { clean, clashes } => {
            assert_eq!(clean, 1);
            assert_eq!(clashes, vec!["finish the VPS audit"]);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn coming_back_takes_things_in_in_the_order_you_said_them() {
    let m = Merge::TakeItAll { count: 14 };
    let said = on_return(&m, 5);
    assert!(said.contains("14 things from your phone"));
    assert!(said.contains("in the order you said them"));
}

#[test]
fn coming_back_after_a_long_absence_flags_its_own_state_as_stale() {
    let said = on_return(&Merge::TakeItAll { count: 3 }, 180);
    assert!(said.contains("gone 180 days"));
    assert!(said.contains("mid-way through as stale"));
}

#[test]
fn nothing_captured_produces_nothing_to_say() {
    assert_eq!(on_return(&Merge::Nothing, 2), "");
}

#[test]
fn they_talk_without_a_server_and_the_cloud_option_fits_your_case() {
    let ways = how_they_talk();
    assert_eq!(ways.len(), 3);
    assert!(ways.iter().all(|(_, _, free)| *free));
    let cloud = ways.iter().find(|(n, _, _)| n.contains("cloud")).unwrap();
    assert!(cloud.1.contains("never on together, which is your case"));
    assert!(cloud.1.contains("encrypted"));
}

#[test]
fn the_earlier_window_only_reasoning_is_marked_as_wrong_rather_than_left_standing() {
    assert!(WHY_NOT_TWO_ATLASES.contains("was wrong"));
    assert!(WHY_NOT_TWO_ATLASES.contains("See sync.rs"));
    assert!(WHAT_IT_CANNOT_DO.contains("Everything else is the same Atlas"));
    assert!(WHAT_IT_CANNOT_DO.contains("the device you're most likely to lose"));
}

#[test]
fn some_things_can_be_changed_on_the_phone_and_some_only_read() {
    assert!(Piece::Notes.writable());
    assert!(Piece::Outstanding.writable());
    assert!(!Piece::Projects.writable());
    assert!(!Piece::CodeCounts.writable());
}

#[test]
fn an_arrangement_you_cannot_reach_in_time_is_reported_as_a_gap() {
    // `gaps` used to hand-write `w == Where::BankBox` where it meant "this
    // cannot be reached while you are unavailable". Same test today, two
    // copies of one fact, and the named predicate had no caller at all --
    // the compiler said so the moment it stopped being `pub`.
    //
    // The assertion is on the *behaviour*, not the variant: a `Where` that
    // cannot be reached without you, with a condition shorter than probate
    // takes, must produce a gap. That stays true if another such arrangement
    // is ever added, which is the failure the hand-written comparison had.
    use atlas::afterme::{AfterMeConfig, Instruction, When, Where};

    let cfg = AfterMeConfig { after_days: 90, ..Default::default() };
    let told = vec![Instruction {
        person: "Sam".into(),
        location: "the safe in the study".into(),
        when: When::OutOfContact { days: 90 },
        then_what: "open it and follow the note".into(),
        they_know: true,
    }];

    let slow = gaps(Some(Where::BankBox), &told, None, &cfg);
    assert!(
        slow.iter().any(|g| g.what.contains("bank box")),
        "an arrangement that cannot be reached in time reported no gap: {slow:#?}"
    );

    let reachable = gaps(Some(Where::YoursAlone), &told, None, &cfg);
    assert!(
        !reachable.iter().any(|g| g.what.contains("bank box")),
        "an arrangement that can be reached reported the slow-access gap anyway"
    );
}
