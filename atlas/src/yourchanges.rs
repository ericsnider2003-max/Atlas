//! Your own edits to the shipped settings files, kept through every update.
//!
//! The shipped `config/*.yaml` files are `upgrade::SHIPPED`: an update
//! replaces them, because that is how a release changes a default. But people
//! edit them by hand -- `policy.yaml` to let Atlas do something without asking,
//! `commands.yaml` to add a phrase, `indexing.yaml` to point at a folder -- and
//! before this file an update would have put every one of those edits back
//! without a word. For `policy.yaml` that is worse than lost work: a
//! permission you deliberately *narrowed* would quietly widen again.
//!
//! `config/settings.yaml` (the Settings page) already survives, because it is
//! a sparse layer of only what you chose, applied over the shipped file. This
//! does the same for hand edits, for every shipped YAML file:
//!
//! - **Base** -- `config/local/base/<file>`: the exact text Atlas last shipped
//!   into `config/<file>`. Kept so a hand edit can be told apart from an older
//!   release's default.
//! - **Yours** -- `config/local/<file>`: the changes you made, one entry per
//!   setting, each with the value you chose and the shipped value it replaced.
//! - **Theirs** -- the new shipped text, built into this program.
//!
//! At startup `keep_hand_edits` compares each shipped file on disk with its
//! base. Anything you changed is moved into `config/local/<file>`, and the
//! shipped file is put back to exactly what this build ships. `Config::load`
//! then lays your changes over the shipped file every time it loads. So:
//!
//! - a default you never touched follows the release;
//! - a value you set stays yours, even when the release changes that default
//!   (and you are told once, so you can decide);
//! - a setting the release removed is reported rather than recreated, because
//!   a setting nothing reads anymore would do nothing and hide that it does.
//!
//! The rule the whole file rests on: **never lose an edit.** When it cannot
//! tell whether a difference is yours (no base on record) it keeps it and says
//! so. When your overlay file will not parse, it touches nothing. When a file
//! it replaces had any change of yours, even only comments, the old copy goes
//! to `config/local/previous/<file>` first.

use serde::{Deserialize, Serialize};
use serde_yaml::{Mapping, Value};
use std::path::{Path, PathBuf};

/// Where your changes live, relative to the config folder.
pub const LOCAL_DIR: &str = "local";

/// The shipped settings files this covers: every top-level YAML file the
/// program carries. Derived from `firstlaunch::DEFAULT_CONFIG` rather than
/// listed again, so a new shipped file is covered the day it ships.
pub fn shipped_yaml() -> Vec<(&'static str, &'static str)> {
    shipped_yaml_from(crate::firstlaunch::DEFAULT_CONFIG)
}

fn shipped_yaml_from(all: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
    all.iter().copied().filter(|(n, _)| n.ends_with(".yaml") && !n.contains('/')).collect()
}

/// One setting you changed.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    /// Where, as mapping keys from the top of the file. Empty means the whole
    /// file (only when the file is not a mapping at all).
    pub path: Vec<String>,
    /// What you set it to. Ignored when `removed`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yours: Option<Value>,
    /// You deleted this setting.
    #[serde(default, skip_serializing_if = "is_false")]
    pub removed: bool,
    /// What the shipped file said when you changed it; absent means the
    /// shipped file did not have it. A release that ships something different
    /// here is a release that changed a default you overrode.
    #[serde(default)]
    pub was: Option<Value>,
    /// Captured without a base on record, so it may be an older release's
    /// default rather than your edit. Kept rather than guessed away.
    #[serde(default, skip_serializing_if = "is_false")]
    pub unsure: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Change {
    pub fn dotted(&self) -> String {
        if self.path.is_empty() {
            "(the whole file)".into()
        } else {
            self.path.join(".")
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OverlayFile {
    #[serde(default)]
    changes: Vec<Change>,
}

const HEADER: &str = "\
# Your own edits to config/FILE, kept here so an update cannot undo them.
# Atlas moved them out of config/FILE (which an update replaces) and lays
# them back over it every time it starts.
#   path    -- which setting
#   yours   -- the value you chose (removed: true means you deleted it)
#   was     -- what the shipped file said at the time
#   unsure  -- Atlas could not tell whether this was your edit or an older
#              release's default; delete the entry if it wasn't yours
# To go back to the shipped default, delete that entry. Delete this whole
# file to go back to the shipped config/FILE entirely.
";

// ------------------------------------------------------------------ diff

fn string_keyed(m: &Mapping) -> bool {
    m.keys().all(|k| k.is_string())
}

/// Every difference between a shipped file and yours, as changes.
///
/// Mappings whose keys are all strings are descended into, so changing one
/// setting records one setting and a release is still free to change its
/// neighbours. Lists and scalars are compared whole: an edited list is your
/// list.
pub fn diff(base: &Value, mine: &Value) -> Vec<Change> {
    let mut out = Vec::new();
    diff_at(&mut Vec::new(), base, mine, &mut out);
    out
}

fn diff_at(path: &mut Vec<String>, base: &Value, mine: &Value, out: &mut Vec<Change>) {
    match (base, mine) {
        (Value::Mapping(b), Value::Mapping(m)) if string_keyed(b) && string_keyed(m) => {
            for (k, mv) in m {
                let key = k.as_str().expect("string keyed").to_string();
                path.push(key);
                match b.get(k) {
                    Some(bv) => diff_at(path, bv, mv, out),
                    None => out.push(Change {
                        path: path.clone(),
                        yours: Some(mv.clone()),
                        removed: false,
                        was: None,
                        unsure: false,
                    }),
                }
                path.pop();
            }
            for (k, bv) in b {
                if !m.contains_key(k) {
                    let mut p = path.clone();
                    p.push(k.as_str().expect("string keyed").to_string());
                    out.push(Change { path: p, yours: None, removed: true, was: Some(bv.clone()), unsure: false });
                }
            }
        }
        _ if base != mine => out.push(Change {
            path: path.clone(),
            yours: Some(mine.clone()),
            removed: false,
            was: Some(base.clone()),
            unsure: false,
        }),
        _ => {}
    }
}

// ------------------------------------------------------------------ apply

/// The value at a path, if every step exists.
fn get<'a>(root: &'a Value, path: &[String]) -> Option<&'a Value> {
    let mut cur = root;
    for p in path {
        cur = cur.as_mapping()?.get(Value::String(p.clone()))?;
    }
    Some(cur)
}

/// Lay your changes over a shipped file. Returns the ones it could not place.
///
/// A change whose parent section the shipped file no longer has is not placed
/// and is returned: the release removed that section, so nothing reads it,
/// and recreating it would hide that rather than report it -- the same rule
/// `preferences::set_at` follows.
pub fn apply(root: &mut Value, changes: &[Change]) -> Vec<Change> {
    let mut unplaced = Vec::new();
    for c in changes {
        if !apply_one(root, c) {
            unplaced.push(c.clone());
        }
    }
    unplaced
}

fn apply_one(root: &mut Value, c: &Change) -> bool {
    let Some((last, parents)) = c.path.split_last() else {
        if c.removed {
            return false;
        }
        *root = c.yours.clone().unwrap_or(Value::Null);
        return true;
    };
    let mut cur = root;
    for p in parents {
        let Some(map) = cur.as_mapping_mut() else { return false };
        let Some(next) = map.get_mut(Value::String(p.clone())) else { return false };
        cur = next;
    }
    let Some(map) = cur.as_mapping_mut() else { return false };
    let key = Value::String(last.clone());
    if c.removed {
        map.remove(&key);
    } else {
        map.insert(key, c.yours.clone().unwrap_or(Value::Null));
    }
    true
}

fn present(v: Option<&Value>) -> Option<&Value> {
    v.filter(|v| !v.is_null())
}

/// Your changes whose shipped value this release changed: you overrode a
/// default, and the default has since moved. Yours still wins; this is so
/// you can decide whether it should.
pub fn conflicts<'a>(shipped: &Value, changes: &'a [Change]) -> Vec<&'a Change> {
    changes
        .iter()
        .filter(|c| present(get(shipped, &c.path)) != present(c.was.as_ref()))
        .collect()
}

/// Add newly captured changes to the ones already kept. A new change at the
/// same place, or above it, replaces what was kept there.
fn merge(kept: &mut Vec<Change>, incoming: Vec<Change>) {
    for c in incoming {
        kept.retain(|k| !k.path.starts_with(&c.path));
        kept.push(c);
    }
}

// ------------------------------------------------------------------ files

pub fn overlay_path(config_dir: &Path, file: &str) -> PathBuf {
    config_dir.join(LOCAL_DIR).join(file)
}

fn base_path(config_dir: &Path, file: &str) -> PathBuf {
    config_dir.join(LOCAL_DIR).join("base").join(file)
}

fn previous_path(config_dir: &Path, file: &str) -> PathBuf {
    config_dir.join(LOCAL_DIR).join("previous").join(file)
}

/// Your kept changes for one shipped file. No file means none; a file that
/// will not parse is an error, and the caller must neither apply it nor
/// write over it.
pub fn load_overlay(config_dir: &Path, file: &str) -> Result<Vec<Change>, String> {
    let path = overlay_path(config_dir, file);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(format!("{}: {e}", path.display())),
    };
    if text.lines().all(|l| l.trim().is_empty() || l.trim_start().starts_with('#')) {
        return Ok(Vec::new());
    }
    serde_yaml::from_str::<OverlayFile>(&text)
        .map(|o| o.changes)
        .map_err(|e| format!("{} does not parse ({e})", path.display()))
}

fn save_overlay(config_dir: &Path, file: &str, changes: &[Change]) -> std::io::Result<()> {
    let path = overlay_path(config_dir, file);
    if changes.is_empty() {
        return match std::fs::remove_file(&path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    let body = serde_yaml::to_string(&OverlayFile { changes: changes.to_vec() })
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    write_whole(&path, &format!("{}{body}", HEADER.replace("FILE", file)))
}

/// Write by renaming a finished temporary file over the old one, so a crash
/// leaves either the old file or the new one, never half of either.
fn write_whole(path: &Path, text: &str) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("yaml.writing");
    std::fs::write(&tmp, text)?;
    std::fs::rename(&tmp, path)
}

// ------------------------------------------------------------------ renames

/// Move changes made under a setting's old name to its new one.
///
/// `renames` is `(file, old.dotted.path, new.dotted.path)`. A rename of a
/// section carries everything under it.
fn rename_paths(changes: &mut [Change], file: &str, renames: &[(&str, &str, &str)]) -> bool {
    let mut moved = false;
    for (f, from, to) in renames {
        if *f != file {
            continue;
        }
        let from: Vec<String> = from.split('.').map(String::from).collect();
        let to: Vec<String> = to.split('.').map(String::from).collect();
        for c in changes.iter_mut() {
            if c.path.starts_with(&from) {
                let rest = c.path[from.len()..].to_vec();
                c.path = to.iter().cloned().chain(rest).collect();
                moved = true;
            }
        }
    }
    moved
}

/// The same for Settings-page choices, which are dotted keys over `tools.yaml`.
fn rename_preferences(config_dir: &Path, renames: &[(&str, &str, &str)]) -> Vec<String> {
    let mut prefs = crate::preferences::Preferences::load(config_dir);
    let mut notes = Vec::new();
    for (f, from, to) in renames {
        if *f != "tools.yaml" {
            continue;
        }
        let keys: Vec<String> = prefs.chosen.keys().cloned().collect();
        for k in keys {
            let new_key = if k == *from {
                to.to_string()
            } else if let Some(rest) = k.strip_prefix(&format!("{from}.")) {
                format!("{to}.{rest}")
            } else {
                continue;
            };
            if let Some(v) = prefs.chosen.remove(&k) {
                prefs.chosen.entry(new_key.clone()).or_insert(v);
                notes.push(format!("Your setting {k} is now called {new_key}; your choice moved with it."));
            }
        }
    }
    if !notes.is_empty() {
        if let Err(e) = prefs.save(config_dir) {
            return vec![format!("A setting of yours was renamed, and I couldn't save the new name: {e}")];
        }
    }
    notes
}

// ------------------------------------------------------------------ Settings choices

/// What the shipped `tools.yaml` said under each Settings choice, when it was
/// chosen -- so a release that changes that default can say so, the same as
/// for a hand edit. `None` means the shipped file did not have it.
fn settings_was_path(config_dir: &Path) -> PathBuf {
    config_dir.join(LOCAL_DIR).join("settings-was.yaml")
}

fn load_settings_was(config_dir: &Path) -> std::collections::BTreeMap<String, Option<Value>> {
    std::fs::read_to_string(settings_was_path(config_dir))
        .ok()
        .and_then(|t| serde_yaml::from_str(&t).ok())
        .unwrap_or_default()
}

fn key_path(key: &str) -> Vec<String> {
    crate::preferences::path_for(key).split('.').map(String::from).collect()
}

/// Fill in the shipped value for every Settings choice that has none on
/// record, and drop the ones you have since cleared. Read from the base on
/// record (what was shipped before this version), or failing that the file
/// on disk, never from this build's text -- otherwise a default this version
/// changed would be recorded as the one you chose against.
fn record_setting_defaults(config_dir: &Path) {
    let prefs = crate::preferences::Preferences::load(config_dir);
    let mut was = load_settings_was(config_dir);
    let before = was.clone();
    was.retain(|k, _| prefs.chosen.contains_key(k));
    let missing: Vec<String> = prefs.chosen.keys().filter(|k| !was.contains_key(*k)).cloned().collect();
    if !missing.is_empty() {
        let text = std::fs::read_to_string(base_path(config_dir, "tools.yaml"))
            .or_else(|_| std::fs::read_to_string(config_dir.join("tools.yaml")));
        if let Some(root) = text.ok().and_then(|t| serde_yaml::from_str::<Value>(&t).ok()) {
            for k in missing {
                let v = get(&root, &key_path(&k)).cloned();
                was.insert(k, v);
            }
        }
    }
    if was != before {
        let text = format!(
            "# What the shipped tools.yaml said under each of your Settings choices when you made it.\n\
             # Atlas keeps this to tell you when an update changes one of those defaults.\n{}",
            serde_yaml::to_string(&was).unwrap_or_default()
        );
        let _ = write_whole(&settings_was_path(config_dir), &text);
    }
}

// ------------------------------------------------------------------ what you have kept

/// One kept hand edit, for the "Your edits" page and `atlas edits`.
#[derive(Debug, Clone, PartialEq)]
pub struct KeptEdit {
    pub file: String,
    pub change: Change,
    /// What this build ships at that place now. Differs from `change.was`
    /// when a release moved the default since you changed it.
    pub shipped_now: Option<Value>,
}

impl KeptEdit {
    pub fn default_moved(&self) -> bool {
        present(self.shipped_now.as_ref()) != present(self.change.was.as_ref())
    }
}

/// A value on one line, for a person to read.
pub fn shown(v: Option<&Value>) -> String {
    match v {
        None => "(not set)".into(),
        Some(Value::String(s)) => s.clone(),
        Some(other) => {
            let t = serde_yaml::to_string(other).unwrap_or_default();
            let one: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
            if one.chars().count() > 120 {
                format!("{}…", one.chars().take(120).collect::<String>())
            } else {
                one
            }
        }
    }
}

/// Every hand edit Atlas is keeping, with any file it could not read.
pub fn all_kept(config_dir: &Path) -> (Vec<KeptEdit>, Vec<String>) {
    let mut out = Vec::new();
    let mut problems = Vec::new();
    for (file, text) in shipped_yaml() {
        match load_overlay(config_dir, file) {
            Ok(changes) => {
                let now: Option<Value> = serde_yaml::from_str(text).ok();
                for c in changes {
                    let shipped_now = now.as_ref().and_then(|n| get(n, &c.path)).cloned();
                    out.push(KeptEdit { file: file.to_string(), change: c, shipped_now });
                }
            }
            Err(e) => problems.push(e),
        }
    }
    (out, problems)
}

/// Stop keeping one edit, so that setting goes back to what Atlas ships.
/// `Ok(false)` when there was no such edit.
pub fn forget(config_dir: &Path, file: &str, dotted: &str) -> Result<bool, String> {
    if !shipped_yaml().iter().any(|(f, _)| *f == file) {
        return Err(format!("{file} is not one of Atlas's shipped config files"));
    }
    let mut changes = load_overlay(config_dir, file)?;
    let before = changes.len();
    changes.retain(|c| c.dotted() != dotted);
    if changes.len() == before {
        return Ok(false);
    }
    save_overlay(config_dir, file, &changes).map_err(|e| format!("couldn't save the change: {e}"))?;
    Ok(true)
}

// ------------------------------------------------------------------ startup

/// How many of your hand edits are kept, across every shipped file. An
/// overlay that will not parse counts as none, and `atlas doctor` says why.
pub fn kept_count(config_dir: &Path) -> usize {
    shipped_yaml().iter().filter_map(|(f, _)| load_overlay(config_dir, f).ok()).map(|c| c.len()).sum()
}

/// What `keep_hand_edits` did, for the person.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Kept {
    /// Plain sentences, each worth printing.
    pub notices: Vec<String>,
    /// How many of your edits are kept, across every file, after this run.
    pub total: usize,
}

/// Move hand edits out of the shipped files and bring the shipped files up
/// to this build. Run at startup, before `Config::load`. Never fails: the
/// worst case is that a file is left exactly as it was, and said so.
pub fn keep_hand_edits(config_dir: &Path) -> Kept {
    if is_source_tree(config_dir) {
        // These files *are* what gets built into the program. Moving an edit
        // out of them would revert the source a developer is working on.
        return Kept::default();
    }
    keep_hand_edits_with(config_dir, &shipped_yaml(), crate::upgrade::RENAMED_SETTINGS, crate::upgrade::version())
}

/// A config folder that sits in Atlas's own source tree: the files there are
/// the ones `include_str!` builds in, so an edit is a change to the next
/// release, not a hand edit to keep.
pub fn is_source_tree(config_dir: &Path) -> bool {
    config_dir.parent().is_some_and(|p| p.join("Cargo.toml").is_file() && p.join("src").join("firstlaunch.rs").is_file())
}

pub fn keep_hand_edits_with(
    config_dir: &Path,
    shipped: &[(&str, &str)],
    renames: &[(&str, &str, &str)],
    version: &str,
) -> Kept {
    let mut kept = Kept::default();
    kept.notices.extend(rename_preferences(config_dir, renames));
    // Before any file is brought up to date: the shipped value under each
    // Settings choice is read from what was shipped *before* this version.
    record_setting_defaults(config_dir);

    for (file, new_text) in shipped {
        if let Some(n) = keep_one(config_dir, file, new_text, renames) {
            kept.notices.push(n);
        }
    }

    // Once per version: the defaults this release changed under your edits
    // and under your Settings choices.
    let told = config_dir.join(LOCAL_DIR).join("told-about-version");
    let already = std::fs::read_to_string(&told).map(|s| s.trim() == version).unwrap_or(false);
    let mut anything_of_yours = false;
    for (file, new_text) in shipped {
        let Ok(changes) = load_overlay(config_dir, file) else { continue };
        if changes.is_empty() {
            continue;
        }
        anything_of_yours = true;
        kept.total += changes.len();
        if already {
            continue;
        }
        let Ok(new) = serde_yaml::from_str::<Value>(new_text) else { continue };
        let moved: Vec<String> = conflicts(&new, &changes).iter().map(|c| c.dotted()).collect();
        if !moved.is_empty() {
            kept.notices.push(format!(
                "This version changed the default for {} in config/{file}, which you had set yourself. \
                 I kept yours. To take the new default, delete {} from config/{LOCAL_DIR}/{file}, \
                 or press \"back to default\" on the hub's Your edits page.",
                moved.join(", "),
                if moved.len() == 1 { "that entry" } else { "those entries" }
            ));
        }
    }
    let was = load_settings_was(config_dir);
    if !was.is_empty() {
        anything_of_yours = true;
        if !already {
            if let Some((_, tools)) = shipped.iter().find(|(f, _)| *f == "tools.yaml") {
                if let Ok(new) = serde_yaml::from_str::<Value>(tools) {
                    let moved: Vec<&String> = was
                        .iter()
                        .filter(|(k, w)| present(get(&new, &key_path(k))) != present(w.as_ref()))
                        .map(|(k, _)| k)
                        .collect();
                    if !moved.is_empty() {
                        kept.notices.push(format!(
                            "This version changed the default for {}, which you had chosen in Settings. \
                             Your choice still stands; reset it in Settings to take the new default.",
                            moved.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(", ")
                        ));
                    }
                }
            }
        }
    }
    if anything_of_yours && !already {
        let _ = write_whole(&told, version);
    }
    kept
}

fn keep_one(config_dir: &Path, file: &str, new_text: &str, renames: &[(&str, &str, &str)]) -> Option<String> {
    let disk_path = config_dir.join(file);
    let base_file = base_path(config_dir, file);

    // Renames first, so edits kept under an old name follow before anything
    // is compared.
    let mut kept_changes = match load_overlay(config_dir, file) {
        Ok(c) => c,
        Err(e) => {
            // Your overlay will not parse. Touch nothing for this file --
            // merging into it would overwrite it, and replacing the shipped
            // file could lose an edit not yet captured.
            return Some(format!("{e}. I left config/{file} exactly as it is until that is fixed."));
        }
    };
    if rename_paths(&mut kept_changes, file, renames) {
        if let Err(e) = save_overlay(config_dir, file, &kept_changes) {
            return Some(format!("A setting in config/{file} was renamed, and I couldn't move your edit: {e}"));
        }
    }

    let disk_text = match std::fs::read_to_string(&disk_path) {
        Ok(t) => t,
        // Missing: first launch writes it. Nothing of yours is in a file
        // that is not there.
        Err(_) => return None,
    };
    if disk_text == new_text {
        if std::fs::read_to_string(&base_file).ok().as_deref() != Some(new_text) {
            let _ = write_whole(&base_file, new_text);
        }
        return None;
    }

    let Ok(disk) = serde_yaml::from_str::<Value>(&disk_text) else {
        return Some(format!(
            "{} does not parse, so I left it as it is and couldn't bring it up to this version.",
            disk_path.display()
        ));
    };
    let Ok(new) = serde_yaml::from_str::<Value>(new_text) else { return None };
    let base_text = std::fs::read_to_string(&base_file).ok();
    let base = base_text.as_deref().and_then(|t| serde_yaml::from_str::<Value>(t).ok());
    let unsure = base.is_none();
    let base = base.unwrap_or_else(|| new.clone());

    let mut captured = diff(&base, &disk);
    if captured.is_empty() && base_text.as_deref() == Some(new_text) {
        // Only comments or spacing differ, and the release did not change
        // this file. Nothing to carry; leave your file alone.
        return None;
    }
    for c in &mut captured {
        c.unsure = unsure;
    }
    let count = captured.len();

    // Order matters for a crash halfway: the overlay is written first, so a
    // restart finds the edit either still in the file or already kept, never
    // in neither.
    if count > 0 {
        merge(&mut kept_changes, captured.clone());
        if let Err(e) = save_overlay(config_dir, file, &kept_changes) {
            return Some(format!("I couldn't keep your edits to config/{file} ({e}), so I left it as it is."));
        }
    }
    if base_text.as_deref() != Some(disk_text.as_str()) {
        if let Err(e) = write_whole(&previous_path(config_dir, file), &disk_text) {
            return Some(format!("I couldn't save a copy of your config/{file} ({e}), so I left it as it is."));
        }
    }
    if let Err(e) = write_whole(&disk_path, new_text) {
        return Some(format!("I couldn't bring config/{file} up to this version: {e}"));
    }
    let _ = write_whole(&base_file, new_text);

    if count == 0 {
        return None;
    }
    let which: Vec<String> = captured.iter().map(|c| c.dotted()).collect();
    Some(if unsure {
        format!(
            "Your config/{file} differed from what I ship ({}). I couldn't tell whether that was you or an \
             older version of me, so I kept it all in config/{LOCAL_DIR}/{file} -- delete any entry that \
             wasn't yours and you'll get the current default.",
            which.join(", ")
        )
    } else {
        format!(
            "I moved your edit{} to config/{file} ({}) into config/{LOCAL_DIR}/{file}, so updates keep {}.",
            if count == 1 { "" } else { "s" },
            which.join(", "),
            if count == 1 { "it" } else { "them" }
        )
    })
}

// ------------------------------------------------------------------ tests

#[cfg(test)]
mod tests {
    use super::*;

    fn y(s: &str) -> Value {
        serde_yaml::from_str(s).unwrap()
    }

    fn scratch(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("atlas-yourchanges-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    const V1: &str = "# shipped v1\nask_before:\n  delete: true\n  send: true\nlimit: 5\nfolders: [a, b]\n";
    const V2: &str = "# shipped v2\nask_before:\n  delete: true\n  send: true\n  buy: true\nlimit: 10\nfolders: [a, b]\n";

    #[test]
    fn diff_records_only_what_changed_and_what_it_replaced() {
        let d = diff(&y(V1), &y("ask_before:\n  delete: false\n  send: true\nlimit: 5\nfolders: [a, b, c]\nextra: 1\n"));
        let paths: Vec<String> = d.iter().map(|c| c.dotted()).collect();
        assert_eq!(paths, ["ask_before.delete", "folders", "extra"]);
        assert_eq!(d[0].was, Some(Value::Bool(true)));
        assert_eq!(d[0].yours, Some(Value::Bool(false)));
        assert_eq!(d[2].was, None);
    }

    #[test]
    fn diff_records_a_deleted_setting() {
        let d = diff(&y(V1), &y("ask_before:\n  delete: true\nlimit: 5\nfolders: [a, b]\n"));
        assert_eq!(d.len(), 1);
        assert!(d[0].removed);
        assert_eq!(d[0].dotted(), "ask_before.send");
    }

    #[test]
    fn a_default_you_never_touched_follows_the_release_and_yours_stays_yours() {
        let mine = diff(&y(V1), &y("ask_before:\n  delete: false\n  send: true\nlimit: 5\nfolders: [a, b]\n"));
        let mut new = y(V2);
        assert!(apply(&mut new, &mine).is_empty());
        assert_eq!(get(&new, &["ask_before".into(), "delete".into()]), Some(&Value::Bool(false)));
        // Untouched by you, changed by the release: the release wins.
        assert_eq!(get(&new, &["limit".into()]), Some(&y("10")));
        // New in the release: arrives.
        assert_eq!(get(&new, &["ask_before".into(), "buy".into()]), Some(&Value::Bool(true)));
    }

    #[test]
    fn a_setting_whose_section_the_release_removed_is_reported_not_recreated() {
        let mine = vec![Change {
            path: vec!["gone".into(), "x".into()],
            yours: Some(y("1")),
            removed: false,
            was: Some(y("0")),
            unsure: false,
        }];
        let mut new = y(V2);
        let unplaced = apply(&mut new, &mine);
        assert_eq!(unplaced.len(), 1);
        assert!(get(&new, &["gone".into()]).is_none());
    }

    #[test]
    fn a_default_the_release_moved_under_your_edit_is_a_conflict() {
        let mine = diff(&y(V1), &y("ask_before:\n  delete: true\n  send: true\nlimit: 3\nfolders: [a, b]\n"));
        let c = conflicts(&y(V2), &mine);
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].dotted(), "limit");
        // Against the file it was made on, nothing moved.
        assert!(conflicts(&y(V1), &mine).is_empty());
    }

    #[test]
    fn a_later_change_replaces_an_earlier_one_at_or_below_it() {
        let mut kept = diff(&y("a:\n  b: 1\n  c: 2\n"), &y("a:\n  b: 5\n  c: 6\n"));
        merge(&mut kept, diff(&y("a:\n  b: 1\n  c: 2\n"), &y("a: 7\n")));
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].dotted(), "a");
    }

    #[test]
    fn an_update_keeps_a_hand_edit_and_takes_the_new_defaults() {
        let dir = scratch("update");
        // Release 1 is installed, and you edit its file by hand.
        let r1 = [("policy.yaml", V1)];
        std::fs::write(dir.join("policy.yaml"), V1).unwrap();
        keep_hand_edits_with(&dir, &r1, &[], "1");
        std::fs::write(dir.join("policy.yaml"), V1.replace("delete: true", "delete: false")).unwrap();

        // Release 2 arrives.
        let r2 = [("policy.yaml", V2)];
        let k = keep_hand_edits_with(&dir, &r2, &[], "2");
        assert!(k.notices.iter().any(|n| n.contains("ask_before.delete")), "{:?}", k.notices);
        assert_eq!(k.total, 1);
        // The shipped file is exactly release 2, comments and all.
        assert_eq!(std::fs::read_to_string(dir.join("policy.yaml")).unwrap(), V2);
        // Your old file is kept byte for byte.
        assert!(std::fs::read_to_string(previous_path(&dir, "policy.yaml")).unwrap().contains("delete: false"));

        let mut loaded = y(V2);
        apply(&mut loaded, &load_overlay(&dir, "policy.yaml").unwrap());
        assert_eq!(get(&loaded, &["ask_before".into(), "delete".into()]), Some(&Value::Bool(false)));
        assert_eq!(get(&loaded, &["limit".into()]), Some(&y("10")));

        // A second start changes nothing and says nothing.
        let again = keep_hand_edits_with(&dir, &r2, &[], "2");
        assert!(again.notices.is_empty(), "{:?}", again.notices);
        assert_eq!(again.total, 1);
    }

    #[test]
    fn an_older_release_file_with_no_edits_is_simply_brought_up_to_date() {
        let dir = scratch("pristine");
        std::fs::write(dir.join("policy.yaml"), V1).unwrap();
        keep_hand_edits_with(&dir, &[("policy.yaml", V1)], &[], "1");
        let k = keep_hand_edits_with(&dir, &[("policy.yaml", V2)], &[], "2");
        assert!(k.notices.is_empty(), "{:?}", k.notices);
        assert_eq!(k.total, 0);
        assert_eq!(std::fs::read_to_string(dir.join("policy.yaml")).unwrap(), V2);
        assert!(!overlay_path(&dir, "policy.yaml").exists());
    }

    #[test]
    fn with_no_base_on_record_differences_are_kept_and_marked_unsure() {
        let dir = scratch("unsure");
        std::fs::write(dir.join("policy.yaml"), V1).unwrap();
        let k = keep_hand_edits_with(&dir, &[("policy.yaml", V2)], &[], "2");
        assert!(k.notices.iter().any(|n| n.contains("couldn't tell")), "{:?}", k.notices);
        let kept = load_overlay(&dir, "policy.yaml").unwrap();
        assert!(!kept.is_empty() && kept.iter().all(|c| c.unsure));
    }

    #[test]
    fn a_comment_only_edit_is_left_alone_when_the_release_did_not_change_the_file() {
        let dir = scratch("comment");
        std::fs::write(dir.join("policy.yaml"), V1).unwrap();
        keep_hand_edits_with(&dir, &[("policy.yaml", V1)], &[], "1");
        let mine = format!("# my note\n{V1}");
        std::fs::write(dir.join("policy.yaml"), &mine).unwrap();
        let k = keep_hand_edits_with(&dir, &[("policy.yaml", V1)], &[], "1");
        assert!(k.notices.is_empty());
        assert_eq!(std::fs::read_to_string(dir.join("policy.yaml")).unwrap(), mine);
    }

    #[test]
    fn an_unreadable_overlay_means_nothing_is_touched() {
        let dir = scratch("broken");
        std::fs::write(dir.join("policy.yaml"), V1).unwrap();
        keep_hand_edits_with(&dir, &[("policy.yaml", V1)], &[], "1");
        let edited = V1.replace("limit: 5", "limit: 1");
        std::fs::write(dir.join("policy.yaml"), &edited).unwrap();
        write_whole(&overlay_path(&dir, "policy.yaml"), "changes: [this is: not: valid").unwrap();
        let k = keep_hand_edits_with(&dir, &[("policy.yaml", V2)], &[], "2");
        assert!(k.notices.iter().any(|n| n.contains("does not parse")), "{:?}", k.notices);
        assert_eq!(std::fs::read_to_string(dir.join("policy.yaml")).unwrap(), edited);
    }

    #[test]
    fn a_shipped_file_that_does_not_parse_is_left_alone() {
        let dir = scratch("unparsed");
        std::fs::write(dir.join("policy.yaml"), "a: [unclosed").unwrap();
        let k = keep_hand_edits_with(&dir, &[("policy.yaml", V2)], &[], "2");
        assert!(k.notices.iter().any(|n| n.contains("does not parse")));
        assert_eq!(std::fs::read_to_string(dir.join("policy.yaml")).unwrap(), "a: [unclosed");
    }

    #[test]
    fn the_conflict_notice_is_given_once_per_version() {
        let dir = scratch("once");
        std::fs::write(dir.join("policy.yaml"), V1).unwrap();
        keep_hand_edits_with(&dir, &[("policy.yaml", V1)], &[], "1");
        std::fs::write(dir.join("policy.yaml"), V1.replace("limit: 5", "limit: 3")).unwrap();
        keep_hand_edits_with(&dir, &[("policy.yaml", V1)], &[], "1");
        let first = keep_hand_edits_with(&dir, &[("policy.yaml", V2)], &[], "2");
        assert!(first.notices.iter().any(|n| n.contains("changed the default for limit")), "{:?}", first.notices);
        let second = keep_hand_edits_with(&dir, &[("policy.yaml", V2)], &[], "2");
        assert!(second.notices.is_empty(), "{:?}", second.notices);
    }

    #[test]
    fn a_renamed_setting_carries_your_edit_and_your_choice_with_it() {
        let dir = scratch("rename");
        write_whole(
            &overlay_path(&dir, "tools.yaml"),
            &serde_yaml::to_string(&OverlayFile {
                changes: vec![Change {
                    path: vec!["old".into(), "x".into()],
                    yours: Some(y("1")),
                    removed: false,
                    was: None,
                    unsure: false,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        std::fs::write(dir.join("tools.yaml"), "new:\n  x: 0\n  y: false\n").unwrap();
        let mut p = crate::preferences::Preferences::default();
        p.set("old.y", "on");
        p.save(&dir).unwrap();

        let k = keep_hand_edits_with(&dir, &[("tools.yaml", "new:\n  x: 0\n  y: false\n")], &[("tools.yaml", "old", "new")], "2");
        assert!(k.notices.iter().any(|n| n.contains("new.y")), "{:?}", k.notices);
        assert_eq!(crate::preferences::Preferences::load(&dir).chosen.get("new.y").map(String::as_str), Some("on"));
        let mut kept = load_overlay(&dir, "tools.yaml").unwrap();
        assert!(!rename_paths(&mut kept, "tools.yaml", &[("tools.yaml", "old", "new")]));
        assert_eq!(kept[0].dotted(), "new.x");
    }

    #[test]
    fn every_shipped_settings_file_is_covered() {
        let covered: Vec<&str> = shipped_yaml().iter().map(|(n, _)| *n).collect();
        for (path, _) in crate::upgrade::SHIPPED {
            let name = path.strip_prefix("config/").unwrap();
            assert!(covered.contains(&name), "{name} ships but hand edits to it would not survive an update");
        }
        assert!(shipped_yaml_from(&[("labels/x.yaml", ""), ("a.txt", ""), ("b.yaml", "")]) == vec![("b.yaml", "")]);
    }

    #[test]
    fn a_release_that_moves_a_default_you_chose_in_settings_says_so_once() {
        let dir = scratch("settingswas");
        let t1 = "voice:\n  rate: 5\n  pitch: 1\n";
        std::fs::write(dir.join("tools.yaml"), t1).unwrap();
        keep_hand_edits_with(&dir, &[("tools.yaml", t1)], &[], "1");
        let mut p = crate::preferences::Preferences::default();
        p.set("voice.rate", "7");
        p.save(&dir).unwrap();
        // Start again on the same version: the default is recorded, nothing to say.
        assert!(keep_hand_edits_with(&dir, &[("tools.yaml", t1)], &[], "1").notices.is_empty());
        assert_eq!(load_settings_was(&dir).get("voice.rate"), Some(&Some(y("5"))));

        // A release that changes the rate default, and one that doesn't.
        let t2 = "voice:\n  rate: 6\n  pitch: 1\n";
        let k = keep_hand_edits_with(&dir, &[("tools.yaml", t2)], &[], "2");
        assert!(k.notices.iter().any(|n| n.contains("voice.rate") && n.contains("Settings")), "{:?}", k.notices);
        assert!(keep_hand_edits_with(&dir, &[("tools.yaml", t2)], &[], "2").notices.is_empty());

        // Clearing the choice drops the record.
        p.clear("voice.rate");
        p.save(&dir).unwrap();
        keep_hand_edits_with(&dir, &[("tools.yaml", t2)], &[], "2");
        assert!(load_settings_was(&dir).is_empty());
    }

    #[test]
    fn a_kept_edit_can_be_listed_and_given_up() {
        let dir = scratch("forget");
        write_whole(
            &overlay_path(&dir, "policy.yaml"),
            &serde_yaml::to_string(&OverlayFile {
                changes: vec![Change {
                    path: vec!["min_samples".into()],
                    yours: Some(y("9")),
                    removed: false,
                    was: Some(y("4")),
                    unsure: false,
                }],
            })
            .unwrap(),
        )
        .unwrap();
        let (kept, problems) = all_kept(&dir);
        assert!(problems.is_empty());
        let e = kept.iter().find(|e| e.file == "policy.yaml").expect("listed");
        assert_eq!(shown(e.change.yours.as_ref()), "9");
        // The real shipped policy.yaml says something other than 4 (or nothing).
        assert!(e.default_moved() || e.shipped_now.is_some());
        assert_eq!(forget(&dir, "policy.yaml", "nothing.here"), Ok(false));
        assert_eq!(forget(&dir, "policy.yaml", "min_samples"), Ok(true));
        assert!(all_kept(&dir).0.is_empty());
        assert!(forget(&dir, "../secrets.yaml", "x").is_err());
    }

    #[test]
    fn every_file_written_at_first_launch_is_one_whose_edits_are_kept() {
        for (name, _) in crate::firstlaunch::DEFAULT_CONFIG {
            assert!(
                shipped_yaml().iter().any(|(n, _)| n == name),
                "{name} is written out for you to edit, but an edit to it would not survive an update"
            );
        }
    }

    #[test]
    fn the_rename_list_is_sane() {
        let mut from_seen = Vec::new();
        for (file, from, to) in crate::upgrade::RENAMED_SETTINGS {
            assert!(shipped_yaml().iter().any(|(n, _)| n == file), "{file} is not a shipped file");
            assert!(!from.is_empty() && !to.is_empty() && from != to, "bad rename {from} -> {to}");
            assert!(!from_seen.contains(&(file, from)), "{from} in {file} is renamed twice");
            from_seen.push((file, from));
        }
    }

    #[test]
    fn the_source_tree_is_never_rewritten() {
        let here = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("config");
        assert!(is_source_tree(&here));
        assert_eq!(keep_hand_edits(&here), Kept::default());
        assert!(!is_source_tree(&scratch("not-source")));
    }

    #[test]
    fn the_real_shipped_files_parse_so_edits_to_them_can_be_kept() {
        for (name, text) in shipped_yaml() {
            assert!(serde_yaml::from_str::<Value>(text).is_ok(), "{name} does not parse");
        }
    }
}
