//! Thought, build, review, refine, implement.
//!
//! The old loop went straight from a goal to an attempt, which is why it would
//! have rotted. Two things go wrong when you skip the thinking, and both are
//! invisible while the tests are green:
//!
//! 1. You fix the symptom. The test passes, the cause is still there, and it
//!    comes back somewhere else in three weeks.
//! 2. You write a test that would have passed anyway. It proves nothing and
//!    it will never fail again, so it's worse than no test — it's a false
//!    reassurance you'll trust later.
//!
//! The rule that catches both: **before writing any code, say what would prove
//! it fixed, and check that the proof fails right now.** A proving test that
//! passes before the change tests something else.
//!
//! Every stage produces an artefact. You cannot enter a stage without the
//! previous one's artefact, and nothing lands without all five.

use serde::{Deserialize, Serialize};

/// Where a piece of work is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Thought,
    Build,
    Review,
    Refine,
    Implement,
    /// Finished and landed.
    Done,
    /// Stopped, with the reason kept.
    Abandoned,
}

impl Stage {
    /// What to call this stage out loud. `{stage:?}` put "Refine" on the
    /// screen as a Rust variant name.
    pub fn plain(&self) -> &'static str {
        match self {
            Stage::Thought => "thinking it through",
            Stage::Build => "building it",
            Stage::Review => "reviewing it",
            Stage::Refine => "refining it",
            Stage::Implement => "putting it in",
            Stage::Done => "finished",
            Stage::Abandoned => "stopped",
        }
    }

    pub fn next(&self) -> Stage {
        match self {
            Stage::Thought => Stage::Build,
            Stage::Build => Stage::Review,
            Stage::Review => Stage::Refine,
            Stage::Refine => Stage::Implement,
            Stage::Implement => Stage::Done,
            other => *other,
        }
    }
}

// ---------- stage one: what's actually wrong ----------

/// Written before any code exists.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Thought {
    /// What you noticed. The symptom.
    pub symptom: String,
    /// Why it happens. If this is the same sentence as the symptom, the
    /// thinking hasn't happened yet.
    pub cause: String,
    /// Which part of the system is wrong.
    pub where_: String,
    /// The test that would fail now and pass after. Named before writing it.
    pub proof: String,
    /// Confirmed that the proof fails today.
    ///
    /// The single most valuable check in the loop. A proving test that already
    /// passes is testing something else.
    pub proof_fails_now: bool,
    /// What this deliberately isn't fixing.
    pub not_doing: Vec<String>,
}

impl Thought {
    /// Is this thinking, or a restatement?
    pub fn is_thought_through(&self) -> Result<(), String> {
        if self.cause.trim().is_empty() {
            return Err("no cause — that's a symptom, not a diagnosis".into());
        }
        // The commonest failure: "it's slow" / "because it's slow".
        if similar(&self.symptom, &self.cause) {
            return Err(
                "the cause is a restatement of the symptom. Why does it happen?".into()
            );
        }
        if self.proof.trim().is_empty() {
            return Err("nothing named that would prove it fixed".into());
        }
        if !self.proof_fails_now {
            return Err(
                "the proving test passes already, so it isn't testing this. Write one that \
                 fails first"
                    .into(),
            );
        }
        Ok(())
    }
}

/// A diagnosis being collected, one answer per turn.
///
/// `Thought` has four required parts and a confirmation, and there was no way
/// to supply any of them: the only production path into `Work` was
/// `Work::new`, which leaves `thought` as `None` for ever. So this exists to
/// be the way in.
///
/// One question at a time rather than four fields parsed out of a sentence.
/// Parsing would be guessing, and the one part that must not be guessed is
/// `where_` — the review's main check is whether the change landed where the
/// cause was said to be, and a `where_` inferred from the same sentence as
/// the symptom makes that check compare a guess against itself.
///
/// `proof_fails_now` is deliberately **not** collectable here. It is the most
/// valuable check in the loop and the easiest to answer wrongly in good
/// faith, so it is set from actually running the named test — see
/// `Diagnosing::into_thought`, which takes it as an argument rather than
/// asking for it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Diagnosing {
    pub symptom: Option<String>,
    pub cause: Option<String>,
    pub where_: Option<String>,
    pub proof: Option<String>,
}

impl Diagnosing {
    /// What Atlas still needs, or `None` when it has all four.
    pub fn next_question(&self) -> Option<&'static str> {
        if self.symptom.is_none() {
            return Some("What did you notice? The wrong thing, as you'd describe it.");
        }
        if self.cause.is_none() {
            return Some(
                "And why does it happen? If that comes out as the same sentence again, we \
                 haven't got it yet — that's the check, not me being difficult.",
            );
        }
        if self.where_.is_none() {
            return Some("Which part of me is wrong? A file or an area is enough.");
        }
        if self.proof.is_none() {
            return Some(
                "What would prove it fixed? Name the test before I write it — I'll run it \
                 first and it has to fail.",
            );
        }
        None
    }

    /// Take the next answer. Returns the next question, or `None` when done.
    ///
    /// Blank answers are ignored rather than stored, so a stray empty turn
    /// cannot fill a required part with nothing and satisfy
    /// `is_thought_through`'s emptiness checks by accident.
    pub fn answer(&mut self, text: &str) -> Option<&'static str> {
        let t = text.trim();
        if !t.is_empty() {
            let slot = if self.symptom.is_none() {
                &mut self.symptom
            } else if self.cause.is_none() {
                &mut self.cause
            } else if self.where_.is_none() {
                &mut self.where_
            } else if self.proof.is_none() {
                &mut self.proof
            } else {
                return None;
            };
            *slot = Some(t.to_string());
        }
        self.next_question()
    }

    /// The name of the test to run, once there is one.
    pub fn proving_test(&self) -> Option<&str> {
        self.proof.as_deref()
    }

    /// All four parts plus the result of running the proving test.
    ///
    /// `proof_fails_now` comes from the caller because it is a measurement,
    /// not an answer.
    pub fn into_thought(&self, proof_fails_now: bool, not_doing: Vec<String>) -> Option<Thought> {
        Some(Thought {
            symptom: self.symptom.clone()?,
            cause: self.cause.clone()?,
            where_: self.where_.clone()?,
            proof: self.proof.clone()?,
            proof_fails_now,
            not_doing,
        })
    }

    /// Start the last answer again.
    ///
    /// Needed because `is_thought_through` can refuse a complete diagnosis —
    /// a cause that restates the symptom is the common case — and without
    /// this the session would be stuck holding four answers it cannot use and
    /// no way to change one.
    pub fn take_back_the_cause(&mut self) {
        self.cause = None;
    }
}

/// Rough sameness, for catching a restated symptom.
fn similar(a: &str, b: &str) -> bool {
    let words = |s: &str| -> Vec<String> {
        s.to_lowercase()
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .map(str::to_string)
            .collect()
    };
    let (x, y) = (words(a), words(b));
    if x.is_empty() || y.is_empty() {
        return false;
    }
    let shared = x.iter().filter(|w| y.contains(w)).count();
    // Most of the shorter sentence appearing in the longer one means the
    // second is the first said again. "The panel takes too long to open" and
    // "the panel is too slow to open" share two of three.
    shared as f32 / x.len().min(y.len()) as f32 >= 0.6
}

// ---------- stage two: the change ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Build {
    /// Files touched.
    pub touched: Vec<String>,
    /// Tests passing before.
    pub tests_before: usize,
    pub tests_after: usize,
    /// The proving test now passes.
    pub proof_passes: bool,
    /// Nothing else broke.
    pub nothing_else_broke: bool,
}

// ---------- stage three: did it fix the cause ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub notes: Vec<Note>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub kind: Concern,
    pub what: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Concern {
    /// The change is somewhere other than where the thought said the cause
    /// was. Usually means the symptom was patched.
    NotWhereTheCauseWas,
    /// More was changed than the goal needed.
    Scope,
    /// A test was changed to make it pass.
    TestWeakened,
    /// It works and is hard to follow.
    HardToFollow,
    /// Nothing wrong.
    Fine,
}

impl Concern {
    /// Must this be dealt with before landing?
    fn blocks(&self) -> bool {
        matches!(self, Concern::NotWhereTheCauseWas | Concern::TestWeakened)
    }
}

/// Review a build against the thought that produced it.
///
/// The check that does the work: **did the change happen where the cause
/// was?** A fix somewhere else may still make the test pass, and that's
/// exactly what patching a symptom looks like from the inside.
pub fn review(t: &Thought, b: &Build, tests_changed: &[String]) -> Review {
    let mut notes = Vec::new();

    let touched_the_cause = b
        .touched
        .iter()
        .any(|f| f.contains(&t.where_) || t.where_.contains(f.as_str()));
    if !touched_the_cause {
        notes.push(Note {
            kind: Concern::NotWhereTheCauseWas,
            what: format!(
                "the cause was in {} and nothing there changed. The test may be passing for a \
                 different reason",
                t.where_
            ),
        });
    }

    // A test edited in the same change as the fix it's meant to prove is the
    // oldest way to make a suite green and meaningless.
    if !tests_changed.is_empty() && b.tests_after < b.tests_before {
        notes.push(Note {
            kind: Concern::TestWeakened,
            what: format!(
                "{} fewer tests than before, and {} were edited",
                b.tests_before - b.tests_after,
                tests_changed.len()
            ),
        });
    }

    if b.touched.len() > 4 {
        notes.push(Note {
            kind: Concern::Scope,
            what: format!("{} files for one fix — some of that is probably unrelated", b.touched.len()),
        });
    }

    if notes.is_empty() {
        notes.push(Note { kind: Concern::Fine, what: "does what it said, where it said".into() });
    }
    Review { notes }
}

impl Review {
    pub fn clean(&self) -> bool {
        self.notes.iter().all(|n| !n.kind.blocks())
    }

    pub fn blockers(&self) -> Vec<&Note> {
        self.notes.iter().filter(|n| n.kind.blocks()).collect()
    }
}

// ---------- stage four: fixing what the review found ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refinement {
    /// Which review note this addresses. A refinement that answers nothing is
    /// scope creep wearing a hat.
    pub answers: Concern,
    pub what_changed: String,
}

/// Is this refinement legitimate?
pub fn refinement_is_warranted(r: &Refinement, review: &Review) -> Result<(), String> {
    if !review.notes.iter().any(|n| n.kind == r.answers) {
        return Err(format!(
            "nothing in the review was about {:?}, so this is a new change rather than a \
             refinement",
            r.answers
        ));
    }
    Ok(())
}

// ---------- the whole thing ----------

/// Serialisable so a piece of work survives between turns. Without that the
/// five stages can never be reached: a `Work` rebuilt each time is always at
/// `Thought` with no thought in it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Work {
    pub goal: String,
    pub stage: Stage,
    pub thought: Option<Thought>,
    pub build: Option<Build>,
    pub review: Option<Review>,
    pub refinements: Vec<Refinement>,
    /// Times round the review–refine loop.
    pub rounds: u32,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PipelineConfig {
    pub enabled: bool,
    /// Times round review and refine before handing it over.
    ///
    /// Bounded because a loop that can go round forever will, and the third
    /// attempt at the same problem is usually a sign the thought was wrong
    /// rather than the code.
    pub max_rounds: u32,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        PipelineConfig { enabled: false, max_rounds: 3 }
    }
}

/// What happens next.
#[derive(Debug, Clone, PartialEq)]
pub enum Next {
    /// Do this stage.
    Do(Stage),
    /// Can't move on, and why.
    Blocked(String),
    /// Ready to land. Here's what changes, in behaviour.
    Land(String),
    /// Give up and write it up. The thought was probably wrong.
    HandOver(String),
}

impl Work {
    pub fn new(goal: &str) -> Work {
        Work {
            goal: goal.into(),
            stage: Stage::Thought,
            thought: None,
            build: None,
            review: None,
            refinements: Vec::new(),
            rounds: 0,
        }
    }

    pub fn what_next(&self, cfg: &PipelineConfig) -> Next {
        match self.stage {
            Stage::Thought => match &self.thought {
                None => Next::Do(Stage::Thought),
                Some(t) => match t.is_thought_through() {
                    Ok(()) => Next::Do(Stage::Build),
                    Err(why) => Next::Blocked(why),
                },
            },
            Stage::Build => match &self.build {
                None => Next::Do(Stage::Build),
                Some(b) if !b.proof_passes => {
                    Next::Blocked("the proving test still fails".into())
                }
                Some(b) if !b.nothing_else_broke => {
                    Next::Blocked("something else broke".into())
                }
                Some(_) => Next::Do(Stage::Review),
            },
            Stage::Review => match &self.review {
                None => Next::Do(Stage::Review),
                Some(r) if r.clean() => Next::Do(Stage::Implement),
                Some(_) if self.rounds >= cfg.max_rounds => Next::HandOver(format!(
                    "{} rounds on \"{}\" and the review still isn't clean. That usually means the \
                     thought was wrong rather than the code",
                    self.rounds, self.goal
                )),
                Some(r) => Next::Blocked(
                    r.blockers()
                        .first()
                        .map(|n| n.what.clone())
                        .unwrap_or_else(|| "something in the review".into()),
                ),
            },
            Stage::Refine => Next::Do(Stage::Review),
            Stage::Implement => Next::Land(self.in_behaviour()),
            Stage::Done => Next::Land("already landed".into()),
            Stage::Abandoned => Next::HandOver("stopped".into()),
        }
    }

    /// What changes, said as behaviour rather than as a diff.
    ///
    /// The only description that reaches you.
    pub fn in_behaviour(&self) -> String {
        let t = match &self.thought {
            Some(t) => t,
            None => return self.goal.clone(),
        };
        let mut s = format!("{} — it was {}.", t.symptom, t.cause);
        if !t.not_doing.is_empty() {
            s.push_str(&format!(" Not touching: {}.", t.not_doing.join(", ")));
        }
        if !self.refinements.is_empty() {
            s.push_str(&format!(" {} rounds of review.", self.rounds));
        }
        s
    }

    // -----------------------------------------------------------------
    // Advancing. Everything above this line was reachable and inert.
    //
    // ## What was wrong
    //
    // `Work::stage`, `Work::thought`, `Work::build`, `Work::review`,
    // `Work::refinements` and `Work::rounds` were **never written anywhere in
    // `src/`.** `Work::new` set `stage: Stage::Thought` and the four artefacts
    // to `None`/empty, and no code in the tree ever changed any of them again.
    //
    // Every consequence follows from that one fact:
    //
    // * `Session::may_start` refuses while `thought` is `None`, so
    //   `Intent::WorkOnYourself("fix X")` answered *"Before I touch anything:
    //   no diagnosis — what's actually wrong, and what would prove it
    //   fixed?"* and there was no way to answer it. Every time, for ever.
    // * `what_next` could only ever take the `Stage::Thought` arm, so
    //   `Next::Land` — and with it `Daemon::land_it`,
    //   `selfwork::what_holds_it_back`, `selfwork::land`, the four landing
    //   checks and `selfgrant::may_land` — was unreachable in production.
    // * `Session::after`, `Session::begin`, `Session::take_it_elsewhere`,
    //   `run_tests`, `Tried::worth_showing`, `review`,
    //   `refinement_is_warranted` and the whole `strategy` ladder had no
    //   production caller either, because nothing could get past stage one.
    //
    // So "Atlas fixes faulty code in a sandbox, and if it passes it gets
    // fixed then and there" had every piece built, tested and correct, and no
    // way in. A previous pass said the missing part was the last four words;
    // the missing part was the first sentence.
    //
    // ## Why every guard missed it
    //
    // `wiring.rs` and `dead_capabilities.rs` ask whether the program can
    // reach a function. It could: `WorkOnYourself` is a real intent that
    // really dispatches and really calls `may_start` and `what_next`. What
    // nothing asked is whether a **field** those functions branch on is ever
    // given a value. `one_name_one_record.rs` asks that question one level
    // up, about the store's records, and found two. This is the same shape
    // inside a struct, and `tests/a_stage_that_cannot_be_reached.rs` is the
    // guard for it.
    //
    // ## The shape of the fix
    //
    // One method per stage, each of which refuses the artefact that would not
    // justify the advance. The stage is a consequence of an artefact being
    // accepted, never something a caller can set — a settable stage is the
    // same hole with a nicer name.
    // -----------------------------------------------------------------

    /// Take a diagnosis, and move to Build if it holds up.
    ///
    /// The refusal is `Thought::is_thought_through`, unchanged — the point of
    /// this method is that there is now something that can be refused.
    pub fn record_thought(&mut self, t: Thought) -> Result<(), String> {
        if self.stage != Stage::Thought {
            return Err(format!(
                "this one is already past the thinking — it's {} now",
                self.stage.plain()
            ));
        }
        t.is_thought_through()?;
        self.thought = Some(t);
        self.stage = Stage::Build;
        Ok(())
    }

    /// Take the result of building it, and move to Review if it stands up.
    ///
    /// The two refusals are the ones `what_next` already described and could
    /// never reach: the proving test has to pass now, and nothing else may
    /// have broken.
    pub fn record_build(&mut self, b: Build) -> Result<(), String> {
        if self.stage != Stage::Build && self.stage != Stage::Refine {
            return Err(format!("nothing is waiting to be built — it's {}", self.stage.plain()));
        }
        if self.thought.is_none() {
            return Err("nothing said what was wrong, so there is nothing to build".into());
        }
        if !b.proof_passes {
            self.build = Some(b);
            return Err("the proving test still fails".into());
        }
        if !b.nothing_else_broke {
            self.build = Some(b);
            return Err("something else broke".into());
        }
        self.build = Some(b);
        self.stage = Stage::Review;
        Ok(())
    }

    /// Take a review. A clean one moves to Implement; a blocking one goes
    /// round to Refine and counts the round.
    ///
    /// `rounds` is incremented here and nowhere else, which is what makes
    /// `max_rounds` mean anything — it was a config value compared against a
    /// number that was always zero.
    pub fn record_review(&mut self, r: Review) -> Result<(), String> {
        if self.stage != Stage::Review {
            return Err(format!("nothing is waiting to be reviewed — it's {}", self.stage.plain()));
        }
        let clean = r.clean();
        let first_blocker = r.blockers().first().map(|n| n.what.clone());
        self.review = Some(r);
        if clean {
            self.stage = Stage::Implement;
            return Ok(());
        }
        self.rounds += 1;
        self.stage = Stage::Refine;
        Err(first_blocker.unwrap_or_else(|| "something in the review".into()))
    }

    /// Take a refinement that answers a review note, and go back to Build.
    ///
    /// Back to Build rather than straight to Review, because a refinement is
    /// a change and a change has to prove itself again. Refining into a
    /// review that was written about different code is how a loop like this
    /// launders a second fault past a first review.
    pub fn record_refinement(&mut self, r: Refinement) -> Result<(), String> {
        if self.stage != Stage::Refine {
            return Err(format!("nothing is waiting to be refined — it's {}", self.stage.plain()));
        }
        let Some(review) = self.review.as_ref() else {
            return Err("there is no review for this to be answering".into());
        };
        refinement_is_warranted(&r, review)?;
        self.refinements.push(r);
        // The old review described the code before the refinement.
        self.review = None;
        self.stage = Stage::Build;
        Ok(())
    }

    /// It went onto the machine.
    ///
    /// Separate from `record_review` so that a stage can only reach `Done`
    /// after something actually copied files — the landing gate lives in
    /// `selfwork::what_holds_it_back` and can still refuse at `Implement`.
    pub fn landed(&mut self) {
        self.stage = Stage::Done;
    }

    /// Stopped, and it is not coming back.
    pub fn abandon(&mut self) {
        self.stage = Stage::Abandoned;
    }

    /// Nothing lands with a stage missing.
    pub fn may_land(&self) -> Result<(), String> {
        if self.thought.is_none() {
            return Err("no thought — nothing said what was actually wrong".into());
        }
        if self.build.is_none() {
            return Err("nothing was built".into());
        }
        match &self.review {
            None => Err("not reviewed".into()),
            Some(r) if !r.clean() => Err(r.blockers()[0].what.clone()),
            Some(_) => Ok(()),
        }
    }
}

/// Why this shape rather than just running tests.
pub const WHY_THE_STAGES: &str =
    "Green tests tell you nothing about whether you fixed the right thing. A symptom patched \
     somewhere other than the cause passes exactly as well as a real fix, and comes back in three \
     weeks somewhere else. Naming the cause before writing code, and proving the test fails first, \
     is what tells those two apart.";
