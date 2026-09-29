//! Asking for help properly.
//!
//! Atlas tries. It reads its own error, forms a theory, writes a change into
//! the sandbox, runs the tests, and looks at what happened. Sometimes that
//! works. When it doesn't — after a few honest attempts — the useful thing is
//! not to keep guessing, it's to hand the problem to someone who can solve it.
//!
//! The difference between a good handoff and a bad one is enormous. "It
//! doesn't work" gets a question back. A brief with the symptom, the exact
//! error, what was already tried and why each attempt failed, and the twenty
//! lines that matter, usually gets an answer first time.
//!
//! So this assembles that brief, sized to be read rather than skimmed. You
//! paste it into a chat, paste the answer back, and Atlas applies it to the
//! sandbox and runs its own tests before anything touches your machine.

use crate::sandbox::Attempt;
use serde::{Deserialize, Serialize};

/// One thing Atlas tried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Try {
    /// What it thought was wrong.
    pub theory: String,
    /// What it changed.
    pub change: String,
    /// What happened.
    pub outcome: String,
    pub worked: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Snippet {
    pub path: String,
    /// First line number, for pointing at it.
    pub from_line: usize,
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Problem {
    /// What you asked for, or what broke.
    pub goal: String,
    /// The error, verbatim, trimmed.
    pub error: String,
    pub tried: Vec<Try>,
    /// The code that matters. Not the whole file.
    pub snippets: Vec<Snippet>,
    /// The failing test output.
    pub test_output: String,
    /// What Atlas currently believes, and how sure it is.
    pub theory: Option<String>,
    /// Anything it checked and ruled out — saves the reader repeating it.
    pub ruled_out: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HandoffConfig {
    /// Attempts before Atlas stops and asks.
    ///
    /// Only meaningful because the attempts are genuinely *different* — see
    /// `strategy.rs`. Three retries of the same idea isn't three attempts, so
    /// this counts distinct approaches rather than tries.
    pub attempts_before_asking: u32,
    /// Rough character ceiling for the brief.
    pub max_chars: usize,
    /// Longest single snippet.
    pub max_snippet_lines: usize,
}

impl Default for HandoffConfig {
    fn default() -> Self {
        HandoffConfig {
            // The whole ladder. There's no reason to stop at three when the
            // remaining approaches are genuinely different from the ones
            // already tried.
            attempts_before_asking: 12,
            max_chars: 6000,
            max_snippet_lines: 40,
        }
    }
}

/// Has Atlas done enough to be worth interrupting you?
pub fn should_ask(attempts: &[Attempt], cfg: &HandoffConfig) -> bool {
    let failures = attempts.iter().filter(|a| !a.passed).count() as u32;
    // If the last attempt passed, it isn't stuck.
    if attempts.last().map(|a| a.passed).unwrap_or(false) {
        return false;
    }
    failures >= cfg.attempts_before_asking
}

/// Build the brief.
///
/// Ordered the way a person reads: what I wanted, what happened, what I tried,
/// what I think, what I need. The code goes last because it's reference, not
/// narrative.
pub fn write_brief(p: &Problem, cfg: &HandoffConfig) -> String {
    let mut s = String::new();

    s.push_str("I'm stuck on something in Atlas and could use a hand.\n\n");
    s.push_str(&format!("**What I'm trying to do:** {}\n\n", p.goal.trim()));

    if !p.error.is_empty() {
        s.push_str("**What happens:**\n```\n");
        s.push_str(trim_middle(&p.error, 1200).trim());
        s.push_str("\n```\n\n");
    }

    if !p.tried.is_empty() {
        s.push_str("**What I've already tried:**\n");
        for (i, t) in p.tried.iter().enumerate() {
            s.push_str(&format!(
                "{}. Thought it was {}. Changed {}. {}\n",
                i + 1,
                t.theory.trim(),
                t.change.trim(),
                t.outcome.trim()
            ));
        }
        s.push('\n');
    }

    if !p.ruled_out.is_empty() {
        // Saves the reader suggesting something already eliminated, which is
        // most of what a bad bug report wastes.
        s.push_str(&format!("**Already ruled out:** {}\n\n", p.ruled_out.join("; ")));
    }

    if let Some(t) = &p.theory {
        s.push_str(&format!("**My best guess:** {}\n\n", t.trim()));
    }

    if !p.test_output.is_empty() {
        s.push_str("**Test output:**\n```\n");
        s.push_str(trim_middle(&p.test_output, 1000).trim());
        s.push_str("\n```\n\n");
    }

    for sn in &p.snippets {
        let text = first_lines(&sn.text, cfg.max_snippet_lines);
        s.push_str(&format!("**{}** (from line {}):\n```rust\n{}\n```\n\n", sn.path, sn.from_line, text));
    }

    s.push_str(
        "What I need is the change itself — a replacement for the part that's wrong, \
         so I can apply it and run the tests.\n",
    );

    trim_middle(&s, cfg.max_chars)
}

/// A one-line version, for saying out loud before handing it over.
pub fn spoken(p: &Problem) -> String {
    let tried = p.tried.len();
    let theory = p.theory.as_deref().unwrap_or("no idea what's causing it");
    format!(
        "I'm stuck on {}. Tried {} thing{}, none worked. Best guess is {}. I've written it up — want to hand it over?",
        p.goal.trim(),
        tried,
        if tried == 1 { "" } else { "s" },
        theory
    )
}

// ---------- taking the answer back ----------

/// A block of code from a reply.
#[derive(Debug, Clone, PartialEq)]
pub struct Block {
    pub language: String,
    pub code: String,
    /// A path mentioned just before the block, if there was one.
    pub path: Option<String>,
}

/// Pull the code out of an answer.
///
/// The path matters: an answer usually names the file just above the block,
/// and without it Atlas would have to guess where a change goes.
pub fn extract_blocks(reply: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let mut lines = reply.lines().peekable();
    let mut last_path: Option<String> = None;

    while let Some(line) = lines.next() {
        let t = line.trim();
        if let Some(p) = mentions_a_path(t) {
            last_path = Some(p);
        }
        if !t.starts_with("```") {
            continue;
        }
        let language = t.trim_start_matches('`').trim().to_string();
        let mut code = String::new();
        for body in lines.by_ref() {
            if body.trim().starts_with("```") {
                break;
            }
            code.push_str(body);
            code.push('\n');
        }
        if code.trim().is_empty() {
            continue;
        }
        out.push(Block { language, code, path: last_path.clone() });
    }
    out
}

fn mentions_a_path(line: &str) -> Option<String> {
    // "src/persona.rs" anywhere in a line of prose, or as a bare heading.
    line.split(|c: char| c.is_whitespace() || c == '`' || c == '*' || c == ':')
        .map(|w| w.trim_matches(|c: char| c == '(' || c == ')' || c == ',' || c == '.'))
        .find(|w| {
            // A bare file name counts too ("calc.py" on its own line above
            // the block is how most answers name the file).
            w.len() > 3
                && [".rs", ".yaml", ".yml", ".toml", ".md", ".bat", ".py", ".js", ".ts", ".json", ".sh", ".ps1", ".txt", ".csv"]
                    .iter()
                    .any(|e| w.ends_with(e))
        })
        .map(|w| w.to_string())
}

/// Is this answer usable, or does it need a follow-up?
#[derive(Debug, Clone, PartialEq)]
pub enum Usable {
    /// Apply these, then run the tests.
    Apply(Vec<Block>),
    /// It explained but didn't give code.
    NeedsCode(String),
    /// It asked a question back.
    Asked(String),
}

pub fn read_answer(reply: &str) -> Usable {
    let blocks = extract_blocks(reply);
    if !blocks.is_empty() {
        return Usable::Apply(blocks);
    }
    // A question back is common and shouldn't be treated as a failure.
    let last = reply.trim().lines().last().unwrap_or("").trim();
    if last.ends_with('?') {
        return Usable::Asked(last.to_string());
    }
    Usable::NeedsCode("no code came back — I'll ask for the change itself".into())
}

/// Keep the beginning and the end of something too long.
///
/// The first error and the final summary are the two useful parts of any
/// long output; the middle is repetition.
pub fn trim_middle(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let head: String = s.chars().take(max * 2 / 3).collect();
    let tail: String = {
        let t: String = s.chars().rev().take(max / 3).collect();
        t.chars().rev().collect()
    };
    format!("{head}\n\n[…trimmed…]\n\n{tail}")
}

fn first_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().collect();
    if lines.len() <= n {
        return s.to_string();
    }
    format!("{}\n// …{} more lines", lines[..n].join("\n"), lines.len() - n)
}
