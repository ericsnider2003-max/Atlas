//! Supervises Atlas's background process after native termination, records
//! restart outcomes, and stops after a bounded number of rapid failures.

use crate::whystopped::Runs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

pub const MAX_RESTARTS: usize = 3;
pub const WINDOW_SECS: u64 = 600;

pub fn should_restart(runs: &Runs) -> bool {
    runs.now.as_ref().is_some_and(|run| run.ended.is_none())
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
        let mut child = match Command::new(&exe).arg("--daemon").spawn() {
            Ok(child) => child,
            Err(_) => std::process::exit(1),
        };
        let _status = child.wait();
        let runs = Runs::load(&state_dir);
        if !should_restart(&runs) {
            std::process::exit(0);
        }
        let now = crate::store::now();
        restarts.retain(|at| now.saturating_sub(*at) < WINDOW_SECS);
        if !recent_restart_allowed(&restarts, now) {
            std::process::exit(1);
        }
        restarts.push(now);
        thread::sleep(Duration::from_secs(2));
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
        assert!(!should_restart(&runs));
    }

    #[test]
    fn an_unannounced_exit_is_restartable() {
        let mut runs = Runs::default();
        runs.start(10, None);
        assert!(should_restart(&runs));
    }

    #[test]
    fn restart_limit_is_three_in_ten_minutes() {
        assert!(recent_restart_allowed(&[100, 110], 120));
        assert!(!recent_restart_allowed(&[100, 110, 120], 121));
        assert!(recent_restart_allowed(&[100, 110, 120], 721));
    }
}
