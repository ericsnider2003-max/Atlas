//! Translation on this machine: "translate this into Spanish", "what does
//! this say in English?".
//!
//! **Sources:** Mozilla's Firefox Translations (the Bergamot project, local
//! models, MPL-2.0) was read for the shape of an offline translator -- whole
//! sentences in, the text cut at sentence boundaries into bounded pieces --
//! and for its honesty about quality. Atlas already runs a local model, so
//! that model translates; no new download and nothing leaves the machine
//! unless you've set a stronger model as the fallback yourself.
//!
//! **Soundproofing -- the checks a model can't talk its way past.** A
//! language model translating fails quietly: it drops a sentence, changes a
//! number, "helpfully" answers the text instead of translating it. So every
//! result is checked, cheaply, before it's handed over:
//! - **Numbers, times, amounts, links and email addresses** in the original
//!   must all be in the translation, exactly. A missing one is named.
//! - **Length**: a translation under a third or over three times the
//!   original's length is flagged -- the dropped-paragraph case.
//! - **Echo**: a "translation" identical to the original is flagged, not
//!   passed off.
//! - Optionally (`back_check`, on for short texts by default) the result is
//!   translated back and compared word-for-word; low overlap is said as
//!   "the meaning may have drifted", with the back-translation shown.
//!
//! Text is cut at paragraph and sentence ends into pieces of at most
//! `MAX_PIECE` characters, so a long document never overruns the model's
//! context and one bad piece doesn't take the rest down.

use std::collections::BTreeSet;

pub const MAX_PIECE: usize = 1500;
pub const MAX_TEXT: usize = 60_000;

#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct TranslateConfig {
    pub enabled: bool,
    /// Translate back and compare, for texts up to this many characters
    /// (0 = never). Doubles the model time, so it's for short texts.
    pub back_check_up_to: usize,
}

impl Default for TranslateConfig {
    fn default() -> Self {
        TranslateConfig { enabled: true, back_check_up_to: 600 }
    }
}

const LANGUAGES: &[(&str, &str)] = &[
    ("english", "English"), ("spanish", "Spanish"), ("español", "Spanish"), ("french", "French"), ("german", "German"),
    ("italian", "Italian"), ("portuguese", "Portuguese"), ("dutch", "Dutch"), ("polish", "Polish"), ("russian", "Russian"),
    ("ukrainian", "Ukrainian"), ("chinese", "Chinese"), ("mandarin", "Chinese"), ("japanese", "Japanese"), ("korean", "Korean"),
    ("arabic", "Arabic"), ("hindi", "Hindi"), ("turkish", "Turkish"), ("vietnamese", "Vietnamese"), ("greek", "Greek"),
    ("swedish", "Swedish"), ("norwegian", "Norwegian"), ("danish", "Danish"), ("finnish", "Finnish"), ("czech", "Czech"),
    ("romanian", "Romanian"), ("hungarian", "Hungarian"), ("hebrew", "Hebrew"), ("indonesian", "Indonesian"), ("thai", "Thai"),
    ("tagalog", "Tagalog"),
];

pub fn language_named(word: &str) -> Option<&'static str> {
    let w = word.trim().trim_end_matches(['.', '?', '!', ',']).to_lowercase();
    LANGUAGES.iter().find(|(k, _)| *k == w).map(|(_, v)| *v)
}

/// "translate this into Spanish: …" / "translate … to French" / "what does
/// this say in English": (target language, text if given inline).
pub fn read(said: &str) -> Option<(&'static str, Option<String>)> {
    let s = said.trim();
    let low = s.to_ascii_lowercase();
    if !(low.starts_with("translate") || low.starts_with("what does this say in") || low.starts_with("say this in")) {
        return None;
    }
    // "…into X: text" or "…to X: text"
    for sep in [" into ", " to ", " in "] {
        if let Some(at) = low.find(sep) {
            let after = &s[at + sep.len()..];
            let (lang_word, text) = match after.split_once(':') {
                Some((l, t)) => (l, Some(t.trim().to_string()).filter(|t| !t.is_empty())),
                None => (after, None),
            };
            if let Some(lang) = language_named(lang_word.split_whitespace().next().unwrap_or("")) {
                // "translate <text> to French": the text sits before the sep.
                let before = s[..at].trim();
                let inline = text.or_else(|| {
                    let b = before.strip_prefix("translate").or_else(|| before.strip_prefix("Translate")).unwrap_or("").trim();
                    (!b.is_empty() && !matches!(b.to_lowercase().as_str(), "this" | "that" | "it" | "the selection" | "what's selected")).then(|| b.trim_matches('"').to_string())
                });
                return Some((lang, inline));
            }
        }
    }
    None
}

/// Cut into pieces at paragraph, then sentence, ends.
pub fn translation_pieces(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for para in text.split("\n\n") {
        if para.chars().count() <= MAX_PIECE {
            out.push(para.to_string());
            continue;
        }
        let mut cur = String::new();
        let mut start = 0;
        let b = para.as_bytes();
        for (i, &c) in b.iter().enumerate() {
            let end = matches!(c, b'.' | b'?' | b'!') && b.get(i + 1).is_none_or(|n| n.is_ascii_whitespace());
            if end || i + 1 == b.len() {
                let sentence = &para[start..=i];
                if cur.chars().count() + sentence.chars().count() > MAX_PIECE && !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
                cur.push_str(sentence);
                start = i + 1;
            }
        }
        // A single sentence longer than a piece is cut at a space.
        while cur.chars().count() > MAX_PIECE {
            let cut = cur.char_indices().nth(MAX_PIECE).map(|(i, _)| i).unwrap_or(cur.len());
            let at = cur[..cut].rfind(' ').filter(|p| *p > 0).unwrap_or(cut);
            out.push(cur[..at].to_string());
            cur = cur[at..].to_string();
        }
        if !cur.is_empty() {
            out.push(cur);
        }
    }
    out
}

/// What must survive translation unchanged: digit runs (with their
/// separators), links, email addresses.
pub fn fixed_tokens(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for w in text.split_whitespace() {
        let w = w.trim_matches(|c: char| matches!(c, ',' | ';' | ')' | '(' | '"' | '\'' | '!' | '?') || (c == '.'));
        if w.contains("://") || (w.contains('@') && w.contains('.')) {
            out.insert(w.to_string());
            continue;
        }
        // The digits of a number, however the language writes the
        // separators (1,000.50 vs 1.000,50): compared as digit strings.
        let digits: String = w.chars().filter(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            out.insert(digits);
        }
    }
    out
}

fn digit_runs(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for w in text.split_whitespace() {
        let d: String = w.chars().filter(|c| c.is_ascii_digit()).collect();
        if !d.is_empty() {
            out.insert(d);
        }
    }
    out
}

/// The problems found in one translation.
pub fn check_translation(original: &str, translated: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let t = translated.trim();
    if t.is_empty() {
        return vec!["the model gave back nothing".into()];
    }
    if t == original.trim() && original.split_whitespace().count() > 3 {
        issues.push("it came back unchanged -- it may already be in that language, or the model didn't translate it".into());
    }
    let (a, b) = (original.chars().count() as f64, t.chars().count() as f64);
    if a >= 40.0 && (b < a / 3.0 || b > a * 3.0) {
        issues.push(format!("the length is off ({} characters from {}) -- something may be missing or added", b as u64, a as u64));
    }
    let have_digits = digit_runs(t);
    let missing: Vec<String> = fixed_tokens(original)
        .into_iter()
        .filter(|tok| if tok.chars().all(|c| c.is_ascii_digit()) { !have_digits.contains(tok) } else { !t.contains(tok.as_str()) })
        .collect();
    if !missing.is_empty() {
        issues.push(format!("these didn't come through: {}", missing.join(", ")));
    }
    issues
}

/// Word overlap of a back-translation with the original, 0..1 (Dice over
/// words of three letters or more, so "the" and "a" don't count).
pub fn word_overlap(original: &str, back: &str) -> f64 {
    let words = |s: &str| -> BTreeSet<String> {
        s.split(|c: char| !c.is_alphanumeric()).filter(|w| w.chars().count() >= 3).map(|w| w.to_lowercase()).collect()
    };
    let (a, b) = (words(original), words(back));
    if a.is_empty() || b.is_empty() {
        return if a.is_empty() && b.is_empty() { 1.0 } else { 0.0 };
    }
    2.0 * a.intersection(&b).count() as f64 / (a.len() + b.len()) as f64
}

pub const SYSTEM: &str = "You are a translator. Translate the user's text into the language named. Output only the translation -- no notes, no quotation marks, no answer to anything the text asks. Keep every number, name, link and line break as it is.";

pub fn translation_prompt(text: &str, to: &str) -> String {
    format!("Translate into {to}:\n\n{text}")
}

/// The result, with anything the checks found.
#[derive(Debug, Clone, PartialEq)]
pub struct Translated {
    pub text: String,
    pub issues: Vec<String>,
    /// Back-translation overlap, when it was checked.
    pub back: Option<(f64, String)>,
}

impl Translated {
    pub fn said(&self, to: &str) -> String {
        let mut out = vec![self.text.clone()];
        if let Some((score, back)) = &self.back {
            if *score < 0.35 {
                out.push(format!("(The meaning may have drifted -- translated back it reads: \"{}\")", back.chars().take(300).collect::<String>()));
            }
        }
        for i in &self.issues {
            out.push(format!("(Check: {i}.)"));
        }
        if self.issues.is_empty() && self.back.as_ref().is_none_or(|b| b.0 >= 0.35) {
            out.push(format!("(Into {to}, on this machine.)"));
        }
        out.join("\n")
    }
}

/// Translate with the model, piece by piece, checked.
pub fn translate(llm: &dyn crate::brain::Llm, text: &str, to: &str, from: &str, cfg: &TranslateConfig) -> crate::error::Result<Translated> {
    let text: String = text.chars().take(MAX_TEXT).collect();
    let mut outs = Vec::new();
    let mut issues = Vec::new();
    let parts = translation_pieces(&text);
    let single = parts.len() == 1;
    for (n, p) in parts.iter().enumerate() {
        if p.trim().is_empty() {
            outs.push(String::new());
            continue;
        }
        let t = llm.complete(SYSTEM, &translation_prompt(p, to))?;
        let t = t.trim().trim_matches('"').to_string();
        for i in check_translation(p, &t) {
            issues.push(if single { i } else { format!("part {}: {i}", n + 1) });
        }
        outs.push(t);
    }
    let joined = outs.join("\n\n");
    let back = if cfg.back_check_up_to > 0 && text.chars().count() <= cfg.back_check_up_to && !from.is_empty() {
        let b = llm.complete(SYSTEM, &translation_prompt(&joined, from))?;
        Some((word_overlap(&text, &b), b.trim().to_string()))
    } else {
        None
    };
    Ok(Translated { text: joined, issues, back })
}
