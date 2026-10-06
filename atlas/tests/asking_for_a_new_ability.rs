//! Telling Atlas what you want it to be able to do (5 Oct 2026, Eric: "I
//! need the ability to tell Atlas what I want when I want to add new
//! capabilities to Atlas").

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-asking-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn a_request_is_kept_not_acted_on_and_comes_back_when_asked() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let store = tmp("kept");
    let mut d = Daemon::new(&c, &p, None, Store::new(store.clone()), Proactive::new(ProactiveConfig::default()));
    // "send texts" inside it must not send a text.
    let r = d.turn("I want you to be able to send texts from my phone when I'm driving", 100);
    assert!(r.starts_with("Kept as request 1"), "{r}");
    assert!(r.contains("Nothing's built yet"), "says honestly it isn't built: {r}");
    // Said again: not kept twice.
    let again = d.turn("I want you to be able to send texts from my phone while I'm driving", 110);
    assert!(again.contains("request 1"), "{again}");
    let r2 = d.turn("add a capability that tracks my sleep", 120);
    assert!(r2.starts_with("Kept as request 2"), "{r2}");
    let list = d.turn("what have I asked you to be able to do?", 130);
    assert!(list.contains("1. send texts") && list.contains("2. tracks my sleep"), "{list}");
    let dropped = d.turn("drop request 2", 140);
    assert!(dropped.contains("dropped"), "{dropped}");
    let list = d.turn("show my requests", 150);
    assert!(!list.contains("sleep"), "{list}");
    // Kept where a coding session can read it.
    let kept: atlas::requests::Requests = Store::new(store).load(atlas::requests::FILE);
    assert_eq!(kept.list.len(), 2);
    assert_eq!(kept.list[0].more.len(), 1, "the second wording is kept with the first");
}
