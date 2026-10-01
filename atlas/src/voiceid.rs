//! Recognising your voice.
//!
//! You asked for this at the very start and I talked you out of it. That was
//! half right: voice is genuinely weak as *authentication*, because a
//! recording of you passes. It is genuinely useful as a *filter*, because the
//! television, a podcast, and someone at the next desk should not be able to
//! drive your workspace.
//!
//! So the rule this module enforces:
//!
//! > Voice identity decides whether Atlas **listens**. It never decides
//! > whether Atlas is **allowed**. Anything consequential still goes through
//! > the approval gate, where a human answer is required.
//!
//! Embeddings come from an external speaker-encoder; everything here is the
//! comparison and the policy, which is where the mistakes actually live.

use crate::error::{AtlasError, Result};
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};

/// How sure Atlas is that it was you.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    You(f32),
    NotYou(f32),
    /// In the grey band. Common with a cold, a bad mic, or distance.
    Unsure(f32),
    /// Nothing to compare against yet.
    NotEnrolled,
}

impl Verdict {
    pub fn score(&self) -> f32 {
        match self {
            Verdict::You(s) | Verdict::NotYou(s) | Verdict::Unsure(s) => *s,
            Verdict::NotEnrolled => 0.0,
        }
    }
}

/// What to do about it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Handling {
    /// Treat normally.
    Proceed,
    /// Sounded like you, but the action matters. Confirm out loud.
    Confirm,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Voiceprint {
    /// Enrollment embeddings, kept so the centroid can be recomputed.
    pub samples: Vec<Vec<f32>>,
    pub centroid: Vec<f32>,
    pub enrolled_at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct VoiceIdConfig {
    pub enabled: bool,
    /// At or above this, it's you.
    pub accept: f32,
    /// At or below this, it isn't.
    pub reject: f32,
    /// Enrollment samples needed before the check is trusted at all.
    pub min_samples: usize,
    /// Keep adapting to your voice as it changes day to day.
    pub adapt: bool,
    /// The same two lines for Atlas's built-in encoder (`speaker`, GMM-UBM
    /// supervectors), whose scores sit on a different scale. Measured, not
    /// chosen: on the round-5 corpus (six voices, ten sentences each, five
    /// enrolled) 2% of other voices scored above 0.28 and none of your own
    /// below 0.40 — so "you" starts at 0.30 and "not you" at 0.15, with the
    /// grey band between. `tests/voice_measured.rs` checks them against that
    /// measurement every run.
    pub builtin_accept: f32,
    pub builtin_reject: f32,
}

impl Default for VoiceIdConfig {
    fn default() -> Self {
        VoiceIdConfig {
            enabled: false,
            // Deliberately wide grey band. A false reject makes Atlas ignore
            // you, which is the more annoying failure of the two.
            accept: 0.72,
            reject: 0.45,
            min_samples: 3,
            adapt: true,
            builtin_accept: 0.30,
            builtin_reject: 0.15,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct VoiceId {
    pub print: Option<Voiceprint>,
    /// Scores of turns that matched, kept so the fixed thresholds can be
    /// MEASURED against where your voice actually lands.
    ///
    /// This is deliberately the whole of what is done with them. Whether the
    /// thresholds may move toward these scores is an open ruling — a
    /// threshold that adapts toward accepted samples accepts more over time,
    /// which is right for ergonomics and wrong for a credential. Until that
    /// is ruled on, Atlas measures and says; it does not move anything.
    #[serde(default)]
    pub accepted: Vec<f32>,
    /// Scores of every turn that came after the wake word, whatever the
    /// verdict: in your own home those are nearly always you, so unlike
    /// `accepted` (only what already passed) they show how low your voice
    /// really goes on an off day (30 Sep 2026).
    #[serde(default)]
    pub after_name: Vec<f32>,
    /// Scores of open-floor voices judged not you and left unanswered: the
    /// television, the radio, someone else.
    #[serde(default)]
    pub turned_away: Vec<f32>,
}

impl VoiceId {
    pub fn load(store: &Store) -> VoiceId {
        store.load("voiceprint")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("voiceprint", self)
    }

    pub fn enrolled(&self) -> usize {
        self.print.as_ref().map(|p| p.samples.len()).unwrap_or(0)
    }

    /// Add an enrollment sample. Several are needed because one recording
    /// captures one mood, one distance and one microphone.
    pub fn enroll(&mut self, embedding: &[f32]) -> Result<usize> {
        if embedding.is_empty() {
            return Err(AtlasError::Platform("empty voice embedding".into()));
        }
        let p = self.print.get_or_insert_with(Voiceprint::default);
        if let Some(first) = p.samples.first() {
            if first.len() != embedding.len() {
                return Err(AtlasError::Platform(
                    "voice embedding size changed — re-enroll after changing the encoder".into(),
                ));
            }
        }
        p.samples.push(embedding.to_vec());
        p.enrolled_at = now();
        p.centroid = mean(&p.samples);
        Ok(p.samples.len())
    }

    pub fn forget(&mut self) {
        self.print = None;
        // A voice that has been forgotten leaves no score history either —
        // the distribution IS a sketch of the voice.
        self.accepted.clear();
        self.after_name.clear();
        self.turned_away.clear();
    }

    /// Note the score of a turn that matched. Bounded the same way `adapt`'s
    /// samples are: enough to see a distribution, not a diary.
    pub fn note_accepted(&mut self, score: f32) {
        self.accepted.push(score);
        if self.accepted.len() > 60 {
            self.accepted.remove(0);
        }
    }

    /// Note the score of a turn after the wake word (see `after_name`).
    pub fn note_after_name(&mut self, score: f32) {
        self.after_name.push(score);
        if self.after_name.len() > 100 {
            self.after_name.remove(0);
        }
    }

    /// Note the score of a voice turned away on the open floor.
    pub fn note_turned_away(&mut self, score: f32) {
        self.turned_away.push(score);
        if self.turned_away.len() > 100 {
            self.turned_away.remove(0);
        }
    }

    /// The evidence for where the "not you" line should sit, from your own
    /// turns: how low your voice scored after the wake word, how high the
    /// voices Atlas turned away scored, and the line between. `None` until
    /// there are at least ten turns after the wake word. Said, never applied
    /// by itself: moving the line is your call.
    pub fn calibration_report(&self, reject: f32) -> Option<String> {
        if self.after_name.len() < 10 {
            return None;
        }
        let mut yours = self.after_name.clone();
        yours.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let low = yours[yours.len() / 20]; // the 5th percentile
        let below = yours.iter().filter(|s| **s <= reject).count();
        let mut line = format!(
            "after the wake word ({} turns, nearly always you) your voice scored as low as {low:.2};              {below} of them were at or under the \"not you\" line ({reject:.2})",
            yours.len()
        );
        if let Some(top) = self.turned_away.iter().cloned().fold(None, |m: Option<f32>, s| Some(m.map_or(s, |m| m.max(s)))) {
            line.push_str(&format!("; the voices turned away scored at most {top:.2}"));
            if top < low {
                line.push_str(&format!(", so a line near {:.2} would keep both", (top + low) / 2.0));
            } else {
                line.push_str(" -- they overlap with yours, so more enrolment samples would help more than moving the line");
            }
        }
        if below > 0 {
            line.push_str(". Some of your turns would have been ignored on the open floor; saying \"Atlas\" first always works");
        }
        Some(line)
    }

    /// Where the fixed thresholds sit against your own accepted scores.
    ///
    /// Measurement without action — the evidence the threshold ruling needs,
    /// produced by the code that would be governed by it. `None` until
    /// enough turns have matched for the middle to mean anything.
    pub fn thresholds_report(&self, cfg: &VoiceIdConfig) -> Option<String> {
        if self.accepted.len() < 10 {
            return None;
        }
        let v: Vec<f64> = self.accepted.iter().map(|s| *s as f64).collect();
        let (usually, varies_by) = crate::judgment::ordinary_for(&v)?;
        if varies_by <= 0.0 {
            return None;
        }
        let gap = (usually - cfg.accept as f64) / varies_by;
        Some(format!(
            "your matched turns usually score {usually:.2} (±{varies_by:.2}); the accept \
             line at {:.2} sits {gap:.1} of your own variations below that{}",
            cfg.accept,
            if gap < 2.0 {
                " — close enough that an off day will read as a stranger"
            } else {
                ""
            }
        ))
    }

    pub fn check(&self, embedding: &[f32], cfg: &VoiceIdConfig) -> Verdict {
        let Some(p) = &self.print else { return Verdict::NotEnrolled };
        if p.samples.len() < cfg.min_samples || p.centroid.len() != embedding.len() {
            return Verdict::NotEnrolled;
        }
        let s = cosine(&p.centroid, embedding);
        let (accept, reject) = if embedding.len() == crate::speaker::BUILTIN_DIMS {
            (cfg.builtin_accept, cfg.builtin_reject)
        } else {
            (cfg.accept, cfg.reject)
        };
        if s >= accept {
            Verdict::You(s)
        } else if s <= reject {
            Verdict::NotYou(s)
        } else {
            Verdict::Unsure(s)
        }
    }

    /// Fold a confirmed sample back in, so a cold or a new headset does not
    /// gradually lock you out. Bounded, or one bad day skews the centroid.
    pub fn adapt(&mut self, embedding: &[f32], cfg: &VoiceIdConfig) {
        if !cfg.adapt {
            return;
        }
        let Some(p) = &mut self.print else { return };
        if p.centroid.len() != embedding.len() {
            return;
        }
        p.samples.push(embedding.to_vec());
        if p.samples.len() > 20 {
            p.samples.remove(0);
        }
        p.centroid = mean(&p.samples);
    }
}

/// The policy, kept separate from the maths because this is the part that
/// matters.
///
/// `consequential` means the action changes something outside Atlas. The rule
/// now has no exceptions: **a voice reading may only ever escalate.** It can
/// turn an action into a question. It can never refuse one, grant one, or
/// make one silently not happen.
///
/// # What was here before, and why it is gone
///
/// `NotYou` used to return `Ignore`, and `daemon.rs` acted on it by breaking
/// out of the turn: no answer, no error, nothing logged where you would see
/// it. The intent was a podcast in the background never reaching the command
/// path. The effect was a lockout, because nothing distinguishes a podcast
/// from you with a cold, you on a new headset, or you sitting further from
/// the microphone — `check` compares an embedding to a centroid and returns
/// a number. On the wrong side of a threshold, Atlas went deaf to its owner
/// and did not say so.
///
/// That is the worse failure of the two by a long way, and it is not close.
/// An unfamiliar voice now behaves exactly like an unsure one: harmless
/// things are answered, consequential ones are asked about out loud. The
/// podcast case is handled by the thing that actually distinguishes it — a
/// consequential action needs a spoken yes, in the moment, and the podcast
/// is not listening for the question.
pub fn handle(v: Verdict, consequential: bool, cfg: &VoiceIdConfig) -> Handling {
    if !cfg.enabled {
        return Handling::Proceed;
    }
    match v {
        // Never gate on a system that hasn't been taught your voice.
        Verdict::NotEnrolled => Handling::Proceed,
        Verdict::You(_) => {
            if consequential {
                // Voice is not proof. A recording of you sounds like you.
                Handling::Confirm
            } else {
                Handling::Proceed
            }
        }
        Verdict::Unsure(_) => {
            if consequential {
                Handling::Confirm
            } else {
                Handling::Proceed
            }
        }
        // Deliberately identical to `Unsure`. "Not you" from a cosine
        // distance is a *maybe* wearing a confident name, and treating it as
        // more than that is what locked the owner out.
        Verdict::NotYou(_) => {
            if consequential {
                Handling::Confirm
            } else {
                Handling::Proceed
            }
        }
    }
}

/// Cosine similarity. Embeddings are compared by direction, not magnitude,
/// so loudness and distance from the mic do not dominate.
pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

fn mean(samples: &[Vec<f32>]) -> Vec<f32> {
    let Some(first) = samples.first() else { return Vec::new() };
    let mut out = vec![0.0f32; first.len()];
    for s in samples {
        if s.len() != out.len() {
            continue;
        }
        for (i, v) in s.iter().enumerate() {
            out[i] += v;
        }
    }
    let n = samples.len() as f32;
    for v in out.iter_mut() {
        *v /= n;
    }
    out
}

/// What Atlas says while learning your voice.
pub fn enrollment_prompt(done: usize, needed: usize) -> String {
    if done >= needed {
        return "That's enough — I know your voice now.".into();
    }
    match done {
        0 => "Say a sentence or two in your normal voice.".into(),
        _ => format!("Got it, {done} of {needed}. Once more, a bit differently."),
    }
}
