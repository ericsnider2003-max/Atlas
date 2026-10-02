//! Why Atlas stopped, written down (item 33, 1 Oct 2026).
//!
//! On 30 Sep Atlas was absent for 14 hours of Eric's day (6:37 am to 8:31 pm
//! Pacific): 23 starts and only 4 clean stops, and nothing recorded why the
//! other 19 ended. This keeps one small file, `data/state/runs.json`:
//!
//! - the run going on now -- when it started and when it was last alive
//!   (written with the instance lock's heartbeat, about once a minute);
//! - the last few runs, each with how it ended, in words.
//!
//! A clean way out says why (`goodbye::Why`). A run that never said
//! goodbye is judged at the next start: if the computer started after the
//! run was last alive, the computer restarted (Windows Update, power, or a
//! restart from the Start menu); otherwise Atlas was ended without warning
//! (a crash or Task Manager). That is all the evidence there is, so that is
//! all it claims.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The file, in Atlas's state folder.
pub const FILE: &str = "runs.json";

/// Runs kept, newest last.
pub const KEEP: usize = 30;

/// How often the "still alive" time is written, in seconds: often enough to
/// place an unclean end within a minute, rare enough to cost nothing.
pub const ALIVE_EVERY_SECS: u64 = 60;

/// How a run ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ended {
    /// It said why on the way out.
    Clean(crate::goodbye::Why),
    /// It didn't, and the computer started again after it was last alive.
    ComputerRestarted,
    /// It didn't, and the computer kept running: a crash, or ended from
    /// Task Manager.
    EndedWithoutWarning,
}

impl Ended {
    /// In words, for the log and for "why did you stop?".
    pub fn plain(&self) -> String {
        match self {
            Ended::Clean(why) => why.plain().to_string(),
            Ended::ComputerRestarted => "the computer restarted (Windows Update, power, or a restart)".into(),
            Ended::EndedWithoutWarning => "it was ended without warning (a crash, or closed from Task Manager)".into(),
        }
    }
}

/// One run of Atlas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub started: u64,
    pub last_alive: u64,
    #[serde(default)]
    pub ended: Option<Ended>,
}

/// The file's contents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Runs {
    #[serde(default)]
    pub now: Option<Run>,
    #[serde(default)]
    pub past: Vec<Run>,
}

impl Runs {
    pub fn path(state_dir: &Path) -> PathBuf {
        state_dir.join(FILE)
    }

    pub fn load(state_dir: &Path) -> Runs {
        std::fs::read(Self::path(state_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, state_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(state_dir)?;
        let tmp = state_dir.join(format!("{FILE}.part"));
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).unwrap_or_default())?;
        std::fs::rename(tmp, Self::path(state_dir))
    }

    /// A new run begins at `now`. The one before, if it never said goodbye,
    /// is judged by `computer_started` (when this boot of the computer
    /// began). Returns a line about the last run when it ended badly.
    pub fn start(&mut self, now: u64, computer_started: Option<u64>) -> Option<String> {
        let mut note = None;
        if let Some(mut last) = self.now.take() {
            if last.ended.is_none() {
                let restarted = computer_started.is_some_and(|boot| boot > last.last_alive);
                let ended = if restarted { Ended::ComputerRestarted } else { Ended::EndedWithoutWarning };
                note = Some(format!(
                    "Atlas last stopped {} without closing properly: {}.",
                    crate::freshness::ago(now.saturating_sub(last.last_alive)),
                    ended.plain()
                ));
                last.ended = Some(ended);
            }
            self.past.push(last);
        }
        let excess = self.past.len().saturating_sub(KEEP);
        self.past.drain(..excess);
        self.now = Some(Run { started: now, last_alive: now, ended: None });
        note
    }

    /// Still alive at `now`. Whether it's worth writing (a minute since the
    /// last write).
    pub fn alive(&mut self, now: u64) -> bool {
        match self.now.as_mut() {
            Some(r) if now >= r.last_alive + ALIVE_EVERY_SECS => {
                r.last_alive = now;
                true
            }
            _ => false,
        }
    }

    /// The way out, with why.
    pub fn stop(&mut self, now: u64, why: crate::goodbye::Why) {
        if let Some(mut r) = self.now.take() {
            r.last_alive = now;
            r.ended = Some(Ended::Clean(why));
            self.past.push(r);
            let excess = self.past.len().saturating_sub(KEEP);
            self.past.drain(..excess);
        }
    }

    /// The last run that ended, and how, in a sentence.
    pub fn last_stop(&self, now: u64) -> Option<String> {
        let r = self.past.last()?;
        let how = r.ended.as_ref().map(Ended::plain).unwrap_or_else(|| "for a reason it didn't record".into());
        Some(format!("I last stopped {}: {how}.", crate::freshness::ago(now.saturating_sub(r.last_alive))))
    }
}

/// When this boot of the computer began (Unix seconds), from how long it
/// has been up. `None` where that can't be read.
pub fn computer_started(now: u64) -> Option<u64> {
    uptime_secs().map(|up| now.saturating_sub(up))
}

#[cfg(windows)]
fn uptime_secs() -> Option<u64> {
    // Milliseconds since the computer started; sleep counts as running, so a
    // laptop that slept is not mistaken for one that restarted.
    Some(unsafe { windows::Win32::System::SystemInformation::GetTickCount64() } / 1000)
}

#[cfg(not(windows))]
fn uptime_secs() -> Option<u64> {
    let s = std::fs::read_to_string("/proc/uptime").ok()?;
    s.split_whitespace().next()?.parse::<f64>().ok().map(|f| f as u64)
}

/// Ask Windows to start Atlas again after an update restarts the computer
/// (`RegisterApplicationRestart`: it only does so for a program that has run
/// at least a minute). Not after a crash or a hang -- Atlas's own start-up
/// task covers those -- so a program that keeps crashing isn't restarted in
/// a loop. Nothing on other systems.
pub fn come_back_after_updates(args: &str) -> bool {
    #[cfg(windows)]
    {
        use windows::core::PCWSTR;
        // RESTART_NO_CRASH | RESTART_NO_HANG: only restarts and updates.
        const FLAGS: u32 = 1 | 2;
        let wide: Vec<u16> = args.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            windows::Win32::System::Recovery::RegisterApplicationRestart(
                PCWSTR(wide.as_ptr()),
                windows::Win32::System::Recovery::REGISTER_APPLICATION_RESTART_FLAGS(FLAGS),
            )
            .is_ok()
        }
    }
    #[cfg(not(windows))]
    {
        let _ = args;
        false
    }
}
