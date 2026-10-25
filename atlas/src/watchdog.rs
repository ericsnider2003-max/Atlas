//! Supervises the background Atlas process after native termination: the logon
//! task starts `--watchdog`, which runs `--daemon` and starts it again only if it
//! died without writing a goodbye (never after a clean stop), at most three times
//! in ten minutes, with a growing wait between tries.

use crate::whystopped::Runs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub const MAX_RESTARTS: usize = 3;
pub const WINDOW_SECS: u64 = 600;

/// The supervisor's rules and loop.
pub struct Watchdog;

impl Watchdog {
    /// Restart only when the child we just ran began a run of its own and never
    /// said goodbye. A child that was refused (another Atlas holds the lock) or
    /// died before beginning a run leaves an older record behind; that is not a
    /// crash of ours, so it is not restarted.
    pub fn should_restart(runs: &Runs, spawned_at: u64) -> bool {
        runs.now
            .as_ref()
            .is_some_and(|run| run.ended.is_none() && run.started >= spawned_at)
    }

    /// Waits longer after each recent restart: 2s, 4s, 8s.
    pub fn backoff_secs(recent_restarts: usize) -> u64 {
        2u64 << recent_restarts.min(4)
    }

    pub fn recent_restart_allowed(restarts: &[u64], now: u64) -> bool {
        restarts
            .iter()
            .filter(|at| now.saturating_sub(**at) < WINDOW_SECS)
            .count()
            < MAX_RESTARTS
    }

    pub fn run(exe: PathBuf, state_dir: PathBuf) -> ! {
        let mut restarts = Vec::new();
        loop {
            let spawned_at = crate::store::now();
            let mut child = match Command::new(&exe).arg("--daemon").spawn() {
                Ok(child) => child,
                Err(_) => std::process::exit(1),
            };
            let _status = child.wait();
            let runs = Runs::load(&state_dir);
            if !Self::should_restart(&runs, spawned_at) {
                std::process::exit(0);
            }
            let now = crate::store::now();
            restarts.retain(|at| now.saturating_sub(*at) < WINDOW_SECS);
            if !Self::recent_restart_allowed(&restarts, now) {
                std::process::exit(1);
            }
            thread::sleep(Duration::from_secs(Self::backoff_secs(restarts.len())));
            restarts.push(now);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_stop_is_not_restarted() {
        let mut runs = Runs::default();
        runs.start(10, None);
        runs.stop(20, crate::goodbye::Why::YouClosedIt);
        assert!(!Watchdog::should_restart(&runs, 5));
    }

    #[test]
    fn an_unannounced_exit_is_restartable() {
        let mut runs = Runs::default();
        runs.start(10, None);
        assert!(Watchdog::should_restart(&runs, 5));
    }

    #[test]
    fn a_refused_or_early_dead_child_is_not_restarted() {
        // The record is the running daemon's (or an older crash's), begun
        // before this child was spawned.
        let mut runs = Runs::default();
        runs.start(10, None);
        assert!(!Watchdog::should_restart(&runs, 11));
        assert!(!Watchdog::should_restart(&Runs::default(), 0));
    }

    #[test]
    fn back_off_grows_and_is_bounded() {
        assert_eq!(Watchdog::backoff_secs(0), 2);
        assert_eq!(Watchdog::backoff_secs(2), 8);
        assert_eq!(Watchdog::backoff_secs(50), 32);
    }

    #[test]
    fn restart_limit_is_three_in_ten_minutes() {
        assert!(Watchdog::recent_restart_allowed(&[100, 110], 120));
        assert!(!Watchdog::recent_restart_allowed(&[100, 110, 120], 121));
        assert!(Watchdog::recent_restart_allowed(&[100, 110, 120], 721));
    }
}
