//! Seeing: what Atlas is allowed to claim about a picture.
//!
//! Almost nothing here is about whether the arithmetic is right. The models
//! do the arithmetic, and no test in this file can check their opinion of a
//! photograph. What it can check — and what actually decides whether this
//! feature is worth having — is everything around them:
//!
//! - that "I couldn't look" can never come out as "there's nothing there"
//! - that an unsure reading is left out rather than reported flatly
//! - that a name carries the hedge its confidence earns
//! - that a mismatched label list is an error rather than confident nonsense
//! - that recognising a face never authorises anything
//!
//! Each of those is a way this could look like it works and not, which is the
//! shape of failure this codebase keeps producing.

use atlas::infer::{Kind, Layout, Outputs};
use atlas::vision::{
    alike, faces, letterbox, thin_out, things, to_unit, whole_picture, Album, Face, Framing,
    Guess, Object, Patch, Scene, Sight, VisionConfig,
};

fn settings() -> VisionConfig {
    VisionConfig { enabled: true, ..VisionConfig::default() }
}

fn thing(name: &str, x: f32, y: f32, w: f32, h: f32, sure: f32) -> Object {
    Object { name: name.into(), area: Patch::new(x, y, w, h), sure }
}

// ---------------------------------------------------------------------------
// An unread camera is not an empty room
// ---------------------------------------------------------------------------

#[test]
fn not_looking_and_seeing_nothing_are_different_answers() {
    let cfg = settings();
    let empty = Sight::Looked(Scene::default());
    let blind = Sight::Unread("the camera wouldn't open".into());

    assert!(empty.scene().is_some(), "a look that found nothing is still a look");
    assert!(blind.scene().is_none(), "a look that didn't happen has no scene");

    let said_empty = empty.spoken(&cfg);
    let said_blind = blind.spoken(&cfg);
    assert_ne!(said_empty, said_blind);
    assert!(said_blind.contains("couldn't look"), "{said_blind}");
    assert!(said_blind.contains("camera wouldn't open"), "and why: {said_blind}");
}

#[test]
fn a_camera_that_failed_never_reports_an_empty_room_to_presence() {
    // The line `presence` reads. "faces: 0" means nobody is there and is
    // acted on; a failed look must not produce it, or a broken webcam becomes
    // permission to talk about private things out loud.
    let blind = Sight::Unread("the detector fell over".into());
    let lines = blind.as_lines("me");
    assert!(lines.starts_with("could_not:"), "{lines}");
    assert!(!lines.contains("faces: 0"), "{lines}");
}

#[test]
fn a_look_that_really_found_nobody_does_say_so() {
    // The other half of the same rule. If an honest empty room could not be
    // reported, `presence` would never learn that Eric had left.
    let lines = Sight::Looked(Scene::default()).as_lines("me");
    assert!(lines.contains("faces: 0"), "{lines}");
}

#[test]
fn a_half_failed_look_reports_the_half_that_failed() {
    let mut scene = Scene::default();
    scene.things.push(thing("mug", 0.1, 0.1, 0.2, 0.2, 0.9));
    scene.could_not.push("I couldn't look for faces: model missing".into());
    let lines = Sight::Looked(scene).as_lines("me");
    assert!(lines.contains("could_not:"), "{lines}");
    assert!(
        !lines.contains("faces: 0"),
        "a face model that failed is not a room with nobody in it: {lines}"
    );
}

#[test]
fn how_many_faces_is_reported_at_the_confidence_of_the_weakest() {
    // Three faces reported with the confidence of the clearest one claims
    // certainty about the two Atlas is least sure of.
    let face = |sure: f32| Face {
        area: Patch::new(0.1, 0.1, 0.1, 0.1),
        sure,
        who: None,
        sure_who: 0.0,
    };
    let scene = Scene { faces: vec![face(0.95), face(0.61)], ..Scene::default() };
    let lines = Sight::Looked(scene).as_lines("me");
    assert!(lines.contains("faces: 2 0.610"), "{lines}");
}

// ---------------------------------------------------------------------------
// A name carries how sure it is
// ---------------------------------------------------------------------------

#[test]
fn a_confident_name_and_a_guess_are_worded_differently() {
    let cfg = settings();
    let sure = thing("coffee mug", 0.0, 0.0, 0.1, 0.1, 0.95);
    let not_sure = thing("coffee mug", 0.0, 0.0, 0.1, 0.1, 0.45);
    let a = sure.spoken(cfg.sure_enough_to_say_plainly);
    let b = not_sure.spoken(cfg.sure_enough_to_say_plainly);
    assert_ne!(a, b, "the same sentence for both is the failure");
    assert!(b.contains("looks like"), "{b}");
    assert!(!a.contains("looks like"), "{a}");
}

#[test]
fn a_room_with_nothing_nameable_says_so_without_dressing_it_up() {
    let said = Scene::default().spoken(&settings());
    // Not an error, and not a confident nothing either — `hollow` exists
    // because "All fine. 0 gigabytes free" reads as an answer.
    assert!(said.contains("nothing I can name"), "{said}");
    assert!(!said.contains("0"), "a count of zero is not an answer: {said}");
}

#[test]
fn an_unrecognised_person_is_said_to_be_unrecognised_rather_than_left_out() {
    let scene = Scene {
        faces: vec![Face {
            area: Patch::new(0.4, 0.2, 0.2, 0.3),
            sure: 0.9,
            who: None,
            sure_who: 0.0,
        }],
        ..Scene::default()
    };
    let said = scene.spoken(&settings());
    assert!(said.contains("don't recognise"), "{said}");
}

// ---------------------------------------------------------------------------
// Pointing
// ---------------------------------------------------------------------------

#[test]
fn the_thing_being_pointed_at_is_the_smallest_one_under_the_finger() {
    // A finger over a keyboard is also over the desk and the monitor. The
    // smallest box is the specific answer; the largest is always "desk".
    let scene = Scene {
        things: vec![
            thing("dining table", 0.0, 0.0, 1.0, 1.0, 0.8),
            thing("tv", 0.1, 0.1, 0.6, 0.6, 0.8),
            thing("keyboard", 0.3, 0.3, 0.1, 0.1, 0.8),
        ],
        ..Scene::default()
    };
    assert_eq!(scene.at(0.35, 0.35).map(|t| t.name.as_str()), Some("keyboard"));
    assert_eq!(scene.at(0.15, 0.15).map(|t| t.name.as_str()), Some("tv"));
    assert_eq!(scene.at(0.95, 0.95).map(|t| t.name.as_str()), Some("dining table"));
}

#[test]
fn pointing_at_nothing_is_nothing_rather_than_the_nearest_guess() {
    let scene = Scene {
        things: vec![thing("keyboard", 0.3, 0.3, 0.1, 0.1, 0.8)],
        ..Scene::default()
    };
    assert!(scene.at(0.9, 0.9).is_none());
}

// ---------------------------------------------------------------------------
// Things Atlas has been shown
// ---------------------------------------------------------------------------

#[test]
fn a_thing_shown_once_is_recognised_again() {
    let mut album = Album::default();
    album.remember_thing("my mug", &[1.0, 0.0, 0.0, 0.0], 10).unwrap();
    match album.which_thing(&[0.98, 0.1, 0.0, 0.0], 0.5, 0.06) {
        Guess::Is(name, _) => assert_eq!(name, "my mug"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn two_close_matches_are_reported_as_not_knowing_rather_than_as_the_winner() {
    // The margin rule. Without it the higher of two near-identical scores is
    // announced as a fact, and a coin toss reads as recognition.
    let mut album = Album::default();
    album.remember_face("alex", &[1.0, 0.0, 0.0], 1).unwrap();
    album.remember_face("sam", &[0.999, 0.045, 0.0], 1).unwrap();
    match album.whose_face(&[1.0, 0.02, 0.0], 0.5, 0.06) {
        Guess::Unsure(who) => {
            assert_eq!(who.len(), 2, "{who:?}");
            assert!(who.contains(&"alex".to_string()) && who.contains(&"sam".to_string()));
        }
        other => panic!("a close call was reported as certainty: {other:?}"),
    }
}

#[test]
fn nothing_like_it_is_no_idea_rather_than_the_least_bad_option() {
    let mut album = Album::default();
    album.remember_thing("my mug", &[1.0, 0.0, 0.0], 1).unwrap();
    assert_eq!(album.which_thing(&[0.0, 0.0, 1.0], 0.5, 0.06), Guess::NoIdea);
}

#[test]
fn an_empty_reading_is_refused_rather_than_stored() {
    // All zeros would match everything a little and nothing well, and the
    // symptom would be "recognition is unreliable" rather than "that
    // recording was no good".
    let mut album = Album::default();
    assert!(album.remember_thing("my mug", &[0.0, 0.0, 0.0], 1).is_err());
    assert!(album.remember_thing("my mug", &[], 1).is_err());
    assert!(album.remember_thing("my mug", &[f32::NAN, 1.0], 1).is_err());
    assert!(album.things.is_empty(), "nothing was stored");
}

#[test]
fn a_thing_with_no_name_is_refused() {
    let mut album = Album::default();
    assert!(album.remember_thing("   ", &[1.0, 0.0], 1).is_err());
}

#[test]
fn showing_the_same_thing_twice_adds_a_view_rather_than_a_second_entry() {
    let mut album = Album::default();
    album.remember_thing("my mug", &[1.0, 0.0], 1).unwrap();
    album.remember_thing("My Mug", &[0.0, 1.0], 2).unwrap();
    assert_eq!(album.things.len(), 1, "one thing, two views");
    assert_eq!(album.things[0].views.len(), 2);
    // And either view finds it, which is the point of keeping more than one.
    assert!(matches!(album.which_thing(&[0.0, 1.0], 0.5, 0.06), Guess::Is(_, _)));
    assert!(matches!(album.which_thing(&[1.0, 0.0], 0.5, 0.06), Guess::Is(_, _)));
}

#[test]
fn readings_from_different_models_are_never_compared() {
    // A face reading and a picture reading are different lengths and mean
    // nothing to each other. A number comparing them would still be a number.
    assert_eq!(alike(&[1.0, 0.0], &[1.0, 0.0, 0.0]), 0.0);
    assert_eq!(alike(&[], &[]), 0.0);
}

#[test]
fn forgetting_a_thing_actually_removes_it() {
    let mut album = Album::default();
    album.remember_thing("my mug", &[1.0, 0.0], 1).unwrap();
    assert!(album.forget("MY MUG"));
    assert!(album.things.is_empty());
    assert!(!album.forget("my mug"), "forgetting it twice is not a second removal");
}

#[test]
fn what_it_has_been_shown_says_how_many_views_back_each_one() {
    // One view is a system that knows you in one light. Saying so is what
    // makes someone show it again.
    let mut album = Album::default();
    album.remember_face("me", &[1.0, 0.0], 1).unwrap();
    let listed = album.everything();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].contains("me"), "{:?}", listed[0]);
    assert!(listed[0].contains("1 view"), "{:?}", listed[0]);
}

#[test]
fn a_reading_is_scaled_before_it_is_stored_so_a_brighter_picture_is_not_a_better_match() {
    let a = to_unit(&[3.0, 4.0]).unwrap();
    let length: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    assert!((length - 1.0).abs() < 0.001, "{length}");
    assert!(to_unit(&[0.0, 0.0]).is_none());
    assert!(to_unit(&[f32::INFINITY]).is_none());
}

// ---------------------------------------------------------------------------
// Reading a detector
// ---------------------------------------------------------------------------

/// A frame's worth of nothing, with one bright square in it.
fn blank(w: usize, h: usize) -> Vec<u8> {
    vec![90u8; w * h * 3]
}

#[test]
fn a_wide_frame_is_fitted_into_a_square_model_without_being_squashed() {
    let (pixels, placed) = letterbox(&blank(640, 480), 640, 480, Kind::Objects);
    let r = Kind::Objects.recipe();
    assert_eq!(pixels.len(), r.values());
    // The picture keeps its shape and the spare space is padding.
    assert!((placed.scale - 1.0).abs() < 0.001, "{placed:?}");
    assert!(placed.pad_y > 70.0, "the short side should be padded: {placed:?}");
    assert!(placed.pad_x.abs() < 0.001, "the long side should not be: {placed:?}");
}

#[test]
fn a_box_found_in_the_model_comes_back_as_a_place_in_the_real_frame() {
    let placed = Framing::work_out(640, 480, 640, 640);
    // Dead centre of the padded square is dead centre of the frame.
    let area = placed.back(320.0 - 10.0, 320.0 - 10.0, 20.0, 20.0);
    assert!((area.x + area.width / 2.0 - 0.5).abs() < 0.01, "{area:?}");
    assert!((area.y + area.height / 2.0 - 0.5).abs() < 0.01, "{area:?}");
}

#[test]
fn the_face_model_is_read_from_the_outputs_that_hold_the_boxes() {
    // Twelve outputs. The boxes are outputs seven to nine, and reading output
    // zero and calling it the answer is how a detector that runs perfectly
    // never finds a face. Built here as the real model lays it out.
    let (w, h) = Kind::Faces.wants();
    let sizes: Vec<usize> = [8usize, 16, 32].iter().map(|s| (w / s) * (h / s)).collect();
    let mut outs: Vec<Vec<f32>> = Vec::new();
    for n in &sizes {
        outs.push(vec![0.0; *n]); // cls
    }
    for n in &sizes {
        outs.push(vec![0.0; *n]); // obj
    }
    for n in &sizes {
        outs.push(vec![0.0; n * 4]); // bbox
    }
    for n in &sizes {
        outs.push(vec![0.0; n * 10]); // keypoints
    }

    // One face, at the coarsest level, in cell (5, 5): stride 32, so the
    // middle lands at (5.5*32, 5.5*32) out of 640.
    let cols = w / 32;
    let cell = 5 * cols + 5;
    outs[2][cell] = 1.0;
    outs[5][cell] = 1.0;
    outs[8][cell * 4] = 0.5;
    outs[8][cell * 4 + 1] = 0.5;
    outs[8][cell * 4 + 2] = 0.0; // exp(0) * 32 = 32 wide
    outs[8][cell * 4 + 3] = 0.0;

    let placed = Framing::work_out(640, 640, 640, 640);
    let found = faces(&Outputs::of("face_detect.onnx", outs), &placed, &settings()).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    let a = found[0].area;
    assert!((a.x + a.width / 2.0 - 176.0 / 640.0).abs() < 0.01, "{a:?}");
    assert!((a.y + a.height / 2.0 - 176.0 / 640.0).abs() < 0.01, "{a:?}");
    assert!(found[0].who.is_none(), "finding a face is not knowing whose it is");
}

#[test]
fn a_face_model_with_the_wrong_shape_is_named_rather_than_read_as_noise() {
    let outs = vec![vec![0.0; 4]; 12];
    let placed = Framing::work_out(640, 640, 640, 640);
    let err = faces(&Outputs::of("face_detect.onnx", outs), &placed, &settings()).unwrap_err();
    let said = format!("{err}");
    assert!(said.contains("isn't the model I was written for"), "{said}");
}

#[test]
fn an_output_a_model_does_not_have_is_an_error_that_names_the_model() {
    let outs = Outputs::of("objects.onnx", vec![vec![1.0, 2.0]]);
    let err = outs.at(3).unwrap_err();
    let said = format!("{err}");
    assert!(said.contains("objects.onnx"), "{said}");
    assert!(said.contains("isn't the model I was written for"), "{said}");
}

#[test]
fn the_object_model_is_read_into_a_name_and_a_place() {
    let (w, h) = Kind::Objects.wants();
    let cells: usize = [8usize, 16, 32].iter().map(|s| (w / s) * (h / s)).sum();
    let names = 80usize;
    let row = 5 + names;
    let mut raw = vec![0.0f32; cells * row];

    // One strong candidate in the first (finest) level, cell (10, 10).
    let cols = w / 8;
    let i = 10 * cols + 10;
    raw[i * row] = 0.5;
    raw[i * row + 1] = 0.5;
    raw[i * row + 2] = 0.0;
    raw[i * row + 3] = 0.0;
    raw[i * row + 4] = 0.95; // it is something
    raw[i * row + 5 + 41] = 0.9; // and that something is name 41

    let placed = Framing::work_out(640, 640, 640, 640);
    let found = things(&Outputs::of("objects.onnx", vec![raw]), &placed, &settings()).unwrap();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(!found[0].name.is_empty());
    let a = found[0].area;
    assert!((a.x + a.width / 2.0 - 84.0 / 640.0).abs() < 0.01, "{a:?}");
}

#[test]
fn a_name_list_that_does_not_match_the_model_is_refused_rather_than_used_anyway() {
    // The failure this guard exists for: a model with a different number of
    // names read with this one's names is every answer confidently wrong and
    // nothing reporting a problem.
    let placed = Framing::work_out(640, 640, 640, 640);
    let err = things(
        &Outputs::of("objects.onnx", vec![vec![0.0; 99]]),
        &placed,
        &settings(),
    )
    .unwrap_err();
    let said = format!("{err}");
    assert!(said.contains("don't match"), "{said}");
}

#[test]
fn a_picture_score_list_that_does_not_match_is_refused_too() {
    let err = whole_picture(&Outputs::of("picture.onnx", vec![vec![0.1; 7]]), &settings())
        .unwrap_err();
    assert!(format!("{err}").contains("don't match"), "{err}");
}

#[test]
fn an_unsure_object_is_left_out_rather_than_listed_weakly() {
    let (w, _) = Kind::Objects.wants();
    let cells: usize = [8usize, 16, 32].iter().map(|s| (w / s) * (w / s)).sum();
    let row = 85;
    let mut raw = vec![0.0f32; cells * row];
    let i = 3;
    raw[i * row + 4] = 0.2; // below the floor
    raw[i * row + 5] = 0.9;
    let placed = Framing::work_out(640, 640, 640, 640);
    let found = things(&Outputs::of("objects.onnx", vec![raw]), &placed, &settings()).unwrap();
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn the_same_thing_found_three_times_is_reported_once() {
    let boxes = vec![
        (Patch::new(0.10, 0.10, 0.20, 0.20), 0.70),
        (Patch::new(0.11, 0.11, 0.20, 0.20), 0.90),
        (Patch::new(0.12, 0.12, 0.20, 0.20), 0.60),
        (Patch::new(0.70, 0.70, 0.20, 0.20), 0.80),
    ];
    let kept = thin_out(&boxes, 0.45);
    assert_eq!(kept.len(), 2, "{kept:?}");
    assert!(kept.contains(&1), "the strongest of the overlapping three survives: {kept:?}");
    assert!(kept.contains(&3), "and the one somewhere else: {kept:?}");
}

// ---------------------------------------------------------------------------
// Preparing a picture: the assumptions that fail silently
// ---------------------------------------------------------------------------

#[test]
fn models_that_want_the_channels_apart_get_them_apart() {
    // Handing a model the wrong layout is not an error — the numbers all fit
    // — it is a model that runs perfectly and sees nothing recognisable. So
    // the arrangement is stated per model rather than assumed once.
    assert_eq!(Kind::HandLandmarks.recipe().layout, Layout::Interleaved);
    assert_eq!(Kind::Faces.recipe().layout, Layout::Planar);
    assert_eq!(Kind::Objects.recipe().layout, Layout::Planar);
    assert_eq!(Kind::Picture.recipe().layout, Layout::Planar);
}

#[test]
fn laying_a_picture_out_for_a_planar_model_puts_all_the_reds_first() {
    let r = atlas::infer::Recipe {
        width: 2,
        height: 1,
        layout: Layout::Planar,
        top: 1.0,
        mean: [0.0; 3],
        deviation: [1.0; 3],
        blue_first: false,
    };
    // Two pixels: pure red, then pure green.
    let fitted = vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
    let out = atlas::infer::arrange(&fitted, &r);
    assert_eq!(out, vec![1.0, 0.0, /* reds */ 0.0, 1.0, /* greens */ 0.0, 0.0]);
}

#[test]
fn a_model_that_wants_blue_first_gets_blue_first() {
    let r = atlas::infer::Recipe {
        width: 1,
        height: 1,
        layout: Layout::Interleaved,
        top: 1.0,
        mean: [0.0; 3],
        deviation: [1.0; 3],
        blue_first: true,
    };
    let out = atlas::infer::arrange(&[1.0, 0.5, 0.0], &r);
    assert_eq!(out, vec![0.0, 0.5, 1.0], "red and blue swap, green stays put");
}

#[test]
fn a_model_that_wants_whole_numbers_does_not_get_fractions() {
    let r = Kind::Faces.recipe();
    assert!((r.top - 255.0).abs() < 0.001, "{r:?}");
    // Heap-allocated, not a stack array: 640*640*3 f32s is ~4.7MB, which is
    // larger than the default stack a test thread gets and overflows it
    // immediately -- a crash in the test's own setup, not in `arrange`,
    // which already heap-allocates everything it touches.
    let white = vec![1.0f32; 640 * 640 * 3];
    let out = atlas::infer::arrange(&white, &r);
    assert!((out[0] - 255.0).abs() < 0.001, "{}", out[0]);
}

#[test]
fn every_model_names_itself_in_words_and_in_a_file_of_its_own() {
    let all = Kind::all();
    for k in all {
        assert!(k.file().ends_with(".onnx"), "{}", k.file());
        assert!(!k.plain().contains('_'), "{}", k.plain());
        let r = k.recipe();
        assert!(r.width > 0 && r.height > 0, "{:?}", k);
        assert!(r.deviation.iter().all(|d| *d != 0.0), "dividing by nothing: {k:?}");
    }
    for (i, a) in all.iter().enumerate() {
        for b in all.iter().skip(i + 1) {
            assert_ne!(a.file(), b.file(), "two models cannot share a filename");
            assert_ne!(a.plain(), b.plain(), "or a description");
        }
    }
}

#[test]
fn seeing_and_hand_tracking_are_reported_as_separate_installs() {
    // One list would report seeing as broken because hand tracking is not set
    // up, and the person would go looking for the wrong file.
    let nowhere = std::path::Path::new("/definitely/not/here");
    let seeing = atlas::infer::whats_missing(nowhere, &Kind::for_seeing());
    let hands = atlas::infer::whats_missing(nowhere, &Kind::for_hands());
    assert_eq!(seeing.len(), 4);
    assert_eq!(hands.len(), 2);
    for (k, _) in &seeing {
        assert!(!Kind::for_hands().contains(k), "{k:?} is in both lists");
    }
}

// ---------------------------------------------------------------------------
// A face is not a password
// ---------------------------------------------------------------------------

#[test]
fn nothing_in_here_turns_a_face_into_permission() {
    // Guarded by reading the source, because the failure is a line of code
    // that does not exist yet: the moment something asks `vision` whether it
    // may proceed, a photograph held up to a webcam becomes a key.
    let source = include_str!("../src/vision.rs");
    for word in ["approve", "authorise", "authorize", "unlock", "grant", "may_proceed"] {
        assert!(
            !source.to_lowercase().contains(&format!("fn {word}")),
            "vision must never decide whether something is allowed: found {word}"
        );
    }
    assert!(
        source.contains("A face is not a password"),
        "the rule has to stay written down where the next person will read it"
    );
}

#[test]
fn naming_a_face_is_a_higher_bar_than_finding_one() {
    // One number cannot mean both "there is a face here" and "that is Eric",
    // and using one for both is how a stranger becomes a session.
    let cfg = VisionConfig::default();
    assert!(
        cfg.sure_enough_to_name_a_face > cfg.floor,
        "{} is not above {}",
        cfg.sure_enough_to_name_a_face,
        cfg.floor
    );
}

#[test]
fn seeing_is_off_until_it_is_turned_on() {
    assert!(
        !VisionConfig::default().enabled,
        "a camera that starts looking because you updated is not something anyone should have \
         to discover"
    );
}
