//! A misspelt key in a config file used to be dropped by serde without a word:
//! you change `poll_ms` to `poll_msec` and nothing happens. It is not an
//! error (a newer release's file has to load on an older Atlas), but it is
//! reported, the same way a Settings choice that reached nothing is.

use std::path::Path;

fn copy_config(to: &Path) {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("config");
    std::fs::create_dir_all(to).unwrap();
    for f in ["apps.yaml", "layouts.yaml", "commands.yaml", "policy.yaml", "indexing.yaml", "tools.yaml"] {
        std::fs::copy(from.join(f), to.join(f)).unwrap();
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    d
}

#[test]
fn the_shipped_config_has_no_key_that_matches_nothing() {
    let d = scratch("shipped-keys");
    copy_config(&d);
    let cfg = atlas::config::Config::load(&d).unwrap();
    assert!(cfg.settings_that_went_nowhere.is_empty(), "{:?}", cfg.settings_that_went_nowhere);
    let _ = std::fs::remove_dir_all(&d);
}

#[test]
fn a_misspelt_policy_key_is_reported_and_still_loads() {
    let d = scratch("typo-key");
    copy_config(&d);
    let p = d.join("policy.yaml");
    let text = std::fs::read_to_string(&p).unwrap();
    std::fs::write(&p, text.replace("min_samples:", "min_sampels:")).unwrap();
    let cfg = atlas::config::Config::load(&d).unwrap();
    assert!(
        cfg.settings_that_went_nowhere.iter().any(|m| m.contains("min_sampels")),
        "{:?}",
        cfg.settings_that_went_nowhere
    );
    let _ = std::fs::remove_dir_all(&d);
}
