//! Telling Atlas what you want it to be able to do (5 Oct 2026, Eric: "I
//! need the ability to tell Atlas what I want when I want to add new
//! capabilities to Atlas"). `growth` took these since 1 Oct, but only
//! phrased "give yourself the ability to ..."; the usual way of saying it
//! reached whatever a word inside it matched.

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
fn i_want_you_to_be_able_to_is_written_down_not_acted_on() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let store = tmp("kept");
    let mut d = Daemon::new(&c, &p, None, Store::new(store.clone()), Proactive::new(ProactiveConfig::default()));
    // "send texts" inside it must not send a text.
    let r = d.turn("I want you to be able to send texts from my phone when I'm driving", 100);
    assert!(r.contains("written it") && r.contains("send texts from my phone"), "{r}");
    // A second wording, after the first: still heard as a request.
    let again = d.turn("I want you to be able to send texts from my phone while I'm driving", 110);
    assert!(again.contains("written it"), "{again}");
    let list = d.turn("what abilities have I asked for?", 120);
    assert!(list.contains("send texts"), "{list}");
    let yes = d.turn("approve that ability", 130);
    assert!(yes.starts_with("Approved"), "{yes}");
    let kept: atlas::growth::WantedAbilities = Store::new(store).load(atlas::growth::STORE);
    assert!(kept.items.iter().any(|w| w.state == atlas::growth::State::Approved));
    assert!(atlas::growth::section(&kept).contains("approved"));
}
