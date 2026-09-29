//! Reading a face and a pair of hands off the camera.
//!
//! The loop this closes: `presence::Look`, `presence::Gesture`,
//! `presence::Sensor::observe` and `answering::saw` were all written, all
//! tested, and **nothing anywhere ever produced one**. Atlas could decide what
//! a thumbs-up means and had no way to see a thumb. `presence` has sat in the
//! unwired list since it was written for exactly that reason.
//!
//! ## What this is and is not
//!
//! It is not a vision model. It is the honest join between one — whatever you
//! install — and the decisions Atlas already knows how to make. The detector is
//! an external tool like `ocr` and `stt`: Atlas runs it on a frame and reads
//! lines back. That keeps the model a thing you choose and can replace, and
//! keeps Atlas's side testable without a camera.
//!
//! ## The three rules that matter more than the detection
//!
//! **An unread camera is not an empty room.** The failure this codebase keeps
//! producing. If the tool is missing, fails, or is switched off, that is
//! `blind()` — not "nobody there". One of those means "speak freely, he's
//! gone"; the other means "you have no idea". `presence::Presence::Unknown`
//! already treats worth_speaking as true, which is the right call for a
//! machine with no camera.
//!
//! **Unsure is absent, not false.** A face seen at forty percent confidence is
//! not a face and it is not the absence of one. Below the floor the field is
//! left out, so nothing downstream can read a shrug as a finding.
//!
//! **A face is not a password.** Recognising you well enough to say "he's back
//! at the desk" is a much lower bar than "unlock the vault", and the same
//! number should not serve both. Identity from a camera never satisfies
//! anything the confidence rules call Sensitive — that is what
//! `identity.rs` and a typed passphrase are for. A photograph held up to a
//! webcam is a real attack and there is no honest way to rule it out here.

use crate::presence::{Gesture, Look};
use serde::{Deserialize, Serialize};

/// How Atlas watches the room.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default)]
pub struct GazeConfig {
    /// Off unless you turn it on. A camera that starts watching because you
    /// updated is not something anyone should have to discover.
    pub enabled: bool,
    /// Below this a reading is treated as absent rather than negative.
    pub min_confidence: f32,
    /// The higher bar for "that is Eric, not a person who looks like him".
    ///
    /// Separate from `min_confidence` on purpose: one number cannot mean both
    /// "there is a face here" and "this is the right face", and using one for
    /// both is how a stranger becomes a session.
    pub min_identity_confidence: f32,
    /// How often to look, in seconds, when nothing more urgent is happening.
    pub every_secs: u64,
    /// May Atlas look while you're talking to it?
    ///
    /// Off by default even when the camera is on. "Sometimes I want Atlas to
    /// see me when talking" is not "watch every conversation", and watching
    /// every conversation would mean a camera on for most of the working day.
    pub watch_while_talking: bool,
}

impl Default for GazeConfig {
    fn default() -> Self {
        GazeConfig {
            enabled: false,
            min_confidence: 0.6,
            min_identity_confidence: 0.85,
            every_secs: 20,
            watch_while_talking: false,
        }
    }
}

/// What the camera saw, once.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sighting {
    /// How many faces, when the detector was sure enough to say.
    pub faces: Option<usize>,
    /// Whether one of them was you, when it was sure enough to say.
    ///
    /// `None` is not `Some(false)`. "I couldn't tell" and "that isn't you"
    /// send Atlas to different places.
    pub you: Option<bool>,
    /// Whether the face was turned towards the screen.
    pub looking: Option<bool>,
    /// A hand shape, when there was one.
    pub gesture: Option<Gesture>,
    /// Anything the detector said it could not do.
    pub could_not: Vec<String>,
}

impl Sighting {
    /// Did the camera tell Atlas anything at all?
    ///
    /// The distinction the whole module turns on. An empty sighting means the
    /// camera was not read, and must never be handed to `Sensor::observe` as
    /// an observation of an empty room.
    pub fn saw_anything(&self) -> bool {
        self.faces.is_some() || self.you.is_some() || self.gesture.is_some()
    }

    /// The observation `presence` takes, when there is one to give.
    pub fn as_look(&self) -> Option<Look> {
        let faces = self.faces?;
        Some(Look {
            faces,
            // Unknown identity is not you. Erring the other way would let a
            // shrug from the detector put a stranger at your desk.
            you: self.you.unwrap_or(false),
        })
    }
}

/// Read what a detector printed.
///
/// One `key: value` per line, so anything can be plugged in — a MediaPipe
/// script, an ONNX runner, something you write later — without Atlas caring
/// which. Unknown keys are ignored rather than rejected: a detector that
/// reports more than Atlas asked for is a detector working, not a broken one.
///
/// Recognised keys:
/// - `faces: <n> <confidence>`
/// - `you: yes|no <confidence>`
/// - `looking: yes|no <confidence>`
/// - `gesture: <name> <confidence>`
/// - `could_not: <reason>`
pub fn read(printed: &str, cfg: &GazeConfig) -> Sighting {
    let mut out = Sighting::default();
    for line in printed.lines() {
        let line = line.trim();
        let Some((key, rest)) = line.split_once(':') else {
            continue;
        };
        let parts: Vec<&str> = rest.split_whitespace().collect();
        let value = parts.first().copied().unwrap_or_default();
        let sure: f32 = parts.get(1).and_then(|c| c.parse().ok()).unwrap_or(1.0);

        match key.trim().to_lowercase().as_str() {
            "could_not" => out.could_not.push(rest.trim().to_string()),
            // Below the floor the field is left out. An unsure reading is not
            // a negative one, and writing it down as one is how a shrug
            // becomes a finding.
            _ if sure < cfg.min_confidence => {}
            "faces" => out.faces = value.parse().ok(),
            "you" => {
                // The higher bar. One number cannot mean both "there is a
                // face" and "this is the right face".
                if sure >= cfg.min_identity_confidence {
                    out.you = Some(value.eq_ignore_ascii_case("yes"));
                } else if value.eq_ignore_ascii_case("no") {
                    // Ruling you out is safe at the lower bar; ruling you in
                    // is not.
                    out.you = Some(false);
                }
            }
            "looking" => out.looking = Some(value.eq_ignore_ascii_case("yes")),
            "gesture" => out.gesture = gesture_named(value),
            _ => {}
        }
    }
    out
}

/// A hand shape by name.
///
/// Unknown names produce nothing rather than a default. A detector reporting a
/// shape Atlas does not know is not the same as it reporting a thumbs-up, and
/// guessing here would mean a wave could approve something.
pub fn gesture_named(name: &str) -> Option<Gesture> {
    match name.trim().to_lowercase().as_str() {
        "thumb_up" | "thumbup" | "thumbs_up" => Some(Gesture::ThumbUp),
        "thumb_down" | "thumbdown" | "thumbs_down" => Some(Gesture::ThumbDown),
        "open_palm" | "openpalm" | "palm" | "stop" => Some(Gesture::OpenPalm),
        // A nod and a head shake are read as the same yes and no a thumb
        // means. Deliberately *not* new variants: `interpret` already decides
        // what a yes may authorise, and a second vocabulary reaching the same
        // decisions is a second set of rules to keep honest.
        "nod" => Some(Gesture::ThumbUp),
        "shake" | "head_shake" => Some(Gesture::ThumbDown),
        // A wave is a greeting, not an answer. Mapping it to anything would
        // mean saying hello could approve something.
        _ => None,
    }
}

/// What Atlas should say about the camera when asked.
///
/// Says which of the three states it is in, because "nothing to report" covers
/// all three and distinguishes none of them.
pub fn spoken(s: &Sighting, cfg: &GazeConfig) -> String {
    if !cfg.enabled {
        return "I'm not watching the room — the camera is switched off.".into();
    }
    if !s.saw_anything() {
        let why = if s.could_not.is_empty() {
            "I couldn't read anything from the camera".to_string()
        } else {
            format!("the camera couldn't manage it: {}", s.could_not.join("; "))
        };
        return format!("{why}. That isn't the same as nobody being there.");
    }
    let mut parts = Vec::new();
    match s.faces {
        Some(0) => parts.push("nobody in front of the camera".to_string()),
        Some(1) => parts.push("one person there".to_string()),
        Some(n) => parts.push(format!("{n} people there")),
        None => {}
    }
    match s.you {
        Some(true) => parts.push("and it looks like you".to_string()),
        Some(false) if s.faces.unwrap_or(0) > 0 => {
            parts.push("and I don't recognise them".to_string())
        }
        _ => {}
    }
    if s.looking == Some(false) {
        parts.push("not looking at the screen".to_string());
    }
    if let Some(g) = s.gesture {
        parts.push(format!("hand: {}", gesture_plain(g)));
    }
    format!("{}.", parts.join(", "))
}

/// A hand shape, said rather than named.
fn gesture_plain(g: Gesture) -> &'static str {
    match g {
        Gesture::ThumbUp => "thumbs up",
        Gesture::ThumbDown => "thumbs down",
        Gesture::OpenPalm => "open palm",
        Gesture::None => "nothing",
    }
}

// ===========================================================================
// When the camera is on, and why.
//
// The first version looked every twenty seconds on a timer. That is wrong in
// both directions at once: it watches the room when nothing needs watching,
// and it is blind at the one moment that matters — Atlas asks a question and
// then does not look for another nineteen seconds while a hand is held up in
// front of it.
//
// **The camera opens for a reason and closes when the reason ends.** Never a
// bare timer, and the reason is always something Atlas can say out loud. A
// camera you cannot get an answer about is a camera you turn off.
// ===========================================================================

/// Why the camera is on right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    /// Atlas asked something and is waiting. A hand may answer it.
    Waiting,
    /// You are mid-conversation with Atlas.
    ///
    /// Eric asked for this specifically: sometimes he wants Atlas to see him
    /// while he's talking. Seeing a shake of the head halfway through an
    /// answer is worth more than the transcript of it.
    Talking,
    /// Atlas is about to read something private out loud and wants to know
    /// whether anyone else is in the room first.
    CheckingWhoElse,
    /// You told it to watch.
    YouAsked,
    /// Steering the displays by hand.
    Steering,
}

impl Reason {
    /// Said out loud, so "why is my camera on" always has an answer.
    pub fn plain(self) -> &'static str {
        match self {
            Reason::Waiting => "I asked you something and I'm watching for an answer",
            Reason::Talking => "we're mid-conversation and you asked me to watch while we talk",
            Reason::CheckingWhoElse => "I'm about to read something private and I'm checking you're alone",
            Reason::YouAsked => "you told me to watch",
            Reason::Steering => "you're moving things around by hand",
        }
    }

    /// May a hand issue a command in this state, or only answer one?
    ///
    /// Only while steering, and only ever for the display arrangement. A
    /// misread gesture that answers a question Atlas already asked is a wrong
    /// answer to a known question; a misread gesture that issues a command is
    /// something nobody asked for at all.
    pub fn hands_may_steer(self) -> bool {
        matches!(self, Reason::Steering)
    }
}

/// What is going on, as far as deciding whether to look is concerned.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Situation {
    /// Atlas asked something and nothing has answered yet.
    pub question_waiting: bool,
    /// A turn is in flight.
    pub mid_conversation: bool,
    /// The next thing Atlas would say is private.
    pub about_to_be_private: bool,
    /// You asked Atlas to watch.
    pub asked_to_watch: bool,
    /// Steering mode is on.
    pub steering: bool,
    /// Whether you have said Atlas may look while you talk.
    pub watch_while_talking: bool,
}

/// Should the camera be on, and why?
///
/// Ordered by how much the answer matters, not by how likely each is. Steering
/// first because a hand held up is worthless if Atlas is looking for a
/// different reason with a different vocabulary.
pub fn why_look(now: &Situation, cfg: &GazeConfig) -> Option<Reason> {
    if !cfg.enabled {
        return None;
    }
    if now.steering {
        return Some(Reason::Steering);
    }
    if now.question_waiting {
        return Some(Reason::Waiting);
    }
    if now.about_to_be_private {
        return Some(Reason::CheckingWhoElse);
    }
    if now.asked_to_watch {
        return Some(Reason::YouAsked);
    }
    // Only when he has said so. Watching every conversation by default would
    // be a camera on for most of the day, which is not what "sometimes I want
    // Atlas to see me" asked for.
    if now.mid_conversation && now.watch_while_talking {
        return Some(Reason::Talking);
    }
    None
}

/// How often to look, given why.
///
/// A question on the table needs a fast loop — a hand held up for two seconds
/// and missed is a feature that does not work. Nothing on the table needs no
/// loop at all.
pub fn how_often(reason: Reason) -> u64 {
    match reason {
        // Steering has to feel immediate or it feels broken.
        Reason::Steering => 1,
        Reason::Waiting | Reason::Talking => 2,
        Reason::CheckingWhoElse => 1,
        Reason::YouAsked => 5,
    }
}

// ---------------------------------------------------------------------------
// Steering the displays by hand
// ---------------------------------------------------------------------------

/// Where your hand is, and what it is doing.
///
/// The model the first version got wrong. It had `Next`/`Previous` — step
/// through panels one at a time, like a remote control. What the reference
/// actually shows is **direct manipulation**: the screen mirrors you, panels
/// sit around you, and you reach out and move the one you want. A remote
/// control is what you build when you cannot see where the hand is; a cursor
/// is what you build when you can.
///
/// Coordinates are 0.0–1.0 across the camera frame, origin top-left, so the
/// detector never has to know anything about the monitors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hand {
    pub x: f32,
    pub y: f32,
    /// Fingers closed on something.
    pub pinching: bool,
    /// Flat palm out — the summon-and-stop shape.
    pub open: bool,
    /// How sure the detector is it is seeing a hand at all.
    pub sure: f32,
}

/// What a hand did between one look and the next.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Move {
    /// Just moving — the pointer follows.
    Point { x: i32, y: i32 },
    /// Closed on something at this point.
    Grab { x: i32, y: i32 },
    /// Moved while holding.
    Drag { x: i32, y: i32 },
    /// Let go.
    Drop { x: i32, y: i32 },
    /// A pinch with no movement: a click.
    Tap { x: i32, y: i32 },
    /// Two hands moving apart or together.
    Resize { by: f32 },
    /// Palm held up: bring the hub to the front.
    Summon,
    /// Palm again, or the hand leaving the frame: stop steering.
    Done,
}

impl Move {
    /// Said out loud, so anything Atlas did by hand can be recognised in the
    /// day's account rather than appearing as an unexplained change.
    pub fn plain(self) -> String {
        match self {
            Move::Point { .. } => "moved the pointer".into(),
            Move::Grab { .. } => "picked something up".into(),
            Move::Drag { .. } => "moved it".into(),
            Move::Drop { .. } => "put it down".into(),
            Move::Tap { .. } => "selected something".into(),
            Move::Resize { by } if by > 1.0 => "made it bigger".into(),
            Move::Resize { .. } => "made it smaller".into(),
            Move::Summon => "brought the hub up".into(),
            Move::Done => "stopped steering".into(),
        }
    }

    /// Can this be taken back?
    ///
    /// Not a limit on what a hand may do — Eric asked for everything, and
    /// everything is what this drives. It is how Atlas knows what to offer to
    /// undo when a gesture lands somewhere it wasn't meant to, which it will.
    pub fn undoable(self) -> bool {
        !matches!(self, Move::Point { .. })
    }
}

/// Where the hand was last, so a move can be told from a hold.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Steering {
    pub was: Option<Hand>,
    /// Where the pinch started, so a pinch that never moved is a tap.
    pub grabbed_at: Option<(i32, i32)>,
    /// Frames with no hand. Used to end steering rather than ending it on the
    /// first blink, which would drop whatever was being carried.
    pub missing: u32,
    /// Consecutive frames with a flat palm, so it has to be held.
    pub open_for: u32,
}

/// How many empty looks before the hand is treated as gone.
///
/// Not one. A detector misses a frame when you turn your wrist, and ending a
/// drag on that would fling whatever you were holding.
pub const GONE_AFTER: u32 = 3;

/// How many consecutive frames a flat palm must be held.
///
/// Roughly the `DELIBERATE_MS` hold at the rate steering looks. Without it,
/// relaxing your hand after a drag summons Atlas — the exact "on the way to"
/// collision the registry warns about, in Atlas's own vocabulary.
pub const OPEN_FRAMES: u32 = 2;

/// Map a hand in the camera frame to a point on the desktop.
///
/// Mirrored horizontally, because the screen shows you facing yourself: your
/// right hand appears on the left of the image and must move the pointer to
/// your right. Getting this backwards makes the whole thing feel broken in a
/// way people struggle to describe.
pub fn to_screen(h: &Hand, width: i32, height: i32) -> (i32, i32) {
    let x = ((1.0 - h.x).clamp(0.0, 1.0) * width as f32) as i32;
    let y = (h.y.clamp(0.0, 1.0) * height as f32) as i32;
    (x.clamp(0, width - 1), y.clamp(0, height - 1))
}

/// How far a pinch may drift and still count as a tap rather than a drag, in
/// pixels.
pub const TAP_SLOP: i32 = 24;

/// Work out what just happened.
///
/// Takes the hand now and what it was doing before, and returns the one move
/// that follows. Pure, so every transition below is testable without a camera
/// and without a desktop.
pub fn what_happened(
    now: Option<Hand>,
    state: &mut Steering,
    width: i32,
    height: i32,
) -> Option<Move> {
    let Some(h) = now else {
        state.missing += 1;
        if state.missing < GONE_AFTER {
            return None;
        }
        // The hand is gone. Anything being carried is put down where it is
        // rather than left held, which would leave the desktop mid-drag.
        let held = state.grabbed_at.take();
        state.was = None;
        state.missing = 0;
        return held.map(|(x, y)| Move::Drop { x, y });
    };
    state.missing = 0;
    let (x, y) = to_screen(&h, width, height);
    let was = state.was.replace(h);

    // A flat palm is the one shape that means the same thing throughout: it
    // summons when nothing is held, and it stops when something is.
    //
    // Held, not instant. A pinch opening back into a flat hand passes through
    // this shape on the way out, so an instant palm would summon Atlas at the
    // end of every single drag. `in_use()` records the hold for the same
    // reason, and `snags` would flag it if anyone removed it.
    if h.open && !h.pinching {
        state.open_for += 1;
        if state.open_for < OPEN_FRAMES {
            return None;
        }
    } else {
        state.open_for = 0;
    }
    if h.open && !h.pinching {
        if state.grabbed_at.take().is_some() {
            return Some(Move::Done);
        }
        return Some(Move::Summon);
    }

    let was_pinching = was.map(|w| w.pinching).unwrap_or(false);
    match (was_pinching, h.pinching) {
        (false, true) => {
            state.grabbed_at = Some((x, y));
            Some(Move::Grab { x, y })
        }
        (true, true) => Some(Move::Drag { x, y }),
        (true, false) => {
            let from = state.grabbed_at.take();
            match from {
                // A pinch that never went anywhere is a click, not a
                // zero-length drag. Told apart here rather than by whatever
                // receives it.
                Some((gx, gy)) if (gx - x).abs() <= TAP_SLOP && (gy - y).abs() <= TAP_SLOP => {
                    Some(Move::Tap { x, y })
                }
                _ => Some(Move::Drop { x, y }),
            }
        }
        (false, false) => Some(Move::Point { x, y }),
    }
}

/// Read a hand out of what the detector printed.
///
/// `hand: <x> <y> <pinch|open|none> <confidence>`
pub fn read_hand(printed: &str, cfg: &GazeConfig) -> Option<Hand> {
    for line in printed.lines() {
        let Some((key, rest)) = line.trim().split_once(':') else {
            continue;
        };
        if !key.trim().eq_ignore_ascii_case("hand") {
            continue;
        }
        let p: Vec<&str> = rest.split_whitespace().collect();
        let x: f32 = p.first()?.parse().ok()?;
        let y: f32 = p.get(1)?.parse().ok()?;
        let shape = p.get(2).copied().unwrap_or("none").to_lowercase();
        let sure: f32 = p.get(3).and_then(|c| c.parse().ok()).unwrap_or(1.0);
        if sure < cfg.min_confidence {
            continue;
        }
        return Some(Hand {
            x,
            y,
            pinching: shape == "pinch",
            open: shape == "open",
            sure,
        });
    }
    None
}

/// How long steering stays on with no hand seen, in seconds.
///
/// It switches itself off. A mode you can leave on by accident is a camera
/// left watching the room because you walked away mid-gesture.
pub const STEERING_STOPS_AFTER: u64 = 30;

// ===========================================================================
// Choosing gestures that won't collide later.
//
// Eric asked whether he could use something like covering his eyes to hide a
// tab, and whether opposites of existing gestures would confuse the model
// later. Both good questions, and the second is the one that actually bites.
//
// The danger is not the detector mixing up two shapes. Detectors are decent at
// telling a fist from a flat palm. The danger is:
//
// **1. Things you already do without meaning them.** Rubbing your eyes,
// scratching your face, resting your chin on your hand, crossing your arms,
// waving at someone walking past. A gesture that collides with one of those
// fires when you are tired, not when you decided something.
//
// **2. Shapes that are a step on the way to another shape.** An open hand
// closing into a fist passes through every stage in between. If "fist" means
// one thing and "open palm" means another, every grab ends by issuing the
// palm command as your hand relaxes. This is the one that looks fine in a demo
// and falls apart in use.
//
// **3. Opposites that are the same shape mirrored or rotated.** Thumb up and
// thumb down are genuinely distinguishable. Swipe left and swipe right are
// distinguishable. Palm-toward-screen and palm-away are *not*, at webcam
// distance, and neither is a hand rotated thirty degrees.
//
// So this is a registry with a check, rather than a fixed list. Eric adds what
// he wants; Atlas says which of the three problems it would run into, if any,
// before it goes in — rather than after a week of it firing at the wrong time.
// ===========================================================================

/// Something a hand or face can do, as a candidate command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sign {
    /// What the detector will call it.
    pub name: &'static str,
    /// What it should do.
    pub means: &'static str,
    /// Roughly what the hand is doing, used to spot collisions.
    pub shape: Shape,
    /// Must be held to count, in milliseconds. Zero means instant.
    pub hold_ms: u32,
}

/// The rough form of a sign, for collision checking only.
///
/// Deliberately coarse. A precise description would be a second model written
/// in an enum, and would be wrong in different ways than the real one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shape {
    /// Fingers spread, palm out.
    OpenHand,
    /// Fingers closed.
    ClosedHand,
    /// Finger and thumb together.
    Pinch,
    /// One finger out.
    Point,
    /// A hand moving across, direction included.
    Sweep { rightwards: bool },
    /// Hand at or over the face.
    AtFace,
    /// Both hands doing it.
    TwoHanded,
}

impl Shape {
    /// Does getting into this shape pass through that one?
    ///
    /// The failure that looks fine in a demo: an open hand closing into a fist
    /// is an open hand for the first few frames. Bind both and every grab
    /// fires the open-hand command on the way in, and again on the way out.
    fn passes_through(self, other: Shape) -> bool {
        matches!(
            (self, other),
            (Shape::ClosedHand, Shape::OpenHand)
                | (Shape::Pinch, Shape::OpenHand)
                | (Shape::Pinch, Shape::Point)
                | (Shape::TwoHanded, Shape::OpenHand)
                | (Shape::Sweep { .. }, Shape::OpenHand)
        )
    }

    /// Do people do this without meaning to?
    ///
    /// The list is short and specific rather than a guess at everything. Each
    /// one is something a person does at a desk several times an hour.
    fn happens_by_accident(self) -> Option<&'static str> {
        match self {
            Shape::AtFace => Some(
                "people rub their eyes, scratch their nose and rest their chin \
                 on their hand all day — this would fire when you're tired \
                 rather than when you decided something",
            ),
            Shape::Point => Some(
                "pointing at your own screen while thinking is common enough \
                 that this would fire on its own",
            ),
            _ => None,
        }
    }
}

/// What is wrong with a proposed sign, if anything.
#[derive(Debug, Clone, PartialEq)]
pub enum Snag {
    /// Two signs, same shape.
    SameAs(&'static str),
    /// Making this one passes through that one.
    OnTheWayTo(&'static str),
    /// People do this without meaning to.
    Accidental(&'static str),
    /// Too close to tell apart at webcam distance.
    LooksTheSame(&'static str),
}

impl Snag {
    /// Said the way you would say it to someone choosing a gesture.
    pub fn plain(&self) -> String {
        match self {
            Snag::SameAs(other) => format!("that's the same shape as {other}"),
            Snag::OnTheWayTo(other) => format!(
                "you pass through {other} on the way into this one, so {other} \
                 would fire every time"
            ),
            Snag::Accidental(why) => (*why).to_string(),
            Snag::LooksTheSame(other) => format!(
                "at webcam distance this and {other} are hard to tell apart — \
                 near enough that it'll get it wrong sometimes, which is worse \
                 than not having it"
            ),
        }
    }

    /// Can it be used anyway, with care?
    ///
    /// Not a refusal. Eric asked not to have things withheld because they look
    /// risky — so a snag is something Atlas says, and a hold time is usually
    /// the fix. Only an outright duplicate genuinely cannot work.
    pub fn fatal(&self) -> bool {
        matches!(self, Snag::SameAs(_))
    }

    /// What would make it work.
    pub fn fix(&self) -> Option<&'static str> {
        match self {
            Snag::SameAs(_) => None,
            Snag::OnTheWayTo(_) | Snag::Accidental(_) => {
                Some("hold it for about a second and it stops firing by accident")
            }
            Snag::LooksTheSame(_) => Some("use two hands for one of them"),
        }
    }
}

/// How long a hold has to be to count as deliberate.
///
/// Below about three quarters of a second people do it by accident; above
/// about a second and a half it feels like the thing is broken.
pub const DELIBERATE_MS: u32 = 900;

/// Check a proposed sign against the ones already in use.
pub fn snags(new: &Sign, existing: &[Sign]) -> Vec<Snag> {
    let mut out = Vec::new();

    if let Some(why) = new.shape.happens_by_accident() {
        if new.hold_ms < DELIBERATE_MS {
            out.push(Snag::Accidental(why));
        }
    }

    for old in existing {
        if old.name == new.name {
            continue;
        }
        if old.shape == new.shape {
            out.push(Snag::SameAs(old.means));
            continue;
        }
        // Only a problem when the one you pass through is instant. If it has
        // to be held, passing through it briefly does not trigger it.
        if new.shape.passes_through(old.shape) && old.hold_ms < DELIBERATE_MS {
            out.push(Snag::OnTheWayTo(old.means));
        }
        if hard_to_tell_apart(new.shape, old.shape) {
            out.push(Snag::LooksTheSame(old.means));
        }
    }
    out
}

/// Shapes a webcam struggles to separate.
///
/// Sweeps in opposite directions are fine — movement is the easiest thing to
/// read. Static hands that differ only by rotation are not.
fn hard_to_tell_apart(a: Shape, b: Shape) -> bool {
    matches!(
        (a, b),
        (Shape::OpenHand, Shape::Point) | (Shape::Point, Shape::OpenHand)
    )
}

/// What Atlas says when you propose a gesture.
///
/// Says yes, or says what would go wrong and how to fix it. Never just no —
/// the point is to choose a working gesture, not to be refused one.
pub fn verdict(new: &Sign, existing: &[Sign]) -> String {
    let found = snags(new, existing);
    if found.is_empty() {
        return format!("{} for {} — that'll work.", new.name, new.means);
    }
    let mut out = String::new();
    for s in &found {
        out.push_str(&s.plain());
        match (s.fatal(), s.fix()) {
            (true, _) => out.push_str(". You'd need a different shape for one of them"),
            (false, Some(fix)) => {
                out.push_str(&format!(", but {fix}"));
            }
            _ => {}
        }
        out.push_str(". ");
    }
    out.trim_end().to_string()
}

/// The signs currently in use.
///
/// Kept here so `snags` has something to check against and so there is one
/// place to look when a new one misbehaves.
pub fn in_use() -> Vec<Sign> {
    vec![
        Sign {
            name: "pinch",
            means: "picking something up",
            shape: Shape::Pinch,
            hold_ms: 0,
        },
        Sign {
            name: "open palm",
            means: "bringing Atlas up, or stopping",
            shape: Shape::OpenHand,
            // Held, precisely because a pinch passes through an open hand on
            // the way out. Without this, every drop would summon Atlas.
            hold_ms: DELIBERATE_MS,
        },
    ]
}
