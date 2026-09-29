//! Saying what's wrong in your own words.
//!
//! The last version needed you to speak in LUFS and Kelvin, which is
//! backwards: you can hear that something's wrong, and knowing the word for it
//! is Atlas's job, not yours.
//!
//! So "I'm not loud enough" becomes a measurement, a check, and a fix. And
//! when what you said could mean two different things — "the music's too
//! loud" might be the music or might be your voice being too quiet — Atlas
//! measures rather than guessing, because those need opposite fixes.

use serde::{Deserialize, Serialize};

/// What you said, turned into something checkable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reading {
    /// What Atlas will measure.
    pub measure: &'static str,
    /// What "right" looks like.
    pub target: &'static str,
    /// What it'll do about it.
    pub fix: &'static str,
    /// The other thing you might have meant.
    pub or_maybe: Option<&'static str>,
}

/// Turn a complaint into a measurement.
pub fn understand(said: &str) -> Option<Reading> {
    let t = said.to_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| t.contains(w));

    // Voice too quiet — the most common one, and it has two causes.
    if has(&["not loud enough", "too quiet", "can't hear me", "cant hear me", "sounds quiet",
             "hard to hear"]) {
        return Some(Reading {
            measure: "your dialogue loudness against the music, and against what the platform normalises to",
            target: "voice at -14 LUFS, music about 12dB below it",
            fix: "raise the voice and duck the music under it",
            or_maybe: Some("the music being too loud rather than you being quiet — I'll measure both"),
        });
    }

    if has(&["music is too loud", "music too loud", "backing track", "music drowns"]) {
        return Some(Reading {
            measure: "the gap between your voice and the music",
            target: "music 12dB under the voice, ducking to 18dB while you speak",
            fix: "sidechain the music to your voice so it drops when you talk",
            or_maybe: Some("your voice being quiet, which sounds the same and needs the opposite fix"),
        });
    }

    if has(&["music is too quiet", "music too quiet", "can't hear the music", "cant hear the music"]) {
        return Some(Reading {
            measure: "the music level in the gaps where you aren't talking",
            target: "music comes up to about 6dB under the voice between lines",
            fix: "lift the music floor and let it breathe in the pauses",
            or_maybe: None,
        });
    }

    if has(&["sounds muffled", "muddy", "boxy", "sounds thin", "tinny"]) {
        return Some(Reading {
            measure: "the balance of low and high frequencies in your voice",
            target: "clean under 100Hz, a gentle lift around 3kHz for clarity",
            fix: "high-pass the rumble and lift the presence range",
            or_maybe: Some("the room rather than the mic — I'll say which"),
        });
    }

    if has(&["echo", "hollow", "sounds like a room", "reverb", "bathroom"]) {
        return Some(Reading {
            measure: "reflections after your voice stops",
            target: "under 300ms of tail",
            fix: "reduce it, but honestly this is a room problem — soft things near the mic help more",
            or_maybe: None,
        });
    }

    if has(&["harsh", "sharp s", "hissy", "sibilant", "spitty"]) {
        return Some(Reading {
            measure: "energy around 6 to 8kHz",
            target: "no peaks more than 4dB above the surrounding range",
            fix: "de-ess it",
            or_maybe: None,
        });
    }

    if has(&["background noise", "hum", "buzz", "fan", "noisy"]) {
        return Some(Reading {
            measure: "the noise floor between your words",
            target: "below -50dB",
            fix: "reduce it — and a closer mic does more than any amount of processing",
            or_maybe: None,
        });
    }

    // Picture.
    if has(&["too dark", "can't see me", "cant see me", "underexposed", "dim"]) {
        return Some(Reading {
            measure: "average brightness and how much of the frame is pure black",
            target: "around 50% average, nothing crushed to zero",
            fix: "lift the exposure and the shadows",
            or_maybe: Some("a light behind you rather than in front, which no edit fixes"),
        });
    }

    if has(&["washed out", "flat", "looks dull", "no contrast", "grey"]) {
        return Some(Reading {
            measure: "the spread between your darkest and brightest points",
            target: "using most of the range without clipping either end",
            fix: "add contrast through the curve rather than by crushing the blacks",
            or_maybe: None,
        });
    }

    if has(&["too orange", "too warm", "too blue", "too cold", "colour looks", "color looks",
             "skin looks"]) {
        return Some(Reading {
            measure: "the colour temperature of your skin tones",
            target: "between 4800 and 6200K",
            fix: "shift the white balance until skin sits naturally",
            or_maybe: None,
        });
    }

    if has(&["too much colour", "too much color", "oversaturated", "looks fake", "cartoonish"]) {
        return Some(Reading {
            measure: "overall saturation",
            target: "about 1.1x, no more",
            fix: "pull it back — saturation reads as amateur faster than anything else",
            or_maybe: None,
        });
    }

    if has(&["blurry", "soft", "out of focus", "not sharp"]) {
        return Some(Reading {
            measure: "edge sharpness across the frame",
            target: "your face the sharpest thing in it",
            fix: "some of this can be sharpened; if the focus missed, it can't",
            or_maybe: Some("motion blur from a low shutter speed, which is a camera setting"),
        });
    }

    if has(&["jerky", "stuttery", "choppy", "not smooth"]) {
        return Some(Reading {
            measure: "frame rate and whether frames are dropped",
            target: "a steady 30 or 60",
            fix: "nothing in the edit fixes this — it's the recording",
            or_maybe: None,
        });
    }

    if has(&["too long", "drags", "boring", "loses me"]) {
        return Some(Reading {
            measure: "where the substance starts, and how much dead air there is",
            target: "the point inside 3 seconds, no gap over 400ms",
            fix: "cut the opening and the silences — usually takes 15 to 20% off",
            or_maybe: None,
        });
    }

    None
}

/// What Atlas says back. It repeats what it thinks you meant, because getting
/// that wrong wastes the whole edit.
pub fn confirm(r: &Reading) -> String {
    let mut s = format!("I'll check {}. Should be {}.", r.measure, r.target);
    if let Some(other) = r.or_maybe {
        s.push_str(&format!(" It might be {other}."));
    }
    s
}

/// Said in your words, once it's measured.
///
/// The number is there if you want it, at the end, rather than in the way.
pub fn result(_what_you_said: &str, measured: &str, fixed: bool) -> String {
    if fixed {
        format!("Fixed. You were right — {measured}.")
    } else {
        format!("Measured it and it looks fine, actually. {measured}. Want me to change it anyway?")
    }
}

/// Nothing matched.
pub fn didnt_understand(said: &str) -> String {
    format!(
        "I'm not sure which part you mean by \"{}\". Is it the sound, the picture, or the pace?",
        said.trim()
    )
}
