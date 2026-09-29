//! Making it look and sound like someone made it on purpose.
//!
//! The gap between amateur and professional in short-form is almost never the
//! camera. It's four things, all fixable after the fact and all measurable:
//! loudness, dialogue clarity, colour that doesn't clip, and text that isn't
//! under the interface.
//!
//! Everything here is a number, not a taste. "Make it look better" is not
//! actionable; "your dialogue is at -22 LUFS and the platform will normalise
//! everything else up to -14, so you'll be the quiet one in the feed" is.

use serde::{Deserialize, Serialize};

/// What the platforms actually do to your audio.
///
/// They normalise to a target. Delivering quieter doesn't make you safe, it
/// makes you quiet — everything around you gets raised and you don't.
pub const TARGET_LUFS: f32 = -14.0;
/// Above this and you risk clipping after their re-encode.
pub const MAX_TRUE_PEAK_DB: f32 = -1.0;

/// What `atlas video grade` measures against.
///
/// `config/tools.yaml` has carried a `grade:` block since this module was
/// written — `target_lufs: -14`, `max_true_peak_db: -1`, `preset: clean` —
/// and until 18 Sep 2026 **no such type existed**, so serde dropped the whole
/// section on the floor. `config::NO_FIELD_TO_LAND_IN` recorded it as having
/// nowhere to land. The numbers in the file matched the constants here
/// exactly, which is why nobody noticed: the advice was right and the file
/// had nothing to do with it.
///
/// The constants stay and are the defaults. They are what the platforms do,
/// not a preference — but a person mastering for somewhere with a different
/// target has no other way to say so.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct GradeConfig {
    /// The loudness the platform normalises to.
    pub target_lufs: f32,
    /// The true-peak ceiling to stay under before their re-encode.
    pub max_true_peak_db: f32,
    /// The look `atlas video grade` offers as a starting point, by name from
    /// [`presets`]. A name that is not one of them is said rather than
    /// silently ignored.
    pub preset: String,
}

impl Default for GradeConfig {
    fn default() -> Self {
        GradeConfig {
            target_lufs: TARGET_LUFS,
            max_true_peak_db: MAX_TRUE_PEAK_DB,
            preset: "clean".into(),
        }
    }
}

/// The preset you named, if it is one.
///
/// `None` says the name matched nothing, which the caller says out loud —
/// a misspelled preset that silently becomes the default is a setting that
/// does nothing while looking like it worked.
pub fn preset_named(name: &str) -> Option<Preset> {
    presets().into_iter().find(|p| p.name.eq_ignore_ascii_case(name.trim()))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Audio {
    /// Integrated loudness.
    pub lufs: f32,
    /// Highest true peak.
    pub true_peak_db: f32,
    /// Difference between loudest and quietest, in dB. Very high means the
    /// quiet parts vanish on a phone speaker.
    pub range_db: f32,
    /// Background noise floor.
    pub noise_floor_db: f32,
    /// Sibilance — harsh S sounds.
    pub harsh_s: bool,
    /// Low rumble under 80Hz, which is almost always room or handling noise.
    pub rumble: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Picture {
    /// Fraction of pixels at pure black. Above a little means crushed shadows.
    pub clipped_black: f32,
    /// Fraction at pure white.
    pub clipped_white: f32,
    /// Overall exposure, 0 to 1.
    pub brightness: f32,
    /// Colour temperature of the skin tones, in Kelvin.
    pub skin_kelvin: Option<u32>,
    /// Saturation, where 1.0 is untouched.
    pub saturation: f32,
    /// Frame size.
    pub width: u32,
    pub height: u32,
    pub fps: f32,
}

/// Something to fix, with the number behind it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub what: String,
    /// The measurement.
    pub because: String,
    /// The change, as something that can actually be applied.
    pub fix: String,
    /// Fixable after the fact, or does it need re-recording?
    pub fixable_now: bool,
    /// Bigger number, more visible to a viewer.
    pub matters: f32,
}

pub fn check_audio(a: &Audio, cfg: &GradeConfig) -> Vec<Note> {
    let mut out = Vec::new();
    let target = cfg.target_lufs;
    let ceiling = cfg.max_true_peak_db;

    let off = target - a.lufs;
    if off.abs() > 1.5 {
        out.push(Note {
            what: if off > 0.0 { "it's quiet".into() } else { "it's too loud".into() },
            because: format!(
                "{:.1} LUFS against the {target} the platforms normalise to — everything \
                 around you gets raised {:.0}dB and you don't",
                a.lufs, off.abs()
            ),
            fix: format!("loudnorm to {target} LUFS, true peak {ceiling}dB"),
            fixable_now: true,
            matters: 0.95,
        });
    }

    if a.true_peak_db > ceiling {
        out.push(Note {
            what: "it'll clip after their re-encode".into(),
            because: format!("true peak is {:.1}dB, and encoding pushes it higher", a.true_peak_db),
            fix: "limit to -1dBTP before export".into(),
            fixable_now: true,
            matters: 0.8,
        });
    }

    // On a phone speaker in a noisy room, wide range means the quiet half is
    // simply inaudible.
    if a.range_db > 14.0 {
        out.push(Note {
            what: "the quiet parts will vanish on a phone".into(),
            because: format!("{:.0}dB between your loudest and quietest", a.range_db),
            fix: "gentle compression, 3:1 around -18dB, then re-normalise".into(),
            fixable_now: true,
            matters: 0.7,
        });
    }

    if a.noise_floor_db > -50.0 {
        out.push(Note {
            what: "there's audible room noise".into(),
            because: format!("noise floor at {:.0}dB", a.noise_floor_db),
            fix: "noise reduction, and get the mic closer next time".into(),
            fixable_now: true,
            matters: 0.6,
        });
    }

    if a.rumble {
        out.push(Note {
            what: "there's low rumble".into(),
            because: "energy under 80Hz, which is room or handling noise rather than voice".into(),
            fix: "high-pass at 80Hz — it takes nothing out of a voice".into(),
            fixable_now: true,
            matters: 0.5,
        });
    }

    if a.harsh_s {
        out.push(Note {
            what: "the S sounds are harsh".into(),
            because: "sibilance around 6–8kHz".into(),
            fix: "de-ess, 4dB around 7kHz".into(),
            fixable_now: true,
            matters: 0.45,
        });
    }

    out.sort_by(|a, b| b.matters.partial_cmp(&a.matters).unwrap_or(std::cmp::Ordering::Equal));
    out
}

pub fn check_picture(p: &Picture) -> Vec<Note> {
    let mut out = Vec::new();

    // Crushed blacks are the single most common thing that makes footage look
    // cheap, and it's the one people do to themselves reaching for "contrast".
    if p.clipped_black > 0.02 {
        out.push(Note {
            what: "the shadows are crushed".into(),
            because: format!("{:.0}% of the frame is pure black, with no detail in it", p.clipped_black * 100.0),
            fix: "lift the black point until nothing is at zero. Contrast comes from the curve, not the floor".into(),
            fixable_now: true,
            matters: 0.9,
        });
    }
    if p.clipped_white > 0.01 {
        out.push(Note {
            what: "the highlights are blown".into(),
            because: format!("{:.0}% of the frame is pure white", p.clipped_white * 100.0),
            fix: "pull the highlights down — but if it's a window behind you, that needs re-shooting".into(),
            fixable_now: p.clipped_white < 0.06,
            matters: 0.75,
        });
    }
    if p.brightness < 0.3 {
        out.push(Note {
            what: "it's underexposed".into(),
            because: format!("average brightness {:.0}%, and phones dim in bright rooms", p.brightness * 100.0),
            fix: "lift exposure — and add a light in front of you, not behind".into(),
            fixable_now: true,
            matters: 0.8,
        });
    }

    // Skin tone is what the eye judges everything else by.
    if let Some(k) = p.skin_kelvin {
        if !(4800..=6200).contains(&k) {
            let warm = k < 4800;
            out.push(Note {
                what: if warm { "the skin looks orange".into() } else { "the skin looks blue".into() },
                because: format!("white balance around {k}K"),
                fix: format!(
                    "shift white balance {} until skin sits naturally",
                    if warm { "cooler" } else { "warmer" }
                ),
                fixable_now: true,
                matters: 0.85,
            });
        }
    }

    if p.saturation > 1.3 {
        out.push(Note {
            what: "the colour is overcooked".into(),
            because: format!("saturation at {:.1}x", p.saturation),
            fix: "back it off to about 1.1. Saturation reads as amateur faster than anything else".into(),
            fixable_now: true,
            matters: 0.65,
        });
    }

    if p.width * 16 != p.height * 9 {
        out.push(Note {
            what: "it isn't 9:16".into(),
            because: format!("{}x{}", p.width, p.height),
            fix: "crop to 1080x1920 rather than letterboxing — bars read as reposted".into(),
            fixable_now: true,
            matters: 0.9,
        });
    }
    if p.fps < 29.0 {
        out.push(Note {
            what: "the frame rate is low".into(),
            because: format!("{:.0}fps", p.fps),
            fix: "shoot at 30 or 60. Below 30 reads as a screen recording".into(),
            fixable_now: false,
            matters: 0.6,
        });
    }

    out.sort_by(|a, b| b.matters.partial_cmp(&a.matters).unwrap_or(std::cmp::Ordering::Equal));
    out
}

/// Where the interface covers your frame.
///
/// Text placed here is text nobody reads, and it's the most common avoidable
/// mistake in vertical video.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct SafeArea {
    pub top_pct: f32,
    pub bottom_pct: f32,
    pub right_pct: f32,
    pub left_pct: f32,
}

impl SafeArea {
    /// Roughly what the apps cover. Conservative on purpose — being 3% clear
    /// costs nothing and being 3% under costs the whole caption.
    pub fn typical() -> SafeArea {
        SafeArea { top_pct: 12.0, bottom_pct: 22.0, right_pct: 16.0, left_pct: 4.0 }
    }

    /// Is this text position safe?
    pub fn clears(&self, x_pct: f32, y_pct: f32) -> bool {
        y_pct > self.top_pct
            && y_pct < 100.0 - self.bottom_pct
            && x_pct > self.left_pct
            && x_pct < 100.0 - self.right_pct
    }

    /// Where captions should sit.
    pub fn caption_band(&self) -> (f32, f32) {
        (100.0 - self.bottom_pct - 18.0, 100.0 - self.bottom_pct - 4.0)
    }
}

/// A look you can apply and then adjust, rather than starting from nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Preset {
    pub name: &'static str,
    pub what_it_is: &'static str,
    /// When it suits.
    pub for_what: &'static str,
    pub lift: f32,
    pub contrast: f32,
    pub saturation: f32,
    pub warmth: i32,
    /// Slight vignette pulls the eye to the middle.
    pub vignette: f32,
}

/// Starting points. Deliberately few and deliberately restrained — the ones
/// that look like a filter are the ones that date.
pub fn presets() -> Vec<Preset> {
    vec![
        Preset {
            name: "clean",
            what_it_is: "neutral, slightly lifted, true colour",
            for_what: "talking to camera, anything you want to age well",
            lift: 0.03, contrast: 1.05, saturation: 1.05, warmth: 0, vignette: 0.0,
        },
        Preset {
            name: "warm room",
            what_it_is: "a touch warm, soft shadows",
            for_what: "indoors, evening, anything conversational",
            lift: 0.05, contrast: 1.02, saturation: 1.08, warmth: 180, vignette: 0.08,
        },
        Preset {
            name: "hard light",
            what_it_is: "more contrast, cooler, colour held back",
            for_what: "screens, product, anything technical",
            lift: 0.01, contrast: 1.18, saturation: 0.95, warmth: -120, vignette: 0.05,
        },
        Preset {
            name: "film",
            what_it_is: "lifted blacks, gentle roll-off, muted",
            for_what: "story, slower pieces",
            lift: 0.07, contrast: 0.96, saturation: 0.9, warmth: 90, vignette: 0.12,
        },
    ]
}

/// ffmpeg for a preset. Nothing here needs anything installed beyond ffmpeg.
pub fn preset_filter(p: &Preset) -> String {
    format!(
        "eq=brightness={:.3}:contrast={:.3}:saturation={:.3},colortemperature=temperature={}",
        p.lift,
        p.contrast,
        p.saturation,
        6500 + p.warmth
    )
}

/// The audio chain, in order. Order matters — normalising before compressing
/// undoes the normalising.
pub fn audio_chain(a: &Audio) -> Vec<String> {
    let mut chain = Vec::new();
    if a.rumble {
        chain.push("highpass=f=80".to_string());
    }
    if a.noise_floor_db > -50.0 {
        chain.push("afftdn=nf=-25".to_string());
    }
    if a.harsh_s {
        chain.push("deesser=i=0.4".to_string());
    }
    if a.range_db > 14.0 {
        chain.push("acompressor=threshold=-18dB:ratio=3:attack=5:release=120".to_string());
    }
    // Always last: anything after it changes the loudness you just set.
    chain.push(format!("loudnorm=I={TARGET_LUFS}:TP={MAX_TRUE_PEAK_DB}:LRA=9"));
    chain
}

/// What Atlas says. The thing a viewer would notice first.
pub fn spoken(audio: &[Note], picture: &[Note]) -> String {
    let mut all: Vec<&Note> = audio.iter().chain(picture.iter()).collect();
    all.sort_by(|a, b| b.matters.partial_cmp(&a.matters).unwrap_or(std::cmp::Ordering::Equal));

    match all.split_first() {
        None => "Sounds and looks fine. Nothing worth changing.".into(),
        Some((first, rest)) => {
            let mut s = format!("{} — {}. {}", first.what, first.because, first.fix);
            let unfixable = rest.iter().filter(|n| !n.fixable_now).count();
            if !rest.is_empty() {
                s.push_str(&format!(" {} other thing{}.", rest.len(), if rest.len() == 1 { "" } else { "s" }));
            }
            if unfixable > 0 || !first.fixable_now {
                s.push_str(" Some of that needs the next recording, not this one.");
            }
            s
        }
    }
}

/// What to change about how you record, rather than about this file.
///
/// Worth separating: fixing it in the edit every time is a tax you pay
/// forever, and most of these are a one-off change to the room.
pub fn recording_advice(notes: &[Note]) -> Vec<String> {
    notes
        .iter()
        .filter(|n| !n.fixable_now || n.fix.contains("next time") || n.fix.contains("re-shoot"))
        .map(|n| n.fix.clone())
        .collect()
}
