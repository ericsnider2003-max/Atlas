//! What you changed in the settings page, kept.
//!
//! Every toggle on the hub reported success and wrote nothing.
//!
//! `Settings::set` mutates one in-memory `Setting`, and in the running daemon
//! the whole `Settings` is rebuilt per request from `settings::registry(&self
//! .tools_cfg())` and then dropped — so the handler logged *"Voice is now
//! on"*, redirected to a page that re-rendered from the unchanged config, and
//! the toggle snapped back. There was no writer for `tools.yaml` anywhere in
//! the tree; the only YAML writers were `adapt.rs` (machine.yaml) and
//! `kin.rs` (kin_peers.yaml). That covered every switch on the page,
//! including the ones the module marks `Permission` and `Sensitive`.
//!
//! **Why a separate file rather than editing `tools.yaml`.**
//! `config/tools.yaml` is about nineteen hundred lines, and most of them are
//! the reasoning: why a number is what it is, what breaks if you change it,
//! which decision it came from. Round-tripping it through a YAML serializer
//! would delete all of that on the first toggle. It is also `upgrade::SHIPPED`
//! — the installer replaces it wholesale — so anything written into it is
//! lost on the next update anyway.
//!
//! So this is a third layer, and the project already had the shape for it:
//! the generic `config/*.yaml` ships to anyone, `config/machine.yaml` is what
//! `atlas adapt` found on this computer, and now `config/settings.yaml` is
//! what you chose. Each is applied over the last, in `Config::load`, in one
//! place rather than at every call site.
//!
//! `config/settings.yaml` is in `upgrade::YOURS`, so an update keeps it.
//! Without that line it would be the trash-folder bug again: a file Atlas
//! promises to keep and the updater has never heard of.

use crate::error::Result;
use serde_yaml::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// The one setting whose key is not its path in `tools.yaml`.
///
/// `voice.enabled` is the master switch, which lives at the top level of
/// `tools.yaml` as plain `enabled:`. Named here, with its reason, rather than
/// left as a silent special case — an alias table nobody can see is how two
/// declarations of one fact start.
const ALIASES: &[(&str, &str)] = &[("voice.enabled", "enabled")];

/// Settings that were numbers in an older `tools.yaml` and are words now.
/// `persona.wit` was a 0-to-1 dial until 29 Sep 2026 and is off/dry/full;
/// `wit::Wit` still reads the number, so both shapes load.
const NOW_WORDS: &[&str] = &["persona.wit"];

/// Where a Settings key lives in `tools.yaml`.
pub fn path_for(key: &str) -> &str {
    ALIASES.iter().find(|(k, _)| *k == key).map(|(_, p)| *p).unwrap_or(key)
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preferences {
    /// Dotted key to the raw text you chose. Ordered, so the file is stable
    /// and a diff of it is readable.
    pub chosen: BTreeMap<String, String>,
}

impl Preferences {
    pub fn file(dir: &Path) -> std::path::PathBuf {
        dir.join("settings.yaml")
    }

    pub fn load(dir: &Path) -> Preferences {
        Self::load_checked(dir).unwrap_or_default()
    }

    /// As `load`, but a file that's there and won't read says why, so a
    /// caller can tell you rather than quietly using the defaults.
    pub fn load_checked(dir: &Path) -> std::result::Result<Preferences, String> {
        let Ok(text) = std::fs::read_to_string(Self::file(dir)) else {
            return Ok(Preferences::default());
        };
        if text.trim().is_empty() {
            return Ok(Preferences::default());
        }
        // A settings file that will not parse must not stop Atlas starting,
        // and must not be silently overwritten either — the same rule
        // `Store::load` follows for state. `save` sets it aside first.
        serde_yaml::from_str::<BTreeMap<String, Value>>(&text)
            .map(|m| Preferences { chosen: m.into_iter().map(|(k, v)| (k, scalar_text(&v))).collect() })
            .map_err(|e| format!("{} won't read: {e}", Self::file(dir).display()))
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let header = "# Written by Atlas when you change something in Settings.\n\
                      # Safe to edit by hand, and safe to delete -- deleting it puts\n\
                      # every setting back to what config/tools.yaml says.\n\
                      # This file survives `atlas update`; tools.yaml does not.\n";
        let mut body = String::new();
        for (k, v) in &self.chosen {
            // Quoted, so `on`, `off`, `yes` and `no` cannot be read back as
            // booleans by a YAML parser and change meaning on the way in.
            body.push_str(&format!("{k}: {}\n", serde_yaml::to_string(v).unwrap_or_default().trim()));
        }
        let file = Self::file(dir);
        // A file you'd edited by hand into something that won't read is
        // kept, beside, rather than replaced by this one.
        if Self::load_checked(dir).is_err() {
            let _ = std::fs::rename(&file, dir.join("settings.unreadable.yaml"));
        }
        // Written whole, then swapped in: a power cut mid-write leaves the
        // old file, never half of the new one.
        let part = dir.join("settings.yaml.part");
        std::fs::write(&part, format!("{header}{body}"))?;
        crate::store::rename_patiently(&part, &file)?;
        Ok(())
    }

    pub fn set(&mut self, key: &str, raw: &str) {
        self.chosen.insert(key.to_string(), raw.to_string());
    }

    /// Back to whatever the shipped config says.
    pub fn clear(&mut self, key: &str) -> bool {
        self.chosen.remove(key).is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.chosen.is_empty()
    }

    /// Lay these over a parsed `tools.yaml`.
    ///
    /// Applied to the YAML *before* it becomes a `ToolsConfig`, which is what
    /// makes this work for all 55 settings without a 55-arm setter that would
    /// be a second place the key list lives and a second place it could drift.
    ///
    /// Returns the keys it could not place, so `atlas doctor` can say so
    /// rather than leaving you to wonder why a setting did nothing — which is
    /// the exact failure this module exists to end, and it would be poor to
    /// reintroduce it one layer up.
    pub fn apply_to(&self, root: &mut Value) -> Vec<String> {
        let mut unplaced = Vec::new();
        for (key, raw) in &self.chosen {
            if !set_at(root, path_for(key), raw) {
                unplaced.push(key.clone());
            }
        }
        unplaced
    }
}

/// A scalar as the text a person typed, without YAML's quoting.
fn scalar_text(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        other => serde_yaml::to_string(other).unwrap_or_default().trim().to_string(),
    }
}

/// Set a dotted path, taking the type from what is already there.
///
/// The type matters: writing the string `"true"` where `tools.yaml` holds a
/// boolean gives a config that fails to load, which would turn a toggle that
/// did nothing into a toggle that stops Atlas starting — a strictly worse
/// bug. So the shipped file's own type decides, and only when there is
/// nothing there at all is the type inferred from the text.
fn set_at(root: &mut Value, path: &str, raw: &str) -> bool {
    let parts: Vec<&str> = path.split('.').collect();
    let Some((last, parents)) = parts.split_last() else { return false };

    let mut cur = root;
    for p in parents {
        let key = Value::String((*p).to_string());
        if !cur.is_mapping() {
            return false;
        }
        let map = cur.as_mapping_mut().expect("checked");
        if !map.contains_key(&key) {
            // The parent section is absent from the shipped file. Refuse
            // rather than invent one: a setting whose section does not exist
            // is a setting nothing reads, and inventing the section would
            // hide that instead of reporting it.
            return false;
        }
        cur = map.get_mut(&key).expect("checked");
    }

    let Some(map) = cur.as_mapping_mut() else { return false };
    let key = Value::String((*last).to_string());
    let typed = match map.get(&key) {
        Some(Value::Bool(_)) => match parse_bool(raw) {
            Some(b) => Value::Bool(b),
            None => return false,
        },
        Some(Value::Number(_)) => match raw.trim().parse::<f64>() {
            Ok(f) if f.fract() == 0.0 && f.abs() < 9e15 => Value::Number((f as i64).into()),
            Ok(f) => Value::Number(f.into()),
            // A setting that was a number and is now a word: an older
            // tools.yaml still holds the number, and the word is what the
            // setting takes now. Named, not inferred -- any other number
            // slot given a word is still refused.
            Err(_) if NOW_WORDS.contains(&path) => Value::String(raw.to_string()),
            Err(_) => return false,
        },
        Some(Value::String(_)) => Value::String(raw.to_string()),
        Some(_) => return false,
        // Not in the shipped file. Infer, but still place it — a key the
        // config omits is a key the struct defaults, and overriding a default
        // is legitimate.
        None => match parse_bool(raw) {
            Some(b) => Value::Bool(b),
            None => match raw.trim().parse::<i64>() {
                Ok(i) => Value::Number(i.into()),
                Err(_) => Value::String(raw.to_string()),
            },
        },
    };
    map.insert(key, typed);
    true
}

fn parse_bool(raw: &str) -> Option<bool> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "true" | "on" | "yes" | "1" => Some(true),
        "false" | "off" | "no" | "0" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn yaml(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    #[test]
    fn a_boolean_stays_a_boolean() {
        // The failure this avoids: writing the *string* "true" where the
        // config holds a bool gives a file that will not load, turning a
        // toggle that did nothing into one that stops Atlas starting.
        let mut v = yaml("wake:\n  enabled: false\n");
        assert!(set_at(&mut v, "wake.enabled", "on"));
        assert_eq!(v["wake"]["enabled"], Value::Bool(true));
        assert!(set_at(&mut v, "wake.enabled", "off"));
        assert_eq!(v["wake"]["enabled"], Value::Bool(false));
    }

    #[test]
    fn a_number_stays_a_number_and_a_whole_one_stays_whole() {
        let mut v = yaml("models:\n  memory_budget_mb: 6000\n");
        assert!(set_at(&mut v, "models.memory_budget_mb", "3500"));
        assert_eq!(v["models"]["memory_budget_mb"], Value::Number(3500.into()));
    }

    #[test]
    fn a_string_stays_a_string_even_when_it_looks_like_something_else() {
        let mut v = yaml("voice_settings:\n  voice: \"en_US-amy-medium\"\n");
        assert!(set_at(&mut v, "voice_settings.voice", "af_bella"));
        assert_eq!(v["voice_settings"]["voice"], Value::String("af_bella".into()));
        // "off" into a string field is the literal word, not `false`.
        assert!(set_at(&mut v, "voice_settings.voice", "off"));
        assert_eq!(v["voice_settings"]["voice"], Value::String("off".into()));
    }

    #[test]
    fn nonsense_for_a_typed_field_is_refused_rather_than_guessed() {
        let mut v = yaml("wake:\n  enabled: false\n");
        assert!(!set_at(&mut v, "wake.enabled", "maybe"));
        assert_eq!(v["wake"]["enabled"], Value::Bool(false), "it was changed anyway");
    }

    #[test]
    fn a_section_that_does_not_exist_is_reported_and_not_invented() {
        // Inventing the section would hide the real problem — a setting whose
        // section is absent is a setting nothing reads.
        let mut v = yaml("wake:\n  enabled: false\n");
        assert!(!set_at(&mut v, "nosuch.thing", "true"));
        assert!(v.get("nosuch").is_none());
    }

    #[test]
    fn the_master_switch_knows_where_it_actually_lives() {
        // `voice.enabled` on the settings page is plain `enabled:` at the top
        // of tools.yaml. Without the alias this one setting silently does
        // nothing while the other 54 work — the worst possible outcome,
        // because it is the switch for hearing you at all.
        assert_eq!(path_for("voice.enabled"), "enabled");
        assert_eq!(path_for("wake.enabled"), "wake.enabled");

        let mut v = yaml("enabled: false\nwake:\n  enabled: false\n");
        let mut p = Preferences::default();
        p.set("voice.enabled", "true");
        assert!(p.apply_to(&mut v).is_empty());
        assert_eq!(v["enabled"], Value::Bool(true));
    }

    #[test]
    fn unplaceable_keys_are_reported_not_swallowed() {
        let mut v = yaml("wake:\n  enabled: false\n");
        let mut p = Preferences::default();
        p.set("wake.enabled", "true");
        p.set("ghost.setting", "true");
        let missed = p.apply_to(&mut v);
        assert_eq!(missed, vec!["ghost.setting".to_string()]);
        assert_eq!(v["wake"]["enabled"], Value::Bool(true), "the good one still applied");
    }

    #[test]
    fn what_you_chose_survives_a_round_trip_through_the_file() {
        let dir = std::env::temp_dir().join(format!("atlas-prefs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let mut p = Preferences::default();
        p.set("wake.enabled", "true");
        p.set("voice_settings.voice", "af_bella");
        p.save(&dir).unwrap();

        let back = Preferences::load(&dir);
        assert_eq!(back, p, "a setting did not survive being written down");

        // And clearing puts it back to the shipped value.
        let mut back = back;
        assert!(back.clear("wake.enabled"));
        assert!(!back.clear("wake.enabled"), "clearing twice reported a change twice");
        back.save(&dir).unwrap();
        assert!(!Preferences::load(&dir).chosen.contains_key("wake.enabled"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_unreadable_settings_file_does_not_stop_atlas_starting() {
        let dir = std::env::temp_dir().join(format!("atlas-prefs-bad-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        std::fs::write(Preferences::file(&dir), "{{{ not yaml").unwrap();
        assert!(Preferences::load(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_the_normal_case_and_is_not_an_error() {
        let dir = std::env::temp_dir().join(format!("atlas-prefs-none-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        assert!(Preferences::load(&dir).is_empty());
    }
}
