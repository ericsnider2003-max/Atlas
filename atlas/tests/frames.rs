//! Pictures out of the camera, continuously.
//!
//! Atlas already had `capture_webcam`: run ffmpeg, get one PNG. That is right
//! for looking at the room once every twenty seconds and wrong for tracking a
//! hand — it pays the whole cost of opening the camera and starting a process
//! for every frame. Fifteen times a second that is a machine doing nothing but
//! starting and stopping ffmpeg.

use atlas::frames::{from_capture_args, Feed, Rolling};
use atlas::handshape::{from_model, POINTS};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

// ---------------------------------------------------------------------------
// Reusing what already knows how to open the camera
// ---------------------------------------------------------------------------

#[test]
fn the_one_shot_capture_command_is_reused_for_the_continuous_feed() {
    // `capture_webcam` already knows which device and which backend this
    // machine uses. Re-deriving that would be two places to get right and one
    // of them silently rotting.
    let one_shot = args(&[
        "-hide_banner", "-loglevel", "error", "-f", "dshow", "-i",
        "video=Integrated Camera", "-frames:v", "1", "-y", "out.png",
    ]);
    let feed = from_capture_args(&one_shot);
    assert!(feed.contains(&"dshow".to_string()), "{feed:?}");
    assert!(
        feed.contains(&"video=Integrated Camera".to_string()),
        "the device has to survive: {feed:?}"
    );
}

#[test]
fn everything_about_writing_one_png_is_dropped() {
    let one_shot = args(&["-i", "video=cam", "-frames:v", "1", "-y", "shot.png"]);
    let feed = from_capture_args(&one_shot);
    for gone in ["-frames:v", "1", "-y", "shot.png"] {
        assert!(
            !feed.contains(&gone.to_string()),
            "{gone} belongs to the one-shot version and must not survive: {feed:?}"
        );
    }
}

#[test]
fn a_templated_output_path_is_dropped_too() {
    let one_shot = args(&["-i", "video=cam", "-y", "{out_png}"]);
    assert!(!from_capture_args(&one_shot).iter().any(|a| a.contains("out_png")));
}

#[test]
fn an_empty_capture_command_produces_an_empty_feed_rather_than_nonsense() {
    assert!(from_capture_args(&[]).is_empty());
}

// ---------------------------------------------------------------------------
// Opening it
// ---------------------------------------------------------------------------

#[test]
fn a_camera_that_was_never_configured_says_so() {
    // Not a silent no-op. Nothing happening is what makes a person conclude
    // the whole feature doesn't work.
    let said = match Rolling::start(&Feed::default()) {
        Ok(_) => panic!("it opened a camera that was never configured"),
        Err(e) => format!("{e}"),
    };
    assert!(said.contains("no camera set up"), "{said}");
    assert!(said.contains("tools.yaml"), "and where to fix it: {said}");
}

#[test]
fn the_capture_size_is_small_because_the_model_shrinks_it_anyway() {
    // Capturing at 1080p means moving several megabytes a frame in order to
    // throw almost all of it away.
    let f = Feed::default();
    assert!(f.width <= 1280 && f.height <= 720, "{}x{}", f.width, f.height);
    assert!(f.width >= 320, "too small to find a hand in");
}

// ---------------------------------------------------------------------------
// Reading what the model returned
// ---------------------------------------------------------------------------

fn flat(n: usize) -> Vec<f32> {
    (0..n).map(|i| (i % 100) as f32 / 100.0).collect()
}

#[test]
fn a_full_set_of_joints_is_read() {
    let out = flat(POINTS * 3);
    let marks = from_model(&out).expect("a whole hand");
    assert!((marks.points[0].x - out[0]).abs() < 0.001);
    assert!((marks.points[20].z - out[POINTS * 3 - 1]).abs() < 0.001);
    assert_eq!(marks.sure, 1.0, "no confidence given means certain");
}

#[test]
fn a_confidence_on_the_end_is_used_when_the_model_provides_one() {
    let mut out = flat(POINTS * 3);
    out.push(0.42);
    assert!((from_model(&out).unwrap().sure - 0.42).abs() < 0.001);
}

#[test]
fn a_short_output_is_refused_rather_than_padded() {
    // Padding with zeros would put every missing joint at the top-left corner
    // of the frame, which reads as a real hand in a real place.
    assert!(from_model(&flat(POINTS * 3 - 1)).is_none());
    assert!(from_model(&[]).is_none());
}

#[test]
fn a_broken_output_is_refused_rather_than_tracked() {
    let mut out = flat(POINTS * 3);
    out[8] = f32::NAN;
    assert!(
        from_model(&out).is_none(),
        "a hand at not-a-number would send the pointer nowhere describable"
    );
    let mut inf = flat(POINTS * 3);
    inf[2] = f32::INFINITY;
    assert!(from_model(&inf).is_none());
}

#[test]
fn a_confidence_outside_its_range_is_brought_back_in() {
    let mut out = flat(POINTS * 3);
    out.push(7.5);
    assert_eq!(from_model(&out).unwrap().sure, 1.0);
}

#[test]
fn the_joints_keep_the_order_every_gesture_depends_on() {
    // `Reading` looks up fingertips by fixed index. If the order shifted,
    // every gesture would still evaluate and all of them would be wrong.
    let out = flat(POINTS * 3);
    let marks = from_model(&out).unwrap();
    assert_eq!(marks.points.len(), POINTS);
    assert!((marks.index_tip().x - out[8 * 3]).abs() < 0.001);
    assert!((marks.thumb_tip().x - out[4 * 3]).abs() < 0.001);
}
