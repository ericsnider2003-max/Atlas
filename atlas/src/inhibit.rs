//! Actually keeping the machine awake while work runs.
//!
//! `awake` decides whether a piece of work is worth holding the machine up
//! for, and the daemon asked it every night — then could do nothing with the
//! answer, because no platform hook existed ("Atlas cannot physically inhibit
//! sleep"). This is the hook, one per system, each scoped to the work and
//! released the moment it ends:
//!
//! - **Windows:** `SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED)`
//!   on a thread that lives exactly as long as the hold. The screen may still
//!   turn off; only system sleep is held. (The approach keepawake-rs, MIT,
//!   takes; written here against the API directly.)
//! - **Linux:** a `systemd-inhibit --what=sleep` child that holds the lock
//!   until it is killed.
//! - **macOS:** `caffeinate -i`, the same way.
//!
//! Dropping the `Held` lets go. So does the process ending: every one of
//! these is released by the operating system when its owner goes, so a crash
//! can't leave a laptop hot in a bag.

use crate::awake::Hold;

/// Sleep held off. Drop it to let go.
pub struct Held {
    why: String,
    inner: platform::Inner,
}

impl Held {
    pub fn why(&self) -> &str {
        &self.why
    }
}

impl Drop for Held {
    fn drop(&mut self) {
        platform::let_go(&mut self.inner);
    }
}

/// Ask the system not to sleep, for `why`. Err says why it couldn't.
fn hold_off_sleep(why: &str) -> Result<Held, String> {
    Ok(Held { why: why.to_string(), inner: platform::take_hold(why)? })
}

/// Apply what `awake::decide` said, keeping or dropping the hold in `slot`.
/// Returns a line to log when something changed.
pub fn apply(slot: &mut Option<Held>, decision: Hold, why: &str) -> Option<String> {
    match (decision, slot.is_some()) {
        (Hold::SystemOnly, false) => match hold_off_sleep(why) {
            Ok(h) => {
                *slot = Some(h);
                Some(format!("keeping the machine awake: {why}"))
            }
            Err(e) => Some(format!("couldn't keep the machine awake ({e}); the work may stop if it sleeps")),
        },
        (Hold::Release, true) => {
            *slot = None;
            Some("letting the machine sleep again".into())
        }
        _ => None,
    }
}

#[cfg(windows)]
mod platform {
    use windows::Win32::System::Power::{SetThreadExecutionState, ES_CONTINUOUS, ES_SYSTEM_REQUIRED};

    /// The thread holding the state, and the way to tell it to stop.
    pub struct Inner {
        stop: Option<std::sync::mpsc::Sender<()>>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    pub(super) fn take_hold(_why: &str) -> Result<Inner, String> {
        let (stop_tx, stop_rx) = std::sync::mpsc::channel::<()>();
        let (ok_tx, ok_rx) = std::sync::mpsc::channel();
        let thread = std::thread::spawn(move || {
            // The state belongs to the thread that set it, so this thread
            // lives as long as the hold does.
            let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
            let _ = ok_tx.send(prev.0 != 0);
            let _ = stop_rx.recv();
            unsafe {
                SetThreadExecutionState(ES_CONTINUOUS);
            }
        });
        match ok_rx.recv() {
            Ok(true) => Ok(Inner { stop: Some(stop_tx), thread: Some(thread) }),
            _ => Err("Windows refused the request".into()),
        }
    }

    pub(super) fn let_go(i: &mut Inner) {
        i.stop.take();
        if let Some(t) = i.thread.take() {
            let _ = t.join();
        }
    }
}

#[cfg(not(windows))]
mod platform {
    pub struct Inner {
        child: Option<std::process::Child>,
    }

    pub(super) fn take_hold(why: &str) -> Result<Inner, String> {
        let mut cmd = if cfg!(target_os = "macos") {
            let mut c = crate::tools::command("caffeinate");
            c.arg("-i");
            c
        } else {
            let mut c = crate::tools::command("systemd-inhibit");
            c.args(["--what=sleep", "--who=Atlas", &format!("--why={why}"), "--mode=block", "sleep", "infinity"]);
            c
        };
        let mut child = cmd
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| format!("couldn't start the sleep inhibitor: {e}"))?;
        // An inhibitor that can't reach the session bus exits at once; one
        // that is holding stays up.
        std::thread::sleep(std::time::Duration::from_millis(300));
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut err = String::new();
                if let Some(mut e) = child.stderr.take() {
                    use std::io::Read;
                    crate::heard!(e.read_to_string(&mut err));
                }
                Err(format!("the sleep inhibitor stopped at once ({status}): {}", err.trim()))
            }
            _ => Ok(Inner { child: Some(child) }),
        }
    }

    pub(super) fn let_go(i: &mut Inner) {
        if let Some(mut c) = i.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}
