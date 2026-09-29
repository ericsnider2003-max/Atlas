//! The phone's own language model (OPEN_GAPS P.7), with the engine compiled in
//! (`--features phone-llm`), run on this machine's CPU.
//!
//! What a phone build does, minus the phone: llama.cpp inside Atlas, loaded
//! from the models folder, answering through the same `brain::Llm` the daemon
//! talks to. The model file is given by `ATLAS_PHONE_TEST_MODEL` (any GGUF;
//! `stories15M-q4_0.gguf`, 19 MB, is enough to prove the path); without it the
//! tests that need one say so and stop.
//!
//! One process for all of it: the phone holds one model, process-wide.

use atlas::brain::Llm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::phonemodel::{attach, attached, PhoneLlm};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn model() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("ATLAS_PHONE_TEST_MODEL").ok()?);
    p.is_file().then_some(p)
}

#[test]
fn the_phone_thinks_with_its_own_model_or_says_how_to_get_one() {
    // Before any model: a plain answer saying what to do, not a hang.
    let said = PhoneLlm.complete("You are Atlas.", "hello").unwrap_err().to_string();
    assert!(said.contains("get your own model"), "{said}");

    let Some(path) = model() else {
        eprintln!("ATLAS_PHONE_TEST_MODEL isn't set: the engine itself wasn't run");
        return;
    };
    // In the models folder, as the phone keeps it.
    let dir = std::env::temp_dir().join(format!("atlas-phone-think-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let here = dir.join(path.file_name().unwrap());
    std::fs::copy(&path, &here).unwrap();
    let started = std::time::Instant::now();
    attach(&here).unwrap();
    println!("LIVE loaded {} in {:.2}s", here.display(), started.elapsed().as_secs_f64());
    assert!(attached().is_some());

    let t = std::time::Instant::now();
    let out = PhoneLlm.complete("", "Once upon a time, there was a little cat").unwrap();
    println!("LIVE [{:.2}s] {out}", t.elapsed().as_secs_f64());
    assert!(out.split_whitespace().count() >= 5, "the model said almost nothing: {out:?}");

    // Through the daemon, the way the phone core wires it (`mobile::phone_llm`).
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 390, height: 844, primary: true }]);
    let llm: std::sync::Arc<dyn Llm> = std::sync::Arc::new(PhoneLlm);
    let mut d = Daemon::new(&c, &p, Some(llm), Store::new(dir.join("state")), Proactive::new(ProactiveConfig::default()));
    let t = std::time::Instant::now();
    let reply = d.turn("tell me a story about a cat", 1_700_000_000);
    println!("LIVE [daemon {:.1}s] {reply}", t.elapsed().as_secs_f64());
    let t = std::time::Instant::now();
    let other = d.turn("what's a good name for a grey cat?", 1_700_000_100);
    println!("LIVE [daemon {:.1}s] {other}", t.elapsed().as_secs_f64());
    assert!(!reply.trim().is_empty() && !reply.contains("get your own model"), "{reply}");
}
