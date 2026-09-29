//! Making a slow detector feel instant.
//!
//! None of this needs a model. The parts of hand tracking that decide whether
//! it feels good are not the parts that recognise a hand — a ten-frames-a-
//! second detector with prediction feels better than a thirty-frames-a-second
//! one without it.

use atlas::handtrack::{Pace, PaceConfig, SmoothConfig, Track, PREDICT_FOR_MS};

fn cfg() -> SmoothConfig {
    SmoothConfig::default()
}

// ---------------------------------------------------------------------------
// Steady when still, responsive when moving
// ---------------------------------------------------------------------------

#[test]
fn a_still_hand_stops_jittering() {
    // A detector wobbles by a pixel or two on a hand that isn't moving. Left
    // alone that's a pointer that shivers, which reads as broken.
    let mut t = Track::default();
    let jitter = [500.0, 502.0, 498.0, 501.0, 499.0, 500.0, 502.0, 498.0];
    let mut last = 0.0;
    for (i, x) in jitter.iter().enumerate() {
        last = t.saw(*x, 300.0, (i as u32 + 1) * 50, &cfg()).0;
    }
    assert!(
        (last - 500.0).abs() < 1.5,
        "smoothed to within a pixel and a half: {last}"
    );
}

#[test]
fn a_moving_hand_is_not_left_behind() {
    // The compromise a fixed filter forces: steady at rest OR responsive in
    // motion. Varying the smoothing with speed gets both.
    let mut t = Track::default();
    let mut last = (0.0, 0.0);
    for i in 1..=10u32 {
        last = t.saw(i as f32 * 100.0, 300.0, i * 50, &cfg());
    }
    assert!(
        last.0 > 850.0,
        "should be close behind a hand that's travelled to 1000: {}",
        last.0
    );
}

#[test]
fn the_first_reading_is_used_as_is() {
    let mut t = Track::default();
    assert_eq!(t.saw(400.0, 200.0, 10, &cfg()), (400.0, 200.0));
}

#[test]
fn two_readings_with_the_same_timestamp_do_not_send_the_pointer_to_infinity() {
    // Dividing by a zero interval is how a pointer vanishes off the screen.
    let mut t = Track::default();
    t.saw(100.0, 100.0, 500, &cfg());
    let (x, y) = t.saw(140.0, 100.0, 500, &cfg());
    assert!(x.is_finite() && y.is_finite(), "{x},{y}");
    assert!((0.0..2000.0).contains(&x), "{x}");
}

#[test]
fn a_reading_from_the_past_does_not_break_it() {
    let mut t = Track::default();
    t.saw(100.0, 100.0, 900, &cfg());
    let (x, _) = t.saw(120.0, 100.0, 400, &cfg());
    assert!(x.is_finite(), "{x}");
}

// ---------------------------------------------------------------------------
// Prediction: the actual latency fix
// ---------------------------------------------------------------------------

#[test]
fn the_pointer_keeps_moving_between_detections() {
    // A detector at ten a second updates every hundred milliseconds. Without
    // this the pointer lags, then jumps to catch up, and the jump is what
    // makes people give up within a minute.
    let mut t = Track::default();
    for i in 1..=6u32 {
        t.saw(i as f32 * 100.0, 200.0, i * 100, &cfg());
    }
    let at_reading = t.where_now(600).expect("just read");
    let later = t.where_now(660).expect("still predicting");
    assert!(
        later.0 > at_reading.0,
        "it should carry on in the direction it was going: {at_reading:?} -> {later:?}"
    );
}

#[test]
fn prediction_runs_out_rather_than_gliding_forever() {
    let mut t = Track::default();
    for i in 1..=6u32 {
        t.saw(i as f32 * 100.0, 200.0, i * 100, &cfg());
    }
    assert!(t.where_now(600 + PREDICT_FOR_MS - 10).is_some());
    assert_eq!(
        t.where_now(600 + PREDICT_FOR_MS + 50),
        None,
        "a pointer that keeps gliding after you drop your arm is worse than \
         one that stops"
    );
}

#[test]
fn a_still_hand_is_predicted_to_stay_still() {
    let mut t = Track::default();
    for i in 1..=8u32 {
        t.saw(500.0, 300.0, i * 100, &cfg());
    }
    let (x, y) = t.where_now(850).expect("predicting");
    assert!((x - 500.0).abs() < 8.0 && (y - 300.0).abs() < 8.0, "{x},{y}");
}

#[test]
fn a_guess_is_never_mistaken_for_a_reading() {
    // Predicting where a pointer is between frames is fine. Clicking
    // somewhere predicted is not.
    let mut t = Track::default();
    t.saw(100.0, 100.0, 1000, &cfg());
    assert!(t.confident(1010), "just read");
    assert!(!t.confident(1200), "that's a guess now");
}

#[test]
fn nothing_is_confident_before_anything_has_been_seen() {
    assert!(!Track::default().confident(0));
    assert_eq!(Track::default().where_now(0), None);
}

#[test]
fn losing_the_hand_stops_the_pointer_dead() {
    let mut t = Track::default();
    for i in 1..=5u32 {
        t.saw(i as f32 * 100.0, 200.0, i * 100, &cfg());
    }
    t.lost();
    assert_eq!(t.where_now(520), None);
    assert_eq!(t.moving(), 0.0);
}

// ---------------------------------------------------------------------------
// Not eating the machine
// ---------------------------------------------------------------------------

#[test]
fn a_cheap_detector_runs_at_the_rate_you_asked_for() {
    let c = PaceConfig::default();
    let mut p = Pace::default();
    for _ in 0..20 {
        p.took(3);
    }
    assert_eq!(
        p.wait_ms(&c),
        1000 / c.want_per_second,
        "nothing to throttle"
    );
}

#[test]
fn an_expensive_detector_is_slowed_down_rather_than_left_to_hog_the_core() {
    // Eric's requirement in his own words: it must not slow down his system or
    // his work. A tracker that runs flat out is one he turns off.
    let c = PaceConfig::default();
    let mut p = Pace::default();
    for _ in 0..20 {
        p.took(60);
    }
    let wait = p.wait_ms(&c);
    assert!(
        wait >= 60 * 4,
        "at a fifth of a core, 60ms of work needs ~240ms of gap: {wait}"
    );
    let used = 60.0 / (wait as f32);
    assert!(used <= c.share_of_a_core + 0.02, "using {used} of a core");
}

/// The budget wins over the floor.
///
/// An earlier version clamped the wait so tracking never dropped below the
/// floor — which quietly let an expensive detector exceed its share of the
/// core to hold that rate. That is the opposite of "don't slow my system".
#[test]
fn a_very_expensive_detector_is_slowed_past_the_floor_rather_than_given_the_core() {
    let c = PaceConfig::default();
    let mut p = Pace::default();
    for _ in 0..20 {
        p.took(5000);
    }
    assert!(
        p.wait_ms(&c) > 1000 / c.floor_per_second,
        "the floor is not a promise of speed, it is the line below which this \
         is not worth having on"
    );
    assert!(
        p.keeping_up(&c).is_some(),
        "and crossing it has to be said out loud"
    );
}

#[test]
fn a_machine_that_gets_busy_is_noticed_within_a_second_or_two() {
    let c = PaceConfig::default();
    let mut p = Pace::default();
    for _ in 0..30 {
        p.took(3);
    }
    let before = p.wait_ms(&c);
    for _ in 0..10 {
        p.took(80);
    }
    assert!(
        p.wait_ms(&c) > before,
        "a long average would take a minute to react"
    );
}

#[test]
fn it_says_when_it_cannot_keep_up_rather_than_degrading_quietly() {
    let c = PaceConfig::default();
    let mut p = Pace::default();
    for _ in 0..20 {
        p.took(400);
    }
    let said = p.keeping_up(&c).expect("it should speak up");
    assert!(said.contains("laggy"), "{said}");
    assert!(
        said.contains("turning off"),
        "tracking at four frames a second is not tracking, and he shouldn't be \
         left to conclude the whole idea doesn't work: {said}"
    );
    assert!(!said.contains('_') && !said.contains("::"), "{said}");
}

#[test]
fn it_does_not_complain_before_it_has_seen_enough_to_know() {
    let c = PaceConfig::default();
    let mut p = Pace::default();
    p.took(900);
    assert_eq!(p.keeping_up(&c), None, "one slow first run proves nothing");
}

#[test]
fn a_healthy_tracker_says_nothing() {
    let c = PaceConfig::default();
    let mut p = Pace::default();
    for _ in 0..30 {
        p.took(8);
    }
    assert_eq!(p.keeping_up(&c), None);
}
