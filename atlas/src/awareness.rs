//! Layer 1 awareness: what you are doing right now, cheaply.
//!
//! Deliberately excludes screenshots and OCR. Those are Layer 3 — expensive,
//! privacy-heavy, and on request only. Active window title plus file-change
//! events covers most of what proactive assistance actually needs, at a cost
//! low enough to poll all day on a laptop.

use crate::index::{Changes, Index, IndexConfig};
use crate::platform::{ActiveWindow, OsQuiet, Platform};
use crate::store::now;

#[derive(Debug, Clone, Default)]
pub struct Signals {
    pub active: Option<ActiveWindow>,
    /// Seconds the focused window has been unchanged.
    pub dwell_secs: u64,
    /// Seconds since you last said anything.
    pub idle_secs: u64,
    pub recent_changes: Changes,
    /// True while Atlas is mid-conversation with you.
    pub in_conversation: bool,
    /// Seconds since the last keyboard or mouse input, where the OS says.
    pub input_idle_secs: Option<u64>,
    /// What the OS says about interrupting you now, where it says.
    pub os_quiet: Option<OsQuiet>,
    /// A natural break in what you're doing: you've just moved to another
    /// app, just come back to the keyboard, or paused typing. Iqbal & Bailey
    /// (CHI 2008) measured these coarse breakpoints as the least costly
    /// moments to be interrupted.
    pub at_breakpoint: bool,
}

/// How long after an app switch or a return it still counts as a break.
pub const BREAK_WINDOW: u64 = 30;
/// Keyboard and mouse silence that counts as a pause.
pub const PAUSE: u64 = 20;

pub struct Awareness {
    last_active: Option<ActiveWindow>,
    focus_since: u64,
    pub last_spoke: u64,
    // (accessor for last_active is below; the field stays private so nothing
    // can write it without going through the observation path)
    last_index_scan: u64,
    /// When the foreground app (not just its title) last changed.
    app_changed_at: u64,
    /// When input last resumed after a minute or more of silence.
    came_back_at: u64,
    last_idle: Option<u64>,
    /// A natural pause per app, learned from your own (`worklog::
    /// pause_thresholds`); an app not in here uses `PAUSE`.
    pub learned_pause: std::collections::BTreeMap<String, u64>,
    /// How often the index is rechecked. Polling beats a watcher here only
    /// because it is simple and bounded; swap for real watchers later.
    pub scan_every: u64,
    /// Scans in a row that found nothing. Each one doubles the wait.
    quiet_scans: u32,
    /// Folder scans since start, so the cost of watching is a number.
    scans: u64,
    /// Walk the folders on a thread of their own rather than inside
    /// `observe` (27 Sep 2026). Off by default, so a test that calls
    /// `observe` sees the scan's result on the same call; the running Atlas
    /// turns it on (`Daemon::run`), where a walk of a big folder used to
    /// hold the loop -- the hub and the typing box with it -- for as long
    /// as the disk took.
    pub scan_in_background: bool,
    /// The walk in progress, when one is.
    walking: Option<std::sync::mpsc::Receiver<crate::index::Index>>,
}

/// The longest the folder scan backs off to while nobody is at the machine.
pub const SCAN_BACKOFF_MAX: u64 = 1800;
/// The longest while somebody plainly is (the foreground window changed in
/// the last `AT_THE_MACHINE` seconds): being slow to notice costs more then.
pub const SCAN_BACKOFF_PRESENT: u64 = 300;
/// How recently the foreground must have changed to count as "at the machine".
pub const AT_THE_MACHINE: u64 = 600;

impl Default for Awareness {
    fn default() -> Self {
        Awareness {
            last_active: None,
            focus_since: now(),
            last_spoke: now(),
            last_index_scan: 0,
            scan_every: 60,
            app_changed_at: 0,
            came_back_at: 0,
            last_idle: None,
            learned_pause: std::collections::BTreeMap::new(),
            quiet_scans: 0,
            scans: 0,
            scan_in_background: false,
            walking: None,
        }
    }
}

impl Awareness {
    /// What had the foreground last time Atlas looked.
    ///
    /// Read-only. Needed by the notification path to tell whether you are on
    /// a call, which decides whether Atlas may speak at all.
    pub fn last_active_window(&self) -> Option<&ActiveWindow> {
        self.last_active.as_ref()
    }

    pub fn observe(
        &mut self,
        plat: &dyn Platform,
        index: &mut Index,
        idx_cfg: Option<&IndexConfig>,
        in_conversation: bool,
        t: u64,
    ) -> Signals {
        let active = plat.active_window().unwrap_or(None);
        if active != self.last_active {
            let app = |w: &Option<ActiveWindow>| w.as_ref().map(|w| w.process.clone());
            if app(&active) != app(&self.last_active) && self.last_active.is_some() {
                self.app_changed_at = t;
            }
            self.focus_since = t;
            self.last_active = active.clone();
        }
        let input_idle_secs = plat.input_idle_secs();
        let os_quiet = plat.quiet_state();
        if let (Some(before), Some(now_idle)) = (self.last_idle, input_idle_secs) {
            if before >= 60 && now_idle < before {
                self.came_back_at = t.saturating_sub(now_idle);
            }
        }
        self.last_idle = input_idle_secs;
        let at_breakpoint = t.saturating_sub(self.app_changed_at) <= BREAK_WINDOW && self.app_changed_at > 0
            || t.saturating_sub(self.came_back_at) <= BREAK_WINDOW && self.came_back_at > 0
            || input_idle_secs.map(|i| i >= self.pause_in(active.as_ref())).unwrap_or(false);

        // A walk finished on its own thread: its changes are this
        // observation's, exactly as if it had been walked here.
        if let Some(rx) = &self.walking {
            match rx.try_recv() {
                Ok(fresh) => {
                    self.walking = None;
                    self.scans += 1;
                    let found = index.apply(fresh);
                    self.quiet_scans = if found.is_empty() { self.quiet_scans.saturating_add(1) } else { 0 };
                    return self.signals(plat, active, input_idle_secs, os_quiet, at_breakpoint, in_conversation, found, t);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.walking = None,
            }
        }
        let recent_changes = match idx_cfg {
            Some(cfg)
                if self.scan_in_background
                    && self.walking.is_none()
                    && t.saturating_sub(self.last_index_scan) >= self.how_long_to_wait(t) =>
            {
                self.last_index_scan = t;
                let (tx, rx) = std::sync::mpsc::channel();
                let cfg = cfg.clone();
                let started = std::thread::Builder::new().name("atlas-index-walk".into()).spawn(move || {
                    let _ = tx.send(crate::index::Index::scan(&cfg));
                });
                if started.is_ok() {
                    self.walking = Some(rx);
                }
                Changes::default()
            }
            Some(_) if self.scan_in_background => Changes::default(),
            Some(cfg) if t.saturating_sub(self.last_index_scan) >= self.how_long_to_wait(t) => {
                self.last_index_scan = t;
                self.scans += 1;
                let found = index.rescan(cfg);
                self.quiet_scans = if found.is_empty() { self.quiet_scans.saturating_add(1) } else { 0 };
                found
            }
            _ => Changes::default(),
        };
        self.signals(plat, active, input_idle_secs, os_quiet, at_breakpoint, in_conversation, recent_changes, t)
    }

    #[allow(clippy::too_many_arguments)]
    fn signals(
        &self,
        plat: &dyn Platform,
        active: Option<ActiveWindow>,
        input_idle_secs: Option<u64>,
        os_quiet: Option<OsQuiet>,
        at_breakpoint: bool,
        in_conversation: bool,
        recent_changes: Changes,
        t: u64,
    ) -> Signals {
        Signals {
            active,
            dwell_secs: t.saturating_sub(self.focus_since),
            // Quiet means not talking to Atlas AND not typing or moving the
            // mouse — what `lanes` has always said a gap is ("no keyboard,
            // no mouse, no speech"). Only the speech half was measured, so
            // foreground work could "find a gap" while you were typing.
            idle_secs: t.saturating_sub(self.last_spoke).min(plat.input_idle_secs().unwrap_or(u64::MAX)),
            recent_changes,
            in_conversation,
            input_idle_secs,
            os_quiet,
            at_breakpoint,
        }
    }

    /// How long a pause in this app has to be to count as a break.
    fn pause_in(&self, w: Option<&ActiveWindow>) -> u64 {
        w.and_then(|w| self.learned_pause.get(&w.process)).copied().unwrap_or(PAUSE)
    }

    pub fn heard_you(&mut self, t: u64) {
        self.last_spoke = t;
        self.quiet_scans = 0;
    }

    /// Folder scans since start.
    pub fn scans(&self) -> u64 {
        self.scans
    }

    /// Seconds between folder scans now.
    ///
    /// Every indexed folder was walked every sixty seconds, all night, with
    /// the lid shut: tens of thousands of `stat` calls a minute for nothing.
    /// Each scan that finds nothing doubles the wait, up to half an hour, so
    /// eight quiet hours is about 16 walks instead of 480. Any change, or you
    /// saying anything, puts it straight back to `scan_every`; and while
    /// you're plainly at the machine the longest gap is five minutes.
    /// What Atlas notices is unchanged — only how quickly, during a stretch
    /// when nothing has happened for an hour.
    pub fn how_long_to_wait(&self, t: u64) -> u64 {
        let base = self.scan_every.max(1);
        let backed = base.saturating_mul(1u64 << self.quiet_scans.min(16));
        let present = self.focus_since > 0 && t.saturating_sub(self.focus_since) < AT_THE_MACHINE;
        let ceiling = if present { SCAN_BACKOFF_PRESENT } else { SCAN_BACKOFF_MAX };
        backed.min(ceiling.max(base))
    }
}

/// A one-line description of what you're doing, for the model's context.
pub fn describe(s: &Signals) -> String {
    match &s.active {
        Some(a) if !a.title.is_empty() => {
            format!("You are in {} — \"{}\" (for {}s)", a.process, a.title, s.dwell_secs)
        }
        Some(a) => format!("You are in {}", a.process),
        None => "Nothing focused".into(),
    }
}
