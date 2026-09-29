//! Choosing how to touch the workspace.
//!
//! There is no single best mechanism. Each one is good at something and bad at
//! something else, so Atlas keeps all of them and picks per request:
//!
//! | backend        | works on            | steals focus | needs its own copy |
//! |----------------|---------------------|--------------|--------------------|
//! | Cdp            | Chrome pages        | no           | optional           |
//! | Uia            | cooperating apps    | rarely       | no                 |
//! | HiddenDesktop  | literally anything  | never        | yes                |
//! | SendInput      | anything visible    | always       | no                 |
//!
//! The routing rule is: cheapest backend that can do the job, on this app,
//! given whether we are allowed to interrupt. And it **learns** — a backend
//! that keeps failing on Discord stops being first choice for Discord,
//! without being written off everywhere else.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    /// Chrome DevTools Protocol. Precise, scriptable, cheap to be right about.
    Cdp,
    /// Windows accessibility tree. Reads controls without a screenshot.
    Uia,
    /// A separate invisible desktop running its own copy of an app.
    HiddenDesktop,
    /// Synthetic mouse and keyboard. Universal and disruptive.
    SendInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    ReadText,
    Click,
    Scroll,
    TypeText,
    FillForm,
    Navigate,
    WaitForElement,
    ReadWindowTitle,
}

#[derive(Debug, Clone)]
pub struct Spec {
    pub backend: Backend,
    pub can: Vec<Capability>,
    /// Resident memory while running, megabytes.
    pub memory_mb: u64,
    /// Cold-start cost, milliseconds.
    pub startup_ms: u64,
    /// Takes the foreground away from you.
    pub steals_focus: bool,
    /// Acts on a separate copy, not the window you have open.
    pub own_instance: bool,
    /// Only applies to these apps. Empty means any.
    pub only_for: Vec<String>,
}

// Private: only `Router::new` below builds from it, and nothing outside this
// module has ever asked for the spec table directly. It was `pub` from the
// moment the module was written, which the untested-helper count in
// `dead_capabilities.rs` correctly reads as surface area nothing needs.
fn default_specs() -> Vec<Spec> {
    use Capability::*;
    vec![
        Spec {
            backend: Backend::Cdp,
            can: vec![ReadText, Click, Scroll, TypeText, FillForm, Navigate, WaitForElement],
            memory_mb: 180,
            startup_ms: 900,
            steals_focus: false,
            own_instance: true,
            only_for: vec!["chrome".into()],
        },
        Spec {
            backend: Backend::Uia,
            can: vec![ReadText, ReadWindowTitle, Click, TypeText],
            memory_mb: 25,
            startup_ms: 60,
            steals_focus: false,
            own_instance: false,
            only_for: vec![],
        },
        Spec {
            backend: Backend::HiddenDesktop,
            can: vec![ReadText, Click, Scroll, TypeText, FillForm, Navigate, WaitForElement],
            memory_mb: 300,
            startup_ms: 4000,
            steals_focus: false,
            own_instance: true,
            only_for: vec![],
        },
        Spec {
            backend: Backend::SendInput,
            can: vec![Click, Scroll, TypeText],
            memory_mb: 0,
            startup_ms: 0,
            steals_focus: true,
            own_instance: false,
            only_for: vec![],
        },
    ]
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Outcome {
    pub tries: u32,
    pub wins: u32,
}

impl Outcome {
    /// Optimistic when untried: a backend deserves a chance before judgement.
    /// Laplace smoothing, so one failure is a dent rather than a death.
    pub fn rate(&self) -> f32 {
        (self.wins as f32 + 1.0) / (self.tries as f32 + 2.0)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Learned {
    /// Keyed "backend/app", because a backend can be great in Chrome and
    /// useless in Discord.
    pub outcomes: BTreeMap<String, Outcome>,
}

impl Learned {
    fn key(b: Backend, app: &str) -> String {
        format!("{b:?}/{}", app.to_lowercase())
    }

    pub fn record(&mut self, b: Backend, app: &str, ok: bool) {
        let e = self.outcomes.entry(Self::key(b, app)).or_default();
        e.tries += 1;
        if ok {
            e.wins += 1;
        }
    }

    pub fn rate(&self, b: Backend, app: &str) -> f32 {
        self.outcomes
            .get(&Self::key(b, app))
            .map(Outcome::rate)
            .unwrap_or(0.5)
    }

    fn tries(&self, b: Backend, app: &str) -> u32 {
        self.outcomes.get(&Self::key(b, app)).map(|o| o.tries).unwrap_or(0)
    }

    /// Forget everything about one app — e.g. after it updates and its
    /// accessibility support changes.
    pub fn forget_app(&mut self, app: &str) {
        let suffix = format!("/{}", app.to_lowercase());
        self.outcomes.retain(|k, _| !k.ends_with(&suffix));
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Request<'a> {
    pub capability: Capability,
    pub app: &'a str,
    /// May we take the screen? False while you are working.
    pub may_steal_focus: bool,
    /// Must act on YOUR window, not a fresh copy — "what's on my screen".
    pub must_be_your_window: bool,
}

pub struct Router {
    pub specs: Vec<Spec>,
    pub learned: Learned,
    /// Below this success rate a backend is skipped when something else fits.
    pub distrust_below: f32,
    /// How many attempts before a poor rate counts as evidence. One failure is
    /// a bad day, not a verdict — and demoting on it would swap a 25MB backend
    /// for a 300MB one on the strength of a single stumble.
    pub min_tries_to_distrust: u32,
}

impl Default for Router {
    fn default() -> Self {
        Router {
            specs: default_specs(),
            learned: Learned::default(),
            distrust_below: 0.35,
            min_tries_to_distrust: 4,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub backend: Backend,
    pub why: String,
}

impl Router {
    /// The store file for the learned outcomes. Only the learning persists —
    /// `specs` and the thresholds are static config rebuilt from defaults, so
    /// changing them in code takes effect without a migration.
    const FILE: &'static str = "backends";

    /// Load a router whose learned outcomes come from the store and whose
    /// specs/thresholds are the current defaults.
    pub fn load(store: &crate::store::Store) -> Router {
        Router { learned: store.load::<Learned>(Self::FILE), ..Router::default() }
    }

    /// Persist only what was learned.
    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(Self::FILE, &self.learned)
    }

    /// A plain-language account of what has been learned about reading each
    /// app — which the accessibility backend reads cleanly, and which it
    /// keeps failing on. This is the reader that consumes what the daemon's
    /// window-reads record, so the learning is surfaced rather than only
    /// written. One line per app that has been tried enough to mean anything.
    pub fn what_ive_learned(&self) -> Vec<String> {
        let mut lines = Vec::new();
        for (key, outcome) in &self.learned.outcomes {
            // key is "Backend/app"; report per app, only once there is
            // evidence (a single try is a bad day, not a verdict).
            if outcome.tries < self.min_tries_to_distrust {
                continue;
            }
            let (backend, app) = key.split_once('/').unwrap_or(("?", key.as_str()));
            let rate = outcome.rate();
            let verdict = if rate >= 0.8 {
                "reads cleanly"
            } else if rate >= self.distrust_below {
                "reads sometimes"
            } else {
                "can't read it"
            };
            lines.push(format!(
                "{app}: {verdict} with {backend} ({}/{} reads)",
                outcome.wins, outcome.tries
            ));
        }
        lines.sort();
        lines
    }

    /// Forget everything learned about one app — e.g. after it updates and
    /// its accessibility support changes. Passes through to the learned map.
    pub fn forget_app(&mut self, app: &str) {
        self.learned.forget_app(app);
    }

    fn eligible(&self, spec: &Spec, r: &Request) -> bool {
        if !spec.can.contains(&r.capability) {
            return false;
        }
        if !spec.only_for.is_empty() && !spec.only_for.iter().any(|a| a == r.app) {
            return false;
        }
        if spec.steals_focus && !r.may_steal_focus {
            return false;
        }
        if spec.own_instance && r.must_be_your_window {
            return false;
        }
        true
    }

    /// Best backend for this request, or None if nothing can do it under the
    /// constraints — which is a real answer, not a failure.
    pub fn choose(&self, r: &Request) -> Option<Choice> {
        let mut viable: Vec<(&Spec, f32)> = self
            .specs
            .iter()
            .filter(|s| self.eligible(s, r))
            .map(|s| (s, self.learned.rate(s.backend, r.app)))
            .collect();
        if viable.is_empty() {
            return None;
        }

        // Prefer trusted ones, but never rule the last option out entirely.
        let trusted: Vec<(&Spec, f32)> = viable
            .iter()
            .filter(|(spec, rate)| {
                *rate >= self.distrust_below
                    || self.learned.tries(spec.backend, r.app) < self.min_tries_to_distrust
            })
            .cloned()
            .collect();
        if !trusted.is_empty() {
            viable = trusted;
        }

        self.rank(&mut viable);

        let (spec, rate) = viable[0];
        let tries = self.learned.tries(spec.backend, r.app);
        let why = if tries == 0 {
            format!("{:?}: cheapest option that can {:?} on {}", spec.backend, r.capability, r.app)
        } else {
            format!(
                "{:?}: {}/{} successful on {} ({}% )",
                spec.backend,
                self.learned.outcomes.get(&Learned::key(spec.backend, r.app)).map(|o| o.wins).unwrap_or(0),
                tries,
                r.app,
                (rate * 100.0).round()
            )
        };
        Some(Choice { backend: spec.backend, why })
    }

    /// Ordered fallbacks, so a failure retries with the next best rather than
    /// giving up.
    pub fn ladder(&self, r: &Request) -> Vec<Backend> {
        let mut viable: Vec<(&Spec, f32)> = self
            .specs
            .iter()
            .filter(|s| self.eligible(s, r))
            .map(|s| (s, self.learned.rate(s.backend, r.app)))
            .collect();
        self.rank(&mut viable);
        viable.into_iter().map(|(s, _)| s.backend).collect()
    }

    /// Score, not lexicographic sort.
    ///
    /// Ranking by success rate alone has a nasty failure: an untried 300MB
    /// backend outranks a 25MB one that failed once, because an untried
    /// backend carries an optimistic prior. Cost has to be part of the same
    /// score rather than a tiebreaker, so a single stumble doesn't promote
    /// something twelve times heavier.
    fn rank(&self, viable: &mut [(&Spec, f32)]) {
        let max_cost = self.specs.iter().map(cost).max().unwrap_or(1).max(1) as f32;
        viable.sort_by(|a, b| {
            let sa = a.1 - (cost(a.0) as f32 / max_cost) * COST_INFLUENCE;
            let sb = b.1 - (cost(b.0) as f32 / max_cost) * COST_INFLUENCE;
            sb.partial_cmp(&sa)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.0.backend.cmp(&b.0.backend))
        });
    }

    pub fn record(&mut self, b: Backend, app: &str, ok: bool) {
        self.learned.record(b, app, ok);
    }
}

/// How much weight cheapness gets against reliability. High enough that a
/// heavyweight backend must be clearly better to win, low enough that a
/// backend which genuinely does not work gets dropped anyway.
const COST_INFLUENCE: f32 = 0.25;

/// Rough total cost of using a backend once: memory plus startup delay.
fn cost(s: &Spec) -> u64 {
    s.memory_mb + s.startup_ms / 100
}
