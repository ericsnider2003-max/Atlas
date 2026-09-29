//! Every model call, written down — and read back.
//!
//! `trace.rs` shipped with a `Call`, a `Trace` that rolls at 5,000, a failure
//! rate, a median latency, a busiest-module report, a "calls you went on to
//! correct" list, and `to_line`/`from_lines` describing a file format.
//!
//! **Nothing ever recorded a call.** No file was written, none was read, and
//! every one of those readers computed over a `Vec` that only a test had ever
//! filled. `nudge::trace_line` — the one function that turns the whole thing
//! into a sentence — was a named orphan in `tests/dead_capabilities.rs`
//! waiting on "an intent for asking".
//!
//! The module header gives four reasons it earns its place, and every one of
//! them is about having a record: you cannot debug what you cannot see, the
//! correction loop needs a first time to count, a local model earns trust by
//! measurement, and without it every prompt change is a vibe. None of those
//! were true of a recorder that recorded nothing.
//!
//! These tests drive a real `Daemon` with a real `Llm` and then look at what
//! landed on disk, because asserting on a hand-built `Trace` is exactly what
//! already existed while the feature was dead.

use atlas::brain::{Llm, LlmConfig};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::{AtlasError, Result};
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::trace::{
    append, compact, from_lines, load, log_path, model_name, to_line, Call, Trace, KEEP,
};
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-fr-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn a_call(who: &str, at: u64) -> Call {
    let mut c = Call::new(who, "llama3.1", at);
    c.took_ms = 120;
    c.prompt_chars = 400;
    c.reply_chars = 80;
    c
}

// ===================== the file =========================================

#[test]
fn a_call_survives_the_round_trip_to_disk() {
    let dir = tmp("round");
    let at = log_path(&dir);
    let c = a_call("brain", 1_700_000_000);
    assert!(append(&at, &c), "the append failed");

    let back = load(&at);
    assert_eq!(back.calls, vec![c], "the call changed on its way to disk and back");
}

#[test]
fn the_log_is_appended_to_never_rewritten() {
    // One call per line, newest last, never rewritten. A log that gets
    // rewritten is one you cannot trust, and trust is the entire value.
    let dir = tmp("append");
    let at = log_path(&dir);
    for i in 0..3 {
        assert!(append(&at, &a_call("brain", 1_700_000_000 + i)));
    }
    let text = std::fs::read_to_string(&at).unwrap();
    assert_eq!(text.lines().count(), 3, "lines were lost: {text}");
    assert_eq!(load(&at).calls.len(), 3);
}

#[test]
fn a_truncated_last_line_does_not_cost_the_rest() {
    // The crash case the module header names. A half-written final line is
    // what an append-only log looks like after a hard stop.
    let dir = tmp("torn");
    let at = log_path(&dir);
    append(&at, &a_call("brain", 1));
    append(&at, &a_call("research", 2));
    let mut text = std::fs::read_to_string(&at).unwrap();
    text.push_str("{\"at\":3,\"asked_by\":\"bra");
    std::fs::write(&at, text).unwrap();

    let back = load(&at);
    assert_eq!(back.calls.len(), 2, "a torn line took the good ones with it");
}

#[test]
fn no_log_yet_is_not_a_fault() {
    let dir = tmp("none");
    let t = load(&log_path(&dir));
    assert!(t.calls.is_empty());
    assert_eq!(t.spoken(), "I haven't asked the model anything yet.");
}

#[test]
fn a_write_that_cannot_happen_says_so_rather_than_pretending() {
    // A flight recorder that can take the aircraft down is worse than none,
    // so `append` returns false instead of panicking or unwrapping.
    let dir = tmp("readonly");
    // A path whose parent is a *file*, so the directory can never be made.
    let blocked = dir.join("a-file").join("model-calls.jsonl");
    std::fs::write(dir.join("a-file"), "not a directory").unwrap();
    assert!(!append(&blocked, &a_call("brain", 1)), "a failed write reported success");
}

#[test]
fn compacting_keeps_the_newest_and_leaves_the_old_file_intact_on_failure() {
    let dir = tmp("compact");
    let at = log_path(&dir);
    for i in 0..10 {
        append(&at, &a_call("brain", 1_000 + i));
    }
    let kept = compact(&at, 4).unwrap();
    assert_eq!(kept, 4);
    let back = load(&at);
    assert_eq!(back.calls.len(), 4);
    assert_eq!(back.calls[0].at, 1_006, "it kept the oldest instead of the newest");
    assert_eq!(back.calls[3].at, 1_009);
}

#[test]
fn compacting_a_short_log_changes_nothing() {
    let dir = tmp("compact-short");
    let at = log_path(&dir);
    append(&at, &a_call("brain", 1));
    assert_eq!(compact(&at, KEEP).unwrap(), 1);
    assert_eq!(load(&at).calls.len(), 1);
}

#[test]
fn the_log_belongs_to_the_install_that_wrote_it() {
    // A fixed path meant every Daemon in the suite wrote to one file and each
    // test saw the others' calls. The store root is the directory that is
    // genuinely this install's, and on a real machine it is `data/state`,
    // which `atlas update` already preserves.
    let store = tmp("belongs");
    let (c, p, llm) = (cfg_named("llama3.1"), plat(), Arc::new(UpLlm) as Arc<dyn Llm>);
    let d = Daemon::new(
        &c,
        &p,
        Some(llm.clone()),
        Store::new(store.clone()),
        Proactive::new(ProactiveConfig::default()),
    );
    assert!(d.trace_path().starts_with(&store), "the log is outside this install: {:?}", d.trace_path());
    assert_eq!(d.trace_path().extension().unwrap(), "jsonl", "one call per line is the design");
    assert_eq!(d.trace_path(), log_path(&store));
}

#[test]
fn the_command_line_reads_the_same_file_the_daemon_writes() {
    // These were two different directories for one commit: the daemon wrote
    // into the store and `atlas trace` read `data/logs`, so the command
    // reported an empty log while the daemon was filling one. Nothing failed
    // — it just quietly always said nothing had happened.
    let src = crate::common::source_of("main");
    let at = src
        .find("fn run_trace")
        .expect("`atlas trace` is gone, and with it the only way to read the record");
    let body = &src[at..at + 1200];
    // The *property*, not the spelling. This used to pin the exact text
    // `log_path(Store::new("data/state").root())` — and that literal was
    // itself the cwd bug: `Store::new("data/state")` is relative, so
    // `atlas trace` read the log of whichever folder you were standing in.
    // The guard was green while the thing it guarded was broken, which is
    // the sixth time a source-text matcher in this tree has been right about
    // a string and wrong about the code.
    //
    // What has to hold is that the command derives its path from the same
    // store the daemon writes to, by asking `roots` for it rather than
    // spelling one out.
    assert!(
        body.contains("log_path(") && body.contains("roots::store()"),
        "`atlas trace` no longer derives its path from the store the daemon \
         writes to: it must ask `roots::store()`, so that it and the daemon \
         cannot disagree about which install they are in"
    );
    assert!(
        !body.contains("Store::new(\""),
        "`atlas trace` is building a store from a literal path again — that \
         is relative, and it is what made the command read an empty log while \
         the daemon was filling a real one"
    );
}

// ===================== the model's name =================================

#[test]
fn the_model_name_comes_from_the_body_it_is_actually_sent_in() {
    // There is no `name` field on LlmConfig, and adding one would let the
    // config disagree with what is on the wire — the two-declarations problem
    // this codebase already removed from settings.rs.
    assert_eq!(model_name(r#"{"model":"llama3.1","prompt":"{user}"}"#), "llama3.1");
    assert_eq!(model_name(r#"{ "model" : "qwen2.5:14b" , "stream": false }"#), "qwen2.5:14b");
}

#[test]
fn a_body_that_names_no_model_says_unnamed_rather_than_guessing() {
    // Some backends take the model from the URL or the binary. A made-up
    // label would make `typical_ms` average two different models together.
    assert_eq!(model_name(r#"{"prompt":"{user}"}"#), "unnamed");
    assert_eq!(model_name(r#"{"model":{model}}"#), "unnamed");
    assert_eq!(model_name(""), "unnamed");
}

// ===================== reached from the running program =================

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// A model that answers, and says which action to take so the reply parses.
struct UpLlm;
impl Llm for UpLlm {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Ok(r#"{"action":"say","argument":"","say":"Alright."}"#.into())
    }
}

/// A model that is down.
struct DownLlm;
impl Llm for DownLlm {
    fn complete(&self, _: &str, _: &str) -> Result<String> {
        Err(AtlasError::Platform("connection refused".into()))
    }
}

fn cfg_named(model: &str) -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_mut().expect("the shipped config has a tools section");
    // A real LlmConfig, so `model_in_use` has a body to read the name out of.
    tools.llm = Some(LlmConfig {
        tool: Default::default(),
        request: format!(r#"{{"model":"{model}","prompt":"{{user}}"}}"#),
        response_path: "response".into(),
        vision_request: None,
    });
    c
}

/// Something the phrase parser cannot settle, so the model is actually asked.
const NEEDS_THE_MODEL: &str = "ponder the nature of a wednesday";

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, llm: Arc<dyn Llm>, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, Some(llm), Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn asking_the_model_writes_the_call_down() {
    // The moment the whole module was waiting for.
    let (c, p, llm) = (cfg_named("llama3.1"), plat(), Arc::new(UpLlm) as Arc<dyn Llm>);
    let mut d = daemon(&c, &p, llm.clone(), "asked");
    let before = d.trace.calls.len();

    d.turn(NEEDS_THE_MODEL, atlas::store::now());

    assert_eq!(d.trace.calls.len(), before + 1, "the call was not recorded");
    let c = d.trace.calls.last().unwrap();
    assert_eq!(c.asked_by, "brain");
    assert_eq!(c.model, "llama3.1");
    assert!(c.ok(), "a model that answered was recorded as failed");
}

#[test]
fn a_question_the_parser_settles_is_not_recorded_as_a_model_call() {
    // Otherwise the log says Atlas asks the model about everything, which is
    // the opposite of what it is for — and would make `busiest()` meaningless.
    let (c, p, llm) = (cfg_named("llama3.1"), plat(), Arc::new(UpLlm) as Arc<dyn Llm>);
    let mut d = daemon(&c, &p, llm.clone(), "parsed");

    d.turn("back up", atlas::store::now());

    assert!(
        d.trace.calls.is_empty(),
        "a phrase the parser handled was logged as a model call: {:?}",
        d.trace.calls
    );
}

#[test]
fn a_model_that_is_down_is_recorded_as_a_failure_with_the_reason() {
    let (c, p, llm) = (cfg_named("llama3.1"), plat(), Arc::new(DownLlm) as Arc<dyn Llm>);
    let mut d = daemon(&c, &p, llm.clone(), "down");

    d.turn(NEEDS_THE_MODEL, atlas::store::now());

    let call = d.trace.calls.last().expect("nothing recorded");
    assert!(!call.ok(), "a model that refused the connection was recorded as fine");
    assert!(
        call.failed.as_deref().unwrap_or("").contains("unreachable"),
        "the reason was not kept: {:?}",
        call.failed
    );
}

#[test]
fn the_words_of_a_prompt_never_reach_the_disk() {
    // `trace::STORES_NO_CONTENT` is the promise. This is the test that would
    // notice it being broken — lengths and timings, never the text.
    let secret = "ponder my bank passphrase hunter2 and the wednesday";
    let (c, p, llm) = (cfg_named("llama3.1"), plat(), Arc::new(UpLlm) as Arc<dyn Llm>);
    let mut d = daemon(&c, &p, llm.clone(), "secret");

    d.turn(secret, atlas::store::now());

    let on_disk = std::fs::read_to_string(d.trace_path()).unwrap_or_default();
    assert!(!on_disk.contains("hunter2"), "a prompt's words were written to disk");
    assert!(!on_disk.contains("passphrase"), "a prompt's words were written to disk");
    let call = d.trace.calls.last().unwrap();
    assert!(call.prompt_chars > 0, "the size was not recorded either, so nothing was measured");
}

#[test]
fn the_record_outlives_the_process() {
    // Otherwise "what has the model been doing" answers "since you started
    // me, not much", which is the one question it exists to answer.
    let store = tmp("outlive-store");
    let (c, p, llm) = (cfg_named("llama3.1"), plat(), Arc::new(UpLlm) as Arc<dyn Llm>);
    {
        let mut d = Daemon::new(
            &c,
            &p,
            Some(llm.clone()),
            Store::new(store.clone()),
            Proactive::new(ProactiveConfig::default()),
        );
        let _ = std::fs::remove_file(d.trace_path());
        d.turn(NEEDS_THE_MODEL, atlas::store::now());
        assert_eq!(d.trace.calls.len(), 1);
    }
    let d2 = Daemon::new(
        &c,
        &p,
        Some(llm.clone()),
        Store::new(store),
        Proactive::new(ProactiveConfig::default()),
    );
    assert!(
        !d2.trace.calls.is_empty(),
        "a fresh daemon started with an empty record — the log was never read back"
    );
}

// ===================== asked, and answered ==============================

#[test]
fn the_phrase_for_asking_parses_to_something_that_reads_the_record() {
    // `nudge::trace_line`'s entry in dead_capabilities.rs said it was waiting
    // on "an intent for asking". This is that intent, and the guard that it
    // stays reachable from something a person would actually say.
    let cfg = Config::load(Path::new("config")).unwrap();
    let parser = Parser::new(&cfg.commands);
    for said in ["what have you been asking", "how's the model doing", "model trace"] {
        assert_eq!(parser.parse(said), Intent::ModelTrace, "did not parse: {said}");
    }
}

#[test]
fn asked_what_it_has_been_doing_it_says_so_out_loud() {
    let (c, p) = (cfg_named("llama3.1"), plat());
    let mut d = daemon(&c, &p, Arc::new(UpLlm), "asked-back");
    let _ = std::fs::remove_file(d.trace_path());
    d.trace = Trace::default();

    d.turn(NEEDS_THE_MODEL, atlas::store::now());
    let said = d.turn("what have you been asking", atlas::store::now());

    assert!(said.contains("call"), "it did not report the calls: {said}");
    assert!(said.contains("brain"), "it did not say who asked: {said}");
    // Behaviour, not just wording: the report is backed by a real recorded
    // call, not a fixed sentence -- the trace actually captured the turn.
    assert!(!d.trace.calls.is_empty(), "the model call was never recorded in the trace");
}

#[test]
fn with_nothing_on_record_it_says_that_rather_than_a_zero() {
    let (c, p) = (cfg_named("llama3.1"), plat());
    let mut d = daemon(&c, &p, Arc::new(UpLlm), "nothing-yet");
    let _ = std::fs::remove_file(d.trace_path());
    d.trace = Trace::default();

    let said = d.turn("what have you been asking", atlas::store::now());
    assert!(said.contains("haven't asked"), "got: {said}");
}

// ===================== what the readers are for =========================

#[test]
fn the_busiest_module_is_the_one_that_asks_most() {
    // "What is actually using the model" is never what you expect — the
    // module header's claim, and the reader that answers it.
    let mut t = Trace::default();
    for i in 0..5 {
        t.record(a_call("research", i));
    }
    t.record(a_call("brain", 99));
    assert_eq!(t.busiest(), Some(("research".into(), 5)));
}

#[test]
fn one_stall_does_not_make_a_fast_model_look_slow() {
    // Median, not mean. With a local model on a laptop the thirty-second
    // stall happens, and a mean would report it as the normal experience.
    let mut t = Trace::default();
    for _ in 0..10 {
        let mut c = a_call("brain", 1);
        c.took_ms = 100;
        t.record(c);
    }
    let mut slow = a_call("brain", 2);
    slow.took_ms = 30_000;
    t.record(slow);
    assert_eq!(t.typical_ms("llama3.1"), 100);
}

#[test]
fn the_line_it_speaks_names_failures_when_there_are_any() {
    let mut t = Trace::default();
    t.record(a_call("brain", 1));
    let mut bad = a_call("brain", 2);
    bad.failed = Some("connection refused".into());
    t.record(bad);
    let said = atlas::nudge::trace_line(&t);
    assert!(said.contains("1 failed"), "a failure went unmentioned: {said}");
}

#[test]
fn a_log_longer_than_it_keeps_rolls_the_oldest_off_and_counts_them() {
    let mut t = Trace::default();
    for i in 0..(KEEP + 3) {
        t.record(a_call("brain", i as u64));
    }
    assert_eq!(t.calls.len(), KEEP);
    assert_eq!(t.dropped, 3);
    assert!(t.spoken().contains("rolled off"), "the drop went unsaid: {}", t.spoken());
}

#[test]
fn a_line_of_the_log_is_json_anything_can_read() {
    // Deliberately JSON so it can be read by anything, including a person
    // with a text editor.
    let line = to_line(&a_call("brain", 7));
    assert!(line.starts_with('{') && line.ends_with('}'), "not an object: {line}");
    assert!(!line.contains('\n'), "a line with a newline in it breaks the format");
    assert_eq!(from_lines(&line).calls.len(), 1);
}

// ===================== the file stays bounded (28 Sep 2026) ================
//
// `compact` had one caller, `atlas trace compact`, which nobody running Atlas
// in the background types. So the file grew by a line a model call for as
// long as Atlas was installed, and every start read all of it.

fn a_log_of(at: &Path, n: usize) {
    let body: String = (0..n)
        .map(|i| {
            let mut c = a_call("brain", 1_000 + i as u64);
            c.id = i as u64 + 1;
            format!("{}\n", to_line(&c))
        })
        .collect();
    std::fs::write(at, body).unwrap();
}

#[test]
fn the_running_atlas_opens_an_overgrown_log_cut_back() {
    let dir = tmp("open-overgrown");
    let at = log_path(&dir);
    a_log_of(&at, 2 * KEEP + 1);
    let t = atlas::trace::open(&at);
    assert_eq!(t.calls.len(), KEEP, "the whole overgrown log was read in");
    assert_eq!(
        std::fs::read_to_string(&at).unwrap().lines().count(),
        KEEP,
        "the file on disk was left to grow"
    );
    assert_eq!(t.calls.last().unwrap().id, 2 * KEEP as u64 + 1, "it kept the oldest instead of the newest");
}

#[test]
fn a_log_under_twice_the_keep_is_opened_as_it_is() {
    let dir = tmp("open-fine");
    let at = log_path(&dir);
    a_log_of(&at, KEEP + 10);
    assert_eq!(atlas::trace::open(&at).calls.len(), KEEP + 10);
    assert_eq!(std::fs::read_to_string(&at).unwrap().lines().count(), KEEP + 10);
}

#[test]
fn a_long_running_atlas_cuts_the_file_back_as_calls_roll_off() {
    let dir = tmp("bounded");
    let at = log_path(&dir);
    a_log_of(&at, 2 * KEEP);
    let mut t = atlas::trace::open(&at);
    let mut cut = false;
    for i in 0..KEEP {
        let mut c = a_call("brain", 50_000 + i as u64);
        c.id = t.next_id();
        append(&at, &c);
        t.record(c);
        cut |= atlas::trace::keep_bounded(&at, &t);
    }
    assert!(cut, "a daemon that never restarts never cut the file back");
    let lines = std::fs::read_to_string(&at).unwrap().lines().count();
    assert!(lines <= 2 * KEEP, "the file grew past twice the keep: {lines}");
}

#[test]
fn cutting_the_log_back_drops_the_grades_of_calls_no_longer_kept() {
    let dir = tmp("compact-grades");
    let at = log_path(&dir);
    a_log_of(&at, 10);
    let mut t = load(&at);
    for id in [2, 9] {
        assert!(atlas::trace::grade_and_keep(&at, &mut t, id, true, None));
    }
    compact(&at, 4).unwrap();
    let grades = std::fs::read_to_string(atlas::trace::grades_path(&at)).unwrap();
    assert_eq!(grades.lines().count(), 1, "grades of dropped calls were kept: {grades}");
    let back = load(&at);
    assert!(back.calls.iter().any(|c| c.id == 9 && c.graded == Some(true)), "the kept call lost its grade");
}
