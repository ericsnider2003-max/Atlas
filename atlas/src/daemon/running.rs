//! Keeping the daemon alive: saving state, the main loop (`run`), waking on the
//! hotkey, tray or hub, the microphone, shutting down, the conversation loop, and
//! saying things out loud (and stopping). Also the `Drop` that saves on the way out.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// Write everything down, and NOTICE when that does not work.
    ///
    /// Every line here used to be `let _ = ...`, discarding the result. All
    /// sixteen. So when a save failed -- a disk that filled, a file locked by
    /// antivirus or a sync client mid-write, permissions changed by an update
    /// -- Atlas said "noted", "got it", "I'll remember that", and remembered
    /// nothing, with no indication anywhere that anything had gone wrong.
    ///
    /// That is the same failure this file already records being bitten by
    /// twice, in the comments just below: the outbox "quietly emptied while
    /// doctor kept saying nothing was lost", and `selfwork` restarting from
    /// the beginning every run. Both were fixed by ADDING a save. Neither
    /// fixed the part where a save that fails says nothing.
    ///
    /// `onlyone.rs` describes the shape of the damage exactly: "You notice
    /// weeks later that something you told it didn't stick."
    ///
    /// The failures are recorded rather than returned, because `persist` is
    /// called from a dozen places that cannot sensibly handle an error, and
    /// making them all handle it would mean sixteen new ways to get it wrong.
    /// `tick` reports the transition once; `persist_failures` stays readable
    /// so `doctor` and the hub can report the standing state.
    pub fn persist(&mut self) {
        fn note(
            failed: &mut Vec<(&'static str, String)>,
            what: &'static str,
            r: crate::error::Result<()>,
        ) {
            if let Err(e) = r {
                failed.push((what, e.to_string()));
            }
        }

        let mut failed: Vec<(&'static str, String)> = Vec::new();
        self.reached.save(&self.store);
        note(&mut failed, "thread", self.thread.save(&self.store));
        note(&mut failed, "modes", self.modes.save(&self.store));
        note(&mut failed, "watcher", self.watcher.save(&self.store));
        note(&mut failed, "anticipator", self.anticipator.save(&self.store));
        note(&mut failed, "flows", self.flows.save(&self.store));
        note(&mut failed, "journal", self.journal.save(&self.store));
        note(&mut failed, "backlog", self.backlog.save(&self.store));
        note(&mut failed, "person", self.person.save(&self.store));
        note(&mut failed, "access", self.access.save(&self.store));
        // App permissions. Only `Always` grants are ever in here at rest
        // (`new_session` drops the rest at startup), so this persists exactly
        // the "always allow X" decisions and nothing narrower.
        note(&mut failed, "permissions", self.store.save("permissions", &self.permissions));
        note(&mut failed, "publisher", self.publisher.save(&self.store));
        note(&mut failed, "workshop", self.workshop.save(&self.store));
        note(&mut failed, "calendar", self.calendar.save(&self.store));
        // What you've told Atlas to remember — the typed, indexed fact book.
        // Absent from this list at first, which made it a memory that forgot
        // everything on restart.
        note(&mut failed, "facts", self.facts.save(&self.store));
        note(&mut failed, "reminded", self.store.save("reminded", &self.reminded));
        note(&mut failed, "proposals", self.store.save("proposals", &self.proposals));
        note(&mut failed, "memory", self.memory.save(&self.store));
        note(&mut failed, "scheduler", self.scheduler.save(&self.store));
        // Only when it changed (`index_on_disk`), and written on a thread of
        // its own (30 Sep 2026): the whole file index is turned into text and
        // written, which on a real disk of files took the loop -- the hub, the
        // talk key -- half a second or more every half hour, once each walk
        // of your folders came in. The copy is made here; the writing isn't.
        // How the last write went is taken on the next pass.
        if let Some(h) = self.index_saving.take_if(|h| h.1.is_finished()) {
            match h.1.join() {
                Ok(Ok(())) => self.index_on_disk = Some(h.0),
                Ok(Err(e)) => note(&mut failed, "index", Err(e)),
                Err(_) => note(&mut failed, "index", Err(crate::error::AtlasError::Platform("writing the file index stopped unexpectedly".into()))),
            }
        }
        // Anything but the sweep: wait for a write still going, so what's on
        // disk is what's here when this returns (a turn, stopping, a test).
        if !self.index_behind {
            if let Some(h) = self.index_saving.take() {
                if let Ok(Ok(())) = h.1.join() {
                    self.index_on_disk = Some(h.0);
                }
            }
        }
        if self.index_behind && self.index_saving.is_none() && self.index_on_disk != Some(self.index.written_as()) {
            let snapshot = self.index.clone();
            let store = self.store.clone();
            let mark = snapshot.written_as();
            match std::thread::Builder::new().name("atlas-index-save".into()).spawn(move || snapshot.save(&store)) {
                Ok(h) => self.index_saving = Some((mark, h)),
                // No thread to be had: written here, as it always was.
                Err(_) => {
                    let r = self.index.save(&self.store);
                    if r.is_ok() {
                        self.index_on_disk = Some(self.index.written_as());
                    }
                    note(&mut failed, "index", r);
                }
            }
        }
        if !self.index_behind && self.index_on_disk != Some(self.index.written_as()) {
            let r = self.index.save(&self.store);
            if r.is_ok() {
                self.index_on_disk = Some(self.index.written_as());
            }
            note(&mut failed, "index", r);
        }
        note(&mut failed, "long_work", self.long_work.save(&self.store));
        // What a restart would cut off while it waits on you: questions not
        // yet answered and a request being worked through. Until 1 Oct 2026
        // these lived only in memory and a restart dropped them silently
        // (research report, Stage 1 item 6). Kept as words: they're named
        // after a restart and asked again, never carried out on an old yes.
        if self.left_waiting_read {
            let mut waiting: Vec<LeftWaiting> = self
                .session
                .all_approvals()
                .into_iter()
                .map(|(_, d)| LeftWaiting { what: d, asked: true, at: crate::store::now() })
                .collect();
            if let Some(l) = &self.task_loop {
                waiting.push(LeftWaiting { what: l.in_words(), asked: false, at: crate::store::now() });
            }
            note(&mut failed, "left_waiting", self.store.save(LEFT_WAITING, &waiting));
            // A workflow, whole, so one waiting on your yes is asked again
            // after a restart rather than lost (research report, Stage 1
            // item 6: `current_flow` was never saved).
            note(&mut failed, "current_flow", self.store.save(FLOW_LEFT, &self.current_flow));
        }
        // What research taught it. Absent from this list when `learned`
        // gained its first caller, which would have made the knowledge
        // store a write-only diary that died with the process.
        note(&mut failed, "known", self.store.save("known", &self.known));
        // Anything still waiting to reach you. Left out of this list at first,
        // which meant a restart quietly emptied the outbox while doctor kept
        // saying nothing was lost.
        note(&mut failed, "outbox", self.outbox.save(&self.store));
        // Work in progress on itself. Without this the pipeline restarts at
        // Thought every time Atlas is restarted, which is the same fault as
        // rebuilding the session every turn, one level up.
        note(&mut failed, "selfwork", self.store.save("selfwork", &self.selfwork));
        note(&mut failed, "overnight", self.store.save("overnight", &self.overnight));
        note(&mut failed, "synclog", self.store.save("synclog", &Some(self.synclog.clone())));
        note(&mut failed, "sync_seen", self.store.save("sync_seen", &self.seen_up_to));
        // What's been learned about which backend can read which app.
        note(&mut failed, "backends", self.backends.save(&self.store));

        // Logged on the way in and the way out, so the log shows a span
        // rather than one line per tick for as long as the disk is unhappy.
        if !failed.is_empty() && self.persist_failures.is_empty() {
            self.log.warn(&format!(
                "could not write state to disk: {}",
                failed
                    .iter()
                    .map(|(w, e)| format!("{w} ({e})"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        } else if failed.is_empty() && !self.persist_failures.is_empty() {
            self.log.info("writing state to disk is working again");
        }
        self.persist_failures = failed;
    }

    /// Anything that could not be written down, and needs saying out loud.
    ///
    /// Returns a sentence only on the CHANGE -- when it starts failing and
    /// when it recovers. A message every tick for a full disk would be its
    /// own kind of broken, and the standing state is readable from
    /// `persist_failures` for anything that wants to show it.
    ///
    /// Called from the top of `tick`, above the early returns, so a paused or
    /// focused Atlas still reports it -- that is the state where losing
    /// everything you say matters most and where nothing else would mention
    /// it. The cost of being above them is one tick of lag: it reads what the
    /// previous `persist` left, so the news arrives on the tick after the
    /// save that failed rather than the same one. Bounded by the tick
    /// interval, and the alternative is putting it below a `return` that the
    /// commonest quiet states take.
    pub(super) fn persist_trouble(&mut self) -> Option<String> {
        let failing = !self.persist_failures.is_empty();
        if failing == self.persist_told {
            return None;
        }
        self.persist_told = failing;
        if !failing {
            return Some("I can write things down again.".into());
        }
        let what: Vec<&str> = self.persist_failures.iter().map(|(w, _)| *w).collect();
        Some(format!(
            "I can't write to my own state folder, so anything you tell me now will \
             be gone when I restart. {} couldn't be saved. Worth checking the disk \
             isn't full and that nothing has locked {}.",
            what.join(", "),
            self.store.root().display()
        ))
    }

    // ---------- the real loop ----------

    /// The main loop.
    ///
    /// Voice is primary; typing is checked every pass and always works, so the
    /// fallback is not a mode you have to switch into — it is just there.
    /// Sleep length between passes is set by the throttle, so an idle machine
    /// costs almost nothing.
    pub fn run(
        &mut self,
        ears: &dyn Ears,
        mouth: &dyn Mouth,
        keyboard: &Keyboard,
        throttle: &mut Throttle,
        audio_ok: bool,
        clock: &dyn Fn() -> u64,
    ) {
        // The wake word only when it is switched on (27 Sep 2026). With it
        // off, Atlas used to start in the wake-word tier anyway and record
        // three seconds and run speech-to-text on it every pass of this loop
        // -- a core busy and the loop deaf to the hub for those seconds, for
        // a word it was told not to listen for.
        // Why the last run ended, written down, and this one begun (item
        // 33): 19 of 23 runs on 30 Sep ended with nothing recorded.
        {
            let state = self.store.data_dir().join("state");
            let now = clock();
            let mut runs = crate::whystopped::Runs::load(&state);
            if let Some(line) = runs.start(now, crate::whystopped::computer_started(now)) {
                self.log.warn(&line);
            }
            let _ = runs.save(&state);
            self.runs = Some(runs);
            // Windows starts Atlas again after an update restarts the
            // computer -- the same way it was started this time.
            // A start-with-Windows task from before the unlock and wake
            // triggers is brought up to date, off the loop.
            if let Ok(exe) = std::env::current_exe() {
                let state_dir = state.clone();
                std::thread::spawn(move || {
                    let _ = crate::startup::bring_up_to_date(&exe, &state_dir);
                });
            }
            let args: Vec<String> = std::env::args().skip(1).collect();
            if crate::whystopped::come_back_after_updates(&args.join(" ")) {
                self.log.info("Windows will start me again after an update restart");
            }
        }
        self.tiers.set_wake(self.wake_on());
        // The microphone on its own thread (28 Sep 2026): each wake-word
        // clip used to hold this loop -- the hub, the typing box, the icon's
        // Pause -- for three seconds at a time. Started whether or not the
        // wake word is on, because cutting in by voice uses it too.
        if audio_ok {
            self.ensure_mic(ears);
            self.steer_mic();
        }
        // The folder walk on its own thread from here on: a big folder held
        // the whole loop, the hub included, for as long as the disk took.
        self.awareness.scan_in_background = true;
        // The model, loading while nothing waits for it (28 Sep 2026). It
        // was started by the first thing you said, so the first answer of
        // every session waited for the whole model to come off the disk and
        // then for the model to read the whole prompt from nothing.
        // Starting it is quick (`keep_model_server` waits for nothing); the
        // reading happens on a thread of its own (`warm_the_model`).
        self.keep_model_server(clock());
        let _ = self.warm_the_model(clock());
        // The hearing model too, when it's here and chosen (`parakeet`):
        // loading it takes seconds, and the first thing you say shouldn't.
        let engine = self.tools_ref().map(|t| t.stt_engine.trim().to_lowercase()).unwrap_or_default();
        if engine != "whisper" {
            crate::parakeet::warm(&crate::roots::install_root());
        }
        if !audio_ok {
            self.audio_tools_missing = true;
            if let Some(m) = self.tiers.audio_unavailable() {
                crate::outln!("{m}");
                // Written down too: this program has no console window.
                self.log.warn(&m);
            }
        }

        // Said once, if last time ended badly. Cleared as it is said, so it
        // is a report rather than a every-morning complaint.
        if let Some(note) = crate::crash::take(&self.store) {
            self.log.info(&note.detail());
            self.say(mouth, &note.plain());
        }

        // What Atlas can't do on this machine, said at start-up (Eric, F10:
        // "no reason for me to talk to Atlas if it can't hear me").
        if let Some(line) = self.cant_do_here() {
            self.say(mouth, &line);
        }

        // The push-to-talk key, heard from any window (`hotkey`). Started
        // here rather than in `new` so a test daemon never hooks a keyboard.
        // Failing to start is said once and the console's Enter key stays the
        // trigger, as before.
        //
        // Merged 26 Sep: on Windows the third chat's `hotkeys` (push-to-talk
        // *and* the typing box, set from Settings, measured on the laptop)
        // is the one keyboard hook -- two hooks on one key would each hold
        // it back and hand taps back twice. `hotkey` keeps the platforms
        // `hotkeys` doesn't reach (Linux reads the keyboard device).
        let ptt: Option<crate::hotkey::Keys> = if audio_ok && !cfg!(windows) {
            let pc = &self.tools_cfg().push_to_talk;
            match crate::hotkey::spawn(&pc.key, pc.hold_ms) {
                Ok(rx) => Some(rx),
                Err(why) => {
                    self.log.warn(&format!("push-to-talk key not available: {why}"));
                    None
                }
            }
        } else {
            None
        };

        // A stop request left from before this start (Atlas wasn't running
        // to take it) must not end this run the moment it begins.
        let _ = std::fs::remove_file(crate::goodbye::stop_file(&crate::roots::state_dir()));
        // Correcting as you type (Eric, H4): its own thread, polling the box
        // you're typing in. Only in the running Atlas, and only when on.
        let prose = self.tools_cfg().prose.clone();
        if prose.enabled && self.typing_thread.is_none() {
            self.typing_thread = Some(crate::astype::start(
                prose,
                self.store.clone(),
                self.typing_busy.clone(),
                self.typing_said.clone(),
                self.typing_stop.clone(),
            ));
        }
        // The typing box, started hidden now so its key shows it at once.
        if self.hotkeys.is_some() && self.typebox.is_none() {
            let tx = keyboard.sender();
            self.typebox = match crate::typebox::Standby::start(move |text| {
                let _ = tx.send(crate::input::Utterance { text, source: crate::input::Source::Typed });
            }) {
                Ok(b) => Some(b),
                // Said in the log, not dropped: the key then starts one
                // itself, and says so if that fails too.
                Err(e) => {
                    self.log.warn(&format!("couldn't keep the typing box ready: {e}"));
                    None
                }
            };
        }
        // Words typed while the loop was napping, taken at the top of the
        // next pass.
        let mut typed_meanwhile: Option<crate::input::Utterance> = None;
        loop {
            // The way out. Checked here rather than at the bottom because
            // several paths below `continue`, and a `break` at the bottom of
            // a loop that is often skipped is a break that often does not
            // happen. See `goodbye`.
            if crate::goodbye::asked_to_stop()
                || crate::goodbye::asked_by_file(&crate::roots::state_dir())
            {
                break;
            }

            let t = clock();
            // Caught. Before this, a single `unwrap` on a malformed file
            // ended the process, the console window closed, and nothing
            // brought it back -- and Atlas did not know next time that it
            // had happened at all. A panic here now costs one tick and a
            // sentence; the loop goes round again and reloads what it needs.
            //
            // Deliberately not retried: a panic means an assumption was
            // wrong, and repeating it immediately is how a crash becomes a
            // loop.
            let tick_started = std::time::Instant::now();
            let lines = match crate::crash::caught("thinking about what to do next", || self.tick(t))
            {
                Ok(lines) => lines,
                Err(why) => vec![why],
            };
            // A tick is time the hub and the typing box wait: a slow one is
            // written down, so where the time goes can be seen.
            let tick_ms = tick_started.elapsed().as_millis() as u64;
            // Whatever ran after the last named part, named too -- every tick,
            // not only slow ones (2 Oct 2026: it was left out of the quarter
            // hour's sum, so a cost hiding there was never in the reading).
            self.tick_laps.mark("the rest");
            // Every tick's parts, added up; every quarter hour, Atlas's own
            // CPU over it and where the loop's time went (`cpuuse`).
            self.cpu_meter.add(self.tick_laps.parts());
            if let Some(r) = self.cpu_meter.read(t, crate::cpuuse::own_cpu_ms()) {
                if r.idle && r.percent >= crate::cpuuse::WARN_PERCENT {
                    self.log.warn(&format!("idle but busy: {}", r.plain()));
                } else {
                    self.log.info(&format!("cpu: {}", r.plain()));
                }
                let mut kept: Vec<crate::cpuuse::Reading> = self.store.load(crate::cpuuse::KEPT);
                kept.push(r);
                let from = kept.len().saturating_sub(96);
                let _ = self.store.save(crate::cpuuse::KEPT, &kept[from..].to_vec());
            }
            if tick_ms >= SLOW_TICK_MS {
                // Which parts took it, so the next look at a log says where
                // the time went (30 Sep 2026: the laptop's said only "1777ms").
                self.log.info(&format!("timing: tick took {tick_ms}ms ({})", self.tick_laps.plain(3)));
            }
            // A tick that ran long (a render, a sweep) is Atlas busy, not
            // you gone: the next break check measures from when it finished.
            self.done_working(clock());
            for line in lines {
                self.say(mouth, &line);
            }
            // The typing watcher: which windows Atlas is typing in itself
            // (it keeps out of those), and anything it has to say.
            if self.typing_thread.is_some() {
                if let Ok(mut b) = self.typing_busy.lock() {
                    *b = self.working_for_you.iter().map(|w| w.win.0).collect();
                }
                let said: Vec<String> = self.typing_said.lock().map(|mut s| std::mem::take(&mut *s)).unwrap_or_default();
                for line in said {
                    self.say(mouth, &line);
                }
            }

            // The hub, answered by the Atlas that is actually running --
            // every request waiting, not one (`answer_hub`).
            self.answer_hub(mouth, 0);
            // The icon by the clock: Pause/Resume from its menu, and whether
            // it should say "paused" (`notifyicon`).
            self.answer_tray(clock());
            // The microphone told what's wanted of it now: paused or not, the
            // wake word or not, cutting in by voice or not.
            self.steer_mic();

            // Typing is always live, whatever tier we are on. What arrived
            // during the nap below is taken first.
            if let Some(u) = typed_meanwhile.take().or_else(|| keyboard.poll()) {
                self.turn_was_typed = u.source == crate::input::Source::Typed;
                self.converse(&u.text, ears, mouth, clock);
                self.turn_was_typed = false;
                continue;
            }

            // Push-to-talk and the typing box, alongside the wake word
            // rather than instead of it (Eric, H1).
            if let Some(ev) = self.hotkeys.as_ref().and_then(|h| h.poll()) {
                self.log.info(&format!("key: {}", ev.plain()));
                match ev {
                    // With the microphone on its own thread, the recording
                    // is done there and the loop carries on -- the hub, the
                    // typing box, everything -- while you talk; the words
                    // come back as `Heard::Talk` (30 Sep 2026: this held the
                    // loop for the whole recording and its transcription).
                    crate::hotkeys::Pressed::TalkStart if self.mic.is_some() => {
                        if let (Some(m), Some(h)) = (self.mic.as_ref(), self.hotkeys.as_ref()) {
                            m.talk(h.held_fn());
                        }
                    }
                    crate::hotkeys::Pressed::TalkStart => {
                        // The wake word's recorder lets go of the microphone
                        // while the key is held.
                        self.mic_busy(true);
                        let held_from = std::time::Instant::now();
                        let heard = match self.hotkeys.as_ref() {
                            Some(h) => ears.listen_while(&|| h.held()),
                            None => Ok(None),
                        };
                        self.mic_busy(false);
                        match heard {
                            Ok(Some(said)) => {
                                if let Some(m) = self.tiers.heard_you() {
                                    self.log.info(&m);
                                }
                                self.heard_through_this_ear(&said);
                                self.converse(&said, ears, mouth, clock)
                            }
                            // A tap is not a question. A key held for a
                            // second or more that came back with no words is
                            // said, not swallowed (29 Sep 2026: Eric held the
                            // key four times and Atlas never answered).
                            Ok(None) if held_from.elapsed() >= std::time::Duration::from_secs(1) => {
                                self.log.info(&format!(
                                    "push-to-talk: no words in {:.1}s of recording",
                                    held_from.elapsed().as_secs_f32()
                                ));
                                self.say(mouth, "I didn't catch anything that time. Hold the key while you talk, and I'll listen until you let go.");
                            }
                            Ok(None) => {}
                            Err(e) => self.degrade_because(mouth, &e),
                        }
                    }
                    crate::hotkeys::Pressed::TalkStop => {}
                    crate::hotkeys::Pressed::TypingBox => {
                        // The box kept ready is shown at once; if it isn't
                        // there, one is started (and kept for next time).
                        let shown = self.typebox.as_mut().map(|b| b.show()).unwrap_or(false);
                        if !shown {
                            self.typebox = None;
                            let tx = keyboard.sender();
                            match crate::typebox::Standby::start(move |text| {
                                let _ = tx.send(crate::input::Utterance { text, source: crate::input::Source::Typed });
                            }) {
                                Ok(mut b) => {
                                    // Its window needs a moment to exist before
                                    // it can be shown: asked again for up to two
                                    // seconds, and said if it never appears (29
                                    // Sep 2026: one try at 300 ms, and the key
                                    // did nothing on a slow start).
                                    let mut shown = false;
                                    for _ in 0..10 {
                                        std::thread::sleep(std::time::Duration::from_millis(200));
                                        if b.show() {
                                            shown = true;
                                            break;
                                        }
                                    }
                                    if !shown {
                                        self.log.warn("the typing box started but its window didn't appear");
                                    }
                                    self.typebox = Some(b);
                                }
                                Err(e) => self.say(mouth, &e),
                            }
                        }
                    }
                }
                continue;
            }

            // A message from another Atlas is checked with the same
            // never-block guarantee as everything else in this loop --
            // `poll_once` returns immediately whether or not anyone called.
            if let Some(said) = self.answer_peer_door(t) {
                if !said.is_empty() {
                    self.say(mouth, &said);
                }
                continue;
            }

            // Holding the key means "listen now", whatever tier is live.
            if let Some(keys) = ptt.as_ref() {
                if keys.held() {
                    self.mic_busy(true);
                    let heard = ears.listen();
                    self.mic_busy(false);
                    match heard {
                        Ok(said) => self.converse(&said, ears, mouth, clock),
                        Err(e) => self.degrade_because(mouth, &e),
                    }
                    continue;
                }
            }

            // The wake word, heard on the microphone's own thread: taken
            // here if it has news, never waited for.
            if self.mic.is_some() {
                if self.listen_pass(ears, mouth, clock) {
                    continue;
                }
            }

            match self.tiers.tier {
                // Done above, on its own thread.
                Tier::Voice if self.mic.is_some() => {}
                // Ears that can't be handed to a thread (test stand-ins):
                // on the loop, as it always was.
                Tier::Voice => match ears.wake_once() {
                    Ok(true) => match ears.listen() {
                        Ok(said) => {
                            if let Some(m) = self.tiers.succeeded() {
                                self.say(mouth, &m);
                            }
                            self.heard_through_this_ear(&said);
                            self.converse(&said, ears, mouth, clock);
                        }
                        Err(e) => self.degrade_because(mouth, &e),
                    },
                    Ok(false) => {}
                    Err(e) => self.degrade_because(mouth, &e),
                },
                Tier::PushToTalk => {
                    // Off Windows, the held key (`hotkey`) when it could be
                    // hooked; on Windows the key is `hotkeys`, handled above
                    // (H1). Enter in the console when neither could.
                    // In slices, with the hub answered between them: half a
                    // second in one piece was half a second in which a click
                    // on the hub waited.
                    //
                    // Without a key of its own (Windows, where `hotkeys`
                    // above is the key), what arrives on the keyboard queue
                    // is words -- the console skips empty lines -- and they
                    // are answered as words (27 Sep 2026). They used to be
                    // taken as "start listening" and thrown away, which
                    // mattered once this became the tier Atlas starts in
                    // when the wake word is off: everything typed in the
                    // typing box would have been lost.
                    let mut pressed = false;
                    match ptt.as_ref() {
                        Some(keys) => {
                            for _ in 0..10 {
                                if keys.held_within(25) {
                                    pressed = true;
                                    break;
                                }
                                self.answer_hub(mouth, 25);
                            }
                        }
                        None => {
                            if let Some(u) = self.wait_for_typing(keyboard, mouth, 500) {
                                self.turn_was_typed = u.source == crate::input::Source::Typed;
                                self.converse(&u.text, ears, mouth, clock);
                                self.turn_was_typed = false;
                            }
                        }
                    }
                    if pressed {
                        match ears.listen() {
                            Ok(said) => {
                                if let Some(m) = self.tiers.succeeded() {
                                    self.say(mouth, &m);
                                }
                                self.converse(&said, ears, mouth, clock);
                            }
                            Err(e) => self.degrade_because(mouth, &e),
                        }
                    }
                }
                Tier::Typed => {
                    if let Some(u) = self.wait_for_typing(keyboard, mouth, 500) {
                        self.turn_was_typed = u.source == crate::input::Source::Typed;
                        self.converse(&u.text, ears, mouth, clock);
                        self.turn_was_typed = false;
                    }
                }
            }

            // The model server, looked after on every pass without waiting:
            // started once the first check answers, not at the first thing
            // you say (29 Sep 2026).
            self.keep_model_server_waiting(clock(), std::time::Duration::ZERO, false);
            self.keep_deep_brain();
            self.look_again_at_audio(ears, clock());
            self.look_again_at_the_microphone(clock());
            self.back_to_the_wake_word(mouth, clock());
            self.open_signal_door_again(clock());
            let signals = self.observe(clock());
            let nap = throttle.next_interval(&signals, self.power);
            if self.tiers.tier == Tier::Typed {
                continue; // keyboard.wait already paced us
            }
            // Deliberately not `nap` directly. `next_interval` backs off to
            // idle_tick_secs (60s, up to 180s on battery) to save compute
            // when nothing is happening -- right for deciding whether an
            // index rescan is worth its cost, wrong for how long a message
            // from another Atlas sits unnoticed. Checking for one costs
            // nothing when none has arrived, so there is no real saving in
            // waiting longer between checks, only a longer worst-case delay.
            // Capped at 2s regardless of what the general backoff computed,
            // so kin signals and everything else polled at the top of this
            // loop are never held hostage by it.
            const MAX_SLEEP_SECS: u64 = 2;
            // `goodbye::nap`, not `thread::sleep`: the same wait, in slices,
            // so Ctrl-C is answered in 50ms rather than up to two seconds.
            // Sooner still when the crew has news or a hand free for queued
            // work: waiting work only starts on a tick, and a slot freed just
            // after one otherwise sat empty for up to two seconds. Only when
            // something could actually start — work blocked behind a render
            // is not a reason to spin.
            // And while a request of several steps is under way: its worker
            // waits on the tick for each step (`take_task_loop_news`).
            let nap_ms = if self.crew.wants_attention() || self.working_through_steps() { CREW_NAP_MS } else { nap.min(MAX_SLEEP_SECS) * 1000 };
            // Not asleep, though (27 Sep 2026): the nap is spent answering
            // the hub the moment a request arrives, and watching for typing,
            // which ends it. Two seconds with the hub unanswered and the
            // typing box's words sitting in a queue was most of why clicking
            // around and talking to Atlas felt slow.
            typed_meanwhile = self.nap_awake(keyboard, mouth, nap_ms);
        }

        for line in self.shut_down() {
            self.say(mouth, &line);
        }
    }

    /// One line in the log per turn: how long each stage took, and how much
    /// of the doing was the model (from the flight recorder's calls made
    /// during the turn). Numbers only -- never the words, which the
    /// flight recorder promises too (`trace::STORES_NO_CONTENT`).
    fn log_turn_timing(&self, t: &crate::timing::Turn, calls_before: u64, speaking_ms: u64) {
        let calls = self.trace.recorded_since(calls_before);
        let model_ms: u64 = calls.iter().map(|c| c.took_ms).sum();
        let mut line = String::from("timing: turn");
        for stage in crate::timing::Stage::all() {
            if let Some(ms) = t.get(stage) {
                line.push_str(&format!(" {}={ms}ms", stage.key()));
            }
        }
        line.push_str(&format!(" model={model_ms}ms/{} call(s) delivering={speaking_ms}ms", calls.len()));
        // When the first sentence went to be spoken, counted from when the
        // model was asked (30 Sep 2026): the target is under 3 s.
        if let Some(ms) = self.first_words_ms.take() {
            line.push_str(&format!(" first_words={ms}ms"));
        }
        // The silence Eric sits through (Phase 0.2, target under 3 s).
        if let Some(ms) = self.silence_ms.take() {
            line.push_str(&format!(" end_of_speech_to_first_audio={ms}ms"));
        }
        // What the model server itself counted for the last call: a whole
        // prompt read again (a cache miss) reads very differently from a
        // long reply (29 Sep 2026).
        if let Some(st) = crate::models::take_last_timings() {
            line.push_str(&format!("; {}", st.line()));
        }
        self.log.info(&line);
    }

    /// Whether the wake word is switched on in your settings.
    pub fn wake_on(&self) -> bool {
        self.tools_cfg().wake.as_ref().map(|w| w.enabled).unwrap_or(false)
    }

    /// Answer every hub request waiting, waiting up to `wait_ms` for one if
    /// none is. Returns how many were answered.
    ///
    /// Taken out and put back so the handler can borrow `self` mutably --
    /// the connections are read on their own threads, but every answer is
    /// still worked out here, on the loop's thread, which is the only one
    /// that holds `&mut Daemon`.
    /// What the icon by the clock asked for (`notifyicon`), done the way the hub's
    /// Pause button does it, and the icon told whether Atlas is paused. The
    /// icon's own thread never touches the daemon; this is the only door.
    pub(super) fn answer_tray(&mut self, t: u64) {
        for ask in crate::notifyicon::tray_asks() {
            match ask {
                crate::notifyicon::TrayAction::Pause => {
                    let _ = self.turn("pause", t);
                }
                crate::notifyicon::TrayAction::Resume => {
                    let _ = self.turn("carry on", t);
                }
                _ => {}
            }
        }
        crate::notifyicon::tray_paused_now(self.attention.is_paused());
    }

    pub(super) fn answer_hub(&mut self, mouth: &dyn Mouth, wait_ms: u64) -> usize {
        self.answer_hub_saying(Some(mouth), wait_ms)
    }

    /// `answer_hub` in the middle of a reply: the requests are answered, and
    /// anything the hub has to say (`guesses_worth_mentioning`) waits for
    /// the next ordinary answering rather than being said between two of
    /// the reply's sentences.
    pub(super) fn answer_hub_mid_reply(&mut self) -> usize {
        self.answer_hub_saying(None, 0)
    }

    fn answer_hub_saying(&mut self, mouth: Option<&dyn Mouth>, wait_ms: u64) -> usize {
        let Some(server) = self.hub_server.take() else {
            if wait_ms > 0 {
                crate::goodbye::nap(wait_ms);
            }
            return 0;
        };
        let mut handle = |action| {
            // Caught: a panic answering the hub would otherwise take the
            // whole of Atlas down with the page.
            crate::crash::caught("answering the hub", || crate::hublive::reply(self, action)).unwrap_or_else(|why| {
                crate::server::Reply { status: 500, body: serde_json::json!({ "error": why }).to_string(), ..Default::default() }
            })
        };
        let done = if wait_ms > 0 { server.wait_and_answer(wait_ms, &mut handle) } else { server.answer_waiting(&mut handle) };
        // The door's own news (its port taken, where it opened instead:
        // `server::open_hub`), in the log where it can be found.
        for line in server.take_news() {
            self.log.info(&line);
        }
        for a in &done {
            self.hub_timing(a);
        }
        // A run of wrong tokens is said once, not every loop.
        if let Some(mouth) = mouth {
            let guesses = server.guesses_worth_mentioning();
            if guesses.is_some() && guesses != self.guesses_said {
                if let Some(g) = &guesses {
                    self.say(mouth, g);
                }
            }
            self.guesses_said = guesses;
        }
        self.hub_server = Some(server);
        done.len()
    }

    /// What one hub request cost, in the log: method and path, how long it
    /// waited for the loop and how long the answer took. Never the query or
    /// a body. A slow one is written on its own line; the quick ones are
    /// counted and summed up once a minute, so a page that refreshes itself
    /// every few seconds doesn't fill the log.
    pub(super) fn hub_timing(&mut self, a: &crate::server::HubCost) {
        let total = a.waited_ms + a.took_ms;
        if total >= SLOW_HUB_MS {
            self.log.info(&format!("timing: hub {} waited {}ms, answered in {}ms", a.what, a.waited_ms, a.took_ms));
        }
        let t = &mut self.hub_times;
        t.count += 1;
        t.total_ms += total;
        t.slowest_ms = t.slowest_ms.max(total);
        let now = crate::store::now();
        if t.since == 0 {
            t.since = now;
        }
        if now.saturating_sub(t.since) >= 60 {
            let line = format!(
                "timing: hub {} requests in {}s, mean {}ms, slowest {}ms",
                t.count,
                now - t.since,
                t.total_ms / t.count.max(1),
                t.slowest_ms
            );
            self.log.info(&line);
            self.hub_times = HubTimes { since: now, ..HubTimes::default() };
        }
    }

    /// Typing, waited for in slices with the hub answered between them.
    fn wait_for_typing(&mut self, keyboard: &Keyboard, mouth: &dyn Mouth, ms: u64) -> Option<crate::input::Utterance> {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < until {
            if let Some(u) = keyboard.wait(20) {
                return Some(u);
            }
            self.answer_hub(mouth, 30);
        }
        None
    }

    /// The loop's rest between passes, spent answering the hub as requests
    /// arrive and ended early by typing, Ctrl-C or a stop request. Returns
    /// what was typed, for the next pass to take.
    fn nap_awake(&mut self, keyboard: &Keyboard, mouth: &dyn Mouth, ms: u64) -> Option<crate::input::Utterance> {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        while std::time::Instant::now() < until {
            if crate::goodbye::asked_to_stop() {
                return None;
            }
            if let Some(u) = keyboard.wait(10) {
                return Some(u);
            }
            // The wake word heard mid-nap ends the nap: it waits for the
            // next pass, not for two seconds.
            if self.mic_heard.is_none() {
                self.mic_heard = self.mic.as_ref().and_then(|m| m.poll()).filter(|h| !matches!(h, crate::micthread::Heard::FollowUp(..)));
            }
            if self.mic_heard.is_some() {
                return None;
            }
            self.answer_hub(mouth, 40);
        }
        None
    }

    /// Stop, then start a new copy of Atlas in this one's place (an update
    /// going in). The caller exits on `Ok`.
    ///
    /// ## Why it's not "start the new copy, then exit"
    ///
    /// That is what it was (until 28 Sep 2026): `persist`, `relaunch_self`,
    /// `process::exit(0)`. `exit` skips every way out -- so the instance lock
    /// was still held, and beaten moments before, when the new copy asked
    /// for it. The new copy read `Running`, said "Atlas is already running"
    /// to a console that isn't there, and ended. The old one had already
    /// gone. So an update that went in automatically left NO Atlas running:
    /// the icon by the clock went, the hub stopped answering, and nothing
    /// came back until the next sign-in. `exit` also skipped `Drop for
    /// Helpers`, leaving the model server running with its memory and
    /// nobody's.
    ///
    /// Now the whole way out runs first (`shut_down`: state written, helpers
    /// stopped, the lock let go -- last), and only then is the new copy
    /// started. If it can't be started, this Atlas takes its lock back and
    /// carries on rather than leave nothing running.
    pub fn restart_as(&mut self, relaunch: impl FnOnce() -> std::result::Result<(), String>) -> std::result::Result<(), String> {
        for line in self.shut_down() {
            self.log.info(&line);
        }
        // The icon by the clock, taken away before the new copy puts up its
        // own: the caller exits with `process::exit`, which drops nothing, so
        // the old icon stayed beside the new one as a ghost (28 Sep 2026).
        crate::notifyicon::take_icon_away();
        match relaunch() {
            Ok(()) => Ok(()),
            Err(why) => {
                crate::notifyicon::bring_icon_back();
                // Still here after all: hold the lock again and keep going.
                // Helpers start again when next wanted.
                let _ = crate::onlyone::OnlyOne::at(&self.store.data_dir()).take(crate::store::now());
                self.stopped = false;
                Err(why)
            }
        }
    }

    /// Start the microphone's thread, once, if these ears can have one.
    pub(super) fn ensure_mic(&mut self, ears: &dyn Ears) {
        if self.mic.is_none() && !self.mic_asked {
            self.mic_asked = true;
            self.mic = ears.mic_work().map(crate::micthread::MicThread::start);
            // How late and how loud Atlas reaches the microphone, as the
            // last run learned it: the first cut-in needn't wait for it to
            // be learned again.
            if let (Some(m), Some(l)) = (self.mic.as_ref(), self.store.load::<Option<crate::micthread::Learned>>(CUT_IN_STATE)) {
                m.seed(l);
            }
        }
    }

    /// Tell the microphone's thread what's wanted of it now. Paused means
    /// not listening at all: no wake word, no watching while Atlas speaks.
    pub(super) fn steer_mic(&mut self) {
        let Some(m) = self.mic.as_ref() else { return };
        let cfg = self.tools_cfg();
        m.set_paused(self.attention.is_paused());
        m.set_wake(self.tiers.tier == Tier::Voice && cfg.wake.as_ref().map(|w| w.enabled).unwrap_or(false));
        // Cutting in by voice on for a headset even when it's off in
        // settings: its microphone doesn't hear Atlas (`for_microphone`).
        m.set_barge(&cfg.barge_in.for_microphone(&crate::voice::microphone_now(&cfg).0));
        for n in m.take_notes() {
            self.log.info(&n);
        }
        if let Some(l) = m.take_new_learned() {
            let _ = self.store.save(CUT_IN_STATE, &Some(l));
        }
    }

    /// A turn is going on elsewhere (the talk key, a conversation): the
    /// wake word's recorder stays off the microphone meanwhile.
    fn mic_busy(&self, on: bool) {
        if let Some(m) = self.mic.as_ref() {
            m.set_busy(on);
        }
    }

    /// The microphone's part of one pass of the loop: whatever its thread
    /// heard, taken without waiting. A wake word comes with what you said
    /// after it, and is answered here. Returns whether anything was.
    ///
    /// Public so the test can show a pass returns at once while the thread
    /// is mid-recording.
    pub fn listen_pass(&mut self, ears: &dyn Ears, mouth: &dyn Mouth, clock: &dyn Fn() -> u64) -> bool {
        if let Some(back) = self.tiers.try_the_wake_word_again(clock()) {
            self.log.info(&format!("trying the wake word again by itself: {back}"));
        }
        self.ensure_mic(ears);
        self.steer_mic();
        self.call_mute_setting();
        let heard = self.mic_heard.take().or_else(|| self.mic.as_ref().and_then(|m| m.poll()));
        let Some(heard) = heard else { return false };
        // A follow-up nobody is waiting for any more (`follow_up` gave up).
        if matches!(heard, crate::micthread::Heard::FollowUp(..)) {
            return false;
        }
        match heard {
            crate::micthread::Heard::Wake(Ok(said)) => {
                if let Some(m) = self.tiers.succeeded() {
                    self.say(mouth, &m);
                }
                self.heard_through_this_ear(&said);
                self.converse(&said, ears, mouth, clock);
            }
            // Your name, then nothing: "Yes?", and whatever you say next is
            // the request. Not a failure. (29 Sep 2026: this said "I heard
            // my name but nothing after it" -- when you'd said the request in
            // the same breath and it had been thrown away with the clip the
            // name was found in. That is fixed in `utterance`; this is only
            // for the name said on its own.)
            crate::micthread::Heard::Named => self.answer_the_name(ears, mouth, clock),
            crate::micthread::Heard::Wake(Err(why)) if why.contains(crate::voice::HEARD_NOTHING) => {
                self.answer_the_name(ears, mouth, clock)
            }
            crate::micthread::Heard::Wake(Err(why)) | crate::micthread::Heard::Trouble(why) => {
                self.log.info(&format!("listening for the wake word: {why}"));
                self.degrade(mouth);
            }
            crate::micthread::Heard::FollowUp(..) => {}
            // The talk key, recorded on the microphone's thread.
            crate::micthread::Heard::Talk(got, held_secs) => {
                self.mic_busy(false);
                match got {
                    Ok(Some(said)) => {
                        if let Some(m) = self.tiers.heard_you() {
                            self.log.info(&m);
                        }
                        self.heard_through_this_ear(&said);
                        self.converse(&said, ears, mouth, clock);
                    }
                    // A tap is not a question; a key held a second or more
                    // with no words is said, not swallowed.
                    Ok(None) if held_secs >= 1.0 => {
                        self.log.info(&format!("push-to-talk: no words in {held_secs:.1}s of recording"));
                        self.say(mouth, "I didn't catch anything that time. Hold the key while you talk, and I'll listen until you let go.");
                    }
                    Ok(None) => {}
                    Err(why) => {
                        self.log.info(&format!("push-to-talk: {why}"));
                        self.say(mouth, &format!("I couldn't listen just then: {why}"));
                    }
                }
            }
        }
        // Done talking with Atlas: the call hears you again (`callmute`).
        let muted = crate::callmute::muted_now();
        crate::callmute::release();
        if !muted.is_empty() {
            let names: Vec<&str> = muted.iter().map(|m| m.exe.rsplit(['\\', '/']).next().unwrap_or(&m.exe)).collect();
            self.log.info(&format!("call: unmuted {} after talking with Atlas", names.join(", ")));
        }
        if let Some(m) = self.mic.as_ref() {
            m.rearm();
        }
        self.steer_mic();
        true
    }

    /// Your name said on its own: "Yes?", then listen for the rest -- after
    /// the question is said, so its own sound isn't taken for your answer.
    fn answer_the_name(&mut self, ears: &dyn Ears, mouth: &dyn Mouth, clock: &dyn Fn() -> u64) {
        if let Some(m) = self.tiers.succeeded() {
            self.say(mouth, &m);
        }
        self.say(mouth, "Yes?");
        match self.follow_up(ears, NAME_ALONE_WAIT_SECS) {
            Some(said) => {
                self.heard_through_this_ear(&said);
                self.converse(&said, ears, mouth, clock);
            }
            None => self.log.info("heard my name, then nothing"),
        }
    }

    /// Push-to-talk because the wake word's microphone failed -- not because
    /// you turned the wake word off -- comes back to listening for the name
    /// once the microphone gives sound again (29 Sep 2026: the log showed
    /// "Switching to push-to-talk -- wake word isn't working" after a
    /// missing device, and it never switched back, not even after the
    /// microphone was picked again). The microphone's thread is asked for a
    /// short recording now and then; any sound at all is enough.
    pub fn back_to_the_wake_word(&mut self, mouth: &dyn Mouth, t: u64) {
        if self.tiers.tier != Tier::PushToTalk || !self.tiers.wake_on() {
            self.mic_heard_before_ptt = None;
            return;
        }
        let Some(m) = self.mic.as_ref() else { return };
        let heard = m.audio_heard();
        let Some(before) = self.mic_heard_before_ptt else {
            // Just dropped: from here on, sound means it works.
            self.mic_heard_before_ptt = Some(heard);
            self.mic_probe_at = t + MIC_PROBE_EVERY_SECS;
            return;
        };
        if heard > before {
            self.mic_heard_before_ptt = None;
            if let Some(said) = self.tiers.microphone_works_again() {
                self.say(mouth, &format!("{said} The microphone works again."));
            }
            self.steer_mic();
            return;
        }
        if t >= self.mic_probe_at {
            self.mic_probe_at = t + MIC_PROBE_EVERY_SECS;
            m.probe();
        }
    }

    /// Whether the microphone's own thread is running: what the Status page
    /// reads to say whether Atlas can hear you at all.
    pub(crate) fn mic_running(&self) -> bool {
        self.mic.is_some()
    }

    /// Is the microphone's thread recording right now? For the tests and the
    /// hub's honesty about Pause.
    pub fn mic_recording_for_test(&self) -> bool {
        self.mic.as_ref().is_some_and(|m| m.is_recording())
    }

    /// Has the microphone's thread ended (or never started)?
    pub fn mic_stopped_for_test(&self) -> bool {
        self.mic.as_ref().is_none_or(|m| m.is_stopped())
    }

    /// The way out, done in the order that matters.
    ///
    /// ## Why this exists at all
    ///
    /// `run` was `loop { .. }` with no `break`, so the only way Atlas ever
    /// ended was being killed. Three things written for the way out therefore
    /// never ran, and each had a visible cost:
    ///
    /// * **The instance lock was never released.** `OnlyOne::release` had a
    ///   caller in the tests and none in `src`. Quit and restart inside the
    ///   150-second staleness window and Atlas refused to start, blaming a
    ///   process that no longer existed.
    /// * **Helpers outlived Atlas.** `Helpers::stop_all` -- "everything down,
    ///   on the way out" -- had no caller either, so whisper, piper and the
    ///   model server kept their memory and the next start spawned more.
    ///   `Drop for Helpers` kills the children it is still holding, which
    ///   covers the process tree but says nothing and leaves the supervisor's
    ///   own record of what is running untouched.
    /// * **State changed since the last `persist` was lost.** `Drop for
    ///   Daemon` persists, but `Drop` needs the value to go out of scope, and
    ///   a killed process does not unwind.
    ///
    /// ## The order
    ///
    /// State, then processes, then the lock -- and the lock is deliberately
    /// last. While it is held, a second Atlas refuses to start; release it
    /// first and a second Atlas could take it, load the old state and write
    /// over everything this one is still in the middle of saving. A stale
    /// lock costs a wait. Two Atlases writing the same folder costs the
    /// state.
    ///
    /// Returns what is worth saying out loud, rather than saying it, so the
    /// tests can watch the order without a `Mouth`.
    pub fn shut_down(&mut self) -> Vec<String> {
        if self.stopped {
            return Vec::new();
        }
        self.stopped = true;
        // The work log keeps its own schedule; the way out is its last save.
        let _ = self.store.save("worklog", &self.worklog);
        // The microphone let go first: nothing recorded on the way out.
        if let Some(mut m) = self.mic.take() {
            m.stop();
        }
        // The hearing server Atlas started goes with it.
        crate::parakeet::stop();
        // The typing watcher saves what it learned on the way out.
        self.typing_stop.store(true, std::sync::atomic::Ordering::Relaxed);
        if let Some(h) = self.typing_thread.take() {
            let _ = h.join();
        }

        let mut out = Vec::new();

        // First, and it does not wait. Background errands can be minutes
        // long, and `Drop for Crew` waits up to SHUTDOWN_DEADLINE_SECS for
        // them at the very end. Asking here means they are winding down
        // while the state is written, which is the difference between a
        // shutdown that overlaps its work and one that queues it.
        // Paused errands live on their own threads, so a restart can't keep
        // them. Said, rather than let them vanish as if never paused.
        let lost: Vec<String> = self
            .errand_candidates()
            .into_iter()
            .filter(|c| c.paused)
            .map(|c| crate::which_errand::describe(&c))
            .collect();
        let asked = self.crew.ask_everyone_to_stop();
        if !lost.is_empty() {
            out.push(format!(
                "{} {} paused, and a pause doesn't survive me closing — ask for {} again when \
                 you want {}.",
                lost.join(" and "),
                if lost.len() == 1 { "was" } else { "were" },
                if lost.len() == 1 { "it" } else { "them" },
                if lost.len() == 1 { "it" } else { "them" }
            ));
        }

        // Then state, because it is the only step whose failure loses
        // something that starting again cannot recover.
        self.persist();
        if !self.persist_failures.is_empty() {
            let what: Vec<&str> = self.persist_failures.iter().map(|(w, _)| *w).collect();
            out.push(format!(
                "I couldn't save {} on the way out, so that much is gone. Everything \
                 else is written down.",
                what.join(", ")
            ));
        }

        // Second. Said out loud only when something was actually running:
        // "stopped nothing" is not news.
        let stopped = self.helpers.stop_all();
        for said in &stopped {
            self.log.info(said);
        }

        // Anything Atlas started and does not wait for. Collected, and
        // whatever is still going is named rather than left as a surprise:
        // these are the person's own windows -- a panel, an app they asked
        // for, a headless browser -- and Atlas does not close them on the way
        // out. It should say they are there, because it opened them and
        // nothing else will mention it.
        crate::unwaited::reap();
        let left_open = crate::unwaited::still_running();
        if left_open > 0 {
            out.push(format!(
                "I'm leaving {left_open} thing(s) I opened still running -- they're \
                 yours, so I'm not closing them behind you."
            ));
        }

        // How this run ended, before the lock goes (item 33).
        if let Some(mut runs) = self.runs.take() {
            let why = crate::goodbye::why();
            runs.stop(crate::store::now(), why);
            let _ = runs.save(&self.store.data_dir().join("state"));
            self.log.info(&format!("stopping because {}", why.plain()));
        }

        // Last. See the order above.
        crate::onlyone::OnlyOne::at(&self.store.data_dir()).release();

        self.log.info(&format!(
            "stopped cleanly -- {} errand(s) called off, {} helper(s) shut down, \
             state written, lock released",
            asked,
            stopped.len()
        ));

        // A second Ctrl-C means "I meant it". Saying goodbye goes through
        // `say`, which speaks it -- a second or two of a sentence nobody
        // asked for, on top of a wait the person has already told you was
        // too long. So the pleasantry is dropped and anything actually lost
        // is still reported.
        if !crate::goodbye::asked_twice() {
            out.push("Right — I'm stopping. Everything's written down.".into());
        }
        out
    }

    /// One exchange, plus however many follow-ups you keep going with.
    ///
    /// This is also where a turn gets timed. It is the one place that sees a
    /// whole exchange end to end — the listen that produced the words, the
    /// work, and the speaking of the reply — so it is the only place that can
    /// divide the time up honestly. `timing::Recent` was added holding a
    /// window and nothing filled it, which reads exactly like a fast machine.
    pub fn converse(&mut self, first: &str, ears: &dyn Ears, mouth: &dyn Mouth, clock: &dyn Fn() -> u64) {
        // The wake word's recorder off the microphone for the whole exchange:
        // the follow-up listens are this turn's, and two recorders on one
        // microphone is one too many.
        self.mic_busy(true);
        // Caught, as `tick` is. A panic on the conversation path used to
        // unwind out of the run loop and end the process (prose.rs recorded
        // one such crash); now it costs that exchange and a sentence.
        if let Err(why) = crate::crash::caught("answering you", || self.converse_inner(first, ears, mouth, clock)) {
            self.say(mouth, &why);
        }
        self.mic_busy(false);
        self.done_working(clock());
    }

    fn converse_inner(&mut self, first: &str, ears: &dyn Ears, mouth: &dyn Mouth, clock: &dyn Fn() -> u64) {
        let mut said = first.to_string();
        // The first utterance came through the wake word or push-to-talk, so
        // it is unambiguously for Atlas. Follow-ups do not: the floor is left
        // open so you needn't repeat the wake word, which means the next thing
        // the microphone hears might be you answering, or might be your phone
        // ringing. Only the first is `Directed`.
        let mut how = Arrival::Directed;
        loop {
            // Whose voice was that?
            //
            // `voiceid` decides whether Atlas *listens*, never whether it is
            // allowed — a recording of you passes this, so anything
            // consequential still goes through the approval gate below. What
            // it stops is the television, a podcast, and the person at the
            // next desk driving your workspace.
            self.last_verdict = match ears.voiceprint() {
                Some(print) => {
                    let v = self.voice_id.check(&print, &self.tools_cfg().voice_id);
                    // A confirmed match folds back in, so a cold or a new
                    // headset does not gradually lock you out.
                    if let crate::voiceid::Verdict::You(s) = v {
                        self.voice_id.adapt(&print, &self.tools_cfg().voice_id);
                        // The score itself is kept too, so the fixed
                        // thresholds can be measured against where your
                        // voice actually lands. Measured, not moved — the
                        // adaptive-threshold question is an open ruling.
                        self.voice_id.note_accepted(s);
                        let _ = self.voice_id.save(&self.store);
                    }
                    v
                }
                // No encoder, or it failed. No opinion — never "not you".
                None => crate::voiceid::Verdict::NotEnrolled,
            };
            // Noted, and nothing more. This used to `break` -- the turn
            // ended, nothing was answered and nothing was said, so a cold or
            // a new headset read as Atlas being switched off. See
            // `voiceid::handle`'s own doc for why that is gone.
            // The evidence for where the lines should sit (`calibration_report`).
            match (&how, &self.last_verdict) {
                (_, crate::voiceid::Verdict::NotEnrolled) => {}
                (Arrival::Directed, v) => {
                    self.voice_id.note_after_name(v.score());
                    let _ = self.voice_id.save(&self.store);
                }
                (Arrival::OpenMic, crate::voiceid::Verdict::NotYou(s)) => {
                    self.voice_id.note_turned_away(*s);
                    let _ = self.voice_id.save(&self.store);
                }
                _ => {}
            }
            if matches!(self.last_verdict, crate::voiceid::Verdict::NotYou(_)) {
                self.log.info(&format!(
                    "a voice that didn't match yours ({:.2}) -- carrying on, and \
                     asking before anything consequential",
                    self.last_verdict.score()
                ));
            }

            let mut timed = crate::timing::Turn {
                about: about_short(&said),
                ..Default::default()
            };
            // The listen that produced these words happened just before this
            // was called. Absent when the ears can't split it — a stage that
            // wasn't measured is left out, not written down as zero.
            if let Some((recording, transcribing)) = ears.last_listen_split_ms() {
                timed.note(crate::timing::Stage::Listening, recording);
                timed.note(crate::timing::Stage::Hearing, transcribing);
            }

            let started = std::time::Instant::now();
            // What Eric feels (Phase 0.2): the silence from the end of his
            // speech to the first sound back. The words came back from
            // speech-to-text `hearing` ms after he stopped; the first sound
            // is caught where playback starts (`speaking::heard_now`).
            crate::speaking::listen_for_first_sound();
            let heard_ms = timed.get(crate::timing::Stage::Hearing).map(|ms| ms as u64);
            self.model_done_at.set(None);
            let calls_before = self.trace.recorded();
            // The model's reply is spoken a sentence at a time as it is
            // written (`speak_while_thinking`), rather than after the whole
            // of it -- the first words arrive in a second or two, not ten.
            //
            // Only a turn started here is spoken here (28 Sep 2026): a Talk
            // page turn still thinking is left to the Talk page's queue.
            let waiting_before = self.pending_turn.as_ref().map(|p| p.id);
            self.defer_turns = waiting_before.is_none();
            let reply = self.turn_from(&said, clock(), how);
            self.defer_turns = false;
            let started_here = self
                .pending_turn
                .as_ref()
                .filter(|p| Some(p.id) != waiting_before && p.talk.is_none())
                .map(|p| p.id);
            // The reply being said, from its first sentence (spoken while the
            // model still writes) to its last: one watch of the microphone and
            // one queue of sentences for all of it (28 Sep 2026: both started
            // again for every sentence).
            let mut saying: Option<crate::speakthread::Saying> = None;
            let (reply, spoke, cut_while_thinking) = match started_here {
                Some(id) => self.speak_while_thinking(id, ears, mouth, &mut saying),
                None => (reply, false, None),
            };
            // Whose voice it was is about that one utterance. Left standing,
            // a stranger's voice would still be "the last voice" when you
            // next typed, and a typed turn would be judged by it.
            self.last_verdict = crate::voiceid::Verdict::NotEnrolled;
            // "Doing" ends when the answer came back, not after it was
            // played: before Phase 0.2 the queued sentences were waited out
            // first, so the 30 Sep "17.5 s of thinking" was mostly speech.
            let doing = self.model_done_at.take().map(|at| at.saturating_duration_since(started)).unwrap_or_else(|| started.elapsed());
            timed.note(crate::timing::Stage::Doing, doing.as_millis().min(u32::MAX as u128) as u32);
            how = Arrival::OpenMic;
            if let Some(words) = cut_while_thinking {
                // Set aside for what you said instead.
                if let Some(mut s) = saying.take() {
                    s.cut(crate::speech::YOUR_TURN);
                    let _ = s.finish();
                }
                self.timing.add(timed);
                said = words;
                continue;
            }
            if reply.is_empty() && !spoke && saying.is_none() {
                self.timing.add(timed);
                break;
            }
            // Spoken so you can cut it off. `say_interruptibly` and the
            // parking it does in `self.unsaid` were built, tested, and never
            // called — every reply went through plain `say`, so every long
            // answer was unstoppable and "carry on" (a resume phrase
            // `attention` has recognised all along) had nothing to finish.
            //
            // How you cut in (27 Sep 2026): by holding the talk key. This
            // used to record a second and run speech-to-text on it before
            // every sentence and after the last -- a reply of five sentences
            // cost six recordings and six starts of the speech engine, most
            // of the wait for a spoken answer, and for a typed turn it was
            // listening for nothing. Now nothing is recorded unless the key
            // is down; what you say while holding it stops the reply and,
            // unless it was only "stop", is answered next.
            //
            // Or by voice, with `barge_in` on (28 Sep 2026): the
            // microphone's thread stops the playback when it hears you and
            // hands over what you said (`speakthread::Saying`).
            let cut_in: std::cell::RefCell<Option<String>> = std::cell::RefCell::new(None);
            // Taken out for the reply and put back, so the listen can read
            // the key while the reply borrows `self`.
            let talk_key = self.hotkeys.take();
            let mut quick_listen = || key_cut_in(talk_key.as_ref(), ears, &cut_in);
            let started_speaking = std::time::Instant::now();
            self.cut_in_by_voice = None;
            match saying.take() {
                // Started while the model wrote: the rest joins it.
                Some(mut s) => {
                    s.add(&reply);
                    let _ = self.end_saying(s, mouth, &mut quick_listen);
                }
                None if !reply.is_empty() => {
                    let _ = self.say_interruptibly(mouth, &reply, &mut quick_listen);
                }
                None => {}
            }
            self.hotkeys = talk_key;
            let speaking_ms = started_speaking.elapsed().as_millis() as u64;
            if let Some((synth, playback)) = mouth.last_speak_split_ms() {
                timed.note(crate::timing::Stage::Speaking, synth);
                timed.note(crate::timing::Stage::Playing, playback);
            }
            let first_sound = crate::speaking::take_first_sound().map(|at| at.saturating_duration_since(started).as_millis() as u64);
            self.silence_ms.set(crate::timing::silence_before_first_sound(heard_ms, first_sound));
            self.log_turn_timing(&timed, calls_before, speaking_ms);
            self.timing.add(timed);
            self.last_spoke_at = clock();
            if let Some(words) = cut_in.into_inner().or(self.cut_in_by_voice.take()).filter(|w| !w.trim().is_empty()) {
                said = words;
                continue;
            }

            // Keep the floor open. Being made to repeat the wake word to
            // answer a question is the fastest way to make a voice assistant
            // feel broken, so Atlas waits longer when it just asked something
            // -- and in a conversation, long enough to think of a reply.
            //
            // Hands-free (29 Sep 2026, on by default): the conversation stays
            // open until you've said nothing for `conversation.quiet_secs`,
            // then Atlas goes back to listening for its name. Nothing is
            // transcribed while nobody speaks (`utterance::next_utterance`),
            // so the long wait costs nothing.
            let talk = self.tools_cfg().conversation.clone();
            let window = if talk.hands_free {
                talk.quiet_secs.max(self.followup_secs)
            } else if self.session.is_waiting() {
                self.followup_secs * 2
            } else if self.last_register == crate::register::Register::Chatting {
                self.followup_secs.max(CHATTING_FOLLOWUP_SECS)
            } else {
                self.followup_secs
            };
            if let Some(u) = self.keyboard_followup() {
                said = u;
                continue;
            }
            // Not after a typed turn (27 Sep 2026): you typed, so the next
            // thing is typed too, and six to twelve seconds of recording
            // here held the whole loop -- the hub and the typing box
            // included -- after every typed answer.
            if self.tiers.tier == Tier::Typed || self.turn_was_typed {
                break;
            }
            // Paused means the microphone is off (28 Sep 2026), including
            // the open floor after a reply.
            if self.attention.is_paused() {
                break;
            }
            match self.follow_up(ears, window) {
                // "That's all", "bye", "thanks Atlas": said back briefly, the
                // floor closes, and Atlas goes back to listening for its name.
                Some(next) if crate::utterance::is_goodbye(&next) || crate::session::ends_the_conversation(&next) => {
                    self.log.info("the conversation ended: you said so");
                    self.thread.append(&next, "Anytime.", None, clock());
                    self.say(mouth, "Anytime.");
                    break;
                }
                Some(next) => said = next,
                None => {
                    if talk.hands_free {
                        self.log.info(&format!("the conversation ended: nothing said for {window}s"));
                    }
                    break;
                }
            }
        }
    }

    /// The open floor after a reply: up to `secs` of listening for you to
    /// carry on without the wake word.
    ///
    /// On the microphone's own thread when there is one (28 Sep 2026): the
    /// six to twenty seconds of recording here held the loop, and every page
    /// of the hub with it, after every spoken reply. The hub is answered
    /// while the thread listens; a pause, or Atlas closing, ends it at once.
    /// Ears that can't be handed to a thread (test stand-ins) listen here,
    /// as before.
    pub(super) fn follow_up(&mut self, ears: &dyn Ears, secs: u32) -> Option<String> {
        let Some(id) = self.mic.as_ref().filter(|m| !m.is_stopped()).map(|m| m.follow_up(secs)) else {
            return match ears.listen_briefly(secs) {
                Ok(got) => got.filter(|n| !n.trim().is_empty()),
                Err(e) => {
                    self.log.warn(&format!("listening after a reply failed: {e}"));
                    None
                }
            };
        };
        // The window, then time for the words to be made out.
        let until = std::time::Instant::now() + std::time::Duration::from_secs(u64::from(secs) + FOLLOW_UP_HEARING_SECS);
        loop {
            match self.mic.as_ref().and_then(|m| m.poll()) {
                Some(crate::micthread::Heard::FollowUp(n, got)) if n == id => {
                    return match got {
                        Ok(got) => got.filter(|n| !n.trim().is_empty()),
                        Err(e) => {
                            self.log.warn(&format!("listening after a reply failed: {e}"));
                            None
                        }
                    };
                }
                // An earlier one nobody waited for.
                Some(crate::micthread::Heard::FollowUp(..)) => {}
                // Anything else is for the next pass of the loop.
                Some(other) => {
                    if self.mic_heard.is_none() {
                        self.mic_heard = Some(other);
                    }
                }
                None => {}
            }
            if crate::goodbye::asked_to_stop() || self.attention.is_paused() || std::time::Instant::now() >= until {
                if let Some(m) = self.mic.as_ref() {
                    m.cancel_follow_up();
                }
                return None;
            }
            // Nothing is said from the hub into an open microphone; it waits
            // for the next ordinary answering.
            self.answer_hub_saying(None, 20);
        }
    }

    fn keyboard_followup(&self) -> Option<String> {
        None
    }

    /// `degrade`, with the reason written down first. Every failure here
    /// used to be `Err(_)`: the tier dropped and nothing anywhere said why,
    /// so a microphone that didn't exist looked exactly like a quiet room
    /// (29 Sep 2026).
    fn degrade_because(&mut self, mouth: &dyn Mouth, why: &dyn std::fmt::Display) {
        let why = why.to_string();
        // Silence isn't a broken microphone (`voice::HEARD_NOTHING`).
        if why.contains(crate::voice::HEARD_NOTHING) {
            self.log.info("listened and heard nothing");
            return;
        }
        self.log.warn(&format!("listening failed: {why}"));
        // The microphone may have gone (unplugged, the headset switched
        // off): look for the right one now rather than in a few minutes.
        self.mic_look_at = 0;
        self.degrade(mouth);
    }

    fn degrade(&mut self, mouth: &dyn Mouth) {
        if let Some(m) = self.tiers.failed() {
            self.say(mouth, &m);
        }
    }

    /// Sound & voice says a line may be spoken now: not muted, not quiet
    /// hours, and the reply rule allows this turn (`sound`).
    pub(crate) fn sound_allows_speaking(&self) -> bool {
        let now = crate::store::now() as i64 + crate::localclock::offset_secs();
        let minute = (now.rem_euclid(86_400) / 60) as u32;
        self.tools_cfg().sound.may_speak_now(minute, !self.turn_was_typed)
    }

    pub(super) fn say(&self, mouth: &dyn Mouth, line: &str) {
        crate::outln!("{line}");
        self.log.info(line);
        // Never over a reply still playing on its own thread.
        // The whole reply, not 30 seconds of it: a long reply on a busy
        // machine took 45 (30 Sep 2026), and the two then played at once.
        crate::speakthread::wait_quiet(std::time::Duration::from_secs(180));
        // Paused means quiet, not blind: the line is still printed and
        // logged, but the speaker stays silent. `may_speak` existed to
        // answer exactly this and `say` never asked, so a scheduled job
        // finishing mid-pause talked over the quiet you asked for.
        let speakable = self.tiers.tier != Tier::Typed || crate::input::can_speak(self.tools_ref());
        if speakable && self.attention.may_speak() && self.sound_allows_speaking() {
            // The screen got the line as written; the speaker gets it as
            // said. "$2.35" reads as words, "mph" is spoken not spelled,
            // a stray markdown marker is dropped rather than pronounced.
            let _voice = crate::speakthread::hold_voice();
            if let Err(e) = mouth.speak(&crate::spoken_form::for_speech(line)) {
                // Written down: a reply that failed to play was silence with
                // no reason anywhere (29 Sep 2026).
                self.log.warn(&format!("couldn't say it out loud: {e}"));
            }
        }
    }

    /// Speak a reply that you can cut off.
    ///
    /// Only an explicit stop or pause interrupts — a cough or a colleague
    /// does not. Anything cut off is returned rather than being recorded as
    /// said, because believing it told you something it never did is worse
    /// than the interruption itself.
    ///
    /// Played on its own thread (`speakthread`) when the mouth can give one:
    /// the loop answers the hub and watches the talk key all the while, not
    /// only between sentences.
    pub fn say_interruptibly(
        &mut self,
        mouth: &dyn Mouth,
        line: &str,
        listen: &mut dyn FnMut() -> Option<String>,
    ) -> crate::speech::Delivery {
        let mut s = self.start_saying(mouth);
        s.add(line);
        self.end_saying(s, mouth, listen)
    }

    /// A reply starts being said: printed only in the typed tier or when
    /// sound says not now; the microphone watched for your voice when
    /// cutting in by voice is on and Atlas isn't paused (`micthread`).
    pub(super) fn start_saying<'m>(&self, mouth: &'m dyn Mouth) -> crate::speakthread::Saying<'m> {
        let typed = (self.tiers.tier == Tier::Typed && !crate::input::can_speak(self.tools_ref())) || !self.sound_allows_speaking();
        let watching = if typed || self.attention.is_paused() {
            None
        } else {
            self.mic.as_ref().map(|m| m.link()).filter(|l| l.barge_on())
        };
        crate::speakthread::Saying::start(mouth, typed, watching)
    }

    /// Said to the end (or cut off): what wasn't said parked for "carry on",
    /// starting with the sentence that was cut, and the cut acknowledged.
    fn end_saying(
        &mut self,
        mut s: crate::speakthread::Saying,
        mouth: &dyn Mouth,
        listen: &mut dyn FnMut() -> Option<String>,
    ) -> crate::speech::Delivery {
        s.wait(self, listen);
        self.stop_saying(s, mouth)
    }

    /// The reply is over, as far as it got.
    pub(super) fn stop_saying(&mut self, s: crate::speakthread::Saying, mouth: &dyn Mouth) -> crate::speech::Delivery {
        let said = s.finish();
        let over_it = said.words.clone();
        if let Some(w) = said.words {
            self.cut_in_by_voice = Some(w);
        }
        let d = said.delivery;
        if d.was_interrupted() {
            // Cut by a sound that wasn't words -- a cough, a door, Atlas's own
            // voice coming back -- is no reason to stop: the rest is said
            // (29 Sep 2026: "Paused." and then nothing, over and over).
            if over_it.as_deref().is_some_and(|w| crate::voice::not_really_said(w)) {
                self.cut_in_by_voice = None;
                self.unsaid = None;
                let rest = d.remaining_text();
                if !rest.trim().is_empty() {
                    self.log.info("cut in by a sound, not words: carrying on");
                    self.say(mouth, &rest);
                }
                return d;
            }
            // Park what was not said, so "carry on" can finish it.
            self.unsaid = Some(d.remaining_text());
            // Words said over it are answered next, straight away: no
            // "Paused." in front of the answer. A stop still says so.
            // (Merged 30 Sep 2026: both chats fixed this. The other chat
            // skipped the "Paused." here when words came with the cut; this
            // chat's `speech::YOUR_TURN` is the reason given for any cut that
            // is you taking the turn -- your voice, the talk key, the cut
            // switch -- and `acknowledge` says nothing for it, so the check
            // here was the same thing twice and went.)
            let ack = crate::speech::acknowledge(&d);
            if !ack.is_empty() {
                self.say(mouth, &ack);
            }
        } else {
            self.unsaid = None;
        }
        d
    }

    /// The rest of an interrupted reply, if you ask for it.
    pub fn finish_saying(&mut self) -> Option<String> {
        self.unsaid.take()
    }
}

/// Anything learned or scheduled is flushed on shutdown. Without this, state
/// changed outside a turn — a job you scheduled, a preference set — is lost
/// when the daemon stops.
///
/// Still the backstop rather than the mechanism. `Drop` only runs when the
/// value goes out of scope, which a killed process never does, and it cannot
/// stop helpers or release the lock in a defined order because the order
/// fields drop in is not the order the way out needs. `shut_down` is the way
/// out; this catches the paths that do not go through it — the CLI, the
/// tests, a panic unwinding out of `run`.
impl Drop for Daemon<'_> {
    fn drop(&mut self) {
        // Other programs Atlas started (`mcp`) end with it.
        self.mcp.stop_all();
        if self.stopped {
            return;
        }
        self.persist();
    }
}

/// Where `persist` keeps the workflow in hand.
pub(super) const FLOW_LEFT: &str = "current_flow";

/// Where `persist` keeps what a restart would cut off.
pub(super) const LEFT_WAITING: &str = "left_waiting";

/// One thing left waiting when Atlas stopped.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct LeftWaiting {
    pub what: String,
    /// A question to you (true), or work being done (false).
    pub asked: bool,
    pub at: u64,
}


