//! Context acquisition by moving the workspace.
//!
//! When you ask something Atlas can't answer from the focused window, it can
//! go and look: bring another window forward, capture it, and put your focus
//! back where it was. You get the answer without losing your place.
//!
//! Two rules make this safe to do without asking every time. It only ever
//! *reads* — focus and capture, never a click or a keystroke. And it always
//! restores focus, even when a capture fails partway through.

use crate::config::Config;
use crate::error::{AtlasError, Result};
use crate::platform::Platform;
use crate::workspace;

#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Whatever is focused right now. No workspace manipulation.
    Active,
    /// One named app, brought forward first.
    App(String),
    /// Every managed app in turn — the "look at my whole workspace" case.
    Workspace,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Capture {
    pub app: String,
    pub title: String,
    pub path: String,
}

pub struct Probe {
    /// Time for a window to actually come forward and repaint before capture.
    pub settle_ms: u64,
    pub restore_focus: bool,
}

impl Default for Probe {
    fn default() -> Self {
        Probe { settle_ms: 400, restore_focus: true }
    }
}

impl Probe {
    /// `shoot` takes a screenshot and returns its path — injected so this is
    /// testable without a screen.
    pub fn gather(
        &self,
        cfg: &Config,
        plat: &dyn Platform,
        shoot: &dyn Fn() -> Result<String>,
        target: &Target,
    ) -> Result<Vec<Capture>> {
        let was_focused = plat.active_window().unwrap_or(None);

        let apps: Vec<String> = match target {
            Target::Active => Vec::new(),
            Target::App(a) => vec![a.clone()],
            Target::Workspace => cfg
                .apps
                .startup_order
                .iter()
                .filter(|n| {
                    cfg.apps.get(n).map(|s| plat.find_window(s).ok().flatten().is_some()).unwrap_or(false)
                })
                .cloned()
                .collect(),
        };

        let mut out = Vec::new();
        let mut first_err = None;

        if apps.is_empty() {
            match shoot() {
                Ok(path) => out.push(Capture {
                    app: was_focused.as_ref().map(|w| w.process.clone()).unwrap_or_default(),
                    title: was_focused.as_ref().map(|w| w.title.clone()).unwrap_or_default(),
                    path,
                }),
                Err(e) => first_err = Some(e),
            }
        }

        for name in &apps {
            if let Err(e) = workspace::focus_app(cfg, plat, name) {
                first_err.get_or_insert(e);
                continue;
            }
            plat.sleep_ms(self.settle_ms);
            let title = plat.active_window().unwrap_or(None).map(|w| w.title).unwrap_or_default();
            match shoot() {
                Ok(path) => out.push(Capture { app: name.clone(), title, path }),
                Err(e) => {
                    first_err.get_or_insert(e);
                }
            }
        }

        // Always, even on failure. Losing your place is worse than a failed
        // capture.
        if self.restore_focus && !apps.is_empty() {
            if let Some(w) = &was_focused {
                if let Some(name) = app_named(cfg, &w.process) {
                    crate::heard!(workspace::focus_app(cfg, plat, &name));
                }
            }
        }

        if out.is_empty() {
            return Err(first_err
                .unwrap_or_else(|| AtlasError::Platform("nothing could be captured".into())));
        }
        Ok(out)
    }
}

/// Map a running process back to the app name in config.
pub fn app_named(cfg: &Config, process: &str) -> Option<String> {
    let p = process.to_lowercase();
    cfg.apps
        .apps
        .iter()
        .find(|(_, s)| s.process_names.iter().any(|n| n.to_lowercase() == p))
        .map(|(n, _)| n.clone())
}

/// Decide what to look at from what was asked. Deliberately conservative:
/// moving your windows around is only justified when the question is clearly
/// about something other than what is already in front of you.
pub fn target_for(question: &str, cfg: &Config) -> Target {
    let q = question.to_lowercase();
    for name in cfg.apps.apps.keys() {
        if q.contains(&name.to_lowercase()) {
            return Target::App(name.clone());
        }
    }
    if q.contains("workspace") || q.contains("everything") || q.contains("all my") {
        return Target::Workspace;
    }
    Target::Active
}
