//! Atlas builds code from a description, and checks it before trusting it.
//!
//! The pure logic (generation, code extraction, the fix loop) is unit-tested
//! in `build_it`. These drive the whole thing through the daemon: the intent
//! parses and reaches the arm, the honest paths hold (no model → say so), and
//! a request is handed off to a background worker so the turn is not blocked
//! while the toolchain runs.

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
    let p = std::env::temp_dir().join(format!("atlas-build-{tag}"));
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

#[test]
fn asking_to_build_something_with_no_model_says_so_plainly() {
    let (c, p) = (cfg(), plat());
    // No LLM configured.
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("nomodel")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("write me a function that adds two numbers", 100);
    assert!(
        reply.to_lowercase().contains("model"),
        "with no model it should say it needs one, got: {reply}"
    );
}

#[test]
fn a_build_request_is_handed_off_to_run_in_the_background() {
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("```rust\nfn add(a:i32,b:i32)->i32{a+b}\n```".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("handoff")), Proactive::new(ProactiveConfig::default()));
    let reply = d.turn("write me a function that adds two numbers", 100);
    // The turn returns an acknowledgement rather than blocking on cargo; the
    // real drafting and checking happen in the crew errand.
    assert!(
        reply.to_lowercase().contains("check") || reply.to_lowercase().contains("write it")
            || reply.to_lowercase().contains("on it"),
        "a build should be acknowledged and run in the background, got: {reply}"
    );
    assert!(
        !reply.to_lowercase().contains("switched off"),
        "building ships on and should not report itself off: {reply}"
    );
}

#[test]
fn an_empty_build_request_asks_what_to_build() {
    let (c, p) = (cfg(), plat());
    let llm: Arc<dyn Llm> = Arc::new(MockLlm("code".into()));
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(tmp("empty")), Proactive::new(ProactiveConfig::default()));
    // "build me" with no description.
    let reply = d.turn("build me", 100);
    assert!(
        reply.to_lowercase().contains("what to build") || reply.to_lowercase().contains("tell me"),
        "an empty description should ask what to build, got: {reply}"
    );
}
