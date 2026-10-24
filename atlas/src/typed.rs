//! Something you **type**, in a codebase that is otherwise spoken to.
//!
//! One module, because there is exactly one thing in Atlas that a microphone
//! must never carry: the vault passphrase. `handover::take_back` rests on it,
//! `vault::Vault::open` checks it, and both of those would be decoration if
//! the way it arrived were "say your passphrase out loud in the room where
//! you have just handed your laptop to somebody else".
//!
//! So a spoken phrase may *summon* this prompt. It can never answer it.
//!
//! # Why a trait and not a function
//!
//! `daemon.rs` is a library module that tests drive thousands of times a run.
//! A function that reads stdin would make the daemon untestable at exactly
//! the point worth testing — and worse, would make the tests hang rather than
//! fail, which is the failure mode people work around by deleting the test.
//!
//! The daemon therefore holds an `Option<Box<dyn AsksQuietly>>`. `None` is
//! the honest default and the one the tests get by construction: no terminal
//! was wired up, so Atlas says so and names the command that has one, rather
//! than pretending to have asked. The binary installs `Console`.

/// Somewhere a person can type something nobody else should hear.
pub trait AsksQuietly {
    /// Put the prompt in front of them and read one line.
    ///
    /// `None` means nothing usable was typed: cancelled, empty, end of
    /// input, or no terminal at all. It must never be interpreted as a
    /// wrong answer *or* a right one — the caller's job on `None` is to
    /// leave everything exactly as it was.
    fn ask(&self, prompt: &str) -> Option<String>;

    /// Where that prompt shows up, in words Atlas can say out loud.
    ///
    /// Needed because the person who said "I'm back" is looking at a
    /// microphone, not at a screen, and "type it" is useless advice if they
    /// do not know which window is waiting.
    fn where_it_appears(&self) -> &'static str {
        "where I'm running"
    }
}

/// The terminal Atlas itself was started in.
pub struct Console;

impl AsksQuietly for Console {
    fn ask(&self, prompt: &str) -> Option<String> {
        ask_quietly(prompt)
    }
    fn where_it_appears(&self) -> &'static str {
        "in the terminal I'm running in"
    }
}

/// Read a line without putting it on the screen.
///
/// In-house rather than a crate, and honest when it cannot manage it: on a
/// terminal where the echo cannot be turned off it says so *before* you type,
/// rather than quietly printing your passphrase into the scrollback where it
/// will sit until the window closes.
pub fn ask_quietly(prompt: &str) -> Option<String> {
    use std::io::Write;
    let _masked = MaskedGuard::enter();

    // Windows, in this process: the console's own input with its echo
    // turned off (`console::Quiet`). Nothing leaves Atlas -- no child
    // process, no pipe -- which is what the PowerShell read below could not
    // promise. Falls through when there is no console to ask.
    #[cfg(windows)]
    if let Some(q) = console::Quiet::start() {
        crate::out!("{prompt}");
        let _ = std::io::stdout().flush();
        let got = q.read_line();
        drop(q);
        crate::outln!();
        return got.and_then(clean);
    }

    // The platform's own masked read, where there is one. On Windows this is
    // the whole of it -- the prompt is ours, the reading is PowerShell's --
    // and it returns `None` only when PowerShell could not be run at all, so
    // an ordinary empty line still falls through to the same "nothing typed"
    // answer as everywhere else.
    if let Some(got) = masked_read(prompt) {
        return clean(got);
    }

    let hidden = hide_typing(true);
    if !hidden {
        crate::outln!("(I can't turn off the echo on this terminal — what you type will show.)");
    }
    crate::out!("{prompt}");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    let read = std::io::stdin().read_line(&mut line);
    if hidden {
        hide_typing(false);
        crate::outln!();
    }
    match read {
        Ok(0) | Err(_) => None,
        Ok(_) => clean(line),
    }
}

static MASKED_OPEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Held while a masked prompt is on screen (23 Oct 2026): the correction log
/// writes nothing while one is open.
pub struct MaskedGuard;
impl MaskedGuard {
    pub fn enter() -> MaskedGuard {
        MASKED_OPEN.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        MaskedGuard
    }
}
impl Drop for MaskedGuard {
    fn drop(&mut self) {
        MASKED_OPEN.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Is a masked read open right now?
pub fn masked_read_active() -> bool {
    MASKED_OPEN.load(std::sync::atomic::Ordering::SeqCst) > 0
}

/// Lines for the prompt, read on a thread only when one is asked for
/// (22 Oct 2026). A thread that reads ahead is already blocked in
/// `read_line` while a masked prompt (`ask_quietly`, above) is on screen, and
/// takes whatever is typed into it -- a passphrase would arrive as the next
/// command. Here nothing is read until `want` is sent, and the caller sends
/// it only when it is back at its own prompt, so while a secret is asked for
/// nothing else is reading the same input. `None` in the channel is the end
/// of input. Dropping either end stops the thread.
pub fn lines_on_demand<F>(
    mut read: F,
) -> (
    std::sync::mpsc::Sender<()>,
    std::sync::mpsc::Receiver<Option<String>>,
)
where
    F: FnMut(&mut String) -> std::io::Result<usize> + Send + 'static,
{
    let (want_tx, want_rx) = std::sync::mpsc::channel::<()>();
    let (lines_tx, lines_rx) = std::sync::mpsc::channel::<Option<String>>();
    std::thread::spawn(move || {
        while want_rx.recv().is_ok() {
            let mut line = String::new();
            match read(&mut line) {
                // Nothing more to read is the end, not an empty line.
                Ok(0) | Err(_) => {
                    // unheard-ok: the prompt side has already gone
                    let _ = lines_tx.send(None);
                    return;
                }
                Ok(_) => {
                    if lines_tx.send(Some(line)).is_err() {
                        return;
                    }
                }
            }
        }
    });
    (want_tx, lines_rx)
}

fn clean(line: String) -> Option<String> {
    let t = line.trim_end_matches(['\r', '\n']).to_string();
    if t.is_empty() {
        None
    } else {
        Some(t)
    }
}

/// Windows: read through PowerShell's `Read-Host -AsSecureString`.
///
/// # Why this, and what it costs
///
/// What was here before was `cmd /C echo off`, and it did nothing at all.
/// `echo off` is a *batch* directive about echoing script lines; it has no
/// effect on the console's input echo, and none whatsoever on the parent
/// console, since the child process exits immediately. The function then
/// returned `false` unconditionally, so the behaviour on Windows was the
/// honest fallback with a pointless process launch in front of it. Honest,
/// but every Windows passphrase went into the scrollback.
///
/// The console API that would do this properly — `GetConsoleMode` /
/// `SetConsoleMode` with `ENABLE_ECHO_INPUT` cleared — is not exposed by
/// Rust's standard library, and this tree does not take a dependency for it.
/// PowerShell ships on every supported Windows and its `-AsSecureString`
/// read does not echo.
///
/// The cost, stated rather than hidden: the passphrase comes back over a pipe
/// from a child process. That is a local pipe between two processes of the
/// same user, and the passphrase is never an argument, so it does not appear
/// in the command line, the process list, or any history. It is a smaller
/// exposure than printing it on the screen, which is what it replaces, and a
/// larger one than never leaving this process — which is what the console API
/// would buy, and what a later pass should do.
///
/// Since round 10 this is the fallback: `console::Quiet` reads in-process
/// first, and this runs only when there is no console to ask.
#[cfg(windows)]
fn masked_read(prompt: &str) -> Option<String> {
    use std::io::Write;
    // Our prompt, on our own console, because PowerShell's own would be
    // swallowed by the pipe we are reading its answer from.
    crate::out!("{prompt}");
    let _ = std::io::stdout().flush();

    let script = "\
        $s = Read-Host -AsSecureString; \
        $b = [Runtime.InteropServices.Marshal]::SecureStringToBSTR($s); \
        try { [Runtime.InteropServices.Marshal]::PtrToStringBSTR($b) } \
        finally { [Runtime.InteropServices.Marshal]::ZeroFreeBSTR($b) }";

    let out = crate::tools::command("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .stdin(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    crate::outln!();
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// How a typed passphrase is kept off the screen on this machine, for
/// `atlas doctor`: `(hidden, how)`. On Windows the console's echo is really
/// turned off and back on again to find out -- the check is the thing.
pub fn how_typing_is_hidden() -> (bool, &'static str) {
    #[cfg(windows)]
    {
        match console::Quiet::start() {
            Some(q) if q.echo_is_off() => {
                drop(q);
                (
                    true,
                    "in this process: the console's echo is switched off while you type",
                )
            }
            _ => (
                true,
                "through PowerShell's Read-Host -AsSecureString (no console to ask directly)",
            ),
        }
    }
    #[cfg(not(windows))]
    {
        (
            true,
            "the terminal's echo is switched off (stty -echo) while you type",
        )
    }
}

/// The Windows console, asked directly (`GetConsoleMode`/`SetConsoleMode`
/// with `ENABLE_ECHO_INPUT` cleared, then `ReadConsoleW`) -- the in-process
/// read the PowerShell path was standing in for. Opened as `CONIN$`, so it
/// works whether or not stdin was redirected, as long as there is a console.
#[cfg(windows)]
mod console {
    use windows::core::w;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Console::{
        GetConsoleMode, ReadConsoleW, SetConsoleMode, CONSOLE_MODE, ENABLE_ECHO_INPUT,
        ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
    };

    /// The console with its echo off; the mode it had comes back on drop,
    /// whatever happens in between.
    pub struct Quiet {
        h: HANDLE,
        was: CONSOLE_MODE,
    }

    impl Quiet {
        pub(super) fn start() -> Option<Quiet> {
            // SAFETY: plain console calls; the handle opened here is closed on the failure paths or
            // by `Quiet`'s Drop.
            unsafe {
                let h = CreateFileW(
                    w!("CONIN$"),
                    GENERIC_READ.0 | GENERIC_WRITE.0,
                    FILE_SHARE_READ | FILE_SHARE_WRITE,
                    None,
                    OPEN_EXISTING,
                    FILE_FLAGS_AND_ATTRIBUTES(0),
                    HANDLE::default(),
                )
                .ok()?;
                let mut was = CONSOLE_MODE(0);
                if GetConsoleMode(h, &mut was).is_err() {
                    let _ = CloseHandle(h);
                    return None;
                }
                let quiet = CONSOLE_MODE(
                    (was.0 & !ENABLE_ECHO_INPUT.0) | ENABLE_LINE_INPUT.0 | ENABLE_PROCESSED_INPUT.0,
                );
                if SetConsoleMode(h, quiet).is_err() {
                    let _ = CloseHandle(h);
                    return None;
                }
                Some(Quiet { h, was })
            }
        }

        /// Read back from the console itself, not assumed.
        pub(super) fn echo_is_off(&self) -> bool {
            let mut now = CONSOLE_MODE(0);
            // SAFETY: `self.h` is the console handle owned by this `Quiet` until Drop, and `now` is
            // a live local the call writes.
            unsafe { GetConsoleMode(self.h, &mut now).is_ok() && now.0 & ENABLE_ECHO_INPUT.0 == 0 }
        }

        /// One line, up to Enter. The buffer is zeroed after it's copied out.
        pub(super) fn read_line(&self) -> Option<String> {
            let mut all: Vec<u16> = Vec::new();
            let mut buf = [0u16; 256];
            loop {
                let mut n = 0u32;
                // SAFETY: `buf` is a live array of `buf.len()` u16 and the length passed is in
                // characters, as ReadConsoleW requires; `n` is a live local.
                let ok = unsafe {
                    ReadConsoleW(
                        self.h,
                        buf.as_mut_ptr() as *mut _,
                        buf.len() as u32,
                        &mut n,
                        None,
                    )
                };
                if ok.is_err() || n == 0 {
                    buf.iter_mut().for_each(|c| *c = 0);
                    return None;
                }
                all.extend_from_slice(&buf[..n as usize]);
                buf.iter_mut().for_each(|c| *c = 0);
                if all.contains(&(b'\n' as u16))
                    || all.contains(&(b'\r' as u16))
                    || all.len() > 4096
                {
                    break;
                }
            }
            let line = String::from_utf16_lossy(&all);
            all.iter_mut().for_each(|c| *c = 0);
            Some(line)
        }
    }

    impl Drop for Quiet {
        fn drop(&mut self) {
            // SAFETY: `self.h` is the console handle owned by this `Quiet`; it is restored and
            // closed here exactly once.
            unsafe {
                let _ = SetConsoleMode(self.h, self.was);
                let _ = CloseHandle(self.h);
            }
        }
    }
}

#[cfg(not(windows))]
fn masked_read(_prompt: &str) -> Option<String> {
    // Unix turns the echo off in the terminal itself, below.
    None
}

#[cfg(windows)]
fn hide_typing(_on: bool) -> bool {
    // Nothing here can turn the echo off; `masked_read` above is Windows'
    // answer, and reaching this means it could not be run. Saying `false`
    // makes the caller warn before a single character is typed, which is the
    // only useful thing left to do.
    false
}

#[cfg(not(windows))]
fn hide_typing(on: bool) -> bool {
    crate::tools::command("stty")
        .arg(if on { "-echo" } else { "echo" })
        .stdin(std::process::Stdio::inherit())
        // Its own complaint about a pipe is not news to the person typing --
        // they are about to be told, in a sentence, that the echo is on.
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 22 Oct 2026: nothing is read until a line is asked for, so a secret
    /// typed at a masked prompt is never taken as the next command.
    #[test]
    fn lines_are_read_only_when_asked_for() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        let reads = Arc::new(AtomicUsize::new(0));
        let seen = reads.clone();
        let (want, lines) = lines_on_demand(move |buf: &mut String| {
            let n = seen.fetch_add(1, Ordering::SeqCst);
            buf.push_str(&format!("line {n}\n"));
            Ok(buf.len())
        });
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(reads.load(Ordering::SeqCst), 0, "read without being asked");
        assert!(lines.try_recv().is_err());
        want.send(()).unwrap();
        let got = lines
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap();
        assert_eq!(got.as_deref(), Some("line 0\n"));
        std::thread::sleep(std::time::Duration::from_millis(150));
        assert_eq!(reads.load(Ordering::SeqCst), 1, "read ahead of the prompt");
    }

    #[test]
    fn the_end_of_input_is_none() {
        let (want, lines) = lines_on_demand(|_: &mut String| Ok(0));
        want.send(()).unwrap();
        assert_eq!(
            lines.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            None
        );
    }

    /// The default that keeps a test suite from hanging on stdin, and keeps
    /// Atlas from claiming to have asked.
    struct Nowhere;
    impl AsksQuietly for Nowhere {
        fn ask(&self, _: &str) -> Option<String> {
            None
        }
    }

    #[test]
    fn nothing_typed_is_not_a_wrong_answer() {
        // The distinction the whole trait turns on. A cancelled prompt has to
        // be distinguishable from a refused passphrase, because one of them
        // should be counted against you and the other is you changing your
        // mind.
        let asker: Box<dyn AsksQuietly> = Box::new(Nowhere);
        assert_eq!(asker.ask("Passphrase: "), None);
    }

    #[test]
    fn a_blank_line_is_nothing_typed() {
        assert_eq!(clean("\n".into()), None);
        assert_eq!(clean("".into()), None);
        assert_eq!(
            clean("  \r\n".into()),
            Some("  ".into()),
            "leading space is part of it"
        );
        assert_eq!(clean("hunter2\r\n".into()), Some("hunter2".into()));
    }

    #[test]
    fn a_prompt_says_where_it_will_appear() {
        // Said out loud to somebody looking at a microphone. "Type it" is
        // not directions.
        assert!(Console.where_it_appears().contains("terminal"));
        assert!(!Nowhere.where_it_appears().is_empty());
    }
}
