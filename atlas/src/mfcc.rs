//! Mel-frequency cepstral coefficients: the numbers speech tools compare.
//!
//! The standard front end (Davis & Mermelstein 1980; the HTK recipe that
//! every speaker and wake-word system since has used): pre-emphasis, 25 ms
//! Hamming frames every 10 ms, a power spectrum, 40 triangular filters spaced
//! on the mel scale, their log, and a DCT-II that keeps the first 20
//! coefficients. c0 (overall loudness) is dropped from what gets compared,
//! because how loud you are says nothing about who you are.
//!
//! Written here rather than taken from a crate so the speaker check
//! (`speaker`), the wake word (`wakeword`) and room calibration all share one
//! front end that nothing outside the tree can change underneath them.
#![allow(clippy::needless_range_loop, reason = "numeric kernels step through several arrays by one index; the index loop is the clear form")]

/// Coefficients kept per frame, c1..c19 (c0 is loudness, dropped).
pub const CEPS: usize = 19;
const FILTERS: usize = 40;

/// One frame's coefficients, and its loudness in dB (for dropping silence).
#[derive(Debug, Clone, PartialEq)]
pub struct Frame {
    pub c: [f32; CEPS],
    pub db: f32,
}

/// Coefficients for every 10 ms of `samples`.
pub fn frames(samples: &[i16], rate: u32) -> Vec<Frame> {
    let rate = rate.max(8000) as usize;
    let len = rate * 25 / 1000;
    let hop = rate / 100;
    let n_fft = len.next_power_of_two();
    if samples.len() < len {
        return Vec::new();
    }
    let bank = mel_bank(n_fft, rate);
    let window: Vec<f64> = (0..len)
        .map(|i| 0.54 - 0.46 * (2.0 * std::f64::consts::PI * i as f64 / (len - 1) as f64).cos())
        .collect();
    let mut out = Vec::new();
    let mut start = 0;
    while start + len <= samples.len() {
        let mut re = vec![0.0f64; n_fft];
        let mut im = vec![0.0f64; n_fft];
        let mut energy = 0.0;
        for i in 0..len {
            let x = samples[start + i] as f64 / 32768.0;
            let prev = if start + i > 0 { samples[start + i - 1] as f64 / 32768.0 } else { 0.0 };
            let y = (x - 0.97 * prev) * window[i];
            re[i] = y;
            energy += x * x;
        }
        fft_in_place(&mut re, &mut im);
        let power: Vec<f64> = (0..n_fft / 2 + 1).map(|k| re[k] * re[k] + im[k] * im[k]).collect();
        let logmel: Vec<f64> = bank
            .iter()
            .map(|f| f.iter().map(|(k, w)| power[*k] * w).sum::<f64>().max(1e-12).ln())
            .collect();
        let mut c = [0f32; CEPS];
        for (n, slot) in c.iter_mut().enumerate() {
            let q = n + 1;
            let s: f64 = logmel
                .iter()
                .enumerate()
                .map(|(m, v)| v * (std::f64::consts::PI * q as f64 * (m as f64 + 0.5) / FILTERS as f64).cos())
                .sum();
            *slot = (s * (2.0 / FILTERS as f64).sqrt()) as f32;
        }
        let db = (10.0 * (energy / len as f64).max(1e-12).log10()) as f32;
        out.push(Frame { c, db });
        start += hop;
    }
    out
}

/// The frames within `below_peak` dB of the loudest: speech, with the
/// silence between words left out.
pub fn voiced(frames: &[Frame], below_peak: f32) -> Vec<&Frame> {
    let peak = frames.iter().map(|f| f.db).fold(f32::MIN, f32::max);
    frames.iter().filter(|f| f.db >= peak - below_peak).collect()
}

/// Subtract each coefficient's own average over the clip (cepstral mean
/// normalisation): removes the microphone and the room's fixed colouring, so
/// the same word through a different mic still looks like the same word.
pub fn normalised(frames: &[&Frame]) -> Vec<[f32; CEPS]> {
    if frames.is_empty() {
        return Vec::new();
    }
    let mut mean = [0f32; CEPS];
    for f in frames {
        for i in 0..CEPS {
            mean[i] += f.c[i];
        }
    }
    for m in mean.iter_mut() {
        *m /= frames.len() as f32;
    }
    frames
        .iter()
        .map(|f| {
            let mut o = [0f32; CEPS];
            for i in 0..CEPS {
                o[i] = f.c[i] - mean[i];
            }
            o
        })
        .collect()
}

fn hz_to_mel(hz: f64) -> f64 {
    2595.0 * (1.0 + hz / 700.0).log10()
}
fn mel_to_hz(mel: f64) -> f64 {
    700.0 * (10f64.powf(mel / 2595.0) - 1.0)
}

/// Triangular filters, 60 Hz to 7.6 kHz (or just under Nyquist), as sparse
/// (bin, weight) lists.
fn mel_bank(n_fft: usize, rate: usize) -> Vec<Vec<(usize, f64)>> {
    let top = (rate as f64 / 2.0 - 200.0).min(7600.0);
    let (lo, hi) = (hz_to_mel(60.0), hz_to_mel(top));
    let pts: Vec<f64> = (0..FILTERS + 2)
        .map(|i| mel_to_hz(lo + (hi - lo) * i as f64 / (FILTERS + 1) as f64) * n_fft as f64 / rate as f64)
        .collect();
    (0..FILTERS)
        .map(|m| {
            let (a, b, c) = (pts[m], pts[m + 1], pts[m + 2]);
            let mut f = Vec::new();
            for k in a.floor() as usize..=(c.ceil() as usize).min(n_fft / 2) {
                let x = k as f64;
                let w = if x < a || x > c {
                    0.0
                } else if x <= b {
                    (x - a) / (b - a).max(1e-9)
                } else {
                    (c - x) / (c - b).max(1e-9)
                };
                if w > 0.0 {
                    f.push((k, w));
                }
            }
            f
        })
        .collect()
}

/// In-place radix-2 FFT over f64 (`re.len()` a power of two). The one copy:
/// `vad` uses this too (until 5 Oct 2026 it carried its own, line for line).
/// `speakernet` keeps an f32 one, because its model works in f32.
pub(crate) fn fft_in_place(re: &mut [f64], im: &mut [f64]) {
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
                let (wr, wi) = ((ang * k as f64).cos(), (ang * k as f64).sin());
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * wr - im[b] * wi;
                let ti = re[b] * wi + im[b] * wr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f64, secs: f64) -> Vec<i16> {
        (0..(16000.0 * secs) as usize)
            .map(|i| ((2.0 * std::f64::consts::PI * hz * i as f64 / 16000.0).sin() * 8000.0) as i16)
            .collect()
    }

    #[test]
    fn a_frame_every_ten_ms_and_different_sounds_differ() {
        let low = frames(&tone(300.0, 1.0), 16000);
        let high = frames(&tone(2500.0, 1.0), 16000);
        assert_eq!(low.len(), (16000 - 400) / 160 + 1);
        let d: f32 = low[50].c.iter().zip(high[50].c.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(d > 5.0, "300 Hz and 2.5 kHz should not look alike: {d}");
        let same: f32 = low[50].c.iter().zip(low[60].c.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(same < 0.5, "a steady tone should look the same frame to frame: {same}");
    }
}
