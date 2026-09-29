//! Fixing what's broken without papering over it.
//!
//! `handoff` asks a person for help when Atlas is stuck. `plainly` says what
//! is wrong in your words. This is the layer underneath both: **deciding
//! whether a proposed fix is a fix at all.**
//!
//! Every failing check has a cheap way out. A borrow error goes away if you
//! clone. A type error goes away if you widen the type to something that
//! accepts anything. A failing test goes away if you delete the test. Each of
//! those turns the build green, and each leaves the thing that was actually
//! wrong exactly where it was, now with nothing pointing at it.
//!
//! That is worse than the original bug. The bug was visible; the paper-over is
//! a bug with a green tick on it. And an assistant that is graded on the build
//! passing will find the cheap way out every time, because it is faster and it
//! works.
//!
//! So this refuses them by name, and it does that rather than trusting good
//! intentions, because the pressure to take the cheap route is strongest at
//! 3am on the eighth attempt.
//!
//! ## The second half: asking someone who doesn't read code
//!
//! Some failures cannot be fixed without a decision, and those decisions are
//! almost never technical. "Should a missing file be an error or an empty
//! list" is a question about what the thing should do, and you can answer it
//! without reading a line.
//!
//! The rule this enforces: **a question put to you must be answerable without
//! reading code.** No identifiers, no file paths, no error codes, no jargon.
//! If Atlas cannot phrase it that way, it has not understood the problem well
//! enough to be asking yet.

use serde::{Deserialize, Serialize};

/// What kind of thing went wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// One right answer, and the tool has already said what it is.
    /// A missing import, a typo, a wrong number of arguments.
    Mechanical,
    /// The code is consistent and does the wrong thing. Needs a theory before
    /// a change.
    Behavioural,
    /// Two reasonable answers and the code can't choose. Needs you.
    Undecided,
    /// Nothing to do with the code — a missing tool, no disk, no permission.
    Environment,
    /// Atlas doesn't know. Said out loud rather than guessed at.
    Unclear,
}

impl Kind {
    /// Can Atlas settle this on its own?
    pub fn atlas_can_fix(&self) -> bool {
        matches!(self, Kind::Mechanical | Kind::Behavioural)
    }

    /// Should it stop and ask rather than keep trying?
    ///
    /// `Unclear` counts. Eight more attempts at something Atlas does not
    /// understand produces eight more variations of not understanding it.
    pub fn needs_you(&self) -> bool {
        matches!(self, Kind::Undecided | Kind::Environment | Kind::Unclear)
    }
}

/// A change that makes an error go away without fixing anything.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Cheat {
    /// Removed or disabled the test that was failing.
    DeletedTheTest,
    /// Silenced the warning rather than the cause.
    SilencedIt,
    /// Widened a type until it stopped complaining.
    MadeItAnything,
    /// Caught the error and did nothing with it.
    SwallowedIt,
    /// Replaced a real value with a default so the failure became a zero.
    DefaultedIt,
    /// Marked it skipped or ignored.
    SkippedIt,
}

impl Cheat {
    /// Why this isn't a fix, in one line.
    pub fn why_not(&self) -> &'static str {
        match self {
            Cheat::DeletedTheTest => {
                "that removes the thing that noticed, not the thing that was wrong"
            }
            Cheat::SilencedIt => "the warning was the messenger",
            Cheat::MadeItAnything => {
                "a type that accepts anything stops the compiler helping anywhere it's used"
            }
            Cheat::SwallowedIt => "the error still happens, you just stop hearing about it",
            Cheat::DefaultedIt => {
                "a failure that becomes a zero is worse than one that stops — it keeps going"
            }
            Cheat::SkippedIt => "a skipped test is a test that will never fail again",
        }
    }
}

/// Shapes that mean a change is a paper-over rather than a fix.
///
/// Matched against the diff, deliberately literally. Something cleverer that
/// guessed at intent would flag legitimate work — there are real reasons to
/// allow a lint or to use a default — and a check that cries wolf is one that
/// gets switched off. These fire on the shapes that are almost never right in
/// a change whose whole purpose is to make a failing check pass.
const PAPER_OVER: &[(&str, Cheat)] = &[
    ("#[ignore]", Cheat::SkippedIt),
    ("@pytest.mark.skip", Cheat::SkippedIt),
    ("@unittest.skip", Cheat::SkippedIt),
    ("#[allow(", Cheat::SilencedIt),
    ("# type: ignore", Cheat::SilencedIt),
    ("# noqa", Cheat::SilencedIt),
    ("#[cfg(ignore)]", Cheat::SkippedIt),
    (": Any", Cheat::MadeItAnything),
    ("-> Any", Cheat::MadeItAnything),
    ("Box<dyn std::any::Any>", Cheat::MadeItAnything),
    ("except: pass", Cheat::SwallowedIt),
    ("except Exception: pass", Cheat::SwallowedIt),
    ("catch_unwind", Cheat::SwallowedIt),
    ("let _ = ", Cheat::SwallowedIt),
    ("unwrap_or_default()", Cheat::DefaultedIt),
    ("unwrap_or(0)", Cheat::DefaultedIt),
];

/// One line that was removed, for spotting a deleted test.
///
/// Separate from the list above because it is about what left the file rather
/// than what arrived in it, and that needs the diff's removed side.
const TEST_MARKERS: &[&str] = &["#[test]", "def test_", "assert!", "assert_eq!", "assert "];

/// A change Atlas wants to make.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposed {
    /// What it thinks is wrong.
    pub theory: String,
    /// Lines being added.
    pub added: Vec<String>,
    /// Lines being removed.
    pub removed: Vec<String>,
}

/// Everything about this change that isn't a fix.
///
/// Empty means it is worth trying. Non-empty means it would turn the build
/// green while leaving the problem in place.
pub fn paper_overs(p: &Proposed) -> Vec<Cheat> {
    let mut out = Vec::new();
    for line in &p.added {
        for (needle, cheat) in PAPER_OVER {
            if line.contains(needle) && !out.contains(cheat) {
                out.push(*cheat);
            }
        }
    }
    // A test that left the file.
    let lost_a_test = p
        .removed
        .iter()
        .any(|l| TEST_MARKERS.iter().any(|m| l.contains(m)))
        && !p
            .added
            .iter()
            .any(|l| TEST_MARKERS.iter().any(|m| l.contains(m)));
    if lost_a_test && !out.contains(&Cheat::DeletedTheTest) {
        out.push(Cheat::DeletedTheTest);
    }
    out
}

/// How to explain a refusal, without assuming you read code.
pub fn refusal(cheats: &[Cheat]) -> String {
    match cheats {
        [] => String::new(),
        [one] => format!(
            "I could make that error go away, but {} — so I'd rather keep looking.",
            one.why_not()
        ),
        many => format!(
            "There are {} shortcuts here that would turn it green without fixing it — \
             the first being that {}. I'd rather keep looking.",
            many.len(),
            many[0].why_not()
        ),
    }
}

// ---------------------------------------------------------------------------
// Asking someone who doesn't read code
// ---------------------------------------------------------------------------

/// A question for you, about behaviour rather than about code.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    /// What Atlas is trying to do, in one line.
    pub doing: String,
    /// The choice, put in terms of what happens.
    pub asks: String,
    /// The options, described by their consequence.
    pub options: Vec<String>,
    /// What stops until you answer.
    ///
    /// Not "what Atlas will do instead". An earlier version had a default
    /// action, and that was wrong: a design decision guessed at is a confident
    /// wrong answer that then has everything else built on top of it. The
    /// whole point of asking is that Atlas does not know.
    ///
    /// So nothing is guessed. This one thing is set aside, the rest of the
    /// work carries on around it, and the question becomes an item on your
    /// list. Silence costs you one parked task rather than a night.
    pub set_aside: String,
}

/// Words that mean the question can't be answered without reading code.
///
/// Not exhaustive and not meant to be. It catches the common ways a technical
/// question wears a plain-English coat, which is enough to send Atlas back to
/// rephrase — and a question it cannot rephrase is one it does not understand
/// well enough to be asking.
const NEEDS_CODE: &[&str] = &[
    "function", "variable", "parameter", "argument", "struct", "enum", "trait",
    "class", "method", "module", "import", "compile", "compiler", "type error",
    "null", "none type", "exception", "stack trace", "line ", "borrow",
    "lifetime", "pointer", "async", "thread", "mutex", "return value",
    "signature", "generic", "syntax", ".rs", ".py", "()", "::", "_t ", "api",
];

impl Question {
    /// Can this be answered without reading code?
    ///
    /// The rule the whole module turns on. If it fails, Atlas has not
    /// understood the problem well enough to ask about it yet.
    pub fn answerable_without_code(&self) -> Result<(), Vec<String>> {
        let all = format!(
            "{} {} {} {}",
            self.doing,
            self.asks,
            self.options.join(" "),
            self.set_aside
        )
        .to_lowercase();
        let found: Vec<String> = NEEDS_CODE
            .iter()
            .filter(|w| all.contains(**w))
            .map(|w| (*w).trim().to_string())
            .collect();
        if found.is_empty() {
            Ok(())
        } else {
            Err(found)
        }
    }

    /// Is it actually a choice?
    ///
    /// One option is not a question, it is an announcement. Three or more and
    /// you are being asked to design rather than to decide.
    pub fn is_a_real_choice(&self) -> bool {
        (2..=3).contains(&self.options.len())
    }

    /// How it reads out loud, addressed to you.
    ///
    /// `called` is what you want to be called. Passed in rather than looked up
    /// here, because how someone wants to be addressed is a fact about them
    /// and belongs where the other facts about them live.
    ///
    /// It opens by saying a problem was found and that this needs a decision.
    /// Leading with the question makes it sound like curiosity; leading with
    /// "I found something and I can't choose" tells you why you are being
    /// interrupted.
    pub fn spoken(&self, called: &str) -> String {
        format!(
            "{called}, I found a problem in this and I need you to decide how it \
             should work. {} {} {} Until you say, I'll leave {} and carry on with \
             the rest.",
            self.doing,
            self.asks,
            self.options
                .iter()
                .enumerate()
                .map(|(i, o)| format!("{}: {o}.", i + 1))
                .collect::<Vec<_>>()
                .join(" "),
            self.set_aside
        )
    }

    /// The item this becomes on your list.
    ///
    /// Parked rather than pending an answer that never comes. It clears only
    /// when you choose — being back at the machine is not a decision.
    pub fn parked(&self) -> crate::backlog::Blocker {
        crate::backlog::Blocker::NeedsYourDecision {
            question: self.asks.clone(),
            set_aside: self.set_aside.clone(),
        }
    }
}

/// Whether a failure is worth putting to you at all.
///
/// Most are not. Asking about every mechanical error is how an assistant
/// becomes something you stop reading.
pub fn should_ask(kind: Kind, attempts_so_far: u32) -> bool {
    match kind {
        Kind::Undecided | Kind::Environment => true,
        // Give it a couple of goes first — plenty of unclear failures become
        // clear once something has been tried.
        Kind::Unclear => attempts_so_far >= 2,
        Kind::Mechanical | Kind::Behavioural => false,
    }
}

// ---------------------------------------------------------------------------
// The half that was missing: something to ask about.
//
// `Question` knows how to be phrased, how to check it is answerable without
// reading code, how to read out loud and what backlog item it becomes.
// **Nothing ever built one**, so `Blocker::NeedsYourDecision` — the one
// backlog variant that means "Atlas found something only you can settle" —
// was produced by exactly one function, `parked()`, and that function had no
// caller. The variant existed, `explain()` described it, the brief surfaced
// it, and nothing could ever create one.
//
// What was missing was not a caller. It was noticing that the daemon already
// had the moment this is for and was dropping it on the floor: asked to do
// something needing approval while you are not there, it said "That needs your
// say-so. I'll wait." and recorded nothing at all. Nothing waited. The next
// time you looked there was no trace that you had been asked.
// ---------------------------------------------------------------------------

/// A decision Atlas cannot take on its own, phrased for you rather than about
/// code.
///
/// `doing` and `set_aside` are the same thing said two ways on purpose: what
/// it was trying to do, and what stops until you answer. Deliberately no
/// default action — a design decision guessed at is a confident wrong answer
/// with everything else built on top of it.
pub fn about_approval(request: &str) -> Question {
    let what = plainer(request);
    Question {
        doing: format!("You asked me to {what}."),
        asks: format!("Do you want me to go ahead with {what}?"),
        options: vec![
            format!("yes — I do it and tell you what happened"),
            format!("no — I drop it and stop asking"),
        ],
        set_aside: what,
    }
}

// `about_ambiguity` stood here: a Question for a request Atlas understood two
// ways. It was deleted rather than kept, and the reason is the whole point of
// this codebase's ratchets.
//
// The moment it was written for is real -- `Decision::AskClarification` parks
// its question in the session, which evaporates if you walk away, exactly like
// the approval branch did. But the daemon has no *two readings* to offer: it
// has one clarification sentence. Building the question would have meant
// inventing the options, and a choice between two things Atlas made up is
// worse than being asked to rephrase.
//
// So it went, and the moment is written down in the outstanding list instead.
// A function with no caller and no way to get one is the thing being counted.


/// A request, with the things only a programmer says taken out of it.
///
/// The point of `answerable_without_code` is that Atlas has to be able to
/// phrase the question plainly, and the commonest reason it cannot is that it
/// is quoting your words back with a file path or an error in them. This
/// strips the ones a request usually carries; anything it misses still fails
/// the check, which is the behaviour that was wanted — a question Atlas cannot
/// phrase is one it does not understand well enough to be asking.
fn plainer(request: &str) -> String {
    let mut words: Vec<&str> = Vec::new();
    for word in request.split_whitespace() {
        // Checked against the word as written, not a trimmed copy. The first
        // version trimmed the punctuation off first and then looked for it --
        // so `cleanup()` had its parentheses removed and sailed through as an
        // ordinary word.
        let looks_technical = word.contains('/')
            || word.contains("::")
            || word.contains("()")
            || word
                .trim_end_matches(|c: char| !c.is_alphanumeric())
                .rsplit('.')
                .next()
                .is_some_and(|ext| {
                    word.contains('.') && (1..=4).contains(&ext.len()) && !ext.is_empty()
                });
        words.push(if looks_technical { "that" } else { word });
    }
    // Two stripped words in a row read as "that that".
    words.dedup_by(|a, b| *a == "that" && *b == "that");

    let joined = words.join(" ");
    let trimmed = joined.trim().trim_end_matches(['.', '!', '?']).trim().to_string();
    if trimmed.is_empty() {
        "that".into()
    } else {
        trimmed
    }
}
