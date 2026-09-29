//! The typed prompt used to answer six intents and say "not wired to an action
//! yet" for everything else. That sentence was true of the prompt, not of
//! Atlas — every one of those actions already existed in `daemon`.
//!
//! Found by running on Windows, which is the only place it was visible: the
//! test suite exercises `Daemon::execute` directly and so never went through
//! the path a person actually types into.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::{Intent, Parser};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-prompt-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

/// Everything the old prompt could not answer. Each of these parsed correctly
/// and then fell into the catch-all.
const WAS_UNREACHABLE_FROM_THE_PROMPT: &[&str] = &[
    "what's outstanding",
    "what's queued",
    "how's the machine",
    "pause",
    "resume",
    "undo that",
];

#[test]
fn the_prompt_reaches_the_same_actions_the_daemon_does() {
    let (c, p) = (cfg(), plat());
    let parser = Parser::new(&c.commands);
    let mut d = daemon(&c, &p, "reach");

    for line in WAS_UNREACHABLE_FROM_THE_PROMPT {
        let intent = parser.parse(line);
        assert!(
            !matches!(intent, Intent::Unknown(_)),
            "'{line}' no longer parses, so this test is measuring the wrong thing"
        );
        let out = d.execute(&intent);
        assert!(
            !out.contains("not wired to an action yet"),
            "'{line}' parsed as {intent:?} and still reports itself unwired"
        );
        assert!(!out.trim().is_empty(), "'{line}' answered with nothing at all");
    }
}

#[test]
fn asking_what_is_outstanding_answers_rather_than_describing_the_parse() {
    let (c, p) = (cfg(), plat());
    let parser = Parser::new(&c.commands);
    let mut d = daemon(&c, &p, "outstanding");
    let out = d.execute(&parser.parse("what's outstanding"));
    // The failure mode is a debug print of the Intent leaking to the user.
    assert!(!out.contains("Outstanding"), "the parse leaked into the answer: {out}");
    assert!(!out.contains("parsed "), "{out}");
}
