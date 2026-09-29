//! Explaining code to someone who doesn't code.
//!
//! The same honest split as `craft`, `taste` and `motion`. The model writes the
//! explanation — and whether an explanation is *correct* needs actually
//! understanding the code, which is the model's job and, in the end, a person's
//! read. What a machine can check offline is whether it *reads* like a
//! non-coder explanation: that it's words and not leaked code, that it's the
//! length a plain summary should be, and that it isn't quietly leaning on jargon
//! a non-coder won't know. This is that check — the reliable half — and it never
//! claims the explanation is right, only that it's shaped like an explanation
//! rather than a wall of code or a sentence of unexplained terms.

use serde::{Deserialize, Serialize};

/// How much a finding matters — blocking (it isn't a usable explanation at all)
/// versus advisory (it works, but a non-coder would still trip on something).
/// Same split as the sibling modules: blocking is the fix loop's cue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Severity {
    Blocking,
    Advisory,
}

/// One thing the check found, in plain words.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    pub severity: Severity,
    pub rule: String,
    pub detail: String,
}

/// How deep the explanation goes — the dial a person turns with "like I'm
/// five" or "in more detail". It changes the instruction to the model AND how
/// strictly the check reads the result: a simple explanation may use no jargon
/// at all, a technical one may use precise terms as long as it defines them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Depth {
    /// Like explaining to a curious child — no jargon, short, an analogy if it
    /// helps.
    Simple,
    /// Plain English for a capable non-coder. The default.
    Normal,
    /// Thorough, for someone smart who doesn't code — precise terms allowed,
    /// each defined in plain words.
    Technical,
}

impl Depth {
    /// Read the dial out of the request: "like I'm five" / "simply" → Simple,
    /// "in detail" / "technical" → Technical, otherwise Normal.
    pub fn from_words(low: &str) -> Depth {
        const SIMPLE: &[&str] = &[
            "like i'm five", "like im five", "like i am five", "eli5", "simply",
            "in simple terms", "keep it simple", "dead simple", "for a child",
            "for a beginner", "really simple",
        ];
        const TECHNICAL: &[&str] = &[
            "in detail", "more detail", "in more detail", "technical", "technically",
            "deeply", "in depth", "in-depth", "thorough", "the full picture",
        ];
        if SIMPLE.iter().any(|p| low.contains(p)) {
            Depth::Simple
        } else if TECHNICAL.iter().any(|p| low.contains(p)) {
            Depth::Technical
        } else {
            Depth::Normal
        }
    }

    fn plain(&self) -> &'static str {
        match self {
            Depth::Simple => "as simply as possible",
            Depth::Normal => "in plain English",
            Depth::Technical => "in more detail",
        }
    }
}

/// The instruction handed to the model, at the chosen depth. Private: `draft`
/// is the only thing that picks a system prompt.
fn system_for(depth: Depth) -> &'static str {
    match depth {
        Depth::Simple => EXPLAIN_SIMPLE_SYSTEM,
        Depth::Normal => EXPLAIN_SYSTEM,
        Depth::Technical => EXPLAIN_TECHNICAL_SYSTEM,
    }
}

/// The instruction handed to the model. Strict about plain language and no
/// code, because the whole point is a non-coder reads it.
pub const EXPLAIN_SYSTEM: &str = "\
Explain what this code does and why, for someone who does not write code. Use \
plain, everyday language and short sentences. If a technical word is \
unavoidable, define it in ordinary terms in the same breath. Keep it to a few \
sentences. Do NOT restate the code, quote it, or include any code, symbols, or \
syntax -- describe what it accomplishes, not how it is written. No preamble, no \
sign-off, just the explanation.";

/// The instruction at the Simple depth — for a curious child.
pub const EXPLAIN_SIMPLE_SYSTEM: &str = "\
Explain what this code does for a curious ten-year-old. Use the simplest \
everyday words and a short comparison to something familiar if it helps. Use NO \
technical words at all. Two or three short sentences. Do NOT include any code, \
symbols, or syntax. No preamble, just the explanation.";

/// The instruction at the Technical depth — thorough, for a smart non-coder.
pub const EXPLAIN_TECHNICAL_SYSTEM: &str = "\
Explain what this code does and why, for someone who is smart but does not \
write code. You may use a precise technical term where it is the right word, \
but define it in plain language the first time you use it. Be thorough and \
clear, and cover the why, not only the what. Do NOT restate the code or include \
any code, symbols, or syntax. No preamble, just the explanation.";

/// The instruction for a fix round.
pub const EXPLAIN_FIX_SYSTEM: &str = "\
Your explanation did not pass a plain-language check. You are given the specific \
problems and your explanation. Rewrite it to fix exactly those problems -- still \
for someone who does not code, still in plain language with no code or symbols -- \
and return only the rewritten explanation, no preamble.";

/// Common terms a non-coder is unlikely to know without a plain-language gloss.
/// Deliberately the clearly-technical ones, not every computing word, so the
/// advice is worth heeding rather than noise. Whole-word matches only.
const JARGON: &[&str] = &[
    "boolean", "instantiate", "instantiated", "recursion", "recursive", "asynchronous",
    "concurrency", "pointer", "struct", "enum", "closure", "mutex", "buffer", "regex",
    "hashmap", "iterator", "polymorphism", "dereference", "serialize", "deserialize",
    "middleware", "idempotent", "nullable", "stdin", "stdout", "runtime",
];

/// Check an explanation for how well a non-coder can read it, at the default
/// (Normal) depth. `check_at` is the depth-aware version.
pub fn check(explanation: &str) -> Vec<Finding> {
    check_at(explanation, Depth::Normal)
}

/// Check an explanation against a chosen depth.
///
/// Deterministic and offline — plain scanning, no model. It reads the words,
/// not the meaning: it cannot tell you the explanation is *wrong* about the
/// code (that needs the code), and it does not pretend to. Depth changes only
/// the bar for jargon and length: a Simple explanation must use no jargon (it's
/// blocking), a Technical one is allowed its precise terms (jargon isn't
/// flagged) and more room to be thorough.
pub fn check_at(explanation: &str, depth: Depth) -> Vec<Finding> {
    let mut out = Vec::new();
    let trimmed = explanation.trim();

    if trimmed.is_empty() {
        return vec![Finding {
            severity: Severity::Blocking,
            rule: "says something".into(),
            detail: "there's nothing here to read".into(),
        }];
    }

    // Leaked code: an explanation that shows code instead of explaining it. Read
    // from shapes, not function names, so this never collides with a symbol
    // elsewhere: matched braces or arrows, or the density of code punctuation.
    let has_block = trimmed.contains('{') && trimmed.contains('}');
    let has_arrow = trimmed.contains("=>") || trimmed.contains("->");
    let has_semis = trimmed.matches(';').count() >= 2;
    let punct = trimmed.chars().filter(|c| "{}[]<>;=|&".contains(*c)).count();
    let dense = punct * 12 > trimmed.chars().count(); // >~8% code punctuation
    if has_block || has_arrow || has_semis || dense {
        out.push(Finding {
            severity: Severity::Blocking,
            rule: "words not code".into(),
            detail: "it's showing code or symbols instead of explaining in plain words".into(),
        });
    }

    // Long enough to explain anything, short enough for its depth. A Simple
    // explanation should be tight; a Technical one has more room to be thorough.
    let words = trimmed.split_whitespace().count();
    let cap = match depth {
        Depth::Simple => 90,
        Depth::Normal => 130,
        Depth::Technical => 220,
    };
    if words < 12 {
        out.push(Finding {
            severity: Severity::Blocking,
            rule: "enough to go on".into(),
            detail: "too short to actually explain what the code does".into(),
        });
    } else if words > cap {
        out.push(Finding {
            severity: Severity::Advisory,
            rule: "kept short".into(),
            detail: format!("{words} words is long for {} — trim it", depth.plain()),
        });
    }

    // Jargon a non-coder is unlikely to know. How much it matters depends on the
    // depth: at Simple it's a blocker (the whole point is no jargon), at Normal
    // it's worth flagging, and at Technical precise terms are allowed, so the
    // rule doesn't fire.
    if depth != Depth::Technical {
        let lower = trimmed.to_lowercase();
        let mut hit: Vec<&str> = JARGON.iter().copied().filter(|term| contains_word(&lower, term)).collect();
        hit.dedup();
        if !hit.is_empty() {
            out.push(Finding {
                severity: if depth == Depth::Simple { Severity::Blocking } else { Severity::Advisory },
                rule: "plain words".into(),
                detail: format!(
                    "uses technical term{} a non-coder may not know: {}",
                    if hit.len() == 1 { "" } else { "s" },
                    hit.join(", ")
                ),
            });
        }
    }

    out
}

/// The findings that block, in order — the fix loop's cue.
pub fn blocking(findings: &[Finding]) -> Vec<&Finding> {
    findings.iter().filter(|f| f.severity == Severity::Blocking).collect()
}

/// One honest line: what a clean check does and does not establish.
pub fn spoken(findings: &[Finding]) -> String {
    let blocking = findings.iter().filter(|f| f.severity == Severity::Blocking).count();
    let advisory = findings.len() - blocking;
    if findings.is_empty() {
        return "Reads as a plain-English explanation — whether it's accurate about the code is for \
                you to judge, not something I can check by reading the words."
            .into();
    }
    if blocking > 0 {
        let mut s = format!(
            "Not a usable explanation yet — {blocking} thing{} to fix",
            if blocking == 1 { "" } else { "s" }
        );
        if advisory > 0 {
            s.push_str(&format!(", plus {advisory} worth a look"));
        }
        s.push('.');
        return s;
    }
    format!(
        "Reads plainly; {advisory} thing{} a non-coder might still trip on.",
        if advisory == 1 { "" } else { "s" }
    )
}

// --- writing an explanation, and fixing what the check catches --------------

/// What an explain attempt ended as.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// Reads as a plain explanation. Any remaining findings are advisory notes.
    Explained { text: String, rounds: u32, notes: Vec<Finding> },
    /// A blocking problem survived the budget. The best draft comes back WITH
    /// the problems attached, never as if it worked.
    Struggled { text: String, rounds: u32, findings: Vec<Finding> },
    /// No usable draft — no model output.
    NoDraft(String),
}

/// Write a plain-language explanation of `code` and iterate against the check
/// until it reads plainly or the fix budget runs out.
///
/// The checking is injected so the loop is testable without a model; in
/// production it is `check`. The model writes, the check decides — the same
/// contract as `build_it::build_loop` and `motion::draw_loop`.
pub fn explain_loop(
    code: &str,
    llm: &dyn crate::brain::Llm,
    max_rounds: u32,
    depth: Depth,
    mut check_fn: impl FnMut(&str) -> Vec<Finding>,
) -> Outcome {
    let mut text = match draft(code, llm, depth) {
        Ok(t) => t,
        Err(e) => return Outcome::NoDraft(e),
    };
    let mut rounds = 0;
    loop {
        let findings = check_fn(&text);
        let has_blocker = findings.iter().any(|f| f.severity == Severity::Blocking);
        if !has_blocker {
            let notes = findings.into_iter().filter(|f| f.severity == Severity::Advisory).collect();
            return Outcome::Explained { text, rounds, notes };
        }
        if rounds >= max_rounds {
            return Outcome::Struggled { text, rounds, findings };
        }
        rounds += 1;
        let problems = findings
            .iter()
            .filter(|f| f.severity == Severity::Blocking)
            .map(|f| f.detail.as_str())
            .collect::<Vec<_>>()
            .join("; ");
        match fix(&text, &problems, llm) {
            Ok(t) => text = t,
            Err(e) => {
                let mut findings = findings;
                findings.push(Finding {
                    severity: Severity::Blocking,
                    rule: "fix".into(),
                    detail: format!("couldn't get a rewrite: {e}"),
                });
                return Outcome::Struggled { text, rounds, findings };
            }
        }
    }
}

/// A plain-English explanation of `code`, iterated to read plainly — the
/// convenience the build and improve paths use to hand generated code back
/// *with* its explanation, so nothing generated arrives unexplained. `None`
/// only when the model produced nothing at all; a struggle still returns its
/// best attempt, because a rough explanation beats none beside fresh code.
pub fn in_plain_english(code: &str, llm: &dyn crate::brain::Llm, max_rounds: u32) -> Option<String> {
    match explain_loop(code, llm, max_rounds, Depth::Normal, |t| check_at(t, Depth::Normal)) {
        Outcome::Explained { text, .. } | Outcome::Struggled { text, .. } => Some(text),
        Outcome::NoDraft(_) => None,
    }
}

/// First draft from the model, at the chosen depth.
fn draft(code: &str, llm: &dyn crate::brain::Llm, depth: Depth) -> Result<String, String> {
    let reply = llm.complete(system_for(depth), code).map_err(|e| e.to_string())?;
    let text = reply.trim().to_string();
    if text.is_empty() {
        return Err("the model returned nothing to explain with".into());
    }
    Ok(text)
}

/// One rewrite round: the explanation and the check's complaints.
fn fix(explanation: &str, problems: &str, llm: &dyn crate::brain::Llm) -> Result<String, String> {
    let user = format!("The problems:\n{problems}\n\nYour explanation:\n{explanation}");
    let reply = llm.complete(EXPLAIN_FIX_SYSTEM, &user).map_err(|e| e.to_string())?;
    let text = reply.trim().to_string();
    if text.is_empty() {
        return Err("nothing came back on the rewrite".into());
    }
    Ok(text)
}

/// Whole-word containment: `term` bounded by non-alphanumerics, so "enum"
/// doesn't fire on "enumerate" and "buffer" not on "buffered".
fn contains_word(haystack: &str, term: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(rel) = haystack[from..].find(term) {
        let at = from + rel;
        let before_ok = at == 0 || !is_word(bytes[at - 1]);
        let after = at + term.len();
        let after_ok = after >= bytes.len() || !is_word(bytes[after]);
        if before_ok && after_ok {
            return true;
        }
        from = at + term.len();
    }
    false
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}
