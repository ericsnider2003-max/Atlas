//! The plain API another Atlas reads ("how's the homelab Atlas?" --
//! `elsewhere::ask`). /status, /outstanding, /queued and /health were
//! routed and then answered "That isn't a page" in HTML (2 Oct 2026).

use atlas::server::Action;

#[test]
fn each_route_answers_in_plain_words() {
    use atlas::daemon::Daemon;
    use atlas::platform::{mock::MockPlatform, Monitor};
    use atlas::proactive::{Proactive, ProactiveConfig};
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-plain-api-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &plat, None, atlas::store::Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let health = atlas::hublive::reply(&mut d, Action::Health);
    assert_eq!((health.status, health.body.as_str()), (200, "ok"));
    let status = atlas::hublive::reply(&mut d, Action::Status).body;
    assert!(status.starts_with("Running. 0 errands in hand, 0 waiting."), "{status}");
    for a in [Action::Outstanding, Action::Queued] {
        let r = atlas::hublive::reply(&mut d, a);
        assert_eq!(r.status, 200);
        assert!(!r.body.contains("isn't a page") && !r.body.contains("<html"), "{}", r.body);
    }
}
