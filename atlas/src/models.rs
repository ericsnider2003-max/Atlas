//! Model registry and engine — the rest of what Ollama does, in Rust.
//!
//! Ollama's job is: find the models on disk, read their metadata, work out
//! what fits in memory, format the prompt the way each model expects, and run
//! a llama.cpp process to do the actual inference. None of that requires Go or
//! Ollama. This module does it directly, so Atlas talks to `llama-server`
//! itself and there is one fewer piece of someone else's software in the path.

use crate::error::{AtlasError, Result};
use crate::gguf::Gguf;
use crate::tools::{ExternalTool, Vars};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    pub path: PathBuf,
    /// Filename stem — what you refer to it by.
    pub id: String,
    pub architecture: String,
    pub quant: String,
    pub parameters: u64,
    pub weight_bytes: u64,
    pub max_context: u64,
    pub chat_template: Option<String>,
}

impl Model {
    /// A vision projector rather than a language model.
    pub fn is_a_projector(&self) -> bool {
        self.architecture.eq_ignore_ascii_case("clip") || self.id.to_lowercase().starts_with("mmproj")
    }

    /// This model's picture encoder (`mmproj-<model>-<quant>.gguf`) when
    /// it's in the same folder: what lets the talking model see (item 79).
    /// `Qwen3VL-4B-Instruct-Q4_K_M` pairs with
    /// `mmproj-Qwen3VL-4B-Instruct-Q8_0`: the names match once the
    /// quantisation at the end is taken off both.
    pub fn projector(&self) -> Option<PathBuf> {
        if self.is_a_projector() {
            return None;
        }
        let want = without_quant(&self.id).to_lowercase();
        let dir = self.path.parent()?;
        let mut found: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else { return false };
                p.extension().is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
                    && stem.strip_prefix("mmproj-").is_some_and(|rest| without_quant(rest) == want)
            })
            .collect();
        found.sort();
        found.into_iter().next()
    }

    fn inspect(path: &Path) -> Result<Model> {
        let g = Gguf::open(path)?;
        Ok(Model {
            id: path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default(),
            architecture: g.architecture().unwrap_or("unknown").to_string(),
            quant: g.dominant_quant().name().to_string(),
            parameters: g.total_parameters(),
            weight_bytes: g.weight_bytes(),
            max_context: g.context_length().unwrap_or(2048),
            chat_template: g.chat_template().map(str::to_string),
            path: path.to_path_buf(),
        })
    }

    /// Human-scale parameter count: 7B, 13B, 70B.
    pub fn size_label(&self) -> String {
        let b = self.parameters as f64 / 1e9;
        if b >= 1.0 {
            format!("{b:.0}B")
        } else {
            format!("{:.0}M", self.parameters as f64 / 1e6)
        }
    }

    /// Memory needed at a given context, in bytes.
    pub fn memory_needed(&self, context: u64) -> Result<u64> {
        Ok(Gguf::open(&self.path)?.memory_needed(context))
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ModelsConfig {
    /// Where GGUF files live.
    pub dir: String,
    /// Preferred model id. Empty means pick the best that fits.
    pub prefer: String,
    /// Context to request. 0 means the model's own maximum.
    pub context: u64,
    /// How much RAM Atlas may use for a model, megabytes.
    pub memory_budget_mb: u64,
    /// The llama.cpp server binary, driven directly — no Ollama in the path.
    pub server: Option<ExternalTool>,
    pub port: u16,
    /// Where the model server listens. Empty: this machine only. On your own
    /// server, its tunnel address (`10.77.0.1`), so the laptop and phone can
    /// reach its models over WireGuard. Private addresses only — never
    /// `0.0.0.0`, never a public one (`server::bind_address` decides).
    pub listen_on: String,
    /// How many of the model's layers go on the graphics: `auto` (the
    /// default), `off`, or a number. `auto` measures a real graphics card's
    /// memory; with none measured, it puts every layer on the graphics when
    /// the downloaded server is a graphics build (Vulkan, CUDA, Metal) --
    /// integrated graphics share the memory the model was already sized to
    /// fit -- and none when it is a processor-only build (`layers_here`).
    pub gpu_layers: String,
    /// The largest model, in billions of parameters, chosen for talking
    /// when nobody named one (`prefer`). 0: no ceiling, the largest that
    /// fits. A conversation wants a quick answer more than the biggest
    /// model the memory allows; see `Registry::choose_for`.
    pub talk_ceiling_b: u64,
    /// A small model of the same family to draft for the big one
    /// (speculative decoding): it guesses a few words ahead, the big model
    /// checks them in one pass, and every guess it keeps is time saved.
    /// A file name in `dir`, or a full path. Empty: off. Used only when the
    /// file is there. For the shipped Qwen3-VL 4B, Qwen3-0.6B (Q8_0) has the
    /// identical vocabulary, which llama.cpp requires (`DRAFT_FOR_QWEN3`).
    pub draft: String,
    /// Speculation with no second model: llama.cpp guesses ahead from what
    /// has already been said (`ngram-mod`, `ngram-simple`, `ngram-map-k`,
    /// `ngram-map-k4v`, `ngram-cache`). `off` (or empty): off. Helps most when a reply
    /// repeats what it read -- a summary, code, a quoted list.
    pub speculate: String,
    /// How the model picks its words (`Sampling`): sent with every request,
    /// so the server's own defaults -- no penalty for repeating at all --
    /// never apply.
    pub sampling: Sampling,
    /// The model you talk with (30 Sep 2026, `deepbrain`): `faster` (the
    /// shipped Qwen3-VL 4B -- also the default, empty), `better` (Qwen3.5
    /// 4B: more natural replies, about half as quick to read a prompt,
    /// measured on Eric's laptop), or a model's file name in `dir`. The
    /// hub's "Better answers" and "use the better model" set it.
    pub talk: String,
    /// The model for work nobody is waiting on word by word -- research,
    /// drafts, summaries, the night's work, a request of several steps --
    /// run as a second server beside the talking one, started when such
    /// work comes and stopped when it has been idle (`deepbrain`). Empty:
    /// Qwen3.5 9B (`deepbrain::DEEP_DEFAULT`) when its file is in `dir`;
    /// `off`: none, and that work shares the talking model's other slot as
    /// before; or a model's file name.
    pub deep: String,
    /// The deep model's server port. 0: the talking model's port plus one.
    pub deep_port: u16,
    /// How long the deep model stays loaded with nothing to do, in seconds.
    pub deep_idle_secs: u64,
    /// The deep model's context, in tokens.
    pub deep_context: u64,
    /// Offline first, online second (Eric, 30 Sep 2026): when the model on
    /// this machine can't answer, or there isn't one, a free model online
    /// that needs no account (`freeonline`). `false`: nothing is sent.
    pub online_second: bool,
    /// The talking model sees pictures itself (Eric, 1 Oct 2026, item 79).
    /// When the talking model is a picture model (the shipped Qwen3-VL 4B)
    /// and its picture encoder (`mmproj-…`) is beside it, the server is
    /// started with that encoder and "look at me" / "look at my screen" are
    /// asked of the model already running: about 0.45 GB more, instead of a
    /// second 3.6 GB copy of the same model started for every look -- which
    /// is what "the picture reader needs about 3.5 GB and only 2.7 GB is
    /// free" was. `false`: the server is started without it, as before.
    pub see_with_talking_model: bool,
}

/// How the model picks its next word, sent with every request (29 Sep 2026).
///
/// llama-server's defaults are `repeat_penalty` 1.0, `presence_penalty` 0 and
/// DRY off: nothing discourages the model from saying again what it just
/// said, and on Eric's laptop the 4B model answered twenty sentences in a
/// row with the same paragraph. These are Qwen's published settings for its
/// Qwen3-VL Instruct models (the model card's "Generation Hyperparameters",
/// VL: temperature 0.7, top_p 0.8, top_k 20; Qwen3 Instruct cards add min_p
/// 0 and presence_penalty "between 0 and 2 to reduce endless repetitions")
/// plus llama.cpp's DRY sampler, which penalises extending a run of words
/// that already appeared -- exactly the loop above -- and leaves single
/// repeated words (every JSON key of a tool call) alone.
///
/// The presence penalty stays at 1.0 rather than the card's 1.5: at 1.5 the
/// model garbled the JSON of its tool calls (every key repeats), measured
/// 27 Sep 2026. `stronger` is what a reply caught looping is asked again with
/// (`brain`), without tools.
#[derive(Debug, Clone, PartialEq, Deserialize, serde::Serialize)]
#[serde(default)]
pub struct Sampling {
    pub temperature: f32,
    pub top_p: f32,
    pub top_k: u32,
    pub min_p: f32,
    pub presence_penalty: f32,
    pub repeat_penalty: f32,
    /// llama.cpp's DRY ("don't repeat yourself") strength; 0 is off.
    pub dry_multiplier: f32,
    pub dry_base: f32,
    /// A repeated run this many words long costs nothing; longer ones do.
    pub dry_allowed_length: u32,
    /// The presence penalty and DRY strength for a reply asked again because
    /// the first one looped.
    pub stronger_presence_penalty: f32,
    pub stronger_dry_multiplier: f32,
}

impl Default for Sampling {
    fn default() -> Self {
        Sampling {
            temperature: 0.7,
            top_p: 0.8,
            top_k: 20,
            min_p: 0.0,
            presence_penalty: 1.0,
            repeat_penalty: 1.05,
            dry_multiplier: 0.8,
            dry_base: 1.75,
            dry_allowed_length: 2,
            stronger_presence_penalty: 1.5,
            stronger_dry_multiplier: 1.2,
        }
    }
}

impl Sampling {
    /// The fields a llama-server request takes, as JSON. `stronger`: the
    /// settings for asking again after a loop.
    fn fields(&self, stronger: bool) -> serde_json::Map<String, serde_json::Value> {
        use serde_json::json;
        let mut m = serde_json::Map::new();
        m.insert("temperature".into(), json!(self.temperature));
        m.insert("top_p".into(), json!(self.top_p));
        m.insert("top_k".into(), json!(self.top_k));
        m.insert("min_p".into(), json!(self.min_p));
        m.insert(
            "presence_penalty".into(),
            json!(if stronger { self.stronger_presence_penalty.max(self.presence_penalty) } else { self.presence_penalty }),
        );
        m.insert("repeat_penalty".into(), json!(self.repeat_penalty));
        m.insert(
            "dry_multiplier".into(),
            json!(if stronger { self.stronger_dry_multiplier.max(self.dry_multiplier) } else { self.dry_multiplier }),
        );
        m.insert("dry_base".into(), json!(self.dry_base));
        m.insert("dry_allowed_length".into(), json!(self.dry_allowed_length));
        m
    }
}

/// The sampling every chat request carries: set from the settings when the
/// connection is built (`connection`), the defaults until then.
static SAMPLING: std::sync::RwLock<Option<Sampling>> = std::sync::RwLock::new(None);

/// Use these sampling settings for every chat request from now on.
fn set_sampling(s: &Sampling) {
    if let Ok(mut g) = SAMPLING.write() {
        *g = Some(s.clone());
    }
}

/// The sampling chat requests are sent with now.
fn sampling_now() -> Sampling {
    SAMPLING.read().ok().and_then(|g| g.clone()).unwrap_or_default()
}

impl Default for ModelsConfig {
    fn default() -> Self {
        ModelsConfig {
            dir: "models".into(),
            prefer: String::new(),
            context: 8192,
            // Measured, not assumed. See `budget_bytes`: 0 means "whatever
            // this machine can spare", which is the only honest default for a
            // number nobody can know in advance.
            memory_budget_mb: 0,
            server: None,
            port: 8080,
            listen_on: String::new(),
            gpu_layers: "auto".into(),
            talk_ceiling_b: 5,
            draft: String::new(),
            speculate: "off".into(),
            sampling: Sampling::default(),
            talk: String::new(),
            deep: String::new(),
            deep_port: 0,
            deep_idle_secs: 300,
            deep_context: 8192,
            online_second: true,
            see_with_talking_model: true,
        }
    }
}

/// The talking model's file name for `models.talk`: `better` and `faster`
/// named, anything else taken as a file name; `None` for empty (30 Sep 2026).
pub fn talk_id(cfg: &ModelsConfig) -> Option<String> {
    match cfg.talk.trim() {
        "" => None,
        w if w.eq_ignore_ascii_case("better") => Some(crate::deepbrain::BETTER_TALK.to_string()),
        w if w.eq_ignore_ascii_case("faster") => Some(crate::deepbrain::FASTER_TALK.to_string()),
        id => Some(id.trim_end_matches(".gguf").to_string()),
    }
}

/// Is the better talking model the one asked for?
pub fn talks_better(cfg: &ModelsConfig) -> bool {
    talk_id(cfg).as_deref() == Some(crate::deepbrain::BETTER_TALK)
}

#[derive(Debug, Clone, Default)]
pub struct Registry {
    pub models: Vec<Model>,
}

impl Registry {
    /// Scan a directory for GGUF files. Unreadable ones are skipped rather
    /// than aborting the scan — one bad download must not hide the rest.
    ///
    /// What is *not* skipped quietly is the folder itself. `if let Ok(rd)`
    /// with no else made an unreadable directory produce an empty registry,
    /// which reads as "no models installed" — and that is the message you get
    /// when the folder is missing, when the path is wrong, and when a
    /// permission is off. Three different problems, one answer, and the answer
    /// sends you to download something you already have.
    pub fn scan(dir: &Path) -> Registry {
        Self::scan_reporting(dir).0
    }

    /// The scan, plus why it might be empty.
    ///
    /// `None` means the folder was read. `Some` means it wasn't, and the
    /// caller can say which of the three it is rather than guessing.
    /// The folder to scan, anchored to the install.
    ///
    /// `dir` defaults to `"models"`, which is relative — so the registry
    /// found models only when Atlas happened to be launched from the
    /// install folder, and reported "no models" from anywhere else. Same
    /// class as `data/state`: a path that looks per-install and is not.
    pub fn dir_for(cfg: &ModelsConfig) -> std::path::PathBuf {
        crate::roots::under_install(&cfg.dir)
    }

    pub fn scan_reporting(dir: &Path) -> (Registry, Option<String>) {
        let mut models = Vec::new();
        let mut trouble = None;
        let mut unreadable_files = 0u32;

        match std::fs::read_dir(dir) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                trouble = Some(format!(
                    "{} doesn't exist, so I haven't got any models — that's different \
                     from having none installed.",
                    dir.display()
                ));
            }
            Err(e) => {
                trouble = Some(format!(
                    "I couldn't read {}: {e}. There may well be models in there.",
                    dir.display()
                ));
            }
            Ok(rd) => {
                for entry in rd.flatten() {
                    let p = entry.path();
                    if p.extension().map(|x| x == "gguf").unwrap_or(false) {
                        match Model::inspect(&p) {
                            // A vision projector (`mmproj-…`, architecture
                            // `clip`) is half of a picture model, not a model
                            // that can talk. It was being offered as the
                            // "smallest I have" (on Eric's laptop, 26 Sep 2026).
                            Ok(m) if m.is_a_projector() => {}
                            Ok(m) => models.push(m),
                            // A file that is there and won't open is a broken
                            // download, which is worth saying — it is the case
                            // where re-downloading actually helps.
                            Err(_) => unreadable_files += 1,
                        }
                    }
                }
                if models.is_empty() && unreadable_files > 0 {
                    trouble = Some(format!(
                        "{unreadable_files} model file(s) in {} wouldn't open — most likely \
                         a download that didn't finish.",
                        dir.display()
                    ));
                }
            }
        }

        models.sort_by(|a, b| b.parameters.cmp(&a.parameters));
        (Registry { models }, trouble)
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.id == id)
    }

    /// The largest model that fits the budget at this context.
    ///
    /// Bigger is better up to the point where it does not fit; past that it
    /// swaps to disk and becomes unusably slow, which is worse than a smaller
    /// model that runs.
    pub fn best_fit(&self, budget_bytes: u64, context: u64) -> Option<&Model> {
        self.models
            .iter()
            .filter(|m| estimate_memory(m, context) <= budget_bytes)
            .max_by_key(|m| m.parameters)
    }

    pub fn choose<'a>(&'a self, cfg: &ModelsConfig) -> Option<&'a Model> {
        if let Some(m) = self.talking_named(cfg) {
            return Some(m);
        }
        if !cfg.prefer.is_empty() {
            if let Some(m) = self.get(&cfg.prefer) {
                return Some(m);
            }
        }
        self.best_fit(cfg.memory_budget_mb * 1024 * 1024, cfg.context)
    }

    /// Why a model was or wasn't picked — so "it used the small one" is never
    /// a mystery.
    pub fn explain(&self, cfg: &ModelsConfig) -> String {
        if self.models.is_empty() {
            return format!("no GGUF models found in {}", cfg.dir);
        }
        match self.choose(cfg) {
            Some(m) => format!(
                "{} ({} {}, ~{}MB at {} context)",
                m.id,
                m.size_label(),
                m.quant,
                estimate_memory(m, cfg.context) / (1024 * 1024),
                cfg.context
            ),
            None => {
                let smallest = self.models.iter().min_by_key(|m| m.parameters);
                match smallest {
                    Some(m) => format!(
                        "nothing fits {}MB — the smallest, {}, needs ~{}MB",
                        cfg.memory_budget_mb,
                        m.id,
                        estimate_memory(m, cfg.context) / (1024 * 1024)
                    ),
                    None => "no models".into(),
                }
            }
        }
    }
}

/// Memory estimate without re-reading the file.
/// A model's name without the quantisation on the end: `Qwen3VL-4B-Instruct`
/// from `Qwen3VL-4B-Instruct-Q4_K_M`, `-Q8_0`, `-f16`, `-IQ4_XS` or `-BF16`.
pub fn without_quant(id: &str) -> String {
    let lower = id.to_lowercase();
    if let Some((head, tail)) = lower.rsplit_once(['-', '.']) {
        let quant = tail.starts_with('q') && tail[1..].starts_with(|c: char| c.is_ascii_digit())
            || tail.starts_with("iq") && tail[2..].starts_with(|c: char| c.is_ascii_digit())
            || matches!(tail, "f16" | "f32" | "bf16");
        if quant {
            return head.to_string();
        }
    }
    lower
}

pub fn estimate_memory(m: &Model, context: u64) -> u64 {
    // KV cache scales with context; approximate from the weights when the
    // architecture details aren't to hand.
    let ctx = context.min(m.max_context).max(1);
    let kv = (m.weight_bytes / 40) * (ctx / 1024).max(1);
    let base = m.weight_bytes + kv;
    base + base / 10
}

/// The picture encoder's share when the talking model is started with it:
/// the file, plus room for one picture being read.
pub fn projector_memory(m: &Model) -> u64 {
    m.projector()
        .and_then(|p| std::fs::metadata(p).ok())
        .map(|md| md.len() + 150 * 1024 * 1024)
        .unwrap_or(0)
}

// ---------- prompt templating ----------

/// The other half of Ollama's job: models are trained on a specific chat
/// format, and feeding one the wrong markers degrades it badly — often
/// subtly, which is worse than an outright failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Template {
    ChatMl,
    Llama3,
    Mistral,
    Gemma,
    Plain,
}

impl Template {
    /// Guess from the model's own template string, falling back to its name.
    pub fn detect(chat_template: Option<&str>, model_id: &str) -> Template {
        if let Some(t) = chat_template {
            if t.contains("<|im_start|>") {
                return Template::ChatMl;
            }
            if t.contains("<|start_header_id|>") {
                return Template::Llama3;
            }
            if t.contains("[INST]") {
                return Template::Mistral;
            }
            if t.contains("<start_of_turn>") {
                return Template::Gemma;
            }
        }
        let id = model_id.to_lowercase();
        if id.contains("qwen") || id.contains("chatml") {
            Template::ChatMl
        } else if id.contains("llama-3") || id.contains("llama3") {
            Template::Llama3
        } else if id.contains("mistral") || id.contains("mixtral") {
            Template::Mistral
        } else if id.contains("gemma") {
            Template::Gemma
        } else {
            Template::Plain
        }
    }

    pub fn render(&self, system: &str, user: &str) -> String {
        match self {
            Template::ChatMl => format!(
                "<|im_start|>system\n{system}<|im_end|>\n<|im_start|>user\n{user}<|im_end|>\n<|im_start|>assistant\n"
            ),
            Template::Llama3 => format!(
                "<|begin_of_text|><|start_header_id|>system<|end_header_id|>\n\n{system}<|eot_id|>\
                 <|start_header_id|>user<|end_header_id|>\n\n{user}<|eot_id|>\
                 <|start_header_id|>assistant<|end_header_id|>\n\n"
            ),
            // Mistral has no system role; it is folded into the first turn.
            Template::Mistral => format!("[INST] {system}\n\n{user} [/INST]"),
            Template::Gemma => format!(
                "<start_of_turn>user\n{system}\n\n{user}<end_of_turn>\n<start_of_turn>model\n"
            ),
            Template::Plain => format!("{system}\n\n{user}\n\n"),
        }
    }

    /// Where generation should stop for this format.
    pub fn stop_tokens(&self) -> Vec<&'static str> {
        match self {
            Template::ChatMl => vec!["<|im_end|>", "<|im_start|>"],
            Template::Llama3 => vec!["<|eot_id|>"],
            Template::Mistral => vec!["</s>", "[INST]"],
            Template::Gemma => vec!["<end_of_turn>"],
            Template::Plain => vec!["\n\n\n"],
        }
    }
}

// ---------- running it ----------

/// Command line for llama.cpp's own server. This is the piece that removes
/// Ollama: same engine underneath, one fewer wrapper on top.
pub fn server_args(model: &Model, cfg: &ModelsConfig, gpu_layers: u32) -> Vec<String> {
    let ctx = if cfg.context == 0 { model.max_context } else { cfg.context.min(model.max_context) };
    vec![
        "-m".into(),
        model.path.display().to_string(),
        "-c".into(),
        ctx.to_string(),
        "--port".into(),
        cfg.port.to_string(),
        "--host".into(),
        listen_host(cfg), // never a public interface
        "-ngl".into(),
        gpu_layers.to_string(),
        "--no-webui".into(),
        // The chat endpoint with the model's own template, so tools and
        // turns are formatted the way the model was trained on (Qwen's
        // `<tool_call>` format) rather than by hand (27 Sep 2026).
        "--jinja".into(),
        // Two slots, so talking to you and background work (folding the
        // conversation, drafting) each keep their own cached prompt. One
        // slot meant a background fold evicted the conversation's prefix,
        // and the next thing you said paid for the whole prompt again. The
        // conversation names slot 0 (`chat_body`), which any server has;
        // background calls name none and take the free one. `--kv-unified`
        // shares the one context between the two rather than halving it.
        "-np".into(),
        "2".into(),
        "--kv-unified".into(),
        // Reuse cached chunks of a prompt whose middle moved, not only an
        // identical prefix. The shipped Qwen3-VL model can't (its position
        // encoding doesn't shift): the server says "cache_reuse is not
        // supported by this context" and carries on without it, measured
        // 27 Sep 2026. A text-only model gets the benefit.
        "--cache-reuse".into(),
        "256".into(),
        // The host-memory prompt cache is 8 GB by default in this llama.cpp
        // build (`--cache-ram`, checked on the laptop's own llama-server
        // 30 Sep 2026). On a 16 GB laptop beside a browser that's the
        // memory running at 84-86% all evening; 1 GB holds the two slots'
        // prompts with room to spare.
        "--cache-ram".into(),
        "1024".into(),
    ]
    .into_iter()
    // Speculative decoding, only when set (`models.draft`, `models.speculate`).
    .chain(speculation_args(cfg, &model.path))
    .collect()
}

/// `--mmproj <encoder>` when the talking model can see and is allowed to.
pub fn projector_args(model: &Model, cfg: &ModelsConfig) -> Vec<String> {
    if !cfg.see_with_talking_model {
        return Vec::new();
    }
    match model.projector() {
        Some(p) => vec!["--mmproj".into(), p.display().to_string()],
        None => Vec::new(),
    }
}

/// The no-model speculation kinds the pinned llama.cpp (b10456) takes for
/// `--spec-type` -- read off that build's `tools/server/README.md` and
/// `common/speculative.cpp` (28 Sep 2026). Anything else in `speculate` is
/// ignored rather than passed, because an unknown kind stops the server.
pub const NGRAM_KINDS: &[&str] = &["ngram-mod", "ngram-simple", "ngram-map-k", "ngram-map-k4v", "ngram-cache"];

/// How many words the draft model guesses ahead, at most and at least.
/// b10456 took `--draft-max`/`--draft-min` away (its README: "the argument
/// has been removed"); these are their replacements.
pub const DRAFT_MAX: u32 = 16;
pub const DRAFT_MIN: u32 = 1;

/// Where the draft model is, when one is set and the file is there.
pub fn draft_path(cfg: &ModelsConfig) -> Option<PathBuf> {
    let name = cfg.draft.trim();
    if name.is_empty() {
        return None;
    }
    let p = PathBuf::from(name);
    let p = if p.is_absolute() { p } else { Path::new(&cfg.dir).join(p) };
    p.is_file().then_some(p)
}

/// The speculation flags for the server: a draft model (`-md`, as
/// `draft-simple`: a plain GGUF isn't recognised as a draft on its own in
/// b10456 -- `common_speculative_types_from_gguf` only knows MTP/DFlash
/// heads), and/or a no-model kind. Nothing when neither is set, or the draft
/// file isn't there -- or is the very model being run (on a machine small
/// enough that the draft was the one chosen to talk).
pub fn speculation_args(cfg: &ModelsConfig, running: &Path) -> Vec<String> {
    let mut kinds: Vec<&str> = Vec::new();
    let mut out: Vec<String> = Vec::new();
    let same = |d: &Path| match (d.canonicalize(), running.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => d == running,
    };
    if let Some(d) = draft_path(cfg).filter(|d| !same(d)) {
        kinds.push("draft-simple");
        out.extend([
            "-md".to_string(),
            d.display().to_string(),
            "--spec-draft-n-max".into(),
            DRAFT_MAX.to_string(),
            "--spec-draft-n-min".into(),
            DRAFT_MIN.to_string(),
        ]);
    }
    let ngram = cfg.speculate.trim();
    if NGRAM_KINDS.contains(&ngram) {
        kinds.push(ngram);
    }
    if kinds.is_empty() {
        return Vec::new();
    }
    out.push("--spec-type".into());
    out.push(kinds.join(","));
    out
}

/// How many layers to put on the GPU, given its free VRAM. Offloading more
/// than fits is slower than offloading none, because the driver starts paging.
pub fn gpu_layers_for(model: &Model, vram_bytes: u64, context: u64) -> u32 {
    if vram_bytes == 0 {
        return 0;
    }
    let need = estimate_memory(model, context);
    if need <= vram_bytes {
        return 999; // all of them
    }
    let layers = 32u64; // typical; refined from block_count when available
    let per_layer = (need / layers).max(1);
    let usable = vram_bytes.saturating_sub(need / 8); // leave room for the cache
    ((usable / per_layer) as u32).min(layers as u32)
}

/// The address the model server is told to listen on: loopback, unless
/// `listen_on` names a private address `server::bind_address` accepts. A
/// setting it refuses falls back to loopback — the safe side — and `atlas
/// doctor` says why.
pub fn listen_host(cfg: &ModelsConfig) -> String {
    crate::server::bind_address(&cfg.listen_on)
        .map(|a| a.to_string())
        .unwrap_or_else(|_| "127.0.0.1".into())
}

/// A loopback URL moved to wherever the server actually listens.
fn at_listen_host(url: String, cfg: &ModelsConfig) -> String {
    url.replacen("127.0.0.1", &listen_host(cfg), 1)
}

pub fn health_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/health")
}

/// The talking model's chat address, at the host it listens on.
pub fn talking_chat_url(cfg: &ModelsConfig) -> String {
    at_listen_host(format!("http://127.0.0.1:{}/v1/chat/completions", cfg.port), cfg)
}

pub fn completion_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}/completion")
}

/// Request body for llama.cpp's /completion endpoint.
///
/// Qwen's own recommended sampling for its Instruct models (the one Atlas
/// ships): with llama-server's defaults it looped and rambled, and each loop
/// cost another call (27 Sep 2026). Now from the settings (`Sampling`), with
/// DRY added (29 Sep 2026).
///
/// `id_slot` 1 (29 Sep 2026): a one-prompt call is always work beside the
/// conversation -- the running summary, a council seat, a screen read.
/// Naming no slot let llama.cpp put it in the conversation's slot 0 when
/// that one was idle, and the next turn read its whole prompt again.
pub fn completion_body(prompt: &str, template: Template, max_tokens: u32, sampling: &Sampling) -> String {
    let stops: Vec<serde_json::Value> =
        template.stop_tokens().iter().map(|s| serde_json::Value::String(s.to_string())).collect();
    let mut body = serde_json::Map::new();
    body.insert("prompt".into(), serde_json::Value::String(prompt.to_string()));
    body.insert("n_predict".into(), serde_json::json!(max_tokens));
    body.insert("stream".into(), serde_json::json!(false));
    body.extend(sampling.fields(false));
    body.insert("cache_prompt".into(), serde_json::json!(true));
    body.insert("id_slot".into(), serde_json::json!(1));
    body.insert("stop".into(), serde_json::Value::Array(stops));
    serde_json::Value::Object(body).to_string()
}

/// Start the server for a chosen model.
///
/// Hands back the child process rather than dropping it on the floor. It was
/// spawn-and-forget, which meant nothing could stop it, nothing could reap
/// it, and Atlas exiting left a multi-gigabyte server running. The caller
/// (`Daemon::start_model_server`) hands it to `lifecycle::Helpers`, which
/// owns it for the rest of the session.
pub fn launch(
    model: &Model,
    cfg: &ModelsConfig,
    gpu_layers: u32,
    vars: &Vars,
) -> Result<std::process::Child> {
    // The talking model gets its eyes (item 79); the deep model's server,
    // started through `launch_logging` directly, never does.
    let eyes = projector_args(model, cfg);
    let child = launch_with(model, cfg, gpu_layers, vars, "model-server.log", &eyes)?;
    LAUNCHED.store(crate::store::now(), std::sync::atomic::Ordering::Relaxed);
    SEES.store(!eyes.is_empty(), std::sync::atomic::Ordering::Relaxed);
    Ok(child)
}

/// Was the talking model's server last started able to see pictures?
static SEES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Can the running talking model be asked about a picture? True when its
/// server was started with the picture encoder (`launch`). Whether that
/// server is still up is the caller's to check.
pub fn talking_model_sees() -> bool {
    SEES.load(std::sync::atomic::Ordering::Relaxed)
}

/// `launch`, with the server's messages in `data/logs/<log>`, and without
/// marking the talking model's server as just started -- what the deep
/// model's server is started with (`deepbrain`), so a question to the
/// talking model never waits on the other one loading.
pub fn launch_logging(
    model: &Model,
    cfg: &ModelsConfig,
    gpu_layers: u32,
    vars: &Vars,
    log: &str,
) -> Result<std::process::Child> {
    launch_with(model, cfg, gpu_layers, vars, log, &[])
}

fn launch_with(
    model: &Model,
    cfg: &ModelsConfig,
    gpu_layers: u32,
    vars: &Vars,
    log: &str,
    extra: &[String],
) -> Result<std::process::Child> {
    let tool = server_tool(cfg)
        .ok_or_else(|| AtlasError::Config("no llama-server configured in models.server".into()))?;
    let (cmd, mut args) = tool.resolved(vars);
    args.extend(server_args(model, cfg, gpu_layers));
    args.extend(extra.iter().cloned());
    // The picked tools after the conversation (`tools_late_template`).
    if let Some(path) = tools_late_template_file(model) {
        args.push("--chat-template-file".into());
        args.push(path.display().to_string());
    }
    let child = crate::tools::command(&cmd)
        .args(&args)
        // Each slot keeps what it has read (28 Sep 2026). With `--kv-unified`,
        // llama.cpp clears every idle slot whenever another starts work
        // (`--cache-idle-slots`, on by default), so a call beside the
        // conversation on the other slot wiped the conversation's slot and
        // the next turn read its whole prompt again -- measured on the real
        // server: 1,946 tokens read instead of 601. Set by its environment
        // name rather than the flag: a build without the setting ignores an
        // unknown variable, where an unknown flag stops the server.
        .env(CACHE_IDLE_SLOTS_ENV, "0")
        .stdout(std::process::Stdio::null())
        // What it says goes to data/logs/model-server.log (29 Sep 2026): a
        // server that died loading its model (a bad file, not enough
        // graphics memory) left no reason anywhere.
        .stderr(model_server_log(log))
        .spawn()
        .map_err(|e| AtlasError::Platform(format!("could not start {cmd}: {e}")))?;
    Ok(child)
}

/// Where the model server's own messages go: `data/logs/model-server.log`,
/// started afresh each launch. Nowhere, when that can't be opened.
fn model_server_log(name: &str) -> std::process::Stdio {
    let dir = crate::roots::data_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    // The last run's log kept beside it (`.previous`): started afresh, a
    // server that died and was restarted took the reason with it (30 Sep
    // 2026: four restarts in an hour on Eric's laptop, none explained).
    let _ = std::fs::rename(dir.join(name), dir.join(format!("{name}.previous")));
    match std::fs::File::create(dir.join(name)) {
        Ok(f) => std::process::Stdio::from(f),
        Err(_) => std::process::Stdio::null(),
    }
}

/// Why the model server stopped, in its own words: the last line of
/// `data/logs/model-server.log` that says something went wrong, or its last
/// line. `None` when the log is empty or can't be read.
pub fn model_server_last_words() -> Option<String> {
    let text = std::fs::read_to_string(crate::roots::data_dir().join("logs").join("model-server.log")).ok()?;
    last_words_in(&text)
}

/// The telling line of a model server's log (see `model_server_last_words`).
pub fn last_words_in(text: &str) -> Option<String> {
    let lines: Vec<&str> = text.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let bad = |l: &&&str| {
        let l = l.to_ascii_lowercase();
        l.contains("error") || l.contains("failed") || l.contains("out of memory") || l.contains("unable")
    };
    let line = lines.iter().rev().find(bad).or(lines.last())?;
    let mut line = line.to_string();
    if line.len() > 200 {
        let mut cut = 200;
        while !line.is_char_boundary(cut) {
            cut -= 1;
        }
        line.truncate(cut);
    }
    Some(line)
}

/// llama.cpp's switch for clearing idle slots, by its environment name.
pub const CACHE_IDLE_SLOTS_ENV: &str = "LLAMA_ARG_CACHE_IDLE_SLOTS";

/// When Atlas last started a model server, in seconds (0: never). What
/// `WaitsForServer` goes by: a server that was just started is worth
/// waiting for while it loads; one nobody started isn't coming.
static LAUNCHED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// How long a model takes to load, at most, before a question gives up on
/// it. The 4B picture model loads in well under a minute on a laptop CPU.
pub const LOADING_SECS: u64 = 120;

/// How long ago Atlas last started a model server; `None` if it hasn't.
pub fn launched_secs_ago() -> Option<u64> {
    let at = LAUNCHED.load(std::sync::atomic::Ordering::Relaxed);
    (at != 0).then(|| crate::store::now().saturating_sub(at))
}

/// Is the model server Atlas started probably still loading its model? Just
/// started, within the time a load takes.
pub fn probably_still_loading(launched_secs_ago: Option<u64>) -> bool {
    launched_secs_ago.is_some_and(|s| s < LOADING_SECS)
}

/// The server program to run. The shipped setting says `llama-server`,
/// which a fresh Windows doesn't have on its PATH; `atlas get pictures`
/// puts it in `tools/llama/` in Atlas's own folder, so that's used when
/// it's there. A path you wrote yourself is left as you wrote it.
pub fn server_tool(cfg: &ModelsConfig) -> Option<ExternalTool> {
    let mut tool = cfg.server.clone()?;
    if matches!(tool.command.as_str(), "llama-server" | "llama-server.exe") {
        let ours = crate::roots::under_install(if cfg!(windows) {
            "tools/llama/llama-server.exe"
        } else {
            "tools/llama/llama-server"
        });
        if ours.is_file() {
            tool.command = ours.display().to_string();
        }
    }
    Some(tool)
}

/// Wait for a server that's loading its model. True once it answers.
fn wait_until_up(cfg: &ModelsConfig, vars: &Vars, secs: u64) -> bool {
    let http = server_get();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        if is_running(cfg, &http, vars) {
            return true;
        }
        if std::time::Instant::now() >= until {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Atlas's own model connection, which waits for a server Atlas has just
/// started to finish loading before putting a question to it. Without it
/// the first question after starting went to a server still reading 2.5 GB
/// off the disk, and failed.
pub struct WaitsForServer {
    pub inner: std::sync::Arc<dyn crate::brain::Llm>,
    pub cfg: ModelsConfig,
    pub vars: Vars,
}

impl crate::brain::Llm for WaitsForServer {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        let launched = LAUNCHED.load(std::sync::atomic::Ordering::Relaxed);
        if launched != 0 && crate::store::now().saturating_sub(launched) < LOADING_SECS {
            let _ = wait_until_up(&self.cfg, &self.vars, LOADING_SECS);
        }
        self.inner.complete(system, user)
    }

    fn native_chat(&self) -> bool {
        self.inner.native_chat()
    }

    fn chat(
        &self,
        req: &crate::brain::ChatRequest,
        on_text: &mut dyn FnMut(&str) -> bool,
    ) -> Result<crate::brain::ChatReply> {
        let launched = LAUNCHED.load(std::sync::atomic::Ordering::Relaxed);
        if launched != 0 && crate::store::now().saturating_sub(launched) < LOADING_SECS {
            let _ = wait_until_up(&self.cfg, &self.vars, LOADING_SECS);
        }
        self.inner.chat(req, on_text)
    }
}

// ---------------------------------------------------------------------------
// The half that was missing: which model, on *this* machine.
//
// Everything above can read a GGUF, size it, pick the best that fits and
// explain the choice. Nothing ever scanned a real folder, and the budget it
// sized against was a number typed into a config file.
//
// That number was once raised as a decision -- "is 3500 or 6000 right for
// `memory_budget_mb`?" -- and withdrawn as not being one, because `fit.rs`
// already *measures* what this machine can spare. Two declarations of one
// fact, and the measured one is the true one. Same correction already made to
// `wants.rs` and to `settings.rs`.
// ---------------------------------------------------------------------------

use crate::fit::Machine;

/// What Atlas may actually spend on a model here, in bytes.
///
/// The machine decides; the config may only lower it. `memory_budget_mb: 0`
/// means "whatever this machine can spare", which is the honest default for a
/// number nobody can know in advance. A non-zero value is a *cap* -- "never
/// use more than this even if there is room" is a real thing to want, and it
/// is the only thing this setting can truthfully be.
///
/// Never the other way round: a config that raises the budget past what was
/// measured produces a model that will not load, which is the failure `fit.rs`
/// exists to prevent, arrived at from the other side.
pub fn budget_bytes(cfg: &ModelsConfig, m: &Machine) -> u64 {
    let measured = m.budget_mb();
    let mb = if cfg.memory_budget_mb == 0 {
        measured
    } else {
        cfg.memory_budget_mb.min(measured)
    };
    mb * 1024 * 1024
}

/// The model to run: the best that fits what's free right now, else -- when
/// you haven't set a memory limit yourself -- the best that fits in half the
/// machine's memory. What's "free" on a laptop with a browser open is small,
/// and that alone left Eric's Atlas with no model at all (27 Sep 2026: every
/// question got "I can't answer that here"); Windows pages out what's idle.
pub fn pick<'a>(registry: &'a Registry, cfg: &ModelsConfig, m: &crate::fit::Machine) -> Option<&'a Model> {
    registry.choose_for(cfg, budget_bytes(cfg, m)).or_else(|| {
        if cfg.memory_budget_mb != 0 {
            return None;
        }
        registry.choose_for(cfg, m.total_ram_mb * 1024 * 1024 / 2)
    })
}

impl Registry {
    /// The model `models.talk` names, when its file is here.
    ///
    /// `faster` -- the shipped default -- doesn't override a model named in
    /// `prefer`; `better` and a file name do.
    fn talking_named(&self, cfg: &ModelsConfig) -> Option<&Model> {
        let id = talk_id(cfg)?;
        if id == crate::deepbrain::FASTER_TALK && !cfg.prefer.trim().is_empty() {
            return None;
        }
        self.get(&id)
    }
}

/// The best model that fits a budget worked out from the machine.
///
/// `choose` sizes against the config alone and stays for callers that have no
/// machine to hand. This is the one the daemon uses.
impl Registry {
    ///
    /// ## Not simply the largest (27 Sep 2026)
    ///
    /// It used to be `best_fit`: the largest model that fits. On a laptop
    /// that is the slowest one that fits, and a conversation where every
    /// answer takes twice as long is what Eric called unacceptable. Setup
    /// downloads a 4B model sized for talking; a bigger one dropped in the
    /// folder was chosen over it the moment memory allowed. Now the largest
    /// that fits AT OR UNDER `talk_ceiling_b` is chosen; only when none does
    /// is a bigger one used, and then the smallest of those. Naming a model
    /// (`prefer`) still gets exactly that one, and `talk_ceiling_b: 0` gives
    /// the old behaviour.
    pub fn choose_for<'a>(&'a self, cfg: &ModelsConfig, budget: u64) -> Option<&'a Model> {
        if let Some(m) = self.talking_named(cfg) {
            return Some(m);
        }
        if !cfg.prefer.is_empty() {
            if let Some(m) = self.get(&cfg.prefer) {
                return Some(m);
            }
        }
        // Nothing named: the shipped talking model when it's here (30 Sep
        // 2026) -- a bigger one dropped in the folder, the deep model
        // included, never takes over talking by being bigger.
        if cfg.prefer.is_empty() {
            if let Some(m) = self.get(crate::deepbrain::FASTER_TALK).filter(|m| estimate_memory(m, cfg.context) <= budget) {
                return Some(m);
            }
        }
        if cfg.talk_ceiling_b == 0 {
            return self.best_fit(budget, cfg.context);
        }
        let ceiling = cfg.talk_ceiling_b.saturating_mul(1_000_000_000);
        let fits = || self.models.iter().filter(|m| estimate_memory(m, cfg.context) <= budget);
        fits()
            .filter(|m| m.parameters <= ceiling)
            .max_by_key(|m| m.parameters)
            .or_else(|| fits().min_by_key(|m| m.parameters))
    }

    /// Why this model, in one line, sized against the real machine.
    ///
    /// Says what was measured as well as what was chosen, because "it used
    /// the small one" is only not a mystery if you can see the number it was
    /// deciding against.
    pub fn explain_for(&self, cfg: &ModelsConfig, m: &Machine, budget: u64) -> String {
        let mb = budget / (1024 * 1024);
        if self.models.is_empty() {
            return format!(
                "No models in {} — nothing to run locally. ({}, {mb}MB spare.)",
                cfg.dir,
                crate::fit::describe(m)
            );
        }
        match self.choose_for(cfg, budget) {
            Some(model) => {
                let needs = estimate_memory(model, cfg.context) / (1024 * 1024);
                let why = if cfg.prefer == model.id {
                    " — you asked for this one"
                } else {
                    ""
                };
                format!(
                    "{} ({} {}), needs ~{needs}MB of {mb}MB spare{why}.",
                    model.id,
                    model.size_label(),
                    model.quant
                )
            }
            None => {
                let smallest = self.models.iter().min_by_key(|x| x.parameters);
                match smallest {
                    Some(s) => format!(
                        "Nothing fits. The smallest I have, {}, needs ~{}MB and there is {mb}MB spare.",
                        s.id,
                        estimate_memory(s, cfg.context) / (1024 * 1024)
                    ),
                    None => format!("No models in {}.", cfg.dir),
                }
            }
        }
    }
}

/// How many layers this machine's graphics can take.
///
/// `launch` already existed and already spawns the server; what it never had
/// was a caller that knew the machine. This is the missing argument, and it
/// goes through `Machine::usable_vram_mb` rather than `vram_mb` because
/// integrated graphics share system memory — counting that as spare video
/// memory offloads more than fits, and the driver starts paging, which is
/// slower than offloading nothing at all.
///
/// ## Graphics builds with no card measured (27 Sep 2026)
///
/// `fit::measure` cannot see a graphics card's own memory, so on every
/// machine this came out 0 -- and setup downloads llama.cpp's *Vulkan*
/// build precisely to use the laptop's graphics (Eric's has Intel Arc,
/// integrated). The model ran on the processor alone. Integrated graphics
/// have no memory of their own to overflow: they use the same memory the
/// model was already chosen to fit (`pick`), so the paging worry above is
/// about a dedicated card that is too small, which `gpu_layers_for` still
/// handles when one is measured. With none measured and a graphics build
/// installed, every layer goes on the graphics; a Vulkan build on a machine
/// with no usable graphics runs on the processor anyway. `gpu_layers` in
/// your settings overrides this either way.
pub fn layers_here(model: &Model, cfg: &ModelsConfig, m: &Machine) -> u32 {
    let setting = cfg.gpu_layers.trim().to_ascii_lowercase();
    match setting.as_str() {
        "" | "auto" => {}
        "off" | "none" | "cpu" | "no" => return 0,
        "all" | "max" => return ALL_LAYERS,
        n => {
            if let Ok(n) = n.parse::<u32>() {
                return n;
            }
            // Not a word or a number we know: `auto`, the safe reading.
        }
    }
    let measured = gpu_layers_for(model, m.usable_vram_mb() * 1024 * 1024, cfg.context);
    if measured > 0 || m.usable_vram_mb() > 0 {
        return measured;
    }
    match server_tool(cfg) {
        Some(tool) if is_graphics_build(Path::new(&tool.command)) => ALL_LAYERS,
        _ => 0,
    }
}

/// "Every layer", as llama.cpp's `-ngl` takes it.
pub const ALL_LAYERS: u32 = 999;

/// Whether a llama.cpp server program can use the graphics: its path names a
/// graphics backend (`…-vulkan-…`), or its folder holds one of the backend
/// libraries the graphics builds ship beside it (`ggml-vulkan.dll`,
/// `ggml-cuda.dll`, `libggml-metal.dylib`, …).
pub fn is_graphics_build(server: &Path) -> bool {
    const BACKENDS: [&str; 5] = ["vulkan", "cuda", "metal", "hip", "sycl"];
    let named = server.to_string_lossy().to_ascii_lowercase();
    if BACKENDS.iter().any(|b| named.contains(b)) {
        return true;
    }
    let Some(dir) = server.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return false;
    };
    let Ok(rd) = std::fs::read_dir(dir) else { return false };
    rd.flatten().any(|e| {
        let n = e.file_name().to_string_lossy().to_ascii_lowercase();
        n.contains("ggml-") && BACKENDS.iter().any(|b| n.contains(b))
    })
}

/// Roughly how much memory the server will take, for the `lifecycle` budget.
///
/// The weights plus the cache — the same estimate `best_fit` sized against,
/// so the supervisor and the chooser cannot disagree about how big the thing
/// they are both talking about is.
pub fn footprint_mb(model: &Model, cfg: &ModelsConfig) -> u64 {
    let eyes = if cfg.see_with_talking_model { projector_memory(model) } else { 0 };
    (estimate_memory(model, cfg.context) + eyes) / (1024 * 1024)
}

/// How Atlas talks to its own model server: a POST with the request body on
/// stdin, through `curl`, which every supported Windows ships
/// (`C:\\Windows\\System32\\curl.exe`).
///
/// This used to be borrowed from `research.fetch`, on the reasoning that it
/// was "the one HTTP tool configured". By the time anyone ran it that tool was
/// a headless Chrome with `--dump-dom`: it can't POST, sends no body, and
/// returns an HTML page. So on any install where Atlas built its own model
/// connection, every question went to the model as a bare GET and came back
/// as a web page — which Atlas then said to you (Eric's friend, 26 Sep 2026:
/// "it would only respond in reports… literally making a document to reply").
/// A model server is not a web page to read, so it gets a tool of its own.
pub fn server_post() -> ExternalTool {
    ExternalTool {
        command: "curl".into(),
        args: [
            "-s", "-S", "--noproxy", "*", "-X", "POST",
            "-H", "Content-Type: application/json", "--data-binary", "@-", "{url}",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect(),
        stdin_text: true,
        result_file: None,
        timeout_secs: 180,
    }
}

/// Where the connection Atlas builds for itself goes: this machine, unless
/// the server is set to listen somewhere else (your own server over the
/// tunnel). What the context builder asks before putting a window title in
/// a prompt.
pub fn self_built_endpoint(cfg: &ModelsConfig) -> crate::brain::Endpoint {
    let mut tool = server_post();
    tool.args = tool.args.iter().map(|a| a.replace("{url}", &at_listen_host(completion_url(cfg.port), cfg))).collect();
    crate::brain::LlmConfig { tool, request: String::new(), response_path: String::new(), vision_request: None }.endpoint()
}

/// A plain GET against the model server, for `/health`.
pub fn server_get() -> ExternalTool {
    ExternalTool {
        command: "curl".into(),
        args: ["-s", "--noproxy", "*", "-m", "5", "{url}"].iter().map(|s| s.to_string()).collect(),
        stdin_text: false,
        result_file: None,
        timeout_secs: 10,
    }
}

/// Does a health reply say llama-server is up and ready?
pub fn health_says_ok(body: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(body.trim())
        .ok()
        .and_then(|v| v.get("status").and_then(|s| s.as_str()).map(|s| s == "ok"))
        .unwrap_or(false)
}

/// Is the server already up?
///
/// A URL rather than a request, because the caller owns the HTTP tool — this
/// module deliberately shells out through the configured `ExternalTool` like
/// everything else rather than pulling in an HTTP client for one GET.
pub fn is_running(cfg: &ModelsConfig, http: &ExternalTool, vars: &Vars) -> bool {
    let mut v = vars.clone();
    v.insert("url".into(), at_listen_host(health_url(cfg.port), cfg));
    match http.run(&v, None) {
        // llama-server's own answer, `{"status":"ok"}`, and nothing else
        // (29 Sep 2026): any reply containing the letters "ok" -- "token",
        // "book" -- from some other program on the port counted as the model
        // being up, and Atlas never started its own.
        Ok(body) => health_says_ok(&body),
        Err(_) => false,
    }
}

/// A one-prompt call to a model whose template thinks out loud unless told
/// not to (30 Sep 2026): Qwen3.5 4B and 9B's own template -- read out of the
/// published GGUF files -- starts every answer with `<think>` unless
/// `enable_thinking` is false, and false writes an empty thought instead.
/// The chat path sends that switch (`chat_body`); `/completion` takes a
/// prompt written here, so the empty thought is written here. A template
/// with no such switch (the shipped Qwen3-VL Instruct's) is left alone.
pub fn no_thinking_prompt(prompt: String, chat_template: Option<&str>) -> String {
    let switch = chat_template.is_some_and(|t| t.contains("enable_thinking"));
    if switch && prompt.ends_with("<|im_start|>assistant\n") {
        prompt + "<think>\n\n</think>\n\n"
    } else {
        prompt
    }
}

/// The model connection Atlas would build for itself, given a chosen model.
///
/// This is the piece that makes "no Ollama in the path" true rather than
/// merely possible. `ShellLlm` can already talk to `llama-server` — it speaks
/// whatever `tools.llm.request` says — but only if *you* hand-write a request
/// body whose prompt is wrapped in the exact control tokens that model expects.
/// Get it wrong and the model still answers, just worse, and nothing tells
/// you. `Template::detect` already knows the right wrapping from the file's own
/// chat template; this hands it to the request body so nobody has to.
///
/// `{system}` and `{user}` are left as placeholders for `ShellLlm` to
/// substitute, exactly as a hand-written config would.
pub fn llm_config_for(model: &Model, cfg: &ModelsConfig, http: &ExternalTool) -> crate::brain::LlmConfig {
    let template = Template::detect(model.chat_template.as_deref(), &model.id);
    let prompt = no_thinking_prompt(template.render("{system}", "{user}"), model.chat_template.as_deref());
    let body = completion_body(&prompt, template, 512, &cfg.sampling);

    let mut tool = http.clone();
    // The endpoint, resolved here rather than left to a `{url}` the caller
    // might forget to set.
    tool.args = tool
        .args
        .iter()
        .map(|a| a.replace("{url}", &at_listen_host(completion_url(cfg.port), cfg)))
        .collect();

    crate::brain::LlmConfig {
        tool,
        request: body,
        // llama.cpp returns the text at `content`; Ollama returns `response`.
        // Naming the right one here is half of what removing Ollama means.
        response_path: "content".into(),
        vision_request: None,
    }
}

/// The model connection Atlas builds for itself from its settings: the
/// hand-written `tools.llm` if there is one, else the model in its models
/// folder that fits, through the server on `models.port` -- waited for while
/// it loads, with `tools.llm_secondary` as the fallback. What the laptop's
/// daemon and the phone core both use.
/// Why there is no model to talk with, in words for a reply (29 Sep 2026:
/// "it isn't loaded yet" was said whatever the reason, for the whole
/// session). The folder's own trouble first, then an empty folder, then
/// none that fits the memory free.
pub fn why_no_model(cfg: &ModelsConfig) -> String {
    let (registry, trouble) = Registry::scan_reporting(&Registry::dir_for(cfg));
    if let Some(t) = trouble {
        return t.trim().trim_end_matches('.').to_string();
    }
    if registry.models.is_empty() {
        return "there's no language model in my models folder yet; opening Atlas runs setup, which fetches one".into();
    }
    let m = crate::fit::measure();
    format!(
        "none of the models in my models folder fits the memory free right now: {}",
        registry.explain_for(cfg, &m, budget_bytes(cfg, &m)).trim().trim_end_matches('.')
    )
}

pub fn connection(tc: &crate::voice::ToolsConfig) -> Option<std::sync::Arc<dyn crate::brain::Llm>> {
    // Every chat request from here on carries the settings' sampling.
    set_sampling(&tc.models.sampling);
    let derived: Option<crate::brain::LlmConfig> = if tc.llm.is_some() {
        None
    } else {
        // A POST of its own (`models::server_post`), not `research.fetch`:
        // that ships as a headless Chrome, which turned every question into a
        // GET and every answer into a web page.
        {
            let mcfg = &tc.models;
            let (registry, _) =
                crate::models::Registry::scan_reporting(&crate::models::Registry::dir_for(mcfg));
            // A server already running has its model loaded: the memory it
            // uses is already spent, so measuring what's free now would say
            // nothing fits and leave Atlas with no model at all (the laptop,
            // 26 Sep 2026: "Nothing fits … 297MB spare" with the server up).
            let up = crate::models::is_running(mcfg, &crate::models::server_get(), &tc.vars);
            let chosen = if up { registry.choose_for(mcfg, u64::MAX) } else { pick(&registry, mcfg, &crate::fit::measure()) };
            chosen
                .map(|model| crate::models::llm_config_for(model, mcfg, &crate::models::server_post()))
        }
    };
    let local = tc.llm.as_ref().or(derived.as_ref()).map(|lc| {
        std::sync::Arc::new(crate::brain::ShellLlm { cfg: lc.clone(), vars: tc.vars.clone() })
            as std::sync::Arc<dyn crate::brain::Llm>
    });
    // No model on this machine: the free online ones are the only ones, when
    // they're allowed (`models.online_second`).
    let Some(primary) = local else {
        return tc.models.online_second.then(|| {
            std::sync::Arc::new(crate::freeonline::FreeOnline::new()) as std::sync::Arc<dyn crate::brain::Llm>
        });
    };
    // Atlas's own connection waits for a server Atlas has just started
    // (`Daemon::keep_model_server`) to finish loading its model.
    let primary = if tc.llm.is_none() {
        std::sync::Arc::new(crate::models::WaitsForServer {
            inner: primary,
            cfg: tc.models.clone(),
            vars: tc.vars.clone(),
        }) as std::sync::Arc<dyn crate::brain::Llm>
    } else {
        primary
    };
    // An optional stronger model for the hard drafts, and as a fallback if the
    // local one fails. Unset (the default) → this is just the local model, so
    // an offline install is unchanged.
    let own_second = tc.llm_secondary.is_some();
    let secondary = tc
        .llm_secondary
        .as_ref()
        .map(|lc| {
            std::sync::Arc::new(crate::brain::ShellLlm { cfg: lc.clone(), vars: tc.vars.clone() })
                as std::sync::Arc<dyn crate::brain::Llm>
        })
        // Your own second model first; otherwise the free online ones.
        .or_else(|| {
            tc.models.online_second.then(|| {
                std::sync::Arc::new(crate::freeonline::FreeOnline::new()) as std::sync::Arc<dyn crate::brain::Llm>
            })
        });
    let both = crate::brain::FallbackLlm::new(primary, secondary);
    let both = if own_second { both.secondary_is_your_own() } else { both };
    Some(std::sync::Arc::new(both) as std::sync::Arc<dyn crate::brain::Llm>)
}

// ---------------------------------------------------------------------------
// Talking to the model as a conversation (27 Sep 2026).
//
// llama-server's own `/completion` takes one prompt Atlas wrapped by hand in
// the model's control tokens: one system and one user message, so the whole
// conversation was pasted into the user message. Its OpenAI-shaped
// `/v1/chat/completions` takes real turns and tools, applies the model's own
// chat template (`--jinja`), and streams. Atlas talks to it in-process over
// plain HTTP -- the server is on this machine or at the other end of the
// tunnel, never https -- so a reply can be read, spoken and shown sentence by
// sentence while the model is still writing it.
// ---------------------------------------------------------------------------

/// The chat address beside a llama-server `/completion` (or a chat address
/// given outright), when it is plain `http://`. `None` for anything else --
/// Ollama, a hosted API -- which keeps the one-prompt path it always had.
pub fn chat_url_beside(cfg: &crate::brain::LlmConfig) -> Option<String> {
    cfg.tool.args.iter().chain(std::iter::once(&cfg.tool.command)).find_map(|a| {
        let a = a.trim();
        if !a.starts_with("http://") {
            return None;
        }
        if a.ends_with("/v1/chat/completions") {
            return Some(a.to_string());
        }
        a.strip_suffix("/completion").map(|base| format!("{base}/v1/chat/completions"))
    })
}

/// Is `host` (`name:port` or an address) on Tailscale: its 100.64.0.0/10
/// addresses, its IPv6 prefix, or a MagicDNS name?
pub fn on_tailscale(host: &str) -> bool {
    let name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host).trim_matches(['[', ']']);
    if name.ends_with(".ts.net") {
        return true;
    }
    match name.parse::<std::net::IpAddr>() {
        Ok(std::net::IpAddr::V4(v4)) => {
            let o = v4.octets();
            o[0] == 100 && (64..128).contains(&o[1])
        }
        Ok(std::net::IpAddr::V6(v6)) => v6.segments()[..3] == [0xfd7a, 0x115c, 0xa1e0],
        Err(_) => false,
    }
}

/// What to say when the model at `host` can't be reached, in words a person
/// can act on. Over Tailscale -- a phone reaching the laptop's model -- that
/// means naming Tailscale and the laptop, not "connect 100.101.3.4:8080:
/// Connection refused (os error 111)" (28 Sep 2026).
pub fn unreachable_words(host: &str, why: &str) -> String {
    if on_tailscale(host) {
        format!(
            "I couldn't reach the model on your other computer ({host}) over Tailscale: {why}. Check that Tailscale is \
             on, on this device and on that computer, and that the computer is awake with Atlas running. I'll try \
             again with your next message."
        )
    } else {
        format!("I couldn't reach the model at {host}: {why}. I'll try again with your next message.")
    }
}

/// Chat addresses that failed, and until when they are left alone (seconds).
/// A server too old for tools, or started without `--jinja`, answers the
/// chat call with an error; the one-prompt path answers instead, and the
/// chat call isn't tried again for ten minutes rather than paying for the
/// failure on every turn.
static CHAT_OFF: std::sync::Mutex<Vec<(String, u64)>> = std::sync::Mutex::new(Vec::new());

/// How long a failed chat address is left alone.
pub const CHAT_RETRY_SECS: u64 = 600;

/// Is this chat address worth trying now?
pub fn chat_available(url: &str) -> bool {
    let now = crate::store::now();
    CHAT_OFF.lock().map(|v| !v.iter().any(|(u, until)| u == url && *until > now)).unwrap_or(true)
}

/// Note that a chat address failed.
fn chat_failed(url: &str) {
    let until = crate::store::now() + CHAT_RETRY_SECS;
    if let Ok(mut v) = CHAT_OFF.lock() {
        v.retain(|(u, _)| u != url);
        v.push((url.to_string(), until));
    }
}

/// The request body for `/v1/chat/completions`.
///
/// Same sampling as `completion_body` (`Sampling`, from the settings).
///
/// Talking always uses slot 0, which every llama-server has however many
/// slots it was started with; a call beside the conversation uses slot 1
/// (`ChatRequest::aside`). Measured live on 27 Sep 2026 (Qwen3-VL-4B,
/// b10456, two slots): left to choose, the server put a follow-up on the
/// other slot and read all 1,692 tokens again (52 s on that machine) where
/// the same slot would have read the ~300 new ones. Background calls name no
/// slot, so they take whichever is free -- usually the other one.
pub fn chat_body(req: &crate::brain::ChatRequest, stream: bool) -> String {
    use serde_json::{json, Value};
    let messages: Vec<Value> =
        req.messages.iter().map(|m| json!({"role": m.role.name(), "content": m.content})).collect();
    let mut body = json!({
        "messages": messages,
        "max_tokens": req.max_tokens.max(16),
        "stream": stream,
        "cache_prompt": true,
        // The conversation's slot, or the other one for a call beside it
        // (`ChatRequest::aside`). A server with one slot wraps 1 round to 0.
        "id_slot": if req.aside { 1 } else { 0 },
    });
    // The sampling from the settings (`Sampling`); stronger for a reply
    // asked again because the first one looped.
    if let Some(m) = body.as_object_mut() {
        m.extend(sampling_now().fields(req.stronger));
    }
    // No thinking out loud (30 Sep 2026): Qwen3 and Qwen3.5 templates think
    // before answering unless told not to, which is hundreds of tokens before
    // the first spoken word on a laptop writing 15-23 a second. Eric's
    // measurement that answered in 1-3 s sent exactly this. A template that
    // doesn't read it ignores it.
    body["chat_template_kwargs"] = json!({ "enable_thinking": false });
    if !req.tools.is_empty() {
        body["tools"] = Value::Array(req.tools.clone());
        body["tool_choice"] = json!("auto");
        // A call required: llama.cpp (b10456, Qwen3-VL, its own template or
        // Atlas's) doesn't hold the model to `tool_choice: "required"` --
        // measured 30 Sep 2026, "tell me a joke" with a call required came
        // back a joke, and every forced retry that evening came back words
        // ("Your last YouTube video got 12K views"). A JSON schema is held
        // to: the reply is `{"name": <one of the tools>, "arg": "..."}`,
        // made a tool call by `forced_call`.
        if req.force_tool {
            let names: Vec<Value> = req
                .tools
                .iter()
                .filter_map(|t| t.pointer("/function/name").cloned())
                .collect();
            body["response_format"] = json!({
                "type": "json_schema",
                "json_schema": { "name": "call", "schema": {
                    "type": "object",
                    "properties": { "name": { "type": "string", "enum": names }, "arg": { "type": "string" } },
                    "required": ["name", "arg"]
                }}
            });
        }
        // Which tools are the same every turn, for Atlas's own template
        // (`tools_late_template`); any other template never reads it.
        if req.stable_tools > 0 && req.stable_tools < req.tools.len() {
            body["chat_template_kwargs"][STABLE_TOOLS_KWARG] = json!(req.stable_tools);
        }
    }
    // No thinking out loud before answering (30 Sep 2026): Qwen3.5 and
    // Gemma 4 think by default, which spends seconds of generation on
    // words nobody hears before the first one that is spoken. A template
    // without the switch never reads it.
    if !body["chat_template_kwargs"].is_object() {
        body["chat_template_kwargs"] = json!({});
    }
    body["chat_template_kwargs"]["enable_thinking"] = json!(false);
    body.to_string()
}

/// A forced call's answer (`chat_body`'s schema) as a tool call: the tool's
/// name and, for a tool that takes one, its `arg`. `None` when the text isn't
/// one of the offered tools.
pub fn forced_call(text: &str, tools: &[serde_json::Value]) -> Option<crate::brain::ToolCall> {
    use serde_json::{json, Value};
    let v: Value = serde_json::from_str(text.trim()).ok()?;
    let name = v.get("name")?.as_str()?.trim().to_string();
    let spec = tools.iter().find(|t| t.pointer("/function/name").and_then(|n| n.as_str()) == Some(name.as_str()))?;
    let takes_arg = spec.pointer("/function/parameters/properties/arg").is_some();
    let arg = v.get("arg").and_then(|a| a.as_str()).unwrap_or("").trim().to_string();
    let arguments = if takes_arg { json!({ "arg": arg }) } else { json!({}) };
    Some(crate::brain::ToolCall { name, arguments })
}

/// The name the chat template knows the number of every-turn tools by.
pub const STABLE_TOOLS_KWARG: &str = "atlas_stable_tools";

/// The model's own chat template, changed in one respect: the tools picked
/// for this sentence are shown just before what you said, after the
/// conversation, instead of in the system turn at the top with the rest
/// (28 Sep 2026).
///
/// ## Why
///
/// llama.cpp reuses a prompt only up to its first changed token -- and for
/// the shipped Qwen3-VL not even a moved middle (`--cache-reuse` is turned
/// off for it: "cache_reuse is not supported by this context"). Qwen's
/// template puts every tool in the system turn, before the conversation.
/// The core tools are the same every turn; the six picked for each
/// sentence are not, so the prompt changed right after the core tools and
/// the whole conversation behind them was read again on every turn --
/// measured on the real server (llama.cpp, Qwen3-VL 2B, a 12-turn
/// conversation with three-sentence replies): 821 tokens read again a turn
/// on average; the laptop's processor reads about 32 a second (27 Sep).
/// With the picked tools after the conversation, only the last exchange,
/// the picked tools and the new sentence are new: 514 on average. Over 8
/// requests that needed a tool, the 2B model chose the same tools either
/// way, except one it got right only this way (`find_file`, not `agenda`).
///
/// ## How
///
/// The core tools stay where the model was trained to find them, and the
/// picked ones get a system turn of their own, in the same `<tools>` form,
/// right before the last user message. How many are core comes with each
/// request (`STABLE_TOOLS_KWARG`, from `ChatRequest::stable_tools`); a
/// request that doesn't say -- the phone, background work -- is rendered
/// exactly as the model's own template would.
///
/// `None` unless the template is the one this was written against and
/// tested on a real server (Qwen3-VL's): one tools loop in the system
/// turn, one user branch in the message loop. Anything else keeps the
/// model's own template, as before.
pub fn tools_late_template(original: &str) -> Option<String> {
    const TOOLS_LOOP: &str = "{%- for tool in tools %}";
    const USER_BRANCH: &str = "{%- if message.role == \"user\" %}";
    // Qwen3.5's template (30 Sep 2026) tests the user role in an `elif`: the
    // late tools go just inside that branch, before the user's words.
    const USER_ELIF: &str = "{%- elif message.role == \"user\" %}";
    let (if_form, elif_form) = (original.matches(USER_BRANCH).count(), original.matches(USER_ELIF).count());
    if original.matches(TOOLS_LOOP).count() != 1
        || if_form + elif_form != 1
        || !original.contains("<tool_call>")
        || !original.contains("<tools>")
        || original.contains(STABLE_TOOLS_KWARG)
    {
        return None;
    }
    let early = format!(
        "{{%- set atlas_early = ({k} | int) if {k} is defined else (tools | length if tools else 0) %}}\n",
        k = STABLE_TOOLS_KWARG
    );
    let late = "{%- if message.role == \"user\" and loop.last and tools and (tools | length) > atlas_early %}\n\
        {{- '<|im_start|>system\\n# Tools for this request\\n\\nYou may also call these functions, the same way:\\n<tools>' }}\n\
        {%- for tool in tools[atlas_early:] %}\n\
        {{- \"\\n\" }}\n\
        {{- tool | tojson }}\n\
        {%- endfor %}\n\
        {{- '\\n</tools><|im_end|>\\n' }}\n\
        {%- endif %}\n    ";
    let early_loop = original.replacen(TOOLS_LOOP, "{%- for tool in tools[:atlas_early] %}", 1);
    let t = if if_form == 1 {
        early_loop.replacen(USER_BRANCH, &format!("{late}{USER_BRANCH}"), 1)
    } else {
        early_loop.replacen(USER_ELIF, &format!("{USER_ELIF}\n    {late}"), 1)
    };
    Some(format!("{early}{t}"))
}

/// Where the changed template is written for the server to read, when the
/// model's own template is one `tools_late_template` knows.
fn tools_late_template_file(model: &Model) -> Option<PathBuf> {
    let t = tools_late_template(model.chat_template.as_deref()?)?;
    let dir = crate::roots::state_dir().join("model-templates");
    std::fs::create_dir_all(&dir).ok()?;
    let name: String = model.id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
    let path = dir.join(format!("{name}.jinja"));
    std::fs::write(&path, t).ok()?;
    Some(path)
}

/// A tool call being assembled from streamed pieces.
#[derive(Default)]
struct PartCall {
    name: String,
    args: String,
}

/// Reads a chat reply as it streams: server-sent events, one JSON object per
/// `data:` line. Kept apart from the socket so it can be tested on text.
#[derive(Default)]
pub struct ChatStream {
    text: String,
    calls: Vec<PartCall>,
    /// The server said it was done.
    pub done: bool,
    /// An error the server sent instead of a reply.
    pub error: Option<String>,
    /// The server's own count of what it read and wrote (`ServerTimings`).
    pub timings: Option<ServerTimings>,
}

/// What llama-server says a call cost, from the `timings` object its last
/// streamed piece carries (29 Sep 2026). The one way to tell a slow turn
/// that read its whole prompt again (a cache miss) from one that read a
/// little and wrote a lot, or one that waited behind other work: Eric's
/// laptop took 26-40 seconds a turn and nothing Atlas logged could say which.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ServerTimings {
    /// Prompt tokens reused from the slot's cache.
    pub cached: u64,
    /// Prompt tokens read this time.
    pub read: u64,
    pub read_ms: u64,
    /// Tokens written.
    pub wrote: u64,
    pub wrote_ms: u64,
}

impl ServerTimings {
    /// From the `timings` object, when it has the counts.
    fn from_json(t: &serde_json::Value) -> Option<ServerTimings> {
        let n = |k: &str| t.get(k).and_then(|v| v.as_f64()).map(|v| v.max(0.0).round() as u64);
        Some(ServerTimings {
            cached: n("cache_n").unwrap_or(0),
            read: n("prompt_n")?,
            read_ms: n("prompt_ms").unwrap_or(0),
            wrote: n("predicted_n")?,
            wrote_ms: n("predicted_ms").unwrap_or(0),
        })
    }

    /// For the log's timing line.
    pub fn line(&self) -> String {
        format!(
            "server read {} new tokens ({} from cache) in {}ms, wrote {} in {}ms",
            self.read, self.cached, self.read_ms, self.wrote, self.wrote_ms
        )
    }
}

/// The last chat call's `ServerTimings`, for the turn's timing line.
static LAST_TIMINGS: std::sync::Mutex<Option<ServerTimings>> = std::sync::Mutex::new(None);

/// What the last chat call cost, as the server counted it; taken, so the
/// next turn's line says only its own.
pub fn take_last_timings() -> Option<ServerTimings> {
    LAST_TIMINGS.lock().ok().and_then(|mut g| g.take())
}

impl ChatStream {
    /// Take one line of the stream. Returns the new words, if any.
    pub fn line(&mut self, line: &str) -> Option<String> {
        let l = line.trim();
        // A server that ignored `stream` answers with one plain JSON object.
        let data = match l.strip_prefix("data:") {
            Some(d) => d.trim(),
            None if l.starts_with('{') => l,
            None => return None,
        };
        if data == "[DONE]" {
            self.done = true;
            return None;
        }
        let v: serde_json::Value = serde_json::from_str(data).ok()?;
        if let Some(t) = v.get("timings").and_then(|t| ServerTimings::from_json(t)) {
            self.timings = Some(t);
        }
        if let Some(e) = v.get("error") {
            self.error = Some(e.get("message").and_then(|m| m.as_str()).map(str::to_string).unwrap_or_else(|| e.to_string()));
            return None;
        }
        let choice = v.get("choices").and_then(|c| c.get(0))?;
        let delta = choice.get("delta").or_else(|| choice.get("message"))?;
        if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
            for (k, c) in calls.iter().enumerate() {
                let i = c.get("index").and_then(|i| i.as_u64()).map(|i| i as usize).unwrap_or(k);
                while self.calls.len() <= i {
                    self.calls.push(PartCall::default());
                }
                if let Some(f) = c.get("function") {
                    if let Some(n) = f.get("name").and_then(|n| n.as_str()) {
                        self.calls[i].name.push_str(n);
                    }
                    match f.get("arguments") {
                        Some(serde_json::Value::String(a)) => self.calls[i].args.push_str(a),
                        Some(a) if !a.is_null() => self.calls[i].args.push_str(&a.to_string()),
                        _ => {}
                    }
                }
            }
        }
        if choice.get("finish_reason").and_then(|f| f.as_str()).is_some() && choice.get("delta").is_none() {
            self.done = true;
        }
        let piece = delta.get("content").and_then(|c| c.as_str()).unwrap_or("");
        if piece.is_empty() {
            return None;
        }
        self.text.push_str(piece);
        Some(piece.to_string())
    }

    /// Everything read so far, as a reply.
    pub fn reply(&self) -> crate::brain::ChatReply {
        let (text, mut calls) = crate::brain::inline_tool_calls(&crate::phonemodel::without_thinking(&self.text));
        for c in &self.calls {
            if c.name.trim().is_empty() {
                continue;
            }
            let arguments = serde_json::from_str(&c.args).unwrap_or(serde_json::Value::Object(Default::default()));
            calls.push(crate::brain::ToolCall { name: c.name.trim().to_string(), arguments });
        }
        crate::brain::ChatReply { text, tool_calls: calls }
    }
}

/// How long to wait for the next piece of a streamed reply. The first can be
/// slow: a long prompt on a laptop's processor.
const CHAT_READ_SECS: u64 = 180;

/// Why a chat call failed, which decides what happens next.
#[derive(Debug, Clone, PartialEq)]
pub enum ChatFail {
    /// This server can't do chat with tools (no chat endpoint, or started
    /// without `--jinja`): left alone for `CHAT_RETRY_SECS`.
    NoChat(String),
    /// Still loading its model (503 "Loading model"): wait and ask again.
    Loading(String),
    /// The prompt is bigger than its context: shorten it and ask once more.
    TooLong(String),
    /// Anything else: this turn falls back, and the next turn tries again.
    Other(String),
}

impl ChatFail {
    fn into_error(self) -> AtlasError {
        match self {
            ChatFail::NoChat(m) | ChatFail::Loading(m) | ChatFail::TooLong(m) | ChatFail::Other(m) => AtlasError::Platform(m),
        }
    }
}

/// What an error answer from the model server means.
///
/// Every answer that wasn't 200 used to switch chat off for ten minutes
/// (28 Sep 2026): a model still loading, or one long conversation, cost ten
/// minutes of the one-prompt path. Only a server that genuinely can't do
/// chat -- no endpoint, or a template/tools error -- is left alone now.
pub fn judge_chat_failure(status: u16, body: &str) -> ChatFail {
    let b = body.to_lowercase();
    let msg = if status == 0 {
        format!("the model server said: {}", body.trim().chars().take(200).collect::<String>())
    } else {
        format!("the model server answered {status}: {}", body.trim().chars().take(200).collect::<String>())
    };
    let about_chat = ["jinja", "tool", "template", "chat format", "not supported"].iter().any(|w| b.contains(w));
    let too_long = b.contains("n_ctx")
        || b.contains("context size")
        || b.contains("context length")
        || b.contains("context window")
        || b.contains("exceeds the available context")
        || b.contains("exceed_context")
        || b.contains("too many tokens")
        || (b.contains("context") && (b.contains("exceed") || b.contains("too long")));
    if status == 503 && b.contains("loading") {
        return ChatFail::Loading(msg);
    }
    if too_long && (status == 400 || status == 0 || status == 413 || status == 500) {
        return ChatFail::TooLong(msg);
    }
    if status == 404 || status == 405 {
        return ChatFail::NoChat(msg);
    }
    if matches!(status, 0 | 400 | 500 | 501) && about_chat {
        return ChatFail::NoChat(msg);
    }
    ChatFail::Other(msg)
}

/// The request made to fit: the earlier conversation dropped, then -- if
/// that isn't enough -- the middle of the longest message cut out. `None`
/// when there is nothing left to shorten.
fn shortened_request(req: &crate::brain::ChatRequest) -> Option<crate::brain::ChatRequest> {
    use crate::brain::{Msg, Role};
    let mut out = req.clone();
    let first_system = req.messages.first().filter(|m| m.role == Role::System).cloned();
    let last = req.messages.last().cloned()?;
    if req.messages.len() > 2 || (req.messages.len() == 2 && first_system.is_none()) {
        let mut m: Vec<Msg> = first_system.into_iter().collect();
        m.push(last);
        out.messages = m;
        return Some(out);
    }
    let (i, longest) = out.messages.iter().enumerate().max_by_key(|(_, m)| m.content.len())?;
    if longest.content.chars().count() < 2000 {
        return None;
    }
    let short = crate::brain::head_and_tail(&longest.content, longest.content.chars().count() / 2);
    out.messages[i].content = short;
    Some(out)
}

/// How long to wait for a model server that says it is still loading.
const LOADING_WAIT_SECS: u64 = 90;

/// One chat call, streamed. `on_text` gets the words as they arrive and
/// returns `false` to stop: the connection is closed, and the server stops
/// generating when it notices.
pub fn chat_call(
    url: &str,
    req: &crate::brain::ChatRequest,
    on_text: &mut dyn FnMut(&str) -> bool,
) -> Result<crate::brain::ChatReply> {
    chat_call_until(url, req, on_text, &|| true)
}

/// `chat_call`, stopped as soon as `keep_going` says so (30 Sep 2026): asked
/// a few times a second while nothing arrives -- the server still reading the
/// prompt -- as well as between words. The deep model gives way to a turn
/// this way even while it reads a long prompt, when no word comes for
/// `on_text` to stop at (`deepbrain`). What came before the stop is the reply.
pub fn chat_call_until(
    url: &str,
    req: &crate::brain::ChatRequest,
    on_text: &mut dyn FnMut(&str) -> bool,
    keep_going: &dyn Fn() -> bool,
) -> Result<crate::brain::ChatReply> {
    let started = std::time::Instant::now();
    let mut req = req.clone();
    let mut shortened = false;
    loop {
        match chat_call_once(url, &req, on_text, keep_going) {
            Ok(r) => return Ok(r),
            Err(ChatFail::NoChat(m)) => {
                chat_failed(url);
                return Err(AtlasError::Platform(m));
            }
            Err(ChatFail::Loading(_)) if started.elapsed().as_secs() < LOADING_WAIT_SECS => {
                std::thread::sleep(std::time::Duration::from_secs(2));
            }
            Err(ChatFail::TooLong(m)) if !shortened => match shortened_request(&req) {
                Some(r) => {
                    req = r;
                    shortened = true;
                }
                None => return Err(AtlasError::Platform(m)),
            },
            Err(f) => return Err(f.into_error()),
        }
    }
}

/// One attempt at a chat call. Nothing is passed to `on_text` before the
/// server has answered 200, so a retry never repeats words.
fn chat_call_once(
    url: &str,
    req: &crate::brain::ChatRequest,
    on_text: &mut dyn FnMut(&str) -> bool,
    keep_going: &dyn Fn() -> bool,
) -> std::result::Result<crate::brain::ChatReply, ChatFail> {
    match chat_call_io(url, req, on_text, keep_going) {
        Ok(r) => r,
        Err(e) => Err(ChatFail::Other(e.to_string())),
    }
}

/// How often a read with nothing arriving stops to ask `keep_going`.
const READ_SLICE_MS: u64 = 200;

/// One read, in slices of `READ_SLICE_MS`, asking `keep_going` between them.
/// `Ok(None)`: told to stop. A read that times out altogether (nothing for
/// `timeout`) is the error it always was.
fn read_or_stop(
    s: &mut std::net::TcpStream,
    buf: &mut [u8],
    keep_going: &dyn Fn() -> bool,
    timeout: std::time::Duration,
) -> std::io::Result<Option<usize>> {
    use std::io::Read;
    let began = std::time::Instant::now();
    loop {
        match s.read(buf) {
            Ok(n) => return Ok(Some(n)),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {
                if !keep_going() {
                    return Ok(None);
                }
                if began.elapsed() >= timeout {
                    return Err(e);
                }
            }
            Err(e) => return Err(e),
        }
    }
}

fn chat_call_io(
    url: &str,
    req: &crate::brain::ChatRequest,
    on_text: &mut dyn FnMut(&str) -> bool,
    keep_going: &dyn Fn() -> bool,
) -> Result<std::result::Result<crate::brain::ChatReply, ChatFail>> {
    use std::io::Write;
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| AtlasError::Platform(format!("not a plain http address: {url}")))?;
    let (host, path) = match rest.find('/') {
        Some(k) => (&rest[..k], &rest[k..]),
        None => (rest, "/"),
    };
    let host = if host.contains(':') { host.to_string() } else { format!("{host}:80") };
    // One more try after a moment: a phone that has just woken, or whose
    // Tailscale is reconnecting, often can't reach the laptop on the first
    // attempt and can on the second.
    let reach = || -> std::result::Result<std::net::TcpStream, String> {
        use std::net::ToSocketAddrs;
        let addr = host
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
            .ok_or_else(|| "its name couldn't be looked up".to_string())?;
        std::net::TcpStream::connect_timeout(&addr, std::time::Duration::from_secs(5)).map_err(|e| e.to_string())
    };
    let timeout = std::time::Duration::from_secs(CHAT_READ_SECS);
    let mut s = match reach() {
        Ok(s) => s,
        Err(_) => {
            std::thread::sleep(std::time::Duration::from_secs(1));
            reach().map_err(|why| AtlasError::Platform(unreachable_words(&host, &why)))?
        }
    };
    s.set_read_timeout(Some(std::time::Duration::from_millis(READ_SLICE_MS)))?;
    s.set_write_timeout(Some(timeout))?;
    let body = chat_body(req, true);
    s.write_all(crate::http::build_request("POST", &host, path, Some(&body)).as_bytes())?;

    let mut raw: Vec<u8> = Vec::new();
    let mut buf = [0u8; 4096];
    // The headers.
    let head_end = loop {
        if let Some(i) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
            break i;
        }
        // Told to stop before the server has answered at all (still reading
        // the prompt): nothing was said, and the connection is closed.
        let Some(n) = read_or_stop(&mut s, &mut buf, keep_going, timeout)? else {
            return Ok(Ok(crate::brain::ChatReply::default()));
        };
        if n == 0 {
            return Err(AtlasError::Platform("the model server closed the connection".into()));
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > 64 * 1024 {
            return Err(AtlasError::Platform("the model server sent no end to its headers".into()));
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let status: u16 = head.lines().next().and_then(|l| l.split_whitespace().nth(1)).and_then(|c| c.parse().ok()).unwrap_or(0);
    let chunked = head.to_lowercase().contains("transfer-encoding: chunked");
    let error_len: Option<usize> = head
        .lines()
        .find_map(|l| l.to_lowercase().strip_prefix("content-length:").map(|v| v.trim().to_string()))
        .and_then(|v| v.parse().ok());
    let mut pending: Vec<u8> = raw[head_end + 4..].to_vec();
    let mut body_bytes: Vec<u8> = Vec::new();
    let mut stream = ChatStream::default();
    let mut stopped = false;
    let mut eof = false;
    loop {
        // Move what has arrived into the body, undoing chunking.
        if chunked {
            loop {
                let Some(eol) = pending.windows(2).position(|w| w == b"\r\n") else { break };
                let size_txt = String::from_utf8_lossy(&pending[..eol]).to_string();
                let Ok(n) = usize::from_str_radix(size_txt.trim().split(';').next().unwrap_or("").trim(), 16) else {
                    eof = true;
                    break;
                };
                if n == 0 {
                    eof = true;
                    break;
                }
                if pending.len() < eol + 2 + n + 2 {
                    break;
                }
                body_bytes.extend_from_slice(&pending[eol + 2..eol + 2 + n]);
                pending.drain(..eol + 2 + n + 2);
            }
        } else {
            body_bytes.append(&mut pending);
        }
        // Whole lines.
        while status == 200 {
            let Some(nl) = body_bytes.iter().position(|b| *b == b'\n') else { break };
            let line: Vec<u8> = body_bytes.drain(..=nl).collect();
            let line = String::from_utf8_lossy(&line).to_string();
            if let Some(piece) = stream.line(&line) {
                if !on_text(&piece) {
                    stopped = true;
                    break;
                }
            }
        }
        if stopped || eof || stream.done {
            break;
        }
        // An error answer with a length is read to its end, not until the
        // server closes (which a kept-alive connection never does).
        if status != 200 && !chunked && error_len.is_some_and(|l| body_bytes.len() >= l) {
            break;
        }
        let Some(n) = read_or_stop(&mut s, &mut buf, keep_going, timeout)? else {
            stopped = true;
            break;
        };
        if n == 0 {
            break;
        }
        pending.extend_from_slice(&buf[..n]);
    }
    // The server answered, and not with a reply. What it means is judged
    // from the answer (`judge_chat_failure`): only a server that can't do
    // chat at all is left alone for a while.
    if status != 200 {
        let rest = String::from_utf8_lossy(&body_bytes).to_string();
        return Ok(Err(judge_chat_failure(status, &rest)));
    }
    // What's left without a newline at the end.
    if !stopped && !body_bytes.is_empty() {
        let line = String::from_utf8_lossy(&body_bytes).to_string();
        if let Some(piece) = stream.line(&line) {
            on_text(&piece);
        }
    }
    if let Some(e) = stream.error.take() {
        return Ok(Err(judge_chat_failure(0, &e)));
    }
    // Ended without the server saying it was done, and not because it was
    // asked to stop: the connection dropped partway, and what came is not
    // the whole reply (28 Sep 2026: it was taken as one).
    if !stopped && !stream.done {
        return Ok(Err(ChatFail::Other("the model server stopped partway through its reply".into())));
    }
    if let Some(t) = stream.timings {
        if let Ok(mut g) = LAST_TIMINGS.lock() {
            *g = Some(t);
        }
    }
    Ok(Ok(stream.reply()))
}

#[cfg(test)]
mod eyes_tests {
    use super::*;

    #[test]
    fn a_model_name_without_its_quantisation() {
        assert_eq!(without_quant("Qwen3VL-4B-Instruct-Q4_K_M"), "qwen3vl-4b-instruct");
        assert_eq!(without_quant("Qwen3VL-4B-Instruct-Q8_0"), "qwen3vl-4b-instruct");
        assert_eq!(without_quant("Qwen_Qwen3.5-9B-IQ4_XS"), "qwen_qwen3.5-9b");
        assert_eq!(without_quant("Qwen_Qwen3.5-4B-f16"), "qwen_qwen3.5-4b");
        assert_eq!(without_quant("plain-model"), "plain-model");
    }

    fn model_at(path: &Path) -> Model {
        Model {
            path: path.to_path_buf(),
            id: path.file_stem().unwrap().to_string_lossy().to_string(),
            architecture: "qwen3vl".into(),
            quant: "Q4_K_M".into(),
            parameters: 4_000_000_000,
            weight_bytes: 2_500_000_000,
            max_context: 8192,
            chat_template: None,
        }
    }

    #[test]
    fn the_talking_model_is_started_with_its_own_picture_encoder() {
        let dir = std::env::temp_dir().join(format!("atlas-eyes-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let talk = dir.join("Qwen3VL-4B-Instruct-Q4_K_M.gguf");
        let eyes = dir.join("mmproj-Qwen3VL-4B-Instruct-Q8_0.gguf");
        let other = dir.join("mmproj-SomethingElse-F16.gguf");
        for p in [&talk, &eyes, &other] {
            std::fs::write(p, b"x").unwrap();
        }
        let m = model_at(&talk);
        assert_eq!(m.projector(), Some(eyes.clone()));
        let cfg = ModelsConfig::default();
        assert_eq!(projector_args(&m, &cfg), vec!["--mmproj".to_string(), eyes.display().to_string()]);
        let off = ModelsConfig { see_with_talking_model: false, ..ModelsConfig::default() };
        assert!(projector_args(&m, &off).is_empty());
        // The deep model has no encoder beside it: nothing added.
        let deep = model_at(&dir.join("Qwen_Qwen3.5-9B-IQ4_XS.gguf"));
        assert!(projector_args(&deep, &cfg).is_empty());
        // The encoder's memory is counted in what the server needs.
        assert!(footprint_mb(&m, &cfg) > footprint_mb(&m, &off));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
