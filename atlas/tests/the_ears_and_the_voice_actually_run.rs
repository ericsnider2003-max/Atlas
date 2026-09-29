//! The shipped speech templates, run against the real programs.
//!
//! Everything else in this tree tests the speech path with the tools mocked
//! out, because the binaries are a download the build machine may not have.
//! That leaves the one thing nothing can vouch for: that `tools.yaml`'s own
//! `stt:` and `tts:` command templates, exactly as shipped, drive a real
//! whisper-cli and a real piper to a real transcript. A template can hold a
//! typo'd flag for months and every mocked test stays green.
//!
//! ## Why `#[ignore]` and not a silent skip
//!
//! This tree already caught one test that skipped when its subject was
//! absent — which is the normal case after a handover, so it read green
//! while running approximately never. These tests are therefore ignored by
//! default and HARD-FAIL when asked to run without the tools: green means
//! they ran, and there is no third state.
//!
//! ## Running them
//!
//! ```text
//! ATLAS_STT=/path/to/whisper-cli \
//! ATLAS_STT_MODEL=/path/to/ggml-base.en.bin \
//! ATLAS_TTS=/path/to/piper \
//! ATLAS_TTS_VOICE=/path/to/en_US-amy-medium.onnx \
//! cargo test --test the_ears_and_the_voice_actually_run -- --ignored
//! ```
//!
//! ffmpeg must be on PATH (it is the shipped recorder, and the resampler
//! between the two: piper speaks at its voice's rate, whisper's models are
//! trained at `RECORD_RATE_HZ`).
//!
//! First run on real binaries (20 Sep 2026, Linux container, whisper.cpp
//! master + piper 2023.11.14-2 + amy-medium): the round trip came back
//! word-perfect through the shipped templates, unmodified.

use atlas::config::Config;
use atlas::tools::Vars;
use std::path::Path;

fn need(var: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| {
        panic!(
            "{var} is not set. These tests exist to run the REAL tools; \
             without them they fail rather than reading green. See the file \
             header for the invocation."
        )
    })
}

fn tools() -> atlas::voice::ToolsConfig {
    Config::load(Path::new("config"))
        .expect("config/tools.yaml loads")
        .tools
        .expect("tools section present")
}

/// The vars the daemon would supply, pointed at real binaries and a scratch
/// directory of our own.
fn vars(dir: &Path) -> Vars {
    let mut v = Vars::new();
    let stem = dir.join("turn");
    v.insert("whisper".into(), need("ATLAS_STT"));
    v.insert("piper".into(), need("ATLAS_TTS"));
    v.insert("stt_model".into(), need("ATLAS_STT_MODEL"));
    v.insert("voice_file".into(), need("ATLAS_TTS_VOICE"));
    v.insert("in_wav".into(), format!("{}.wav", stem.display()));
    v.insert("out_wav".into(), format!("{}_out.wav", stem.display()));
    v.insert("stem".into(), stem.display().to_string());
    v.insert("transcript".into(), format!("{}.txt", stem.display()));
    // English, no translation — the daemon's own defaults: the optional
    // flag pairs expand to nothing and must be DROPPED, not passed as `-l ""`,
    // which is itself part of what this test proves.
    v.insert("task_opt".into(), String::new());
    v.insert("lang_opt".into(), String::new());
    v.insert("lang_val".into(), String::new());
    // The three voice-settings sliders, at piper's neutral values.
    v.insert("speed".into(), "1.00".into());
    v.insert("variation".into(), "0.667".into());
    v.insert("sentence_gap".into(), "0.20".into());
    v
}

fn scratch(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-hearing-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

const SAID: &str = "The quick brown fox jumps over the lazy dog.";

#[test]
#[ignore = "needs whisper-cli, piper, a model and a voice on this machine -- see the file header"]
fn the_shipped_templates_speak_and_hear_a_whole_sentence() {
    let dir = scratch("roundtrip");
    let t = tools();
    let v = vars(&dir);

    // 1. Speak, through the shipped tts template (piper reads stdin).
    t.tts.run(&v, Some(SAID)).expect("piper spoke");
    let spoken = v.get("out_wav").unwrap();
    let bytes = std::fs::metadata(spoken).expect("the wav exists").len();
    assert!(bytes > 20_000, "a whole sentence is more than {bytes} bytes of audio");

    // 2. Resample to what the listening model was trained at. On a live
    //    install ffmpeg records at this rate directly; here it converts
    //    piper's output, standing in for the microphone.
    let rate = atlas::voice::RECORD_RATE_HZ.to_string();
    let ok = std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i", spoken, "-ar", &rate, "-ac", "1"])
        .arg(v.get("in_wav").unwrap())
        .status()
        .expect("ffmpeg is on PATH")
        .success();
    assert!(ok, "ffmpeg refused the resample");

    // 3. Hear it back, through the shipped stt template — result_file and
    //    all, exactly the call `Voice::listen` makes.
    let heard = t.stt.run(&v, None).expect("whisper transcribed");
    let clean = |s: &str| {
        s.to_lowercase().chars().filter(|c| c.is_alphanumeric() || *c == ' ').collect::<String>()
    };
    assert!(
        clean(&heard).contains(&clean(SAID)),
        "what came back is not what was said.\n  said:  {SAID}\n  heard: {}",
        heard.trim()
    );
}

#[test]
#[ignore = "needs whisper-cli and a model on this machine -- see the file header"]
fn an_empty_optional_flag_is_dropped_not_handed_to_whisper() {
    // `-l ""` is a real whisper failure the templates are shaped around:
    // `resolved` drops arguments that expand to nothing. Mocked tests prove
    // the dropping; this proves whisper accepts what remains.
    let dir = scratch("flags");
    let t = tools();
    let v = vars(&dir);

    // A short real clip to transcribe: one word of silence-padded speech.
    t.tts.run(&v, Some("Yes.")).expect("piper spoke");
    let rate = atlas::voice::RECORD_RATE_HZ.to_string();
    assert!(std::process::Command::new("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i", v.get("out_wav").unwrap(), "-ar", &rate, "-ac", "1"])
        .arg(v.get("in_wav").unwrap())
        .status()
        .unwrap()
        .success());

    let (_, args) = t.stt.resolved(&v);
    assert!(
        args.iter().all(|a| !a.is_empty()),
        "an empty argument survived resolution: {args:?}"
    );
    t.stt.run(&v, None).expect("whisper accepted the resolved arguments");
}
