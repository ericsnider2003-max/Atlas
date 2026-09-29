//! Gestures you define, from the geometry of your hand.
//!
//! ## Why this exists
//!
//! There are two shapes a hand model can come in, and the difference decides
//! whether the gesture vocabulary is fixed forever or open.
//!
//! A **classifier** outputs a label from a list somebody else chose:
//! `thumb_up`, `fist`, `victory`. Whatever is on that list is what you get. Ask
//! for a gesture the trainer did not think of and the answer is "retrain the
//! model", which in practice means never.
//!
//! A **landmark model** outputs the position of every joint — twenty-one
//! points per hand. It has no opinion about what any of them mean. Every
//! gesture is then arithmetic on those points, written here, in Atlas.
//!
//! Eric's constraint settles it: he does not want his vocabulary limited by
//! what somebody else's model already knows. So Atlas reads landmarks and
//! defines the gestures itself, and adding one is a few lines of geometry
//! rather than a training run. Two hands, twenty-one points each, is enough to
//! express anything a hand can physically do.
//!
//! ## Only computing what is actually used
//!
//! His other constraint: nothing processed that nothing acts on. So a
//! `Vocabulary` holds only the gestures currently bound to something, and
//! `Reading` computes a feature the first time a gesture asks for it and not
//! at all if none do. Curl for five fingers, spread, pinch distance and
//! orientation are cheap individually and pointless in aggregate when three
//! gestures are enabled.

use serde::{Deserialize, Serialize};

/// One joint, in camera-frame coordinates 0.0–1.0.
///
/// `z` is depth relative to the wrist where the model provides it, and zero
/// where it does not — gestures that need depth say so and are simply
/// unavailable rather than silently wrong.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Point {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point {
    pub fn from(x: f32, y: f32) -> Point {
        Point { x, y, z: 0.0 }
    }

    fn away_from(&self, other: &Point) -> f32 {
        ((self.x - other.x).powi(2) + (self.y - other.y).powi(2)).sqrt()
    }
}

/// The twenty-one points, in the order every landmark model reports them.
///
/// Named rather than indexed by number at the call sites, because
/// `points[8]` is a bug waiting to be written and `index_tip()` is not.
pub const POINTS: usize = 21;

/// A hand, as joints.
#[derive(Debug, Clone, PartialEq)]
pub struct Landmarks {
    pub points: [Point; POINTS],
    /// Whether this is the right hand, when the model says.
    pub right: Option<bool>,
    pub sure: f32,
}

impl Landmarks {
    fn wrist(&self) -> Point {
        self.points[0]
    }
    pub fn thumb_tip(&self) -> Point {
        self.points[4]
    }
    pub fn index_tip(&self) -> Point {
        self.points[8]
    }
    fn little_tip(&self) -> Point {
        self.points[20]
    }
    fn knuckle(&self, finger: usize) -> Point {
        self.points[[2, 5, 9, 13, 17][finger]]
    }
    fn tip(&self, finger: usize) -> Point {
        self.points[[4, 8, 12, 16, 20][finger]]
    }

    /// How big the hand is on screen, used to make everything else
    /// distance-independent.
    ///
    /// Without this, every threshold would be right at one arm's length and
    /// wrong at another — the single most common way hand gestures come out
    /// unreliable.
    pub fn span(&self) -> f32 {
        let s = self.wrist().away_from(&self.points[9]);
        if s < 0.0001 {
            0.01
        } else {
            s
        }
    }
}

/// Read what a landmark model returned.
///
/// Every landmark model of this shape outputs the joints in the same order,
/// three numbers each. What varies is whether a confidence is tacked on the
/// end, so that is read when present and assumed certain when not — rather
/// than refusing a model that is simply terser.
///
/// A short or malformed output produces `None`. Padding it with zeros would
/// put every missing joint at the top-left corner of the frame, which reads as
/// a real hand in a real place.
/// Put a model's reading back where it happened.
///
/// The landmark model is given a small square cut out of the camera frame and
/// answers in pixels of that square. Everything downstream — the pointer, the
/// overlay ring, the drag — works in fractions of the whole frame, because a
/// fraction still means the same thing after a resize and a pixel does not.
///
/// Nothing did this conversion before, and the gap was invisible while there
/// were no weights to run: the numbers all had the right shape, so the first
/// real reading would have multiplied a couple of hundred by the width of the
/// screen and put the pointer somewhere that does not exist.
///
/// `sure` is carried along on the end, where `from_model` looks for it.
pub fn in_the_frame(
    joints: &[f32],
    sure: f32,
    model: (f32, f32),
    crop_at: (f32, f32),
    crop_size: (f32, f32),
) -> Vec<f32> {
    let need = POINTS * 3;
    if joints.len() < need || model.0 <= 0.0 || model.1 <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(need + 1);
    for i in 0..POINTS {
        out.push(crop_at.0 + (joints[i * 3] / model.0) * crop_size.0);
        out.push(crop_at.1 + (joints[i * 3 + 1] / model.1) * crop_size.1);
        // Depth is in the same units the model measured width in, so it is
        // scaled the same way. Left in the frame's terms rather than the
        // crop's, or a hand near the camera and a hand far from it would
        // report the same depth.
        out.push((joints[i * 3 + 2] / model.0) * crop_size.0);
    }
    out.push(sure);
    out
}

pub fn from_model(out: &[f32]) -> Option<Landmarks> {
    let need = POINTS * 3;
    if out.len() < need {
        return None;
    }
    let mut points = [Point::default(); POINTS];
    for (i, p) in points.iter_mut().enumerate() {
        *p = Point {
            x: out[i * 3],
            y: out[i * 3 + 1],
            z: out[i * 3 + 2],
        };
    }
    // All three, not just x and y. A depth value that is not a number would
    // pass a check on the other two and then poison anything that later
    // reasons about how far away a finger is.
    if points
        .iter()
        .any(|p| !p.x.is_finite() || !p.y.is_finite() || !p.z.is_finite())
    {
        return None;
    }
    let sure = out.get(need).copied().unwrap_or(1.0).clamp(0.0, 1.0);
    Some(Landmarks { points, right: None, sure })
}

/// Features computed from landmarks, on demand.
///
/// Each is worked out the first time something asks and cached. Nothing is
/// computed because it might be useful.
#[derive(Debug, Clone)]
pub struct Reading<'a> {
    hand: &'a Landmarks,
    motion: Motion,
    curl: [Option<f32>; 5],
    pinch: Option<f32>,
    spread: Option<f32>,
}

impl<'a> Reading<'a> {
    pub fn of(hand: &'a Landmarks) -> Reading<'a> {
        Reading { hand, motion: Motion::default(), curl: [None; 5], pinch: None, spread: None }
    }

    /// The same, knowing how the hand is moving.
    pub fn moving(hand: &'a Landmarks, motion: Motion) -> Reading<'a> {
        Reading { hand, motion, ..Reading::of(hand) }
    }

    /// How closed a finger is: 0.0 straight out, 1.0 folded into the palm.
    ///
    /// Measured tip-to-knuckle against the hand's own span, so it means the
    /// same at any distance from the camera.
    pub fn curl(&mut self, finger: usize) -> f32 {
        let finger = finger.min(4);
        if let Some(c) = self.curl[finger] {
            return c;
        }
        let reach = self.hand.tip(finger).away_from(&self.hand.knuckle(finger));
        // An extended finger reaches roughly one span from its knuckle.
        let c = (1.0 - (reach / self.hand.span())).clamp(0.0, 1.0);
        self.curl[finger] = Some(c);
        c
    }

    /// Thumb-to-index distance, relative to hand size.
    pub fn pinch(&mut self) -> f32 {
        if let Some(p) = self.pinch {
            return p;
        }
        let p = self.pinch_with(1);
        self.pinch = Some(p);
        p
    }

    /// Thumb to any named finger, relative to hand size.
    ///
    /// Not cached: unlike the plain pinch, several fingers may be asked about
    /// and caching one of them would answer for all.
    fn pinch_with(&mut self, finger: usize) -> f32 {
        self.hand.thumb_tip().away_from(&self.hand.tip(finger.min(4).max(1))) / self.hand.span()
    }

    /// How far apart the fingertips are — high when the hand is spread.
    pub fn spread(&mut self) -> f32 {
        if let Some(s) = self.spread {
            return s;
        }
        let s = self.hand.index_tip().away_from(&self.hand.little_tip()) / self.hand.span();
        self.spread = Some(s);
        s
    }

    /// How many fingers are extended.
    pub fn extended(&mut self) -> u8 {
        (0..5).filter(|f| self.curl(*f) < 0.4).count() as u8
    }

    pub fn hand(&self) -> &Landmarks {
        self.hand
    }
}

/// How the hand is moving, when anything knows.
///
/// Added because Eric's own vocabulary is movement-heavy and this file was
/// static-only. Four of the nine gestures he demonstrated are the *same hand
/// shape* as another one and differ only by what the hand was doing: a pinch
/// held is picking something up, the same pinch flicked away is dismissing it.
/// A static-only test system cannot tell those apart, and would have fired the
/// wrong one about half the time.
///
/// Speeds are hand-spans per second, so they mean the same at any distance
/// from the camera — the same reason every other threshold here is measured
/// against the span rather than in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    pub across: f32,
    pub down: f32,
    /// How many times the hand changed direction in the last second.
    ///
    /// The second thing the recordings forced into existence. Two of the new
    /// gestures are not a shape and not a direction — they are a shape done
    /// *repeatedly*: a pinch opening and closing, a hooked hand stroking
    /// downward again and again. Both are the same shape moving the same way
    /// as something else, and only the repetition separates them.
    ///
    /// Without this, a scroll would fire "dismiss" on its first stroke and
    /// then again on every stroke after it.
    pub reversals: f32,
}

impl Motion {
    /// Below this, the hand counts as still.
    ///
    /// Nobody holds a hand perfectly steady, and treating any drift as a
    /// swipe is how a held gesture becomes an accidental one.
    pub const STILL: f32 = 0.6;
    /// Above this, it is a deliberate movement rather than drift.
    pub const MOVING: f32 = 1.6;

    fn speed(&self) -> f32 {
        (self.across * self.across + self.down * self.down).sqrt()
    }

    pub fn still(&self) -> bool {
        self.speed() < Self::STILL
    }

    /// Reversals a second above which the hand is doing something repeatedly
    /// rather than once.
    ///
    /// Two is one full back-and-forth. Below that, an ordinary gesture that
    /// happens to overshoot and correct would read as repeating.
    pub const RHYTHM: f32 = 2.5;

    pub fn repeating(&self) -> bool {
        self.reversals >= Self::RHYTHM
    }
}

/// Turns a stream of hand positions into a [`Motion`].
///
/// Until this existed, nothing in production ever built a `Motion` with real
/// numbers in it — `recognise_moving` had tests and no caller, so every
/// motion-bound gesture (a swipe, a repeated stroke) was defined, evaluated
/// against `Motion::default()`, and could never fire. The tracking loop feeds
/// this one index-tip position per frame and reads the motion back out.
///
/// **Units.** Positions arrive in the landmark frame's own 0..1 coordinates,
/// so speeds here are frame-widths per second. Against that unit the
/// constants above land where they should: a hand held deliberately still
/// drifts well under [`Motion::STILL`] (0.6/s), and a crisp swipe crosses the
/// frame at two to four widths a second, over [`Motion::MOVING`] (1.6/s).
/// That mapping is an assumption stated here rather than a measurement — the
/// day it is measured against a real camera, this comment is where the answer
/// goes.
#[derive(Debug, Clone, Default)]
pub struct Trail {
    /// (x, y, ms) — newest last.
    samples: Vec<(f32, f32, u32)>,
}

impl Trail {
    /// Speed is read over this much recent movement. Short enough that a
    /// swipe reads as fast while it is happening, long enough that one frame
    /// of jitter is not a velocity.
    const SPEED_WINDOW_MS: u32 = 300;
    /// Reversals are counted over a full second, because `reversals` is
    /// defined as direction changes *per second*.
    const RHYTHM_WINDOW_MS: u32 = 1000;
    /// A frame-to-frame step smaller than this is sensor noise, not a
    /// direction. Counting noise as reversals would make every still hand
    /// read as repeating.
    const STEP: f32 = 0.008;

    /// Feed one position, read the motion so far.
    pub fn saw(&mut self, x: f32, y: f32, now_ms: u32) -> Motion {
        self.samples.push((x, y, now_ms));
        let keep_from = now_ms.saturating_sub(Self::RHYTHM_WINDOW_MS);
        self.samples.retain(|(_, _, t)| *t >= keep_from);

        // Speed: oldest sample inside the speed window against now.
        let speed_from = now_ms.saturating_sub(Self::SPEED_WINDOW_MS);
        let base = self.samples.iter().find(|(_, _, t)| *t >= speed_from);
        let (across, down) = match base {
            Some(&(bx, by, bt)) if now_ms > bt => {
                let secs = (now_ms - bt) as f32 / 1000.0;
                ((x - bx) / secs, (y - by) / secs)
            }
            _ => (0.0, 0.0),
        };

        // Reversals: sign changes of the dominant-axis step, per second of
        // window actually held.
        let mut flips = 0u32;
        let mut last_sign = 0i8;
        for pair in self.samples.windows(2) {
            let (ax, ay, _) = pair[0];
            let (bx, by, _) = pair[1];
            let (dx, dy) = (bx - ax, by - ay);
            let step = if dx.abs() >= dy.abs() { dx } else { dy };
            if step.abs() < Self::STEP {
                continue;
            }
            let sign = if step > 0.0 { 1i8 } else { -1i8 };
            if last_sign != 0 && sign != last_sign {
                flips += 1;
            }
            last_sign = sign;
        }
        let held_ms = self
            .samples
            .first()
            .map(|(_, _, t)| now_ms.saturating_sub(*t))
            .unwrap_or(0)
            .max(1);
        let reversals = flips as f32 * 1000.0 / held_ms as f32;

        Motion { across, down, reversals }
    }

    /// The hand went away. Forget the trail rather than measuring a jump
    /// from wherever it was last seen to wherever it reappears.
    pub fn lost(&mut self) {
        self.samples.clear();
    }
}

/// A test a gesture applies to a reading.
///
/// Composable, so a new gesture is a few of these rather than new code. The
/// list is deliberately small: anything a hand does is some combination of
/// which fingers are out, how close two of them are, and how spread the hand
/// is.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Test {
    /// This finger is extended. 0 thumb, 4 little finger.
    Out(usize),
    /// This finger is folded.
    In(usize),
    /// Thumb and index are together.
    Pinched,
    /// Thumb meets a specific finger. 1 index, 2 middle, and so on.
    ///
    /// Added so two pinches can be told apart by *shape* rather than by
    /// timing. Separating by shape is known on the first frame; separating by
    /// timing needs history, and anything needing history feels slow.
    PinchedWith(usize),
    /// Half closed — the hooked hand you make turning a page.
    ///
    /// Neither out nor folded. Without this, a hooked stroke and a flat push
    /// are the same hand moving the same way, and only repetition separates
    /// them — which means the first stroke of a scroll lags until the second
    /// arrives.
    Curled(usize),
    /// Thumb and index are apart.
    NotPinched,
    /// Fingers spread wide.
    Spread,
    /// Exactly this many fingers extended.
    Extended(u8),
    /// The hand is held steady.
    Still,
    /// Moving up, down, left or right — from your point of view, so the
    /// mirroring is already dealt with by the time a gesture sees it.
    MovingUp,
    MovingDown,
    MovingLeft,
    MovingRight,
    /// The shape is being made over and over rather than once.
    Repeating,
    /// Done once, not repeatedly.
    Once,
}

impl Test {
    /// Does this test need to know how the hand is moving?
    ///
    /// Used to keep the promise that nothing is computed for gestures nothing
    /// is bound to: a vocabulary of purely static shapes never asks for
    /// motion, and motion is the only part that needs history.
    fn needs_motion(self) -> bool {
        matches!(
            self,
            Test::Still
                | Test::MovingUp
                | Test::MovingDown
                | Test::MovingLeft
                | Test::MovingRight
                | Test::Repeating
                | Test::Once
        )
    }

    fn holds(self, r: &mut Reading) -> bool {
        let m = r.motion;
        match self {
            Test::Still => m.still(),
            Test::MovingUp => -m.down > Motion::MOVING,
            Test::MovingDown => m.down > Motion::MOVING,
            Test::MovingLeft => -m.across > Motion::MOVING,
            Test::MovingRight => m.across > Motion::MOVING,
            Test::Repeating => m.repeating(),
            Test::Once => !m.repeating(),
            Test::Out(f) => r.curl(f) < 0.4,
            Test::In(f) => r.curl(f) > 0.6,
            Test::Pinched => r.pinch() < 0.45,
            Test::PinchedWith(f) => r.pinch_with(f) < 0.45,
            Test::Curled(f) => (0.4..=0.75).contains(&r.curl(f)),
            Test::NotPinched => r.pinch() >= 0.45,
            Test::Spread => r.spread() > 1.1,
            Test::Extended(n) => r.extended() == n,
        }
    }

    /// Said the way you would describe it out loud, for explaining a clash.
    pub fn plain(self) -> String {
        let name = |f: usize| ["thumb", "index finger", "middle finger", "ring finger", "little finger"][f.min(4)];
        match self {
            Test::Out(f) => format!("{} out", name(f)),
            Test::In(f) => format!("{} folded", name(f)),
            Test::Pinched => "thumb and index finger together".into(),
            Test::PinchedWith(f) => format!("thumb and {} together", name(f)),
            Test::Curled(f) => format!("{} hooked", name(f)),
            Test::NotPinched => "thumb and finger apart".into(),
            Test::Spread => "fingers spread".into(),
            Test::Extended(n) => format!("{n} fingers out"),
            Test::Still => "held steady".into(),
            Test::MovingUp => "moving up".into(),
            Test::MovingDown => "moving down".into(),
            Test::MovingLeft => "moving left".into(),
            Test::MovingRight => "moving right".into(),
            Test::Repeating => "done over and over".into(),
            Test::Once => "done once".into(),
        }
    }
}

/// A feature as one particular hand actually makes it: the middle of the
/// teaching samples and how much they wandered.
///
/// This is what the twenty demonstration samples are FOR. For a long time
/// `Learning::watch` measured each one against the universal thresholds,
/// kept a tally, and threw the measurements away — so a gesture taught by a
/// hand whose "finger out" curls at 0.38 was matched forever against the
/// universal 0.4, one frame of jitter from not being seen. The samples were
/// always enough to calibrate per hand; nothing kept them.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Band {
    /// The median of the teaching samples for this feature.
    pub usually: f32,
    /// Their mean distance from that median.
    pub varies_by: f32,
}

impl Band {
    /// How far from `usually` still counts as the same shape.
    ///
    /// Four spreads, floored: a demonstration held eerily steady must not
    /// produce a band nothing human can stay inside, and a fifteenth of the
    /// curl scale is about one frame of ordinary sensor jitter.
    pub fn slack(&self) -> f32 {
        (self.varies_by * 4.0).max(0.06)
    }
}

/// A gesture you defined.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Gesture {
    /// What you call it.
    pub name: String,
    /// What it does. The reason it is bound at all.
    pub does: String,
    /// All of these must hold.
    pub tests: Vec<Test>,
    /// Milliseconds it must be held. Zero acts on sight.
    pub hold_ms: u32,
    /// Both hands doing it.
    pub two_handed: bool,
    /// How the hand that taught this gesture actually made each feature.
    /// Empty on the shipped defaults and on anything saved before
    /// calibration existed — those match against the universal thresholds,
    /// exactly as before.
    #[serde(default)]
    pub bands: Vec<(Test, Band)>,
}

impl Gesture {
    pub fn holds_for(&self, r: &mut Reading) -> bool {
        self.tests.iter().all(|t| self.test_holds(*t, r))
    }

    /// One test, against the teaching hand's own band when there is one and
    /// the universal threshold when there is not.
    ///
    /// The caps on each arm keep a calibrated gesture from drifting into
    /// absurdity: however loose the demonstration, an "out" finger is never
    /// accepted past half-curled, and a fold is never accepted below it.
    fn test_holds(&self, t: Test, r: &mut Reading) -> bool {
        let band = self.bands.iter().find(|(bt, _)| *bt == t).map(|(_, b)| *b);
        match (t, band) {
            (Test::Out(f), Some(b)) => r.curl(f) <= (b.usually + b.slack()).min(0.55),
            (Test::In(f), Some(b)) => r.curl(f) >= (b.usually - b.slack()).max(0.45),
            (Test::Pinched, Some(b)) => r.pinch() <= (b.usually + b.slack()).min(0.55),
            (Test::Spread, Some(b)) => r.spread() >= (b.usually - b.slack()).max(1.0),
            _ => t.holds(r),
        }
    }

    /// Said out loud, so a gesture can be described without showing anyone a
    /// list of joint indices.
    pub fn describe(&self) -> String {
        let bits: Vec<String> = self.tests.iter().map(|t| t.plain()).collect();
        let held = if self.hold_ms > 0 {
            format!(", held for {:.1}s", self.hold_ms as f32 / 1000.0)
        } else {
            String::new()
        };
        format!("{}: {}{} — {}", self.name, bits.join(", "), held, self.does)
    }
}

/// The gestures currently bound to something.
///
/// Only these are ever evaluated. A gesture that is defined and not bound
/// costs nothing, which is the whole point — Eric asked for nothing processed
/// that nothing acts on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Vocabulary {
    pub gestures: Vec<Gesture>,
}

impl Vocabulary {
    /// Which gesture this hand is making, if any.
    ///
    /// First match wins, so order is meaningful: put the specific before the
    /// general, or a three-test gesture never fires because a one-test one
    /// above it already matched.
    pub fn recognise(&self, hand: &Landmarks) -> Option<&Gesture> {
        self.recognise_moving(hand, Motion::default())
    }

    /// The same, knowing how the hand is moving.
    pub fn recognise_moving(&self, hand: &Landmarks, motion: Motion) -> Option<&Gesture> {
        if self.gestures.is_empty() {
            // Nothing bound. Not a single feature is computed.
            return None;
        }
        let mut r = Reading::moving(hand, motion);
        self.matched(&mut r).map(|i| &self.gestures[i])
    }

    /// Which bound gesture this reading matches, by position.
    ///
    /// The tracking loop needs the index — its hold clocks are kept per
    /// gesture — and it already holds a `Reading`, so this is the shape it
    /// calls. One reading serves the whole vocabulary; the old loop built a
    /// fresh one per gesture and re-measured the same hand as many times as
    /// there were bindings. First match wins, same as [`Self::recognise`].
    pub fn matched(&self, r: &mut Reading) -> Option<usize> {
        self.gestures.iter().position(|g| g.holds_for(r))
    }

    /// Two gestures that can both be true at once.
    ///
    /// The clash that matters with composable tests: `Extended(1)` and
    /// `Out(1), In(0)` describe the same hand. Whichever is listed first wins
    /// and the other never fires, which looks like the detector failing.
    pub fn overlaps(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for (i, a) in self.gestures.iter().enumerate() {
            for b in self.gestures.iter().skip(i + 1) {
                // One pair of hands cannot be doing a two-handed gesture and a
                // one-handed one at the same time. Missing this made the
                // two-handed sweep look like it shadowed everything with five
                // fingers out.
                if a.two_handed != b.two_handed {
                    continue;
                }
                if compatible(&a.tests, &b.tests) {
                    out.push((a.name.clone(), b.name.clone()));
                }
            }
        }
        out
    }

    /// Does anything here care how the hand is moving?
    ///
    /// So the caller can skip tracking velocity entirely for a vocabulary of
    /// static shapes.
    pub fn any_motion(&self) -> bool {
        self.gestures
            .iter()
            .any(|g| g.tests.iter().any(|t| t.needs_motion()))
    }

    /// Does anything here need holding?
    ///
    /// Used to decide whether the hold is worth the delay at all — if nothing
    /// in the vocabulary is ambiguous, nothing has to wait.
    pub fn any_held(&self) -> bool {
        self.gestures.iter().any(|g| g.hold_ms > 0)
    }
}

/// Could one hand satisfy both sets of tests?
fn compatible(a: &[Test], b: &[Test]) -> bool {
    for x in a {
        for y in b {
            if contradicts(*x, *y) {
                return false;
            }
        }
    }
    true
}

fn contradicts(a: Test, b: Test) -> bool {
    match (a, b) {
        (Test::Out(x), Test::In(y)) | (Test::In(y), Test::Out(x)) => x == y,
        (Test::Pinched, Test::NotPinched) | (Test::NotPinched, Test::Pinched) => true,
        (Test::Extended(x), Test::Extended(y)) => x != y,
        // A pinch folds the index finger towards the thumb, so it cannot also
        // be extended.
        (Test::Pinched, Test::Out(1)) | (Test::Out(1), Test::Pinched) => true,
        (Test::Spread, Test::Extended(n)) | (Test::Extended(n), Test::Spread) => n < 4,
        // A count of extended fingers contradicts any statement about a
        // specific finger it cannot agree with. Five out means none folded;
        // none out means none extended. Missing this rule made eight pairs in
        // the demonstrated vocabulary look like they described the same hand.
        (Test::Extended(5), Test::In(_)) | (Test::In(_), Test::Extended(5)) => true,
        (Test::Extended(0), Test::Out(_)) | (Test::Out(_), Test::Extended(0)) => true,
        (Test::Extended(0), Test::Pinched) | (Test::Pinched, Test::Extended(0)) => true,
        // A pinch bends the index towards the thumb, so it cannot also be one
        // of five fingers held out.
        (Test::Extended(5), Test::Pinched) | (Test::Pinched, Test::Extended(5)) => true,
        // Two pinches to different fingers are different hands.
        (Test::PinchedWith(x), Test::PinchedWith(y)) => x != y,
        (Test::Pinched, Test::PinchedWith(y)) | (Test::PinchedWith(y), Test::Pinched) => y != 1,
        // A hooked finger is neither out nor folded.
        (Test::Curled(x), Test::Out(y)) | (Test::Out(y), Test::Curled(x)) => x == y,
        (Test::Curled(x), Test::In(y)) | (Test::In(y), Test::Curled(x)) => x == y,
        (Test::Curled(_), Test::Extended(5)) | (Test::Extended(5), Test::Curled(_)) => true,
        (Test::Curled(_), Test::Extended(0)) | (Test::Extended(0), Test::Curled(_)) => true,
        // Pinching needs the thumb reaching out to a finger, so it cannot also
        // be folded into the palm, nor can the hand be flat.
        (Test::PinchedWith(_), Test::In(0)) | (Test::In(0), Test::PinchedWith(_)) => true,
        (Test::PinchedWith(_), Test::Extended(5)) | (Test::Extended(5), Test::PinchedWith(_)) => true,
        (Test::PinchedWith(_), Test::Extended(0)) | (Test::Extended(0), Test::PinchedWith(_)) => true,
        // "Thumb and index apart" is exactly the denial of pinching the index.
        (Test::NotPinched, Test::PinchedWith(1)) | (Test::PinchedWith(1), Test::NotPinched) => true,
        // A fingertip held against the thumb is not hooked in mid-air.
        (Test::PinchedWith(x), Test::Curled(y)) | (Test::Curled(y), Test::PinchedWith(x)) => x == y,
        // A hand cannot be still and moving, nor going two ways at once.
        (Test::Still, b) if b.needs_motion() => true,
        (a, Test::Still) if a.needs_motion() => true,
        (Test::MovingUp, Test::MovingDown) | (Test::MovingDown, Test::MovingUp) => true,
        (Test::MovingLeft, Test::MovingRight) | (Test::MovingRight, Test::MovingLeft) => true,
        (Test::Repeating, Test::Once) | (Test::Once, Test::Repeating) => true,
        _ => false,
    }
}

// ===========================================================================
// Holding without feeling slow.
//
// Eric agreed a hold stops accidental firing and then said the thing that
// matters: it must not feel behind or delayed. Those are only in tension if
// "held" means "nothing happens until the hold completes". It doesn't have to.
//
// Three rules, and together they mean the hold costs no perceived latency:
//
// **Only ambiguous gestures hold at all.** A gesture nothing else passes
// through, and that nobody makes by accident, fires on sight. Most do. The
// hold is not a general tax — it applies to the specific shapes that need it.
//
// **Feedback starts immediately.** The moment the shape is recognised, the
// ring begins filling. The delay becomes visible progress rather than a dead
// interface, and a visible half-second reads as deliberate where an invisible
// one reads as broken.
//
// **Reversible things act at once and undo if abandoned.** Moving a window can
// start on sight — if you let go early it goes back. Only what cannot be taken
// back genuinely waits for the hold.
// ===========================================================================

/// Where a gesture is up to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Progress {
    /// Not being made.
    No,
    /// Being made, this far through the hold, 0.0–1.0.
    ///
    /// Drives the ring. This is the value that makes a hold feel deliberate
    /// rather than laggy.
    Holding(f32),
    /// Held long enough. Act.
    Done,
}

/// Tracks one gesture being held.
#[derive(Debug, Clone, Copy, Default)]
pub struct Holding {
    since_ms: Option<u32>,
    /// Already fired, so it does not fire again every frame while held.
    fired: bool,
}

impl Holding {
    /// Feed whether the shape is currently being made.
    pub fn seen(&mut self, making: bool, now_ms: u32, hold_ms: u32) -> Progress {
        if !making {
            *self = Holding::default();
            return Progress::No;
        }
        if self.fired {
            // Still holding after it fired. Not a second command.
            return Progress::No;
        }
        let since = *self.since_ms.get_or_insert(now_ms);
        if hold_ms == 0 {
            self.fired = true;
            return Progress::Done;
        }
        let held = now_ms.saturating_sub(since);
        if held >= hold_ms {
            self.fired = true;
            return Progress::Done;
        }
        Progress::Holding((held as f32 / hold_ms as f32).clamp(0.0, 1.0))
    }
}

/// Does this gesture need to be held at all?
///
/// A hold is the price of ambiguity, so anything unambiguous should not pay
/// it. Pinch is the obvious case: nothing passes through a pinch on the way
/// anywhere, and nobody pinches by accident at a desk, so it fires on sight.
pub fn needs_holding(g: &Gesture, others: &[Gesture]) -> bool {
    // Two hands is already deliberate — nobody does that by accident.
    if g.two_handed {
        return false;
    }
    // A gesture another one passes through has to wait, or the one passing
    // through fires it. An open hand is the usual culprit: a pinch relaxes
    // through it.
    let passed_through = others.iter().any(|o| {
        o.name != g.name && o.tests.iter().any(|t| matches!(t, Test::Pinched))
            && g.tests.iter().any(|t| matches!(t, Test::NotPinched | Test::Spread))
    });
    passed_through
}

// ===========================================================================
// Learning a gesture by being shown one.
//
// Eric's question: if he thinks of a new gesture later, can Atlas adopt it?
//
// Editing a config file would technically answer yes and practically answer
// no — nobody invents a gesture at their desk and then goes and writes
// `Test::Out(3), Test::In(1)` in YAML. The honest yes is: hold the shape up,
// say what it should do, and Atlas works out the geometry itself.
//
// That is entirely doable without any training. The landmark model already
// reports the joints; deriving "index out, middle folded, thumb apart" from
// them is the same arithmetic `Reading` already does, run backwards.
// ===========================================================================

/// How many readings to take before deciding what a shape is.
///
/// Enough that one bad frame does not define the gesture, few enough that
/// holding it is not a chore. About a second at twenty a second.
pub const SAMPLES: usize = 20;

/// How consistent a feature has to be across those samples to count.
///
/// A finger that is out in eighteen of twenty samples is out. One that is out
/// in eleven is not part of this gesture — it is a finger that happened to
/// move, and binding it would make the gesture unrepeatable.
pub const AGREEMENT: f32 = 0.85;

/// Watching a shape being demonstrated.
#[derive(Debug, Clone, Default)]
pub struct Learning {
    /// How many samples had each finger extended.
    out: [usize; 5],
    pinched: usize,
    spread: usize,
    taken: usize,
    /// The measurements themselves, kept rather than collapsed to the
    /// tallies above. These are what [`Self::worked_out`] turns into
    /// per-hand [`Band`]s — for a long time they were measured, compared
    /// against the universal thresholds, and dropped on the floor, which
    /// left every taught gesture calibrated to nobody.
    curls: [Vec<f32>; 5],
    pinches: Vec<f32>,
    spreads: Vec<f32>,
}

impl Learning {
    /// Take one reading of the shape being held.
    pub fn watch(&mut self, hand: &Landmarks) {
        let mut r = Reading::of(hand);
        for f in 0..5 {
            let c = r.curl(f);
            if c < 0.4 {
                self.out[f] += 1;
            }
            self.curls[f].push(c);
        }
        let p = r.pinch();
        if p < 0.45 {
            self.pinched += 1;
        }
        self.pinches.push(p);
        let s = r.spread();
        if s > 1.1 {
            self.spread += 1;
        }
        self.spreads.push(s);
        self.taken += 1;
    }

    /// The middle and wobble of one retained series, as a [`Band`].
    ///
    /// Goes through `judgment::ordinary_for` rather than re-deriving a
    /// median here — that function is the tree's one definition of
    /// "usually", settled against real data, and a second copy of it would
    /// be a place for the two to disagree.
    fn band_of(values: &[f32]) -> Option<Band> {
        let v: Vec<f64> = values.iter().map(|x| *x as f64).collect();
        let (usually, varies_by) = crate::judgment::ordinary_for(&v)?;
        Some(Band { usually: usually as f32, varies_by: varies_by as f32 })
    }

    pub fn enough(&self) -> bool {
        self.taken >= SAMPLES
    }

    pub fn taken(&self) -> usize {
        self.taken
    }

    /// What was being held.
    ///
    /// Only features that held steady across the demonstration become tests.
    /// A finger that wandered is left out entirely rather than pinned to
    /// whatever it happened to be doing on the last frame — that is the
    /// difference between a gesture that works tomorrow and one that only
    /// worked while it was being taught.
    pub fn worked_out(&self, name: &str, does: &str) -> Option<Gesture> {
        if self.taken == 0 {
            return None;
        }
        let agrees = |n: usize| n as f32 / self.taken as f32 >= AGREEMENT;
        let disagrees = |n: usize| (self.taken - n) as f32 / self.taken as f32 >= AGREEMENT;

        let mut tests = Vec::new();
        let mut bands = Vec::new();
        let mut keep = |t: Test, values: &[f32]| {
            if let Some(b) = Self::band_of(values) {
                bands.push((t, b));
            }
            tests.push(t);
        };
        for f in 0..5 {
            if agrees(self.out[f]) {
                keep(Test::Out(f), &self.curls[f]);
            } else if disagrees(self.out[f]) {
                keep(Test::In(f), &self.curls[f]);
            }
        }
        if agrees(self.pinched) {
            keep(Test::Pinched, &self.pinches);
        }
        if agrees(self.spread) {
            keep(Test::Spread, &self.spreads);
        }

        // A shape with nothing steady about it is not a shape. Refusing here
        // beats adding a gesture that matches every hand and shadows
        // everything below it.
        //
        // Counted in *fingers*, not tests: a hand that wandered can still
        // leave one finger consistently folded, and a gesture defined by one
        // finger matches an enormous family of hands. A real shape has an
        // opinion about most of them.
        let steady = (0..5)
            .filter(|f| agrees(self.out[*f]) || disagrees(self.out[*f]))
            .count();
        if steady < 3 {
            return None;
        }
        Some(Gesture {
            name: name.into(),
            does: does.into(),
            tests,
            hold_ms: 0,
            two_handed: false,
            bands,
        })
    }

    /// What to say when the shape could not be pinned down.
    pub fn why_not(&self) -> String {
        if self.taken == 0 {
            "I didn't see your hand at all.".into()
        } else {
            "Your hand moved too much for me to tell what shape you meant — \
             hold it still for a second and try again."
                .into()
        }
    }
}

impl Vocabulary {
    /// Add a gesture Atlas just watched being made.
    ///
    /// Checked against what is already bound before it goes in, so a clash is
    /// found now rather than after a week of one of them never firing. Holding
    /// is decided here too, from what it would collide with — so a gesture
    /// that needs no hold does not get one.
    pub fn adopt(&mut self, mut fresh: Gesture) -> std::result::Result<String, String> {
        if fresh.name.trim().is_empty() {
            return Err("That gesture needs a name before I can use it.".into());
        }
        if let Some(existing) = self.gestures.iter().find(|g| g.name == fresh.name) {
            return Err(format!(
                "You already have one called that — it does {}.",
                existing.does
            ));
        }

        let clash = self.gestures.iter().find(|g| compatible(&g.tests, &fresh.tests));
        if let Some(other) = clash {
            return Err(format!(
                "That's the same shape as {}, which does {}. Change one of \
                 them — a finger in or out is enough.",
                other.name, other.does
            ));
        }

        if needs_holding(&fresh, &self.gestures) {
            fresh.hold_ms = 900;
        }
        let said = format!(
            "Got it. {}{}",
            fresh.describe(),
            if fresh.hold_ms > 0 {
                " I'll want it held for a moment, because your hand passes \
                 through that shape on the way out of a pinch."
            } else {
                ""
            }
        );
        self.gestures.push(fresh);
        Ok(said)
    }

    /// Stop using one.
    pub fn forget(&mut self, name: &str) -> bool {
        let before = self.gestures.len();
        self.gestures.retain(|g| g.name != name);
        self.gestures.len() != before
    }

    /// Where the vocabulary is kept, so a gesture invented today is still
    /// there tomorrow.
    pub const FILE: &'static str = "gestures";

    pub fn load(store: &crate::store::Store) -> Vocabulary {
        store.load::<Vocabulary>(Self::FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(Self::FILE, self)
    }
}

// ===========================================================================
// The vocabulary Eric demonstrated.
//
// Nine recordings, read frame by frame. Seven are unambiguous. Two pairs are
// not, and both are recorded here as the problem rather than resolved by
// guessing — a gesture Atlas guessed the meaning of is one he would spend a
// week working around.
//
// What the recordings changed about this file: four of the nine are the *same
// hand shape* as another and differ only by movement. A pinch held still is
// picking something up; the same pinch flicked away is dismissing it. Before
// this, the test vocabulary was static-only and would have fired the wrong one
// roughly half the time.
// ===========================================================================

/// What Atlas starts with, from the nine gestures Eric recorded.
///
/// Deliberately not written into the saved vocabulary — this is the default,
/// and anything he teaches or changes lives in his own file and wins. A
/// default that overwrites what someone taught is a default nobody trusts.
pub fn as_demonstrated() -> Vec<Gesture> {
    let g = |name: &str, does: &str, tests: Vec<Test>, hold_ms: u32| Gesture {
        name: name.into(),
        does: does.into(),
        tests,
        hold_ms,
        two_handed: false,
        // The defaults are calibrated to nobody on purpose -- they were read
        // off recordings, not taught by the hand that will use them.
        bands: Vec::new(),
    };
    let two = |name: &str, does: &str, tests: Vec<Test>| Gesture {
        name: name.into(),
        does: does.into(),
        tests,
        hold_ms: 0,
        two_handed: true,
        bands: Vec::new(),
    };
    vec![
        // ---- answering ----
        // `NotPinched` matters: without it a thumbs-down and a downward flick
        // describe the same hand, because a thumb held away from folded
        // fingers can still be near the index tip. Atlas's own collision check
        // caught that in this very list before any of it shipped.
        g("thumb up", "yes",
          vec![Test::Out(0), Test::In(1), Test::In(2), Test::NotPinched, Test::Still], 0),
        g("thumb down", "no",
          vec![Test::Out(0), Test::In(1), Test::In(2), Test::NotPinched, Test::MovingDown, Test::Once], 0),

        // ---- handling things ----
        // Thumb to index, other fingers folded. The natural grabbing shape,
        // kept for the thing you do most.
        g("pinch", "pick it up",
          vec![Test::PinchedWith(1), Test::In(2), Test::Still, Test::Once], 0),
        // Thumb to index with the other three OUT — the ring shape. Separated
        // from picking up by the fingers rather than by timing, so it is known
        // on the first frame.
        g("ring", "confirm it",
          vec![Test::PinchedWith(1), Test::Out(2), Test::Out(3), Test::Out(4), Test::Still], 0),
        // Thumb to MIDDLE finger, opening and shutting. Moved off the index so
        // a single deliberate pinch that wobbles can never read as this.
        g("middle pinch, open and shut", "resize it",
          vec![Test::PinchedWith(2), Test::Repeating], 0),
        g("flick away", "dismiss it",
          vec![Test::PinchedWith(1), Test::In(2), Test::MovingDown, Test::Once], 0),

        // ---- moving about ----
        // Hooked fingers, like turning a page. Separated from a flat push by
        // shape, so the first stroke works rather than waiting for a second.
        g("hooked stroke down", "scroll down",
          vec![Test::In(0), Test::Curled(1), Test::Curled(2), Test::MovingDown], 0),
        g("hooked stroke up", "scroll up",
          vec![Test::In(0), Test::Curled(1), Test::Curled(2), Test::MovingUp], 0),
        // Both hands. Nobody sweeps two hands across by accident, so it needs
        // no hold, and it cannot collide with anything one-handed — which is
        // what made the single-handed sweep impossible to tell from raising a
        // hand over the eyes.
        two("two-handed sweep", "next screen",
            vec![Test::Extended(5), Test::MovingLeft]),

        // ---- Atlas itself ----
        // Undo. The only one with no hold: waiting a second to undo a mistake
        // is the one place a delay is actively wrong.
        g("fist", "undo that", vec![Test::Extended(0), Test::Still], 0),
        // Held, because people rub their eyes all day.
        g("cover eyes", "hide what's on screen",
          vec![Test::Extended(5), Test::NotPinched, Test::MovingUp, Test::Once],
          crate::gaze::DELIBERATE_MS),
        g("palm out", "stop",
          vec![Test::Extended(5), Test::NotPinched, Test::Still], crate::gaze::DELIBERATE_MS),
        // One finger up, held. Shows every gesture — which matters most when
        // you have forgotten them and gestures are how you drive things.
        g("one finger up", "show me the gestures",
          vec![Test::Out(1), Test::In(0), Test::In(2), Test::In(3), Test::In(4), Test::Still],
          crate::gaze::DELIBERATE_MS),
    ]
}

/// The two pairs Atlas could not tell apart from the recordings.
///
/// Reported rather than resolved. Each names what it could not separate and
/// what would separate them — a second recording is not needed, a small change
/// to one of the two is.
pub fn needs_deciding() -> Vec<(&'static str, &'static str)> {
    // The three pairs from the recordings are settled, each by separating on
    // *shape* rather than timing: the sweep is two-handed, resize moved to the
    // middle finger, and scrolling hooks the fingers. Shape is known on the
    // first frame; timing needs history, and anything needing history feels
    // slow.
    //
    // What is left is the one thing shape cannot settle.
    vec![(
        "confirming something and answering a question",
        "the ring shape confirms an action and a thumb up answers a question \
         Atlas asked. They are different hands and will not be confused — but \
         they mean nearly the same thing to a person, and which one is right \
         depends on whether Atlas asked or you decided. Worth using for a \
         while before deciding whether both should exist.",
    )]
}

// ===========================================================================
// Showing someone the gestures.
//
// Eric's requirement: a way to be reminded, and *not* a recording of him doing
// them. He is giving instances of this to friends, and a video of the author
// waving at a webcam is both an odd thing to ship and useless the moment
// anyone changes a gesture.
//
// So the reference is **drawn from the definitions**. Every card below is
// generated from the same `Test` list the recogniser actually evaluates, which
// means two things worth more than the drawing itself:
//
// - It cannot drift. A reference written separately goes stale the first time
//   a threshold moves; this one is the definition, rendered.
// - Teaching a new gesture adds its card automatically, with no second step
//   anybody has to remember.
//
// Deliberately schematic rather than lifelike. A recognisable hand outline is
// a lot of drawing for no gain — what a person needs to see is which fingers
// are out and which way it moves.
// ===========================================================================

/// Which fingers a gesture wants out, in or unspecified.
///
/// Worked out from the tests, including the ones that imply the others:
/// `Extended(5)` means every finger out without naming any of them.
fn fingers_of(g: &Gesture) -> [Option<bool>; 5] {
    let mut out = [None; 5];
    for t in &g.tests {
        match t {
            Test::Out(f) => out[(*f).min(4)] = Some(true),
            Test::In(f) => out[(*f).min(4)] = Some(false),
            Test::Extended(5) => out = [Some(true); 5],
            Test::Extended(0) => out = [Some(false); 5],
            Test::Pinched => {
                // Thumb and index meeting: shown as folded, since that is
                // what they look like from the front.
                out[0] = Some(false);
                out[1] = Some(false);
            }
            _ => {}
        }
    }
    out
}

/// The way a gesture moves, as an arrow, or nothing.
fn arrow_of(g: &Gesture) -> Option<&'static str> {
    g.tests.iter().find_map(|t| match t {
        Test::MovingUp => Some("up"),
        Test::MovingDown => Some("down"),
        Test::MovingLeft => Some("left"),
        Test::MovingRight => Some("right"),
        _ => None,
    })
}

/// A small drawing of one gesture.
///
/// Plain SVG so it renders in the hub, in Atlas's own window, and in anything
/// printed, with no image files to ship and nothing to load.
pub fn sketch(g: &Gesture) -> String {
    let fingers = fingers_of(g);
    let mut parts = String::new();

    // Palm.
    parts.push_str(
        "<rect x='26' y='52' width='36' height='34' rx='9' \
         fill='none' stroke='currentColor' stroke-width='3'/>",
    );

    // Four fingers, left to right, and the thumb off the side.
    for (i, state) in fingers.iter().enumerate().skip(1) {
        let x = 30 + (i as i32 - 1) * 9;
        let (y, h) = match state {
            Some(true) => (18, 34),
            // Folded fingers are drawn short rather than missing, so a fist
            // still reads as a hand.
            _ => (42, 10),
        };
        parts.push_str(&format!(
            "<rect x='{x}' y='{y}' width='6' height='{h}' rx='3' \
             fill='none' stroke='currentColor' stroke-width='3'/>"
        ));
    }
    let thumb = match fingers[0] {
        Some(true) => "<rect x='12' y='56' width='14' height='6' rx='3' \
                       fill='none' stroke='currentColor' stroke-width='3'/>",
        _ => "<rect x='18' y='60' width='9' height='6' rx='3' \
              fill='none' stroke='currentColor' stroke-width='3'/>",
    };
    parts.push_str(thumb);

    if g.tests.contains(&Test::Pinched) {
        // The pinch itself, since two folded fingers alone would look like
        // any other fold.
        parts.push_str(
            "<circle cx='30' cy='50' r='6' fill='none' stroke='currentColor' \
             stroke-width='3'/>",
        );
    }

    if let Some(way) = arrow_of(g) {
        let d = match way {
            "up" => "M74 76 L74 32 M67 41 L74 32 L81 41",
            "down" => "M74 32 L74 76 M67 67 L74 76 L81 67",
            "left" => "M84 54 L40 54 M49 47 L40 54 L49 61",
            _ => "M40 54 L84 54 M75 47 L84 54 L75 61",
        };
        parts.push_str(&format!(
            "<path d='{d}' fill='none' stroke='currentColor' stroke-width='3' \
             stroke-linecap='round' opacity='0.75'/>"
        ));
    }

    if g.tests.contains(&Test::Repeating) {
        // Two chevrons: the shorthand for "and again".
        parts.push_str(
            "<path d='M84 84 l6 6 l-6 6 M92 84 l6 6 l-6 6' fill='none' \
             stroke='currentColor' stroke-width='3' stroke-linecap='round' \
             opacity='0.75'/>",
        );
    }

    format!(
        "<svg viewBox='0 0 104 104' width='104' height='104' \
         xmlns='http://www.w3.org/2000/svg' role='img' aria-label='{}'>{parts}</svg>",
        g.name
    )
}

/// How to make the gesture, in words.
///
/// The drawing is a reminder; this is the instruction. Both come from the same
/// tests, so neither can disagree with what actually fires.
pub fn how_to(g: &Gesture) -> String {
    let fingers = fingers_of(g);
    let names = ["thumb", "index finger", "middle finger", "ring finger", "little finger"];
    let out: Vec<&str> = (0..5)
        .filter(|i| fingers[*i] == Some(true))
        .map(|i| names[i])
        .collect();

    let mut said = if g.tests.contains(&Test::Pinched) {
        "Thumb and index finger together".to_string()
    } else if out.len() == 5 {
        "All five fingers out".to_string()
    } else if out.is_empty() {
        "Hand closed".to_string()
    } else {
        format!("{} out", sentence_list(&out))
    };

    match arrow_of(g) {
        Some(way) => said.push_str(&format!(", moving {way}")),
        None if g.tests.contains(&Test::Still) => said.push_str(", held steady"),
        None => {}
    }
    if g.tests.contains(&Test::Repeating) {
        said.push_str(", over and over");
    }
    if g.hold_ms > 0 {
        said.push_str(&format!(" for about {:.0} second", g.hold_ms as f32 / 1000.0));
    }
    format!("{said}.")
}

fn sentence_list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => one.to_string(),
        [a, b] => format!("{a} and {b}"),
        _ => format!(
            "{} and {}",
            items[..items.len() - 1].join(", "),
            items[items.len() - 1]
        ),
    }
}
