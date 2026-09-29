//! Atlas working on Atlas.
//!
//! **This module is the machinery; `pipeline` is the discipline.** What was
//! here went straight from a goal to an attempt, which is how a self-improving
//! system rots: green tests say nothing about whether the right thing was
//! fixed, and a patched symptom passes exactly as well as a real fix.
//!
//! `Session::after` still drives the attempts. What changed is that a session
//! can no longer start without a diagnosis, or land without a review.
//!
//! The pieces have all existed for a while — a sandbox it can be wrong in, a
//! ladder of distinct approaches, a way to ask for help, and 1158 tests. This
//! is the loop that joins them.
//!
//! The test suite is what makes this reasonable rather than reckless. A change
//! that breaks something gets caught before you ever see it, and nothing
//! reaches your files until you've seen the diff.

use crate::sandbox::{Attempt, Sandbox};
use crate::strategy::{Angle, Campaign, Effort, Next, StrategyConfig};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SelfWorkConfig {
    pub enabled: bool,
    /// Files it may change. Anything outside is refused.
    pub may_touch: Vec<String>,
    /// Files it may never change, however it's asked.
    pub never_touch: Vec<String>,
    /// The command that proves a change is safe.
    pub test_command: String,
    /// Tests must not drop below this. A change that "fixes" something by
    /// deleting tests is not a fix.
    pub min_tests: usize,
    /// Lines changed in one go before it wants a second look.
    pub large_change_lines: usize,
}

impl Default for SelfWorkConfig {
    fn default() -> Self {
        SelfWorkConfig {
            enabled: false,
            may_touch: vec!["src/".into(), "tests/".into(), "config/".into(), "docs/".into()],
            never_touch: vec![
                // The things that would let it widen its own permissions or
                // hide what it did.
                "config/policy.yaml".into(),
                "src/policy.rs".into(),
                "src/system.rs".into(),
                "src/finance.rs".into(),
                "src/consent.rs".into(),
                "Cargo.toml".into(),
            ],
            test_command: "cargo test".into(),
            min_tests: 1,
            large_change_lines: 150,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Edit {
    pub path: String,
    /// The whole new file. Whole-file rather than a patch, so what lands is
    /// exactly what was tested.
    pub content: String,
    /// One line on why.
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// Allowed to try it.
    Allowed,
    Refused(String),
}

/// May Atlas change this file?
///
/// The never-list is the important half. Those are the files that decide what
/// Atlas is allowed to do and what it has to tell you — a system that can edit
/// its own permissions has none.
///
/// ## Why the configured never-list is a floor and not the rule
///
/// It used to be the rule, and the rule had a hole the size of the thing it
/// was protecting. `cfg.never_touch` defaults to six paths, and
/// `cfg.may_touch` is `["src/", "tests/", "config/", "docs/"]` — so on the
/// shipped config Atlas could edit:
///
/// * **`config/tools.yaml`**, which is where `self_grant:` lives. That is the
///   setting that says how far Atlas may go alone. A system that can edit the
///   file holding its own grant does not have a grant, it has a suggestion.
/// * **`tests/wiring.rs`, `tests/guards.rs`, `tests/ceiling.rs`** — the
///   ratchets. Those are the tests that would notice the previous line
///   happening. Editing the limit and editing the thing that reports the
///   limit was edited are the same act in two steps.
/// * **`src/grants.rs`, `src/selfgrant.rs`, `src/pipeline.rs`,
///   `src/categories.rs`, `src/confirmed.rs`, `src/voice.rs`**, and
///   `build.rs` / `.cargo/` / `.github/`, which can make the whole suite
///   vacuous without touching a test.
///
/// `selfgrant::reach_of` already knew every one of those — it classifies them
/// `Reach::ItsOwnLimits`, and `Reach::ever_grantable` says that reach is never
/// granted however the config is set. Nothing consulted it: `selfgrant::may_land`
/// has no production caller, and this function, which does, had never heard of
/// it. Two correct halves that had never been introduced.
///
/// So the order here matters: `reach_of` first, and **regardless of
/// `never_touch`**, because a list that can be edited cannot be the thing
/// protecting the list.
pub fn may_edit(path: &str, cfg: &SelfWorkConfig) -> Verdict {
    let p = path.replace('\\', "/");
    if p.contains("..") {
        return Verdict::Refused("that path climbs out of the project".into());
    }
    // Its own limits. Checked before the configured lists and not derived
    // from them; see the note above.
    if !crate::selfgrant::reach_of(&p).ever_grantable() {
        return Verdict::Refused(format!(
            "{p} decides what I'm allowed to do, or is what would notice me changing \
             that — I don't get to edit it however this is set"
        ));
    }
    for never in &cfg.never_touch {
        if p.ends_with(never.trim_start_matches("./")) || p == *never {
            return Verdict::Refused(format!(
                "{never} decides what I'm allowed to do — I don't get to edit that"
            ));
        }
    }
    if !cfg.may_touch.iter().any(|allowed| p.starts_with(allowed.trim_start_matches("./"))) {
        return Verdict::Refused(format!("{p} isn't in the parts of the project I work on"));
    }
    Verdict::Allowed
}

/// What happened when a change was tried.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tried {
    pub edits: Vec<Edit>,
    pub passed: bool,
    pub tests_run: usize,
    /// The first real error, when it failed.
    pub problem: Option<String>,
    pub lines_changed: usize,
}

impl Tried {
    /// Is this fit to show you?
    ///
    /// Passing is not enough. A change that passes because it deleted the
    /// tests that were failing has "passed" and is worthless.
    pub fn worth_showing(&self, before_tests: usize, cfg: &SelfWorkConfig) -> Result<(), String> {
        if !self.passed {
            return Err("the tests don't pass".into());
        }
        if self.tests_run < before_tests {
            return Err(format!(
                "there are {} fewer tests than before — a change that passes by deleting tests \
                 isn't a fix",
                before_tests - self.tests_run
            ));
        }
        if self.tests_run < cfg.min_tests {
            return Err("almost nothing ran, so passing means nothing".into());
        }
        Ok(())
    }

    fn is_large(&self, cfg: &SelfWorkConfig) -> bool {
        self.lines_changed > cfg.large_change_lines
    }
}

/// One run at fixing something.
/// Serialisable because a pipeline that is rebuilt every turn is a state
/// machine with no state. `Session::new` was called fresh on each
/// `WorkOnYourself`, so `stage` was always `Thought` and `thought` always
/// `None` — it answered "Thought next." forever and could not reach Build, let
/// alone Implement.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub goal: String,
    /// The five stages. A session with no thought behind it can't start.
    pub work: crate::pipeline::Work,
    pub campaign: Campaign,
    pub tests_before: usize,
    pub best: Option<Tried>,
    /// The diagnosis being collected, one answer per turn.
    ///
    /// `#[serde(default)]` because a session stored before this field existed
    /// still has to load — a self-work session that fails to deserialise
    /// comes back as `None` from the store and silently starts again, which is
    /// the same class of quiet loss the stage machine already had.
    #[serde(default)]
    pub diagnosing: crate::pipeline::Diagnosing,
}

/// What to do next.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// Try this approach.
    Attempt { angle: Angle, instruction: &'static str },
    /// It works. Explain what will be different — in behaviour, not in code —
    /// and wait.
    Propose { summary: String },
    /// Out of ideas here — take it to a conversation rather than stopping.
    HandOff(String),
    /// Stop, with a reason it shouldn't have started.
    Refuse(String),
}

impl Session {
    pub fn new(goal: &str, tests_before: usize) -> Session {
        Session {
            work: crate::pipeline::Work::new(goal),
            goal: goal.to_string(),
            campaign: Campaign::new(goal),
            tests_before,
            best: None,
            // The goal IS the symptom. Asking "what did you notice?"
            // immediately after someone has said "the settings panel is slow"
            // is a question they have just answered, and a flow that opens by
            // ignoring what you said is one you stop using. It also cost a
            // turn: five answers for four parts, with the first two identical.
            diagnosing: crate::pipeline::Diagnosing {
                symptom: Some(goal.to_string()),
                ..Default::default()
            },
        }
    }

    // ------------------------------------------------------------------
    // Collecting the diagnosis. The way in that did not exist.
    //
    // `may_start` refuses while `work.thought` is `None`, and nothing in the
    // tree ever set `work.thought`. So `WorkOnYourself` answered "no
    // diagnosis" every time and there was nowhere to put one. These three
    // methods are that nowhere.
    // ------------------------------------------------------------------

    /// What Atlas needs before it can start, or `None` when it has it all.
    ///
    /// An abandoned session needs nothing: it was dropped part-way, and one
    /// that kept asking its next question would be indistinguishable from one
    /// that was never dropped.
    pub fn still_needs(&self) -> Option<&'static str> {
        if self.work.thought.is_some()
            || self.work.stage == crate::pipeline::Stage::Abandoned
        {
            return None;
        }
        self.diagnosing.next_question()
    }

    /// Take one answer. Returns the next question, or `None` when the four
    /// parts are in and the proving test is what remains.
    pub fn heard(&mut self, text: &str) -> Option<&'static str> {
        if self.work.thought.is_some() {
            return None;
        }
        self.diagnosing.answer(text)
    }

    /// The test to run before any code is written.
    pub fn proving_test(&self) -> Option<&str> {
        self.diagnosing.proving_test()
    }

    /// Turn the collected answers into the diagnosis, given what running the
    /// proving test actually did.
    ///
    /// `proof_fails_now` is measured by the caller, which is the whole reason
    /// it is a parameter: a diagnosis that asserts its own proof fails is a
    /// diagnosis that has not been checked. `is_thought_through` refuses when
    /// it passed already, and a refusal here rewinds the cause so the session
    /// can be corrected rather than being stuck holding four unusable
    /// answers.
    pub fn accept_diagnosis(&mut self, proof_fails_now: bool) -> Result<(), String> {
        let Some(t) = self.diagnosing.into_thought(proof_fails_now, Vec::new()) else {
            return Err("I haven't got all four parts yet".into());
        };
        match self.work.record_thought(t) {
            Ok(()) => Ok(()),
            Err(why) => {
                // Only the cause is rewound. The symptom and the location are
                // facts about the system; the cause is the judgement, and it
                // is the one `is_thought_through` refuses.
                if why.contains("restatement") || why.contains("no cause") {
                    self.diagnosing.take_back_the_cause();
                }
                Err(why)
            }
        }
    }

    /// Nothing starts without a diagnosis that holds up.
    ///
    /// The check that stops this rotting: a cause that isn't the symptom said
    /// again, and a proving test that fails today.
    pub fn may_start(&self) -> Result<(), String> {
        match &self.work.thought {
            None => Err("no diagnosis — what's actually wrong, and what would prove it fixed?".into()),
            Some(t) => t.is_thought_through(),
        }
    }

    /// And nothing lands without a review.
    pub fn may_land(&self) -> Result<(), String> {
        self.work.may_land()
    }

    /// Out of ideas — take it somewhere else rather than giving up.
    ///
    /// This is the difference between a system that improves and one that
    /// stalls at the edge of what it already knew. Everything on the ladder
    /// has been tried; the write-up goes into a conversation and Atlas works
    /// through it until something testable comes back.
    ///
    /// It carries the diagnosis, not just the symptom — asking someone "this
    /// is broken, help" wastes the thinking that already happened.
    pub fn take_it_elsewhere(&self) -> Option<crate::consult::Consultation> {
        let t = self.work.thought.as_ref()?;
        Some(crate::consult::Consultation::new(&format!(
            "{}. I think {}, in {}. What should prove it fixed is {}. I've tried everything I know and it still doesn't.",
            t.symptom, t.cause, t.where_, t.proof
        )))
    }

    /// Take in a result and decide what happens next.
    pub fn after(&mut self, tried: Tried, learned: &str, cfg: &SelfWorkConfig, scfg: &StrategyConfig) -> Step {
        for e in &tried.edits {
            if let Verdict::Refused(why) = may_edit(&e.path, cfg) {
                return Step::Refuse(why);
            }
        }

        let acceptable = tried.worth_showing(self.tests_before, cfg);
        let angle = self
            .campaign
            .efforts
            .last()
            .map(|e| e.angle)
            .unwrap_or(Angle::ReadTheError);

        self.campaign.record(Effort {
            angle,
            learned: learned.to_string(),
            error: tried.problem.clone().unwrap_or_default(),
            solved: acceptable.is_ok(),
        });

        match acceptable {
            Ok(()) => {
                self.best = Some(tried.clone());
                Step::Propose { summary: self.summarise(&tried, cfg) }
            }
            Err(_) => match self.campaign.next(scfg) {
                Next::Try { angle, instruction } => Step::Attempt { angle, instruction },
                Next::Done => Step::Propose { summary: self.summarise(&tried, cfg) },
                Next::Exhausted(why) => Step::HandOff(why),
            },
        }
    }

    /// What Atlas says when it has something to show you.
    ///
    /// Areas rather than filenames, because "how it hears you" means something
    /// and `src/endpoint.rs` doesn't.
    fn summarise(&self, t: &Tried, cfg: &SelfWorkConfig) -> String {
        let mut areas: Vec<String> = t
            .edits
            .iter()
            .map(|e| crate::plainchange::area_of(&e.path).to_string())
            .collect();
        areas.sort();
        areas.dedup();
        let mut s = format!(
            "{} — changed {}. All {} tests pass.",
            self.goal,
            areas.join(" and "),
            t.tests_run
        );
        let _ = t.lines_changed;
        if t.is_large(cfg) {
            s.push_str(" That's a big change for one go — worth reading before you take it.");
        }
        s.push_str(&format!(" Took {} attempt{}.",
            self.campaign.efforts.len(),
            if self.campaign.efforts.len() == 1 { "" } else { "s" }));
        s
    }

    /// The first thing to try.
    pub fn begin(&self, scfg: &StrategyConfig) -> Step {
        match self.campaign.next(scfg) {
            Next::Try { angle, instruction } => Step::Attempt { angle, instruction },
            Next::Done => Step::HandOff("nothing to do".into()),
            Next::Exhausted(why) => Step::HandOff(why),
        }
    }
}

/// Which file(s) the diagnosis's "where" points at, with their current
/// contents, ready to hand to `draft_fix`.
///
/// The diagnosis names *where the cause lives* in words — sometimes a path
/// ("src/settings.rs"), sometimes a module ("settings"), sometimes prose ("the
/// settings panel"). This turns that into concrete files under `root`, reading
/// each one. It is deliberately conservative: a path that exists wins outright;
/// otherwise it tries `src/<word>.rs` for the words in the phrase. If it finds
/// nothing, the caller asks you which file rather than guessing — resolving the
/// wrong file is how a fix lands in the wrong place, which the review exists to
/// catch, so it is better not to start there.
pub fn files_named(where_: &str, root: &std::path::Path) -> Vec<Edit> {
    let mut out = Vec::new();
    let lower = where_.to_lowercase();

    // 1. An explicit path to a file that exists.
    for token in where_.split_whitespace() {
        let t = token.trim_matches(|c: char| !c.is_alphanumeric() && c != '/' && c != '.' && c != '_');
        if t.ends_with(".rs") && root.join(t).is_file() {
            if let Ok(content) = std::fs::read_to_string(root.join(t)) {
                out.push(Edit { path: t.to_string(), content, reason: String::new() });
            }
        }
    }
    if !out.is_empty() {
        return out;
    }

    // 2. A module name: src/<word>.rs.
    for word in lower.split(|c: char| !c.is_alphanumeric()) {
        if word.len() < 3 {
            continue;
        }
        let rel = format!("src/{word}.rs");
        if root.join(&rel).is_file() {
            if let Ok(content) = std::fs::read_to_string(root.join(&rel)) {
                out.push(Edit { path: rel, content, reason: String::new() });
            }
        }
        if out.len() >= 3 {
            break; // one fix should not span the whole tree.
        }
    }
    out
}

/// The instruction for writing a self-fix.
///
/// The point Atlas must not forget: the fix goes *where the cause is*, and it
/// does not touch the tests. Both are things the review checks afterwards
/// (`NotWhereTheCauseWas`, `TestWeakened`), so a draft that ignores them is
/// caught — but a draft that respects them from the start is the one that gets
/// through, and the model writes better when it knows what it's being held to.
pub const MEND_SYSTEM: &str = "\
You are fixing a fault in an existing program by editing one of its files. You \
are given the diagnosis -- the symptom, the underlying cause, where in the \
system the cause lives, and the test that must pass once it's fixed -- and the \
current contents of the file. Change the cause itself, in this file, not the \
symptom somewhere downstream. Do NOT weaken, delete, or edit any test to make \
things pass. Change as little as anything else. Return the COMPLETE corrected \
file in a single fenced code block -- the whole file, not a diff, and no \
explanation.";

/// Draft a candidate fix for the diagnosed cause: hand the model the file the
/// diagnosis points at, take back the whole corrected file. One call per file.
///
/// The model writes it; it is **never** the authority on whether it worked.
/// That is `run_the_proof` (the named test passes now) and `run_tests`
/// (nothing else broke), run against the change in a copy of the tree. This
/// only produces a candidate to put through them -- the same division of labour
/// as `build_it::build_loop`, where the model drafts and the compiler decides.
pub fn draft_fix(
    thought: &crate::pipeline::Thought,
    instruction: &str,
    current: &[Edit],
    llm: &dyn crate::brain::Llm,
) -> Result<Vec<Edit>, String> {
    if current.is_empty() {
        return Err("there's no file named to change".into());
    }
    let mut out = Vec::new();
    for file in current {
        let approach = if instruction.trim().is_empty() {
            String::new()
        } else {
            format!("Approach to try: {instruction}\n")
        };
        let user = format!(
            "The symptom: {}\nThe cause: {}\nWhere it lives: {}\nThe test that must pass after: {}\n{approach}\nThe current contents of {}:\n```\n{}\n```\n\nReturn the complete corrected file.",
            thought.symptom, thought.cause, thought.where_, thought.proof, file.path, file.content,
        );
        // Drafting a self-fix is the hard task this whole subsystem exists for:
        // escalate to the stronger model when one is configured.
        let reply = llm.complete_hard(MEND_SYSTEM, &user).map_err(|e| e.to_string())?;
        let fixed = crate::build_it::extract_code(&reply);
        if fixed.trim().is_empty() {
            return Err(format!("the model returned no code for {}", file.path));
        }
        if fixed.trim() == file.content.trim() {
            return Err(format!("the model returned {} unchanged -- no fix to try", file.path));
        }
        out.push(Edit { path: file.path.clone(), content: fixed, reason: thought.cause.clone() });
    }
    Ok(out)
}

/// How many lines differ between the files as they are and a candidate — the
/// figure `Tried.lines_changed` carries, and what the review's scope check and
/// the "big change" note both read.
pub fn lines_touched(before: &[Edit], after: &[Edit]) -> usize {
    // Lines added plus lines removed by the shortest edit script (`diff`,
    // Myers), as `git diff --stat` counts them. This used to count lines "in
    // one and not the other" as sets, which reads a file of `}` and blank
    // lines as barely touched however much of it moved.
    after
        .iter()
        .map(|a| {
            let was = before.iter().find(|b| b.path == a.path).map(|b| b.content.as_str()).unwrap_or("");
            crate::diff::lines_changed(was, &a.content)
        })
        .sum()
}

/// What running the named proving test says about the tree as it is now.
///
/// Four outcomes rather than a bool, because the interesting ones are not
/// "fails" and "passes".
#[derive(Debug, Clone, PartialEq)]
pub enum ProofToday {
    /// It fails now. The one outcome that lets work start.
    Fails,
    /// It passes already, so it is testing something else. The single most
    /// valuable check in the loop, and the one a person answering a question
    /// would get wrong in good faith.
    PassesAlready,
    /// The filter matched nothing — the test has not been written yet.
    ///
    /// Counted as failing, and said out loud rather than folded in: a named
    /// test that does not exist is not currently satisfied, and writing it is
    /// the first half of the Build stage. What must not happen is this being
    /// reported as evidence.
    NotWrittenYet,
    /// It could not be run at all, so nothing is known either way.
    CouldNotRun(String),
}

/// How long the proving test may take before Atlas stops waiting.
///
/// The same reasoning as `workspace::BRINGUP_BUDGET_SECS`, and for the same
/// thread: this runs on the tick, which is what listens, answers, polls, and
/// refreshes the instance lock. The staleness window is 150s, so an unbounded
/// `cargo test` here would let a second Atlas read the lock as abandoned and
/// take it.
pub const PROOF_BUDGET_SECS: u64 = 120;

/// Run the proving test against the project as it stands.
///
/// Against the real tree rather than a sandbox on purpose: the question is
/// whether the proof fails **today**, and a sandbox is a copy of today with
/// something already changed in it.
///
/// The heartbeat is refreshed while waiting, for the reason above.
pub fn run_the_proof(
    test_name: &str,
    cfg: &SelfWorkConfig,
    root: &std::path::Path,
) -> ProofToday {
    let filter = test_name.trim();
    if filter.is_empty() {
        return ProofToday::CouldNotRun("no test was named".into());
    }
    // The filter is passed as a single argument to the test binary, never
    // through a shell, so it cannot become a second command.
    let mut parts = cfg.test_command.split_whitespace();
    let program = parts.next().unwrap_or("cargo");
    let mut cmd = crate::tools::command(program);
    cmd.args(parts).arg(filter).current_dir(root);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());

    let mut child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ProofToday::CouldNotRun(format!("could not start {program}: {e}")),
    };

    let deadline = std::time::Instant::now()
        + std::time::Duration::from_secs(PROOF_BUDGET_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Err(e) => return ProofToday::CouldNotRun(format!("lost track of the test run: {e}")),
            Ok(None) => {}
        }
        if crate::goodbye::asked_to_stop() {
            let _ = child.kill();
            crate::unwaited::dont_wait(child);
            return ProofToday::CouldNotRun("you asked me to stop while it was running".into());
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            // Handed to `unwaited` rather than waited on here: a killed cargo
            // can take a moment to go, and blocking on it is the thing the
            // budget exists to avoid.
            crate::unwaited::dont_wait(child);
            return ProofToday::CouldNotRun(format!(
                "it was still running after {PROOF_BUDGET_SECS} seconds, so I stopped it"
            ));
        }
        let _ = crate::onlyone::OnlyOne::at(&crate::roots::data_dir()).beat(crate::store::now());
        crate::goodbye::nap(200);
    }

    let out = match child.wait_with_output() {
        Ok(o) => o,
        Err(e) => return ProofToday::CouldNotRun(format!("could not read the test run: {e}")),
    };
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    read_a_proof_run(&text)
}

/// Read a test run's output. Separated from running it so the reading is
/// testable without a compiler.
pub fn read_a_proof_run(text: &str) -> ProofToday {
    // A tree that does not build says nothing about the proof.
    if text.contains("error: could not compile")
        || text.contains("error[E")
        || text.contains("error: no test target")
    {
        return ProofToday::CouldNotRun(
            "the tree doesn't build, so the test couldn't tell me anything".into(),
        );
    }
    // "test result: ok. 5 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out"
    //
    // Read by name rather than by position: a number followed by the word
    // that says what it counts. Position broke the moment the verdict word
    // ("ok." / "FAILED.") sat between the colon and the first figure.
    let mut passed = 0usize;
    let mut failed = 0usize;
    for line in text.lines() {
        let Some(rest) = line.trim().strip_prefix("test result:") else { continue };
        let words: Vec<&str> = rest.split_whitespace().collect();
        for pair in words.windows(2) {
            let Ok(n) = pair[0].trim_end_matches([';', '.']).parse::<usize>() else { continue };
            match pair[1].trim_end_matches([';', '.']) {
                "passed" => passed += n,
                "failed" => failed += n,
                _ => {}
            }
        }
    }
    let ran = passed + failed;
    if failed > 0 {
        return ProofToday::Fails;
    }
    if ran == 0 {
        return ProofToday::NotWrittenYet;
    }
    ProofToday::PassesAlready
}

/// Run the test suite in the sandbox and read the result.
pub fn run_tests(sandbox: &mut Sandbox, cfg: &SelfWorkConfig) -> Tried {
    let tool = crate::tools::ExternalTool {
        command: cfg.test_command.split_whitespace().next().unwrap_or("cargo").into(),
        args: cfg.test_command.split_whitespace().skip(1).map(String::from).collect(),
        ..Default::default()
    };
    let attempt = sandbox.run(&tool, &Default::default(), 20_000);
    from_attempt(&attempt)
}

/// Read a test run.
fn from_attempt(a: &Attempt) -> Tried {
    Tried {
        edits: Vec::new(),
        passed: a.passed,
        tests_run: count_passing(&a.output),
        problem: a.first_problem(),
        lines_changed: 0,
    }
}

/// How many tests actually ran.
///
/// The number matters: it's what catches a change that passes by removing the
/// test that was failing.
pub fn count_passing(output: &str) -> usize {
    output
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let rest = l.strip_prefix("test result: ok.")?;
            rest.split_whitespace().next()?.parse::<usize>().ok()
        })
        .sum()
}

// ---------------------------------------------------------------------------
// Landing: the half where a fix that passed actually reaches the tree.
//
// `sandbox::plan` builds the list of `Change`s and calls itself "the preview".
// It was the whole story: nothing ever applied one. `pipeline::Next::Land`
// returned a sentence describing what would change, the daemon printed the
// sentence, and the sandbox copy sat where it was.
//
// So the rule "Atlas fixes faulty code in a sandbox, and if it passes it gets
// fixed then and there" had every piece except the last four words.
//
// The constraint that comes with it -- *a fix that does not change or limit
// what Atlas can do* -- is what this gate is. Four checks, and each one exists
// because the cheap way past it is the tempting one:
//
// 1. **Every path is one Atlas is allowed to edit.** `may_edit` already knew
//    which, including that it may never edit the file that decides what it is
//    allowed to edit.
// 2. **Nothing lands over an edit you made meanwhile.** `Change::target_was`
//    was written for exactly this and its own doc says why: overnight work
//    makes the gap hours wide, and a whole-file copy silently wins against
//    anything you changed in the evening.
// 3. **No paper-overs.** `mend::paper_overs` names the six shapes that make a
//    check pass while leaving the fault in place. A deleted test, a silenced
//    warning or a widened type *does* limit what Atlas can do -- it removes
//    the thing that would have noticed. This is that constraint, mechanically.
// 4. **The review was clean.** `pipeline::review` blocks on the fix being
//    somewhere other than where the cause was, and on tests being weakened.
//
// 3 and 4 are complements rather than duplicates: the review counts tests and
// checks location, `paper_overs` reads the shapes in the text. `#[ignore]`
// leaves the count unchanged, and a review that only counts would pass it.
// ---------------------------------------------------------------------------

/// Why a change that passed its tests still did not land.
#[derive(Debug, Clone, PartialEq)]
pub enum Held {
    /// A path Atlas is not allowed to edit.
    NotMine(String),
    /// The file changed under it since the plan was made.
    YouChangedIt(String),
    /// The change makes the check pass without fixing anything.
    PapersOver(String),
    /// The review found something that blocks.
    ReviewSaysNo(String),
    /// You have not said Atlas may change something this far-reaching.
    ///
    /// The gap this closes: `selfgrant::may_land` is the function that knows
    /// what a standing grant covers, and **it had no production caller**.
    /// `may_edit` is binary — allowed or refused — so a change to
    /// `src/browser.rs` (`Reach::WhatItTouches`) landed exactly as readily as
    /// one to `src/persona.rs` (`Reach::WhatItSays`), and the shipped
    /// `tools.yaml` says `may_change: nothing`.
    NotGranted(String),
    /// Never, whatever is granted.
    NeverMine(String),
}

impl Held {
    /// What Atlas says instead of landing it.
    pub fn plain(&self) -> String {
        match self {
            Held::NotMine(p) => format!("I'm not allowed to edit {p}, so I've left it."),
            Held::YouChangedIt(p) => format!(
                "You changed {p} while I was working on it. I'm not landing on top of that — \
                 the change is still in the sandbox."
            ),
            Held::PapersOver(why) => format!(
                "It passes, but {why}. That makes the check go green and leaves the fault \
                 where it was, so I haven't landed it."
            ),
            Held::ReviewSaysNo(why) => format!("It passes, but {why}. I haven't landed it."),
            Held::NotGranted(why) => format!(
                "It passes and it's ready. {why} — so it's yours rather than mine. Say go \
                 ahead and I'll put it in."
            ),
            Held::NeverMine(why) => format!("I'm not landing that one: {why}."),
        }
    }
}

/// Everything standing between a passing change and the tree.
///
/// Every check runs, rather than stopping at the first: a change with three
/// problems reported one at a time takes three rounds to learn about, and the
/// second and third are usually the informative ones.
pub fn what_holds_it_back(
    changes: &[crate::sandbox::Change],
    review: Option<&crate::pipeline::Review>,
    cfg: &SelfWorkConfig,
    grant: &crate::selfgrant::SelfGrantConfig,
    root: &std::path::Path,
) -> Vec<Held> {
    let mut held = Vec::new();
    // The project-relative paths, kept for the grant check below. `may_edit`
    // answers "may Atlas ever edit this file"; the grant answers "has Eric
    // said Atlas may do it without asking", and those are different
    // questions about the same change.
    let mut rel_paths: Vec<String> = Vec::new();
    let any_new_file = changes.iter().any(|c| c.new_file);

    for c in changes {
        // Relative to the project, because that is what `may_edit` reads:
        // its `may_touch` is `["src/", "tests/", ...]` and an absolute path
        // starts with none of them. The first version of this passed
        // `c.target` straight in, and `sandbox::plan` produces absolute
        // targets — so every change, including a real fix, was refused as
        // "not in the parts of the project I work on". Found by a test that
        // expected a good change to land.
        //
        // A target outside the project entirely has no relative form, and is
        // refused for that reason rather than by the allowlist.
        let path = match c.target.strip_prefix(root) {
            Ok(rel) => rel.to_string_lossy().to_string(),
            Err(_) => {
                held.push(Held::NotMine(format!(
                    "{} is outside the project",
                    c.target.display()
                )));
                continue;
            }
        };
        if let Verdict::Refused(why) = may_edit(&path, cfg) {
            held.push(Held::NotMine(why));
        }
        rel_paths.push(path.clone());
        // A file that existed when the plan was made and has moved since.
        if !c.new_file {
            let now = crate::sandbox::Fingerprint::of(&c.target);
            match (c.target_was, now) {
                (Some(then), Some(now)) if then != now => {
                    held.push(Held::YouChangedIt(path.clone()))
                }
                // It was there when planned and is gone now. Landing would
                // recreate a file you deleted, which is not a fix.
                (Some(_), None) => held.push(Held::YouChangedIt(path.clone())),
                _ => {}
            }
        }

        // The shapes that make a check pass without fixing anything. Read from
        // what would land, against what is there now.
        let Ok(after) = std::fs::read_to_string(&c.source) else { continue };
        let before = std::fs::read_to_string(&c.target).unwrap_or_default();
        let proposed = crate::mend::Proposed {
            theory: String::new(),
            added: added_lines(&before, &after),
            removed: added_lines(&after, &before),
        };
        for cheat in crate::mend::paper_overs(&proposed) {
            held.push(Held::PapersOver(cheat.why_not().to_string()));
        }
    }

    if let Some(r) = review {
        for note in r.blockers() {
            held.push(Held::ReviewSaysNo(note.what.clone()));
        }
    }

    // How far the change reaches, against how far you have said Atlas may go.
    //
    // `tests_passed` and `reversible` are both `true` here and both are
    // earned rather than assumed: this function is only called from
    // `Daemon::land_it` by way of `Next::Land`, which `what_next` returns
    // only at `Stage::Implement`, which `record_review` reaches only from a
    // clean review of a build whose proving test passed and which broke
    // nothing else. The review's own blockers are checked again above, so a
    // stale review cannot slip past by being `Some`.
    //
    // `reversible` is `true` because `land` copies the previous version of
    // every file it overwrites into `landed-over/` first.
    if !rel_paths.is_empty() {
        // Grants stand until you take them back (Eric, B3).
        let granted = grant.granted().map(|up_to| crate::selfgrant::Granted { up_to, at: 0 });
        match crate::selfgrant::may_land(
            &rel_paths,
            true,
            true,
            granted.as_ref(),
            any_new_file,
            grant,
        ) {
            crate::selfgrant::Verdict::GoAhead { .. } => {}
            crate::selfgrant::Verdict::AskFirst(why) => held.push(Held::NotGranted(why)),
            crate::selfgrant::Verdict::Never(why) => held.push(Held::NeverMine(why)),
        }
    }
    held
}

/// Lines in `after` that are not in `before`.
///
/// A line-set difference rather than a real diff: `paper_overs` matches
/// substrings against single lines, so it needs the lines that arrived and the
/// lines that left, and neither of those needs to know where they moved to.
fn added_lines(before: &str, after: &str) -> Vec<String> {
    let was: std::collections::BTreeSet<&str> = before.lines().map(str::trim).collect();
    after
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !was.contains(l))
        .map(str::to_string)
        .collect()
}

/// Put the changes on the machine.
///
/// Only ever called with an empty `what_holds_it_back`. The old version of
/// each file goes to `keep` first -- a fix Atlas landed on its own has to be
/// something you can put back without asking it.
pub fn land(changes: &[crate::sandbox::Change], keep: &std::path::Path) -> crate::error::Result<usize> {
    std::fs::create_dir_all(keep)?;

    // Phase 1 — back up every existing file we're about to overwrite, *before*
    // touching anything. The old bytes are held in memory as well as written to
    // `keep`, so a rollback restores exactly what was there even if two changes
    // share a basename. The `.before` file is no longer best-effort (23 Sep):
    // it is how *you* put a landed fix back later, so a failure to write it —
    // a full disk — stops the landing before anything is touched. A fix Atlas landed on its own has to be something you
    // can put back, and that guarantee cannot depend on the apply half working.
    let mut old_bytes: Vec<(std::path::PathBuf, Vec<u8>)> = Vec::new();
    for c in changes {
        if !c.new_file {
            if let Ok(old) = std::fs::read(&c.target) {
                let name = c
                    .target
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "file".into());
                // The kept copy is the promise that a landed fix can be put
                // back. If it can't be written (a full disk), nothing lands.
                std::fs::write(keep.join(format!("{name}.before")), &old)?;
                old_bytes.push((c.target.clone(), old));
            }
        }
    }

    // Phase 2 — apply. If any file fails to write, undo everything already
    // applied this call and stop: restore the files we overwrote from the bytes
    // held above, and delete the new files we created. A half-landed change can
    // never be left on the machine, which is what makes an unattended land safe.
    let mut applied: Vec<&crate::sandbox::Change> = Vec::new();
    let fail = |applied: &[&crate::sandbox::Change], e: std::io::Error| -> crate::error::Result<usize> {
        for c in applied {
            if c.new_file {
                let _ = std::fs::remove_file(&c.target);
            } else if let Some((_, old)) = old_bytes.iter().find(|(t, _)| t == &c.target) {
                let _ = std::fs::write(&c.target, old);
            }
        }
        Err(e.into())
    };
    for c in changes {
        if let Some(parent) = c.target.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return fail(&applied, e);
            }
        }
        if let Err(e) = std::fs::copy(&c.source, &c.target) {
            return fail(&applied, e);
        }
        applied.push(c);
    }
    Ok(applied.len())
}

// ---------------------------------------------------------------------------
// Integrated verification: proving a change *inside* a real project.
//
// `prove_in_a_copy` above does this for Atlas's own tree. This is the same
// idea generalised to any project on disk, and it is the difference between
// two claims that are easy to confuse and must never be:
//
//   isolated  — "this file compiles on its own, as its own little unit"
//   integrated — "the project still builds and its own tests still pass with
//                 this change in it"
//
// A weak model's draft can pass the first and break the second, and reporting
// the first as though it were the second is the overclaim this whole corner of
// the tree exists to prevent. For a system someone's livelihood runs on, that
// distinction is the whole point.
// ---------------------------------------------------------------------------

/// What proving a set of edits against a real project actually established.
#[derive(Debug, Clone, PartialEq)]
pub struct ProjectProof {
    /// The project still builds and every one of its own tests passed with the
    /// change applied. `false` covers both "didn't build" and "a test failed".
    pub built_and_passed: bool,
    /// How many of the project's tests ran — the guard against a green run that
    /// is green because nothing ran.
    pub tests_run: usize,
    /// The toolchain's own words, for the caller to show or fix from.
    pub output: String,
    /// True when at least one edit replaced a file the project already had.
    ///
    /// Load-bearing for honesty: a green suite only *exercises* a change that
    /// replaced code the project already compiled and tested. A brand-new,
    /// unreferenced file can leave the suite green while proving nothing about
    /// the new code, so the caller must not call that "verified in the
    /// project" — see `Verdict::plain`.
    pub replaced_existing: bool,
}

impl ProjectProof {
    /// The honest one-liner for what this proof does and does not establish.
    pub fn plain(&self, project: &str) -> String {
        if self.built_and_passed && self.replaced_existing {
            format!(
                "Verified inside {project}: it still builds and all {} of its own tests pass with \
                 the change in it.",
                self.tests_run
            )
        } else if self.built_and_passed {
            format!(
                "{project} still builds and its {} tests pass with this alongside — but the change \
                 is a new file nothing references yet, so its tests don't exercise it. Read it \
                 rather than trust it.",
                self.tests_run
            )
        } else {
            format!("It doesn't hold up inside {project}: {}", first_line_of(&self.output))
        }
    }
}

fn first_line_of(s: &str) -> String {
    s.lines().map(str::trim).find(|l| !l.is_empty()).unwrap_or("no output").to_string()
}

/// How long a project's own suite may run before Atlas stops waiting. Larger
/// than the single-test budget because this is the whole suite of somebody
/// else's project, cold.
pub const PROJECT_PROOF_BUDGET_SECS: u64 = 300;

/// Prove edits *inside a copy of a real project*: apply them, run the project's
/// own test command, and report whether it still builds and its tests pass.
///
/// The project itself is never touched — everything happens in a throwaway
/// copy. `test_command` is the project's own (e.g. `cargo test`, `go test
/// ./...`, `pytest -q`), split on whitespace and run as a program plus
/// arguments, never through a shell, so it cannot become a second command.
/// `edits` carry project-relative paths and whole-file contents, the same
/// shape `draft_fix` produces.
pub fn prove_in_project(
    root: &std::path::Path,
    test_command: &str,
    edits: &[Edit],
    base: &std::path::Path,
) -> Result<ProjectProof, String> {
    if !root.is_dir() {
        return Err(format!("{} isn't a folder I can reach", root.display()));
    }
    let cmd = test_command.trim();
    if cmd.is_empty() {
        return Err("no test command is set for that project".into());
    }

    // Did any edit replace a file the project already had? Decided against the
    // real project, before the copy, because the copy is about to contain them
    // all either way.
    let replaced_existing = edits.iter().any(|e| root.join(&e.path).is_file());

    // A throwaway copy, skipping the things that must not be copied: build
    // output (huge, and a stale one would poison the run), version control,
    // and dependency caches.
    let dst = base.join(format!("projfix-{}", crate::store::now()));
    let _ = std::fs::remove_dir_all(&dst);
    copy_project(root, &dst).map_err(|e| format!("couldn't copy the project to work in: {e}"))?;

    // Apply the edits in the copy.
    for e in edits {
        let target = dst.join(&e.path);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|err| format!("couldn't make {}: {err}", parent.display()))?;
        }
        std::fs::write(&target, &e.content)
            .map_err(|err| format!("couldn't write {} into the copy: {err}", e.path))?;
    }

    // Run the project's own suite in the copy, bounded.
    let output = run_bounded(cmd, &dst, PROJECT_PROOF_BUDGET_SECS)?;
    let _ = std::fs::remove_dir_all(&dst);

    let read = read_a_proof_run(&output);
    let built_and_passed = read == ProofToday::PassesAlready;
    let tests_run = count_passing(&output);
    Ok(ProjectProof { built_and_passed, tests_run, output, replaced_existing })
}

/// Copy a project tree, skipping build output, version control and dependency
/// caches — the things that are huge, machine-specific, or would poison a fresh
/// build. A cold build in the copy is the price of not touching the original.
fn copy_project(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    const SKIP: &[&str] =
        &["target", ".git", "node_modules", "__pycache__", ".venv", "venv", "dist", "build"];
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)?.flatten() {
        let name = entry.file_name();
        if SKIP.iter().any(|s| *s == name) {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_project(&src, &dst)?;
        } else if src.is_file() {
            std::fs::copy(&src, &dst)?;
        }
    }
    Ok(())
}

/// Run one command in a directory with a wall-clock budget, returning its
/// combined output. Killed and reported if it runs long, rather than letting a
/// stuck build hang the caller.
fn run_bounded(
    command: &str,
    dir: &std::path::Path,
    budget_secs: u64,
) -> Result<String, String> {
    let mut parts = command.split_whitespace();
    let program = parts.next().unwrap_or("cargo");
    let mut cmd = crate::tools::command(program);
    cmd.args(parts).current_dir(dir);
    cmd.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| format!("could not start {program}: {e}"))?;

    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(budget_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Err(e) => return Err(format!("lost track of the run: {e}")),
            Ok(None) => {}
        }
        if crate::goodbye::asked_to_stop() {
            let _ = child.kill();
            crate::unwaited::dont_wait(child);
            return Err("you asked me to stop while it was running".into());
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            crate::unwaited::dont_wait(child);
            return Err(format!("it was still running after {budget_secs}s, so I stopped it"));
        }
        crate::goodbye::nap(200);
    }
    let out = child.wait_with_output().map_err(|e| format!("could not read the run: {e}"))?;
    let mut text = String::from_utf8_lossy(&out.stdout).to_string();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    Ok(text)
}
