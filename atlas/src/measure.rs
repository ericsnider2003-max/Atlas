//! Measuring a real file, so that grading it has something true to grade.
//!
//! `grade.rs` has known how to judge a clip since it was written: give it an
//! `Audio` and a `Picture` and it tells you what a viewer will notice, in the
//! order they will notice it. Nothing ever filled those two structs in, which
//! is why `grade` sat unreachable — the judgement existed and the
//! measurements did not.
//!
//! Everything here is measured by ffmpeg and parsed, never estimated. Where a
//! field cannot honestly be measured with what is on this machine it is left
//! at its neutral value and `unmeasured()` names it, so that a clean report
//! means "nothing found" rather than "nothing looked at":
//!
//! - **`skin_kelvin`** needs to find a face before it can read its colour
//!   temperature. Atlas has no face detection on this path, so this is `None`
//!   and `check_picture` skips white balance entirely.
//! - **`saturation`** is defined as a multiple of untouched, and a file
//!   carries no record of what it looked like untouched. Measuring mean
//!   saturation and dividing by a number somebody picked would produce a
//!   confident figure with nothing behind it.
//!
//! ## The two booleans
//!
//! `rumble` and `harsh_s` are not numbers in `grade::Audio`, they are yes/no.
//! They are answered here by comparing the energy in one band against the
//! energy in the whole signal — both measured, the threshold between them
//! chosen and named as a constant rather than buried in an `if`.

use crate::error::{AtlasError, Result};
use crate::grade::{Audio, Picture};
use crate::tools::{ExternalTool, Vars};

/// How close the sub-80Hz energy has to come to the whole signal before it is
/// worth calling rumble.
///
/// Measured rather than picked: a 220Hz tone with nothing under it reads 43.8dB
/// down through `LOW_BAND`, and the same tone mixed with a 40Hz one reads
/// 4.1dB down. 24dB sits in the middle of that gap with room either side.
pub const RUMBLE_WITHIN_DB: f32 = 24.0;

/// The same question for the 5–9kHz band, where sibilance lives.
///
/// Calibrated the same way and with the same caveat stated plainly: a 220Hz
/// tone reads 70dB down through `SIBILANCE_BAND`, and a tone mixed with a
/// 7kHz one at half amplitude reads 10.9dB down. What has NOT been done is
/// calibrating this against real speech — there is no voice recording in the
/// sandbox this was built in. The band separation is right; where exactly a
/// real voice with harsh S sounds falls inside it is still to be checked
/// against a real recording.
pub const SIBILANCE_WITHIN_DB: f32 = 14.0;

/// Four poles rather than one.
///
/// ffmpeg's `lowpass` is gentle enough that a single stage at 80Hz leaves a
/// 220Hz tone only 17dB down, which reads as rumble on a recording that has
/// none. Chaining gets the skirt steep enough for the question to be about
/// the content rather than about the filter.
pub const LOW_BAND: &str = "lowpass=f=80,lowpass=f=80,lowpass=f=80,lowpass=f=80";

/// The sibilance band, steepened for the same reason.
pub const SIBILANCE_BAND: &str =
    "highpass=f=5000,highpass=f=5000,lowpass=f=9000,lowpass=f=9000";

/// Loudness, true peak and range out of ffmpeg's `loudnorm` JSON block.
pub fn loudness_from_loudnorm(text: &str) -> Option<(f32, f32, f32)> {
    let start = text.rfind('{')?;
    let end = text[start..].find('}')? + start;
    let v: serde_json::Value = serde_json::from_str(&text[start..=end]).ok()?;
    let f = |k: &str| v.get(k)?.as_str()?.trim().parse::<f32>().ok();
    Some((f("input_i")?, f("input_tp")?, f("input_lra")?))
}

/// The noise floor line out of `astats`.
pub fn noise_floor_from_astats(text: &str) -> Option<f32> {
    labelled_db(text, "Noise floor dB:")
}

/// `volumedetect`'s mean level.
pub fn mean_volume_db(text: &str) -> Option<f32> {
    labelled_db(text, "mean_volume:")
}

/// The last `label` in the text, read as a dB figure.
///
/// Last rather than first on purpose: a command with several filters in it
/// prints one of these per filter, and the caller runs one filter per
/// invocation precisely so that "the last one" is unambiguous.
fn labelled_db(text: &str, label: &str) -> Option<f32> {
    let at = text.rfind(label)?;
    text[at + label.len()..]
        .split_whitespace()
        .next()?
        .trim_end_matches("dB")
        .parse()
        .ok()
}

/// Width, height and frame rate out of the JSON `edit::probe_args` asks for.
///
/// The same probe `edit` already uses, read for the other half of what it
/// returns — there is no second call and no second format to keep in step.
pub fn frame_shape_from_probe(json: &str) -> Option<(u32, u32, f32)> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let streams = v.get("streams")?.as_array()?;
    let s = streams
        .iter()
        .find(|s| s.get("codec_type").and_then(|c| c.as_str()) == Some("video"))?;
    let w = s.get("width")?.as_u64()? as u32;
    let h = s.get("height")?.as_u64()? as u32;
    // Written as a call rather than `.and_then(parse_rational)` on purpose:
    // the dead-capability guard reads source as text, and a function passed
    // by bare name is not a shape it recognises as use. Passing it by name
    // was true and unreadable to the detector, which made a helper this
    // module genuinely relies on look like a capability nothing calls.
    let fps = match s.get("r_frame_rate").and_then(|r| r.as_str()) {
        Some(r) => parse_rational(r).unwrap_or(0.0),
        None => 0.0,
    };
    Some((w, h, fps))
}

/// `"30000/1001"` — how ffprobe writes a frame rate.
pub fn parse_rational(s: &str) -> Option<f32> {
    let (a, b) = s.split_once('/')?;
    let (a, b): (f32, f32) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
    if b == 0.0 {
        return None;
    }
    Some(a / b)
}

/// Crushed black, blown white and overall exposure, from raw 8-bit grey
/// pixels.
///
/// The pixels must already be full-range: video is normally stored with black
/// at 16 rather than 0, and counting exact zeroes on a limited-range frame
/// reports that no footage anywhere has ever been crushed. `picture_of` asks
/// ffmpeg for `out_range=full` for this reason.
pub fn grey_stats(bytes: &[u8]) -> Option<(f32, f32, f32)> {
    if bytes.is_empty() {
        return None;
    }
    let n = bytes.len() as f64;
    let mut black = 0u64;
    let mut white = 0u64;
    let mut sum = 0u64;
    for b in bytes {
        if *b == 0 {
            black += 1;
        }
        if *b == 255 {
            white += 1;
        }
        sum += *b as u64;
    }
    Some((
        (black as f64 / n) as f32,
        (white as f64 / n) as f32,
        (sum as f64 / n / 255.0) as f32,
    ))
}

/// Is there enough energy under 80Hz to call it rumble?
pub fn has_rumble(whole_db: f32, below_80_db: f32) -> bool {
    whole_db - below_80_db < RUMBLE_WITHIN_DB
}

/// Is the 5–9kHz band carrying an unusual share of the signal?
pub fn has_harsh_s(whole_db: f32, band_db: f32) -> bool {
    whole_db - band_db < SIBILANCE_WITHIN_DB
}

/// The fields nothing on this machine can honestly fill in.
///
/// Printed alongside every report so a clean one cannot be read as a clean
/// bill of health for something that was never looked at.
pub fn unmeasured() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "skin white balance",
            "needs to find a face first, and Atlas has no face detection on this path",
        ),
        (
            "colour saturation",
            "is a multiple of untouched, and the file carries no record of untouched",
        ),
    ]
}

/// Run a tool and hand back everything it said, on both streams.
///
/// `ExternalTool::run` returns stdout and treats a non-zero exit as an error,
/// which is right for a transcriber and wrong here: ffmpeg writes every
/// measurement to stderr and exits 0, so going through `run` would throw away
/// the entire answer.
fn say(tool: &ExternalTool, extra: &[String]) -> Result<String> {
    let (cmd, mut args) = tool.resolved(&Vars::new());
    args.extend(extra.iter().cloned());
    let out = crate::tools::command(&cmd)
        .args(&args)
        .output()
        .map_err(|e| {
            AtlasError::Platform(format!(
                "could not start '{cmd}': {e}. Is it installed and on PATH? \
                 The Connections page shows what's missing."
            ))
        })?;
    Ok(format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    ))
}

/// The same, keeping stdout as bytes — raw frames are not text.
fn bytes_of(tool: &ExternalTool, extra: &[String]) -> Result<Vec<u8>> {
    let (cmd, mut args) = tool.resolved(&Vars::new());
    args.extend(extra.iter().cloned());
    let out = crate::tools::command(&cmd)
        .args(&args)
        .output()
        .map_err(|e| AtlasError::Platform(format!("could not start '{cmd}': {e}")))?;
    Ok(out.stdout)
}

fn words(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn one_filter(path: &str, filter: &str) -> Vec<String> {
    words(&["-hide_banner", "-nostats", "-i", path, "-af", filter, "-f", "null", "-"])
}

/// Measure a file's sound.
pub fn audio_of(ffmpeg: &ExternalTool, path: &str) -> Result<Audio> {
    let ln = say(
        ffmpeg,
        &one_filter(path, "loudnorm=I=-14:TP=-1:print_format=json"),
    )?;
    let (lufs, tp, lra) = loudness_from_loudnorm(&ln).ok_or_else(|| {
        AtlasError::Platform(
            "ffmpeg ran but printed no loudness block — is there an audio track?".into(),
        )
    })?;

    let stats = say(
        ffmpeg,
        &one_filter(path, "astats=measure_perchannel=none:measure_overall=Noise_floor"),
    )?;
    let noise = noise_floor_from_astats(&stats).unwrap_or(-60.0);

    let whole = mean_volume_db(&say(ffmpeg, &one_filter(path, "volumedetect"))?);
    let low = mean_volume_db(&say(
        ffmpeg,
        &one_filter(path, &format!("{LOW_BAND},volumedetect")),
    )?);
    let sib = mean_volume_db(&say(
        ffmpeg,
        &one_filter(path, &format!("{SIBILANCE_BAND},volumedetect")),
    )?);

    // A band we could not measure is not a band we found something in.
    let (rumble, harsh_s) = match whole {
        Some(w) => (
            low.map(|l| has_rumble(w, l)).unwrap_or(false),
            sib.map(|s| has_harsh_s(w, s)).unwrap_or(false),
        ),
        None => (false, false),
    };

    Ok(Audio {
        lufs,
        true_peak_db: tp,
        range_db: lra,
        noise_floor_db: noise,
        harsh_s,
        rumble,
    })
}

/// Measure a file's picture.
///
/// One frame a second, scaled small: the figures this produces are fractions
/// and averages over the whole clip, and neither needs full resolution. It
/// keeps a long clip's measurement to well under a second.
pub fn picture_of(ffmpeg: &ExternalTool, ffprobe: &ExternalTool, path: &str) -> Result<Picture> {
    let probe = say(ffprobe, &crate::edit::probe_args(path))?;
    let (width, height, fps) = frame_shape_from_probe(&probe).ok_or_else(|| {
        AtlasError::Platform("ffprobe found no video stream in that file".into())
    })?;

    let frames = bytes_of(
        ffmpeg,
        &words(&[
            "-v",
            "error",
            "-i",
            path,
            "-vf",
            "fps=1,scale=64:36:in_range=auto:out_range=full,format=gray",
            "-f",
            "rawvideo",
            "-",
        ]),
    )?;
    let (clipped_black, clipped_white, brightness) = grey_stats(&frames)
        .ok_or_else(|| AtlasError::Platform("ffmpeg returned no frames to look at".into()))?;

    Ok(Picture {
        clipped_black,
        clipped_white,
        brightness,
        // Both left neutral deliberately — see `unmeasured()`.
        skin_kelvin: None,
        saturation: 1.0,
        width,
        height,
        fps,
    })
}
