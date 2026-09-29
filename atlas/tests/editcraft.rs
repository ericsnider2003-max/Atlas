use atlas::editcraft::{
    check_cuts, reply_to, too_many_effects, what_they_asked, BrandAsks, Cut, EditCraftConfig,
    Transition, NOT_THE_SAME, RIGHTS_ARE_THE_PRICE, THE_PRINCIPLE,
};

#[test]
fn grid_lines_are_for_steering_the_eye_not_just_safe_zones() {
    // The idea most editors never articulate.
    let seamless = Cut { leaving_at: 0.62, arriving_at: 0.68 };
    assert!(seamless.seamless());
    assert!(seamless.note().is_none());

    let jarring = Cut { leaving_at: 0.25, arriving_at: 0.80 };
    assert!(!jarring.seamless());
    let note = jarring.note().unwrap();
    assert!(note.contains("that's the jump you can feel"));
    assert!(note.contains("the cut vanishes"));
}

#[test]
fn a_sequence_is_checked_cut_by_cut_and_only_the_bad_ones_are_named() {
    let cuts = vec![
        Cut { leaving_at: 0.5, arriving_at: 0.55 },
        Cut { leaving_at: 0.2, arriving_at: 0.9 },
        Cut { leaving_at: 0.4, arriving_at: 0.45 },
    ];
    let found = check_cuts(&cuts);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].0, 1, "the second one");
}

#[test]
fn the_easiest_way_to_level_up_editing_is_not_editing() {
    // Transitions made with the camera cost nothing in post and don't date.
    assert!(Transition::Directional.how().contains("turn your head the way you're going"));
    assert!(Transition::Directional.how().contains("Cut on the turn"));
}

#[test]
fn most_cuts_should_just_be_cuts() {
    assert!(Transition::Hard.how().contains("Most cuts should be this"));
}

#[test]
fn too_many_software_effects_is_named_with_what_to_do_instead() {
    let many: Vec<Transition> = (0..8).map(|_| Transition::InPost).collect();
    let said = too_many_effects(&many, 30.0).unwrap();
    assert!(said.contains("could be a straight cut"));
    assert!(said.contains("done in camera"));

    let few = vec![Transition::InPost, Transition::Hard, Transition::Hard];
    assert!(too_many_effects(&few, 120.0).is_none());
}

// ================= what a brand says =================

#[test]
fn the_four_things_brands_say_are_recognised() {
    assert_eq!(what_they_asked("can you send over your rate card?"), BrandAsks::RateCard);
    assert_eq!(what_they_asked("we're only a small brand, can you do it for a bit less"), BrandAsks::Cheaper);
    assert_eq!(what_they_asked("our payment terms are net 60"), BrandAsks::LatePayment);
    assert_eq!(
        what_they_asked("full perpetual worldwide usage across every platform"),
        BrandAsks::HugeRightsSmallBudget
    );
}

#[test]
fn never_quote_before_you_know_what_they_want() {
    // A number given without scope is a number you'll be held to.
    let r = reply_to(BrandAsks::RateCard).unwrap();
    assert!(r.contains("usage rights, deliverables, budget and timeline"));
    assert!(!r.contains('$') && !r.contains('£'), "no number in it");
}

#[test]
fn the_answer_to_cheaper_moves_the_scope_and_not_the_rate() {
    let r = reply_to(BrandAsks::Cheaper).unwrap();
    assert!(r.contains("can't move on the rate"));
    assert!(r.contains("adjust the scope"));
    assert!(THE_PRINCIPLE.contains("A rate you dropped once is your rate forever"));
}

#[test]
fn late_payment_gets_a_split_rather_than_an_argument() {
    assert!(reply_to(BrandAsks::LatePayment).unwrap().contains("50% up front"));
}

#[test]
fn perpetual_rights_are_countered_with_a_term_rather_than_refused() {
    // It costs them almost nothing and is worth a great deal to you.
    let r = reply_to(BrandAsks::HugeRightsSmallBudget).unwrap();
    assert!(r.contains("defining a usage term"));
    assert!(r.contains("you can always renew"));
    assert!(RIGHTS_ARE_THE_PRICE.contains("hand over without noticing"));
    // Behaviour, not just wording: this ask is met with a counter (Some),
    // where a request with no scripted answer declines (None) -- so it is
    // countered, not refused by omission.
    assert!(reply_to(BrandAsks::HugeRightsSmallBudget).is_some());
    assert!(reply_to(BrandAsks::Other).is_none(), "a genuine no-counter case still returned a reply");
}

#[test]
fn something_that_is_not_one_of_the_four_gets_no_canned_reply() {
    assert!(reply_to(BrandAsks::Other).is_none());
}

#[test]
fn atlas_drafts_these_and_never_sends_them() {
    // Your name and your money.
    assert!(!EditCraftConfig::default().may_send_replies);
    let parsed: EditCraftConfig =
        serde_yaml::from_str("enabled: true\nmay_send_replies: true\n").unwrap();
    assert!(!parsed.may_send_replies);
}

#[test]
fn an_affiliate_link_and_a_partnership_are_not_the_same_thing() {
    // The words get used interchangeably and the obligations are completely
    // different.
    assert!(NOT_THE_SAME.contains("An affiliate link is you recommending something you already use"));
    assert!(NOT_THE_SAME.contains("owing someone deliverables is the bad version of both"));
}

// ================= an affiliate deal that isn't one =================

use atlas::editcraft::{
    is_scheduling_rather_than_capturing, judge_deal, ladder, next_rung, profile_note, what_it_is,
    DealTerms, Rung, WhatItIs, AFFILIATE_IS_NOT_EXCLUSIVE,
};

fn plain_affiliate() -> DealTerms {
    DealTerms {
        exclusive: false,
        deliverables: 0,
        bio_placement: false,
        posting_quota: false,
        paid_up_front: false,
        commission_pct: 10.0,
        you_already_use_it: true,
    }
}

#[test]
fn a_real_affiliate_arrangement_asks_nothing_of_you() {
    assert_eq!(what_it_is(&plain_affiliate()), WhatItIs::RealAffiliate);
    let said = judge_deal(&plain_affiliate());
    assert!(said.contains("nothing owed"));
    // The answer to "isn't that free labour".
    assert!(said.contains("taking a cut of something you were doing for nothing"));
}

#[test]
fn obligations_without_money_up_front_is_the_one_to_walk_away_from() {
    // The line, stated plainly: it doesn't matter what the email calls it.
    let dressed = DealTerms {
        exclusive: true,
        deliverables: 2,
        bio_placement: true,
        ..plain_affiliate()
    };
    assert_eq!(what_it_is(&dressed), WhatItIs::PartnershipDressedAsAffiliate);
    let said = judge_deal(&dressed);
    assert!(said.contains("is called an affiliate deal and isn't one"));
    assert!(said.contains("exclusivity"));
    assert!(said.contains("a bio placement"));
    assert!(said.contains("a partnership you'd be doing for free"));
}

#[test]
fn obligations_with_money_is_just_a_partnership_and_is_judged_on_the_money() {
    let real = DealTerms { deliverables: 2, paid_up_front: true, ..plain_affiliate() };
    assert_eq!(what_it_is(&real), WhatItIs::RealPartnership);
    assert!(judge_deal(&real).contains("Judge it on the money"));
}

#[test]
fn a_single_obligation_is_enough_to_change_what_it_is() {
    let quota = DealTerms { posting_quota: true, ..plain_affiliate() };
    assert_eq!(what_it_is(&quota), WhatItIs::PartnershipDressedAsAffiliate);
}

#[test]
fn exclusivity_is_named_as_the_thing_an_affiliate_deal_never_has() {
    assert!(AFFILIATE_IS_NOT_EXCLUSIVE.contains("is not an exclusivity"));
    assert!(AFFILIATE_IS_NOT_EXCLUSIVE.contains("whatever they call it"));
}

// ================= the profile ladder =================

#[test]
fn the_rungs_are_in_the_order_that_actually_matters() {
    // A good bio under a bad name does less than a decent name and no bio,
    // because the name is what people see in a feed.
    let l = ladder();
    assert_eq!(l[0], Rung::Photo);
    assert_eq!(l[1], Rung::NameAndNiche);
    assert!(l.iter().position(|r| *r == Rung::NameAndNiche) < l.iter().position(|r| *r == Rung::Proof));
}

#[test]
fn it_gives_you_the_next_thing_not_a_list_of_five() {
    // A review that hands you five changes gets none of them done.
    let has = vec![Rung::Photo, Rung::NameAndNiche];
    assert_eq!(next_rung(&has), Some(Rung::WhoYouHelp));
    let said = profile_note(&has);
    assert!(said.contains("2 of 5"));
    assert!(said.contains("who this is for"));
    assert!(!said.contains("proof"), "one thing at a time");
}

#[test]
fn a_finished_profile_is_left_alone() {
    assert!(profile_note(&ladder()).contains("doing everything it can"));
}

// ================= habit, not event =================

#[test]
fn everything_arriving_on_two_days_is_a_shoot_not_a_habit() {
    // The difference between creators who grow and creators who don't.
    let mut days = vec![0u32; 28];
    days[3] = 8;
    days[17] = 9;
    let said = is_scheduling_rather_than_capturing(&days).unwrap();
    assert!(said.contains("That's a shoot, not a habit"));
    assert!(said.contains("record the moment they have the idea"));
}

#[test]
fn capturing_across_the_week_gets_no_comment() {
    let spread: Vec<u32> = (0..28).map(|i| if i % 2 == 0 { 2 } else { 1 }).collect();
    assert!(is_scheduling_rather_than_capturing(&spread).is_none());
}

#[test]
fn too_little_to_judge_says_nothing_rather_than_guessing() {
    assert!(is_scheduling_rather_than_capturing(&[1, 2, 3]).is_none());
    assert!(is_scheduling_rather_than_capturing(&vec![0u32; 28]).is_none());
}
