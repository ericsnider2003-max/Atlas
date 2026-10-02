//! "It can't seem to code still" (the owner, 2 Oct 2026). The fixes, driven
//! from the outside: the ways people actually ask for code reach the
//! builder; a coding agent installed here is offered first and a no to it
//! still gets the work done; a folder named in the sentence is where the
//! work goes and what the project is; "run it" asks before it runs; and a
//! file that looks cut off is never written over one of yours.
//!
//! The pure parts -- room to write a whole file, seeing a cut-off reply,
//! fix rounds as edits, which model writes, packages, folders -- are tested
//! beside the code in `build_it`. Nothing here needs a toolchain or a real
//! model: the crew errands these start are not waited on.

use atlas::brain::{Llm, MockLlm};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-real-code-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn parser() -> Parser {
    Parser::new(&cfg().commands)
}

fn no_agent(_: &str) -> Option<String> {
    None
}

fn claude_only(p: &str) -> Option<String> {
    (p == "claude").then(|| "/opt/bin/claude".to_string())
}

#[test]
fn the_everyday_ways_of_asking_for_code_reach_the_builder() {
    let p = parser();
    for said in [
        "write a python script that renames my photos by date",
        "create a script to back up my notes folder",
        "make me a tool that converts csv files to json",
        "make me a little app that tracks my water intake",
        "write code to parse my bank statements",
        "code me a scraper for concert listings",
        "can you write a python script that sorts my downloads",
        "create a program that times my pomodoros",
    ] {
        assert!(matches!(p.parse(said), Intent::Build(_)), "{said:?} should build, got {:?}", p.parse(said));
    }
    assert!(matches!(p.parse("add a feature to my app that exports to csv"), Intent::Improve(_)));
    assert!(matches!(p.parse("run it"), Intent::RunBuild(_)));
}

#[test]
fn the_new_phrases_take_nothing_from_their_neighbours() {
    let p = parser();
    // Words, not code.
    for said in ["write a note to call mum", "write a script for my youtube video", "make me a coffee", "create a reminder for friday"] {
        assert!(!matches!(p.parse(said), Intent::Build(_)), "{said:?} is not code, got {:?}", p.parse(said));
    }
    assert!(matches!(p.parse("make me a friend link"), Intent::Friend(_)));
    assert!(matches!(p.parse("create an account on github"), Intent::CreateAccount(_)));
    assert!(matches!(p.parse("add Sam to the Friends group"), Intent::ChangeGroup(_)));
    // "run it by me" is conversation, not a run.
    assert!(!matches!(p.parse("run it by me again"), Intent::RunBuild(_)));
}

#[test]
fn a_folder_first_then_the_work_is_the_work_in_that_folder() {
    let p = parser();
    match p.parse(r"in C:\code\app, add a dark mode toggle") {
        Intent::Improve(w) => assert!(w.contains(r"C:\code\app"), "the folder stays in the words: {w}"),
        other => panic!("expected a project change, got {other:?}"),
    }
    assert!(matches!(p.parse(r"write a python script that renames photos and save it to D:\tools"), Intent::Build(_)));
}

#[test]
fn an_installed_coding_agent_is_offered_first_and_a_no_still_gets_it_written() {
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```python\nprint('hi')\n```".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("agent-ask")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(claude_only);
    let asked = d.turn("write a python script that prints hello", 100);
    assert!(asked.contains("Claude Code") && asked.contains("Say yes"), "the agent is offered first: {asked}");
    let after_no = d.turn("no", 101);
    assert!(after_no.contains("On it") && after_no.contains("Python"), "a no to the agent is a yes to writing it: {after_no}");
}

#[test]
fn with_no_agent_atlas_writes_it_in_python_and_says_who_writes_it() {
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```python\nprint('hi')\n```".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("own")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(no_agent);
    let reply = d.turn("make me a tool that totals my receipts", 100);
    assert!(reply.contains("Python"), "Python when no language is named: {reply}");
    assert!(reply.contains("the model on this computer"), "says which model writes it: {reply}");
}

#[test]
fn the_coding_agent_can_be_switched_off() {
    let (mut c, p) = (cfg(), plat());
    if let Some(t) = c.tools.as_mut() {
        t.build.coding_agent = atlas::build_it::AgentUse::Off;
    }
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```python\nprint('hi')\n```".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("agent-off")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(claude_only);
    let reply = d.turn("write a python script that prints hello", 100);
    assert!(!reply.contains("Claude Code"), "off means not offered: {reply}");
}

#[test]
fn a_project_named_by_its_folder_is_registered_with_its_own_language() {
    let (c, p) = (cfg(), plat());
    let proj = tmp("proj").join("receipts-app");
    std::fs::create_dir_all(&proj).unwrap();
    std::fs::write(proj.join("pyproject.toml"), "[project]\nname = \"receipts\"\n").unwrap();
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```python\nprint('hi')\n```".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("proj-store")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(no_agent);
    let reply = d.turn(&format!("in {}, add a feature that prints the version", proj.display()), 100);
    assert!(reply.contains("receipts-app"), "the folder names the project, not \"my\": {reply}");
    assert!(reply.contains("Python"), "the project's own language: {reply}");
    let registered = d.workshop.resolve("receipts-app").map(|p| p.folder.clone());
    assert_eq!(registered.as_deref(), Some(proj.to_string_lossy().as_ref()));
}

#[test]
fn my_project_is_not_a_project_called_my() {
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```python\nprint('hi')\n```".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("my")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(no_agent);
    let reply = d.turn("add a feature to my app in my project", 100);
    assert!(reply.contains("Which project"), "asks rather than inventing \"my\": {reply}");
    assert!(d.workshop.resolve("my").is_none());
}

#[test]
fn a_change_that_looks_cut_off_is_never_written_over_your_file() {
    let (c, p) = (cfg(), plat());
    let proj = tmp("cutoff-proj");
    std::fs::write(proj.join("main.py"), "def total(xs):\n    return sum(xs)\n").unwrap();
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("ok".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("cutoff-store")), Proactive::new(ProactiveConfig::default()));
    d.workshop.register("Ledger", &proj.to_string_lossy(), 1);
    let half = atlas::workshop::FileEdit { path: "main.py".into(), content: "def total(xs):\n    return sum([\n        x for x in xs".into() };
    d.workshop.propose("Ledger", "faster totals", "sum faster", vec![half], false, "", 2).unwrap();
    let reply = d.turn("implement faster totals", 100);
    assert!(reply.contains("cut off"), "says why it wasn't written: {reply}");
    assert_eq!(std::fs::read_to_string(proj.join("main.py")).unwrap(), "def total(xs):\n    return sum(xs)\n");
}

#[test]
fn run_it_asks_first_and_names_what_will_run() {
    let (c, p) = (cfg(), plat());
    let dir = tmp("run");
    let file = dir.join("hello.py");
    std::fs::write(&file, "print('hello')\n").unwrap();
    atlas::build_it::LastBuild { path: file.to_string_lossy().into_owned(), lang: atlas::craft::Lang::Python, built: true }.keep();
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("ok".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("run-store")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("run it", 100);
    if atlas::codetools::any_python(&atlas::roots::install_root()).is_some() {
        assert!(reply.contains("hello.py") && reply.contains("Say yes"), "asks first, naming the file: {reply}");
        assert!(!reply.contains("printed"), "nothing has run yet: {reply}");
    } else {
        assert!(reply.contains("no Python"), "{reply}");
    }
}

#[test]
fn only_your_yes_hands_work_to_the_agent() {
    let (c, p) = (cfg(), plat());
    let proj = tmp("yes-only");
    std::fs::write(proj.join("main.py"), "print(1)\n").unwrap();
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("ok".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("yes-only-store")), Proactive::new(ProactiveConfig::default()));
    d.find_coding_agents_with_for_test(claude_only);
    d.workshop.register("Tally", &proj.to_string_lossy(), 1);
    // The marker arriving any way but as the answer to the question --
    // a model's tool call, say -- is asked about, not run.
    let said = d.execute(&Intent::Improve(format!("{}on the Tally project, add a total", atlas::coding_agent::HAND_OVER)));
    assert!(said.contains("Say yes"), "asks rather than handing over: {said}");
    let said = d.execute(&Intent::RunBuild(atlas::build_it::RUN_CONFIRMED.into()));
    assert!(!said.starts_with("Running"), "a run is asked about first: {said}");
}
