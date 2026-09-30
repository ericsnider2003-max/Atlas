//! Only one Atlas at a time.
//!
//! Nothing stopped two from running. Start it from the shortcut, forget, start
//! it again from the batch file, and there are two — both reading and writing
//! `data/state`, both rewriting the trash ledger, both holding the microphone.
//!
//! The damage is quiet, which is what makes it worth guarding rather than
//! documenting. Each instance reads a file, changes it in memory, and writes
//! the whole thing back. Neither is corrupt. The second write simply erases
//! whatever the first learned, and nothing anywhere reports a problem. You
//! notice weeks later that something you told it didn't stick.
//!
//! The server port is the only accidental protection today — the second
//! instance fails to bind — and it is partial: the second daemon carries on
//! doing everything else, minus a web interface it never needed to work.
//!
//! ## Why a heartbeat rather than a process id
//!
//! The obvious lock holds the pid and checks whether that process is alive.
//! Doing that properly needs different system calls on Windows and Linux, and
//! pids get reused, so a stale lock can point at something unrelated that has
//! since started.
//!
//! A file whose modification time is refreshed while running avoids all of it.
//! Fresh means someone is there. Old means whoever held it is gone — crashed,
//! killed, or power-cut — and nothing needs to ask the operating system
//! anything.
//!
//! The cost is that recovering from a crash takes as long as the staleness
//! window. That is the right trade: refusing to start for a couple of minutes
//! after a crash is a small annoyance, and starting a second instance
//! alongside a live one costs you state.

use std::path::{Path, PathBuf};

/// How often the holder refreshes the lock.
pub const BEAT_EVERY_SECS: u64 = 30;

/// Past this with no refresh, whoever held it is gone.
///
/// Several beats rather than one. A machine that sleeps, a disk that stalls,
/// or a long transcription can all delay a beat, and treating the first missed
/// one as a death would mean Atlas killing itself for being busy.
pub const GONE_AFTER_SECS: u64 = 5 * BEAT_EVERY_SECS;

/// Past this, a claim file is a crash rather than a competitor.
///
/// Short, because taking the lock is a handful of syscalls: anything holding
/// a claim for five seconds is not mid-decision, it is gone. See
/// `OnlyOne::take`.
pub const CLAIM_STALE_SECS: u64 = 5;

/// How long to keep watching a lock that has only just gone quiet before
/// believing its holder is gone (28 Sep 2026).
///
/// The staleness above is measured on the wall clock, and a laptop that
/// sleeps stops every beat while the wall clock runs on. So in the first
/// moments after the lid opens, EVERY lock reads `Abandoned` -- including the
/// one held by the Atlas that is about to wake up and beat again. Whatever
/// looks first wins that race:
///
/// * the desktop overlay checked every three seconds, saw `Abandoned`, and
///   closed itself for good -- the words on the desktop were gone after a
///   sleep about as often as not, until Atlas was restarted;
/// * a second Atlas started in those seconds (Open Atlas from the Start menu
///   while the hub wasn't answering yet) took the lock over from a live one.
///
/// A holder that was merely asleep beats within a few seconds of waking; one
/// that is really gone never beats again. Watching for this long tells them
/// apart without asking the operating system anything, which is the design
/// this module chose.
pub const WOKE_GRACE_SECS: u64 = 20;

/// What the lock says about who is already here.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Found {
    /// Nobody. Free to take.
    Free,
    /// Someone is running and refreshing it.
    Running { last_beat_secs_ago: u64 },
    /// A lock left by something that died.
    Abandoned { silent_for_secs: u64 },
}

impl Found {
    pub fn can_take(&self) -> bool {
        !matches!(self, Found::Running { .. })
    }

    /// What to say to whoever tried to start.
    pub fn plain(&self) -> String {
        match self {
            Found::Free => "Nothing else is running.".into(),
            Found::Running { last_beat_secs_ago } => format!(
                "Atlas is already running — it checked in {last_beat_secs_ago} seconds ago. \
                 Starting a second one would have both of them writing over each other's \
                 state, so I've stopped. Close the other one first."
            ),
            Found::Abandoned { silent_for_secs } => format!(
                "There's a lock from a previous run that stopped {} minutes ago without \
                 tidying up — most likely a crash or a power cut. Taking it over.",
                silent_for_secs / 60
            ),
        }
    }
}

/// The lock itself.
#[derive(Debug, Clone)]
pub struct OnlyOne {
    path: PathBuf,
}

impl OnlyOne {
    pub fn at(dir: &Path) -> OnlyOne {
        OnlyOne { path: dir.join("running.lock") }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Who, if anyone, is already here.
    ///
    /// The time comes from inside the file, not from its modification time.
    /// The beat writes a timestamp; reading the filesystem's instead would be
    /// two sources of truth for one fact, and they disagree the moment a clock
    /// is adjusted, a file is copied, or a folder is restored from a backup —
    /// all of which would make a dead lock look alive or the reverse.
    pub fn look(&self, now: u64) -> Found {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            // NOT-THERE and CANNOT-READ are different answers.
            //
            // This was `let Ok(..) else { return Found::Free }`, so a
            // permission error, a sharing violation from antivirus, or EMFILE
            // on the lock file all read as "nobody is here" and admitted a
            // second instance. Ten lines down, an *unparseable* lock is
            // deliberately treated as occupied, with the reasoning written
            // out: "tells you nothing must not read as nobody is there." An
            // unreadable one tells you exactly as little.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Found::Free,
            Err(_) => return Found::Running { last_beat_secs_ago: 0 },
        };
        // 29 Sep 2026: a holder whose process has ended is nobody. Eric ended
        // a stuck Atlas in Task Manager and opened it again; the lock still
        // read "running" for up to GONE_AFTER_SECS, the new background Atlas
        // refused to start, and nothing said so. The lock now names its
        // holder, and one that is certainly gone frees it at once.
        if let Some(pid) = holder_in(&text) {
            if pid != std::process::id() && process_gone(pid) {
                return Found::Free;
            }
        }
        let Some(age) = moment_in(&text).map(|written| now.saturating_sub(written))
        else {
            // A lock whose contents cannot be read tells you nothing, and
            // "tells you nothing" must not read as "nobody is there". Treated
            // as occupied: refusing to start is recoverable by deleting a
            // file, and the other way round costs state.
            return Found::Running { last_beat_secs_ago: 0 };
        };
        if age <= GONE_AFTER_SECS {
            Found::Running { last_beat_secs_ago: age }
        } else {
            Found::Abandoned { silent_for_secs: age }
        }
    }

    /// Take it, or say why not.
    pub fn take(&self, now: u64) -> Result<Found, String> {
        let found = self.look(now);
        if !found.can_take() {
            // Say WHERE, and say how long. `plain()` alone tells you to
            // "close the other one first", and there are two common cases
            // where there is no other one to close:
            //
            // 1. **You just quit and restarted.** Nothing calls `release()`
            //    -- `Daemon::run` is an infinite loop with no shutdown path,
            //    so the lock always outlives a clean exit. Restart inside
            //    GONE_AFTER_SECS and Atlas refuses, blames a process that
            //    is gone, and gives no hint that waiting is the answer.
            // 2. **The lock file is unreadable.** `beat` is a truncate-then-
            //    write, so a power cut inside that window leaves it empty,
            //    and `look` maps unparseable to `Running { 0 }` on purpose
            //    ("tells you nothing must not read as nobody is there").
            //    That is the right call, but it never ages into `Abandoned`,
            //    so Atlas refuses to start FOREVER and the message named no
            //    file to delete.
            //
            // The conservative read stays; what changes is that the way out
            // is in the message rather than in the source.
            let secs_left = match found {
                Found::Running { last_beat_secs_ago } => {
                    GONE_AFTER_SECS.saturating_sub(last_beat_secs_ago)
                }
                _ => GONE_AFTER_SECS,
            };
            return Err(format!(
                "{}\n\nIf nothing else is actually running -- you just quit, or it was \
                 killed -- this clears itself in {secs_left}s, or delete this file to \
                 clear it now:\n  {}",
                found.plain(),
                self.path.display()
            ));
        }
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }

        // ## The race this closes
        //
        // `look` then `write` is a check and an act with a gap between them,
        // and nothing held the gap shut. Two launches at the same moment --
        // the shortcut plus ATLAS.bat, which is this module's own opening
        // example; or a service and a manual start after a crash, when both
        // see `Abandoned` -- both read a free lock, both wrote it, and both
        // ran. Two Atlases then rewrite `data/state` from their own memory,
        // and what you notice is that something did not stick, weeks later.
        //
        // The claim file is the interlock. `create_new` is a single atomic
        // operation on every platform Atlas runs on: exactly one process can
        // succeed. The winner re-checks and writes; the loser is told
        // somebody else is deciding right now, which is the truth.
        let claim = self.path.with_extension("claim");
        match std::fs::OpenOptions::new().write(true).create_new(true).open(&claim) {
            Ok(mut f) => {
                use std::io::Write;
                let _ = write!(f, "{now}");
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // A claim that is merely old is a crash, not a competitor: a
                // process that died between creating it and removing it
                // would otherwise block every future start for ever. Anything
                // recent is a real race and loses.
                let stale = std::fs::read_to_string(&claim)
                    .ok()
                    .and_then(|t| t.trim().parse::<u64>().ok())
                    .map(|then| now.saturating_sub(then) > CLAIM_STALE_SECS)
                    .unwrap_or(true);
                if !stale {
                    return Err(format!(
                        "another Atlas is starting up at this moment and got there first. \
                         Nothing has been taken. Try again in a second.\n  {}",
                        claim.display()
                    ));
                }
                let _ = std::fs::remove_file(&claim);
                let _ = std::fs::write(&claim, format!("{now}"));
            }
            Err(e) => {
                return Err(format!("couldn't claim the lock at {}: {e}", claim.display()));
            }
        }

        // Re-read inside the claim. The `look` above happened before anyone
        // was excluded; this is the one whose answer can be acted on.
        let found = self.look(now);
        if !found.can_take() {
            let _ = std::fs::remove_file(&claim);
            return Err(format!(
                "{}\n\nSomething took the lock while this one was starting.",
                found.plain()
            ));
        }
        let wrote = std::fs::write(&self.path, lock_line(now))
            .map_err(|e| format!("couldn't take the lock at {}: {e}", self.path.display()));
        let _ = std::fs::remove_file(&claim);
        wrote?;
        Ok(found)
    }

    /// Still here. Called from the tick.
    ///
    /// Writes rather than touches, because setting a modification time
    /// portably is more trouble than rewriting a dozen bytes.
    /// Returns whether the beat actually landed.
    ///
    /// It used to be `let _ = write(..)`. If beats stop landing while the
    /// daemon runs happily -- a full disk, a file a sync client has locked --
    /// the lock ages past `GONE_AFTER_SECS`, the next start reads
    /// `Abandoned`, takes over, and two instances write the same state
    /// folder. The holder never learned it had lost the lock. Now the caller
    /// can say so.
    pub fn beat(&self, now: u64) -> bool {
        std::fs::write(&self.path, lock_line(now)).is_ok()
    }

    /// Should it beat yet?
    ///
    /// Kept here rather than in the caller so the interval and the staleness
    /// window stay in one place — the two only mean something relative to each
    /// other, and separating them is how one gets tuned and the other doesn't.
    pub fn due(&self, last_beat: u64, now: u64) -> bool {
        now.saturating_sub(last_beat) >= BEAT_EVERY_SECS
    }

    /// The moment written in the lock, as written; `None` when there is no
    /// lock or it can't be read.
    fn written(&self) -> Option<u64> {
        std::fs::read_to_string(&self.path).ok().and_then(|t| moment_in(&t))
    }

    /// A lock that reads `Abandoned`, looked at again after `wait` in case
    /// its holder was only asleep (see [`WOKE_GRACE_SECS`]).
    ///
    /// Anything but `Abandoned` is returned as it is, at once: `Free` is a
    /// released lock and `Running` is a live one. If the lock's moment moved
    /// during the wait, somebody beat it -- a holder waking up -- and the
    /// answer is looked up again at `now()`, which reads `Running`. If it
    /// didn't move, the holder really is gone and `first` stands.
    fn look_again_after(&self, first: Found, wait: std::time::Duration, now: &dyn Fn() -> u64) -> Found {
        if !matches!(first, Found::Abandoned { .. }) {
            return first;
        }
        let before = self.written();
        let until = std::time::Instant::now() + wait;
        while std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(50).min(wait));
            let after = self.written();
            if after.is_some() && after != before {
                return self.look(now());
            }
        }
        first
    }

    /// `take`, for a long-running Atlas starting up: a lock that reads
    /// abandoned is watched for `wait` first, and if its holder beats in
    /// that time it was only asleep and this start is refused like any other
    /// second start (`WOKE_GRACE_SECS`). A free or live lock costs no wait.
    pub fn take_patiently(&self, wait: std::time::Duration, now: &dyn Fn() -> u64) -> Result<Found, String> {
        let _ = self.look_again_after(self.look(now()), wait, now);
        self.take(now())
    }

    /// Let go on the way out.
    ///
    /// Best effort. A crash skips this, which is exactly what the staleness
    /// window is for — the lock is correct without a clean shutdown, and a
    /// design that needs one is a design that breaks on the first power cut.
    pub fn release(&self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Watching another process's lock over time, for something that should end
/// when its Atlas does -- the desktop overlay (28 Sep 2026).
///
/// A released lock (`Free`) means that Atlas quit: gone at once. A lock that
/// reads `Abandoned` is gone only once it has stayed that way for
/// [`WOKE_GRACE_SECS`] of watching. Before this the overlay closed on the
/// first `Abandoned` it saw, which after a sleep is usually the moment before
/// the live Atlas beats again, so the overlay vanished for the rest of the
/// session. Time is counted from the first quiet look, so the hours asleep
/// don't count against it.
#[derive(Debug, Clone, Default)]
pub struct Watching {
    quiet_since: Option<u64>,
}

impl Watching {
    /// Is that Atlas still there, given what its lock says at `now`?
    pub fn still_there(&mut self, found: &Found, now: u64) -> bool {
        match found {
            Found::Running { .. } => {
                self.quiet_since = None;
                true
            }
            Found::Free => false,
            Found::Abandoned { .. } => {
                let since = *self.quiet_since.get_or_insert(now);
                now.saturating_sub(since) < WOKE_GRACE_SECS
            }
        }
    }
}

/// What a lock holds: the moment, then this process's id.
fn lock_line(now: u64) -> String {
    format!("{now} {}", std::process::id())
}

/// The moment in a lock, written either as `moment` (before 29 Sep 2026) or
/// `moment pid`.
pub fn moment_in(text: &str) -> Option<u64> {
    text.split_whitespace().next()?.parse().ok()
}

/// The process id in a lock, if it names one.
pub fn holder_in(text: &str) -> Option<u32> {
    text.split_whitespace().nth(1)?.parse().ok()
}

/// Is the process `pid` certainly gone? Only a sure answer says yes: a
/// process that can't be asked (another user's, or a platform with no way
/// to ask) is treated as still there, and the moment decides as before.
fn process_gone(pid: u32) -> bool {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, ERROR_INVALID_PARAMETER};
        use windows::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        unsafe {
            match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                Ok(h) => {
                    let mut code = 0u32;
                    let got = GetExitCodeProcess(h, &mut code).is_ok();
                    let _ = CloseHandle(h);
                    // 259 is STILL_ACTIVE.
                    got && code != 259
                }
                // No such process: Windows says the id is not a valid one.
                Err(e) => e.code() == ERROR_INVALID_PARAMETER.to_hresult(),
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        !std::path::Path::new(&format!("/proc/{pid}")).exists()
    }
    #[cfg(not(any(windows, target_os = "linux")))]
    {
        let _ = pid;
        false
    }
}

/// End the Atlas holding the lock in `dir`, when it wouldn't stop when asked
/// (29 Sep 2026: reinstalling over a running Atlas left the old one running
/// from the file it was moved aside to, and the new one never started). Only
/// a holder named in the lock whose program really is an Atlas
/// (`is_atlas_program`) is ended; never this process. True when it has gone.
pub fn end_holder(dir: &Path, wait: std::time::Duration) -> bool {
    let lock = OnlyOne::at(dir);
    let Some(pid) = std::fs::read_to_string(lock.path()).ok().and_then(|t| holder_in(&t)) else { return false };
    if pid == std::process::id() {
        return false;
    }
    if process_gone(pid) {
        return true;
    }
    if !crate::onion::process_program(pid).is_some_and(|p| is_atlas_program(&p)) {
        return false;
    }
    if !crate::onion::kill_process(pid) {
        return false;
    }
    let until = std::time::Instant::now() + wait;
    while std::time::Instant::now() < until {
        if process_gone(pid) {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    process_gone(pid)
}

/// Is this program file an Atlas: `atlas.exe`, or one set aside by an
/// update or a reinstall (`atlas.exe.set-aside-…`, `atlas-previous.exe`)?
pub fn is_atlas_program(p: &Path) -> bool {
    let full = p.to_string_lossy().to_lowercase();
    let name = full.rsplit(['/', '\\']).next().unwrap_or("").to_string();
    name == "atlas" || name.starts_with("atlas.exe") || (name.starts_with("atlas") && name.ends_with(".exe"))
}
