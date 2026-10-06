//! Who said what: a recording turned into lines with a speaker on each.
//!
//! **Sources:** agglomerative hierarchical clustering with average linkage
//! over cosine similarity (Ward's family of methods; the recipe used by
//! `pyannote.audio` (MIT) after its embedding step and by the NIST RT
//! diarization baselines): every speech segment starts as its own speaker,
//! the two most similar groups merge, and merging stops when no two groups
//! are similar enough to be one voice. Segments come from `vad::segments`;
//! the embeddings from whichever speaker encoder `speaker` runs. Clean-room.
//! The second looks: ΔBIC between full-covariance Gaussians on pooled MFCCs
//! (Chen & Gopalakrishnan 1998) for `merge_same_voices` and `to_count`;
//! leave-one-out Gaussian log-likelihood per frame for `split_strangers`.
//!
//! **Why Atlas wants it.** `call_notes` has been in `tools.yaml` — scope
//! "you only" or "everyone" — with nothing behind it that could tell one
//! voice from another. With the encoder Atlas already uses for voice-lock,
//! a recording becomes "You: … / Speaker 2: …", and "you only" can mean it.
#![allow(clippy::needless_range_loop, reason = "numeric kernels step through several arrays by one index; the index loop is the clear form")]

/// Average-linkage clustering. Returns a cluster number per embedding,
/// numbered in order of first appearance.
fn cluster(embs: &[Vec<f32>], same_voice_at: f32) -> Vec<usize> {
    let n = embs.len();
    let sim: Vec<Vec<f32>> = (0..n).map(|i| (0..n).map(|j| crate::voiceid::cosine(&embs[i], &embs[j])).collect()).collect();
    let mut groups: Vec<Vec<usize>> = (0..n).map(|i| vec![i]).collect();
    loop {
        let mut best: Option<(usize, usize, f32)> = None;
        for a in 0..groups.len() {
            for b in a + 1..groups.len() {
                let mut total = 0.0;
                for i in &groups[a] {
                    for j in &groups[b] {
                        total += sim[*i][*j];
                    }
                }
                let avg = total / (groups[a].len() * groups[b].len()) as f32;
                if best.is_none_or(|x| avg > x.2) {
                    best = Some((a, b, avg));
                }
            }
        }
        match best {
            Some((a, b, s)) if s >= same_voice_at => {
                let moved = groups.remove(b);
                groups[a].extend(moved);
            }
            _ => break,
        }
    }
    let mut label = vec![0; n];
    groups.sort_by_key(|g| *g.iter().min().unwrap_or(&0));
    for (k, g) in groups.iter().enumerate() {
        for i in g {
            label[*i] = k;
        }
    }
    label
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub start_ms: u64,
    pub end_ms: u64,
    pub speaker: String,
    pub text: String,
}

impl Line {
    pub fn say(&self) -> String {
        let s = self.start_ms / 1000;
        format!("[{:02}:{:02}] {}: {}", s / 60, s % 60, self.speaker, self.text)
    }
}

/// The whole pipeline over one recording. `embed` and `transcribe` are the
/// external tools, one segment at a time; `you` is the enrolled voiceprint's
/// centroid, if there is one, and `accept` the voice-lock threshold.
pub fn who_said_what(
    samples: &[i16],
    rate: u32,
    embed: &mut dyn FnMut(&[i16]) -> Option<Vec<f32>>,
    transcribe: &mut dyn FnMut(&[i16]) -> Option<String>,
    you: Option<&[f32]>,
    accept: f32,
) -> Vec<Line> {
    // A little below the voice-lock threshold: grouping a speaker's own
    // segments is an easier question than proving who they are.
    who_said_what_grouped(samples, rate, embed, transcribe, you, accept, (accept - 0.1).max(0.3))
}

/// The same, with the grouping line given: embeddings centred on the
/// recording (`speaker::recording_embeddings`) group at
/// `speaker::GROUP_CENTRED_AT`, not near the voice-lock line.
pub fn who_said_what_grouped(
    samples: &[i16],
    rate: u32,
    embed: &mut dyn FnMut(&[i16]) -> Option<Vec<f32>>,
    transcribe: &mut dyn FnMut(&[i16]) -> Option<String>,
    you: Option<&[f32]>,
    accept: f32,
    group_at: f32,
) -> Vec<Line> {
    let segs = crate::vad::segments(samples, rate);
    let embs: Vec<Option<Vec<f32>>> = segs.iter().map(|(s, e)| embed(&samples[*s..*e])).collect();
    let known: Vec<Vec<f32>> = embs.iter().flatten().cloned().collect();
    let labels = cluster(&known, group_at);
    // Which cluster is you: the one whose average is closest to your print,
    // if it's close enough to pass voice-lock.
    let clusters = labels.iter().copied().max().map(|m| m + 1).unwrap_or(0);
    let mut yours: Option<usize> = None;
    if let Some(p) = you {
        let mut best = (0usize, f32::MIN);
        for c in 0..clusters {
            let members: Vec<&Vec<f32>> = known.iter().zip(&labels).filter(|(_, l)| **l == c).map(|(e, _)| e).collect();
            let avg = members.iter().map(|e| crate::voiceid::cosine(e, p)).sum::<f32>() / members.len().max(1) as f32;
            if avg > best.1 {
                best = (c, avg);
            }
        }
        if best.1 >= accept {
            yours = Some(best.0);
        }
    }
    let mut k = 0usize;
    let mut lines: Vec<Line> = segs.iter()
        .zip(embs.iter())
        .map(|((s, e), emb)| {
            let speaker = match emb {
                Some(_) => {
                    let c = labels[k];
                    k += 1;
                    if Some(c) == yours {
                        "You".to_string()
                    } else {
                        // Numbered by appearance among the others, from 2.
                        let rank = (0..clusters).filter(|x| Some(*x) != yours).position(|x| x == c).unwrap_or(0);
                        format!("Speaker {}", rank + 2)
                    }
                }
                None => "Someone".to_string(),
            };
            Line {
                start_ms: *s as u64 * 1000 / rate as u64,
                end_ms: *e as u64 * 1000 / rate as u64,
                speaker,
                text: transcribe(&samples[*s..*e]).unwrap_or_else(|| "(not transcribed)".into()),
            }
        })
        .collect();
    // A fragment too short for the encoder (a clipped word between pauses)
    // takes the speaker of whatever is within a second of it — before, if
    // there is one — rather than "Someone": encoders refuse short clips, and
    // a fragment inside someone's turn is theirs. Found on real speech in a
    // fan-noise room, 23 Sep.
    for i in 0..lines.len() {
        if lines[i].speaker != "Someone" {
            continue;
        }
        let near = |j: usize| -> bool {
            let (a, b) = (&lines[i], &lines[j]);
            b.speaker != "Someone" && (a.start_ms.saturating_sub(b.end_ms) <= 1000 && b.start_ms.saturating_sub(a.end_ms) <= 1000)
        };
        let pick = (0..i).rev().find(|j| near(*j)).or_else(|| (i + 1..lines.len()).find(|j| near(*j)));
        if let Some(j) = pick {
            lines[i].speaker = lines[j].speaker.clone();
        }
    }
    lines
}

/// 16-bit PCM from a WAV file (mono, or the channels averaged), and its rate.
pub fn read_wav(bytes: &[u8]) -> Result<(Vec<i16>, u32), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err("not a WAV file".into());
    }
    let (mut rate, mut channels) = (0u32, 0u16);
    let mut i = 12;
    while i + 8 <= bytes.len() {
        let id = &bytes[i..i + 4];
        let len = u32::from_le_bytes([bytes[i + 4], bytes[i + 5], bytes[i + 6], bytes[i + 7]]) as usize;
        let body = &bytes[i + 8..(i + 8 + len).min(bytes.len())];
        if id == b"fmt " && body.len() >= 16 {
            let fmt = u16::from_le_bytes([body[0], body[1]]);
            channels = u16::from_le_bytes([body[2], body[3]]);
            rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
            let bits = u16::from_le_bytes([body[14], body[15]]);
            if fmt != 1 || bits != 16 {
                return Err(format!("only 16-bit PCM WAV is read here (this is format {fmt}, {bits}-bit)"));
            }
        } else if id == b"data" {
            if rate == 0 || channels == 0 {
                return Err("the WAV has no format chunk before its data".into());
            }
            let ch = channels as usize;
            let frames: Vec<i16> = body
                .chunks_exact(2 * ch)
                .map(|f| {
                    let sum: i32 = f.as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes([b[0], b[1]]) as i32).sum();
                    (sum / ch as i32) as i16
                })
                .collect();
            return Ok((frames, rate));
        }
        i += 8 + len + (len & 1);
    }
    Err("the WAV has no audio data".into())
}

/// Full-covariance log-likelihood term of a set of frames: N/2 · log|Σ|.
/// `None` when there are too few frames for the covariance to be full rank.
fn half_n_logdet(frames: &[&[f32; crate::mfcc::CEPS]]) -> Option<f64> {
    const D: usize = crate::mfcc::CEPS;
    let n = frames.len();
    if n <= D * 2 {
        return None;
    }
    let mut mean = [0f64; D];
    for f in frames {
        for d in 0..D {
            mean[d] += f[d] as f64;
        }
    }
    for m in mean.iter_mut() {
        *m /= n as f64;
    }
    let mut cov = vec![[0f64; D]; D];
    for f in frames {
        for i in 0..D {
            let a = f[i] as f64 - mean[i];
            for j in 0..=i {
                cov[i][j] += a * (f[j] as f64 - mean[j]);
            }
        }
    }
    // Cholesky; log|Σ| is twice the sum of the log diagonal.
    let (_, logdet) = cholesky(&cov, |i, j| cov[i][j] / n as f64)?;
    Some(n as f64 / 2.0 * logdet)
}

/// A full-covariance Gaussian over MFCC frames: mean and the Cholesky
/// factor of the covariance, and N/2·log|Σ| folded into a per-frame constant.
struct Voice {
    mean: [f64; crate::mfcc::CEPS],
    chol: Vec<[f64; crate::mfcc::CEPS]>,
    half_logdet: f64,
}

fn fit_voice(frames: &[&[f32; crate::mfcc::CEPS]]) -> Option<Voice> {
    const D: usize = crate::mfcc::CEPS;
    let n = frames.len();
    if n <= D * 2 {
        return None;
    }
    let mut mean = [0f64; D];
    for f in frames {
        for d in 0..D {
            mean[d] += f[d] as f64 / n as f64;
        }
    }
    let mut cov = vec![[0f64; D]; D];
    for f in frames {
        for i in 0..D {
            let a = f[i] as f64 - mean[i];
            for j in 0..=i {
                cov[i][j] += a * (f[j] as f64 - mean[j]) / n as f64;
            }
        }
    }
    // A little floor on the diagonal: a few seconds of speech is a thin
    // estimate of nineteen dimensions.
    let (l, logdet) = cholesky(&cov, |i, j| cov[i][j] + if i == j { 1e-3 } else { 0.0 })?;
    Some(Voice { mean, chol: l, half_logdet: logdet / 2.0 })
}

/// How well a voice explains some frames: mean log-likelihood per frame
/// (without the constant every voice shares).
fn fits(v: &Voice, frames: &[&[f32; crate::mfcc::CEPS]]) -> f64 {
    const D: usize = crate::mfcc::CEPS;
    let mut total = 0.0;
    for f in frames {
        // Solve L·z = x − μ; the Mahalanobis distance is |z|².
        let mut z = [0f64; D];
        for i in 0..D {
            let mut s = f[i] as f64 - v.mean[i];
            for k in 0..i {
                s -= v.chol[i][k] * z[k];
            }
            z[i] = s / v.chol[i][i];
        }
        total += -0.5 * z.iter().map(|x| x * x).sum::<f64>() - v.half_logdet;
    }
    total / frames.len().max(1) as f64
}

/// How far below the call's median fit (log-likelihood per frame) a turn
/// must fall to be split off, chosen on forty synthetic calls in which some
/// people speak only once (`tests/voice_measured.rs`). On twenty calls it
/// wasn't chosen on: calls exactly right 12 → 15, two people under one name
/// 7 → 4, over-split 1 → 1. So `atlas notes` takes it by default.
pub const STRANGER_MARGIN: f64 = 8.0;

/// A person who speaks only once is easily swallowed by the nearest voice:
/// grouping turns one at a time put such a turn under someone else's name in
/// 17 of 40 measured calls. Here each turn is scored against its label's
/// *other* turns (a full-covariance Gaussian over their MFCCs); a turn that
/// fits more than `margin` worse than the call's median turn fits its own
/// speaker is someone else, and gets a label of its own.
pub fn split_strangers(samples: &[i16], rate: u32, lines: Vec<Line>, margin: f64) -> Vec<Line> {
    type F = [f32; crate::mfcc::CEPS];
    let frames_of = |l: &Line| -> Vec<F> {
        let s = (l.start_ms as usize * rate as usize / 1000).min(samples.len());
        let e = (l.end_ms as usize * rate as usize / 1000).min(samples.len());
        let all = crate::mfcc::frames(&samples[s..e], rate);
        crate::mfcc::voiced(&all, 30.0).iter().map(|f| f.c).collect()
    };
    let per_line: Vec<Vec<F>> = lines.iter().map(frames_of).collect();
    let mut scores: Vec<(usize, f64)> = Vec::new();
    for i in 0..lines.len() {
        let name = &lines[i].speaker;
        if name == "Someone" || per_line[i].is_empty() {
            continue;
        }
        let others: Vec<&F> = (0..lines.len()).filter(|&j| j != i && &lines[j].speaker == name).flat_map(|j| per_line[j].iter()).collect();
        if let Some(v) = fit_voice(&others) {
            let own: Vec<&F> = per_line[i].iter().collect();
            scores.push((i, fits(&v, &own)));
        }
    }
    if scores.len() < 3 {
        return lines;
    }
    let mut sorted: Vec<f64> = scores.iter().map(|s| s.1).collect();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median = sorted[sorted.len() / 2];
    let mut out = lines;
    let mut fresh = 0;
    for (i, s) in scores {
        if s < median - margin && out[i].speaker != "You" {
            fresh += 1;
            // A name no speaker has; renumbered just below.
            out[i].speaker = format!("\u{1}stranger {fresh}");
        }
    }
    number_the_speakers(&mut out);
    out
}

/// When you know how many people were on the call: labels are merged, the
/// most alike first (lowest ΔBIC on pooled speech, whatever its sign), until
/// there are `people`; if there are fewer, the turn that fits its own label
/// worst is given a label of its own, until there are enough. "You" keeps
/// its name. With the count known, the grouping's only open question —
/// how many voices — is answered by the one person who knows.
pub fn to_count(samples: &[i16], rate: u32, lines: Vec<Line>, people: usize) -> Vec<Line> {
    type F = [f32; crate::mfcc::CEPS];
    if people == 0 {
        return lines;
    }
    let frames_of = |l: &Line| -> Vec<F> {
        let s = (l.start_ms as usize * rate as usize / 1000).min(samples.len());
        let e = (l.end_ms as usize * rate as usize / 1000).min(samples.len());
        let all = crate::mfcc::frames(&samples[s..e], rate);
        crate::mfcc::voiced(&all, 30.0).iter().map(|f| f.c).collect()
    };
    let per_line: Vec<Vec<F>> = lines.iter().map(frames_of).collect();
    let mut out = lines;
    let names = |out: &[Line]| -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for l in out {
            if l.speaker != "Someone" && !v.contains(&l.speaker) {
                v.push(l.speaker.clone());
            }
        }
        v
    };
    let pooled = |out: &[Line], name: &str| -> Vec<&F> {
        (0..out.len()).filter(|&i| out[i].speaker == name).flat_map(|i| per_line[i].iter()).collect()
    };
    // Too many: merge the most alike pair.
    loop {
        let now = names(&out);
        if now.len() <= people {
            break;
        }
        let mut best: Option<(usize, usize, f64)> = None;
        for a in 0..now.len() {
            for b in a + 1..now.len() {
                // Too little speech for a full model: ranked last, but still
                // mergeable so the count is always reached.
                let d = delta_bic(&pooled(&out, &now[a]), &pooled(&out, &now[b]), 1.0).unwrap_or(f64::MAX / 2.0);
                if best.is_none_or(|x| d < x.2) {
                    best = Some((a, b, d));
                }
            }
        }
        let Some((a, b, _)) = best else { break };
        let (keep, gone) = if now[b] == "You" { (now[b].clone(), now[a].clone()) } else { (now[a].clone(), now[b].clone()) };
        for l in out.iter_mut().filter(|l| l.speaker == gone) {
            l.speaker = keep.clone();
        }
    }
    // Too few: split off the turn that fits its own speaker worst.
    let mut fresh = 0;
    while names(&out).len() < people {
        let mut worst: Option<(usize, f64)> = None;
        for i in 0..out.len() {
            if out[i].speaker == "Someone" || out[i].speaker == "You" || per_line[i].is_empty() {
                continue;
            }
            let others: Vec<&F> = (0..out.len()).filter(|&j| j != i && out[j].speaker == out[i].speaker).flat_map(|j| per_line[j].iter()).collect();
            if let Some(v) = fit_voice(&others) {
                let own: Vec<&F> = per_line[i].iter().collect();
                let s = fits(&v, &own);
                if worst.is_none_or(|w| s < w.1) {
                    worst = Some((i, s));
                }
            }
        }
        let Some((i, _)) = worst else { break };
        fresh += 1;
        out[i].speaker = format!("\u{1}new {fresh}");
    }
    number_the_speakers(&mut out);
    out
}

/// ΔBIC for "these two sets of frames are one voice" (Chen & Gopalakrishnan,
/// 1998): the likelihood lost by modelling both with one full-covariance
/// Gaussian, less the penalty for the parameters a second Gaussian would
/// cost. Below zero, one voice explains them better than two.
fn delta_bic(a: &[&[f32; crate::mfcc::CEPS]], b: &[&[f32; crate::mfcc::CEPS]], lambda: f64) -> Option<f64> {
    const D: f64 = crate::mfcc::CEPS as f64;
    let both: Vec<&[f32; crate::mfcc::CEPS]> = a.iter().chain(b.iter()).copied().collect();
    let n = both.len() as f64;
    let penalty = 0.5 * (D + 0.5 * D * (D + 1.0)) * n.ln();
    Some(half_n_logdet(&both)? - half_n_logdet(a)? - half_n_logdet(b)? - lambda * penalty)
}

/// The penalty weight for [`merge_same_voices`], chosen on forty synthetic
/// calls (`tests/voice_measured.rs`): 1.1 got the most calls exactly right
/// without mixing more people than grouping alone. On twenty calls it wasn't
/// chosen on, it fixed the one over-split and mixed two people once — which
/// is why `atlas notes` only takes the second look when asked
/// (`--merge-voices`).
pub const SAME_VOICE_LAMBDA: f64 = 1.1;

/// A second look at who's who, on whole speakers rather than single turns.
///
/// Grouping turns one at a time can split one person in two: a turn whose
/// words differ most from their others looks like a new voice. Here every
/// label's speech is pooled — seconds of it, not one sentence — and two labels
/// merge when one voice explains their pooled speech better than two (ΔBIC
/// below zero), most alike first, until no pair qualifies. It only ever
/// merges, so it can't split what was right; a label that merges into "You"
/// stays "You", and the others are renumbered by first appearance.
pub fn merge_same_voices(samples: &[i16], rate: u32, lines: Vec<Line>, lambda: f64) -> Vec<Line> {
    let mut labels: Vec<String> = Vec::new();
    for l in &lines {
        if l.speaker != "Someone" && !labels.contains(&l.speaker) {
            labels.push(l.speaker.clone());
        }
    }
    let frames_of = |l: &Line| -> Vec<[f32; crate::mfcc::CEPS]> {
        let s = (l.start_ms as usize * rate as usize / 1000).min(samples.len());
        let e = (l.end_ms as usize * rate as usize / 1000).min(samples.len());
        let all = crate::mfcc::frames(&samples[s..e], rate);
        crate::mfcc::voiced(&all, 30.0).iter().map(|f| f.c).collect()
    };
    let per_line: Vec<Vec<[f32; crate::mfcc::CEPS]>> = lines.iter().map(frames_of).collect();
    // Which label each label has become.
    let mut into: Vec<usize> = (0..labels.len()).collect();
    fn root(into: &[usize], mut i: usize) -> usize {
        while into[i] != i {
            i = into[i];
        }
        i
    }
    loop {
        let groups: Vec<usize> = (0..labels.len()).filter(|&i| root(&into, i) == i).collect();
        let pooled = |g: usize| -> Vec<&[f32; crate::mfcc::CEPS]> {
            lines
                .iter()
                .zip(&per_line)
                .filter(|(l, _)| labels.iter().position(|x| x == &l.speaker).map(|i| root(&into, i)) == Some(g))
                .flat_map(|(_, f)| f.iter())
                .collect()
        };
        let mut best: Option<(usize, usize, f64)> = None;
        for (x, &a) in groups.iter().enumerate() {
            for &b in &groups[x + 1..] {
                if let Some(d) = delta_bic(&pooled(a), &pooled(b), lambda) {
                    if d < 0.0 && best.is_none_or(|(_, _, bd)| d < bd) {
                        best = Some((a, b, d));
                    }
                }
            }
        }
        let Some((a, b, _)) = best else { break };
        // "You" wins the name; otherwise the earlier label does.
        let (keep, gone) = if labels[b] == "You" { (b, a) } else { (a, b) };
        into[gone] = keep;
    }
    // Rename: each label to its group's surviving name, then renumber others.
    let mut order: Vec<String> = Vec::new();
    let mut out = lines;
    for l in out.iter_mut() {
        if let Some(i) = labels.iter().position(|x| x == &l.speaker) {
            l.speaker = labels[root(&into, i)].clone();
        }
        if l.speaker != "You" && l.speaker != "Someone" && !order.contains(&l.speaker) {
            order.push(l.speaker.clone());
        }
    }
    for l in out.iter_mut() {
        if let Some(k) = order.iter().position(|x| x == &l.speaker) {
            l.speaker = format!("Speaker {}", k + 2);
        }
    }
    out
}

/// Everyone but you and "Someone" renamed "Speaker 2", "Speaker 3"... in the
/// order they first speak. One copy (audit Q3).
fn number_the_speakers(out: &mut [Line]) {
    let mut order: Vec<String> = Vec::new();
    for l in out.iter() {
        if l.speaker != "You" && l.speaker != "Someone" && !order.contains(&l.speaker) {
            order.push(l.speaker.clone());
        }
    }
    for l in out.iter_mut() {
        if let Some(k) = order.iter().position(|x| x == &l.speaker) {
            l.speaker = format!("Speaker {}", k + 2);
        }
    }
}

/// The Cholesky factor of a symmetric matrix whose lower-triangle entries are
/// `entry(i, j)`, with log|Σ| (twice the sum of the log diagonal). `None`
/// when it isn't positive definite. One copy (audit Q3): the change score and
/// a voice's model each had one.
fn cholesky<const D: usize>(_shape: &[[f64; D]], entry: impl Fn(usize, usize) -> f64) -> Option<(Vec<[f64; D]>, f64)> {
    let mut l = vec![[0f64; D]; D];
    let mut logdet = 0.0;
    for i in 0..D {
        for j in 0..=i {
            let mut s = entry(i, j);
            for k in 0..j {
                s -= l[i][k] * l[j][k];
            }
            if i == j {
                if s <= 1e-12 {
                    return None;
                }
                l[i][i] = s.sqrt();
                logdet += 2.0 * l[i][i].ln();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    Some((l, logdet))
}
