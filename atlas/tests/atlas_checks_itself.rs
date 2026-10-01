//! Research report, 30 Sep 2026, Stage 2: Atlas checking its own work by
//! running it, and keeping what went wrong so it's checked every time after.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-checks-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn code_no_test_notices_breaking_becomes_something_to_fix() {
    let survivors = vec![atlas::mutation::Survivor {
        file: "src/budget.rs".into(),
        function: "total".into(),
        line: 12,
        replacement: "0".into(),
    }];
    let signals = atlas::signals::from_survivors(&survivors);
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].kind, atlas::selfaudit::Kind::NeverFailed);
    let recs = atlas::selfaudit::recommend(&signals, 3);
    assert!(!recs.is_empty(), "a survivor is enough to recommend a look");
}

#[test]
fn a_self_test_failure_is_read_by_self_repair() {
    let case = atlas::regressions::Case {
        said: "focus the quarterly budget".into(),
        wrong: "I'm focusing on it now.".into(),
        wanted: "not: says it did something it didn't".into(),
        command: "focus_app".into(),
        source: atlas::regressions::Source::SelfTest,
        at: 1,
    };
    let signals = atlas::signals::from_regressions(&[case]);
    assert_eq!(signals[0].kind, atlas::selfaudit::Kind::SelfTestFails);
    assert_eq!(signals[0].subject, "focus_app");
    assert!(!atlas::selfaudit::recommend(&signals, 3).is_empty());
}

#[test]
fn a_correction_is_kept_as_a_case_the_self_test_says_again() {
    let c: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let dir = scratch("correction");
    let mut d = Daemon::new(c, &p, None, Store::new(dir.clone()), Proactive::new(ProactiveConfig::default()));
    let t = 1_790_000_000;
    let _ = d.turn("what's queued", t);
    let _ = d.turn("that's wrong", t + 10);
    let _ = d.turn("show me what's waiting to go out, with who it's to", t + 20);
    let cases: Vec<atlas::regressions::Case> = Store::new(dir).load(atlas::regressions::FILE);
    assert!(
        cases.iter().any(|k| k.source == atlas::regressions::Source::Correction && k.said == "what's queued"),
        "{cases:?}"
    );
}
