//! Making a slow detector feel instant.
//!
//! Everything here is arithmetic. No model, no dependency, nothing to
//! download — the parts of hand tracking that decide whether it feels good
//! are not the parts that recognise a hand.
//!
//! ## The problem this solves
//!
//! A hand detector that runs at ten frames a second gives you a pointer that
//! updates every hundred milliseconds. That is not a slow pointer, it is a
//! *broken-feeling* one: it lags behind your hand, then jumps to catch up, and
//! the jump is what makes people give up on gesture control within a minute.
//!
//! Two things fix it, and neither needs a faster model:
//!
//! **Predict where the hand is going.** Between detections, keep moving the
//! pointer along the direction it was already travelling. By the time the next
//! real reading arrives you are usually within a few pixels of it, and the
//! correction is invisible instead of a jump.
//!
//! **Smooth harder when still, barely at all when moving.** A fixed smoothing
//! filter forces a choice between a jittery pointer at rest and a laggy one in
//! motion. Making the smoothing depend on speed gets both — this is the
//! one-euro filter, and it is about thirty lines.
//!
//! ## What it deliberately does not do
//!
//! It does not invent a hand. Prediction runs for a fixed short window and
//! then stops; a pointer that keeps gliding after you drop your arm is worse
//! than one that stops. `Track::confident` says which you are getting.

use serde::{Deserialize, Serialize};

/// How long to keep predicting after the last real reading, in milliseconds.
///
/// About three frames at ten detections a second. Long enough to cover a
/// missed frame, short enough that a hand leaving the frame stops the pointer
/// rather than sending it off on its own.
pub const PREDICT_FOR_MS: u32 = 250;

/// Smoothing settings.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
pub struct SmoothConfig {
    /// Smoothing when the hand is still. Lower is steadier and laggier.
    pub min_cutoff: f32,
    /// How much movement loosens the smoothing. Higher reacts faster to
    /// fast movement.
    pub speed_coefficient: f32,
    /// Smoothing applied to the speed estimate itself.
    pub derivative_cutoff: f32,
}

impl Default for SmoothConfig {
    fn default() -> Self {
        // Starting points from the one-euro paper's own guidance, not guesses:
        // tune `min_cutoff` until a still hand stops jittering, then raise
        // `speed_coefficient` until fast movement stops lagging.
        SmoothConfig {
            min_cutoff: 1.0,
            speed_coefficient: 0.007,
            derivative_cutoff: 1.0,
        }
    }
}

/// A value that is smoothed by how fast it is changing.
///
/// The one-euro filter. A plain low-pass filter has one setting and therefore
/// one compromise: steady at rest *or* responsive in motion, never both. This
/// varies the cutoff with speed, so a resting hand is filtered hard and a
/// moving one is barely filtered at all.
#[derive(Debug, Clone, Copy, Default)]
struct OneEuro {
    value: Option<f32>,
    speed: f32,
    at_ms: u32,
}

impl OneEuro {
    fn feed(&mut self, x: f32, at_ms: u32, cfg: &SmoothConfig) -> f32 {
        let Some(previous) = self.value else {
            self.value = Some(x);
            self.at_ms = at_ms;
            return x;
        };
        // Guard the interval. A repeated or out-of-order timestamp would
        // divide by zero and send the pointer to infinity, which on a real
        // machine means it vanishes off the screen.
        let dt = ((at_ms.saturating_sub(self.at_ms)).max(1)) as f32 / 1000.0;
        self.at_ms = at_ms;

        let raw_speed = (x - previous) / dt;
        self.speed = low_pass(self.speed, raw_speed, alpha(cfg.derivative_cutoff, dt));

        let cutoff = cfg.min_cutoff + cfg.speed_coefficient * self.speed.abs();
        let smoothed = low_pass(previous, x, alpha(cutoff, dt));
        self.value = Some(smoothed);
        smoothed
    }
}

fn alpha(cutoff: f32, dt: f32) -> f32 {
    let tau = 1.0 / (2.0 * std::f32::consts::PI * cutoff.max(0.0001));
    1.0 / (1.0 + tau / dt)
}

fn low_pass(previous: f32, now: f32, a: f32) -> f32 {
    a * now + (1.0 - a) * previous
}

/// Where the pointer is, between and during detections.
#[derive(Debug, Clone, Copy, Default)]
pub struct Track {
    x: OneEuro,
    y: OneEuro,
    /// Last smoothed position.
    at: Option<(f32, f32)>,
    /// Pixels per second, for predicting forwards.
    velocity: (f32, f32),
    /// When the last real reading arrived.
    last_reading_ms: u32,
    /// Whether anything has been seen at all.
    started: bool,
}

impl Track {
    /// A real reading from the detector.
    ///
    /// Returns the smoothed position to use right now.
    pub fn saw(&mut self, x: f32, y: f32, at_ms: u32, cfg: &SmoothConfig) -> (f32, f32) {
        let sx = self.x.feed(x, at_ms, cfg);
        let sy = self.y.feed(y, at_ms, cfg);

        if let Some((px, py)) = self.at {
            let dt = ((at_ms.saturating_sub(self.last_reading_ms)).max(1)) as f32 / 1000.0;
            self.velocity = ((sx - px) / dt, (sy - py) / dt);
        }
        self.at = Some((sx, sy));
        self.last_reading_ms = at_ms;
        self.started = true;
        (sx, sy)
    }

    /// Where the hand probably is now, with no new reading.
    ///
    /// This is what runs at screen rate while the detector runs at its own,
    /// much slower rate. `None` once prediction has run out — a pointer that
    /// keeps gliding after you drop your arm is worse than one that stops.
    pub fn where_now(&self, now_ms: u32) -> Option<(f32, f32)> {
        let (x, y) = self.at?;
        let ahead = now_ms.saturating_sub(self.last_reading_ms);
        if ahead > PREDICT_FOR_MS {
            return None;
        }
        let dt = ahead as f32 / 1000.0;
        Some((x + self.velocity.0 * dt, y + self.velocity.1 * dt))
    }

    /// Is this a real reading rather than a guess?
    ///
    /// Worth asking before anything irreversible. Predicting where a pointer
    /// is between frames is fine; clicking somewhere predicted is not.
    pub fn confident(&self, now_ms: u32) -> bool {
        self.started && now_ms.saturating_sub(self.last_reading_ms) <= 40
    }

    /// The hand is gone. Stop predicting.
    pub fn lost(&mut self) {
        self.at = None;
        self.velocity = (0.0, 0.0);
        self.x = OneEuro::default();
        self.y = OneEuro::default();
    }

    pub fn moving(&self) -> f32 {
        (self.velocity.0.powi(2) + self.velocity.1.powi(2)).sqrt()
    }
}

// ---------------------------------------------------------------------------
// Not eating the machine
// ---------------------------------------------------------------------------

/// How much of one core hand tracking may use.
///
/// Eric's requirement, in his words: it must not slow down his system or his
/// work. A hand tracker that runs flat out is one he turns off, which makes
/// every other decision here irrelevant.
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(default)]
pub struct PaceConfig {
    /// Target detections per second when there is room.
    pub want_per_second: u32,
    /// Never go below this — under it, tracking is not worth having on.
    pub floor_per_second: u32,
    /// The share of one core it may use, 0.0–1.0.
    pub share_of_a_core: f32,
}

impl Default for PaceConfig {
    fn default() -> Self {
        PaceConfig {
            want_per_second: 20,
            floor_per_second: 6,
            // A fifth of one core. Enough for a small detector, small enough
            // that a build running in the background does not stutter.
            share_of_a_core: 0.2,
        }
    }
}

/// Works out how often to look, from how long looking actually costs.
///
/// Measured rather than assumed, because the cost depends on his machine, the
/// model he ends up with, and what else is running — none of which is knowable
/// from here.
#[derive(Debug, Clone, Copy, Default)]
pub struct Pace {
    /// Rolling average cost of one detection, in milliseconds.
    typical_ms: f32,
    seen: u32,
}

impl Pace {
    /// Record how long a detection took.
    pub fn took(&mut self, ms: u32) {
        // Weighted towards recent, so a machine that gets busy is noticed
        // within a second or two rather than after a minute of averaging.
        let ms = ms as f32;
        self.typical_ms = if self.seen == 0 {
            ms
        } else {
            self.typical_ms * 0.8 + ms * 0.2
        };
        self.seen += 1;
    }

    /// Milliseconds to wait before looking again.
    ///
    /// Falls back to looking less often when detection gets expensive, rather
    /// than either dropping frames or hogging the core. Slower tracking is
    /// still tracking; a laptop that stutters is not.
    ///
    /// **The budget wins over the floor.** An earlier version clamped the wait
    /// so tracking never dropped below `floor_per_second`, which quietly meant
    /// an expensive detector was allowed past its share of the core to hold
    /// that rate — the opposite of what Eric asked for. `floor_per_second` is
    /// not a promise of speed; it is the line below which tracking is not
    /// worth having on, and `keeping_up` says so when it is crossed.
    pub fn wait_ms(&self, cfg: &PaceConfig) -> u32 {
        if self.seen == 0 {
            return 1000 / cfg.want_per_second.max(1);
        }
        // To use `share` of a core, one detection costing `t` must be followed
        // by a gap of `t / share`.
        let share = cfg.share_of_a_core.clamp(0.01, 1.0);
        let budgeted = (self.typical_ms / share) as u32;
        let wanted = 1000 / cfg.want_per_second.max(1);
        budgeted.max(wanted)
    }

    /// How many times a second Atlas is actually managing.
    fn per_second(&self, cfg: &PaceConfig) -> u32 {
        let wait = self.wait_ms(cfg).max(1);
        1000 / wait
    }

    /// Is it keeping up well enough to be worth having on?
    ///
    /// Said plainly rather than degrading quietly. Tracking at four frames a
    /// second is not tracking, and Atlas should say so instead of letting him
    /// conclude the whole idea does not work.
    pub fn keeping_up(&self, cfg: &PaceConfig) -> Option<String> {
        if self.seen < 10 {
            return None;
        }
        if self.per_second(cfg) > cfg.floor_per_second {
            return None;
        }
        Some(format!(
            "Reading your hands is taking about {}ms a go on this machine, so \
             I'm only managing {} looks a second. It'll feel laggy — worth \
             turning off until there's a lighter model.",
            self.typical_ms.round(),
            self.per_second(cfg)
        ))
    }
}
