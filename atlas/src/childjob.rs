//! Every program Atlas starts for its own work ends when Atlas ends.
//!
//! A `std::process::Child` has no `Drop`: when Atlas stops -- closed, updated,
//! crashed -- whatever it started keeps running. On Eric's laptop that meant
//! a llama-server holding gigabytes, an ffmpeg holding the camera open, or a
//! whisper still transcribing, all spinning the fans after Atlas was gone
//! (the clean-pass checklist, 5 Oct 2026, F1; first done for Tor alone in
//! `onion`). Each is put in one Windows job object created with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`; the handle is never closed, so
//! Windows closes it when Atlas's process ends, and ends them with it.
//!
//! Not for what Atlas opens *for you* -- an app, a file, your browser
//! (`window`, `platform`): those are yours and outlive Atlas, as they should.

/// Tie `child` to this process: it ends when Atlas ends. Best effort -- a
/// child that can't be added (already in a job that forbids it) runs as before.
#[cfg(windows)]
pub fn tie(child: &std::process::Child) {
    use std::os::windows::io::AsRawHandle;
    use windows::Win32::Foundation::HANDLE;
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    // One job for the life of Atlas: never closed by Atlas, so it closes
    // when Atlas ends, and everything in it ends with it.
    static JOB: std::sync::OnceLock<Option<usize>> = std::sync::OnceLock::new();
    let job = JOB.get_or_init(|| {
        // SAFETY: plain Win32 calls; `info` outlives the call that reads it.
        unsafe {
            let job = CreateJobObjectW(None, None).ok()?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const std::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .ok()?;
            Some(job.0 as usize)
        }
    });
    if let Some(job) = job {
        // SAFETY: both handles are valid for the call; the child's is owned
        // by `child`, which outlives it.
        unsafe {
            let _ = AssignProcessToJobObject(HANDLE(*job as *mut std::ffi::c_void), HANDLE(child.as_raw_handle()));
        }
    }
}

/// Elsewhere, nothing to do here: a child is reaped by `Drop` handlers and
/// the session ending, and Atlas runs one way on phones.
#[cfg(not(windows))]
pub fn tie(_child: &std::process::Child) {}

#[cfg(test)]
mod tests {
    #[test]
    fn a_tied_child_still_runs_and_finishes() {
        // Tying must never stop the work itself: the child runs to its end
        // as before (on Windows inside the job, elsewhere untouched).
        let mut c = if cfg!(windows) {
            let mut c = std::process::Command::new("cmd");
            c.args(["/C", "exit 0"]);
            c
        } else {
            std::process::Command::new("true")
        };
        let mut child = c.spawn().expect("a trivial program starts");
        super::tie(&child);
        assert!(child.wait().expect("it ends").success());
    }
}
