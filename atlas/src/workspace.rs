//! workspace_on / workspace_off.
//!
//! Two properties the old scaffold did not have:
//!   1. It waits for a window to actually exist before placing it.
//!   2. A single app failing does not abort the whole sequence — it is
//!      collected and reported, because a half-open workspace you can see is
//!      more useful than an error and three unplaced windows.

use crate::config::Config;
use crate::error::{AtlasError, Result};
use crate::layout::{monitor_for_role, to_pixels};
use crate::platform::Platform;

/// How long a whole bring-up may take, however many apps are in it.
///
/// ## Why there is a cap at all
///
/// Each app has its own budget in `apps.yaml` — `retries` × `poll_ms` — and
/// the shipped file has claude at 30 × 500ms, chrome at 40 × 750ms, and two
/// more besides. Those are per-app and nothing added them up: the sequence
/// could block for a minute and a half, and it blocks on the tick thread,
/// which is the thread that listens, answers, polls the dashboard, checks for
/// a message from another Atlas, and refreshes the instance lock. For that
/// minute and a half Atlas is indistinguishable from crashed.
///
/// Forty-five seconds because the common case is nothing like it: apps that
/// are already up are placed without waiting at all, and a cold start of a
/// slow Electron app is the ten-to-fifteen-second end of it. A bring-up
/// taking longer than this is not a slow morning, it is an app that is not
/// coming, and the remaining apps are better served by a report that says so
/// than by more waiting.
pub const BRINGUP_BUDGET_SECS: u64 = 45;

#[derive(Debug, Default)]
pub struct Report {
    pub placed: Vec<String>,
    pub failed: Vec<(String, String)>,
    /// The bring-up ran out of time, so these were never attempted.
    ///
    /// Separate from `failed` on purpose: "chrome never showed a window" and
    /// "we never got to notepad" are different facts, and reporting the
    /// second as the first would blame an app that was never asked.
    pub not_reached: Vec<String>,
    /// Someone asked Atlas to stop part-way through.
    pub abandoned: bool,
}

impl Report {
    pub fn ok(&self) -> bool {
        self.failed.is_empty() && self.not_reached.is_empty() && !self.abandoned
    }

    /// What happened, in a sentence, including the parts that are easy to
    /// leave out.
    pub fn plain(&self) -> String {
        let mut bits = Vec::new();
        if !self.placed.is_empty() {
            bits.push(format!("{} up", self.placed.len()));
        }
        for (name, why) in &self.failed {
            bits.push(format!("{name} didn't come up ({why})"));
        }
        if !self.not_reached.is_empty() {
            // Two different reasons for the same list, and they must not come
            // out as the same sentence. "I ran out of time" said about a
            // Ctrl-C blames a budget that was never reached -- and it used to
            // print alongside the abandonment line, so the person got both
            // explanations for one event.
            if self.abandoned {
                bits.push(format!(
                    "and I never got to {} -- you asked me to stop",
                    self.not_reached.join(", ")
                ));
            } else {
                bits.push(format!(
                    "and I ran out of time before {} -- waiting on the ones before it \
                     used up the time I allow for getting everything going",
                    self.not_reached.join(", ")
                ));
            }
        } else if self.abandoned {
            bits.push("stopped part-way because you asked me to stop".into());
        }
        if bits.is_empty() {
            return "Nothing to bring up.".into();
        }
        format!("{}.", bits.join("; "))
    }
}

pub fn workspace_on(cfg: &Config, plat: &dyn Platform) -> Result<Report> {
    workspace_on_within(cfg, plat, std::time::Duration::from_secs(BRINGUP_BUDGET_SECS))
}

/// The same bring-up, with the wall-clock budget given rather than assumed.
///
/// Exists because the budget is the behaviour worth testing and forty-five
/// seconds is not a test. A parameter rather than a `#[cfg(test)]` hook, so
/// the code that runs under test is the code that runs for real.
pub fn workspace_on_within(
    cfg: &Config,
    plat: &dyn Platform,
    budget: std::time::Duration,
) -> Result<Report> {
    let monitors = plat.monitors()?;
    if monitors.is_empty() {
        return Err(AtlasError::Platform("no monitors detected".into()));
    }
    let roles = crate::layout::resolve_roles_with(&cfg.layouts, &monitors, plat.built_in_monitor());
    let mut report = Report::default();

    let deadline = std::time::Instant::now() + budget;
    let standalone = monitors.len() == 1;
    let mut stop_here = false;
    for name in &cfg.apps.startup_order {
        let spec = cfg.apps.get(name)?;
        if standalone && spec.docked_only {
            continue; // no room for it on the laptop panel alone
        }
        if stop_here || crate::goodbye::asked_to_stop() {
            // Named rather than silently skipped. A workspace that came up
            // three quarters of the way and said "Workspace online." is a
            // report that will be believed.
            // `|=`, not `=`. Assigning would clear a flag a previous
            // iteration had set -- safe only because `ASKED` is one-way in
            // production, which is exactly the kind of "correct by accident"
            // that `goodbye::reset_for_test` breaks under test.
            report.abandoned |= crate::goodbye::asked_to_stop();
            report.not_reached.push(name.clone());
            continue;
        }
        if std::time::Instant::now() >= deadline {
            stop_here = true;
            report.not_reached.push(name.clone());
            continue;
        }
        match place_one(cfg, plat, &roles, name, deadline) {
            Ok(()) => report.placed.push(name.clone()),
            Err(e) => {
                // A stop that arrives while waiting on an app comes back as
                // that app's error. Recorded as an abandonment rather than a
                // failure, because "chrome didn't come up" is a claim about
                // chrome and the truth is that you pressed Ctrl-C. It used to
                // land in `failed`, and if the stop arrived on the LAST app in
                // the order the loop then ended without `abandoned` ever being
                // set -- so a stop was reported purely as an app failure.
                if crate::goodbye::asked_to_stop() {
                    report.abandoned = true;
                    report.not_reached.push(name.clone());
                } else {
                    report.failed.push((name.clone(), e.to_string()));
                }
            }
        }
        let _ = spec;
    }
    Ok(report)
}

fn place_one(
    cfg: &Config,
    plat: &dyn Platform,
    roles: &crate::layout::RoleMap,
    name: &str,
    deadline: std::time::Instant,
) -> Result<()> {
    let spec = cfg.apps.get(name)?;
    let monitor = monitor_for_role(&cfg.layouts, roles, &spec.role)?;
    // One screen is a different layout problem, not a smaller version of three.
    let mut ids: Vec<u32> = roles.values().map(|m| m.id).collect();
    ids.sort_unstable();
    ids.dedup();
    let standalone = ids.len() == 1;
    let layout_name = match (standalone, &spec.standalone_layout) {
        (true, Some(l)) => l,
        _ => &spec.layout,
    };
    let frac = cfg.layouts.layout(layout_name)?;
    let rect = to_pixels(&monitor, frac);

    if plat.find_window(spec)?.is_none() {
        plat.launch(spec)?;
    }

    let mut attempt = 0;
    let win = loop {
        if let Some(w) = plat.find_window(spec)? {
            break w;
        }
        attempt += 1;
        if attempt >= spec.retries {
            return Err(AtlasError::WindowNeverAppeared(
                name.to_string(),
                spec.retries,
            ));
        }
        if std::time::Instant::now() >= deadline {
            return Err(AtlasError::Platform(format!(
                "{name} still had no window when the time I allow for getting the \
                 whole workspace up ran out"
            )));
        }
        // Asked to stop while waiting on a window. Without this, Ctrl-C
        // during a bring-up is ignored for as long as the bring-up lasts,
        // which on a cold morning is the better part of a minute -- and on
        // Windows a console close gives about five seconds before killing
        // the process regardless. See `goodbye`.
        if crate::goodbye::asked_to_stop() {
            return Err(AtlasError::Platform(format!(
                "stopped waiting for {name} because you asked me to stop"
            )));
        }
        // The heartbeat, refreshed from inside the wait.
        //
        // `Daemon::tick` beats once a pass and this runs inside one pass, so
        // for the length of a bring-up nothing was beating. The staleness
        // window is 150s and a worst-case bring-up was approaching that, at
        // which point a second Atlas would read the lock as abandoned, take
        // it, and two of them would write the same state folder. The budget
        // above makes that arithmetic safe on its own; this makes it safe
        // without depending on the arithmetic.
        // The result is deliberately not read here. `Daemon::tick` counts
        // consecutive failures and tells the person; doing it again from
        // inside a bring-up would be a second voice on the same fact, and
        // there is no `&mut self` here to count on anyway.
        let _ = crate::onlyone::OnlyOne::at(&crate::roots::data_dir()).beat(crate::store::now());
        plat.sleep_ms(spec.poll_ms);
    };

    plat.place(win, rect)?;
    Ok(())
}

/// Launch and place a single app.
///
/// Given the same whole-workspace budget as a bring-up, which for one app is
/// generous: its own `retries` × `poll_ms` is the tighter limit in every
/// shipped case. It is here so that "open chrome" cannot block the tick
/// longer than "start my workspace" can, whatever someone later puts in
/// `apps.yaml`.
pub fn open_app(cfg: &Config, plat: &dyn Platform, name: &str) -> Result<()> {
    let monitors = plat.monitors()?;
    let roles = crate::layout::resolve_roles_with(&cfg.layouts, &monitors, plat.built_in_monitor());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(BRINGUP_BUDGET_SECS);
    place_one(cfg, plat, &roles, name, deadline)
}

pub fn close_app(cfg: &Config, plat: &dyn Platform, name: &str) -> Result<()> {
    plat.close(cfg.apps.get(name)?)
}

/// Apps Atlas will arrange but never type into.
pub fn input_blocked_apps(cfg: &Config) -> Vec<String> {
    cfg.apps.apps.iter().filter(|(_, s)| s.no_input).map(|(n, _)| n.clone()).collect()
}

/// Bring an app forward, launching it first if it isn't running.
pub fn focus_app(cfg: &Config, plat: &dyn Platform, name: &str) -> Result<()> {
    let spec = cfg.apps.get(name)?;
    match plat.find_window(spec)? {
        Some(w) => plat.focus(w),
        None => open_app(cfg, plat, name),
    }
}

pub fn workspace_off(cfg: &Config, plat: &dyn Platform) -> Result<Report> {
    let mut report = Report::default();
    for name in &cfg.apps.shutdown_order {
        match cfg.apps.get(name).and_then(|s| plat.close(s)) {
            Ok(()) => report.placed.push(name.clone()),
            Err(e) => report.failed.push((name.clone(), e.to_string())),
        }
    }
    Ok(report)
}
