use atlas::nudge::{is_medical, Goal, NudgeConfig, Nudger, Part, Response, Trigger};

const DAY: u64 = 86_400;
/// A Monday-ish base so `t / 86_400` day boundaries are obvious.
const T0: u64 = 100 * DAY;

fn nudger() -> Nudger {
    Nudger::new(NudgeConfig::default())
}

/// The daypart greeting is the fallback and fires on the first call in any
/// morning/afternoon/evening. These tests are about the other triggers, so
/// they look past it rather than pretending it isn't there.
fn beyond_greeting(
    n: &mut Nudger,
    t: u64,
    hour: u8,
    dwell: u64,
    idle: u64,
) -> Option<atlas::nudge::Nudge> {
    match n.consider(t, hour, dwell, idle) {
        Some(x) if x.trigger == Trigger::Daypart => n.consider(t, hour, dwell, idle),
        other => other,
    }
}

// ================= it actually pushes =================

#[test]
fn a_goal_that_stops_moving_gets_raised() {
    let mut n = nudger();
    n.track(Goal::new("invoices", "the invoice chase", T0));
    // Same day: nothing to say yet.
    assert!(beyond_greeting(&mut n, T0 + 3600, 14, 0, 0).is_none());

    let out = beyond_greeting(&mut n, T0 + 4 * DAY, 14, 0, 0).expect("four days is a stall");
    assert_eq!(out.trigger, Trigger::Stalled);
    assert!(out.message.contains("invoice chase"));
    assert!(out.message.contains("4 days"), "{}", out.message);
}

#[test]
fn movement_resets_the_clock_and_the_silence_count() {
    let mut n = nudger();
    n.track(Goal::new("deck", "the pitch deck", T0));
    n.record(Some("deck"), Response::Ignored, T0);
    n.record(Some("deck"), Response::Ignored, T0);
    assert_eq!(n.goals[0].ignored, 2);

    n.moved("deck", T0 + 4 * DAY);
    assert_eq!(n.goals[0].ignored, 0, "getting on with it is the answer it wanted");
    assert!(beyond_greeting(&mut n, T0 + 4 * DAY, 14, 0, 0).is_none());
}

// ================= the offer to take work =================

#[test]
fn a_nudge_that_names_a_problem_also_offers_to_take_it() {
    let mut n = nudger();
    n.track(Goal::new("invoices", "the invoice chase", T0).offering("the invoice chase"));
    let out = beyond_greeting(&mut n, T0 + 4 * DAY, 14, 0, 0).unwrap();
    assert!(out.relief.is_some(), "a nudge with no relief is a nag");
    assert!(out.message.contains("I can take"));
}

#[test]
fn pointing_without_helping_has_to_be_more_certain_than_offering_help() {
    let mut bare = nudger();
    bare.track(Goal::new("a", "the thing", T0));
    let mut helpful = nudger();
    helpful.track(Goal::new("a", "the thing", T0).offering("the first draft"));

    let b = beyond_greeting(&mut bare, T0 + 4 * DAY, 14, 0, 0).unwrap();
    let h = beyond_greeting(&mut helpful, T0 + 4 * DAY, 14, 0, 0).unwrap();
    assert!(h.confidence > b.confidence);
}

#[test]
fn drifting_offers_relief_from_a_real_goal_rather_than_inventing_one() {
    let mut n = nudger();
    n.track(Goal::new("ads", "the ad review", T0).offering("the ad review"));
    let out = beyond_greeting(&mut n, T0, 14, 3600, 900).unwrap();
    assert_eq!(out.trigger, Trigger::Drifting);
    assert_eq!(out.relief.as_deref(), Some("the ad review"));
    assert!(out.message.starts_with("Looks like we've slowed down"));
}

#[test]
fn drift_needs_both_a_long_dwell_and_real_quiet() {
    // Long dwell but you are talking: that is working, not drifting. The
    // daypart greeting may still fire, so assert on the trigger rather than
    // on there being nothing at all.
    let mut busy = nudger();
    let got = busy.consider(T0 + 14 * 3600, 14, 3600, 10);
    assert_ne!(got.map(|x| x.trigger), Some(Trigger::Drifting));

    // Quiet but only just settled on the window: also not drift.
    let mut fresh = nudger();
    let got = fresh.consider(T0 + 14 * 3600, 14, 60, 900);
    assert_ne!(got.map(|x| x.trigger), Some(Trigger::Drifting));
}

// ================= silence backs off, it never repeats =================

#[test]
fn every_silence_widens_the_gap_and_nothing_narrows_it() {
    let n = nudger();
    let mut g = Goal::new("a", "the thing", T0);
    let quiet = n.wait_for(&g);
    g.ignored = 1;
    let wider = n.wait_for(&g);
    g.ignored = 2;
    let widest = n.wait_for(&g);
    assert!(wider > quiet && widest > wider);
}

#[test]
fn asking_why_happens_once_and_never_again() {
    let mut n = nudger();
    n.track(Goal::new("a", "the thing", T0).offering("the draft"));
    for _ in 0..n.cfg.ask_why_after {
        n.record(Some("a"), Response::Ignored, T0);
    }
    // Far enough out to clear the widened backoff.
    let t = T0 + 400 * DAY;
    let first = beyond_greeting(&mut n, t, 14, 0, 0).expect("silence earns one question");
    assert!(first.asking_why);
    assert!(first.message.contains("What am I getting wrong"));

    let second = beyond_greeting(&mut n, t + 400 * DAY, 14, 0, 0).unwrap();
    assert!(!second.asking_why, "asking why twice is the nagging it was meant to avoid");
}

#[test]
fn saying_no_is_believed_the_first_time() {
    let mut n = nudger();
    n.track(Goal::new("a", "the thing", T0));
    n.record(Some("a"), Response::Declined, T0);
    assert!(beyond_greeting(&mut n, T0 + 400 * DAY, 14, 0, 0).is_none());
}

#[test]
fn enough_silence_ends_it_even_without_a_refusal() {
    let mut n = nudger();
    n.track(Goal::new("a", "the thing", T0));
    for _ in 0..n.cfg.give_up_after {
        n.record(Some("a"), Response::Ignored, T0);
    }
    assert!(beyond_greeting(&mut n, T0 + 10_000 * DAY, 14, 0, 0).is_none());
}

// ================= the daypart brief =================

#[test]
fn each_daypart_greets_once_and_says_how_much_is_on() {
    let mut n = nudger();
    n.track(Goal::new("a", "one", T0));
    n.track(Goal::new("b", "two", T0));
    n.track(Goal::new("c", "three", T0));

    let m = n.consider(T0 + 8 * 3600, 8, 0, 0).unwrap();
    assert_eq!(m.trigger, Trigger::Daypart);
    assert!(m.message.starts_with("Good morning"));
    assert!(m.message.contains("3 things open"), "{}", m.message);
    assert!(m.message.contains("Where would you like to start?"));

    assert!(n.consider(T0 + 9 * 3600, 9, 0, 0).is_none(), "one morning, one greeting");
    let a = n.consider(T0 + 14 * 3600, 14, 0, 0).unwrap();
    assert!(a.message.starts_with("Good afternoon"));
}

#[test]
fn the_small_hours_get_nothing() {
    assert_eq!(Part::from_hour(3), None);
    assert_eq!(Part::from_hour(23), None);
    assert_eq!(Part::from_hour(8), Some(Part::Morning));
    let mut n = nudger();
    assert!(n.consider(T0 + 3 * 3600, 3, 0, 0).is_none());
}

// ================= what it will not touch =================

#[test]
fn habits_are_in_scope_and_medicine_is_not() {
    // You asked for habits and health. Habits are behaviour you declared.
    assert!(!is_medical("get off the machine by seven"));
    assert!(!is_medical("walk every morning"));
    // Medicine is not, and no setting reaches this.
    assert!(is_medical("that symptom you mentioned"));
    assert!(is_medical("your blood pressure reading"));
    assert!(is_medical("whether to change the dosage"));
    assert!(is_medical("the scan"));
}

#[test]
fn a_medical_goal_is_never_raised_even_with_personal_nudges_on() {
    let mut cfg = NudgeConfig::default();
    cfg.personal = true;
    let mut n = Nudger::new(cfg);
    n.track(Goal::new("personal:bp", "your blood pressure reading", T0));
    assert!(beyond_greeting(&mut n, T0 + 500 * DAY, 14, 0, 0).is_none());
}

#[test]
fn personal_nudges_are_off_until_you_turn_them_on() {
    let mut off = Nudger::new(NudgeConfig::default());
    off.track(Goal::new("personal:walk", "the daily walk", T0));
    assert!(beyond_greeting(&mut off, T0 + 5 * DAY, 14, 0, 0).is_none());

    let mut cfg = NudgeConfig::default();
    cfg.personal = true;
    let mut on = Nudger::new(cfg);
    on.track(Goal::new("personal:walk", "the daily walk", T0));
    assert!(beyond_greeting(&mut on, T0 + 5 * DAY, 14, 0, 0).is_some());
}

// ================= it goes through the same door as everything else =================

#[test]
fn a_nudge_becomes_an_ordinary_offer_so_it_can_be_declined_like_one() {
    let mut n = nudger();
    n.track(Goal::new("a", "the thing", T0).offering("the first draft"));
    let nudge = beyond_greeting(&mut n, T0 + 4 * DAY, 14, 0, 0).unwrap();
    let offer = atlas::proactive::from_nudge(&nudge);
    assert_eq!(offer.kind, "nudge_stalled");
    assert_eq!(offer.command, "the first draft");
    assert_eq!(offer.message, nudge.message);
}

#[test]
fn the_oldest_stall_is_raised_first() {
    let mut n = nudger();
    n.track(Goal::new("recent", "the recent one", T0 + 3 * DAY));
    n.track(Goal::new("ancient", "the ancient one", T0));
    let out = beyond_greeting(&mut n, T0 + 20 * DAY, 14, 0, 0).unwrap();
    assert_eq!(out.subject.as_deref(), Some("ancient"));
}

#[test]
fn a_commitment_outranks_a_greeting() {
    let mut n = nudger();
    n.track(Goal::new("a", "the thing", T0));
    // Morning, and something is stalled. You should hear about the stall.
    let out = n.consider(T0 + 8 * 3600 + 5 * DAY, 8, 0, 0).unwrap();
    assert_eq!(out.trigger, Trigger::Stalled);
}
