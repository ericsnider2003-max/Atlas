//! Parakeet recognition through an owned, bounded socket-free sherpa process.
//! CPU-only requests load the local model independently. Atlas neither starts
//! nor adopts the historical fixed-port server.
//! Cancellation is distinct from provider failure and never asks Whisper to
//! transcribe the canceled request.

use crate::error::{AtlasError, Result};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The files it needs, under the install.
#[derive(Debug, Clone, PartialEq)]
pub struct Files {
    pub program: PathBuf,
    pub model: PathBuf,
}

/// The socket-free recognizer program's name in the sherpa-onnx build.
pub fn recognizer_name() -> &'static str {
    if cfg!(windows) {
        "sherpa-onnx-offline.exe"
    } else {
        "sherpa-onnx-offline"
    }
}

/// Are the socket-free recognizer and the model here (`atlas get hearing`)?
pub fn installed(root: &Path) -> Option<Files> {
    let program = root.join("tools").join("sherpa").join("bin").join(recognizer_name());
    let model = root.join("models").join("parakeet");
    let needed = ["encoder.int8.onnx", "decoder.int8.onnx", "joiner.int8.onnx", "tokens.txt"];
    (program.is_file() && needed.iter().all(|f| model.join(f).is_file())).then_some(Files { program, model })
}

/// The words in the server's answer.
pub fn text_from(answer: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(answer).ok()?;
    v.get("text").and_then(|t| t.as_str()).map(|t| t.trim().to_string())
}

static GENERATION: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

pub(crate) fn cancellation_epoch() -> usize { GENERATION.load(std::sync::atomic::Ordering::SeqCst) }

/// Compatibility hook: production hearing uses an owned socket-free process.
/// No installed listener is started or adopted here.
pub fn warm(_root: &Path) {}

/// Cancel only Atlas-owned recognition requests; the next request is independent.
pub fn stop() { GENERATION.fetch_add(1, std::sync::atomic::Ordering::SeqCst); }

/// None means unavailable; Ok(None) means canceled and must not invoke another engine.
pub fn transcribe_file_until(root: &Path, wav: &Path, stop: &dyn Fn() -> bool) -> Option<Result<Option<String>>> {
    let Files { program: cli, model } = installed(root)?;
    let generation = GENERATION.load(std::sync::atomic::Ordering::SeqCst);
    let stopped = || stop() || GENERATION.load(std::sync::atomic::Ordering::SeqCst) != generation;
    Some((|| {
        if stopped() { return Ok(None); }
        if std::fs::metadata(wav)?.len() > 8 * 1024 * 1024 { return Err(AtlasError::Platform("hearing recording exceeds 8 MiB".into())); }
        let bytes = std::fs::read(wav)?;
        let (samples, rate) = crate::diarize::read_wav(&bytes).map_err(AtlasError::Platform)?;
        if rate == 0 || samples.is_empty() || samples.len() > rate as usize * 60 { return Err(AtlasError::Platform("hearing needs a nonempty recording of at most 60 seconds".into())); }
        let mut command = crate::tools::command(&cli);
        for (argument, name) in [("--encoder=", "encoder.int8.onnx"), ("--decoder=", "decoder.int8.onnx"), ("--joiner=", "joiner.int8.onnx"), ("--tokens=", "tokens.txt")] {
            let mut arg = std::ffi::OsString::from(argument); arg.push(model.join(name)); command.arg(arg);
        }
        command.args(["--model-type=nemo_transducer", "--num-threads=2", "--provider=cpu", "--lm-provider=cpu"]).arg(wav);
        let run = crate::tools::run_scoped(&mut command, Duration::from_secs(30), 64 * 1024, None, Some(&stopped));
        if stopped() || matches!(run.end, crate::tools::ProcessEnd::Stopped) { return Ok(None); }
        if !matches!(run.end, crate::tools::ProcessEnd::Exited(status) if status.success()) || run.truncated {
            let why = match &run.end {
                crate::tools::ProcessEnd::Exited(status) => format!("recognizer exited with {status}"),
                crate::tools::ProcessEnd::TimedOut => "recognizer exceeded 30 seconds".into(),
                crate::tools::ProcessEnd::Stopped => "recognizer stopped".into(),
                crate::tools::ProcessEnd::Failed(message) => message.clone(),
            };
            return Err(AtlasError::Platform(format!("hearing did not complete: {why}; {}", String::from_utf8_lossy(&run.stderr))));
        }
        let stdout = String::from_utf8_lossy(&run.stdout);
        let text = text_from(&stdout).or_else(|| stdout.lines().find_map(text_from)).or_else(|| {
            let first = stdout.find('{')?; let last = stdout.rfind('}')?; text_from(&stdout[first..=last])
        }).ok_or_else(|| AtlasError::Platform("hearing returned no recognizer text JSON".into()))?;
        Ok(Some(text))
    })())
}

#[cfg(test)]
mod native_roundtrip_proof {
    use super::*;
    #[test]
    #[ignore = "installed socket-free Parakeet CLI and owned synthetic Kokoro WAVs"]
    fn socket_free_synthetic_voice_understands_pause_and_stops_owned_asr() {
        let root = PathBuf::from(std::env::var("ATLAS_ASR_INSTALL").expect("explicit read-only install path required"));
        let narration = PathBuf::from(std::env::var("KOKORO_OUT").expect("owned synthetic preview required"));
        let pause = PathBuf::from(std::env::var("KOKORO_COMMAND_OUT").expect("owned synthetic Pause WAV required"));
        assert!(narration.starts_with(std::env::temp_dir()) && pause.starts_with(std::env::temp_dir()));
        let started = std::time::Instant::now();
        let transcribe = |wave: &Path| {
            let began = std::time::Instant::now();
            let words = transcribe_file_until(&root, wave, &|| started.elapsed() >= Duration::from_secs(90)).expect("installed socket-free adapter").expect("recognition completed").expect("recognition not canceled");
            (words, began.elapsed())
        };
        let (heard, cold) = transcribe(&narration);
        let heard = heard.to_lowercase();
        assert!(heard.contains("atlas") && heard.contains("reply") && heard.contains("sentence"), "synthetic narration: {heard}");
        let (repeat_words, repeat) = transcribe(&narration);
        assert_eq!(repeat_words.to_lowercase(), heard, "repeat recognition remains stable");
        let (command, command_time) = transcribe(&pause);
        let cfg = crate::config::Config::load(Path::new("config")).unwrap();
        assert_eq!(crate::intent::Parser::new(&cfg.commands).parse(&command), crate::intent::Intent::Pause, "synthetic command: {command}");
        let cancel_started = std::time::Instant::now();
        let stopped = transcribe_file_until(&root, &narration, &|| cancel_started.elapsed() >= Duration::from_millis(100)).expect("installed adapter").expect("cancellation is a result");
        assert!(stopped.is_none(), "native ASR must acknowledge interruption");
        assert!(cancel_started.elapsed() < Duration::from_secs(2));
        eprintln!("socket-free native ASR cold={cold:?} repeat={repeat:?} command={command_time:?}; narration={heard}; command={command}; cancel={:?}", cancel_started.elapsed());
    }
}
