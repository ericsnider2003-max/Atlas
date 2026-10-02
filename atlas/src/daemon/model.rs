//! Which model answers and keeping its server up, asking the room of seats, the
//! record of model calls, the notes index and meaning search, learning what you
//! state, the self-check, and answering from facts and notes.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// Which model Atlas would run here, and why.
    ///
    /// `models.rs` calls itself "the rest of what Ollama does, in Rust" —
    /// find the models on disk, read their metadata, work out what fits, and
    /// run `llama-server` directly so there is one fewer piece of someone
    /// else's software in the path. Every part of that was written and none of
    /// it was ever called: nothing scanned a real folder, and the budget it
    /// sized against was a number typed into a config file rather than the
    /// memory `fit.rs` already measures.
    pub fn which_model(&mut self) -> String {
        let cfg = self.tools_cfg().models.clone();
        let dir = std::path::PathBuf::from(&cfg.dir);
        let (registry, trouble) = crate::models::Registry::scan_reporting(&crate::roots::under_install(&dir));

        // Reported, not swallowed. An unreadable folder and an empty one both
        // produce no models, and they send you to very different places —
        // `scan_reporting` exists for exactly this and had no caller.
        if let Some(why) = trouble {
            return format!("I can't read the models folder: {why}");
        }

        let machine = crate::fit::measure();
        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
        // Already running: its memory is spent, and "nothing fits" would be
        // untrue about a model that's answering right now.
        if crate::models::is_running(&cfg, &crate::models::server_get(), &vars) {
            if let Some(m) = registry.choose_for(&cfg, u64::MAX) {
                return format!("{} — it's running now, on this machine.", m.id);
            }
        }
        let budget = crate::models::budget_bytes(&cfg, &machine);
        let mut said = registry.explain_for(&cfg, &machine, budget);

        // What it would take to actually run it, which is a different question
        // from what fits.
        let chosen = registry.choose_for(&cfg, budget);
        if chosen.is_some() && cfg.server.is_none() {
            said.push_str(" I've no llama-server configured, so I can't run it myself yet.");
        }

        // Whether it is up right now. Asked through the configured HTTP tool
        // rather than an HTTP client of its own, like everything else here.
        if let (Some(model), Some(http)) = (chosen, self.http_tool()) {
            let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
            if crate::models::is_running(&cfg, &http, &vars) {
                said.push_str(" It's up.");
            } else if cfg.server.is_some() {
                // It used to stop at "I'd start it", which is the shape of
                // hollowness this tree keeps finding: a sentence describing
                // an action nothing performs. `models::launch` was written,
                // tested, and had no caller anywhere in `src`. Now the
                // sentence is a report of something that happened.
                let layers = crate::models::layers_here(model, &cfg, &machine);
                // The same estimate `best_fit` sized against, so what Atlas
                // says it will take and what it chose by cannot disagree.
                let mb = crate::models::footprint_mb(model, &cfg);
                let model = model.clone();
                match self.start_model_server(&model, &cfg, layers, mb) {
                    Ok(evicted) => {
                        self.model_running_id = Some(model.id.clone());
                        said.push_str(&format!(
                            " It wasn't running, so I've started it with {layers} layers on \
                             the graphics, taking about {mb}MB."
                        ));
                        for e in evicted {
                            said.push_str(&format!(" I stopped {e} to make room."));
                        }
                    }
                    Err(why) => said.push_str(&format!(" Not running, and I can't start it: {why}")),
                }
            }
        }
        said
    }

    /// The model for work nobody is waiting on word by word -- research,
    /// drafts, summaries, the night's work, a request of several steps
    /// (30 Sep 2026): the deep model when one is set up (`deepbrain`), which
    /// falls back to the talking model's other slot whenever it can't run;
    /// the talking model itself when there's none. `None` with no model.
    pub(crate) fn background_llm(&self) -> Option<std::sync::Arc<dyn Llm>> {
        let talk = self.llm.clone()?;
        Some(self.deep.for_background(talk))
    }

    /// Give this Atlas a deep model (the tests' scripted one; the running
    /// Atlas sets up its own from the settings, `keep_model_server`).
    pub fn use_deep_brain_for_test(&mut self, deep: crate::deepbrain::DeepBrain) {
        self.deep = deep;
    }

    /// How a coding agent's program is looked for, for the tests: a stand-in
    /// for this machine's PATH (`coding_agent`).
    pub fn find_coding_agents_with_for_test(&mut self, lookup: fn(&str) -> Option<String>) {
        self.agent_lookup = lookup;
    }

    /// Where the deep model is and what it has done, for the tests.
    pub fn deep_gate_for_test(&self) -> std::sync::Arc<crate::deepbrain::Gate> {
        self.deep.gate.clone()
    }

    /// A turn is being answered while this is held: the deep model gives
    /// way (`deepbrain`).
    pub(crate) fn talking_guard(&self) -> (crate::deepbrain::TalkGuard, crate::deepbrain::TalkGuard) {
        // The coding model gives way too (2 Oct 2026, `coder`): it shares
        // the graphics the same way.
        (self.deep.gate.talk(), self.coder.brain.gate.talk())
    }

    /// Give this Atlas a coding model (the tests' scripted one; the running
    /// Atlas sets up its own from the settings, `keep_model_server`).
    pub fn use_coder_for_test(&mut self, coder: crate::coder::Coder) {
        self.coder = coder;
    }


    /// The deep model, looked after once a pass: started for waiting work
    /// when there's room, stopped once idle. What it did goes in the log.
    pub(super) fn keep_deep_brain(&mut self) {
        for line in self.deep.keep() {
            self.log.info(&line);
        }
        self.keep_coder();
    }

    /// The coding model, looked after once a pass beside the deep one: room
    /// made for it by letting the talking model go when a build waits and
    /// both don't fit, started, stopped once idle, and the talking model
    /// given back (`coder::Coder::keep`).
    pub(super) fn keep_coder(&mut self) {
        if !self.coder.is_set_up() {
            return;
        }
        for line in self.with_chat_room(|coder, room| coder.keep(room)) {
            self.log.info(&line);
        }
    }

    /// The coding model and the talking model's room, together
    /// (`DaemonRoom`): borrowed apart, so the coding model can let the
    /// talking model go or bring it back.
    fn with_chat_room<R>(&mut self, f: impl FnOnce(&mut crate::coder::Coder, &mut DaemonRoom) -> R) -> R {
        let talking = self.deep.gate.talking();
        let mut room = DaemonRoom {
            helpers: &mut self.helpers,
            rested: &mut self.model_rested,
            running_id: &mut self.model_running_id,
            started: &mut self.model_started,
            start_tried: &mut self.model_start_tried,
            seen: &self.model_server_seen,
            talking,
        };
        f(&mut self.coder, &mut room)
    }

    /// A turn is starting while the talking model is let go for the coding
    /// model: the coding model steps aside (`coder::Coder::give_way_to_a_turn`).
    fn coder_gives_way(&mut self) {
        if !self.coder.holds_chat_room {
            return;
        }
        if let Some(line) = self.with_chat_room(|coder, room| coder.give_way_to_a_turn(room)) {
            self.log.info(&line);
        }
    }

    /// Make sure the model server Atlas's own connection talks to is up, or
    /// on its way up; and if it's Atlas's, that it stays warm.
    ///
    /// Only for the connection Atlas builds itself: a hand-written `llm:`
    /// (Ollama, your own server) is yours to run. A server someone started
    /// by hand is left alone. Nothing is waited for here.
    pub(super) fn keep_model_server(&mut self, t: u64) {
        self.keep_model_server_waiting(t, MODEL_PROBE_WAIT, true);
    }

    /// At typing-only, look once a minute at whether the voice tools work
    /// now, and come back to listening if they do (29 Sep 2026). The check
    /// was made once at start: a download still finishing, or a tool briefly
    /// held by antivirus, left Atlas deaf and silent for the whole session.
    pub(super) fn look_again_at_audio(&mut self, ears: &dyn Ears, t: u64) {
        if !self.audio_tools_missing || self.tiers.tier != Tier::Typed || t < self.audio_look_at {
            return;
        }
        self.audio_look_at = t + 60;
        if crate::input::audio_available(self.tools_ref()).unwrap_or(false) {
            self.audio_tools_missing = false;
            if let Some(m) = self.tiers.heard_you() {
                self.log.info(&format!("the voice tools are there now. {m}"));
            }
            self.ensure_mic(ears);
            self.steer_mic();
        }
    }

    /// List the sound devices every few minutes, off the loop, and record
    /// from a different microphone when the pick changes: a headset that
    /// connected after sign-in, a dock, the picked one unplugged (29 Sep
    /// 2026: the microphone was picked once, at start, for the whole
    /// session). A listing that fails changes nothing.
    pub(super) fn look_again_at_the_microphone(&mut self, t: u64) {
        if let Some(rx) = &self.mic_listing {
            match rx.try_recv() {
                Ok(Ok(devices)) => {
                    self.mic_listing = None;
                    self.microphone_from(&devices, t);
                }
                Ok(Err(e)) => {
                    self.mic_listing = None;
                    self.log.warn(&format!("couldn't list the microphones this time: {e}"));
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.mic_listing = None,
            }
            return;
        }
        if t < self.mic_look_at || self.tiers.tier == Tier::Typed && self.audio_tools_missing {
            return;
        }
        self.mic_look_at = t + MIC_LOOK_EVERY_SECS;
        let Some(tc) = self.tools_ref() else { return };
        let ffmpeg = tc.vars.get("ffmpeg").cloned().unwrap_or_else(|| "ffmpeg".into());
        let (tx, rx) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new().name("atlas-mic-list".into()).spawn(move || {
            let _ = tx.send(crate::audio::probe_devices(&ffmpeg).map_err(|e| e.to_string()));
        });
        if spawned.is_ok() {
            self.mic_listing = Some(rx);
        }
    }

    /// The pick from a fresh listing; switched to, and said, when it differs.
    fn microphone_from(&mut self, devices: &[crate::audio::Device], t: u64) {
        let Some(tc) = self.tools_ref().cloned() else { return };
        let screens = self.plat.monitors();
        let w = crate::hearing::Where {
            at_desk: screens.as_ref().map(|m| !m.is_empty()).unwrap_or(true),
            presence_unknown: screens.is_err(),
            headset_connected: devices.iter().any(|d| d.kind == crate::audio::Kind::Input && crate::hearing::Candidate::from(d).bluetooth),
            phone_active: false,
            audio_playing: false,
        };
        let store = crate::roots::store();
        let mut hearing = crate::hearing::Hearing::load_from(&store);
        hearing.observe_devices(devices);
        hearing.learn_levels(&crate::leveller::remembered());
        let laptop_active = self.plat.built_in_screen_on().unwrap_or(true);
        let Some(p) = crate::hearing::pick_microphone(devices, &mut hearing, &tc, &w, laptop_active, t) else { return };
        let _ = hearing.save_to(&store);
        let (now_name, now_device) = crate::voice::microphone_now(&tc);
        if let Some(line) = microphone_change(&now_name, &now_device, &p) {
            crate::voice::set_microphone(&p.name, &p.device);
            // A different microphone: if the last one failed the wake word,
            // try this one now rather than at the next turn of the clock.
            self.mic_probe_at = 0;
            self.log.info(&line);
            self.heard_note = Some(line);
        }
    }

    /// `keep_model_server`, waiting at most `wait` for the "is it up?"
    /// answer. The loop calls it with no wait on every pass (29 Sep 2026):
    /// at start-up the first check hadn't answered in time, and nothing
    /// asked again until you said something -- so with the microphone not
    /// working, the model never started at all.
    pub(super) fn keep_model_server_waiting(&mut self, t: u64, wait: std::time::Duration, a_turn: bool) {
        // No model when Atlas started (none downloaded yet, or none judged to
        // fit): look again once a minute rather than for the rest of the
        // session. Setup fetches the model while Atlas is already running.
        if self.starts_model_server && self.llm.is_none() && t >= self.model_look_at {
            self.model_look_at = t + 60;
            if let Some(tc) = self.tools_ref().cloned() {
                if tc.llm.is_none() {
                    self.llm = crate::models::connection(&tc);
                    // Why not, said once and kept for the replies.
                    if self.llm.is_none() {
                        let why = crate::models::why_no_model(&tc.models);
                        if self.model_server_trouble.as_deref() != Some(why.as_str()) {
                            self.log.warn(&format!("no language model: {why}"));
                            self.model_server_trouble = Some(why);
                        }
                    } else if self.model_server_trouble.is_some() {
                        self.model_server_trouble = None;
                    }
                }
            }
        }
        if !self.starts_model_server || self.llm.is_none() {
            return;
        }
        // The deep model beside it, once its file is in the models folder
        // (looked for once a minute: "Get the deep brain" fetches it while
        // Atlas runs).
        if !self.deep.is_set_up() && t >= self.deep_look_at {
            self.deep_look_at = t + 60;
            if let Some(tc) = self.tools_ref().cloned() {
                self.deep = crate::deepbrain::for_settings(&tc);
                if self.deep.is_set_up() {
                    self.log.info(&self.deep.describe());
                }
            }
        }
        // The coding model likewise (2 Oct 2026, `coder`): set up once its
        // file is here, looked for once a minute.
        if !self.coder.is_set_up() && t >= self.coder_look_at {
            self.coder_look_at = t + 60;
            if let Some(tc) = self.tools_ref().cloned() {
                self.coder = crate::coder::for_settings(&tc);
                if self.coder.is_set_up() {
                    self.log.info(&self.coder.brain.describe());
                }
            }
        }
        // The talking model let go for a build: a pass leaves it be, a turn
        // takes it back and the coding model steps aside.
        if self.coder.holds_chat_room {
            if !a_turn {
                return;
            }
            self.coder_gives_way();
        }
        // "Better answers" switched: the server follows the setting.
        self.follow_the_talk_setting();
        let Some(tc) = self.tools_ref() else { return };
        if tc.llm.is_some() || tc.models.server.is_none() {
            return;
        }
        let cfg = tc.models.clone();
        let vars = tc.vars.clone();
        if self.helpers.is_running("model-server") {
            // A call to it failed: is it answering at all? Not while it may
            // still be loading. One that isn't is stopped, and the next turn
            // starts a fresh one (29 Sep 2026: a running process was trusted
            // however stuck it was, and every question waited out the whole
            // read timeout, twice, before failing).
            let loading = self.model_started.is_some_and(|s| s.elapsed() < MODEL_SERVER_LOADING);
            if self.model_suspect && !loading {
                // (The finding is cleared on every pass that isn't asking,
                // below, so what this reads is this question's answer.)
                match self.probe_model_server(&cfg, &vars, wait) {
                    Some(false) => {
                        self.model_suspect = false;
                        self.log.warn("my model server is running but not answering; restarting it");
                        self.helpers.finished("model-server");
                        self.model_started = None;
                        self.model_start_tried = None;
                        self.model_server_trouble = Some("my language model stopped answering, so I'm restarting it".into());
                        return;
                    }
                    Some(true) => self.model_suspect = false,
                    // Still being asked: looked at again next pass.
                    None => return,
                }
            }
            // Lived through its load and some use: not dying young.
            if self.model_started.is_some_and(|s| s.elapsed() > MODEL_SERVER_YOUNG) {
                self.model_started = None;
                self.model_deaths = 0;
            }
            // Ours and running: a turn counts as using it. A pass of the loop
            // doesn't (29 Sep 2026: every pass counted, so the half-hour
            // keep-warm never ran out and the model held its memory all day).
            if a_turn {
                let _ = self.helpers.want("model-server", 0, t, || Ok(None));
                self.helpers.done("model-server", t);
            }
            // Ours is the one answering: once it is let go, the next turn
            // asks afresh rather than trusting a minute-old "it's up".
            if let Ok(mut seen) = self.model_server_seen.lock() {
                *seen = None;
            }
            return;
        }
        // Let go for being idle: started again by the next thing you say, not
        // by the loop's next pass (Phase 0.2: stopped "idle" and restarted
        // within seconds, thirteen times on 30 Sep, with nobody talking).
        if self.model_rested {
            if !a_turn {
                return;
            }
            self.model_rested = false;
        }
        // Not ours. Whether one is up anyway (started by another Atlas, or by
        // hand) is asked at most once a minute, off this thread, and a turn
        // waits for the answer no more than a moment (28 Sep 2026: every
        // turn scanned the model folder, measured the machine and made an
        // HTTP check of up to ten seconds, on the loop).
        let up = match self.model_server_seen.lock().ok().and_then(|g| *g) {
            Some((at, up)) if at.elapsed() < MODEL_SERVER_RECHECK => up,
            _ => match self.probe_model_server(&cfg, &vars, wait) {
                Some(up) => up,
                // Still asking: decided on a later turn.
                None => return,
            },
        };
        if up {
            return;
        }
        // Tried a moment ago and it didn't start: not again on every turn,
        // and longer each time one dies young.
        if self.model_start_tried.is_some_and(|at| at.elapsed() < model_server_pause(self.model_deaths)) {
            return;
        }
        self.model_start_tried = Some(std::time::Instant::now());
        let (registry, _) = crate::models::Registry::scan_reporting(&crate::models::Registry::dir_for(&cfg));
        let machine = crate::fit::measure();
        let Some(model) = crate::models::pick(&registry, &cfg, &machine).cloned() else {
            // Said once, not silently skipped: this is why nothing answers.
            let why = "no language model in the models folder fits this machine".to_string();
            if self.model_server_trouble.as_deref() != Some(why.as_str()) {
                self.log.warn(&format!("couldn't start the model server: {why}"));
                self.model_server_trouble = Some(why);
            }
            return;
        };
        // One was started before and isn't running now: why, from its own
        // log, before that log is replaced.
        if let Some(was) = &self.model_running_id {
            let words = crate::models::model_server_last_words().unwrap_or_else(|| "its log says nothing".into());
            self.log.warn(&format!("the model server ({was}) isn't running any more; its last words: {words}"));
        }
        let layers = crate::models::layers_here(&model, &cfg, &machine);
        let mb = crate::models::footprint_mb(&model, &cfg);
        let trouble = match self.start_model_server(&model, &cfg, layers, mb) {
            Ok(_) => {
                self.log.info(&format!("started the model server: {} ({layers} layers on the graphics)", model.id));
                // The pause between tries is for a start that failed, not one
                // that worked and was later let go (merge 28 Sep 2026). A
                // start that dies young is caught by `model_server_died`.
                self.model_start_tried = None;
                self.model_started = Some(std::time::Instant::now());
                self.model_running_id = Some(model.id.clone());
                None
            }
            Err(why) => Some(why),
        };
        if trouble.is_some() && trouble != self.model_server_trouble {
            self.log.warn(&format!("couldn't start the model server: {}", trouble.as_deref().unwrap_or("")));
        }
        self.model_server_trouble = trouble;
    }

    /// Our model server stopped on its own (not let go for being idle). One
    /// that died soon after being started is counted, and the next start
    /// waits a doubling pause from now (29 Sep 2026: one that died while
    /// loading -- too many layers for the graphics, the port taken, a bad
    /// file -- was started again on the very next pass, reading the whole
    /// model off the disk every time, and nobody was told).
    pub(super) fn model_server_died(&mut self) {
        let Some(started) = self.model_started.take() else { return };
        if started.elapsed() >= MODEL_SERVER_YOUNG {
            return;
        }
        self.model_deaths = self.model_deaths.saturating_add(1);
        self.model_start_tried = Some(std::time::Instant::now());
        let why = match crate::models::model_server_last_words() {
            Some(w) => format!("my language model stopped soon after starting ({w})"),
            None => "my language model stopped soon after starting".to_string(),
        };
        self.log.warn(&format!(
            "the model server ended soon after starting; trying again in {} min: {why}",
            model_server_pause(self.model_deaths).as_secs() / 60
        ));
        self.model_server_trouble = Some(why);
    }

    /// Ask, on a thread of its own, whether a model server is answering;
    /// wait for the answer up to `MODEL_PROBE_WAIT`. `None` while it is
    /// still being asked -- the answer lands in `model_server_seen`.
    fn probe_model_server(&mut self, cfg: &crate::models::ModelsConfig, vars: &crate::tools::Vars, wait: std::time::Duration) -> Option<bool> {
        use std::sync::atomic::Ordering;
        if !self.model_probe_busy.swap(true, Ordering::SeqCst) {
            let (seen, busy, cfg, vars) =
                (self.model_server_seen.clone(), self.model_probe_busy.clone(), cfg.clone(), vars.clone());
            let spawned = std::thread::Builder::new().name("atlas-model-probe".into()).spawn(move || {
                let up = crate::models::is_running(&cfg, &crate::models::server_get(), &vars);
                if let Ok(mut g) = seen.lock() {
                    *g = Some((std::time::Instant::now(), up));
                }
                busy.store(false, Ordering::SeqCst);
            });
            if spawned.is_err() {
                self.model_probe_busy.store(false, Ordering::SeqCst);
                return None;
            }
        }
        let until = std::time::Instant::now() + wait;
        while std::time::Instant::now() < until {
            if let Some((at, up)) = self.model_server_seen.lock().ok().and_then(|g| *g) {
                if at.elapsed() < MODEL_SERVER_RECHECK {
                    return Some(up);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        None
    }

    /// Start the local model server, under the memory budget.
    ///
    /// The Supervisor's whole reason for existing, finally connected. The
    /// biggest single cost measured in a turn on this laptop was the model
    /// server unloading between turns; `keep_warm_secs` is what stops that,
    /// and it governed nothing because nothing ever told the Supervisor a
    /// server had started. A 4GB helper is also exactly the thing a memory
    /// budget should be allowed to refuse.
    fn start_model_server(
        &mut self,
        model: &crate::models::Model,
        cfg: &crate::models::ModelsConfig,
        gpu_layers: u32,
        footprint_mb: u64,
    ) -> std::result::Result<Vec<String>, String> {
        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
        let t = crate::store::now();
        let model = model.clone();
        let cfg2 = cfg.clone();
        // Counted as nothing against the helpers' budget (600 MB, sized for a
        // browser and a camera), which refused every model outright. How big
        // a model may be is decided where it's chosen, against the memory
        // this machine has free (`models::budget_bytes`); `footprint_mb` is
        // for saying so.
        let _ = footprint_mb;
        let evicted = self.helpers.want("model-server", 0, t, move || {
            crate::models::launch(&model, &cfg2, gpu_layers, &vars)
                .map(Some)
                .map_err(|e| e.to_string())
        })?;
        // Warm, not in use: `keep_warm_secs` decides how long it stays.
        self.helpers.done("model-server", t);
        Ok(evicted)
    }

    /// Room for a one-shot model run (`Helpers::want_heavy`), by what this
    /// machine has free right now. When the talking model's server had to be
    /// stopped for it, its state is let go too, so the next question starts
    /// a fresh one instead of asking a port nobody is behind.
    pub(super) fn room_for_heavy(&mut self, name: &str, memory_mb: u64, t: u64) -> std::result::Result<(), String> {
        let was_running = self.helpers.is_running("model-server");
        let mut asked = 0;
        let mut room = || {
            // Asked again only after the server was stopped: a stopped
            // process gives its memory back within a moment, not at once.
            asked += 1;
            if asked > 1 {
                std::thread::sleep(std::time::Duration::from_millis(800));
            }
            crate::fit::measure().free_ram_mb
        };
        let said = self.helpers.want_heavy(name, memory_mb, t, &mut room)?;
        for s in &said {
            self.log.info(s);
        }
        if was_running && !self.helpers.is_running("model-server") {
            self.model_running_id = None;
            self.model_started = None;
            self.model_start_tried = None;
            if let Ok(mut seen) = self.model_server_seen.lock() {
                *seen = None;
            }
        }
        Ok(())
    }

    /// The HTTP tool for asking the local model server whether it's up.
    ///
    /// Was `research.fetch`, which ships as a headless Chrome: it "checked"
    /// the server by rendering its answer as a web page. See
    /// `models::server_post` for what that did to the model connection.
    fn http_tool(&self) -> Option<crate::tools::ExternalTool> {
        Some(crate::models::server_get())
    }

    /// Put a question to a room of seats that do not agree with each other.
    ///
    /// **This costs one model call per seat** — five for either room. That is
    /// the reason it sat unwired, and it is handled rather than hidden: every
    /// call goes through `record_model_call` as `council`, so `atlas trace`
    /// shows exactly what the room cost and `busiest()` will name it if it
    /// starts being the thing spending your model budget.
    ///
    /// Blind round only. `Round::Open` — where seats read each other and may
    /// revise — is a second five calls, and `council`'s own rule is that Open
    /// may only follow Blind, so stopping after Blind is the design working
    /// rather than half of it.
    pub fn ask_the_room(&mut self, question: &str) -> String {
        let q = question.trim();
        if q.is_empty() {
            return "Ask the room what?".into();
        }
        // Five seats, asked in the background: the deep model's work.
        let Some(llm) = self.background_llm() else {
            return "A council is five different answers to the same question, and I need a \
                    model to get them. I haven't got one configured."
                .into();
        };

        // The machine, measured now rather than remembered. `fit::measure`
        // reads what `health` already takes every tick, so this is free.
        let hardware = crate::council::is_hardware_question(q);
        let machine = crate::fit::measure();

        // Two doors into the same room. A hardware question gets seats that
        // have been handed the measured machine; anything else gets the
        // general room, which is what `convene` has always built.
        // A "should I build this?" or an "is this safe?" gets a room built
        // for that question (`council::room_for`); anything else, the
        // general room.
        let (room, prompts) = if hardware {
            crate::nudge::convene_with(crate::council::hardware_room(&machine, &self.fit), q)
        } else if let Some((_, room)) = crate::council::room_for(q) {
            crate::nudge::convene_with(room, q)
        } else {
            crate::nudge::convene(q)
        };

        // Checked before any seat is asked, not after. Both of these were
        // written and never called: a room that cannot disagree is worth
        // refusing *before* it costs five model calls, not explaining after.
        if !room.is_quorate() {
            return "That room hasn't enough seats to disagree.".into();
        }
        if !room.covers_different_ground() {
            return "Those seats all want the same thing, which is one seat five times.".into();
        }

        let question_owned = q.to_string();
        let q_for_closure = q.to_string();
        let work: crew::Work = Box::new(move |ctl| {
            let mut calls = Vec::new();
            let mut opinions = ask_seats(llm.as_ref(), &prompts, ctl, &mut calls, room.retests);

            // Called off between seats: a room with seats missing because it
            // was stopped is not a room that failed to agree.
            if ctl.stopping() {
                return Err("stopped before every seat answered".into());
            }
            let outcome = if opinions.len() < crate::council::MIN_SEATS {
                CouncilOutcome {
                    calls,
                    text: format!(
                        "Only {} of {} seats answered — not enough to call it a room. The model may be struggling.",
                        opinions.len(),
                        prompts.len()
                    ),
                    needs_guidance: false,
                    technical_failure: true,
                }
            } else {
                let blind_verdict = room.tally(&opinions, crate::council::Round::Blind);

                // `Round::Open` — seats read each other and may revise — only
                // earns its five extra calls the one time the blind round
                // turned out too thin to settle anything: a genuine tie, no
                // tiebreak seat to break it. Anything that already reached a
                // call, blind, is left alone. If the open round *also* fails
                // to reach a call, that is a real deadlock, not a technical
                // gap — `needs_guidance` below is what asks for it.
                let mut opened = false;
                let verdict = if blind_verdict.call.is_none() {
                    let open_prompts = room.open_prompts(&q_for_closure, &opinions);
                    let open_opinions = ask_seats(llm.as_ref(), &open_prompts, ctl, &mut calls, room.retests);
                    if open_opinions.len() >= crate::council::MIN_SEATS {
                        opened = true;
                        opinions = open_opinions;
                        room.tally(&opinions, crate::council::Round::Open)
                    } else {
                        // Too few seats answered the second time to trust it
                        // more than the tie it was meant to break.
                        blind_verdict
                    }
                } else {
                    blind_verdict
                };

                let needs_guidance = verdict.call.is_none();
                let mut said = room.spoken(&verdict);
                if room.retests {
                    said = format!("First look — {said}");
                }
                if opened {
                    said.push_str(
                        " (The room was split on the first pass, so seats read each other and \
                         answered again.)",
                    );
                }

                // Seats that would not commit. `Seat::prompt` tells each one
                // that a seat which will not commit is an empty chair, so
                // when one turns up anyway it is worth saying rather than
                // counting as "it depends".
                let empty = crate::council::empty_chairs(&opinions);
                if !empty.is_empty() {
                    said.push_str(&format!(" {} wouldn't commit.", empty.join(" and ")));
                }

                // A no that comes with what would make it a yes: the
                // proposal is changed that way and put back to both rooms —
                // should it be built, and is it safe — because fixing one
                // worry can open another. Up to `MAX_RETESTS` passes; what
                // is still a no after that is yours to decide.
                let mut needs_guidance = needs_guidance;
                if room.retests {
                    let flat = crate::council::flat_noes(&opinions);
                    if !flat.is_empty() {
                        said.push_str(&format!(
                            " {} said no without saying what would change it.",
                            flat.join(" and ")
                        ));
                    }
                    let mut changes = crate::council::conditions(&opinions);
                    let mut asked_for = changes.clone();
                    let mut pass = 0;
                    while !changes.is_empty() && pass < crate::council::MAX_RETESTS && !ctl.stopping() {
                        pass += 1;
                        let listed: Vec<String> = changes.iter().map(|(seat, c)| format!("{seat}: {c}")).collect();
                        said.push_str(&format!(
                            "{} It would be OK if — {}.",
                            if pass == 1 { "" } else { " Still not there." },
                            listed.join("; ")
                        ));
                        let question = crate::council::amended(&q_for_closure, &asked_for);
                        let mut next: Vec<(String, String)> = Vec::new();
                        let mut parts: Vec<String> = Vec::new();
                        let mut all_called = true;
                        for (name, retest_room) in crate::council::retest_rooms() {
                            let opinions = ask_seats(llm.as_ref(), &retest_room.blind_prompts(&question), ctl, &mut calls, true);
                            if opinions.len() < crate::council::MIN_SEATS {
                                parts.push(format!("{name}? Too few seats answered to say."));
                                all_called = false;
                                continue;
                            }
                            let v = retest_room.tally(&opinions, crate::council::Round::Blind);
                            if v.call.is_none() {
                                all_called = false;
                            }
                            parts.push(format!("{name}? {}", retest_room.spoken(&v)));
                            for c in crate::council::conditions(&opinions) {
                                if !asked_for.iter().any(|(_, have)| have.eq_ignore_ascii_case(&c.1)) {
                                    next.push(c);
                                }
                            }
                        }
                        said.push_str(&format!(" Changed that way — {}", parts.join(" ")));
                        asked_for.extend(next.iter().cloned());
                        changes = next;
                        needs_guidance = !all_called;
                    }
                    if !changes.is_empty() {
                        let left: Vec<String> = changes.iter().map(|(seat, c)| format!("{seat}: {c}")).collect();
                        said.push_str(&format!(
                            " After {} rounds of changes some still say no unless {} — that one's yours to decide.",
                            pass,
                            left.join("; ")
                        ));
                        needs_guidance = true;
                    }
                }
                if hardware {
                    said.push_str(&format!(
                        " They were all looking at: {}.",
                        crate::fit::describe(&machine)
                    ));
                }
                CouncilOutcome { calls, text: said, needs_guidance, technical_failure: false }
            };
            Ok(serde_json::to_string(&outcome).unwrap_or_default())
        });

        if self.hand_off("council", crate::store::now(), work, Some(question_owned), SpeakPolicy::Always) {
            "Asking the room. I'll let you know what they say.".into()
        } else {
            "I'm swamped with background work right now — ask the room again in a moment.".into()
        }
    }

    /// Where the flight recorder writes.
    ///
    /// **In the store**, not at a fixed `data/logs`. The first two versions
    /// used a fixed path and then the store's parent, and both meant every
    /// test that built a `Daemon` wrote into one shared file — two tests
    /// running at once each saw the other's calls, and the repo collected a
    /// log nobody asked for. The store root is the one directory that is
    /// genuinely this install's, which is also what makes it isolated per
    /// test. On a real machine that is `data/state`, which `upgrade::YOURS`
    /// already preserves, so the record survives an update like everything
    /// else Atlas has learned.
    pub fn trace_path(&self) -> std::path::PathBuf {
        crate::trace::log_path(self.store.root())
    }

    /// The model's name, from the body it is actually sent in.
    /// The talking model's name for the model-call log. On the default
    /// setup there's no `llm:` block -- Atlas runs its own server -- and
    /// every call was logged as "none" until 1 Oct 2026, so models couldn't
    /// be compared over time (research report, Stage 1 item 4). The model
    /// the server was started with is the one answering.
    fn model_in_use(&self) -> String {
        if let Some(named) = self.tools_ref().and_then(|t| t.llm.as_ref()).map(|l| crate::trace::model_name(&l.request)) {
            return named;
        }
        if let Some(id) = &self.model_running_id {
            return id.clone();
        }
        let Some(tc) = self.tools_ref() else { return "none".into() };
        let (registry, _) = crate::models::Registry::scan_reporting(&crate::models::Registry::dir_for(&tc.models));
        registry.choose_for(&tc.models, u64::MAX).map(|m| m.id.clone()).unwrap_or_else(|| "none".into())
    }

    /// Write down one model call: who asked, which model, how long, how big,
    /// and whether it worked.
    ///
    /// Never the words. `trace::STORES_NO_CONTENT` is the promise and
    /// `tests/guards.rs` holds it: lengths and timings go to disk, prompts do
    /// not. A record of everything Atlas was asked, sitting unencrypted next
    /// to the notes, is a worse problem than the one this solves.
    ///
    /// A write that fails must not fail the call it is recording, so the
    /// failure goes to the ordinary log and the turn carries on.
    pub(crate) fn record_model_call(
        &mut self,
        asked_by: &str,
        took_ms: u64,
        prompt_chars: usize,
        reply_chars: usize,
        failed: Option<String>,
    ) -> u64 {
        let mut c = crate::trace::Call::new(asked_by, &self.model_in_use(), crate::store::now());
        c.took_ms = took_ms;
        c.prompt_chars = prompt_chars;
        c.reply_chars = reply_chars;
        c.failed = failed;
        c.id = self.trace.next_id();
        let id = c.id;
        if !crate::trace::append(&self.trace_path(), &c) {
            self.log.warn("couldn't write to the model-call log");
        }
        self.trace.record(c);
        crate::trace::keep_bounded(&self.trace_path(), &self.trace);
        id
    }

    /// Keep a graded call's words as an example, scrubbed, when that's
    /// switched on (`trace::TraceConfig`, "Keep graded examples").
    pub(super) fn keep_words(&mut self, id: u64, words: Option<&crate::trace::Words>) {
        let cfg = self.tools_cfg().trace.clone();
        let Some(words) = words.filter(|_| cfg.keep_examples) else { return };
        if let Some(ex) = crate::trace::example_of(&self.trace, id, words) {
            if !crate::trace::keep_example(&self.trace_path(), &ex, cfg.examples_max) {
                self.log.warn("couldn't keep a graded example");
            }
        }
    }

    /// Grade a recorded call, and keep the grade (`trace::grade_and_keep`).
    /// A failed call isn't graded: it produced nothing to judge.
    pub(super) fn grade_call(&mut self, id: u64, good: bool, why: &str) {
        let failed = self.trace.calls.iter().any(|c| c.id == id && !c.ok());
        if failed {
            return;
        }
        let why = (!why.is_empty()).then_some(why);
        if !crate::trace::grade_and_keep(&self.trace_path(), &mut self.trace, id, good, why) {
            self.log.warn("couldn't write a model-call grade");
        }
    }

    /// Where the notes live, as configured.
    ///
    /// One reader, so the library, the index and the drift check can never
    /// end up looking at two different folders and disagreeing about what is
    /// on disk — which would report as drift and be a bug in this function.
    pub fn notes_dir(&self) -> std::path::PathBuf {
        self.tools_ref()
            .map(|t| t.research.clone())
            .unwrap_or_default()
            .resolved(&self.store.install_root())
            .notes_dir
            .into()
    }

    /// Load the master index, building it once if there has never been one.
    ///
    /// A folder that has never been indexed disagrees with nothing, so the
    /// first build is silent. Every build after that is something Eric asked
    /// for, because an index Atlas quietly re-derives is an index that cannot
    /// be wrong, and being able to be wrong is what makes the check mean
    /// anything.
    pub(super) fn load_index(&mut self) {
        let dir = self.notes_dir();
        let folder = dir.to_string_lossy().to_string();
        match crate::contents::load(&folder, &crate::contents::master_path(&dir)) {
            Some(c) => self.contents = c,
            None => {
                // No folder yet is the ordinary case on a fresh install and
                // must not write an empty index file into a directory that
                // does not exist. An empty folder is the same case one step
                // along: an index of nothing describes nothing and cannot
                // drift, and writing one puts a file on disk every time a
                // daemon starts against a folder nobody has used yet.
                self.contents = crate::contents::from_folder(&dir);
                if self.contents.lines.is_empty() {
                    return;
                }
                if let Err(e) = crate::contents::save(&self.contents, &crate::contents::master_path(&dir)) {
                    self.log.warn(&format!("couldn't write the notes index: {e}"));
                }
            }
        }
    }

    /// Where the written-down index and the folder disagree, right now.
    pub fn index_drift(&self) -> crate::contents::Drift {
        crate::contents::drift(&self.contents, &crate::contents::names_on_disk(&self.notes_dir()))
    }

    /// Rebuild the index from the folder and say what changed.
    ///
    /// What the drift nudge offers. Reports against the drift measured
    /// *before* the rebuild, because afterwards there is none by
    /// construction, and "the index matches what's there" is not an answer to
    /// "what did you just do".
    pub fn rebuild_index(&mut self) -> String {
        let dir = self.notes_dir();
        if !dir.is_dir() {
            return "There's no notes folder yet, so there's nothing to index.".into();
        }
        let before = self.index_drift();
        match crate::contents::rebuild(&dir) {
            Ok(c) => {
                let added = before.unlisted.len();
                let dropped = before.missing.len();
                let blank = c.useless_lines().len();
                self.contents = c;
                let mut said = if added == 0 && dropped == 0 {
                    "Rebuilt the index. It already matched.".to_string()
                } else {
                    format!(
                        "Rebuilt the index: {} added, {} dropped. {}",
                        added,
                        dropped,
                        crate::nudge::what_i_know_of(&self.contents)
                    )
                };
                // A note Atlas could not summarise is listed by name and says
                // nothing — findable, but not from the index. Worth saying
                // once rather than leaving as a silent hole in the map.
                if blank > 0 {
                    said.push_str(&format!(
                        " {} of them I couldn't summarise — those notes have no prose in them.",
                        blank
                    ));
                }
                // The index is only as good as the folder it describes, and
                // past MAX_LINES it is a list you scroll rather than a map.
                if self.contents.needs_splitting() {
                    said.push_str(&format!(
                        " It's past {} lines now — worth splitting the notes into folders.",
                        crate::contents::MAX_LINES
                    ));
                }
                said
            }
            Err(e) => format!("I couldn't write the index: {e}"),
        }
    }

    /// The question's meaning vector, when meaning search can actually run.
    ///
    /// `None` on any gap — semantic switched off, no encoder, encoder failed —
    /// because `Library::search` treats `None` as "words alone", which is the
    /// degradation this feature was designed around. A search must never fail
    /// because the reach it grew last month is unavailable today.
    fn query_meaning(&self, question: &str, cfg: &crate::recall::RecallConfig) -> Option<Vec<f32>> {
        if !cfg.semantic {
            return None;
        }
        if let Some(v) = self.meaning_route.as_ref().and_then(|m| m.text(question)) {
            return Some(v);
        }
        let mcfg = self.meaning_cfg();
        let vars = self.tool_vars();
        if !crate::meaning::available(&mcfg, &vars) {
            return None;
        }
        crate::meaning::embed(&mcfg, &vars, question).ok()
    }

    /// The last few turns, as words recall can use to break ties.
    ///
    /// Contextual recall's whole input. Three exchanges is deliberate — the
    /// thread's own away-brief reads the same window — and the question's own
    /// words are excluded so context never double-counts the query.
    pub(super) fn context_terms(&self, question: &str) -> Vec<String> {
        let recent: Vec<String> =
            self.thread.recent.iter().rev().take(3).map(|e| e.said.clone()).collect();
        crate::recall::context_terms_from(&recent, question)
    }

    /// The recent conversation as one meaning vector, for contextual recall.
    ///
    /// The same recent-turns window `context_terms` uses, embedded as a single
    /// text so a piece can be lifted by what the conversation was *about*
    /// rather than only the words it repeated. `None` on every gap — semantic
    /// off, no encoder, empty thread, or an encoder failure — so it degrades
    /// to the word-overlap nudge, which itself degrades to no nudge. It never
    /// introduces a hit, only re-ranks, exactly as the word path does.
    fn context_meaning(&self, cfg: &crate::recall::RecallConfig) -> Option<Vec<f32>> {
        if !cfg.semantic {
            return None;
        }
        let recent: Vec<String> =
            self.thread.recent.iter().rev().take(3).map(|e| e.said.clone()).collect();
        if recent.is_empty() {
            return None;
        }
        if let Some(v) = self.meaning_route.as_ref().and_then(|m| m.text(&recent.join("\n"))) {
            return Some(v);
        }
        let mcfg = self.meaning_cfg();
        let vars = self.tool_vars();
        if !crate::meaning::available(&mcfg, &vars) {
            return None;
        }
        crate::meaning::embed(&mcfg, &vars, &recent.join("\n")).ok()
    }

    /// Start the resident encoder once, if one is installed: tool choice by
    /// meaning, and recall without starting the encoder per question.
    pub(super) fn start_meaning_route(&mut self) {
        if self.meaning_route.is_some() {
            return;
        }
        // Tried once already: again only after the meaning model was asked
        // for and has now landed.
        if self.meaning_route_tried {
            let landed = crate::getpieces::understanding().iter().all(|p| crate::getpieces::have(p, &self.store.install_root()));
            if !(self.meaning_route_retry && landed) {
                return;
            }
            self.meaning_route_retry = false;
        }
        self.meaning_route_tried = true;
        let (mcfg, vars) = (self.meaning_cfg(), self.tool_vars());
        let root = self.store.install_root();
        self.meaning_route = crate::meaningroute::Route::start(&mcfg, &vars, Some(&root), self.router.texts().to_vec());
        if self.meaning_route.is_some() {
            self.log.info("meaning: the encoder is running; tools are chosen by meaning as well as words");
        }
    }

    fn meaning_cfg(&self) -> crate::meaning::MeaningConfig {
        self.tools_ref().map(|t| t.meaning.clone()).unwrap_or_default()
    }

    fn tool_vars(&self) -> crate::tools::Vars {
        self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default()
    }

    /// Make meaning vectors for notes that don't have one yet, a couple per
    /// tick.
    ///
    /// Off the turn path on purpose: a fresh install with a folder of notes
    /// has all of them unembedded, and running the encoder over the lot in
    /// one go would block whatever the tick was about to do. Two per tick
    /// drains a realistic notes folder in a minute or two of idling and is
    /// invisible while it happens. The vectors are remembered by content, so
    /// this work is done once per note, not once per session.
    pub(super) fn embed_backlog(&mut self) {
        let cfg = self.tools_ref().map(|t| t.recall.clone()).unwrap_or_default();
        if !cfg.semantic {
            return;
        }
        let mcfg = self.meaning_cfg();
        let vars = self.tool_vars();
        if !crate::meaning::available(&mcfg, &vars) {
            // Not an error to raise every tick: `doctor` names the gap once,
            // with what to do about it, which is where a missing install
            // belongs.
            return;
        }
        let todo = self.library.unembedded();
        if todo.is_empty() {
            if self.search_check_due && !self.library.is_empty() {
                self.search_check_due = false;
                self.start_search_check();
            }
            return;
        }
        let mut changed = false;
        for id in todo.into_iter().take(2) {
            let Some((title, text)) = self.library.get(id).map(|p| (p.title.clone(), p.text.clone()))
            else {
                continue;
            };
            match crate::meaning::embed(&mcfg, &vars, &format!("{title}\n{text}")) {
                Ok(v) => {
                    self.meaning.put(&title, &text, v.clone());
                    self.library.set_embedding(id, v);
                    changed = true;
                }
                // An encoder that just failed will most likely fail for the
                // next note too; stop for this tick rather than paying the
                // tool's timeout once per remaining note.
                Err(_) => break,
            }
        }
        if changed {
            let _ = self.meaning.save(&self.store);
        }
    }

    /// Measure search, off the tick: the known questions asked by words,
    /// then by words and meaning, and the answer compared with last time.
    /// Returns what's said now.
    pub fn start_search_check(&mut self) -> String {
        if self.library.is_empty() {
            return "There are no notes to search yet, so there's nothing to measure.".into();
        }
        let mut questions = crate::recall::questions_written(
            &std::fs::read_to_string(self.store.root().join("search-questions.txt")).unwrap_or_default(),
        );
        questions.extend(crate::recall::questions_from(&self.library, 40));
        if questions.is_empty() {
            return "The notes are too short to make questions from.".into();
        }
        let lib = self.library.clone();
        let rcfg = self.tools_ref().map(|t| t.recall.clone()).unwrap_or_default();
        let mcfg = self.meaning_cfg();
        let vars = self.tool_vars();
        let semantic = rcfg.semantic && crate::meaning::available(&mcfg, &vars);
        let model = crate::meaning::fingerprint(&mcfg, &vars);
        let t = crate::store::now();
        let work: crew::Work = Box::new(move |c: &crew::Control| {
            let words = crate::recall::measure(&lib, &questions, None, &rcfg, t);
            // Between the two passes: a pause holds here, and a stop ends it
            // before the slower meaning pass starts.
            let _ = c.checkpoint();
            if c.stopping() {
                return Err("you asked me to stop".into());
            }
            let meaning = semantic.then(|| {
                let embed = |q: &str| crate::meaning::embed(&mcfg, &vars, q).ok();
                crate::recall::measure(&lib, &questions, Some(&embed), &rcfg, t)
            });
            let check = crate::recall::SearchCheck { at: t, model: if semantic { model } else { String::new() }, words, meaning };
            serde_json::to_string(&check).map_err(|e| e.to_string())
        });
        if self.hand_off("search-check", t, work, None, SpeakPolicy::Always) {
            "Measuring how well search finds things — I'll say when it's done.".into()
        } else {
            "I've too much on to measure search right now.".into()
        }
    }

    pub fn reload_library(&mut self) {
        // Through `notes_dir()` rather than reading the config again here —
        // a second place declaring the same path is exactly how
        // `backup.dir` and this function ended up looking at a folder that
        // wasn't resolved against the store root at all.
        let dir = self.notes_dir();
        // Vectors from a different encoder can't be compared with this
        // one's: forget them, and the backlog re-embeds with this one — and
        // measures search again once it has (`search_check_due`).
        let fp = crate::meaning::fingerprint(&self.meaning_cfg(), &self.tool_vars());
        if !fp.is_empty() && self.meaning.for_model(&fp) {
            self.log.info("The meaning model changed, so every note will be re-read with the new one.");
            let _ = self.meaning.save(&self.store);
            self.search_check_due = true;
        }
        // Rehydrated from the remembered vectors rather than started as
        // `None`: the library is rebuilt from disk on every load, and without
        // this each restart would re-run the encoder over every unchanged
        // note. Content-keyed, so an edited note misses here and is
        // re-embedded off the tick — its old vector described text that no
        // longer exists.
        let mut lib = crate::recall::library_from_dir(&dir);
        // And what Atlas has read, chunked and cited (`add_readings`).
        crate::recall::add_readings(&mut lib, &crate::roots::data_sub("reading"));
        for p in lib.pieces.iter_mut() {
            p.embedding = self.meaning.get(&p.title, &p.text).cloned();
        }
        // Vectors for notes that no longer exist describe nothing; without
        // this the remembered set only ever grows, one orphan per edit.
        let keys: Vec<String> =
            lib.pieces.iter().map(|p| crate::meaning::key(&p.title, &p.text)).collect();
        let before = self.meaning.len();
        self.meaning.keep_only(&keys);
        if self.meaning.len() != before {
            let _ = self.meaning.save(&self.store);
        }
        self.library = lib;
    }

    /// Answer from what Atlas has already written down, if it can.
    ///
    /// Returns `None` when the notes do not settle it, so the caller falls
    /// through to the model rather than getting a confident non-answer.
    /// Not a question a note could answer -- check whether it's someone
    /// working something out rather than asking anything. The clear cases
    /// (`decide`'s own 0.7 confidence line) are answered directly; a
    /// genuinely unclear one is asked about, and the answer is resolved
    /// from `pending_wanted` on the next turn, not guessed at here.
    pub(super) fn wanted_check(&mut self, raw: &str) -> Option<String> {
        let reading = crate::wanted::read(raw);
        if reading.wanted == crate::wanted::Wanted::Hearing && reading.confidence >= 0.7 {
            let reply = format!("{} {}", crate::wanted::heard(raw), crate::wanted::then_offer());
            // `heard()`'s own template can never produce one of these
            // phrases, but the check is what actually enforces that stays
            // true if this reply is ever built from something less
            // literal than a straight echo -- worth having exercised now
            // rather than the first time this path grows an LLM-generated
            // paraphrase in place of the plain echo.
            debug_assert!(!crate::wanted::is_empty_sympathy(&reply));
            return Some(reply);
        }
        let may_ask = self.tools_ref().map(|t| t.wanted.ask_when_unclear).unwrap_or(true);
        // Not for a command Atlas couldn't carry out: "close the quarterly
        // budget" starts the way a command does, and isn't something to think
        // through together or be listened to about (the capability sweep, 30
        // Sep 2026: every short unknown command got this question when the
        // model was down).
        let a_command = self.router.shortlist(raw, 1).first().map(|(_, s)| *s == f64::MAX).unwrap_or(false);
        let about_you = !a_command;
        if reading.wanted == crate::wanted::Wanted::Unclear && may_ask && about_you {
            self.pending_wanted = Some(raw.to_string());
            let q = crate::wanted::ask_which().to_string();
            self.session.ask(&q);
            return Some(q);
        }
        None
    }

    /// A confident answer straight from the fact book, or `None`.
    ///
    /// What you stated and what Atlas researched both live in one indexed book,
    /// so a question it genuinely knows the answer to is answered from
    /// knowledge — said at the volume its freshness has earned — rather than
    /// pointing at a note. The `answers` gate keeps this strict: only a fact
    /// that really matches the question replies; anything weaker falls through
    /// to searching the notes.
    /// Learn a fact stated in plain words, with no "note that" in front of it —
    /// a correction or a declaration Atlas can tell is meant to update what it
    /// knows.
    ///
    /// It fires only when the sentence parses as a `(subject, attribute, value)`
    /// slot AND either carries a correction marker ("actually", "correction",
    /// "from now on") or fills a slot the book already holds a fact about. That
    /// double gate is what keeps ordinary conversation out of memory: "actually
    /// that's funny" parses as no slot and is ignored, while "actually my car is
    /// a Toyota" both parses and is marked, so it lands and — through the slot
    /// supersession in `Book::learn` — replaces the old value rather than
    /// contradicting it. Never while handed over: a guest does not get to
    /// rewrite the owner's memory. Returns the acknowledgement to say, or `None`
    /// to fall through.
    /// Record an alias declaration ("the windows vps is also known as the
    /// homelab server") so the two names resolve to one entity and facts
    /// about either collapse together. Returns the acknowledgement, or None if
    /// this wasn't an alias declaration.
    fn try_alias(&mut self, raw: &str) -> Option<String> {
        let (alias, canonical) = crate::facts::alias_decl(raw)?;
        let now = clock();
        self.facts.note_alias(&alias, &canonical, now);
        let _ = self.facts.save(&self.store);
        Some(format!(
            "Got it — I'll treat \"{alias}\" and \"{canonical}\" as the same thing."
        ))
    }

    pub(super) fn learn_stated(&mut self, raw: &str) -> Option<String> {
        if self.handover().stance.handed_over() {
            return None;
        }
        // An alias declaration is handled before anything else — it's about the
        // names of a thing, not a fact to file.
        if let Some(ack) = self.try_alias(raw) {
            return Some(ack);
        }
        // Stamp with the same wall clock the capture path uses, so a correction
        // is never mistaken for older than the fact it corrects — the merge
        // keeps the newer value, and "newer" has to be measured on one clock.
        let now = clock();
        let low = raw.to_lowercase();
        // The correction word has to open the sentence (27 Sep 2026): "what
        // is actually in a hot dog" is a question, not a correction, and
        // anywhere-in-the-sentence filed it as a fact.
        let opening = low.trim_start_matches(|c: char| !c.is_alphanumeric());
        let opening = opening.strip_prefix("atlas").map(|r| r.trim_start_matches([',', ' '])).unwrap_or(opening);
        let opening = opening.strip_prefix("no").filter(|r| r.starts_with([',', ' '])).map(|r| r.trim_start_matches([',', ' '])).unwrap_or(opening);
        let marked = [
            "actually",
            "correction",
            "i meant",
            "scratch that",
            "update:",
            "from now on",
            "as of now",
            "change that to",
        ]
        .iter()
        .any(|m| opening.starts_with(m));
        let stmt = strip_correction_lead(raw);
        let (subject, attribute, _value) = crate::facts::triple(&stmt)?;
        let known_slot = self.facts.facts.iter().any(|f| {
            f.subject.as_deref() == Some(subject.as_str())
                && f.attribute.as_deref() == Some(attribute.as_str())
        });
        // A first-person frame ("my …", "our …") is what separates a
        // declaration about your own world from a passing remark. Without a
        // correction marker, only a first-person statement that updates a slot
        // Atlas already holds is learned, so "the meeting is running late" in
        // conversation can't quietly overwrite a fact.
        let low_stmt = stmt.trim().to_lowercase();
        let first_person = low_stmt.starts_with("my ") || low_stmt.starts_with("our ");
        if !marked && !(known_slot && first_person) {
            return None;
        }
        self.facts.learn(crate::facts::Fact::stated(&stmt, now), now);
        self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
        let _ = self.facts.save(&self.store);
        Some(format!(
            "Got it — updated: {}.",
            stmt.trim().trim_end_matches(['.', '!', '?'])
        ))
    }

    /// The commissioning shakedown: walk every capability that has never run on
    /// this machine and verify as much of it as can be verified without you.
    ///
    /// This is the answer to "do I have to sit and confirm hundreds of things":
    /// the read-only ones are checked now, the ones with a visible side effect
    /// wait for a one-tap go-ahead (Atlas does it, reads the result back, undoes
    /// it), the data-and-logic ones ride on your first real use, and only the
    /// camera handful need your eyes.
    pub(super) fn shakedown(&self) -> String {
        let mut steps = Vec::new();
        for c in crate::capability::all() {
            if c.state != crate::capability::State::Untested {
                continue;
            }
            let outcome = self.probe_untested(&c);
            steps.push(crate::shakedown::Step { capability: c.what, outcome });
        }
        let mut out = crate::shakedown::report(&steps);
        // A closing line drawn from whether any check actually failed, not
        // restated by hand.
        if crate::shakedown::all_clear(&steps) {
            out.push_str("\nNothing's broken — this is a to-do list, not a fault report.");
        } else {
            out.push_str("\nThe flagged items are real failures — worth looking at before you rely on them.");
        }
        out
    }

    /// What the shakedown can establish about one never-run capability, from how
    /// it would be verified and what the platform can tell Atlas right now.
    fn probe_untested(&self, c: &crate::capability::Capability) -> crate::shakedown::Outcome {
        use crate::capability::Verify;
        use crate::portable::Needs;
        use crate::shakedown::Outcome;
        match crate::capability::how_verified(c) {
            Verify::YourEyes => {
                Outcome::NeedsYou("you confirm it recognised the right thing".into())
            }
            Verify::Computes => {
                Outcome::OnRealData("runs on your files or model when you use it".into())
            }
            Verify::ReadBack => {
                // A capability that only *reads* the screen can be checked now,
                // with nothing of yours touched. One that has to *act* — open an
                // app, move a window — waits for your go-ahead, because doing
                // that unasked is the disruption a shakedown must not cause.
                let acts = c.runs.contains(&Needs::Windows_)
                    || c.runs.contains(&Needs::LaunchApps)
                    || c.runs.contains(&Needs::ActInApps);
                if acts {
                    Outcome::OnYourGo(
                        "I open or move a window and read it back, then undo it".into(),
                    )
                } else {
                    match self.plat.active_window() {
                        Ok(_) => {
                            Outcome::Verified("read the active window off the screen".into())
                        }
                        Err(e) => Outcome::Failed(e.to_string()),
                    }
                }
            }
        }
    }

    /// A fast, safe self-check of the parts everything rests on, plus an honest
    /// count of what's ready on this machine. This is the setup-time answer to
    /// "does Atlas work here" — seconds, not the thousands of development tests,
    /// which run on a build machine, not a person's computer.
    pub(super) fn self_check(&self) -> String {
        use crate::checkup::Check;
        let mut checks = Vec::new();

        // The store — everything durable rests on it. Write a probe and read it
        // straight back, so a broken or read-only data directory is caught here
        // rather than the first time something is lost.
        let stamp = crate::store::now();
        match self.store.save("checkup_probe", &stamp) {
            Err(e) => checks.push(Check::fail("saving state", e.to_string())),
            Ok(()) => {
                let back: u64 = self.store.load("checkup_probe");
                if back == stamp {
                    checks.push(Check::pass("saving and reading state"));
                } else {
                    checks.push(Check::fail("saving and reading state", "what came back wasn't what went in"));
                }
            }
        }

        // Memory — that the book loaded, and how much it holds.
        let facts = self.facts.facts.len();
        checks.push(Check::note(
            "memory",
            format!("{facts} fact{} remembered", if facts == 1 { "" } else { "s" }),
        ));

        // The phone app: no screens to arrange, nothing of a laptop's to
        // fetch or commission (2 Oct 2026, Eric's phone listed hand
        // tracking and Windows keys as "your part in setup").
        let phone = crate::phonemode::on();
        // The screen — that it can see the displays it will arrange on.
        if !phone {
            match self.plat.monitors() {
            Ok(m) => checks.push(Check::note(
                "displays",
                format!("{} screen{} it can see", m.len(), if m.len() == 1 { "" } else { "s" }),
            )),
                Err(e) => checks.push(Check::fail("displays", e.to_string())),
            }
        }

        // The model — configured or not, and where it runs. Reported, not
        // called: a self-check must stay fast and must not put a prompt on the
        // network just to prove a model is there.
        let model = match self.llm.as_ref() {
            None => "none configured — it says so rather than guessing".to_string(),
            Some(_) => match self.tools_ref().and_then(|t| t.llm.as_ref()) {
                Some(lc) => format!("configured, {}", lc.endpoint().describe()),
                None => "configured".to_string(),
            },
        };
        checks.push(Check::note("model", model));

        // And the honest picture of what's ready here versus what still needs a
        // run or an install.
        checks.push(Check::note("what's ready", crate::capability::summary()));
        // How much of the first-run verification actually needs you — the
        // answer is "a short list", not "all of it".
        if !phone {
            checks.push(Check::note("your part in setup", crate::capability::commissioning_report()));
        } else {
            checks.push(Check::note("models on this phone", crate::phonemode::models_said()));
        }

        // Setup, and the language model: what "what's outstanding in your
        // setup" was really asking (Eric, 27 Sep 2026).
        let root = self.store.install_root();
        let missing: Vec<&str> = crate::getpieces::setup_pieces()
            .iter()
            .filter(|_| !phone)
            .filter(|p| !crate::getpieces::have(p, &root))
            .map(|p| p.name)
            .collect();
        if missing.is_empty() {
            checks.push(Check::pass("setup: everything's fetched"));
        } else {
            checks.push(Check::fail("setup", format!("still to fetch: {} (starting Atlas again fetches them)", missing.join(", "))));
        }
        if self.llm.is_some() {
            checks.push(Check::pass("the language model"));
        } else {
            checks.push(Check::fail("the language model", "not loaded, so only set phrases work"));
        }


        let mut out = crate::checkup::report(&checks);
        // A closing line that tells you what to do about it, drawn from whether
        // anything actually failed rather than restated by hand.
        if crate::checkup::all_clear(&checks) {
            out.push_str("\nGood to use.");
        } else {
            out.push_str("\nFix the flagged items before relying on it.");
        }
        out
    }

    fn facts_answer(&self, question: &str, now: u64) -> Option<String> {
        // Precise recall first: if the question names a subject the book has a
        // fact about ("what kind of car", "what's the wifi password"), answer
        // with that one fact's current value rather than everything that shares
        // a word. This is what carries a correction through — the slot holds the
        // latest value, so the answer is current, not contradicted.
        if let Some(fact) = self.facts.slot_answer(question, now) {
            return Some(fact.answer(now));
        }
        // In context: when two facts share the question's word, the one about
        // what you were just discussing wins. Silent — the answer just gets
        // more apt; nothing is volunteered that the question did not find.
        let hits = self.facts.recall_in_context(question, &self.context_terms(question), now);
        let top = hits.first()?;
        if !crate::facts::answers(top, question) {
            return None;
        }
        Some(top.answer(now))
    }

    pub(super) fn from_notes(&self, question: &str, now: u64) -> Option<String> {
        // The fact book first — it holds what you stated and what Atlas looked
        // up, and answers directly when it knows.
        if let Some(answer) = self.facts_answer(question, now) {
            return Some(answer);
        }
        if self.library.is_empty() {
            return None;
        }
        let cfg = self.tools_ref().map(|t| t.recall.clone()).unwrap_or_default();
        // The question's own meaning vector, when meaning search is on and an
        // encoder is installed. `None` degrades to word search inside
        // `search`, so an encoder that is missing or fails costs the extra
        // reach and nothing else.
        let question_meaning = self.query_meaning(question, &cfg);
        // The recent conversation as one meaning vector, so context can lift a
        // note by what it was about, not only by shared words (#6). Same gate
        // as the query vector — off, no encoder, or a failure all yield None,
        // which drops silently back to the word-overlap nudge.
        let context_meaning = self.context_meaning(&cfg);
        let hits = self.library.search_in_context(
            question,
            question_meaning.as_deref(),
            &self.context_terms(question),
            context_meaning.as_deref(),
            &cfg,
            now,
        );
        match crate::recall::Library::clarity(&hits) {
            crate::recall::Clarity::NothingFound => None,
            crate::recall::Clarity::OneClearAnswer => {
                let mut answer = crate::recall::spoken(&hits);
                // A recalled fact is only as current as the note it rests on.
                // The ranking already weighs freshness to pick the winner, but
                // the winner is then said as though it were checked today --
                // and a version number pulled from a note written eight months
                // ago is exactly the kind of answer that reads as fact and
                // isn't. `certainty::aged` looks at the shelf that note sits on
                // and, when it has run out, hands back the caveat to attach; a
                // note still inside its shelf it leaves untouched, so a fresh
                // answer is not padded with a qualifier nobody needs.
                if let Some(top) = hits.first() {
                    if let Some(piece) = self.library.get(top.id) {
                        let known = crate::freshness::Known::new(
                            &piece.title,
                            crate::freshness::shelf_for(&format!(
                                "{} {}",
                                piece.title, piece.text
                            )),
                            crate::freshness::Checkable::File(piece.source.clone()),
                            piece.at,
                        );
                        let (_level, caveat) = crate::certainty::aged(
                            crate::certainty::Confidence::Fine,
                            std::slice::from_ref(&known),
                            now,
                        );
                        if let Some(caveat) = caveat {
                            answer.push_str(&format!(" — {caveat}."));
                        }
                        // The caveat says how old the note is; it does not say
                        // that the note came from a file on this machine that
                        // Atlas can go and read for itself. `should_recheck`
                        // draws exactly that line -- worth rechecking by shelf,
                        // stale by state, AND from a source recheckable alone --
                        // so the offer is only made when acting on it is
                        // actually in Atlas's power, never over a note that only
                        // the user could update.
                        if known.should_recheck(now) {
                            answer.push_str(&format!(
                                " I still have where that came from ({}), so I can go back and check it -- say the word.",
                                known.source.plain()
                            ));
                        }
                    }
                }
                Some(answer)
            }
            // The one thing the ranking knows that a ranked list does not
            // show. Handing over the top hit here would be picking a side in a
            // disagreement without mentioning there was one.
            crate::recall::Clarity::TwoEquallyGood { first, second } => Some(format!(
                "My notes answer that two ways: {first}, and {second}. \
                 I'd rather you pick than guess for you."
            )),
        }
    }

    pub(super) fn browser_cfg(&self) -> crate::browser::BrowserConfig {
        self.tools_ref()
            .map(|t| t.browser.clone())
            .unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// Two brains, and "better answers" (30 Sep 2026, `deepbrain`).
// ---------------------------------------------------------------------------

impl<'a> Daemon<'a> {
    /// "Use the better model" / "use the faster model": `models.talk`
    /// changed, kept in your settings the way the hub keeps it, and the
    /// talking model's server started again on the new one
    /// (`follow_the_talk_setting`).
    pub(super) fn choose_talk_model(&mut self, better: bool) -> String {
        if self.handover().stance.handed_over() {
            return "That's the owner's setting to change, not mine to change for you.".into();
        }
        let cfg = self.tools_cfg().models.clone();
        let root = self.store.install_root();
        let piece = if better { crate::getpieces::better_talk_model() } else { crate::getpieces::faster_talk_model() };
        let here = crate::getpieces::have(&piece, &root)
            || crate::models::Registry::dir_for(&cfg).join(format!("{}.gguf", if better { crate::deepbrain::BETTER_TALK } else { crate::deepbrain::FASTER_TALK })).is_file();
        if crate::models::talks_better(&cfg) == better && here {
            return if better {
                "I'm already using the better model.".into()
            } else {
                "I'm already using the faster model.".into()
            };
        }
        if !here {
            return if better {
                format!(
                    "The better model isn't on this computer yet. The Connections page can fetch it ({}), or it's picked up \
                     from the models folder if you put it there.",
                    crate::getpieces::gib_label(piece.bytes)
                )
            } else {
                "The faster model isn't on this computer -- setup fetches it with the rest.".into()
            };
        }
        let word = if better { "better" } else { "faster" };
        let Some(dir) = self.settings_dir() else {
            return "I can't keep that change: I'm not watching a settings folder.".into();
        };
        let mut settings = crate::settings::registry(&self.tools_cfg());
        let kept = settings.set_and_keep("models.talk", word, &dir);
        if kept.starts_with("I couldn't keep") || kept.starts_with("no setting") {
            return kept;
        }
        let _ = self.pick_up_settings();
        self.follow_the_talk_setting();
        if better {
            "Switching to the better model: more natural answers, a second or two slower each. It takes a few seconds to load."
                .into()
        } else {
            "Switching to the faster model: quicker answers, a little plainer. It takes a few seconds to load.".into()
        }
    }

    /// The talking model's server runs the model `models.talk` asks for:
    /// when the setting changed since it was started, it is stopped and the
    /// connection rebuilt, and the next pass starts it on the new one.
    pub(super) fn follow_the_talk_setting(&mut self) {
        let Some(tc) = self.tools_ref().cloned() else { return };
        let asked = tc.models.talk.trim().to_ascii_lowercase();
        let before = self.talk_setting_seen.replace(asked.clone());
        // The first look only notes it: the server is started on it anyway.
        if before.is_none() || before.as_deref() == Some(asked.as_str()) || !self.starts_model_server || tc.llm.is_some() {
            return;
        }
        let (registry, _) = crate::models::Registry::scan_reporting(&crate::models::Registry::dir_for(&tc.models));
        let wanted = registry.choose_for(&tc.models, u64::MAX).map(|m| m.id.clone());
        if wanted.is_none() || wanted == self.model_running_id {
            return;
        }
        if self.helpers.is_running("model-server") {
            self.helpers.finished("model-server");
            self.log.info(&format!(
                "switching the talking model to {} -- stopped {}",
                wanted.as_deref().unwrap_or("?"),
                self.model_running_id.as_deref().unwrap_or("the one running")
            ));
        }
        self.model_running_id = None;
        self.model_started = None;
        self.model_start_tried = None;
        if let Ok(mut seen) = self.model_server_seen.lock() {
            *seen = None;
        }
        // Built for the new model (its own template), once its server is up.
        self.llm = crate::models::connection(&tc);
    }

    /// The Connections page's section on the two models: which one talks
    /// ("Better answers"), the deep model, and the buttons that fetch them.
    pub(crate) fn brains_block(&self) -> String {
        let cfg = self.tools_cfg().models.clone();
        let root = self.store.install_root();
        let better = crate::getpieces::better_talk_model();
        let deep = crate::getpieces::deep_model();
        let better_here = crate::getpieces::have(&better, &root);
        let deep_here = crate::getpieces::have(&deep, &root) || self.deep.is_set_up();
        let talking = if crate::models::talks_better(&cfg) {
            "Better answers are on: Qwen3.5 4B talks with you -- more natural, a second or two slower a reply. \
             Pictures are still read by the Qwen3-VL model."
        } else {
            "Faster answers: the Qwen3-VL 4B model talks with you (answers in 1 to 3 seconds on this laptop)."
        };
        let mut buttons = String::new();
        if better_here {
            let (what, label) = if crate::models::talks_better(&cfg) { ("faster", "Use the faster model") } else { ("better", "Use the better model") };
            buttons.push_str(&format!("<button name=what value={what}>{label}</button>"));
        } else {
            buttons.push_str(&format!(
                "<button name=what value=get-better>Get the better model \u{b7} {}</button>",
                crate::getpieces::gib_label(better.bytes)
            ));
        }
        if !deep_here {
            buttons.push_str(&format!(
                "<button name=what value=get-deep>Get the deep brain \u{b7} {}</button>",
                crate::getpieces::gib_label(deep.bytes)
            ));
        }
        // The bigger model (2 Oct 2026): offered only where this machine has
        // room for it beside the helpers; talks on its own once it's here.
        let bigger = crate::getpieces::bigger_talk_model();
        let bigger_here = bigger.iter().all(|p| crate::getpieces::have(p, &root));
        let bigger_bytes: u64 = bigger.iter().map(|p| p.bytes).sum();
        let room = crate::models::room_for_bigger_talk(
            &crate::fit::measure(),
            &cfg,
            crate::getpieces::helpers_resident_mb(&root),
            crate::models::bigger_talk_needs(&cfg, bigger_bytes),
        );
        let bigger_line = match (bigger_here, room) {
            (true, true) if self.model_running_id.as_deref() == Some(crate::deepbrain::BIGGER_TALK) => {
                "The bigger model (Qwen3-VL 8B) is talking with you: this machine has room for it.".to_string()
            }
            (true, true) => "The bigger model (Qwen3-VL 8B) is here; it takes over talking the next time the model starts.".to_string(),
            (true, false) => "The bigger model (Qwen3-VL 8B) is here, but this machine hasn't the room for it beside everything else right now, so the 4B talks.".to_string(),
            (false, true) => {
                buttons.push_str(&format!(
                    "<button name=what value=get-bigger>Get the bigger model \u{b7} {}</button>",
                    crate::getpieces::gib_label(bigger_bytes)
                ));
                "This machine has room for a bigger talking model (Qwen3-VL 8B): it follows what you mean better, and answers a little slower.".to_string()
            }
            (false, false) => String::new(),
        };
        let deep_line = if deep_here {
            self.deep.describe()
        } else {
            "No deep model yet: research, drafts, summaries and the night's work share the talking model. The deep \
             brain (Qwen3.5 9B) writes better and runs beside it only while there's such work, giving way whenever \
             you're talking."
                .to_string()
        };
        let bigger_line = if bigger_line.is_empty() { String::new() } else { format!("<p>{}</p>", crate::hub::esc(&bigger_line)) };
        // The coding model (2 Oct 2026, `coder`): whichever this machine
        // has room for, or a line saying there's none that fits.
        let coder_piece = crate::coder::pieces_for_here().into_iter().next();
        let coder_here = self.coder.is_set_up() || coder_piece.as_ref().is_some_and(|p| crate::getpieces::have(p, &root));
        let coder_line = match (&coder_piece, coder_here) {
            (_, true) if self.coder.is_set_up() => self.coder.brain.describe(),
            (_, true) => "The coding model is here; it's set up within a minute.".to_string(),
            (Some(p), false) => {
                buttons.push_str(&format!(
                    "<button name=what value=get-coder>Get the coding model \u{b7} {}</button>",
                    crate::getpieces::gib_label(p.bytes)
                ));
                "No coding model yet: code is written by the models above. A model trained for code writes it \
                 better, and runs only while a build needs it -- the talking model steps aside if both don't fit."
                    .to_string()
            }
            (None, false) => "No coding model: this computer hasn't the memory for one to help, so code is written by the models above.".to_string(),
        };
        format!(
            "<section aria-labelledby=brains-h><h2 id=brains-h>Two brains</h2><p>{}</p>{bigger_line}<p>{}</p><p>{}</p>\
             <form method=post action=/hub/brains>{buttons}</form></section>",
            crate::hub::esc(talking),
            crate::hub::esc(&deep_line),
            crate::hub::esc(&coder_line)
        )
    }

    /// A button in that section.
    pub(crate) fn brains_button(&mut self, what: &str) -> String {
        match what {
            "better" => self.choose_talk_model(true),
            "faster" => self.choose_talk_model(false),
            "get-better" => self.get_model_piece(crate::getpieces::better_talk_model(), "the better model"),
            "get-deep" => self.get_model_piece(crate::getpieces::deep_model(), "the deep brain"),
            "get-bigger" => self.get_bigger_talk(),
            "get-understanding" => self.get_understanding(),
            "get-coder" => match crate::coder::pieces_for_here().into_iter().next() {
                Some(p) => self.get_model_piece(p, "the coding model"),
                None => "This computer hasn't the memory for a coding model to help, so I haven't fetched one.".into(),
            },
            _ => "That button isn't wired to anything, so nothing changed.".into(),
        }
    }

    /// The Ideas page's free wins, each with whether it's on here and, when
    /// it isn't, the button that does it.
    pub(crate) fn free_wins(&self) -> Vec<crate::hub::FreeWin> {
        use crate::improve::Gain;
        let root = self.store.install_root();
        let deep_here = crate::getpieces::have(&crate::getpieces::deep_model(), &root) || self.deep.is_set_up();
        let understanding = self.understanding_here();
        crate::improve::automatic()
            .into_iter()
            .map(|m| {
                let (here, get) = match m.gain {
                    Gain::Precomputed if !understanding => (
                        "Not here yet: it needs the meaning model.".to_string(),
                        Some(("Get the meaning model (91 MB)".to_string(), "get-understanding".to_string())),
                    ),
                    Gain::WarmModel if self.llm.is_none() => ("Not here yet: no model is set up.".into(), None),
                    Gain::RightSizedModel if !deep_here => (
                        "Half here: the talking model is, the deep one isn't.".into(),
                        Some(("Get the deep model".into(), "get-deep".into())),
                    ),
                    _ => ("On here.".into(), None),
                };
                crate::hub::FreeWin { what: m.what, worth: m.worth.to_string(), here, get }
            })
            .collect()
    }

    /// Can this Atlas understand meaning: the model inside Atlas, or a
    /// configured encoder program?
    fn understanding_here(&self) -> bool {
        let root = self.store.install_root();
        crate::getpieces::understanding().iter().all(|p| crate::getpieces::have(p, &root))
            || crate::meaning::available(&self.meaning_cfg(), &self.tool_vars())
    }

    /// Fetch the meaning model and its word list on the crew (`atlas get
    /// understanding` from a button). The tick starts the encoder once
    /// they're here.
    fn get_understanding(&mut self) -> String {
        if self.handover().stance.handed_over() {
            return "Not while this is handed over -- downloads onto this machine are the owner's.".into();
        }
        if self.understanding_here() {
            return "The meaning model is already here.".into();
        }
        let root = self.store.install_root();
        let pieces = crate::getpieces::understanding();
        let mb: u64 = pieces.iter().map(|p| p.megabytes()).sum();
        let work: crew::Work = Box::new(move |_ctl| {
            for p in &pieces {
                if !crate::getpieces::have(p, &root) {
                    crate::getpieces::fetch(p, &root, &crate::getpieces::Tools::default(), &|_, _| {})?;
                }
            }
            Ok("The meaning model is here and checked. I'll understand what you mean, not only the words, from now on.".into())
        });
        if self.hand_off("model-piece", crate::store::now(), work, Some("the meaning model".into()), SpeakPolicy::Always) {
            self.meaning_route_retry = true;
            format!("Getting the meaning model ({mb} MB) -- I'll say when it's ready.")
        } else {
            "I've too much going on to start that download now. Try again in a minute.".into()
        }
    }

    /// Fetch the bigger talking model and its picture reader on the crew
    /// (2 Oct 2026). Only where the machine has room: on a smaller one it
    /// would sit unused, and 5.4 GB of disk is not nothing.
    fn get_bigger_talk(&mut self) -> String {
        if self.handover().stance.handed_over() {
            return "Not while this is handed over -- downloads onto this machine are the owner's.".into();
        }
        let root = self.store.install_root();
        let pieces = crate::getpieces::bigger_talk_model();
        if pieces.iter().all(|p| crate::getpieces::have(p, &root)) {
            return "The bigger model is already here.".into();
        }
        let cfg = self.tools_cfg().models.clone();
        let bytes: u64 = pieces.iter().map(|p| p.bytes).sum();
        if !crate::models::room_for_bigger_talk(&crate::fit::measure(), &cfg, crate::getpieces::helpers_resident_mb(&root), crate::models::bigger_talk_needs(&cfg, bytes)) {
            return "This machine hasn't the room to talk through the bigger model beside everything else I run, so I've left it -- the 4B stays.".into();
        }
        let label = crate::getpieces::gib_label(bytes);
        let work: crew::Work = Box::new(move |_ctl| {
            let places = crate::getpieces::places_it_may_be(&root);
            for p in &pieces {
                if crate::getpieces::have(p, &root) {
                    continue;
                }
                if crate::getpieces::take_in(p, &root, &places)?.is_none() {
                    crate::getpieces::fetch(p, &root, &crate::getpieces::Tools::default(), &|_, _| {})?;
                }
            }
            Ok("The bigger model is here and checked. It takes over talking the next time the model starts.".into())
        });
        if self.hand_off("model-piece", crate::store::now(), work, Some("the bigger model".into()), SpeakPolicy::Always) {
            format!("Getting the bigger model ({label}) -- I'll say when it's ready.")
        } else {
            "I've too much going on to start that download now. Try again in a minute.".into()
        }
    }

    /// Fetch one of the two models on the crew -- or take it in from where
    /// it already is on this computer (the models folder, or a `model-bench`
    /// folder beside it), checked against its SHA-256 either way. Never while
    /// handed over: it's the owner's machine.
    fn get_model_piece(&mut self, piece: crate::getpieces::Piece, name: &'static str) -> String {
        if self.handover().stance.handed_over() {
            return "Not while this is handed over -- downloads onto this machine are the owner's.".into();
        }
        let root = self.store.install_root();
        if crate::getpieces::have(&piece, &root) {
            return format!("{} is already here.", capital(name));
        }
        let label = crate::getpieces::gib_label(piece.bytes);
        let work: crew::Work = Box::new(move |_ctl| {
            let places = crate::getpieces::places_it_may_be(&root);
            let taken = crate::getpieces::take_in(&piece, &root, &places)?;
            if taken.is_none() {
                crate::getpieces::fetch(&piece, &root, &crate::getpieces::Tools::default(), &|_, _| {})?;
            }
            Ok(match taken {
                Some(from) => format!("{} was already on this computer ({}); I checked it and moved it into my models folder.", capital(name), from.display()),
                None => format!("{} is here and checked.", capital(name)),
            })
        });
        // Keyed by which model: "get the better model" and "get the deep
        // brain" are two downloads, not one asked twice (both were
        // "model-piece" with nothing to tell them apart, so the second
        // joined the first and never ran).
        if self.hand_off("model-piece", crate::store::now(), work, Some(name.to_string()), SpeakPolicy::Always) {
            format!("Getting {name} ({label}) -- I'll say when it's ready.")
        } else {
            "I've too much going on to start that download now. Try again in a minute.".into()
        }
    }
}

fn capital(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// The talking model's server as the coding model sees it (`coder::ChatRoom`):
/// the daemon's helpers and the state that says whether it's running.
struct DaemonRoom<'a> {
    helpers: &'a mut crate::lifecycle::Helpers,
    rested: &'a mut bool,
    running_id: &'a mut Option<String>,
    started: &'a mut Option<std::time::Instant>,
    start_tried: &'a mut Option<std::time::Instant>,
    seen: &'a std::sync::Arc<std::sync::Mutex<Option<(std::time::Instant, bool)>>>,
    talking: bool,
}

impl crate::coder::ChatRoom for DaemonRoom<'_> {
    fn chat_running(&self) -> bool {
        self.helpers.is_running("model-server")
    }

    fn chat_in_use(&self) -> bool {
        self.talking
    }

    /// Stopped and forgotten, as `room_for_heavy` does: the next start is a
    /// fresh one, not a question to a port nobody is behind.
    fn let_chat_go(&mut self) {
        self.helpers.finished("model-server");
        *self.running_id = None;
        *self.started = None;
        *self.start_tried = None;
        if let Ok(mut s) = self.seen.lock() {
            *s = None;
        }
    }

    fn bring_chat_back(&mut self, now: bool) {
        // Lazily: it waits for you, as after an idle let-go (`model_rested`).
        *self.rested = !now;
        *self.start_tried = None;
        if let Ok(mut s) = self.seen.lock() {
            *s = None;
        }
    }
}
