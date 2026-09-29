//! **A question put twice in the same thread is answered like it was heard
//! the first time.**
//!
//! `thread::asked_before` was built and unit-tested but nothing in the daemon
//! asked it, so repeating a question got the same answer replayed verbatim --
//! the small thing its own doc says "makes an assistant feel like it isn't
//! listening". `run_command` now calls it right after the turn is appended:
//! the just-added turn is skipped, an earlier identical `said` is looked for,
//! and when one is found the reply leads with a short acknowledgement instead
//! of pretending the question is new.
//!
//! This drives it through the real turn path (`Daemon::turn`), which is the
//! honest home -- the same code a spoken or typed turn runs.

use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-twice-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

/// The one monitor every mock daemon here is handed.
fn one_screen() -> Vec<Monitor> {
    vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]
}

/// The lead-in the wire adds. Kept here as prose so no test string is shaped
/// like a call.
const LEAD_IN: &str = "You asked this a little earlier";

#[test]
fn the_second_time_the_same_question_is_asked_it_is_acknowledged() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(one_screen());
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(tmp("second")),
        Proactive::new(ProactiveConfig::default()),
    );

    // A question long enough for `asked_before` to judge (its normaliser
    // ignores anything under eight characters, so "hi" would never trip it)
    // and one whose answer is self-contained -- it does not leave a question
    // hanging that the next turn would be read as answering.
    let question = "how are you doing this fine morning";

    let first = d.turn(question, 100);
    assert!(
        !first.starts_with(LEAD_IN),
        "the first time a question is asked it must not be treated as a \
         repeat:\n{first}"
    );

    let second = d.turn(question, 200);
    assert!(
        second.starts_with(LEAD_IN),
        "asking the same question again should be acknowledged, not replayed \
         as if new:\n{second}"
    );
}

#[test]
fn a_different_question_is_not_flagged_as_a_repeat() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(one_screen());
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(tmp("different")),
        Proactive::new(ProactiveConfig::default()),
    );

    let _ = d.turn("how are you doing this fine morning", 100);
    let other = d.turn("what is on my calendar for tomorrow morning", 200);
    assert!(
        !other.starts_with(LEAD_IN),
        "an unrelated question must not be mistaken for a repeat:\n{other}"
    );
}
