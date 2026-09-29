//! Atlas's settings in its own window (23 Sep 2026): the same list the hub
//! renders, kept through the same path the hub's form takes.

use atlas::settings::{Value, Weight};
use atlas::settingswin::{current_settings, keep_setting, needs_a_yes, put_back, raw_of, why_ask};
use std::path::PathBuf;

fn fresh(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atlas-native-settings-{tag}")).join("config");
    let _ = std::fs::remove_dir_all(dir.parent().unwrap());
    atlas::firstlaunch::write_default_config(&dir).unwrap();
    dir
}

fn value_of(dir: &std::path::Path, key: &str) -> Value {
    current_settings(dir).unwrap().get(key).unwrap_or_else(|| panic!("no {key}")).value.clone()
}

#[test]
fn a_change_is_kept_and_read_back_the_next_time_settings_load() {
    let dir = fresh("keep");
    assert_eq!(value_of(&dir, "voice.enabled"), Value::Toggle(false));
    let said = keep_setting(&dir, "voice.enabled", "on").unwrap();
    assert!(said.contains("now on"), "{said}");
    // Read back through Config::load, the way Atlas reads it at start.
    assert_eq!(value_of(&dir, "voice.enabled"), Value::Toggle(true));
    // Kept in the file that survives an update, not in tools.yaml.
    let kept = std::fs::read_to_string(dir.join("settings.yaml")).unwrap();
    assert!(kept.contains("voice.enabled"), "{kept}");
    assert_eq!(
        std::fs::read_to_string(dir.join("tools.yaml")).unwrap(),
        std::fs::read_to_string("config/tools.yaml").unwrap(),
        "tools.yaml was edited"
    );
}

#[test]
fn putting_back_returns_to_what_atlas_ships_with() {
    let dir = fresh("back");
    keep_setting(&dir, "voice.enabled", "on").unwrap();
    let said = put_back(&dir, "voice.enabled").unwrap();
    assert!(said.contains("back to off"), "{said}");
    assert_eq!(value_of(&dir, "voice.enabled"), Value::Toggle(false));
    assert!(!current_settings(&dir).unwrap().get("voice.enabled").unwrap().changed());
}

#[test]
fn a_value_that_doesnt_fit_is_refused_and_nothing_is_written() {
    let dir = fresh("refuse");
    let settings = current_settings(&dir).unwrap();
    let number = settings
        .items
        .iter()
        .find(|s| matches!(s.value, Value::Number { .. }))
        .expect("the registry has at least one number")
        .clone();
    let Value::Number { max, .. } = number.value else { unreachable!() };
    let err = keep_setting(&dir, &number.key, &format!("{}", max * 10.0 + 1.0)).unwrap_err();
    assert!(err.contains("between"), "{err}");
    assert!(!dir.join("settings.yaml").exists(), "a refused change was written");
    assert!(keep_setting(&dir, "no.such.setting", "on").is_err());
}

#[test]
fn only_widening_a_sensor_or_permission_asks_first() {
    let dir = fresh("ask");
    let settings = current_settings(&dir).unwrap();
    let sensitive = settings
        .items
        .iter()
        .find(|s| s.weight == Weight::Sensitive && matches!(s.value, Value::Toggle(_)))
        .expect("a sensor toggle");
    assert!(needs_a_yes(sensitive, "on"));
    assert!(!needs_a_yes(sensitive, "off"), "turning a sensor off never needs a yes");
    let plain = settings.items.iter().find(|s| s.weight == Weight::Preference).expect("a preference");
    assert!(!needs_a_yes(plain, "on"));
    assert_ne!(why_ask(Weight::Sensitive), why_ask(Weight::Permission));
    assert!(why_ask(Weight::Sensitive).ends_with("Keep it?"));
}

#[test]
fn every_value_round_trips_through_its_control() {
    let dir = fresh("round");
    let mut settings = current_settings(&dir).unwrap();
    let keys: Vec<(String, Value)> = settings.items.iter().map(|s| (s.key.clone(), s.value.clone())).collect();
    for (key, value) in keys {
        // What a control hands back for the current value is read back as the
        // same value — so opening Settings and touching nothing changes nothing.
        settings.set(&key, &raw_of(&value)).unwrap_or_else(|e| panic!("{key}: {e}"));
        assert_eq!(settings.get(&key).unwrap().value, value, "{key}");
    }
}

#[test]
fn the_stop_request_is_a_file_in_atlas_own_folder() {
    let dir = std::env::temp_dir().join("atlas-native-settings-stop");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    assert!(atlas::goodbye::stop_file(&dir).starts_with(&dir));
    // No request, no stop.
    assert!(!atlas::goodbye::asked_by_file(&dir));
    // With nothing running, asking to stop leaves no request behind to trip
    // the next start.
    let root = dir.join("install");
    // 28 Sep 2026: asked of the install's own lock, not of a port.
    assert!(atlas::firstlaunch::ask_atlas_to_stop(&root, std::time::Duration::from_millis(300)));
    assert!(!atlas::goodbye::stop_file(&root.join("data").join("state")).exists());
}
