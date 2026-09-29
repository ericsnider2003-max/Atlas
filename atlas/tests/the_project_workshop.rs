//! Working on a project, through its hold queue.
//!
//! You tell Atlas to change a project; it scopes and builds the change (itself
//! or delegated), checks it, and files it as a *proposed* change in that
//! project's queue — titled, described, not yet applied. You implement it when
//! ready, by title. The workshop's own logic is unit-tested in `workshop`;
//! these drive the two intents through the daemon.

use atlas::brain::{Llm, MockLlm};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-workshop-{tag}"));
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

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str, llm: Option<Arc<dyn Llm>>) -> Daemon<'a> {
    Daemon::new(c, p, llm, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn improving_a_project_without_naming_one_asks_which() {
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```rust\nfn x() {}\n```".into()));
    let mut d = daemon(&c, &p, "which", Some(llm));
    let reply = d.turn("improve the date parsing", 100);
    assert!(
        reply.to_lowercase().contains("which project"),
        "with no project named it should ask which, got: {reply}"
    );
}

#[test]
fn improving_a_named_project_is_scoped_and_handed_off() {
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```rust\nfn add(a:i32,b:i32)->i32{a+b}\n```".into()));
    let mut d = daemon(&c, &p, "named", Some(llm));
    // Register a project so the name resolves.
    d.workshop.register("Atlas", "/tmp/atlas", 1);
    let reply = d.turn("on the Atlas project, add a function that adds numbers", 100);
    assert!(
        reply.to_lowercase().contains("atlas") && reply.to_lowercase().contains("queue"),
        "it should acknowledge scoping the change for the project's queue, got: {reply}"
    );
}

#[test]
fn implementing_an_unknown_title_says_so() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "impl-unknown", None);
    let reply = d.turn("implement the nonexistent thing", 100);
    assert!(
        reply.to_lowercase().contains("don't have a change"),
        "an unknown title should be reported, got: {reply}"
    );
}

#[test]
fn implementing_a_queued_change_writes_its_files_and_marks_it_done() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "impl", None);
    // A real folder to write into.
    let proj = tmp("proj-target");
    d.workshop.register("Atlas", proj.to_str().unwrap(), 1);
    d.workshop.propose(
        "Atlas",
        "add greeting",
        "adds a hello function",
        vec![atlas::workshop::FileEdit { path: "hello.rs".into(), content: "fn hello() {}\n".into() }],
        true,
        "checks passed",
        1,
    );
    let reply = d.turn("implement add greeting", 100);
    assert!(reply.to_lowercase().contains("implemented"), "should confirm, got: {reply}");
    // The file was actually written.
    assert!(proj.join("hello.rs").exists(), "the change's file should be on disk");
    // And it's out of the ready queue.
    assert_eq!(d.workshop.resolve("Atlas").unwrap().ready().len(), 0);
}

#[test]
fn implementing_a_change_with_no_folder_asks_where_the_project_is() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "nofolder", None);
    // Filed before the folder was known.
    d.workshop.propose(
        "Mystery",
        "do a thing",
        "does a thing",
        vec![atlas::workshop::FileEdit { path: "x.rs".into(), content: "// x".into() }],
        true,
        "",
        1,
    );
    let reply = d.turn("implement do a thing", 100);
    assert!(
        reply.to_lowercase().contains("where") && reply.to_lowercase().contains("mystery"),
        "it should ask where the project lives, got: {reply}"
    );
}
