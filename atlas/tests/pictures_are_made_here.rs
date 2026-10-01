//! **Pictures made on this machine (30 Sep 2026).**
//!
//! "Draw me a lighthouse at dusk" goes to the picture maker (`imagemake`):
//! Z-Image Turbo through stable-diffusion.cpp, nothing uploaded. Without it
//! installed Atlas says what's missing and how to get it. With
//! `ATLAS_PICTURE_KIT` naming a folder holding `sd-cli` (Linux build),
//! `z_image_turbo-Q3_K.gguf`, `Qwen3-4B-Instruct-2507-Q4_0.gguf` and
//! `ae.safetensors`, a real picture is made on the processor alone, small, to
//! prove the whole path.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let dir = std::env::temp_dir().join(format!("atlas-pictures-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    Daemon::new(c, p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn asked_for_a_picture_without_the_maker_it_says_what_to_get() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = daemon(&c, &p, "none");
    let reply = d.turn("draw me a lighthouse at dusk", 1_790_760_000);
    assert!(reply.contains("picture maker") && reply.contains("6.5 GB"), "{reply}");
    let parser = atlas::intent::Parser::new(&c.commands);
    assert_eq!(atlas::session::kind_of(&parser.parse("draw me a lighthouse at dusk")), "make_picture");
    assert_eq!(atlas::session::kind_of(&parser.parse("make a picture of a red bicycle")), "make_picture");
    // An animation and a 3-D scene still go where they went.
    assert_eq!(atlas::session::kind_of(&parser.parse("draw an animation of a bouncing ball")), "animate");
}

#[test]
fn the_download_is_hash_pinned_and_lands_where_the_maker_looks() {
    let pieces = atlas::getpieces::picture_making();
    assert_eq!(pieces.len(), 4);
    assert!(pieces.iter().all(|p| p.sha256.len() == 64 && p.bytes > 0), "every piece hash-pinned with its size");
    let cfg = atlas::imagemake::PictureMakingConfig::default();
    let lands: Vec<&str> = pieces.iter().map(|p| p.key_path()).collect();
    for want in [cfg.model.as_str(), cfg.encoder.as_str(), cfg.vae.as_str()] {
        assert!(lands.contains(&want), "{want} isn't where a piece lands: {lands:?}");
    }
    let total: u64 = pieces.iter().map(|p| p.bytes).sum();
    assert!((6_000_000_000..7_000_000_000).contains(&total), "{total}");
}

#[test]
fn a_real_picture_on_the_processor() {
    let Some(kit) = std::env::var_os("ATLAS_PICTURE_KIT").map(std::path::PathBuf::from) else { return };
    let root = std::env::temp_dir().join(format!("atlas-picture-root-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let cfg = atlas::imagemake::PictureMakingConfig {
        program: kit.join("sd/sd-cli").display().to_string(),
        model: kit.join("z_image_turbo-Q3_K.gguf").display().to_string(),
        encoder: kit.join("Qwen3-4B-Instruct-2507-Q4_0.gguf").display().to_string(),
        vae: kit.join("ae.safetensors").display().to_string(),
        width: 256,
        height: 256,
        steps: 4,
        offload: false,
        ..Default::default()
    };
    let out = root.join("lighthouse.png");
    let started = std::time::Instant::now();
    let made = atlas::imagemake::make(&cfg, &root, "a lighthouse at dusk, watercolour", &out, 7, &|| false);
    eprintln!("made {:?} in {:?}", made, started.elapsed());
    let path = made.unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(bytes.starts_with(b"\x89PNG"), "not a PNG");
    let _ = std::fs::copy(&path, "/home/claude/imagegen/lighthouse-test.png");
}

#[test]
fn everything_is_one_set_with_no_piece_twice() {
    let (_, all) = atlas::getpieces::set(Some("everything")).expect("`atlas get everything`");
    let mut keys: Vec<&str> = all.iter().map(|p| p.key_path()).collect();
    let n = keys.len();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), n, "a piece listed twice");
    for want in ["models/pictures/z_image_turbo-Q4_0.gguf", "models/all-MiniLM-L6-v2.onnx"] {
        assert!(keys.iter().any(|k| *k == want || k.ends_with(want.rsplit('/').next().unwrap())), "{want} missing: {keys:?}");
    }
    assert!(all.iter().all(|p| p.sha256.len() == 64), "every piece hash-pinned");
}
