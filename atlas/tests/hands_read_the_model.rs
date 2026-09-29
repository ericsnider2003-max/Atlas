//! What the hand models actually hand back, and what Atlas does with it.
//!
//! All of this was written before there were any weights to run, and every
//! one of these tests exists because something in that code was wrong in a
//! way that could not show up until a real model was installed. None of them
//! would have failed on a machine with no models: the feature simply did
//! nothing, which is this codebase's signature failure.

use atlas::handloop::where_the_hand_is;
use atlas::handshape::{from_model, in_the_frame, POINTS};
use atlas::infer::Outputs;

/// The finding model's two outputs, at the size the real one reports.
fn hand_outputs(boxes: Vec<f32>, scores: Vec<f32>) -> Outputs {
    Outputs::of("hand_presence.onnx", vec![boxes, scores])
}

const CANDIDATES: usize = 24 * 24 * 2 + 12 * 12 * 6;

#[test]
fn the_candidate_count_matches_what_the_model_reports() {
    assert_eq!(CANDIDATES, 2016, "the model reports 2016 candidates per frame");
}

#[test]
fn whether_a_hand_is_there_is_read_from_the_scores_and_not_from_the_boxes() {
    // The bug, stated: the old code read result number zero and compared it
    // to a half. Result zero is the box measurements. So "is there a hand"
    // came down to an arbitrary coordinate offset, and the symptom would have
    // been tracking that works sometimes for no reason.
    let all_boxes_large = vec![9.0; CANDIDATES * 18];
    let no_hand_at_all = vec![-12.0; CANDIDATES];
    assert!(
        where_the_hand_is(&hand_outputs(all_boxes_large, no_hand_at_all)).is_none(),
        "big numbers in the boxes are not a hand"
    );
}

#[test]
fn a_confident_candidate_is_found_and_placed() {
    let mut boxes = vec![0.0; CANDIDATES * 18];
    let mut scores = vec![-12.0; CANDIDATES];
    // The very first candidate sits at the middle of the first cell of a
    // 24-by-24 grid, so half a cell in from the corner.
    scores[0] = 12.0;
    boxes[2] = 48.0; // a quarter of 192 wide
    boxes[3] = 48.0;
    let found = where_the_hand_is(&hand_outputs(boxes, scores)).expect("a hand");
    assert!((found.x + found.width / 2.0 - 0.5 / 24.0).abs() < 0.01, "{found:?}");
    assert!((found.y + found.height / 2.0 - 0.5 / 24.0).abs() < 0.01, "{found:?}");
    assert!((found.width - 0.25).abs() < 0.01, "{found:?}");
}

#[test]
fn a_model_of_the_wrong_shape_is_no_hand_rather_than_a_wrong_one() {
    assert!(where_the_hand_is(&hand_outputs(vec![0.0; 10], vec![9.0; 3])).is_none());
    assert!(where_the_hand_is(&Outputs::of("hand_presence.onnx", vec![vec![0.0; 10]])).is_none());
}

// ---------------------------------------------------------------------------
// The reading has to come back in the frame's terms
// ---------------------------------------------------------------------------

#[test]
fn joints_measured_in_the_models_own_pixels_come_back_as_places_in_the_frame() {
    // This is the one that would have been most confusing to debug. The
    // landmark model answers in pixels of its own 224-pixel input; everything
    // downstream multiplies by the width of the screen. Uncorrected, the very
    // first sighting throws the pointer thousands of pixels off the display
    // and leaves it there, and it reads as the camera being broken.
    let mut joints = vec![0.0f32; POINTS * 3];
    for i in 0..POINTS {
        joints[i * 3] = 112.0; // the middle of the model's picture
        joints[i * 3 + 1] = 112.0;
    }
    // The hand was found in a box a quarter of the way across the frame,
    // half the frame wide.
    let mapped = in_the_frame(&joints, 0.8, (224.0, 224.0), (0.25, 0.25), (0.5, 0.5));
    let marks = from_model(&mapped).expect("a reading");
    assert!(
        (marks.points[8].x - 0.5).abs() < 0.01,
        "the middle of that crop is the middle of the frame: {:?}",
        marks.points[8]
    );
    assert!((marks.points[8].y - 0.5).abs() < 0.01, "{:?}", marks.points[8]);
    assert!(
        marks.points.iter().all(|p| (0.0..=1.0).contains(&p.x)),
        "everything downstream treats these as fractions"
    );
}

#[test]
fn how_sure_the_model_is_survives_the_journey() {
    // It arrives in the model's *second* output. Reading only the first is
    // why every reading came back perfectly certain — including the ones
    // taken of an empty desk.
    let joints = vec![50.0f32; POINTS * 3];
    let mapped = in_the_frame(&joints, 0.42, (224.0, 224.0), (0.0, 0.0), (1.0, 1.0));
    let marks = from_model(&mapped).expect("a reading");
    assert!((marks.sure - 0.42).abs() < 0.001, "{}", marks.sure);
}

#[test]
fn a_short_reading_is_refused_rather_than_padded() {
    // Zeros would put every missing joint at the top-left of the frame, which
    // reads as a real hand in a real place.
    assert!(in_the_frame(&[1.0, 2.0], 1.0, (224.0, 224.0), (0.0, 0.0), (1.0, 1.0)).is_empty());
    assert!(in_the_frame(&[0.0; 63], 1.0, (0.0, 224.0), (0.0, 0.0), (1.0, 1.0)).is_empty());
}
