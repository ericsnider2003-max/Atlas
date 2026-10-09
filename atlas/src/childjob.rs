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

/// Ownership of one work process tree, independent of Atlas's lifetime job.
pub(crate) struct Scope {
    #[cfg(windows)]
    handle: Option<usize>,
    #[cfg(unix)]
    group: Option<i32>,
}

impl Scope {
    pub(crate) fn spawn(command: &mut std::process::Command) -> std::io::Result<(std::process::Child, Self)> {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            // The command cannot change anything before its tree is owned.
            command.creation_flags(0x0800_0000 | 0x0000_0004);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }
        let mut child = command.spawn()?;
        let scope = match Self::attach(&child) {
            Ok(scope) => scope,
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error); }
        };
        #[cfg(windows)]
        if let Err(error) = resume_owned(&child) {
            drop(scope);
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
        Ok((child, scope))
    }

    pub(crate) fn attach(child: &std::process::Child) -> std::io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::io::AsRawHandle;
            use windows::Win32::Foundation::{CloseHandle, HANDLE};
            use windows::Win32::System::JobObjects::*;
            // SAFETY: the owned job is closed on every failed setup path.
            unsafe {
                let job = CreateJobObjectW(None, None).map_err(std::io::Error::other)?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                let configured = SetInformationJobObject(job, JobObjectExtendedLimitInformation,
                    &info as *const _ as *const std::ffi::c_void, std::mem::size_of_val(&info) as u32)
                    .and_then(|_| AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())));
                if let Err(error) = configured { let _ = CloseHandle(job); return Err(std::io::Error::other(error)); }
                Ok(Self { handle: Some(job.0 as usize) })
            }
        }
        #[cfg(unix)]
        {
            let id = child.id() as i32;
            // SAFETY: getpgid only reads process metadata; never use Atlas's group.
            if unsafe { libc::getpgid(id) } != id { return Err(std::io::Error::other("the command has no independently owned process group")); }
            Ok(Self { group: Some(id) })
        }
        #[cfg(not(any(windows, unix)))]
        { let _ = child; Err(std::io::Error::other("process-tree ownership is unavailable on this platform")) }
    }

    pub(crate) fn stop(&mut self) {
        #[cfg(windows)]
        if let Some(handle) = self.handle.take() {
            // SAFETY: this handle belongs to this Scope, never the lifetime job.
            unsafe { let _ = windows::Win32::Foundation::CloseHandle(windows::Win32::Foundation::HANDLE(handle as *mut std::ffi::c_void)); }
        }
        #[cfg(unix)]
        if let Some(group) = self.group.take() {
            // SAFETY: Scope accepts only the separately created work group.
            unsafe { libc::kill(-group, libc::SIGKILL); }
        }
    }
}
impl Drop for Scope { fn drop(&mut self) { self.stop(); } }

#[cfg(windows)]
fn resume_owned(child: &std::process::Child) -> std::io::Result<()> {
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Diagnostics::ToolHelp::{CreateToolhelp32Snapshot, Thread32First, Thread32Next, THREADENTRY32, TH32CS_SNAPTHREAD};
    use windows::Win32::System::Threading::{OpenThread, ResumeThread, THREAD_SUSPEND_RESUME};
    // The suspended primary thread is found by this owned child's exact PID.
    // No thread from another process is opened or resumed.
    unsafe {
        let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0).map_err(std::io::Error::other)?;
        let mut entry = THREADENTRY32::default();
        entry.dwSize = std::mem::size_of_val(&entry) as u32;
        let mut next = Thread32First(snapshot, &mut entry);
        let mut resumed = false;
        let mut failure = None;
        while next.is_ok() {
            if entry.th32OwnerProcessID == child.id() {
                match OpenThread(THREAD_SUSPEND_RESUME, false, entry.th32ThreadID) {
                    Ok(thread) => {
                        if ResumeThread(thread) == u32::MAX { failure = Some(std::io::Error::last_os_error()); }
                        else { resumed = true; }
                        let _ = CloseHandle(thread);
                    }
                    Err(error) => failure = Some(std::io::Error::other(error)),
                }
            }
            next = Thread32Next(snapshot, &mut entry);
        }
        let _ = CloseHandle(snapshot);
        if let Some(error) = failure { return Err(error); }
        if resumed { Ok(()) } else { Err(std::io::Error::other("the owned command's primary thread could not be resumed")) }
    }
}

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
