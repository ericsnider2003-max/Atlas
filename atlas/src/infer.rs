//! Running a model inside Atlas.
//!
//! `tract` is a pure-Rust inference engine. It compiles into `atlas.exe` —
//! no second process, no Python, no runtime to install. That makes it more
//! self-contained than what Atlas already does for speech, which launches
//! `whisper-cli.exe` and `piper.exe` as separate programs.
//!
//! What it does not remove is the weights file. No inference engine invents
//! one, and training a hand tracker from nothing is months of work and a
//! dataset Eric does not have. So the arrangement is the same one Atlas
//! already uses for `ggml-base.en.bin` and the Piper voice: **our code,
//! somebody's weights, downloaded once.** The difference is that the code
//! running them is now ours rather than another program.
//!
//! ## What this module is careful about
//!
//! Loading a model is slow and happens once; running it happens twenty times
//! a second. So the load is separate from the run, failures at load are
//! reported plainly rather than retried per frame, and a model that is absent
//! is a stated fact rather than a silent no-op — which is the failure this
//! whole codebase keeps producing.
//!
//! ## Models do not agree on anything
//!
//! The first version of this module assumed one shape of model, because it
//! only had one job. Every model it fed got a picture laid out as
//! `[1, height, width, 3]`, scaled to nought-to-one, red first, and every
//! caller read output number zero.
//!
//! None of that is universal, and each assumption fails silently rather than
//! loudly:
//!
//! - **Layout.** A picture can arrive with each pixel's three channels
//!   together (`[1, h, w, 3]`) or with all the reds, then all the greens,
//!   then all the blues (`[1, 3, h, w]`). The hand models want the first;
//!   the face, object and picture models want the second. Handing a model
//!   the wrong one is not an error — the numbers all fit — it is a model
//!   that runs perfectly and sees nothing recognisable.
//! - **Scale.** Some models want nought-to-one, some want the raw
//!   nought-to-255, some want the average subtracted first.
//! - **Which output.** A model with one answer has one output. A face
//!   detector has twelve, and the boxes are not the first of them. Reading
//!   output zero and calling it the answer is how you get a face detector
//!   that appears to work and can never find a face.
//!
//! So a `Kind` now carries a full `Recipe`, and running a model hands back
//! *every* output, named by position, with a stated error when the one asked
//! for is not there.

use crate::error::{AtlasError, Result};
use std::path::{Path, PathBuf};

/// How a picture is laid out for a model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// Each pixel's three channels together: `[1, height, width, 3]`.
    Interleaved,
    /// All the reds, then all the greens, then all the blues:
    /// `[1, 3, height, width]`.
    Planar,
    /// One channel, no colour at all: `[1, 1, height, width]`.
    ///
    /// The text recogniser wants this. Handing it three channels is not a
    /// shape error it would report — it is a tensor three times the size it
    /// expects, and `tract` refuses at load with a message about dimensions
    /// that says nothing about colour. Worth its own variant so the refusal
    /// happens here, where it can be explained.
    Grey,
}

impl Layout {
    /// How many channels a model with this layout is fed.
    pub fn channels(self) -> usize {
        match self {
            Layout::Interleaved | Layout::Planar => 3,
            Layout::Grey => 1,
        }
    }
}

/// Everything a model wants done to a picture before it sees it.
///
/// Held as data rather than as code per model, because the difference between
/// two of these is a handful of numbers and the difference between two
/// hand-written preparation functions is a place for one of them to rot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Recipe {
    pub width: usize,
    pub height: usize,
    pub layout: Layout,
    /// What this model calls white. `1.0` for a model wanting fractions,
    /// `255.0` for one wanting the raw byte values back.
    pub top: f32,
    /// Subtracted per channel, after scaling to `top`.
    ///
    /// In the model's own channel order, not the camera's — so with
    /// `blue_first` set, `mean[0]` is the blue one.
    pub mean: [f32; 3],
    /// Divided per channel, after the mean is subtracted. Same order as
    /// `mean`.
    pub deviation: [f32; 3],
    /// Some models were trained on pictures with blue first. Getting this
    /// wrong does not fail — it quietly swaps every red thing for a blue one.
    pub blue_first: bool,
}

impl Recipe {
    /// The plain nought-to-one, channels-together, red-first arrangement.
    const fn plain(width: usize, height: usize, layout: Layout) -> Recipe {
        Recipe {
            width,
            height,
            layout,
            top: 1.0,
            mean: [0.0; 3],
            deviation: [1.0; 3],
            blue_first: false,
        }
    }

    /// How many floats a prepared picture has.
    pub fn values(&self) -> usize {
        self.width * self.height * self.layout.channels()
    }

    /// The tensor shape this model is fed.
    ///
    /// One place, used by both the load and the run. They were two separate
    /// `match` blocks over the same enum, which is a pair that stays correct
    /// only for as long as nobody adds a variant to one of them — and adding
    /// the grey variant is exactly that.
    pub fn shape(&self) -> [usize; 4] {
        match self.layout {
            Layout::Interleaved => [1, self.height, self.width, 3],
            Layout::Planar => [1, 3, self.height, self.width],
            Layout::Grey => [1, 1, self.height, self.width],
        }
    }
}

/// What a model file is for.
///
/// Named rather than passed as a path, so a missing one can be reported as
/// "hand tracking isn't installed" rather than as a filename nobody
/// recognises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Finds where hands are in the picture.
    HandPresence,
    /// Turns a cropped hand into twenty-one joints.
    HandLandmarks,
    /// Finds where faces are in the picture.
    Faces,
    /// Turns a cropped face into a set of numbers that can be compared to
    /// another face.
    FaceId,
    /// Finds and names the things in a picture.
    Objects,
    /// Describes a whole picture as a set of numbers, so two pictures can be
    /// compared and one you have named before can be recognised again.
    Picture,
    /// Finds where the text is. Not what it says — where.
    TextFind,
    /// Turns one strip of picture into letters.
    TextRead,
}

impl Kind {
    /// Everything Atlas can load.
    pub fn all() -> [Kind; 8] {
        [
            Kind::HandPresence,
            Kind::HandLandmarks,
            Kind::Faces,
            Kind::FaceId,
            Kind::Objects,
            Kind::Picture,
            Kind::TextFind,
            Kind::TextRead,
        ]
    }

    /// The models reading text needs.
    ///
    /// Both, always. A finder with no reader knows where the words are and
    /// cannot say one of them, which is not half the feature — it is none of
    /// it wearing the shape of half.
    pub fn for_reading() -> [Kind; 2] {
        [Kind::TextFind, Kind::TextRead]
    }

    /// The models hand tracking needs.
    pub fn for_hands() -> [Kind; 2] {
        [Kind::HandPresence, Kind::HandLandmarks]
    }

    /// The models seeing needs.
    pub fn for_seeing() -> [Kind; 4] {
        [Kind::Faces, Kind::FaceId, Kind::Objects, Kind::Picture]
    }

    pub fn plain(self) -> &'static str {
        match self {
            Kind::HandPresence => "finding your hands",
            Kind::HandLandmarks => "reading your fingers",
            Kind::Faces => "finding faces",
            Kind::FaceId => "telling faces apart",
            Kind::Objects => "naming things",
            Kind::Picture => "describing a picture",
            Kind::TextFind => "finding words on the screen",
            Kind::TextRead => "reading those words",
        }
    }

    /// The filename Atlas expects under `models/`.
    pub fn file(self) -> &'static str {
        match self {
            Kind::HandPresence => "hand_presence.onnx",
            Kind::HandLandmarks => "hand_landmarks.onnx",
            Kind::Faces => "face_detect.onnx",
            Kind::FaceId => "face_id.onnx",
            Kind::Objects => "objects.onnx",
            Kind::Picture => "picture.onnx",
            Kind::TextFind => "text_find.onnx",
            Kind::TextRead => "text_read.onnx",
        }
    }

    /// What this model wants done to a picture.
    ///
    /// Stated here rather than read from the file so the camera frame can be
    /// prepared before the model is even loaded — and so a mismatch is caught
    /// as a clear error instead of a shape panic inside the engine.
    pub fn recipe(self) -> Recipe {
        match self {
            // MediaPipe: channels together, nought to one.
            Kind::HandPresence => Recipe::plain(192, 192, Layout::Interleaved),
            Kind::HandLandmarks => Recipe::plain(224, 224, Layout::Interleaved),
            // YuNet: planar, and the raw byte values rather than fractions.
            //
            // Blue first, like the object model below: OpenCV hands YuNet the
            // picture exactly as it reads it off disk (`blobFromImage` with
            // no channel swap), and OpenCV reads blue first.
            Kind::Faces => Recipe {
                top: 255.0,
                blue_first: true,
                ..Recipe::plain(640, 640, Layout::Planar)
            },
            // SFace: planar, raw values, a face already cropped to 112.
            Kind::FaceId => Recipe {
                top: 255.0,
                ..Recipe::plain(112, 112, Layout::Planar)
            },
            // YOLOX: planar, raw values, no averaging, and **blue first** —
            // the OpenCV zoo trained and runs it on pictures as OpenCV reads
            // them. Found 24 Sep 2026, the first time the real model ran on
            // a real picture (`tests/seeing_real_pictures.rs`): fed
            // red-first, a bowl of oranges came back as "a bed" and "a
            // vase"; blue-first, a dozen oranges at 0.55–0.77. The person
            // and the football were found either way, which is how it hid.
            Kind::Objects => Recipe {
                top: 255.0,
                blue_first: true,
                ..Recipe::plain(640, 640, Layout::Planar)
            },
            // The usual ImageNet arrangement: fractions, then the average
            // picture subtracted.
            Kind::Picture => Recipe {
                mean: [0.485, 0.456, 0.406],
                deviation: [0.229, 0.224, 0.225],
                ..Recipe::plain(224, 224, Layout::Planar)
            },
            // PP-OCRv3 detection.
            //
            // These numbers look wrong and are not. They are the ImageNet
            // *red-green-blue* averages, and `blue_first` puts them on a
            // blue-first picture — so the red average lands on the blue
            // channel. That is what PaddleOCR trained with and what OpenCV
            // runs it with, which means the model has never once seen them
            // the other way round. Correcting them to look sensible would
            // break it, quietly, by a few per cent of accuracy that nobody
            // would trace back to here.
            //
            // OpenCV computes `scale * (pixel - mean)` with
            // `scale = 1/255/std`; Atlas computes `(pixel - mean) / deviation`,
            // so the deviation is 255 times the standard deviation.
            Kind::TextFind => Recipe {
                top: 255.0,
                mean: [123.675, 116.28, 103.53],
                deviation: [58.395, 57.12, 57.375],
                blue_first: true,
                ..Recipe::plain(736, 736, Layout::Planar)
            },
            // CRNN. One grey channel, a hundred wide by thirty-two high —
            // wider than it is tall, unlike every other model here, because
            // it reads a strip of a line rather than a square of picture.
            Kind::TextRead => Recipe {
                top: 255.0,
                mean: [127.5; 3],
                deviation: [127.5; 3],
                ..Recipe::plain(100, 32, Layout::Grey)
            },
        }
    }

    /// What the model wants in, as (width, height).
    pub fn wants(self) -> (usize, usize) {
        let r = self.recipe();
        (r.width, r.height)
    }
}

/// What a model handed back.
///
/// Every output, not just the first. A detector's boxes are rarely its first
/// output, and reading output zero and calling it the answer is how a model
/// that runs perfectly is read as a model that never finds anything.
#[derive(Debug, Clone, Default)]
pub struct Outputs {
    model: &'static str,
    values: Vec<Vec<f32>>,
}

impl Outputs {
    /// For tests and for callers assembling a reading by hand.
    pub fn of(model: &'static str, values: Vec<Vec<f32>>) -> Outputs {
        Outputs { model, values }
    }

    pub fn count(&self) -> usize {
        self.values.len()
    }

    /// One output, by position.
    ///
    /// An error rather than `None`, and it says which model and how many
    /// outputs there actually were: asking for an output a model does not
    /// have means the model is not the one this code was written for, and
    /// that is worth saying out loud rather than treating as an empty answer.
    pub fn at(&self, i: usize) -> Result<&[f32]> {
        self.values.get(i).map(|v| v.as_slice()).ok_or_else(|| {
            AtlasError::Platform(format!(
                "{} gave {} result{}, and I wanted number {} — that isn't the model I was \
                 written for",
                self.model,
                self.values.len(),
                if self.values.len() == 1 { "" } else { "s" },
                i + 1
            ))
        })
    }

    /// The first output, when a model genuinely has only one answer.
    pub fn only(&self) -> Result<&[f32]> {
        self.at(0)
    }
}

/// A loaded model, ready to run. Backed by ONNX (tract) when the `onnx`
/// feature is on — which it is by default, on the desktop. A build without it
/// (a first mobile baseline that drops the native ML toolchain) keeps the same
/// shape and reports the capability as unavailable rather than failing to
/// compile: sync, the hub, projects and the rest never touch it.
#[cfg(feature = "onnx")]
pub struct Model {
    kind: Kind,
    plan: std::sync::Arc<tract_onnx::prelude::TypedRunnableModel>,
    /// How long the last run took, so `handtrack::Pace` has something real to
    /// pace against rather than an assumption.
    pub last_ms: u32,
}

/// The same surface for a build without the `onnx` feature. `load` and `run`
/// say plainly that this Atlas has no on-device ONNX inference, so callers
/// (vision, hand tracking) degrade to "not available here" — the way
/// `sync::Kind::Standalone.cannot()` already frames a phone's missing
/// hardware — instead of the module failing to build.
#[cfg(not(feature = "onnx"))]
pub struct Model {
    kind: Kind,
    pub last_ms: u32,
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // The plan itself has no useful debug output and a great deal of it.
        f.debug_struct("Model")
            .field("kind", &self.kind)
            .field("last_ms", &self.last_ms)
            .finish()
    }
}

#[cfg(feature = "onnx")]
impl Model {
    /// Load one, once.
    ///
    /// Slow — hundreds of milliseconds. Never call this per frame.
    pub fn load(kind: Kind, models_dir: &Path) -> Result<Model> {
        use tract_onnx::prelude::*;

        let path = models_dir.join(kind.file());
        if !path.exists() {
            // Named, not a shrug. "Nothing happened" is what makes a person
            // conclude the whole feature doesn't work.
            return Err(AtlasError::Config(format!(
                "the model for {} isn't installed — {} should be in models/",
                kind.plain(),
                kind.file()
            )));
        }
        let r = kind.recipe();
        let shape = r.shape();
        let plan = tract_onnx::onnx()
            .model_for_path(&path)
            .map_err(|e| AtlasError::Config(format!("couldn't read {}: {e}", kind.file())))?
            .with_input_fact(0, f32::fact(shape).into())
            .map_err(|e| {
                AtlasError::Config(format!(
                    "{} doesn't take the picture size I'd give it: {e}",
                    kind.file()
                ))
            })?
            .into_optimized()
            .map_err(|e| AtlasError::Config(format!("couldn't prepare {}: {e}", kind.file())))?
            .into_runnable()
            .map_err(|e| AtlasError::Config(format!("couldn't prepare {}: {e}", kind.file())))?;

        Ok(Model { kind, plan, last_ms: 0 })
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Run it on one prepared picture.
    ///
    /// `pixels` is what `prepare` produced: already resized, already laid out
    /// and scaled the way this model wants. Preparation belongs to the caller
    /// because a single camera frame is often fed to more than one model, and
    /// doing it here would mean doing the resize twice.
    pub fn run(&mut self, pixels: &[f32]) -> Result<Outputs> {
        use tract_onnx::prelude::*;

        let r = self.kind.recipe();
        let want = r.values();
        if pixels.len() != want {
            // Caught here rather than as a panic inside the engine, which
            // would take the whole daemon down over a resize bug.
            return Err(AtlasError::Platform(format!(
                "gave {} the wrong size picture: {} values, wanted {want}",
                self.kind.file(),
                pixels.len()
            )));
        }

        let s = r.shape();
        let began = std::time::Instant::now();
        let input: Tensor = tract_ndarray::Array4::from_shape_vec((s[0], s[1], s[2], s[3]), pixels.to_vec())
            .map_err(|e| AtlasError::Platform(format!("couldn't shape the picture: {e}")))?
            .into();
        let out = self
            .plan
            .run(tvec!(input.into()))
            .map_err(|e| AtlasError::Platform(format!("{} failed: {e}", self.kind.file())))?;
        self.last_ms = began.elapsed().as_millis().min(u32::MAX as u128) as u32;

        let mut values = Vec::with_capacity(out.len());
        for t in out.iter() {
            let tensor: &Tensor = t;
            let view = tensor.view();
            let slice = view
                .as_slice::<f32>()
                .map_err(|e| AtlasError::Platform(format!("couldn't read the result: {e}")))?;
            values.push(slice.to_vec());
        }
        if values.is_empty() {
            return Err(AtlasError::Platform(format!(
                "{} returned nothing",
                self.kind.file()
            )));
        }
        Ok(Outputs { model: self.kind.file(), values })
    }
}

/// The no-ONNX build: the capability is absent, said plainly so `doctor` can
/// report it rather than the feature silently doing nothing.
#[cfg(not(feature = "onnx"))]
impl Model {
    pub fn load(_kind: Kind, _models_dir: &Path) -> Result<Model> {
        Err(AtlasError::Config(
            "this Atlas was built without on-device ONNX inference (the `onnx` feature is off); \
             the vision and hand models aren't available in this build"
                .into(),
        ))
    }

    pub fn kind(&self) -> Kind {
        self.kind
    }

    pub fn run(&mut self, _pixels: &[f32]) -> Result<Outputs> {
        Err(AtlasError::Platform(
            "no on-device ONNX inference is built into this Atlas".into(),
        ))
    }
}

/// Which of the models asked for are present, and which are missing.
///
/// Asked once at start so `doctor` can say "hand tracking needs one more
/// file" rather than the feature silently doing nothing — the same failure
/// this codebase keeps producing.
///
/// The caller names which models it needs. Seeing and hand tracking are
/// separate features that can be installed separately, and a single list
/// would report seeing as broken because hand tracking is not set up.
pub fn whats_missing(models_dir: &Path, want: &[Kind]) -> Vec<(Kind, PathBuf)> {
    want.iter()
        .map(|k| (*k, models_dir.join(k.file())))
        .filter(|(_, p)| !p.exists())
        .collect()
}

/// Said out loud.
pub fn spoken(missing: &[(Kind, PathBuf)]) -> String {
    if missing.is_empty() {
        return "Everything I need is installed.".into();
    }
    let what: Vec<&str> = missing.iter().map(|(k, _)| k.plain()).collect();
    format!(
        "I can't do {} yet — the model file{} {} there. It's a one-off \
         download into models/.",
        what.join(" or "),
        if missing.len() == 1 { "" } else { "s" },
        if missing.len() == 1 { "isn't" } else { "aren't" }
    )
}

/// Turn a raw camera frame into what a model wants.
///
/// The whole job: resize, lay out, scale, average. Every model gets its
/// picture through here, so there is one place where a preparation mistake
/// can live rather than one per model.
pub fn prepare(rgb: &[u8], from_w: usize, from_h: usize, kind: Kind) -> Vec<f32> {
    let r = kind.recipe();
    let small = fit(rgb, from_w, from_h, r.width, r.height);
    arrange(&small, &r)
}

/// The same, from part of a frame.
///
/// Cropping first and resizing second, rather than resizing the whole frame
/// and cropping the result: a face is a small part of a camera frame, and
/// shrinking the whole picture to 640 before taking a 112-pixel face out of
/// it throws away most of the face.
pub fn prepare_crop(
    rgb: &[u8],
    from_w: usize,
    from_h: usize,
    area: (usize, usize, usize, usize),
    kind: Kind,
) -> Vec<f32> {
    let (cx, cy, cw, ch) = area;
    let r = kind.recipe();
    if cw == 0 || ch == 0 || from_w == 0 || from_h == 0 {
        return Vec::new();
    }
    let mut cut = Vec::with_capacity(cw * ch * 3);
    for y in 0..ch {
        let sy = (cy + y).min(from_h.saturating_sub(1));
        for x in 0..cw {
            let sx = (cx + x).min(from_w.saturating_sub(1));
            let at = (sy * from_w + sx) * 3;
            for c in 0..3 {
                cut.push(rgb.get(at + c).copied().unwrap_or(0));
            }
        }
    }
    let small = fit(&cut, cw, ch, r.width, r.height);
    arrange(&small, &r)
}

/// Resize a frame, keeping it as fractions with the channels together.
///
/// Nearest-neighbour, deliberately. Bilinear would be slightly better input
/// and costs several times as much per frame, and this runs twenty times a
/// second — the model is far more tolerant of a slightly rough resize than
/// Eric's laptop is of the extra work.
pub fn fit(rgb: &[u8], from_w: usize, from_h: usize, to_w: usize, to_h: usize) -> Vec<f32> {
    let mut out = Vec::with_capacity(to_w * to_h * 3);
    if from_w == 0 || from_h == 0 {
        return out;
    }
    // Sample the middle of each source region, not its top-left corner.
    // Corner sampling shifts the whole picture up and left by half a region,
    // which on a downscale is several real pixels — a systematic bias in
    // where the model thinks your hand is, applied to every single frame.
    for y in 0..to_h {
        let sy = ((y * 2 + 1) * from_h / (to_h * 2)).min(from_h - 1);
        for x in 0..to_w {
            let sx = ((x * 2 + 1) * from_w / (to_w * 2)).min(from_w - 1);
            let at = (sy * from_w + sx) * 3;
            for c in 0..3 {
                out.push(rgb.get(at + c).copied().unwrap_or(0) as f32 / 255.0);
            }
        }
    }
    out
}

/// Lay out and scale an already-resized picture the way a model wants it.
///
/// Takes what `fit` produced — fractions, channels together, red first — and
/// rearranges it. Separate from `fit` so the geometry and the arithmetic can
/// each be got wrong in only one place.
pub fn arrange(fitted: &[f32], r: &Recipe) -> Vec<f32> {
    let pixels = r.width * r.height;
    if fitted.len() < pixels * 3 {
        return Vec::new();
    }
    let value = |p: usize, c: usize| -> f32 {
        let from = if r.blue_first { 2 - c } else { c };
        (fitted[p * 3 + from] * r.top - r.mean[c]) / r.deviation[c]
    };
    let mut out = vec![0.0f32; r.values()];
    match r.layout {
        Layout::Interleaved => {
            for p in 0..pixels {
                for c in 0..3 {
                    out[p * 3 + c] = value(p, c);
                }
            }
        }
        Layout::Planar => {
            for c in 0..3 {
                let base = c * pixels;
                for p in 0..pixels {
                    out[base + p] = value(p, c);
                }
            }
        }
        // The colour is flattened first, then the mean and deviation are
        // applied to the one channel that is left. `mean[0]` is used, and
        // `blue_first` means nothing here — there is no order to get wrong.
        //
        // The weights are the ones OpenCV uses, because the recogniser was
        // trained on pictures greyed by OpenCV. A plain average of the three
        // channels is a different picture, and the model has never seen one.
        Layout::Grey => {
            for p in 0..pixels {
                let grey = fitted[p * 3] * 0.299 + fitted[p * 3 + 1] * 0.587 + fitted[p * 3 + 2] * 0.114;
                out[p] = (grey * r.top - r.mean[0]) / r.deviation[0];
            }
        }
    }
    out
}
