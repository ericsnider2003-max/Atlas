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

fn copy_tree(from: &Path, to: &Path, rel: &Path, out: &mut Vec<String>, budget: Option<&crate::tools::WorkBudget<'_>>, bytes: &mut u64) -> std::io::Result<()> {
    let dest = std::fs::canonicalize(to).unwrap_or_else(|_| to.to_path_buf());
    for e in std::fs::read_dir(from.join(rel))? {
        if let Some(b) = budget { b.check().map_err(std::io::Error::other)?; }
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
        if e.file_type()?.is_symlink() { return Err(std::io::Error::other("repair input is a symbolic link; no complete copy was made")); }
        if e.file_type()?.is_dir() {
            std::fs::create_dir_all(to.join(&r))?;
            copy_tree(from, to, &r, out, budget, bytes)?;
        } else if e.metadata()?.len() <= 512 * 1024 {
            *bytes = bytes.saturating_add(e.metadata()?.len());
            if *bytes > 64 * 1024 * 1024 || out.len() >= 4096 { return Err(std::io::Error::other("repair inputs exceed the bounded copy budget")); }
            std::fs::copy(e.path(), to.join(&r))?;
            out.push(r.to_string_lossy().replace('\\', "/"));
        } else { return Err(std::io::Error::other("a repair input exceeds the per-file copy budget; no complete copy was made")); }
    }
    Ok(())
}

/// How long one test run in the loop may take.
pub const TEST_LIMIT_SECS: u64 = 20 * 60;

fn run_test(dir: &Path, test: &[String], work: Option<&crate::tools::WorkBudget<'_>>) -> (bool, String) {
    let Some((prog, args)) = test.split_first() else { return (false, "no test command given".into()) };
    // Python caches compiled files by modification time and size to the
    // second; a fix written in the same second as a same-length wrong one
    // would run the wrong one again. Found by this module's own test.
    // Through the sandbox's runner, for its time limit: a test that hangs
    // stops the attempt, not Atlas (research report, Stage 1 item 1).
    let stop = || work.is_some_and(|b| b.stopping());
    let limit = work.map_or(TEST_LIMIT_SECS, |b| b.remaining(std::time::Duration::from_secs(TEST_LIMIT_SECS)).as_secs().max(1));
    let result = crate::sandbox::run_within_controlled(prog, args, &[("PYTHONDONTWRITEBYTECODE", "1")], dir, limit, 4000, Some(&stop));
    if let Some(b) = work { if let Err(why) = b.check() { return (false, format!("error: {why}")); } }
    result
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
    run_controlled(job, counsel, base, strategy, handoff, consult, None)
}

pub(crate) fn run_controlled(job: &Job, counsel: &mut dyn Counsel, base: &Path, strategy: &StrategyConfig, handoff: &HandoffConfig, consult: &ConsultConfig, work: Option<&crate::tools::WorkBudget<'_>>) -> Result<Outcome, String> {
    if let Some(work) = work { work.check()?; }
    let sandbox = crate::sandbox::Sandbox::create(base, "fix").map_err(|e| e.to_string())?;
    let copy = sandbox.root.clone();
    let mut files = Vec::new();
    copy_tree(&job.folder, &copy, Path::new(""), &mut files, work, &mut 0u64).map_err(|e| format!("couldn't copy {}: {e}", job.folder.display()))?;
    files.sort();
    let originals: Vec<(String, String)> = files.iter().filter_map(|f| std::fs::read_to_string(copy.join(f)).ok().map(|t| (f.clone(), t))).collect();

    let mut steps = Vec::new();
    let (mut passed, mut output) = run_test(&copy, &job.test, work);
    steps.push(format!("ran the test: {}", if passed { "passes already" } else { "fails" }));
    // Search the bounded local recipe set before consulting any model.
    // Failed candidates are reverted exactly; the original tests stay intact.
    if !passed && crate::selfwork::read_run(&output).failed > 0 {
        let original_edits: Vec<_> = originals.iter().map(|(path, content)| crate::selfwork::Edit { path: path.clone(), content: content.clone(), reason: "unchanged source".into() }).collect();
        for (recipe, edits) in crate::selfwork::recipe_candidates(&original_edits) {
            if let Some(work) = work { work.check()?; }
            for edit in &edits { sandbox.write(&edit.path, &edit.content).map_err(|e| e.to_string())?; }
            let (green, said) = run_test(&copy, &job.test, work);
            if green && crate::selfwork::count_passing(&said) >= crate::selfwork::count_passing(&output) && crate::selfwork::count_passing(&said) > 0 {
                passed = true; output = said; steps.push(format!("local recipe proved: {recipe}")); break;
            }
            for edit in &edits {
                let before = originals.iter().find(|(path, _)| path == &edit.path).ok_or("the recipe named an unknown source file")?;
                sandbox.write(&edit.path, &before.1).map_err(|e| e.to_string())?;
            }
        }
    }
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
        if let Some(work) = work { work.check()?; }
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
            if let Some(work) = work { work.check()?; }
            let reply = counsel.ask(&message)?;
            if let Some(work) = work { work.check()?; }
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
                    let (p, out) = run_test(&copy, &job.test, work);
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

#[cfg(test)]
mod local_recipe_tests {
    use super::*;
    struct NoModel;
    impl Counsel for NoModel { fn ask(&mut self, _: &str) -> Result<String, String> { panic!("the local recipe must solve this without model consultation") } }
    #[test]
    fn actual_failing_python_total_is_repaired_in_copy_without_model_or_test_edit() {
        let Some(python) = crate::tools::which("python") else { eprintln!("Python is unavailable; real recipe fixture did not run"); return; };
        let root = std::env::temp_dir().join(format!("atlas-local-recipe-{}-{}", std::process::id(), crate::store::now()));
        let source = root.join("source"); let copies = root.join("copies");
        std::fs::create_dir_all(&source).unwrap();
        let broken = "def total(prices):\n    return sum(prices[:-1])\n";
        let tests = "from pricing import total\nimport sys\npassed = 0\nfailed = 0\nfor values, expected in [([0], 0), ([5, 7, 11], 23)]:\n    try:\n        assert total(values) == expected\n        passed += 1\n    except AssertionError:\n        failed += 1\nprint('test result: ' + ('FAILED.' if failed else 'ok.') + f' {passed} passed; {failed} failed')\nsys.exit(1 if failed else 0)\n";
        std::fs::write(source.join("pricing.py"), broken).unwrap();
        std::fs::write(source.join("check.py"), tests).unwrap();
        let job = Job { folder: source.clone(), test: vec![python, "-B".into(), "check.py".into()], goal: "The complete list must contribute to the total".into() };
        let stop = || false;
        let budget = crate::tools::WorkBudget::new(std::time::Duration::from_secs(20), &stop);
        let result = run_controlled(&job, &mut NoModel, &copies, &Default::default(), &Default::default(), &Default::default(), Some(&budget)).unwrap();
        assert!(result.solved); assert_eq!(result.exchanges, 0);
        assert!(result.steps.first().unwrap().contains("fails"));
        assert!(result.steps.iter().any(|step| step.contains("local recipe proved")));
        assert_eq!(result.changes.len(), 1); assert_eq!(result.changes[0].0, "pricing.py");
        assert_eq!(std::fs::read_to_string(source.join("pricing.py")).unwrap(), broken);
        assert_eq!(std::fs::read_to_string(result.copy.join("check.py")).unwrap(), tests);
        assert_eq!(std::fs::read_to_string(result.copy.join("pricing.py")).unwrap(), "def total(prices):\n    return sum(prices)\n");
        let _ = std::fs::remove_dir_all(root);
    }
}
#[cfg(test)]
mod native_repair_proof {
    use super::*;
    use crate::brain::Llm;
    fn model() -> crate::brain::ShellLlm {
        let endpoint = std::env::var("ATLAS_NATIVE_REPAIR_URL").expect("explicit local fixture endpoint required");
        assert!(endpoint.starts_with("http://127.0.0.1:"));
        assert!(endpoint.ends_with("/v1/chat/completions"));
        crate::brain::ShellLlm { cfg: crate::brain::LlmConfig {
            tool: crate::tools::ExternalTool { command: "curl".into(), args: vec![endpoint], ..Default::default() },
            request: String::new(), response_path: "choices.0.message.content".into(), vision_request: None,
        }, vars: Default::default() }
    }
    fn fixture() -> (std::path::PathBuf, Job, String, String) {
        let root = std::env::temp_dir().join(format!("atlas-native-repair-proof-{}-{}", std::process::id(), crate::store::now()));
        let source = root.join("original"); std::fs::create_dir_all(&source).unwrap();
        let code = "def largest(values):\n    return min(values, default=0)\n".to_string();
        let tests = "from largest import largest\nimport sys\npassed = 0\nfailed = 0\nfor values, expected in [([], 0), ([7, 3], 7), ([-8, -2], -2)]:\n    try:\n        assert largest(values) == expected\n        passed += 1\n    except AssertionError:\n        failed += 1\nprint('test result: ' + ('FAILED.' if failed else 'ok.') + f' {passed} passed; {failed} failed')\nsys.exit(1 if failed else 0)\n".to_string();
        std::fs::write(source.join("largest.py"), &code).unwrap(); std::fs::write(source.join("check.py"), &tests).unwrap();
        let python = crate::tools::which("python").expect("installed Python required");
        let job = Job { folder: source, test: vec![python, "-B".into(), "check.py".into()], goal: "largest returns the greatest value; empty input returns zero. Keep all tests unchanged.".into() };
        (root, job, code, tests)
    }
    #[test]
    #[ignore = "explicit local 7B endpoint proof, not a regular suite test"]
    fn installed_coder_repairs_real_red_tests_in_copy() {
        let model = model(); assert!(model.supports_bounded_chat());
        let (root, job, code, tests) = fixture();
        let stop = || false; let budget = crate::tools::WorkBudget::new(std::time::Duration::from_secs(180), &stop);
        let bounded = crate::selfwork::BoundedModel { model: &model, budget: &budget };
        let mut counsel = ModelCounsel::new(&bounded);
        let strategy = StrategyConfig { max_angles: 2, stop_after_identical: 2, ..Default::default() };
        let started = std::time::Instant::now();
        let outcome = run_controlled(&job, &mut counsel, &root.join("copies"), &strategy, &Default::default(), &Default::default(), Some(&budget)).unwrap();
        assert!(outcome.steps.first().unwrap().contains("fails"));
        assert!(outcome.solved && outcome.exchanges > 0 && outcome.attempts > 0, "{}", outcome.steps.join(" | "));
        assert_eq!(outcome.changes.len(), 1); assert_eq!(outcome.changes[0].0, "largest.py");
        assert_eq!(std::fs::read_to_string(job.folder.join("largest.py")).unwrap(), code);
        assert_eq!(std::fs::read_to_string(job.folder.join("check.py")).unwrap(), tests);
        assert_eq!(std::fs::read_to_string(outcome.copy.join("check.py")).unwrap(), tests);
        let (passed, output) = crate::sandbox::run_within_controlled(&job.test[0], &job.test[1..], &[], &outcome.copy, 10, 8192, Some(&stop));
        assert!(passed && crate::selfwork::count_passing(&output) == 3, "{output}");
        eprintln!("native 7B repair elapsed {:?}; attempts {}; exchanges {}", started.elapsed(), outcome.attempts, outcome.exchanges);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    #[ignore = "explicit local 7B endpoint cancellation proof"]
    fn native_repair_stop_after_model_request_preserves_originals() {
        use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
        struct Observed<'a> { inner: ModelCounsel<'a>, requested: Arc<AtomicBool> }
        impl Counsel for Observed<'_> {
            fn ask(&mut self, message: &str) -> Result<String, String> {
                self.requested.store(true, Ordering::SeqCst);
                self.inner.ask(message)
            }
        }
        let model = model(); let (root, job, code, tests) = fixture();
        let requested = Arc::new(AtomicBool::new(false)); let stopped = Arc::new(AtomicBool::new(false));
        let request_flag = requested.clone(); let stop_flag = stopped.clone();
        let timer = std::thread::spawn(move || {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
            while !request_flag.load(Ordering::SeqCst) && std::time::Instant::now() < deadline { std::thread::sleep(std::time::Duration::from_millis(5)); }
            assert!(request_flag.load(Ordering::SeqCst), "repair never reached the native model");
            std::thread::sleep(std::time::Duration::from_millis(100)); stop_flag.store(true, Ordering::SeqCst);
        });
        let stop = || stopped.load(Ordering::SeqCst);
        let budget = crate::tools::WorkBudget::new(std::time::Duration::from_secs(20), &stop);
        let bounded = crate::selfwork::BoundedModel { model: &model, budget: &budget };
        let mut counsel = Observed { inner: ModelCounsel::new(&bounded), requested };
        let started = std::time::Instant::now();
        let result = run_controlled(&job, &mut counsel, &root.join("copies"), &Default::default(), &Default::default(), &Default::default(), Some(&budget));
        timer.join().unwrap();
        assert!(result.is_err(), "interrupted native repair must not claim a solved proposal");
        assert!(started.elapsed() < std::time::Duration::from_secs(8), "native cancellation exceeded its bounded response target");
        assert_eq!(std::fs::read_to_string(job.folder.join("largest.py")).unwrap(), code);
        assert_eq!(std::fs::read_to_string(job.folder.join("check.py")).unwrap(), tests);
        std::fs::remove_dir_all(root).unwrap();
    }
}
