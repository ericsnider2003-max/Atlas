//! A model that writes code (2 Oct 2026: "it still can't code").
//!
//! The talking model is a 4B chosen to answer quickly, and code was written
//! with it. A model trained for code writes it better at the same size, so
//! code draft and fix calls now go to one first, when this device can hold
//! it: **Qwen2.5-Coder 7B Instruct** at Q4_K_M, or on a small machine
//! **Qwen2.5-Coder 1.5B Instruct** at Q4_K_M -- both Qwen's own files, both
//! Apache-2.0. Not the 3B: its licence is Qwen's research licence,
//! non-commercial only, and Atlas is given to friends and may be sold.
//!
//! Which one is this device's to fetch is decided by its memory
//! (`size_for`), never by Eric's laptop: friends run their own copies.
//!
//! ## Swapping, not stacking
//!
//! The coding model runs the way the deep model does (`deepbrain`): a
//! second llama-server on a port of its own, started when a code call is
//! waiting, stopped once idle, giving way to every turn. The difference is
//! memory. On a 16 GB laptop the 7B (about 5.8 GB at its context) and the
//! talking model (about 3 GB) together may not fit beside what's open, so
//! when it can't start for want of room and the talking model is idle, the
//! talking model is let go first (`Coder::keep`). Once the build is done
//! the coding model goes after `coder_idle_secs`, and the talking model is
//! started again by the next thing you say -- or at once, if a build is
//! still waiting on a model when the coding model can't be had, because
//! then the fallback needs it. A turn arriving while the talking model is
//! let go takes it back, and the coding model is stopped for it: talking to
//! you comes before the build, whose call then goes down the usual chain.
//!
//! A code call that can't have the coding model (none fits, it didn't
//! start) fails at once, so `build_it::build_with` hands it to the next
//! writer: your second model, the worker, the model on this computer, the
//! free online ones -- never a quiet fall into the talking model under the
//! coding model's name.

use crate::brain::Llm;
use crate::deepbrain::DeepBrain;
use std::sync::Arc;
use std::time::Duration;

/// Bytes the cache takes per token of context, for each coder (f16 keys and
/// values: 2 x layers x key-value heads x head size x 2 bytes). Qwen2.5-Coder
/// 7B: 28 layers, 4 key-value heads of 128; 1.5B: 28 layers, 2 heads of 128
/// (the models' own config.json).
const CACHE_PER_TOKEN_7B: u64 = 2 * 28 * 4 * 128 * 2;
const CACHE_PER_TOKEN_1_5B: u64 = 2 * 28 * 2 * 128 * 2;

/// The coding model's context when the settings don't say, in tokens:
/// room for the project's relevant pieces and a whole file back.
pub const CONTEXT_DEFAULT: u64 = 16384;

/// Which coding model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    /// Qwen2.5-Coder 7B Instruct, Q4_K_M.
    Seven,
    /// Qwen2.5-Coder 1.5B Instruct, Q4_K_M.
    OneAndAHalf,
}

impl Size {
    /// The model's file, as `getpieces` fetches it.
    pub fn piece(self) -> crate::getpieces::Piece {
        match self {
            Size::Seven => crate::getpieces::coder_model(),
            Size::OneAndAHalf => crate::getpieces::small_coder_model(),
        }
    }

    /// Its id in the models folder: the file name without `.gguf`.
    pub fn id(self) -> &'static str {
        match self {
            Size::Seven => "qwen2.5-coder-7b-instruct-q4_k_m",
            Size::OneAndAHalf => "qwen2.5-coder-1.5b-instruct-q4_k_m",
        }
    }

    /// What it needs to run at `context`, megabytes: the weights, the cache,
    /// and a tenth more -- the same sum `deepbrain` makes from the file once
    /// it is here.
    pub fn need_mb(self, context: u64) -> u64 {
        let (bytes, per) = match self {
            Size::Seven => (crate::getpieces::coder_model().bytes, CACHE_PER_TOKEN_7B),
            Size::OneAndAHalf => (crate::getpieces::small_coder_model().bytes, CACHE_PER_TOKEN_1_5B),
        };
        (bytes + per * context.max(1024)) * 11 / 10 / (1024 * 1024)
    }
}

/// The room a coding model may have on this device, megabytes, with the
/// talking model let go for it: half the memory (what `models::pick` sizes
/// a model to when nothing is set), or a graphics card's own when that is
/// more; never more than a memory limit you set.
fn room_mb(total_ram_mb: u64, usable_vram_mb: u64, budget_set_mb: u64) -> u64 {
    let room = (total_ram_mb / 2).max(usable_vram_mb);
    match crate::models::budget_set_mb(budget_set_mb) {
        0 => room,
        set => room.min(set),
    }
}

/// Which coding model this device should have: the 7B when it fits the
/// room, else the 1.5B, else none -- a coding model that pages the machine
/// writes nothing faster than the talking model would.
pub fn size_for(total_ram_mb: u64, usable_vram_mb: u64, budget_set_mb: u64, context: u64) -> Option<Size> {
    let room = room_mb(total_ram_mb, usable_vram_mb, budget_set_mb);
    [Size::Seven, Size::OneAndAHalf].into_iter().find(|s| s.need_mb(context) <= room)
}

/// The coding model to fetch on this machine, measured now.
pub fn pieces_for_here() -> Vec<crate::getpieces::Piece> {
    let m = crate::fit::measure();
    let cfg = crate::models::ModelsConfig::default();
    size_for(m.total_ram_mb, m.usable_vram_mb(), cfg.memory_budget_mb, coder_context(&cfg)).map(|s| vec![s.piece()]).unwrap_or_default()
}

/// Is this model a coding model? Never the one that talks.
pub fn is_coder_id(id: &str) -> bool {
    id.to_ascii_lowercase().contains("coder")
}

/// "Qwen2.5-Coder 7B", for saying who wrote the code.
pub fn plain_name(id: &str) -> String {
    let low = id.to_ascii_lowercase();
    if low.starts_with("qwen2.5-coder-7b") {
        "Qwen2.5-Coder 7B".into()
    } else if low.starts_with("qwen2.5-coder-1.5b") {
        "Qwen2.5-Coder 1.5B".into()
    } else {
        id.to_string()
    }
}

/// The coding model's context for these settings.
fn coder_context(cfg: &crate::models::ModelsConfig) -> u64 {
    if cfg.coder_context == 0 {
        CONTEXT_DEFAULT
    } else {
        cfg.coder_context
    }
}

/// The coding model for these settings, when its file is here: `models.coder`
/// names one, `off` none, and empty the largest of Qwen2.5-Coder 7B and 1.5B
/// whose file is here and that fits this device's room.
pub fn coder_among<'r>(
    registry: &'r crate::models::Registry,
    cfg: &crate::models::ModelsConfig,
    total_ram_mb: u64,
    usable_vram_mb: u64,
) -> Option<&'r crate::models::Model> {
    match cfg.coder.trim() {
        w if w.eq_ignore_ascii_case("off") || w.eq_ignore_ascii_case("none") => None,
        "" => {
            let room = room_mb(total_ram_mb, usable_vram_mb, cfg.memory_budget_mb);
            let ctx = coder_context(cfg);
            [Size::Seven, Size::OneAndAHalf]
                .into_iter()
                .filter(|s| s.need_mb(ctx) <= room)
                .find_map(|s| registry.get(s.id()))
        }
        id => registry.get(id.trim_end_matches(".gguf")),
    }
}

/// The settings the coding model's server is started with: its own port
/// (the talking model's plus two, past the deep model's) and context.
pub fn coder_settings(cfg: &crate::models::ModelsConfig) -> crate::models::ModelsConfig {
    let mut c = cfg.clone();
    c.port = if cfg.coder_port == 0 { cfg.port.saturating_add(2) } else { cfg.coder_port };
    c.context = coder_context(cfg);
    // No helper model, and no guessing ahead from the prompt: it's set up
    // for the talking model.
    c.draft = String::new();
    c
}

/// Stands where the talking model stands in the deep model's connection:
/// a code call that can't have the coding model fails, so the next writer
/// takes it under its own name.
struct NotHere {
    context: u32,
}

impl Llm for NotHere {
    fn complete(&self, _: &str, _: &str) -> crate::error::Result<String> {
        Err(crate::error::AtlasError::Platform("the coding model can't be had right now".into()))
    }

    fn context_tokens(&self) -> Option<u32> {
        Some(self.context)
    }
}

/// The talking model, as the coding model sees it: is it running, is a turn
/// using it, and letting it go or bringing it back. The daemon's helpers in
/// Atlas (`Daemon::keep_coder`); a stand-in in the tests.
pub trait ChatRoom {
    /// Atlas's own talking model server is running.
    fn chat_running(&self) -> bool;
    /// A turn is being answered with it now.
    fn chat_in_use(&self) -> bool;
    /// Stop it, to make room.
    fn let_chat_go(&mut self);
    /// It may come back: `now` -- at the next pass, because work is waiting
    /// on a model -- or lazily, with the next thing you say.
    fn bring_chat_back(&mut self, now: bool);
}

/// The coding model, and whether the talking model was let go for it.
pub struct Coder {
    pub brain: DeepBrain,
    /// The talking model was stopped to make room for this one.
    pub holds_chat_room: bool,
    context: u32,
    /// When the talking model was let go: its memory comes back over a
    /// moment, so the coding model isn't started in the same breath.
    let_go_at: Option<std::time::Instant>,
}

/// How long after the talking model is let go before the coding model is
/// started in its room.
pub const SETTLE: Duration = Duration::from_millis(1500);

impl Coder {
    /// No coding model here.
    pub fn none() -> Coder {
        Coder { brain: DeepBrain::none(), holds_chat_room: false, context: 0, let_go_at: None }
    }

    /// A coding model run as `brain` (a `DeepBrain` labelled for code),
    /// with `context` tokens.
    pub fn new(mut brain: DeepBrain, context: u32) -> Coder {
        brain.label = "coding model";
        brain.for_what = "code";
        Coder { brain, holds_chat_room: false, context, let_go_at: None }
    }

    pub fn is_set_up(&self) -> bool {
        self.brain.is_set_up()
    }

    /// The connection code calls use: the coding model, giving way to turns,
    /// failing at once when it can't be had.
    pub fn llm(&self) -> Option<Arc<dyn Llm>> {
        if !self.is_set_up() {
            return None;
        }
        Some(self.brain.for_background(Arc::new(NotHere { context: self.context })))
    }

    /// Once a pass: make room for it when a code call waits and it can't
    /// start beside the talking model, start and stop it (`DeepBrain::keep`),
    /// and give the talking model back once it is let go. What happened, for
    /// the log.
    pub fn keep(&mut self, room: &mut dyn ChatRoom) -> Vec<String> {
        let mut said = Vec::new();
        if !self.is_set_up() {
            return said;
        }
        if self.let_go_at.is_some_and(|at| at.elapsed() < SETTLE) {
            return said;
        }
        self.let_go_at = None;
        if let Some(short) = self.brain.short_of_room_mb() {
            if room.chat_running() && !room.chat_in_use() && !self.brain.gate.talking() {
                room.let_chat_go();
                self.holds_chat_room = true;
                self.let_go_at = Some(std::time::Instant::now());
                self.brain.look_now();
                said.push(format!(
                    "stopped the talking model to make room for the coding model ({}, {short} MB short); it comes back after the build",
                    self.brain.model_id
                ));
                // The memory comes back over a moment: started once it has.
                return said;
            }
        }
        said.extend(self.brain.keep());
        let state = self.brain.gate.state();
        if self.holds_chat_room && !matches!(state, crate::deepbrain::State::Up | crate::deepbrain::State::Starting) {
            self.holds_chat_room = false;
            // A build still waiting on a model is falling back to the chain,
            // which may be the talking model: it comes back now. Otherwise
            // with the next thing you say.
            let now = self.brain.wanted();
            room.bring_chat_back(now);
            said.push(if now {
                "the coding model can't be had; starting the talking model again for the build".to_string()
            } else {
                "the build is done; the talking model starts again with the next thing you say".to_string()
            });
        }
        said
    }

    /// A turn needs the talking model while it's let go for this one: the
    /// coding model is stopped and the room handed back.
    pub fn give_way_to_a_turn(&mut self, room: &mut dyn ChatRoom) -> Option<String> {
        if !self.holds_chat_room {
            return None;
        }
        self.brain.stop();
        self.holds_chat_room = false;
        room.bring_chat_back(true);
        Some("a turn needs the talking model: stopped the coding model and gave its room back".into())
    }
}

/// The coding model for Atlas's own settings, when its file is here: a
/// llama-server on its own port. `Coder::none` for a hand-written `llm:`,
/// no server, `models.coder: off`, or no coding model file that fits.
pub fn for_settings(tc: &crate::voice::ToolsConfig) -> Coder {
    if tc.llm.is_some() || tc.models.server.is_none() {
        return Coder::none();
    }
    let (registry, _) = crate::models::Registry::scan_reporting(&crate::models::Registry::dir_for(&tc.models));
    let machine = crate::fit::measure();
    let Some(model) = coder_among(&registry, &tc.models, machine.total_ram_mb, machine.usable_vram_mb()).cloned() else {
        return Coder::none();
    };
    let ccfg = coder_settings(&tc.models);
    let layers = crate::models::layers_here(&model, &ccfg, &machine);
    let conn: Arc<dyn Llm> = Arc::new(crate::brain::ShellLlm {
        cfg: crate::models::llm_config_for(&model, &ccfg, &crate::models::server_post()),
        vars: tc.vars.clone(),
    });
    let need = model
        .memory_needed(ccfg.context)
        .unwrap_or_else(|_| crate::models::estimate_memory(&model, ccfg.context))
        / (1024 * 1024);
    let mut engine = crate::deepbrain::ServerEngine::new(model.clone(), ccfg.clone(), layers, tc.vars.clone());
    engine.log = "coding-model-server.log";
    let brain = DeepBrain::new(
        Box::new(engine),
        conn,
        &model.id,
        need,
        Duration::from_secs(tc.models.coder_idle_secs.max(30)),
        Box::new(|| crate::fit::measure().free_ram_mb),
    );
    Coder::new(brain, ccfg.context.min(u64::from(u32::MAX)) as u32)
}
