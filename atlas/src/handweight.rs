//! How heavy hand tracking is allowed to be on the machine it's on.
//!
//! ## Why this exists (2 Oct 2026)
//!
//! Eric, on his laptop: "may need a lighter model for gestures" -- the fans
//! spin while Atlas watches his hands. Looking at what actually ran showed
//! the model was not the main problem:
//!
//! - **The models are already the light ones.** `hand_presence.onnx` and
//!   `hand_landmarks.onnx` are MediaPipe's *lite* palm finder and *lite*
//!   landmark reader (960,126 and 1,011,716 weights -- the same counts as
//!   Google's `palm_detection_lite.tflite` and `hand_landmark_lite.tflite`;
//!   the "full" landmark reader is 2.7 million). The int8 copies OpenCV
//!   publishes are smaller on disk but ran 3 to 6 times *slower* on a
//!   processor when measured (2 Oct 2026, ONNX Runtime, one thread), and
//!   OpenCV's own notes say the int8 landmark reader "may produce invalid
//!   results". So there is no lighter file to swap in.
//! - **The engine was the slow part.** Both models ran in `tract`, which
//!   took about 116 ms to find a hand and 47 ms to read one on one
//!   processor thread; ONNX Runtime -- already on the machine, it comes with
//!   the Kokoro voice -- took about 16 and 5 for the same files. The hands
//!   now use it (and the NPU where there is one: `handloop::Reader`).
//! - **Both models ran on every look,** even while a hand was already being
//!   followed. The palm finder is the expensive one (about three times the
//!   landmark reader), and MediaPipe itself only runs it when it has lost
//!   the hand: the hand found last time says where to read this time.
//! - **It looked just as often at an empty room** as at a hand, forever.
//!   Nothing switched it off when the hand went away, and nothing checked
//!   whether anything in the picture had even changed.
//! - **Every machine got the same settings** -- a 640 by 480 picture and up
//!   to twenty looks a second -- whether it was a desktop on mains or a
//!   four-core laptop on battery.
//!
//! This file is the arithmetic for all four: which plan suits this machine,
//! whether anything moved, whether to find a hand or follow the one already
//! found, how long to wait, and when to stop. All of it pure, so all of it
//! is tested with numbers rather than a camera.

use crate::handshape::Landmarks;
use crate::handtrack::PaceConfig;
use crate::vision::Patch;
use serde::{Deserialize, Serialize};

/// How heavy hand tracking may be. A setting, so a machine the automatic
/// choice gets wrong can be told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Weight {
    /// Decided from this machine: its cores, its memory, whether it is on
    /// battery, and what reading a hand actually cost here last time.
    #[default]
    Auto,
    /// A smaller picture and fewer looks a second, whatever the machine.
    Light,
    /// The full picture and pace, whatever the machine.
    Full,
}

impl Weight {
    /// The words the setting accepts, in the order the hub shows them.
    pub const WORDS: [&'static str; 3] = ["auto", "light", "full"];

    pub fn word(self) -> &'static str {
        match self {
            Weight::Auto => "auto",
            Weight::Light => "light",
            Weight::Full => "full",
        }
    }
}

/// The hand-tracking settings (`hands:` in tools.yaml).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct HandsConfig {
    /// `auto`, `light` or `full`.
    pub weight: Weight,
    /// Read hands on the NPU when this machine has an Intel one and its
    /// engine is downloaded (the same engine search and voice ID use).
    pub npu: bool,
    /// Stop watching after this many seconds with no hand in view. 0 means
    /// never -- the camera stays on until you say stop.
    pub stop_after_quiet_secs: u64,
}

impl Default for HandsConfig {
    fn default() -> Self {
        HandsConfig {
            weight: Weight::Auto,
            npu: true,
            // Two minutes, not `gaze::STEERING_STOPS_AFTER`'s thirty seconds:
            // that is how long steering waits for a hand in the room-watching
            // path, and putting your hand down to read something for half a
            // minute shouldn't end the mode you asked for. Two minutes of an
            // empty picture is long enough to mean you've moved on.
            stop_after_quiet_secs: 120,
        }
    }
}

/// What about this machine decides the plan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Machine {
    /// Logical processors.
    pub cores: usize,
    /// Total memory in GB; 0 when it couldn't be read.
    pub ram_gb: f32,
    pub on_battery: bool,
}

impl Machine {
    /// This computer, read now.
    pub fn this_one() -> Machine {
        let r = crate::health::read_machine();
        Machine {
            cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4),
            ram_gb: r.ram_total_gb,
            on_battery: r.on_battery,
        }
    }
}

/// At or under this many logical processors, a machine is treated as small.
pub const SMALL_CORES: usize = 4;

/// Under this much memory, in GB, a machine is treated as small.
pub const SMALL_RAM_GB: f32 = 8.0;

/// The picture the light plan asks the camera for. The palm finder reads
/// 192 by 192 whatever it's given, so the picture size mostly decides how
/// much ffmpeg scales and pipes -- a quarter of the bytes of 640 by 480.
pub const LIGHT_PICTURE: (usize, usize) = (320, 240);

/// Most looks a second on the light plan.
pub const LIGHT_MOST_PER_SECOND: u32 = 12;

/// Looks a second with no hand in view, full and light. Enough to notice a
/// hand coming up within a quarter or half a second; most of these looks
/// stop at the movement check and never reach a model.
pub const IDLE_PER_SECOND: (u32, u32) = (4, 2);

/// How hand tracking will run here, and why.
#[derive(Debug, Clone)]
pub struct Plan {
    pub light: bool,
    /// The picture asked of the camera.
    pub width: usize,
    pub height: usize,
    /// Pace while a hand is in view.
    pub pace: PaceConfig,
    /// Looks a second while no hand is in view.
    pub idle_per_second: u32,
    /// In plain words, for the log and for "why is it slow".
    pub why: String,
}

/// Would the full plan, at what reading a hand cost last time, fall to the
/// floor or under? Then it was never going to feel right here.
pub fn too_slow_for_full(cost_ms: u32, base: &PaceConfig) -> bool {
    if cost_ms == 0 {
        return false;
    }
    let share = base.share_of_a_core.clamp(0.01, 1.0);
    let per_second = (share * 1000.0 / cost_ms as f32) as u32;
    per_second <= base.floor_per_second
}

/// Choose the plan for this machine.
///
/// `last_cost_ms` is what one look cost the last time hand tracking ran in
/// this Atlas (`handloop::last_cost_ms`), or `None` the first time.
pub fn plan(cfg: &HandsConfig, m: &Machine, last_cost_ms: Option<u32>, base: &PaceConfig) -> Plan {
    let mut reasons: Vec<String> = Vec::new();
    let light = match cfg.weight {
        Weight::Light => {
            reasons.push("you set it to light".into());
            true
        }
        Weight::Full => {
            reasons.push("you set it to full".into());
            false
        }
        Weight::Auto => {
            if m.cores <= SMALL_CORES {
                reasons.push(format!("this machine has {} processors", m.cores));
            }
            if m.ram_gb > 0.0 && m.ram_gb < SMALL_RAM_GB {
                reasons.push(format!("this machine has {:.0} GB of memory", m.ram_gb));
            }
            if m.on_battery {
                reasons.push("it's on battery".into());
            }
            if let Some(ms) = last_cost_ms.filter(|ms| too_slow_for_full(*ms, base)) {
                reasons.push(format!("a look took {ms} ms here last time"));
            }
            !reasons.is_empty()
        }
    };
    if light {
        let want = base.want_per_second.min(LIGHT_MOST_PER_SECOND).max(1);
        Plan {
            light,
            width: LIGHT_PICTURE.0,
            height: LIGHT_PICTURE.1,
            pace: PaceConfig {
                want_per_second: want,
                floor_per_second: base.floor_per_second.min(want),
                // Half the share of a core the full plan gets.
                share_of_a_core: (base.share_of_a_core * 0.5).max(0.01),
            },
            idle_per_second: IDLE_PER_SECOND.1,
            why: format!("light, because {}", reasons.join(" and ")),
        }
    } else {
        let feed = crate::frames::Feed::default();
        Plan {
            light,
            width: feed.width,
            height: feed.height,
            pace: *base,
            idle_per_second: IDLE_PER_SECOND.0,
            why: if reasons.is_empty() { "full, because this machine has room".into() } else { format!("full, because {}", reasons.join(" and ")) },
        }
    }
}

// ---------------------------------------------------------------------------
// Did anything move?
// ---------------------------------------------------------------------------

/// The movement check's picture: a 32 by 24 grey thumbnail.
pub const GRID: (usize, usize) = (32, 24);

/// How much one thumbnail square has to change, out of 255, to count. Above
/// a webcam's own flicker in a dim room.
pub const SQUARE_STEP: u8 = 16;

/// The share of squares that must change for the picture to have moved --
/// about fifteen of the 768, roughly a hand's width coming into view.
pub const MOVED_SHARE: f32 = 0.02;

/// Even with nothing moving, look properly this often (ms) -- a hand held
/// perfectly still in view is still a hand.
pub const LOOK_ANYWAY_MS: u32 = 2000;

/// A frame shrunk to the grey thumbnail the movement check compares.
///
/// Each square is the average of a sparse sample of its pixels -- every
/// fourth row and column -- which is plenty for "did something change" and
/// costs a fraction of a millisecond on a 640 by 480 picture.
pub fn thumbnail(rgb: &[u8], w: usize, h: usize) -> Vec<u8> {
    let (gw, gh) = GRID;
    let mut out = vec![0u8; gw * gh];
    if w < gw || h < gh || rgb.len() < w * h * 3 {
        return out;
    }
    for gy in 0..gh {
        let (y0, y1) = (gy * h / gh, (gy + 1) * h / gh);
        for gx in 0..gw {
            let (x0, x1) = (gx * w / gw, (gx + 1) * w / gw);
            let (mut sum, mut n) = (0u32, 0u32);
            for y in (y0..y1).step_by(4) {
                for x in (x0..x1).step_by(4) {
                    let i = (y * w + x) * 3;
                    sum += (rgb[i] as u32 + rgb[i + 1] as u32 * 2 + rgb[i + 2] as u32) / 4;
                    n += 1;
                }
            }
            out[gy * gw + gx] = (sum / n.max(1)) as u8;
        }
    }
    out
}

/// The share of squares that changed by more than `SQUARE_STEP`. 1.0 when
/// there is nothing to compare with yet, so the first look is a real one.
pub fn moved(before: &[u8], now: &[u8]) -> f32 {
    if before.is_empty() || before.len() != now.len() {
        return 1.0;
    }
    let changed = before.iter().zip(now).filter(|(a, b)| a.abs_diff(**b) > SQUARE_STEP).count();
    changed as f32 / now.len() as f32
}

// ---------------------------------------------------------------------------
// Find, follow, or skip
// ---------------------------------------------------------------------------

/// What one look does.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Step {
    /// Run the palm finder over the whole picture, then read the hand.
    Find,
    /// Read the hand where it was last time; the palm finder is not run.
    Follow(Patch),
    /// Nothing moved and there was no hand: no model runs at all.
    Skip,
}

/// Decide what this look does.
///
/// `following` is where the hand was on the last look, if one was read;
/// `moved_share` is `moved`'s answer; `since_found_ms` is how long since the
/// palm finder last ran.
pub fn step(following: Option<Patch>, moved_share: f32, since_found_ms: u32) -> Step {
    if let Some(p) = following {
        return Step::Follow(p);
    }
    if moved_share >= MOVED_SHARE || since_found_ms >= LOOK_ANYWAY_MS {
        return Step::Find;
    }
    Step::Skip
}

/// How much bigger than the joints' own box to read the next look, on each
/// side, as a share of the hand's size: room for the hand to move between
/// looks and for the fingers to spread.
pub const FOLLOW_MARGIN: f32 = 0.25;

/// Below this, the landmark reader is not sure it's still looking at a hand,
/// and the next look goes back to finding one.
pub const STILL_A_HAND: f32 = 0.5;

/// Where to read the hand next time, from where its joints were this time:
/// square in the picture (the reader wants a square), widened by
/// `FOLLOW_MARGIN`, as fractions of the frame. `None` when the hand is too
/// small, too unsure, or off the picture.
pub fn follow_box(marks: &Landmarks, w: usize, h: usize) -> Option<Patch> {
    if marks.sure < STILL_A_HAND || w == 0 || h == 0 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in &marks.points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
        return None;
    }
    // In pixels, so "square" means square in the picture rather than in
    // fractions of a frame that is wider than it is tall.
    let (fw, fh) = (w as f32, h as f32);
    let side = ((x1 - x0) * fw).max((y1 - y0) * fh) * (1.0 + 2.0 * FOLLOW_MARGIN);
    if side < 8.0 {
        return None;
    }
    let (cx, cy) = ((x0 + x1) / 2.0 * fw, (y0 + y1) / 2.0 * fh);
    if cx < 0.0 || cy < 0.0 || cx > fw || cy > fh {
        return None;
    }
    let left = (cx - side / 2.0).max(0.0);
    let top = (cy - side / 2.0).max(0.0);
    let right = (cx + side / 2.0).min(fw);
    let bottom = (cy + side / 2.0).min(fh);
    Some(Patch::new(left / fw, top / fh, (right - left) / fw, (bottom - top) / fh))
}

// ---------------------------------------------------------------------------
// How long to wait, and when to stop
// ---------------------------------------------------------------------------

/// A hand gone for less than this (ms) is a missed look, not an empty room:
/// the pace stays up so a turned wrist doesn't make steering sluggish.
pub const QUIET_AFTER_MS: u32 = 1000;

/// Milliseconds to wait before the next look. `paced` is what
/// `handtrack::Pace::wait_ms` allows; with no hand for `QUIET_AFTER_MS` the
/// wait stretches to the idle rate.
pub fn wait_ms(paced: u32, quiet_ms: u32, idle_per_second: u32) -> u32 {
    if quiet_ms < QUIET_AFTER_MS {
        return paced;
    }
    paced.max(1000 / idle_per_second.max(1))
}

/// Has the hand been gone long enough to stop watching?
pub fn time_to_stop(quiet_ms: u32, cfg: &HandsConfig) -> bool {
    cfg.stop_after_quiet_secs > 0 && quiet_ms as u64 >= cfg.stop_after_quiet_secs.saturating_mul(1000)
}
