//! Hearing with Parakeet: NVIDIA's Parakeet TDT 0.6B v2 speech model, run on
//! this machine by sherpa-onnx's own server and kept loaded.
//!
//! 30 Sep 2026, measured on LibriSpeech speech (20 clips, 188 s) through the
//! pinned files below, two processor cores:
//!
//! | | clean | far, quiet microphone |
//! |---|---|---|
//! | whisper base.en (what Atlas used) | 10.5% words wrong | 58.9% |
//! | Parakeet TDT 0.6B v2, int8 | 2.7% | 17.6% |
//!
//! Same speed (about 3x real time on two cores), a quarter of the mistakes up
//! close and a third of them across the room -- the case Eric kept hitting.
//! Loading the model takes ~3 s, so it isn't run per sentence: sherpa-onnx's
//! offline WebSocket server loads it once and answers each recording in about
//! a tenth of its length (1.2 s for 9.9 s of speech, two cores).
//!
//! Its protocol (sherpa-onnx `offline-websocket-server`): one binary message
//! holding the sample rate (i32, little-endian), the byte count of what
//! follows (i32), then the samples as f32 -- sent in pieces of up to 10,240
//! bytes -- answered with one text message of JSON whose `text` is the words;
//! then "Done" ends the connection.
//!
//! Whisper stays: nothing here is required, and any failure falls back to it
//! for that sentence (`voice::transcribe_heard`).

use crate::error::{AtlasError, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Where it listens, on this machine only.
pub const PORT: u16 = 8094;

/// How long a freshly started server is waited for.
pub const START_WAIT_SECS: u64 = 25;

/// The files it needs, under the install.
#[derive(Debug, Clone, PartialEq)]
pub struct Files {
    pub server: PathBuf,
    pub model: PathBuf,
}

/// The server program's name in the sherpa-onnx build.
pub fn server_name() -> &'static str {
    if cfg!(windows) {
        "sherpa-onnx-offline-websocket-server.exe"
    } else {
        "sherpa-onnx-offline-websocket-server"
    }
}

/// Are the server and the model here (`atlas get hearing`)?
pub fn installed(root: &Path) -> Option<Files> {
    let server = root.join("tools").join("sherpa").join("bin").join(server_name());
    let model = root.join("models").join("parakeet");
    let needed = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];
    (server.is_file() && needed.iter().all(|f| model.join(f).is_file())).then_some(Files { server, model })
}

/// The server's command line.
pub fn launch_args(files: &Files, port: u16, threads: usize) -> Vec<String> {
    let m = |f: &str| files.model.join(f).display().to_string();
    vec![
        format!("--port={port}"),
        format!("--encoder={}", m("encoder.int8.onnx")),
        format!("--decoder={}", m("decoder.int8.onnx")),
        format!("--joiner={}", m("joiner.int8.onnx")),
        format!("--tokens={}", m("tokens.txt")),
        "--model-type=nemo_transducer".into(),
        format!("--num-threads={}", threads.max(1)),
        "--max-batch-size=1".into(),
    ]
}

/// Threads for it: half the processor, at least two, at most four -- the
/// model server and the rest of the machine need the rest.
pub fn threads_for(cores: usize) -> usize {
    (cores / 2).clamp(2, 4)
}

/// One recording as the server wants it.
pub fn request_bytes(samples: &[f32], rate: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(8 + samples.len() * 4);
    out.extend_from_slice(&(rate as i32).to_le_bytes());
    out.extend_from_slice(&((samples.len() * 4) as i32).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// The words in the server's answer.
pub fn text_from(answer: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(answer).ok()?;
    v.get("text").and_then(|t| t.as_str()).map(|t| t.trim().to_string())
}

static SERVER: std::sync::Mutex<Option<std::process::Child>> = std::sync::Mutex::new(None);

fn answering(port: u16) -> bool {
    std::net::TcpStream::connect_timeout(&std::net::SocketAddr::from(([127, 0, 0, 1], port)), Duration::from_millis(300)).is_ok()
}

/// Start the server if it isn't running (once per call at most), and wait
/// for it. `Err` says why it isn't there.
fn ensure_running(files: &Files, port: u16) -> Result<()> {
    if answering(port) {
        return Ok(());
    }
    let mut g = SERVER.lock().map_err(|_| AtlasError::Platform("the hearing server's lock broke".into()))?;
    let alive = g.as_mut().is_some_and(|c| matches!(c.try_wait(), Ok(None)));
    if !alive {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let mut cmd = crate::tools::command(&files.server);
        cmd.args(launch_args(files, port, threads_for(cores)))
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        // The DLLs sit beside the program; run from there.
        if let Some(dir) = files.server.parent() {
            cmd.current_dir(dir);
        }
        let child = cmd.spawn().map_err(|e| AtlasError::Platform(format!("couldn't start the hearing server: {e}")))?;
        *g = Some(child);
    }
    drop(g);
    let until = std::time::Instant::now() + Duration::from_secs(START_WAIT_SECS);
    while std::time::Instant::now() < until {
        if answering(port) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    Err(AtlasError::Platform(format!("the hearing server didn't answer within {START_WAIT_SECS} s")))
}

/// Start it without waiting, so the first thing you say isn't the one that
/// waits for the model to load.
pub fn warm(root: &Path) {
    if let Some(files) = installed(root) {
        crate::heard!(std::thread::Builder::new().name("atlas-hearing-warm".into()).spawn(move || {
            crate::heard!(ensure_running(&files, PORT));
        }));
    }
}

/// Stop the server Atlas started (Atlas closing).
pub fn stop() {
    if let Ok(mut g) = SERVER.lock().or_else(crate::crash::unpoison) {
        if let Some(mut c) = g.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }
}

/// Words for a recording, from the running server.
fn words_from(samples: &[f32], rate: u32, port: u16) -> Result<String> {
    let mut ws = crate::ws::WebSocket::connect(&format!("ws://127.0.0.1:{port}/"), Duration::from_secs(20))?;
    let bytes = request_bytes(samples, rate);
    for piece in bytes.chunks(10_240) {
        ws.send_binary(piece)?;
    }
    let answer = ws.recv_text()?;
    let _ = ws.send_text("Done");
    ws.close();
    text_from(&answer).ok_or_else(|| AtlasError::Platform(format!("the hearing server's answer had no words: {answer}")))
}

/// Words for a WAV file through Parakeet, starting the server if needed.
/// `None` when Parakeet isn't installed; `Some(Err)` when it is and failed.
pub fn transcribe_file(root: &Path, wav: &Path) -> Option<Result<String>> {
    let files = installed(root)?;
    Some((|| {
        ensure_running(&files, PORT)?;
        let bytes = std::fs::read(wav)?;
        let (samples, rate) = crate::diarize::read_wav(&bytes).map_err(AtlasError::Platform)?;
        let f: Vec<f32> = samples.iter().map(|s| *s as f32 / 32768.0).collect();
        words_from(&f, rate, PORT)
    })())
}
