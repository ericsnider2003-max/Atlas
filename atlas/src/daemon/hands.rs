//! Looking and hands: gestures, looking at the room and the screen, call notes,
//! working in your windows for you, reaching you, audio outputs and the panel.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// A hand answering the question Atlas already asked.
    ///
    /// Routed through the same offer path a spoken "yes" takes, so a gesture
    /// cannot reach anything a word could not. `answering::saw` decides
    /// whether a thumb is allowed to approve *this* question — a thumbs-down
    /// always is, because declining is safe at any stakes; a thumbs-up is not
    /// when the thing is consequential, and then Atlas asks for a word.
    fn gesture_answers(&mut self, g: crate::presence::Gesture, t: u64) -> Option<String> {
        let Pending::Clarification(_) = self.session.pending.clone() else {
            // Nothing on the table. A gesture with no question is not a
            // command, and treating it as one is the failure this is designed
            // around.
            return None;
        };
        let offer = self.pending_offer.clone()?;
        let cfg = self.tools_cfg().answering.clone();
        // Whether a thumb may settle this is decided by what the offer would
        // actually do, not by the offer's own opinion of itself. `earned`
        // already ranks the cost of being wrong per kind of work, and anything
        // above reversible housekeeping needs a word.
        let intent = self.parser.parse(&offer.command);
        let consequential =
            crate::earned::kind_of(&intent).cost_of_being_wrong() > 2;
        let mut q = crate::answering::Question::new(&offer.message, consequential, t);
        match q.saw(g, &cfg) {
            crate::answering::Step::Settled(answer) => {
                let yes = answer.approved();
                self.proactive.record_response(&offer.kind, yes, &mut self.memory);
                self.session.pending = Pending::Nothing;
                self.pending_offer = None;
                if yes {
                    let reply = self.run_command(&offer.command, t);
                    self.persist();
                    return Some(reply);
                }
                self.persist();
                Some("Alright.".into())
            }
            // Consequential, and a thumb is not enough. Said out loud rather
            // than ignored, or the gesture looks broken.
            crate::answering::Step::NeedsAWord(why) => Some(why),
            _ => None,
        }
    }

    /// Look at the room, if it is time and Atlas is allowed to.
    ///
    /// Closes the loop `presence` and `answering` were written for. Both have
    /// been complete and tested since the day they were written, and neither
    /// has ever been given anything to work with.
    ///
    /// Returns anything worth saying out loud — which is only ever a gesture
    /// answering a question Atlas already asked. Seeing you at your desk is
    /// not news.
    pub(super) fn look_at_the_room(&mut self, t: u64) -> Option<String> {
        let tools = self.tools_cfg();
        let cfg = tools.gaze.clone();

        // The camera opens for a reason and closes when the reason ends. The
        // first version looked every twenty seconds on a timer, which watches
        // the room when nothing needs watching and is blind at the one moment
        // that matters — Atlas asks a question and then does not look again
        // for nineteen seconds while a hand is held up in front of it.
        let now = crate::gaze::Situation {
            question_waiting: matches!(self.session.pending, Pending::Clarification(_)),
            mid_conversation: !self.thread.recent.is_empty()
                && t.saturating_sub(self.awareness.last_spoke) < 30,
            about_to_be_private: false,
            asked_to_watch: self.watching_me,
            steering: self.steering_until.is_some_and(|until| t < until),
            watch_while_talking: cfg.watch_while_talking,
        };
        let Some(reason) = crate::gaze::why_look(&now, &cfg) else {
            // Nothing needs looking at. Not an error and not worth saying —
            // but the camera really is off, rather than idling.
            return None;
        };
        if t.saturating_sub(self.looked_at) < crate::gaze::how_often(reason) {
            return None;
        }
        self.looked_at = t;

        let began = std::time::Instant::now();
        let printed = self.take_a_look_raw(&tools);
        // What it actually costs on this machine, rather than what it was
        // assumed to cost. Everything about pacing follows from this one
        // number and nothing else can know it.
        self.pace
            .took(began.elapsed().as_millis().min(u32::MAX as u128) as u32);
        if let Some(struggling) = self.pace.keeping_up(&tools.pace) {
            // Once, not every look. `may_raise` is the same back-off every
            // other nudge obeys.
            if self.nudger.may_raise("hand tracking", t) {
                self.nudger.raised("hand tracking", &struggling, t);
                return Some(struggling);
            }
        }

        // Steering reads a different vocabulary, and only while steering. A
        // swipe can never answer a question and a thumb can never move a
        // window, because neither is ever read in the other mode.
        if reason.hands_may_steer() {
            let hand = crate::gaze::read_hand(&printed, &cfg).map(|h| {
                // Smoothed before anything acts on it. A detector wobbles by a
                // pixel or two on a hand that isn't moving, and an unsmoothed
                // pointer shivers, which reads as broken.
                let ms = (t.saturating_mul(1000)) as u32;
                let (x, y) = self.track.saw(h.x, h.y, ms, &tools.smoothing);
                crate::gaze::Hand { x, y, ..h }
            });
            if hand.is_none() {
                self.track.lost();
            }
            return self.steer_displays(hand, t);
        }

        let seen = crate::gaze::read(&printed, &cfg);

        // The rule the whole module turns on. An unread camera is not an empty
        // room: one of those means "speak freely, he's gone" and the other
        // means "you have no idea". Handing an empty sighting to `observe`
        // would report nobody there on a machine whose camera simply failed.
        let Some(look) = seen.as_look().filter(|_| seen.saw_anything()) else {
            self.eyes.blind();
            self.last_sighting = Some(seen);
            return None;
        };

        let change = self.eyes.observe(look, t);
        if let crate::presence::Change::Returned = change {
            self.log.info("you're back at the desk");
        }

        // A hand only ever answers a question already on the table. Never a
        // command: a misread gesture that opens an app is confusing, and one
        // that approves something is unacceptable.
        let said = seen.gesture.and_then(|g| self.gesture_answers(g, t));
        self.last_sighting = Some(seen);
        said
    }

    /// Run the detector over one frame and hand back what it printed.
    ///
    /// Raw rather than parsed, because two vocabularies read the same frame
    /// and each must be read only in its own mode.
    fn take_a_look_raw(&mut self, tools: &crate::voice::ToolsConfig) -> String {
        let Some(detector) = tools.gaze_detector.as_ref().cloned() else {
            // Atlas reads the frame itself.
            //
            // This is the line that closes a loop that has been open since
            // `gaze` was written. `gaze` was built to *read* a detector's
            // output rather than to be one, so that any detector could be
            // plugged in — a good shape, with one consequence nobody drew out:
            // under the offline rule there was never going to be a
            // third-party detector to plug in. So `gaze_detector` stayed
            // commented out in tools.yaml, this function returned "no detector
            // is set up" every time, and everything downstream of it —
            // presence, returning to the desk, a hand answering a question —
            // has never once run on a real picture.
            return if tools.vision.enabled {
                let you = tools.vision.your_face.clone();
                self.see().as_lines(&you)
            } else {
                "could_not: seeing is switched off in settings".into()
            };
        };
        let Some(capture) = tools.capture_webcam.as_ref() else {
            return "could_not: there's no camera configured".into();
        };

        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
        let frame = std::path::Path::new(&tools.work_dir).join("look.png");
        let mut v = vars.clone();
        v.insert("out_image".into(), frame.to_string_lossy().to_string());
        if capture.run(&v, None).is_err() {
            return "could_not: the camera didn't give me a picture".into();
        }
        v.insert("image".into(), frame.to_string_lossy().to_string());
        let printed = match detector.run(&v, None) {
            Ok(p) => p,
            // The frame goes either way. A picture of the room is not
            // something to leave lying about because a tool crashed.
            Err(e) => {
                let _ = std::fs::remove_file(&frame);
                return format!("could_not: the detector failed: {e}");
            }
        };
        // Read, then gone — the same rule as watching a video. Atlas keeps
        // what it worked out, never the picture of you.
        let _ = std::fs::remove_file(&frame);
        printed
    }



    /// What `diagnose` needs to check Atlas on itself.
    ///
    /// Assembled from what the daemon already knows rather than probed again —
    /// `doctor` has already found the missing tools and models by the time
    /// anything asks for this.
    pub(crate) fn vitals(&self) -> crate::diagnose::Vitals {
        let r = crate::health::read_machine();
        crate::diagnose::Vitals {
            config_loaded: self.tools_ref().is_some(),
            config_error: None,
            state_writable: self.store.root().exists(),
            missing_tools: Vec::new(),
            missing_models: Vec::new(),
            scratch_dir_missing: !std::path::Path::new(&self.tools_cfg().work_dir).exists(),
            preserved_files: self.store.preserved().len(),
            disk_free_gb: r.disk_free_gb,
            ram_used_fraction: if r.ram_total_gb > 0.0 {
                r.ram_used_gb / r.ram_total_gb
            } else {
                0.0
            },
            // The timing window Atlas already keeps. `diagnose` asked for the
            // slowest stage and nothing had ever handed it one.
            slowest_stage: self
                .timing
                .worst_stage()
                // `plain()`, not `{s:?}`: `diagnose` drops this straight
                // into a sentence you read — "Hearing is taking 5.2
                // seconds" was a variant name pretending to be English.
                .map(|(s, ms)| (s.plain().to_string(), ms as u64)),
            ..crate::diagnose::Vitals::default()
        }
    }


    /// What did that pinch actually land on?
    ///
    /// `uia` — reading an open window without a screenshot — was written,
    /// tested, and called by nothing. It is what tells a button from a label,
    /// which is the difference between "clicked at 840,220" and "selected
    /// Send". A click Atlas cannot name is one that can never be trusted with
    /// anything that matters.
    fn note_what_was_clicked(&mut self, x: i32, y: i32) {
        let Some(id) = self.plat.window_at(x, y).ok().flatten() else {
            return;
        };
        // The app this window belongs to, so the router learns per-app rather
        // than in general -- UIA can be perfect in one app and blind in
        // another. Falls back to the window id when the platform can't name
        // the process, so a reading is still recorded against *something*
        // stable rather than dropped.
        let app = self
            .plat
            .active_window()
            .ok()
            .flatten()
            .map(|w| w.process)
            .filter(|p| !p.is_empty())
            // `id.0`, not `{id:?}`. This string is the key the router learns
            // against, and `atlas backends` prints those keys back to you --
            // so `{id:?}` would have put "window:WindowId(12345)" on screen,
            // a Rust newtype arriving as English. `tests/hub_is_not_code.rs`
            // caught it; it was mine, in the `backends::Router` wiring the
            // 17 Sep merge dropped and I restored by hand.
            .unwrap_or_else(|| format!("window:{}", id.0));
        let Ok(Some(tree)) = self.plat.read_window(id) else {
            // The app publishes nothing readable. Said plainly rather than
            // pretending the click was understood -- and recorded as a failed
            // accessibility read on this app, so the router learns UIA does
            // not work here.
            self.backends.record(crate::backends::Backend::Uia, &app, false);
            self.log.info("selected something I can't read the name of");
            return;
        };
        let cfg = self.tools_cfg().uia.clone();
        match crate::uia::assess(&tree, cfg.min_nodes, cfg.min_named_ratio) {
            crate::uia::Quality::Usable => {
                let what = crate::uia::outline(&tree, 1);
                // A real, usable accessibility read: UIA works on this app.
                self.backends.record(crate::backends::Backend::Uia, &app, true);
                self.log.info(&format!("selected: {}", what.trim()));
            }
            other => {
                // Named, because "too shallow" and "unlabelled" send you to
                // different fixes -- one is the app, one is a setting. Recorded
                // as a failed read: the tree came back but wasn't usable, which
                // for the router's purpose is the same as UIA not working here.
                self.backends.record(crate::backends::Backend::Uia, &app, false);
                self.log.info(&format!("selected something I can't read: {}", other.plain()));
            }
        }
    }


    /// The camera and models the tracking thread will own.
    ///
    /// Built here and handed over whole, so the camera opens as the thread
    /// starts and closes when it ends — there is no window in which a webcam
    /// is lit with nothing looking through it.
    ///
    /// Returns `None` rather than a double that would report no hands forever,
    /// which is indistinguishable from a camera that cannot see.
    /// One picture from the camera, as raw pixels.
    ///
    /// Through `frames` rather than the one-shot capture command, for a reason
    /// that is not about speed: the one-shot writes a PNG, and Atlas has no
    /// picture decoder — adding one would be a dependency bought to undo work
    /// that did not need doing. `frames` already asks for raw pixels, so the
    /// picture arrives in the form the models want.
    ///
    /// The camera is closed on the way out of this function, every time.
    pub(super) fn one_frame(&mut self) -> std::result::Result<(Vec<u8>, usize, usize), String> {
        let tools = self.tools_cfg();
        let capture = tools
            .capture_webcam
            .as_ref()
            .ok_or("there's no camera set up on this machine")?
            .clone();
        let feed = crate::frames::Feed {
            open_with: crate::frames::from_capture_args(&resolved(&capture.args, &tools.vars)),
            ..crate::frames::Feed::default()
        };
        // Asked for, rather than just taken. The camera is ~60MB of ffmpeg
        // and it is the one helper Atlas opens on your say-so, so it is the
        // right place for the budget to be able to say no — and for the
        // refusal to be a sentence rather than a machine that swaps.
        let t = crate::store::now();
        let name = "camera";
        self.helpers
            .want(name, crate::lifecycle::typical_mb(name), t, || Ok(None))
            .map_err(|why| format!("I can't open the camera right now: {why}"))?;
        let mut rolling = match crate::frames::Rolling::start(&feed) {
            Ok(r) => r,
            Err(e) => {
                // It never opened, so it must not stay on the books.
                self.helpers.finished(name);
                return Err(format!("the camera wouldn't open: {e}"));
            }
        };
        let (w, h) = rolling.size();
        // The first frame off a camera that has just opened is often the one
        // taken while it was still working out its exposure. Reading a couple
        // and keeping the last costs a fraction of a second and is the
        // difference between a dark picture and a usable one.
        let mut frame = None;
        for _ in 0..3 {
            if let Some(f) = rolling.next() {
                frame = Some(f.to_vec());
            }
        }
        // Closed on the way out, every time -- so it leaves the budget too.
        drop(rolling);
        self.helpers.finished(name);
        frame
            .map(|f| (f, w, h))
            .ok_or_else(|| "the camera didn't hand back a picture".to_string())
    }

    /// Open the seeing models, if they are not open already.
    pub(super) fn start_looking(&mut self) -> &mut crate::vision::Looking {
        if self.looking.is_none() {
            let models = std::path::Path::new(&self.tools_cfg().models.dir).to_path_buf();
            self.looking = Some(crate::vision::Looking::open(&models));
        }
        self.looking.as_mut().expect("just filled in")
    }

    /// Look, once.
    ///
    /// Every refusal names the thing to fix, and every one of them is a
    /// `Sight::Unread` rather than an empty scene — the distinction the whole
    /// module turns on.
    fn see(&mut self) -> crate::vision::Sight {
        let cfg = self.tools_cfg().vision.clone();
        if !cfg.enabled {
            return crate::vision::Sight::Unread(
                "seeing is switched off — turn on Recognising things in settings and I'll \
                 pick this up"
                    .into(),
            );
        }
        let (frame, w, h) = match self.one_frame() {
            Ok(f) => f,
            Err(why) => return crate::vision::Sight::Unread(why),
        };
        let album = self.album.clone();
        let looking = self.start_looking();
        looking.look(&frame, w, h, &cfg, &album)
    }

    /// "Look at my screen", "what does this chart show?" — a picture taken
    /// and asked about with the local picture reader (`picture_talk`).
    ///
    /// This arm used to answer "Capture runs through the voice layer." —
    /// true of an older door, and the running Atlas never looked at anything.
    /// The screenshot is deleted as soon as it has been read: a picture of
    /// your screen is the most private file Atlas could leave lying about.
    pub(super) fn look_closer(&mut self, what: Capture) -> String {
        let tools = self.tools_cfg();
        let root = self.store.install_root();
        if let Err(why) = crate::picture_talk::ready(&tools.picture_talk, &root) {
            // The screen's words can still be read, by Windows' own recognizer.
            if matches!(what, Capture::Screen) {
                if let Some(said) = self.screen_words_instead() {
                    return said;
                }
            }
            return format!("I can't read pictures yet: {why}.");
        }
        let (tool, word) = match what {
            Capture::Screen => (tools.capture_screen.clone(), "screen"),
            Capture::Camera => (tools.capture_webcam.clone(), "webcam"),
        };
        let dir = std::path::PathBuf::from(&tools.work_dir);
        let _ = std::fs::create_dir_all(&dir);
        let t = crate::store::now();
        let shot = dir.join(format!("{word}_{t}.png"));
        // A question about another app ("what does Slack say?") brings that
        // window forward for the picture and puts yours back afterwards
        // (Eric, G3) — but never while you're in the middle of something.
        let target = if word == "screen" {
            crate::probe::target_for(&self.last_said, self.cfg)
        } else {
            crate::probe::Target::Active
        };
        // The screen you're working on -- or every screen, or the one you
        // named -- captured by Atlas itself, in real pixels (29 Sep 2026).
        // The configured capture (ffmpeg's gdigrab of the whole desktop) is
        // not DPI-aware: with the laptop's screen scaled and the monitors
        // not, it cropped the desktop wrongly, and one picture of three
        // screens shrunk for the reader left each too small to read.
        let mut label = format!("your {word}");
        let mut taken_here = false;
        if matches!(what, Capture::Screen) && matches!(target, crate::probe::Target::Active) {
            if let Some((named, grab)) = self.screen_picture() {
                let rgba: Vec<u8> = grab.rgb.chunks_exact(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect();
                let png = crate::pngcodec::write_png(&crate::pngcodec::Rgba { width: grab.width, height: grab.height, pixels: rgba });
                if std::fs::write(&shot, png).is_ok() {
                    label = named;
                    taken_here = true;
                }
            }
        }
        if !taken_here {
            let Some(tool) = tool else {
                return format!("There's no {word} capture set up on this machine.");
            };
            let mut vars = tools.vars.clone();
            vars.insert("out_png".into(), shot.display().to_string());
            if let crate::probe::Target::App(app) = &target {
                if self.plat.input_idle_secs().map_or(false, |s| s < 3) {
                    return format!(
                        "You're in the middle of something, so I won't move your windows to look at {app}. \
                         Ask again when you pause."
                    );
                }
                let shoot = || -> crate::error::Result<String> {
                    tool.run(&vars, None).map(|_| shot.display().to_string())
                };
                if let Err(e) = self.probe.gather(self.cfg, self.plat, &shoot, &target) {
                    let _ = std::fs::remove_file(&shot);
                    return format!("I couldn't take the picture of {app}: {e}");
                }
            } else if let Err(e) = tool.run(&vars, None) {
                // A capture that failed partway can leave a half-written file.
                let _ = std::fs::remove_file(&shot);
                return format!("I couldn't take the picture: {e}");
            }
        }
        let small = crate::picture_talk::smaller(&shot);
        // About 3 GB while it runs, so the memory budget gets a say first,
        // and a refusal is a sentence rather than a laptop that swaps.
        let name = "picture reader";
        if let Err(why) = self.helpers.want(name, crate::picture_talk::MEMORY_MB, t, || Ok(None)) {
            let _ = std::fs::remove_file(&shot);
            if let Some(p) = &small {
                let _ = std::fs::remove_file(p);
            }
            if matches!(what, Capture::Screen) {
                if let Some(said) = self.screen_words_instead() {
                    return said;
                }
            }
            return format!("I can't read it right now: {why}");
        }
        // The camera is asked about you, not about a screen (30 Sep 2026).
        let question = match what {
            Capture::Camera => crate::camera_ask::question(&self.last_said),
            Capture::Screen => crate::picture_talk::question_for(&self.last_said),
        };
        let shot_again = shot.clone();
        // The model takes a while on a laptop, so it's a crew errand: Atlas
        // keeps listening, "stop" reaches it, and the answer is said when
        // it's ready. The memory it holds is handed back when it settles.
        let cfg = tools.picture_talk.clone();
        let small_again = small.clone();
        let work: crew::Work = Box::new(move |c: &crew::Control| {
            let started = std::time::Instant::now();
            let answer =
                crate::picture_talk::ask_until(&cfg, &root, small.as_deref().unwrap_or(&shot), &question, &|| c.stopping());
            let _ = std::fs::remove_file(&shot);
            if let Some(p) = &small {
                let _ = std::fs::remove_file(p);
            }
            let o = PictureOutcome {
                took_ms: started.elapsed().as_millis() as u64,
                prompt_chars: question.len(),
                answer: answer.as_ref().ok().cloned().unwrap_or_default(),
                failed: answer.err(),
            };
            serde_json::to_string(&o).map_err(|e| e.to_string())
        });
        if self.hand_off("pictures", t, work, Some(word.to_string()), SpeakPolicy::Always) {
            if matches!(what, Capture::Camera) {
                // Said as it happens: the camera is never on unannounced.
                return format!("{} — I'll tell you what I see in a moment.", crate::camera_ask::LOOKING);
            }
            format!("Looking at {label} — I'll tell you in a moment.")
        } else {
            self.helpers.finished(name);
            let _ = std::fs::remove_file(&shot_again);
            if let Some(p) = &small_again {
                let _ = std::fs::remove_file(p);
            }
            "I have too much on to look right now — ask me again in a minute.".into()
        }
    }

    /// Once per tick: look for a call (every five seconds), finish any
    /// write-ups, and clear out audio older than you keep it.
    pub(super) fn call_notes_tick(&mut self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        self.call_notes.cfg = self.tools_cfg().call_notes.clone();
        // Paused: the recording holds — nothing captured, the call's notes
        // kept and carried on when you're back. Pausing holds, it doesn't
        // erase (the crew's rule, and Eric's for single errands).
        if let Some(line) = self.call_notes.hold(self.attention.is_paused()) {
            out.push(line);
        }
        // Switched off, or handed to someone else: the call being noted ends
        // with what was taken so far written up. Someone else at the laptop
        // may be on their own call, which is not yours to note.
        let why_not = if !self.call_notes.cfg.enabled {
            Some("Call notes were switched off, so I've stopped recording.")
        } else if self.handover().stance.handed_over() {
            Some("I'm handed over to someone else, so I've stopped recording the call.")
        } else {
            None
        };
        if let Some(why) = why_not {
            if self.call_notes.call.is_some() {
                let said = self.call_notes.end(t);
                out.push(why.to_string());
                out.extend(self.carry_out(said));
            }
        } else if t.saturating_sub(self.last_call_look) >= 5 {
            self.last_call_look = t;
            let said = self.call_notes.look(t, crate::callwatch::call_now());
            out.extend(self.carry_out(said));
        }
        let keep = self.call_notes.cfg.keep_audio_days;
        for old in crate::callnotes::audio_to_delete(&self.call_notes.dir, t, keep) {
            let _ = std::fs::remove_file(old);
        }
        out
    }

    /// Say what call notes said, and start writing up a finished call.
    fn carry_out(&mut self, said: crate::callnotes::Said) -> Vec<String> {
        if let Some(done) = said.to_transcribe {
            let tools = self.tools_cfg();
            let notes_dir = self.notes_dir();
            let llm = self.llm.clone();
            let mut vars = tools.vars.clone();
            self.add_language_vars(&mut vars);
            match tools.stt_timed.clone() {
                Some(timed) => {
                    // A crew errand, not a thread of its own: "what's
                    // queued" names it, "pause" holds it with the rest, and
                    // shutdown waits for it the way it waits for the crew.
                    let app = done.app.clone();
                    let work: crew::Work = Box::new(move |_c: &crew::Control| {
                        let w = crate::callnotes::write_up(&done, &timed, &vars, llm.as_deref(), &notes_dir)?;
                        serde_json::to_string(&w).map_err(|e| e.to_string())
                    });
                    if !self.hand_off("call-notes", crate::store::now(), work, Some(app), SpeakPolicy::Always) {
                        let mut lines = said.lines;
                        lines.push("I have too much on to write the call up right now; the audio is kept in data/calls.".into());
                        return lines;
                    }
                }
                None => {
                    let mut lines = said.lines;
                    lines.push("I recorded the call but there's no transcriber set up, so the audio is kept in data/calls.".into());
                    return lines;
                }
            }
        }
        said.lines
    }

    /// "Take notes on this call", "record everyone", "they said yes/no",
    /// "stop taking notes".
    pub(super) fn call_notes_command(&mut self, what: &str) -> String {
        let t = crate::store::now();
        self.call_notes.cfg = self.tools_cfg().call_notes.clone();
        // "Stop" and "are you recording" always work, switched off or not:
        // a way out must never depend on a setting.
        let always = matches!(what, "status" | "off" | "stop" | "what_it_does");
        if !self.call_notes.cfg.enabled && !(always && self.call_notes.call.is_some()) {
            return "Call notes are switched off — turn on Call notes in Settings and I'll take them.".into();
        }
        let said = match what {
            "start" => {
                if self.call_notes.call.is_some() {
                    return "I'm already noting this call.".into();
                }
                match crate::callwatch::call_now() {
                    Some(app) => {
                        self.call_notes.watch.saw(Some(app));
                        self.call_notes.begin(app, t)
                    }
                    // No call app seen: noted until you say stop, rather
                    // than ended five seconds later when the watch looks.
                    None => self.call_notes.begin_by_hand("this call", t),
                }
            }
            "everyone" => self.call_notes.everyone(),
            "agreed" => self.call_notes.they_agreed(),
            "declined" => self.call_notes.they_declined(),
            "couldnt_ask" => self.call_notes.couldnt_ask(),
            "no_answer" => self.call_notes.nobody_answered(),
            "just_mine" => self.call_notes.just_mine(),
            "status" => return self.call_notes.status(),
            "what_it_does" => return self.call_notes.what_it_does(),
            // The watch is left as it is: it still sees the call, so it
            // won't see it "start" again five seconds later and undo your
            // stop. It notices the next call once this one has ended.
            _ => self.call_notes.end(t),
        };
        let lines = self.carry_out(said);
        if lines.is_empty() {
            "There's no call I'm noting.".into()
        } else {
            lines.join(" ")
        }
    }

    /// "Finish this conversation until I'm back", "draft a reply to this".
    ///
    /// Starts a window errand. It doesn't replace or stop anything else
    /// Atlas is doing, and nothing you ask afterwards stops it — only "stop"
    /// (or "stop the Slack one" with several going), like every errand.
    pub(super) fn start_working_for_you(&mut self, said: &str, _intent: &Intent) -> String {
        let dcfg = self.tools_cfg().delegate.clone();
        if !dcfg.enabled {
            return "Working your apps is switched off — turn it on in Settings and I'll take this.".into();
        }
        if self.llm.is_none() {
            return "I need a model set up to write replies, and there isn't one yet.".into();
        }
        let (Ok(Some(win)), Ok(Some(front))) = (self.plat.active_window_id(), self.plat.active_window()) else {
            return "I can't tell which window you mean — put it in front and ask again.".into();
        };
        let app = front.process.trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
        if app.is_empty() {
            return "I can't tell which app that window belongs to.".into();
        }
        // No "may I use it?" question here. That question (`grants` rule 1)
        // is for an app Atlas would pick on its own; this is the window you
        // put in front and told it to work — you naming the app is the
        // permission (rule 2). Apps you've marked to confirm every message
        // (rule 3) still never get a send from Atlas: see below.
        let facts = self.app_facts(&app);
        let mut job = crate::delegate::for_the_window(&app, said, &dcfg);
        let mut lead_note = String::new();
        if facts.confirm_each_time && job.reach == crate::delegate::Reach::Converse {
            job.reach = crate::delegate::Reach::Draft;
            job.max_turns = 1;
            lead_note = format!(" {app} is one you confirm every message in, so I'll leave my reply in the box for you to send.");
        }
        // A new instruction for a window already being worked replaces the
        // old one there — one set of hands per window.
        let mut replaced = String::new();
        if let Some(i) = self.working_for_you.iter().position(|w| w.win == win) {
            let old = self.working_for_you.remove(i);
            replaced = format!(" (That replaces what I was doing in {}.)", old.job.app);
        }
        let drafting = job.reach == crate::delegate::Reach::Draft;
        let id = self.next_window_job;
        self.next_window_job += 1;
        let now = crate::store::now();
        self.working_for_you.push(WorkingForYou {
            id,
            job,
            win,
            after_mine: None,
            looked: 0,
            started: now,
            held: false,
            waiting_for_gap: false,
            composing: None,
            composed_from: None,
            ready: None,
        });
        // The first reply goes now: you just asked, with the window in front.
        let first = self.work_one(id, now, true);
        let lead = if drafting {
            format!("Drafting a reply in {app}; I won't send it.{lead_note}{replaced}")
        } else {
            format!(
                "Carrying on the conversation in {app} — keep working, I'll only type when you pause. \
                 Say \"stop the {app} one\" to stop it.{replaced}"
            )
        };
        if first.is_empty() { lead } else { format!("{lead} {}", first.join(" ")) }
    }

    /// Once a tick: every window job, each at most every ten seconds.
    /// `busy` is you typing, clicking or talking right now (`lanes`): a job
    /// with something to write waits for a gap rather than fighting you for
    /// the keyboard.
    pub(super) fn work_for_you(&mut self, t: u64, busy: bool) -> Vec<String> {
        // Paused, or handed to someone else: every job holds where it is,
        // with nothing lost, and carries on when you're back.
        if self.attention.is_paused() || self.handover().stance.handed_over() {
            return Vec::new();
        }
        // The first reply of a job you've just asked for goes as soon as
        // your hands are off the keyboard — the twenty-second gap is for
        // work you didn't ask for this moment, and you've just spoken to
        // Atlas, which would otherwise count as busy.
        let hands_off = self.plat.input_idle_secs().map(|s| s >= 2).unwrap_or(true);
        let jobs: Vec<(u64, bool)> = self.working_for_you.iter().map(|w| (w.id, w.job.turns == 0)).collect();
        let mut out = Vec::new();
        for (id, first) in jobs {
            out.extend(self.work_one(id, t, !busy || (first && hands_off)));
        }
        out
    }

    /// One look at one window job. `may_type` is whether the keyboard is
    /// free for it: you've asked just now, or you've stopped typing.
    ///
    /// The model is never asked on the tick: that can take a minute on a
    /// local model, and the tick never waits (`crew`). Writing the reply is
    /// handed to the crew; the reply comes back through `take_crew_news`
    /// into `ready`, and is typed here once there's a gap.
    fn work_one(&mut self, id: u64, t: u64, may_type: bool) -> Vec<String> {
        let mut out = Vec::new();
        let Some(i) = self.working_for_you.iter().position(|w| w.id == id) else { return out };
        let mut w = self.working_for_you.remove(i);
        let keep = |me: &mut Self, w: WorkingForYou| me.working_for_you.insert(i.min(me.working_for_you.len()), w);
        if w.held || w.composing.is_some() {
            keep(self, w);
            return out;
        }
        // A reply written and waiting: typed as soon as the keyboard is free.
        if let Some(text) = w.ready.take() {
            if !may_type {
                if !w.waiting_for_gap {
                    w.waiting_for_gap = true;
                    self.log.info(&format!("{}: reply ready; waiting for a gap in your typing", w.job.app));
                }
                w.ready = Some(text);
                keep(self, w);
                return out;
            }
            w.waiting_for_gap = false;
            if let crate::delegate::Step::Place { text, send } = w.job.composed(&text) {
                // A blank left in it ("[Your Name]") is never sent as you: it
                // goes in the box for you to fill, and the job waits for you.
                let blanks = crate::draft::blanks(&text);
                let send = send && blanks.is_empty();
                if !blanks.is_empty() {
                    out.push(format!(
                        "I've left my reply in {} unsent — it has a blank to fill in ({}).",
                        w.job.app,
                        blanks.join(", ")
                    ));
                    w.held = true;
                }
                match self.type_into(w.win, &text, send) {
                    Ok(()) if send => {
                        out.push(format!("Replied in {} ({} so far).", w.job.app, w.job.turns));
                        // Something went out as you: on the record, like a
                        // published post.
                        let gist: String = text.chars().take(80).collect();
                        self.journal.record_at(Act::Published, &format!("replied in {}: {gist}", w.job.app), true, t);
                    }
                    Ok(()) => out.push(format!("The reply is in the box in {}, not sent.", w.job.app)),
                    Err(why) => {
                        out.push(format!("I couldn't type into {}: {why}", w.job.app));
                        self.journal.record_at(Act::Blocked, &format!("couldn't reply in {}: {why}", w.job.app), false, t);
                        return out;
                    }
                }
                // What "nothing new" is measured against, taken once the app
                // has had a moment to show the reply, so Atlas doesn't
                // answer itself.
                self.plat.sleep_ms(2_000);
                w.after_mine = self.plat.read_window(w.win).ok().flatten().map(|t| t.text());
                w.looked = t;
            }
            if w.job.finished() {
                out.push(w.job.summary());
            } else {
                keep(self, w);
            }
            return out;
        }
        if t.saturating_sub(w.looked) < 10 {
            keep(self, w);
            return out;
        }
        w.looked = t;
        // Read in the background: UI Automation reads a window whether or
        // not it's in front. A window that's gone ends the job.
        let screen = match self.plat.read_window(w.win) {
            Ok(Some(tree)) => tree.text(),
            _ => {
                out.push(format!(
                    "I can't read {} any more — the window's closed or doesn't show its text — so I've stopped working it.",
                    w.job.app
                ));
                return out;
            }
        };
        // Only when something new has arrived since Atlas wrote — judged
        // after its own reply, with times and "seen" left out.
        if w.after_mine.is_some()
            && !crate::delegate::something_new(&screen, w.after_mine.as_deref(), w.job.transcript.last().map(|s| s.as_str()))
        {
            keep(self, w);
            return out;
        }
        let facts = crate::grants::AppFacts { known: true, confirm_each_time: false };
        match w.job.advance(&screen, &self.permissions, &facts) {
            crate::delegate::Step::Compose { context } => {
                let Some(llm) = self.llm.clone() else {
                    out.push(format!("There's no model to write with, so I've stopped working {}.", w.job.app));
                    return out;
                };
                let system = w.job.system_prompt();
                let quoted = crate::untrusted::Read::new(&w.job.app, &context, crate::store::now()).quoted();
                let work: crew::Work = Box::new(move |_c: &crew::Control| {
                    let counted = CountedLlm { inner: llm.as_ref(), calls: std::sync::Mutex::new(Vec::new()) };
                    let reply = counted.complete(&system, &quoted).map_err(|e| e.to_string());
                    // Read back before it goes out as you: a chatbot's
                    // phrases, stock openers, filler or a blank get one
                    // rewrite (`draft::for_replies`), kept only if it's
                    // actually better.
                    // How it turned out, graded without a model: the first
                    // draft is good if the read-back found nothing, bad by
                    // what it found; a rewrite is good if it was kept.
                    let mut first: Option<(bool, String)> = None;
                    let mut rewrites: Option<(bool, String)> = None;
                    let text = match &reply {
                        Ok(t) if !t.trim().is_empty() => {
                            let found = crate::draft::critique(t.trim(), None, &crate::draft::for_replies());
                            let why = found.iter().map(|n| n.fault.what()).collect::<Vec<_>>().join("; ");
                            match crate::draft::revise(t.trim(), &counted, &crate::draft::for_replies()) {
                                crate::draft::Outcome::Revised { text, .. } => {
                                    first = Some((false, why));
                                    rewrites = Some((true, String::new()));
                                    text
                                }
                                crate::draft::Outcome::KeptOriginal(text) => {
                                    first = Some((false, why));
                                    rewrites = Some((false, "rewrite was no better".into()));
                                    text
                                }
                                crate::draft::Outcome::AsWritten => {
                                    first = Some((true, String::new()));
                                    t.trim().to_string()
                                }
                            }
                        }
                        Ok(_) => String::new(),
                        Err(_) => String::new(),
                    };
                    let mut calls = counted.calls.into_inner().unwrap_or_default();
                    for (i, c) in calls.iter_mut().enumerate() {
                        c.grade = if i == 0 { first.clone() } else { rewrites.clone() };
                    }
                    let envelope = ReplyOutcome { text, failed: reply.err(), calls };
                    serde_json::to_string(&envelope).map_err(|e| e.to_string())
                });
                // The crew's id comes back from `hand`; remembered so the
                // reply finds its way to this window.
                if let Some(crew_id) =
                    self.hand_off_as("conversation-reply", t, work, Some(w.job.app.clone()), SpeakPolicy::ViaWatcher)
                {
                    w.composing = Some(crew_id);
                    w.composed_from = Some(screen);
                } else {
                    out.push(format!("I'm too busy to write for {} right now; I'll try again in a moment.", w.job.app));
                    w.looked = t.saturating_sub(5);
                }
            }
            crate::delegate::Step::Confirm(q) => {
                // Kept and asked, not dropped. The job used to be thrown away
                // here, so answering the question did nothing: there was no
                // job left to say yes to.
                let line = format!("{q} I've stopped working {} until you say.", w.job.app);
                self.session.ask(&line);
                self.pending_window_confirm = Some(w.id);
                w.held = true;
                keep(self, w);
                out.push(line);
                return out;
            }
            crate::delegate::Step::Finished(summary) => {
                out.push(summary);
                return out;
            }
            crate::delegate::Step::Observe | crate::delegate::Step::Place { .. } => {}
        }
        if w.job.finished() {
            out.push(w.job.summary());
        } else {
            keep(self, w);
        }
        out
    }

    /// A reply the crew wrote for a window job, back on the tick: kept for
    /// typing in the next gap, or — when the model had nothing to say —
    /// taken as "wait for them".
    pub(super) fn reply_written(&mut self, crew_id: u64, ending: &crew::Ending) -> Vec<String> {
        let mut out = Vec::new();
        let Some(i) = self.working_for_you.iter().position(|w| w.composing == Some(crew_id)) else {
            // Its job was called off while the model was writing.
            return out;
        };
        let outcome = match ending {
            crew::Ending::Done(Ok(json)) => serde_json::from_str::<ReplyOutcome>(json).ok(),
            _ => None,
        };
        if let Some(o) = &outcome {
            for c in &o.calls {
                let id = self.record_model_call("conversation-reply", c.took_ms, c.prompt_chars, c.reply_chars, c.failed.clone());
                if let Some((good, why)) = &c.grade {
                    self.grade_call(id, *good, why);
                    self.keep_words(id, c.words.as_ref());
                }
            }
        }
        let w = &mut self.working_for_you[i];
        w.composing = None;
        match outcome {
            Some(o) if o.failed.is_none() && !o.text.is_empty() => w.ready = Some(o.text),
            // Nothing to say: the model is waiting for them. Measured from
            // the screen it was shown, so it isn't asked about it again.
            Some(o) if o.failed.is_none() => w.after_mine = w.composed_from.take(),
            other => {
                let why = other.and_then(|o| o.failed).unwrap_or_else(|| "it stopped without an answer".into());
                let app = w.job.app.clone();
                self.working_for_you.remove(i);
                out.push(format!("The model couldn't write for {app} ({why}), so I've stopped working it."));
            }
        }
        out
    }

    /// Type into a window that may not be in front — `delegate::type_into_window`,
    /// the one path `atlas window type` also uses on the real laptop.
    fn type_into(&mut self, win: crate::platform::WindowId, text: &str, send: bool) -> std::result::Result<(), String> {
        crate::delegate::type_into_window(self.plat, win, text, send)
    }

    /// Window jobs, as `which_errand` sees errands.
    pub(super) fn window_job_candidates(&self) -> Vec<crate::which_errand::Candidate> {
        self.working_for_you
            .iter()
            .map(|w| crate::which_errand::Candidate {
                id: w.id,
                label: "conversation".into(),
                topic: Some(w.job.app.clone()),
                started: w.started,
                paused: w.held,
                can_hold: true,
            })
            .collect()
    }

    /// "What do you see?"
    pub(super) fn whats_there(&mut self) -> String {
        let cfg = self.tools_cfg().vision.clone();
        // You asked. The "Recognising things" switch is for looking on
        // Atlas's own initiative; asked, it looks the way "can you see me"
        // does -- asked once, said, frame deleted (30 Sep 2026: it answered
        // "seeing is switched off").
        if !cfg.enabled {
            return self.look_at_you();
        }
        let sight = self.see();
        if let Some(scene) = sight.scene() {
            // A look that half-worked says which half. Reporting only what was
            // found would turn "the object model failed" into "there is
            // nothing on your desk".
            if !scene.could_not.is_empty() {
                return format!("{} ({})", scene.spoken(&cfg), scene.could_not.join("; "));
            }
        }
        sight.spoken(&cfg)
    }

    /// "What's this?" — the one thing being held up or pointed at.
    ///
    /// This is where the hand work and the seeing work meet, and it is the
    /// question neither could answer alone. The hands knew where a finger was
    /// and had no idea what was under it; seeing knew what was in the picture
    /// and had no idea which of it was meant.
    pub(super) fn whats_this(&mut self) -> String {
        let cfg = self.tools_cfg().vision.clone();
        // Asked, so looked -- through the camera path (`look_at_you`), not
        // refused over a switch meant for looking unasked (30 Sep 2026).
        if !cfg.enabled {
            return self.look_at_you();
        }
        let (frame, w, h) = match self.one_frame() {
            Ok(f) => f,
            Err(why) => return format!("I couldn't look — {why}."),
        };
        let album = self.album.clone();
        let looking = self.start_looking();
        let sight = looking.look(&frame, w, h, &cfg, &album);
        // Out of the same frame, so the finger and the things it might be
        // over are measured against the same picture.
        let finger = looking.finger(&frame, w, h);
        let Some(scene) = sight.scene() else {
            return sight.spoken(&cfg);
        };
        if let Some((fx, fy)) = finger {
            if let Some(thing) = scene.at(fx, fy) {
                return format!("That's {}.", thing.spoken(cfg.sure_enough_to_say_plainly));
            }
        }
        // Nothing under the finger, or no finger. Something held up is in the
        // middle of the picture, and the answer is the most specific thing
        // there — the mug, not the person holding it. That is the same
        // question `at` already answers, asked about the middle of the frame
        // rather than about a fingertip.
        match scene.at(0.5, 0.5) {
            Some(thing) => format!("That's {}.", thing.spoken(cfg.sure_enough_to_say_plainly)),
            None => match &scene.whole {
                Some(w) => format!(
                    "I can't pick one thing out, but the picture looks like {}.",
                    w.spoken(cfg.sure_enough_to_say_plainly)
                ),
                None => "I'm looking, and I can't make out what you mean.".into(),
            },
        }
    }

    /// "This is my mug."
    ///
    /// A face if there is one in the picture, otherwise the picture itself.
    /// Not a choice the caller makes, because "this is my sister" and "this is
    /// my mug" are the same sentence and the difference is in what the camera
    /// can see.
    pub(super) fn name_this(&mut self, name: &str, t: u64) -> String {
        let cfg = self.tools_cfg().vision.clone();
        if !cfg.enabled {
            return "Seeing is switched off — turn on Recognising things in settings and \
                    I'll pick this up."
                .into();
        }
        let (frame, w, h) = match self.one_frame() {
            Ok(f) => f,
            Err(why) => return format!("I couldn't look — {why}."),
        };
        let album = self.album.clone();
        let cfg2 = cfg.clone();
        let looking = self.start_looking();
        let sight = looking.look(&frame, w, h, &cfg2, &album);
        let face = sight.scene().and_then(|s| s.faces.first().cloned());

        let reading = match &face {
            Some(f) => looking.face_reading(&frame, w, h, &f.area, cfg2.face_margin),
            None => looking.describe(&frame, w, h),
        };
        let reading = match reading {
            Ok(r) => r,
            Err(e) => return format!("I couldn't get a clear enough look to remember that: {e}"),
        };
        let put = match &face {
            Some(_) => self.album.remember_face(name, &reading, t),
            None => self.album.remember_thing(name, &reading, t),
        };
        if let Err(e) = put {
            return format!("{e}");
        }
        let _ = self.album.save(&self.store);
        let views = self
            .album
            .everything()
            .into_iter()
            .find(|l| l.to_lowercase().starts_with(&name.trim().to_lowercase()))
            .unwrap_or_default();
        // Says how many views it has, because one view is a system that
        // recognises you in one light and at one angle, and knowing that is
        // what makes someone show it again.
        format!("Got it — {}. Show me again from another angle and I'll be surer.", views)
    }

    pub(super) fn build_eyes(&self, models: &std::path::Path) -> Option<Box<dyn crate::handloop::Eyes>> {
        let tools = self.tools_cfg();
        // The one-shot capture command already knows which device and backend
        // this machine uses. Re-deriving that here would be two places to get
        // right and one of them silently rotting.
        let capture = tools.capture_webcam.as_ref()?;
        let feed = crate::frames::Feed {
            open_with: crate::frames::from_capture_args(&resolved(&capture.args, &tools.vars)),
            ..crate::frames::Feed::default()
        };
        match crate::handloop::Seeing::start(&feed, models) {
            Ok(seeing) => Some(Box::new(seeing)),
            Err(e) => {
                // Named once, here, rather than twenty times a second there.
                self.log.warn(&format!("couldn't start hand tracking: {e}"));
                None
            }
        }
    }

    /// Turn hand control on or off.
    ///
    /// The way in. Before this there wasn't one: `steering_until` was only set
    /// inside the steering handler, which only ran when already steering, so
    /// the mode could never be entered at all.
    ///
    /// Three ways to reach it, because the whole point is not having to talk:
    /// this command, the hub, and — with the wake shape switched on — holding
    /// a hand up without saying anything.
    pub fn watch_hands(&mut self, on: bool, t: u64) -> String {
        let cfg = self.tools_cfg().gaze.clone();
        if !on {
            // Stopped, and waited for. `Tracking::stop` joins rather than
            // detaching for exactly this reason.
            if let Some(mut running) = self.hands.take() {
                running.stop();
            }
            self.steering_until = None;
            self.carrying = None;
            self.pointing_at = None;
            let _ = self.plat.draw_overlay(&[]);
            return "Alright — I've stopped watching your hands.".into();
        }
        if self.hands.as_ref().is_some_and(|h| h.running()) {
            return "Already watching.".into();
        }
        if !cfg.enabled {
            // Named, not a shrug. "Nothing happened" is the answer that makes
            // a person give up on a feature.
            return "The camera's switched off — turn on Watching the room in \
                    settings and I'll pick this up."
                .into();
        }
        // There used to be a check here that `gaze_detector` was configured.
        // Hand tracking has never used it: it reads its own models through
        // `infer`, in this process. So the check refused to start a working
        // feature over an unrelated setting that Eric's offline rule means
        // nobody will ever fill in — and it refused in words that said the
        // opposite of the truth ("nothing here reads hands yet"). What is
        // actually needed is checked immediately below.
        // The models have to be there before a thread is worth starting.
        // Where whisper and piper already keep theirs. One folder, not a new one.
        let models = std::path::Path::new(&self.tools_cfg().models.dir).to_path_buf();
        let missing = crate::infer::whats_missing(&models, &crate::infer::Kind::for_hands());
        if !missing.is_empty() {
            return crate::infer::spoken(&missing);
        }
        let Some(pointer) = self.plat.pointer_handle() else {
            return "I can see your hands but I can't drive the pointer on this \
                    machine yet."
                .into();
        };
        let Some(eyes) = self.build_eyes(&models) else {
            return "I couldn't start the camera.".into();
        };

        let tools = self.tools_cfg();
        self.hands = Some(crate::handloop::start(crate::handloop::Setup {
            eyes,
            pointer,
            vocabulary: self.gestures.clone(),
            smoothing: tools.smoothing,
            pace: tools.pace,
        }));
        self.steering_until = Some(t + crate::gaze::STEERING_STOPS_AFTER);
        self.steering_hand = crate::gaze::Steering::default();

        // Said now, not discovered over a week of one gesture never firing.
        // Two shapes one hand can satisfy at once means whichever is bound
        // first shadows the other — `overlaps` has known how to find that
        // since it was written, and nothing at the moment of switching-on
        // ever asked it.
        let clashes = self.gestures.overlaps();
        let mut said = String::from("Watching your hands. Flat palm to stop.");
        if let Some((a, b)) = clashes.first() {
            said.push_str(&format!(
                " One thing: {a} and {b} are shapes one hand can make at \
                 once, so whichever I check first wins — change a finger on \
                 one of them if the other never seems to fire."
            ));
        }
        if self.gestures.any_held() {
            said.push_str(" Held shapes need a beat before they fire.");
        }
        said
    }

    /// Ring whatever is under the pointer.
    ///
    /// `overlay.rs` — "drawing on the desktop itself" — was written, tested,
    /// and called by nothing. This is what it was for: a hand pointing at
    /// something with no mark on screen is a hand you have to guess with.
    fn outline_under(&mut self, x: i32, y: i32, holding: bool) {
        if !self.tools_cfg().overlay.enabled {
            return;
        }
        let rect = self
            .plat
            .window_at(x, y)
            .ok()
            .flatten()
            .and_then(|id| self.plat.rect_of(id).ok());
        self.pointing_at = rect;
        if let Some(r) = rect {
            let element = crate::overlay::around((r.x, r.y, r.width, r.height), holding);
            let _ = self.plat.draw_overlay(&[element]);
        } else {
            // Nothing under the hand. Cleared rather than left pointing at
            // whatever it was last over, which would be a lie about where you
            // are.
            let _ = self.plat.draw_overlay(&[]);
        }
    }

    /// Move things about by hand.
    ///
    /// Everything on the monitors, not just Atlas's own panels. Eric asked for
    /// the whole desktop and asked not to have it narrowed on grounds of
    /// risk — so this drives the real platform: `place`, `focus`, `click`,
    /// `scroll`. The safety here is that every move can be taken back and
    /// every move is named in the day's account, not that some of them are
    /// refused.
    fn steer_displays(&mut self, hand: Option<crate::gaze::Hand>, t: u64) -> Option<String> {
        use crate::gaze::Move;
        let (w, h) = self
            .plat
            .monitors()
            .ok()
            .and_then(|m| m.into_iter().find(|m| m.primary).or_else(|| None))
            .map(|m| (m.width, m.height))
            .unwrap_or((1920, 1080));

        let mut state = self.steering_hand;
        let what = crate::gaze::what_happened(hand, &mut state, w, h);
        self.steering_hand = state;
        if hand.is_some() {
            self.steering_until = Some(t + crate::gaze::STEERING_STOPS_AFTER);
        }
        let what = what?;

        let said = match what {
            // The pointer follows the hand. Not recorded and not spoken —
            // moving your hand is not an event.
            Move::Point { x, y } => {
                let _ = self.plat.move_cursor(x, y);
                self.outline_under(x, y, false);
                return None;
            }
            Move::Grab { x, y } => {
                let _ = self.plat.move_cursor(x, y);
                self.carrying = self.plat.window_at(x, y).ok().flatten();
                self.carried_from = self.carrying.and_then(|id| self.plat.rect_of(id).ok());
                self.outline_under(x, y, true);
                None
            }
            Move::Drag { x, y } => {
                let _ = self.plat.move_cursor(x, y);
                if let (Some(id), Some(from)) = (self.carrying, self.carried_from) {
                    // Carry it by the point it was picked up, so it doesn't
                    // jump its own top-left corner under your hand.
                    let rect = crate::platform::PixelRect {
                        x: x - from.width / 2,
                        y: y - 20,
                        width: from.width,
                        height: from.height,
                    };
                    let _ = self.plat.place(id, rect);
                }
                return None;
            }
            Move::Drop { x, y } => {
                self.carrying = None;
                self.outline_under(x, y, false);
                None
            }
            Move::Tap { x, y } => {
                let _ = self.plat.move_cursor(x, y);
                let _ = self.plat.click(x, y, crate::platform::Button::Left);
                // Say what was selected, not just that something was. A click
                // with no idea what it hit is the version that cannot be
                // trusted for anything consequential later.
                self.note_what_was_clicked(x, y);
                None
            }
            Move::Resize { by } => {
                if let (Some(id), Some(from)) = (self.carrying, self.carried_from) {
                    let rect = crate::platform::PixelRect {
                        x: from.x,
                        y: from.y,
                        width: ((from.width as f32) * by) as i32,
                        height: ((from.height as f32) * by) as i32,
                    };
                    let _ = self.plat.place(id, rect);
                }
                None
            }
            Move::Summon => {
                // Bring Atlas up. What's outstanding is what you want when
                // you summon it without saying anything.
                let now = crate::store::now();
                let lines = self.outstanding_lines(now);
                self.show_panel(
                    crate::window::Panel::Outstanding,
                    "Outstanding",
                    if lines.is_empty() {
                        vec!["Nothing outstanding.".to_string()]
                    } else {
                        lines
                    },
                );
                None
            }
            Move::Done => {
                self.steering_until = None;
                self.carrying = None;
                // The ring goes with the mode. An outline left on screen after
                // you stopped steering is a desktop that looks stuck.
                self.pointing_at = None;
                Some("Done — I've stopped watching your hands.".to_string())
            }
        };

        // Every move that changed something is written down, so a gesture that
        // landed somewhere unintended can be found and put back rather than
        // appearing as an unexplained change to the desktop.
        if what.undoable() {
            self.log.info(&format!("by hand: {}", what.plain()));
        }
        said
    }

    /// Read the next thing you handed over.
    ///
    /// One per tick, oldest first. One rather than all of them because reading
    /// a page means running an external tool, and a tray with twenty links in
    /// it should not stall the loop that also answers you when you speak.
    ///
    /// **What comes back is text from outside.** It is stored to be shown and
    /// nothing else. It never reaches the parser, and there is no path from
    /// here to `execute` — a fetched page that says "delete everything" is a
    /// page that says that, not an instruction Atlas received.
    pub(super) fn read_one_handed_thing(&mut self) -> Option<String> {
        let item = self.tray.next_to_read()?.clone();
        let title = item.title();

        let found = match item.sort {
            crate::tray::Sort::Words => {
                // Already the thing itself. Nothing to fetch, and nothing to
                // interpret either.
                Ok(item.what.clone())
            }
            // A photo is almost always a picture of something written down —
            // a receipt, a whiteboard, a page of a contract, a parking sign.
            // Said what it looks like, so a receipt isn't filed as a page of
            // notes (H3, `files::after_scan`).
            crate::tray::Sort::Image => self.read_photo(&item).map(|text| {
                let what = crate::files::what_was_scanned(&text);
                let words = text.split_whitespace().count();
                format!("{text}\n\n{}", crate::files::after_scan(what, words))
            }),
            // The words are the point, so the sound is what gets read.
            // A video is watched when Atlas can watch; listened to when it
            // can't. Sound alone is still most of a recorded call, so falling
            // back to it beats refusing.
            crate::tray::Sort::Video if self.tools_cfg().viewing.enabled => {
                let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
                match self.watch(&item, &path) {
                    Ok(account) => Ok(account),
                    Err(why) => self.listen_to(&item).map_err(|also| {
                        format!("{why} I couldn't hear it either: {also}")
                    }),
                }
            }
            crate::tray::Sort::Video | crate::tray::Sort::Audio => self.listen_to(&item),
            // Read as code, not as prose. Summarising the first four
            // sentences of a source file gets you the import block, which is
            // the least interesting part of it by some distance.
            crate::tray::Sort::Code => {
                let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
                let name = std::path::Path::new(&path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                match std::fs::read_to_string(&path) {
                    Ok(code) => {
                        let tongue = crate::hollowcode::Tongue::of(&name, &code);
                        let mut found = crate::hollowcode::read(&code, tongue);
                        // Packages it imports that the project never lists,
                        // checked against the project's own manifest when
                        // there's one near the file.
                        let original = std::path::Path::new(&item.what);
                        let near = if original.exists() { original } else { std::path::Path::new(&path) };
                        if let Some(manifest) = crate::hollowcode::manifest_near(near) {
                            found.extend(crate::hollowcode::made_up_dependencies(&code, tongue, &manifest));
                            found.sort_by_key(|f| f.line);
                        }
                        let mut said = crate::hollowcode::spoken(&item.title(), tongue, &found);
                        // Only when it is worth saying. Telling him a Rust file
                        // is Rust and could be ported to Rust is noise.
                        if tongue != crate::hollowcode::Tongue::Rust
                            && tongue != crate::hollowcode::Tongue::Unknown
                        {
                            said.push_str("\n\n");
                            said.push_str(&crate::hollowcode::porting_notes(tongue, &code));
                        }
                        Ok(said)
                    }
                    Err(e) => Err(format!("I couldn't open that file: {e}")),
                }
            }
            crate::tray::Sort::Document | crate::tray::Sort::File if Self::read_as_document(&item) => {
                let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
                self.read_document_at(&path, false)
            }
            crate::tray::Sort::Document | crate::tray::Sort::File => {
                let path = item.stored_at.clone().unwrap_or_else(|| item.what.clone());
                match std::fs::read_to_string(&path) {
                    Ok(text) => Ok(crate::research::first_sentences(&text, 4)),
                    // Not every document is text. Saying which kind it is and
                    // that Atlas can't read it beats "couldn't open" — one of
                    // those tells you whether to bother trying again.
                    Err(_) => Err(format!(
                        "I've kept that {}, but I can't read inside this kind of \
                         file yet — I can open it for you.",
                        item.sort.title().to_lowercase()
                    )),
                }
            }
            crate::tray::Sort::Link => {
                let cfg = self.tools_cfg().research.clone();
                match cfg.fetch.as_ref() {
                    None => Err("I haven't got a way to fetch a page — turn on \
                                 web research in settings."
                        .to_string()),
                    Some(fetch) => {
                        let mut vars =
                            self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();
                        vars.insert("url".into(), item.what.clone());
                        match fetch.run(&vars, None) {
                            Ok(html) => {
                                let text = crate::research::page_text(&html);
                                if text.trim().len() < 80 {
                                    Err("I opened it and there was nothing readable in it."
                                        .to_string())
                                } else {
                                    Ok(crate::research::first_sentences(&text, 4))
                                }
                            }
                            Err(e) => Err(format!("I couldn't open that: {e}")),
                        }
                    }
                }
            }
        };

        match found {
            Ok(text) => {
                self.tray.read(item.id, &text);
                let _ = self.tray.save(&self.store);
                // What you asked for, repeated back with what was in it. The
                // question is yours, so it is safe to treat as a request --
                // unlike the contents, which came from outside.
                if let Some(asked) = item.asked.as_deref() {
                    return Some(format!(
                        "That {title} you sent me — you asked: {asked}. Here's what's \
                         in it: {text}"
                    ));
                }
                // Said once, when it is ready, and only if Atlas is allowed to
                // speak up at all. You handed it over on purpose, so hearing
                // back is the point -- but it goes through the same gate as
                // everything else rather than getting its own way in.
                Some(format!("That {} you sent me — {}", title, text))
            }
            Err(why) => {
                self.tray.stuck(item.id, &why);
                let _ = self.tray.save(&self.store);
                Some(format!("The {title} you sent me: {why}"))
            }
        }
    }

    /// Get something to the person, by whatever route currently works.
    ///
    /// Everything Atlas decides is worth saying while you may not be at the
    /// desk goes through here. Before this existed there were two
    /// destinations: the speakers, which reach an empty room, and the log,
    /// which reaches nobody. A disk filling up while you are out is exactly
    /// the case both get wrong.
    ///
    /// Returns what actually happened rather than a bool, so a caller cannot
    /// read "we tried" as "you were told".
    pub fn reach_you(&mut self, note: crate::notify::Note, t: u64) -> crate::notify::Sent {
        use crate::notify::{route, Route, Sent};
        let cfg = self.notify_cfg();
        // Presence is not sensed on most machines, so `Unknown` is the honest
        // default -- and `worth_speaking()` already treats Unknown as "go
        // ahead", which is the right call for a machine with no camera. What
        // decides the route then is whether you have been idle long enough to
        // have left.
        //
        // When the camera IS on, its reading wins. Until 18 Sep 2026 this
        // function ignored `self.eyes` entirely and guessed from idle time,
        // so a machine that had just seen a face it did not recognise still
        // reported `Unknown` here -- the sensor ran, and nothing it saw
        // reached the one place that decides what gets said out loud.
        let idle_guess = if self.quiet_for(t) > self.away_after {
            crate::presence::Presence::Away
        } else {
            crate::presence::Presence::Unknown
        };
        let seen = self.eyes.cfg.enabled && self.eyes.state != crate::presence::Presence::Unknown;
        let here = if seen { self.eyes.state } else { idle_guess };

        // Discretion is not a routing decision -- `notify::route` explains at
        // length why holding a note until the room empties is the wrong
        // answer, and that reasoning stands. What `discreet_with_strangers`
        // buys is the *other* lever the same comment names: how much of the
        // note appears. Someone else in the room turns every note into a
        // knock, which is what `Note::private` already means, so the setting
        // reaches the behaviour that was built for it rather than a second
        // mechanism beside it.
        //
        // Worth knowing before turning the camera on: `Sighting::as_look`
        // treats an identity it could not read as "not you", so a detector
        // that reports faces without recognising them makes every sighting a
        // stranger. That degrades notes to knocks; it never silences them.
        let note = if seen && crate::presence::keep_it_to_yourself(here, &self.eyes.cfg) {
            note.private()
        } else {
            note
        };
        let vars = self.tools_ref().map(|t| t.vars.clone()).unwrap_or_default();

        // Can this be said out loud, and through what?
        //
        // Headphones are what makes a busy room stop mattering: if they are
        // connected, speaking is private wherever you are. A call beats
        // everything. With neither, Atlas shows rather than says.
        let on_call = crate::notify::on_a_call(self.awareness.last_active_window());
        let say =
            crate::notify::how_to_say(self.audio_outputs(), on_call, self.modes.in_public());
        let can_speak = !matches!(say, crate::notify::Say::Silent(_)) && self.sound_allows_speaking();
        // The interrupt rule locked with Eric (Sound & voice → When Atlas may
        // pop up): what may not pop up now waits, held, in the hub and the brief.
        if !self.tools_cfg().sound.may_pop_up(note.urgency == crate::notify::Urgency::Urgent) {
            self.outbox.hold(note, &cfg);
            return Sent::Held;
        }

        // Gone long enough that you are probably not at the machine at all,
        // rather than merely away from the keyboard. Four times the away
        // threshold: long enough to have left the building, short enough that
        // something urgent still reaches you the same afternoon.
        let phone_cfg = self.phone_cfg();
        let long_gone = self.quiet_for(t) > self.away_after * 4;
        let can_phone = crate::phone::configured(&phone_cfg).is_ok();

        match route(here, can_speak, crate::notify::can_notify(&cfg), can_phone, long_gone) {
            Route::Phone => match crate::phone::send(&note, &phone_cfg) {
                Ok(()) => Sent::Notified,
                // A push that did not land must not vanish. Held, so it still
                // reaches you at the machine.
                Err(e) => {
                    self.outbox.hold(note, &cfg);
                    Sent::Failed(e.to_string())
                }
            },
            Route::Speak => {
                let through = match &say {
                    crate::notify::Say::Aloud(d) => format!(" (aloud, {d})"),
                    crate::notify::Say::InYourEar(d) => format!(" (privately, {d})"),
                    crate::notify::Say::Silent(_) => String::new(),
                };
                // `shown()` rather than the body, for the same reason the
                // screen uses it: a private note read out in full in a room
                // with someone else in it is the disclosure the knock exists
                // to avoid. The two routes disagreed on what private meant.
                let (title, body) = note.shown();
                self.log.info(&format!("{title}: {body}{through}"));
                Sent::Spoken
            }
            Route::Notify => match crate::notify::show(&note, &cfg, &vars) {
                Ok(()) => Sent::Notified,
                // A failed notification must not vanish. It is held instead,
                // so it still reaches you when you sit down.
                Err(e) => {
                    self.outbox.hold(note, &cfg);
                    Sent::Failed(e)
                }
            },
            Route::Hold => {
                self.outbox.hold(note, &cfg);
                Sent::Held
            }
        }
    }

    /// Ask Atlas its own standard questions and judge the answers.
    ///
    /// Runs through `execute`, so what is judged is what a person would
    /// actually have heard -- not a stub, not a mock. Every question in
    /// `SELF_QUESTIONS` is read-only by design, so this is safe to run
    /// unprompted.
    pub fn hollow_answers(&mut self) -> Vec<crate::hollow::Hollow> {
        let questions: Vec<String> =
            crate::hollow::SELF_QUESTIONS.iter().map(|q| q.to_string()).collect();
        let mut found = Vec::new();
        for q in questions {
            let intent = self.parser.parse(&q);
            let answer = self.execute(&intent);
            if let Some(h) = crate::hollow::judge(&q, &answer) {
                found.push(h);
            }
        }
        // The instruments as well as the answers. An answer can read perfectly
        // and still be built on a reading that never happened.
        if let Some(h) = crate::hollow::judge_readings(&self.readings()) {
            found.push(h);
        }
        found
    }

    /// The audio devices Atlas last actually saw.
    ///
    /// `None` until voice mode has probed them. Deliberately not probed on
    /// demand here: enumerating devices shells out to ffmpeg, and doing that
    /// on every notification would put a process launch in the path of every
    /// alert.
    fn audio_outputs(&self) -> Option<&[crate::audio::Device]> {
        self.audio_devices.as_deref()
    }

    /// Enumerate the audio devices once, and remember the answer.
    ///
    /// Called from the tick rather than from the notification path: listing
    /// devices launches ffmpeg, and putting a process launch in front of every
    /// alert would make alerting the slowest thing Atlas does.
    ///
    /// A listing that failed is left as "never looked" -- which `how_to_say`
    /// reads as "assume the system default works" -- and tried again in five
    /// minutes (29 Sep 2026: it was recorded as an empty list, "looked and
    /// found nothing", so one failed listing at sign-in, ffmpeg not fetched
    /// yet, silenced spoken notifications for the rest of the session).
    pub(super) fn refresh_audio_once(&mut self) {
        let now = crate::store::now();
        if self.audio_devices.is_some() || now < self.audio_devices_retry_at {
            return;
        }
        let ffmpeg = self.tools_ref()
            .and_then(|t| t.vars.get("ffmpeg").cloned())
            .unwrap_or_else(|| "ffmpeg".into());
        match crate::audio::probe_devices(&ffmpeg) {
            // With the speakers too, where the listing names none (Windows):
            // a notice then knows it is going into your headphones (29 Sep 2026).
            Ok(mut list) => {
                if !list.iter().any(|d| d.kind == crate::audio::Kind::Output) {
                    list.extend(crate::playout::output_devices());
                }
                self.audio_devices = Some(list);
            }
            Err(e) => {
                self.log.warn(&format!("couldn't list the sound devices (trying again in five minutes): {e}"));
                self.audio_devices_retry_at = now + 300;
            }
        }
    }

    /// Put one of Atlas's own panels on screen.
    ///
    /// Best-effort by design: a machine with no display, or a window that
    /// fails to start, must not change what Atlas says. The spoken answer is
    /// the answer; the panel is the same thing where you can keep looking at
    /// it. Deliberately *not* the hub, which stays a page you open yourself.
    /// What the native window draws for a panel Atlas decided to show (H2).
    ///
    /// Until 27 Sep 2026 `wants_panel` was set by "I'm here", "show me my
    /// tasks" and "what are you doing", and nothing ever drew it: the only
    /// reader faded it out. So the morning brief's waking mark never
    /// appeared on "I'm here" at all. This is the one place the decision
    /// becomes a window, with the same lines the voice just said.
    pub fn panel_contents(&mut self, panel: crate::panel::Panel) -> Option<(crate::window::Panel, String, Vec<String>)> {
        use crate::panel::Panel as P;
        use crate::window::Panel as W;
        match panel {
            P::Waking => {
                let mut items = self.brief_items();
                // The same order the voice says it in (`mind::speak_brief`).
                items.sort_by(|a, b| b.weight.cmp(&a.weight));
                let mut lines: Vec<String> = items.into_iter().map(|i| i.what).collect();
                if lines.is_empty() {
                    // The waking mark still arrives; it just has nothing under it.
                    lines.push("Nothing needs you right now.".into());
                }
                let greeting = self.deck(crate::store::now(), crate::localclock::offset_secs()).greeting;
                Some((W::Brief, greeting, lines))
            }
            P::Tasks => {
                let b = self.brief_now(crate::store::now());
                let lines: Vec<String> =
                    b.yours.iter().chain(b.drafted.iter()).map(|i| format!("{} — {}", i.source.plain(), i.headline())).collect();
                Some((W::Outstanding, "Outstanding".into(), lines))
            }
            P::Mind => {
                let w = self.mind.focus()?;
                let lines: Vec<String> = w.recent_thinking(6).iter().map(|th| th.text.clone()).collect();
                Some((W::Thinking, w.asked.clone(), lines))
            }
            // Settings and the hub open Atlas's own window; presence is a
            // sensor, not something to look at.
            P::Controls | P::Presence => None,
        }
    }

    /// Draw a panel Atlas decided on (`panel_contents`).
    pub(super) fn draw_panel(&mut self, panel: crate::panel::Panel) {
        if let Some((p, title, lines)) = self.panel_contents(panel) {
            self.show_panel(p, &title, lines);
        }
    }

    pub(super) fn show_panel(&mut self, panel: crate::window::Panel, title: &str, lines: Vec<String>) {
        if lines.is_empty() || !crate::window::can_open() {
            return;
        }
        let c = crate::window::Contents::new(panel, title, lines);
        // The panel is a second copy of Atlas's own binary, re-exec'd — the
        // `hidden_desktop` entry `LifecycleConfig::default` has reserved a
        // 120-second keep-warm for since the day it was written, with
        // nothing on the other end of it. Budgeted now, so a machine under
        // memory pressure stops drawing panels rather than swapping.
        //
        // No child handle: `window::open` detaches deliberately, so the
        // window survives a restart of the daemon. It is on the books and
        // reaped from them; it is not killed.
        let t = crate::store::now();
        let name = "hidden_desktop";
        let opened = self.helpers.want(name, crate::lifecycle::typical_mb(name), t, || {
            crate::window::open(&c).map(|()| None)
        });
        match opened {
            Ok(_) => self.helpers.done(name, t),
            Err(e) => {
                // Logged, never spoken. A failure to draw a window is Atlas's
                // problem, not a thing to interrupt you with.
                self.log.warn(&format!("couldn't open the {} panel: {e}", panel.heading()));
            }
        }
    }
}
