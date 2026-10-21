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
        self.architecture.eq_ignore_ascii_case("clip")
            || self.id.to_lowercase().starts_with("mmproj")
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
                let Some(stem) = p.file_stem().map(|s| s.to_string_lossy().to_lowercase()) else {
                    return false;
                };
                p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("gguf"))
                    && stem
                        .strip_prefix("mmproj-")
                        .is_some_and(|rest| without_quant(rest) == want)
            })
            .collect();
        found.sort();
        found.into_iter().next()
    }

    /// `inspect`, remembered for as long as the file is unchanged (same
    /// size, same modified time). Reading a GGUF's header walks its whole
    /// vocabulary -- a hundred thousand strings or more per model -- and
    /// "which model" read every model in the folder each time it was asked
    /// (self-test, 4 Oct 2026: 7.5 seconds on the laptop).
    fn inspect_kept(path: &Path) -> Result<Model> {
        type Kept = std::collections::HashMap<PathBuf, (u64, Option<std::time::SystemTime>, Model)>;
        static KEPT: std::sync::Mutex<Option<Kept>> = std::sync::Mutex::new(None);
        let meta = std::fs::metadata(path).ok();
        let stamp = meta.as_ref().map(|m| (m.len(), m.modified().ok()));
        if let Some((len, when)) = stamp {
            let kept = KEPT.lock().unwrap_or_else(|p| p.into_inner());
            if let Some((l, w, m)) = kept.as_ref().and_then(|k| k.get(path)) {
                if *l == len && *w == when {
                    return Ok(m.clone());
                }
            }
        }
        let m = Model::inspect(path)?;
        if let Some((len, when)) = stamp {
            KEPT.lock()
                .unwrap_or_else(|p| p.into_inner())
                .get_or_insert_with(Default::default)
                .insert(path.to_path_buf(), (len, when, m.clone()));
        }
        Ok(m)
    }

    fn inspect(path: &Path) -> Result<Model> {
        let g = Gguf::open(path)?;
        Ok(Model {
            id: path
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default(),
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
    /// Not a setting: set by Atlas for a start made in safe mode after the
    /// graphics reset (`gpulost`), never read from your settings.
    #[serde(skip)]
    pub gpu_safe: bool,
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
    /// Muse Spark, for whoever connects their own Meta key (`muse`): the
    /// model, the monthly limit Atlas keeps, Meta's prices and the effort.
    pub muse: crate::muse::MuseConfig,
    /// The talking model sees pictures itself (Eric, 1 Oct 2026, item 79).
    /// When the talking model is a picture model (the shipped Qwen3-VL 4B)
    /// and its picture encoder (`mmproj-…`) is beside it, the server is
    /// started with that encoder and "look at me" / "look at my screen" are
    /// asked of the model already running: about 0.45 GB more, instead of a
    /// second 3.6 GB copy of the same model started for every look -- which
    /// is what "the picture reader needs about 3.5 GB and only 2.7 GB is
    /// free" was. `false`: the server is started without it, as before.
    pub see_with_talking_model: bool,
    /// The model code is written with (2 Oct 2026, `coder`): empty -- the
    /// largest Qwen2.5-Coder (7B, else 1.5B) whose file is here and that this
    /// device has room for; `off` -- none, code goes to the usual writers;
    /// or a model's file name in `dir`. Run as its own server while a build
    /// needs it, the talking model let go when both don't fit.
    pub coder: String,
    /// The coding model's server port. 0: the talking model's port plus two.
    pub coder_port: u16,
    /// How long the coding model stays loaded after the last code call, in
    /// seconds, before the talking model gets its room back.
    pub coder_idle_secs: u64,
    /// The coding model's context, in tokens. 0: `coder::CONTEXT_DEFAULT`.
    pub coder_context: u64,
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
            json!(if stronger {
                self.stronger_presence_penalty.max(self.presence_penalty)
            } else {
                self.presence_penalty
            }),
        );
        m.insert("repeat_penalty".into(), json!(self.repeat_penalty));
        m.insert(
            "dry_multiplier".into(),
            json!(if stronger {
                self.stronger_dry_multiplier.max(self.dry_multiplier)
            } else {
                self.dry_multiplier
            }),
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
    SAMPLING
        .read()
        .ok()
        .and_then(|g| g.clone())
        .unwrap_or_default()
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
            gpu_safe: false,
            talk_ceiling_b: 5,
            draft: String::new(),
            // Measured on Eric's laptop, 1 Oct 2026 (Qwen3-VL 4B, five
            // everyday prompts each): no speculation 22.2 words-pieces a
            // second, `ngram-mod` 23.9 (faster on every prompt, no extra
            // memory), the Qwen3-0.6B draft model 5.7 -- four times
            // *slower*: it guessed right 8-45% of the time and every miss
            // costs a check. So guessing from what's already been said is
            // on by default, and the draft model is not.
            speculate: "ngram-mod".into(),
            sampling: Sampling::default(),
            talk: String::new(),
            deep: String::new(),
            deep_port: 0,
            deep_idle_secs: 300,
            deep_context: 8192,
            online_second: true,
            muse: crate::muse::MuseConfig::default(),
            see_with_talking_model: true,
            coder: String::new(),
            coder_port: 0,
            // Short: the talking model waits for its room back.
            coder_idle_secs: 90,
            coder_context: 0,
        }
    }
}

/// The talking model's file name for `models.talk`: `better` and `faster`
/// named, anything else taken as a file name; `None` for empty (30 Sep 2026).
/// The helper model's id: drafts for speculative decoding, never talking.
pub const DRAFT_ONLY: &str = "Qwen3-0.6B-Q8_0";

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
                        match Model::inspect_kept(&p) {
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

        models.sort_by_key(|b| std::cmp::Reverse(b.parameters));
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
        self.best_fit(
            budget_set_mb(cfg.memory_budget_mb) * 1024 * 1024,
            cfg.context,
        )
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
                        budget_set_mb(cfg.memory_budget_mb),
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
fn projector_memory(m: &Model) -> u64 {
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
    let ctx = if cfg.context == 0 {
        model.max_context
    } else {
        cfg.context.min(model.max_context)
    };
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
        //
        // 6 Oct 2026: 256. With the model on integrated graphics, the
        // server's own memory came to 5.2 GB on the laptop, and Atlas told
        // you "memory is nearly full" seconds after starting it. The two
        // slots keep their prompts in the context itself; this is only for
        // prompts pushed out of it, and one fits in 256 MB.
        "--cache-ram".into(),
        "256".into(),
        // The context's memory (keys and values) at 8 bits rather than 16:
        // half the size -- about 0.6 GB less for the 4B at 8192 -- for a
        // difference in answers too small to measure. llama.cpp turns flash
        // attention on for it; tried on the laptop's Intel Arc with this
        // build (Vulkan, b10456) on 6 Oct 2026 and it answered normally.
        "-ctk".into(),
        "q8_0".into(),
        "-ctv".into(),
        "q8_0".into(),
    ]
    .into_iter()
    // After the graphics reset under it (`gpulost`): no graphics device at
    // all -- `-ngl 0` alone still lets the backend take big prompt batches,
    // which is where the laptop's resets happened -- and the picture
    // encoder on the processor too.
    .chain(
        cfg.gpu_safe
            .then(|| ["--device", "none", "--no-mmproj-offload"].map(String::from))
            .into_iter()
            .flatten(),
    )
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
pub const NGRAM_KINDS: &[&str] = &[
    "ngram-mod",
    "ngram-simple",
    "ngram-map-k",
    "ngram-map-k4v",
    "ngram-cache",
];

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
    let p = if p.is_absolute() {
        p
    } else {
        Path::new(&cfg.dir).join(p)
    };
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
    u32::try_from((usable / per_layer).min(layers)).unwrap_or(0)
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
    at_listen_host(
        format!("http://127.0.0.1:{}/v1/chat/completions", cfg.port),
        cfg,
    )
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
pub fn completion_body(
    prompt: &str,
    template: Template,
    max_tokens: u32,
    sampling: &Sampling,
) -> String {
    let stops: Vec<serde_json::Value> = template
        .stop_tokens()
        .iter()
        .map(|s| serde_json::Value::String(s.to_string()))
        .collect();
    let mut body = serde_json::Map::new();
    body.insert(
        "prompt".into(),
        serde_json::Value::String(prompt.to_string()),
    );
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
    // A fresh log: what the last look found belonged to the old server.
    forget_log_look();
    // A new launch starts a new log: nothing in it has been handled yet.
    LOSS_HANDLED_AT.store(0, std::sync::atomic::Ordering::Relaxed);
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
    crate::childjob::tie(&child);
    // Lowered while a reply plays, so the voice keeps up (`voicefirst`).
    crate::voicefirst::model_started(child.id());
    Ok(child)
}

/// Where the model server's own messages go: `data/logs/model-server.log`,
/// started afresh each launch. Nowhere, when that can't be opened.
fn model_server_log(name: &str) -> std::process::Stdio {
    let dir = crate::roots::data_dir().join("logs");
    crate::heard!(std::fs::create_dir_all(&dir));
    // The last run's log kept beside it (`.previous`): started afresh, a
    // server that died and was restarted took the reason with it (30 Sep
    // 2026: four restarts in an hour on Eric's laptop, none explained).
    crate::kept!(std::fs::rename(
        dir.join(name),
        dir.join(format!("{name}.previous"))
    ));
    match std::fs::File::create(dir.join(name)) {
        Ok(f) => std::process::Stdio::from(f),
        Err(_) => std::process::Stdio::null(),
    }
}

/// Why the model server stopped, in its own words: the last line of
/// `data/logs/model-server.log` that says something went wrong, or its last
/// line. `None` when the log is empty or can't be read.
pub fn model_server_last_words() -> Option<String> {
    let path = crate::roots::data_dir()
        .join("logs")
        .join("model-server.log");
    let file = std::fs::File::open(path).ok()?;
    if !file.metadata().ok()?.is_file() || file.metadata().ok()?.len() > 16 * 1024 * 1024 {
        return None;
    }
    let mut text = String::new();
    use std::io::Read;
    file.take(16 * 1024 * 1024 + 1)
        .read_to_string(&mut text)
        .ok()?;
    if text.len() > 16 * 1024 * 1024 {
        return None;
    }
    last_words_in(&text)
}

/// Does the model server's log, as it stands, say its graphics device was
/// lost (`gpulost`)? The log is started afresh by each launch, so what it
/// holds is the running server's.
pub fn model_server_log_says_device_lost() -> bool {
    new_loss_in_log().is_some_and(|(lost, _)| lost)
}

/// How far into the current `model-server.log` a device loss has already
/// been handled (21 Oct 2026). The log is only replaced by the next launch,
/// so without this the same old text was found again during the restart
/// pause and the daemon restarted the server in a loop. `launch` resets it.
static LOSS_HANDLED_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Does `log`, past byte `from`, say the device was lost? Looks at no more
/// than the last 64 KB, as `gpulost::log_tail` does.
pub fn loss_after(log: &[u8], from: u64) -> bool {
    let from = usize::try_from(from).unwrap_or(usize::MAX).min(log.len());
    let start = from.max(log.len().saturating_sub(64 * 1024));
    crate::gpulost::device_lost_in(&String::from_utf8_lossy(&log[start..]))
}

/// The loss text in the current log past what was handled, and the log's
/// length now. `None` when the log can't be read.
fn new_loss_in_log() -> Option<(bool, u64)> {
    let path = crate::roots::data_dir()
        .join("logs")
        .join("model-server.log");
    let bytes = std::fs::read(path).ok()?;
    let from = LOSS_HANDLED_AT.load(std::sync::atomic::Ordering::Relaxed);
    Some((loss_after(&bytes, from), bytes.len() as u64))
}

/// The device loss in the log as it stands has been dealt with: it is not
/// reported again until the log has new loss text or a new launch.
pub fn note_loss_handled() {
    if let Some((_, len)) = new_loss_in_log() {
        LOSS_HANDLED_AT.store(len, std::sync::atomic::Ordering::Relaxed);
    }
    forget_log_look();
}

/// How often a request (or a health check) reads the log for a graphics
/// reset. Same as the daemon's own look (`MODEL_LOG_LOOK`).
const LOG_LOOK_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

/// When the log was last read for a graphics reset, and what it said.
static LOG_LOOK: std::sync::Mutex<Option<(std::time::Instant, bool)>> = std::sync::Mutex::new(None);

fn forget_log_look() {
    if let Ok(mut g) = LOG_LOOK.lock().or_else(crate::crash::unpoison) {
        *g = None;
    }
}

/// Is the server usable, given its `/health` answer and whether its log
/// shows a graphics reset (21 Oct 2026)? `/health` keeps saying ok after the
/// graphics device is lost, so an ok that sits beside the device-lost text
/// does not count.
pub fn usable_given_log(health_ok: bool, log_shows_loss: bool) -> bool {
    health_ok && !log_shows_loss
}

/// Read the server's log for a graphics reset, at most every few seconds
/// (the last answer stands in between). Found, the live flag is raised, so
/// the daemon's next pass takes the stop-and-restart-in-safe-mode path and
/// calls are refused at once rather than sent to a broken server.
pub fn log_shows_lost_graphics() -> bool {
    let Ok(mut g) = LOG_LOOK.lock().or_else(crate::crash::unpoison) else {
        return false;
    };
    if let Some((at, lost)) = *g {
        if at.elapsed() < LOG_LOOK_EVERY {
            return lost;
        }
    }
    let lost = new_loss_in_log().is_some_and(|(lost, _)| lost);
    *g = Some((std::time::Instant::now(), lost));
    if lost && !crate::gpulost::flagged() {
        crate::gpulost::flag();
    }
    lost
}

/// The telling line of a model server's log (see `model_server_last_words`).
pub fn last_words_in(text: &str) -> Option<String> {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    let bad = |l: &&&str| {
        let l = l.to_ascii_lowercase();
        l.contains("error")
            || l.contains("failed")
            || l.contains("out of memory")
            || l.contains("unable")
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
pub fn wait_until_up(cfg: &ModelsConfig, vars: &Vars, secs: u64) -> bool {
    let http = server_get();
    let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    loop {
        // A server that lost its graphics is not coming up: its replacement
        // is the daemon's to start, not a question's to wait for.
        if crate::gpulost::flagged() {
            return false;
        }
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

impl WaitsForServer {
    /// Before a call: refuse at once while the server is known to have lost
    /// its graphics (the daemon is starting a safe one; the rules-only path
    /// answers meanwhile), else wait for a server that is still loading.
    fn ready(&self) -> Result<()> {
        // Our own server's log first (21 Oct 2026): the daemon reads it only
        // between turns, and a long turn or job went on sending requests to a
        // server whose graphics were gone. Raises the flag when it finds that.
        if LAUNCHED.load(std::sync::atomic::Ordering::Relaxed) != 0 {
            // unheard-ok: returns `bool`; a finding raises the flag checked next
            let _ = log_shows_lost_graphics();
        }
        if crate::gpulost::flagged() {
            return Err(crate::gpulost::unavailable());
        }
        let launched = LAUNCHED.load(std::sync::atomic::Ordering::Relaxed);
        if launched != 0 && crate::store::now().saturating_sub(launched) < LOADING_SECS {
            // unheard-ok: returns `bool`, not a Result
            let _ = wait_until_up(&self.cfg, &self.vars, LOADING_SECS);
            // The device can be lost while we wait (21 Oct 2026).
            if crate::gpulost::flagged() {
                return Err(crate::gpulost::unavailable());
            }
        }
        Ok(())
    }

    /// After a call: an answer that says the device was lost raises the flag,
    /// so the next call doesn't wait for the same thing.
    fn noting<T>(&self, r: Result<T>) -> Result<T> {
        if let Err(e) = &r {
            if crate::gpulost::device_lost_in(&e.to_string()) {
                crate::gpulost::flag();
            }
        }
        r
    }
}

impl crate::brain::Llm for WaitsForServer {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        self.ready()?;
        self.noting(self.inner.complete(system, user))
    }

    fn complete_long(
        &self,
        system: &str,
        user: &str,
        max_tokens: u32,
    ) -> Result<crate::brain::LongReply> {
        self.ready()?;
        self.noting(self.inner.complete_long(system, user, max_tokens))
    }

    /// The server's context as the settings start it; 0 there means the
    /// model's own, which isn't known from here.
    fn context_tokens(&self) -> Option<u32> {
        let ctx = match (self.cfg.context, crate::gpulost::safe_running()) {
            (0, _) => return None,
            (c, true) => c.min(crate::gpulost::SAFE_CONTEXT),
            (c, false) => c,
        };
        Some(ctx.min(u64::from(u32::MAX)) as u32)
    }

    fn native_chat(&self) -> bool {
        self.inner.native_chat()
    }

    fn chat(
        &self,
        req: &crate::brain::ChatRequest,
        on_text: &mut dyn FnMut(&str) -> bool,
    ) -> Result<crate::brain::ChatReply> {
        self.ready()?;
        self.noting(self.inner.chat(req, on_text))
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
/// The memory limit you set, in MB. A number up to 64 can only have meant
/// gigabytes -- no model runs in 2 MB (1 Oct 2026: Eric's settings held
/// `memory_budget_mb: '2'`, and Atlas ran with no language model at all,
/// saying "there is 2MB spare" with 5 GB free).
pub fn budget_set_mb(set: u64) -> u64 {
    match set {
        1..=64 => set * 1024,
        n => n,
    }
}

pub fn budget_bytes(cfg: &ModelsConfig, m: &Machine) -> u64 {
    let measured = m.budget_mb();
    let mb = match budget_set_mb(cfg.memory_budget_mb) {
        0 => measured,
        set => set.min(measured),
    };
    mb * 1024 * 1024
}

/// The model to run: the best that fits what's free right now, else -- when
/// you haven't set a memory limit yourself -- the best that fits in half the
/// machine's memory. What's "free" on a laptop with a browser open is small,
/// and that alone left Eric's Atlas with no model at all (27 Sep 2026: every
/// question got "I can't answer that here"); Windows pages out what's idle.
pub fn pick<'a>(
    registry: &'a Registry,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
) -> Option<&'a Model> {
    // The smaller core set's tiers, when both of its talking models are here
    // (21 Oct 2026): what is available now decides 4B, 1.7B or neither.
    if let Some(tiered) = pick_by_tier(registry, cfg, m) {
        return tiered;
    }
    let chosen = pick_by_budget(registry, cfg, m);
    // The bigger talking model, when it's here and this machine has room for
    // it beside the helpers installed (2 Oct 2026).
    let helpers = crate::getpieces::helpers_resident_mb(&crate::roots::install_root());
    match registry.get(crate::deepbrain::BIGGER_TALK) {
        Some(big)
            if bigger_talk_wanted(cfg) && chosen.is_none_or(|c| c.parameters < big.parameters) =>
        {
            let projector = big
                .projector()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|x| x.len())
                .unwrap_or(0);
            let needs = bigger_talk_needs(cfg, big.weight_bytes + projector);
            // 21 Oct 2026: room_for_bigger_talk looks at the machine's total
            // memory, not what is free, so the 8B also has to leave the PC its
            // 2 GB of what is free right now (the owner's floor rule).
            let leaves_floor = m.free_ram_mb >= needs / (1024 * 1024) + FLOOR_FREE_MB;
            if leaves_floor && room_for_bigger_talk(m, cfg, helpers, needs) {
                Some(big)
            } else {
                chosen
            }
        }
        _ => chosen,
    }
}

/// The small talking model beside the 4B (`getpieces::small_talk_model`):
/// Qwen3-1.7B at 8-bit, the file name without its extension.
pub const SMALL_TALK: &str = "Qwen3-1.7B-Q8_0";

/// Memory Atlas leaves free for the PC whatever it loads (the owner's 2 GB
/// floor rule, `docs/OFFLINE_ASSISTANT_DESIGN_2026-10-20.md`).
pub const FLOOR_FREE_MB: u64 = 2048;

/// Context for the small model when memory is comfortable, and when tight.
pub const SMALL_CONTEXT: u64 = 4096;
pub const SMALL_TIGHT_CONTEXT: u64 = 2048;

/// What the memory available right now allows (21 Oct 2026, the design's RAM
/// table): the 4B from 6.5 GB; the 1.7B at 4K context from 4.5 GB; the 1.7B
/// at 2K from 3.7 GB; below that no model (the floor: phrase table and plain
/// tools, labelled online replies). Whether the chosen model also leaves
/// `FLOOR_FREE_MB` is checked against its real size (or the cost measured
/// here) in `pick_by_tier`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryTier {
    Full,
    Small,
    SmallTight,
    Floor,
}

pub fn memory_tier(available_mb: u64) -> MemoryTier {
    match available_mb {
        6656.. => MemoryTier::Full,
        4608..=6655 => MemoryTier::Small,
        3789..=4607 => MemoryTier::SmallTight,
        _ => MemoryTier::Floor,
    }
}

/// The tier one step down: 4B, then the 1.7B at 4K, then the 1.7B at 2K, then
/// the floor (no model).
fn drop_tier(t: MemoryTier) -> MemoryTier {
    match t {
        MemoryTier::Full => MemoryTier::Small,
        MemoryTier::Small => MemoryTier::SmallTight,
        MemoryTier::SmallTight | MemoryTier::Floor => MemoryTier::Floor,
    }
}

/// From `start`, the first tier whose model fits (`fits`), dropping one tier
/// at a time; the floor always "fits" (21 Oct 2026).
fn choose_tier(start: MemoryTier, fits: &dyn Fn(MemoryTier) -> bool) -> MemoryTier {
    let mut t = start;
    while t != MemoryTier::Floor && !fits(t) {
        t = drop_tier(t);
    }
    t
}

// ---------------------------------------------------------------------------
// What a model really costs on this machine (21 Oct 2026).
//
// The estimate under-counted on an integrated-graphics laptop: with 6.4 GB free
// Atlas started the 1.7B (999 layers on the graphics) and available memory fell
// to 1,160 MB, under the owner's 2 GB floor. Vulkan offload allocates shared
// system memory the server's working set does not show, beside the daemon's
// own growth and the resident hearing server. So every start is measured:
// available memory before, and the lowest over a settle window afterwards. The
// difference is the cost, kept per model file, context and graphics/processor
// setting, and the tier decision uses the larger of the estimate and it.
// ---------------------------------------------------------------------------

/// The estimate's share on top when the server offloads every layer to
/// integrated graphics (percent): their memory is the system's, and none of
/// it shows in the server's working set. A first guess until the machine has
/// measured its own (`ModelCosts`); the 1.2 factor already in the estimate
/// stays, so the first load there is 1.5 times the raw estimate.
pub const IGPU_OFFLOAD_PERCENT: u64 = 125;

/// How long after the server is ready the lowest available memory is watched
/// before the measured cost is recorded.
pub const COST_SETTLE_SECS: u64 = 30;

/// What one start was measured to cost, for one model file at one context and
/// graphics setting.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
pub struct CostRecord {
    pub key: String,
    pub cost_mb: u64,
    #[serde(default)]
    pub at: u64,
}

/// The durable record of measured costs (store name `model_costs`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, Deserialize)]
pub struct ModelCosts {
    #[serde(default)]
    pub records: Vec<CostRecord>,
}

impl ModelCosts {
    const RECORD: &'static str = "model_costs";

    pub fn load() -> ModelCosts {
        crate::roots::store().load(Self::RECORD)
    }

    /// Keep what a start was measured to cost.
    pub fn keep(key: &str, cost_mb: u64) -> Result<()> {
        let costs = ModelCosts::load().with(key, cost_mb, crate::store::now());
        crate::roots::store().save(Self::RECORD, &costs)
    }

    /// The key a measured cost is kept under: the model file, its context,
    /// and whether layers went to the graphics.
    pub fn key(model_id: &str, context: u64, offload: bool) -> String {
        format!(
            "{model_id}|ctx{context}|{}",
            if offload { "gpu" } else { "cpu" }
        )
    }

    pub fn cost_for(&self, key: &str) -> Option<u64> {
        self.records.iter().find(|r| r.key == key).map(|r| r.cost_mb)
    }

    /// The latest measurement for `key` replaces the one before it.
    pub fn with(mut self, key: &str, cost_mb: u64, at: u64) -> ModelCosts {
        self.records.retain(|r| r.key != key);
        self.records.push(CostRecord {
            key: key.to_string(),
            cost_mb,
            at,
        });
        self
    }
}

/// Whether every layer goes to integrated graphics here: layers offloaded and
/// no dedicated memory of the graphics' own to put them in.
fn integrated_offload(m: &Machine, layers: u32) -> bool {
    layers > 0 && m.usable_vram_mb() == 0
}

/// What a model is expected to take, in megabytes: the estimate plus a fifth
/// (the working set ran 14-19% over it), plus `IGPU_OFFLOAD_PERCENT` when it
/// offloads to integrated graphics.
fn estimated_need_mb(estimate_bytes: u64, integrated_offload: bool) -> u64 {
    let mb = estimate_bytes / (1024 * 1024) * 6 / 5;
    if integrated_offload {
        mb * IGPU_OFFLOAD_PERCENT / 100
    } else {
        mb
    }
}

/// The larger of the estimate and what was measured here.
fn needed_mb(estimated_mb: u64, measured_mb: Option<u64>) -> u64 {
    estimated_mb.max(measured_mb.unwrap_or(0))
}

/// What a start cost: available memory before it less the lowest reading
/// after.
pub fn measured_cost_mb(before_mb: u64, lowest_mb: u64) -> u64 {
    before_mb.saturating_sub(lowest_mb)
}

/// Whether `needed_mb` fits in `available_mb` and still leaves the PC its
/// floor (`FLOOR_FREE_MB`). Also the rule for loading the same model again:
/// not until available memory is at least its measured cost plus 2 GB.
fn fits_with_floor(available_mb: u64, needed_mb: u64) -> bool {
    available_mb >= needed_mb + FLOOR_FREE_MB
}

/// The post-load guard: the lowest available memory seen is under the floor,
/// so the server is stopped at once.
pub fn guard_trips(lowest_mb: u64) -> bool {
    lowest_mb < FLOOR_FREE_MB
}

/// The cost to keep when the guard trips: a cost already measured at the end
/// of a clean settle window stands (a game opened later is not the model's
/// doing); otherwise the difference seen.
pub fn cost_to_record(before_mb: u64, lowest_mb: u64, settled_cost_mb: Option<u64>) -> u64 {
    settled_cost_mb.unwrap_or_else(|| measured_cost_mb(before_mb, lowest_mb))
}

/// The one log line a settle or a guard trip writes, with the numbers.
pub fn guard_line(
    model_id: &str,
    before_mb: u64,
    lowest_mb: u64,
    cost_mb: u64,
    action: &str,
) -> String {
    format!(
        "model memory: {model_id}: available before the start {before_mb} MB, lowest since \
         {lowest_mb} MB, measured cost {cost_mb} MB, floor {FLOOR_FREE_MB} MB: {action}"
    )
}

/// What `model` needs free to run at `context` here, in megabytes, before the
/// PC's floor: the larger of the estimate and the measured cost.
fn need_mb(
    model: &Model,
    context: u64,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
    costs: &ModelCosts,
) -> u64 {
    let layers = layers_here(model, cfg, m);
    let estimated = estimated_need_mb(
        estimate_memory(model, context),
        integrated_offload(m, layers),
    );
    needed_mb(
        estimated,
        costs.cost_for(&ModelCosts::key(&model.id, context, layers > 0)),
    )
}

/// The small model's context at a tier: the tier's, lowered to the configured
/// one (`0` leaves the tier's).
fn small_context_at(cfg: &ModelsConfig, cap: u64) -> u64 {
    if cfg.context == 0 {
        cap
    } else {
        cfg.context.min(cap)
    }
}

/// The tier the tiers rule gives, or `None` when they do not apply (a memory
/// limit's model, a model you named, or either talking model missing).
fn tier_choice<'a>(
    registry: &'a Registry,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
    costs: &ModelCosts,
) -> Option<(MemoryTier, &'a Model, &'a Model)> {
    if !bigger_talk_wanted(cfg) {
        return None;
    }
    let four = registry.get(crate::deepbrain::FASTER_TALK)?;
    let small = registry.get(SMALL_TALK)?;
    let available_mb = tier_available_mb(cfg, m);
    // The floor rule beside the model's real size: a model that would leave
    // the PC under 2 GB is not loaded. `estimate_memory` runs about 14-19%
    // under what the server's working set came to on the laptop (21 Oct
    // 2026, Vulkan build b10456, `-ctk/-ctv q8_0`: the 1.7B at 2K 2,394 MB
    // against an estimate of 2,021; at 4K 2,517 against 2,116; the 4B at 4K
    // 3,275 against 2,882), so 20% goes on top, more on integrated graphics,
    // and never less than a start here was measured to cost.
    let fits = |t: MemoryTier| {
        let (model, ctx) = match t {
            MemoryTier::Full => (four, cfg.context),
            MemoryTier::Small => (small, small_context_at(cfg, SMALL_CONTEXT)),
            MemoryTier::SmallTight => (small, small_context_at(cfg, SMALL_TIGHT_CONTEXT)),
            MemoryTier::Floor => return true,
        };
        fits_with_floor(available_mb, need_mb(model, ctx, cfg, m, costs))
    };
    Some((choose_tier(memory_tier(available_mb), &fits), four, small))
}

/// `pick` by tier: `None` when tiers do not apply (a memory limit you set, a
/// model you named, or either talking model missing) and `Some(choice)` when
/// they do. `Full` is left to the ordinary pick, which takes the 4B (or the
/// bigger one where the machine has room).
fn pick_by_tier<'a>(
    registry: &'a Registry,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
) -> Option<Option<&'a Model>> {
    pick_by_tier_with(registry, cfg, m, &ModelCosts::load())
}

fn pick_by_tier_with<'a>(
    registry: &'a Registry,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
    costs: &ModelCosts,
) -> Option<Option<&'a Model>> {
    let (tier, _, small) = tier_choice(registry, cfg, m, costs)?;
    match tier {
        MemoryTier::Full => None,
        MemoryTier::Small | MemoryTier::SmallTight => Some(Some(small)),
        MemoryTier::Floor => Some(None),
    }
}

/// What the tiers are measured against (21 Oct 2026): the memory free right
/// now, lowered to the limit you set in `memory_budget_mb` when there is one
/// (it is a cap, never a raise). Before, a limit switched the tiers off and
/// the 4B was started whatever the limit said.
fn tier_available_mb(cfg: &ModelsConfig, m: &crate::fit::Machine) -> u64 {
    match budget_set_mb(cfg.memory_budget_mb) {
        0 => m.free_ram_mb,
        set => set.min(m.free_ram_mb),
    }
}

/// When the tiers apply and say no model fits right now: the small model and
/// what it needs free (megabytes, its working set at 2K context plus the PC's
/// 2 GB), always more than what is free, so the floor can say how much is
/// free against how much is needed and what to close. `None` when the tiers
/// do not apply or a model fits.
pub fn tier_no_fit(registry: &Registry, cfg: &ModelsConfig, m: &crate::fit::Machine) -> Option<(String, u64)> {
    let costs = ModelCosts::load();
    if !matches!(pick_by_tier_with(registry, cfg, m, &costs), Some(None)) {
        return None;
    }
    let small = registry.get(SMALL_TALK)?;
    let takes_mb = need_mb(small, small_context_at(cfg, SMALL_TIGHT_CONTEXT), cfg, m, &costs);
    let need = (takes_mb + FLOOR_FREE_MB).max(m.free_ram_mb + 1);
    Some((small.id.clone(), need))
}

/// The "language model" line of the health report (21 Oct 2026): which tier
/// is in use and why, or that there is no model right now and what it needs.
/// `None` when the tiers do not apply (the plain line is used).
pub fn tier_line(
    registry: &Registry,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
    costs: &ModelCosts,
) -> Option<String> {
    let (tier, four, small) = tier_choice(registry, cfg, m, costs)?;
    let free = tier_available_mb(cfg, m);
    let gb = |mb: u64| format!("{:.1} GB", mb as f64 / 1024.0);
    let by_table = memory_tier(free);
    // Why, when the table alone would have said a bigger one: what it needs
    // here, measured or estimated.
    let higher_needs = |model: &Model, ctx: u64| {
        format!(
            "{} was free and the bigger one needs {} free",
            gb(free),
            gb(need_mb(model, ctx, cfg, m, costs) + FLOOR_FREE_MB)
        )
    };
    let because = match tier {
        MemoryTier::Full => format!("{} was free", gb(free)),
        _ if tier == by_table => format!("{} was free", gb(free)),
        MemoryTier::Small => higher_needs(four, cfg.context),
        _ => higher_needs(small, small_context_at(cfg, SMALL_CONTEXT)),
    };
    Some(match tier {
        MemoryTier::Full => format!("4B, because {because}"),
        MemoryTier::Small => format!("1.7B, because {because}"),
        MemoryTier::SmallTight => format!("1.7B at 2K context, because {because}"),
        MemoryTier::Floor => {
            let need = need_mb(small, small_context_at(cfg, SMALL_TIGHT_CONTEXT), cfg, m, costs)
                + FLOOR_FREE_MB;
            format!(
                "no model right now: it needs {} and {} is free",
                gb(need.max(free + 1)),
                gb(free)
            )
        }
    })
}

/// The settings to start `model` with on this machine: the tier's context
/// when the small model was picked by tier (4K, or 2K when tight), the
/// configured context otherwise.
pub fn tuned_for(model: &Model, cfg: &ModelsConfig, m: &crate::fit::Machine) -> ModelsConfig {
    tuned_for_with(model, cfg, m, &ModelCosts::load())
}

fn tuned_for_with(
    model: &Model,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
    costs: &ModelCosts,
) -> ModelsConfig {
    let mut out = cfg.clone();
    if model.id == SMALL_TALK {
        let available_mb = tier_available_mb(cfg, m);
        // The 4B's tier is not this model's: it starts at the 1.7B's.
        let start = match memory_tier(available_mb) {
            MemoryTier::Full => MemoryTier::Small,
            t => t,
        };
        let tier = choose_tier(start, &|t| {
            let ctx = match t {
                MemoryTier::Small => small_context_at(cfg, SMALL_CONTEXT),
                MemoryTier::SmallTight => small_context_at(cfg, SMALL_TIGHT_CONTEXT),
                _ => return true,
            };
            fits_with_floor(available_mb, need_mb(model, ctx, cfg, m, costs))
        });
        let cap = match tier {
            MemoryTier::Small => SMALL_CONTEXT,
            _ => SMALL_TIGHT_CONTEXT,
        };
        out.context = small_context_at(cfg, cap);
    }
    out
}

/// Whether the settings leave the choice of talking model to Atlas: nothing
/// named in `prefer`, and `talk` the shipped `faster` or empty. "better" or
/// a file name is yours, and kept to.
fn bigger_talk_wanted(cfg: &ModelsConfig) -> bool {
    cfg.prefer.trim().is_empty()
        && matches!(
            talk_id(cfg).as_deref(),
            None | Some(crate::deepbrain::FASTER_TALK)
        )
}

/// The bigger talking model's cache per token of context, in bytes: Qwen3-VL
/// 8B's text model has 36 layers of 8 key/value heads of 128 (Qwen's own
/// `config.json`, read 2 Oct 2026), cached at 16 bits, keys and values.
pub const BIGGER_TALK_CACHE_PER_TOKEN: u64 = 2 * 36 * 8 * 128 * 2;

/// What the bigger talking model takes at the configured context, in bytes:
/// its files (`files_bytes`, the model and its picture encoder), its cache,
/// and a tenth on top -- the same tenth `deepbrain` allows.
pub fn bigger_talk_needs(cfg: &ModelsConfig, files_bytes: u64) -> u64 {
    let cache = BIGGER_TALK_CACHE_PER_TOKEN * cfg.context.max(2048);
    (files_bytes + cache) * 11 / 10
}

/// Can this machine talk through the bigger model (2 Oct 2026)?
///
/// The room is sized the way `pick` sizes the talking model -- to the
/// machine rather than this minute (Windows pages out what's idle), or to the
/// limit you set, never more than was measured -- but at two fifths of the
/// memory rather than half: an 8B on memory shared with the graphics leaves
/// Windows and your programs too little on a 16 GB machine, where the 4B
/// doesn't. Plus a graphics card's own
/// memory when it has enough to count (`Machine::usable_vram_mb`: integrated
/// graphics share the same memory and add nothing). From it come the helpers
/// already installed (`getpieces::helpers_resident_mb`: hearing, the voice,
/// the hands, the meaning model) and `deepbrain::HEADROOM_MB` for everything
/// else. On a 16 GB laptop with integrated graphics that leaves the 4B
/// talking; 32 GB, or a card with 8 GB of its own, has room for the 8B.
pub fn room_for_bigger_talk(
    m: &Machine,
    cfg: &ModelsConfig,
    helpers_mb: u64,
    needs_bytes: u64,
) -> bool {
    let room_mb = match budget_set_mb(cfg.memory_budget_mb) {
        0 => (m.total_ram_mb * 2 / 5).max(m.budget_mb()) + m.usable_vram_mb(),
        set => set.min(m.budget_mb()) + m.usable_vram_mb(),
    };
    let need_mb = needs_bytes / (1024 * 1024) + helpers_mb + crate::deepbrain::HEADROOM_MB;
    room_mb >= need_mb
}

/// `pick` before the bigger model is considered: the best that fits the
/// budget, sized to the machine.
fn pick_by_budget<'a>(
    registry: &'a Registry,
    cfg: &ModelsConfig,
    m: &crate::fit::Machine,
) -> Option<&'a Model> {
    let now = registry.choose_for(cfg, budget_bytes(cfg, m));
    if cfg.memory_budget_mb != 0 {
        return now;
    }
    // Sized to the machine, not to this minute: what fits in half its
    // memory, when that's more than what's free right now picks. Windows
    // pages out what's idle, and a browser open at start isn't a reason to
    // talk through the smallest model all day (1 Oct 2026: with 1.4 GB free
    // on a 16 GB laptop, Atlas chose the 0.6B model and left the 4B one --
    // there, downloaded -- unused).
    let roomy = registry.choose_for(cfg, m.total_ram_mb * 1024 * 1024 / 2);
    match (now, roomy) {
        (Some(a), Some(b)) if b.parameters > a.parameters => Some(b),
        (None, b) => b,
        (a, _) => a,
    }
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
            if let Some(m) = self
                .get(crate::deepbrain::FASTER_TALK)
                .filter(|m| estimate_memory(m, cfg.context) <= budget)
            {
                return Some(m);
            }
        }
        // The helper model (speculative drafts, `getpieces::draft_model`) is
        // never the one that talks: on the laptop 1 Oct 2026, with 2.3 GB
        // free at start, the 4B didn't fit the budget and the smallest model
        // that did -- the 0.6B helper Atlas had just fetched for itself --
        // took over the conversation. When the talking model is here it is
        // used even over budget: it runs on the graphics chip, and a 0.6B
        // answering is worse than a slower start.
        // Nor a coding model (2 Oct 2026, `coder`): it writes code, it
        // doesn't talk, and the 1.5B would otherwise win on a small budget.
        let talks = |m: &&Model| {
            m.id != DRAFT_ONLY
                && !m.id.to_lowercase().starts_with("mmproj")
                && !crate::coder::is_coder_id(&m.id)
        };
        let chosen = if cfg.talk_ceiling_b == 0 {
            self.models
                .iter()
                .filter(talks)
                .filter(|m| estimate_memory(m, cfg.context) <= budget)
                .max_by_key(|m| m.parameters)
        } else {
            let ceiling = cfg.talk_ceiling_b.saturating_mul(1_000_000_000);
            let fits = || {
                self.models
                    .iter()
                    .filter(talks)
                    .filter(|m| estimate_memory(m, cfg.context) <= budget)
            };
            fits()
                .filter(|m| m.parameters <= ceiling)
                .max_by_key(|m| m.parameters)
                .or_else(|| fits().min_by_key(|m| m.parameters))
        };
        chosen.or_else(|| {
            if cfg.prefer.is_empty() {
                self.get(crate::deepbrain::FASTER_TALK)
            } else {
                None
            }
        })
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
    if cfg.gpu_safe {
        return 0;
    }
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
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
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
    let eyes = if cfg.see_with_talking_model {
        projector_memory(model)
    } else {
        0
    };
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
            "-s",
            "-S",
            "--noproxy",
            "*",
            "-X",
            "POST",
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            "@-",
            "{url}",
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
    tool.args = tool
        .args
        .iter()
        .map(|a| a.replace("{url}", &at_listen_host(completion_url(cfg.port), cfg)))
        .collect();
    crate::brain::LlmConfig {
        tool,
        request: String::new(),
        response_path: String::new(),
        vision_request: None,
    }
    .endpoint()
}

/// A plain GET against the model server, for `/health`.
pub fn server_get() -> ExternalTool {
    ExternalTool {
        command: "curl".into(),
        args: ["-s", "--noproxy", "*", "-m", "5", "{url}"]
            .iter()
            .map(|s| s.to_string())
            .collect(),
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
        // And its log must not show a graphics reset (21 Oct 2026): a server
        // that lost its device still answers /health with ok.
        Ok(body) => {
            let ok = health_says_ok(&body);
            // The log is only read when /health said ok.
            usable_given_log(ok, ok && log_shows_lost_graphics())
        }
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
pub fn llm_config_for(
    model: &Model,
    cfg: &ModelsConfig,
    http: &ExternalTool,
) -> crate::brain::LlmConfig {
    let template = Template::detect(model.chat_template.as_deref(), &model.id);
    let prompt = no_thinking_prompt(
        template.render("{system}", "{user}"),
        model.chat_template.as_deref(),
    );
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
        registry
            .explain_for(cfg, &m, budget_bytes(cfg, &m))
            .trim()
            .trim_end_matches('.')
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
            let chosen = if up {
                registry.choose_for(mcfg, u64::MAX)
            } else {
                pick(&registry, mcfg, &crate::fit::measure())
            };
            chosen.map(|model| {
                crate::models::llm_config_for(model, mcfg, &crate::models::server_post())
            })
        }
    };
    let local = tc.llm.as_ref().or(derived.as_ref()).map(|lc| {
        std::sync::Arc::new(crate::brain::ShellLlm {
            cfg: lc.clone(),
            vars: tc.vars.clone(),
        }) as std::sync::Arc<dyn crate::brain::Llm>
    });
    // No model on this machine: the free online ones are the only ones, when
    // they're allowed (`models.online_second`).
    let Some(primary) = local else {
        return tc.models.online_second.then(|| {
            std::sync::Arc::new(crate::freeonline::FreeOnline::new())
                as std::sync::Arc<dyn crate::brain::Llm>
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
            std::sync::Arc::new(crate::brain::ShellLlm {
                cfg: lc.clone(),
                vars: tc.vars.clone(),
            }) as std::sync::Arc<dyn crate::brain::Llm>
        })
        // Your own second model first; otherwise the free online ones.
        .or_else(|| {
            tc.models.online_second.then(|| {
                std::sync::Arc::new(crate::freeonline::FreeOnline::new())
                    as std::sync::Arc<dyn crate::brain::Llm>
            })
        });
    let both = crate::brain::FallbackLlm::new(primary, secondary);
    let both = if own_second {
        both.secondary_is_your_own()
    } else {
        both
    };
    // Muse Spark first for the hard work while someone has connected it and
    // it's under their monthly limit; otherwise exactly the above.
    Some(std::sync::Arc::new(crate::muse::MuseFirst {
        muse: std::sync::Arc::new(crate::muse::MuseSpark::new(tc.models.muse.clone())),
        rest: std::sync::Arc::new(both),
    }) as std::sync::Arc<dyn crate::brain::Llm>)
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
    cfg.tool
        .args
        .iter()
        .chain(std::iter::once(&cfg.tool.command))
        .find_map(|a| {
            let a = a.trim();
            if !a.starts_with("http://") {
                return None;
            }
            if a.ends_with("/v1/chat/completions") {
                return Some(a.to_string());
            }
            a.strip_suffix("/completion")
                .map(|base| format!("{base}/v1/chat/completions"))
        })
}

/// Is `host` (`name:port` or an address) on Tailscale: its 100.64.0.0/10
/// addresses, its IPv6 prefix, or a MagicDNS name?
pub fn on_tailscale(host: &str) -> bool {
    let name = host
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(host)
        .trim_matches(['[', ']']);
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
        format!(
            "I couldn't reach the model at {host}: {why}. I'll try again with your next message."
        )
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
    CHAT_OFF
        .lock()
        .or_else(crate::crash::unpoison)
        .map(|v| !v.iter().any(|(u, until)| u == url && *until > now))
        .unwrap_or(true)
}

/// Note that a chat address failed.
fn chat_failed(url: &str) {
    let until = crate::store::now() + CHAT_RETRY_SECS;
    if let Ok(mut v) = CHAT_OFF.lock().or_else(crate::crash::unpoison) {
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
    let messages: Vec<Value> = req
        .messages
        .iter()
        .map(|m| json!({"role": m.role.name(), "content": m.content}))
        .collect();
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
    if req.tools.is_empty() {
        if let Some(schema) = &req.output_schema {
            body["response_format"] = json!({"type":"json_schema", "json_schema":{"name":"structured_reply", "schema":schema}});
        }
    }
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
    let spec = tools
        .iter()
        .find(|t| t.pointer("/function/name").and_then(|n| n.as_str()) == Some(name.as_str()))?;
    let takes_arg = spec
        .pointer("/function/parameters/properties/arg")
        .is_some();
    let arg = v
        .get("arg")
        .and_then(|a| a.as_str())
        .unwrap_or("")
        .trim()
        .to_string();
    let arguments = if takes_arg {
        json!({ "arg": arg })
    } else {
        json!({})
    };
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
    let (if_form, elif_form) = (
        original.matches(USER_BRANCH).count(),
        original.matches(USER_ELIF).count(),
    );
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
    let name: String = model
        .id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
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
        let n = |k: &str| {
            t.get(k)
                .and_then(|v| v.as_f64())
                .map(|v| v.max(0.0).round() as u64)
        };
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
    LAST_TIMINGS
        .lock()
        .or_else(crate::crash::unpoison)
        .ok()
        .and_then(|mut g| g.take())
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
            self.error = Some(
                e.get("message")
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
                    .unwrap_or_else(|| e.to_string()),
            );
            return None;
        }
        let choice = v.get("choices").and_then(|c| c.get(0))?;
        let delta = choice.get("delta").or_else(|| choice.get("message"))?;
        if let Some(calls) = delta.get("tool_calls").and_then(|c| c.as_array()) {
            for (k, c) in calls.iter().enumerate() {
                let i = c
                    .get("index")
                    .and_then(|i| i.as_u64())
                    .map(|i| i as usize)
                    .unwrap_or(k);
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
        if choice
            .get("finish_reason")
            .and_then(|f| f.as_str())
            .is_some()
        {
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
        let (text, mut calls) =
            crate::brain::inline_tool_calls(&crate::phonemodel::without_thinking(&self.text));
        for c in &self.calls {
            if c.name.trim().is_empty() {
                continue;
            }
            let arguments = serde_json::from_str(&c.args)
                .unwrap_or(serde_json::Value::Object(Default::default()));
            calls.push(crate::brain::ToolCall {
                name: c.name.trim().to_string(),
                arguments,
            });
        }
        crate::brain::ChatReply {
            text,
            tool_calls: calls,
        }
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
    Stopped,
}

impl ChatFail {
    fn into_error(self) -> AtlasError {
        match self {
            ChatFail::NoChat(m)
            | ChatFail::Loading(m)
            | ChatFail::TooLong(m)
            | ChatFail::Other(m) => AtlasError::Platform(m),
            ChatFail::Stopped => stopped_chat(),
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
        format!(
            "the model server said: {}",
            body.trim().chars().take(200).collect::<String>()
        )
    } else {
        format!(
            "the model server answered {status}: {}",
            body.trim().chars().take(200).collect::<String>()
        )
    };
    let about_chat = ["jinja", "tool", "template", "chat format", "not supported"]
        .iter()
        .any(|w| b.contains(w));
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
    let first_system = req
        .messages
        .first()
        .filter(|m| m.role == Role::System)
        .cloned();
    let last = req.messages.last().cloned()?;
    if req.messages.len() > 2 || (req.messages.len() == 2 && first_system.is_none()) {
        let mut m: Vec<Msg> = first_system.into_iter().collect();
        m.push(last);
        out.messages = m;
        return Some(out);
    }
    let (i, longest) = out
        .messages
        .iter()
        .enumerate()
        .max_by_key(|(_, m)| m.content.len())?;
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

#[derive(Debug)]
struct ChatStopped;
impl std::fmt::Display for ChatStopped {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("model request stopped before completion")
    }
}
impl std::error::Error for ChatStopped {}
fn stopped_chat() -> AtlasError {
    std::io::Error::new(std::io::ErrorKind::Interrupted, ChatStopped).into()
}
pub(crate) fn chat_was_stopped(error: &AtlasError) -> bool {
    matches!(error, AtlasError::Io(error) if error.get_ref().is_some_and(|cause| cause.is::<ChatStopped>()))
}
struct ResolveChat {
    host: String,
    cancelled: std::sync::Arc<std::sync::atomic::AtomicBool>,
    reply: std::sync::mpsc::SyncSender<std::result::Result<Vec<std::net::SocketAddr>, String>>,
}
struct CancelResolution(std::sync::Arc<std::sync::atomic::AtomicBool>);
impl Drop for CancelResolution {
    fn drop(&mut self) {
        self.0.store(true, std::sync::atomic::Ordering::Release);
    }
}
fn wait_chat_resolution(
    reply: &std::sync::mpsc::Receiver<std::result::Result<Vec<std::net::SocketAddr>, String>>,
    keep: &dyn Fn() -> bool,
) -> Result<Vec<std::net::SocketAddr>> {
    let began = std::time::Instant::now();
    loop {
        if !keep() {
            return Err(stopped_chat());
        }
        if began.elapsed() >= std::time::Duration::from_secs(5) {
            return Err(AtlasError::Platform(
                "model endpoint name lookup timed out".into(),
            ));
        }
        match reply.recv_timeout(std::time::Duration::from_millis(25)) {
            Ok(addresses) => {
                if !keep() {
                    return Err(stopped_chat());
                }
                return addresses.map_err(AtlasError::Platform);
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => {
                return Err(AtlasError::Platform(
                    "model endpoint resolver stopped".into(),
                ))
            }
        }
    }
}
fn resolve_chat_until(host: &str, keep: &dyn Fn() -> bool) -> Result<Vec<std::net::SocketAddr>> {
    if !keep() {
        return Err(stopped_chat());
    }
    if let Ok(address) = host.parse() {
        return Ok(vec![address]);
    }
    if let Some((name, port)) = host.rsplit_once(':') {
        if name.eq_ignore_ascii_case("localhost") {
            return Ok(vec![std::net::SocketAddr::from((
                [127, 0, 0, 1],
                port.parse::<u16>()
                    .map_err(|_| AtlasError::Platform("invalid model port".into()))?,
            ))]);
        }
    }
    // At most one native DNS call runs, with at most two pending requests.
    // A slow OS resolver cannot create an unbounded set of abandoned threads.
    static RESOLVER: std::sync::OnceLock<
        std::result::Result<std::sync::mpsc::SyncSender<ResolveChat>, String>,
    > = std::sync::OnceLock::new();
    let resolver = RESOLVER
        .get_or_init(|| {
            let (send, receive) = std::sync::mpsc::sync_channel::<ResolveChat>(2);
            std::thread::Builder::new()
                .name("atlas-model-dns".into())
                .spawn(move || {
                    use std::net::ToSocketAddrs;
                    while let Ok(job) = receive.recv() {
                        if job.cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            continue;
                        }
                        let answer = job
                            .host
                            .to_socket_addrs()
                            .map(|addresses| addresses.take(8).collect())
                            .map_err(|error| error.to_string());
                        if !job.cancelled.load(std::sync::atomic::Ordering::Acquire) {
                            crate::heard!(job.reply.try_send(answer));
                        }
                    }
                })
                .map(|_| send)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(|error| {
            AtlasError::Platform(format!("model endpoint resolver unavailable: {error}"))
        })?;
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let _cancel = CancelResolution(cancelled.clone());
    let (send, reply) = std::sync::mpsc::sync_channel(1);
    resolver
        .try_send(ResolveChat {
            host: host.into(),
            cancelled,
            reply: send,
        })
        .map_err(|_| {
            AtlasError::Platform("model endpoint resolver is busy; request was not sent".into())
        })?;
    wait_chat_resolution(&reply, keep)
}
fn pause_chat_until(duration: std::time::Duration, keep: &dyn Fn() -> bool) -> Result<()> {
    let began = std::time::Instant::now();
    while began.elapsed() < duration {
        if !keep() {
            return Err(stopped_chat());
        }
        std::thread::sleep(
            std::time::Duration::from_millis(25).min(duration.saturating_sub(began.elapsed())),
        );
    }
    if !keep() {
        return Err(stopped_chat());
    }
    Ok(())
}
fn connect_chat_until(
    addresses: &[std::net::SocketAddr],
    keep: &dyn Fn() -> bool,
) -> Result<std::net::TcpStream> {
    let began = std::time::Instant::now();
    let mut last = "no addresses were found".to_string();
    for address in addresses {
        loop {
            if !keep() {
                return Err(stopped_chat());
            }
            let left = std::time::Duration::from_secs(5).saturating_sub(began.elapsed());
            if left.is_zero() {
                return Err(AtlasError::Platform("model connection timed out".into()));
            }
            match std::net::TcpStream::connect_timeout(
                address,
                std::time::Duration::from_millis(200).min(left),
            ) {
                Ok(stream) => {
                    if !keep() {
                        return Err(stopped_chat());
                    }
                    return Ok(stream);
                }
                Err(error) => {
                    last = error.to_string();
                    if matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    ) {
                        continue;
                    }
                    break;
                }
            }
        }
    }
    Err(AtlasError::Platform(last))
}
fn write_chat_until(
    stream: &mut std::net::TcpStream,
    request: &[u8],
    keep: &dyn Fn() -> bool,
) -> Result<()> {
    use std::io::Write;
    if request.len() > 2 * 1024 * 1024 {
        return Err(AtlasError::Platform(
            "model request exceeds the bounded transport limit".into(),
        ));
    }
    stream.set_write_timeout(Some(std::time::Duration::from_millis(200)))?;
    let began = std::time::Instant::now();
    let mut written = 0;
    while written < request.len() {
        if !keep() {
            return Err(stopped_chat());
        }
        if began.elapsed() >= std::time::Duration::from_secs(CHAT_READ_SECS) {
            return Err(AtlasError::Platform("model request write timed out".into()));
        }
        match stream.write(&request[written..(written + 16 * 1024).min(request.len())]) {
            Ok(0) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::WriteZero,
                    "model socket stopped accepting the request",
                )
                .into())
            }
            Ok(n) => written += n,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut
                        | std::io::ErrorKind::WouldBlock
                        | std::io::ErrorKind::Interrupted
                ) => {}
            Err(error) => return Err(error.into()),
        }
    }
    if !keep() {
        return Err(stopped_chat());
    }
    Ok(())
}

/// `chat_call`, stopped as soon as `keep_going` says so (30 Sep 2026): asked
/// a few times a second while nothing arrives -- the server still reading the
/// prompt -- as well as between words. The deep model gives way to a turn
/// this way even while it reads a long prompt, when no word comes for
/// `on_text` to stop at (`deepbrain`). Explicit callback truncation keeps its
/// partial reply; a cancelled keep_going request returns an error, never empty success.
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
        if !keep_going() {
            return Err(stopped_chat());
        }
        match chat_call_once(url, &req, on_text, keep_going) {
            Ok(r) => return Ok(r),
            Err(ChatFail::NoChat(m)) => {
                chat_failed(url);
                return Err(AtlasError::Platform(m));
            }
            Err(ChatFail::Loading(_)) if started.elapsed().as_secs() < LOADING_WAIT_SECS => {
                for _ in 0..40 {
                    if !keep_going() {
                        return Err(stopped_chat());
                    }
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
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
        Err(e) if chat_was_stopped(&e) => Err(ChatFail::Stopped),
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
        if !keep_going() {
            return Ok(None);
        }
        match s.read(buf) {
            Ok(n) => return Ok(Some(n)),
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
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
    let rest = url
        .strip_prefix("http://")
        .ok_or_else(|| AtlasError::Platform(format!("not a plain http address: {url}")))?;
    let (host, path) = match rest.find('/') {
        Some(k) => (&rest[..k], &rest[k..]),
        None => (rest, "/"),
    };
    let host = if host.contains(':') {
        host.to_string()
    } else {
        format!("{host}:80")
    };
    let addresses = resolve_chat_until(&host, keep_going)?;
    let timeout = std::time::Duration::from_secs(CHAT_READ_SECS);
    let mut s = match connect_chat_until(&addresses, keep_going) {
        Ok(stream) => stream,
        Err(error) if chat_was_stopped(&error) => return Err(error),
        Err(_) => {
            pause_chat_until(std::time::Duration::from_secs(1), keep_going)?;
            connect_chat_until(&addresses, keep_going).map_err(|error| {
                if chat_was_stopped(&error) {
                    error
                } else {
                    AtlasError::Platform(unreachable_words(&host, &error.to_string()))
                }
            })?
        }
    };
    s.set_read_timeout(Some(std::time::Duration::from_millis(READ_SLICE_MS)))?;
    let body = chat_body(req, true);
    write_chat_until(
        &mut s,
        crate::http::build_request("POST", &host, path, Some(&body)).as_bytes(),
        keep_going,
    )?;

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
            return Err(stopped_chat());
        };
        if n == 0 {
            return Err(AtlasError::Platform(
                "the model server closed the connection".into(),
            ));
        }
        raw.extend_from_slice(&buf[..n]);
        if raw.len() > 64 * 1024 {
            return Err(AtlasError::Platform(
                "the model server sent no end to its headers".into(),
            ));
        }
    };
    let head = String::from_utf8_lossy(&raw[..head_end]).to_string();
    let status: u16 = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let chunked = head.to_lowercase().contains("transfer-encoding: chunked");
    let error_len: Option<usize> = head
        .lines()
        .find_map(|l| {
            l.to_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().to_string())
        })
        .and_then(|v| v.parse().ok());
    let mut pending: Vec<u8> = raw[head_end + 4..].to_vec();
    let mut body_bytes: Vec<u8> = Vec::new();
    let mut stream = ChatStream::default();
    let mut stopped = false;
    let mut eof = false;
    loop {
        // Move what has arrived into the body, undoing chunking.
        if chunked {
            while let Some(eol) = pending.windows(2).position(|w| w == b"\r\n") {
                let size_txt = String::from_utf8_lossy(&pending[..eol]).to_string();
                let Ok(n) = usize::from_str_radix(
                    size_txt.trim().split(';').next().unwrap_or("").trim(),
                    16,
                ) else {
                    eof = true;
                    break;
                };
                if n == 0 {
                    eof = true;
                    break;
                }
                // A size the server chose: checked, so a huge one can't wrap.
                let Some(whole) = n.checked_add(eol + 4) else {
                    eof = true;
                    break;
                };
                if pending.len() < whole {
                    break;
                }
                body_bytes.extend_from_slice(&pending[eol + 2..eol + 2 + n]);
                pending.drain(..whole);
            }
        } else {
            body_bytes.append(&mut pending);
        }
        // Whole lines (an `if` around a `loop`: it was `while status == 200`,
        // which clippy rightly flags -- `status` never changes in the loop).
        if status == 200 {
            while let Some(nl) = body_bytes.iter().position(|b| *b == b'\n') {
                let line: Vec<u8> = body_bytes.drain(..=nl).collect();
                let line = String::from_utf8_lossy(&line).to_string();
                if let Some(piece) = stream.line(&line) {
                    if !on_text(&piece) {
                        stopped = true;
                        break;
                    }
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
            return Err(stopped_chat());
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
        return Ok(Err(ChatFail::Other(
            "the model server stopped partway through its reply".into(),
        )));
    }
    if let Some(t) = stream.timings {
        if let Ok(mut g) = LAST_TIMINGS.lock().or_else(crate::crash::unpoison) {
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
        assert_eq!(
            without_quant("Qwen3VL-4B-Instruct-Q4_K_M"),
            "qwen3vl-4b-instruct"
        );
        assert_eq!(
            without_quant("Qwen3VL-4B-Instruct-Q8_0"),
            "qwen3vl-4b-instruct"
        );
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
        assert_eq!(
            projector_args(&m, &cfg),
            vec!["--mmproj".to_string(), eyes.display().to_string()]
        );
        let off = ModelsConfig {
            see_with_talking_model: false,
            ..ModelsConfig::default()
        };
        assert!(projector_args(&m, &off).is_empty());
        // The deep model has no encoder beside it: nothing added.
        let deep = model_at(&dir.join("Qwen_Qwen3.5-9B-IQ4_XS.gguf"));
        assert!(projector_args(&deep, &cfg).is_empty());
        // The encoder's memory is counted in what the server needs.
        assert!(footprint_mb(&m, &cfg) > footprint_mb(&m, &off));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod bounded_chat_cancellation {
    use super::*;
    use std::io::Read;
    #[test]
    fn cancellation_before_headers_is_an_error_and_closes_the_connection() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let (accepted, waiting) = std::sync::mpsc::channel();
        let worker = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut buffer = [0; 4096];
            let n = peer.read(&mut buffer).unwrap();
            assert!(n > 0);
            accepted.send(()).unwrap();
            loop {
                match peer.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(_) => {}
                    Err(error) => panic!("cancelled request remained open: {error}"),
                }
            }
        });
        let began = std::time::Instant::now();
        let output = chat_call_until(
            &format!("http://{address}/v1/chat/completions"),
            &crate::brain::ChatRequest::default(),
            &mut |_| panic!("no generation should arrive"),
            &|| began.elapsed() < std::time::Duration::from_millis(50),
        );
        waiting
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap();
        worker.join().unwrap();
        assert!(
            output.is_err(),
            "a pre-header deadline is not a completed empty generation"
        );
        assert!(
            began.elapsed() < std::time::Duration::from_secs(1),
            "one read slice plus connection cleanup must stay bounded"
        );
    }
    fn reply_server(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let thread = std::thread::spawn(move || {
            use std::io::Write;
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut bytes = [0; 4096];
            assert!(peer.read(&mut bytes).unwrap() > 0);
            peer.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).unwrap();
        });
        (url, thread)
    }
    #[test]
    fn partial_generation_disconnect_is_not_a_completed_reply() {
        let (url, worker) =
            reply_server("data: {\"choices\":[{\"delta\":{\"content\":\"partial draft\"}}]}\n\n");
        let mut received = String::new();
        let result = chat_call_until(
            &url,
            &crate::brain::ChatRequest::default(),
            &mut |text| {
                received.push_str(text);
                true
            },
            &|| true,
        );
        worker.join().unwrap();
        assert_eq!(received, "partial draft");
        assert!(
            result.is_err(),
            "connection close is not a provider completion marker"
        );
    }
    #[test]
    fn provider_finish_reason_completes_a_stream_without_done_line() {
        let (url, worker) = reply_server("data: {\"choices\":[{\"delta\":{\"content\":\"complete draft\"},\"finish_reason\":\"stop\"}]}\n\n");
        let result = chat_call_until(
            &url,
            &crate::brain::ChatRequest::default(),
            &mut |_| true,
            &|| true,
        );
        worker.join().unwrap();
        assert_eq!(result.unwrap().text, "complete draft");
    }
    #[test]
    fn stopped_resolver_mailbox_drops_late_result_without_transport() {
        let (send, reply) = std::sync::mpsc::sync_channel(1);
        let began = std::time::Instant::now();
        assert!(chat_was_stopped(
            &wait_chat_resolution(&reply, &|| began.elapsed()
                < std::time::Duration::from_millis(50))
            .unwrap_err()
        ));
        drop(reply);
        assert!(matches!(
            send.try_send(Ok(vec!["127.0.0.1:1".parse().unwrap()])),
            Err(std::sync::mpsc::TrySendError::Disconnected(_))
        ));
        assert!(began.elapsed() < std::time::Duration::from_millis(250));
    }
    #[test]
    fn refused_connection_retry_polls_stop_instead_of_sleeping_a_second() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);
        let began = std::time::Instant::now();
        let result = chat_call_until(
            &format!("http://{address}/v1/chat/completions"),
            &crate::brain::ChatRequest::default(),
            &mut |_| true,
            &|| began.elapsed() < std::time::Duration::from_millis(50),
        );
        assert!(chat_was_stopped(&result.unwrap_err()));
        assert!(began.elapsed() < std::time::Duration::from_millis(300));
    }
    #[test]
    fn writing_large_request_stops_between_chunks_and_closes_socket() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let worker = std::thread::spawn(move || {
            let (mut peer, _) = listener.accept().unwrap();
            peer.set_read_timeout(Some(std::time::Duration::from_secs(2)))
                .unwrap();
            let mut bytes = Vec::new();
            peer.read_to_end(&mut bytes).unwrap();
            bytes.len()
        });
        let mut stream = std::net::TcpStream::connect(address).unwrap();
        let checkpoints = std::cell::Cell::new(0);
        let result = write_chat_until(&mut stream, &vec![b'x'; 2 * 1024 * 1024], &|| {
            checkpoints.set(checkpoints.get() + 1);
            checkpoints.get() < 3
        });
        drop(stream);
        let received = worker.join().unwrap();
        assert!(chat_was_stopped(&result.unwrap_err()));
        assert!(
            received > 0 && received < 2 * 1024 * 1024,
            "only the initial bounded chunks may be sent"
        );
    }
    #[test]
    fn already_cancelled_request_never_connects() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        assert!(chat_call_until(
            &format!(
                "http://{}/v1/chat/completions",
                listener.local_addr().unwrap()
            ),
            &crate::brain::ChatRequest::default(),
            &mut |_| true,
            &|| false
        )
        .is_err());
        assert!(
            matches!(listener.accept(), Err(error) if error.kind() == std::io::ErrorKind::WouldBlock)
        );
    }
}

#[cfg(test)]
mod memory_tiers {
    use super::*;

    fn model(id: &str, params: u64, bytes: u64) -> Model {
        Model {
            path: PathBuf::from(format!("/nowhere/{id}.gguf")),
            id: id.into(),
            architecture: "qwen3".into(),
            quant: "Q4_K_M".into(),
            parameters: params,
            weight_bytes: bytes,
            max_context: 32_768,
            chat_template: None,
        }
    }

    fn both() -> Registry {
        Registry {
            models: vec![
                model(crate::deepbrain::FASTER_TALK, 4_000_000_000, 2_497_281_664),
                model(SMALL_TALK, 1_700_000_000, 1_834_426_016),
            ],
        }
    }

    fn laptop(free_mb: u64) -> crate::fit::Machine {
        crate::fit::Machine {
            total_ram_mb: 16_384,
            free_ram_mb: free_mb,
            cpu_cores: 8,
            ..Default::default()
        }
    }

    fn picked(r: &Registry, free_mb: u64) -> Option<String> {
        pick(r, &ModelsConfig::default(), &laptop(free_mb)).map(|m| m.id.clone())
    }

    #[test]
    fn the_tiers_follow_the_designs_ram_table() {
        assert_eq!(memory_tier(8_000), MemoryTier::Full);
        assert_eq!(memory_tier(6_656), MemoryTier::Full);
        assert_eq!(memory_tier(6_655), MemoryTier::Small);
        assert_eq!(memory_tier(4_608), MemoryTier::Small);
        assert_eq!(memory_tier(4_607), MemoryTier::SmallTight);
        assert_eq!(memory_tier(3_789), MemoryTier::SmallTight);
        assert_eq!(memory_tier(3_788), MemoryTier::Floor);
        assert_eq!(memory_tier(0), MemoryTier::Floor);
    }

    #[test]
    fn with_both_here_the_free_memory_picks_the_model() {
        let r = both();
        let four = crate::deepbrain::FASTER_TALK.to_string();
        let small = SMALL_TALK.to_string();
        assert_eq!(picked(&r, 12_000), Some(four.clone()));
        assert_eq!(picked(&r, 6_700), Some(four));
        // 5 GB and 6.5 GB (just under): the 1.7B.
        assert_eq!(picked(&r, 6_600), Some(small.clone()));
        assert_eq!(picked(&r, 5_000), Some(small.clone()));
        assert_eq!(picked(&r, 4_608), Some(small.clone()));
        // Under 4.5 GB the 1.7B at 2K, while it leaves the PC 2 GB (its
        // measured working set at 2K is about 2.4 GB).
        assert_eq!(picked(&r, 4_500), Some(small));
        // The 1.7B's 8-bit file would leave the PC under 2 GB: the floor.
        assert_eq!(picked(&r, 4_400), None);
        assert_eq!(picked(&r, 3_900), None);
        assert_eq!(picked(&r, 2_500), None);
        assert_eq!(picked(&r, 500), None);
    }

    #[test]
    fn the_small_models_context_follows_the_tier() {
        let r = both();
        let small = r.get(SMALL_TALK).unwrap();
        let four = r.get(crate::deepbrain::FASTER_TALK).unwrap();
        let cfg = ModelsConfig::default();
        assert_eq!(tuned_for(small, &cfg, &laptop(5_500)).context, 4_096);
        assert_eq!(tuned_for(small, &cfg, &laptop(4_300)).context, 2_048);
        // The 4B is left as configured.
        assert_eq!(tuned_for(four, &cfg, &laptop(8_000)).context, cfg.context);
        // Never raised past what is configured.
        let low = ModelsConfig {
            context: 1_024,
            ..ModelsConfig::default()
        };
        assert_eq!(tuned_for(small, &low, &laptop(5_500)).context, 1_024);
    }

    /// 21 Oct 2026: a 32 GB machine with only 7 GB free got the 8B because
    /// the room was sized from total memory; it must leave 2 GB free too.
    #[test]
    fn the_8b_needs_free_memory_not_only_total_memory() {
        let r = Registry {
            models: vec![
                model(crate::deepbrain::FASTER_TALK, 4_000_000_000, 2_497_281_664),
                model(crate::deepbrain::BIGGER_TALK, 8_000_000_000, 5_000_000_000),
            ],
        };
        let big = |free_mb: u64| crate::fit::Machine {
            total_ram_mb: 32_768,
            free_ram_mb: free_mb,
            cpu_cores: 8,
            ..Default::default()
        };
        let cfg = ModelsConfig::default();
        let at = |free| pick(&r, &cfg, &big(free)).map(|m| m.id.clone());
        assert_eq!(at(7_000), Some(crate::deepbrain::FASTER_TALK.to_string()));
        assert_eq!(at(28_000), Some(crate::deepbrain::BIGGER_TALK.to_string()));
    }

    /// 21 Oct 2026: a limit set in `memory_budget_mb` switched the tiers off
    /// and the 4B was started whatever it said.
    #[test]
    fn a_memory_limit_lowers_the_tiers_instead_of_switching_them_off() {
        let r = both();
        let limit = |mb| ModelsConfig {
            memory_budget_mb: mb,
            ..ModelsConfig::default()
        };
        let at = |mb, free| pick(&r, &limit(mb), &laptop(free)).map(|m| m.id.clone());
        // Plenty free, but a 5 GB limit: the 1.7B, not the 4B.
        assert_eq!(at(5_000, 12_000), Some(SMALL_TALK.to_string()));
        // A 3 GB limit leaves no model that keeps the PC its 2 GB.
        assert_eq!(at(3_000, 12_000), None);
        // A limit above what is free changes nothing: free decides.
        assert_eq!(at(16_000, 5_000), Some(SMALL_TALK.to_string()));
        assert_eq!(
            at(16_000, 12_000),
            Some(crate::deepbrain::FASTER_TALK.to_string())
        );
    }

    /// 21 Oct 2026: with both models here and about 3.5 GB free no model
    /// fits; the floor must get the 1.7B and its need, not the 4B.
    #[test]
    fn no_fit_by_tier_names_the_small_model_and_what_it_needs() {
        let r = both();
        let cfg = ModelsConfig::default();
        let (id, need) = tier_no_fit(&r, &cfg, &laptop(3_500)).unwrap();
        assert_eq!(id, SMALL_TALK);
        assert!(need > 3_500 && need >= FLOOR_FREE_MB + 2_000, "{need}");
        // The floor's decision on those facts is the tier's answer (a Low ask
        // naming the 1.7B with the shortfall), never Proceed.
        let facts = crate::floor::Facts {
            loaded: false,
            model: Some((id.clone(), need)),
            free_mb: 3_500,
            total_mb: 16_384,
            online_allowed: true,
            offline_known: false,
            closable: Vec::new(),
        };
        match crate::floor::decide(&facts) {
            crate::floor::Choice::Low(ask) => {
                assert_eq!(ask.model, SMALL_TALK);
                assert!(ask.shortfall_mb > 0 && ask.online);
            }
            other => panic!("expected Low, got {other:?}"),
        }
        // Something fits, or the tiers do not apply: nothing to say.
        assert!(tier_no_fit(&r, &cfg, &laptop(5_000)).is_none());
        let named = ModelsConfig {
            prefer: crate::deepbrain::FASTER_TALK.into(),
            ..ModelsConfig::default()
        };
        assert!(tier_no_fit(&r, &named, &laptop(3_500)).is_none());
        let only4 = Registry {
            models: vec![r.models[0].clone()],
        };
        assert!(tier_no_fit(&only4, &cfg, &laptop(3_500)).is_none());
    }

    #[test]
    fn tiers_do_not_override_a_choice_the_owner_made() {
        let r = both();
        let named = ModelsConfig {
            prefer: crate::deepbrain::FASTER_TALK.into(),
            ..ModelsConfig::default()
        };
        assert_eq!(
            pick(&r, &named, &laptop(2_500)).map(|m| m.id.clone()),
            Some(crate::deepbrain::FASTER_TALK.to_string())
        );
        let limited = ModelsConfig {
            memory_budget_mb: 8_192,
            ..ModelsConfig::default()
        };
        // A limit lowers what the tiers see, it no longer switches them off:
        // 8 GB limit with 2.5 GB free is the floor.
        assert!(matches!(pick_by_tier(&r, &limited, &laptop(2_500)), Some(None)));
        // With the small model not here, the ordinary pick is unchanged.
        let only4 = Registry {
            models: vec![r.models[0].clone()],
        };
        assert_eq!(
            picked(&only4, 5_000),
            Some(crate::deepbrain::FASTER_TALK.to_string())
        );
    }

    // ---- measured cost (21 Oct 2026) ----

    fn four_key() -> String {
        ModelCosts::key(crate::deepbrain::FASTER_TALK, ModelsConfig::default().context, false)
    }

    fn small_key(ctx: u64) -> String {
        ModelCosts::key(SMALL_TALK, ctx, false)
    }

    fn by_tier(r: &Registry, free_mb: u64, costs: &ModelCosts) -> Option<Option<String>> {
        pick_by_tier_with(r, &ModelsConfig::default(), &laptop(free_mb), costs)
            .map(|c| c.map(|m| m.id.clone()))
    }

    #[test]
    fn the_cost_arithmetic_is_the_larger_of_estimate_and_measured() {
        let mb = 1024 * 1024;
        assert_eq!(estimated_need_mb(2_000 * mb, false), 2_400);
        // Integrated graphics offload: a quarter more on top, documented.
        assert_eq!(estimated_need_mb(2_000 * mb, true), 3_000);
        assert_eq!(needed_mb(2_400, None), 2_400);
        assert_eq!(needed_mb(2_400, Some(5_200)), 5_200);
        assert_eq!(needed_mb(6_000, Some(5_200)), 6_000);
        assert_eq!(measured_cost_mb(6_400, 1_160), 5_240);
        assert_eq!(measured_cost_mb(1_000, 1_200), 0);
        assert!(fits_with_floor(4_048, 2_000));
        assert!(!fits_with_floor(4_047, 2_000));
        let igpu = crate::fit::Machine::default();
        assert!(integrated_offload(&igpu, 999));
        assert!(!integrated_offload(&igpu, 0));
        let card = crate::fit::Machine {
            vram_mb: 8_192,
            ..Default::default()
        };
        assert!(!integrated_offload(&card, 999));
    }

    #[test]
    fn the_records_keep_the_latest_per_key_and_survive_a_round_trip() {
        let c = ModelCosts::default()
            .with("a|ctx1|gpu", 100, 1)
            .with("b|ctx1|cpu", 200, 2)
            .with("a|ctx1|gpu", 300, 3);
        assert_eq!(c.cost_for("a|ctx1|gpu"), Some(300));
        assert_eq!(c.cost_for("b|ctx1|cpu"), Some(200));
        assert_eq!(c.cost_for("nope"), None);
        let json = serde_json::to_string(&c).unwrap();
        let back: ModelCosts = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        assert_eq!(serde_json::from_str::<ModelCosts>("{}").unwrap(), ModelCosts::default());
        assert_ne!(ModelCosts::key("m", 4096, true), ModelCosts::key("m", 4096, false));
    }

    #[test]
    fn a_measured_cost_drops_the_tier_and_blocks_a_reload_until_there_is_headroom() {
        let r = both();
        let four = crate::deepbrain::FASTER_TALK.to_string();
        let none = ModelCosts::default();
        assert_eq!(by_tier(&r, 6_700, &none), None, "no cost known: the 4B as before");
        // The 4B measured 5.2 GB here: 6.7 GB free is not 5.2 + 2.
        let c4 = ModelCosts::default().with(&four_key(), 5_200, 1);
        assert_eq!(by_tier(&r, 6_700, &c4), Some(Some(SMALL_TALK.to_string())));
        // Not reloaded until available memory is cost + 2 GB.
        assert_eq!(by_tier(&r, 7_247, &c4), Some(Some(SMALL_TALK.to_string())));
        assert_eq!(by_tier(&r, 7_248, &c4), None);
        // The 1.7B at 4K measured as dear: one more tier down, to 2K.
        let c = c4.with(&small_key(4_096), 5_200, 2);
        assert_eq!(by_tier(&r, 5_000, &c), Some(Some(SMALL_TALK.to_string())));
        let small = r.get(SMALL_TALK).unwrap();
        assert_eq!(
            tuned_for_with(small, &ModelsConfig::default(), &laptop(5_000), &c).context,
            2_048
        );
        // And the 2K one measured as dear: the floor, then back with headroom.
        let c = c.with(&small_key(2_048), 4_000, 3);
        assert_eq!(by_tier(&r, 5_000, &c), Some(None));
        assert_eq!(by_tier(&r, 6_048, &c), Some(Some(SMALL_TALK.to_string())));
        let _ = four;
    }

    #[test]
    fn integrated_graphics_offload_raises_the_first_estimate() {
        let r = both();
        let gpu = ModelsConfig {
            gpu_layers: "all".into(),
            ..ModelsConfig::default()
        };
        let none = ModelCosts::default();
        // The 4B fits at 6.7 GB on the processor; offloaded to integrated
        // graphics it is estimated a quarter higher and does not.
        let at = |cfg: &ModelsConfig| {
            pick_by_tier_with(&r, cfg, &laptop(6_700), &none).map(|c| c.map(|m| m.id.clone()))
        };
        assert_eq!(at(&ModelsConfig::default()), None);
        assert_eq!(at(&gpu), Some(Some(SMALL_TALK.to_string())));
    }

    #[test]
    fn the_post_load_guard_trips_under_the_floor_and_keeps_the_settled_cost() {
        // The 21 Oct evidence: 6.4 GB free before, 1,160 MB lowest.
        assert!(guard_trips(1_160));
        assert!(guard_trips(2_047));
        assert!(!guard_trips(2_048));
        assert_eq!(cost_to_record(6_400, 1_160, None), 5_240);
        // A cost settled cleanly earlier stands against a later dip.
        assert_eq!(cost_to_record(6_400, 1_160, Some(2_500)), 2_500);
        let line = guard_line("Qwen3-1.7B-Q8_0", 6_400, 1_160, 5_240, "stopped it");
        for part in ["6400 MB", "1160 MB", "5240 MB", "2048 MB", "stopped it"] {
            assert!(line.contains(part), "{line}");
        }
    }

    #[test]
    fn the_health_line_says_which_tier_and_why() {
        let r = both();
        let cfg = ModelsConfig::default();
        let none = ModelCosts::default();
        let line = |free| tier_line(&r, &cfg, &laptop(free), &none).unwrap();
        assert_eq!(line(8_000), "4B, because 7.8 GB was free");
        assert_eq!(line(4_900), "1.7B, because 4.8 GB was free");
        assert!(line(4_500).starts_with("1.7B at 2K context, because"), "{}", line(4_500));
        let floor = line(2_150);
        assert!(floor.starts_with("no model right now: it needs"), "{floor}");
        assert!(floor.ends_with("and 2.1 GB is free"), "{floor}");
        // A measured cost that pushed it down is named.
        let c4 = ModelCosts::default().with(&four_key(), 5_200, 1);
        let said = tier_line(&r, &cfg, &laptop(6_700), &c4).unwrap();
        assert!(said.starts_with("1.7B, because 6.5 GB was free and the bigger one needs 7.1 GB"), "{said}");
        // Tiers off: the plain line is used.
        let named = ModelsConfig {
            prefer: crate::deepbrain::FASTER_TALK.into(),
            ..ModelsConfig::default()
        };
        assert!(tier_line(&r, &named, &laptop(2_500), &none).is_none());
    }

    #[test]
    fn choose_tier_drops_one_at_a_time() {
        let all = |_: MemoryTier| true;
        let none = |t: MemoryTier| t == MemoryTier::Floor;
        assert_eq!(choose_tier(MemoryTier::Full, &all), MemoryTier::Full);
        assert_eq!(choose_tier(MemoryTier::Full, &none), MemoryTier::Floor);
        let only_tight = |t: MemoryTier| t == MemoryTier::SmallTight;
        assert_eq!(choose_tier(MemoryTier::Full, &only_tight), MemoryTier::SmallTight);
        assert_eq!(drop_tier(MemoryTier::Floor), MemoryTier::Floor);
    }
}
