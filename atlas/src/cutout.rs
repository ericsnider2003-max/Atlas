//! Telling the subject of a photo from its background, on this machine.
//!
//! "Blur the background" and "remove the background" both need the same
//! thing first: a matte, one number per pixel saying how much of it is the
//! subject. Atlas makes it with a small model run in-process by `tract`
//! (behind the `onnx` feature, as the rest of seeing is), writes it out as a
//! plain grey picture, and hands the compositing to ffmpeg (`photo`).
//!
//! ## The two models, and why these two
//!
//! - **MODNet** (portrait matting, Apache-2.0, code and weights;
//!   github.com/ZHKKKe/MODNet, the ONNX export at huggingface.co/Xenova/modnet).
//!   A true alpha matte with soft hair edges, which is what a blurred
//!   background behind a person needs. Tried first.
//! - **U^2-Net small** (`u2netp`, Apache-2.0; github.com/xuebinqin/U-2-Net,
//!   the ONNX file rembg publishes). 4.6 MB, any subject -- a product, a pet --
//!   with coarser edges. Used when MODNet isn't there.
//!
//! RMBG (BRIA) is deliberately not here: its licence is non-commercial, and
//! photos Eric posts for income are commercial use.
//!
//! Both files are optional downloads (`getpieces::photos`), pinned by
//! SHA-256. A missing one is a sentence -- what it's for, how big, how to get
//! it -- not an error.
//!
//! The pre- and post-processing follow rembg's (MIT) per-model recipes:
//! MODNet takes RGB scaled to -1..1 with the short side at 512 and both sides
//! a multiple of 32, and gives the matte directly; u2netp takes 320x320 with
//! ImageNet's mean and deviation after dividing by the brightest value, and
//! its first output is stretched to 0..1.
//!
//! Speed, measured 29 Sep 2026 on a two-core Linux container with tract
//! 0.23.7 in an optimised build: MODNet ~2.9 s for a 768x512 input, u2netp
//! ~1.9 s at 320x320, each plus ~0.25 s to load and optimise. An unoptimised
//! (test) build is about ten times slower. Not yet timed on Eric's laptop.

use std::path::{Path, PathBuf};

/// Which model made a matte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Matter {
    /// MODNet: people, soft edges.
    Portrait,
    /// u2netp: any subject, coarse edges.
    Anything,
}

impl Matter {
    /// Where its file lives, under `models/`.
    pub fn file(self) -> &'static str {
        match self {
            Matter::Portrait => "modnet.onnx",
            Matter::Anything => "u2netp.onnx",
        }
    }

    pub fn plain(self) -> &'static str {
        match self {
            Matter::Portrait => "the portrait cut-out model",
            Matter::Anything => "the general cut-out model",
        }
    }

    /// The picture size this model wants for a `w` x `h` photo.
    ///
    /// MODNet: the short side at 512, both sides rounded to a multiple of 32
    /// (the network halves the picture five times). u2netp: always 320x320.
    pub fn input_size(self, w: u32, h: u32) -> (u32, u32) {
        match self {
            Matter::Anything => (320, 320),
            Matter::Portrait => {
                let (w, h) = (w.max(1) as f64, h.max(1) as f64);
                let k = 512.0 / w.min(h);
                let r32 = |v: f64| (((v * k) / 32.0).round().max(1.0) as u32) * 32;
                (r32(w), r32(h))
            }
        }
    }
}

/// The model that would be used, and its path, if one is installed.
/// MODNet first, u2netp as the fallback.
pub fn installed(models: &Path) -> Option<(Matter, PathBuf)> {
    [Matter::Portrait, Matter::Anything]
        .into_iter()
        .map(|m| (m, models.join(m.file())))
        .find(|(_, p)| p.is_file())
}

/// Said when neither model is here. Plain words: what it's for, what it
/// costs, and what to say to get it.
pub fn missing() -> String {
    let mb = crate::getpieces::photos().iter().map(|p| p.bytes).sum::<u64>().div_ceil(1_000_000);
    format!(
        "Blurring or removing a background needs the cut-out models, and they aren't on this machine yet. \
         They're free and about {mb} MB -- say \"get the photo models\" and I'll fetch them."
    )
}

/// A matte: one byte per pixel, 255 = subject, 0 = background.
#[derive(Debug, Clone, PartialEq)]
pub struct Matte {
    pub width: u32,
    pub height: u32,
    pub alpha: Vec<u8>,
    pub by: Matter,
    /// How long the model took, for the record and the tests.
    pub ms: u128,
}

impl Matte {
    /// The share of the picture that is subject (alpha over half).
    pub fn subject_share(&self) -> f32 {
        if self.alpha.is_empty() {
            return 0.0;
        }
        self.alpha.iter().filter(|&&a| a >= 128).count() as f32 / self.alpha.len() as f32
    }
}

/// The picture laid out and scaled for `by`: planar RGB, `[1, 3, h, w]`.
fn prepare(by: Matter, rgb: &[u8]) -> Vec<f32> {
    let n = rgb.len() / 3;
    let mut out = vec![0f32; n * 3];
    match by {
        Matter::Portrait => {
            for i in 0..n {
                for c in 0..3 {
                    out[c * n + i] = (rgb[i * 3 + c] as f32 / 255.0 - 0.5) / 0.5;
                }
            }
        }
        Matter::Anything => {
            // rembg divides by the brightest value in the picture, not 255.
            let top = rgb.iter().copied().max().unwrap_or(255).max(1) as f32;
            let mean = [0.485f32, 0.456, 0.406];
            let dev = [0.229f32, 0.224, 0.225];
            for i in 0..n {
                for c in 0..3 {
                    out[c * n + i] = (rgb[i * 3 + c] as f32 / top - mean[c]) / dev[c];
                }
            }
        }
    }
    out
}

/// The model's answer as bytes: MODNet's is already 0..1; u2netp's is
/// stretched between its own lowest and highest value, as rembg does.
fn finish(by: Matter, raw: &[f32]) -> Vec<u8> {
    let (lo, hi) = match by {
        Matter::Portrait => (0.0, 1.0),
        Matter::Anything => {
            let lo = raw.iter().copied().fold(f32::INFINITY, f32::min);
            let hi = raw.iter().copied().fold(f32::NEG_INFINITY, f32::max);
            (lo, hi)
        }
    };
    let span = (hi - lo).max(1e-6);
    raw.iter().map(|v| (((v - lo) / span).clamp(0.0, 1.0) * 255.0).round() as u8).collect()
}

/// Make a matte for a picture already sized for `by` (`input_size`), as
/// interleaved RGB bytes.
#[cfg(feature = "onnx")]
pub fn matte(by: Matter, model: &Path, rgb: &[u8], w: u32, h: u32) -> Result<Matte, String> {
    use tract_onnx::prelude::*;
    let (w, h) = (w as usize, h as usize);
    if rgb.len() != w * h * 3 {
        return Err(format!("the picture for {} was the wrong size", by.plain()));
    }
    let began = std::time::Instant::now();
    let plan = tract_onnx::onnx()
        .model_for_path(model)
        .map_err(|e| format!("I couldn't read {}: {e}", by.plain()))?
        .with_input_fact(0, f32::fact([1, 3, h, w]).into())
        .and_then(|m| m.into_optimized())
        .and_then(|m| m.into_runnable())
        .map_err(|e| format!("I couldn't get {} ready: {e}", by.plain()))?;
    let input: Tensor = tract_ndarray::Array4::from_shape_vec((1, 3, h, w), prepare(by, rgb))
        .map_err(|e| e.to_string())?
        .into();
    let out = plan.run(tvec!(input.into())).map_err(|e| format!("{} failed: {e}", by.plain()))?;
    let first = out.first().ok_or_else(|| format!("{} gave nothing back", by.plain()))?;
    let first = first.cast_to::<f32>().map_err(|e| e.to_string())?;
    let tensor: &Tensor = &first;
    let view = tensor.view();
    let view = view.as_slice::<f32>().map_err(|e| e.to_string())?;
    if view.len() != w * h {
        return Err(format!("{} gave back a matte of the wrong size ({} values for {w}x{h})", by.plain(), view.len()));
    }
    Ok(Matte { width: w as u32, height: h as u32, alpha: finish(by, view), by, ms: began.elapsed().as_millis() })
}

/// This build has no in-process models (a phone build): said, not failed.
#[cfg(not(feature = "onnx"))]
pub fn matte(by: Matter, _model: &Path, _rgb: &[u8], _w: u32, _h: u32) -> Result<Matte, String> {
    let _ = (prepare(by, &[]), finish(by, &[]));
    Err("This copy of Atlas was built without its picture models, so it can't cut out a background here.".into())
}
