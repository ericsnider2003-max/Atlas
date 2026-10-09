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
    /// Where Atlas's own source is on this computer. Empty: looked for
    /// (`source_root`), on whoever's computer this is.
    pub source_dir: String,
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
            source_dir: String::new(),
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
    /// A session for one of Atlas's own ideas (the Improvements page's "Have
    /// a go"), with its diagnosis already given.
    ///
    /// `new` holds the symptom already, so the first answer is the cause.
    /// 29 Sep 2026: the hub handed it the symptom again first, which made the
    /// symptom the cause, and every idea was refused as "the cause is a
    /// restatement of the symptom".
    pub fn from_recommendation(r: &crate::selfaudit::Recommendation, tests_before: usize) -> Session {
        let thought = crate::selfaudit::as_thought(r);
        let mut session = Session::new(&thought.symptom, tests_before);
        for answer in [&thought.cause, &thought.where_, &thought.proof] {
            // unheard-ok: returns `Option<&str>`, not a Result
            let _ = session.diagnosing.answer(answer);
        }
        session
    }

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
/// Small local repair recipes. Each structured edit is only a candidate:
/// the existing, unchanged proving test and full suite decide its meaning.
pub(crate) fn recipe_candidates(current: &[Edit]) -> Vec<(String, Vec<Edit>)> {
    let mut candidates = Vec::new();
    for file in current.iter().take(16) {
        if file.content.len() > 512 * 1024 { continue; }
        if file.path.ends_with(".rs") {
            for (start, _) in file.content.match_indices(".take(").take(8) {
                let rest = &file.content[start + 6..];
                let Some(end) = rest.find(".len() - 1)") else { continue };
                let name = &rest[..end];
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { continue; }
                if let Ok(content) = apply_edit_blocks(&file.content, &[(format!(".take({name}.len() - 1)"), format!(".take({name}.len())"))]) {
                    candidates.push(("Include the omitted iterator endpoint".into(), vec![Edit { path: file.path.clone(), content, reason: "Local recipe: test whether the omitted iterator endpoint caused the failure.".into() }]));
                    if candidates.len() >= 8 { return candidates; }
                }
            }
        }
        if !file.path.ends_with(".py") { continue; }
        for suffix in ["[:-1]", "[1:]"] {
            for (start, _) in file.content.match_indices("sum(").take(8) {
                let rest = &file.content[start + 4..];
                let Some(end) = rest.find(&format!("{suffix})")) else { continue };
                let name = &rest[..end];
                if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') { continue; }
                let before = format!("sum({name}{suffix})");
                let after = format!("sum({name})");
                let blocks = vec![(before, after)];
                if let Ok(content) = apply_edit_blocks(&file.content, &blocks) {
                    let edit = Edit { path: file.path.clone(), content, reason: "Local recipe: check whether the omitted endpoint caused the failing total.".into() };
                    candidates.push(("Include the omitted endpoint in a sum".into(), vec![edit]));
                    if candidates.len() >= 8 { return candidates; }
                }
            }
        }
    }
    candidates
}

/// Bounded repair calls never fall back to a synchronous model adapter.
pub(crate) struct BoundedModel<'a, 'b> { pub model: &'a dyn crate::brain::Llm, pub budget: &'a crate::tools::WorkBudget<'b> }
impl crate::brain::Llm for BoundedModel<'_, '_> {
    fn complete(&self, system: &str, user: &str) -> crate::error::Result<String> {
        self.budget.check().map_err(crate::error::AtlasError::Platform)?;
        if !self.model.supports_bounded_chat() { return Err(crate::error::AtlasError::Platform("this model connection cannot guarantee bounded cancellation for repair".into())); }
        let req = crate::brain::ChatRequest { messages: vec![crate::brain::Msg::system(system), crate::brain::Msg::user(user)], tools: Vec::new(), max_tokens: 4096, force_tool: false, stable_tools: 0, aside: true, stronger: false, output_schema: None };
        let mut bytes = 0usize;
        let mut over = false;
        let mut on_text = |text: &str| { bytes = bytes.saturating_add(text.len()); over |= bytes > 2 * 1024 * 1024; !over && !self.budget.stopping() };
        let reply = self.model.chat_until(&req, &mut on_text, &|| !self.budget.stopping())?;
        self.budget.check().map_err(crate::error::AtlasError::Platform)?;
        if over || reply.text.len() > 2 * 1024 * 1024 { return Err(crate::error::AtlasError::Platform("the repair draft exceeded its safe response budget".into())); }
        Ok(reply.text)
    }
}

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
        let diagnosis = format!(
            "The symptom: {}\nThe cause: {}\nWhere it lives: {}\nThe test that must pass after: {}\n{approach}",
            thought.symptom, thought.cause, thought.where_, thought.proof,
        );
        // A small file comes back whole. A big one can't: the model on this
        // computer (a 7B coder, a few thousand tokens of room) was asked for
        // the whole of a 2,000-line Rust file and could not give it back, so
        // no self-fix of Atlas was ever drafted (5 Oct 2026). It gets the
        // part of the file the diagnosis points at, and returns edits.
        let lines = file.content.lines().count();
        let fixed = if lines <= WHOLE_FILE_LINES {
            let user = format!(
                "{diagnosis}\nThe current contents of {}:\n```\n{}\n```\n\nReturn the complete corrected file.",
                file.path, file.content,
            );
            // Drafting a self-fix is the hard task this whole subsystem exists
            // for: escalate to the stronger model when one is configured.
            let reply = llm.complete_hard(MEND_SYSTEM, &user).map_err(|e| e.to_string())?;
            crate::build_it::extract_code(&reply)
        } else {
            let (from, excerpt) = excerpt_for(&file.content, &thought.where_, &thought.cause);
            let user = format!(
                "{diagnosis}\nPart of {} (from line {}, of {lines}):\n```\n{excerpt}\n```\n\nReturn your change as edit blocks.",
                file.path,
                from + 1,
            );
            let reply = llm.complete_hard(MEND_BY_EDITS_SYSTEM, &user).map_err(|e| e.to_string())?;
            let blocks = edit_blocks(&reply);
            if blocks.is_empty() {
                return Err(format!("the model returned no edits for {}", file.path));
            }
            apply_edit_blocks(&file.content, &blocks).map_err(|why| format!("{} in {}", why, file.path))?
        };
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

/// Files up to this many lines go to the model whole and come back whole;
/// past it, a part of the file goes and edits come back (`draft_fix`).
pub const WHOLE_FILE_LINES: usize = 250;

/// How much of a big file the model is shown around where the diagnosis
/// points: a few thousand tokens, what a 7B coder on a laptop has room for.
pub const EXCERPT_LINES: usize = 220;

/// For a file too big to send whole.
pub const MEND_BY_EDITS_SYSTEM: &str = "\
You are fixing a fault in an existing program by editing one of its files. You \
are given the diagnosis -- the symptom, the underlying cause, where it lives, \
and the test that must pass once it's fixed -- and the part of the file where \
the cause is. Change the cause itself, not the symptom downstream. Do NOT \
weaken, delete, or edit any test to make things pass. Change as little as you \
can. Reply ONLY with one or more edit blocks, exactly like this:\n\
<<<<<<< SEARCH\n\
lines copied exactly from the file, enough to be unique\n\
=======\n\
the lines to put in their place\n\
>>>>>>> REPLACE\n\
No explanation, no other text.";

/// The part of a big file the diagnosis points at: the first line naming
/// something the diagnosis names (a function, a type, a constant), with
/// room either side. The start of the file when nothing matches. Returns
/// the 0-based first line and the text.
pub fn excerpt_for(content: &str, where_: &str, cause: &str) -> (usize, String) {
    let lines: Vec<&str> = content.lines().collect();
    let names: Vec<String> = format!("{where_} {cause}")
        .split(|c: char| !(c.is_alphanumeric() || c == '_'))
        .filter(|w| w.len() >= 4 && (w.contains('_') || w.chars().any(|c| c.is_uppercase()) && w.chars().any(|c| c.is_lowercase())))
        .map(str::to_string)
        .collect();
    let hit = lines.iter().position(|l| {
        let t = l.trim_start();
        (t.starts_with("fn ") || t.starts_with("pub ") || t.starts_with("const ") || t.starts_with("struct ") || t.starts_with("enum ") || t.starts_with("impl"))
            && names.iter().any(|n| l.contains(n.as_str()))
    })
    .or_else(|| lines.iter().position(|l| names.iter().any(|n| l.contains(n.as_str()))));
    let from = hit.map(|h| h.saturating_sub(EXCERPT_LINES / 4)).unwrap_or(0);
    let to = (from + EXCERPT_LINES).min(lines.len());
    (from, lines[from..to].join("\n"))
}

/// The SEARCH/REPLACE blocks in a reply, in order.
pub fn edit_blocks(reply: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = reply;
    while let Some(a) = rest.find("<<<<<<< SEARCH") {
        let after = &rest[a + "<<<<<<< SEARCH".len()..];
        let Some(mid) = after.find("\n=======") else { break };
        let search = after[..mid].trim_start_matches(['\r', '\n']).to_string();
        let after_mid = &after[mid + "\n=======".len()..];
        let Some(end) = after_mid.find(">>>>>>> REPLACE") else { break };
        let replace = after_mid[..end].trim_start_matches(['\r', '\n']).trim_end_matches([' ', '\t']).to_string();
        let replace = replace.strip_suffix('\n').map(str::to_string).unwrap_or(replace);
        let replace = replace.strip_suffix('\r').map(str::to_string).unwrap_or(replace);
        out.push((search.trim_end_matches(['\r', '\n']).to_string(), replace));
        rest = &after_mid[end + ">>>>>>> REPLACE".len()..];
    }
    out
}

/// Apply edit blocks to a file. Each SEARCH must match exactly one place --
/// exactly, or line by line ignoring indentation and trailing space (what a
/// small model gets wrong most). Nothing is guessed: a block that matches
/// nowhere, or in two places, fails the whole draft and says which.
pub fn apply_edit_blocks(content: &str, blocks: &[(String, String)]) -> Result<String, String> {
    let mut text = content.to_string();
    for (i, (search, replace)) in blocks.iter().enumerate() {
        if search.trim().is_empty() {
            return Err(format!("edit {} has nothing to search for", i + 1));
        }
        let exact = text.matches(search.as_str()).count();
        if exact == 1 {
            text = text.replacen(search.as_str(), replace, 1);
            continue;
        }
        if exact > 1 {
            return Err(format!("edit {} matches {exact} places", i + 1));
        }
        // Line by line, ignoring leading and trailing whitespace.
        let want: Vec<&str> = search.lines().map(str::trim).collect();
        let have: Vec<&str> = text.lines().collect();
        let starts: Vec<usize> = (0..=have.len().saturating_sub(want.len()))
            .filter(|&s| want.iter().enumerate().all(|(k, w)| have.get(s + k).is_some_and(|h| h.trim() == *w)))
            .collect();
        match starts.len() {
            0 => return Err(format!("edit {} doesn't match the file", i + 1)),
            1 => {
                let s = starts[0];
                let mut lines: Vec<String> = have[..s].iter().map(|l| l.to_string()).collect();
                lines.extend(replace.lines().map(str::to_string));
                lines.extend(have[s + want.len()..].iter().map(|l| l.to_string()));
                let mut joined = lines.join("\n");
                if text.ends_with('\n') {
                    joined.push('\n');
                }
                text = joined;
            }
            n => return Err(format!("edit {} matches {n} places", i + 1)),
        }
    }
    Ok(text)
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
/// It was two minutes, for a reason that stopped being true on 1 Oct 2026:
/// this ran on the tick, which refreshes the instance lock. Both callers now
/// run it on the crew (the proof check and `prove_in_a_copy`), and the tick
/// keeps the lock fresh by itself. The two minutes stayed, and on Eric's
/// laptop a cold build of the tree takes twenty to forty, so every proof
/// came back "it was still running after 120 seconds, so I stopped it" and
/// no self-repair ever got past it (5 Oct 2026). Long enough for a cold
/// build; with the shared cache (`roots::build_cache`) only the first is.
pub const PROOF_BUDGET_SECS: u64 = 40 * 60;

/// How long the whole suite may take in a sandbox copy (`run_tests`).
pub const SUITE_LIMIT_SECS: u64 = 45 * 60;

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
    run_the_proof_controlled(test_name, cfg, root, None)
}

pub(crate) fn run_the_proof_controlled(test_name: &str, cfg: &SelfWorkConfig, root: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>) -> ProofToday {
    let filter = test_name.trim();
    if filter.is_empty() { return ProofToday::CouldNotRun("no test was named".into()); }
    if let Some(budget) = budget { if let Err(why) = budget.check() { return ProofToday::CouldNotRun(why); } }
    let mut parts = cfg.test_command.split_whitespace();
    let program = parts.next().unwrap_or("cargo");
    let mut cmd = crate::tools::command(program);
    cmd.args(parts).arg(filter).current_dir(root);
    crate::sandbox::warm_cargo(&mut cmd, program, &[]);
    let cap = std::time::Duration::from_secs(PROOF_BUDGET_SECS);
    let limit = budget.map_or(cap, |b| b.remaining(cap));
    let stopping = || budget.is_some_and(|b| b.stopping());
    let run = crate::tools::run_scoped(&mut cmd, limit, 8 * 1024 * 1024, None, Some(&stopping));
    if let Some(b) = budget { if let Err(why) = b.check() { return ProofToday::CouldNotRun(why); } }
    let succeeded = matches!(run.end, crate::tools::ProcessEnd::Exited(status) if status.success());
    let exited = matches!(run.end, crate::tools::ProcessEnd::Exited(_));
    let complete = !run.truncated;
    let (_, output) = run.said(8 * 1024 * 1024);
    if !exited || !complete { return ProofToday::CouldNotRun(output); }
    let proof = read_a_proof_run(&output);
    if proof == ProofToday::PassesAlready && !succeeded { ProofToday::CouldNotRun("the test text reported success but the process failed".into()) }
    else { proof }
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
    let RunRead { passed, failed, crashed } = read_run(text);
    if crashed {
        return ProofToday::Fails;
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

/// What a test run says, whichever runner printed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RunRead {
    pub passed: usize,
    pub failed: usize,
    /// A test program died without saying how it went -- killed, a stack
    /// overflow, a crash. Its tests are in neither count, so this alone has
    /// to mean "not passing".
    pub crashed: bool,
}

/// Read a test run's totals (research report item 5, 1 Oct 2026: only
/// cargo's own lines were read, so a JavaScript or Python project's proof
/// read as "no test yet", and a cargo test program that crashed before its
/// summary line could read as passing).
///
/// Cargo's `test result:` lines are added up across every target, by name
/// rather than by position. When there are none, the other runners' summary
/// lines are read: pytest (`== 3 passed, 1 failed in 0.2s ==`), Jest/Vitest
/// (`Tests: 1 failed, 5 passed, 6 total`), node's own runner and TAP
/// (`# pass 5` / `# fail 0`), and go (`--- FAIL:` / `ok  pkg`).
pub fn read_run(text: &str) -> RunRead {
    let mut r = RunRead::default();
    let mut suites_started = 0usize;
    let mut suites_ended = 0usize;
    let count = |rest: &str, r: &mut RunRead| {
        let words: Vec<&str> = rest.split(|c: char| c.is_whitespace() || c == ',').filter(|w| !w.is_empty()).collect();
        for pair in words.windows(2) {
            let Ok(n) = pair[0].trim_end_matches([';', '.', ':']).parse::<usize>() else { continue };
            match pair[1].trim_end_matches([';', '.', ',']).to_lowercase().as_str() {
                "passed" | "passing" => r.passed += n,
                "failed" | "failing" | "errors" | "error" => r.failed += n,
                _ => {}
            }
        }
    };
    for line in text.lines() {
        let l = line.trim();
        if l.starts_with("Running ") && (l.contains("target/") || l.contains("target\\") || l.contains("deps")) {
            suites_started += 1;
        }
        if let Some(rest) = l.strip_prefix("test result:") {
            suites_ended += 1;
            count(rest, &mut r);
        }
        // A test binary killed by a signal, or that exited without its
        // summary: cargo says so in its own words.
        if l.contains("process didn't exit successfully") && (l.contains("signal:") || l.contains("SIGSEGV") || l.contains("SIGABRT") || l.contains("SIGKILL")) {
            r.crashed = true;
        }
        if l.contains("has overflowed its stack") {
            r.crashed = true;
        }
    }
    if suites_ended > 0 {
        // More test programs started than reported: one never finished.
        if suites_started > suites_ended && !text.contains("error: test failed") {
            r.crashed = true;
        }
        return r;
    }
    // Not cargo. Each runner's summary line, read the same way.
    let mut found = false;
    for line in text.lines() {
        let l = line.trim();
        let lower = l.to_lowercase();
        let pytest = l.starts_with('=') && l.ends_with('=') && (lower.contains(" passed") || lower.contains(" failed")) && lower.contains(" in ");
        let jest = lower.starts_with("tests:") && lower.contains("total");
        if pytest || jest {
            found = true;
            count(l.trim_matches('='), &mut r);
        } else if let Some(n) = lower.strip_prefix("# pass ").or_else(|| lower.strip_prefix("ℹ pass ")).and_then(|n| n.trim().parse::<usize>().ok()) {
            found = true;
            r.passed += n;
        } else if let Some(n) = lower.strip_prefix("# fail ").or_else(|| lower.strip_prefix("ℹ fail ")).and_then(|n| n.trim().parse::<usize>().ok()) {
            found = true;
            r.failed += n;
        } else if l.starts_with("--- FAIL:") {
            found = true;
            r.failed += 1;
        } else if l.starts_with("--- PASS:") {
            found = true;
            r.passed += 1;
        }
    }
    if !found && text.lines().any(|l| l.starts_with("ok  \t") || l.starts_with("ok  	")) && !text.lines().any(|l| l.starts_with("FAIL")) {
        // `go test` without -v: a package line per package, no counts.
        r.passed = text.lines().filter(|l| l.starts_with("ok ")).count();
    }
    r
}

/// Run the test suite in the sandbox and read the result.

pub(crate) fn run_tests_controlled(sandbox: &mut Sandbox, cfg: &SelfWorkConfig, budget: Option<&crate::tools::WorkBudget<'_>>) -> Tried {
    let tool = crate::tools::ExternalTool {
        command: cfg.test_command.split_whitespace().next().unwrap_or("cargo").into(),
        args: cfg.test_command.split_whitespace().skip(1).map(String::from).collect(),
        // A cold build of the whole tree plus the suite, on a laptop. Past
        // this it is stuck, and is stopped rather than waited on for ever.
        timeout_secs: budget.map_or(SUITE_LIMIT_SECS, |b| b.remaining(std::time::Duration::from_secs(SUITE_LIMIT_SECS)).as_secs().max(1)),
        ..Default::default()
    };
    let stop = || budget.is_some_and(|b| b.stopping());
    let mut attempt = sandbox.run_controlled(&tool, &Default::default(), 20_000, Some(&stop));
    if let Some(b) = budget { if let Err(why) = b.check() { attempt.passed = false; attempt.output = format!("error: {why}"); } }
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
    if !output.contains("test result:") {
        // Another runner's summary (`read_run`): its passes, when none failed.
        let r = read_run(output);
        return if r.failed == 0 && !r.crashed { r.passed } else { 0 };
    }
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
    let root = crate::store::state_root_for(keep)?.unwrap_or_else(|| keep.to_path_buf());
    let _state = crate::store::state_transaction(&root)?;
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
                crate::heard!(std::fs::remove_file(&c.target));
            } else if let Some((_, old)) = old_bytes.iter().find(|(t, _)| t == &c.target) {
                crate::kept!(std::fs::write(&c.target, old));
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
    prove_in_project_controlled(root, test_command, edits, base, None)
}

pub(crate) fn prove_in_project_controlled(root: &std::path::Path, test_command: &str, edits: &[Edit], base: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>) -> Result<ProjectProof, String> {
    if let Some(b) = budget { b.check()?; }
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
    crate::heard!(std::fs::remove_dir_all(&dst));
    copy_project_controlled(root, &dst, budget, &mut (0usize, 0u64)).map_err(|e| format!("couldn't copy the project to work in: {e}"))?;

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
    let (exited_successfully, output) = run_bounded_controlled(cmd, &dst, PROJECT_PROOF_BUDGET_SECS, budget)?;
    crate::heard!(std::fs::remove_dir_all(&dst));

    let read = read_a_proof_run(&output);
    let built_and_passed = exited_successfully && read == ProofToday::PassesAlready;
    let tests_run = count_passing(&output);
    Ok(ProjectProof { built_and_passed, tests_run, output, replaced_existing })
}


fn copy_project_controlled(from: &std::path::Path, to: &std::path::Path, budget: Option<&crate::tools::WorkBudget<'_>>, copied: &mut (usize, u64)) -> std::io::Result<()> {
    if let Some(b) = budget { b.check().map_err(std::io::Error::other)?; }
    const SKIP: &[&str] =
        &["target", ".git", "node_modules", "__pycache__", ".venv", "venv", "dist", "build"];
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        if let Some(b) = budget { b.check().map_err(std::io::Error::other)?; }
        let entry = entry?;
        let name = entry.file_name();
        if SKIP.iter().any(|s| *s == name) {
            continue;
        }
        let src = entry.path();
        let dst = to.join(&name);
        if src.is_dir() {
            copy_project_controlled(&src, &dst, budget, copied)?;
        } else if src.is_file() {
            use std::io::{Read, Write};
            copied.0 = copied.0.saturating_add(1); copied.1 = copied.1.saturating_add(entry.metadata()?.len());
            if copied.0 > 8192 || copied.1 > 128 * 1024 * 1024 { return Err(std::io::Error::other("project repair exceeds the bounded copy budget; no complete copy was made")); }
            let mut input = std::fs::File::open(&src)?; let mut output = std::fs::File::create(&dst)?; let mut bytes = [0u8; 64 * 1024];
            loop { if let Some(b) = budget { b.check().map_err(std::io::Error::other)?; } let n = input.read(&mut bytes)?; if n == 0 { break; } output.write_all(&bytes[..n])?; }
        }
    }
    Ok(())
}


fn run_bounded_controlled(command: &str, dir: &std::path::Path, budget_secs: u64, budget: Option<&crate::tools::WorkBudget<'_>>) -> Result<(bool, String), String> {
    if let Some(b) = budget { b.check()?; }
    let mut parts = command.split_whitespace();
    let program = parts.next().unwrap_or("cargo");
    let mut cmd = crate::tools::command(program);
    cmd.args(parts).current_dir(dir);
    crate::sandbox::warm_cargo(&mut cmd, program, &[]);
    let cap = std::time::Duration::from_secs(budget_secs);
    let stop = || budget.is_some_and(|b| b.stopping());
    let run = crate::tools::run_scoped(&mut cmd, budget.map_or(cap, |b| b.remaining(cap)), 8 * 1024 * 1024, None, Some(&stop));
    let completed = matches!(run.end, crate::tools::ProcessEnd::Exited(_)) && !run.truncated;
    let (passed, text) = run.said(8 * 1024 * 1024);
    if let Some(b) = budget { b.check()?; }
    if completed { Ok((passed, text)) }
    else { Err(format!("the verification command did not complete successfully: {text}")) }
}

/// Is this folder Atlas's own source -- a `Cargo.toml` naming the `atlas`
/// package -- rather than wherever an installed copy was started from?
pub fn is_a_source_checkout(root: &std::path::Path) -> bool {
    std::fs::read_to_string(root.join("Cargo.toml"))
        .map(|t| t.lines().any(|l| {
            let l = l.replace(' ', "");
            l == "name=\"atlas\""
        }))
        .unwrap_or(false)
}


/// Where Atlas's own source is on this computer, if anywhere: the folder set
/// in settings, then `ATLAS_SOURCE`, then the folder it was started in and
/// the folders above the program, then a short look through the home folder
/// (three levels, a bounded number of folders). Never assumes one person's
/// layout: an installed Atlas is started from wherever Windows likes --
/// System32, 1 Oct 2026 -- and a friend's copy may have no source at all.
pub fn source_root(cfg: &SelfWorkConfig) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    let here = |p: PathBuf| -> Option<PathBuf> {
        if is_a_source_checkout(&p) {
            return Some(p);
        }
        let inner = p.join("atlas");
        is_a_source_checkout(&inner).then_some(inner)
    };
    let set = crate::doctor::expand_env(cfg.source_dir.trim());
    if !set.is_empty() {
        if let Some(p) = here(PathBuf::from(&set)) {
            return Some(p);
        }
    }
    if let Some(p) = crate::doctor::lookup_env("ATLAS_SOURCE").and_then(|e| here(PathBuf::from(e))) {
        return Some(p);
    }
    if let Some(p) = std::env::current_dir().ok().and_then(here) {
        return Some(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        for a in exe.ancestors().skip(1).take(4) {
            if let Some(p) = here(a.to_path_buf()) {
                return Some(p);
            }
        }
    }
    let home = crate::doctor::lookup_env("USERPROFILE").or_else(|| crate::doctor::lookup_env("HOME"))?;
    // The walk below reads up to 4000 folders. Its answer is kept for ten
    // minutes (2 Oct 2026: asked every tick, it was most of the daemon's
    // idle CPU), and is checked still to be a checkout before it's reused.
    static WALKED: std::sync::Mutex<Option<(String, std::time::Instant, Option<PathBuf>)>> = std::sync::Mutex::new(None);
    if let Ok(w) = WALKED.lock().or_else(crate::crash::unpoison) {
        if let Some((h, at, found)) = w.as_ref() {
            if *h == home && at.elapsed() < std::time::Duration::from_secs(600) && found.as_ref().is_none_or(|p| is_a_source_checkout(p)) {
                return found.clone();
            }
        }
    }
    let found = walk_home_for_source(&home);
    if let Ok(mut w) = WALKED.lock().or_else(crate::crash::unpoison) {
        *w = Some((home, std::time::Instant::now(), found.clone()));
    }
    found
}

fn walk_home_for_source(home: &str) -> Option<std::path::PathBuf> {
    use std::path::PathBuf;
    const SKIP: &[&str] = &["appdata", "library", "pictures", "music", "videos", "node_modules", "target", "onedrive", "dropbox", "google drive", "icloud drive"];
    let mut level = vec![PathBuf::from(home)];
    let mut seen = 0usize;
    for _ in 0..3 {
        let mut next = Vec::new();
        for d in &level {
            let Ok(rd) = std::fs::read_dir(d) else { continue };
            for e in rd.flatten() {
                seen += 1;
                if seen > 4000 {
                    return None;
                }
                let name = e.file_name().to_string_lossy().to_lowercase();
                if name.starts_with('.') || SKIP.contains(&name.as_str()) || !e.file_type().is_ok_and(|t| t.is_dir()) {
                    continue;
                }
                if is_a_source_checkout(&e.path()) {
                    return Some(e.path());
                }
                next.push(e.path());
            }
        }
        level = next;
    }
    None
}

#[cfg(test)]
mod edits_for_a_big_file {
    use super::*;

    #[test]
    fn an_exact_block_replaces_its_one_place() {
        let file = "fn a() {\n    1\n}\nfn b() {\n    2\n}\n";
        let reply = "<<<<<<< SEARCH\nfn b() {\n    2\n=======\nfn b() {\n    3\n>>>>>>> REPLACE\n";
        let blocks = edit_blocks(reply);
        assert_eq!(blocks.len(), 1);
        assert_eq!(apply_edit_blocks(file, &blocks).unwrap(), "fn a() {\n    1\n}\nfn b() {\n    3\n}\n");
    }

    #[test]
    fn indentation_a_small_model_got_wrong_still_matches() {
        let file = "impl X {\n        fn go(&self) -> u32 {\n            7\n        }\n}\n";
        let blocks = vec![("fn go(&self) -> u32 {\n    7".to_string(), "        fn go(&self) -> u32 {\n            8".to_string())];
        assert_eq!(apply_edit_blocks(file, &blocks).unwrap(), "impl X {\n        fn go(&self) -> u32 {\n            8\n        }\n}\n");
    }

    #[test]
    fn a_block_that_matches_nowhere_or_twice_fails_rather_than_guessing() {
        let file = "x = 1\nx = 1\n";
        assert!(apply_edit_blocks(file, &[("x = 1".into(), "x = 2".into())]).unwrap_err().contains("2 places"));
        assert!(apply_edit_blocks(file, &[("y = 1".into(), "y = 2".into())]).unwrap_err().contains("doesn't match"));
    }

    #[test]
    fn the_excerpt_is_where_the_diagnosis_points() {
        let mut file = String::new();
        for i in 0..1000 {
            file.push_str(&format!("// line {i}\n"));
        }
        file.push_str("pub fn reconcile_at(now: u64) {}\n");
        let (from, text) = excerpt_for(&file, "health::Reporter::reconcile_at", "");
        assert!(text.contains("pub fn reconcile_at"));
        assert!(from > 900 && text.lines().count() <= EXCERPT_LINES);
        // Nothing named: the start of the file.
        assert_eq!(excerpt_for(&file, "somewhere", "").0, 0);
    }
}

#[cfg(test)]
mod bounded_repair_tests {
    use super::*;
    use crate::brain::Llm;
    struct UnsafeModel;
    impl Llm for UnsafeModel { fn complete(&self, _: &str, _: &str) -> crate::error::Result<String> { panic!("an unbounded model must never be called") } }
    struct WaitingModel;
    impl Llm for WaitingModel {
        fn complete(&self, _: &str, _: &str) -> crate::error::Result<String> { panic!("bounded repair must use chat_until") }
        fn supports_bounded_chat(&self) -> bool { true }
        fn chat_until(&self, _: &crate::brain::ChatRequest, _: &mut dyn FnMut(&str) -> bool, keep: &dyn Fn() -> bool) -> crate::error::Result<crate::brain::ChatReply> {
            while keep() { std::thread::sleep(std::time::Duration::from_millis(5)); }
            Ok(crate::brain::ChatReply::from_text("unfinished"))
        }
    }
    #[test]
    fn unbounded_model_is_refused_and_bounded_model_deadline_is_real() {
        let stop = || false;
        let budget = crate::tools::WorkBudget::new(std::time::Duration::from_millis(40), &stop);
        let unsafe_model = BoundedModel { model: &UnsafeModel, budget: &budget };
        assert!(unsafe_model.complete("repair", "source").unwrap_err().to_string().contains("cannot guarantee"));
        let model = BoundedModel { model: &WaitingModel, budget: &budget };
        let start = std::time::Instant::now();
        assert!(model.complete("repair", "source").unwrap_err().to_string().contains("time budget"));
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
    }
    #[test]
    fn exhausted_shared_budget_refuses_a_later_command_before_action() {
        let stop = || false;
        let budget = crate::tools::WorkBudget::new(std::time::Duration::ZERO, &stop);
        let result = run_bounded_controlled("a-program-that-must-never-start", std::path::Path::new("."), 60, Some(&budget));
        assert!(result.unwrap_err().contains("time budget"));
    }
    #[test]
    fn ambiguous_recipe_is_not_guessed_and_rust_endpoint_candidate_is_structured() {
        let edit = Edit { path: "pricing.py".into(), content: "sum(prices[:-1]) + sum(prices[:-1])".into(), reason: String::new() };
        assert!(recipe_candidates(&[edit]).is_empty());
        let rust = Edit { path: "pricing.rs".into(), content: "values.iter().take(values.len() - 1).sum::<i32>()".into(), reason: String::new() };
        let candidates = recipe_candidates(&[rust]);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].1[0].content, "values.iter().take(values.len()).sum::<i32>()");
    }
}
