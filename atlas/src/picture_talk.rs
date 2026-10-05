//! Asking about a picture: a chart, a screenshot, a photo.
//!
//! `vision` finds faces and names things from a fixed list, and `words`
//! reads the text on a screen. Neither can say what a chart *shows* — that
//! sales rose from 120 to 190 over six months and dipped in March — and
//! `vision`'s own header said why: "a model that talks about pictures is
//! several gigabytes and a different decision." Eric, 24 Sep 2026, item 5:
//! real local vision, charts included, with the laptop's memory checked
//! first.
//!
//! The laptop has 15.7 GB (Core Ultra 7 256V, Arc 140V sharing it). The model
//! chosen is Qwen3-VL 4B Instruct at 4-bit, 2.5 GB, plus its 0.45 GB picture
//! encoder — about 3 GB while it runs and nothing when it doesn't, because it
//! is run once per question by llama.cpp's own `llama-mtmd-cli` rather than
//! kept loaded. Both files are Qwen's own, pinned by SHA-256 in `atlas get`.
//! Nothing leaves the machine: the picture is a file in Atlas's scratch
//! folder and the program is on this laptop.
//!
//! Before this, "look at my screen" reached the running Atlas and was
//! answered "Capture runs through the voice layer." — a sentence about the
//! code, spoken to the person, and nothing looked at the screen at all.

use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The picture reader's settings.
#[derive(Debug, Clone, Deserialize, PartialEq)]
#[serde(default)]
pub struct PictureTalkConfig {
    pub enabled: bool,
    /// llama.cpp's picture-and-words program, relative to Atlas's folder.
    pub program: String,
    /// The language model, relative to Atlas's folder.
    pub model: String,
    /// The picture encoder that goes with it.
    pub projector: String,
    /// The longest answer, in tokens. A spoken answer about a chart is a few
    /// sentences; a runaway one is a minute of speech nobody asked for.
    pub most_words: u32,
}

impl Default for PictureTalkConfig {
    fn default() -> Self {
        PictureTalkConfig {
            enabled: true,
            program: if cfg!(windows) { "tools/llama/llama-mtmd-cli.exe" } else { "tools/llama/llama-mtmd-cli" }.into(),
            model: "models/Qwen3VL-4B-Instruct-Q4_K_M.gguf".into(),
            projector: "models/mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf".into(),
            most_words: 300,
        }
    }
}

/// Roughly what it takes while running, for the memory budget: the two
/// files plus room for the picture and the answer.
pub const MEMORY_MB: u64 = 3_600;

/// Where the three files are, resolved against Atlas's folder.
pub fn where_they_are(cfg: &PictureTalkConfig, root: &Path) -> [PathBuf; 3] {
    let at = |p: &str| {
        let p = PathBuf::from(p);
        if p.is_absolute() { p } else { root.join(p) }
    };
    [at(&cfg.program), at(&cfg.model), at(&cfg.projector)]
}

/// Can it run? `Err` names what's missing, in words.
pub fn ready(cfg: &PictureTalkConfig, root: &Path) -> Result<(), String> {
    if !cfg.enabled {
        return Err("reading pictures is switched off in settings".into());
    }
    let [program, model, projector] = where_they_are(cfg, root);
    let missing: Vec<&str> = [
        (program.is_file(), "the picture reader"),
        (model.is_file(), "its model"),
        (projector.is_file(), "its picture encoder"),
    ]
    .iter()
    .filter(|(there, _)| !there)
    .map(|(_, what)| *what)
    .collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "I don't have {} yet — they're a one-off download of about 3 GB, and starting Atlas again fetches them",
            missing.join(" or ")
        ))
    }
}

/// What to ask the model, from what was said.
///
/// "Look at my screen" on its own is a request to look, not a question; the
/// model is asked for what a person glancing over would say, with a chart
/// read properly if there is one. A real question is passed on as asked.
pub fn question_for(said: &str) -> String {
    let s = said.trim().trim_end_matches(['.', '?', '!']).to_lowercase();
    let only_looking = ["view my display", "look at my screen", "view display", "what's on my screen", "whats on my screen", "what is on my screen", ""];
    if only_looking.contains(&s.as_str()) {
        "Say briefly what is on this screen, in two or three plain sentences for someone who can't see it. \
         If there is a chart or graph, say what it measures, the main trend, and the highest and lowest \
         values you can read. Don't guess at numbers you can't read."
            .into()
    } else {
        format!(
            "{}\n\nAnswer in two or three plain sentences for someone who can't see the picture. \
             Don't guess at numbers or words you can't read.",
            said.trim()
        )
    }
}

/// The command line for one question about one picture.
pub fn command_line(cfg: &PictureTalkConfig, root: &Path, image: &Path, question: &str) -> Vec<String> {
    let [_, model, projector] = where_they_are(cfg, root);
    vec![
        "-m".into(),
        model.display().to_string(),
        "--mmproj".into(),
        projector.display().to_string(),
        "--image".into(),
        image.display().to_string(),
        "-p".into(),
        question.to_string(),
        "-n".into(),
        cfg.most_words.to_string(),
        // The model's own default context is 262,144 tokens, and llama.cpp
        // sizes its memory for that up front: over 3 GB extra before the
        // picture is even read (found 24 Sep 2026 — the first run was killed
        // for running out of memory). A screenshot plus a question plus a
        // few sentences back fits comfortably in 4,096.
        "-c".into(),
        "4096".into(),
        // Low but not zero: a description should be the same each time you
        // ask, without the flat repetition greedy decoding falls into.
        "--temp".into(),
        "0.2".into(),
        "--no-warmup".into(),
    ]
}

/// The answer, from what the program printed.
///
/// llama.cpp prints its progress to the error stream and the answer to the
/// ordinary one, but a build can still leave the odd log line or an
/// end-of-text marker in the answer. Those are taken off; anything else is
/// the model's own words.
pub fn the_answer_in(stdout: &str) -> String {
    let kept: Vec<&str> = stdout
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            !(t.starts_with("llama_") || t.starts_with("main:") || t.starts_with("clip_") || t.starts_with("mtmd_")
                || t.starts_with("encoding image") || t.starts_with("image ") && t.contains("encoded")
                || t.starts_with("decoding image") || t.starts_with("load_") || t.starts_with("ggml_"))
        })
        .collect();
    let joined = kept.join("\n");
    let mut out = joined.trim().to_string();
    for marker in ["<|im_end|>", "<|endoftext|>", "</s>"] {
        out = out.replace(marker, "");
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Ask one question about one picture. Blocks while the model runs — tens of
/// seconds on a laptop processor.
/// Ask the picture reader about `image`, stopping it the moment `stop`
/// says so — "stop" said to Atlas while it looks.
pub fn ask_until(
    cfg: &PictureTalkConfig,
    root: &Path,
    image: &Path,
    question: &str,
    stop: &dyn Fn() -> bool,
) -> Result<String, String> {
    use std::io::Read;
    ready(cfg, root)?;
    let [program, ..] = where_they_are(cfg, root);
    let mut child = crate::tools::command(&program)
        .args(command_line(cfg, root, image, question))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| format!("the picture reader wouldn't start: {e}"))?;
    // Read both pipes as they fill, so a chatty model can't stall on a full
    // pipe while this waits for it to finish.
    // Each pipe is read on its own thread and handed back over a channel,
    // so a stopped reader is never waited on: anything it started keeps the
    // pipes open after it's killed, and a plain join would wait that out.
    let drain = |p: Option<Box<dyn Read + Send>>| {
        let (tx, rx) = std::sync::mpsc::channel::<String>();
        std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(mut p) = p {
                crate::heard!(p.read_to_string(&mut s));
            }
            let _ = tx.send(s);
        });
        rx
    };
    let out = drain(child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let err = drain(child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>));
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(ASK_TIMEOUT_SECS);
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if std::time::Instant::now() < deadline && !stop() => {
                std::thread::sleep(std::time::Duration::from_millis(200))
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
        }
    };
    // Finished by itself: its output is all there. Stopped: a moment for
    // what's already written, and no more.
    let wait = if status.is_some() { std::time::Duration::from_secs(30) } else { std::time::Duration::from_millis(500) };
    let stdout = out.recv_timeout(wait).unwrap_or_default();
    let stderr = err.recv_timeout(wait).unwrap_or_default();
    let Some(status) = status else {
        if stop() {
            return Err("you asked me to stop".into());
        }
        return Err(format!("the picture reader took longer than {} minutes, so I stopped it", ASK_TIMEOUT_SECS / 60));
    };
    let answer = the_answer_in(&stdout);
    if !status.success() || answer.is_empty() {
        let last = stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it gave no answer");
        return Err(format!("the picture reader stopped without an answer ({})", last.trim()));
    }
    Ok(answer)
}

// ---------------------------------------------------------------------------
// Asking the model that is already running (Eric, 1 Oct 2026, item 79).
//
// The talking model is Qwen3-VL 4B -- the same model this file starts a
// second copy of for every picture. When its server was started with the
// picture encoder (`models::launch`, `--mmproj`), the picture goes to that
// server instead: nothing to load, about 0.45 GB more instead of 3.6 GB, and
// "the picture reader needs about 3.5 GB and only 2.7 GB is free" stops
// being the answer to "look at me".
// ---------------------------------------------------------------------------

/// The request body for one picture and one question, in the OpenAI shape
/// llama-server's `/v1/chat/completions` takes images in (`image_url` with a
/// data address). Slot 1: a look is work beside the conversation, and must
/// not throw away the conversation's cached prompt in slot 0.
pub fn server_body(question: &str, png: &[u8], most_words: u32) -> String {
    let url = format!("data:image/png;base64,{}", crate::b64::encode(png));
    serde_json::json!({
        "messages": [{
            "role": "user",
            "content": [
                {"type": "image_url", "image_url": {"url": url}},
                {"type": "text", "text": question},
            ],
        }],
        "max_tokens": most_words,
        "temperature": 0.2,
        "stream": false,
        "id_slot": 1,
    })
    .to_string()
}

/// The answer in a `/v1/chat/completions` reply; `Err` says what came back
/// instead, in words.
pub fn server_answer(raw: &str) -> Result<String, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw.trim()).map_err(|_| format!("the model's reply wasn't readable ({})", raw.chars().take(120).collect::<String>()))?;
    if let Some(e) = v.get("error") {
        let msg = e.get("message").and_then(|m| m.as_str()).unwrap_or("an error");
        return Err(format!("the model couldn't read the picture: {msg}"));
    }
    let text = v
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    let text = the_answer_in(&text);
    if text.is_empty() {
        return Err("the model gave no answer about the picture".into());
    }
    Ok(text)
}

/// Ask the running talking model about `image`. `Err` when it isn't up, has
/// no eyes, or didn't answer -- the caller then falls back to `ask_until`
/// if there's room for it.
pub fn ask_server(url: &str, image: &Path, question: &str, most_words: u32) -> Result<String, String> {
    let png = std::fs::read(image).map_err(|e| format!("I couldn't read the picture back ({e})"))?;
    ask_server_png(url, &png, question, most_words)
}

/// `ask_server` for a picture held in memory, never written to disk -- what
/// watching uses (`camwatch`).
pub fn ask_server_png(url: &str, png: &[u8], question: &str, most_words: u32) -> Result<String, String> {
    let body = server_body(question, png, most_words);
    let mut tool = crate::models::server_post();
    tool.timeout_secs = 120;
    let mut vars = crate::tools::Vars::new();
    vars.insert("url".into(), url.to_string());
    let raw = tool.run(&vars, Some(&body)).map_err(|e| format!("the model didn't answer ({e})"))?;
    server_answer(&raw)
}

/// How long one question may take. A 4B model on a laptop answers in well
/// under a minute; five minutes means something is stuck.
pub const ASK_TIMEOUT_SECS: u64 = 300;

/// The widest picture the reader is given. A 4K screenshot is four times
/// the work for no better answer; the model reads it at this size anyway.
pub const MAX_WIDTH: u32 = 1600;

/// A copy of `image` no wider than `MAX_WIDTH`, made with ffmpeg, beside it.
/// `None` when ffmpeg isn't there or fails: the original is used instead.
pub fn smaller(image: &Path) -> Option<std::path::PathBuf> {
    let small = image.with_file_name(format!(
        "{}_small.png",
        image.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()
    ));
    let ok = crate::tools::command("ffmpeg")
        .args(["-y", "-loglevel", "error", "-i"])
        .arg(image)
        .args(["-vf", &format!("scale='min({MAX_WIDTH},iw)':-2")])
        .arg(&small)
        .stdin(std::process::Stdio::null())
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false);
    if ok && small.exists() {
        Some(small)
    } else {
        crate::heard!(std::fs::remove_file(&small));
        None
    }
}

#[cfg(test)]
mod server_tests {
    use super::*;

    #[test]
    fn the_picture_goes_to_the_running_model_as_an_image_url() {
        let body = server_body("Can you see me?", &[137, 80, 78, 71], 300);
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        let parts = &v["messages"][0]["content"];
        assert_eq!(parts[0]["type"], "image_url");
        assert!(parts[0]["image_url"]["url"].as_str().unwrap().starts_with("data:image/png;base64,iVBORw"));
        assert_eq!(parts[1]["text"], "Can you see me?");
        assert_eq!(v["id_slot"], 1);
    }

    #[test]
    fn the_answer_or_the_reason_comes_back() {
        let ok = r#"{"choices":[{"message":{"role":"assistant","content":"You're at your desk.<|im_end|>"}}]}"#;
        assert_eq!(server_answer(ok).unwrap(), "You're at your desk.");
        let no_eyes = r#"{"error":{"code":500,"message":"image input is not supported - hint: if this is unexpected, you may need to provide the mmproj"}}"#;
        assert!(server_answer(no_eyes).unwrap_err().contains("not supported"));
        assert!(server_answer("").is_err());
    }
}
