//! Seeing, rather than reading.
//!
//! ## What was missing
//!
//! Until now Atlas's only eyes were `ocr` — it could read the *words* in a
//! picture and nothing else. A chart was a picture with no words in it. A
//! face was a picture with no words in it. A mug on the desk, a thing being
//! pointed at, a photo with no caption: all of them came back as "the writing
//! in it is too unclear for me to read honestly", which is a true sentence
//! about the wrong question.
//!
//! This module answers the other question: not what does it *say*, but what
//! *is* it.
//!
//! ## Local, and why that was never really a choice
//!
//! The alternative was a hosted vision model, and Eric's standing rule rules
//! it out on its own — as offline and self-built as possible, no
//! subscriptions, third-party services are concepts to copy rather than
//! dependencies to adopt. But the numbers say the same thing without needing
//! the rule: the four model files together are well under a tenth of a
//! gigabyte, against the spare memory `fit` actually measures on his laptop.
//! Sending his camera to somebody else's computer would cost more, do less,
//! stop working on a train, and drag the whole personal/business boundary
//! question along behind it.
//!
//! Everything here runs inside `atlas.exe` through `infer`. No second
//! process, no Python, no account.
//!
//! ## The four honesty rules
//!
//! Each of these is a way this feature could look like it works and not.
//!
//! 1. **An unread camera is not an empty room.** `Sight::Unread` exists so
//!    "I couldn't look" can never be reported as "there's nothing there".
//!    The same rule `gaze` already turns on.
//! 2. **Below the floor, say nothing rather than guess.** A thing Atlas is
//!    not sure about is left out of the list, not listed with a low number
//!    nobody reads.
//! 3. **A name carries how sure it is.** Above the higher bar Atlas says what
//!    it is; below, it says what it *thinks* it is; the same sentence with
//!    and without a hedge is the difference between useful and misleading.
//! 4. **A face is not a password.** Recognising a face never authorises
//!    anything, for the reason `gaze` already gives: a photograph held up to
//!    a webcam has no honest defence at this layer.
//!
//! ## What it is not
//!
//! It does not describe a scene in a sentence, reason about what is
//! happening, or read a chart's meaning. It finds faces, finds and names
//! things from a fixed list, describes a whole picture, and recognises
//! anything Eric has shown it and named. A model that talks about pictures is
//! several gigabytes and a different decision.

use crate::error::{AtlasError, Result};
use crate::infer::{Kind, Model, Outputs};
use crate::store::Store;
use serde::{Deserialize, Serialize};

/// The names of the things the object model can find, in its own order.
///
/// Compiled in rather than read from disk: a label list is part of the model,
/// not part of the configuration, and a missing file would mean confidently
/// wrong names rather than an error.
const OBJECT_NAMES: &str = include_str!("../config/labels/objects.txt");

/// The names the picture model knows.
const PICTURE_NAMES: &str = include_str!("../config/labels/picture.txt");

// ---------------------------------------------------------------------------
// Where something is
// ---------------------------------------------------------------------------

/// A box in a picture, as fractions of the whole.
///
/// Fractions rather than pixels so a reading survives the frame being resized
/// — and so nothing downstream has to remember which of the several sizes in
/// this pipeline a number was measured against.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Patch {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Patch {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Patch {
        Patch { x, y, width, height }
    }

    pub fn holds(&self, x: f32, y: f32) -> bool {
        x >= self.x && x <= self.x + self.width && y >= self.y && y <= self.y + self.height
    }

    /// How much of the picture this takes up.
    pub fn size(&self) -> f32 {
        (self.width * self.height).max(0.0)
    }

    /// How much two boxes share, as a fraction of what they cover together.
    pub fn overlap(&self, other: &Patch) -> f32 {
        let x = (self.x.max(other.x)).max(0.0);
        let y = (self.y.max(other.y)).max(0.0);
        let r = (self.x + self.width).min(other.x + other.width);
        let b = (self.y + self.height).min(other.y + other.height);
        if r <= x || b <= y {
            return 0.0;
        }
        let both = (r - x) * (b - y);
        let all = self.size() + other.size() - both;
        if all <= 0.0 {
            0.0
        } else {
            both / all
        }
    }

    /// The same box in pixels of a frame this size, clamped to it.
    ///
    /// Widened a little, because a detector's box is usually tight around a
    /// face and the recogniser was trained on a slightly wider crop.
    pub fn in_pixels(&self, w: usize, h: usize, margin: f32) -> (usize, usize, usize, usize) {
        let grow_w = self.width * margin;
        let grow_h = self.height * margin;
        let x0 = ((self.x - grow_w).max(0.0) * w as f32) as usize;
        let y0 = ((self.y - grow_h).max(0.0) * h as f32) as usize;
        let x1 = (((self.x + self.width + grow_w).min(1.0)) * w as f32) as usize;
        let y1 = (((self.y + self.height + grow_h).min(1.0)) * h as f32) as usize;
        (x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0))
    }

    fn sane(&self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
            && self.width > 0.0
            && self.height > 0.0
    }
}

// ---------------------------------------------------------------------------
// What was found
// ---------------------------------------------------------------------------

/// One thing Atlas found and can name.
#[derive(Debug, Clone, PartialEq)]
pub struct Object {
    pub name: String,
    pub area: Patch,
    pub sure: f32,
}

impl Object {
    /// Said out loud, hedged when it should be.
    ///
    /// The hedge is the whole point. "A coffee mug" and "what looks like a
    /// coffee mug" are the same finding with the same number behind it, and
    /// only one of them lets you judge whether to believe it.
    pub fn spoken(&self, sure_enough: f32) -> String {
        if self.sure >= sure_enough {
            format!("a {}", self.name)
        } else {
            format!("what looks like a {}", self.name)
        }
    }
}

/// A face, and possibly whose.
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    pub area: Patch,
    pub sure: f32,
    /// Who it is, when Atlas has been shown them and is sure enough.
    pub who: Option<String>,
    /// How sure of *that*, which is a different and higher bar than being
    /// sure there is a face at all.
    pub sure_who: f32,
}

/// Everything one look found.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Scene {
    pub faces: Vec<Face>,
    pub things: Vec<Object>,
    /// What the whole picture looks like, when nothing more specific was
    /// found — or as the answer to "what is this a picture of".
    pub whole: Option<Object>,
    /// Anything that went wrong on the way, in words. Never empty *and*
    /// silent: a look that half-failed says so.
    pub could_not: Vec<String>,
    /// A hand answering a question ("thumb_up", how sure), on a look made
    /// for one.
    pub gesture: Option<(String, f32)>,
}

impl Scene {
    pub fn saw_anything(&self) -> bool {
        !self.faces.is_empty() || !self.things.is_empty() || self.whole.is_some()
    }

    /// Is one of them his?
    fn you_are_here(&self, you: &str) -> Option<&Face> {
        self.faces
            .iter()
            .find(|f| f.who.as_deref().is_some_and(|w| w.eq_ignore_ascii_case(you)))
    }

    /// The most specific thing at a point.
    ///
    /// Smallest box wins, not first or highest-scoring. A hand pointing at a
    /// keyboard is inside the box for the desk, the monitor and the keyboard
    /// at once, and the smallest of those is the one being pointed at.
    pub fn at(&self, x: f32, y: f32) -> Option<&Object> {
        self.things
            .iter()
            .filter(|t| t.area.holds(x, y))
            .min_by(|a, b| {
                a.area
                    .size()
                    .partial_cmp(&b.area.size())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    }

    /// Said out loud.
    pub fn spoken(&self, cfg: &VisionConfig) -> String {
        let mut parts: Vec<String> = Vec::new();
        match self.faces.len() {
            0 => {}
            1 => {
                let f = &self.faces[0];
                parts.push(match f.who.as_deref() {
                    Some(who) => who.to_string(),
                    None => "someone I don't recognise".into(),
                });
            }
            n => {
                let named: Vec<&str> =
                    self.faces.iter().filter_map(|f| f.who.as_deref()).collect();
                if named.is_empty() {
                    parts.push(format!("{n} people"));
                } else {
                    parts.push(format!("{n} people, including {}", named.join(" and ")));
                }
            }
        }
        let mut named: Vec<String> = self
            .things
            .iter()
            .take(cfg.most_things_said)
            .map(|t| t.spoken(cfg.sure_enough_to_say_plainly))
            .collect();
        parts.append(&mut named);

        if parts.is_empty() {
            return match &self.whole {
                Some(w) => format!("It looks like {}.", w.spoken(cfg.sure_enough_to_say_plainly)),
                // Not a failure, and not dressed up as one. Nothing was found
                // and the camera worked.
                None => "I looked, and there's nothing I can name.".into(),
            };
        }
        format!("I can see {}.", plainly_joined(&parts))
    }

    /// The same reading, in the lines a detector prints.
    ///
    /// `gaze` was written to read a detector's output rather than to *be* one,
    /// so that any detector could be plugged in. That was the right shape and
    /// it left one problem: under Eric's offline rule there was never going to
    /// be a third-party detector to plug in, so `gaze_detector` stayed
    /// commented out in `tools.yaml` and everything downstream of it —
    /// presence, discretion, gestures answering a question — has never once
    /// run on real input.
    ///
    /// This closes that loop without changing `gaze`: Atlas prints the lines
    /// itself.
    pub fn as_lines(&self, you: &str) -> String {
        let mut out = Vec::new();
        for c in &self.could_not {
            out.push(format!("could_not: {c}"));
        }
        // Confidence for "how many faces" is the *weakest* of them, not the
        // strongest. Reporting three faces with the confidence of the clearest
        // one would claim certainty about the two Atlas is least sure of.
        if let Some(weakest) = self
            .faces
            .iter()
            .map(|f| f.sure)
            .fold(None::<f32>, |acc, s| Some(acc.map_or(s, |a: f32| a.min(s))))
        {
            out.push(format!("faces: {} {:.3}", self.faces.len(), weakest));
        } else if self.could_not.is_empty() {
            // Nothing found, and the look worked: that is a real reading of
            // zero, which is what tells `presence` the room is empty.
            out.push("faces: 0 1.000".to_string());
        }
        if let Some((g, sure)) = &self.gesture {
            out.push(format!("gesture: {g} {sure:.3}"));
        }
        if let Some(f) = self.you_are_here(you) {
            out.push(format!("you: yes {:.3}", f.sure_who));
        } else if !self.faces.is_empty() {
            // Someone is there and it is not him — worth saying, and at the
            // confidence of *finding* the face rather than of naming it.
            let sure = self.faces.iter().map(|f| f.sure).fold(0.0f32, f32::max);
            out.push(format!("you: no {sure:.3}"));
        }
        out.join("\n")
    }
}

/// A look, or an explanation of why there wasn't one.
///
/// The distinction the whole module turns on. One of these means "the room is
/// empty" and the other means "you have no idea", and a type that can hold
/// only the first is how the second gets reported as the first.
#[derive(Debug, Clone, PartialEq)]
pub enum Sight {
    /// Atlas looked. What it found may still be nothing.
    Looked(Scene),
    /// Atlas did not look, and this is why — in words that name the thing to
    /// fix.
    Unread(String),
}

impl Sight {
    pub fn scene(&self) -> Option<&Scene> {
        match self {
            Sight::Looked(s) => Some(s),
            Sight::Unread(_) => None,
        }
    }

    pub fn spoken(&self, cfg: &VisionConfig) -> String {
        match self {
            Sight::Looked(s) => s.spoken(cfg),
            Sight::Unread(why) => format!("I couldn't look — {why}."),
        }
    }

    /// The detector lines, or the failure in the vocabulary `gaze` reads.
    pub fn as_lines(&self, you: &str) -> String {
        match self {
            Sight::Looked(s) => s.as_lines(you),
            Sight::Unread(why) => format!("could_not: {why}"),
        }
    }
}

fn plainly_joined(parts: &[String]) -> String {
    match parts.len() {
        0 => String::new(),
        1 => parts[0].clone(),
        2 => format!("{} and {}", parts[0], parts[1]),
        _ => {
            let (last, rest) = parts.split_last().expect("checked above");
            format!("{}, and {}", rest.join(", "), last)
        }
    }
}

// ---------------------------------------------------------------------------
// Settings
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
#[serde(default)]
pub struct VisionConfig {
    /// Off unless turned on, for the same reason `gaze` is: a camera that
    /// starts looking because you updated is not something anyone should have
    /// to discover.
    pub enabled: bool,
    /// Below this a finding is left out entirely rather than reported weakly.
    pub floor: f32,
    /// Above this Atlas says what a thing is; below it, what it looks like.
    pub sure_enough_to_say_plainly: f32,
    /// The bar for saying a picture is one of the things you named.
    pub sure_enough_to_name_a_thing: f32,
    /// The separate, higher bar for putting a name to a face.
    ///
    /// One number cannot mean both "there is a face here" and "this is that
    /// particular person", and using one for both is how a stranger becomes a
    /// session. The same split `gaze` already makes.
    pub sure_enough_to_name_a_face: f32,
    /// How far ahead of the runner-up a match has to be.
    ///
    /// Without this, two people who look somewhat alike both score well and
    /// the higher one wins by a hair — reported as certainty. With it, a
    /// close call is reported as not knowing.
    pub margin: f32,
    /// Boxes overlapping more than this are the same thing found twice.
    pub same_thing: f32,
    /// How many things to name in one spoken answer.
    pub most_things_said: usize,
    /// How much wider than the detected box to cut a face for recognising.
    pub face_margin: f32,
    /// Which name in the album means *you*.
    ///
    /// A name rather than a setting to fill in: say "this is me" to the camera
    /// once and the album has it. Anything else in there is somebody else, and
    /// `presence` is told the difference.
    pub your_face: String,
}

impl Default for VisionConfig {
    fn default() -> Self {
        VisionConfig {
            enabled: false,
            floor: 0.4,
            sure_enough_to_say_plainly: 0.7,
            sure_enough_to_name_a_thing: 0.75,
            sure_enough_to_name_a_face: 0.5,
            margin: 0.06,
            same_thing: 0.45,
            most_things_said: 4,
            face_margin: 0.15,
            your_face: "me".into(),
        }
    }
}

// ---------------------------------------------------------------------------
// Fitting a frame to a model without distorting it
// ---------------------------------------------------------------------------

/// Where a frame ended up inside a square model input.
///
/// A camera frame is wider than it is tall and a detector's input is square.
/// Stretching one to the other squashes everything vertically, which a
/// detector tolerates but does not enjoy — and, more importantly, it moves
/// where things are. So the frame is scaled to fit and the rest padded, and
/// this records enough to undo that when reading the boxes back out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
    pub scale: f32,
    pub pad_x: f32,
    pub pad_y: f32,
    pub frame_w: usize,
    pub frame_h: usize,
    pub into_w: usize,
    pub into_h: usize,
}

impl Framing {
    pub fn work_out(frame_w: usize, frame_h: usize, into_w: usize, into_h: usize) -> Framing {
        let scale = if frame_w == 0 || frame_h == 0 {
            1.0
        } else {
            (into_w as f32 / frame_w as f32).min(into_h as f32 / frame_h as f32)
        };
        let used_w = frame_w as f32 * scale;
        let used_h = frame_h as f32 * scale;
        Framing {
            scale,
            pad_x: (into_w as f32 - used_w) / 2.0,
            pad_y: (into_h as f32 - used_h) / 2.0,
            frame_w,
            frame_h,
            into_w,
            into_h,
        }
    }

    /// A box in model pixels, back to fractions of the original frame.
    pub fn back(&self, x: f32, y: f32, w: f32, h: f32) -> Patch {
        if self.scale <= 0.0 || self.frame_w == 0 || self.frame_h == 0 {
            return Patch::new(0.0, 0.0, 0.0, 0.0);
        }
        Patch::new(
            (x - self.pad_x) / self.scale / self.frame_w as f32,
            (y - self.pad_y) / self.scale / self.frame_h as f32,
            w / self.scale / self.frame_w as f32,
            h / self.scale / self.frame_h as f32,
        )
    }
}

/// Scale a frame to fit a model's square input, padding the rest grey.
///
/// Grey rather than black on purpose: black is a colour things in the picture
/// can be, and a black border reads as part of the scene.
pub fn letterbox(rgb: &[u8], frame_w: usize, frame_h: usize, kind: Kind) -> (Vec<f32>, Framing) {
    let r = kind.recipe();
    let placed = Framing::work_out(frame_w, frame_h, r.width, r.height);
    let mut canvas = vec![114u8; r.width * r.height * 3];
    if frame_w > 0 && frame_h > 0 && placed.scale > 0.0 {
        let used_w = (frame_w as f32 * placed.scale).round() as usize;
        let used_h = (frame_h as f32 * placed.scale).round() as usize;
        for y in 0..used_h.min(r.height) {
            // The middle of each source region, for the same reason `fit`
            // does it: sampling the corner shifts the whole picture.
            let sy = (((y * 2 + 1) as f32 / (2.0 * placed.scale)) as usize).min(frame_h - 1);
            let ty = y + placed.pad_y as usize;
            if ty >= r.height {
                break;
            }
            for x in 0..used_w.min(r.width) {
                let sx = (((x * 2 + 1) as f32 / (2.0 * placed.scale)) as usize).min(frame_w - 1);
                let tx = x + placed.pad_x as usize;
                if tx >= r.width {
                    break;
                }
                let from = (sy * frame_w + sx) * 3;
                let to = (ty * r.width + tx) * 3;
                for c in 0..3 {
                    canvas[to + c] = rgb.get(from + c).copied().unwrap_or(114);
                }
            }
        }
    }
    // Already the right size, so `fit` here is a straight conversion to
    // fractions rather than a resize.
    let fitted = crate::infer::fit(&canvas, r.width, r.height, r.width, r.height);
    (crate::infer::arrange(&fitted, &r), placed)
}

// ---------------------------------------------------------------------------
// Reading a detector's output
// ---------------------------------------------------------------------------

/// The strides a detector's three levels are laid out at.
const LEVELS: [usize; 3] = [8, 16, 32];

/// Faces, from what the face model printed.
///
/// The model reports at three levels of detail, and each level has its own
/// three outputs — is this a face, is this anything at all, and where is it.
/// Twelve outputs in total, which is exactly why `infer::Model::run` hands
/// back all of them: the first is the least useful of the twelve.
pub fn faces(out: &Outputs, placed: &Framing, cfg: &VisionConfig) -> Result<Vec<Face>> {
    let (w, h) = Kind::Faces.wants();
    let mut found = Vec::new();
    for (level, stride) in LEVELS.iter().enumerate() {
        let cols = w / stride;
        let rows = h / stride;
        let cls = out.at(level)?;
        let obj = out.at(3 + level)?;
        let bbox = out.at(6 + level)?;
        let cells = cols * rows;
        if cls.len() < cells || obj.len() < cells || bbox.len() < cells * 4 {
            return Err(AtlasError::Platform(format!(
                "the face model reported {} cells at level {stride}, and I expected {cells} — \
                 that isn't the model I was written for",
                cls.len()
            )));
        }
        for row in 0..rows {
            for col in 0..cols {
                let i = row * cols + col;
                // Both, multiplied: one says "there is something here", the
                // other "that something is a face". Either alone is a
                // detector that fires on shoulders.
                let sure = (cls[i].max(0.0) * obj[i].max(0.0)).sqrt();
                if !sure.is_finite() || sure < cfg.floor {
                    continue;
                }
                let s = *stride as f32;
                let cx = (col as f32 + bbox[i * 4]) * s;
                let cy = (row as f32 + bbox[i * 4 + 1]) * s;
                let bw = bbox[i * 4 + 2].exp() * s;
                let bh = bbox[i * 4 + 3].exp() * s;
                let area = placed.back(cx - bw / 2.0, cy - bh / 2.0, bw, bh);
                if !area.sane() {
                    continue;
                }
                found.push(Face { area, sure, who: None, sure_who: 0.0 });
            }
        }
    }
    let boxes: Vec<(Patch, f32)> = found.iter().map(|f| (f.area, f.sure)).collect();
    let kept = thin_out(&boxes, cfg.same_thing);
    Ok(kept.into_iter().map(|i| found[i].clone()).collect())
}

/// Things, from what the object model printed.
///
/// One output, laid out as one row per candidate: where it is, whether it is
/// anything, and how much it looks like each of the names the model knows.
pub fn things(out: &Outputs, placed: &Framing, cfg: &VisionConfig) -> Result<Vec<Object>> {
    let names = object_names();
    let raw = out.only()?;
    let (w, h) = Kind::Objects.wants();
    let cells: usize = LEVELS.iter().map(|s| (w / s) * (h / s)).sum();
    let stride_of_row = 5 + names.len();
    // The guard that makes a mismatched label list impossible to miss. A
    // model with a different number of names would otherwise be read with
    // this one's names — every answer confidently wrong, nothing reporting a
    // problem.
    if raw.len() != cells * stride_of_row {
        return Err(AtlasError::Platform(format!(
            "the object model reported {} numbers and I expected {} ({cells} candidates \
             against {} names) — the model and the name list don't match",
            raw.len(),
            cells * stride_of_row,
            names.len()
        )));
    }

    let mut grid: Vec<(f32, f32, f32)> = Vec::with_capacity(cells);
    for stride in LEVELS {
        let cols = w / stride;
        let rows = h / stride;
        for row in 0..rows {
            for col in 0..cols {
                grid.push((col as f32, row as f32, stride as f32));
            }
        }
    }

    let mut found: Vec<Object> = Vec::new();
    for (i, (gx, gy, s)) in grid.iter().enumerate() {
        let row = &raw[i * stride_of_row..(i + 1) * stride_of_row];
        let anything = row[4];
        if !anything.is_finite() || anything < cfg.floor {
            continue;
        }
        let mut best = 0usize;
        let mut best_score = f32::MIN;
        for (n, v) in row[5..].iter().enumerate() {
            if *v > best_score {
                best_score = *v;
                best = n;
            }
        }
        let sure = anything * best_score.max(0.0);
        if !sure.is_finite() || sure < cfg.floor {
            continue;
        }
        let cx = (row[0] + gx) * s;
        let cy = (row[1] + gy) * s;
        let bw = row[2].exp() * s;
        let bh = row[3].exp() * s;
        let area = placed.back(cx - bw / 2.0, cy - bh / 2.0, bw, bh);
        if !area.sane() {
            continue;
        }
        found.push(Object { name: names[best].to_string(), area, sure });
    }

    let boxes: Vec<(Patch, f32)> = found.iter().map(|t| (t.area, t.sure)).collect();
    let kept = thin_out(&boxes, cfg.same_thing);
    Ok(kept.into_iter().map(|i| found[i].clone()).collect())
}

/// What a whole picture looks like.
pub fn whole_picture(out: &Outputs, cfg: &VisionConfig) -> Result<Option<Object>> {
    let names = picture_names();
    let raw = out.only()?;
    if raw.len() != names.len() {
        return Err(AtlasError::Platform(format!(
            "the picture model reported {} scores against {} names — the model and the \
             name list don't match",
            raw.len(),
            names.len()
        )));
    }
    let scores = softened(raw);
    let mut best = 0usize;
    for (i, v) in scores.iter().enumerate() {
        if *v > scores[best] {
            best = i;
        }
    }
    if scores[best] < cfg.floor {
        return Ok(None);
    }
    Ok(Some(Object {
        name: names[best].to_string(),
        area: Patch::new(0.0, 0.0, 1.0, 1.0),
        sure: scores[best],
    }))
}

/// Raw scores into something that adds up to one.
///
/// The largest is taken off first, so a big score cannot overflow on the way
/// to being compared with a small one.
fn softened(raw: &[f32]) -> Vec<f32> {
    let top = raw.iter().copied().fold(f32::MIN, f32::max);
    if !top.is_finite() {
        return vec![0.0; raw.len()];
    }
    let exps: Vec<f32> = raw.iter().map(|v| (v - top).exp()).collect();
    let total: f32 = exps.iter().sum();
    if total <= 0.0 {
        return vec![0.0; raw.len()];
    }
    exps.into_iter().map(|v| v / total).collect()
}

/// Drop boxes that are the same thing found more than once.
///
/// Keeps the strongest, then discards anything overlapping it too much, then
/// repeats. Returns positions rather than values so the caller keeps whatever
/// it was holding alongside each box.
pub fn thin_out(boxes: &[(Patch, f32)], too_much: f32) -> Vec<usize> {
    let mut order: Vec<usize> = (0..boxes.len()).collect();
    order.sort_by(|a, b| {
        boxes[*b]
            .1
            .partial_cmp(&boxes[*a].1)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<usize> = Vec::new();
    let mut dropped = vec![false; boxes.len()];
    for i in order {
        if dropped[i] {
            continue;
        }
        kept.push(i);
        for j in 0..boxes.len() {
            if j != i && !dropped[j] && boxes[i].0.overlap(&boxes[j].0) > too_much {
                dropped[j] = true;
            }
        }
    }
    kept
}

fn object_names() -> Vec<&'static str> {
    OBJECT_NAMES.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}

fn picture_names() -> Vec<&'static str> {
    PICTURE_NAMES.lines().map(str::trim).filter(|l| !l.is_empty()).collect()
}

// ---------------------------------------------------------------------------
// Things Atlas has been shown
// ---------------------------------------------------------------------------

/// One thing or person Atlas has been shown and told the name of.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Shown {
    pub name: String,
    /// Several views of the same thing. One photograph of a face is one
    /// lighting condition and one angle, and a system that recognises you
    /// only at your desk in the afternoon is one you stop trusting.
    pub views: Vec<Vec<f32>>,
    pub added: u64,
}

/// Everything Atlas has been shown.
///
/// This is what makes the vocabulary open. A model hands you a fixed list of
/// names somebody else chose; being shown a thing and told its name is a list
/// that grows — the same argument that settled hand gestures, where a
/// landmark model plus geometry beat a classifier with a fixed vocabulary.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Album {
    pub faces: Vec<Shown>,
    pub things: Vec<Shown>,
}

/// What came of trying to put a name to something.
#[derive(Debug, Clone, PartialEq)]
pub enum Guess {
    /// One clear match.
    Is(String, f32),
    /// Two or more were close enough that picking one would be a coin toss.
    ///
    /// Reported rather than resolved, for the same reason `recall::clarity`
    /// reports two notes that disagree instead of silently choosing.
    Unsure(Vec<String>),
    /// Nothing came close.
    NoIdea,
}

impl Album {
    pub fn load(store: &Store) -> Album {
        store.load("album")
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("album", self)
    }

    /// Remember a face as somebody's.
    pub fn remember_face(&mut self, name: &str, view: &[f32], when: u64) -> Result<()> {
        put_away(&mut self.faces, name, view, when)
    }

    /// Remember a thing by name.
    pub fn remember_thing(&mut self, name: &str, view: &[f32], when: u64) -> Result<()> {
        put_away(&mut self.things, name, view, when)
    }

    /// Whose face is this?
    pub fn whose_face(&self, view: &[f32], floor: f32, margin: f32) -> Guess {
        look_up(&self.faces, view, floor, margin)
    }

    /// What thing is this?
    pub fn which_thing(&self, view: &[f32], floor: f32, margin: f32) -> Guess {
        look_up(&self.things, view, floor, margin)
    }

    /// Everything it has been shown, for the settings page.
    ///
    /// Each line is what it is, what it's called, and how many views back it —
    /// because one view is a system that recognises you in one light, and
    /// saying so is what tells someone to show it again.
    pub fn everything(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for (what, shelf) in [("face", &self.faces), ("thing", &self.things)] {
            for s in shelf.iter() {
                out.push(format!(
                    "{} — {what}, {} view{}",
                    s.name,
                    s.views.len(),
                    if s.views.len() == 1 { "" } else { "s" }
                ));
            }
        }
        out
    }

    /// Forget one, by name.
    pub fn forget(&mut self, name: &str) -> bool {
        let before = self.faces.len() + self.things.len();
        self.faces.retain(|s| !s.name.eq_ignore_ascii_case(name));
        self.things.retain(|s| !s.name.eq_ignore_ascii_case(name));
        before != self.faces.len() + self.things.len()
    }
}

/// Put a reading on a shelf under a name.
///
/// A reading that is empty, all zeros, or has anything in it that is not a
/// number is refused rather than stored: it would match everything a little
/// and nothing well, and the failure would look like recognition being
/// unreliable rather than like a bad recording.
fn put_away(shelf: &mut Vec<Shown>, name: &str, view: &[f32], when: u64) -> Result<()> {
    let unit = to_unit(view).ok_or_else(|| {
        AtlasError::Platform(
            "that reading was empty, so there's nothing to remember it by — try again with the \
             thing in clear view"
                .into(),
        )
    })?;
    let name = name.trim();
    if name.is_empty() {
        return Err(AtlasError::Config("I need a name to file that under".into()));
    }
    match shelf.iter_mut().find(|s| s.name.eq_ignore_ascii_case(name)) {
        Some(existing) => existing.views.push(unit),
        None => shelf.push(Shown {
            name: name.to_string(),
            views: vec![unit],
            added: when,
        }),
    }
    Ok(())
}

/// Find the best name on a shelf for a reading.
///
/// The margin is what stops a coin toss being reported as a fact: two people
/// who look somewhat alike both score well, and without it the higher one
/// wins by a hair and is announced with no hint that it was close.
fn look_up(shelf: &[Shown], view: &[f32], floor: f32, margin: f32) -> Guess {
    let Some(unit) = to_unit(view) else {
        return Guess::NoIdea;
    };
    let mut scores: Vec<(f32, &str)> = shelf
        .iter()
        .map(|s| {
            let best = s.views.iter().map(|v| alike(&unit, v)).fold(f32::MIN, f32::max);
            (best, s.name.as_str())
        })
        .filter(|(score, _)| score.is_finite())
        .collect();
    scores.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    let Some((best, name)) = scores.first().copied() else {
        return Guess::NoIdea;
    };
    if best < floor {
        return Guess::NoIdea;
    }
    let close: Vec<String> = scores
        .iter()
        .take_while(|(s, _)| best - *s <= margin)
        .map(|(_, n)| n.to_string())
        .collect();
    if close.len() > 1 {
        return Guess::Unsure(close);
    }
    Guess::Is(name.to_string(), best)
}

/// A reading, scaled so its length is one.
///
/// Returns `None` for anything that cannot be compared — empty, all zeros, or
/// containing something that is not a number. Each of those would otherwise
/// produce a similarity score that means nothing and reads like one that does.
pub fn to_unit(v: &[f32]) -> Option<Vec<f32>> {
    if v.is_empty() || v.iter().any(|x| !x.is_finite()) {
        return None;
    }
    let length = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if length <= f32::EPSILON {
        return None;
    }
    Some(v.iter().map(|x| x / length).collect())
}

/// How alike two readings of the same length are, from -1 to 1.
///
/// Different lengths give 0 rather than an error or a partial comparison:
/// they came from different models, and no number comparing them means
/// anything.
pub fn alike(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

// ---------------------------------------------------------------------------
// The models, loaded
// ---------------------------------------------------------------------------

/// The models seeing needs, held open.
///
/// Loaded once. Each is separately optional: finding faces without being able
/// to tell whose they are is still worth having, and refusing to do any of it
/// because one file is missing is how a feature nobody can partly install
/// becomes a feature nobody installs.
pub struct Looking {
    pub finding_faces: Option<Model>,
    pub telling_faces: Option<Model>,
    pub naming_things: Option<Model>,
    pub describing: Option<Model>,
    /// The hand models again, so a finger and the thing it is over can be
    /// read out of *the same picture*.
    ///
    /// Hand tracking already owns a copy of these on its own thread. Two
    /// copies of eight megabytes is the price of not sharing them, and the
    /// alternative — asking the tracking thread where the hand was — would
    /// answer in the wrong space: it reports where the pointer is on the
    /// screen, and the question here is where a finger is in a camera frame.
    /// Those are different pictures, and treating them as one would name
    /// whatever happened to be in that corner of the room.
    pub finding_hands: Option<Model>,
    pub reading_hands: Option<Model>,
}

impl std::fmt::Debug for Looking {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Looking")
            .field("finding_faces", &self.finding_faces.is_some())
            .field("telling_faces", &self.telling_faces.is_some())
            .field("naming_things", &self.naming_things.is_some())
            .field("describing", &self.describing.is_some())
            .field("finding_hands", &self.finding_hands.is_some())
            .finish()
    }
}

impl Looking {
    /// Load whatever is there.
    ///
    /// Never fails: a model that will not load is a model Atlas does not have,
    /// and which ones those are is reported by `missing`.
    pub fn open(models_dir: &std::path::Path) -> Looking {
        let one = |k: Kind| Model::load(k, models_dir).ok();
        Looking {
            finding_faces: one(Kind::Faces),
            telling_faces: one(Kind::FaceId),
            naming_things: one(Kind::Objects),
            describing: one(Kind::Picture),
            finding_hands: one(Kind::HandPresence),
            reading_hands: one(Kind::HandLandmarks),
        }
    }

    /// Can Atlas see at all?
    pub fn any(&self) -> bool {
        self.finding_faces.is_some() || self.naming_things.is_some() || self.describing.is_some()
    }

    /// Look at one frame.
    ///
    /// Every step is allowed to fail on its own and say so. A picture where
    /// the faces could be found and the things could not is a real and useful
    /// answer; refusing the whole look because one model stumbled is not.
    pub fn look(&mut self, rgb: &[u8], w: usize, h: usize, cfg: &VisionConfig, album: &Album) -> Sight {
        self.look_for(rgb, w, h, cfg, album, true)
    }

    /// A look for whether you're there and what your hand says: faces and the
    /// two small hand models only. Naming things and describing the picture
    /// are the heavy part, and nothing here needs them.
    pub fn look_at_you(&mut self, rgb: &[u8], w: usize, h: usize, cfg: &VisionConfig, album: &Album) -> Sight {
        let mut sight = self.look_for(rgb, w, h, cfg, album, false);
        if let Some(hand) = self.hand(rgb, w, h) {
            let g = crate::handshape::answer_from(&hand).map(|(g, s)| (g.to_string(), s));
            if let Sight::Looked(scene) = &mut sight {
                scene.gesture = g;
            }
        }
        sight
    }

    fn look_for(&mut self, rgb: &[u8], w: usize, h: usize, cfg: &VisionConfig, album: &Album, name_things: bool) -> Sight {
        if w == 0 || h == 0 || rgb.len() < w * h * 3 {
            return Sight::Unread("the camera handed back a picture I couldn't read".into());
        }
        if !self.any() {
            return Sight::Unread(
                "none of the seeing models are installed — they're a one-off download into \
                 models/"
                    .into(),
            );
        }
        let mut scene = Scene::default();

        if let Some(model) = self.finding_faces.as_mut() {
            let (pixels, placed) = letterbox(rgb, w, h, Kind::Faces);
            match model.run(&pixels).and_then(|out| faces(&out, &placed, cfg)) {
                Ok(found) => scene.faces = found,
                Err(e) => scene.could_not.push(format!("I couldn't look for faces: {e}")),
            }
        }

        // Whose face, separately and at a higher bar. Only ever run on a face
        // that was actually found, because a recogniser handed a picture of a
        // wall returns a set of numbers just as confidently as it does for a
        // face.
        if let (Some(model), false) = (self.telling_faces.as_mut(), scene.faces.is_empty()) {
            for face in scene.faces.iter_mut() {
                let cut = face.area.in_pixels(w, h, cfg.face_margin);
                if cut.2 == 0 || cut.3 == 0 {
                    continue;
                }
                let pixels = crate::infer::prepare_crop(rgb, w, h, cut, Kind::FaceId);
                let Ok(out) = model.run(&pixels) else { continue };
                let Ok(reading) = out.only() else { continue };
                match album.whose_face(reading, cfg.sure_enough_to_name_a_face, cfg.margin) {
                    Guess::Is(who, how) => {
                        face.who = Some(who);
                        face.sure_who = how;
                    }
                    // Deliberately left unnamed. Two people it could be is
                    // not one person it is.
                    Guess::Unsure(_) | Guess::NoIdea => {}
                }
            }
        }

        if let (Some(model), true) = (self.naming_things.as_mut(), name_things) {
            let (pixels, placed) = letterbox(rgb, w, h, Kind::Objects);
            match model.run(&pixels).and_then(|out| things(&out, &placed, cfg)) {
                Ok(found) => scene.things = found,
                Err(e) => scene.could_not.push(format!("I couldn't name what's there: {e}")),
            }
        }

        if let (Some(model), true) = (self.describing.as_mut(), name_things) {
            let pixels = crate::infer::prepare(rgb, w, h, Kind::Picture);
            match model.run(&pixels) {
                Ok(out) => match out.only() {
                    Ok(reading) => {
                        match whole_picture(&out, cfg) {
                            Ok(found) => scene.whole = found,
                            Err(e) => scene.could_not.push(format!("I couldn't describe it: {e}")),
                        }
                        // And what *he* calls it, which beats what the model
                        // calls it. Without this the album could be added to
                        // and never read: "this is my mug" would be accepted,
                        // stored, and never recognised again — accepting
                        // something and then never using it is the exact
                        // shape of failure this codebase keeps producing.
                        if let Guess::Is(name, sure) = album.which_thing(
                            reading,
                            cfg.sure_enough_to_name_a_thing,
                            cfg.margin,
                        ) {
                            scene.whole = Some(Object {
                                name,
                                area: Patch::new(0.0, 0.0, 1.0, 1.0),
                                sure,
                            });
                        }
                    }
                    Err(e) => scene.could_not.push(format!("I couldn't describe it: {e}")),
                },
                Err(e) => scene.could_not.push(format!("I couldn't describe it: {e}")),
            }
        }

        // Everything failed. That is not an empty room — it is a look that
        // did not happen, and the difference is the whole point of `Sight`.
        if !scene.saw_anything() && !scene.could_not.is_empty() {
            return Sight::Unread(scene.could_not.join("; "));
        }
        Sight::Looked(scene)
    }

    /// The hand in this picture, as joints in the picture's terms: the palm
    /// found by the first small model, its joints read by the second.
    pub fn hand(&mut self, rgb: &[u8], w: usize, h: usize) -> Option<crate::handshape::Landmarks> {
        let finding = self.finding_hands.as_mut()?;
        let (fw, fh) = Kind::HandPresence.wants();
        let small = crate::infer::fit(rgb, w, h, fw, fh);
        let found = finding.run(&small).ok()?;
        let hand = crate::handloop::where_the_hand_is(&found)?;
        let reading = self.reading_hands.as_mut()?;
        let (cx, cy, cw, ch) = hand.in_pixels(w, h, 0.5);
        if cw == 0 || ch == 0 {
            return None;
        }
        let cut = crate::infer::prepare_crop(rgb, w, h, (cx, cy, cw, ch), Kind::HandLandmarks);
        let out = reading.run(&cut).ok()?;
        let joints = out.at(0).ok()?;
        let sure = out.at(1).ok().and_then(|s| s.first().copied()).unwrap_or(1.0);
        let (rw, rh) = Kind::HandLandmarks.wants();
        crate::handshape::from_model(&crate::handshape::in_the_frame(
            joints,
            sure,
            (rw as f32, rh as f32),
            (cx as f32 / w as f32, cy as f32 / h as f32),
            (cw as f32 / w as f32, ch as f32 / h as f32),
        ))
    }

    /// Where the pointing finger is in this picture, as fractions of it.
    ///
    /// The join between the two halves of this work. Hand tracking knew where
    /// a fingertip was and had no idea what was underneath it; seeing knew
    /// what was in the picture and had no idea which of it was meant. Both
    /// answers now come out of the same frame, so no coordinate has to be
    /// translated between two pictures taken at two moments.
    ///
    /// `None` when the hand models are not installed, or no hand is up. Both
    /// are ordinary, and both fall back to the thing in the middle.
    pub fn finger(&mut self, rgb: &[u8], w: usize, h: usize) -> Option<(f32, f32)> {
        let marks = self.hand(rgb, w, h)?;
        // The index fingertip. Point eight of twenty-one — the same joint the
        // pointer follows, so the ring on screen and the answer to "what's
        // this" can never disagree about which part of the hand is doing the
        // pointing.
        let tip = marks.points[8];
        (tip.x.is_finite() && tip.y.is_finite()).then_some((tip.x, tip.y))
    }

    /// The set of numbers that stands for a whole picture, for remembering it.
    pub fn describe(&mut self, rgb: &[u8], w: usize, h: usize) -> Result<Vec<f32>> {
        let model = self.describing.as_mut().ok_or_else(|| {
            AtlasError::Config(format!(
                "the model for {} isn't installed — {} should be in models/",
                Kind::Picture.plain(),
                Kind::Picture.file()
            ))
        })?;
        let pixels = crate::infer::prepare(rgb, w, h, Kind::Picture);
        Ok(model.run(&pixels)?.only()?.to_vec())
    }

    /// The set of numbers that stands for one face, for remembering whose.
    pub fn face_reading(
        &mut self,
        rgb: &[u8],
        w: usize,
        h: usize,
        area: &Patch,
        margin: f32,
    ) -> Result<Vec<f32>> {
        let model = self.telling_faces.as_mut().ok_or_else(|| {
            AtlasError::Config(format!(
                "the model for {} isn't installed — {} should be in models/",
                Kind::FaceId.plain(),
                Kind::FaceId.file()
            ))
        })?;
        let cut = area.in_pixels(w, h, margin);
        if cut.2 == 0 || cut.3 == 0 {
            return Err(AtlasError::Platform(
                "that face is too small in the picture to read".into(),
            ));
        }
        let pixels = crate::infer::prepare_crop(rgb, w, h, cut, Kind::FaceId);
        Ok(model.run(&pixels)?.only()?.to_vec())
    }
}
