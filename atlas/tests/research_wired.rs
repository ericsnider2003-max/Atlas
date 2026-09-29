//! Research, actually reachable.
//!
//! `research.rs` was complete — search, fetch, HTML stripping, per-source
//! tracking, note saving, all with tests — and `Daemon::execute` answered
//! "Research isn't built yet, so I can't look into X". `config/tools.yaml`
//! had a working search tool (curl against DuckDuckGo) and fetch tool
//! (headless Chrome) configured the whole time. The only missing piece was a
//! function calling it.
//!
//! That message is the honest half of the `hollow.rs` pattern — a catch-all
//! telling the truth about itself while nobody listens. It is worse than a
//! silent stub in one way: it taught its user the feature did not exist.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-rw-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

/// The shipped config with research switched on.
///
/// Research ships *off* — it is the one thing in this tree that leaves the
/// machine, so it waits to be asked for. Every test below about what
/// research *does* therefore has to turn it on itself. They used to lean on
/// the shipped file having it on, which meant a test about the daemon's
/// failure messages would start failing the day someone changed a default,
/// and would blame the daemon when it did. That is exactly what happened.
fn cfg_researching() -> Config {
    let mut c = cfg();
    if let Some(t) = c.tools.as_mut() {
        t.research.enabled = true;
    }
    c
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

#[test]
fn asking_for_research_no_longer_says_it_is_not_built() {
    // The specific regression. Whatever else it answers, it must not claim the
    // module does not exist.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "built");
    let reply = d.turn("research tide times at Ventura", 100);
    assert!(
        !reply.contains("isn't built"),
        "still reporting itself as unbuilt: {reply}"
    );
}

#[test]
fn research_ships_off_but_ready_to_be_switched_on() {
    // It used to ship on, and this test used to assert that, on the
    // reasoning that a silent feature looks like a daemon bug rather than a
    // setting. The setting won: research is the only thing here that reaches
    // outside the machine, and a thing like that waits to be asked for.
    //
    // What still has to hold is the other half of that reasoning. Turning it
    // on in the hub must be the *only* step — if the tools underneath were
    // missing too, the switch would do nothing and you would be back to
    // debugging the daemon.
    let c = cfg();
    let r = c.tools.as_ref().expect("tools.yaml loads").research.clone();
    // Ships on since 27 Sep 2026 (Eric: when Atlas doesn't know, it looks it
    // up); the hub's switch turns it off.
    assert!(r.enabled, "web lookup ships on");
    assert!(r.search.is_some(), "no search tool configured, so the switch would do nothing");
    assert!(r.fetch.is_some(), "no fetch tool configured, so the switch would do nothing");
    assert!(r.max_sources > 0, "a source cap of zero can never find anything");
}

#[test]
fn a_failed_lookup_lands_on_the_outstanding_list_rather_than_evaporating() {
    // The old message told you it was impossible and recorded nothing. A
    // request that failed for a reason that might not hold later has to be
    // recoverable, or "what's outstanding" lies by omission.
    //
    // Driven through the offline path rather than a live lookup: a test that
    // depends on reaching DuckDuckGo would pass or fail on the network rather
    // than on the code, which is not a test.
    let (c, p) = (cfg_researching(), plat());
    let mut d = daemon(&c, &p, "backlog");
    d.connectivity.set(atlas::connectivity::Reach::Offline, 100);
    let before = d.backlog.outstanding().len();
    let reply = d.turn("research tide times at Ventura", 100);
    let after = d.backlog.outstanding().len();
    assert!(
        after > before,
        "a blocked research request vanished instead of being recorded. reply was: {reply}"
    );
}

#[test]
fn a_failure_names_what_went_wrong_rather_than_one_catch_all() {
    // Different failures send you to different places. "No connection",
    // "no model configured" and "every source failed" are three problems, and
    // collapsing them into one sentence is what the old code did.
    let (c, p) = (cfg_researching(), plat());
    let mut d = daemon(&c, &p, "named");
    let reply = d.turn("research tide times at Ventura", 100);
    let named = reply.contains("connection")
        || reply.contains("model")
        || reply.contains("couldn't finish")
        || reply.contains("switched off");
    assert!(named, "the failure was unexplained: {reply}");
}

#[test]
fn research_without_a_model_says_so_rather_than_searching_pointlessly() {
    // The daemon here has `llm: None`. Searching and fetching four pages
    // before discovering there is nothing to write the summary with would
    // waste the work and the wait.
    let (c, p) = (cfg_researching(), plat());
    let mut d = daemon(&c, &p, "nomodel");
    // Force the connectivity check past, so the model branch is the one under
    // test rather than the offline one.
    d.connectivity.set(atlas::connectivity::Reach::Online, 100);
    let reply = d.turn("research tide times at Ventura", 100);
    assert!(reply.contains("model"), "did not name the missing model: {reply}");
}

#[test]
fn being_offline_is_reported_as_offline_not_as_a_research_failure() {
    let (c, p) = (cfg_researching(), plat());
    let mut d = daemon(&c, &p, "offline");
    d.connectivity.set(atlas::connectivity::Reach::Offline, 100);
    let reply = d.turn("research tide times at Ventura", 100);
    assert!(
        reply.contains("connection"),
        "an offline machine blamed the research instead of the network: {reply}"
    );
}
