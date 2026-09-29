//! Writing something, then being honest about it.
//!
//! Phase 3. A model asked to write a post writes one, and it is usually
//! mediocre in predictable ways: it opens with throat-clearing, it hedges, it
//! says the same thing twice, and it ends with a question nobody asked.
//!
//! The fix is not a better prompt, it's a second pass. Atlas writes, then
//! reads what it wrote against a list of things that are actually wrong with
//! most first drafts, then rewrites. The critique is the valuable half, and
//! it's worth showing you even when you don't want the rewrite.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Fault {
    /// Opens with a sentence that says nothing.
    ThroatClearing,
    /// Hedges so much it commits to nothing.
    Hedging,
    /// Says the same thing twice in different words.
    Repetition,
    /// Ends by asking the reader something nobody asked for.
    TackedOnQuestion,
    /// Words that sound like effort and mean nothing.
    Filler,
    /// Over the limit for where it's going.
    TooLong,
    /// Nothing specific in it — no number, name, or detail.
    Vague,
    /// Every sentence the same length and shape.
    Monotonous,
    /// Reads like a chatbot, not a person: "I hope this helps!",
    /// "Certainly!", "As an AI…". A message sent as you must never say these.
    Chatbot,
    /// A blank left to fill in: "[Your Name]", "[date]". Never sent.
    Blank,
}

impl Fault {
    pub fn what(&self) -> &'static str {
        match self {
            Fault::ThroatClearing => "opens with a sentence that says nothing",
            Fault::Hedging => "hedges so much it doesn't commit to anything",
            Fault::Repetition => "says the same thing twice",
            Fault::TackedOnQuestion => "ends with a question nobody asked",
            Fault::Filler => "has words in it that sound like effort and mean nothing",
            Fault::TooLong => "is over the limit",
            Fault::Vague => "has nothing specific in it",
            Fault::Monotonous => "every sentence is the same length",
            Fault::Chatbot => "sounds like a chatbot, not a person",
            Fault::Blank => "has a blank left in it to fill in",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub fault: Fault,
    /// The bit that's wrong, so you can see it rather than take Atlas's word.
    pub evidence: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DraftConfig {
    /// Rewrite automatically, or just say what's wrong.
    pub revise: bool,
    /// Passes before it stops fiddling. More than two rarely helps and often
    /// sands the life out of it.
    pub max_passes: u32,
    /// Faults to ignore.
    pub ignore: Vec<Fault>,
}

impl Default for DraftConfig {
    fn default() -> Self {
        DraftConfig { revise: true, max_passes: 2, ignore: Vec::new() }
    }
}

const THROAT_CLEARING: &[&str] = &[
    "in today's world", "in this day and age", "it goes without saying",
    "as we all know", "i wanted to reach out", "i hope this finds you well",
    "let's dive in", "let's take a look", "when it comes to", "at the end of the day",
];

/// The worst tells of machine writing: said by a chatbot, never by a person
/// writing their own message. From the P0/P1 lists of the MIT-licensed
/// avoid-ai-writing skill in wshobson/agents (THIRD_PARTY_NOTICES.md).
const CHATBOT: &[&str] = &[
    "i hope this helps", "hope this helps!", "certainly!", "absolutely!", "great question",
    "feel free to reach out", "don't hesitate to reach out", "do not hesitate to reach out",
    "as an ai", "as a language model", "as of my last update", "as of my knowledge cutoff",
    "i'd be happy to help", "i would be happy to help", "happy to help!", "you're absolutely right",
    "i hope this message finds you well", "i hope this email finds you well",
    "let me know if you have any other questions", "let me know if you have any questions",
    "let's break this down", "let's dive into", "here's my thought process",
];

const HEDGES: &[&str] = &[
    "i think", "perhaps", "maybe", "possibly", "arguably", "somewhat",
    "fairly", "rather", "quite", "sort of", "kind of", "it could be argued",
    "in some ways", "to some extent",
];

const FILLER: &[&str] = &[
    "leverage", "synergy", "utilise", "utilize", "robust", "seamless",
    "cutting-edge", "game-changing", "best-in-class", "holistic", "paradigm",
    "delve", "tapestry", "landscape of", "realm of", "testament to",
    // Tier 1 of avoid-ai-writing's word list (wshobson/agents, MIT).
    "pivotal", "meticulous", "embark on", "underscores", "showcasing", "vibrant",
    "thriving", "nestled", "bustling", "intricacies", "deep dive", "dive into",
    "watershed moment", "game-changer",
];

/// A blank left in a draft: "[Your Name]", "[date]", "[Company]" — square
/// brackets around a short run of words with no digits in it. "[1]" and
/// "[x]" aren't blanks.
pub fn blanks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else { break };
        let inside = &after[..close];
        let words = inside.split_whitespace().count();
        if (1..=4).contains(&words)
            && inside.chars().count() >= 3
            && inside.chars().any(|c| c.is_alphabetic())
            && !inside.chars().any(|c| c.is_ascii_digit())
            && !inside.contains('[')
        {
            out.push(format!("[{inside}]"));
        }
        rest = &after[close + 1..];
    }
    out
}

/// Read a draft and say what's wrong with it.
pub fn critique(text: &str, limit: Option<usize>, cfg: &DraftConfig) -> Vec<Note> {
    let mut notes = Vec::new();
    let lower = text.to_lowercase();
    let sentences: Vec<&str> = text
        .split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    // Opening.
    if let Some(first) = sentences.first() {
        let f = first.to_lowercase();
        if THROAT_CLEARING.iter().any(|t| f.contains(t)) {
            notes.push(Note { fault: Fault::ThroatClearing, evidence: (*first).to_string() });
        }
    }

    // A chatbot's voice.
    let chatbot: Vec<&str> = CHATBOT.iter().filter(|c| lower.contains(**c)).copied().collect();
    if !chatbot.is_empty() {
        notes.push(Note { fault: Fault::Chatbot, evidence: chatbot.join(", ") });
    }
    let blanks = blanks(text);
    if !blanks.is_empty() {
        notes.push(Note { fault: Fault::Blank, evidence: blanks.join(", ") });
    }

    // Hedging — one is fine, three is a refusal to commit.
    let hedges: Vec<&str> = HEDGES.iter().filter(|h| lower.contains(**h)).copied().collect();
    if hedges.len() >= 3 {
        notes.push(Note { fault: Fault::Hedging, evidence: hedges.join(", ") });
    }

    // Filler.
    let filler: Vec<&str> = FILLER.iter().filter(|f| lower.contains(**f)).copied().collect();
    if !filler.is_empty() {
        notes.push(Note { fault: Fault::Filler, evidence: filler.join(", ") });
    }

    // Repetition: two sentences sharing most of their meaningful words.
    for (i, a) in sentences.iter().enumerate() {
        for b in sentences.iter().skip(i + 1) {
            if overlap(a, b) > 0.62 {
                notes.push(Note {
                    fault: Fault::Repetition,
                    evidence: format!("\"{}\" and \"{}\"", trim_to(a, 50), trim_to(b, 50)),
                });
                break;
            }
        }
        if notes.iter().any(|n| n.fault == Fault::Repetition) {
            break;
        }
    }

    // A question at the end that isn't part of the point.
    if let Some(last) = sentences.last() {
        let l = last.to_lowercase();
        if last.ends_with('?')
            && ["thoughts", "what do you think", "let me know", "agree", "am i wrong"]
                .iter()
                .any(|q| l.contains(q))
        {
            notes.push(Note { fault: Fault::TackedOnQuestion, evidence: (*last).to_string() });
        }
    }

    // Length.
    if let Some(max) = limit {
        let n = text.chars().count();
        if n > max {
            notes.push(Note {
                fault: Fault::TooLong,
                evidence: format!("{n} characters, limit is {max}"),
            });
        }
    }

    // Anything concrete in it at all?
    let has_number = text.chars().any(|c| c.is_ascii_digit());
    let has_proper_noun = text
        .split_whitespace()
        .skip(1)
        .any(|w| w.chars().next().map(|c| c.is_uppercase()).unwrap_or(false));
    if !has_number && !has_proper_noun && text.split_whitespace().count() > 25 {
        notes.push(Note {
            fault: Fault::Vague,
            evidence: "no numbers, names or specifics anywhere".into(),
        });
    }

    // Rhythm: all sentences within a whisker of each other reads as machine
    // output, whatever the words are.
    if sentences.len() >= 4 {
        let lengths: Vec<usize> = sentences.iter().map(|s| s.split_whitespace().count()).collect();
        let mean = lengths.iter().sum::<usize>() as f32 / lengths.len() as f32;
        let spread = lengths
            .iter()
            .map(|l| (*l as f32 - mean).abs())
            .fold(0.0f32, f32::max);
        if mean >= 5.0 && spread < mean * 0.25 {
            notes.push(Note {
                fault: Fault::Monotonous,
                evidence: format!("every sentence is around {mean:.0} words"),
            });
        }
    }

    notes.retain(|n| !cfg.ignore.contains(&n.fault));
    notes
}

fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.to_lowercase())
        .filter(|w| w.len() > 3)
        .collect()
}

fn overlap(a: &str, b: &str) -> f32 {
    let (wa, wb) = (words(a), words(b));
    if wa.len() < 4 || wb.len() < 4 {
        return 0.0;
    }
    let shared = wa.iter().filter(|w| wb.contains(w)).count() as f32;
    shared / wa.len().min(wb.len()) as f32
}

fn trim_to(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}

/// The instruction for a rewrite, built from what's actually wrong.
///
/// Specific instructions produce a better second draft than "make it better",
/// and a rewrite with nothing to fix makes it worse.
pub fn revision_brief(notes: &[Note]) -> Option<String> {
    if notes.is_empty() {
        return None;
    }
    let mut s = String::from("Rewrite it. Specifically:\n");
    for n in notes {
        s.push_str(&format!("- It {} — {}\n", n.fault.what(), n.evidence));
    }
    s.push_str("Keep what's good. Don't add anything new.");
    Some(s)
}

/// What Atlas says about a draft, spoken.
pub fn spoken(notes: &[Note]) -> String {
    match notes.len() {
        0 => "That reads well. Nothing I'd change.".into(),
        1 => format!("One thing: it {}.", notes[0].fault.what()),
        n => {
            let first = notes[0].fault.what();
            format!("{n} things — the main one is that it {first}.")
        }
    }
}

/// Is the second draft actually better?
///
/// A rewrite that fixes two faults and introduces three is not an improvement,
/// and without checking, a revision loop happily makes things worse.
pub fn improved(before: &[Note], after: &[Note]) -> bool {
    after.len() < before.len()
}

#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Good enough as written.
    AsWritten,
    /// Rewritten, with what changed.
    Revised { text: String, fixed: Vec<Fault>, passes: u32 },
    /// Tried and the rewrite was worse, so the original stands.
    KeptOriginal(String),
}

/// The instruction handed to the model when rewriting a draft. The specifics
/// come from `revision_brief`; this is the standing part.
pub const REVISE_SYSTEM: &str =
    "You are tightening a short message you already wrote. Rewrite it to fix the \
     specific problems listed, keep everything that was good, add nothing new, \
     and return only the rewritten message.";

/// Actually rewrite a draft, up to `max_passes` times, keeping only genuine
/// improvements.
///
/// `critique` finds what is wrong and `revision_brief` says how to fix it, but
/// nothing ever *rewrote* the draft — `max_passes` bounded a loop that did not
/// exist. This is that loop: critique, hand the faults to the model, critique
/// the result, and keep the rewrite only when it has fewer faults than before
/// (a rewrite that fixes two problems and introduces three is not an
/// improvement — `improved` is the guard). It stops as soon as a pass stops
/// helping, so it rarely uses the whole budget, and it never sands the life
/// out of something that was already fine.
pub fn revise(text: &str, llm: &dyn crate::brain::Llm, cfg: &DraftConfig) -> Outcome {
    let mut notes = critique(text, None, cfg);
    if notes.is_empty() || !cfg.revise {
        return Outcome::AsWritten;
    }
    let mut current = text.to_string();
    let mut fixed: Vec<Fault> = Vec::new();
    let mut passes = 0u32;
    while passes < cfg.max_passes {
        let Some(brief) = revision_brief(&notes) else { break };
        let Ok(rewritten) = llm.complete(REVISE_SYSTEM, &format!("{brief}\n\nThe message:\n{current}"))
        else {
            break;
        };
        let rewritten = rewritten.trim().to_string();
        if rewritten.is_empty() {
            break;
        }
        let after = critique(&rewritten, None, cfg);
        passes += 1;
        if improved(&notes, &after) {
            // Record which faults this pass cleared.
            for n in &notes {
                if !after.iter().any(|a| a.fault == n.fault) {
                    fixed.push(n.fault);
                }
            }
            current = rewritten;
            notes = after;
            if notes.is_empty() {
                break; // nothing left to fix
            }
        } else {
            // This pass didn't help. If we've never improved it, the original
            // stands; otherwise keep the best we had.
            if fixed.is_empty() {
                return Outcome::KeptOriginal(text.to_string());
            }
            break;
        }
    }
    if fixed.is_empty() {
        Outcome::KeptOriginal(text.to_string())
    } else {
        fixed.sort_by_key(|f| format!("{f:?}"));
        fixed.dedup();
        Outcome::Revised { text: current, fixed, passes }
    }
}

#[cfg(test)]
mod revise_tests {
    use super::*;
    use crate::brain::MockLlm;

    fn cfg() -> DraftConfig {
        DraftConfig { revise: true, max_passes: 2, ignore: Vec::new() }
    }

    #[test]
    fn a_clean_draft_is_left_alone() {
        let llm = MockLlm("whatever".into());
        // A short, clean line has nothing to critique.
        assert_eq!(revise("Thanks — I'll have it to you Monday.", &llm, &cfg()), Outcome::AsWritten);
    }

    #[test]
    fn a_flawed_draft_is_rewritten_when_the_rewrite_is_better() {
        // The original opens with throat-clearing and leans on jargon filler;
        // the model returns a clean line with neither.
        let flawed = "I wanted to reach out about leveraging synergy on this.";
        let clean = "Following up on the proposal.";
        let llm = MockLlm(clean.into());
        match revise(flawed, &llm, &cfg()) {
            Outcome::Revised { text, passes, .. } => {
                assert_eq!(text, clean);
                assert!(passes >= 1);
            }
            other => panic!("expected a rewrite, got {other:?}"),
        }
    }

    #[test]
    fn a_rewrite_that_is_not_better_keeps_the_original() {
        // The "rewrite" has just as many faults, so the original stands.
        let flawed = "I wanted to reach out about leverage.";
        let llm = MockLlm("I hope this finds you well about synergy.".into());
        match revise(flawed, &llm, &cfg()) {
            Outcome::KeptOriginal(t) => assert_eq!(t, flawed),
            other => panic!("expected the original kept, got {other:?}"),
        }
    }

    #[test]
    fn max_passes_zero_never_rewrites() {
        let mut c = cfg();
        c.max_passes = 0;
        let llm = MockLlm("clean".into());
        let flawed = "I wanted to reach out about leveraging synergy.";
        assert_eq!(revise(flawed, &llm, &c), Outcome::KeptOriginal(flawed.to_string()));
    }
}

/// How a reply written as you in a chat or mail window is checked. A chat
/// reply is short and casual: "maybe", a question back, one short line are
/// all normal there, so hedging, questions, length, vagueness and rhythm
/// aren't faults. Sounding like a chatbot, leaving a blank, stock openers,
/// filler and saying things twice are.
pub fn for_replies() -> DraftConfig {
    DraftConfig {
        revise: true,
        max_passes: 1,
        ignore: vec![Fault::Hedging, Fault::TackedOnQuestion, Fault::TooLong, Fault::Vague, Fault::Monotonous],
    }
}
