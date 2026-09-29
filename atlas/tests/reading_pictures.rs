//! Asking about a picture with the local picture reader (`picture_talk`).
//!
//! The first half needs nothing. The second runs the real model — Qwen3-VL
//! 4B through llama.cpp's `llama-mtmd-cli` — on a chart whose numbers are
//! known, when `ATLAS_PICTURE_KIT` names a folder holding `tools/llama/`
//! and `models/` laid out as `atlas get pictures` leaves them, plus
//! `chart.png` (monthly sales, Jan–Jun: 120, 135, 128, 160, 190, 175).

use atlas::picture_talk::{self, PictureTalkConfig};
use std::path::{Path, PathBuf};

#[test]
fn looking_is_turned_into_a_question_and_a_question_is_kept() {
    let q = picture_talk::question_for("look at my screen");
    assert!(q.contains("chart or graph") && q.contains("Don't guess"), "{q}");
    let asked = picture_talk::question_for("what does this chart show about March?");
    assert!(asked.starts_with("what does this chart show about March?"), "{asked}");
    assert!(asked.contains("Don't guess"));
}

#[test]
fn the_command_names_the_model_the_encoder_and_a_bounded_context() {
    let cfg = PictureTalkConfig::default();
    let args = picture_talk::command_line(&cfg, Path::new("/atlas"), Path::new("/tmp/shot.png"), "what is this?");
    // Separators as this platform writes them, so the check holds on Windows
    // too (found running the suite natively there, 26 Sep 2026).
    let joined = args.join(" ").replace('\\', "/");
    assert!(joined.contains("/atlas/models/Qwen3VL-4B-Instruct-Q4_K_M.gguf"), "{joined}");
    assert!(joined.contains("--mmproj /atlas/models/mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf"), "{joined}");
    assert!(joined.contains("--image /tmp/shot.png"));
    let c = args.iter().position(|a| a == "-c").expect("a context size, or it asks for gigabytes");
    assert!(args[c + 1].parse::<u32>().unwrap() <= 8192);
}

#[test]
fn only_the_answer_is_kept_from_what_the_program_prints() {
    let printed = "\nThis chart shows monthly sales.\nSales peaked in May.<|im_end|>\nllama_perf_context_print: total time\n";
    assert_eq!(picture_talk::the_answer_in(printed), "This chart shows monthly sales. Sales peaked in May.");
}

#[test]
fn what_is_missing_is_named_and_switched_off_says_so() {
    let root = std::env::temp_dir().join(format!("atlas-pictures-none-{}", std::process::id()));
    let why = picture_talk::ready(&PictureTalkConfig::default(), &root).unwrap_err();
    assert!(why.contains("starting Atlas again fetches them") && why.contains("picture reader"), "{why}");
    // And asking anyway refuses with the same sentence rather than starting
    // a program that isn't there.
    let asked = picture_talk::ask_until(&PictureTalkConfig::default(), &root, &root.join("x.png"), "what is this?", &|| false);
    assert_eq!(asked, Err(why));
    let off = PictureTalkConfig { enabled: false, ..PictureTalkConfig::default() };
    assert!(picture_talk::ready(&off, &root).unwrap_err().contains("switched off"));
}

#[test]
fn the_pieces_are_pinned_and_fetched_as_their_own_set() {
    let (what, pieces) = atlas::getpieces::set(Some("pictures")).expect("a set");
    assert!(what.contains("3 GB"));
    assert_eq!(pieces, atlas::getpieces::pictures());
    let cfg = PictureTalkConfig::default();
    let [program, model, projector] = picture_talk::where_they_are(&cfg, Path::new(""));
    let lands: Vec<String> = pieces.iter().map(|p| p.key_path().to_string()).collect();
    for f in [program, model, projector] {
        let f = f.display().to_string().replace(".exe", "");
        assert!(lands.iter().any(|l| l.replace(".exe", "") == f), "{f} isn't what `atlas get pictures` fetches: {lands:?}");
    }
    assert!(pieces.iter().all(|p| p.sha256.len() == 64));
    assert!(atlas::getpieces::set(Some("seeing")).unwrap().1.len() == 8);
    assert!(atlas::getpieces::set(Some("nonsense")).is_none());
}

#[test]
fn a_big_download_is_hashed_without_holding_it_in_memory() {
    let dir = std::env::temp_dir().join(format!("atlas-hash-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    // Sizes either side of every padding edge, and across the 1 MB read size.
    for len in [0usize, 1, 55, 56, 63, 64, 65, 1 << 20, (1 << 20) + 7, 3 * (1 << 20) + 63] {
        let data: Vec<u8> = (0..len).map(|i| (i * 31 % 251) as u8).collect();
        let f = dir.join("x");
        std::fs::write(&f, &data).unwrap();
        assert_eq!(atlas::digest::sha256_file_hex(&f).unwrap(), atlas::digest::sha256_hex(&data), "length {len}");
    }
    assert_eq!(
        atlas::digest::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn kit() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("ATLAS_PICTURE_KIT").ok()?);
    p.join("chart.png").is_file().then_some(p)
}

#[test]
fn a_real_chart_is_read_by_the_real_model() {
    let Some(kit) = kit() else {
        eprintln!("skipped: ATLAS_PICTURE_KIT isn't set");
        return;
    };
    let cfg = PictureTalkConfig::default();
    picture_talk::ready(&cfg, &kit).expect("the kit is laid out as `atlas get pictures` leaves it");
    let answer = picture_talk::ask_until(&cfg, &kit, &kit.join("chart.png"), &picture_talk::question_for("what does this chart show?"), &|| false).unwrap();
    eprintln!("{answer}");
    let a = answer.to_lowercase();
    assert!(a.contains("sales"), "{answer}");
    assert!(a.contains("may"), "the peak month: {answer}");
    assert!(a.contains("190"), "the highest value: {answer}");
    assert!(a.contains("120"), "the lowest value: {answer}");
}
