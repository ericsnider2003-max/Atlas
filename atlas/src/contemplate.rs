//! "Think hard about ...": several answers, then the best of them (6 Oct
//! 2026, taken from Muse Spark's "Contemplating" mode).
//!
//! Meta's deepest mode runs several reasoners side by side and combines what
//! they find. Atlas does the same with the model on this machine, for free:
//! three drafts, each told to come at the question a different way, then one
//! more pass that keeps what they agree on and says plainly where they don't.
//! On a laptop the drafts run one after another -- two at once would share
//! the graphics chip's memory and each take twice as long -- so this is the
//! slow, careful answer, made in the background and said when it's ready.
//! With Muse connected it goes to Muse at its highest effort instead, which
//! does the same inside one call.

use crate::brain::Llm;

/// The ways in: each draft is told one.
pub const ANGLES: &[(&str, &str)] = &[
    ("direct", "Answer directly and concretely, the way an expert would if asked in person."),
    ("careful", "First check the assumptions in the question and say which ones matter; then answer, and say what would change the answer."),
    ("step by step", "Work through it step by step, showing the reasoning briefly, then give the answer."),
];

const DRAFT_SYSTEM: &str = "You are Atlas, a personal assistant, answering one question with care. \
Be specific and honest about what you don't know. No preamble. Keep it under 250 words.";

/// What Muse is told when asked outright or to think hard: the same voice.
pub const MUSE_SYSTEM: &str = "You are answering for Atlas, a personal assistant, out loud to its owner. \
Be specific, honest about what you don't know, and brief: a few sentences unless more is asked for. No preamble.";

const MERGE_SYSTEM: &str = "You are Atlas. Several drafts answered the same question. Write the one best \
answer: keep what the drafts agree on, take the strongest specific points from each, and where they \
disagree say so plainly and say which is better supported. Add no fact that none of the drafts gives. \
No preamble, no mention of drafts unless they disagree. Under 250 words.";

/// The prompt for each draft: (system, user).
fn draft_prompts(question: &str) -> Vec<(String, String)> {
    ANGLES
        .iter()
        .map(|(_, how)| (format!("{DRAFT_SYSTEM} {how}"), question.trim().to_string()))
        .collect()
}

/// The prompt that combines them.
fn merge_prompt(question: &str, drafts: &[String]) -> (String, String) {
    let mut user = format!("Question: {}\n", question.trim());
    for (i, d) in drafts.iter().enumerate() {
        user.push_str(&format!("\nDraft {}:\n{}\n", i + 1, d.trim()));
    }
    (MERGE_SYSTEM.to_string(), user)
}

/// Three drafts and a merge. `stopping` is checked between calls. With
/// only one draft there is nothing to combine, and it is the answer.
pub fn contemplate(llm: &dyn Llm, question: &str, stopping: &dyn Fn() -> bool) -> Result<String, String> {
    let mut drafts: Vec<String> = Vec::new();
    let mut why = Vec::new();
    for (system, user) in draft_prompts(question) {
        if stopping() {
            return Err("stopped".into());
        }
        match llm.complete_long(&system, &user, 700) {
            Ok(r) if !r.text.trim().is_empty() => drafts.push(r.text),
            Ok(_) => why.push("an empty draft".to_string()),
            Err(e) => why.push(e.to_string()),
        }
    }
    match drafts.len() {
        0 => Err(format!("no draft came back ({})", why.join("; "))),
        1 => Ok(drafts.remove(0)),
        _ => {
            if stopping() {
                return Err("stopped".into());
            }
            let (system, user) = merge_prompt(question, &drafts);
            llm.complete_long(&system, &user, 900).map(|r| r.text).map_err(|e| e.to_string())
        }
    }
}

/// "think hard about X", "take your time and think about X", "contemplate
/// X": the question, when asked that way.
pub fn asked_to_think_hard(said: &str) -> Option<String> {
    let t = said.trim();
    let low = t.to_ascii_lowercase();
    for p in [
        "think hard about ",
        "think really hard about ",
        "think carefully about ",
        "really think about ",
        "take your time and think about ",
        "take your time on ",
        "think it through: ",
        "think this through: ",
        "contemplate ",
    ] {
        if low.starts_with(p) {
            let q = t[p.len()..].trim().trim_end_matches('.').trim();
            return (!q.is_empty()).then(|| q.to_string());
        }
    }
    None
}
