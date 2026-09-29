//! Reading a face and a pair of hands off the camera.
//!
//! `presence::Look`, `presence::Gesture`, `Sensor::observe` and
//! `answering::saw` were all written, all tested, and nothing anywhere ever
//! produced one. Atlas could decide what a thumbs-up means and had no way to
//! see a thumb. This is the join, and these tests are mostly about the three
//! rules that matter more than the detection does.

use atlas::gaze::{
    gesture_named, how_often, in_use, read, read_hand,
    snags, spoken, to_screen, verdict, what_happened, why_look, GazeConfig, Hand, Move, Reason,
    Shape, Sign, Situation, Snag, Steering, DELIBERATE_MS, GONE_AFTER, TAP_SLOP,
};
use atlas::presence::Gesture;

fn cfg() -> GazeConfig {
    GazeConfig { enabled: true, ..GazeConfig::default() }
}

// ---------------------------------------------------------------------------
// An unread camera is not an empty room
// ---------------------------------------------------------------------------

#[test]
fn a_camera_that_said_nothing_is_not_a_report_of_nobody_there() {
    // The failure this codebase keeps producing. One of these means "speak
    // freely, he's gone" and the other means "you have no idea".
    let s = read("", &cfg());
    assert!(!s.saw_anything());
    assert_eq!(
        s.as_look(),
        None,
        "handing this to the presence sensor would report an empty room on a \
         machine whose camera simply failed"
    );
}

#[test]
fn a_camera_that_looked_and_saw_nobody_is_a_real_observation() {
    let s = read("faces: 0 0.99\n", &cfg());
    assert!(s.saw_anything());
    assert_eq!(s.as_look().map(|l| l.faces), Some(0));
}

#[test]
fn what_the_detector_could_not_do_is_kept_and_said() {
    let s = read("could_not: the lens is covered\n", &cfg());
    assert_eq!(s.could_not, vec!["the lens is covered"]);
    let said = spoken(&s, &cfg());
    assert!(said.contains("lens is covered"));
    assert!(
        said.contains("isn't the same as nobody being there"),
        "the distinction has to reach the person, not just the code: {said}"
    );
}

#[test]
fn a_switched_off_camera_says_so_rather_than_reporting_an_empty_room() {
    let off = GazeConfig { enabled: false, ..GazeConfig::default() };
    assert!(spoken(&read("", &off), &off).contains("switched off"));
}

// ---------------------------------------------------------------------------
// Unsure is absent, not false
// ---------------------------------------------------------------------------

#[test]
fn a_reading_below_the_floor_is_left_out_rather_than_written_down_as_no() {
    let s = read("faces: 2 0.30\ngesture: thumb_up 0.20\n", &cfg());
    assert_eq!(s.faces, None, "an unsure reading is not a negative one");
    assert_eq!(s.gesture, None);
    assert!(!s.saw_anything(), "so nothing downstream reads a shrug as a finding");
}

#[test]
fn a_confident_reading_gets_through() {
    let s = read("faces: 1 0.95\ngesture: thumb_up 0.91\n", &cfg());
    assert_eq!(s.faces, Some(1));
    assert_eq!(s.gesture, Some(Gesture::ThumbUp));
}

#[test]
fn a_detector_that_reports_more_than_atlas_asked_for_is_not_broken() {
    let s = read("faces: 1 0.99\nemotion: cheerful 0.8\nnonsense\n", &cfg());
    assert_eq!(s.faces, Some(1), "the useful line still landed");
}

// ---------------------------------------------------------------------------
// A face is not a password
// ---------------------------------------------------------------------------

#[test]
fn recognising_you_needs_a_higher_bar_than_seeing_a_face() {
    // One number cannot mean both "there is a face here" and "this is the
    // right face", and using one for both is how a stranger becomes a session.
    let c = cfg();
    assert!(c.min_identity_confidence > c.min_confidence);

    let s = read("faces: 1 0.99\nyou: yes 0.70\n", &c);
    assert_eq!(s.faces, Some(1), "the face was seen");
    assert_eq!(
        s.you, None,
        "and not confidently enough identified to claim it's him"
    );
}

#[test]
fn ruling_you_out_is_safe_at_the_lower_bar_but_ruling_you_in_is_not() {
    let s = read("faces: 1 0.99\nyou: no 0.70\n", &cfg());
    assert_eq!(
        s.you,
        Some(false),
        "a stranger flagged at moderate confidence should still be treated as \
         a stranger"
    );
}

#[test]
fn an_unknown_identity_does_not_become_you_by_default() {
    let s = read("faces: 1 0.99\n", &cfg());
    let look = s.as_look().unwrap();
    assert!(
        !look.you,
        "erring the other way would let a shrug from the detector put a \
         stranger at your desk"
    );
}

// ---------------------------------------------------------------------------
// Hands
// ---------------------------------------------------------------------------

#[test]
fn a_nod_and_a_shake_reuse_the_vocabulary_a_thumb_already_has() {
    // Deliberately not new variants: `presence::interpret` already decides
    // what a yes may authorise, and a second vocabulary reaching the same
    // decisions is a second set of rules to keep honest.
    assert_eq!(gesture_named("nod"), Some(Gesture::ThumbUp));
    assert_eq!(gesture_named("head_shake"), Some(Gesture::ThumbDown));
    assert_eq!(gesture_named("THUMBS_UP"), Some(Gesture::ThumbUp));
    assert_eq!(gesture_named("open_palm"), Some(Gesture::OpenPalm));
}

#[test]
fn a_shape_atlas_does_not_know_means_nothing_rather_than_something() {
    assert_eq!(
        gesture_named("finger_gun"),
        None,
        "guessing here would mean an unrecognised shape could approve something"
    );
    assert_eq!(gesture_named(""), None);
}

#[test]
fn a_wave_is_a_greeting_and_not_an_answer() {
    assert_eq!(
        gesture_named("wave"),
        None,
        "mapping it to anything would mean saying hello could approve something"
    );
}

// ---------------------------------------------------------------------------
// What it says
// ---------------------------------------------------------------------------

#[test]
fn it_distinguishes_the_three_states_a_person_would_confuse() {
    let c = cfg();
    let off = GazeConfig { enabled: false, ..GazeConfig::default() };
    let unread = spoken(&read("", &c), &c);
    let empty = spoken(&read("faces: 0 0.99\n", &c), &c);
    let disabled = spoken(&read("", &off), &off);

    assert_ne!(unread, empty, "\"couldn't look\" and \"nobody there\" differ");
    assert_ne!(unread, disabled, "\"couldn't look\" and \"not watching\" differ");
    assert!(empty.contains("nobody"));
}

#[test]
fn a_stranger_is_reported_as_a_stranger() {
    let c = cfg();
    let said = spoken(&read("faces: 1 0.99\nyou: no 0.95\n", &c), &c);
    assert!(said.contains("don't recognise"), "{said}");
}

#[test]
fn looking_away_is_worth_saying_and_looking_at_it_is_not() {
    let c = cfg();
    let away = spoken(&read("faces: 1 0.99\nlooking: no 0.9\n", &c), &c);
    assert!(away.contains("not looking at the screen"), "{away}");
    let at = spoken(&read("faces: 1 0.99\nlooking: yes 0.9\n", &c), &c);
    assert!(
        !at.contains("looking at the screen"),
        "you being where you should be is not news: {at}"
    );
}

#[test]
fn nothing_it_says_reads_like_a_variable_name() {
    let c = cfg();
    for printed in [
        "faces: 2 0.99\nyou: yes 0.99\ngesture: thumb_down 0.9\n",
        "could_not: no camera\n",
        "faces: 0 0.99\n",
    ] {
        let said = spoken(&read(printed, &c), &c);
        assert!(!said.contains('_') && !said.contains("::"), "{said}");
        assert!(said.ends_with('.'), "{said}");
    }
}

#[test]
fn the_camera_is_off_until_it_is_turned_on() {
    assert!(
        !GazeConfig::default().enabled,
        "a camera that starts watching because you updated is not something \
         anyone should have to discover"
    );
}


// ---------------------------------------------------------------------------
// When the camera is on, and why
// ---------------------------------------------------------------------------
//
// The first version looked every twenty seconds on a timer, which is wrong in
// both directions at once: it watches the room when nothing needs watching,
// and it is blind at the one moment that matters — Atlas asks a question and
// then does not look again for nineteen seconds while a hand is held up.

#[test]
fn an_idle_atlas_does_not_watch_the_room() {
    assert_eq!(
        why_look(&Situation::default(), &cfg()),
        None,
        "nothing needs looking at, so the camera is genuinely off"
    );
}

#[test]
fn a_question_on_the_table_opens_the_camera() {
    let now = Situation { question_waiting: true, ..Situation::default() };
    assert_eq!(why_look(&now, &cfg()), Some(Reason::Waiting));
}

#[test]
fn a_question_on_the_table_is_looked_at_often_enough_to_catch_a_hand() {
    assert!(
        how_often(Reason::Waiting) <= 3,
        "a hand held up for two seconds and missed is a feature that does not \
         work"
    );
    assert!(how_often(Reason::Steering) <= 2, "steering has to feel immediate");
}

#[test]
fn watching_while_you_talk_is_something_you_turn_on_not_the_default() {
    // "Sometimes I want Atlas to see me when talking" is not "watch every
    // conversation", and watching every conversation is a camera on for most
    // of the working day.
    let mid = Situation { mid_conversation: true, ..Situation::default() };
    assert_eq!(why_look(&mid, &cfg()), None);

    let opted_in = Situation { watch_while_talking: true, ..mid };
    assert_eq!(why_look(&opted_in, &cfg()), Some(Reason::Talking));
    assert!(!GazeConfig::default().watch_while_talking);
}

#[test]
fn steering_outranks_everything_else() {
    // A hand held up is worthless if Atlas is looking for a different reason
    // with a different vocabulary.
    let now = Situation {
        steering: true,
        question_waiting: true,
        about_to_be_private: true,
        ..Situation::default()
    };
    assert_eq!(why_look(&now, &cfg()), Some(Reason::Steering));
}

#[test]
fn a_switched_off_camera_has_no_reason_to_be_on_whatever_is_happening() {
    let off = GazeConfig { enabled: false, ..GazeConfig::default() };
    let busy = Situation {
        steering: true,
        question_waiting: true,
        asked_to_watch: true,
        ..Situation::default()
    };
    assert_eq!(why_look(&busy, &off), None);
}

#[test]
fn every_reason_can_be_said_out_loud() {
    // A camera you cannot get an answer about is a camera you turn off.
    for r in [
        Reason::Waiting,
        Reason::Talking,
        Reason::CheckingWhoElse,
        Reason::YouAsked,
        Reason::Steering,
    ] {
        let said = r.plain();
        assert!(said.split_whitespace().count() >= 4, "{said}");
        assert!(!said.contains('_') && !said.contains("::"), "{said}");
    }
}

// ---------------------------------------------------------------------------
// Two vocabularies that cannot reach each other
// ---------------------------------------------------------------------------

#[test]
fn only_steering_lets_a_hand_command_anything() {
    for r in [Reason::Waiting, Reason::Talking, Reason::CheckingWhoElse, Reason::YouAsked] {
        assert!(
            !r.hands_may_steer(),
            "a misread gesture that answers a question is a wrong answer to a \
             known question; one that issues a command is something nobody \
             asked for at all"
        );
    }
    assert!(Reason::Steering.hands_may_steer());
}







#[test]
fn steering_switches_itself_off() {
    assert!(
        atlas::gaze::STEERING_STOPS_AFTER <= 60,
        "a mode you can leave on by accident is a camera left watching an \
         empty room because you walked away mid-gesture"
    );
}


// ---------------------------------------------------------------------------
// Direct manipulation
// ---------------------------------------------------------------------------
//
// The first version had Next/Previous — step through panels like a remote
// control. The reference Eric sent shows something else: the screen mirrors
// you, panels sit around you, and you reach out and move the one you want. A
// remote control is what you build when you cannot see where the hand is.

fn hand(x: f32, y: f32) -> Hand {
    Hand { x, y, pinching: false, open: false, sure: 0.9 }
}

fn pinch(x: f32, y: f32) -> Hand {
    Hand { pinching: true, ..hand(x, y) }
}

#[test]
fn your_right_hand_moves_the_pointer_to_your_right() {
    // The screen shows you facing yourself, so the image is mirrored. Getting
    // this backwards makes the whole thing feel broken in a way people
    // struggle to describe.
    let (x, _) = to_screen(&hand(0.1, 0.5), 1000, 500);
    assert!(x > 800, "a hand at the left of the frame is your right hand: {x}");
    let (x2, _) = to_screen(&hand(0.9, 0.5), 1000, 500);
    assert!(x2 < 200, "{x2}");
}

#[test]
fn a_hand_outside_the_frame_cannot_point_off_the_screen() {
    let (x, y) = to_screen(&hand(-3.0, 9.0), 1000, 500);
    assert!((0..1000).contains(&x) && (0..500).contains(&y), "{x},{y}");
}

#[test]
fn moving_an_open_hand_just_moves_the_pointer() {
    let mut st = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st, 1000, 500);
    let m = what_happened(Some(hand(0.4, 0.5)), &mut st, 1000, 500);
    assert!(matches!(m, Some(Move::Point { .. })));
    assert!(!m.unwrap().undoable(), "moving your hand is not an event");
}

#[test]
fn closing_your_fingers_picks_something_up_and_opening_them_puts_it_down() {
    let mut st = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st, 1000, 500);
    assert!(matches!(
        what_happened(Some(pinch(0.5, 0.5)), &mut st, 1000, 500),
        Some(Move::Grab { .. })
    ));
    assert!(matches!(
        what_happened(Some(pinch(0.2, 0.5)), &mut st, 1000, 500),
        Some(Move::Drag { .. })
    ));
    assert!(matches!(
        what_happened(Some(hand(0.2, 0.5)), &mut st, 1000, 500),
        Some(Move::Drop { .. })
    ));
}

#[test]
fn a_pinch_that_never_moved_is_a_click_not_a_zero_length_drag() {
    let mut st = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st, 1000, 500);
    what_happened(Some(pinch(0.5, 0.5)), &mut st, 1000, 500);
    let m = what_happened(Some(hand(0.5005, 0.5)), &mut st, 1000, 500);
    assert!(
        matches!(m, Some(Move::Tap { .. })),
        "told apart here rather than by whatever receives it: {m:?}"
    );
}

#[test]
fn a_pinch_that_moved_further_than_a_wobble_is_a_drag() {
    let mut st = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st, 1000, 500);
    what_happened(Some(pinch(0.5, 0.5)), &mut st, 1000, 500);
    let m = what_happened(Some(hand(0.2, 0.5)), &mut st, 1000, 500);
    assert!(matches!(m, Some(Move::Drop { .. })), "{m:?}");
    assert!(TAP_SLOP > 0 && TAP_SLOP < 100, "a wobble, not a journey");
}

#[test]
fn one_missed_frame_does_not_fling_what_you_are_carrying() {
    // A detector misses a frame when you turn your wrist. Ending a drag on
    // that would throw the window across the desk.
    let mut st = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st, 1000, 500);
    what_happened(Some(pinch(0.5, 0.5)), &mut st, 1000, 500);
    assert_eq!(what_happened(None, &mut st, 1000, 500), None);
    assert!(
        matches!(
            what_happened(Some(pinch(0.45, 0.5)), &mut st, 1000, 500),
            Some(Move::Drag { .. })
        ),
        "still carrying it"
    );
}

#[test]
fn a_hand_that_really_left_puts_down_what_it_was_carrying() {
    let mut st = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st, 1000, 500);
    what_happened(Some(pinch(0.5, 0.5)), &mut st, 1000, 500);
    let mut last = None;
    for _ in 0..GONE_AFTER {
        last = what_happened(None, &mut st, 1000, 500);
    }
    assert!(
        matches!(last, Some(Move::Drop { .. })),
        "left held would leave the desktop mid-drag: {last:?}"
    );
}

#[test]
fn a_flat_palm_summons_when_empty_handed_and_stops_when_carrying() {
    let mut st = Steering::default();
    let palm = Hand { open: true, ..hand(0.5, 0.5) };
    // Held, not instant — see below for why.
    let mut got = None;
    for _ in 0..4 {
        if let Some(m) = what_happened(Some(palm), &mut st, 1000, 500) {
            got = Some(m);
            break;
        }
    }
    assert_eq!(got, Some(Move::Summon));

    let mut st2 = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st2, 1000, 500);
    what_happened(Some(pinch(0.5, 0.5)), &mut st2, 1000, 500);
    let mut got2 = None;
    for _ in 0..4 {
        if let Some(m) = what_happened(Some(palm), &mut st2, 1000, 500) {
            got2 = Some(m);
            break;
        }
    }
    assert_eq!(got2, Some(Move::Done));
}

/// The collision Atlas's own registry warns about, in Atlas's own vocabulary.
///
/// A pinch opening back into a flat hand passes through the palm shape on the
/// way out. With an instant palm, every drag would end by summoning Atlas —
/// the thing that looks fine in a demo and falls apart in use.
#[test]
fn letting_go_of_something_does_not_summon_atlas() {
    let mut st = Steering::default();
    what_happened(Some(hand(0.5, 0.5)), &mut st, 1000, 500);
    what_happened(Some(pinch(0.5, 0.5)), &mut st, 1000, 500);
    what_happened(Some(pinch(0.2, 0.5)), &mut st, 1000, 500);
    // Fingers relax: open hand, one frame, on the way to resting.
    let released = what_happened(Some(Hand { open: true, ..hand(0.2, 0.5) }), &mut st, 1000, 500);
    assert_ne!(
        released,
        Some(Move::Summon),
        "relaxing your hand after a drag must not bring Atlas up"
    );
}

#[test]
fn everything_that_changed_something_can_be_taken_back() {
    // Not a limit on what a hand may do — it is how Atlas knows what to offer
    // to undo when a gesture lands somewhere it wasn't meant to, which it will.
    for m in [
        Move::Grab { x: 1, y: 1 },
        Move::Drag { x: 1, y: 1 },
        Move::Drop { x: 1, y: 1 },
        Move::Tap { x: 1, y: 1 },
        Move::Resize { by: 1.2 },
        Move::Summon,
        Move::Done,
    ] {
        assert!(m.undoable(), "{m:?}");
        assert!(!m.plain().is_empty());
        assert!(!m.plain().contains('_'), "{}", m.plain());
    }
}

#[test]
fn a_hand_is_read_out_of_what_the_detector_printed() {
    let c = cfg();
    let h = read_hand("hand: 0.25 0.60 pinch 0.9\n", &c).expect("a hand");
    assert!((h.x - 0.25).abs() < 0.01 && (h.y - 0.60).abs() < 0.01);
    assert!(h.pinching && !h.open);
    assert!(read_hand("hand: 0.25 0.60 pinch 0.2\n", &c).is_none(), "unsure");
    assert!(read_hand("faces: 1 0.99\n", &c).is_none());
}


// ---------------------------------------------------------------------------
// Choosing gestures that won't collide later
// ---------------------------------------------------------------------------
//
// The danger isn't the detector mixing up a fist and a flat palm. It's a
// gesture colliding with something you already do without meaning it, or a
// shape you pass through on the way to another one.

fn sign(name: &'static str, means: &'static str, shape: Shape, hold_ms: u32) -> Sign {
    Sign { name, means, shape, hold_ms }
}

#[test]
fn covering_your_eyes_is_flagged_because_people_rub_their_eyes() {
    let s = sign("cover eyes", "hide the tab", Shape::AtFace, 0);
    let found = snags(&s, &in_use());
    assert!(
        found.iter().any(|s| matches!(s, Snag::Accidental(_))),
        "this would fire when you're tired rather than when you decided \
         something: {found:?}"
    );
}

#[test]
fn holding_it_makes_an_accidental_gesture_usable() {
    // Not a refusal. Eric asked not to have things withheld for looking
    // risky, so a snag is something Atlas says and a hold is the fix.
    let held = sign("cover eyes", "hide the tab", Shape::AtFace, DELIBERATE_MS);
    assert!(
        snags(&held, &in_use()).is_empty(),
        "a held version of the same gesture should be fine"
    );
    let quick = sign("cover eyes", "hide the tab", Shape::AtFace, 0);
    assert!(snags(&quick, &in_use()).iter().all(|s| !s.fatal()));
    assert!(snags(&quick, &in_use())[0].fix().is_some());
}

#[test]
fn a_shape_you_pass_through_on_the_way_to_another_is_caught() {
    // The one that looks fine in a demo and falls apart in use: an open hand
    // closing into a fist is an open hand for the first few frames.
    let existing = vec![sign("open palm", "stopping", Shape::OpenHand, 0)];
    let fist = sign("fist", "undo", Shape::ClosedHand, 0);
    let found = snags(&fist, &existing);
    assert!(
        found.iter().any(|s| matches!(s, Snag::OnTheWayTo(_))),
        "{found:?}"
    );
}

#[test]
fn the_same_thing_held_is_no_longer_something_you_pass_through() {
    let existing = vec![sign("open palm", "stopping", Shape::OpenHand, DELIBERATE_MS)];
    let fist = sign("fist", "undo", Shape::ClosedHand, 0);
    assert!(
        snags(&fist, &existing).is_empty(),
        "passing through a held shape briefly does not trigger it"
    );
}

#[test]
fn two_gestures_with_the_same_shape_is_the_one_thing_that_cannot_work() {
    let existing = vec![sign("pinch", "picking up", Shape::Pinch, 0)];
    let clash = sign("squeeze", "closing a tab", Shape::Pinch, 0);
    let found = snags(&clash, &existing);
    assert!(found.iter().any(|s| s.fatal()));
    assert!(found[0].fix().is_none(), "a hold cannot fix a duplicate");
}

#[test]
fn opposites_that_move_in_opposite_directions_are_fine() {
    // Movement is the easiest thing for a camera to read. Eric asked whether
    // reverses would confuse it — sweeps genuinely don't.
    let existing = vec![sign("swipe left", "next", Shape::Sweep { rightwards: false }, 0)];
    let other = sign("swipe right", "back", Shape::Sweep { rightwards: true }, 0);
    assert!(snags(&other, &existing).is_empty());
}

#[test]
fn shapes_that_differ_only_slightly_are_flagged_rather_than_allowed() {
    let existing = vec![sign("open palm", "stopping", Shape::OpenHand, DELIBERATE_MS)];
    let pointing = sign("point", "select", Shape::Point, DELIBERATE_MS);
    let found = snags(&pointing, &existing);
    assert!(
        found.iter().any(|s| matches!(s, Snag::LooksTheSame(_))),
        "getting it wrong sometimes is worse than not having it: {found:?}"
    );
}

#[test]
fn a_clean_gesture_is_simply_accepted() {
    let s = sign("two hands apart", "make it bigger", Shape::TwoHanded, DELIBERATE_MS);
    assert!(
        snags(&s, &in_use()).is_empty(),
        "nothing wrong with it: {:?}",
        snags(&s, &in_use())
    );
    let said = verdict(&s, &in_use());
    assert!(said.contains("that'll work"), "{said}");
}

#[test]
fn a_problem_is_explained_and_a_fix_offered_rather_than_a_refusal() {
    let s = sign("cover eyes", "hide the tab", Shape::AtFace, 0);
    let said = verdict(&s, &in_use());
    assert!(said.contains("rub their eyes"), "it says why: {said}");
    assert!(said.contains("hold it"), "and what would fix it: {said}");
    assert!(!said.to_lowercase().contains("cannot"), "not a refusal: {said}");
}

#[test]
fn the_gestures_already_in_use_do_not_collide_with_each_other() {
    // The registry has to be honest about Atlas's own vocabulary first.
    let all = in_use();
    for (i, s) in all.iter().enumerate() {
        let others: Vec<Sign> = all
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, s)| s.clone())
            .collect();
        assert!(
            snags(s, &others).is_empty(),
            "{} collides with something Atlas already uses: {:?}",
            s.name,
            snags(s, &others)
        );
    }
}
