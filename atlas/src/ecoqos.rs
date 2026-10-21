//! Windows efficiency mode for background work (20 Oct 2026).
//!
//! Crew errands, downloads and the like ran at normal priority with no power
//! hint, so on a laptop they competed with what you were doing and kept the
//! fast cores awake. Windows has two switches for exactly this: EcoQoS
//! ("efficiency mode": the scheduler puts the work on efficient cores at a low
//! clock) and a lower priority. A worker thread calls [`this_thread`] once
//! when it starts; every child process that thread then launches through
//! `tools::run_scoped` is put in efficiency mode too ([`child`]).
//!
//! Never applied to the answer path: the main loop, the speaking thread and
//! the model server's answering are not background work and never call this.
//! Both calls are best effort: a failure is logged once and never fatal, and
//! on other systems they do nothing.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

thread_local! {
    static BACKGROUND: Cell<bool> = const { Cell::new(false) };
}

/// Whether the calling thread was marked as background work.
pub fn on_background_thread() -> bool {
    BACKGROUND.with(|b| b.get())
}

static WARNED: AtomicBool = AtomicBool::new(false);
static FAILURE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

fn failed(what: &str, why: &str) {
    // Once per run: the same refusal on every errand would only fill the log.
    // Kept for the daemon to write (`take_failure`), as there is no process-wide log.
    if !WARNED.swap(true, Ordering::Relaxed) {
        *FAILURE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) =
            Some(format!("efficiency mode: {what} did not apply ({why})"));
    }
}

/// The one failure line to log, if an efficiency call failed since last asked.
pub fn take_failure() -> Option<String> {
    FAILURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
}

/// Mark the calling thread as background work: below-normal priority and
/// EcoQoS. Call it first thing on a worker thread.
pub fn this_thread() {
    BACKGROUND.with(|b| b.set(true));
    #[cfg(windows)]
    {
        use windows::Win32::System::Threading::{
            GetCurrentThread, SetThreadInformation, SetThreadPriority, ThreadPowerThrottling,
            THREAD_POWER_THROTTLING_CURRENT_VERSION, THREAD_POWER_THROTTLING_EXECUTION_SPEED,
            THREAD_POWER_THROTTLING_STATE, THREAD_PRIORITY_BELOW_NORMAL,
        };
        // SAFETY: the pseudo-handle needs no closing; `state` is a live,
        // correctly sized struct for the call.
        unsafe {
            let me = GetCurrentThread();
            if let Err(e) = SetThreadPriority(me, THREAD_PRIORITY_BELOW_NORMAL) {
                failed("thread priority", &e.to_string());
            }
            let state = THREAD_POWER_THROTTLING_STATE {
                Version: THREAD_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: THREAD_POWER_THROTTLING_EXECUTION_SPEED,
            };
            if let Err(e) = SetThreadInformation(
                me,
                ThreadPowerThrottling,
                &state as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<THREAD_POWER_THROTTLING_STATE>() as u32,
            ) {
                failed("thread EcoQoS", &e.to_string());
            }
        }
    }
}

/// Put a child process (by id) in efficiency mode with below-normal priority.
pub fn child(pid: u32) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::CloseHandle;
        use windows::Win32::System::Threading::{
            OpenProcess, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
            BELOW_NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
            PROCESS_SET_INFORMATION,
        };
        // SAFETY: the handle is opened and closed here; `state` is a live,
        // correctly sized struct for the call.
        unsafe {
            let h = match OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
                Ok(h) => h,
                Err(e) => return failed("opening the child", &e.to_string()),
            };
            let state = PROCESS_POWER_THROTTLING_STATE {
                Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
                ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
                StateMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            };
            if let Err(e) = SetProcessInformation(
                h,
                ProcessPowerThrottling,
                &state as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            ) {
                failed("child EcoQoS", &e.to_string());
            }
            if let Err(e) = SetPriorityClass(h, BELOW_NORMAL_PRIORITY_CLASS) {
                failed("child priority", &e.to_string());
            }
            let _ = CloseHandle(h);
        }
    }
    #[cfg(not(windows))]
    let _ = pid;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_only_the_thread_that_asked() {
        std::thread::spawn(|| {
            assert!(!on_background_thread());
            this_thread();
            assert!(on_background_thread());
        })
        .join()
        .unwrap();
        assert!(!on_background_thread());
    }

    #[cfg(windows)]
    #[test]
    fn applies_to_this_thread_and_a_child_without_failing() {
        // A throwaway child: efficiency mode must apply (no warning set).
        let mut c = std::process::Command::new("cmd")
            .args(["/C", "ping -n 3 127.0.0.1 >nul"])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::spawn(|| this_thread()).join().unwrap();
        child(c.id());
        assert!(!WARNED.load(Ordering::Relaxed), "an efficiency call failed");
        let _ = c.kill();
        let _ = c.wait();
    }
}
