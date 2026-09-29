//! Atlas's honest picture of its own remaining work.
//!
//! "What's left to finish" is three different jobs, and lumping them together
//! is how a status report lies: some things Atlas could wire itself, some are
//! built and tested but have never run on real hardware, and some wait on you
//! to install or provide something. `to_finish` classifies each by who can
//! finish it, and "work on yourself" with no goal reports it.

use atlas::capability::{self, Finisher};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-finish-{tag}-{}", std::process::id()));
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
fn the_backlog_separates_who_can_finish_each_thing() {
    let items = capability::to_finish();
    assert!(!items.is_empty(), "there is unfinished work to report");
    // Untested capabilities (built, never run for real) exist in this tree, so
    // there must be at least one "needs a run" item.
    assert!(
        items.iter().any(|u| u.finisher == Finisher::NeedsARun),
        "the built-but-never-run work is called out as needing a run"
    );
    // Blocked capabilities exist, so at least one waits on the user.
    assert!(
        items.iter().any(|u| u.finisher == Finisher::NeedsYou),
        "the install-blocked work is called out as waiting on you"
    );
}

#[test]
fn the_backlog_is_ordered_by_nearness_to_atlas_own_power() {
    // Mine (0) before NeedsARun (1) before NeedsYou (2): what Atlas could do
    // itself comes first, what needs you comes last.
    let rank = |f: Finisher| match f {
        Finisher::Mine => 0u8,
        Finisher::NeedsARun => 1,
        Finisher::NeedsYou => 2,
    };
    let items = capability::to_finish();
    let ranks: Vec<u8> = items.iter().map(|u| rank(u.finisher)).collect();
    let mut sorted = ranks.clone();
    sorted.sort();
    assert_eq!(ranks, sorted, "the backlog is ordered nearest-to-finishable first");
}

#[test]
fn the_report_names_the_split_honestly() {
    let report = capability::to_finish_report();
    assert!(report.to_lowercase().contains("left to finish"), "it's a completion report: {report}");
    // With Untested work present, it must say plainly that the rest needs a
    // real run — the honest reason Atlas can't just finish everything itself.
    assert!(
        report.to_lowercase().contains("run on real hardware") || report.to_lowercase().contains("never run"),
        "it names the real-hardware gap: {report}"
    );
}

#[test]
fn asking_whats_left_to_finish_reports_the_backlog_through_the_daemon() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("ask")), Proactive::new(ProactiveConfig::default()));
    // A plain question — a read, not an action — so it answers directly.
    let reply = d.turn("what's left to finish", 100);
    assert!(reply.to_lowercase().contains("left to finish"), "it reports the backlog: {reply}");
    assert!(
        reply.to_lowercase().contains("never run") || reply.to_lowercase().contains("run on real hardware"),
        "it names the real-hardware gap honestly: {reply}"
    );
    // And it's a real answer, not the catch-all miss.
    assert_ne!(reply.to_lowercase(), "i don't have anything for that.", "the question routed: {reply}");
}
