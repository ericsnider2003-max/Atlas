//! What ranking three talking models on Atlas's own sentences turned up (1
//! Oct 2026), fixed so no model has to get it right: everyday ways of
//! asking that reached nothing, a reminder that reached "sign in", "I've
//! noted that" with nothing noted, and a plain question refused as not code.

use atlas::daemon::Daemon;
use atlas::intent::Parser;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}
fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-ranking-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
const NOW: u64 = 1_790_776_800;

#[test]
fn everyday_ways_of_asking_reach_the_command() {
    let c = cfg();
    let p = Parser::new(&c.commands);
    for (said, want) in [
        ("jot down that the car insurance renews in march", "capture"),
        ("has anybody written to me today", "mail"),
        ("where did I put that lease agreement", "find_file"),
        ("can you see what I've got open right now", "view_display"),
        ("dig into the best budget mechanical keyboards and write it up for me", "research"),
    ] {
        let (_, name) = p.parse_named(said);
        assert_eq!(name.as_deref(), Some(want), "{said}");
    }
}

#[test]
fn a_nudge_is_a_reminder() {
    let c = cfg();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &plat, None, Store::new(tmp("nudge")), Proactive::new(ProactiveConfig::default()));
    let said = d.turn("give me a nudge in 20 minutes to call the dentist", NOW);
    assert!(said.contains("remind you to call the dentist"), "{said}");
    // And it's really booked: due twenty minutes on, not before.
    assert!(d.scheduler.due(NOW + 19 * 60).is_empty());
    assert_eq!(d.scheduler.due(NOW + 21 * 60).len(), 1);
}

#[test]
fn saying_it_was_noted_without_noting_it_is_caught() {
    for s in ["I've noted that your car insurance renews in March.", "I've pulled your Friday schedule.", "I've just added it to your calendar."] {
        assert!(atlas::backed::claims_work_started(s), "{s}");
    }
    assert!(!atlas::backed::claims_work_started("The capital of Australia is Canberra."));
}

/// Eric, 1 Oct 2026: everything a capability needs, Atlas downloads itself.
#[test]
fn atlas_fetches_the_rest_of_what_it_needs_itself() {
    let rest = atlas::getpieces::everything_else();
    let keys: Vec<&str> = rest.iter().map(|p| p.key_path()).collect();
    for want in ["models/kws/", "models/parakeet/", "models/campplus_en_voxceleb.onnx"] {
        assert!(keys.iter().any(|k| k.starts_with(want)), "{want} isn't fetched: {keys:?}");
    }
    // Nothing setup already fetches, nothing twice.
    let setup: Vec<&str> = atlas::getpieces::setup_pieces().iter().map(|p| p.key_path()).collect();
    assert!(keys.iter().all(|k| !setup.contains(k)));
    let mut sorted = keys.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), keys.len());
    // Not the model whose tool calls failed in the pinned server.
    assert!(!rest.iter().any(|p| p.sha256 == atlas::getpieces::better_talk_model().sha256));
    // Every code checker archive is pinned (Windows only; empty elsewhere).
    for p in atlas::codetools::tool_pieces() {
        assert_eq!(p.sha256.len(), 64);
        assert!(p.url.starts_with("https://"));
    }
}
