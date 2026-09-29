//! Saying "I don't know".
//!
//! A small local model will answer anything, confidently, including things it
//! has no idea about. That's worse than a slower model, because a wrong answer
//! delivered in the same tone as a right one has to be checked — and if you
//! have to check everything, the assistant has saved you nothing.
//!
//! So answers are inspected before they're spoken. Not for truth, which can't
//! be measured here, but for the shapes an answer takes when a model is
//! filling a gap: invented specifics, hedging stacked on hedging, and claims
//! about your machine that Atlas has no way to know.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confidence {
    /// Say it.
    Fine,
    /// Say it, but say what's uncertain.
    Qualify,
    /// Don't say it. Say you don't know.
    Withhold,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct CertaintyConfig {
    pub enabled: bool,
    /// Below this it's qualified; well below, withheld.
    pub qualify_below: f32,
    pub withhold_below: f32,
}

impl Default for CertaintyConfig {
    fn default() -> Self {
        CertaintyConfig { enabled: true, qualify_below: 0.62, withhold_below: 0.38 }
    }
}

/// What Atlas knew when it answered.
#[derive(Debug, Clone, Default)]
pub struct Grounding {
    /// The answer came from something Atlas actually read — a file, a page,
    /// its own state.
    pub from_a_source: bool,
    /// The question was about the machine, your files, or your history.
    pub about_your_world: bool,
    /// Atlas had that context to hand.
    pub had_the_context: bool,
}

impl Grounding {
    /// Read out of something Atlas holds — a research note, its own record of
    /// a decision it made, a file it opened this turn.
    ///
    /// This is the case the single production caller could never express. It
    /// passed `Grounding::default()`, which says *no source, no context* — so
    /// an answer Atlas read straight out of your notes was scored as
    /// invention, lost the grounded credit, and was penalised again by the
    /// "claim about your machine without checking" rule for containing the
    /// word "your". The module was right; nothing ever told it the truth.
    pub fn from_what_it_holds() -> Self {
        Grounding { from_a_source: true, about_your_world: true, had_the_context: true }
    }

    /// A research note, graded by whether it actually read anything.
    ///
    /// `sources` is the count the note carries. Zero sources is a write-up
    /// with nothing behind it, which is exactly the shape this module exists
    /// to catch.
    pub fn from_sources(sources: usize) -> Self {
        Grounding {
            from_a_source: sources > 0,
            about_your_world: false,
            had_the_context: sources > 0,
        }
    }
}

/// Phrases that mean the model is hedging rather than answering.
const HEDGES: &[&str] = &[
    "i think", "i believe", "probably", "might be", "could be", "as far as i know",
    "if i recall", "i'm not sure", "im not sure", "it seems", "possibly", "perhaps",
    "generally", "typically", "usually", "i would guess", "presumably",
];

/// Claims about your machine that Atlas can only know by having looked.
const ABOUT_YOUR_WORLD: &[&str] = &[
    "your file", "your folder", "your document", "your desktop", "your download",
    "your picture", "you have", "you installed", "your setting", "your app",
    "on your machine", "in your documents", "your last", "you said earlier",
    "your calendar", "your email", "your account", "your project",
];

pub fn assess(answer: &str, g: &Grounding, cfg: &CertaintyConfig) -> (Confidence, f32, String) {
    if !cfg.enabled {
        return (Confidence::Fine, 1.0, String::new());
    }
    let t = answer.to_lowercase();
    let mut score: f32 = 0.75;
    let mut reasons: Vec<&str> = Vec::new();

    // Hedging is the clearest signal. One is normal; three means the model is
    // padding around a gap.
    let hedges = HEDGES.iter().filter(|h| t.contains(**h)).count();
    if hedges >= 3 {
        score -= 0.35;
        reasons.push("it hedged repeatedly");
    } else if hedges == 2 {
        score -= 0.15;
        reasons.push("it hedged");
    }

    // A claim about your machine, made without having looked, is invention.
    // Any possessive claim counts, not only the phrasings listed. "Your
    // Documents folder has..." is exactly the kind of thing a model invents.
    let claims_your_world = ABOUT_YOUR_WORLD.iter().any(|c| t.contains(*c))
        || t.starts_with("your ")
        || t.contains(" your ");
    if claims_your_world && !g.had_the_context {
        score -= 0.4;
        reasons.push("it made a claim about your machine without checking");
    }
    if g.about_your_world && !g.had_the_context {
        score -= 0.2;
        reasons.push("the question was about your setup and I had nothing to go on");
    }

    // Grounded in something real is the strongest positive signal there is.
    if g.from_a_source {
        score += 0.25;
    }

    // Invented specifics: precise numbers in an answer with no source.
    if !g.from_a_source && has_precise_numbers(answer) {
        score -= 0.2;
        reasons.push("it gave exact figures with nothing behind them");
    }

    let score = score.clamp(0.0, 1.0);
    let level = if score < cfg.withhold_below {
        Confidence::Withhold
    } else if score < cfg.qualify_below {
        Confidence::Qualify
    } else {
        Confidence::Fine
    };
    (level, score, reasons.join(", "))
}

fn has_precise_numbers(s: &str) -> bool {
    s.split_whitespace().any(|w| {
        let cleaned: String = w.chars().filter(|c| c.is_ascii_digit() || *c == '.').collect();
        cleaned.contains('.') && cleaned.len() >= 4
    })
}

/// Turn an assessment into what Atlas actually says.
///
/// A withheld answer is not silence — it says what it doesn't know and what
/// would settle it, which is more useful than a confident guess.
pub fn phrase(answer: &str, level: Confidence, why: &str) -> String {
    match level {
        Confidence::Fine => answer.to_string(),
        Confidence::Qualify => format!("{answer} I'm not certain — {why}."),
        Confidence::Withhold => {
            if why.is_empty() {
                "I don't know that one.".into()
            } else {
                format!("I don't know — {why}. Ask me to look it up and I will.")
            }
        }
    }
}

/// Both failures produce the same experience — a wrong answer in the same
/// tone as a right one — so they resolve to the same scale. An old basis
/// cannot raise confidence, only lower it: a stale fact stated crisply is
/// still stale.
pub fn aged(
    level: Confidence,
    basis: &[crate::freshness::Known],
    now: u64,
) -> (Confidence, Option<String>) {
    use crate::freshness::State;

    if basis.is_empty() {
        return (level, None);
    }
    // The weakest thing an answer rests on decides it. Averaging would let
    // nine fresh notes hide one that is two years out of date.
    let worst = basis
        .iter()
        .min_by(|a, b| {
            a.weight(now)
                .partial_cmp(&b.weight(now))
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .expect("non-empty");

    match (level, worst.state(now)) {
        (Confidence::Withhold, _) => (Confidence::Withhold, None),
        (_, State::Fresh) => (level, None),
        (_, State::Ageing) => (
            Confidence::Qualify,
            Some(format!(
                "what that rests on is from {}",
                crate::freshness::ago(worst.age_secs(now))
            )),
        ),
        (_, State::Stale) => (
            Confidence::Qualify,
            Some(format!(
                "that rests on something from {} that {}",
                crate::freshness::ago(worst.age_secs(now)),
                worst.shelf.plain()
            )),
        ),
    }
}
