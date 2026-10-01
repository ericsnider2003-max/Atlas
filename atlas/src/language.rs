//! Accents, other languages, and understanding a room.
//!
//! Two different problems that share a solution.
//!
//! **Accents.** Whisper was trained on a very wide range of speech, so it
//! handles accents better than almost anything you could build, and better
//! than most people expect. Where it struggles is heavy accents on the
//! smallest models — and the fix is a bigger model, which costs memory you
//! don't have much of. So this measures how well it's actually doing rather
//! than guessing, and suggests a step up only when the evidence says so.
//!
//! **Other languages.** The English-only model cannot hear them at all — not
//! badly, at all. Swapping to the multilingual model of the same size costs
//! nothing in memory and a little accuracy on English, and gains ninety-odd
//! languages plus translation. Whisper can transcribe *or* translate to
//! English in the same pass, which is the whole feature for one flag.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Task {
    /// Write down what was said, in whatever language it was said.
    Transcribe,
    /// Write it down in English, whatever went in.
    Translate,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LanguageConfig {
    /// The language you speak. `auto` lets Whisper work it out per clip,
    /// which is what you want in a room with several languages in it.
    pub my_language: String,
    /// Understand languages other than English at all. Needs the
    /// multilingual model — the `.en` models physically cannot.
    pub multilingual: bool,
    /// Turn anything that isn't your language into English.
    pub translate_others: bool,
    /// Below this confidence, say it didn't catch it rather than guessing.
    pub min_confidence: f32,
    /// Consecutive poor transcriptions before suggesting a bigger model.
    pub struggles_before_suggesting: u32,
}

impl Default for LanguageConfig {
    fn default() -> Self {
        LanguageConfig {
            my_language: "auto".into(),
            multilingual: false,
            translate_others: true,
            min_confidence: 0.55,
            struggles_before_suggesting: 6,
        }
    }
}

/// What a model can and can't do, from its filename.
///
/// The `.en` suffix is not a preference — those models have no other
/// languages in them at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelFacts {
    pub name: String,
    pub english_only: bool,
    /// Rough size on disk, in megabytes.
    pub megabytes: u32,
}

pub fn model_facts(filename: &str) -> ModelFacts {
    let f = filename.to_lowercase();
    let english_only = f.contains(".en.") || f.ends_with(".en") || f.contains("-en.");
    let megabytes = if f.contains("tiny") {
        75
    } else if f.contains("base") {
        142
    } else if f.contains("small") {
        466
    } else if f.contains("medium") {
        1500
    } else if f.contains("large") {
        3100
    } else {
        142
    };
    ModelFacts { name: filename.to_string(), english_only, megabytes }
}

/// The bigger model to try when accents are being missed.
pub fn next_size_up(current: &str) -> Option<&'static str> {
    let f = current.to_lowercase();
    if f.contains("tiny") {
        Some("base")
    } else if f.contains("base") {
        Some("small")
    } else if f.contains("small") {
        // Medium is 1.5GB. On 3.5GB of usable memory that is most of the
        // budget for a marginal gain, so it isn't offered.
        None
    } else {
        None
    }
}

/// The `{language}` and `{task}` a transcription command template should use,
/// given the language settings and which model is actually loaded.
///
/// Wired as template variables rather than a Rust-built arg list on purpose:
/// transcription runs through a configurable tool command (Atlas's tool-slot
/// design), so the honest way to make it respect the language setting is to
/// hand the template the two values it needs, not to replace the template.
///
/// An **English-only model ignores both** — it physically has no other
/// languages, so asking it for French or a translation does nothing useful,
/// and the honest values are an empty language and a plain transcribe. That
/// is what keeps this correct on the shipped default (`ggml-base.en.bin`)
/// while being ready the moment a multilingual model is put in place. Returns
/// `(language, task)` where `language` is a two-letter code, `auto`, or empty
/// (the template omits the flag when empty), and `task` is `transcribe` or
/// `translate`.
pub fn template_vars(cfg: &LanguageConfig, model: &ModelFacts) -> (String, String) {
    // An English-only model gets neither: the flags would be inert at best
    // and misleading at worst.
    if model.english_only || !cfg.multilingual {
        return (String::new(), "transcribe".into());
    }
    let language = if cfg.my_language.is_empty() { "auto".into() } else { cfg.my_language.clone() };
    let task = if cfg.translate_others { "translate" } else { "transcribe" };
    (language, task.into())
}

/// Put `{lang_opt}`, `{lang_val}` and `{task_opt}` into a transcription
/// command's variables, from the language settings and the loaded model.
///
/// One function for every caller. The daemon had its own copy and the
/// microphone path had none, so every spoken turn handed whisper the literal
/// text `{task_opt}` as an argument.
///
/// `model` is what `model_facts` says about the speech model in use.
pub fn insert_whisper_vars(
    cfg: &LanguageConfig,
    model: &ModelFacts,
    vars: &mut std::collections::BTreeMap<String, String>,
) {
    let (language, task) = template_vars(cfg, model);
    if language.is_empty() {
        vars.insert("lang_opt".into(), String::new());
        vars.insert("lang_val".into(), String::new());
    } else {
        vars.insert("lang_opt".into(), "-l".into());
        vars.insert("lang_val".into(), language);
    }
    vars.insert("task_opt".into(), if task == "translate" { "-tr".into() } else { String::new() });
    // Your words as hints (H11) are added by the voice when there are any;
    // empty here, so the command never carries the placeholder itself.
    vars.entry("hint_opt".into()).or_default();
    vars.entry("hint_val".into()).or_default();
}

/// Arguments for one pass of transcription.
pub fn args(model: &str, audio: &str, task: Task, language: &str) -> Vec<String> {
    let mut a = vec![
        "-m".into(),
        model.into(),
        "-f".into(),
        audio.into(),
        "-nt".into(),
        // Confidence per segment, so a poor clip can be recognised as poor.
        "-oj".into(),
    ];
    match task {
        Task::Translate => {
            // Whisper does this in the same pass. Nothing else is needed.
            a.push("-tr".into());
        }
        Task::Transcribe => {}
    }
    if language != "auto" {
        a.push("-l".into());
        a.push(language.into());
    } else {
        a.push("-l".into());
        a.push("auto".into());
    }
    a
}

#[derive(Debug, Clone, PartialEq)]
pub struct Heard {
    pub text: String,
    /// Two-letter code, or empty when it couldn't tell.
    pub language: String,
    pub confidence: f32,
    /// It was translated on the way in.
    pub translated: bool,
}

impl Heard {
    pub fn is_mine(&self, cfg: &LanguageConfig) -> bool {
        cfg.my_language == "auto"
            || self.language.is_empty()
            || self.language.eq_ignore_ascii_case(&cfg.my_language)
    }
}

/// Decide what to do with a clip before spending anything on it.
#[derive(Debug, Clone, PartialEq)]
pub enum Plan {
    /// Straightforward.
    Transcribe { language: String },
    /// Not your language — bring it back in English.
    Translate { from: String },
    /// The model can't hear this language at all.
    CannotHear { language: String, why: String },
}

pub fn plan(detected: &str, cfg: &LanguageConfig, model: &ModelFacts) -> Plan {
    let mine = cfg.my_language == "auto" || detected.eq_ignore_ascii_case(&cfg.my_language);

    if mine || detected.is_empty() {
        return Plan::Transcribe { language: cfg.my_language.clone() };
    }
    if model.english_only {
        // Worth being exact about: this isn't a quality problem, the model
        // has no other languages in it.
        return Plan::CannotHear {
            language: detected.to_string(),
            why: format!(
                "{} is an English-only model — swap it for the multilingual one of the same size",
                model.name
            ),
        };
    }
    if cfg.translate_others {
        Plan::Translate { from: detected.to_string() }
    } else {
        Plan::Transcribe { language: detected.to_string() }
    }
}

/// Watches how well it's actually hearing you, so a suggestion to change the
/// model rests on evidence rather than a hunch.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Listening {
    recent: Vec<f32>,
    pub suggested_at: Option<u64>,
}

impl Listening {
    pub fn record(&mut self, confidence: f32) {
        self.recent.push(confidence);
        if self.recent.len() > 40 {
            self.recent.remove(0);
        }
    }

    pub fn typical(&self) -> Option<f32> {
        if self.recent.is_empty() {
            return None;
        }
        let mut v = self.recent.clone();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(v[v.len() / 2])
    }

    /// Is it struggling enough to be worth mentioning?
    pub fn struggling(&self, cfg: &LanguageConfig) -> bool {
        let poor = self.recent.iter().filter(|c| **c < cfg.min_confidence).count() as u32;
        poor >= cfg.struggles_before_suggesting
    }

    /// What Atlas says about it. Once, not every time.
    pub fn suggestion(&self, current_model: &str, cfg: &LanguageConfig) -> Option<String> {
        if !self.struggling(cfg) || self.suggested_at.is_some() {
            return None;
        }
        let typical = self.typical().unwrap_or(0.0);
        match next_size_up(current_model) {
            Some(bigger) => Some(format!(
                "I'm mishearing you more than I'd like — about {:.0}% confidence. \
                 The {bigger} model would handle your voice better. It's larger, so it'll be a \
                 little slower. Want me to switch?",
                typical * 100.0
            )),
            None => Some(
                "I'm mishearing you more than I'd like, and I'm already on the largest model \
                 that fits comfortably. A closer microphone would do more than a bigger model."
                    .into(),
            ),
        }
    }
}

// ---------- a room with more than one language in it ----------

/// One person's turn in a conversation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Turn {
    /// Who, when known. Otherwise "someone".
    pub speaker: String,
    pub language: String,
    /// What they said, as said.
    pub original: String,
    /// In English, when it wasn't.
    pub english: Option<String>,
    pub at: u64,
}

impl Turn {
    /// What Atlas would read out.
    pub fn readable(&self) -> String {
        match &self.english {
            Some(e) => format!("{}: {e}", self.speaker),
            None => format!("{}: {}", self.speaker, self.original),
        }
    }
}

/// Notes from a conversation with several languages in it.
///
/// Both versions are kept. A translation is an interpretation, and a note that
/// throws away what was actually said can't be checked later.
pub fn notes(turns: &[Turn]) -> String {
    let mut out = String::new();
    let languages: Vec<String> = {
        let mut l: Vec<String> = turns.iter().map(|t| t.language.clone()).collect();
        l.sort();
        l.dedup();
        l.retain(|x| !x.is_empty());
        l
    };
    if languages.len() > 1 {
        out.push_str(&format!("Languages: {}\n\n", languages.join(", ")));
    }
    for t in turns {
        out.push_str(&format!("{}: {}\n", t.speaker, t.original));
        if let Some(e) = &t.english {
            out.push_str(&format!("    → {e}\n"));
        }
    }
    out
}

/// A live line for the panel while a conversation is happening.
pub fn live_line(t: &Turn) -> String {
    match (&t.english, t.language.as_str()) {
        (Some(e), lang) if !lang.is_empty() => format!("{} ({lang}) — {e}", t.speaker),
        _ => t.readable(),
    }
}

/// The speech model to use: the sharper one setup fetches
/// (`getpieces::SHARPER_LISTENING_MODEL`) when it is under `root` and the
/// configured one is the shipped `ggml-base.en.bin`, else the configured one
/// (29 Sep 2026). A model you named yourself is always kept.
pub fn speech_model_for(configured: &str, root: &std::path::Path) -> String {
    let shipped = configured.replace('\\', "/").ends_with("ggml-base.en.bin") || configured.trim().is_empty();
    let sharper = root.join(crate::getpieces::SHARPER_LISTENING_MODEL);
    if shipped && sharper.is_file() {
        return sharper.display().to_string();
    }
    configured.to_string()
}

/// Words the speech model is primed with before any of your own are known
/// (whisper's `--prompt`): the assistant's name, which base.en heard as
/// "At this" and "Brad" (29 Sep 2026). Kept to the name: a long list of
/// words is what whisper writes back when it hears only silence.
pub const SPEECH_PRIMER: &[&str] = &["Atlas"];
