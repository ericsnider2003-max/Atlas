//! Your own wake word, taught from three recordings.
//!
//! The wake word used to mean one of two things: a dedicated detector program
//! fetched from outside (openWakeWord, Porcupine — a model file someone else
//! trained), or running speech-to-text on every two-second clip and looking
//! for the phrase in the transcript, which keeps a CPU core busy all day.
//!
//! This is the classic third way, from before neural detectors (the
//! template-matching keyword spotters of the 1970s–90s; Sakoe & Chiba 1978
//! for the alignment): you say the phrase three times, each take is kept as a
//! sequence of cepstral frames (`mfcc`), and a stretch of audio counts as the
//! phrase when dynamic time warping lines it up closely enough with one of the
//! takes. No model to download, nothing leaves the machine, and it is your
//! phrase in your voice — which also makes it weaker for anyone else's voice,
//! and the measurement says how much.

use crate::mfcc::{self, CEPS};
use serde::{Deserialize, Serialize};

/// One take of the phrase: its frames, trimmed to the speech and mean-normalised.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Take {
    pub frames: Vec<[f32; CEPS]>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WakeModel {
    pub takes: Vec<Take>,
    /// A stretch whose closest alignment is below this is the phrase.
    pub threshold: f32,
}

/// Takes needed before the model is trusted.
pub const TAKES: usize = 3;

/// How far past the takes' own spread a match may be. Measured on the round-5
/// corpus (`tests/voice_measured.rs`): at 1.35 every held-out take of the
/// phrase in the owner's voice was heard, clean and with a fan at 10 dB, and
/// no near-miss ("hey alice", "at least", "hey at last") or sentence was.
pub const MARGIN: f32 = 1.35;

/// Frames of one clip, trimmed to where the voice is.
pub fn phrase_take(samples: &[i16], rate: u32) -> Option<Take> {
    let all = mfcc::frames(samples, rate);
    let peak = all.iter().map(|f| f.db).fold(f32::MIN, f32::max);
    let first = all.iter().position(|f| f.db >= peak - 30.0)?;
    let last = all.iter().rposition(|f| f.db >= peak - 30.0)?;
    if last < first + 20 {
        return None; // under a fifth of a second: not a phrase
    }
    let span: Vec<&mfcc::Frame> = all[first..=last].iter().collect();
    Some(Take { frames: mfcc::normalised(&span) })
}

/// Build the model from the takes. The threshold comes from how far the takes
/// are from each other: saying it three times shows how much your own
/// phrase varies, and anything within that (plus `MARGIN`) is the phrase.
pub fn train(takes: Vec<Take>) -> Result<WakeModel, String> {
    if takes.len() < TAKES {
        return Err(format!("I need {TAKES} takes of the phrase; I have {}", takes.len()));
    }
    let mut worst = 0f32;
    for i in 0..takes.len() {
        for j in i + 1..takes.len() {
            worst = worst.max(distance(&takes[i].frames, &takes[j].frames));
        }
    }
    Ok(WakeModel { takes, threshold: worst * MARGIN })
}

/// Dynamic time warping distance between two frame sequences, averaged over
/// the alignment's length, with the path kept within half the longer
/// sequence of the diagonal (Sakoe–Chiba band) so a phrase cannot be matched
/// by stretching one syllable over the whole take.
pub fn distance(a: &[[f32; CEPS]], b: &[[f32; CEPS]]) -> f32 {
    let (n, m) = (a.len(), b.len());
    if n == 0 || m == 0 {
        return f32::INFINITY;
    }
    let band = (n.max(m) / 2).max(n.abs_diff(m) + 1);
    let inf = f32::INFINITY;
    let mut prev = vec![(inf, 0u32); m + 1];
    let mut cur = vec![(inf, 0u32); m + 1];
    prev[0] = (0.0, 0);
    for i in 1..=n {
        cur[0] = (inf, 0);
        let centre = i * m / n;
        let lo = centre.saturating_sub(band).max(1);
        let hi = (centre + band).min(m);
        for c in cur.iter_mut().take(lo).skip(1) {
            *c = (inf, 0);
        }
        for j in lo..=hi {
            let d = frame_dist(&a[i - 1], &b[j - 1]);
            let best = [prev[j - 1], prev[j], cur[j - 1]]
                .into_iter()
                .min_by(|x, y| x.0.partial_cmp(&y.0).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or((inf, 0));
            cur[j] = (best.0 + d, best.1 + 1);
        }
        for c in cur.iter_mut().skip(hi + 1) {
            *c = (inf, 0);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    let (total, steps) = prev[m];
    if steps == 0 {
        f32::INFINITY
    } else {
        total / steps as f32
    }
}

fn frame_dist(a: &[f32; CEPS], b: &[f32; CEPS]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| (x - y) * (x - y)).sum::<f32>().sqrt()
}

/// The closest any stretch of `samples` comes to any take, and where.
///
/// Slides a window the length of each take (and 20% either side, since you
/// don't say it at exactly the same speed) across the clip every 50 ms.
pub fn closest(samples: &[i16], rate: u32, model: &WakeModel) -> Option<(f32, usize)> {
    let all = mfcc::frames(samples, rate);
    let peak = all.iter().map(|f| f.db).fold(f32::MIN, f32::max);
    let mut best: Option<(f32, usize)> = None;
    for t in &model.takes {
        let base = t.frames.len();
        for len in [base * 8 / 10, base, base * 12 / 10] {
            if len < 10 || len > all.len() {
                continue;
            }
            let mut start = 0;
            while start + len <= all.len() {
                let win = &all[start..start + len];
                // A window that is mostly silence can't be the phrase, and
                // mean-normalising silence produces nonsense that can match.
                let voiced = win.iter().filter(|f| f.db >= peak - 30.0).count();
                if voiced * 2 >= len {
                    let span: Vec<&mfcc::Frame> = win.iter().collect();
                    let d = distance(&mfcc::normalised(&span), &t.frames);
                    if best.map(|b| d < b.0).unwrap_or(true) {
                        best = Some((d, start * 10));
                    }
                }
                start += 5;
            }
        }
    }
    best
}

/// Was the phrase said somewhere in this clip?
pub fn heard(samples: &[i16], rate: u32, model: &WakeModel) -> bool {
    closest(samples, rate, model).map(|(d, _)| d <= model.threshold).unwrap_or(false)
}

const RECORD: &str = "wake_model";

pub fn load(store: &crate::store::Store) -> Option<WakeModel> {
    let m: Option<WakeModel> = store.load(RECORD);
    m.filter(|m| m.takes.len() >= TAKES)
}

fn keep_model(store: &crate::store::Store, m: &WakeModel) -> crate::error::Result<()> {
    store.save(RECORD, &Some(m.clone()))
}

const PENDING: &str = "wake_takes";

/// Add one recording of the phrase. With the third, the model is trained and
/// kept, and the wake loop starts using it. Returns what to tell you.
pub fn add_take(store: &crate::store::Store, samples: &[i16], rate: u32) -> Result<String, String> {
    let t = phrase_take(samples, rate).ok_or("I couldn't find the phrase in that — it needs to be said clearly, with a little quiet around it")?;
    let mut pending: Vec<Take> = store.load(PENDING);
    pending.push(t);
    if pending.len() < TAKES {
        store.save(PENDING, &pending).map_err(|e| e.to_string())?;
        return Ok(format!("Take {} of {TAKES} kept. Say it again the way you normally would.", pending.len()));
    }
    let m = train(pending)?;
    keep_model(store, &m).map_err(|e| e.to_string())?;
    store.save(PENDING, &Vec::<Take>::new()).map_err(|e| e.to_string())?;
    Ok(format!(
        "Learned it from {TAKES} takes. From now on I listen for your phrase directly, without \
         speech-to-text running all the time. It's matched to your voice: someone else saying it \
         may not wake me."
    ))
}

/// Forget the phrase; the wake loop goes back to listening through
/// speech-to-text.
pub fn forget(store: &crate::store::Store) -> crate::error::Result<()> {
    store.save(RECORD, &None::<WakeModel>)?;
    store.save(PENDING, &Vec::<Take>::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sequence_is_nearest_itself_and_warping_absorbs_a_slower_copy() {
        let a: Vec<[f32; CEPS]> = (0..40).map(|i| { let mut f = [0f32; CEPS]; f[0] = (i as f32 / 6.0).sin(); f[1] = (i as f32 / 9.0).cos(); f }).collect();
        let slow: Vec<[f32; CEPS]> = (0..60).map(|i| a[i * 40 / 60]).collect();
        let other: Vec<[f32; CEPS]> = (0..40).map(|i| { let mut f = [0f32; CEPS]; f[0] = (i as f32 / 2.0).cos(); f }).collect();
        assert_eq!(distance(&a, &a), 0.0);
        assert!(distance(&a, &slow) < 0.05, "{}", distance(&a, &slow));
        assert!(distance(&a, &other) > 0.3, "{}", distance(&a, &other));
    }
}
