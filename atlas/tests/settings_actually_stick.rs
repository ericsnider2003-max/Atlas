//! A toggle you move stays moved.
//!
//! It did not. `Settings::set` mutated one in-memory `Setting`, and the hub
//! rebuilt the whole `Settings` per request from
//! `settings::registry(&self.tools_cfg())` and dropped it at the end of the
//! handler — so the log said *"Voice is now on"*, the redirect re-rendered
//! from the unchanged config, and the switch snapped back. There was no
//! writer for `tools.yaml` anywhere in the tree. Every switch on that page
//! was a lie, including the ones the module itself marks `Permission` and
//! `Sensitive`.
//!
//! This is the end-to-end test: write a preference, load the config through
//! the real `Config::load`, and check the value that comes out. If the layer
//! ever stops being applied, these fail — a unit test of the merge function
//! alone would not, because the bug was never in the merge. It was that
//! nothing called one.

use atlas::config::Config;
use atlas::preferences::Preferences;
use atlas::settings;
use std::fs;
use std::path::{Path, PathBuf};

/// A copy of the shipped config, so a test can write into it.
fn install(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("atlas-settings-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    for f in ["tools.yaml", "apps.yaml", "layouts.yaml", "commands.yaml", "policy.yaml", "indexing.yaml"] {
        let from = Path::new("config").join(f);
        if from.is_file() {
            fs::copy(&from, dir.join(f)).unwrap();
        }
    }
    dir
}

fn tools_of(dir: &Path) -> atlas::voice::ToolsConfig {
    Config::load(dir).expect("the config should load").tools.expect("tools.yaml")
}

#[test]
fn a_switch_you_move_is_still_moved_after_a_restart() {
    let dir = install("wake");
    let before = tools_of(&dir).wake.as_ref().map(|w| w.enabled).unwrap_or(false);

    let mut p = Preferences::load(&dir);
    p.set("wake.enabled", if before { "off" } else { "on" });
    p.save(&dir).unwrap();

    let after = tools_of(&dir).wake.as_ref().map(|w| w.enabled).unwrap_or(false);
    assert_ne!(before, after, "the toggle reported success and changed nothing");

    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_master_switch_works_too() {
    // `voice.enabled` is the one settings key whose name is not its path —
    // it is plain `enabled:` at the top of tools.yaml. Without its alias this
    // single setting silently does nothing while the other 54 work, and it is
    // the switch for hearing you at all.
    let dir = install("master");
    assert!(!tools_of(&dir).enabled, "the shipped config no longer ships voice off");

    let mut p = Preferences::load(&dir);
    p.set("voice.enabled", "on");
    p.save(&dir).unwrap();

    assert!(tools_of(&dir).enabled, "the master switch does nothing");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_number_and_a_name_stick_as_well_as_a_toggle() {
    let dir = install("kinds");
    let mut p = Preferences::load(&dir);
    p.set("models.memory_budget_mb", "4096");
    p.set("voice_settings.voice", "en_US-ryan-medium");
    p.save(&dir).unwrap();

    let t = tools_of(&dir);
    assert_eq!(t.models.memory_budget_mb, 4096, "a number setting did not stick");
    assert_eq!(t.voice_settings.voice, "en_US-ryan-medium", "a name setting did not stick");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn every_setting_on_the_page_can_be_written_down() {
    // The one that would have caught this whole class. Each of the 55 keys on
    // the settings page must be a key the preferences layer can actually
    // place — a page with a switch that reaches nothing is exactly the bug.
    let dir = install("all");
    let shipped = tools_of(&dir);
    let page = settings::registry(&shipped);

    let text = fs::read_to_string(dir.join("tools.yaml")).unwrap();
    let mut raw: serde_yaml::Value = serde_yaml::from_str(&text).unwrap();

    let mut p = Preferences::default();
    for item in &page.items {
        // The value does not matter here; placement does. Every entry gets
        // something its own type accepts.
        p.set(&item.key, "true");
    }
    let unplaced = p.apply_to(&mut raw);
    // Some settings are not booleans, so "true" is legitimately refused for
    // them — that is `set_at` protecting the file, not a missing key. What
    // must not happen is a key with nowhere to go at all.
    let missing_section: Vec<&String> = unplaced
        .iter()
        .filter(|k| {
            let path = k.rsplit_once('.').map(|(p, _)| p.to_string()).unwrap_or_default();
            !path.is_empty() && raw.get(&path).is_none() && *k != "voice.enabled"
        })
        .collect();
    assert!(
        missing_section.is_empty(),
        "these settings have no home in tools.yaml, so moving them could never \
         have done anything:\n  {missing_section:?}"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn clearing_a_setting_puts_the_shipped_value_back() {
    let dir = install("clear");
    let shipped = tools_of(&dir).wake.as_ref().map(|w| w.enabled).unwrap_or(false);

    let mut p = Preferences::load(&dir);
    p.set("wake.enabled", if shipped { "off" } else { "on" });
    p.save(&dir).unwrap();
    assert_ne!(tools_of(&dir).wake.as_ref().map(|w| w.enabled).unwrap_or(false), shipped);

    let mut p = Preferences::load(&dir);
    assert!(p.clear("wake.enabled"));
    p.save(&dir).unwrap();
    assert_eq!(
        tools_of(&dir).wake.as_ref().map(|w| w.enabled).unwrap_or(false),
        shipped,
        "there is no way back to the shipped value"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_setting_that_no_longer_exists_is_reported_rather_than_ignored() {
    // Dropping it silently would be the original bug one layer up: you
    // changed something and nothing happened.
    let dir = install("ghost");
    let mut p = Preferences::load(&dir);
    p.set("a_section_that_was_removed.enabled", "true");
    p.save(&dir).unwrap();

    let cfg = Config::load(&dir).expect("a stale preference must not stop Atlas starting");
    assert!(
        cfg.settings_that_went_nowhere.contains(&"a_section_that_was_removed.enabled".to_string()),
        "a preference that reaches nothing is being swallowed again"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_settings_file_does_not_stop_atlas_starting() {
    // The worst outcome of adding a writer: turning a switch that did nothing
    // into a switch that bricks the install.
    let dir = install("damaged");
    fs::write(Preferences::file(&dir), "this: [is not: valid").unwrap();
    let cfg = Config::load(&dir);
    assert!(cfg.is_ok(), "an unreadable settings file stopped Atlas: {:?}", cfg.err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_shipped_config_is_never_written_to() {
    // `config/tools.yaml` is ~1900 lines, most of them the reasoning behind
    // the numbers, and it is `upgrade::SHIPPED` — replaced wholesale by an
    // update. Writing settings into it would delete the comments on the first
    // toggle and lose the settings on the first update.
    let dir = install("untouched");
    let before = fs::read_to_string(dir.join("tools.yaml")).unwrap();

    let mut p = Preferences::load(&dir);
    p.set("wake.enabled", "on");
    p.save(&dir).unwrap();
    let _ = tools_of(&dir);

    assert_eq!(
        fs::read_to_string(dir.join("tools.yaml")).unwrap(),
        before,
        "the shipped config was modified — its comments will not survive this"
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_update_keeps_what_you_chose() {
    // Without this line in `upgrade::YOURS` the settings file is the trash
    // folder again: something Atlas promises to keep that the updater has
    // never heard of.
    let names: Vec<&str> = atlas::upgrade::YOURS.iter().map(|(p, _)| *p).collect();
    assert!(
        names.contains(&"config/settings.yaml"),
        "`atlas update` does not preserve your settings"
    );
    let shipped: Vec<&str> = atlas::upgrade::SHIPPED.iter().map(|(p, _)| *p).collect();
    assert!(
        !shipped.contains(&"config/settings.yaml"),
        "your settings are also listed as a file the update replaces"
    );
}

#[test]
fn the_hub_writes_the_change_and_says_so_if_it_cannot() {
    let src = crate::common::source_of("hublive");
    let at = src.find("Action::HubSet").expect("the settings handler is gone");
    // Since 26 Sep the handler hands the write to `apply_setting`, which the
    // Sound & voice page's form shares, so both write the same way.
    let arm = src[at..].find("Action::HubSet { key, value } =>").map(|i| at + i).unwrap_or(at);
    assert!(src[arm..arm + 1400].contains("self.apply_setting("), "the settings handler no longer uses the shared writer");
    let at = src.find("fn apply_setting(").expect("the shared writer is gone");
    // Since 27 Sep 2026 the validate-then-write lives in
    // `Settings::set_and_keep`, which the settings-only window shares too (it
    // used to validate and write nothing). `apply_setting` must still go
    // through it.
    assert!(src[at..at + 1200].contains(".set_and_keep("), "apply_setting no longer uses the shared writer");
    let settings = fs::read_to_string("src/settings.rs").expect("src/settings.rs");
    let at = settings.find("pub fn set_and_keep(").expect("the shared writer is gone");
    let body = &settings[at..at + 1400];
    let main = crate::common::read_source_path("src/main.rs").expect("src/main.rs");
    let arm = main.find("Action::HubSet { key, value } =>").expect("settings-only mode no longer answers a setting");
    assert!(main[arm..arm + 600].contains(".set_and_keep("), "settings-only mode validates and writes nothing again");
    assert!(
        body.contains("Preferences::load(") && body.contains("prefs.save("),
        "the settings handler no longer writes anything down"
    );
    assert!(
        body.contains("couldn't keep that change"),
        "a failed write would be reported as success again"
    );
    // Validate before writing, or a bad value reaches the file and the next
    // start fails on it.
    let set_at = body.find("self.set(").expect("validation is gone");
    let save_at = body.find("prefs.save(").expect("checked above");
    assert!(set_at < save_at, "the value is written before it is validated");
}
