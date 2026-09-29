//! Reading what ffmpeg actually said.
//!
//! Every string in here is real output, captured from ffmpeg 6.x on the
//! machine this was built on rather than written from memory of the format.
//! That distinction is the whole value of the file: a parser tested against
//! invented output tests the invention.
//!
//! The band constants are pinned the same way — against the figures the real
//! calibration produced — so that steepening or loosening a filter cannot
//! quietly move where "rumble" starts.

use atlas::market::timeframe::Tf;
use atlas::measure::{
    frame_shape_from_probe, grey_stats, has_harsh_s, has_rumble, loudness_from_loudnorm,
    mean_volume_db, noise_floor_from_astats, parse_rational, unmeasured,
};

const LOUDNORM: &str = r#"
frame=  180 fps=0.0 q=-0.0 Lsize=N/A time=00:00:05.96 bitrate=N/A speed=27.7x
[Parsed_loudnorm_0 @ 0x55b870d88940]
{
	"input_i" : "-22.03",
	"input_tp" : "-12.95",
	"input_lra" : "0.10",
	"input_thresh" : "-32.03",
	"output_i" : "-14.03",
	"output_tp" : "-4.97",
	"output_lra" : "0.10",
	"output_thresh" : "-24.03",
	"normalization_type" : "dynamic",
	"target_offset" : "0.03"
}
"#;

const ASTATS: &str = "\
[Parsed_astats_0 @ 0x55812aba1940] Overall
[Parsed_astats_0 @ 0x55812aba1940] RMS level dB: -21.080556
[Parsed_astats_0 @ 0x55812aba1940] Noise floor dB: -18.163953
";

const VOLUMEDETECT: &str = "\
[Parsed_volumedetect_0 @ 0x5555a0aca940] mean_volume: -21.1 dB
[Parsed_volumedetect_0 @ 0x5555a0aca940] max_volume: -13.0 dB
";

const PROBE: &str = r#"{
    "streams": [
        { "codec_type": "audio" },
        { "codec_type": "video", "width": 640, "height": 360, "r_frame_rate": "30/1" }
    ],
    "format": { "duration": "6.000000" }
}"#;

#[test]
fn loudness_comes_off_the_input_side_not_the_output_side() {
    // The block carries both. `output_i` is what loudnorm *would* produce,
    // which is -14 every time by construction and says nothing about the file.
    let (i, tp, lra) = loudness_from_loudnorm(LOUDNORM).expect("a loudnorm block");
    assert!((i - -22.03).abs() < 0.001, "got {i}");
    assert!((tp - -12.95).abs() < 0.001, "got {tp}");
    assert!((lra - 0.10).abs() < 0.001, "got {lra}");
}

#[test]
fn a_run_with_no_audio_track_gives_nothing_rather_than_zero() {
    // Zero LUFS is a real and very loud measurement. Reading "no block" as
    // zero would report a silent file as the loudest thing ever recorded.
    assert!(loudness_from_loudnorm("frame= 180 fps=0.0\n").is_none());
}

#[test]
fn the_noise_floor_is_read_off_astats() {
    let f = noise_floor_from_astats(ASTATS).expect("a noise floor line");
    assert!((f - -18.163953).abs() < 0.001, "got {f}");
}

#[test]
fn the_rms_line_is_not_mistaken_for_the_noise_floor() {
    // They sit next to each other and differ by 3dB here, which is exactly
    // small enough to look plausible if the wrong one were picked up.
    assert_ne!(
        noise_floor_from_astats(ASTATS),
        Some(-21.080556),
        "that is the RMS level, not the noise floor"
    );
}

#[test]
fn mean_volume_is_read_and_max_volume_is_not() {
    assert_eq!(mean_volume_db(VOLUMEDETECT), Some(-21.1));
}

#[test]
fn the_video_stream_is_found_past_the_audio_one() {
    let (w, h, fps) = frame_shape_from_probe(PROBE).expect("a video stream");
    assert_eq!((w, h), (640, 360));
    assert!((fps - 30.0).abs() < 0.001);
}

#[test]
fn a_file_with_no_video_stream_gives_nothing() {
    assert!(frame_shape_from_probe(r#"{"streams":[{"codec_type":"audio"}]}"#).is_none());
}

#[test]
fn ntsc_frame_rates_are_read_as_the_fraction_they_are() {
    let fps = parse_rational("30000/1001").expect("a rate");
    assert!((fps - 29.97).abs() < 0.01, "got {fps}");
    // 0/0 is what ffprobe writes for a stream with no meaningful rate.
    assert!(parse_rational("0/0").is_none());
}

#[test]
fn clipped_pixels_are_counted_and_exposure_averaged() {
    let frame = [0u8, 0, 255, 128];
    let (black, white, brightness) = grey_stats(&frame).expect("stats");
    assert!((black - 0.5).abs() < 0.001);
    assert!((white - 0.25).abs() < 0.001);
    assert!((brightness - (383.0 / 4.0 / 255.0)).abs() < 0.001);
}

#[test]
fn no_frames_gives_nothing_rather_than_a_black_frame() {
    // An empty read would otherwise average to zero, which is "the whole clip
    // is pure black" — a confident claim about a clip nobody looked at.
    assert!(grey_stats(&[]).is_none());
}

#[test]
fn the_rumble_threshold_sits_between_the_two_measured_cases() {
    // Real figures from the calibration in `measure.rs`: a 220Hz tone alone,
    // and the same tone mixed with a 40Hz one.
    assert!(!has_rumble(-21.1, -64.9), "a clean tone is not rumble");
    assert!(has_rumble(-24.1, -28.2), "40Hz under the signal is rumble");
}

#[test]
fn the_sibilance_threshold_sits_between_the_two_measured_cases() {
    assert!(!has_harsh_s(-21.1, -91.0), "nothing in the band is not harsh");
    assert!(has_harsh_s(-26.1, -37.0), "a 7kHz tone in the band is harsh");
}

#[test]
fn what_is_not_measured_is_named_with_a_reason() {
    // A clean report has to be distinguishable from a report nobody ran.
    let gaps = unmeasured();
    assert!(gaps.iter().any(|(w, _)| w.contains("skin")));
    assert!(gaps.iter().any(|(w, _)| w.contains("saturation")));
    for (what, why) in gaps {
        assert!(why.split_whitespace().count() >= 5, "{what} has no real reason");
    }
}

// --- the news blackout is an interval question, not an instant one ----------

#[test]
fn a_bar_that_contains_a_release_is_not_clean() {
    // The failure this pins, found in a feature I had just written: the first
    // version asked whether the bar's OPENING INSTANT fell inside a blackout
    // window. A daily bar is 24 hours and a window is 90 minutes, so the
    // answer was "never" — `in_blackout` came back 0.00% of bars across every
    // Daily file, reporting a clean bar that contained the release.
    //
    // `market/README.md` had already written this failure down in its own
    // words: "the H4 bar closing 16:00 CONTAINS the 12:30 payrolls release; a
    // close-only check calls it clean." Same error, different end of the bar.
    //
    // This is the arithmetic, stated directly rather than through the feature
    // table, so it cannot drift back.
    // Asked through the crate's own `Tf::spans` -- the same function
    // `standdown` uses, so the test and the production reader cannot drift to
    // different overlap arithmetic.
    let bar_open = 1_700_000_000_000i64;

    // A release six hours into a Daily bar, with a ±90 minute window around it.
    let release = bar_open + 6 * 3_600_000;
    let window = (release - 30 * 60_000, release + 60 * 60_000);

    let instant_says = bar_open >= window.0 && bar_open <= window.1;
    let interval_says = Tf::D.spans(bar_open, window.0, window.1);

    assert!(!instant_says, "the old reading: the bar opens clean");
    assert!(interval_says, "the bar plainly contains the release");
}

#[test]
fn a_bar_entirely_clear_of_a_release_stays_clear() {
    // The other direction, so the fix is not simply "always true". An H1 bar
    // that closes long before the window opens does not span it.
    let bar_open = 1_700_000_000_000i64;
    let hour = 3_600_000i64;
    let release = bar_open + 10 * hour;
    let window = (release - 30 * 60_000, release + 60 * 60_000);
    assert!(!Tf::H1.spans(bar_open, window.0, window.1));
}
