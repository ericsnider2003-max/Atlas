//! **Speculative decoding, off until asked for** (28 Sep 2026).
//!
//! llama.cpp b10456 -- what setup downloads -- took `--draft-max` and
//! `--draft-min` away (its server README: "the argument has been removed");
//! a draft model is `-md` with `--spec-type draft-simple` and
//! `--spec-draft-n-max/-min`, and a plain GGUF isn't recognised as a draft
//! without the type named (`common_speculative_types_from_gguf` only knows
//! MTP and DFlash heads). No-model speculation is `--spec-type ngram-…`.
//! These pin what Atlas passes, and that it passes nothing unless set.

use atlas::models::{server_args, speculation_args, Model, ModelsConfig, DRAFT_MAX, DRAFT_MIN, NGRAM_KINDS};
use std::path::PathBuf;

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-spec-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn model(path: PathBuf) -> Model {
    Model {
        id: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
        path,
        architecture: "qwen3vl".into(),
        quant: "Q4_K_M".into(),
        parameters: 4_000_000_000,
        weight_bytes: 2_500_000_000,
        max_context: 32768,
        chat_template: None,
    }
}

#[test]
fn nothing_is_passed_by_default() {
    let d = dir("default");
    let cfg = ModelsConfig { dir: d.display().to_string(), ..Default::default() };
    assert_eq!(cfg.draft, "");
    assert_eq!(cfg.speculate, "off");
    let args = server_args(&model(d.join("big.gguf")), &cfg, 0);
    assert!(!args.iter().any(|a| a.starts_with("-md") || a.starts_with("--spec")), "{args:?}");
    // The flags b10456 removed are never passed.
    assert!(!args.iter().any(|a| a == "--draft-max" || a == "--draft-min"), "{args:?}");
}

#[test]
fn a_draft_model_that_is_there_is_passed_as_draft_simple() {
    let d = dir("draft");
    std::fs::write(d.join("Qwen3-0.6B-Q8_0.gguf"), b"GGUF").unwrap();
    let cfg = ModelsConfig { dir: d.display().to_string(), draft: "Qwen3-0.6B-Q8_0.gguf".into(), ..Default::default() };
    let args = server_args(&model(d.join("big.gguf")), &cfg, 0);
    let at = args.iter().position(|a| a == "-md").expect("no -md");
    assert_eq!(PathBuf::from(&args[at + 1]), d.join("Qwen3-0.6B-Q8_0.gguf"));
    let joined = args.join(" ");
    assert!(joined.contains(&format!("--spec-draft-n-max {DRAFT_MAX} --spec-draft-n-min {DRAFT_MIN}")), "{joined}");
    assert!(joined.ends_with("--spec-type draft-simple"), "{joined}");
    // With a no-model kind too: both, comma-separated, as the README shows.
    let both = ModelsConfig { speculate: "ngram-mod".into(), ..cfg.clone() };
    assert!(speculation_args(&both, &d.join("big.gguf")).join(" ").ends_with("--spec-type draft-simple,ngram-mod"));
}

#[test]
fn a_draft_that_isnt_there_or_is_the_model_itself_is_left_out() {
    let d = dir("missing");
    let cfg = ModelsConfig { dir: d.display().to_string(), draft: "nope.gguf".into(), ..Default::default() };
    assert!(speculation_args(&cfg, &d.join("big.gguf")).is_empty());
    // On a machine small enough that the helper is the model that talks.
    std::fs::write(d.join("small.gguf"), b"GGUF").unwrap();
    let cfg = ModelsConfig { draft: "small.gguf".into(), ..cfg };
    assert!(speculation_args(&cfg, &d.join("small.gguf")).is_empty());
}

#[test]
fn only_the_ngram_kinds_the_pinned_build_knows_are_passed() {
    let d = dir("ngram");
    for k in NGRAM_KINDS {
        let cfg = ModelsConfig { dir: d.display().to_string(), speculate: k.to_string(), ..Default::default() };
        assert_eq!(speculation_args(&cfg, &d.join("m.gguf")), ["--spec-type".to_string(), k.to_string()]);
    }
    for bad in ["", "off", "ngram", "draft-eagle3", "ngram-mod; rm -rf"] {
        let cfg = ModelsConfig { dir: d.display().to_string(), speculate: bad.into(), ..Default::default() };
        assert!(speculation_args(&cfg, &d.join("m.gguf")).is_empty(), "{bad}");
    }
}

#[test]
fn the_helper_model_is_pinned_and_its_settings_wait_for_a_restart() {
    let p = atlas::getpieces::draft_model();
    assert!(p.url.starts_with("https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/"));
    assert_eq!(p.sha256, "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031");
    assert_eq!(p.bytes, 639_446_688);
    assert_eq!(p.key_path(), "models/Qwen3-0.6B-Q8_0.gguf");
    assert!(atlas::settings::needs_a_restart("models.draft") && atlas::settings::needs_a_restart("models.speculate"));
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let t = c.tools.unwrap();
    assert_eq!((t.models.draft.as_str(), t.models.speculate.as_str()), ("", "off"), "shipped off");
    let reg = atlas::settings::registry(&t);
    assert!(reg.get("models.draft").is_some() && reg.get("models.speculate").is_some());
}
