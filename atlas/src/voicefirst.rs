//! The voice comes first while Atlas is speaking (Phase 0.8, 1 Oct 2026).
//!
//! On Eric's laptop Kokoro took about 1.3x as long to make a sentence as to
//! play it, so a reply stalled between sentences while the model server --
//! writing the next sentence on the same processor -- competed for the same
//! cores. While a reply plays, the model server is lowered to "below normal"
//! priority, so the voice wins any tie; it goes back to normal the moment
//! the reply ends. Only the scheduler's tie-break changes: with nothing else
//! wanting the processor, the model runs exactly as fast as before.
//!
//! Windows only (`SetPriorityClass`); elsewhere the state is still kept, so
//! the switching is the same everywhere and testable here.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

/// The model server's process id, set when Atlas starts it (`models`).
static MODEL_PID: AtomicU32 = AtomicU32::new(0);

/// Whether the model server is lowered right now.
static LOWERED: AtomicBool = AtomicBool::new(false);

/// The model server Atlas just started.
pub fn model_started(pid: u32) {
    MODEL_PID.store(pid, Ordering::SeqCst);
    LOWERED.store(false, Ordering::SeqCst);
}

/// Is the model server lowered for the voice right now?
pub fn model_lowered_for_test() -> bool {
    LOWERED.load(Ordering::SeqCst)
}

/// A reply started (`true`) or finished (`false`) playing.
pub fn speaking(on: bool) {
    if LOWERED.swap(on, Ordering::SeqCst) == on {
        return;
    }
    let pid = MODEL_PID.load(Ordering::SeqCst);
    if pid != 0 {
        set_priority(pid, on);
    }
}

#[cfg(windows)]
fn set_priority(pid: u32, lowered: bool) {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{
        OpenProcess, SetPriorityClass, BELOW_NORMAL_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_SET_INFORMATION,
    };
    unsafe {
        if let Ok(h) = OpenProcess(PROCESS_SET_INFORMATION, false, pid) {
            let class = if lowered { BELOW_NORMAL_PRIORITY_CLASS } else { NORMAL_PRIORITY_CLASS };
            let _ = SetPriorityClass(h, class);
            let _ = CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
fn set_priority(_pid: u32, _lowered: bool) {}
