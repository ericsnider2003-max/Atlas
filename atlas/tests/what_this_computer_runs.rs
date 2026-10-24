//! "What can this computer run?" through a real turn (Plan C, 24 Oct 2026):
//! measured memory, every quality tier against it, and nothing changed.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

#[test]
fn asking_what_this_computer_runs_lists_every_tier_against_measured_memory() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let root = std::env::temp_dir().join(format!("atlas-preflight-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    );
    let before = d.modes.active().map(|m| m.name.clone());
    let said = d.turn("What can this computer run?", atlas::store::now());
    eprintln!("{said}");
    assert!(said.starts_with("This computer: "), "{said}");
    assert!(said.contains("measured just now"), "{said}");
    for tier in ["text only", "voice", "voice and reasoning", "everything"] {
        assert!(
            said.contains(&format!("\n- {tier}")),
            "missing {tier}: {said}"
        );
    }
    assert!(said.contains("\nRunning now: "), "{said}");
    assert_eq!(
        d.modes.active().map(|m| m.name.clone()),
        before,
        "asking changed something"
    );
}
