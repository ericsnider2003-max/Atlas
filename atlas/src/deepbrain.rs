//! Two brains (30 Sep 2026): a quick one to talk with, and a deeper one for
//! work nobody is waiting on word by word.
//!
//! Measured on Eric's laptop (Core Ultra 7 256V, Arc 140V sharing 16 GB,
//! llama.cpp Vulkan b10456), the same eight real requests with a clean
//! ~450-token prompt, four tools, thinking off:
//!
//! | model                         | per reply | reads prompt | writes    | tools |
//! |-------------------------------|-----------|--------------|-----------|-------|
//! | Qwen3-VL 4B Q4_K_M (shipped)  | 1.0-3.0 s | 334 tok/s    | 15-23 t/s | 5/5   |
//! | Qwen3.5 4B Q4_K_M             | 2.1-4.8 s | 138 tok/s    | 18-24 t/s | 5/5   |
//! | Qwen3.5 9B IQ4_XS             | 3.5-10.5 s| 80 tok/s     | 9-12 t/s  | 5/5   |
//!
//! The 9B writes better and reads slowly: too slow to talk with, right for
//! research, drafts, the running summary, the night's work and a request of
//! several steps. So it runs as a second llama-server beside the talking one
//! (`models.deep`, on `models.deep_port`), started when such work comes
//! (`DeepBrain::keep` sees a call waiting) and stopped once it has been idle
//! for `models.deep_idle_secs`. Background call sites ask for
//! `Daemon::background_llm`, which is this brain when one is set up and the
//! talking model's other slot (`ChatRequest::aside`, `id_slot` 1) as before
//! when not -- or whenever this one can't run.
//!
//! ## Memory
//!
//! 16 GB is shared by Windows, the programs open, the talking model (about
//! 3 GB with its cache) and the graphics. The 9B needs its weights (5.5 GB)
//! plus its cache (Qwen3.5 caches in one layer of four: about 270 MB at 8,192
//! tokens, `gguf::kv_cache_bytes`) plus a tenth: about 6.4 GB. It is started
//! only when that much *plus* `HEADROOM_MB` is free at that moment; otherwise
//! the work goes to the talking model's other slot, and it looks again a
//! minute later. Never started to then page the machine.
//!
//! ## Never slowing the talking model
//!
//! Both servers would put their work on the one integrated graphics chip,
//! and token generation on it is limited by memory bandwidth, which the two
//! would share: a background write-up running while you ask something makes
//! your answer come slower. Fewer graphics layers for the deep model doesn't
//! help -- the processor reads the same shared memory over the same bus, so
//! it takes the bandwidth anyway, and the deep work gets slower still.
//!
//! So the deep model **yields**: it works only while no turn is being
//! answered. Every talking call holds a `TalkGuard` (the turn's worker, the
//! rewording of a tool's result, a turn answered on the loop, the parts of a
//! request worked side by side). A deep call waits for none to be held
//! before it starts, and one in progress is cut off when a turn begins --
//! within a word while it writes, and within a fifth of a second while it is
//! still reading its prompt (the connection is closed,
//! `models::chat_call_until`), since a long prompt is where it holds the
//! graphics longest. The words so far are kept, and once the turn is
//! answered it carries on from them (sent back as the start of its answer:
//! llama-server continues a last assistant message). How much of the prompt
//! it reads again then depends on llama.cpp's cache for Qwen3.5's mixed
//! layers -- not measured. A request with tools is asked again whole
//! instead: its half-written tool call can't be continued. The deep server
//! isn't even started while a turn is being answered, because loading 5.5 GB
//! competes too.
//!
//! Not covered: a question from the phone answered by this computer's model
//! server holds no guard, so the deep model doesn't give way to it.

use crate::brain::{ChatReply, ChatRequest, Llm, Msg};
use crate::error::{AtlasError, Result};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// The deep model, when nothing else is named: Qwen3.5 9B at IQ4_XS
/// (bartowski's quantisation of Qwen's own weights), the file name without
/// `.gguf`.
pub const DEEP_DEFAULT: &str = "Qwen_Qwen3.5-9B-IQ4_XS";
/// "Better answers": Qwen3.5 4B at Q4_K_M.
pub const BETTER_TALK: &str = "Qwen_Qwen3.5-4B-Q4_K_M";
/// The shipped talking model, which also reads pictures.
pub const FASTER_TALK: &str = "Qwen3VL-4B-Instruct-Q4_K_M";

/// Free memory kept beyond what the deep model needs before it may start.
pub const HEADROOM_MB: u64 = 1024;
/// How long a refusal for memory stands before it looks again.
pub const RECHECK_SECS: u64 = 60;
/// How long a deep call waits for the server to load before the talking
/// model's other slot takes it.
pub const LOAD_WAIT: Duration = Duration::from_secs(150);
/// The longest a deep call waits for turns to finish before going ahead
/// anyway: a conversation that never pauses must not starve the work.
pub const MOST_YIELD: Duration = Duration::from_secs(300);
/// What a one-prompt call may write, on the deep model.
pub const COMPLETE_TOKENS: u32 = 1024;

/// Where the deep server is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Not running; started when work comes.
    Off,
    /// Loading its model.
    Starting,
    /// Answering.
    Up,
    /// Can't run now (not enough memory, it failed to start): work goes to
    /// the talking model's other slot. Looked at again later.
    Unavailable,
}

impl State {
    fn from_u8(v: u8) -> State {
        match v {
            1 => State::Starting,
            2 => State::Up,
            3 => State::Unavailable,
            _ => State::Off,
        }
    }
    fn as_u8(self) -> u8 {
        match self {
            State::Off => 0,
            State::Starting => 1,
            State::Up => 2,
            State::Unavailable => 3,
        }
    }
}

/// What the daemon and every deep call share: whether a turn is being
/// answered, how many deep calls are waiting or running, and where the
/// server is.
#[derive(Default)]
pub struct Gate {
    talking: AtomicUsize,
    in_flight: AtomicUsize,
    state: AtomicU8,
    why_not: Mutex<String>,
    /// Times a deep call was cut off for a turn, and calls the deep model
    /// answered / the talking model answered instead -- for the log and the
    /// tests.
    pub yields: AtomicUsize,
    pub served: AtomicUsize,
    pub fell_back: AtomicUsize,
}

impl Gate {
    pub fn new() -> Arc<Gate> {
        Arc::new(Gate::default())
    }

    /// A turn is being answered for as long as the guard lives.
    pub fn talk(self: &Arc<Gate>) -> TalkGuard {
        self.talking.fetch_add(1, Ordering::SeqCst);
        TalkGuard(self.clone())
    }

    fn talking(&self) -> bool {
        self.talking.load(Ordering::SeqCst) > 0
    }

    fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::SeqCst)
    }

    pub fn state(&self) -> State {
        State::from_u8(self.state.load(Ordering::SeqCst))
    }

    fn set(&self, s: State, why: &str) {
        self.state.store(s.as_u8(), Ordering::SeqCst);
        if let Ok(mut w) = self.why_not.lock() {
            *w = why.to_string();
        }
    }

    /// Why the deep model can't run, when it can't.
    pub fn why_not(&self) -> String {
        self.why_not.lock().map(|w| w.clone()).unwrap_or_default()
    }

    fn begin(self: &Arc<Gate>) -> Busy {
        self.in_flight.fetch_add(1, Ordering::SeqCst);
        Busy(self.clone())
    }

    /// Wait for the server to be up, as long as `most`. `false`: it can't
    /// be had (unavailable, or it didn't come up in time).
    fn wait_up(&self, most: Duration) -> bool {
        let until = Instant::now() + most;
        loop {
            match self.state() {
                State::Up => return true,
                State::Unavailable => return false,
                State::Off | State::Starting => {}
            }
            if Instant::now() >= until {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Wait while a turn is being answered, as long as `most`.
    fn wait_quiet(&self, most: Duration) {
        let until = Instant::now() + most;
        while self.talking() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

/// Held while a turn is being answered (`Gate::talk`).
pub struct TalkGuard(Arc<Gate>);

impl Drop for TalkGuard {
    fn drop(&mut self) {
        self.0.talking.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Held while a deep call waits or runs.
struct Busy(Arc<Gate>);

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.in_flight.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The model background work is given: the deep server when it can be had,
/// else the talking model's other slot.
pub struct DeepLlm {
    /// The deep server's connection.
    pub deep: Arc<dyn Llm>,
    /// The talking model, for when the deep one can't be had.
    pub talk: Arc<dyn Llm>,
    pub gate: Arc<Gate>,
    /// How long to wait for the deep server to load.
    pub load_wait: Duration,
}

impl DeepLlm {
    fn by_talk(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> Result<ChatReply> {
        self.gate.fell_back.fetch_add(1, Ordering::SeqCst);
        let mut r = req.clone();
        // Beside the conversation, never in its slot.
        r.aside = true;
        self.talk.chat(&r, on_text)
    }

    /// A call to the deep server that gives way to every turn.
    fn yielding(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> Result<ChatReply> {
        let mut cur = req.clone();
        let mut so_far = String::new();
        loop {
            self.gate.wait_quiet(MOST_YIELD);
            // Waited its whole allowance and a turn is still going: this
            // round goes on regardless, so the work isn't starved.
            let may_yield = !self.gate.talking();
            let mut yielded = false;
            let mut stopped = false;
            let mut this_round = String::new();
            let gate = &self.gate;
            // Asked while it reads the prompt as well, when no word comes.
            let cut = std::cell::Cell::new(false);
            let keep_going = || {
                let go = !(may_yield && gate.talking());
                if !go {
                    cut.set(true);
                }
                go
            };
            let r = self.deep.chat_until(
                &cur,
                &mut |piece| {
                    // A turn starting cuts in.
                    if may_yield && gate.talking() {
                        yielded = true;
                        return false;
                    }
                    this_round.push_str(piece);
                    if !on_text(piece) {
                        stopped = true;
                        return false;
                    }
                    true
                },
                &keep_going,
            );
            let yielded = yielded || cut.get();
            match r {
                Ok(reply) if yielded && !stopped => {
                    self.gate.yields.fetch_add(1, Ordering::SeqCst);
                    if req.tools.is_empty() {
                        // Carry on from the words so far.
                        so_far.push_str(&this_round);
                        cur = req.clone();
                        cur.messages.push(Msg::assistant(so_far.clone()));
                    } else {
                        // A half-written tool call can't be continued: asked
                        // again whole (its prompt is still cached).
                        let _ = reply;
                        so_far.clear();
                        cur = req.clone();
                    }
                }
                Ok(mut reply) => {
                    self.gate.served.fetch_add(1, Ordering::SeqCst);
                    if !so_far.is_empty() {
                        reply.text = format!("{so_far}{}", reply.text);
                    }
                    return Ok(reply);
                }
                // Nothing from it yet: the talking model answers instead.
                Err(_) if so_far.is_empty() && this_round.is_empty() => return self.by_talk(req, on_text),
                Err(e) => return Err(e),
            }
        }
    }
}

impl Llm for DeepLlm {
    fn complete(&self, system: &str, user: &str) -> Result<String> {
        let _busy = self.gate.begin();
        if !self.gate.wait_up(self.load_wait) {
            self.gate.fell_back.fetch_add(1, Ordering::SeqCst);
            return self.talk.complete(system, user);
        }
        let req = ChatRequest {
            messages: vec![Msg::system(system), Msg::user(user)],
            max_tokens: COMPLETE_TOKENS,
            aside: true,
            ..Default::default()
        };
        let reply = self.yielding(&req, &mut |_| true)?;
        if reply.text.trim().is_empty() && !reply.tool_calls.is_empty() {
            return Err(AtlasError::Platform("the deep model answered with a tool call, not words".into()));
        }
        Ok(reply.text)
    }

    fn native_chat(&self) -> bool {
        true
    }

    fn chat(&self, req: &ChatRequest, on_text: &mut dyn FnMut(&str) -> bool) -> Result<ChatReply> {
        let _busy = self.gate.begin();
        if !self.gate.wait_up(self.load_wait) {
            return self.by_talk(req, on_text);
        }
        self.yielding(req, on_text)
    }

    fn has_stronger(&self) -> bool {
        self.talk.has_stronger()
    }

    fn complete_hard(&self, system: &str, user: &str) -> Result<String> {
        if self.talk.has_stronger() {
            self.talk.complete_hard(system, user)
        } else {
            self.complete(system, user)
        }
    }
}

/// The deep server's process, as `DeepBrain` drives it. A real llama-server
/// (`ServerEngine`), or a scripted one in the tests.
pub trait Engine: Send {
    /// Start it; returns once it's launched, not loaded.
    fn start(&mut self) -> std::result::Result<(), String>;
    /// Stop it.
    fn stop(&mut self);
    /// Is the process still there?
    fn running(&mut self) -> bool;
    /// Is it answering (`/health` says ok)? Quick: at most a fraction of a
    /// second, it's asked from the loop.
    fn healthy(&mut self) -> bool;
}

/// A llama-server for the deep model.
pub struct ServerEngine {
    pub model: crate::models::Model,
    /// The models settings with the deep port and context in place.
    pub cfg: crate::models::ModelsConfig,
    pub gpu_layers: u32,
    pub vars: crate::tools::Vars,
    child: Option<std::process::Child>,
}

impl ServerEngine {
    pub fn new(model: crate::models::Model, cfg: crate::models::ModelsConfig, gpu_layers: u32, vars: crate::tools::Vars) -> ServerEngine {
        ServerEngine { model, cfg, gpu_layers, vars, child: None }
    }
}

impl Engine for ServerEngine {
    fn start(&mut self) -> std::result::Result<(), String> {
        let child = crate::models::launch_logging(&self.model, &self.cfg, self.gpu_layers, &self.vars, "deep-model-server.log")
            .map_err(|e| e.to_string())?;
        self.child = Some(child);
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(mut c) = self.child.take() {
            let _ = c.kill();
            let _ = c.wait();
        }
    }

    fn running(&mut self) -> bool {
        match self.child.as_mut() {
            Some(c) => matches!(c.try_wait(), Ok(None)),
            None => false,
        }
    }

    fn healthy(&mut self) -> bool {
        healthy_at(self.cfg.port, Duration::from_millis(300))
    }
}

impl Drop for ServerEngine {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Does a llama-server on this machine's `port` say it's ready?
pub fn healthy_at(port: u16, wait: Duration) -> bool {
    crate::http::get(&format!("127.0.0.1:{port}"), "/health", wait)
        .map(|r| r.ok() && crate::models::health_says_ok(&r.body))
        .unwrap_or(false)
}

/// The deep model, and when it runs.
pub struct DeepBrain {
    pub gate: Arc<Gate>,
    engine: Option<Box<dyn Engine>>,
    /// Its connection, for `DeepLlm::deep`.
    conn: Option<Arc<dyn Llm>>,
    pub model_id: String,
    /// Memory it needs to run, megabytes.
    pub need_mb: u64,
    idle: Duration,
    free_mb: Box<dyn Fn() -> u64 + Send>,
    last_busy: Instant,
    started: Option<Instant>,
    look_again: Option<Instant>,
    /// How long a load may take before it's given up on.
    pub load_limit: Duration,
}

impl DeepBrain {
    /// No deep model: background work shares the talking model.
    pub fn none() -> DeepBrain {
        let gate = Gate::new();
        gate.set(State::Unavailable, "no deep model is set up");
        DeepBrain {
            gate,
            engine: None,
            conn: None,
            model_id: String::new(),
            need_mb: 0,
            idle: Duration::from_secs(300),
            free_mb: Box::new(|| 0),
            last_busy: Instant::now(),
            started: None,
            look_again: None,
            load_limit: LOAD_WAIT,
        }
    }

    /// A deep model run by `engine`, reached through `conn`, needing
    /// `need_mb`, stopped after `idle` with nothing to do; `free_mb` measures
    /// what's free now.
    pub fn new(
        engine: Box<dyn Engine>,
        conn: Arc<dyn Llm>,
        model_id: &str,
        need_mb: u64,
        idle: Duration,
        free_mb: Box<dyn Fn() -> u64 + Send>,
    ) -> DeepBrain {
        DeepBrain {
            gate: Gate::new(),
            engine: Some(engine),
            conn: Some(conn),
            model_id: model_id.to_string(),
            need_mb,
            idle,
            free_mb,
            last_busy: Instant::now(),
            started: None,
            look_again: None,
            load_limit: LOAD_WAIT,
        }
    }

    /// Is a deep model set up (whether or not it's running)?
    pub fn is_set_up(&self) -> bool {
        self.engine.is_some()
    }

    /// The model for background work: this one when set up, else `talk`.
    pub fn for_background(&self, talk: Arc<dyn Llm>) -> Arc<dyn Llm> {
        match &self.conn {
            Some(deep) => Arc::new(DeepLlm { deep: deep.clone(), talk, gate: self.gate.clone(), load_wait: self.load_limit + Duration::from_secs(5) }),
            None => talk,
        }
    }

    /// Once a pass of the loop: start the server when work is waiting and
    /// there's room, mark it up once it answers, stop it when it has been
    /// idle. What happened, for the log. Waits at most a health check.
    pub fn keep(&mut self) -> Vec<String> {
        let mut said = Vec::new();
        let Some(engine) = self.engine.as_mut() else { return said };
        let busy = self.gate.in_flight() > 0;
        if busy {
            self.last_busy = Instant::now();
        }
        match self.gate.state() {
            State::Off | State::Unavailable => {
                if !busy {
                    return said;
                }
                if self.look_again.is_some_and(|at| Instant::now() < at) {
                    return said;
                }
                // Not while a turn is being answered: loading competes too.
                if self.gate.talking() {
                    return said;
                }
                let free = (self.free_mb)();
                if free < self.need_mb + HEADROOM_MB {
                    let why = format!(
                        "the deep model ({}) needs about {} MB and {} MB is free; background work shares the talking model until there's room",
                        self.model_id, self.need_mb + HEADROOM_MB, free
                    );
                    if self.gate.why_not() != why {
                        said.push(why.clone());
                    }
                    self.gate.set(State::Unavailable, &why);
                    self.look_again = Some(Instant::now() + Duration::from_secs(RECHECK_SECS));
                    return said;
                }
                match engine.start() {
                    Ok(()) => {
                        self.started = Some(Instant::now());
                        self.gate.set(State::Starting, "");
                        said.push(format!("starting the deep model ({}) for background work", self.model_id));
                    }
                    Err(e) => {
                        let why = format!("the deep model couldn't start: {e}");
                        said.push(why.clone());
                        self.gate.set(State::Unavailable, &why);
                        self.look_again = Some(Instant::now() + Duration::from_secs(RECHECK_SECS));
                    }
                }
            }
            State::Starting => {
                if !engine.running() {
                    let why = "the deep model stopped while loading".to_string();
                    said.push(why.clone());
                    self.gate.set(State::Unavailable, &why);
                    self.look_again = Some(Instant::now() + Duration::from_secs(RECHECK_SECS));
                    self.started = None;
                } else if engine.healthy() {
                    self.gate.set(State::Up, "");
                    let took = self.started.map(|s| s.elapsed().as_millis()).unwrap_or(0);
                    said.push(format!("the deep model is up ({}ms to load)", took));
                } else if self.started.is_some_and(|s| s.elapsed() > self.load_limit) {
                    engine.stop();
                    let why = "the deep model took too long to load, so I stopped it".to_string();
                    said.push(why.clone());
                    self.gate.set(State::Unavailable, &why);
                    self.look_again = Some(Instant::now() + Duration::from_secs(RECHECK_SECS));
                    self.started = None;
                }
            }
            State::Up => {
                if !engine.running() {
                    said.push("the deep model stopped; it starts again when there's work".into());
                    self.gate.set(State::Off, "");
                    self.started = None;
                } else if !busy && self.last_busy.elapsed() >= self.idle {
                    engine.stop();
                    self.gate.set(State::Off, "");
                    self.started = None;
                    said.push(format!("stopped the deep model after {}s with nothing to do", self.idle.as_secs()));
                }
            }
        }
        said
    }

    /// Stop it now (Atlas closing, the setting switched off).
    pub fn stop(&mut self) {
        if let Some(e) = self.engine.as_mut() {
            e.stop();
        }
        if self.engine.is_some() {
            self.gate.set(State::Off, "");
        }
    }

    /// Where it is, in a line for "which model".
    pub fn describe(&self) -> String {
        if !self.is_set_up() {
            return "No deep model: background work shares the talking model.".into();
        }
        match self.gate.state() {
            State::Off => format!("Deep model {}: not running; it starts when background work comes.", self.model_id),
            State::Starting => format!("Deep model {}: loading.", self.model_id),
            State::Up => format!("Deep model {}: running.", self.model_id),
            State::Unavailable => format!("Deep model {}: not now -- {}.", self.model_id, self.gate.why_not()),
        }
    }
}

impl Drop for DeepBrain {
    fn drop(&mut self) {
        self.stop();
    }
}

/// The deep model for these settings, when its file is in the models folder
/// and it isn't the talking model: `models.deep`, or `DEEP_DEFAULT`; none for
/// `off`.
pub fn deep_model_among<'r>(registry: &'r crate::models::Registry, cfg: &crate::models::ModelsConfig) -> Option<&'r crate::models::Model> {
    let id = match cfg.deep.trim() {
        w if w.eq_ignore_ascii_case("off") || w.eq_ignore_ascii_case("none") => return None,
        "" => DEEP_DEFAULT.to_string(),
        id => id.trim_end_matches(".gguf").to_string(),
    };
    let talking = crate::models::talk_id(cfg);
    registry.get(&id).filter(|m| talking.as_deref() != Some(m.id.as_str()) && !m.is_a_projector())
}

/// The settings the deep server is started with: its port and context.
pub fn deep_settings(cfg: &crate::models::ModelsConfig) -> crate::models::ModelsConfig {
    let mut d = cfg.clone();
    d.port = if cfg.deep_port == 0 { cfg.port.saturating_add(1) } else { cfg.deep_port };
    d.context = cfg.deep_context;
    // No helper model: it's paired with the talking model.
    d.draft = String::new();
    d
}

/// What the deep model needs, megabytes: read from its file (weights, cache
/// at its context, a tenth more), else estimated from its size.
fn need_mb(model: &crate::models::Model, cfg: &crate::models::ModelsConfig) -> u64 {
    let ctx = cfg.deep_context.max(1024);
    model.memory_needed(ctx).unwrap_or_else(|_| crate::models::estimate_memory(model, ctx)) / (1024 * 1024)
}

/// The deep brain for Atlas's own settings, when one is set up: a
/// llama-server for the deep model on its own port. `DeepBrain::none` when
/// there's no deep model file, it's switched off, or the talking model is
/// hand-written (`tools.llm`: yours to run).
pub fn for_settings(tc: &crate::voice::ToolsConfig) -> DeepBrain {
    if tc.llm.is_some() || tc.models.server.is_none() {
        return DeepBrain::none();
    }
    let (registry, _) = crate::models::Registry::scan_reporting(&crate::models::Registry::dir_for(&tc.models));
    let Some(model) = deep_model_among(&registry, &tc.models).cloned() else { return DeepBrain::none() };
    let dcfg = deep_settings(&tc.models);
    let machine = crate::fit::measure();
    let layers = crate::models::layers_here(&model, &dcfg, &machine);
    let conn: Arc<dyn Llm> = Arc::new(crate::brain::ShellLlm {
        cfg: crate::models::llm_config_for(&model, &dcfg, &crate::models::server_post()),
        vars: tc.vars.clone(),
    });
    let need = need_mb(&model, &dcfg);
    let engine = ServerEngine::new(model.clone(), dcfg.clone(), layers, tc.vars.clone());
    DeepBrain::new(
        Box::new(engine),
        conn,
        &model.id,
        need,
        Duration::from_secs(tc.models.deep_idle_secs.max(30)),
        Box::new(|| crate::fit::measure().free_ram_mb),
    )
}

/// "Use the better model" / "use the faster model", and the like: which one
/// was asked for (`true` better, `false` faster).
pub fn asks_for_talk_model(said: &str) -> Option<bool> {
    let t: String = said.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' }).collect();
    let t = format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "));
    const BETTER: &[&str] = &[
        " use the better model ", " switch to the better model ", " use the smarter model ", " switch to the smarter model ",
        " better answers please ", " turn on better answers ", " give me better answers ", " use better answers ",
    ];
    const FASTER: &[&str] = &[
        " use the faster model ", " switch to the faster model ", " use the quicker model ", " switch to the quicker model ",
        " faster answers please ", " turn off better answers ", " give me faster answers ", " use faster answers ",
    ];
    if BETTER.iter().any(|p| t.contains(p)) {
        Some(true)
    } else if FASTER.iter().any(|p| t.contains(p)) {
        Some(false)
    } else {
        None
    }
}
