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
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            enabled: true,
            max_fix_rounds: 3,
            default_language: Lang::Rust,
            render_svg_command: String::new(),
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

/// The instruction for a fix round. The model is given its own code and the
/// tool's verbatim complaint, and asked to return the whole corrected file.
pub const FIX_SYSTEM: &str =
    "The code you wrote did not pass a check. You are given the code and the \
     tool's exact output. Fix the specific problem the tool reports and return \
     the COMPLETE corrected file in a single fenced code block — not a diff, \
     not just the changed lines, and no explanation. Change as little as \
     possible beyond what the tool is complaining about.";

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
            Outcome::Unchecked { .. } => format!("Queued \"{title}\" on {project} as an unchecked draft. {detail}"),
        }
    }

    /// The code to hand over or land, whichever outcome — even a struggle
    /// produces a best draft worth keeping.
    pub fn code(&self) -> Option<&str> {
        match self {
            Outcome::Built { code, .. } | Outcome::Struggled { code, .. } | Outcome::Unchecked { code, .. } => Some(code),
            Outcome::NoDraft(_) => None,
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
/// with the failure attached, never as if it worked.
pub fn build_loop(
    description: &str,
    lang: Lang,
    llm: &dyn Llm,
    max_rounds: u32,
    mut check: impl FnMut(&str) -> Check,
) -> Outcome {
    let mut code = match generate(description, lang, llm) {
        Ok(c) => c,
        Err(e) => return Outcome::NoDraft(e),
    };
    let mut rounds = 0u32;
    loop {
        match check(&code) {
            Check::Passed(notes) => return Outcome::Built { code, rounds, notes },
            Check::CannotCheck(missing) => return Outcome::Unchecked { code, rounds, missing },
            Check::Failed(output) => {
                if rounds >= max_rounds {
                    return Outcome::Struggled { code, rounds, last_failure: output };
                }
                rounds += 1;
                match fix_draft(&code, &output, llm) {
                    Ok(c) => code = c,
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

/// Turn a description into a first draft. Pulls the code out of whatever the
/// model wraps it in.
pub fn generate(description: &str, lang: Lang, llm: &dyn Llm) -> Result<String, String> {
    let user = format!("Language: {}\n\nWhat to build:\n{description}", lang.plain());
    // Writing code from a description is a hard task: escalate to the stronger
    // model when one is configured.
    let reply = llm.complete_hard(GENERATE_SYSTEM, &user).map_err(|e| e.to_string())?;
    let code = extract_code(&reply);
    if code.trim().is_empty() {
        return Err("the model returned no code".into());
    }
    Ok(code)
}

/// One fix round: hand the model its code and the tool's exact words, take
/// back the whole corrected file.
pub fn fix_draft(code: &str, failure_output: &str, llm: &dyn Llm) -> Result<String, String> {
    let user = format!(
        "The tool said:\n{failure_output}\n\nThe code was:\n```\n{code}\n```\n\n\
         Return the complete corrected file.");
    let reply = llm.complete_hard(FIX_SYSTEM, &user).map_err(|e| e.to_string())?;
    let fixed = extract_code(&reply);
    if fixed.trim().is_empty() {
        return Err("the model returned no code on the fix round".into());
    }
    Ok(fixed)
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
        let code = generate("a function that adds two numbers", Lang::Rust, &llm).unwrap();
        assert_eq!(code, "fn add(a: i32, b: i32) -> i32 { a + b }");
    }

    #[test]
    fn fix_draft_returns_the_corrected_code_from_a_model() {
        let llm = MockLlm("```rust\nfn add(a: i32, b: i32) -> i32 { a + b }\n```".into());
        let fixed = fix_draft("fn add(a,b){a+b}", "error: expected type", &llm).unwrap();
        assert!(fixed.contains("i32"), "the fix round returns the corrected file: {fixed}");
    }

    #[test]
    fn generate_errors_when_the_model_returns_nothing_usable() {
        let llm = MockLlm("   ".into());
        assert!(generate("do a thing", Lang::Rust, &llm).is_err());
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
            if let Ok(next) = fix_draft(&code, &failure, llm) {
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
    const SKIP: &[&str] = &["a", "an", "the", "me", "my", "that", "which", "to", "in", "of", "for", "and", "write", "build", "make", "script", "program", "code", "python", "rust", "go", "javascript", "typescript"];
    let words: Vec<String> = description
        .to_lowercase()
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty() && !SKIP.contains(w))
        .take(5)
        .map(str::to_string)
        .collect();
    let stem = if words.is_empty() { "build".to_string() } else { words.join("-") };
    let mut path = dir.join(format!("{stem}.{ext}"));
    let mut n = 2;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}.{ext}"));
        n += 1;
    }
    path
}
