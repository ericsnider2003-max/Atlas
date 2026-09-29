//! Are you at the desk?
//!
//! This is worth more than it sounds, and not for the reason it first appears.
//! It is not security — a camera cannot tell you from a photograph either.
//! Its value is **timing**:
//!
//! * Foreground work waits for a gap. Presence turns "you've been quiet for
//!   20 seconds" into "you actually left", which is the difference between
//!   guessing and knowing.
//! * Atlas should not talk to an empty room.
//! * Coming back is the moment to deliver the away-brief.
//! * If someone else is at your desk, sensitive output should stay quiet.
//!
//! The camera is off by default, samples at a low rate, and the frames never
//! leave the machine.

use serde::{Deserialize, Serialize};

/// One camera observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Look {
    pub faces: usize,
    /// A face matching your enrollment was among them.
    pub you: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Presence {
    AtDesk,
    Away,
    /// Someone is there, but not you.
    Stranger,
    /// You and at least one other person.
    NotAlone,
    /// Camera off, or nothing seen yet.
    Unknown,
}

impl Presence {
    pub fn here(&self) -> bool {
        matches!(self, Presence::AtDesk | Presence::NotAlone)
    }
    /// Should Atlas keep private things to itself?
    pub fn should_be_discreet(&self) -> bool {
        matches!(self, Presence::Stranger | Presence::NotAlone)
    }
    /// Is it worth talking out loud at all?
    pub fn worth_speaking(&self) -> bool {
        matches!(self, Presence::AtDesk | Presence::NotAlone | Presence::Unknown)
    }
}

/// Should this note knock rather than disclose, given who the camera can see?
///
/// `discreet_with_strangers` shipped `true` and was read by nothing until
/// 18 Sep 2026. Two things were missing, not one: nothing consulted the
/// setting, and `Daemon::reach_you` did not look at the sensor at all — it
/// guessed presence from idle time, so a camera that had just seen an
/// unrecognised face still reported `Unknown` to the code deciding what to
/// say out loud.
///
/// Deliberately *not* a routing decision. `notify::route` sets out why
/// holding a note until the room empties is wrong — in a coffee shop it holds
/// everything forever — and that reasoning stands. What this gates is how
/// much of the note appears, which is the lever `Note::shown` already is.
///
/// `Unknown` is never discreet: a camera that is off or unread is not an
/// empty room, and treating "no idea" as "someone is there" would silence a
/// machine with no camera at all.
pub fn keep_it_to_yourself(state: Presence, cfg: &PresenceConfig) -> bool {
    cfg.enabled && cfg.discreet_with_strangers && state.should_be_discreet()
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PresenceConfig {
    pub enabled: bool,
    /// Seconds between camera samples. Low on purpose — this is a background
    /// signal, not a video feed.
    pub sample_every: u64,
    /// Consecutive empty looks before deciding you left. One missed frame is
    /// you reaching for a coffee, not you leaving.
    pub leave_after: u32,
    /// Consecutive sightings before deciding you're back.
    pub return_after: u32,
    /// Go quiet when a face that isn't yours is present.
    pub discreet_with_strangers: bool,
}

impl Default for PresenceConfig {
    fn default() -> Self {
        PresenceConfig {
            enabled: false,
            sample_every: 20,
            leave_after: 3,
            return_after: 1,
            discreet_with_strangers: true,
        }
    }
}

/// What changed, so the caller can react once rather than every sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    None,
    /// You just left. Time to run deferred work.
    Left,
    /// You just came back. Time for the away-brief.
    Returned,
    /// Someone else appeared.
    StrangerArrived,
    StrangerLeft,
}

#[derive(Debug, Clone)]
pub struct Sensor {
    pub cfg: PresenceConfig,
    pub state: Presence,
    empty_looks: u32,
    seen_looks: u32,
    last_sample: u64,
    pub left_at: Option<u64>,
}

impl Default for Sensor {
    fn default() -> Self {
        Sensor::new(PresenceConfig::default())
    }
}

impl Sensor {
    pub fn new(cfg: PresenceConfig) -> Sensor {
        Sensor {
            cfg,
            state: Presence::Unknown,
            empty_looks: 0,
            seen_looks: 0,
            last_sample: 0,
            left_at: None,
        }
    }

    pub fn due(&self, t: u64) -> bool {
        self.cfg.enabled && t.saturating_sub(self.last_sample) >= self.cfg.sample_every
    }

    /// Feed in one observation. Debounced, so a single missed detection does
    /// not flip Atlas into unattended mode while you sit there.
    pub fn observe(&mut self, look: Look, t: u64) -> Change {
        self.last_sample = t;
        let before = self.state;

        if look.faces == 0 {
            self.seen_looks = 0;
            self.empty_looks += 1;
            if self.empty_looks >= self.cfg.leave_after {
                self.state = Presence::Away;
                if before != Presence::Away {
                    self.left_at = Some(t);
                }
            }
        } else {
            self.empty_looks = 0;
            self.seen_looks += 1;
            if self.seen_looks >= self.cfg.return_after {
                self.state = match (look.you, look.faces) {
                    (true, 1) => Presence::AtDesk,
                    (true, _) => Presence::NotAlone,
                    (false, _) => Presence::Stranger,
                };
                if before == Presence::Away {
                    self.left_at = None;
                }
            }
        }

        classify_change(before, self.state)
    }

    /// Camera unavailable or disabled. Falls back to not knowing, which every
    /// caller must treat as "carry on as normal" rather than "nobody's there".
    pub fn blind(&mut self) {
        self.state = Presence::Unknown;
        self.empty_looks = 0;
        self.seen_looks = 0;
    }
}

fn classify_change(before: Presence, after: Presence) -> Change {
    use Presence::*;
    match (before, after) {
        (a, b) if a == b => Change::None,
        (_, Away) => Change::Left,
        (Away, _) => Change::Returned,
        (AtDesk, Stranger) | (AtDesk, NotAlone) | (Unknown, Stranger) => Change::StrangerArrived,
        (Stranger, AtDesk) | (NotAlone, AtDesk) => Change::StrangerLeft,
        _ => Change::None,
    }
}

/// Silent answers, for when speaking would be wrong.
///
/// The honest case for gestures is narrow. Hand-signing arbitrary commands is
/// slower and less reliable than saying them. But answering *yes* or *no*
/// without speaking — in a meeting, on a call, with someone at your desk — is
/// genuinely better than voice, and it is exactly the moment Atlas most often
/// needs an answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    ThumbUp,
    ThumbDown,
    /// Palm out: stop talking.
    OpenPalm,
    None,
}

/// What a gesture is allowed to mean.
///
/// Only ever an answer to a question Atlas already asked, or a stop. Never a
/// command in its own right — a misread gesture that opens an app is
/// confusing, and one that approves a post is unacceptable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Yes,
    No,
    Stop,
    Ignored,
}

pub fn interpret(g: Gesture, atlas_is_asking: bool, atlas_is_speaking: bool) -> Signal {
    match g {
        Gesture::OpenPalm if atlas_is_speaking => Signal::Stop,
        Gesture::ThumbUp if atlas_is_asking => Signal::Yes,
        Gesture::ThumbDown if atlas_is_asking => Signal::No,
        _ => Signal::Ignored,
    }
}

/// Gestures never approve something consequential on their own.
///
/// A thumbs-up is two fingers of confidence from a webcam. Sending a post or
/// closing unsaved work needs a word.
pub fn may_answer(signal: Signal, consequential: bool) -> bool {
    match signal {
        Signal::Stop => true,
        Signal::Yes => !consequential,
        Signal::No => true, // declining is always safe
        Signal::Ignored => false,
    }
}
