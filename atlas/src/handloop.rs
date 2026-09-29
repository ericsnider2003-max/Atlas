//! Hand tracking on its own thread.
//!
//! ## Why this has to exist
//!
//! Every other capability in Atlas is checked once per daemon tick, and that
//! tick sleeps for up to two seconds. That discipline is right for almost
//! everything: a kin signal, a page request, a link handed over from a phone
//! can all wait two seconds and nobody notices.
//!
//! A pointer cannot. Two seconds of latency is not a slow pointer, it is a
//! broken one, and no amount of smoothing or prediction rescues it —
//! prediction covers a hundred milliseconds, not two thousand. Everything
//! built in `handtrack` and `handshape` is correct and would still have felt
//! unusable, because it was queued behind the same loop as everything else.
//!
//! So this is the first thing in Atlas with its own thread, and the shape of
//! it is deliberately the shape the worker model will need later: a loop that
//! owns its work, paces itself, and reports back over a channel rather than
//! sharing state.
//!
//! ## Nothing is shared
//!
//! The thread does not borrow the daemon's platform. It builds its own — the
//! real platforms hold no state for pointer work, so there is nothing to
//! share and therefore nothing to lock. A mutex on the pointer path would put
//! a contended lock in the hottest loop in the program.
//!
//! What comes back is only what the daemon genuinely needs to know: a gesture
//! that fired, or a complaint that the machine cannot keep up. The pointer
//! itself never crosses the channel, because it is already where it needs to
//! be by the time the daemon would have read it.

use crate::error::Result;
use crate::platform::{Button, PixelRect, WindowId};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

/// The bit of the platform hand tracking needs, and nothing else.
///
/// Narrow on purpose. `Platform` is a large trait with interior mutability in
/// its test double, so it cannot cross a thread; this is the subset that can,
/// and keeping it small is also what stops the tracking thread growing the
/// ability to do things nobody expected it to.
pub trait Pointer: Send + Sync {
    fn move_cursor(&self, x: i32, y: i32) -> Result<()>;
    fn click(&self, x: i32, y: i32, button: Button) -> Result<()>;
    fn window_at(&self, x: i32, y: i32) -> Result<Option<WindowId>>;
    fn rect_of(&self, win: WindowId) -> Result<PixelRect>;
    fn place(&self, win: WindowId, rect: PixelRect) -> Result<()>;
    fn draw_overlay(&self, elements: &[crate::overlay::Element]) -> Result<()>;
    /// The primary monitor, so hand positions can be mapped to pixels.
    fn screen(&self) -> (i32, i32);
}

/// What the tracking thread tells the daemon about.
#[derive(Debug, Clone, PartialEq)]
pub enum Said {
    /// A gesture fired, and what it was bound to.
    Did(String),
    /// Something worth saying out loud.
    Trouble(String),
    /// The hand left, so anything counting on it should stop.
    HandsGone,
}

/// A running tracker.
pub struct Tracking {
    stop: Arc<AtomicBool>,
    heard: Receiver<Said>,
    joined: Option<std::thread::JoinHandle<()>>,
}

impl Tracking {
    /// Anything the thread has said since last asked.
    ///
    /// Never blocks. The daemon calls this once a tick like everything else —
    /// the point is that the *pointer* does not wait for the tick, not that
    /// the reporting does not.
    pub fn heard(&self) -> Vec<Said> {
        self.heard.try_iter().collect()
    }

    /// Ask it to stop, and wait for it.
    ///
    /// Waits rather than detaching, because a tracking thread that outlives
    /// the request would keep moving the pointer after Eric said stop, which
    /// is the single worst way this feature could fail.
    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.joined.take() {
            let _ = t.join();
        }
    }

    pub fn running(&self) -> bool {
        self.joined.is_some() && !self.stop.load(Ordering::Relaxed)
    }
}

impl Drop for Tracking {
    fn drop(&mut self) {
        // Same reason. A dropped handle must not leave a thread driving the
        // mouse.
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.joined.take() {
            let _ = t.join();
        }
    }
}

/// Where the frames and the readings come from.
///
/// A trait so the loop can be tested without a camera or a model — the
/// scheduling, pacing and stop behaviour are the parts most likely to be
/// wrong, and none of them need either.
pub trait Eyes: Send {
    /// One reading, or `None` when no hand was visible.
    ///
    /// Blocking is fine; this is the thread's own time. How long it takes is
    /// measured and fed to the pace.
    fn look(&mut self) -> Option<crate::handshape::Landmarks>;
}

/// The real eyes: a camera, and models to read what it sees.
///
/// Owned entirely by the tracking thread. The camera opens when this is built
/// and closes when the thread ends, so there is no window in which a webcam is
/// lit and nothing is looking through it.
pub struct Seeing {
    rolling: crate::frames::Rolling,
    finding: crate::infer::Model,
    reading: crate::infer::Model,
    /// Said once if the camera dies, rather than reporting no hands forever —
    /// which looks exactly like sitting still.
    pub camera_died: bool,
}

impl Seeing {
    pub fn start(feed: &crate::frames::Feed, models: &std::path::Path) -> crate::error::Result<Seeing> {
        Ok(Seeing {
            rolling: crate::frames::Rolling::start(feed)?,
            finding: crate::infer::Model::load(crate::infer::Kind::HandPresence, models)?,
            reading: crate::infer::Model::load(crate::infer::Kind::HandLandmarks, models)?,
            camera_died: false,
        })
    }
}

impl Eyes for Seeing {
    fn look(&mut self) -> Option<crate::handshape::Landmarks> {
        if self.camera_died {
            return None;
        }
        let (w, h) = self.rolling.size();
        let frame = match self.rolling.next() {
            Some(f) => f.to_vec(),
            None => {
                self.camera_died = self.rolling.ended();
                return None;
            }
        };

        // Where are the hands? Asked first because it is the cheaper of the
        // two models, and running the expensive one on an empty room is the
        // commonest way this kind of pipeline wastes a laptop.
        //
        // Three things here were wrong before any weights existed to run
        // against, and all three would have looked like unreliable tracking
        // rather than like bugs:
        //
        // - The old code read result number *zero* and compared it to a half.
        //   Result zero is the box measurements, not the score; the scores are
        //   result number one. So whether a hand was "there" came down to an
        //   arbitrary coordinate offset.
        // - Nothing cropped. The landmark model reads *a hand*, not a room
        //   with a hand somewhere in it, so it has to be given the box the
        //   first model found.
        // - The result comes back in pixels of the model's own input, and
        //   everything downstream multiplies it by the width of the screen.
        //   Uncorrected, the first sighting would have thrown the pointer
        //   several thousand pixels off the display and left it there.
        let (fw, fh) = crate::infer::Kind::HandPresence.wants();
        let small = crate::infer::fit(&frame, w, h, fw, fh);
        let found = self.finding.run(&small).ok()?;
        let hand = where_the_hand_is(&found)?;

        // The crop, widened: the palm box stops at the wrist, and fingers are
        // the part being read.
        let (cx, cy, cw, ch) = hand.in_pixels(w, h, HAND_MARGIN);
        if cw == 0 || ch == 0 {
            return None;
        }
        let bigger =
            crate::infer::prepare_crop(&frame, w, h, (cx, cy, cw, ch), crate::infer::Kind::HandLandmarks);
        let out = self.reading.run(&bigger).ok()?;

        // Result zero is the joints; result one is how sure the model is that
        // this was a hand at all. Reading only the first is how every reading
        // came back perfectly certain.
        let joints = out.at(0).ok()?;
        let sure = out.at(1).ok().and_then(|s| s.first().copied()).unwrap_or(1.0);
        let (rw, rh) = crate::infer::Kind::HandLandmarks.wants();
        crate::handshape::from_model(&crate::handshape::in_the_frame(
            joints,
            sure,
            (rw as f32, rh as f32),
            (cx as f32 / w as f32, cy as f32 / h as f32),
            (cw as f32 / w as f32, ch as f32 / h as f32),
        ))
    }
}

/// How much wider than the palm box to cut, so the fingers are in the picture.
const HAND_MARGIN: f32 = 0.5;

/// Anything below this is not a hand.
const HAND_FLOOR: f32 = 0.6;

/// The grids the hand-finding model reports on: a coarse pass and a fine one.
///
/// Two thousand and sixteen candidates in total — twenty-four by twenty-four
/// cells with two guesses each, then twelve by twelve with six. Stated here
/// rather than inferred, so a model with a different layout is caught as a
/// mismatch instead of read as noise.
const HAND_GRIDS: [(usize, usize); 2] = [(24, 2), (12, 6)];

/// The strongest hand the finding model saw, as fractions of the frame.
///
/// Deliberately one hand, not all of them: everything downstream — the
/// pointer, the gestures, the drag — is written around a single hand, and
/// handing back two would mean the strongest one silently winning somewhere
/// further along instead of here where it can be seen.
pub fn where_the_hand_is(out: &crate::infer::Outputs) -> Option<crate::vision::Patch> {
    let boxes = out.at(0).ok()?;
    let scores = out.at(1).ok()?;
    let (w, h) = crate::infer::Kind::HandPresence.wants();
    let anchors = hand_anchors();
    if scores.len() < anchors.len() || boxes.len() < anchors.len() * 18 {
        return None;
    }
    let mut best: Option<(f32, crate::vision::Patch)> = None;
    for (i, (ax, ay)) in anchors.iter().enumerate() {
        // The model reports how strongly it believes, not a fraction. This is
        // the step that turns one into the other.
        let sure = 1.0 / (1.0 + (-scores[i]).exp());
        if !sure.is_finite() || sure < HAND_FLOOR {
            continue;
        }
        let at = i * 18;
        let cx = boxes[at] / w as f32 + ax;
        let cy = boxes[at + 1] / h as f32 + ay;
        let bw = boxes[at + 2] / w as f32;
        let bh = boxes[at + 3] / h as f32;
        if !(cx.is_finite() && cy.is_finite() && bw > 0.0 && bh > 0.0) {
            continue;
        }
        let patch = crate::vision::Patch::new(cx - bw / 2.0, cy - bh / 2.0, bw, bh);
        if best.as_ref().is_none_or(|(s, _)| sure > *s) {
            best = Some((sure, patch));
        }
    }
    best.map(|(_, p)| p)
}

/// Where each of the model's candidates sits in the picture.
fn hand_anchors() -> Vec<(f32, f32)> {
    let mut out = Vec::new();
    for (cells, per_cell) in HAND_GRIDS {
        for y in 0..cells {
            for x in 0..cells {
                let cx = (x as f32 + 0.5) / cells as f32;
                let cy = (y as f32 + 0.5) / cells as f32;
                for _ in 0..per_cell {
                    out.push((cx, cy));
                }
            }
        }
    }
    out
}

/// Everything the loop needs to run.
pub struct Setup {
    pub eyes: Box<dyn Eyes>,
    pub pointer: Box<dyn Pointer>,
    pub vocabulary: crate::handshape::Vocabulary,
    pub smoothing: crate::handtrack::SmoothConfig,
    pub pace: crate::handtrack::PaceConfig,
}

/// Start tracking.
pub fn start(setup: Setup) -> Tracking {
    let stop = Arc::new(AtomicBool::new(false));
    let (say, heard) = std::sync::mpsc::channel();
    let mine = stop.clone();
    let joined = std::thread::Builder::new()
        .name("atlas-hands".into())
        .spawn(move || run(setup, mine, say))
        .ok();
    Tracking { stop, heard, joined }
}

fn run(mut setup: Setup, stop: Arc<AtomicBool>, say: Sender<Said>) {
    use crate::gaze::{what_happened, Hand, Steering};
    use crate::handtrack::{Pace, Track};

    let mut track = Track::default();
    let mut pace = Pace::default();
    let mut steering = Steering::default();
    // Velocity is paid for only when something bound reads it. A vocabulary
    // of static shapes tracks no motion at all — `any_motion` exists for
    // exactly this and nothing asked it.
    let wants_motion = setup.vocabulary.any_motion();
    let mut trail = crate::handshape::Trail::default();
    let mut holds: Vec<crate::handshape::Holding> =
        vec![Default::default(); setup.vocabulary.gestures.len()];
    let mut carrying: Option<(WindowId, PixelRect)> = None;
    let (w, h) = setup.pointer.screen();
    let began = std::time::Instant::now();
    let mut complained = false;

    while !stop.load(Ordering::Relaxed) {
        let now_ms = began.elapsed().as_millis().min(u32::MAX as u128) as u32;
        let looked = std::time::Instant::now();
        let seen = setup.eyes.look();
        pace.took(looked.elapsed().as_millis().min(u32::MAX as u128) as u32);

        // Said once, not every frame. A loop that complains twenty times a
        // second about being slow is itself the problem.
        if !complained {
            if let Some(trouble) = pace.keeping_up(&setup.pace) {
                let _ = say.send(Said::Trouble(trouble));
                complained = true;
            }
        }

        let hand = match seen {
            Some(marks) => {
                let (x, y) = track.saw(marks.points[8].x, marks.points[8].y, now_ms, &setup.smoothing);
                // Velocity is only measured when a bound gesture reads it.
                // Everything else gets the default motion, which is exactly
                // what every gesture got before the trail existed.
                let motion = if wants_motion {
                    trail.saw(marks.points[8].x, marks.points[8].y, now_ms)
                } else {
                    crate::handshape::Motion::default()
                };
                // One reading for the whole vocabulary. The old loop built a
                // fresh `Reading` per gesture, which re-measured the same
                // hand as many times as there were bindings.
                let mut r = crate::handshape::Reading::moving(&marks, motion);
                let pinching = r.pinch() < 0.45;
                let open = r.spread() > 1.1 && !pinching;

                // Only gestures bound to something are ever evaluated, and
                // first match wins — the order `recognise`'s own doc
                // promises. The hold clocks stay in the loop even for
                // hold-free vocabularies: `Holding` is also the debounce
                // that stops a shape firing twenty times a second while it
                // is held up.
                let matched = setup.vocabulary.matched(&mut r);
                for (i, g) in setup.vocabulary.gestures.iter().enumerate() {
                    if holds[i].seen(matched == Some(i), now_ms, g.hold_ms)
                        == crate::handshape::Progress::Done
                    {
                        let _ = say.send(Said::Did(g.does.clone()));
                    }
                }
                Some(Hand { x, y, pinching, open, sure: marks.sure })
            }
            None => {
                track.lost();
                trail.lost();
                None
            }
        };

        if let Some(what) = what_happened(hand, &mut steering, w, h) {
            apply(&*setup.pointer, what, &mut carrying, &say);
        }

        // Between readings, keep the pointer moving. This is what makes a
        // detector running at fifteen a second feel immediate — and it is why
        // this loop cannot live in the daemon's tick, where the gap would be
        // two seconds rather than sixty milliseconds.
        let wait = pace.wait_ms(&setup.pace).max(1);
        let step = 16u32.min(wait);
        let mut waited = 0;
        while waited < wait && !stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(step as u64));
            waited += step;
            let at = began.elapsed().as_millis().min(u32::MAX as u128) as u32;
            if let Some((px, py)) = track.where_now(at) {
                let _ = setup
                    .pointer
                    .move_cursor((px * w as f32) as i32, (py * h as f32) as i32);
            }
        }
    }

    // Put down anything being carried. A thread that stops mid-drag leaves a
    // window stuck to the pointer.
    if carrying.take().is_some() {
        let _ = say.send(Said::HandsGone);
    }
    let _ = setup.pointer.draw_overlay(&[]);
}

fn apply(
    pointer: &dyn Pointer,
    what: crate::gaze::Move,
    carrying: &mut Option<(WindowId, PixelRect)>,
    say: &Sender<Said>,
) {
    use crate::gaze::Move;
    match what {
        Move::Point { x, y } => {
            let _ = pointer.move_cursor(x, y);
            outline(pointer, x, y, false);
        }
        Move::Grab { x, y } => {
            let _ = pointer.move_cursor(x, y);
            *carrying = pointer
                .window_at(x, y)
                .ok()
                .flatten()
                .and_then(|id| pointer.rect_of(id).ok().map(|r| (id, r)));
            outline(pointer, x, y, true);
        }
        Move::Drag { x, y } => {
            let _ = pointer.move_cursor(x, y);
            if let Some((id, from)) = carrying {
                let _ = pointer.place(
                    *id,
                    PixelRect {
                        x: x - from.width / 2,
                        y: y - 20,
                        width: from.width,
                        height: from.height,
                    },
                );
            }
        }
        Move::Drop { x, y } => {
            *carrying = None;
            outline(pointer, x, y, false);
        }
        Move::Tap { x, y } => {
            let _ = pointer.move_cursor(x, y);
            let _ = pointer.click(x, y, Button::Left);
        }
        Move::Resize { .. } => {}
        Move::Summon => {
            let _ = say.send(Said::Did("bring Atlas up".into()));
        }
        Move::Done => {
            *carrying = None;
            let _ = pointer.draw_overlay(&[]);
            let _ = say.send(Said::HandsGone);
        }
    }
}

fn outline(pointer: &dyn Pointer, x: i32, y: i32, holding: bool) {
    match pointer.window_at(x, y).ok().flatten() {
        Some(id) => match pointer.rect_of(id) {
            Ok(r) => {
                let e = crate::overlay::around((r.x, r.y, r.width, r.height), holding);
                let _ = pointer.draw_overlay(&[e]);
            }
            Err(_) => {
                let _ = pointer.draw_overlay(&[]);
            }
        },
        // Nothing under the hand. Cleared rather than left ringing whatever it
        // was last over, which would be a lie about where you are.
        None => {
            let _ = pointer.draw_overlay(&[]);
        }
    }
}

/// Watch a shape being held until there are enough readings to tell what it
/// is, or `most_frames` have gone by (Eric's ruling H6: teaching a gesture).
pub fn learn_a_shape(eyes: &mut dyn Eyes, most_frames: usize) -> crate::handshape::Learning {
    let mut l = crate::handshape::Learning::default();
    let mut looked = 0;
    while !l.enough() && looked < most_frames {
        if let Some(h) = eyes.look() {
            l.watch(&h);
        }
        looked += 1;
    }
    l
}

/// The shape you held, worked out as a gesture called `name` that does
/// `does`, or why it couldn't be.
pub fn shape_shown(eyes: &mut dyn Eyes, name: &str, does: &str, most_frames: usize) -> std::result::Result<crate::handshape::Gesture, String> {
    let l = learn_a_shape(eyes, most_frames);
    l.worked_out(name, does).ok_or_else(|| l.why_not())
}
