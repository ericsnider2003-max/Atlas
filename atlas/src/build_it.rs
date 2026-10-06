//! Building code from a description, and proving it before handing it over.
//!
//! You describe what you want; Atlas writes it, then **checks its own work
//! against the real toolchain** — the compiler, the linter, the tests — and
//! only stands behind what passes. A model that writes plausible-looking code
//! is common and not worth much on its own; the value is the loop that reads
//! the compiler's actual complaint and fixes it, and the honesty to say "this
//! is the best I got and here is where it still fails" when the loop runs out.
//!
//! ## Why the compiler is the authority, not the model
//!
//! A language model's confidence that code is correct is worth nothing; the
//! compiler's verdict is worth everything. So generation is the cheap,
//! fallible step and `craft`'s ladder — `cargo check`, `clippy`, `cargo test`
//! — is the ground truth. This module owns the fallible half: turning a
//! description into a first draft, and turning a specific failure into a
//! specific fix. The daemon owns the loop that runs the ladder in a
//! `Sandbox` between rounds, because that touches the filesystem and spawns
//! processes; everything here is pure and takes the model as a `&dyn Llm`, so
//! the whole contract is tested against a mock with no toolchain at all.
//!
//! ## In-house or out-of-house, same check
//!
//! The generating model can be the local one, an in-house crew worker, or a
//! Cloudflare worker when online — that is the caller's choice, and it does
//! not change anything here. Wherever the draft comes from, it is checked on
//! this machine against this machine's toolchain before it is trusted. A
//! draft from a bigger model online is still a draft until the local compiler
//! agrees.

use crate::brain::Llm;
use crate::craft::Lang;
use serde::Deserialize;

/// How the build capability behaves. It is local and safe — everything happens
/// in a throwaway sandbox and the real toolchain is the judge — so it ships
/// on, unlike the things that reach outside the machine.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BuildConfig {
    pub enabled: bool,
    /// How many times to read the compiler's complaint and try again before
    /// handing over the best draft with the failure attached.
    pub max_fix_rounds: u32,
    /// The language to write in when the description doesn't say.
    pub default_language: Lang,
    /// How to rasterise an SVG animation to a PNG, when you want a rendered
    /// still rather than the SVG itself: a command template using `{in}`,
    /// `{out}`, `{w}`, `{h}` — e.g. `rsvg-convert -w {w} -h {h} {in} -o {out}`
    /// or `chromium --headless --screenshot={out} --window-size={w},{h} {in}`.
    /// Empty (the default) means no rasteriser, so an animation stays the SVG
    /// it already is — which renders in any browser on its own.
    #[serde(default)]
    pub render_svg_command: String,
    /// Hand builds and project changes to a coding agent installed on this
    /// computer -- Claude Code (`claude`) or Codex (`codex`) -- when there is
    /// one (`coding_agent`, 2 Oct 2026). `auto` uses one when it's installed;
    /// `off` never does. Off unless you turn it on (5 Oct 2026): both are paid
    /// services run by other companies, and the point of Atlas is not to
    /// depend on them -- its own coding model writes the code.
    pub coding_agent: AgentUse,
    /// Ask before each hand-over to the coding agent. A change to one of your
    /// own projects is asked about whatever this says.
    pub agent_asks_first: bool,
}

/// Whether a coding agent installed here may be handed the work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentUse {
    Auto,
    #[default]
    Off,
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            enabled: true,
            max_fix_rounds: 3,
            // Python, not Rust (2 Oct 2026): a Rust draft needs the whole
            // Rust toolchain to check, and "write me a script" almost always
            // means Python. A project folder's own language still wins.
            default_language: Lang::Python,
            render_svg_command: String::new(),
            coding_agent: AgentUse::Off,
            agent_asks_first: true,
        }
    }
}

/// The instruction handed to the generating model. Deliberately strict about
/// returning code and nothing else, because the next thing that happens to it
/// is a compiler, not a reader.
pub const GENERATE_SYSTEM: &str =
    "You are writing code for a personal assistant to compile and test on the \
     user's machine. Write the complete file that does what is asked — every \
     import, no placeholders, no TODO, no \"...\". Return ONLY the code, in a \
     single fenced code block. No explanation before or after. If the request \
     is ambiguous, make the most reasonable choice and encode it in the code \
     rather than asking.";

/// The instruction for a fix round when the targeted edits didn't apply: the
/// model is given its own code and the tool's verbatim complaint, and asked
/// to return the whole corrected file.
pub const FIX_SYSTEM: &str =
    "The code you wrote did not pass a check. You are given the code and the \
     tool's exact output. Fix the specific problem the tool reports and return \
     the COMPLETE corrected file in a single fenced code block — not a diff, \
     not just the changed lines, and no explanation. Change as little as \
     possible beyond what the tool is complaining about.";

/// The instruction for a fix round (2 Oct 2026): targeted edits to the draft
/// rather than the whole file again. Asking for the whole file every round
/// meant a file too long for the model came back cut off every round; edits
/// are short, and are applied here to the draft that is already whole.
pub const EDIT_SYSTEM: &str =
    "The code you wrote did not pass a check. You are given the current code and \
     the tool's exact output. Fix the specific problem with the smallest edits, \
     written as search/replace blocks -- as many as needed -- and nothing else:\n\
     <<<<<<< SEARCH\n\
     the exact lines from the current code\n\
     =======\n\
     the lines to put in their place\n\
     >>>>>>> REPLACE\n\
     The SEARCH part must match the current code exactly, character for \
     character, and be long enough to be found in only one place. No explanation.";

/// The most a code call asks a model to write, in tokens: a long file, and
/// short of what a laptop's model takes minutes over (2 Oct 2026; chosen).
pub const CODE_TOKENS_MOST: u32 = 6000;
/// The least: a reply this small can't hold a file anyway.
pub const CODE_TOKENS_LEAST: u32 = 256;
/// The context assumed for a model that doesn't say: what Atlas starts its
/// own model server with.
pub const CONTEXT_ASSUMED: u32 = 8192;

/// How much a code call may ask a model to write: whatever its context has
/// left after the prompt, up to `CODE_TOKENS_MOST`. The prompt is counted at
/// three characters a token, which overcounts English and code a little --
/// the safe side, since a request past the context fails outright.
pub fn code_budget(context: Option<u32>, prompt_chars: usize) -> u32 {
    let ctx = context.unwrap_or(CONTEXT_ASSUMED);
    let prompt = (prompt_chars / 3) as u32 + 64;
    ctx.saturating_sub(prompt).saturating_sub(64).clamp(CODE_TOKENS_LEAST, CODE_TOKENS_MOST)
}

/// Does this reply look like the model stopped mid-file? A code fence opened
/// and never closed, or brackets opened and never closed (2 Oct 2026: a cut
/// off draft was handed to the compiler as though it were whole, failed, was
/// "fixed" by another cut-off rewrite, and the struggle blamed the code).
///
/// A fence the model closed means it finished: brackets left open inside it
/// are a mistake in the code, for the compiler to name. The brackets only
/// decide for a reply with no fence at all (bare code, or a file read back).
pub fn looks_cut_off(reply: &str, code: &str, lang: Lang) -> bool {
    let fenced = reply.lines().any(|l| l.trim_start().starts_with("```"));
    unclosed_fence(reply) || (!fenced && still_open(code, lang))
}

/// A ``` that opens a block with no ``` to close it.
fn unclosed_fence(text: &str) -> bool {
    text.lines().filter(|l| l.trim_start().starts_with("```")).count() % 2 == 1
}

/// Brackets or a string left open at the end of the code. Read roughly --
/// strings and comments skipped the way each language writes them -- and
/// only ever answers yes when it is sure: code that closes more than it
/// opened somewhere has confused the reading, and is left to the compiler.
fn still_open(code: &str, lang: Lang) -> bool {
    let b: Vec<char> = code.chars().collect();
    let python = lang == Lang::Python;
    let mut depth: i64 = 0;
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        let next = b.get(i + 1).copied();
        // Comments.
        if (python && c == '#') || (!python && c == '/' && next == Some('/')) {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if !python && c == '/' && next == Some('*') {
            match (i + 2..b.len().saturating_sub(1)).find(|&k| b[k] == '*' && b[k + 1] == '/') {
                Some(k) => {
                    i = k + 2;
                    continue;
                }
                None => return true,
            }
        }
        // Python's triple-quoted strings.
        if python && (c == '"' || c == '\'') && next == Some(c) && b.get(i + 2) == Some(&c) {
            let close = (i + 3..b.len().saturating_sub(2)).find(|&k| b[k] == c && b[k + 1] == c && b[k + 2] == c);
            match close {
                Some(k) => {
                    i = k + 3;
                    continue;
                }
                None => return true,
            }
        }
        // In Rust, Go and C++ a single quote is a character ('x', '\n') or
        // a lifetime ('a), never a string: a literal is stepped over, a
        // lifetime's quote on its own.
        let quote_is_char = matches!(lang, Lang::Rust | Lang::Go | Lang::Cpp);
        if c == '\'' && quote_is_char {
            if next == Some('\\') {
                let close = (i + 2..(i + 12).min(b.len())).find(|&k| b[k] == '\'');
                i = close.map(|k| k + 1).unwrap_or(i + 1);
            } else if b.get(i + 2) == Some(&'\'') {
                i += 3;
            } else {
                i += 1;
            }
            continue;
        }
        if c == '"' || c == '\'' || c == '`' {
            // A string: to its closing quote, escapes skipped. Only Rust's
            // and JavaScript's template strings run over lines.
            let multi = c == '`' || lang == Lang::Rust;
            let mut k = i + 1;
            let mut closed = false;
            while k < b.len() {
                if b[k] == '\\' {
                    k += 2;
                    continue;
                }
                if b[k] == c {
                    closed = true;
                    break;
                }
                if b[k] == '\n' && !multi {
                    break;
                }
                k += 1;
            }
            if !closed && k >= b.len() {
                return true;
            }
            i = k + 1;
            continue;
        }
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
        i += 1;
    }
    depth > 0
}

/// A draft as it came from the model: the code, and whether it was cut off.
#[derive(Debug, Clone, PartialEq)]
pub struct Draft {
    pub code: String,
    pub cut_off: bool,
}

/// What a build attempt ended as. Kept on disk between phases of a piece of
/// work (`phases`), so a restart doesn't redo a build that already finished.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub enum Outcome {
    /// Passed the whole ladder. The code is verified against the toolchain.
    Built { code: String, rounds: u32, notes: Vec<String> },
    /// The fix loop ran out before it passed. The best draft is handed over
    /// WITH the failure attached — never as if it worked.
    Struggled { code: String, rounds: u32, last_failure: String },
    /// Could not even get a first draft (no model, or the model errored).
    NoDraft(String),
    /// Written, but a check that decides whether it works couldn't run: its
    /// program isn't on this computer. Handed over as unchecked, never as
    /// built, and never "fixed" for a missing tool.
    Unchecked { code: String, rounds: u32, missing: String },
    /// The model ran out of room before the file was finished (2 Oct 2026).
    /// Not a failing check -- nothing the compiler said about half a file is
    /// worth fixing -- and never handed over or queued as code: `partial`
    /// is kept only so it can be read.
    TooLong { partial: String, rounds: u32 },
}

impl Outcome {
    /// What to say when handing it over. A built result leads with success; a
    /// struggle leads with the honest limit, because passing off code that
    /// does not compile as done is the one thing this module exists to stop.
    pub fn spoken(&self, lang: Lang) -> String {
        match self {
            Outcome::Built { rounds, notes, .. } => {
                let base = if *rounds == 0 {
                    format!("Wrote it in {} and it passed the checks first try.", lang.plain())
                } else {
                    format!(
                        "Wrote it in {} and got it passing the checks after {} fix{}.",
                        lang.plain(),
                        rounds,
                        if *rounds == 1 { "" } else { "es" }
                    )
                };
                if notes.is_empty() {
                    base
                } else {
                    let plain: Vec<String> = notes.iter().map(|n| plain_note(n)).filter(|n| !n.is_empty()).take(3).collect();
                    format!("{base} Worth a look, none blocking: {}.", plain.join("; "))
                }
            }
            Outcome::Struggled { rounds, last_failure, .. } => format!(
                "I wrote a draft and tried to fix it {} time{}, but it still doesn't pass. \
                 Here's where it's stuck rather than pretending it's done: {}",
                rounds,
                if *rounds == 1 { "" } else { "s" },
                first_line(last_failure)
            ),
            Outcome::NoDraft(why) => format!("I couldn't build that: {why}"),
            Outcome::TooLong { .. } => format!(
                "I started writing it in {}, but it's longer than the model I used can write in one go, so it \
                 came back cut off. Rather than hand over half a file I've stopped there. A bigger model can \
                 write it whole: your own second model, the free online ones if you allow them, or a coding \
                 agent such as Claude Code if it's installed. Or ask for a smaller piece of it.",
                lang.plain()
            ),
            Outcome::Unchecked { missing, .. } => format!(
                "I wrote it in {}, but I couldn't check it: {missing} isn't installed on this computer, \
                 so it's untested. Install {missing} and I can run the checks.",
                lang.plain()
            ),
        }
    }

    /// Did it end verified?
    pub fn is_built(&self) -> bool {
        matches!(self, Outcome::Built { .. })
    }

    /// How to hand a change over *for an existing project*, as opposed to a
    /// standalone build.
    ///
    /// The distinction is a matter of honesty, and it matters most for a system
    /// someone's livelihood runs on. `spoken` says the code "passed the checks"
    /// — and it did, but those checks compiled it **in isolation**, as its own
    /// little unit. That is not the same as "it works inside your project": it
    /// hasn't been built against the project's real code or run through the
    /// project's own tests. Reporting the first as though it were the second is
    /// exactly the overclaim this whole file exists to prevent, one level up. So
    /// a project change always says the check was isolated and asks to be read
    /// before it's applied, and a struggle is handed over as a draft, never as
    /// done.
    pub fn in_project(&self, project: &str, title: &str, lang: Lang) -> String {
        let detail = self.spoken(lang);
        match self {
            Outcome::Built { .. } => format!(
                "Queued \"{title}\" on {project}. {detail} That's an isolated check, though — I \
                 haven't built or tested it inside {project} itself, so it's a proposal to read, \
                 not a proven change. Say \"implement {title}\" and I'll write it with a .before \
                 backup; nothing running is touched."
            ),
            Outcome::Struggled { .. } => format!(
                "Queued \"{title}\" on {project} as a draft. {detail} Treat it as a starting \
                 point, not a finished change."
            ),
            Outcome::NoDraft(_) => detail,
            Outcome::TooLong { .. } => format!("Nothing queued on {project}: {detail}"),
            Outcome::Unchecked { .. } => format!("Queued \"{title}\" on {project} as an unchecked draft. {detail}"),
        }
    }

    /// The code to hand over or land, whichever outcome — even a struggle
    /// produces a best draft worth keeping.
    pub fn code(&self) -> Option<&str> {
        match self {
            Outcome::Built { code, .. } | Outcome::Struggled { code, .. } | Outcome::Unchecked { code, .. } => Some(code),
            Outcome::NoDraft(_) | Outcome::TooLong { .. } => None,
        }
    }
}

/// The result of fact-checking a draft: it passed (with any non-blocking
/// notes), or it failed with the tool's verbatim output to fix from.
#[derive(Debug, Clone, PartialEq)]
pub enum Check {
    Passed(Vec<String>),
    Failed(String),
    /// A deciding check's program isn't on this computer (its name).
    CannotCheck(String),
}

/// The whole generate → check → fix loop, with the checking injected so the
/// contract is testable without a toolchain. In production `check` writes the
/// draft into a `Sandbox` and runs `craft`'s ladder; in tests it is a mock
/// that decides pass/fail. The model is the fallible part and the checker is
/// the authority — a draft is only `Built` when the checker says so, and when
/// the fix budget runs out the best draft is handed over as a `Struggled`,
/// with the failure attached, never as if it worked. A draft the model ran
/// out of room for ends as `TooLong` before any check runs.
pub fn build_loop(
    description: &str,
    lang: Lang,
    llm: &dyn Llm,
    max_rounds: u32,
    mut check: impl FnMut(&str) -> Check,
) -> Outcome {
    let first = match draft_with_room(description, lang, llm) {
        Ok(d) => d,
        Err(e) => return Outcome::NoDraft(e),
    };
    if first.cut_off {
        return Outcome::TooLong { partial: first.code, rounds: 0 };
    }
    let mut code = first.code;
    let mut rounds = 0u32;
    // However many your settings ask for: past this a model is going round
    // in circles, not closing in (2 Oct 2026).
    let max_rounds = max_rounds.min(MOST_ROUNDS);
    loop {
        match check(&code) {
            Check::Passed(notes) => return Outcome::Built { code, rounds, notes },
            Check::CannotCheck(missing) => return Outcome::Unchecked { code, rounds, missing },
            Check::Failed(output) => {
                if rounds >= max_rounds {
                    return Outcome::Struggled { code, rounds, last_failure: output };
                }
                rounds += 1;
                // The lines that say what's wrong, not the whole log: a
                // compiler's progress lines and fifty repeats of one warning
                // pushed the error itself out of a small model's view.
                let said = trim_failure(&output, FAILURE_MOST);
                match fix_round(&code, &said, lang, llm) {
                    // Edits apply to a whole draft, so only a full rewrite
                    // can come back cut off: the draft before it is still
                    // whole, and still failing, so that is the struggle.
                    Ok(d) if d.cut_off => return Outcome::TooLong { partial: d.code, rounds },
                    Ok(d) => code = d.code,
                    Err(e) => {
                        return Outcome::Struggled {
                            code,
                            rounds,
                            last_failure: format!("{output}\n(couldn't get a fix: {e})"),
                        }
                    }
                }
            }
        }
    }
}

/// The most fix rounds a build gets, whatever the settings say.
pub const MOST_ROUNDS: u32 = 8;

/// The most of a check's output a fix round is shown, in characters.
pub const FAILURE_MOST: usize = 2400;

/// Is this a line that says what went wrong (or where)?
fn says_what_failed(line: &str) -> bool {
    let t = line.trim_start();
    const MARKS: &[&str] = &[
        "error", "Error", "ERROR", "panicked", "FAILED", "FAIL", "failed", "Traceback", "Exception", "assert",
        "--> ", "File \"", "E   ", "expected", "undefined", "not found", "cannot find", "No module named",
        "SyntaxError", "TypeError", "NameError", "fatal", "test result:", "Segmentation", "Uncaught",
    ];
    MARKS.iter().any(|m| t.contains(m)) && !t.starts_with("warning: unused")
}

/// A check's output cut down to what a fix needs (2 Oct 2026): the lines
/// that name a failure, a line before and a few after each (the compiler's
/// pointer to the line, the source it quotes), the last line, repeats
/// dropped, within `most` characters -- the earliest failures first, since
/// the first error explains the rest, and the end, where Python and the test
/// runners say what finally happened. Output already short is left whole;
/// output with no such line keeps its end.
pub fn trim_failure(output: &str, most: usize) -> String {
    if output.len() <= most {
        return output.to_string();
    }
    let lines: Vec<&str> = output.lines().collect();
    let mut keep = vec![false; lines.len()];
    let mut any = false;
    for (i, l) in lines.iter().enumerate() {
        if says_what_failed(l) {
            any = true;
            let from = i.saturating_sub(1);
            let to = (i + 5).min(lines.len().saturating_sub(1));
            for k in keep.iter_mut().take(to + 1).skip(from) {
                *k = true;
            }
        }
    }
    if let Some(last) = lines.iter().rposition(|l| !l.trim().is_empty()) {
        keep[last] = true;
    }
    if !any {
        // Nothing names the failure: the end of it, on a line boundary.
        let mut cut = output.len() - most;
        while !output.is_char_boundary(cut) {
            cut += 1;
        }
        let tail = &output[cut..];
        let tail = tail.split_once('\n').map(|(_, rest)| rest).unwrap_or(tail);
        return format!("...\n{tail}");
    }
    // The kept lines, in order, a gap marked, a line seen before dropped.
    let mut seen = std::collections::HashSet::new();
    let mut kept: Vec<String> = Vec::new();
    let mut gap = false;
    for (i, l) in lines.iter().enumerate() {
        if !keep[i] {
            gap = true;
            continue;
        }
        if !l.trim().is_empty() && !seen.insert(l.trim()) {
            continue;
        }
        if gap && !kept.is_empty() {
            kept.push("...".into());
        }
        gap = false;
        kept.push(l.to_string());
    }
    let whole = kept.join("\n");
    if whole.len() <= most {
        return whole;
    }
    // Still too long: the first two thirds of the room from the start, the
    // rest from the end.
    let head_room = most * 2 / 3;
    let (mut head, mut tail) = (Vec::new(), Vec::new());
    let mut used = 0;
    for l in &kept {
        if used + l.len() + 1 > head_room {
            break;
        }
        used += l.len() + 1;
        head.push(l.as_str());
    }
    let mut tail_used = 0;
    for l in kept.iter().rev() {
        if used + tail_used + l.len() + 1 > most.saturating_sub(4) || head.len() + tail.len() >= kept.len() {
            break;
        }
        tail_used += l.len() + 1;
        tail.push(l.as_str());
    }
    tail.reverse();
    let mut out = head.join("\n");
    out.push_str("\n...\n");
    out.push_str(&tail.join("\n"));
    // One line longer than the whole room: cut, on a character boundary.
    if out.len() > most {
        let mut cut = most;
        while !out.is_char_boundary(cut) {
            cut -= 1;
        }
        out.truncate(cut);
    }
    out
}

/// What to tell the model about packages, by language. Python may declare
/// what it needs (PEP 723's block at the top) and the checks install it into
/// the sandbox; anything else is asked to keep to its standard library.
fn packages_note(lang: Lang) -> &'static str {
    match lang {
        Lang::Python => {
            "\n\nUse only the standard library unless a package is really needed. If one is, \
             declare it at the very top of the file, exactly like this:\n\
             # /// script\n# dependencies = [\"requests\"]\n# ///"
        }
        _ => "\n\nUse only the language's standard library.",
    }
}

/// Turn a description into a first draft, with room for a whole file, and
/// whether the model ran out of that room.
pub fn draft_with_room(description: &str, lang: Lang, llm: &dyn Llm) -> Result<Draft, String> {
    let user = format!("Language: {}\n\nWhat to build:\n{description}{}", lang.plain(), packages_note(lang));
    let room = code_budget(llm.context_tokens(), GENERATE_SYSTEM.len() + user.len());
    let reply = llm.complete_long(GENERATE_SYSTEM, &user, room).map_err(|e| e.to_string())?;
    let code = extract_code(&reply.text);
    if code.trim().is_empty() {
        return Err("the model returned no code".into());
    }
    let cut_off = reply.cut_off || looks_cut_off(&reply.text, &code, lang);
    Ok(Draft { code, cut_off })
}

/// One fix round: targeted edits to the draft first; the whole file again
/// only when the edits don't apply (or the model sent a whole file anyway).
pub fn fix_round(code: &str, failure_output: &str, lang: Lang, llm: &dyn Llm) -> Result<Draft, String> {
    let user = format!("The tool said:\n{failure_output}\n\nThe current code:\n```\n{code}\n```");
    let room = code_budget(llm.context_tokens(), EDIT_SYSTEM.len() + user.len());
    if let Ok(reply) = llm.complete_long(EDIT_SYSTEM, &user, room) {
        let (edits, unfinished) = read_edits(&reply.text);
        if !edits.is_empty() {
            if !unfinished && !reply.cut_off {
                if let Ok(fixed) = apply_edits(code, &edits) {
                    if fixed != code {
                        return Ok(Draft { code: fixed, cut_off: false });
                    }
                }
            }
        } else if !fenced_blocks(&reply.text).is_empty() {
            // A whole file instead of edits: taken as one, if it's whole.
            let whole = extract_code(&reply.text);
            if !whole.trim().is_empty() {
                let cut_off = reply.cut_off || looks_cut_off(&reply.text, &whole, lang);
                return Ok(Draft { code: whole, cut_off });
            }
        }
    }
    whole_fix(code, failure_output, lang, llm)
}

/// The fix round's fallback: the whole corrected file, with room for it.
fn whole_fix(code: &str, failure_output: &str, lang: Lang, llm: &dyn Llm) -> Result<Draft, String> {
    let user = format!(
        "The tool said:\n{failure_output}\n\nThe code was:\n```\n{code}\n```\n\n\
         Return the complete corrected file.");
    let room = code_budget(llm.context_tokens(), FIX_SYSTEM.len() + user.len());
    let reply = llm.complete_long(FIX_SYSTEM, &user, room).map_err(|e| e.to_string())?;
    let fixed = extract_code(&reply.text);
    if fixed.trim().is_empty() {
        return Err("the model returned no code on the fix round".into());
    }
    let cut_off = reply.cut_off || looks_cut_off(&reply.text, &fixed, lang);
    Ok(Draft { code: fixed, cut_off })
}

/// One fix round, as code: hand the model its code and the tool's exact
/// words, take back the corrected file. A fix that came back cut off is an
/// error, never code.
pub fn fix_draft(code: &str, failure_output: &str, lang: Lang, llm: &dyn Llm) -> Result<String, String> {
    let d = fix_round(code, failure_output, lang, llm)?;
    if d.cut_off {
        return Err("the fix came back cut off -- it's longer than this model can write in one go".into());
    }
    Ok(d.code)
}

/// The search/replace blocks in a reply, and whether one was left
/// unfinished (the reply was cut off inside it).
pub fn read_edits(reply: &str) -> (Vec<(String, String)>, bool) {
    #[derive(PartialEq)]
    enum At {
        Outside,
        Search,
        Replace,
    }
    let mut at = At::Outside;
    let (mut search, mut replace) = (Vec::<&str>::new(), Vec::<&str>::new());
    let mut out = Vec::new();
    for line in reply.lines() {
        let t = line.trim();
        let marker = |ch: char, word: &str| {
            let n = t.chars().take_while(|c| *c == ch).count();
            n >= 5 && t[n..].trim() == word
        };
        match at {
            At::Outside if marker('<', "SEARCH") => {
                at = At::Search;
                search.clear();
                replace.clear();
            }
            At::Search if marker('=', "") => at = At::Replace,
            At::Search => search.push(line),
            At::Replace if marker('>', "REPLACE") => {
                out.push((search.join("\n"), replace.join("\n")));
                at = At::Outside;
            }
            At::Replace => replace.push(line),
            At::Outside => {}
        }
    }
    (out, at != At::Outside)
}

/// Apply search/replace edits to code, in order. Each search must be found in
/// exactly one place: exactly as written first, then ignoring how the lines
/// are indented and trailing spaces. An edit that can't be placed fails the
/// lot -- half a fix is a third version nobody asked for.
pub fn apply_edits(code: &str, edits: &[(String, String)]) -> Result<String, String> {
    let mut out = code.to_string();
    for (n, (search, replace)) in edits.iter().enumerate() {
        if search.trim().is_empty() {
            return Err(format!("edit {} has nothing to search for", n + 1));
        }
        match out.matches(search.as_str()).count() {
            1 => {
                out = out.replacen(search.as_str(), replace, 1);
                continue;
            }
            0 => {}
            many => return Err(format!("edit {} matches in {many} places", n + 1)),
        }
        // Loosely: line by line, each trimmed.
        let lines: Vec<&str> = out.lines().collect();
        let want: Vec<&str> = search.lines().map(str::trim).collect();
        let fits: Vec<usize> = (0..lines.len().saturating_sub(want.len()) + 1)
            .filter(|&i| i + want.len() <= lines.len() && lines[i..i + want.len()].iter().zip(&want).all(|(a, b)| a.trim() == *b))
            .collect();
        match fits.as_slice() {
            [i] => {
                // Indented as the code is: the replacement moves by however
                // far the search was out (models drop the leading spaces).
                let indent = |l: &str| l.len() - l.trim_start().len();
                let shift = indent(lines[*i]) as isize - search.lines().next().map_or(0, indent) as isize;
                let moved = replace.lines().map(|l| {
                    if l.trim().is_empty() {
                        l.to_string()
                    } else if shift >= 0 {
                        format!("{}{l}", " ".repeat(shift as usize))
                    } else {
                        let cut = (-shift as usize).min(indent(l));
                        l[cut..].to_string()
                    }
                });
                let mut rebuilt: Vec<String> = lines[..*i].iter().map(|l| l.to_string()).collect();
                rebuilt.extend(moved);
                rebuilt.extend(lines[i + want.len()..].iter().map(|l| l.to_string()));
                let ends_nl = out.ends_with('\n');
                out = rebuilt.join("\n");
                if ends_nl {
                    out.push('\n');
                }
            }
            [] => return Err(format!("edit {} doesn't match the code", n + 1)),
            _ => return Err(format!("edit {} matches in {} places", n + 1, fits.len())),
        }
    }
    Ok(out)
}

/// Which model writes the code, by what it is (2 Oct 2026: a 4B model on the
/// laptop wrote everything, whatever else was set up).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Writer {
    /// The coding model on this computer (`coder`, 2 Oct 2026): a model
    /// trained for code, swapped in for the build.
    Coder,
    /// Your own second model (`llm_secondary`): a server you run, or a hosted
    /// model with your key.
    YourSecond,
    /// The Cloudflare worker you set up, when online.
    Worker,
    /// The model on this computer.
    Local,
    /// The free online models (`models.online_second`).
    FreeOnline,
}

impl Writer {
    /// How it's named when saying who wrote the code.
    pub fn named(&self) -> &'static str {
        match self {
            Writer::Coder => "the coding model on this computer",
            Writer::YourSecond => "your second model",
            Writer::Worker => "your Cloudflare worker",
            Writer::Local => "the model on this computer",
            Writer::FreeOnline => "a free online model",
        }
    }

    /// The same, with the coding model's own name when it's the one
    /// (`coder::plain_name`): "the coding model on this computer
    /// (Qwen2.5-Coder 7B)".
    pub fn named_with(&self, coder: &str) -> String {
        match self {
            Writer::Coder if !coder.is_empty() => format!("{} ({coder})", self.named()),
            w => w.named().to_string(),
        }
    }
}

/// What there is to write code with, right now.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Available {
    /// A coding model is set up on this computer (`coder`).
    pub coder: bool,
    pub your_second: bool,
    pub worker: bool,
    pub local: bool,
    /// `models.online_second` is on.
    pub free_online: bool,
    pub online: bool,
    /// The request, or what goes with it, holds something `redact` would
    /// keep back.
    pub private: bool,
}

/// The models to write code with, in the order to try them. Your own
/// stronger models first -- they are yours, and code is the work they do
/// better; then the one on this computer; then the free online ones, and
/// only for a request with nothing private in it -- tried after the local
/// one fails or runs out of room, never first (offline first, online second).
///
/// The coding model, when this computer has one, goes first of all (2 Oct
/// 2026): it is local, it is trained for exactly this, and when it can't be
/// had the call fails at once and the rest of the chain takes it.
pub fn writers(a: &Available) -> Vec<Writer> {
    let mut out = Vec::new();
    if a.coder {
        out.push(Writer::Coder);
    }
    if a.your_second {
        out.push(Writer::YourSecond);
    }
    if a.worker && a.online {
        out.push(Writer::Worker);
    }
    if a.local {
        out.push(Writer::Local);
    }
    if a.free_online && a.online && !a.private {
        out.push(Writer::FreeOnline);
    }
    out
}

/// `build_loop` across the writers, each tried once, in order, until one
/// gets it passing (or written and unable to be checked here). A writer that
/// can't be reached, runs out of room or runs out of fixes hands on to the
/// next -- so a struggle on the laptop's model is tried again on a stronger
/// one. What comes back is the best result and who wrote it.
pub fn build_with(
    description: &str,
    lang: Lang,
    writers: &[(Writer, &dyn Llm)],
    max_rounds: u32,
    mut check: impl FnMut(&str) -> Check,
) -> (Outcome, Option<Writer>) {
    fn rank(o: &Outcome) -> u8 {
        match o {
            Outcome::Built { .. } | Outcome::Unchecked { .. } => 4,
            Outcome::Struggled { .. } => 3,
            Outcome::TooLong { .. } => 2,
            Outcome::NoDraft(_) => 1,
        }
    }
    let mut best: Option<(Outcome, Writer)> = None;
    for (w, llm) in writers {
        let o = build_loop(description, lang, *llm, max_rounds, &mut check);
        if rank(&o) == 4 {
            return (o, Some(*w));
        }
        if best.as_ref().map_or(true, |(b, _)| rank(&o) >= rank(b)) {
            best = Some((o, *w));
        }
    }
    match best {
        Some((o, w)) => (o, Some(w)),
        None => (Outcome::NoDraft("there's no model to write it with".into()), None),
    }
}

/// Pull code out of a model reply. Prefers a fenced block; falls back to the
/// whole reply when there is no fence, because a model that ignored the
/// "fence it" instruction usually just returned bare code.
///
/// When there are several fenced blocks, the largest is taken — a reply that
/// leaks a sentence of explanation as its own tiny "block" should not beat the
/// actual file.
pub fn extract_code(reply: &str) -> String {
    let blocks = fenced_blocks(reply);
    if let Some(biggest) = blocks.into_iter().max_by_key(|b| b.len()) {
        return biggest;
    }
    reply.trim().to_string()
}

/// Every fenced code block in the text, with the opening ```lang line and the
/// closing ``` stripped.
fn fenced_blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_block = false;
    let mut current = String::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            if in_block {
                out.push(current.trim_end().to_string());
                current = String::new();
                in_block = false;
            } else {
                in_block = true;
            }
            continue;
        }
        if in_block {
            current.push_str(line);
            current.push('\n');
        }
    }
    // A block that never closed (model cut off) is still worth keeping.
    if in_block && !current.trim().is_empty() {
        out.push(current.trim_end().to_string());
    }
    out
}

fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}

/// How to phrase the goal for the generating model when the description names
/// a language, so a Python ask does not come back in Rust.
pub fn lang_from_words(description: &str, default: Lang) -> Lang {
    let d = description.to_lowercase();
    // Word-boundary-ish checks, most specific first. TypeScript before
    // JavaScript (every "typescript" also contains no "javascript", but "ts"
    // and "js" are close), and the compiled languages by their unambiguous
    // names. "script" alone still means Python, the way it did before — that
    // is the common "write me a script" case.
    let cpp = d.contains("c++") || d.contains("cpp") || d.contains(".hpp") || d.contains(".cc ") || d.split(|c: char| !c.is_alphanumeric() && c != '+').any(|w| w == "c++");
    if cpp {
        Lang::Cpp
    } else if d.contains("typescript") || d.contains(".ts") || d.contains(".tsx") {
        Lang::TypeScript
    } else if d.contains("javascript") || d.contains(".js") || d.contains("node") {
        Lang::JavaScript
    } else if d.contains("golang") || d.contains(".go") || d.contains(" go ") || d.starts_with("go ") {
        Lang::Go
    } else if d.contains("python") || d.contains(".py") || d.contains("script") {
        Lang::Python
    } else if d.contains("rust") || d.contains(".rs") {
        Lang::Rust
    } else {
        default
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brain::MockLlm;

    #[test]
    fn extract_code_pulls_the_fenced_block() {
        let reply = "Here's the code:\n```rust\nfn main() {}\n```\nHope that helps!";
        assert_eq!(extract_code(reply), "fn main() {}");
    }

    #[test]
    fn extract_code_takes_the_largest_block_over_a_stray_snippet() {
        let reply = "Use `foo`:\n```\nfoo\n```\nThe file:\n```rust\nfn main() {\n    crate::outln!(\"hi\");\n}\n```";
        assert_eq!(extract_code(reply), "fn main() {\n    crate::outln!(\"hi\");\n}");
    }

    #[test]
    fn extract_code_falls_back_to_bare_reply() {
        assert_eq!(extract_code("fn main() {}"), "fn main() {}");
    }

    #[test]
    fn extract_code_keeps_an_unclosed_block() {
        let reply = "```rust\nfn main() {\n    // cut off";
        assert!(extract_code(reply).contains("fn main"));
    }

    #[test]
    fn generate_returns_the_code_from_a_model() {
        let llm = MockLlm("```rust\nfn add(a: i32, b: i32) -> i32 { a + b }\n```".into());
        let d = draft_with_room("a function that adds two numbers", Lang::Rust, &llm).unwrap();
        assert_eq!(d.code, "fn add(a: i32, b: i32) -> i32 { a + b }");
        assert!(!d.cut_off);
    }

    #[test]
    fn fix_draft_returns_the_corrected_code_from_a_model() {
        let llm = MockLlm("```rust\nfn add(a: i32, b: i32) -> i32 { a + b }\n```".into());
        let fixed = fix_draft("fn add(a,b){a+b}", "error: expected type", Lang::Rust, &llm).unwrap();
        assert!(fixed.contains("i32"), "the fix round returns the corrected file: {fixed}");
    }

    #[test]
    fn generate_errors_when_the_model_returns_nothing_usable() {
        let llm = MockLlm("   ".into());
        assert!(draft_with_room("do a thing", Lang::Rust, &llm).is_err());
    }

    #[test]
    fn a_built_outcome_leads_with_success() {
        let o = Outcome::Built { code: "fn main(){}".into(), rounds: 2, notes: vec![] };
        assert!(o.is_built());
        assert!(o.spoken(Lang::Rust).contains("2 fixes"));
    }

    #[test]
    fn a_project_change_never_claims_an_isolated_check_is_project_verified() {
        // The integrity line for someone's livelihood: "it compiled on its own"
        // must not read as "it works inside your project".
        let built = Outcome::Built { code: "fn f(){}".into(), rounds: 0, notes: vec![] };
        let said = built.in_project("Homelab", "backup rotation", Lang::Rust);
        assert!(said.contains("isolated"), "must name the check as isolated: {said}");
        assert!(
            said.contains("proposal") || said.contains("read"),
            "must ask to be read, not claim proven: {said}"
        );
        assert!(said.contains("backup"), "must promise a reversible write: {said}");

        // A struggle is a draft, never a finished change.
        let stuck = Outcome::Struggled {
            code: "fn f(".into(),
            rounds: 3,
            last_failure: "error: expected `}`".into(),
        };
        let said = stuck.in_project("Homelab", "backup rotation", Lang::Rust);
        assert!(said.contains("draft"), "a struggle is handed over as a draft: {said}");
    }

    #[test]
    fn a_struggle_leads_with_the_limit_not_a_false_success() {
        let o = Outcome::Struggled {
            code: "fn main(){".into(),
            rounds: 3,
            last_failure: "error: expected `}`, found end of file".into(),
        };
        assert!(!o.is_built());
        let s = o.spoken(Lang::Rust);
        assert!(s.contains("doesn't pass"), "must not claim success: {s}");
        assert!(s.contains("expected"), "must carry the real error: {s}");
    }

    #[test]
    fn the_loop_returns_built_once_the_checker_is_satisfied() {
        let llm = MockLlm("```\nfn ok() {}\n```".into());
        let mut calls = 0;
        let o = build_loop("a function", Lang::Rust, &llm, 3, |_code| {
            calls += 1;
            if calls == 1 {
                Check::Failed("error: something".into())
            } else {
                Check::Passed(vec![])
            }
        });
        match o {
            Outcome::Built { rounds, .. } => assert_eq!(rounds, 1, "one fix then passed"),
            other => panic!("expected Built, got {other:?}"),
        }
    }

    #[test]
    fn the_loop_gives_up_honestly_after_the_budget() {
        let llm = MockLlm("```\nfn broken( {}\n```".into());
        let o = build_loop("a function", Lang::Rust, &llm, 2, |_code| {
            Check::Failed("error: expected `)`".into())
        });
        match o {
            Outcome::Struggled { rounds, last_failure, .. } => {
                assert_eq!(rounds, 2, "tried the whole budget");
                assert!(last_failure.contains("expected"), "carries the real error");
            }
            other => panic!("expected Struggled, got {other:?}"),
        }
    }

    #[test]
    fn the_loop_reports_no_draft_when_the_model_gives_nothing() {
        let llm = MockLlm("   ".into());
        let o = build_loop("x", Lang::Rust, &llm, 3, |_| Check::Passed(vec![]));
        assert!(matches!(o, Outcome::NoDraft(_)));
    }

    /// A model that answers from a script, one reply per call, and says
    /// how much room it was given.
    struct Scripted {
        replies: std::sync::Mutex<std::collections::VecDeque<crate::brain::LongReply>>,
        asked_for: std::sync::Mutex<Vec<u32>>,
        systems: std::sync::Mutex<Vec<String>>,
    }

    impl Scripted {
        fn new(replies: &[(&str, bool)]) -> Scripted {
            Scripted {
                replies: std::sync::Mutex::new(replies.iter().map(|(t, c)| crate::brain::LongReply { text: t.to_string(), cut_off: *c }).collect()),
                asked_for: std::sync::Mutex::new(Vec::new()),
                systems: std::sync::Mutex::new(Vec::new()),
            }
        }
    }

    impl Llm for Scripted {
        fn complete(&self, system: &str, user: &str) -> crate::error::Result<String> {
            self.complete_long(system, user, 512).map(|r| r.text)
        }
        fn complete_long(&self, system: &str, _: &str, max_tokens: u32) -> crate::error::Result<crate::brain::LongReply> {
            self.asked_for.lock().unwrap().push(max_tokens);
            self.systems.lock().unwrap().push(system.to_string());
            self.replies.lock().unwrap().pop_front().ok_or_else(|| crate::error::AtlasError::Platform("no more replies".into()))
        }
        fn context_tokens(&self) -> Option<u32> {
            Some(16_384)
        }
    }

    #[test]
    fn a_code_call_gets_room_for_a_whole_file_not_a_sentence() {
        // 16k context, a short prompt: the most a code call asks for.
        assert_eq!(code_budget(Some(16_384), 600), CODE_TOKENS_MOST);
        // 8k context and a big prompt: what's left, not past the context.
        let left = code_budget(Some(8192), 15_000);
        assert!(left < 8192 - 5000 && left > 2000, "{left}");
        // Never so little a file can't fit at all.
        assert_eq!(code_budget(Some(1024), 50_000), CODE_TOKENS_LEAST);
        let llm = Scripted::new(&[("```python\nprint(1)\n```", false)]);
        draft_with_room("print one", Lang::Python, &llm).unwrap();
        assert_eq!(llm.asked_for.lock().unwrap()[0], CODE_TOKENS_MOST, "512 was the old cap");
    }

    #[test]
    fn a_reply_cut_off_mid_file_is_seen_as_cut_off() {
        // An unclosed fence.
        assert!(looks_cut_off("```python\ndef f():\n    return 1\n", "def f():\n    return 1", Lang::Python));
        // Brackets left open.
        assert!(looks_cut_off("", "fn main() {\n    let v = vec![1, 2,", Lang::Rust));
        assert!(looks_cut_off("", "items = [\n  1,\n  2,", Lang::Python));
        // A Python string left open in a triple quote.
        assert!(looks_cut_off("", "x = 1\nDOC = \"\"\"starts here", Lang::Python));
        // Whole code is not.
        assert!(!looks_cut_off("```rust\nfn main() { let s = \"{\"; }\n```", "fn main() { let s = \"{\"; let c = '{'; }", Lang::Rust));
        assert!(!looks_cut_off("", "def f(x: 'int') -> str:\n    return f\"{x}\"  # (", Lang::Python));
        assert!(!looks_cut_off("", "fn f<'a>(s: &'a str) -> &'a str { s }", Lang::Rust));
        assert!(!looks_cut_off("", "const s = 'it is (fine';\nconsole.log(s);", Lang::JavaScript));
        // More closed than opened is the reading confused, not a cut-off.
        assert!(!looks_cut_off("", "}\n{", Lang::Rust));
    }

    #[test]
    fn a_cut_off_draft_is_too_long_not_a_failing_check() {
        let llm = Scripted::new(&[("```python\ndef main():\n    data = [\n        1,", false)]);
        let mut checked = 0;
        let o = build_loop("a long thing", Lang::Python, &llm, 3, |_| {
            checked += 1;
            Check::Failed("SyntaxError".into())
        });
        assert!(matches!(o, Outcome::TooLong { .. }), "{o:?}");
        assert_eq!(checked, 0, "half a file is never handed to the compiler");
        assert!(o.code().is_none(), "a cut-off file is never handed over as code");
        assert!(o.spoken(Lang::Python).contains("cut off"));
        assert!(o.in_project("Homelab", "x", Lang::Python).contains("Nothing queued"));
        // The server's own word counts too, whatever the text looks like.
        let llm = Scripted::new(&[("```python\nprint(1)\n```", true)]);
        assert!(matches!(build_loop("x", Lang::Python, &llm, 1, |_| Check::Passed(vec![])), Outcome::TooLong { .. }));
    }

    #[test]
    fn search_replace_edits_are_read_and_applied_to_the_draft() {
        let reply = "Here:\n<<<<<<< SEARCH\n    return a - b\n=======\n    return a + b\n>>>>>>> REPLACE\n";
        let (edits, open) = read_edits(reply);
        assert!(!open);
        assert_eq!(edits, vec![("    return a - b".to_string(), "    return a + b".to_string())]);
        let code = "def add(a, b):\n    return a - b\n";
        assert_eq!(apply_edits(code, &edits).unwrap(), "def add(a, b):\n    return a + b\n");
        // Indented differently in the reply: still placed, line by line.
        let loose = vec![("return a - b".to_string(), "return a + b".to_string())];
        assert_eq!(apply_edits(code, &loose).unwrap(), "def add(a, b):\n    return a + b\n");
        // Not there, or there twice: refused, not guessed.
        assert!(apply_edits(code, &[("nope".into(), "x".into())]).is_err());
        assert!(apply_edits("a\na\n", &[("a".into(), "b".into())]).is_err());
        // Cut off inside a block.
        let (_, open) = read_edits("<<<<<<< SEARCH\nx\n=======\ny");
        assert!(open);
    }

    #[test]
    fn a_fix_round_edits_the_draft_and_only_rewrites_when_the_edits_dont_apply() {
        let code = "def add(a, b):\n    return a - b\n";
        // Edits that apply: one call, the edit system prompt.
        let llm = Scripted::new(&[("<<<<<<< SEARCH\n    return a - b\n=======\n    return a + b\n>>>>>>> REPLACE", false)]);
        let d = fix_round(code, "assert 3 == -1", Lang::Python, &llm).unwrap();
        assert_eq!(d.code, "def add(a, b):\n    return a + b\n");
        assert_eq!(llm.systems.lock().unwrap().as_slice(), &[EDIT_SYSTEM.to_string()]);
        // Edits that don't apply: the whole file, asked for second.
        let llm = Scripted::new(&[
            ("<<<<<<< SEARCH\nnot in the code\n=======\nx\n>>>>>>> REPLACE", false),
            ("```python\ndef add(a, b):\n    return a + b\n```", false),
        ]);
        let d = fix_round(code, "assert", Lang::Python, &llm).unwrap();
        assert!(d.code.contains("a + b") && !d.cut_off);
        assert_eq!(llm.systems.lock().unwrap()[1], FIX_SYSTEM);
        // And a whole-file rewrite that comes back cut off ends the build as
        // too long, with nothing handed over.
        let llm = Scripted::new(&[
            ("```python\ndef add(a, b):\n    return a - b\n```", false),
            ("no edits here", false),
            ("```python\ndef add(a, b):\n    return (a +", false),
        ]);
        let mut n = 0;
        let o = build_loop("add", Lang::Python, &llm, 3, |_| {
            n += 1;
            Check::Failed("assert".into())
        });
        assert!(matches!(o, Outcome::TooLong { rounds: 1, .. }), "{o:?}");
    }

    #[test]
    fn your_own_stronger_models_write_first_and_free_online_only_after_the_local_one() {
        let all = Available { coder: false, your_second: true, worker: true, local: true, free_online: true, online: true, private: false };
        assert_eq!(writers(&all), vec![Writer::YourSecond, Writer::Worker, Writer::Local, Writer::FreeOnline]);
        // Offline: no worker, no free online.
        let offline = Available { online: false, ..all };
        assert_eq!(writers(&offline), vec![Writer::YourSecond, Writer::Local]);
        // Something private in the request never goes to a free service.
        let private = Available { your_second: false, worker: false, private: true, ..all };
        assert_eq!(writers(&private), vec![Writer::Local]);
        // The free online ones only when allowed.
        let not_allowed = Available { your_second: false, worker: false, free_online: false, ..all };
        assert_eq!(writers(&not_allowed), vec![Writer::Local]);
    }

    #[test]
    fn a_struggle_on_the_local_model_is_tried_once_more_on_the_stronger_one() {
        let local = Scripted::new(&[("```python\nbroken(\n```", false), ("```python\nbroken(\n```", false), ("```python\nbroken(\n```", false)]);
        let strong = Scripted::new(&[("```python\nprint('ok')\n```", false)]);
        let refs: Vec<(Writer, &dyn Llm)> = vec![(Writer::Local, &local), (Writer::FreeOnline, &strong)];
        let (o, by) = build_with("print ok", Lang::Python, &refs, 1, |code: &str| {
            if code.contains("print") { Check::Passed(vec![]) } else { Check::Failed("SyntaxError".into()) }
        });
        assert!(o.is_built(), "{o:?}");
        assert_eq!(by, Some(Writer::FreeOnline));
        // The first that passes is kept; nothing after it is asked.
        let strong = Scripted::new(&[("```python\nprint('ok')\n```", false)]);
        let never = Scripted::new(&[]);
        let refs: Vec<(Writer, &dyn Llm)> = vec![(Writer::YourSecond, &strong), (Writer::Local, &never)];
        let (o, by) = build_with("print ok", Lang::Python, &refs, 1, |_| Check::Passed(vec![]));
        assert!(o.is_built() && by == Some(Writer::YourSecond));
        assert!(never.asked_for.lock().unwrap().is_empty());
        // One that can't be reached hands on, and a struggle beats it.
        let down = Scripted::new(&[]);
        let local = Scripted::new(&[("```python\nx(\n```", false), ("```python\nx(\n```", false)]);
        let refs: Vec<(Writer, &dyn Llm)> = vec![(Writer::YourSecond, &down), (Writer::Local, &local)];
        let (o, by) = build_with("x", Lang::Python, &refs, 1, |_| Check::Failed("SyntaxError".into()));
        assert!(matches!(o, Outcome::Struggled { .. }) && by == Some(Writer::Local), "{o:?}");
    }

    #[test]
    fn a_folder_in_the_sentence_is_where_the_work_goes() {
        let (d, rest) = folder_named(r"write a python script that renames photos and save it to C:\code\tools").unwrap();
        assert_eq!(d, std::path::PathBuf::from(r"C:\code\tools"));
        assert_eq!(rest, "write a python script that renames photos");
        let (d, rest) = folder_named("in /home/sam/site, add a dark mode toggle").unwrap();
        assert_eq!(d, std::path::PathBuf::from("/home/sam/site"));
        assert_eq!(rest, "add a dark mode toggle");
        let (d, _) = folder_named(r#"a scraper for prices in "D:\My Code\scraper""#).unwrap();
        assert_eq!(d, std::path::PathBuf::from(r"D:\My Code\scraper"));
        // A folder that's part of what to build stays part of it.
        assert!(folder_named(r"a script that copies files to D:\backup every night").is_none());
        assert!(folder_named("a script that reads /var/log/syslog and counts errors").is_none());
        assert!(folder_named("convert km/h to mph").is_none());
    }

    #[test]
    fn each_build_gets_a_folder_of_its_own() {
        let base = std::env::temp_dir().join(format!("atlas-build-folders-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let one = build_folder(&base, "a page about my dog");
        std::fs::create_dir_all(&one).unwrap();
        let two = build_folder(&base, "a page about my dog");
        assert_ne!(one, two, "never the folder of the build before");
        assert!(one.ends_with("page-about-dog"));
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn declared_python_packages_are_read_and_nothing_else() {
        let pep = "# /// script\n# dependencies = [\"requests>=2\", \"rich\"]\n# ///\nimport requests\n";
        assert_eq!(python_deps(pep), vec!["requests>=2".to_string(), "rich".to_string()]);
        assert_eq!(python_deps("# requires: httpx, bs4\n"), vec!["httpx".to_string(), "bs4".to_string()]);
        // An address or a command is not a package name.
        assert!(python_deps("# requires: git+https://evil/x, ; rm -rf /\n").is_empty());
        assert!(python_deps("import os\n").is_empty());
    }

    #[test]
    fn running_a_build_uses_uv_only_for_one_that_needs_packages() {
        let f = std::path::Path::new("/b/tool.py");
        let uv = std::path::Path::new("/t/uv");
        let (p, a) = run_command(Lang::Python, f, &["rich".into()], Some(uv), Some("python3")).unwrap();
        assert_eq!(p, "/t/uv");
        assert!(a.contains(&"--with".to_string()) && a.contains(&"rich".to_string()));
        let (p, a) = run_command(Lang::Python, f, &[], Some(uv), Some("python3")).unwrap();
        assert_eq!((p.as_str(), a.len()), ("python3", 1));
        assert!(run_command(Lang::Rust, f, &[], None, None).is_none(), "a Rust draft is a library");
        assert!(run_command(Lang::Python, f, &[], None, None).is_none(), "no Python, nothing to run with");
    }

    #[test]
    fn python_is_the_language_when_none_is_named() {
        assert_eq!(BuildConfig::default().default_language, Lang::Python);
        assert_eq!(lang_from_words("a tool that totals my receipts", BuildConfig::default().default_language), Lang::Python);
    }

    #[test]
    fn language_is_read_from_the_description() {
        assert_eq!(lang_from_words("write a python script to sort files", Lang::Rust), Lang::Python);
        assert_eq!(lang_from_words("a rust function", Lang::Python), Lang::Rust);
        assert_eq!(lang_from_words("something", Lang::Rust), Lang::Rust);
    }
}

/// A build that ran out of fix rounds, kept so "keep at it" can carry on
/// from its best draft instead of starting over.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct Struggle {
    pub description: String,
    pub lang: Lang,
    pub code: String,
    pub failure: String,
}

impl Struggle {
    /// Where the last one is kept: beside the drafts it came from.
    pub fn path() -> std::path::PathBuf {
        crate::roots::data_sub("builds").join("last-struggle.json")
    }
}

/// Keep going on a struggled build as a long job (Eric, E3): fix, check,
/// repeat, until it passes, the attempts or hours run out, or it stops
/// getting anywhere (the same draft twice is going in circles).
pub fn keep_building(
    s: &Struggle,
    llm: &dyn Llm,
    limits: &crate::goal::LongJobConfig,
    mut check: impl FnMut(&str) -> Check,
    stop: impl FnMut() -> bool,
) -> (Outcome, String) {
    use crate::goal::{Attempt, Check as GoalCheck, Goal};
    let ladder = GoalCheck::CommandPasses("the compiler, the linter and the tests".into());
    let goal = Goal::new(&s.description, limits.max_attempts).checking(ladder.clone());
    let mut code = s.code.clone();
    let mut failure = s.failure.clone();
    let mut notes = Vec::new();
    let (ended, attempts) = crate::goal::keep_at_it(
        &goal,
        limits,
        |_n| {
            let before = code.clone();
            if let Ok(next) = fix_draft(&code, &failure, s.lang, llm) {
                code = next;
            }
            let changed = if code == before { String::new() } else { "a new draft".to_string() };
            match check(&code) {
                Check::Passed(n) => {
                    notes = n;
                    Attempt { n: 0, passed: vec![ladder.clone()], failed: vec![], changed }
                }
                Check::Failed(out) => {
                    failure = out;
                    Attempt { n: 0, passed: vec![], failed: vec![ladder.clone()], changed }
                }
                // Nothing to fix in the code: the checker isn't here. Kept
                // as the failure so it's what gets said, not a compiler error.
                Check::CannotCheck(missing) => {
                    failure = format!("couldn't check it: {missing} isn't installed on this computer");
                    Attempt { n: 0, passed: vec![], failed: vec![ladder.clone()], changed }
                }
            }
        },
        stop,
        std::time::Instant::now(),
    );
    let said = crate::goal::ended_spoken(&goal, &ended, &attempts);
    let rounds = attempts.len() as u32;
    let outcome = if attempts.last().map_or(false, |a| a.met_everything()) {
        Outcome::Built { code, rounds, notes }
    } else {
        Outcome::Struggled { code, rounds, last_failure: failure }
    };
    (outcome, said)
}

/// How a bigger model is asked for help with a build Atlas couldn't finish.
pub const HELP_PROMPT: &str = "You're helping another assistant that got stuck. Read its write-up, find \
what's actually wrong, and reply with the complete corrected program in one fenced code block. If \
something is missing that you'd need to know, ask one question instead.";

/// A struggle, written up properly for a bigger model (Eric's ruling D,
/// "minimally"): the goal, the exact error, what was tried, and the code.
pub fn write_up(s: &Struggle, rounds: u32, cfg: &crate::handoff::HandoffConfig) -> String {
    let p = crate::handoff::Problem {
        goal: s.description.clone(),
        error: s.failure.clone(),
        tried: vec![crate::handoff::Try {
            theory: "fix the error the checks reported".into(),
            change: format!("{rounds} rounds of redrafting on the local model"),
            outcome: "still failing the same checks".into(),
            worked: false,
        }],
        snippets: vec![crate::handoff::Snippet { path: format!("draft.{}", s.lang.plain().to_lowercase()), from_line: 1, text: s.code.clone() }],
        test_output: s.failure.clone(),
        theory: None,
        ruled_out: Vec::new(),
    };
    crate::handoff::write_brief(&p, cfg)
}

/// Hand a struggle to the stronger model once, and check what comes back
/// here, against the same checks. Its answer is a draft until it passes.
pub fn ask_for_help(
    s: &Struggle,
    rounds: u32,
    llm: &dyn Llm,
    cfg: &crate::handoff::HandoffConfig,
    mut check: impl FnMut(&str) -> Check,
) -> (Outcome, String) {
    let brief = write_up(s, rounds, cfg);
    let reply = match llm.complete_hard(HELP_PROMPT, &brief) {
        Ok(r) => r,
        Err(e) => {
            return (
                Outcome::Struggled { code: s.code.clone(), rounds, last_failure: s.failure.clone() },
                format!("I asked the bigger model for help and couldn't reach it ({e})."),
            )
        }
    };
    match crate::handoff::read_answer(&reply) {
        crate::handoff::Usable::Apply(blocks) => {
            let code = blocks[0].code.clone();
            match check(&code) {
                Check::Passed(notes) => (
                    Outcome::Built { code, rounds: rounds + 1, notes },
                    format!("\"{}\" is done — the bigger model found it, and it checks out here.", s.description),
                ),
                Check::Failed(out) => (
                    Outcome::Struggled { code, rounds: rounds + 1, last_failure: out },
                    "I asked the bigger model for help; its fix still fails the checks here, so I've kept it as a draft.".into(),
                ),
                Check::CannotCheck(missing) => (
                    Outcome::Unchecked { code, rounds: rounds + 1, missing: missing.clone() },
                    format!("The bigger model sent a fix, but I couldn't check it here: {missing} isn't installed."),
                ),
            }
        }
        crate::handoff::Usable::Asked(q) => (
            Outcome::Struggled { code: s.code.clone(), rounds, last_failure: s.failure.clone() },
            format!("I asked the bigger model for help and it asked back: {q}"),
        ),
        crate::handoff::Usable::NeedsCode(_) => (
            Outcome::Struggled { code: s.code.clone(), rounds, last_failure: s.failure.clone() },
            "I asked the bigger model for help and it explained without giving a fix.".into(),
        ),
    }
}


/// A checker's note as a sentence: its first line, without the rule code
/// and the tool's markers (1 Oct 2026: a whole ruff report, arrows and
/// ASCII art included, was read out as the result).
pub fn plain_note(note: &str) -> String {
    let first = note.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let mut words: Vec<&str> = first.split_whitespace().collect();
    // A leading rule code: F401, E501, W0611, clippy::foo, TS2345:, C4996...
    if let Some(w) = words.first() {
        let w = w.trim_end_matches(':');
        let code = w.len() <= 40
            && (w.contains("::") || (w.chars().next().is_some_and(|c| c.is_ascii_uppercase()) && w.chars().skip(1).any(|c| c.is_ascii_digit()) && w.chars().all(|c| c.is_ascii_alphanumeric())));
        if code {
            words.remove(0);
        }
    }
    words.retain(|w| *w != "[*]" && *w != "-->" && *w != "|");
    words.join(" ").replace('`', "").trim_end_matches('.').to_string()
}

/// A file name for what was built, from its description: a few of its words,
/// and never over the top of one built before.
pub fn file_name_for(dir: &std::path::Path, description: &str, ext: &str) -> std::path::PathBuf {
    let stem = slug_for(description);
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    path
}

/// A few words of a description, joined for a file or folder name.
pub fn slug_for(description: &str) -> String {
    const SKIP: &[&str] = &["a", "an", "the", "me", "my", "that", "which", "to", "in", "of", "for", "and", "write", "build", "make", "create", "script", "program", "code", "python", "rust", "go", "javascript", "typescript", "it", "save", "put"];
    let words: Vec<String> = description
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !SKIP.contains(w))
        .take(5)
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        "build".to_string()
    } else {
        words.join("-")
    }
}

/// A folder of its own for one build, under `base`, named for what it does
/// and never one used before (2 Oct 2026: every web page landed on
/// `page.draft.html` in the one folder, over the top of the last).
pub fn build_folder(base: &std::path::Path, description: &str) -> std::path::PathBuf {
    let stem = slug_for(description);
    let mut dir = base.join(&stem);
    let mut n = 2;
    while dir.exists() {
        dir = base.join(format!("{stem}-{n}"));
        n += 1;
    }
    dir
}

/// A folder the sentence names, and the sentence without it: "... and save
/// it to C:\code\tools", "in ~/projects/site, add a dark mode", "... in
/// D:\work\app". Only where the words say it's where the work goes -- "save
/// it to", "put it in", "write it into" anywhere, or "in"/"into"/"inside"
/// a path that ends the sentence or its first part -- so "a script that
/// copies files to D:\backup" keeps its backup folder as part of what to
/// build (2 Oct 2026).
pub fn folder_named(text: &str) -> Option<(std::path::PathBuf, String)> {
    let words: Vec<&str> = text.split_whitespace().collect();
    // A quoted path may hold spaces: joined back into one word first.
    let mut toks: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let w = words[i];
        if (w.starts_with('"') || w.starts_with('\'')) && !(w.len() > 1 && (w.ends_with('"') || w.ends_with('\''))) {
            let q = &w[..1];
            let mut joined = w.to_string();
            let mut k = i + 1;
            while k < words.len() {
                joined.push(' ');
                joined.push_str(words[k]);
                if words[k].trim_end_matches([',', '.', ';']).ends_with(q) {
                    break;
                }
                k += 1;
            }
            if k < words.len() {
                toks.push(joined);
                i = k + 1;
                continue;
            }
        }
        toks.push(w.to_string());
        i += 1;
    }
    for (n, tok) in toks.iter().enumerate() {
        let trailing_comma = tok.ends_with(',');
        let bare = tok.trim_end_matches(['.', ',', ';', '!', '?', ')']).trim_matches(['"', '\'']);
        if !looks_like_a_folder(bare) {
            continue;
        }
        let before: Vec<String> = toks[..n].iter().map(|t| t.to_lowercase()).collect();
        let lead = before.last().map(String::as_str).unwrap_or("");
        let said_it = before.len() >= 3 && matches!(before[before.len() - 3].as_str(), "save" | "put" | "write" | "store" | "keep" | "place") && before[before.len() - 2] == "it";
        let ends = n + 1 == toks.len() || trailing_comma;
        let lead_word = matches!(lead, "in" | "into" | "inside" | "to" | "under" | "at");
        let lead_ok = (said_it && lead_word) || (matches!(lead, "in" | "into" | "inside" | "folder" | "directory") && ends);
        if !lead_ok {
            continue;
        }
        // The words that led to it come out with it.
        let mut cut_from = n - 1;
        if said_it {
            cut_from = n - 3;
            if cut_from > 0 && toks[cut_from - 1].eq_ignore_ascii_case("and") {
                cut_from -= 1;
            }
        } else if cut_from >= 1 && matches!(before[cut_from - 1].as_str(), "the" | "folder" | "directory") {
            cut_from -= 1;
            if cut_from >= 1 && before[cut_from - 1] == "the" {
                cut_from -= 1;
            }
            if cut_from >= 1 && matches!(before[cut_from - 1].as_str(), "in" | "into") {
                cut_from -= 1;
            }
        }
        let mut rest: Vec<String> = toks[..cut_from].to_vec();
        rest.extend(toks[n + 1..].iter().cloned());
        let rest = rest.join(" ").trim().trim_end_matches([',', ';']).trim().to_string();
        return Some((expand_home(bare), rest));
    }
    None
}

/// A word that is a folder path: a drive (`C:\...`), a share (`\\host\x`),
/// home (`~/x`), or an absolute path with at least two parts (`/home/me`).
fn looks_like_a_folder(w: &str) -> bool {
    let b = w.as_bytes();
    let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/');
    let share = w.starts_with("\\\\") && w.len() > 3;
    let home = w.starts_with("~/") || w.starts_with("~\\");
    let unix = w.starts_with('/') && w.len() > 2 && w[1..].contains('/') && !w.contains("//");
    drive || share || home || unix
}

fn expand_home(p: &str) -> std::path::PathBuf {
    if let Some(rest) = p.strip_prefix("~/").or_else(|| p.strip_prefix("~\\")) {
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            return std::path::PathBuf::from(home).join(rest);
        }
    }
    std::path::PathBuf::from(p)
}

/// The packages a Python draft declares it needs: PEP 723's block at the top
/// (`# /// script` ... `# dependencies = ["requests"]` ... `# ///`), or a
/// plain `# requires: requests, rich`. Only names a package index would take
/// -- letters, digits, `-_.` and version marks -- never a path or an address,
/// and at most ten (2 Oct 2026).
pub fn python_deps(code: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    let mut in_block = false;
    let mut block = String::new();
    for line in code.lines() {
        let t = line.trim();
        if t == "# /// script" {
            in_block = true;
            continue;
        }
        if in_block {
            if t == "# ///" {
                in_block = false;
                continue;
            }
            block.push_str(t.trim_start_matches('#'));
            block.push('\n');
            continue;
        }
        if let Some(r) = t.strip_prefix("# requires:").or_else(|| t.strip_prefix("# Requires:")) {
            found.extend(r.split(',').map(|x| x.trim().to_string()));
        }
    }
    if let Some(i) = block.find("dependencies") {
        let after = &block[i..];
        if let (Some(a), Some(b)) = (after.find('['), after.find(']')) {
            if a < b {
                for item in after[a + 1..b].split(',') {
                    found.push(item.trim().trim_matches(['"', '\'']).to_string());
                }
            }
        }
    }
    let ok = |d: &str| {
        !d.is_empty()
            && d.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
            && d.chars().all(|c| c.is_ascii_alphanumeric() || "-_.[]<>=!~,".contains(c))
    };
    let mut out: Vec<String> = Vec::new();
    for d in found {
        if ok(&d) && !out.contains(&d) {
            out.push(d);
        }
    }
    out.truncate(10);
    out
}

/// Where a Python draft's packages are installed in its sandbox, and the
/// files that let the checks see them there -- pytest, mypy and ruff each
/// told about the folder and told not to check the packages themselves.
pub const PY_DEPS_DIR: &str = ".deps";

pub fn python_dep_files() -> Vec<(String, String)> {
    vec![
        ("conftest.py".into(), format!("import os, sys\nsys.path.insert(0, os.path.join(os.path.dirname(__file__), \"{PY_DEPS_DIR}\"))\n")),
        ("mypy.ini".into(), format!("[mypy]\nignore_missing_imports = True\nexclude = ^{}/\nmypy_path = {PY_DEPS_DIR}\n", PY_DEPS_DIR.replace('.', "\\."))),
        ("ruff.toml".into(), format!("extend-exclude = [\"{PY_DEPS_DIR}\", \"conftest.py\"]\n")),
    ]
}

/// The last thing built: where it is and what it's written in, for "run it"
/// (2 Oct 2026). Kept beside the builds.
#[derive(Debug, Clone, PartialEq, serde::Serialize, Deserialize)]
pub struct LastBuild {
    pub path: String,
    pub lang: Lang,
    /// Passed its checks; a draft that didn't is still runnable, and said so.
    pub built: bool,
}

impl LastBuild {
    pub fn path() -> std::path::PathBuf {
        crate::roots::data_sub("builds").join("last-build.json")
    }

    pub fn keep(&self) {
        let p = LastBuild::path();
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        if let Ok(j) = serde_json::to_string(self) {
            let _ = std::fs::write(p, j);
        }
    }

    pub fn last() -> Option<LastBuild> {
        serde_json::from_str(&std::fs::read_to_string(LastBuild::path()).ok()?).ok()
    }
}

/// The yes to "run it?" coming back (`Intent::RunBuild`).
pub const RUN_CONFIRMED: &str = "\u{2192}yes";

/// How long a build is let run when you ask to run it, in seconds.
pub const RUN_FOR_SECS: u64 = 120;

/// How to run a built file: the program and its arguments, or `None` for
/// what isn't a program on its own (a Rust draft is a library; a C++ draft
/// is source). Python with packages runs through uv, which fetches them for
/// that run only; without uv it runs on the plain Python and may not find
/// them, which the reply says (2 Oct 2026).
pub fn run_command(lang: Lang, file: &std::path::Path, deps: &[String], uv: Option<&std::path::Path>, python: Option<&str>) -> Option<(String, Vec<String>)> {
    let f = file.to_string_lossy().into_owned();
    match lang {
        Lang::Python => match (uv, deps.is_empty()) {
            (Some(uv), false) => {
                let mut args = vec!["run".to_string(), "--no-project".to_string()];
                for d in deps {
                    args.push("--with".into());
                    args.push(d.clone());
                }
                args.push(f);
                Some((uv.to_string_lossy().into_owned(), args))
            }
            _ => Some((python?.to_string(), vec![f])),
        },
        Lang::JavaScript | Lang::TypeScript => Some(("node".into(), vec![f])),
        Lang::Go => Some(("go".into(), vec!["run".into(), f])),
        Lang::Rust | Lang::Cpp => None,
    }
}
