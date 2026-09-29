//! "What could make you faster" names its own bottleneck.
//!
//! The `Recommend` intent builds `Observations` from the turn's timings and
//! used to speak only `recommend`/`ask`, which fall silent when nothing on
//! this machine is worth changing -- so the one question the intent exists to
//! answer ("what could make you faster") could come back as "Nothing I'm
//! missing." while a stage was, in fact, plainly the slowest.
//!
//! `wants::slowest` was built and tested for exactly this and had no caller.
//! The daemon's handler now leads the reply with it. This test holds that
//! wiring: given real per-stage timings, the answer names the slowest stage.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::timing::{Stage, Turn};
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-recommend-slowest-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn the_self_report_leads_with_the_slowest_stage() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(tmp("lead")),
        Proactive::new(ProactiveConfig::default()),
    );

    // Two real turns' worth of timings. Speaking is comfortably the slowest of
    // the stages the intent reads (transcribe / think / speak), so it is the
    // one an honest self-report has to name.
    for _ in 0..2 {
        let mut turn = Turn::default();
        turn.note(Stage::Hearing, 200);
        turn.note(Stage::Understanding, 400);
        turn.note(Stage::Speaking, 1800);
        d.timing.add(turn);
    }

    let reply = d.execute(&Intent::Recommend);

    // The measured bottleneck is named, mapped to its spoken stage name, and
    // carried in seconds -- 1800 ms of speaking reads as "1.8 seconds".
    assert!(
        reply.contains("slowest part of a turn is speak"),
        "the self-report did not name the slowest stage: {reply}"
    );
    assert!(
        reply.contains("1.8 seconds"),
        "the slowest stage's measured time was not carried through: {reply}"
    );
}

#[test]
fn with_no_timings_it_does_not_invent_a_bottleneck() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(tmp("empty")),
        Proactive::new(ProactiveConfig::default()),
    );

    // Nothing measured yet: `slowest` is None, and the reply must not claim a
    // slowest stage rather than making one up from an empty record.
    let reply = d.execute(&Intent::Recommend);
    assert!(
        !reply.contains("slowest part of a turn"),
        "named a bottleneck with no timings to name one from: {reply}"
    );
}
