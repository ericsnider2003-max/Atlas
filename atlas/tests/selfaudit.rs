use atlas::pipeline::Thought;
use atlas::selfaudit::{
    as_thought, recommend, spoken, Kind, SelfAuditConfig, Signal, WHY_IT_ASKS,
};

fn signal(kind: Kind, subject: &str, seen: u32, of: u32) -> Signal {
    Signal {
        kind,
        subject: subject.into(),
        seen,
        of,
        example: "yesterday".into(),
    }
}

#[test]
fn one_failure_is_a_bad_day_and_eleven_is_a_fault() {
    assert!(!signal(Kind::RouteFails, "the export route", 1, 20).is_a_fault());
    assert!(signal(Kind::RouteFails, "the export route", 11, 20).is_a_fault());
}

#[test]
fn the_bar_differs_by_signal_because_the_cost_of_being_wrong_does() {
    // Acting on one correction makes Atlas skittish; ignoring eleven failures
    // makes it useless.
    assert!(Kind::YouKeepCorrecting.enough() < Kind::NotUnderstood.enough());
    assert_eq!(Kind::NeverFailed.enough(), 1, "a test that never failed needs one look");
}

#[test]
fn five_out_of_six_is_a_fault_and_five_out_of_five_hundred_is_weather() {
    let bad = signal(Kind::RouteFails, "x", 5, 6);
    let noise = signal(Kind::RouteFails, "x", 5, 500);
    assert!(bad.worth() > noise.worth() * 50.0);
}

#[test]
fn a_signal_becomes_a_diagnosis_rather_than_a_complaint() {
    // A complaint says what happened. A diagnosis says why, where, and what
    // would prove it — so the cause has to differ by kind rather than being
    // one sentence with the subject swapped in.
    let route = recommend(&[signal(Kind::RouteFails, "the export route", 9, 10)], 1);
    let slow = recommend(&[signal(Kind::GotSlower, "the export route", 9, 10)], 1);

    assert_eq!(route.len(), 1);
    assert_ne!(route[0].cause, slow[0].cause, "same cause for two different faults");
    assert_ne!(route[0].proof, slow[0].proof, "same proof for two different faults");
    assert!(!route[0].where_.is_empty());
}

#[test]
fn you_correcting_the_same_thing_outranks_everything_else() {
    // It's the only signal that comes from you, so it wins even against
    // something that fails at a higher rate.
    let recs = recommend(
        &[
            signal(Kind::NeverUsed, "the overlay", 1, 1),
            signal(Kind::YouKeepCorrecting, "how I read dates", 4, 5),
        ],
        3,
    );
    assert_eq!(recs[0].where_, "how I read dates");
    assert!(recs[0].certainty > recs[1].certainty);
}

#[test]
fn a_capability_that_says_it_works_and_never_ran_is_noticed_but_never_leads() {
    // The weakest signal here.
    let recs = recommend(
        &[
            signal(Kind::NeverUsed, "the overlay", 1, 1),
            signal(Kind::RouteFails, "the export route", 9, 10),
        ],
        3,
    );
    assert!(recs[0].symptom.contains("keeps failing"));
    // 30 Sep 2026: it's about what you haven't asked for (`used`).
    assert!(recs.iter().any(|r| r.symptom.contains("the overlay")));
}

#[test]
fn a_test_that_has_never_failed_is_something_atlas_can_see_about_itself() {
    let recs = recommend(&[signal(Kind::NeverFailed, "the panel width", 1, 1)], 3);
    assert!(recs[0].cause.contains("always true"));
    assert!(recs[0].proof.contains("break"), "and the proof is to break it: {}", recs[0].proof);
}

#[test]
fn it_never_brings_more_than_a_handful() {
    // A list of thirty recommendations is a list nobody reads.
    let many: Vec<Signal> = (0..30)
        .map(|i| signal(Kind::RouteFails, &format!("route {i}"), 9, 10))
        .collect();
    assert_eq!(recommend(&many, 3).len(), 3);
    assert_eq!(SelfAuditConfig::default().most_at_once, 3);
}

#[test]
fn confidence_comes_from_the_rate_rather_than_the_count() {
    // Something that fails every time is a clearer fault than something that
    // fails often.
    let always = recommend(&[signal(Kind::RouteFails, "x", 20, 20)], 1);
    let often = recommend(&[signal(Kind::RouteFails, "y", 20, 200)], 1);
    assert!(always[0].certainty > often[0].certainty);
    assert!(always[0].certainty <= 0.95, "never certain");
}

#[test]
fn the_evidence_is_given_so_you_can_disagree_with_it_rather_than_the_conclusion() {
    // The numbers in the sentence are the real ones, not a fixed phrase —
    // change the signal and the sentence changes with it.
    let nine = recommend(&[signal(Kind::RouteFails, "the export route", 9, 10)], 1);
    assert!(nine[0].because.contains("9 times out of 10"));

    let six = recommend(&[signal(Kind::RouteFails, "the export route", 6, 7)], 1);
    assert!(six[0].because.contains("6 times out of 7"));
    assert_ne!(nine[0].because, six[0].because);
}

#[test]
fn what_it_says_is_one_thing_with_the_evidence_and_an_offer() {
    // Not a report — a report is what you write when you don't intend to fix
    // anything.
    let recs = recommend(
        &[
            signal(Kind::RouteFails, "the export route", 9, 10),
            signal(Kind::GotSlower, "opening the panel", 5, 6),
        ],
        3,
    );
    let said = spoken(&recs);
    assert!(said.contains("9 times out of 10"));
    assert!(said.contains("1 other things"));
    assert!(said.ends_with("Want me to have a go?"));
}

#[test]
fn nothing_worth_changing_says_so_in_one_line() {
    assert_eq!(spoken(&[]), "Nothing about myself I'd change.");
}

#[test]
fn a_recommendation_goes_into_the_loop_rather_than_into_a_document() {
    let recs = recommend(&[signal(Kind::RouteFails, "src/route.rs", 9, 10)], 1);
    let t: Thought = as_thought(&recs[0]);
    assert_eq!(t.where_, "src/route.rs");
    assert!(!t.cause.is_empty());
    assert!(!t.proof.is_empty());
}

#[test]
fn a_diagnosis_atlas_wrote_is_not_trusted_more_than_one_you_wrote() {
    // The pipeline still makes it prove the test fails first.
    let recs = recommend(&[signal(Kind::RouteFails, "src/route.rs", 9, 10)], 1);
    let t = as_thought(&recs[0]);
    assert!(!t.proof_fails_now, "assumed, and it must not be");
    assert!(t.is_thought_through().is_err(), "so it can't start yet");
}

#[test]
fn atlas_does_not_act_on_its_own_diagnosis_by_default() {
    // A system that writes its own diagnosis and then acts on it has no
    // outside check on either half.
    assert!(!SelfAuditConfig::default().act_without_asking);
    assert!(WHY_IT_ASKS.contains("no outside check on either half"));
    assert!(WHY_IT_ASKS.contains("I'm worse than you at knowing whether fixing one is worth the risk"));
}

// ================= the page =================

use atlas::hub::{recommendations_page, route, Page};

#[test]
fn recommendations_have_a_page_rather_than_interrupting() {
    // None of this is urgent — it has waited this long and can wait until
    // you're looking.
    assert_eq!(route("/hub/recommendations"), Some(Page::Recommendations));
}

#[test]
fn each_one_carries_its_evidence_so_you_can_disagree_with_that() {
    let recs = recommend(&[signal(Kind::RouteFails, "the export route", 9, 10)], 3);
    let html = recommendations_page(&recs, Some("how it decides things"), &[]);
    assert!(html.contains("keeps failing"));
    assert!(html.contains("9 times out of 10"));
    assert!(html.contains("What would prove it"));
}

#[test]
fn certainty_is_a_word_because_a_percentage_on_a_self_assessment_is_false_precision() {
    let sure = recommend(&[signal(Kind::RouteFails, "x", 20, 20)], 1);
    let unsure = recommend(&[signal(Kind::NeverUsed, "y", 1, 40)], 1);
    let a = recommendations_page(&sure, None, &[]);
    let b = recommendations_page(&unsure, None, &[]);
    assert!(a.contains("fairly sure"));
    assert!(b.contains("not certain"));
    // The number itself never reaches the page.
    assert!(!a.contains(&format!("{:.2}", sure[0].certainty)), "no false precision");
}

#[test]
fn the_page_says_what_atlas_is_already_fixing_without_asking() {
    let granted = recommendations_page(&[], Some("how it decides things"), &[]);
    assert!(granted.contains("on my own and tell you after"));

    let none = recommendations_page(&[], None, &[]);
    assert!(none.contains("not fixing anything on my own"));
}

#[test]
fn every_recommendation_can_be_dismissed_as_well_as_started() {
    // Otherwise the only way to clear one is to let Atlas do it.
    let recs = recommend(&[signal(Kind::RouteFails, "the export route", 9, 10)], 1);
    let html = recommendations_page(&recs, None, &[]);
    assert!(html.contains("Have a go"));
    assert!(html.contains("Not worth it"));
}

#[test]
fn nothing_to_recommend_is_one_line_rather_than_an_empty_table() {
    assert!(recommendations_page(&[], None, &[]).contains("Nothing I'd change"));
}
