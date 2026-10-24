//! A speech recogniser that stays loaded between turns (20 Oct 2026).
//!
//! Until now every utterance started a program of its own -- whisper-cli, or
//! sherpa for Parakeet -- and most of the 2.6 to 3.5 seconds a turn spent
//! hearing you was that program reading its model off the disk again. Here
//! `whisper-server` (it ships beside whisper-cli) is started the first time
//! something needs hearing, kept, and asked over loopback for every clip
//! after: the model is read once.
//!
//! * Owned: tied to Atlas by `childjob::tie`, so it ends when Atlas ends or
//!   crashes, and stopped here after `idle_minutes` with no clip (a reaper
//!   thread, so no caller has to remember).
//! * Private: bound to 127.0.0.1 on a free port picked at start. Never
//!   0.0.0.0, never a fixed port another program could be sitting on.
//! * Never the only way to hear: when it will not start, or a request fails,
//!   the caller runs the old per-clip program for that clip and the reason is
//!   returned (and written to `data/logs/asr-server.events.log`). The server
//!   is tried again after a growing wait, not on every clip.
//! * Cancellable: a clip abandoned by `stop` is abandoned at once (the
//!   request is dropped; the server finishes its own decode in the background).
//! * Memory relief keeps it. It is the floor's ears: with the talking model
//!   gone, a spoken command still has to be heard (`relief::Step::Recogniser`,
//!   the very last step, and only while memory is still critically short).
//!
//! The lifecycle is a pure state machine ([`Lifecycle`]) with time passed in,
//! tested below; the process, the socket and the clock are the thin shell.
//!
//! Parakeet is not resident here. Its program (sherpa-onnx-offline) has no
//! server mode in this build; making it resident would need either the
//! sherpa-onnx websocket server (`sherpa-onnx-offline-websocket-server`,
//! speaking its own protocol, owned and bound exactly as this one is) or an
//! in-process recogniser through the `ort` crate Atlas already links. Until
//! one is installed, the whisper path above is what stays resident.

use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Settings, under `resident_asr:` in tools.yaml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Keep the recogniser loaded between turns. Off means the old
    /// program-per-clip path, always.
    pub enabled: bool,
    /// Minutes with no clip before the server is stopped.
    pub idle_minutes: u64,
    /// Processor threads for the server: 0 is half the machine's, at least 2.
    pub threads: u32,
    /// Whisper's audio context, in encoder frames (1500 is the whole 30 s
    /// window; 750 reads the first 15 s and about halves the time per clip,
    /// measured 0.83 s to 0.40 s on base.en q5_1). 0 is the whole window:
    /// clips longer than the context are cut, so it is not on by default.
    pub audio_ctx: u32,
    /// Seconds one clip may take before it is given to the fallback.
    pub request_timeout_secs: u64,
    /// Seconds the server may take to load its model before it is given up on.
    pub start_timeout_secs: u64,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            idle_minutes: 30,
            threads: 0,
            audio_ctx: 0,
            request_timeout_secs: 20,
            start_timeout_secs: 30,
        }
    }
}

/// Half the machine's processors, at least two and at most four (logical
/// processors halved approximates the physical cores). Kokoro moved to one
/// thread on eight-processor machines on 23 Oct 2026; the recogniser was not
/// re-measured, so it keeps this reading.
pub fn server_thread_count(cfg: &Config) -> u32 {
    if cfg.threads > 0 {
        return cfg.threads;
    }
    let n = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2);
    (n / 2).clamp(2, 4) as u32
}

// ------------------------------------------------------------ the state machine

/// Where the server is. Times are milliseconds on a clock the caller reads.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// Not running; may be started now.
    Stopped,
    /// One caller is starting it (21 Oct 2026). Two clips at once used to both
    /// see `Stopped`, both start a server, and leave one running unowned.
    Starting,
    /// Running and answering; `last_used` is the last clip's time.
    Ready { last_used: u64 },
    /// Failed; not tried again before `until`.
    Backoff { until: u64 },
}

/// What a caller should do for the next clip.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Ask the running server.
    Use,
    /// Start it, then ask it. Only [`Lifecycle::claim`] hands this out to one
    /// caller; the state is `Starting` from then on.
    Start,
    /// Another caller is starting it: wait for that, then plan again.
    Wait,
    /// Use the per-clip program; the server is resting after a failure.
    Fallback { retry_in_ms: u64 },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lifecycle {
    pub state: State,
    /// Failures in a row; reset by a clip it heard.
    pub failures: u32,
}

/// The wait after the nth failure in a row: 5 s, 10 s, 20 s ... at most 5 min.
pub fn backoff_ms(failures: u32) -> u64 {
    let n = failures.saturating_sub(1).min(6);
    (5_000u64 << n).min(300_000)
}

impl Default for Lifecycle {
    fn default() -> Self {
        Lifecycle {
            state: State::Stopped,
            failures: 0,
        }
    }
}

impl Lifecycle {
    pub fn plan(&self, now: u64) -> Plan {
        match self.state {
            State::Ready { .. } => Plan::Use,
            State::Stopped => Plan::Start,
            State::Starting => Plan::Wait,
            State::Backoff { until } if now >= until => Plan::Start,
            State::Backoff { until } => Plan::Fallback {
                retry_in_ms: until - now,
            },
        }
    }
    /// `plan`, and when it is `Start` the caller now owns the start: the
    /// state becomes `Starting`, so a second caller planning at the same time
    /// gets `Wait`. Call under the lock that guards the lifecycle.
    pub fn claim(&mut self, now: u64) -> Plan {
        let plan = self.plan(now);
        if plan == Plan::Start {
            self.state = State::Starting;
        }
        plan
    }
    /// The caller that claimed the start gave up before starting (a stop):
    /// back to `Stopped`, not a failure.
    pub fn start_abandoned(&mut self) {
        if self.state == State::Starting {
            self.state = State::Stopped;
        }
    }
    /// The process is up and listening.
    pub fn started(&mut self, now: u64) {
        self.state = State::Ready { last_used: now };
    }
    /// It would not start. The clip goes to the fallback.
    pub fn start_failed(&mut self, now: u64) {
        self.failures += 1;
        self.state = State::Backoff {
            until: now + backoff_ms(self.failures),
        };
    }
    /// It heard a clip.
    pub fn heard(&mut self, now: u64) {
        self.failures = 0;
        self.state = State::Ready { last_used: now };
    }
    /// A request failed: the process is treated as dead (the caller stops
    /// it), and the next clips go to the fallback until the wait is over.
    pub fn request_failed(&mut self, now: u64) {
        self.start_failed(now);
    }
    /// Has it sat unused for `limit_ms`?
    pub fn idle_expired(&self, now: u64, limit_ms: u64) -> bool {
        matches!(self.state, State::Ready { last_used } if now.saturating_sub(last_used) >= limit_ms)
    }
    /// The process has been stopped (idle, relief, or Atlas ending).
    pub fn stopped(&mut self) {
        if matches!(self.state, State::Ready { .. }) {
            self.state = State::Stopped;
        }
    }
    /// Memory relief: only the last step may stop it, and only while memory is
    /// still critically short. Returns whether the caller should stop it.
    pub fn relief(&mut self, last_step: bool, critical: bool) -> bool {
        if matches!(self.state, State::Ready { .. }) && last_step && critical {
            self.stopped();
            true
        } else {
            false
        }
    }
}

// ----------------------------------------------------------------- the process

struct Running {
    child: std::process::Child,
    port: u16,
    /// The model the server loaded; a different one asked for restarts it.
    model: String,
}

struct Shared {
    life: Lifecycle,
    running: Option<Running>,
    reaper: bool,
}

static SHARED: Mutex<Shared> = Mutex::new(Shared {
    life: Lifecycle {
        state: State::Stopped,
        failures: 0,
    },
    running: None,
    reaper: false,
});

fn shared() -> std::sync::MutexGuard<'static, Shared> {
    SHARED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn clock() -> u64 {
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    T0.get_or_init(Instant::now).elapsed().as_millis() as u64
}

/// What happened, for the caller to put in its record and for the log file.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    /// What the server heard (raw; the caller cleans it).
    Text(String),
    /// `stop` said so: nothing else should be asked.
    Canceled,
    /// Not heard here; run the per-clip program. The reason is in words.
    Unavailable(String),
}

/// `whisper-server` beside the whisper program named by `vars["whisper"]`.
fn server_program(whisper_cli: &str) -> Option<PathBuf> {
    let dir = Path::new(whisper_cli).parent()?;
    let exe = dir.join(if cfg!(windows) {
        "whisper-server.exe"
    } else {
        "whisper-server"
    });
    exe.is_file().then_some(exe)
}

fn note(line: &str) {
    crate::errln!("asr-server: {line}");
    let dir = crate::roots::data_dir().join("logs");
    crate::heard!(std::fs::create_dir_all(&dir));
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("asr-server.events.log"))
    {
        use std::io::Write as _;
        crate::kept!(writeln!(f, "{line}"));
    }
}

fn free_loopback_port() -> std::io::Result<u16> {
    let l = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    Ok(l.local_addr()?.port())
}

/// The arguments that make the server private and loaded with `model`.
pub fn whisper_server_args(model: &str, port: u16, threads: u32, audio_ctx: u32) -> Vec<String> {
    let mut a = vec![
        "-m".into(),
        model.into(),
        "--host".into(),
        "127.0.0.1".into(),
        "--port".into(),
        port.to_string(),
        "-t".into(),
        threads.to_string(),
        "-nt".into(),
        // A plain CPU decode; the server's graphics path was not needed for a
        // base model and is one more thing that can fail to start.
        "-ng".into(),
    ];
    if audio_ctx > 0 {
        a.push("-ac".into());
        a.push(audio_ctx.to_string());
    }
    a
}

/// What a loopback port answered to a plain `GET /`.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Probe {
    /// whisper-server's own page.
    Whisper,
    /// Something else answered.
    Foreign,
    /// Nothing listening yet, or no answer in time.
    NotYet,
}

/// Does this reply look like whisper-server's? Its `GET /` is an HTML page
/// titled "Whisper.cpp Server" (read out of the shipped whisper-server.exe).
pub fn looks_like_whisper_server(raw: &[u8]) -> bool {
    match split_reply(raw) {
        Some((200, body)) => String::from_utf8_lossy(body)
            .to_ascii_lowercase()
            .contains("whisper.cpp server"),
        _ => false,
    }
}

fn probe(port: u16) -> Probe {
    use std::io::{Read, Write};
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let Ok(mut s) = std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(200)) else {
        return Probe::NotYet;
    };
    let timeout = Some(Duration::from_millis(1500));
    if s.set_read_timeout(timeout).is_err() || s.set_write_timeout(timeout).is_err() {
        return Probe::NotYet;
    }
    let head = format!("GET / HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n");
    if s.write_all(head.as_bytes()).is_err() {
        return Probe::NotYet;
    }
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    let until = Instant::now() + Duration::from_millis(1500);
    while raw.len() < 64 * 1024 && Instant::now() < until {
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => raw.extend_from_slice(&buf[..n]),
            // A timeout, or the server closing (a reset on Windows once it
            // has sent everything): what was read is the answer.
            Err(_) => break,
        }
    }
    if raw.is_empty() {
        Probe::NotYet
    } else if looks_like_whisper_server(&raw) {
        Probe::Whisper
    } else {
        Probe::Foreign
    }
}

fn spawn(program: &Path, model: &str, cfg: &Config) -> Result<Running, String> {
    let port = free_loopback_port().map_err(|e| format!("no free loopback port: {e}"))?;
    let dir = crate::roots::data_dir().join("logs");
    crate::heard!(std::fs::create_dir_all(&dir));
    let log = match std::fs::File::create(dir.join("asr-server.log")) {
        Ok(f) => std::process::Stdio::from(f),
        Err(_) => std::process::Stdio::null(),
    };
    let mut child = crate::tools::command(program)
        .args(whisper_server_args(
            model,
            port,
            server_thread_count(cfg),
            cfg.audio_ctx,
        ))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(log)
        .spawn()
        .map_err(|e| format!("could not start {}: {e}", program.display()))?;
    crate::childjob::tie(&child);
    let limit = Instant::now() + Duration::from_secs(cfg.start_timeout_secs.max(1));
    loop {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(format!("server exited while loading ({status})"));
        }
        // Ready means whisper-server answered, not that something accepted a
        // connection: the port was only free a moment ago, and a foreign
        // listener there would be sent the owner's audio (21 Oct 2026).
        match probe(port) {
            Probe::Whisper => {
                return Ok(Running {
                    child,
                    port,
                    model: model.to_string(),
                })
            }
            Probe::Foreign => {
                crate::kept!(child.kill());
                crate::kept!(child.wait());
                return Err(format!(
                    "something that is not whisper-server answered on port {port}; no audio was sent to it"
                ));
            }
            Probe::NotYet => {}
        }
        if Instant::now() >= limit {
            crate::kept!(child.kill());
            crate::kept!(child.wait());
            return Err(format!(
                "server did not listen within {} s",
                cfg.start_timeout_secs
            ));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn kill(mut r: Running) {
    crate::kept!(r.child.kill());
    crate::kept!(r.child.wait());
}

/// Stop the server now (Atlas ending, a different model, the last relief
/// step). True when one was running.
pub fn stop_server_now(why: &str) -> bool {
    let mut s = shared();
    s.life.stopped();
    match s.running.take() {
        Some(r) => {
            drop(s);
            kill(r);
            note(&format!("stopped: {why}"));
            true
        }
        None => false,
    }
}

/// Is the server loaded right now? (For relief and the doctor.)
pub fn resident() -> bool {
    shared().running.is_some()
}

/// Memory relief's last step. Stops it only when `critical`; the reason is
/// logged either way. True when it was stopped.
pub fn relieve_last(critical: bool) -> bool {
    let go = {
        let mut s = shared();
        s.life.relief(true, critical)
    };
    if go {
        if let Some(r) = shared().running.take() {
            kill(r);
        }
        note("stopped: memory critically short after every other step");
    }
    go
}

fn ensure_reaper(idle: Duration) {
    {
        let mut s = shared();
        if s.reaper {
            return;
        }
        s.reaper = true;
    }
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(30).min(idle.max(Duration::from_millis(50))));
        let expired = {
            let mut s = shared();
            if s.running.is_none() {
                // Decided under the same lock that clears the flag, so a
                // server started after this starts a reaper of its own.
                s.reaper = false;
                return;
            }
            s.life.idle_expired(clock(), idle.as_millis() as u64)
        };
        if expired {
            stop_server_now(&format!("idle for {} minutes", idle.as_secs() / 60));
        }
    });
}

// --------------------------------------------------------------------- request

/// A multipart body for `/inference`: the WAV and the settings.
pub fn multipart(boundary: &str, wav: &[u8], fields: &[(&str, &str)]) -> Vec<u8> {
    let mut b = Vec::with_capacity(wav.len() + 512);
    for (k, v) in fields {
        b.extend_from_slice(
            format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n")
                .as_bytes(),
        );
    }
    b.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"clip.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    b.extend_from_slice(wav);
    b.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    b
}

/// The body of an HTTP reply, and whether it said 200.
pub fn split_reply(raw: &[u8]) -> Option<(u16, &[u8])> {
    let end = raw.windows(4).position(|w| w == b"\r\n\r\n")?;
    let head = std::str::from_utf8(&raw[..end]).ok()?;
    let status = head.split_whitespace().nth(1)?.parse().ok()?;
    Some((status, &raw[end + 4..]))
}

/// One request: `Ok(None)` when `stop` abandoned it.
fn ask(
    port: u16,
    wav: &[u8],
    fields: &[(&str, &str)],
    timeout: Duration,
    stop: &dyn Fn() -> bool,
) -> Result<Option<String>, String> {
    use std::io::{Read, Write};
    let boundary = "----atlasasr7d1f3b";
    let body = multipart(boundary, wav, fields);
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let mut s = std::net::TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| format!("connect: {e}"))?;
    s.set_write_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| e.to_string())?;
    // Short reads, so `stop` is looked at ten times a second.
    s.set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(|e| e.to_string())?;
    let head = format!(
        "POST /inference HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nContent-Type: multipart/form-data; boundary={boundary}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    s.write_all(head.as_bytes())
        .and_then(|_| s.write_all(&body))
        .map_err(|e| format!("send: {e}"))?;
    let limit = Instant::now() + timeout;
    let mut raw = Vec::new();
    let mut buf = [0u8; 8192];
    loop {
        if stop() {
            return Ok(None);
        }
        match s.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                raw.extend_from_slice(&buf[..n]);
                if raw.len() > 4 * 1024 * 1024 {
                    return Err("reply too large".into());
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                if Instant::now() >= limit {
                    return Err(format!("no answer within {} s", timeout.as_secs()));
                }
            }
            // Windows reports the server closing as a reset once it has
            // sent everything; what was read is the answer.
            Err(e) if !raw.is_empty() && e.kind() == std::io::ErrorKind::ConnectionReset => break,
            Err(e) => return Err(format!("read: {e}")),
        }
    }
    let (status, text) = split_reply(&raw).ok_or("not an HTTP reply")?;
    let text = String::from_utf8_lossy(text).to_string();
    if status != 200 {
        return Err(format!("status {status}: {}", text.trim()));
    }
    // The server answers an unreadable clip with {"error": "..."}.
    if text.trim_start().starts_with('{') {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
            if let Some(e) = v.get("error") {
                return Err(format!("server said: {e}"));
            }
            if let Some(t) = v.get("text").and_then(|t| t.as_str()) {
                return Ok(Some(t.to_string()));
            }
        }
    }
    Ok(Some(text))
}

/// Hear the clip at `wav` with the resident server, starting it if need be.
/// `vars` are the same ones whisper-cli's command reads (`whisper`,
/// `stt_model`, `lang_val`, `task_opt`, `hint_val`), so the same model,
/// language and hints apply either way.
pub fn transcribe(
    cfg: &Config,
    vars: &crate::tools::Vars,
    wav: &Path,
    stop: &dyn Fn() -> bool,
) -> Outcome {
    let get = |k: &str| vars.get(k).map(|s| s.trim()).unwrap_or("");
    let Some(program) = server_program(get("whisper")) else {
        return Outcome::Unavailable("no whisper-server beside whisper-cli".into());
    };
    let model = get("stt_model").to_string();
    if model.is_empty() {
        return Outcome::Unavailable("no speech model named".into());
    }
    let bytes = match std::fs::read(wav) {
        Ok(b) => b,
        Err(e) => return Outcome::Unavailable(format!("clip unreadable: {e}")),
    };
    let now = clock();
    // A different model than the one loaded: restart with the new one.
    let wrong_model = shared().running.as_ref().is_some_and(|r| r.model != model);
    if wrong_model {
        stop_server_now("the speech model changed");
    }
    // The child may have died by itself (killed, crashed): treat as stopped.
    {
        let mut s = shared();
        let dead = match s.running.as_mut() {
            Some(r) => matches!(r.child.try_wait(), Ok(Some(_))),
            None => false,
        };
        if dead {
            s.running = None;
            s.life.request_failed(now);
            drop(s);
            note("server had exited on its own; waiting before restarting it");
        }
    }
    // Planned and, for a start, claimed under one lock: a second clip at the
    // same moment waits for this start instead of starting another server.
    let waited = Instant::now();
    let wait_limit = Duration::from_secs(cfg.start_timeout_secs.max(1) + 5);
    let plan = loop {
        let p = shared().life.claim(clock());
        if p != Plan::Wait {
            break p;
        }
        if stop() {
            return Outcome::Canceled;
        }
        if waited.elapsed() >= wait_limit {
            return Outcome::Unavailable("resident recogniser is still starting".into());
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    match plan {
        // Waited out above; never reached.
        Plan::Wait => return Outcome::Unavailable("resident recogniser is still starting".into()),
        Plan::Fallback { retry_in_ms } => {
            return Outcome::Unavailable(format!(
                "resident recogniser resting after a failure (retry in {} s)",
                retry_in_ms.div_ceil(1000)
            ));
        }
        Plan::Start => {
            if stop() {
                shared().life.start_abandoned();
                return Outcome::Canceled;
            }
            let began = Instant::now();
            match spawn(&program, &model, cfg) {
                Ok(r) => {
                    let port = r.port;
                    let mut s = shared();
                    s.life.started(clock());
                    s.running = Some(r);
                    drop(s);
                    ensure_reaper(Duration::from_secs(cfg.idle_minutes.max(1) * 60));
                    note(&format!(
                        "started on 127.0.0.1:{port} with {model} in {} ms",
                        began.elapsed().as_millis()
                    ));
                }
                Err(why) => {
                    shared().life.start_failed(clock());
                    note(&format!("start failed: {why}"));
                    return Outcome::Unavailable(format!("resident recogniser: {why}"));
                }
            }
        }
        Plan::Use => {}
    }
    let Some(port) = shared().running.as_ref().map(|r| r.port) else {
        return Outcome::Unavailable("resident recogniser stopped".into());
    };
    let mut fields: Vec<(&str, &str)> = vec![("response_format", "json"), ("temperature", "0.0")];
    let lang = get("lang_val");
    if !lang.is_empty() {
        fields.push(("language", lang));
    }
    if !get("task_opt").is_empty() {
        fields.push(("translate", "true"));
    }
    let prompt = get("hint_val");
    if !prompt.is_empty() {
        fields.push(("prompt", prompt));
    }
    let timeout = Duration::from_secs(cfg.request_timeout_secs.max(1));
    match ask(port, &bytes, &fields, timeout, stop) {
        Ok(Some(text)) => {
            shared().life.heard(clock());
            Outcome::Text(text)
        }
        Ok(None) => Outcome::Canceled,
        Err(why) => {
            // Stopped by someone else mid-request (relief, idle) is not a
            // fault of the server; anything else means it is not trusted.
            let gone = shared().running.as_ref().is_none_or(|r| r.port != port);
            if !gone {
                let taken = {
                    let mut s = shared();
                    s.life.request_failed(clock());
                    s.running.take()
                };
                if let Some(r) = taken {
                    kill(r);
                }
            }
            note(&format!("request failed: {why}"));
            Outcome::Unavailable(format!("resident recogniser request failed: {why}"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_then_ready_then_requests() {
        let mut l = Lifecycle::default();
        assert_eq!(l.plan(0), Plan::Start);
        l.started(100);
        assert_eq!(l.plan(200), Plan::Use);
        l.heard(300);
        assert_eq!(l.state, State::Ready { last_used: 300 });
        assert_eq!(l.plan(400), Plan::Use);
    }

    #[test]
    fn failure_falls_back_with_growing_backoff_then_retries() {
        let mut l = Lifecycle::default();
        l.start_failed(1_000);
        assert_eq!(l.plan(1_001), Plan::Fallback { retry_in_ms: 4_999 });
        assert_eq!(l.plan(6_000), Plan::Start);
        l.start_failed(6_000);
        // Doubled.
        assert_eq!(l.plan(6_001), Plan::Fallback { retry_in_ms: 9_999 });
        assert_eq!(backoff_ms(1), 5_000);
        assert_eq!(backoff_ms(2), 10_000);
        assert_eq!(backoff_ms(3), 20_000);
        assert_eq!(backoff_ms(30), 300_000);
        // A clip it hears clears the count.
        l.started(20_000);
        l.heard(20_100);
        assert_eq!(l.failures, 0);
    }

    #[test]
    fn two_clips_at_once_start_one_server() {
        let mut l = Lifecycle::default();
        // The first caller claims the start; the second must wait, not start.
        assert_eq!(l.claim(0), Plan::Start);
        assert_eq!(l.state, State::Starting);
        assert_eq!(l.claim(1), Plan::Wait);
        assert_eq!(l.claim(2), Plan::Wait);
        // Once it is up the waiter uses it.
        l.started(10);
        assert_eq!(l.claim(11), Plan::Use);
        // A failed start sends the waiter to the fallback, not to a start of
        // its own.
        let mut l = Lifecycle::default();
        assert_eq!(l.claim(0), Plan::Start);
        l.start_failed(5);
        assert!(matches!(l.claim(6), Plan::Fallback { .. }));
        // A claimer that was stopped before starting hands the start back.
        let mut l = Lifecycle::default();
        assert_eq!(l.claim(0), Plan::Start);
        l.start_abandoned();
        assert_eq!(l.claim(1), Plan::Start);
        // Starting is neither idle-expired nor stoppable by relief.
        let mut l = Lifecycle::default();
        l.claim(0);
        assert!(!l.idle_expired(u64::MAX, 1));
        assert!(!l.relief(true, true));
    }

    /// A one-reply HTTP listener on a free loopback port.
    fn listener_replying(reply: &'static [u8]) -> u16 {
        use std::io::{Read, Write};
        let l = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut c, _)) = l.accept() {
                let mut buf = [0u8; 1024];
                let _ = c.read(&mut buf);
                let _ = c.write_all(reply);
            }
        });
        port
    }

    #[test]
    fn only_whisper_servers_own_page_counts_as_ready() {
        assert!(looks_like_whisper_server(
            b"HTTP/1.1 200 OK\r\nContent-Type: text/html\r\n\r\n<html><title>Whisper.cpp Server</title></html>"
        ));
        assert!(!looks_like_whisper_server(
            b"HTTP/1.1 200 OK\r\n\r\n<html>Welcome to my dev server</html>"
        ));
        assert!(!looks_like_whisper_server(
            b"HTTP/1.1 404 Not Found\r\n\r\nWhisper.cpp Server"
        ));
        assert!(!looks_like_whisper_server(b"garbage"));
        // Over a real socket: a foreign listener is told apart and sent no audio.
        let foreign =
            listener_replying(b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\nsome other app");
        assert_eq!(probe(foreign), Probe::Foreign);
        let whisper = listener_replying(
            b"HTTP/1.1 200 OK\r\nConnection: close\r\n\r\n<title>Whisper.cpp Server</title>",
        );
        assert_eq!(probe(whisper), Probe::Whisper);
        // Nothing listening: not yet.
        let free = free_loopback_port().unwrap();
        assert_eq!(probe(free), Probe::NotYet);
    }

    #[test]
    fn a_failed_request_rests_the_server() {
        let mut l = Lifecycle::default();
        l.started(0);
        l.request_failed(50);
        assert!(matches!(l.plan(51), Plan::Fallback { .. }));
    }

    #[test]
    fn idle_stops_it_and_use_keeps_it() {
        let mut l = Lifecycle::default();
        l.started(0);
        let half_hour = 30 * 60_000;
        assert!(!l.idle_expired(half_hour - 1, half_hour));
        l.heard(half_hour - 1);
        assert!(!l.idle_expired(half_hour, half_hour));
        assert!(l.idle_expired(2 * half_hour - 1, half_hour));
        l.stopped();
        assert_eq!(l.plan(2 * half_hour), Plan::Start);
        // Nothing running is never "expired".
        assert!(!l.idle_expired(u64::MAX, half_hour));
    }

    #[test]
    fn relief_keeps_it_until_the_last_step_and_only_when_critical() {
        let mut l = Lifecycle::default();
        l.started(0);
        assert!(!l.relief(false, true));
        assert!(!l.relief(true, false));
        assert_eq!(l.plan(1), Plan::Use);
        assert!(l.relief(true, true));
        assert_eq!(l.plan(2), Plan::Start);
        // Nothing running: nothing to stop.
        assert!(!l.relief(true, true));
    }

    #[test]
    fn server_is_private_and_threads_are_sane() {
        let a = whisper_server_args("m.bin", 5000, 2, 0);
        let at = a.iter().position(|x| x == "--host").unwrap();
        assert_eq!(a[at + 1], "127.0.0.1");
        assert!(!a.iter().any(|x| x == "0.0.0.0"));
        assert!(!a.iter().any(|x| x == "-ac"));
        assert!(whisper_server_args("m.bin", 5000, 2, 750)
            .windows(2)
            .any(|w| w == ["-ac", "750"]));
        assert!((2..=4).contains(&server_thread_count(&Config::default())));
        assert_eq!(
            server_thread_count(&Config {
                threads: 7,
                ..Config::default()
            }),
            7
        );
    }

    #[test]
    fn multipart_carries_fields_and_the_clip() {
        let b = multipart("XX", b"RIFFdata", &[("language", "en")]);
        let s = String::from_utf8_lossy(&b);
        assert!(s.contains("name=\"language\"\r\n\r\nen\r\n"));
        assert!(s.contains("name=\"file\"; filename=\"clip.wav\""));
        assert!(s.contains("RIFFdata"));
        assert!(s.ends_with("--XX--\r\n"));
    }

    #[test]
    fn reply_splits_into_status_and_body() {
        let (st, body) = split_reply(b"HTTP/1.1 200 OK\r\nA: b\r\n\r\n{\"text\":\"hi\"}").unwrap();
        assert_eq!(st, 200);
        assert_eq!(body, b"{\"text\":\"hi\"}");
        assert!(split_reply(b"garbage").is_none());
    }

    #[test]
    fn missing_server_is_unavailable_not_a_crash() {
        let mut v = crate::tools::Vars::new();
        v.insert("whisper".into(), "Z:/nowhere/whisper-cli.exe".into());
        v.insert("stt_model".into(), "m.bin".into());
        let out = transcribe(&Config::default(), &v, Path::new("x.wav"), &|| false);
        assert!(matches!(out, Outcome::Unavailable(_)), "{out:?}");
    }

    /// The real server, one clip. Run with ATLAS_LIVE_ASR=<wav> and the
    /// install's whisper-cli path in ATLAS_LIVE_WHISPER and model in
    /// ATLAS_LIVE_MODEL; `cargo test asr_server -- --ignored`.
    #[test]
    #[ignore]
    fn live_server_hears_one_clip() {
        let (Ok(wav), Ok(cli), Ok(model)) = (
            std::env::var("ATLAS_LIVE_ASR"),
            std::env::var("ATLAS_LIVE_WHISPER"),
            std::env::var("ATLAS_LIVE_MODEL"),
        ) else {
            eprintln!("skipped: ATLAS_LIVE_ASR / ATLAS_LIVE_WHISPER / ATLAS_LIVE_MODEL not set");
            return;
        };
        let mut v = crate::tools::Vars::new();
        v.insert("whisper".into(), cli);
        v.insert("stt_model".into(), model);
        let cfg = Config::default();
        let first = transcribe(&cfg, &v, Path::new(&wav), &|| false);
        let second = transcribe(&cfg, &v, Path::new(&wav), &|| false);
        assert!(resident());
        assert!(stop_server_now("test over"));
        assert!(!resident());
        match (first, second) {
            (Outcome::Text(a), Outcome::Text(b)) => {
                assert!(!a.trim().is_empty(), "no words heard");
                assert_eq!(a.trim(), b.trim());
            }
            other => panic!("{other:?}"),
        }
    }
}
