//! A test run doesn't leave its scratch behind for good (5 Oct 2026, ledger
//! Q21: 18 runs had left 11,452 folders, 12 GB, in the system temp dir).

use std::time::{Duration, SystemTime};

fn build_scratch(tag: &str) -> std::path::PathBuf {
    // A stand-in for a build's target/tmp, inside this run's own scratch.
    let d = std::env::temp_dir().join(format!("atlas-q21-{tag}-{}", std::process::id())).join("target").join("tmp");
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn an_earlier_runs_leftovers_are_cleared_and_this_runs_are_kept() {
    let d = build_scratch("sweep");
    std::fs::create_dir_all(d.join("atlas-step2-cli-111/deep")).unwrap();
    std::fs::write(d.join("atlas-step2-cli-111/deep/file"), b"x").unwrap();
    std::fs::write(d.join("loose.txt"), b"x").unwrap();
    // Judged three hours on: everything there now is an earlier run's.
    let later = SystemTime::now() + Duration::from_secs(3 * 3600);
    assert_eq!(atlas::roots::sweep_old_test_scratch(&d, later), 2);
    assert_eq!(std::fs::read_dir(&d).unwrap().count(), 0);
    // Judged now: what this run just made stays.
    std::fs::create_dir_all(d.join("atlas-fresh")).unwrap();
    assert_eq!(atlas::roots::sweep_old_test_scratch(&d, SystemTime::now()), 0);
    assert!(d.join("atlas-fresh").exists());
    let _ = std::fs::remove_dir_all(d.parent().unwrap().parent().unwrap());
}

#[test]
fn nothing_outside_a_builds_own_scratch_is_ever_touched() {
    let d = std::env::temp_dir().join(format!("atlas-q21-elsewhere-{}", std::process::id()));
    std::fs::create_dir_all(d.join("yours")).unwrap();
    let later = SystemTime::now() + Duration::from_secs(30 * 24 * 3600);
    assert_eq!(atlas::roots::sweep_old_test_scratch(&d, later), 0, "not a target/tmp folder");
    assert!(d.join("yours").exists());
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn under_cargo_the_tests_temp_dir_is_the_builds_own() {
    let cfg = std::fs::read_to_string(".cargo/config.toml").unwrap();
    for var in ["TMPDIR", "TMP", "TEMP"] {
        assert!(cfg.contains(&format!("{var} = {{ value = \"target/tmp\", relative = true, force = true }}")), "{var}");
    }
    let t = std::env::temp_dir();
    assert!(t.ends_with("target/tmp") || t.ends_with("target\\tmp"), "tests are writing to {}", t.display());
}
