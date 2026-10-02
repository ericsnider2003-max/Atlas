//! Gestures defined from geometry, not from someone else's label list.
//!
//! The distinction this file is about: a classifier hands you a fixed
//! vocabulary somebody else chose, and adding to it means retraining. A
//! landmark model hands you twenty-one joints and no opinion, and every
//! gesture is arithmetic written here. Eric doesn't want his vocabulary
//! limited by what a model already knows, so it's the second one.

use atlas::handshape::{
    as_demonstrated, how_to, needs_deciding, needs_holding, sketch, Gesture,
    Holding, Landmarks, Learning, Motion, Point, Progress, Reading, Test, Vocabulary, POINTS,
    SAMPLES,
};
use atlas::store::Store;

/// A hand with everything folded, as a starting point.
fn fist() -> Landmarks {
    let mut points = [Point::default(); POINTS];
    // Wrist and middle knuckle set the span everything else is measured
    // against.
    points[0] = Point::from(0.5, 0.9);
    points[9] = Point::from(0.5, 0.7);
    // Knuckles.
    for (i, k) in [2usize, 5, 9, 13, 17].iter().enumerate() {
        points[*k] = Point::from(0.44 + i as f32 * 0.03, 0.72);
    }
    // Tips folded back onto the knuckles.
    for (i, t) in [4usize, 8, 12, 16, 20].iter().enumerate() {
        points[*t] = Point::from(0.44 + i as f32 * 0.03, 0.73);
    }
    Landmarks { points, right: Some(true), sure: 0.95 }
}

/// Extend one finger straight up from its knuckle.
fn extend(h: &mut Landmarks, finger: usize) {
    let knuckles = [2usize, 5, 9, 13, 17];
    let tips = [4usize, 8, 12, 16, 20];
    let k = h.points[knuckles[finger]];
    let span = h.span();
    h.points[tips[finger]] = Point::from(k.x, k.y - span * 1.1);
}

fn gesture(name: &str, tests: Vec<Test>, hold_ms: u32) -> Gesture {
    Gesture {
        name: name.into(),
        does: format!("do {name}"),
        tests,
        hold_ms,
        two_handed: false,
        bands: Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// Reading a hand
// ---------------------------------------------------------------------------

#[test]
fn a_folded_finger_reads_as_folded_and_an_extended_one_does_not() {
    let mut h = fist();
    let mut r = Reading::of(&h);
    assert!(r.curl(1) > 0.6, "index folded: {}", r.curl(1));

    extend(&mut h, 1);
    let mut r2 = Reading::of(&h);
    assert!(r2.curl(1) < 0.4, "index out: {}", r2.curl(1));
}

#[test]
fn the_same_gesture_reads_the_same_at_any_distance_from_the_camera() {
    // Everything is measured against the hand's own span. Without that, every
    // threshold is right at one arm's length and wrong at another — the single
    // most common way hand gestures come out unreliable.
    let mut near = fist();
    extend(&mut near, 1);

    let mut far = near.clone();
    for p in far.points.iter_mut() {
        p.x = 0.5 + (p.x - 0.5) * 0.4;
        p.y = 0.5 + (p.y - 0.5) * 0.4;
    }

    let near_curl = Reading::of(&near).curl(1);
    let far_curl = Reading::of(&far).curl(1);
    assert!(
        (near_curl - far_curl).abs() < 0.1,
        "{near_curl} vs {far_curl} — same hand, half the size"
    );
}

#[test]
fn counting_extended_fingers_works_without_naming_a_gesture() {
    let mut h = fist();
    assert_eq!(Reading::of(&h).extended(), 0);
    extend(&mut h, 1);
    extend(&mut h, 2);
    assert_eq!(Reading::of(&h).extended(), 2);
}

// ---------------------------------------------------------------------------
// An open vocabulary
// ---------------------------------------------------------------------------

#[test]
fn a_gesture_nobody_trained_a_model_on_still_works() {
    // Ring finger only. No pre-built classifier has this, and it needs no
    // training here — it's three lines of geometry.
    let mut h = fist();
    extend(&mut h, 3);

    let v = Vocabulary {
        gestures: vec![gesture(
            "ring finger up",
            vec![Test::Out(3), Test::In(1), Test::In(2)],
            0,
        )],
    };
    assert_eq!(v.recognise(&h).map(|g| g.name.as_str()), Some("ring finger up"));
}

#[test]
fn adding_a_gesture_does_not_disturb_the_others() {
    let mut h = fist();
    extend(&mut h, 1);
    let v = Vocabulary {
        gestures: vec![
            gesture("point", vec![Test::Out(1), Test::In(2), Test::In(3)], 0),
            gesture("two", vec![Test::Out(1), Test::Out(2)], 0),
        ],
    };
    assert_eq!(v.recognise(&h).map(|g| g.name.as_str()), Some("point"));
}

#[test]
fn a_hand_making_nothing_in_the_vocabulary_matches_nothing() {
    let v = Vocabulary {
        gestures: vec![gesture("three", vec![Test::Extended(3)], 0)],
    };
    assert!(v.recognise(&fist()).is_none());
}

// ---------------------------------------------------------------------------
// Nothing processed that nothing acts on
// ---------------------------------------------------------------------------

#[test]
fn an_empty_vocabulary_computes_nothing_at_all() {
    // His constraint in his own words. A gesture that is defined and not
    // bound must cost nothing.
    let v = Vocabulary::default();
    assert!(v.recognise(&fist()).is_none());
    assert!(!v.any_held(), "and nothing waits on a hold either");
}

#[test]
fn two_gestures_that_describe_the_same_hand_are_reported() {
    // The clash that matters with composable tests: one of them silently
    // never fires, which looks like the detector failing.
    let v = Vocabulary {
        gestures: vec![
            gesture("one finger", vec![Test::Extended(1)], 0),
            gesture("pointing", vec![Test::Out(1)], 0),
        ],
    };
    let clashes = v.overlaps();
    assert_eq!(clashes.len(), 1, "{clashes:?}");
}

#[test]
fn gestures_that_genuinely_cannot_both_be_true_are_left_alone() {
    let v = Vocabulary {
        gestures: vec![
            gesture("pinch", vec![Test::Pinched], 0),
            gesture("open", vec![Test::NotPinched, Test::Spread], 0),
        ],
    };
    assert!(v.overlaps().is_empty(), "{:?}", v.overlaps());
}

// ---------------------------------------------------------------------------
// Holding without feeling slow
// ---------------------------------------------------------------------------

#[test]
fn an_unambiguous_gesture_fires_on_sight() {
    // The hold is the price of ambiguity. Anything unambiguous shouldn't pay
    // it — that's what stops the hold being a general tax on responsiveness.
    let mut h = Holding::default();
    assert_eq!(h.seen(true, 1000, 0), Progress::Done);
}

#[test]
fn a_held_gesture_reports_progress_from_the_first_frame() {
    // The whole answer to "don't let it feel delayed": the delay becomes
    // visible progress. A visible half-second reads as deliberate; an
    // invisible one reads as broken.
    let mut h = Holding::default();
    assert_eq!(h.seen(true, 0, 800), Progress::Holding(0.0));
    assert_eq!(h.seen(true, 400, 800), Progress::Holding(0.5));
    assert_eq!(h.seen(true, 800, 800), Progress::Done);
}

#[test]
fn letting_go_early_cancels_it_completely() {
    let mut h = Holding::default();
    h.seen(true, 0, 800);
    h.seen(true, 400, 800);
    assert_eq!(h.seen(false, 500, 800), Progress::No);
    // And starting again starts from zero, not from halfway.
    assert_eq!(h.seen(true, 600, 800), Progress::Holding(0.0));
}

#[test]
fn holding_on_after_it_fired_is_not_a_second_command() {
    let mut h = Holding::default();
    assert_eq!(h.seen(true, 0, 0), Progress::Done);
    assert_eq!(h.seen(true, 100, 0), Progress::No);
    assert_eq!(h.seen(true, 200, 0), Progress::No);
}

#[test]
fn two_handed_gestures_never_need_a_hold() {
    let mut g = gesture("both hands apart", vec![Test::Spread], 0);
    g.two_handed = true;
    assert!(
        !needs_holding(&g, &[]),
        "nobody does that by accident, so it shouldn't pay the delay"
    );
}

#[test]
fn a_shape_a_pinch_relaxes_through_has_to_wait() {
    // The collision that was live in Atlas's own vocabulary: a pinch opening
    // back into a flat hand is a flat hand for a few frames, so an instant
    // open hand fired at the end of every drag.
    let open = gesture("open hand", vec![Test::NotPinched, Test::Spread], 0);
    let pinch = gesture("pinch", vec![Test::Pinched], 0);
    assert!(needs_holding(&open, &[pinch.clone()]));
    assert!(
        !needs_holding(&pinch, &[open]),
        "the pinch itself is unambiguous and should stay instant"
    );
}

#[test]
fn a_gesture_can_be_described_without_showing_anyone_a_joint_index() {
    let g = gesture("point", vec![Test::Out(1), Test::In(2)], 900);
    let said = g.describe();
    assert!(said.contains("index finger out"), "{said}");
    assert!(said.contains("middle finger folded"), "{said}");
    assert!(said.contains("0.9s"), "{said}");
    assert!(!said.contains('['), "no indices on screen: {said}");
}


// ---------------------------------------------------------------------------
// Learning a gesture by being shown one
// ---------------------------------------------------------------------------
//
// Editing a config file technically answers "can Atlas adopt a new gesture"
// and practically answers no — nobody invents a gesture at their desk and then
// goes and writes joint indices in YAML.

fn demonstrate(shape: &Landmarks, times: usize) -> Learning {
    let mut l = Learning::default();
    for _ in 0..times {
        l.watch(shape);
    }
    l
}

#[test]
fn holding_a_shape_up_is_enough_to_define_it() {
    let mut h = fist();
    extend(&mut h, 3);
    let learned = demonstrate(&h, SAMPLES)
        .worked_out("ring finger", "mute everything")
        .expect("a steady shape");

    assert!(learned.tests.contains(&Test::Out(3)), "{:?}", learned.tests);
    assert!(learned.tests.contains(&Test::In(1)), "{:?}", learned.tests);
    // And it actually matches the hand it was taught from.
    let v = Vocabulary { gestures: vec![learned] };
    assert_eq!(v.recognise(&h).map(|g| g.does.as_str()), Some("mute everything"));
}

#[test]
fn a_finger_that_wandered_is_left_out_rather_than_pinned_to_its_last_position() {
    // The difference between a gesture that works tomorrow and one that only
    // worked while it was being taught.
    let steady = fist();
    let mut wobbly = fist();
    extend(&mut wobbly, 4);

    let mut l = Learning::default();
    for i in 0..SAMPLES {
        l.watch(if i % 2 == 0 { &steady } else { &wobbly });
    }
    let learned = l.worked_out("something", "do a thing").expect("mostly steady");
    assert!(
        !learned.tests.iter().any(|t| matches!(t, Test::Out(4) | Test::In(4))),
        "the little finger was out half the time and should not be in the \
         definition: {:?}",
        learned.tests
    );
}

#[test]
fn a_hand_that_never_settled_is_refused_with_a_reason() {
    let mut a = fist();
    extend(&mut a, 1);
    let mut b = fist();
    extend(&mut b, 3);
    let mut c = fist();
    extend(&mut c, 0);
    extend(&mut c, 4);

    let mut l = Learning::default();
    for i in 0..SAMPLES {
        l.watch(match i % 3 {
            0 => &a,
            1 => &b,
            _ => &c,
        });
    }
    assert!(l.worked_out("chaos", "anything").is_none());
    assert!(l.why_not().contains("hold it still"));
}

#[test]
fn seeing_no_hand_at_all_says_so_rather_than_blaming_you() {
    let l = Learning::default();
    assert!(l.worked_out("x", "y").is_none());
    assert!(l.why_not().contains("didn't see your hand"));
}

#[test]
fn it_knows_when_it_has_watched_long_enough() {
    let h = fist();
    assert!(!demonstrate(&h, SAMPLES - 1).enough());
    assert!(demonstrate(&h, SAMPLES).enough());
}

// ---------------------------------------------------------------------------
// Adopting it
// ---------------------------------------------------------------------------

#[test]
fn a_new_gesture_is_checked_against_the_ones_already_bound() {
    let mut v = Vocabulary {
        gestures: vec![gesture("point", vec![Test::Out(1), Test::In(2)], 0)],
    };
    let clash = gesture("select", vec![Test::Out(1), Test::In(2)], 0);
    let err = v.adopt(clash).unwrap_err();
    assert!(
        err.contains("same shape as point"),
        "found now rather than after a week of one never firing: {err}"
    );
    assert!(err.contains("a finger in or out is enough"), "and how to fix it");
}

#[test]
fn a_genuinely_new_shape_is_adopted_and_described_back() {
    let mut v = Vocabulary {
        gestures: vec![gesture("point", vec![Test::Out(1), Test::In(2)], 0)],
    };
    let fresh = gesture("two fingers", vec![Test::Out(1), Test::Out(2)], 0);
    let said = v.adopt(fresh).expect("no clash");
    assert!(said.contains("index finger out"), "{said}");
    assert_eq!(v.gestures.len(), 2);
}

#[test]
fn two_gestures_cannot_share_a_name() {
    let mut v = Vocabulary::default();
    v.adopt(gesture("mute", vec![Test::Out(3)], 0)).unwrap();
    let err = v.adopt(gesture("mute", vec![Test::Out(4)], 0)).unwrap_err();
    assert!(err.contains("already have one called that"), "{err}");
}

#[test]
fn a_nameless_gesture_is_refused() {
    let mut v = Vocabulary::default();
    assert!(v.adopt(gesture("", vec![Test::Out(1)], 0)).is_err());
}

#[test]
fn a_hold_is_added_only_when_the_shape_needs_one() {
    let mut v = Vocabulary {
        gestures: vec![gesture("pinch", vec![Test::Pinched], 0)],
    };
    v.adopt(gesture("open", vec![Test::NotPinched, Test::Spread], 0))
        .unwrap();
    let added = v.gestures.iter().find(|g| g.name == "open").unwrap();
    assert!(
        added.hold_ms > 0,
        "a pinch relaxes through this shape, so it has to wait"
    );

    let mut v2 = Vocabulary::default();
    v2.adopt(gesture("three", vec![Test::Extended(3)], 0)).unwrap();
    assert_eq!(
        v2.gestures[0].hold_ms, 0,
        "nothing to collide with, so no delay"
    );
}

#[test]
fn a_gesture_invented_today_is_still_there_tomorrow() {
    let dir = std::env::temp_dir().join("atlas-vocab-roundtrip");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::new(&dir);

    let mut v = Vocabulary::load(&store);
    v.adopt(gesture("ring finger", vec![Test::Out(3)], 0)).unwrap();
    v.save(&store).unwrap();

    let back = Vocabulary::load(&store);
    assert_eq!(back.gestures.len(), 1);
    assert_eq!(back.gestures[0].does, "do ring finger");
}

#[test]
fn a_gesture_can_be_dropped_again() {
    let mut v = Vocabulary::default();
    v.adopt(gesture("temporary", vec![Test::Out(2)], 0)).unwrap();
    assert!(v.forget("temporary"));
    assert!(!v.forget("temporary"), "already gone");
    assert!(v.gestures.is_empty());
}


// ---------------------------------------------------------------------------
// Movement, which four of the nine recordings need
// ---------------------------------------------------------------------------
//
// A pinch held still is picking something up; the same pinch flicked away is
// dismissing it. Same fingers, different hand. A static-only test system
// cannot tell those apart and would fire the wrong one about half the time.

fn moving(across: f32, down: f32) -> Motion {
    Motion { across, down, reversals: 0.0 }
}

fn repeating(across: f32, down: f32) -> Motion {
    Motion { across, down, reversals: 4.0 }
}

#[test]
fn the_same_shape_moving_differently_is_a_different_gesture() {
    let mut h = fist();
    // A pinch: thumb and index meeting.
    let span = h.span();
    h.points[4] = Point::from(0.5, 0.7 - span);
    h.points[8] = Point::from(0.5, 0.7 - span);

    let v = Vocabulary {
        gestures: vec![
            gesture("pinch", vec![Test::Pinched, Test::Still], 0),
            gesture("flick", vec![Test::Pinched, Test::MovingDown], 0),
        ],
    };
    assert_eq!(
        v.recognise_moving(&h, moving(0.0, 0.0)).map(|g| g.name.as_str()),
        Some("pinch")
    );
    assert_eq!(
        v.recognise_moving(&h, moving(0.0, 3.0)).map(|g| g.name.as_str()),
        Some("flick")
    );
}

#[test]
fn a_hand_nobody_holds_perfectly_still_still_counts_as_still() {
    // Treating any drift as a swipe is how a held gesture becomes an
    // accidental one.
    assert!(moving(0.2, -0.3).still());
    assert!(!moving(0.0, 3.0).still());
}

#[test]
fn drifting_is_not_a_swipe() {
    let mut h = fist();
    extend(&mut h, 1);
    let v = Vocabulary {
        gestures: vec![gesture("swipe", vec![Test::Out(1), Test::MovingRight], 0)],
    };
    assert!(
        v.recognise_moving(&h, moving(0.9, 0.0)).is_none(),
        "a slow drift right must not fire a swipe"
    );
    assert!(v.recognise_moving(&h, moving(3.0, 0.0)).is_some());
}

#[test]
fn a_hand_cannot_be_still_and_moving_at_once() {
    let v = Vocabulary {
        gestures: vec![
            gesture("held", vec![Test::Pinched, Test::Still], 0),
            gesture("thrown", vec![Test::Pinched, Test::MovingDown], 0),
        ],
    };
    assert!(
        v.overlaps().is_empty(),
        "these describe different hands: {:?}",
        v.overlaps()
    );
}

#[test]
fn opposite_directions_do_not_clash() {
    let v = Vocabulary {
        gestures: vec![
            gesture("next", vec![Test::Out(1), Test::MovingRight], 0),
            gesture("back", vec![Test::Out(1), Test::MovingLeft], 0),
        ],
    };
    assert!(v.overlaps().is_empty(), "{:?}", v.overlaps());
}

#[test]
fn a_static_vocabulary_never_asks_about_motion() {
    // The efficiency promise: motion is the only part needing history, so a
    // vocabulary of static shapes should never pay for it.
    let still_only = Vocabulary {
        gestures: vec![gesture("pinch", vec![Test::Pinched], 0)],
    };
    assert!(!still_only.any_motion());
    let with_motion = Vocabulary {
        gestures: vec![gesture("flick", vec![Test::Pinched, Test::MovingDown], 0)],
    };
    assert!(with_motion.any_motion());
}

// ---------------------------------------------------------------------------
// What the recordings gave us
// ---------------------------------------------------------------------------

#[test]
fn the_demonstrated_gestures_do_not_collide_with_each_other() {
    // The check that matters before any of these ship: two that describe the
    // same hand means one silently never fires.
    let v = Vocabulary { gestures: as_demonstrated() };
    assert!(
        v.overlaps().is_empty(),
        "the vocabulary read off the recordings clashes with itself: {:?}",
        v.overlaps()
    );
}

#[test]
fn yes_and_no_keep_the_meaning_they_already_had() {
    let v = as_demonstrated();
    let yes = v.iter().find(|g| g.does == "yes").expect("thumb up");
    let no = v.iter().find(|g| g.does == "no").expect("thumb down");
    assert!(yes.tests.contains(&Test::Out(0)));
    assert!(no.tests.contains(&Test::Out(0)));
    assert_ne!(yes.tests, no.tests, "and are told apart from each other");
}

#[test]
fn the_gestures_people_make_by_accident_are_held() {
    for g in as_demonstrated() {
        let at_face_or_open = g.name.contains("cover") || g.name.contains("palm");
        if at_face_or_open {
            assert!(
                g.hold_ms > 0,
                "{} fires on sight, and people do that without meaning to",
                g.name
            );
        }
    }
}

#[test]
fn what_could_not_be_read_is_reported_rather_than_guessed() {
    // A gesture Atlas guessed the meaning of is one he'd spend a week working
    // around.
    // The three from the recordings are settled by shape. What remains is the
    // one thing shape cannot settle.
    let unclear = needs_deciding();
    assert_eq!(unclear.len(), 1, "{unclear:?}");
    for (what, why) in &unclear {
        assert!(why.split_whitespace().count() > 12, "{what}: too thin a reason");
        assert!(
            why.contains("separate")
                || why.contains("separates")
                || why.contains("deciding"),
            "each one has to say what would settle it: {why}"
        );
    }
}

#[test]
fn the_defaults_do_not_overwrite_anything_taught() {
    // A default that replaces what someone taught is a default nobody trusts.
    let mut mine = Vocabulary {
        gestures: vec![gesture("my own", vec![Test::In(0), Test::Out(3), Test::In(1), Test::In(2)], 0)],
    };
    let before = mine.gestures.len();
    for g in as_demonstrated() {
        let _ = mine.adopt(g);
    }
    assert!(
        mine.gestures.iter().any(|g| g.name == "my own"),
        "what he taught survived"
    );
    assert!(mine.gestures.len() > before);
}


// ---------------------------------------------------------------------------
// Doing something over and over
// ---------------------------------------------------------------------------
//
// Two of the second set are not a shape and not a direction — they are a shape
// done repeatedly. A pinch opening and closing; a hooked hand stroking down
// again and again. Without repetition as a test, a scroll would fire "dismiss"
// on its first stroke and again on every stroke after it.

#[test]
fn a_shape_repeated_is_a_different_gesture_from_the_same_shape_once() {
    let mut h = fist();
    let span = h.span();
    h.points[4] = Point::from(0.5, 0.7 - span);
    h.points[8] = Point::from(0.5, 0.7 - span);

    let v = Vocabulary {
        gestures: vec![
            gesture("resize", vec![Test::Pinched, Test::Repeating], 0),
            gesture("dismiss", vec![Test::Pinched, Test::MovingDown, Test::Once], 0),
        ],
    };
    assert_eq!(
        v.recognise_moving(&h, repeating(0.0, 2.0)).map(|g| g.name.as_str()),
        Some("resize")
    );
    assert_eq!(
        v.recognise_moving(&h, moving(0.0, 3.0)).map(|g| g.name.as_str()),
        Some("dismiss")
    );
}

#[test]
fn overshooting_and_correcting_once_is_not_repeating() {
    // An ordinary gesture that overshoots and comes back would otherwise read
    // as a rhythm.
    let one_correction = Motion { across: 0.0, down: 2.0, reversals: 1.0 };
    assert!(!one_correction.repeating());
    assert!(repeating(0.0, 2.0).repeating());
}

#[test]
fn repeating_and_once_cannot_both_describe_one_hand() {
    let v = Vocabulary {
        gestures: vec![
            gesture("scroll", vec![Test::In(0), Test::MovingDown, Test::Repeating], 0),
            gesture("push", vec![Test::In(0), Test::MovingDown, Test::Once], 0),
        ],
    };
    assert!(v.overlaps().is_empty(), "{:?}", v.overlaps());
}

#[test]
fn the_three_gaps_are_filled() {
    // Undo, scrolling and switching screens were named as missing last time.
    let v = as_demonstrated();
    let does: Vec<&str> = v.iter().map(|g| g.does.as_str()).collect();
    for wanted in ["undo that", "scroll down", "scroll up"] {
        assert!(does.contains(&wanted), "{wanted} is still missing: {does:?}");
    }
    // Switching screens is back, resolved by making it two-handed — which
    // cannot collide with anything one-handed and needs no hold, because
    // nobody sweeps both hands across by accident.
    assert!(does.contains(&"next screen"), "{does:?}");
    let sweep = v.iter().find(|g| g.does == "next screen").unwrap();
    assert!(sweep.two_handed);
    assert_eq!(sweep.hold_ms, 0);

    // And the two new ones Eric asked for.
    for wanted in ["confirm it", "show me the gestures"] {
        assert!(does.contains(&wanted), "{wanted} is missing: {does:?}");
    }
}

#[test]
fn the_whole_extended_vocabulary_still_does_not_collide_with_itself() {
    let v = Vocabulary { gestures: as_demonstrated() };
    assert!(
        v.overlaps().is_empty(),
        "fifteen recordings in and something shadows something else: {:?}",
        v.overlaps()
    );
}

#[test]
fn undo_is_instant_because_it_is_the_recovery_from_everything_else() {
    let v = as_demonstrated();
    let undo = v.iter().find(|g| g.does == "undo that").expect("fist");
    assert_eq!(
        undo.hold_ms, 0,
        "waiting a second to undo a mistake is the one place a hold is wrong"
    );
}


// ---------------------------------------------------------------------------
// Being reminded, without a video of anybody
// ---------------------------------------------------------------------------
//
// Eric is handing instances of this to friends. A recording of the author
// waving at a webcam is an odd thing to ship and wrong the moment anyone
// changes a gesture. So the reference is generated from the definitions.

#[test]
fn every_gesture_can_show_itself() {
    for g in as_demonstrated() {
        let drawing = sketch(&g);
        assert!(drawing.starts_with("<svg"), "{}", g.name);
        assert!(drawing.contains("</svg>"));
        assert!(
            drawing.contains(&g.name),
            "{} has no label for anyone who can't see it",
            g.name
        );
    }
}

#[test]
fn the_drawing_shows_which_fingers_are_out() {
    let open = Gesture {
        name: "open".into(),
        does: "stop".into(),
        tests: vec![Test::Extended(5)],
        hold_ms: 0,
        two_handed: false,
        bands: Vec::new(),
    };
    let closed = Gesture {
        name: "closed".into(),
        does: "undo".into(),
        tests: vec![Test::Extended(0)],
        hold_ms: 0,
        two_handed: false,
        bands: Vec::new(),
    };
    assert_ne!(
        sketch(&open),
        sketch(&closed),
        "an open hand and a fist must not draw the same"
    );
}

#[test]
fn the_drawing_shows_which_way_it_moves() {
    let up = gesture("up", vec![Test::Extended(5), Test::MovingUp], 0);
    let down = gesture("down", vec![Test::Extended(5), Test::MovingDown], 0);
    assert_ne!(sketch(&up), sketch(&down));
    let still = gesture("still", vec![Test::Extended(5), Test::Still], 0);
    assert_ne!(sketch(&still), sketch(&up), "an arrow only when it moves");
}

#[test]
fn a_repeated_gesture_is_marked_as_repeated() {
    let once = gesture("once", vec![Test::Pinched, Test::Once], 0);
    let again = gesture("again", vec![Test::Pinched, Test::Repeating], 0);
    assert_ne!(sketch(&once), sketch(&again));
}

#[test]
fn how_to_make_it_is_said_in_words_anyone_could_follow() {
    for g in as_demonstrated() {
        let said = how_to(&g);
        assert!(said.ends_with('.'), "{said}");
        assert!(!said.contains("Test::") && !said.contains('['), "{said}");
        assert!(
            said.split_whitespace().count() >= 3,
            "{}: too terse to follow: {said}",
            g.name
        );
    }
}

#[test]
fn the_instruction_names_real_fingers_not_numbers() {
    let g = gesture("point", vec![Test::Out(1), Test::In(2), Test::In(3)], 0);
    let said = how_to(&g);
    assert!(said.contains("index finger"), "{said}");
    assert!(!said.contains('1'), "no joint indices: {said}");
    // And it names every finger the gesture actually cares about, so a
    // three-finger shape cannot be described as a one-finger one.
    assert_eq!(
        said.matches("finger").count(),
        1,
        "only the extended finger is named as out: {said}"
    );
}

#[test]
fn a_pinch_is_described_as_a_pinch_rather_than_two_folded_fingers() {
    let g = gesture("pinch", vec![Test::Pinched, Test::Still], 0);
    let said = how_to(&g);
    assert!(said.contains("Thumb and index finger together"), "{said}");
}

#[test]
fn a_held_gesture_says_how_long_to_hold_it() {
    let g = gesture("palm", vec![Test::Extended(5), Test::Still], 900);
    assert!(how_to(&g).contains("second"), "{}", how_to(&g));
}

#[test]
fn the_reference_covers_whatever_is_in_the_vocabulary_including_new_ones() {
    // Teaching a gesture has to add its card with no second step anyone has
    // to remember, or the reference goes stale the first time it is used.
    let mut v = Vocabulary::default();
    v.adopt(gesture("mine", vec![Test::In(0), Test::Out(3), Test::In(1), Test::In(2)], 0))
        .unwrap();
    let card = sketch(&v.gestures[0]);
    assert!(card.contains("<svg"));
    assert!(!how_to(&v.gestures[0]).is_empty());
}

// ---------------------------------------------------------------------------
// Calibration: the twenty samples are kept, not thrown away
// ---------------------------------------------------------------------------
//
// `Learning::watch` used to measure each sample against the universal
// thresholds, keep a tally, and drop the measurements — so a gesture taught
// by a hand whose "out" curls at 0.38 was matched forever against the
// universal 0.4, one frame of jitter from not being seen. Zero extra user
// effort to fix; what changed is what is stored.

#[test]
fn a_taught_gesture_carries_bands_for_what_held_steady() {
    let mut h = fist();
    extend(&mut h, 3);
    let learned = demonstrate(&h, SAMPLES)
        .worked_out("ring", "mute")
        .expect("steady shape");
    assert!(!learned.bands.is_empty(), "the demonstration was measured and kept");
    for t in &learned.tests {
        if matches!(t, Test::Out(_) | Test::In(_) | Test::Pinched | Test::Spread) {
            assert!(
                learned.bands.iter().any(|(bt, _)| bt == t),
                "{t:?} became a test without its measured band"
            );
        }
    }
}

#[test]
fn the_shipped_defaults_are_calibrated_to_nobody() {
    // They were read off recordings, not taught by the hand that will use
    // them; a friend's instance re-teaches rather than inheriting Eric's.
    for g in as_demonstrated() {
        assert!(g.bands.is_empty(), "{} carries someone's bands", g.name);
    }
}

#[test]
fn a_calibrated_gesture_still_matches_the_hand_that_taught_it() {
    let mut h = fist();
    extend(&mut h, 3);
    let learned = demonstrate(&h, SAMPLES).worked_out("ring", "mute").unwrap();
    let v = Vocabulary { gestures: vec![learned] };
    assert_eq!(v.recognise(&h).map(|g| g.does.as_str()), Some("mute"));
}

#[test]
fn calibration_survives_the_store_and_an_uncalibrated_save_still_loads() {
    let dir = std::env::temp_dir().join("atlas-handshape-bands");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::new(&dir);

    let mut h = fist();
    extend(&mut h, 3);
    let learned = demonstrate(&h, SAMPLES).worked_out("ring", "mute").unwrap();
    let mut v = Vocabulary::default();
    v.adopt(learned).unwrap();
    v.save(&store).unwrap();
    let back = Vocabulary::load(&store);
    assert!(
        !back.gestures[0].bands.is_empty(),
        "the calibration is part of what is saved"
    );
}

#[test]
fn a_band_widens_the_gate_for_the_hand_it_measured() {
    // A hand whose "out" ring finger curls at 0.45 fails the universal 0.4
    // and passes its own band.
    use atlas::handshape::Band;
    let g_universal = Gesture {
        name: "ring".into(),
        does: "mute".into(),
        tests: vec![Test::Out(3)],
        hold_ms: 0,
        two_handed: false,
        bands: Vec::new(),
    };
    let g_calibrated = Gesture {
        bands: vec![(Test::Out(3), Band { usually: 0.45, varies_by: 0.02 })],
        ..g_universal.clone()
    };

    // Build a hand whose ring-finger curl lands near 0.45: tip lifted a
    // little more than half a span above the knuckle.
    let mut h = fist();
    let k = h.points[13];
    let span = h.span();
    h.points[16] = Point::from(k.x, k.y - span * 0.55);
    let mut r = Reading::of(&h);
    let curl = r.curl(3);
    assert!(
        (0.41..0.53).contains(&curl),
        "the test hand's curl must sit between the universal line and the \
         band's ceiling to prove anything: {curl}"
    );

    assert!(!g_universal.holds_for(&mut Reading::of(&h)), "fails the universal 0.4");
    assert!(g_calibrated.holds_for(&mut Reading::of(&h)), "passes its own measured band");
}

#[test]
fn a_band_never_stretches_past_half_curled() {
    // However loose the demonstration, "out" cannot come to mean a folded
    // finger — the cap is what keeps calibration from drifting into absurdity.
    use atlas::handshape::Band;
    let sloppy = Gesture {
        name: "ring".into(),
        does: "mute".into(),
        tests: vec![Test::Out(3)],
        hold_ms: 0,
        two_handed: false,
        bands: vec![(Test::Out(3), Band { usually: 0.5, varies_by: 0.2 })],
    };
    let folded = fist(); // ring curl ~1.0
    assert!(
        !sloppy.holds_for(&mut Reading::of(&folded)),
        "a folded finger passed an 'out' test through a sloppy band"
    );
}

#[test]
fn slack_is_floored_so_an_eerily_steady_demonstration_stays_usable() {
    use atlas::handshape::Band;
    let steady = Band { usually: 0.2, varies_by: 0.0 };
    assert!(
        steady.slack() >= 0.06,
        "zero observed wobble must not produce a band nothing human can stay \
         inside: {}",
        steady.slack()
    );
    let wobbly = Band { usually: 0.2, varies_by: 0.05 };
    assert!(wobbly.slack() > steady.slack(), "measured wobble widens the band");
}

// ---------------------------------------------------------------------------
// Motion, finally measured from somewhere
// ---------------------------------------------------------------------------
//
// `recognise_moving` had tests and no caller, and nothing in production ever
// built a `Motion` with real numbers — so every motion-bound gesture was
// defined, evaluated against `Motion::default()`, and could never fire. The
// `Trail` is the missing source: index-tip positions in, `Motion` out, in
// frame-widths per second.

#[test]
fn a_still_hand_reads_still_and_a_swipe_reads_moving() {
    use atlas::handshape::Trail;
    let mut t = Trail::default();
    // Held nearly still for a second: tiny jitter around one spot.
    let mut m = Motion::default();
    for i in 0..20u32 {
        let wobble = if i % 2 == 0 { 0.001 } else { -0.001 };
        m = t.saw(0.5 + wobble, 0.5, i * 50);
    }
    assert!(m.still(), "jitter around a point is stillness: {m:?}");

    // A crisp swipe right: half the frame in a quarter second.
    let mut t = Trail::default();
    for i in 0..6u32 {
        m = t.saw(0.25 + i as f32 * 0.1, 0.5, 1_000 + i * 50);
    }
    assert!(
        m.across > Motion::MOVING,
        "half a frame in a quarter second is a deliberate movement: {m:?}"
    );
}

#[test]
fn waving_back_and_forth_reads_as_repeating() {
    use atlas::handshape::Trail;
    let mut t = Trail::default();
    let mut m = Motion::default();
    // Four full back-and-forths in a second.
    for i in 0..20u32 {
        let phase = (i / 3) % 2; // direction flips every ~150ms
        let x = if phase == 0 { 0.4 + (i % 3) as f32 * 0.05 } else { 0.55 - (i % 3) as f32 * 0.05 };
        m = t.saw(x, 0.5, i * 50);
    }
    assert!(m.repeating(), "a wave is a repetition, not a swipe: {m:?}");
}

#[test]
fn losing_the_hand_forgets_the_trail() {
    use atlas::handshape::Trail;
    let mut t = Trail::default();
    for i in 0..5u32 {
        t.saw(0.1, 0.5, i * 50);
    }
    t.lost();
    // Reappearing across the frame is a new trail, not a teleport measured
    // as a swipe.
    let m = t.saw(0.9, 0.5, 400);
    assert!(m.still(), "a jump across a gap in sight read as motion: {m:?}");
}

/// 1 Oct 2026: the look meant to catch a thumbs-up ran every seeing model
/// and never read a hand. A hand answering is now read from its joints.
#[test]
fn a_thumb_up_a_thumb_down_and_an_open_palm_answer_a_question() {
    use atlas::handshape::answer_from;
    let mut up = fist();
    extend(&mut up, 0);
    assert_eq!(answer_from(&up).map(|a| a.0), Some("thumb_up"));
    let mut down = fist();
    down.points[4] = Point::from(0.44, 1.3);
    assert_eq!(answer_from(&down).map(|a| a.0), Some("thumb_down"));
    let mut palm = fist();
    for f in 0..5 {
        extend(&mut palm, f);
    }
    assert_eq!(answer_from(&palm).map(|a| a.0), Some("open_palm"));
    assert_eq!(answer_from(&fist()), None, "a fist answers nothing");
}
