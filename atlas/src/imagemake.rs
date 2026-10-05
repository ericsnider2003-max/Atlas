//! Making pictures, on this machine (30 Sep 2026).
//!
//! "Draw me a lighthouse at dusk" makes a picture here, with no service and
//! no account: stable-diffusion.cpp (`sd-cli`, MIT, Vulkan on the laptop's
//! Arc graphics) running Z-Image Turbo (Tongyi-MAI, Apache 2.0), a 6B model
//! that needs only eight steps, quantized to about 3.7 GB. Its text encoder
//! is a Qwen3 4B (Apache 2.0) and its decoder the FLUX-family VAE (335 MB).
//! All four are fetched once with `atlas get pictures-made` or the button,
//! checked against their SHA-256 (`getpieces::picture_making`).
//!
//! Why this one, for a 16 GB laptop whose graphics share its memory: it is
//! the smallest model with modern picture quality and a permissive licence,
//! and stable-diffusion.cpp's `--offload-to-cpu` keeps only the part working
//! at each moment on the graphics side. SDXL Turbo is smaller but its licence
//! is non-commercial; FLUX.1 schnell is Apache but twice the size with its
//! T5 encoder. Measured in the sandbox on two CPU cores only (no graphics,
//! 7 GB): a 256x256 picture in four steps took 207 s with the 3-bit model
//! and `--mmap`, and it was a recognisable lighthouse at dusk. The laptop's
//! graphics do the diffusion itself far faster; that is to be measured there.
//!
//! Integrated graphics and stable-diffusion.cpp: its model manager can read
//! the driver's "free memory" as zero on a shared-memory GPU and refuse to
//! load (issue #2022, Intel and AMD integrated graphics). The documented
//! workaround, `GGML_VK_ALLOW_GTT_OVERCOMMIT=1`, is set for every run.
//!
//! The picture goes in your Pictures folder, under Atlas, named by what was
//! asked; nothing leaves the machine.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// About how much memory a picture takes while it's made, for the memory
/// budget (`lifecycle::Helpers::want`): the diffusion model and the text
/// encoder, one at a time on the graphics side, both in memory.
pub const MEMORY_MB: u64 = 6_500;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct PictureMakingConfig {
    pub enabled: bool,
    /// stable-diffusion.cpp's command-line program, relative to Atlas's folder.
    pub program: String,
    /// The picture model.
    pub model: String,
    /// Its text encoder.
    pub encoder: String,
    /// Its decoder.
    pub vae: String,
    pub width: u32,
    pub height: u32,
    pub steps: u32,
    /// Where the pictures go. Empty: Pictures\Atlas in your home folder.
    pub folder: String,
    /// The longest a picture may take before it's given up on.
    pub most_secs: u64,
    /// Keep the weights in main memory and move each part to the graphics
    /// side as it works (`--offload-to-cpu`). For graphics; with no graphics
    /// it only doubles what's held, and was killed for memory in the sandbox.
    pub offload: bool,
}

impl Default for PictureMakingConfig {
    fn default() -> Self {
        PictureMakingConfig {
            enabled: true,
            program: if cfg!(windows) { "tools/sd/sd-cli.exe" } else { "tools/sd/sd-cli" }.into(),
            model: "models/pictures/z_image_turbo-Q4_0.gguf".into(),
            encoder: "models/pictures/Qwen3-4B-Instruct-2507-Q4_K_M.gguf".into(),
            vae: "models/pictures/ae.safetensors".into(),
            width: 1024,
            height: 768,
            steps: 8,
            folder: String::new(),
            most_secs: 900,
            offload: true,
        }
    }
}

fn at(root: &Path, p: &str) -> PathBuf {
    let p = PathBuf::from(p);
    if p.is_absolute() {
        p
    } else {
        root.join(p)
    }
}

/// Can a picture be made here? `Err` says what's missing, in words.
pub fn ready(cfg: &PictureMakingConfig, root: &Path) -> Result<(), String> {
    if !cfg.enabled {
        return Err("making pictures is switched off in settings".into());
    }
    let missing: Vec<&str> = [
        (at(root, &cfg.program).is_file(), "the picture maker"),
        (at(root, &cfg.model).is_file(), "its model"),
        (at(root, &cfg.encoder).is_file(), "its text encoder"),
        (at(root, &cfg.vae).is_file(), "its decoder"),
    ]
    .iter()
    .filter(|(there, _)| !there)
    .map(|(_, what)| *what)
    .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "I don't have {} yet -- a one-off download of about 6.5 GB. Say \u{201c}get the picture maker\u{201d} and I'll fetch it",
            missing.join(" or ")
        ))
    }
}

/// What to draw, from what was said: "draw me a lighthouse at dusk" is "a
/// lighthouse at dusk".
pub fn subject(said: &str) -> String {
    let t = said.trim().trim_end_matches(['.', '!', '?']);
    let l = t.to_lowercase();
    const LEADS: &[&str] = &[
        "can you ", "could you ", "please ", "atlas ", "atlas, ", "i want you to ", "i'd like you to ",
    ];
    let mut s = l.as_str();
    let mut cut = 0;
    loop {
        let before = cut;
        for p in LEADS {
            if s.starts_with(p) {
                cut += p.len();
                s = &l[cut..];
            }
        }
        if cut == before {
            break;
        }
    }
    const ASKS: &[&str] = &[
        "make me a picture of ", "make a picture of ", "make me an image of ", "make an image of ",
        "generate an image of ", "generate a picture of ", "generate me an image of ", "create an image of ",
        "create a picture of ", "draw me a picture of ", "draw a picture of ", "paint me ", "paint a picture of ",
        "draw me ", "draw ", "paint ", "picture of ", "an image of ", "a picture of ",
    ];
    for a in ASKS {
        if s.starts_with(a) {
            return t[cut + a.len()..].trim().to_string();
        }
    }
    t[cut..].trim().to_string()
}

/// A file name from what was asked: "lighthouse-at-dusk-1790..." .png.
pub fn file_name(subject: &str, t: u64) -> String {
    let mut words: Vec<String> = subject
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty() && !matches!(*w, "a" | "an" | "the" | "of" | "me"))
        .take(6)
        .map(str::to_string)
        .collect();
    if words.is_empty() {
        words.push("picture".into());
    }
    format!("{}-{t}.png", words.join("-"))
}

/// Where the pictures go.
pub fn folder(cfg: &PictureMakingConfig) -> PathBuf {
    if !cfg.folder.trim().is_empty() {
        return PathBuf::from(cfg.folder.trim());
    }
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")).map(PathBuf::from).unwrap_or_default();
    home.join("Pictures").join("Atlas")
}

/// The program and its arguments for one picture.
fn sd_args(cfg: &PictureMakingConfig, root: &Path, prompt: &str, out: &Path, seed: u64) -> (PathBuf, Vec<String>) {
    let s = |p: &str| at(root, p).display().to_string();
    let args = vec![
        "--diffusion-model".into(),
        s(&cfg.model),
        "--llm".into(),
        s(&cfg.encoder),
        "--vae".into(),
        s(&cfg.vae),
        "-p".into(),
        prompt.to_string(),
        // A turbo model: guidance off, eight steps.
        "--cfg-scale".into(),
        "1.0".into(),
        "--steps".into(),
        cfg.steps.max(1).to_string(),
        "-W".into(),
        cfg.width.to_string(),
        "-H".into(),
        cfg.height.to_string(),
        "--seed".into(),
        (seed % 2_147_483_647).to_string(),
        // Only the part working now on the graphics side, flash attention,
        // and the decoder in tiles: what lets it fit beside the talking
        // model in 16 GB of shared memory (stable-diffusion.cpp's own guide
        // for small graphics memory).
        // The model files read as they're needed rather than copied in.
        "--mmap".into(),
        "--diffusion-fa".into(),
        "--vae-tiling".into(),
        "-o".into(),
        out.display().to_string(),
    ];
    let mut args = args;
    if cfg.offload {
        args.push("--offload-to-cpu".into());
    }
    (at(root, &cfg.program), args)
}

/// Make one picture: the program run to the end, or stopped when `stop`
/// says so. The picture's path, or why not.
pub fn make(cfg: &PictureMakingConfig, root: &Path, prompt: &str, out: &Path, seed: u64, stop: &dyn Fn() -> bool) -> Result<PathBuf, String> {
    ready(cfg, root)?;
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("I couldn't make the pictures folder: {e}"))?;
    }
    let (program, args) = sd_args(cfg, root, prompt, out, seed);
    let mut child = crate::tools::command(&program)
        .args(&args)
        .env("GGML_VK_ALLOW_GTT_OVERCOMMIT", "1")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("the picture maker wouldn't start: {e}"))?;
    let err = child.stderr.take().map(|mut e| {
        std::thread::spawn(move || {
            use std::io::Read;
            let mut s = String::new();
            crate::heard!(e.read_to_string(&mut s));
            s
        })
    });
    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let said = err.and_then(|h| h.join().ok()).unwrap_or_default();
                if status.success() && out.is_file() {
                    return Ok(out.to_path_buf());
                }
                let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("no reason given").trim().to_string();
                return Err(format!("the picture maker stopped without a picture ({last})"));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("I lost track of the picture maker: {e}")),
        }
        if stop() {
            let _ = child.kill();
            let _ = child.wait();
            return Err("stopped".into());
        }
        if started.elapsed().as_secs() > cfg.most_secs {
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("it took longer than {} minutes, so I stopped it", cfg.most_secs / 60));
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_to_draw_is_taken_from_the_request() {
        assert_eq!(subject("Draw me a lighthouse at dusk."), "a lighthouse at dusk");
        assert_eq!(subject("can you make a picture of my dog wearing a hat"), "my dog wearing a hat");
        assert_eq!(subject("Atlas, generate an image of a red bicycle"), "a red bicycle");
        assert_eq!(subject("a cat on the moon"), "a cat on the moon");
    }

    #[test]
    fn the_file_is_named_by_what_was_asked() {
        assert_eq!(file_name("a lighthouse at dusk", 7), "lighthouse-at-dusk-7.png");
        assert_eq!(file_name("", 7), "picture-7.png");
    }

    #[test]
    fn missing_pieces_are_named() {
        let root = std::env::temp_dir().join("atlas-imagemake-none");
        let why = ready(&PictureMakingConfig::default(), &root).unwrap_err();
        assert!(why.contains("picture maker") && why.contains("6.5 GB"), "{why}");
    }
}
