//! Making Atlas fit whatever machine it lands on.
//!
//! Everything Atlas needs to know about a computer — where the apps live, what
//! the monitors are, which microphone hears you — is different on every
//! machine, and none of it belongs in a config file that ships to someone
//! else. A config with `C:/Users/erics/` in it is broken for everybody except
//! Eric.
//!
//! So configuration comes in two layers:
//!
//! * **`config/*.yaml`** — generic. Ships to anyone. Uses `%LOCALAPPDATA%`
//!   style placeholders and never a literal username.
//! * **`config/machine.yaml`** — written by Atlas on first run, never shipped.
//!   App paths it found, monitor geometry, the microphone that hears you.
//!
//! The generic layer is the recipe. The machine layer is what this kitchen
//! actually has.

use crate::error::Result;
use crate::platform::{Monitor, Platform};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Machine {
    /// When this was worked out.
    pub detected_at: u64,
    /// App name to how to launch it. A path, or a Store app id.
    #[serde(default)]
    pub apps: BTreeMap<String, AppFound>,
    #[serde(default)]
    pub monitors: Vec<MonitorFact>,
    /// Device names, exactly as the system reports them.
    #[serde(default)]
    pub audio_inputs: Vec<String>,
    #[serde(default)]
    pub audio_outputs: Vec<String>,
    /// Tool name to where it was found.
    #[serde(default)]
    pub tools: BTreeMap<String, String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppFound {
    pub launch: String,
    #[serde(default)]
    pub store: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MonitorFact {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

impl From<&Monitor> for MonitorFact {
    fn from(m: &Monitor) -> Self {
        MonitorFact { id: m.id, x: m.x, y: m.y, width: m.width, height: m.height, primary: m.primary }
    }
}

impl Machine {
    pub fn load(dir: &Path) -> Option<Machine> {
        let text = std::fs::read_to_string(dir.join("machine.yaml")).ok()?;
        serde_yaml::from_str(&text).ok()
    }

    pub fn save(&self, dir: &Path) -> Result<()> {
        std::fs::create_dir_all(dir)?;
        let header = "# Written by `atlas adapt`. Specific to this computer.\n\
                      # Do not share this file — it holds your paths and device names.\n\
                      # Delete it and run `atlas adapt` again after changing your hardware.\n";
        let body = serde_yaml::to_string(self).unwrap_or_default();
        std::fs::write(dir.join("machine.yaml"), format!("{header}{body}"))?;
        Ok(())
    }

    /// How complete is this? A machine file missing half the apps is worth
    /// re-running setup for.
    pub fn coverage(&self, wanted_apps: &[String]) -> f32 {
        if wanted_apps.is_empty() {
            return 1.0;
        }
        let found = wanted_apps.iter().filter(|a| self.apps.contains_key(*a)).count();
        found as f32 / wanted_apps.len() as f32
    }

    /// Has the hardware changed since this was written?
    pub fn matches(&self, monitors: &[Monitor]) -> bool {
        if self.monitors.len() != monitors.len() {
            return false;
        }
        monitors
            .iter()
            .all(|m| self.monitors.iter().any(|f| f.x == m.x && f.width == m.width))
    }

    pub fn summary(&self) -> String {
        format!(
            "{} app{}, {} display{}, {} microphone{}.",
            self.apps.len(),
            if self.apps.len() == 1 { "" } else { "s" },
            self.monitors.len(),
            if self.monitors.len() == 1 { "" } else { "s" },
            self.audio_inputs.len(),
            if self.audio_inputs.len() == 1 { "" } else { "s" }
        )
    }
}

/// Detect what this machine has.
///
/// `find_app` is injected so the whole thing is testable without a real
/// filesystem, and so the search logic stays in one place.
pub fn detect(
    plat: &dyn Platform,
    app_names: &[String],
    find_app: &dyn Fn(&str) -> Option<AppFound>,
    audio_inputs: Vec<String>,
    audio_outputs: Vec<String>,
    find_tool: &dyn Fn(&str) -> Option<String>,
    tool_names: &[String],
    t: u64,
) -> Machine {
    let mut m = Machine { detected_at: t, ..Default::default() };

    if let Ok(mons) = plat.monitors() {
        m.monitors = mons.iter().map(MonitorFact::from).collect();
    }
    for name in app_names {
        if let Some(found) = find_app(name) {
            m.apps.insert(name.clone(), found);
        } else {
            m.notes.push(format!("couldn't find {name}"));
        }
    }
    for t in tool_names {
        if let Some(path) = find_tool(t) {
            m.tools.insert(t.clone(), path);
        }
    }
    m.audio_inputs = audio_inputs;
    m.audio_outputs = audio_outputs;
    m
}

/// Replace a literal home directory with a placeholder, so a path found on
/// one machine doesn't hard-code a username into anything shared.
pub fn portable(path: &str) -> String {
    portable_with(path, &|n| std::env::var(n).ok())
}

/// `portable` with the environment passed in, so a test can give it a
/// variable without changing the whole process's environment.
pub fn portable_with(path: &str, env: &dyn Fn(&str) -> Option<String>) -> String {
    let p = path.replace('\\', "/");
    for var in ["LOCALAPPDATA", "APPDATA", "USERPROFILE", "PROGRAMFILES"] {
        if let Some(v) = env(var) {
            let v = v.replace('\\', "/");
            if !v.is_empty() && p.to_lowercase().starts_with(&v.to_lowercase()) {
                return format!("%{var}%{}", &p[v.len()..]);
            }
        }
    }
    p
}

/// Does this config still contain somebody's actual home directory?
///
/// The check that stops a personal path shipping to a friend. Run against the
/// generic layer, which should never contain one.
pub fn leaks_a_username(text: &str) -> Option<String> {
    let lower = text.to_lowercase().replace('\\', "/");
    let marker = "/users/";
    let mut from = 0;
    while let Some(i) = lower[from..].find(marker) {
        let start = from + i + marker.len();
        let name: String =
            lower[start..].chars().take_while(|c| c.is_alphanumeric() || *c == '-' || *c == '_').collect();
        // "Public" and "Default" are the same on every Windows machine.
        if !name.is_empty() && !matches!(name.as_str(), "public" | "default" | "all" | "you") {
            return Some(name);
        }
        from = start;
    }
    None
}

/// Apply the machine layer over the generic one.
pub fn apply(cfg: &mut crate::config::Config, m: &Machine) -> usize {
    let mut applied = 0;
    for (name, found) in &m.apps {
        if let Some(spec) = cfg.apps.apps.get_mut(name) {
            spec.launch = found.launch.clone();
            spec.store = found.store;
            applied += 1;
        }
    }
    applied
}

/// The monitor fixture, so dry runs match the real desk.
pub fn monitor_fixture(m: &Machine) -> String {
    let mut s = String::from("fn fake_monitors() -> Vec<Monitor> {\n    vec![\n");
    for mon in &m.monitors {
        s.push_str(&format!(
            "        Monitor {{ id: {}, x: {}, y: {}, width: {}, height: {}, primary: {} }},\n",
            mon.id, mon.x, mon.y, mon.width, mon.height, mon.primary
        ));
    }
    s.push_str("    ]\n}\n");
    s
}

/// What Atlas says after working out where it is.
pub fn first_run_message(m: &Machine, wanted: &[String]) -> String {
    let missing: Vec<&String> = wanted.iter().filter(|a| !m.apps.contains_key(*a)).collect();
    let mut s = format!("Had a look around. {}", m.summary());
    if !missing.is_empty() {
        s.push_str(&format!(
            " I couldn't find {}.",
            missing.iter().map(|a| a.as_str()).collect::<Vec<_>>().join(", ")
        ));
    }
    if m.audio_inputs.is_empty() {
        s.push_str(" No microphone, so we're typing for now.");
    }
    s
}
