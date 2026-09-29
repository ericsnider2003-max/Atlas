//! Running a model inside Atlas.
//!
//! `tract` compiles into `atlas.exe`, so this is the first time Atlas runs a
//! model in its own process rather than launching somebody else's program.
//! What matters here is the behaviour around the model, not the arithmetic
//! inside it: a missing file has to be named, a wrong-sized frame has to be
//! caught before it reaches the engine, and loading has to stay separate from
//! running.

use atlas::infer::{fit, spoken, whats_missing, Kind, Model};
use std::path::Path;

// ---------------------------------------------------------------------------
// A missing model is a stated fact, not a silent no-op
// ---------------------------------------------------------------------------

#[test]
fn a_model_that_is_not_installed_is_named_rather_than_shrugged_at() {
    // The failure this codebase keeps producing: a feature that does nothing
    // and says nothing, so the person concludes the whole idea doesn't work.
    let nowhere = Path::new("/definitely/not/here");
    let err = Model::load(Kind::HandLandmarks, nowhere).unwrap_err();
    let said = format!("{err}");
    assert!(said.contains("reading your fingers"), "{said}");
    assert!(said.contains("hand_landmarks.onnx"), "and which file: {said}");
}

#[test]
fn what_is_missing_is_listed_before_anything_tries_to_start() {
    let nowhere = Path::new("/definitely/not/here");
    let missing = whats_missing(nowhere, &Kind::for_hands());
    assert_eq!(missing.len(), 2, "both models are absent");
    let said = spoken(&missing);
    assert!(said.contains("one-off download"), "{said}");
    assert!(
        said.contains("finding your hands") && said.contains("reading your fingers"),
        "each one is named in words, not by filename: {said}"
    );
}

#[test]
fn nothing_missing_says_so_plainly() {
    let said = spoken(&[]);
    assert!(said.contains("Everything I need"), "{said}");
}

#[test]
fn every_kind_names_itself_in_words_and_in_a_filename() {
    for k in [Kind::HandPresence, Kind::HandLandmarks] {
        assert!(!k.plain().contains('_'), "{}", k.plain());
        assert!(k.plain().split_whitespace().count() >= 2, "{}", k.plain());
        assert!(k.file().ends_with(".onnx"), "{}", k.file());
        let (w, h) = k.wants();
        assert!(w > 0 && h > 0, "a model has to want a real size");
    }
    assert_ne!(
        Kind::HandPresence.file(),
        Kind::HandLandmarks.file(),
        "two models cannot share a filename"
    );
}

// ---------------------------------------------------------------------------
// Preparing a frame
// ---------------------------------------------------------------------------

#[test]
fn a_frame_is_resized_to_exactly_what_the_model_wants() {
    let (w, h) = Kind::HandPresence.wants();
    let camera = vec![128u8; 640 * 480 * 3];
    let ready = fit(&camera, 640, 480, w, h);
    assert_eq!(
        ready.len(),
        w * h * 3,
        "the size mismatch has to be impossible by the time it reaches the engine"
    );
}

#[test]
fn pixels_come_out_as_fractions_rather_than_bytes() {
    let camera = vec![255u8; 4 * 4 * 3];
    let ready = fit(&camera, 4, 4, 2, 2);
    assert!(ready.iter().all(|v| (0.0..=1.0).contains(v)), "{ready:?}");
    assert!((ready[0] - 1.0).abs() < 0.001);
}

#[test]
fn resizing_keeps_the_picture_the_right_way_up() {
    // Half black, half white, top to bottom. A resize that flips or transposes
    // would put the hand in the wrong place and be almost impossible to spot
    // from the output.
    let mut camera = vec![0u8; 8 * 8 * 3];
    for y in 4..8 {
        for x in 0..8 {
            for c in 0..3 {
                camera[(y * 8 + x) * 3 + c] = 255;
            }
        }
    }
    let ready = fit(&camera, 8, 8, 4, 4);
    let top = ready[0];
    let bottom = ready[(3 * 4) * 3];
    assert!(top < 0.5 && bottom > 0.5, "top {top}, bottom {bottom}");
}

#[test]
fn a_frame_of_nothing_produces_nothing_rather_than_a_panic() {
    assert!(fit(&[], 0, 0, 8, 8).is_empty());
}

#[test]
fn a_short_frame_is_padded_rather_than_read_past_its_end() {
    // A camera that hands back a truncated buffer must not read out of
    // bounds — the sort of thing that takes the whole daemon down.
    let truncated = vec![200u8; 10];
    let ready = fit(&truncated, 8, 8, 4, 4);
    assert_eq!(ready.len(), 4 * 4 * 3);
    assert!(ready.iter().all(|v| v.is_finite()));
}

/// A downscale must not shift the picture.
///
/// Sampling the top-left of each source region rather than its middle moves
/// everything up and left by half a region — several real pixels on a
/// downscale, applied to every frame. That is a constant bias in where the
/// model thinks your hand is, and it would look like the tracking simply
/// being slightly wrong.
#[test]
fn resizing_does_not_shift_the_picture() {
    // A bright band down the middle of an otherwise black frame.
    let mut camera = vec![0u8; 16 * 16 * 3];
    for y in 0..16 {
        for x in 7..9 {
            for c in 0..3 {
                camera[(y * 16 + x) * 3 + c] = 255;
            }
        }
    }
    let ready = fit(&camera, 16, 16, 8, 8);
    let brightness = |x: usize| ready[(0 * 8 + x) * 3];
    let left: f32 = (0..4).map(brightness).sum();
    let right: f32 = (4..8).map(brightness).sum();
    assert!(
        (left - right).abs() < 1.2,
        "the band drifted to one side: left {left}, right {right}"
    );
}
