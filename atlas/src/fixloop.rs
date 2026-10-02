//! The hand-off loop: working a failing test with a counsel until it passes.
//!
//! `strategy` knows twelve genuinely different angles on a stuck problem,
//! `handoff` writes the brief a reader needs and pulls code back out of an
//! answer, and `consult` keeps a conversation honest — only a testable answer
//! costs an attempt, a question gets answered, a lecture gets asked for the
//! change. All three were built, tested, and driven by nothing. This drives
//! them, in the shape the SWE-agent work (MIT) found makes a model useful on
//! code: small bounded steps, the real test output fed back every time, a
//! fixed budget, and a person's yes before anything leaves the sandbox.
//!
//! The counsel is whatever answers: the local model (offline, the default),
//! the stronger online one if you've set it, or a script in a test. The loop
//! works in a copy of the folder (`sandbox`); your files are never touched.
//! What comes out is either a tested change with its diff, or — when every
//! angle is spent — the brief, written up for you or for someone else.

use crate::consult::{classify, ConsultConfig, Consultation, Move};
use crate::handoff::{write_brief, HandoffConfig, Problem, Snippet};
use crate::strategy::{Campaign, Effort, Next, StrategyConfig};
use std::path::{Path, PathBuf};

/// Whoever is answering.
pub trait Counsel {
    /// Send one message and get the whole reply back.
    fn ask(&mut self, message: &str) -> Result<String, String>;
}

/// The configured model as counsel, with the conversation so far resent each
/// turn (models here are stateless).
pub struct ModelCounsel<'a> {
    llm: &'a dyn crate::brain::Llm,
    transcript: String,
}

impl<'a> ModelCounsel<'a> {
    pub fn new(llm: &'a dyn crate::brain::Llm) -> Self {
        ModelCounsel { llm, transcript: String::new() }
    }
}

const SYSTEM: &str = "You are helping fix a failing test. Answer with the complete new contents of \
     each file you change, each in its own fenced code block, with the file's path on the line \
     just above the block. If you need to see something first, ask for it.";

impl Counsel for ModelCounsel<'_> {
    fn ask(&mut self, message: &str) -> Result<String, String> {
        self.transcript.push_str(&format!("\n\n## Me\n{message}"));
        // A hard task by the brain's own definition: it goes to the stronger
        // model when one is configured, and stays local when not.
        let reply = self.llm.complete_hard(SYSTEM, &self.transcript).map_err(|e| e.to_string())?;
        self.transcript.push_str(&format!("\n\n## You\n{reply}"));
        Ok(reply)
    }
}

/// What to work on.
pub struct Job {
    /// The folder with the failing code. Copied; never written.
    pub folder: PathBuf,
    /// The test, run inside the copy: program then arguments.
    pub test: Vec<String>,
    /// What you were trying to do, in a sentence.
    pub goal: String,
}

/// How it went.
#[derive(Debug, Clone, PartialEq)]
pub struct Outcome {
    pub solved: bool,
    /// Solutions tried (each one applied and tested).
    pub attempts: u32,
    /// Messages exchanged.
    pub exchanges: usize,
    /// The copy it worked in.
    pub copy: PathBuf,
    /// Files changed in the copy: relative path, new contents.
    pub changes: Vec<(String, String)>,
    /// The changes as a diff, for reading before anything lands.
    pub diff: String,
    /// Written when it didn't get there: the brief for whoever picks it up.
    pub brief: Option<String>,
    /// One line per step, for the log and for you.
    pub steps: Vec<String>,
}

/// Files copied into the sandbox: source, not build output.
fn copyable(p: &Path) -> bool {
    !p.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        s == "target" || s == ".git" || s == "node_modules" || s == "__pycache__"
    })
}

fn copy_tree(from: &Path, to: &Path, rel: &Path, out: &mut Vec<String>) -> std::io::Result<()> {
    let dest = std::fs::canonicalize(to).unwrap_or_else(|_| to.to_path_buf());
    for e in std::fs::read_dir(from.join(rel))? {
        let e = e?;
        let r = rel.join(e.file_name());
        if !copyable(&r) {
            continue;
        }
        // Never copy the copy into itself: the work folder can sit inside the
        // folder being worked on (Atlas's own data folder, say).
        let here = std::fs::canonicalize(e.path()).unwrap_or_else(|_| e.path());
        if dest.starts_with(&here) || here.starts_with(&dest) {
            continue;
        }
        if e.file_type()?.is_dir() {
            std::fs::create_dir_all(to.join(&r))?;
            copy_tree(from, to, &r, out)?;
        } else if e.metadata()?.len() <= 512 * 1024 {
            std::fs::copy(e.path(), to.join(&r))?;
            out.push(r.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// How long one test run in the loop may take.
pub const TEST_LIMIT_SECS: u64 = 20 * 60;

fn run_test(dir: &Path, test: &[String]) -> (bool, String) {
    let Some((prog, args)) = test.split_first() else { return (false, "no test command given".into()) };
    // Python caches compiled files by modification time and size to the
    // second; a fix written in the same second as a same-length wrong one
    // would run the wrong one again. Found by this module's own test.
    // Through the sandbox's runner, for its time limit: a test that hangs
    // stops the attempt, not Atlas (research report, Stage 1 item 1).
    crate::sandbox::run_within(prog, args, &[("PYTHONDONTWRITEBYTECODE", "1")], dir, TEST_LIMIT_SECS, 4000)
}

fn first_problem(output: &str) -> String {
    output
        .lines()
        .find(|l| {
            let t = l.trim_start().to_lowercase();
            t.starts_with("error") || t.contains("failed") || t.contains("assert") || t.contains("panicked") || t.contains("traceback")
        })
        .unwrap_or_else(|| output.lines().find(|l| !l.trim().is_empty()).unwrap_or(""))
        .trim()
        .to_string()
}

/// The files the test output names, else the smallest few, as snippets.
fn snippets(copy: &Path, files: &[String], output: &str, max_lines: usize) -> Vec<Snippet> {
    let named: Vec<&String> = files.iter().filter(|f| output.contains(f.as_str()) || output.contains(Path::new(f).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default().as_str())).collect();
    let pick: Vec<&String> = if named.is_empty() { files.iter().take(4).collect() } else { named.into_iter().take(4).collect() };
    pick.into_iter()
        .filter_map(|f| {
            let text = std::fs::read_to_string(copy.join(f)).ok()?;
            let lines: Vec<&str> = text.lines().collect();
            Some(Snippet { path: f.clone(), from_line: 1, text: lines[..lines.len().min(max_lines)].join("\n") })
        })
        .collect()
}

/// Work the job. `base` is where the copy goes.
pub fn run(
    job: &Job,
    counsel: &mut dyn Counsel,
    base: &Path,
    strategy: &StrategyConfig,
    handoff: &HandoffConfig,
    consult: &ConsultConfig,
) -> Result<Outcome, String> {
    let sandbox = crate::sandbox::Sandbox::create(base, "fix").map_err(|e| e.to_string())?;
    let copy = sandbox.root.clone();
    let mut files = Vec::new();
    copy_tree(&job.folder, &copy, Path::new(""), &mut files).map_err(|e| format!("couldn't copy {}: {e}", job.folder.display()))?;
    files.sort();
    let originals: Vec<(String, String)> = files.iter().filter_map(|f| std::fs::read_to_string(copy.join(f)).ok().map(|t| (f.clone(), t))).collect();

    let mut steps = Vec::new();
    let (mut passed, mut output) = run_test(&copy, &job.test);
    steps.push(format!("ran the test: {}", if passed { "passes already" } else { "fails" }));
    let mut problem = Problem {
        goal: job.goal.clone(),
        error: first_problem(&output),
        test_output: output.clone(),
        snippets: snippets(&copy, &files, &output, handoff.max_snippet_lines),
        ..Default::default()
    };
    let mut campaign = Campaign::new(&job.goal);
    let mut talk = Consultation::new(&job.goal);
    let budget = handoff.attempts_before_asking;

    let mut opened = false;
    while !passed {
        let (angle, instruction) = match campaign.next(strategy) {
            Next::Try { angle, instruction } => (angle, instruction),
            Next::Done => break,
            Next::Exhausted(why) => {
                steps.push(format!("stopped: {why}"));
                break;
            }
        };
        let mut message = if !opened {
            opened = true;
            match talk.open(&write_brief(&problem, handoff), consult) {
                Move::Open { message, attach } => format!("{message}\n\n{attach}\n\nApproach: {instruction}"),
                _ => unreachable!(),
            }
        } else {
            format!("{}\n\nTry a different approach this time: {instruction}", talk.report_result(false, &output))
        };
        // One angle: talk until something testable comes back.
        let mut tested = false;
        loop {
            let reply = counsel.ask(&message)?;
            let kind = classify(&reply);
            let left = budget.saturating_sub(talk.attempts_used);
            match talk.next(&kind, left, consult) {
                Move::Test(blocks) => {
                    talk.record(&message, &reply, true);
                    let mut wrote = Vec::new();
                    for b in &blocks {
                        let target = b.path.clone().or_else(|| (problem.snippets.len() == 1).then(|| problem.snippets[0].path.clone()));
                        if let Some(t) = target {
                            let mut t = t.trim_start_matches("./").to_string();
                            // A bare name means the file of that name in the folder.
                            if !t.contains('/') {
                                if let Some(f) = files.iter().find(|f| f.rsplit('/').next() == Some(t.as_str())) {
                                    t = f.clone();
                                }
                            }
                            if sandbox.write(&t, &b.code).is_ok() {
                                wrote.push(t);
                            }
                        }
                    }
                    let (p, out) = run_test(&copy, &job.test);
                    passed = p;
                    output = out;
                    steps.push(format!(
                        "{}: applied {} — {}",
                        angle.label(),
                        if wrote.is_empty() { "nothing (no file named)".to_string() } else { wrote.join(", ") },
                        if passed { "the test passes".to_string() } else { format!("still failing: {}", first_problem(&output)) }
                    ));
                    campaign.record(Effort {
                        angle,
                        learned: reply.lines().find(|l| !l.trim().is_empty() && !l.starts_with("```")).unwrap_or("").trim().to_string(),
                        error: first_problem(&output),
                        solved: passed,
                    });
                    problem.test_output = output.clone();
                    problem.error = first_problem(&output);
                    tested = true;
                    break;
                }
                Move::Reply(text) => {
                    talk.record(&message, &reply, false);
                    // Asked to see something: give it the files it could mean.
                    let extra = match &kind {
                        crate::consult::Reply::WantsMore(_) => snippets(&copy, &files, &reply, handoff.max_snippet_lines)
                            .iter()
                            .map(|s| format!("{}:\n```\n{}\n```", s.path, s.text))
                            .collect::<Vec<_>>()
                            .join("\n\n"),
                        _ => String::new(),
                    };
                    steps.push(format!("{}: {} (no attempt spent)", angle.label(), match kind {
                        crate::consult::Reply::Question(_) => "answered a question",
                        crate::consult::Reply::WantsMore(_) => "sent what was asked for",
                        _ => "asked for the change itself",
                    }));
                    message = if extra.is_empty() { text } else { format!("{text}\n\n{extra}") };
                }
                Move::Stop(why) => {
                    steps.push(format!("stopped: {why}"));
                    break;
                }
                Move::Wait | Move::Open { .. } => break,
            }
        }
        if !tested {
            break;
        }
    }

    let mut changes = Vec::new();
    let mut diff = String::new();
    for f in &files {
        let now = std::fs::read_to_string(copy.join(f)).unwrap_or_default();
        let before = originals.iter().find(|(p, _)| p == f).map(|(_, t)| t.as_str()).unwrap_or("");
        if now != before {
            diff.push_str(&crate::diff::unified(before, &now, f, f, 3));
            changes.push((f.clone(), now));
        }
    }
    let brief = (!passed).then(|| {
        let mut p = problem.clone();
        p.ruled_out = campaign.what_was_learned();
        write_brief(&p, handoff)
    });
    Ok(Outcome {
        solved: passed,
        attempts: talk.attempts_used,
        exchanges: talk.exchanges.len(),
        copy,
        changes,
        diff,
        brief,
        steps,
    })
}

/// Put a solved outcome's files into the real folder, keeping each original
/// as `<name>.before`. Only ever called on your yes.
pub fn land(o: &Outcome, folder: &Path) -> Result<Vec<String>, String> {
    if !o.solved {
        return Err("it didn't pass, so there's nothing to land".into());
    }
    let mut done = Vec::new();
    for (rel, text) in &o.changes {
        let target = folder.join(rel);
        if target.exists() {
            std::fs::copy(&target, target.with_extension(format!("{}.before", target.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default())))
                .map_err(|e| format!("couldn't keep the original of {rel}: {e}; nothing landed for it"))?;
        }
        std::fs::write(&target, text).map_err(|e| format!("couldn't write {rel}: {e}"))?;
        done.push(rel.clone());
    }
    Ok(done)
}
