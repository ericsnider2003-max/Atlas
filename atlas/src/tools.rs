//! External tool runner.
//!
//! Every heavy capability Atlas needs — audio capture, speech-to-text,
//! speech synthesis, playback, screen capture — is a free single-binary
//! program that already exists and is better than anything we'd write.
//! So Atlas shells out to them through a config-declared template instead
//! of linking bindings.
//!
//! Consequences that matter:
//!   * swapping Whisper for Vosk, or Piper for Windows SAPI, is a YAML edit
//!   * nothing here is Windows-specific, so it is testable on any machine
//!   * a missing binary is a clear error, not a link failure

use crate::error::{AtlasError, Result};
use serde::Deserialize;
use std::collections::BTreeMap;
use std::io::Write;
use std::process::Stdio;

pub type Vars = BTreeMap<String, String>;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExternalTool {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Pipe the supplied text into the process's stdin (Piper works this way).
    #[serde(default)]
    pub stdin_text: bool,
    /// If set, the tool's result is the contents of this file rather than its
    /// stdout. Whisper's `-otxt` mode works this way.
    #[serde(default)]
    pub result_file: Option<String>,
    /// Give up after this long and kill the process.
    ///
    /// Every external tool ran on `wait_with_output()` with nothing to stop
    /// it. A model that stalls on load, an ffmpeg waiting on a device that
    /// went away, a whisper build sitting on stdin — any of them blocked the
    /// caller for as long as the process lived, which on the voice path is the
    /// turn loop. From outside, Atlas is simply dead, with no error and
    /// nothing in the log.
    ///
    /// Two minutes suits transcription of a long clip. Anything that
    /// legitimately runs longer sets its own.
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    120
}

/// A process kept from running forever.
///
/// `std` has no `wait_with_output` with a deadline, so this polls `try_wait`
/// while two threads drain stdout and stderr.
///
/// The threads are the part that matters and the part it is easy to skip. A
/// child whose pipe buffer fills blocks on write, so a caller that polls
/// without reading deadlocks against the very process it is trying to time
/// out — and whisper writes progress to stderr the whole way through.
///
/// ## Why stdin is fed from in here, on a thread of its own
///
/// It used to be written by the caller, before this function was ever
/// entered:
///
/// ```text
/// pipe.write_all(stdin.unwrap_or("").as_bytes())?;   // <- blocking
/// let out = wait_or_kill(child, &cmd, limit)?;       // <- where the drains start
/// ```
///
/// A pipe holds a fixed amount — 64KB on Linux, as little as 4KB for a
/// Windows anonymous pipe. Hand a tool more than that and `write_all` blocks
/// until the tool reads some. A tool that is *also* writing — piper emitting
/// WAV to stdout, whisper writing progress to stderr — fills its own output
/// pipe, which nobody is draining yet, and blocks too. Both sides are then
/// waiting for the other, and the timeout that exists precisely for this is
/// two lines further down, never reached. Atlas hangs with no error and
/// nothing in the log, which is the failure this whole function was written
/// to remove.
///
/// Writing on a thread, started with the drains, means the deadline covers
/// the feeding as well as the running: on timeout the child is killed, which
/// closes the pipe, and the writer gets `EPIPE` and ends.
/// How long to wait before the next look at a running tool: 1ms, doubling,
/// to a 200ms ceiling.
///
/// It was a flat 25ms, which was fine when the longest tool was a two-minute
/// transcription and absurd for an hour-long render: 144,000 wake-ups. This
/// is better at both ends. A tool finishing in 30ms is noticed almost at once
/// instead of up to 25ms late, and the hour costs about 18,000 wake-ups. The
/// price is up to 200ms between the deadline passing and the kill.
pub fn poll_gap(polls: u32) -> std::time::Duration {
    const CEILING_MS: u64 = 200;
    std::time::Duration::from_millis(1u64.checked_shl(polls.min(16)).unwrap_or(CEILING_MS).min(CEILING_MS))
}

fn wait_or_kill(
    mut child: std::process::Child,
    cmd: &str,
    limit: std::time::Duration,
    feed: Option<String>,
    stop: Option<&dyn Fn() -> bool>,
) -> Result<Option<std::process::Output>> {
    use std::io::Read;

    let mut si = child.stdin.take();
    let in_t = std::thread::spawn(move || -> Option<String> {
        let mut trouble = None;
        if let Some(pipe) = si.as_mut() {
            let text = feed.unwrap_or_default();
            if let Err(e) = pipe.write_all(text.as_bytes()).and_then(|()| pipe.flush()) {
                trouble = Some(e.to_string());
            }
        }
        // Closes the write end, so the tool sees end-of-input rather than
        // waiting for more. Explicit because it is the whole reason the
        // handle was taken out of the child.
        drop(si);
        trouble
    });

    let mut so = child.stdout.take();
    let mut se = child.stderr.take();
    let out_t = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = so.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });
    let err_t = std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(p) = se.as_mut() {
            let _ = p.read_to_end(&mut buf);
        }
        buf
    });

    let started = std::time::Instant::now();
    let mut polls: u32 = 0;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            Err(e) => {
                return Err(AtlasError::Platform(format!("waiting on '{cmd}': {e}")));
            }
            Ok(None) => {}
        }
        if started.elapsed() >= limit {
            let _ = child.kill();
            let _ = child.wait();
            return Err(AtlasError::Platform(format!(
                "'{cmd}' was still running after {}s, so I stopped it. \
                 If it needs longer, raise timeout_secs for that tool.",
                limit.as_secs()
            )));
        }
        // Asked to stop (Atlas paused, the microphone wanted elsewhere, a
        // reply cut off): ended now, and not an error.
        if stop.is_some_and(|f| f()) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(None);
        }
        let gap = if stop.is_some() { poll_gap(polls).min(std::time::Duration::from_millis(25)) } else { poll_gap(polls) };
        std::thread::sleep(gap.min(limit.saturating_sub(started.elapsed())));
        polls = polls.saturating_add(1);
    };

    // The child has exited, so its end of the stdin pipe is closed and the
    // writer cannot still be blocked on it.
    let feeding = in_t.join().unwrap_or(None);
    let mut stderr = err_t.join().unwrap_or_default();
    if let Some(why) = feeding {
        if !status.success() {
            // Only when the tool actually failed. A tool that ignores the
            // rest of its input and succeeds anyway -- piper given more text
            // than it needed -- is not a fault to report.
            stderr.extend_from_slice(
                format!("\n(and I couldn't finish handing it the text: {why})").as_bytes(),
            );
        }
    }

    Ok(Some(std::process::Output {
        status,
        stdout: out_t.join().unwrap_or_default(),
        stderr,
    }))
}

/// Replace every `{name}` with vars["name"]. Unknown placeholders are left
/// alone so a typo shows up in the error message instead of silently
/// becoming an empty string.
pub fn expand(template: &str, vars: &Vars) -> String {
    let mut out = String::with_capacity(template.len());
    let bytes: Vec<char> = template.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '{' {
            if let Some(close) = bytes[i + 1..].iter().position(|c| *c == '}') {
                let key: String = bytes[i + 1..i + 1 + close].iter().collect();
                if let Some(v) = vars.get(&key) {
                    out.push_str(v);
                    i += close + 2;
                    continue;
                }
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

impl ExternalTool {
    pub fn resolved(&self, vars: &Vars) -> (String, Vec<String>) {
        (
            expand(&self.command, vars),
            self.args
                .iter()
                .map(|a| expand(a, vars))
                // An argument that expands to nothing is dropped rather than
                // passed as an empty string. Nothing in the shipped templates
                // ever means to pass a literal empty argument, and an empty
                // one is almost always a bug (e.g. `-l ""` to whisper). This
                // is what lets an optional flag pair — `{lang_opt} {lang_val}`
                // — vanish cleanly when there is no language to set, without
                // the template system needing conditionals.
                .filter(|a| !a.is_empty())
                .collect(),
        )
    }

    /// Run it. Returns stdout, or the contents of `result_file` if configured.
    pub fn run(&self, vars: &Vars, stdin: Option<&str>) -> Result<String> {
        self.run_unless(vars, stdin, None).map(Option::unwrap_or_default)
    }

    /// `run`, ended early when `stop` says so: `Ok(None)` is "stopped", not
    /// a failure. For the recorder and the player, which Atlas must be able
    /// to stop mid-way — the wake word's clip when you pause, a reply when
    /// you speak over it.
    pub fn run_stoppable(&self, vars: &Vars, stdin: Option<&str>, stop: &dyn Fn() -> bool) -> Result<Option<String>> {
        self.run_unless(vars, stdin, Some(stop))
    }

    fn run_unless(&self, vars: &Vars, stdin: Option<&str>, stop: Option<&dyn Fn() -> bool>) -> Result<Option<String>> {
        let (cmd, args) = self.resolved(vars);

        // On a phone Atlas can't start other programs at all (iOS forbids it;
        // Android has no `curl`), and every model question -- to the phone's
        // own model or the laptop's over Tailscale -- is a `curl` to a plain
        // http:// address. Those are answered in-process (OPEN_GAPS P.7).
        #[cfg(any(target_os = "ios", target_os = "android"))]
        if self.result_file.is_none() {
            if let Some(done) = curl_in_process(&cmd, &args, stdin, self.timeout_secs) {
                return done.map(Some);
            }
        }

        let child = crate::tools::command(&cmd)
            .args(&args)
            .stdin(if self.stdin_text {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| {
                AtlasError::Platform(format!(
                    "could not start '{cmd}': {e}. Is it installed and on PATH? \
                     The Connections page shows what's missing."
                ))
            })?;

        let limit = std::time::Duration::from_secs(if self.timeout_secs == 0 {
            default_timeout()
        } else {
            self.timeout_secs
        });
        // Handed over rather than written here. See `wait_or_kill`: writing
        // it before the drain threads exist is a deadlock that the timeout
        // below cannot break, because the timeout starts inside the function
        // the write never returns from.
        let feed = self.stdin_text.then(|| stdin.unwrap_or("").to_string());
        let Some(out) = wait_or_kill(child, &cmd, limit, feed, stop)? else {
            return Ok(None);
        };
        if !out.status.success() {
            let err = String::from_utf8_lossy(&out.stderr);
            return Err(AtlasError::Platform(format!(
                "'{cmd}' exited {}: {}",
                out.status.code().unwrap_or(-1),
                err.trim()
            )));
        }

        match &self.result_file {
            Some(t) => {
                let path = expand(t, vars);
                std::fs::read_to_string(&path).map(Some).map_err(|e| {
                    AtlasError::Platform(format!("'{cmd}' produced no output at {path}: {e}"))
                })
            }
            None => Ok(Some(String::from_utf8_lossy(&out.stdout).to_string())),
        }
    }

    /// Is the binary present? Used by `atlas doctor`.
    pub fn available(&self, vars: &Vars) -> bool {
        let (cmd, _) = self.resolved(vars);
        which(&cmd).is_some()
    }
}

/// A `curl` to a plain `http://` address, done in-process: what Atlas's own
/// model calls (`models::server_post`, `server_get`) and hand-written
/// `tools.llm` settings ask for. `None` when it isn't that (another program,
/// https, a flag this doesn't read), so the caller runs the real `curl`.
///
/// Reads: `-X METHOD`, `-d`/`--data`/`--data-binary` (`@-` is stdin),
/// `-m`/`--max-time`, and ignores `-s`, `-S`, `-f`, `--noproxy <x>` and
/// `-H` (the body is JSON either way). Anything else declines, so nothing is
/// silently dropped. Failing the way curl with `-f` would: a status of 400 or
/// more is an error, not a body.
pub fn curl_in_process(cmd: &str, args: &[String], stdin: Option<&str>, timeout_secs: u64) -> Option<Result<String>> {
    // Either separator: a Windows path read anywhere.
    let name = cmd.rsplit(['/', '\\']).next()?.to_lowercase();
    if name != "curl" && name != "curl.exe" {
        return None;
    }
    let (mut method, mut body, mut url, mut secs) = (None::<String>, None::<String>, None::<String>, timeout_secs);
    let mut i = 0;
    while i < args.len() {
        let a = args[i].as_str();
        let next = || args.get(i + 1).cloned();
        match a {
            "-s" | "-S" | "-sS" | "-f" | "--fail" | "--silent" | "--show-error" => {}
            "--noproxy" | "-H" | "--header" => i += 1,
            "-X" | "--request" => {
                method = Some(next()?);
                i += 1;
            }
            "-d" | "--data" | "--data-binary" | "--data-raw" => {
                let d = next()?;
                body = Some(if d == "@-" { stdin.unwrap_or("").to_string() } else { d });
                i += 1;
            }
            "-m" | "--max-time" => {
                secs = next()?.parse::<f64>().ok()?.ceil() as u64;
                i += 1;
            }
            u if u.starts_with("http://") && url.is_none() => url = Some(u.to_string()),
            _ => return None,
        }
        i += 1;
    }
    let url = url?;
    let rest = url.strip_prefix("http://")?;
    let (host, path) = match rest.find('/') {
        Some(k) => (&rest[..k], &rest[k..]),
        None => (rest, "/"),
    };
    let host = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    let timeout = std::time::Duration::from_secs(if secs == 0 { default_timeout() } else { secs });
    let method = method.unwrap_or_else(|| if body.is_some() { "POST".into() } else { "GET".into() });
    let got = match method.as_str() {
        "GET" => crate::http::get(&host, path, timeout),
        "POST" => crate::http::post_json(&host, path, body.as_deref().unwrap_or(""), timeout),
        _ => return None,
    };
    Some(got.and_then(|r| {
        if r.status >= 400 {
            Err(AtlasError::Platform(format!("{url} answered {}: {}", r.status, r.body.trim())))
        } else {
            Ok(r.body)
        }
    }))
}

pub fn which(cmd: &str) -> Option<String> {
    let p = std::path::Path::new(cmd);
    if p.is_absolute() || cmd.contains('/') || cmd.contains('\\') {
        return p.exists().then(|| cmd.to_string());
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.BAT;.CMD".into())
            .split(';')
            .map(|s| s.to_string())
            .collect()
    } else {
        vec![String::new()]
    };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for ext in &exts {
            let cand = dir.join(format!("{cmd}{ext}"));
            if cand.is_file() {
                return Some(cand.display().to_string());
            }
        }
    }
    None
}

/// A program Atlas starts, with no window of its own. Atlas is a windowed
/// program (27 Sep 2026: Eric saw a command prompt open with Atlas), and on
/// Windows every console program it starts -- the model server, Tor,
/// Tailscale, PowerShell -- would otherwise open a console window of its own.
/// Programs you ask Atlas to open go through `platform` instead, and show.
pub fn command(program: impl AsRef<std::ffi::OsStr>) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}
