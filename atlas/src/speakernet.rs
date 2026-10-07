//! Telling your voice from others' with a trained speaker model (30 Sep 2026).
//!
//! Eric's ruling on voices: the open floor ignores a voice that clearly
//! isn't his, and asked how to make that accurate. The built-in encoder
//! (`speaker`, a GMM-UBM supervector) is classical and weak; this is
//! 3D-Speaker's CAM++ (Apache-2.0, github.com/modelscope/3D-Speaker), the
//! English VoxCeleb model as sherpa-onnx publishes it
//! (`3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`, 29.6 MB), run in
//! Atlas with tract like the other ONNX models.
//!
//! **Measured before it was wired (30 Sep 2026)**, on 50 LibriSpeech
//! test-clean utterances from 10 speakers (5 each), against onnxruntime and
//! kaldi-native-fbank as the reference:
//!
//! - The model takes **200 frames (2 s) at a time** -- this export only
//!   works on multiples of 200 frames: whole utterances gave an equal-error
//!   rate of 32%, 1.5 s windows 4%, 3 s windows 22%. 2 s windows, averaged:
//!   0.1%. So that is what `embed` does.
//! - Same speaker scored 0.48 to 0.95 (mean 0.78); different speakers at
//!   most 0.48 (mean 0.13). The lines in `VoiceIdConfig` start from there.
//!
//! The features are Kaldi's 80-bin log-mel filterbank (25 ms Povey window,
//! 10 ms shift, pre-emphasis 0.97, no dither), the mean over time taken off
//! each window, samples as -1..1 (the model's `normalize_samples`).

use crate::error::{AtlasError, Result};
use std::path::Path;

/// The model file, in the models folder (`atlas get voiceid`).
pub const FILE: &str = "campplus_en_voxceleb.onnx";

/// Its output size.
pub const DIMS: usize = 512;

/// Frames per window: the model works only on multiples of this.
pub const WINDOW: usize = 200;

const BINS: usize = 80;
const FRAME: usize = 400;
const SHIFT: usize = 160;
const FFT: usize = 512;

/// Kaldi-compatible 80-bin log-mel filterbank of 16 kHz samples in -1..1.
pub fn fbank(samples: &[f32]) -> Vec<[f32; BINS]> {
    if samples.len() < FRAME {
        return Vec::new();
    }
    let n = 1 + (samples.len() - FRAME) / SHIFT;
    let window: Vec<f32> = (0..FRAME)
        .map(|i| (0.5 - 0.5 * (2.0 * std::f64::consts::PI * i as f64 / (FRAME - 1) as f64).cos()).powf(0.85) as f32)
        .collect();
    let banks = mel_banks();
    let mut out = Vec::with_capacity(n);
    let mut re = vec![0f32; FFT];
    let mut im = vec![0f32; FFT];
    for f in 0..n {
        let x = &samples[f * SHIFT..f * SHIFT + FRAME];
        let mean = x.iter().sum::<f32>() / FRAME as f32;
        let mut frame: Vec<f32> = x.iter().map(|v| v - mean).collect();
        for i in (1..FRAME).rev() {
            frame[i] -= 0.97 * frame[i - 1];
        }
        frame[0] -= 0.97 * frame[0];
        for i in 0..FFT {
            re[i] = if i < FRAME { frame[i] * window[i] } else { 0.0 };
            im[i] = 0.0;
        }
        fft(&mut re, &mut im);
        let power: Vec<f32> = (0..=FFT / 2).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
        let mut mel = [0f32; BINS];
        for (b, (start, weights)) in banks.iter().enumerate() {
            let e: f32 = weights.iter().enumerate().map(|(j, w)| w * power[start + j]).sum();
            mel[b] = e.max(f32::EPSILON).ln();
        }
        out.push(mel);
    }
    out
}

fn mel(f: f64) -> f64 {
    1127.0 * (1.0 + f / 700.0).ln()
}

/// Kaldi's triangular mel filters: 20 Hz to Nyquist, over the FFT's 256
/// bins below Nyquist. Each is (first bin, weights).
fn mel_banks() -> Vec<(usize, Vec<f32>)> {
    let fft_bins = FFT / 2;
    let fft_bin_width = 16000.0 / FFT as f64;
    let (lo, hi) = (mel(20.0), mel(8000.0));
    let delta = (hi - lo) / (BINS + 1) as f64;
    (0..BINS)
        .map(|b| {
            let left = lo + b as f64 * delta;
            let center = left + delta;
            let right = center + delta;
            let mut first = None;
            let mut w = Vec::new();
            for i in 0..fft_bins {
                let m = mel(fft_bin_width * i as f64);
                if m > left && m < right {
                    let weight = if m <= center { (m - left) / (center - left) } else { (right - m) / (right - center) };
                    if first.is_none() {
                        first = Some(i);
                    }
                    w.push(weight as f32);
                } else if first.is_some() {
                    break;
                }
            }
            (first.unwrap_or(0), w)
        })
        .collect()
}

/// In-place radix-2 FFT.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2;
    while len <= n {
        let ang = -2.0 * std::f64::consts::PI / len as f64;
        for start in (0..n).step_by(len) {
            for k in 0..len / 2 {
                let (s, c) = (ang * k as f64).sin_cos();
                let (c, s) = (c as f32, s as f32);
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * c - im[b] * s;
                let ti = re[b] * s + im[b] * c;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

/// The windows the model is shown: 200 frames each, half overlapping, each
/// with its own mean taken off. A clip under 200 frames is repeated up to
/// 200 (under half a second isn't a voice to judge: `None`).
#[cfg(any(feature = "onnx", test))]
fn windows(frames: &[[f32; BINS]]) -> Option<Vec<Vec<f32>>> {
    if frames.len() < 50 {
        return None;
    }
    let mut f: Vec<[f32; BINS]> = frames.to_vec();
    while f.len() < WINDOW {
        let more = f.clone();
        f.extend(more);
    }
    let mut out = Vec::new();
    let mut s = 0;
    while s + WINDOW <= f.len() {
        let w = &f[s..s + WINDOW];
        let mut mean = [0f32; BINS];
        for row in w {
            for (m, v) in mean.iter_mut().zip(row) {
                *m += v / WINDOW as f32;
            }
        }
        out.push(w.iter().flat_map(|row| row.iter().zip(&mean).map(|(v, m)| v - m)).collect());
        s += WINDOW / 2;
    }
    Some(out)
}

/// Unit length.
#[cfg(any(feature = "onnx", test))]
fn normalised(mut v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
    v
}

/// Is the model here?
pub(crate) fn installed(models_dir: &Path) -> bool {
    models_dir.join(FILE).is_file()
}

#[cfg(feature = "onnx")]
type Plan = std::sync::Arc<tract_onnx::prelude::TypedRunnableModel>;

#[cfg(feature = "onnx")]
static LOADED: std::sync::Mutex<Option<(std::path::PathBuf, Plan)>> = std::sync::Mutex::new(None);

/// The voice embedding of 16 kHz samples in -1..1: each 2 s window through
/// the model, the results averaged, unit length.
#[cfg(feature = "onnx")]
pub fn embed(samples: &[f32], models_dir: &Path) -> Result<Vec<f32>> {
    embed_where(samples, models_dir, true)
}

/// The same on the processor only (`tract`), for `atlas npu-check`.
#[cfg(feature = "onnx")]
pub fn embed_on_processor(samples: &[f32], models_dir: &Path) -> Result<Vec<f32>> {
    embed_where(samples, models_dir, false)
}

#[cfg(feature = "onnx")]
fn embed_where(samples: &[f32], models_dir: &Path, npu: bool) -> Result<Vec<f32>> {
    use tract_onnx::prelude::*;
    let wins = windows(&fbank(samples)).ok_or_else(|| AtlasError::Platform("under half a second of speech in that".into()))?;
    let path = models_dir.join(FILE);
    // The NPU first, where there is one (item 20): the same windows, the
    // same averaging.
    if npu {
        let t = std::time::Instant::now();
        if let Some(v) = on_npu(&wins, models_dir) {
            // The first voice on the NPU is checked against the processor's
            // once: kept only if it agrees and is quicker (`npu::worth_keeping`).
            if NPU_JUDGED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                return Ok(v);
            }
            let npu_took = t.elapsed();
            let t = std::time::Instant::now();
            let cpu = embed_where(samples, models_dir, false)?;
            let agree = crate::npu::agreement(&cpu, &v);
            if !crate::npu::worth_keeping(npu_took, t.elapsed(), agree) {
                crate::outln!(
                    "telling voices apart stays on the processor: the NPU took {} ms against {} ms (answers agree to {agree:.3})",
                    npu_took.as_millis(),
                    t.elapsed().as_millis()
                );
                if let Ok(mut g) = ON_NPU.lock().or_else(crate::crash::unpoison) {
                    *g = Some(None);
                }
                return Ok(cpu);
            }
            return Ok(v);
        }
    }
    let plan = {
        let mut g = LOADED.lock().map_err(|_| AtlasError::Platform("the voice model's lock broke".into()))?;
        match g.as_ref() {
            Some((p, plan)) if *p == path => plan.clone(),
            _ => {
                if !path.is_file() {
                    return Err(AtlasError::Config(format!("the voice model isn't installed -- {FILE} should be in models/")));
                }
                let plan = tract_onnx::onnx()
                    .model_for_path(&path)
                    .and_then(|m| m.with_input_fact(0, f32::fact([1, WINDOW, BINS]).into()))
                    .and_then(|m| m.into_optimized())
                    .and_then(|m| m.into_runnable())
                    .map_err(|e| AtlasError::Config(format!("couldn't prepare {FILE}: {e}")))?;
                *g = Some((path.clone(), plan.clone()));
                plan
            }
        }
    };
    let mut sum = vec![0f32; DIMS];
    for w in &wins {
        let input: Tensor = tract_ndarray::Array3::from_shape_vec((1, WINDOW, BINS), w.clone())
            .map_err(|e| AtlasError::Platform(format!("couldn't shape the voice: {e}")))?
            .into();
        let out = plan.run(tvec!(input.into())).map_err(|e| AtlasError::Platform(format!("the voice model failed: {e}")))?;
        let t: &Tensor = &out[0];
        let view = t.view();
        let v = view.as_slice::<f32>().map_err(|e| AtlasError::Platform(format!("couldn't read the voice: {e}")))?;
        if v.len() != DIMS {
            return Err(AtlasError::Platform(format!("the voice model gave {} numbers, not {DIMS}", v.len())));
        }
        for (s, x) in sum.iter_mut().zip(normalised(v.to_vec())) {
            *s += x;
        }
    }
    Ok(normalised(sum))
}

/// Whether the NPU's first voice was checked against the processor's.
#[cfg(feature = "onnx")]
static NPU_JUDGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The model on the NPU: opened once, `None` inside once refused (said
/// once), so it stays on `tract`.
#[cfg(feature = "onnx")]
static ON_NPU: std::sync::Mutex<Option<Option<(crate::npu::Session, String)>>> = std::sync::Mutex::new(None);

/// The NPU's copy is being opened, on its own thread.
#[cfg(feature = "onnx")]
static NPU_OPENING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The voice embedding from the NPU, or `None` to use `tract`.
#[cfg(feature = "onnx")]
fn on_npu(wins: &[Vec<f32>], models_dir: &Path) -> Option<Vec<f32>> {
    let root = models_dir.parent()?;
    if !crate::npu::npu_ready(root) {
        return None;
    }
    let g = ON_NPU.lock().or_else(crate::crash::unpoison).ok()?;
    if g.is_none() {
        // Opened on a thread of its own (6 Oct 2026): the first open
        // compiles the model for the NPU, which took the better part of a
        // minute on the laptop, and this runs on whatever asked -- the loop,
        // during a call -- so the hub and everything else waited with it.
        // The processor answers until the NPU is ready.
        drop(g);
        if !NPU_OPENING.swap(true, std::sync::atomic::Ordering::SeqCst) {
            let (root, model) = (root.to_path_buf(), models_dir.join(FILE));
            let spawned = std::thread::Builder::new().name("atlas-voices-npu".into()).spawn(move || {
                let opened = crate::npu::Session::input_names(&root, &model).ok().and_then(|names| {
                    let name = names.first()?.clone();
                    match crate::npu::Session::open(&root, &model, &[(name.clone(), vec![1, WINDOW as i64, BINS as i64])], crate::npu::Where::Npu) {
                        Ok(s) => Some((s, name)),
                        Err(why) => {
                            crate::outln!("telling voices apart stays on the processor: {why}");
                            None
                        }
                    }
                });
                if let Ok(mut g) = ON_NPU.lock().or_else(crate::crash::unpoison) {
                    *g = Some(opened);
                }
            });
            if spawned.is_err() {
                NPU_OPENING.store(false, std::sync::atomic::Ordering::SeqCst);
            }
        }
        return None;
    }
    let (s, name) = g.as_ref()?.as_ref()?;
    let mut sum = vec![0f32; DIMS];
    for w in wins {
        let out = s.run(vec![crate::npu::In::F32(name.clone(), vec![1, WINDOW as i64, BINS as i64], w.clone())]).ok()?;
        let v = out.into_iter().next()?;
        if v.len() != DIMS {
            return None;
        }
        for (acc, x) in sum.iter_mut().zip(normalised(v)) {
            *acc += x;
        }
    }
    Some(normalised(sum))
}

#[cfg(not(feature = "onnx"))]
pub fn embed(_samples: &[f32], _models_dir: &Path) -> Result<Vec<f32>> {
    Err(AtlasError::Config("this build has no ONNX engine for the voice model".into()))
}

#[cfg(not(feature = "onnx"))]
pub fn embed_on_processor(_samples: &[f32], _models_dir: &Path) -> Result<Vec<f32>> {
    Err(AtlasError::Config("this build has no ONNX engine for the voice model".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_a_second_is_too_little_to_judge() {
        assert!(windows(&fbank(&vec![0.01; 16000 / 4])).is_none());
        // One second: 98 frames, doubled to 392, windows at 0 and 100.
        let w = windows(&fbank(&vec![0.01; 16000])).unwrap();
        assert_eq!(w.len(), 2);
        assert_eq!(w[0].len(), WINDOW * 80);
    }

    #[test]
    fn unit_length() {
        let v = normalised(vec![3.0, 4.0]);
        assert!((v[0] - 0.6).abs() < 1e-6 && (v[1] - 0.8).abs() < 1e-6);
    }
}
