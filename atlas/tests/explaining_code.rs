//! Explaining code to a non-coder — the checkable half.
//!
//! `explain::check` can't tell whether an explanation is *correct* about the
//! code — that needs the code, and it's the model's job. What it holds to is
//! whether it *reads* like a plain-English explanation: words not leaked code,
//! long enough to explain and short enough to follow, and jargon flagged. These
//! defend that, and that a clean result is never called "right", only "reads
//! plainly".

use atlas::brain::MockLlm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::explain::{
    blocking, check, check_at, explain_loop, spoken, Depth, Finding, Outcome, Severity,
};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

fn has(f: &[Finding], rule: &str) -> bool {
    f.iter().any(|x| x.rule == rule)
}

const PLAIN: &str = "This code keeps a running total. Each time a new number comes in, it adds \
that number to the total and remembers the new sum, so at any point you can ask how much has been \
added up so far.";

// --- what blocks (not a usable explanation) --------------------------------

#[test]
fn an_empty_explanation_blocks() {
    let f = check("   ");
    assert!(has(&f, "says something"));
    assert_eq!(f[0].severity, Severity::Blocking);
}

#[test]
fn leaked_code_blocks() {
    // Braces and semicolons are code, not an explanation. (No `name(` shapes,
    // to avoid tripping the bare-name reachability scan.)
    let f = check("if the value is positive { total = total + 1; return true; }");
    assert!(has(&f, "words not code"), "{f:?}");
    assert_eq!(f.iter().find(|x| x.rule == "words not code").unwrap().severity, Severity::Blocking);
}

#[test]
fn too_short_to_explain_blocks() {
    assert!(has(&check("It adds numbers."), "enough to go on"));
    // PLAIN is long enough.
    assert!(!has(&check(PLAIN), "enough to go on"));
}

// --- advisories (works, but a non-coder trips) -----------------------------

#[test]
fn jargon_is_flagged_by_whole_word() {
    let f = check(
        "It stores the value in a buffer and uses recursion to walk the structure back to the start \
         so nothing is lost along the way here.",
    );
    let j = f.iter().find(|x| x.rule == "plain words").expect("jargon flagged");
    assert_eq!(j.severity, Severity::Advisory);
    assert!(j.detail.contains("buffer") && j.detail.contains("recursion"), "{}", j.detail);

    // Whole-word only: "enumerate" must NOT fire the "enum" rule.
    assert!(
        !has(&check("It goes through each item to enumerate the ones that match what you asked for here."), "plain words"),
        "enumerate is not enum"
    );
}

#[test]
fn a_very_long_explanation_is_advisory() {
    let long = "word ".repeat(140);
    assert!(has(&check(&long), "kept short"));
}

#[test]
fn a_clean_explanation_reads_plainly_and_is_not_called_correct() {
    let f = check(PLAIN);
    assert!(f.is_empty(), "PLAIN should be clean: {f:?}");
    let said = spoken(&f);
    assert!(said.to_lowercase().contains("reads as a plain-english explanation"), "{said}");
    assert!(said.to_lowercase().contains("for you to judge"), "must not claim it's accurate: {said}");
    assert!(blocking(&f).is_empty());
}

// --- depth control ---------------------------------------------------------

#[test]
fn the_depth_dial_is_read_from_the_words() {
    assert_eq!(Depth::from_words("explain it like i'm five"), Depth::Simple);
    assert_eq!(Depth::from_words("simply please"), Depth::Simple);
    assert_eq!(Depth::from_words("explain in more detail"), Depth::Technical);
    assert_eq!(Depth::from_words("technically what happens"), Depth::Technical);
    assert_eq!(Depth::from_words("just explain the counter"), Depth::Normal);
}

#[test]
fn jargon_blocks_at_simple_but_is_allowed_at_technical() {
    let with_jargon = "It stores the value in a buffer and walks the structure using recursion so \
                       nothing is lost along the way as it goes.";
    // Simple: no jargon allowed — it blocks.
    let s = check_at(with_jargon, Depth::Simple);
    let j = s.iter().find(|x| x.rule == "plain words").expect("jargon caught at Simple");
    assert_eq!(j.severity, Severity::Blocking, "at 'like I'm five', jargon must block");
    // Technical: precise terms are allowed — the rule doesn't fire.
    assert!(!has(&check_at(with_jargon, Depth::Technical), "plain words"), "technical allows terms");
    // Normal: worth flagging, but not blocking.
    let n = check_at(with_jargon, Depth::Normal);
    assert_eq!(n.iter().find(|x| x.rule == "plain words").unwrap().severity, Severity::Advisory);
}

#[test]
fn technical_gets_more_room_than_simple() {
    let medium = "word ".repeat(100); // 100 words: over Simple's cap (90), under Technical's (220)
    assert!(has(&check_at(&medium, Depth::Simple), "kept short"), "100 words is long for Simple");
    assert!(!has(&check_at(&medium, Depth::Technical), "kept short"), "100 words is fine for Technical");
}

// --- the rewrite loop ------------------------------------------------------

fn blocker() -> Vec<Finding> {
    vec![Finding { severity: Severity::Blocking, rule: "words not code".into(), detail: "code".into() }]
}

#[test]
fn the_loop_rewrites_until_it_reads_plainly() {
    let llm = MockLlm("It keeps a running total of the numbers you give it.".into());
    let calls = std::cell::Cell::new(0u32);
    let outcome = explain_loop("fn add() {}", &llm, 3, Depth::Normal, |_t| {
        let n = calls.get();
        calls.set(n + 1);
        if n == 0 {
            blocker()
        } else {
            vec![]
        }
    });
    match outcome {
        Outcome::Explained { rounds, .. } => assert_eq!(rounds, 1, "one rewrite"),
        other => panic!("expected Explained, got {other:?}"),
    }
}

#[test]
fn a_problem_that_survives_the_budget_is_a_struggle() {
    let llm = MockLlm("still not plain".into());
    let outcome = explain_loop("code", &llm, 2, Depth::Normal, |_| blocker());
    match outcome {
        Outcome::Struggled { rounds, findings, .. } => {
            assert_eq!(rounds, 2);
            assert!(findings.iter().any(|f| f.severity == Severity::Blocking));
        }
        other => panic!("expected Struggled, got {other:?}"),
    }
}

#[test]
fn no_model_output_is_no_draft() {
    let llm = MockLlm("   ".into());
    assert!(matches!(explain_loop("code", &llm, 3, Depth::Normal, |_| vec![]), Outcome::NoDraft(_)));
}

#[test]
fn in_plain_english_hands_back_the_explanation_for_auto_explain() {
    // The helper the build/improve paths use so generated code arrives with a
    // plain-English summary.
    let llm = MockLlm("This keeps a running total of the numbers you give it and remembers the sum so far.".into());
    let got = atlas::explain::in_plain_english("fn total() {}", &llm, 2);
    assert!(got.as_deref().map(|s| s.to_lowercase().contains("total")).unwrap_or(false), "{got:?}");
}

#[test]
fn in_plain_english_is_none_when_the_model_says_nothing() {
    let llm = MockLlm("   ".into());
    assert!(atlas::explain::in_plain_english("code", &llm, 2).is_none());
}

// --- end to end through the daemon -----------------------------------------

#[test]
fn explain_routes_through_the_daemon() {
    let dir = std::env::temp_dir().join(format!("atlas-explain-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    // No model in the test daemon → the handler's graceful branch, which still
    // proves the phrase routed to explain_code and the dispatch arm ran. A plain
    // string literal so the coverage guard can parse it.
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("explain the code that keeps a running total", 100);
    assert!(reply.to_lowercase().contains("model"), "with no model it should say it needs one: {reply}");
    // Routed to explain_code as its own branch, not the catch-all: a nonsense
    // line does not get the same answer.
    let junk = d.turn("zzqx frobnicate wibble", 100);
    assert_ne!(reply, junk, "the explain phrase must route to a branch of its own");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explaining_a_change_waiting_to_be_implemented_finds_it() {
    let dir = std::env::temp_dir().join(format!("atlas-explain-q-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    // Seed a ready change into the store the daemon loads.
    let store = Store::new(dir.clone());
    let mut w = atlas::workshop::Workshop::load(&store);
    w.propose(
        "Roofing",
        "the date parser",
        "reads dates from quotes",
        vec![atlas::workshop::FileEdit { path: "dates.rs".into(), content: "date parsing goes here".into() }],
        true,
        "ok",
        1,
    );
    let _ = w.save(&store);

    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));

    // No model, but it must have FOUND the queued change (→ needs a model),
    // not fallen through to "nothing waiting".
    let reply = d.turn("explain the change waiting to be implemented", 100);
    assert!(reply.to_lowercase().contains("model"), "should have found the change and asked for a model: {reply}");
    assert!(!reply.to_lowercase().contains("nothing waiting"), "it exists, so not 'nothing waiting': {reply}");
    // Finding the queued change is a real branch, distinct from the catch-all.
    let junk = d.turn("zzqx frobnicate wibble", 100);
    assert_ne!(reply, junk, "the found-change answer is its own branch");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn explaining_a_queued_change_when_there_is_none_says_so() {
    let dir = std::env::temp_dir().join(format!("atlas-explain-none-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));

    let reply = d.turn("explain the change waiting to be implemented", 100);
    assert!(reply.to_lowercase().contains("nothing waiting"), "no queued change → says so: {reply}");
    // The 'nothing waiting' answer is its own branch, not the catch-all.
    let junk = d.turn("zzqx frobnicate wibble", 100);
    assert_ne!(reply, junk, "the empty-queue answer is a branch of its own");
    let _ = std::fs::remove_dir_all(&dir);
}
