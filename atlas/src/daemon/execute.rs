//! Carrying out an understood intent: `execute`, its timing, and `execute_inner`,
//! the one `match` over every intent.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// `execute`, timed, for the typed prompt.
    ///
    /// A typed turn has no listening, hearing, speaking or playing — only the
    /// work. Those stages are left absent rather than written down as zero,
    /// which is the whole discipline of `timing::Turn`: a stage that did not
    /// run and a stage that took no time are different facts.
    ///
    /// Open question, deliberately not decided here: typed and spoken turns
    /// share one window, so `typical_ms()` is a median over two populations
    /// with genuinely different costs. `worst_stage()` is unaffected — it sums
    /// per stage, and a typed turn only ever adds to `Doing`.
    pub fn execute_timed(&mut self, intent: &Intent, said: &str) -> String {
        // A setting changed since the last thing you asked applies to this
        // one. The voice loop and the typed prompt don't tick, so this is
        // where they pick changes up.
        // A settings file that won't read is said with this answer: the
        // loop's tick says it too, but the typed console never ticks, and
        // dropping it here also marked it seen (30 Sep 2026: lost).
        let warned: Vec<String> = self.pick_up_settings().into_iter().filter(|l| l.starts_with("I couldn't read")).collect();
        // Also set here, not only in `turn_from`: the one-shot CLI path
        // (`atlas "..."`) parses and executes without going through a turn at
        // all, and a correction typed at the command line is still a
        // correction. Missed on the first pass, and the symptom was Atlas
        // asking "what did I get wrong?" about a sentence that had just said.
        self.last_said = said.to_string();
        let started = std::time::Instant::now();
        let out = self.execute(intent);
        let mut timed = crate::timing::Turn {
            about: about_short(said),
            ..Default::default()
        };
        timed.note(
            crate::timing::Stage::Doing,
            started.elapsed().as_millis().min(u32::MAX as u128) as u32,
        );
        self.timing.add(timed);
        if warned.is_empty() {
            out
        } else {
            format!("{} {out}", warned.join(" "))
        }
    }

    /// The last command carried out, by kind (`selftest`): where a
    /// sentence went, whether the phrases or the model chose it.
    pub fn last_reached(&self) -> Option<String> {
        self.last_executed.clone()
    }

    pub fn execute(&mut self, intent: &Intent) -> String {
        // The last line, and the one that catches the callers a gate placed
        // in `turn_from` alone never sees.
        //
        // The one that made this necessary rather than tidy: a parked
        // approval. Atlas asks "shut the workspace down, go ahead?", you hand
        // the laptop over, and the next person says "yes" -- and the answer
        // branch calls `execute` directly, several hundred lines above where
        // `turn_from` does its own check. Same for the work queue and for a
        // scheduled job coming due. Every one of them arrives here.
        //
        // `turn_from` keeps its check anyway: this one is about not *doing*
        // the thing, and that one is also about not asking you a question
        // about something that was never going to happen, and about not
        // answering out of your notes on the way past.
        if let Some(refusal) = self.handed_over_refusal(intent) {
            self.log.info(&format!("refused while handed over: {}", kind_of(intent)));
            return refusal;
        }

        // "Read this" means nothing until "this" has a referent. Resolving it
        // before acting is the difference between an assistant and a command
        // line — and when two candidates are equally likely it asks rather
        // than picking.
        if let Some(question) = self.resolve_subject(intent) {
            return question;
        }
        self.last_executed = Some(kind_of(intent).to_string());
        // Testing itself: what would touch real files, the network or the
        // install is said, not done (`selftest`).
        if self.rehearsal && crate::selftest::tier(kind_of(intent)) == crate::selftest::Tier::Rehearse {
            let plain = intent.plain();
            self.rehearsed.push(plain.clone());
            return format!("[rehearsed] would be {plain}");
        }
        // Counted against the ability that does it (`used`), so "never used"
        // on the Improvements page is something measured.
        let t = crate::store::now();
        if self.used.record(kind_of(intent), t) && t.saturating_sub(self.used_saved) >= 60 {
            self.used_saved = t;
            let _ = self.store.save(crate::used::KEY, &self.used);
        }
        let said = self.execute_inner(intent);
        // Say how sure it is, where being wrong would matter. Answers that
        // are just Atlas reporting its own state don't need it.
        let said = match intent {
            // `Why` reads back decisions Atlas recorded itself — the most
            // grounded thing it can say. It was scored as invention: these
            // answers are full of the word "your" ("your workspace", "your
            // files"), which cost 0.4 under the rule for claiming things
            // about your machine without looking. Atlas was hedging its own
            // written record.
            Intent::Why(_) => self.hedge(&said, crate::certainty::Grounding::from_what_it_holds()),
            // `Ask` and `Research` are assessed where their grounding is
            // actually known — in the arm that knows whether the notes
            // answered, and in the errand that knows how many sources the
            // note read. Assessing them here meant assessing a string with
            // no idea where it came from.
            //
            // For `Research` it was worse than uninformative: what this saw
            // was "Looking into {topic}. I'll let you know what I find." —
            // an acknowledgement, not an answer. It hedged the receipt and
            // never touched the finding.
            _ => said,
        };
        // Anything that changed something goes in the one history. Without
        // this, "what did you do" answers from an empty list and looks like
        // it works.
        // You did something. Atlas's own overnight work is recorded with
        // by_you false elsewhere and never shifts your day.
        let hour = crate::localclock::hour_here(self.now_acting()) as u32;
        self.rhythm.saw(hour, true);

        // How it turned out, per kind of work. Recorded here rather than at
        // the point of deciding, because whether Atlas was right is knowable
        // only after the fact — and a record written at decision time would be
        // a record of intentions.
        //
        // Provisionally good: an action that completed and was not taken back.
        // `Intent::Undo` below rewrites the last one, which is the only signal
        // available without asking you to grade everything.
        let kind = crate::earned::kind_of(intent);
        let went_wrong = said.starts_with("I couldn't")
            || said.starts_with("I can't")
            || said.contains("isn't built")
            || said.contains("switched off");
        self.earned.note(kind, !went_wrong, &said, clock());
        let _ = self.earned.save(&self.store);

        if let Some((what, area, undo)) = worth_recording(intent, &said) {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            self.history.note(&what, area, undo, true, now);
            let _ = self.store.save("undo_history", &self.history);
        }
        said
    }
    /// What the health check actually managed to read.
    ///
    /// A reading that came back empty is not a healthy machine, it is a
    /// question that went unanswered — and a summary built only from the
    /// readings that worked cannot tell you which it was.
    fn health_steps(&self, r: &Readings) -> Vec<crate::faithful::Step> {
        let mut steps = Vec::new();
        let mut note = |what: &str, ok: bool| {
            steps.push(if ok {
                crate::faithful::Step::did(what)
            } else {
                crate::faithful::Step::skipped(what, "couldn't read it")
            });
        };
        note("memory", r.ram_total_gb > 0.0);
        note("disk", r.disk_total_gb > 0.0);
        steps
    }


    fn execute_inner(&mut self, intent: &Intent) -> String {
        match intent {
            // Atlas's own updates and feedback about it, by voice (8.2, 8.14).
            Intent::Updates(what) => self.updates_said(what),
            Intent::Feedback(what) => self.feedback_said(what),
            Intent::PhoneModel(what) => self.phone_model_said(what),
            // Two-factor codes (Eric, B1): read out, or found in your email
            // or texts, typed where you asked.
            Intent::TypeCode(said) => self.type_code(said, crate::store::now()),
            // Turning two-factor on or off: read back, then your yes.
            Intent::TwoFactor(said) => self.two_factor(said),
            // The last build that ran out of tries, as a long job (E3).
            Intent::KeepAtIt => self.keep_at_it(crate::store::now()),
            Intent::RunBuild(what) => self.run_build(what),
            // Goals, for the nudges toward them (F4).
            Intent::Goals(said) => self.goals(said, crate::store::now()),
            // The list for later (F8).
            Intent::Later(said) => self.later_list(said, crate::store::now()),
            // Sorting your mailbox (G1).
            Intent::SortMail(said) => self.sort_mail(said, crate::store::now()),
            // When a post goes (G2).
            Intent::SchedulePost(said) => self.schedule_post(said, crate::store::now()),
            // A button in an app, by name (G4).
            Intent::PressButton(said) => self.press_button(said),
            // Big folders to another drive, findable afterwards (G5).
            Intent::MoveBigFiles(said) => self.move_big_files(said, crate::store::now()),
            Intent::PcTune(said) => self.tune_up(said),
            // The desktop's loose files filed, after a yes (29 Sep 2026).
            Intent::TidyDesktop => self.tidy_desktop(),
            // The microphone you named, kept to (29 Sep 2026).
            Intent::UseMic(kind) => self.use_microphone(kind),
            // A video edited on a copy; the original only after you say (G8).
            Intent::EditMedia(said) => self.edit_media(said, crate::store::now()),
            Intent::EditPhoto(said) => self.edit_photo(said),
            Intent::MakePicture(said) => self.make_picture(said),
            Intent::SelfTest => self.test_everything(),
            Intent::Operate(said) => self.start_operating(said),
            Intent::Clock => crate::localclock::spoken_now(crate::store::now(), crate::localclock::offset_secs()),
            Intent::SetKey(said) => self.set_key(said),
            Intent::Languages(_) => self.languages_heard(),
            Intent::TeachGesture(said) => self.teach_gesture(said, crate::store::now()),
            Intent::MoneyAdvice(said) => self.money_advice(said),
            Intent::CreatorAdvice(said) => self.creator_advice(said),
            Intent::Overnight => self.overnight_account(),
            Intent::Dangling => self.dangling_notes(),
            Intent::Suggestions(said) => self.suggestions(said),
            Intent::DropTask(said) => self.drop_or_bring_back(said, crate::store::now()),
            Intent::Unzip(said) => self.unzip_asked(said),
            Intent::ReadDocument(said) => self.read_document_asked(said),
            // The two halves of knowing the map. `RebuildIndex` is what the
            // drift nudge offers, so saying yes to that nudge lands here and
            // something actually happens.
            Intent::RebuildIndex => self.on_rebuild_index(),
            // The one caller `nudge::trace_line` was written for. Reading a
            // log Atlas keeps about itself is the cheapest thing it does, and
            // it is the only way to answer "is the local model good enough"
            // with a measurement rather than an opinion.
            Intent::ModelTrace => crate::nudge::trace_line(&self.trace),
            // The caller `council.rs` never had. Everything in that module
            // decided what to do with a list of opinions; nothing ever built
            // one, so `tally`, `Verdict` and `spoken` all computed over a list
            // only a test had filled.
            Intent::AskTheRoom(q) => self.ask_the_room(q),
            // The callers `revise.rs` never had.
            // The whole sentence, not the intent's argument: the parser
            // hands over what came *after* the phrase it matched, so the
            // argument is the fix and the complaint has been dropped. Both
            // halves matter — see `last_said`.
            Intent::GotItWrong(_) => self.on_got_it_wrong(),
            Intent::ApplyLesson => self.apply_lesson(),
            Intent::WhichModel => self.which_model(),
            Intent::HowAmIDoing => self.mending_line(),
            Intent::TimeSpent(what) => self.time_spent(what, clock()),
            Intent::ClipHistory(said) => self.wd_clip_history(said, clock()),
            Intent::ScreenText(said) => self.wd_screen_text(said, clock()),
            Intent::MarketDay(said) => self.wd_market_day(said, clock()),
            Intent::WaitingFor(said) => self.wd_waiting_for(said, clock()),
            Intent::NoteReview(said) => self.wd_note_review(said, clock()),
            Intent::Launch(said) => self.wd_launch(said, clock()),
            Intent::TradeDay(said) => self.wd_trade_day(said, clock()),
            Intent::MeetingPrep(said) => self.wd_meeting_prep(said, clock()),
            Intent::Snippet(said) => self.wd_snippet(said, clock()),
            Intent::FindFile(said) => self.wd_find_file(said, clock()),
            Intent::Pdf(said) => self.wd_pdf(said, clock()),
            Intent::People(said) => self.wd_people(said, clock()),
            Intent::Feeds(said) => self.wd_feeds(said, clock()),
            Intent::Opportunities(said) => crate::hunting::said(self, said, clock()),
            Intent::Wit(said) => crate::talkback::said(self, said),
            Intent::Social(said) => self.social_said(said, clock()),
            Intent::Receipt(said) => self.wd_receipt(said, clock()),
            Intent::Habit(said) => self.wd_habit(said, clock()),
            Intent::Cards(said) => self.wd_cards(said, clock()),
            Intent::Translate(said) => self.wd_translate(said, clock()),
            Intent::WhatIHave(q) if q.trim().is_empty() => {
                crate::nudge::what_i_know_of(&self.contents)
            }
            Intent::WhatIHave(q) => self.on_what_i_have(q),
            Intent::WorkspaceOn => report(workspace::workspace_on(self.cfg, self.plat), "Workspace online."),
            Intent::WorkspaceOff => report(workspace::workspace_off(self.cfg, self.plat), "Workspace down."),
            Intent::OpenApp(a) => self.on_open_app(intent, a),
            Intent::CloseApp(a) => match self.gate_app(a, "close", intent) {
                AppGate::Ask(q) => q,
                AppGate::Go => act(workspace::close_app(self.cfg, self.plat, a), &format!("Closed {a}.")),
            },
            Intent::FocusApp(a) => match self.gate_app(a, "focus", intent) {
                AppGate::Ask(q) => q,
                AppGate::Go => act(workspace::focus_app(self.cfg, self.plat, a), &format!("There's {a}.")),
            },
            // A question with a number in it. The shelf answers, or says it
            // can't — a model asked a specific question it doesn't know
            // answers anyway, so falling through quietly is the failure.
            //
            // This has to sit above the catch-all below, which matched Ask
            // and swallowed it.
            Intent::Ask(q) if crate::reference::needs_the_shelf(q) => self.on_ask_the_shelf(q),
            // Ask the notes before answering from the model. Atlas writing a
            // research note and then being unable to find it again is the
            // whole reason `recall` exists, and it sat unwired while
            // `Intent::Ask` echoed whatever the model said.
            Intent::Ask(q) => self.on_ask_plain(q),
            Intent::Say(s) => s.clone(),
            Intent::Research(topic) => self.research(topic),
            Intent::McpTool(p) => self.use_mcp_tool(p),
            Intent::ViewDisplay => self.look_closer(Capture::Screen),
            // Asked once, said as it happens, frame deleted (`daemon::camera`).
            Intent::CaptureWebcam => self.look_at_you(),
            Intent::Gestures(on) => self.watch_hands(*on, crate::store::now()),
            Intent::WhatsThere => self.whats_there(),
            Intent::WhatsThis => self.whats_this(),
            Intent::CallNotes(what) => self.call_notes_command(what),
            Intent::Delegate(said) => self.start_working_for_you(said, intent),
            // Said only because you asked. Atlas never announces it.
            Intent::AfterMe => self.on_after_me(),
            Intent::NameThis(name) => self.name_this(name, crate::store::now()),

            // A self-report against what this machine actually measured --
            // never Machine::default()'s fixed laptop numbers. See
            // wants.rs's own doc on why that distinction is the whole point.
            Intent::Recommend => self.on_recommend(),

            // "Call me boss" / "stop calling me that" / "just talk" --
            // `returning::address_change` does its own phrase detection on
            // the raw text, so the whole utterance is passed through
            // untouched rather than whatever the parser trimmed as "the
            // argument".
            Intent::AddressAs(said) => self.on_address_as(said),
            Intent::Dictate(first) => self.on_dictate(first),
            Intent::Pause => self.on_pause(),
            Intent::Resume => self.on_resume(),
            // What's outstanding, said the way it's actually useful: the one
            // thing to do next, and the one thing holding several others up.
            // A count is what every app already tells you.
            Intent::Outstanding if !self.workspace.is_empty() => self.on_outstanding_workspace(),
            // The backlog summary, with the morning pass in front of it. The
            // brief answers "what should I do", the summary answers "what is
            // outstanding", and asked together the first one goes first.
            Intent::Outstanding => self.on_outstanding_plain(),
            Intent::Queued => self.on_queued(),
            Intent::DraftPost(channel) => self.on_draft_post(channel),
            // Bare "undo" now goes through the one history first, so it can
            // take back a setting or a mail action and not only a file move.
            // The trash is the fallback, not the whole answer.
            // Taking something back is the clearest statement that it was
            // wrong, and the only one available without making you grade every
            // action. It rewrites the outcome recorded a moment ago rather
            // than adding a second one, or a single mistake would count twice.
            Intent::Undo => self.on_undo(),
            Intent::BackUp => self.on_back_up(),
            Intent::SetMode(name) => self.on_set_mode(name),
            Intent::SelfCheck => self.self_check(),
            Intent::Shakedown => self.shakedown(),
            Intent::MachineHealth => self.on_machine_health(),
            Intent::UseClipboard(what) => self.on_use_clipboard(what),
            Intent::Rehearse(command) => self.on_rehearse(command),
            Intent::Ready => self.on_ready(),
            Intent::Show(what) => self.on_show(what),
            Intent::Dismiss => self.on_dismiss(),
            // What works on whatever this is running on. Asked before
            // handing it to someone, and the honest answer differs enough by
            // platform to be worth giving properly.
            // The machine ladder, weighed rather than tried in order.
            //
            // This one was not wrong -- the `if/else` gave the right answer
            // for every sentence naming one machine. It is here because
            // `contains` is a substring search, so "can you do this on a
            // macbook" and "can you do this on macos" worked by accident
            // while "is there an iPad version" fell through to the final
            // `else` and was answered about iOS for the right reason by
            // luck rather than by matching. Weighing it puts the vocabulary
            // in one list the guard can check, beside the other two sets.
            Intent::Capabilities(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::WHICH_MACHINE,
                    &self.tools_cfg().whichone,
                )
                .settled()
                .is_some() => self.on_capabilities_settled(what),

            Intent::Capabilities(what) => self.on_capabilities_plain(what),
            // What did you do, and take it back. One list across everything,
            // because at the moment you ask you don't know which area it was.
            Intent::History(what) => self.on_history(what),


            // Why did you do that — with where the setting is.
            Intent::Why(about) => self.on_why(about),

            // Making an account is a commitment made as you, so it is
            // refused outright on anything financial before any config is
            // even read, and needs approval everywhere else.
            Intent::CreateAccount(where_) => self.on_create_account(where_),

            // Signing you in. The credentials come from the vault and the
            // domain has to match exactly — that check is what makes this
            // safer than typing it yourself.
            Intent::SignIn(where_) => self.on_sign_in(where_),

            // Read them, rather than count them. "3 unread" is the
            // count-shaped notification this codebase is written against: it
            // tells you something happened and makes you go and look.
            Intent::Messages => self.read_messages(),

            // The conversation itself, read back. `session::transcript` already
            // assembles the recent turns for the model; nothing until now read
            // them to you. Distinct from `History`, which is the log of what
            // Atlas *did* -- this is what was said. The current "recap" turn is
            // recorded after this returns, so it is never in its own answer.
            Intent::Recap => self.on_recap(),

            // The autonomy ledger, read back. `earned::may_act_alone` and
            // `earned::rope` decide this on every turn and were reached by
            // nothing a person could ask; this is the door to them.
            Intent::ActAlone => self.what_i_can_do_alone(),

            // The size of the knowledge store, read back. `consolidate::size_note`
            // states the whole module's promise in numbers -- how much is known
            // and roughly what it costs, and that it does not grow while you are
            // not asking -- and was reached only by its own test. This is the
            // question that finally asks it.
            Intent::KnowledgeSize => self.knowledge_store_size(),

            // "File that under groceries" / "that's actually a task". The
            // caller `capture::Notebook::correct` never had: capture only ever
            // *added* notes, so the filing guess it makes was never correctable
            // by anything a person could say. This is the door to it -- it
            // fixes the most recent note's kind or handle and marks it
            // confirmed, so the correction survives and `find` reaches it by
            // the handle you actually used.
            Intent::Refile(correction) => self.refile_note(correction),

            // The caller `knowhow::for_symptom` never had. `known_procedure`
            // above answers "how do I do X" from `for_request`; this starts
            // from what went wrong and scores the symptom against every
            // procedure's snags, so the one function that reads a symptom
            // finally has a question that asks it.
            Intent::Diagnose(symptom) => self.diagnose_symptom(symptom),

            // The producer `knowhow::as_plan` never had. `known_procedure`
            // (reached from the Unknown fallback) matches a task with
            // `for_request` but only ever says `announce` -- "I know this one,
            // 3 steps" -- and stops. This asks for the same match and reads
            // the steps out, so "I know how" is finally followed by "here's
            // how". Offline by construction; the procedures ship compiled in.
            Intent::WalkThrough(asked) => self.walk_me_through(asked),

            Intent::WhoIsIn(arg) => self.who_is_in(arg),

            Intent::NameGroup(arg) => self.rename_group(arg),

            Intent::LeaveGroup(arg) => self.leave_group(arg),
            Intent::ChangeGroup(said) => self.change_group(said),
            Intent::Friend(said) => self.friend(said),

            Intent::Message(raw) => self.send_message(raw),

            // Coming back to what setup skipped. `FirstRun::resume` has
            // existed since firstrun was written; this is the first thing
            // that ever called it.
            Intent::FinishSetup => self.on_finish_setup(),

            // Muting a topic, or unmuting it. The sentence decides which.
            Intent::MuteTopic(said) => self.mute_topic(said),

            // The face in front of the camera is yours. Goes through the same
            // `name_this` the album already uses, under the name config says
            // means you, so there is one path that writes a face and not two.
            Intent::ThisIsMe => self.on_this_is_me(),

            // "Hand over." Free to say, by anybody, with nothing checked --
            // which is the whole design and is why it took so long to notice
            // that no phrase reached it.
            Intent::HandOver(note) => self.on_hand_over(note),

            // "I'm back." The one spoken phrase that ends a handover, and it
            // ends nothing by itself -- it asks for the passphrase somewhere
            // it can be typed. See `take_it_back` below for the whole of it.
            Intent::TakeItBack => self.take_it_back(),

            // Opening the vault. Nothing that needs a secret works until this
            // has happened, and it re-locks itself.
            // Asked with nothing after it: how to, not "that's short enough to
            // be guessable" about an empty passphrase (the capability sweep).
            Intent::Unlock(phrase) if phrase.trim().is_empty() => {
                "Say \u{201c}unlock\u{201d} and your passphrase -- or open it on the Accounts page, where nothing is said out loud.".into()
            }
            Intent::Unlock(phrase) => self.on_unlock(phrase),

            // Generate an invite for someone by name. Off by default (the
            // door itself defaults off -- see kin.rs's module doc on why
            // "enabled with nobody registered" isn't a safe middle state),
            // and refuses cleanly rather than guessing when the two things a
            // spoken pairing can't ask you to say out loud -- your own name,
            // your own Tailscale address -- haven't been set once in config.
            Intent::Pair(their_name) => self.on_pair(their_name),

            // The other half of a pairing. Realistically typed or pasted --
            // see the doc comment on the intent itself -- but wired the same
            // way as every other intent rather than given a second path.
            Intent::AcceptPairing(code) => self.on_accept_pairing(code),

            // Undo a pairing, both directions. Reports rather than asks --
            // see policy::classify -- because removing standing access is
            // the safer direction to move in. Deliberately does NOT check
            // kin_cfg.enabled the way Pair/AcceptPairing do: revoking
            // should never be harder to reach than granting was, including
            // when you've switched the feature off specifically to clean up
            // stale pairings before turning it back on.
            Intent::ForgetPeer(name) => self.on_forget_peer(name),

            // Catch it now, file it afterwards. Asking where it goes at the
            // moment you have the thought is what loses the thought.
            Intent::Capture(text) => self.on_capture(text),

            // Everyday money. Trading has its own rules and its own
            // module; this is the other 95%.
            // Trading has rules of its own that catch people out. Asked
            // about tax or a statement, the trading knowledge comes first
            // because it's the part nobody has.
            // The three mail readings, weighed together rather than tried in
            // order. Until 19 Sep 2026 these were three `match` arms guarded
            // by `what.contains(..)`, so "how much did I spend on my trading
            // statement last month" -- a money question -- reached the tax arm
            // because the tax arm is written first. Nothing failed; the answer
            // was about the wrong thing.
            //
            // `whichone::weigh` scores every reading against the same sentence
            // independently, so the order these arms appear in no longer
            // decides anything, and a win too narrow to trust becomes a
            // question instead of a guess. The guard is one arm above.
            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .clarity
                    == crate::whichone::Clarity::Close => self.on_mail_which_one(what),

            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("rules") => self.on_mail_rules(),

            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("statements") => self.on_mail_statements(),

            // The inbox, sorted by what it asks of you.
            // Messages, sorted the same way the inbox is — by what they ask
            // of you. Group chatter never interrupts.
            Intent::Mail(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_MAIL,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("messages") => self.on_mail_messages(),

            Intent::Mail(what) => self.on_mail_plain(what),

            // Getting what happened here to your other devices.
            //
            // The four arguments to `mesh::choose` are
            // `same_network, mesh_up, cloud_ok, plugged_in`, and all four
            // were hardcoded literals rather than observations -- so the
            // answer was invariably "Sending it through the cloud folder --
            // it'll land shortly", `Path::SameNetwork`, `Path::Mesh` and
            // `Path::Cable` were unreachable, and nothing was transferred
            // either way. `path` was computed and discarded.
            //
            // There is no transfer in this tree yet. So this now says which
            // route it *would* take, from what Atlas can actually observe,
            // and says plainly that the sending half is not built -- which is
            // the difference between a plan and a lie.
            Intent::Sync(_) => self.carry_to_your_other_devices(clock()),

            // Asking one of your other Atlases how it is getting on.
            //
            // `kin.rs` is the door another Atlas knocks on, and its rule --
            // a signal becomes exactly one thing, a nudge, never a command --
            // is right and is untouched. But it runs one way, and the machine
            // that most needs looking in on is a server with nobody logged
            // into it.
            //
            // This is the other direction and it is read-only: your Atlas
            // asks, over a door that checks a token, for things that Atlas
            // already serves. What comes back is words you read. Nothing here
            // turns a reply into an intent, an action or an approval, and
            // `a_brief_is_words_not_instructions` fails the build if that
            // stops being true.
            Intent::BriefOn(who) => self.on_brief_on(who),

            // Before it goes out: what's in the frame, and whether it opens.
            // A brand asked something. The reply is drafted, never sent.
            // Colour, checked against the fixed tree rather than by taste.
            // Does the draft actually say anything? A piece with no position
            // reads as competent and is forgotten.
            // Same treatment as the mail arms above. "Grade this post -- does
            // it say anything?" carries a word for each reading, and taking
            // the first arm meant answering about the argument when the
            // sentence led with the colour.
            Intent::ReviewPost(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_A_POST,
                    &self.tools_cfg().whichone,
                )
                .clarity
                    == crate::whichone::Clarity::Close => self.on_review_post_which_one(what),

            Intent::ReviewPost(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_A_POST,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("stance") => self.on_review_post_stance(what),

            Intent::ReviewPost(what)
                if crate::whichone::weigh(
                    what,
                    crate::whichone::ABOUT_A_POST,
                    &self.tools_cfg().whichone,
                )
                .settled()
                    == Some("grading") => self.on_review_post_grading(),

            Intent::ReviewPost(what)
                if crate::editcraft::what_they_asked(what) != crate::editcraft::BrandAsks::Other => self.on_review_post_brand(what),

            Intent::ReviewPost(what) => self.on_review_post_plain(what),

            // What would lock you out if you left tomorrow.
            //
            // This passed `&[]`. `goingaway::spoken` opens with
            // `if stuck.is_empty() { return "You'd get into all of these from
            // anywhere. Nothing to do." }` -- so with no accounts that was
            // the *only* reachable answer, and the entire lock-out warning
            // below it was unreachable. You would ask "am I ready to travel?"
            // before a flight, be told every account was reachable from
            // anywhere, and land somewhere your SMS codes do not arrive.
            //
            // The real book was on `self` the whole time, and the hub already
            // used it correctly: `hublive.rs` does
            // `goingaway::plan(&self.accounts.accounts)`. Two answers to one
            // question, and the spoken one -- the one you would actually
            // rely on at an airport -- was the wrong one.
            Intent::TravelPrep => self.on_travel_prep(),

            // Reading, converting and joining whatever you point at.
            Intent::Files(what) => self.on_files(what),


            // Something Atlas doesn't have a command for. Before shrugging,
            // check whether it's a procedure it already knows — that was
            // written months ago and had never been consulted.
            // Atlas changing itself. It cannot start without a diagnosis
            // that holds up — a cause that isn't the symptom said again, and
            // a proving test that fails today.
            // What Atlas would change about itself, from what it can see
            // that you can't — which of its own routes keep failing, what you
            // keep correcting.
            // `!self.mid_self_work()` guards both of the arms above the main
            // one, and it is not a refinement.
            //
            // While a diagnosis is being collected, EVERY `WorkOnYourself`
            // turn is an answer to the question just asked. These two arms
            // fire on `what.contains("what")`, `"anything"`, `"yes"`,
            // `"do it"` and a blank turn — all of which are ordinary answers.
            // "every row re-reads whatever the config says" is a cause;
            // "yes, the left panel" is a location; a blank turn is someone
            // pressing enter. Without this, each of those was swallowed by a
            // self-audit or a grant reply, the question came back unchanged,
            // and the diagnosis could not be completed.
            Intent::WorkOnYourself(what)
                if !self.mid_self_work()
                    && (what.trim().is_empty()
                        || what.contains("what")
                        || what.contains("anything")) => self.on_work_on_yourself_what(),

            // "Yes, go on" — the grant makes the difference between a system
            // that recommends and one that fixes.
            Intent::WorkOnYourself(what)
                if !self.mid_self_work()
                    && (what.contains("go on")
                        || what.contains("go ahead")
                        || what.contains("do it")
                        || what.contains("yes")) => self.on_work_on_yourself_go_on(),

            Intent::WorkOnYourself(what) => self.work_on_myself(what),

            Intent::Build(what) => self.build_from_description(what),

            Intent::Improve(what) => self.improve_project(what),

            Intent::Implement(what) => self.implement_change(what),

            Intent::DesignReview(what) => self.design_review(what),

            Intent::Animate(what) => self.animate(what),
            Intent::Scene(what) => self.scene(what),

            Intent::Explain(what) => self.explain_code(what),
            Intent::PlainChange(_) => self.plain_change(),
            Intent::Booking(what) => self.booking(what),
            Intent::Learn(what) => self.learn_knowledge(what),

            Intent::Schedule(what) => self.schedule_event(what),

            Intent::Agenda(what) => self.read_agenda(what),

            // Before giving up, look at what Atlas has already written down.
            //
            // This is where recall earns its place. Most of the questions a
            // note answers — "what did I find out about X", "how much was the
            // quote for Y" — are not phrases the command parser knows, so
            // they arrive here as Unknown. Answering "I didn't catch that"
            // while holding a note on exactly that subject is the worst of
            // both: the search existed, the note existed, and neither was
            // consulted.
            Intent::Unknown(raw) => self.on_unknown(raw),
        }
    }

    // ---- the handlers execute_inner names, in arm order (refactor §4, 30 Sep 2026) ----

    fn on_rebuild_index(&mut self) -> String {
        let said = self.rebuild_index();
        // A rebuild settles the drift. Leaving it held would have the
        // nudge raise the same thing again an hour later, about a
        // folder that now matches.
        self.index_drifted = None;
        said
    }

    fn on_got_it_wrong(&mut self) -> String {
        let (said, now) = (self.last_said.clone(), crate::store::now());
        self.got_it_wrong(&said, now)
    }

    fn on_what_i_have(&mut self, q: &str) -> String {
        // The fact book first: what you actually told Atlas to remember
        // answers "what do you know about X" directly, rather than
        // pointing at a note to open. The index makes this fast however
        // much is remembered. Notes worth opening are the fallback.
        let now = crate::store::now();
        let facts = self.facts.recall_in_context(q, &self.context_terms(q), now);
        if !facts.is_empty() {
            let lines: Vec<String> = facts.iter().take(3).map(|f| f.summary.clone()).collect();
            let more = facts.len().saturating_sub(lines.len());
            let tail = if more > 0 { format!(" (and {more} more I've got on that)") } else { String::new() };
            let mut out = format!("{}{}", lines.join("; "), tail);
            // Associative recall: what else it knows that connects to the
            // answer — the neighbours by shared topic or explicit link —
            // so asking about one thing surfaces the things bound up with
            // it, not just the exact match.
            let shown: std::collections::BTreeSet<&str> =
                facts.iter().take(3).map(|f| f.name.as_str()).collect();
            let related: Vec<String> = self
                .facts
                .related(facts[0], now, 3)
                .into_iter()
                .filter(|f| !shown.contains(f.name.as_str()))
                .map(|f| f.summary.clone())
                .collect();
            if !related.is_empty() {
                out.push_str(&format!(" You also know: {}.", related.join("; ")));
            }
            out
        } else {
            let open = crate::contents::what_to_open(&self.contents, q);
            match open.len() {
                // A real answer, and a better one than opening everything
                // on the chance something matches.
                0 => format!("Nothing in my notes on {}.", q.trim()),
                _ => {
                    let named: Vec<String> = open.iter().take(3).cloned().collect();
                    let more = open.len().saturating_sub(named.len());
                    let tail = if more > 0 { format!(" (and {more} more)") } else { String::new() };
                    // Led with words rather than the name, because the
                    // phrasing layer capitalises the first letter of a
                    // reply and a note's name is a filename, not a
                    // sentence — "Spain-trip." is not what it is called.
                    format!("Worth opening: {}{}.", named.join(", "), tail)
                }
            }
        }
    }

    pub(super) fn on_open_app(&mut self, intent: &Intent, a: &String) -> String {
        match self.gate_app(a, "open", intent) {
        AppGate::Ask(q) => q,
        // Not one of your configured apps: found by name among the
        // Start menu's shortcuts instead (round 11, `launcher`) --
        // after the gate, so an unknown app is still asked about.
        AppGate::Go if !self.cfg.apps.apps.contains_key(a.as_str()) => self.wd_launch(&format!("start up {a}"), clock()),
        AppGate::Go => act(workspace::open_app(self.cfg, self.plat, a), &format!("{a} is up.")),
    }
    }

    fn on_ask_the_shelf(&mut self, q: &str) -> String {
        let cfg = self.tools_cfg().reference.clone();
        // Your `reference.shelves`, which this ignored until 18 Sep
        // 2026 -- see `reference::chosen` for what empty means and
        // why.
        let shelves = crate::reference::chosen(&cfg);
        // `warn_when_stale` ships on and nothing read it. It matters
        // more than it looks: this sentence is what Atlas says
        // instead of inventing a number, and it was claiming to hold
        // shelves nothing has ever fetched.
        crate::reference::nothing_found(
            q,
            &shelves,
            cfg.warn_when_stale,
            crate::store::now(),
        )
    }

    fn on_ask_plain(&mut self, q: &str) -> String {
        match self.answer_from_notes(q, crate::store::now()) {
        // Came out of the library, so it is grounded and says so.
        Some(answer) => {
            self.hedge(&answer, crate::certainty::Grounding::from_what_it_holds())
        }
        // Nothing matched. What goes back is the question itself, for
        // the layer above to carry on with — hedging that produced
        // "What's the capital of France? I'm not certain", which is
        // Atlas casting doubt on your own sentence.
        None => q.to_owned(),
    }
    }

    fn on_after_me(&mut self) -> String {
        // Eric, 25 Sep 2026: "no one else can ask Atlas." A handover
        // refuses it before it gets here, a guest profile can't reach
        // it (`profiles::ONLY_YOU_MAY_ASK`), and a voice that isn't
        // yours — or might not be — is told no here.
        match self.last_verdict {
            crate::voiceid::Verdict::NotYou(_) => {
                return "That's only for the person this Atlas belongs to.".into()
            }
            crate::voiceid::Verdict::Unsure(_) => {
                return "I couldn't be sure that was your voice, and that one's only for you — \
                        ask me again, or type it."
                    .into()
            }
            _ => {}
        }
        let arrangement: crate::afterme::Arrangement = self.store.load(crate::afterme::RECORD);
        arrangement.spoken(&self.tools_cfg().after_me, crate::store::now()).trim().to_string()
    }

    fn on_recommend(&mut self) -> String {
        let mut obs = crate::wants::Observations::default();
        let now = crate::store::now();
        for (stage, name) in [
            (crate::timing::Stage::Hearing, "transcribe"),
            (crate::timing::Stage::Understanding, "think"),
            (crate::timing::Stage::Speaking, "speak"),
        ] {
            if let Some(ms) = self.timing.typical_for(stage) {
                obs.time(name, ms as u64, now);
            }
        }
        let models = std::path::Path::new(&self.tools_cfg().models.dir).to_path_buf();
        for (kind, _path) in crate::infer::whats_missing(&models, &crate::infer::Kind::all()) {
            obs.missing.push(kind.file().to_string());
        }
        // Things you asked for that Atlas had no way to do, carried
        // over from every turn that landed as an unanswerable
        // `Intent::Unknown`. `recommend()` turns each one into a
        // concrete suggestion ("you asked me to X and I couldn't"),
        // which is the whole reason the field exists. `failures` still
        // has no source and stays empty rather than being guessed at:
        // `recommend()` degrades to no recommendation from an empty
        // signal, which is the honest behaviour, not a wrong one.
        obs.unsupported_requests = self.wants_seen.unsupported_requests.clone();
        let machine = crate::wants::machine_from(&crate::fit::measure());
        let recs = crate::wants::recommend(&obs, &machine);
        let advice = crate::wants::ask(&recs);
        // Lead with the measured bottleneck. `recommend`/`ask` answer
        // "what would help", but fall silent when nothing here is worth
        // changing -- and "what could make you faster" still has an
        // honest answer then: the stage that is in fact slowest right
        // now, named from the same timings this self-report is built on.
        // Phrased the way `recommend` phrases a stage time, so the two
        // lines read as one measurement rather than two conventions.
        match obs.slowest() {
            Some((stage, ms)) => format!(
                "Right now the slowest part of a turn is {stage}, about {:.1} seconds. {advice}",
                ms as f32 / 1000.0
            ),
            None => advice,
        }
    }

    fn on_address_as(&mut self, said: &str) -> String {
        match crate::returning::address_change(said) {
        Some(new_address) => {
            let reply = crate::returning::confirm_address(&new_address);
            if let Err(e) = new_address.save(&self.store) {
                return format!("{reply} (though I couldn't save that: {e})");
            }
            // The same fact lands in memory's preference store,
            // which is what `called()` reads. `memory.prefer` had
            // no production writer, so `preference("called")` could
            // only ever return None and the parked-question flow
            // addressed you as nobody — a third notion of your name
            // beside `ReturnConfig` and `Persona`, and the only one
            // nothing set. Cleared means empty, not left stale.
            match &new_address {
                crate::returning::Address::None => self.memory.prefer("called", ""),
                crate::returning::Address::Name(n)
                | crate::returning::Address::Title(n) => {
                    self.memory.prefer("called", n)
                }
            }
            let _ = self.memory.save(&self.store);
            reply
        }
        None => "I didn't catch a name or title in that.".into(),
    }
    }

    fn on_dictate(&mut self, first: &str) -> String {
        // `self.last_present` is this turn's `t`, set at the top of
        // `turn_from`. Reaching for `store::now()` here instead —
        // which is what every other arm does, because none of them
        // keeps a clock — put `last_spoke` on a different clock from
        // the one `tick` hands `idle_check`, so dictation could never
        // time out. Caught by the idle test, not by reading.
        let first = first.to_owned();
        let t = self.last_present;
        self.start_dictating(&first, t)
    }

    fn on_pause(&mut self) -> String {
        let running: Vec<u64> = self
            .queue
            .tasks
            .iter()
            .filter(|x| x.state == crate::lanes::TaskState::Running)
            .map(|x| x.id)
            .collect();
        for id in running {
            self.attention.suspend(id);
        }
        self.drop_pending_turn(crate::store::now(), "Paused before I answered -- nothing was done.");
        self.hold_task_loop(true);
        self.attention.pause(self.current_work(), crate::store::now())
    }

    fn on_resume(&mut self) -> String {
        // "I'm back" is the same four words in two different
        // situations, and the situation tells them apart.
        //
        // Paused, it means carry on -- and that is handled above
        // this, in `hear`, so it works mid-task. Nothing paused and
        // the machine handed over, it means the owner is back at the
        // desk, so it summons the passphrase prompt. The phrase is
        // not in `take_it_back`'s own list precisely because it is
        // already `resume`'s, and two commands claiming one phrase
        // make one of them unreachable.
        //
        // The remaining ambiguity is a guest who paused Atlas and
        // then said "carry on" -- they get a prompt they cannot
        // answer, and nothing happens. That is the harmless side of
        // the mistake; the other ordering would leave the owner
        // saying "I'm back" to an assistant that answers "Wasn't
        // paused." and keeps holding their things.
        if !self.attention.is_paused()
            && self.handover().stance.handed_over()
        {
            self.take_it_back()
        } else {
            let msg = self.attention.resume(crate::store::now());
            let held = self.attention.release();
            let names: Vec<String> = held
                .iter()
                .filter_map(|id| {
                    self.queue
                        .tasks
                        .iter()
                        .find(|x| x.id == *id)
                        .map(|x| x.command.clone())
                })
                .collect();
            if names.is_empty() {
                msg
            } else {
                format!("{msg} Still in hand: {}.", names.join(", "))
            }
        }
    }

    fn on_outstanding_workspace(&mut self) -> String {
        let now = clock();
        // Anything that stopped overnight is settled first and stated
        // in one line each, so the brief that follows is about today.
        let mut settled = self.settle_interrupted().join(" ");
        if !settled.is_empty() {
            settled.push(' ');
        }
        let mut cfg = self.tools_cfg().daily.clone();
        // The hour your day turns over, as learned from your hours
        // rather than fixed (F2).
        cfg.rolls_at_hour = self.rhythm.rolls_at(&cfg);
        let mut said = settled;

        // If the day has turned since you last looked, that comes
        // first — the list you're about to see is a new one.
        if crate::daily::has_rolled(self.last_seen, now, &cfg) {
            let day = crate::daily::day_of(self.last_seen, &cfg);
            let (closed, carried) =
                crate::daily::close(&self.workspace, day, &self.carried, &cfg);
            said.push_str(&crate::daily::opening(&closed, &carried, &cfg));
            said.push(' ');
            self.carried = carried.into_iter().map(|(t, c)| (t, c.0)).collect();
            self.last_seen = now;
            // A day watched, so the hour your day ends can be learned
            // (`Rhythm::quiet_hour` needs days to go on) — and kept,
            // since it was forgotten at every restart.
            self.rhythm.note_day();
            let _ = self.store.save("rhythm", &self.rhythm);
            // "You're usually done by …" — when it's first worked out,
            // and again only when your hours have really moved (F2).
            if let (Some(h), Some(line)) = (self.rhythm.quiet_hour(), self.rhythm.noticed()) {
                let last: Option<(u32, u64)> = self.store.load("rhythm_said");
                if crate::daily::worth_saying_again(h, last, now) {
                    said.push_str(&line);
                    said.push(' ');
                    let _ = self.store.save("rhythm_said", &Some((h, now)));
                }
            }
            // Archived rather than discarded, so there is finally
            // something for `daily::still_keep` to prune -- see the
            // hourly sweep below.
            self.daily_history.retain(|c| c.day != closed.day);
            self.daily_history.push(closed);
            let _ = self.store.save("daily_history", &self.daily_history);
        }

        said.push_str(&crate::workspace_view::spoken(&self.workspace, now));

        // Anything with someone else's time in it, waiting on you.
        let stale = crate::booking::going_stale(&self.proposals, now);
        if let Some(p) = stale.first() {
            said.push(' ');
            said.push_str(&crate::booking::stale_nudge(p));
        }
        said
    }

    fn on_outstanding_plain(&mut self) -> String {
        let b = self.brief_now(crate::store::now());
        let head = crate::brief::spoken(&b);
        let tail = self.backlog.summary();
        // On screen as well as spoken. A list read aloud is gone the
        // moment it finishes; this is the one you can look back at.
        //
        // The brief's items rather than the backlog's, so the panel
        // shows the handoff at the door and the job that failed
        // overnight alongside what Atlas could not finish — the
        // backlog is one source of several now, and a panel that
        // showed only it would disagree with the line just spoken.
        let lines: Vec<String> = b
            .yours
            .iter()
            .chain(b.drafted.iter())
            .map(|i| format!("{} — {}", i.source.plain(), i.headline()))
            .collect();
        self.show_panel(crate::window::Panel::Outstanding, "Outstanding", lines);
        if b.is_empty() { tail } else { format!("{head} {tail}") }
    }

    pub(crate) fn on_queued(&mut self) -> String {
        // Used to answer only about posts waiting to be sent, while
        // `crew::queued`, `crew::in_hand` and `crew::why_waiting` --
        // written for exactly this question -- had no caller at all.
        // Asked "what's queued", you mean everything Atlas is holding,
        // not one kind of it.
        let posts = self.publisher.summary(crate::store::now());
        // The lane queue's parked sets, by the readers written for
        // them. Work held for a connection or for a gap in your day
        // was invisible in this answer before.
        let mut parked: Vec<String> = Vec::new();
        let offline: Vec<&str> =
            self.queue.waiting_for_network().iter().map(|w| w.command.as_str()).collect();
        if !offline.is_empty() {
            parked.push(format!(
                "waiting for a connection: {}",
                offline.join(", ")
            ));
        }
        let gapped: Vec<&str> =
            self.queue.waiting_for_gap().iter().map(|w| w.command.as_str()).collect();
        if !gapped.is_empty() {
            parked.push(format!("waiting for a quiet moment: {}", gapped.join(", ")));
        }
        // Waiting changes whose code has moved on under them.
        for (project, title, files) in self.workshop.outdated() {
            parked.push(format!(
                "the \"{title}\" change for {project} is out of date ({} changed since)",
                files.join(", ")
            ));
        }
        // Windows being worked for you, and where each stands.
        let windows: Vec<String> = self
            .working_for_you
            .iter()
            .map(|w| {
                let state = if w.held {
                    "paused, nothing lost"
                } else if w.waiting_for_gap {
                    "has a reply to type, waiting for you to stop typing"
                } else {
                    "watching for something new"
                };
                format!("the conversation in {} is {state}", w.job.app)
            })
            .collect();
        if !windows.is_empty() {
            parked.push(windows.join("; "));
        }
        // Said after everything above is gathered. It was built
        // before the windows were added, so "what's queued" never
        // mentioned a window being worked.
        let posts = if parked.is_empty() {
            posts
        } else {
            format!("{posts} Also {}.", parked.join("; "))
        };
        // Errands on hold are named with where they stand: holding
        // at a safe point, or still working its way to one.
        let on_hold: Vec<String> = self
            .crew
            .errands()
            .into_iter()
            .filter_map(|e| {
                let c = self.errand_candidates().into_iter().find(|c| c.id == e.id)?;
                let name = crate::which_errand::describe(&c);
                match e.state {
                    crew::State::Holding => Some(format!("{name} is paused, holding with nothing lost")),
                    crew::State::Pausing => Some(format!("{name} is pausing — it holds at its next safe point")),
                    crew::State::WaitingPaused => Some(format!("{name} is paused before it started")),
                    _ => None,
                }
            })
            .collect();
        let posts = if on_hold.is_empty() {
            posts
        } else {
            format!("{posts} {}. Say 'carry on with …' to pick one back up.", on_hold.join("; "))
        };
        let waiting = self.crew.queued();
        if waiting == 0 {
            posts
        } else {
            // Named, not just counted. "Two errands waiting" is a
            // number; "research is behind one other" is an answer.
            let mut lines: Vec<String> = Vec::new();
            let links: Vec<(u64, &'static str)> = self
                .crew_links
                .iter()
                .map(|(id, l)| (*id, l.label))
                .collect();
            for (id, label) in links {
                if !self.crew.in_hand(id) {
                    continue;
                }
                match self.crew.why_waiting(id) {
                    Some(why) => lines.push(format!("{label} is {why}")),
                    None => lines.push(format!("{label} is running")),
                }
            }
            lines.sort();
            if lines.is_empty() {
                format!(
                    "{posts} And {waiting} errand{} waiting.",
                    if waiting == 1 { "" } else { "s" }
                )
            } else {
                format!("{posts} {}.", lines.join(", "))
            }
        }
    }

    fn on_draft_post(&mut self, channel: &str) -> String {
        // "linkedin about finishing a project": the channel, and what it's
        // about when that was said too.
        let (channel, about) = match channel.split_once(" about ") {
            Some((c, a)) if !a.trim().is_empty() => (c.trim().to_string(), Some(a.trim().to_string())),
            _ => (channel.trim().to_string(), None),
        };
        let ch = channel_named(&channel);
        let Some(about) = about else {
            let id = self.publisher.draft(ch, "");
            return format!("Drafting for {channel}. What should it say? (#{id})");
        };
        // Written by the model when there is one; never posted -- a draft
        // waits for you (`publish`).
        let written = self.llm.clone().and_then(|llm| {
            let prompt = format!("Channel: {channel}\nWhat it's about: {about}");
            llm.complete(DRAFT_SYSTEM, &prompt).ok().map(|t| t.trim().trim_matches('"').trim().to_string()).filter(|t| !t.is_empty())
        });
        match written {
            Some(text) => {
                let id = self.publisher.draft(ch, &text);
                let _ = self.publisher.save(&self.store);
                format!("A draft for {channel} (#{id}), not posted:\n\n{text}\n\nIt waits in your drafts; nothing goes out until you say so.")
            }
            None => {
                let id = self.publisher.draft(ch, "");
                format!("Drafting for {channel} about {about}. What should it say? (#{id})")
            }
        }
    }

    fn on_undo(&mut self) -> String {
        self.earned.taken_back();
        let _ = self.earned.save(&self.store);
        // "Undo" means the last thing that CAN be taken back.
        // `Undo::possible` existed to ask exactly that and nothing
        // asked it: when the newest action was irreversible, the
        // answer was its refusal and nothing else — even with a
        // perfectly reversible action sitting right behind it.
        let newest = self.history.last();
        if let Some(n) = newest {
            if !n.undo.possible() {
                let older = self
                    .history
                    .done
                    .iter()
                    .rev()
                    .find(|d| !d.undone && d.undo.possible())
                    .cloned();
                if let Some(o) = older {
                    let refusal = crate::undo::say(&crate::undo::reverse(Some(n)));
                    let offer = crate::undo::say(&crate::undo::reverse(Some(&o)));
                    return format!("{refusal} Before that: {offer}");
                }
            }
        }
        match crate::undo::reverse(self.history.last()) {
        // Asked first, then actually done (Eric, G6): this used to
        // stop at the question.
        crate::undo::Reversal::CanDo { id, confirm, .. } => {
            self.session.ask(&confirm);
            self.pending_undo = Some(id);
            confirm
        }
        crate::undo::Reversal::Nothing => match self.trash.undo_last() {
        Ok(d) => {
            let name = std::path::Path::new(&d.original)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or(d.original);
            format!("Put {name} back.")
        }
            // The reason in words, not "platform: there's nothing
            // to undo" (the capability sweep, 30 Sep 2026).
            Err(crate::error::AtlasError::Platform(why)) => {
                let mut c = why.chars();
                match c.next() {
                    Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str().trim_end_matches('.')),
                    None => "There's nothing to undo.".into(),
                }
            }
            Err(e) => format!("I couldn't undo that: {e}."),
        },
        other => crate::undo::say(&other),
        }
    }

    fn on_back_up(&mut self) -> String {
        let cfg = self.backup_cfg();
        match back_up(self.store.root(), &cfg, crate::store::now()) {
            Ok(b) => {
                prune_backups(&cfg);
                // back_up() just wrote every file itself and counted as it went, so
                // this is never the unreadable case -- unlike `Backup`s read back off
                // disk by `backups()`, which is where files: None matters.
                format!("Backed up {} files.", b.files.unwrap_or(0))
            }
            Err(e) => format!("Backup failed: {e}"),
        }
    }

    fn on_set_mode(&mut self, name: &str) -> String {
        let n = name.trim().trim_end_matches(" mode").trim();
        // Leaving a mode. `enter` was wired and `leave` was not, so a
        // mode could be turned on and never cleanly turned off -- and a
        // mode you cannot get out of is a mode you stop using, which is
        // exactly what `modes::leave`'s own doc warns about. "mode off",
        // "go into normal mode", "mode normal" now restore what was open
        // before, drop the mode's rules and clear the active mode. A
        // leaving word only counts as leaving when a mode is actually
        // on; otherwise it falls through to `enter`, so someone who
        // genuinely built a mode named "normal" can still turn it on.
        let leaving = matches!(
            n.to_lowercase().as_str(),
            "off" | "leave" | "exit" | "normal" | "none" | "nothing"
                | "standard" | "default" | "out"
        );
        if leaving && self.modes.active().is_some() {
            return match self.modes.leave() {
                Some(t) => {
                    let _ = self.modes.save(&self.store);
                    t.say
                }
                None => "You're not in a mode.".into(),
            };
        }
        match self.modes.enter(n, &[]) {
            Some(t) => t.say,
            None if leaving => "You're not in a mode.".into(),
            None => {
                let have: Vec<&str> = self.modes.modes.iter().map(|m| m.name.as_str()).collect();
                if have.is_empty() {
                    format!("I don't have a {n} mode. You haven't made any yet: a mode is kept in modes.json in my data folder -- which apps open, which close, and what's said.")
                } else {
                    format!("I don't have a {n} mode. The ones you have: {}.", have.join(", "))
                }
            }
        }
    }

    fn on_machine_health(&mut self) -> String {
        // Where Windows starts things from is read with reg.exe, a program
        // per key, a second or two in all: read now, beside the two
        // seconds the CPU is sampled for below, instead of after them.
        let startup = std::thread::spawn(|| crate::tune::startup_entries_kept(false));
        let r = self.readings();
        let f = assess_machine(&r, &self.health_cfg());
        let watched = self.watcher.summary();
        let mut s = health_summary(&r, &f);
        // What's using the machine now, and separately what you could
        // change so it stops. The second half was written months ago
        // and had never been reachable.
        // What's measured is passed in. This was an empty survey, so
        // this half could never say anything. Memory by app, startup
        // items, disposable folders and other drives aren't measured
        // yet, and are left empty rather than guessed.
        let models_mb = std::fs::read_dir(self.store.install_root().join("models"))
            .map(|rd| rd.flatten().filter_map(|e| e.metadata().ok()).map(|m| m.len()).sum::<u64>() / 1_000_000)
            .unwrap_or(0);
        // Measured now (1 Oct 2026): memory by program, and the temporary
        // folder. "Used today" is what the work log saw in front of you
        // since this morning -- never assumed, so nothing you use is called
        // forgotten.
        let now = crate::store::now();
        let since = crate::localclock::midnight(now, crate::localclock::offset_secs());
        let used: Vec<String> = self.worklog.between(since, now).iter().map(|sp| sp.app.to_lowercase()).collect();
        let used_today = |app: &str| {
            let a = app.to_lowercase();
            used.iter().any(|u| u.contains(&a) || a.contains(u.as_str()))
        };
        let memory_by_app: Vec<(String, u64, bool)> = crate::tune::memory_by_app()
            .into_iter()
            .take(15)
            .filter(|(app, _)| !app.to_lowercase().starts_with("atlas") && !app.to_lowercase().starts_with("llama-server"))
            .map(|(app, mb)| {
                let u = used_today(&app);
                (app, mb, u)
            })
            .collect();
        let temp = std::env::temp_dir();
        let temp_mb = crate::tune::folder_mb(&temp, std::time::Duration::from_millis(500));
        let survey = crate::tune::Survey {
            memory_by_app,
            disposable: vec![(temp.display().to_string(), temp_mb)],
            disk_free_gb: r.disk_free_gb,
            disk_total_gb: r.disk_total_gb,
            ram_used_gb: r.ram_used_gb,
            ram_total_gb: r.ram_total_gb,
            atlas_mb: models_mb,
            ..Default::default()
        };
        let findings = crate::tune::examine(
            &survey,
            &self.tools_ref().map(|t| t.tune.clone()).unwrap_or_default(),
        );
        // The deeper look: what's holding the memory, whoever it is.
        let mut top: Vec<&(String, u64, bool)> = survey.memory_by_app.iter().collect();
        top.sort_by_key(|b| std::cmp::Reverse(b.1));
        // (On Windows the sampled reading below says this, with CPU.)
        if !top.is_empty() && !cfg!(windows) {
            let named: Vec<String> = top.iter().take(4).map(|(a, mb, _)| format!("{a} {}", if *mb >= 1024 { format!("{:.1} GB", *mb as f32 / 1024.0) } else { format!("{mb} MB") })).collect();
            s.push_str(&format!(" Using the most memory: {}.", named.join(", ")));
        }
        // And what to do about it, on one yes (1 Oct 2026, Eric: "closing
        // things in the task manager that aren't needed, moving files, doing
        // deeper dives and actually making it run well").
        // Deeper since 2 Oct 2026: each program's CPU measured over two
        // seconds rather than memory read once, closing open to background
        // programs that stand on their own (never Windows', never Atlas's,
        // never the one in front of you), and startup read from every place
        // Windows starts things from, not only your Run key
        // (`tune_plan`, shared with "close what I don't need").
        let tune_cfg = self.tools_ref().map(|t| t.tune.clone()).unwrap_or_default();
        let sampled = crate::tune::sample_machine(std::time::Duration::from_secs(2));
        if let Some(sm) = &sampled {
            s.push(' ');
            s.push_str(&crate::tune::slowest_words(&sm.load));
        }
        let _ = startup.join();
        let mut plan = self.tune_plan(sampled.as_ref(), false);
        plan.temp = (temp_mb >= tune_cfg.min_mb).then(|| (temp.clone(), temp_mb));
        if !plan.is_empty() {
            let offer = plan.offer();
            s.push(' ');
            s.push_str(&offer);
            self.session.ask(&offer);
            self.pending_optimize = Some(plan);
        } else if let Some(f) = crate::tune::actionable(&findings).first().copied().or(findings.first()) {
            let (mb, _) = crate::tune::worth_it(&findings);
            s.push_str(&format!(" {}", f.what));
            if mb > 0 {
                s.push_str(&format!(" About {mb} MB could come back in all."));
            }
        }
        if !watched.starts_with("Not watching") {
            s.push(' ');
            s.push_str(&watched);
        }
        // Anything the health check could not actually read is a gap
        // in the answer, not a detail. Say it first rather than
        // letting a fluent summary imply it covered everything.
        {
            // Through the channel rather than straight to faithful:
            // the health answer is a result, so it also has to be
            // sayable to someone who heard no progress at all.
            let (said, leaks) = self.run.close(&self.health_steps(&r), &s);
            for l in &leaks {
                self.log.info(&format!("result leans on progress: {}", l.plain()));
            }
            said
        }
    }

    fn on_use_clipboard(&mut self, what: &str) -> String {
        let cfg = self.clipboard_cfg();
        if !cfg.enabled {
            return "Using the clipboard is switched off.".into();
        }
        // What was copied: whatever a caller already set (a test, or a
        // future push path), else read the OS clipboard now. On request
        // only — this is the sole place it's read, never in the
        // background. `None` from the platform means there's no way to
        // reach the clipboard here, which is not the same as it being
        // empty.
        let copied = match self.clipboard_text.clone() {
            Some(t) => Some(t),
            None => self.plat.read_clipboard().ok().flatten(),
        };
        match copied {
            None => "I can't reach the clipboard on this machine — there's no clipboard tool \
                     available for me to read it.".into(),
            Some(text) => {
                let grab = crate::clipboard::take(&text, &cfg);
                if grab.kind == crate::clipboard::Kind::Empty {
                    return "There's nothing on the clipboard.".into();
                }
                let prompt = crate::clipboard::prompt(what, &grab);
                // Kept for the model-context path and so a turn without
                // a model still has the question ready.
                self.pending_clipboard = Some(prompt.clone());
                // With a model, answer now and — if you've left it on —
                // put the answer back on the clipboard so you can paste
                // it where you were. Without one, just say what was
                // picked up.
                match self.llm.clone() {
                    Some(llm) => match llm.complete(crate::clipboard::ANSWER_SYSTEM, &prompt) {
                        Ok(answer) => {
                            let answer = answer.trim().to_string();
                            if answer.is_empty() {
                                grab.describe()
                            } else if cfg.reply_to_clipboard {
                                // Actually put it back, and only claim
                                // so when it landed. The old code set a
                                // field nothing drained and told you it
                                // was on the clipboard regardless — a
                                // hollow promise this closes.
                                match self.plat.write_clipboard(&answer) {
                                    Ok(()) => {
                                        self.clipboard_writeback = Some(answer.clone());
                                        format!(
                                            "{}\n\n{answer}\n\n(It's back on your clipboard — \
                                             paste it where you were.)",
                                            grab.describe()
                                        )
                                    }
                                    Err(_) => format!(
                                        "{}\n\n{answer}\n\n(I couldn't put it back on the \
                                         clipboard on this machine, so copy it from here.)",
                                        grab.describe()
                                    ),
                                }
                            } else {
                                format!("{}\n\n{answer}", grab.describe())
                            }
                        }
                        // A model that couldn't answer shouldn't lose
                        // you the grab; you can still ask again.
                        Err(_) => grab.describe(),
                    },
                    None => grab.describe(),
                }
            }
        }
    }

    fn on_rehearse(&mut self, command: &str) -> String {
        // Runs against the same fake operating system the tests use,
        // so there is no path from here to your real windows.
        let mock = crate::platform::mock::MockPlatform::new(
            self.plat.monitors().unwrap_or_default(),
        );
        let inner = self.parser.parse(command);
        let reply = match &inner {
            Intent::WorkspaceOn => {
                crate::heard!(workspace::workspace_on(self.cfg, &mock));
                String::new()
            }
            Intent::WorkspaceOff => {
                crate::heard!(workspace::workspace_off(self.cfg, &mock));
                String::new()
            }
            Intent::OpenApp(a) => {
                crate::heard!(workspace::open_app(self.cfg, &mock, a));
                String::new()
            }
            Intent::CloseApp(a) => {
                crate::heard!(workspace::close_app(self.cfg, &mock, a));
                String::new()
            }
            other => format!("I don't know how to rehearse {}.", other.plain()),
        };
        if !reply.is_empty() {
            return reply;
        }
        let r = crate::rehearse::from_actions(command, &mock.actions());
        self.last_rehearsal = Some(r.detail());
        // The "!" that flags an irreversible step lives only in
        // `detail`, which is stored for reading and never spoken. So
        // the spoken line -- the one you actually hear before saying
        // "go" -- gave a step count and no hint that one of those
        // steps closes an app or types into it and cannot be taken
        // back. `touches_anything_irreversible` answers exactly that,
        // and now the warning reaches your ear, not just the stored
        // walk-through.
        let mut spoken =
            format!("{} {}", crate::rehearse::preamble(command), r.summary());
        if r.touches_anything_irreversible() {
            spoken.push_str(" Heads up: some of these can't be undone.");
        }
        spoken
    }

    fn on_ready(&mut self) -> String {
        // The waking moment: the mark, then the one thing that matters.
        self.wants_panel = Some(crate::panel::Panel::Waking);
        self.panel_shown_at = crate::store::now();
        self.draw_panel(crate::panel::Panel::Waking);
        let brief = crate::mind::speak_brief(&self.brief_items());
        self.pending_brief = None;
        brief
    }

    fn on_show(&mut self, what: &str) -> String {
        let w = what.trim().to_lowercase();
        let panel = if w.contains("outstanding") || w.contains("task") || w.contains("list") {
            crate::panel::Panel::Tasks
        } else if w.contains("think") || w.contains("working") || w.contains("doing") {
            crate::panel::Panel::Mind
        } else if w.contains("setting") || w.contains("hub") || w.contains("control") {
            crate::panel::Panel::Controls
        } else {
            // "I don't have a the budget" (the capability sweep): the
            // thing named as it was said, and what can be shown.
            let w = w.trim_start_matches("the ").trim_start_matches("my ").trim_start_matches("a ");
            // "pull up spotify for me" (1 Oct 2026: "I can't put spotify for
            // me on screen"): the politeness off the end.
            let tidied = crate::intent::without_fillers(w);
            let w = tidied.as_str();
            // "show me what jobs you've found": the opportunities, not a panel.
            if ["job", "gig", "opportunit"].iter().any(|k| w.contains(k)) {
                return crate::hunting::said(self, "show me the opportunities", clock());
            }
            // "pull up chrome" is an app, not a panel (30 Sep 2026: "pull up
            // TradingView" got "I don't have tradingview to put up").
            let squashed: String = w.chars().filter(|c| c.is_alphanumeric()).collect();
            if let Some(app) = self.cfg.apps.apps.keys().find(|k| k.eq_ignore_ascii_case(w) || k.eq_ignore_ascii_case(&squashed)).cloned() {
                return match crate::workspace::focus_app(self.cfg, self.plat, &app) {
                    Ok(()) => format!("There's {app}."),
                    Err(e) => format!("I couldn't bring up {app}: {e}"),
                };
            }
            // A window already open with that in its title ("TradingView" in Chrome).
            if let Some(win) = self.cfg.apps.apps.values().find_map(|spec| {
                let mut s = spec.clone();
                s.title_hints = vec![w.to_string(), squashed.clone()];
                self.plat.find_window(&s).ok().flatten()
            }) {
                crate::heard!(self.plat.focus(win));
                return format!("There's {w}.");
            }
            // Not configured and not open: an app by its Start-menu name, as
            // "open" does -- asked about first, the same gate.
            let shortcut_found = !w.is_empty()
                && w.split_whitespace().count() <= 3
                && matches!(
                    crate::launcher::pick(w, &crate::launcher::shortcuts(&crate::launcher::start_menu_dirs()), &Default::default(), clock()),
                    crate::launcher::Pick::Open(_)
                );
            if shortcut_found {
                let intent = Intent::OpenApp(w.to_string());
                match self.gate_app(w, "open", &intent) {
                    AppGate::Ask(q) => return q,
                    AppGate::Go => {
                        let launched = self.wd_launch(&format!("start up {w}"), clock());
                        if !launched.starts_with("I can't find") {
                            return launched;
                        }
                    }
                }
            }
            return format!(
                "I can't put {w} on screen -- it isn't an app I know or a window that's open. I can open an app by name, \
                 or show what's outstanding, what I'm working on, or the settings."
            );
        };
        let monitors = self.plat.monitors().unwrap_or_default();
        match crate::panel::place(panel, &monitors, &self.panel_cfg()) {
            crate::panel::Decision::Show(_) => {
                self.wants_panel = Some(panel);
                self.draw_panel(panel);
                // Spoken as well: the panel is for glancing at, the
                // words are what you actually take in.
                // `panel::narration` owns the "what to say per panel,
                // and whether to say it at all" decision -- it returns
                // None when speaking would be wrong -- so the daemon
                // does not keep a second copy of that table.
                //
                // RECOVERED IN THE 17 SEP MERGE. On 16 Sep this branch
                // was hand-rolling `narration`'s logic; wiring the real
                // function removed the duplicate. The improvements
                // side's `daemon.rs` still had the hand-rolled version,
                // and taking it whole re-orphaned `panel::narration` --
                // caught by `dead_capabilities` listing it as an
                // ORPHAN. Two copies of one decision is how they drift.
                let content = match panel {
                    crate::panel::Panel::Tasks => self.backlog.summary(),
                    crate::panel::Panel::Mind => self.mind_summary(),
                    // Settings and the hub live in Atlas's own window
                    // now — its Settings and Hub pages, not a panel
                    // that listed nothing.
                    _ if w.contains("hub") => {
                        match crate::firstlaunch::open_atlas_window(&crate::firstlaunch::First::Hub("/hub".into())) {
                            Ok(()) => "The hub is open.".into(),
                            Err(e) => format!("I couldn't open the hub: {e}"),
                        }
                    }
                    _ => match crate::firstlaunch::open_atlas_window(&crate::firstlaunch::First::Settings) {
                        Ok(()) => "Settings are open.".into(),
                        Err(e) => format!("I couldn't open my settings: {e}"),
                    },
                };
                crate::panel::narration(panel, &content, &self.panel_cfg())
                    .unwrap_or_default()
            }
            crate::panel::Decision::AskFirst(q) => {
                self.session.ask(&q);
                self.pending_panel = Some(panel);
                q
            }
            crate::panel::Decision::SpeakOnly(_) => match panel {
                crate::panel::Panel::Tasks => self.backlog.summary(),
                crate::panel::Panel::Mind => self.mind_summary(),
                _ => "No screen for that, so ask me and I'll tell you.".into(),
            },
        }
    }

    fn on_dismiss(&mut self) -> String {
        // Taken away without a word -- unless there was nothing up,
        // when silence reads as not having heard (the sweep).
        let was_up = self.wants_panel.is_some() || self.pending_panel.is_some();
        self.wants_panel = None;
        self.pending_panel = None;
        if was_up { String::new() } else { "There's nothing up to put away.".into() }
    }

    fn on_capabilities_settled(&mut self, what: &str) -> String {
        let chose = crate::whichone::weigh(
            what,
            crate::whichone::WHICH_MACHINE,
            &self.tools_cfg().whichone,
        )
        .settled();
        let p = match chose {
            Some("mac") => crate::portable::Platform::Mac,
            Some("linux") => crate::portable::Platform::Linux,
            Some("android") => crate::portable::Platform::Android,
            Some("windows") => crate::portable::Platform::Windows,
            // `ios` and, unreachably, anything else. The guard above
            // only admits a settled reading, and every id in
            // WHICH_MACHINE is named here --
            // `every_machine_reading_maps_to_a_platform` fails the
            // build if a sixth is added without an arm.
            _ => crate::portable::Platform::Ios,
        };
        // Answered from the catalogue rather than from the list of
        // machine powers. "9 of 10 work" was true and useless: what
        // somebody asking this wants is how many of the things Atlas
        // does survive the move, which is a question only the join of
        // the two tables answers.
        format!(
            "{} {} {}",
            crate::capability::on_platform_summary(p),
            crate::portable::honest_summary(p),
            crate::portable::for_a_friend(p)
        )
    }

    fn on_capabilities_plain(&mut self, what: &str) -> String {
        let w = what.trim().to_lowercase();
        if w.contains("offline")
            || w.contains("without internet")
            || w.contains("no internet")
            || w.contains("without a connection")
            || w.contains("no connection")
            || w.contains("unplugged")
        {
            // Offline-first is the whole point of this tree, so "what
            // works without the internet" is a first-class question,
            // not one that should fall through to the catch-all and be
            // matched against a single capability by keyword. Each
            // capability records whether it needs the network;
            // `offline_count` is the join, and the honest answer is a
            // count rather than a promise.
            let (offline, total) = crate::capability::offline_count();
            format!("{offline} of {total} things work with the network unplugged.")
        } else if w.contains("in detail")
            || w.contains("full list")
            || w.contains("one by one")
            || w.contains("each one")
            || w.contains("broken down")
            || w.contains("list them")
            || w.contains("list all")
            || w.contains("list everything")
            || w.contains("whole list")
        {
            // "list them all", "what can you do, in detail" -- the
            // itemised answer rather than the counts. `summary` gives
            // "N things work right now"; someone asking to see the list
            // wants the list, grouped by area with a legend, which is
            // exactly what `full` builds and nothing until now called.
            crate::capability::full()
        } else if w.contains("finish")
            || w.contains("to build")
            || w.contains("still to build")
            || w.contains("unfinished")
        {
            // "what's left to finish", "what's left to build" -- the
            // honest completion backlog, split by who can finish each
            // part: what Atlas could wire itself, what only needs a real
            // run on the machine, and what waits on you.
            // `capability::to_finish_report` is that view and nothing
            // until now called it -- the self-finishing picture a person
            // actually means by "what's left".
            crate::capability::to_finish_report()
        } else if w.contains("right now")
            || w.contains("working")
            || w.contains("work now")
            || w.contains("actually do")
        {
            // "what can you do right now", "what's working" -- name the
            // things usable this moment, not the counts and not the
            // whole catalogue. `summary` says "13 things work right now"
            // and `full` lists everything including what's blocked or
            // off; someone asking what *works* wants only the usable set
            // by name. `capability::working` is that filter and nothing
            // until now asked it -- the question fell through to the
            // keyword match and came back "I don't have anything for
            // that."
            let working = crate::capability::working();
            if working.is_empty() {
                "Nothing's usable right now.".into()
            } else {
                // Grouped by area (28 Sep 2026): with the catalogue
                // complete, 55-odd names in one run-on sentence was a
                // list nobody could follow, heard or read.
                let mut parts: Vec<String> = Vec::new();
                for area in crate::capability::EVERY_AREA {
                    let names: Vec<&str> =
                        working.iter().filter(|c| c.area == *area).map(|c| c.what).collect();
                    if !names.is_empty() {
                        parts.push(format!("{} ({}): {}", area.plain(), names.len(), names.join("; ")));
                    }
                }
                format!("{} things work right now: {}.", working.len(), parts.join(". "))
            }
        } else if w.is_empty() || w.contains("what can you") || w.contains("everything") {
            crate::capability::summary()
        } else if w.contains("new") {
            let recent = crate::capability::since(17);
            match recent.first() {
                Some(c) => format!(
                    "{} new things, most recently: {}.",
                    recent.len(),
                    c.what
                ),
                None => "Nothing new.".into(),
            }
        } else if w.contains("can't") || w.contains("cant") || w.contains("waiting") {
            let blocked = crate::capability::what_would_unblock_most();
            match blocked.first() {
                Some((need, n)) => format!("{n} things are waiting on {need}."),
                None => "Nothing's blocked.".into(),
            }
        } else {
            // "can you read my email?", "can you see me" -- the
            // ability searched for in the catalogue, with its state
            // on this machine and what turns it on (30 Sep 2026: it
            // matched the first entry sharing a long word, and said
            // "I don't have anything for that" otherwise).
            let research = self.tools_cfg().research.enabled;
            // The capabilities tool may name an entry by its id
            // ("vision"): that entry's own state.
            match crate::capability::can(w.trim()).filter(|_| w.trim() != "research") {
                Some((_, why)) => why,
                None => match crate::capability::answer_can(what, research) {
                    Some(a) => a,
                    None => format!(
                        "Nothing I can do is about \u{201c}{}\u{201d}, as far as my own list goes. Ask what I can do for the list.",
                        what.trim()
                    ),
                },
            }
        }
    }

    fn on_history(&mut self, what: &str) -> String {
        use crate::undo::{reverse, say, tell, understand, Asking};
        match understand(if what.trim().is_empty() { "what did you do" } else { what }) {
            Asking::WhatDidYouDo { since_mins } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let since = now.saturating_sub(since_mins * 60);
                tell(&self.history.since(since))
            }
            // "What did you do on your own?" answers from the
            // unprompted actions alone. `on_its_own` was written for
            // exactly this review and nothing reached it: the general
            // list mentioned how many were unasked but gave no way to
            // see only those, so the one question a person asks to
            // check what Atlas took upon itself fell through to the
            // whole log or, worse, to "did you mean...".
            Asking::WhatOnYourOwn { since_mins } => {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                let since = now.saturating_sub(since_mins * 60);
                let mine = self.history.on_its_own(since);
                match mine.first() {
                    None => "Nothing on my own — everything I did, you asked for.".into(),
                    Some(newest) => {
                        let n = mine.len();
                        let word = if n == 1 { "thing" } else { "things" };
                        let mut s =
                            format!("{n} {word} I did on my own. Most recent: {}.", newest.what);
                        s.push_str(" Say undo and I'll take back the last one.");
                        s
                    }
                }
            }
            Asking::UndoLast => say(&reverse(self.history.last())),
            Asking::UndoIn(area) => say(&reverse(self.history.last_in(&area))),
            Asking::SomethingElse => {
                "Did you mean what I've done, or undoing something?".into()
            }
        }
    }

    fn on_why(&mut self, about: &str) -> String {
        let matching: Vec<&crate::why::Decision> = self
            .decisions
            .decisions
            .iter()
            .filter(|_| about.trim().is_empty() || crate::why::is_asking_why(about))
            .collect();
        if about.trim().is_empty() {
            // A bare "why" — "why is my workspace like this?" — asks
            // for everything decided, not just the one thing said
            // last. `why::account` was built for exactly this and
            // had no caller: `answer` only ever received the single
            // latest `Decision`, so a general question got a
            // one-line answer about whatever happened to be most
            // recent rather than an actual account.
            let last_steps = matching.last().map(|d| crate::why::steps(d));
            let account = crate::why::account(&matching);
            if let Some(steps) = last_steps {
                self.show_panel(crate::window::Panel::Thinking, "How I got there", steps);
            }
            // The live half `why::account` cannot know: what the
            // mind is on RIGHT NOW, in its own recent words. A bare
            // "why" during a running job is usually about the job.
            if let Some(w) = self.mind.focus() {
                let recent: Vec<String> = w
                    .recent_thinking(3)
                    .iter()
                    .map(|th| th.text.clone())
                    .collect();
                if !recent.is_empty() {
                    return format!(
                        "Right now, {}: {}. {account}",
                        w.asked,
                        recent.join("; ")
                    );
                }
            }
            return account;
        }
        // Cloned out before the panel call, which needs `&mut self`.
        let latest = matching.last().map(|d| (*d).clone());
        if let Some(d) = &latest {
            self.show_panel(
                crate::window::Panel::Thinking,
                "How I got there",
                crate::why::steps(d),
            );
        }
        match latest.as_ref() {
            Some(d) => crate::why::answer(Some(d)),
            None => crate::why::answer(None),
        }
    }

    fn on_create_account(&mut self, where_: &str) -> String {
        let money = self.tools_ref()
            .map(|t| t.finance.clone())
            .unwrap_or_default();
        match crate::enrol::domain_from(where_) {
            None => "I couldn't tell which site you meant.".into(),
            Some(domain) => {
                let cfg = self.tools_ref()
                    .map(|t| t.enrol.clone())
                    .unwrap_or_default();
                // One gate, not a hand-rolled subset of it. This
                // branch used to check `never_enrols_on` and
                // `cfg.enabled` itself -- and not `cfg.never_on`, so
                // a domain on your OWN never list sailed past the
                // daemon while `enrol::permitted` would have refused
                // it. Two copies of one decision, one of them short a
                // check, is exactly the drift `permitted` exists to
                // prevent.
                match crate::enrol::Enrolment::permitted(&domain, &cfg, &money) {
                    // `permitted`'s own wording for this one is terse;
                    // keep the line that tells you where the switch is.
                    Err(why) if why == "account creation is switched off" => "Signing up is switched off. Turn on \"Make accounts\" in \
                         settings if you want me doing that.".to_string(),
                    Err(why) => why,
                    // Eric, B6: Atlas may make accounts. It
                    // stops for good at payment or ID, and hands
                    // a robot check or a code to you.
                    Ok(()) => self.start_sign_up(&domain, crate::store::now()),
                }
            }
        }
    }

    fn on_sign_in(&mut self, where_: &String) -> String {
        let cfg = self.tools_cfg().signin.clone();
        if !cfg.enabled {
            "Signing in is switched off. Turn on \"Sign you into sites\" in settings.".into()
        } else if self.vault.state() != crate::vault::State::Open {
            "The vault's locked — say the passphrase first.".into()
        } else {
            match self.access.which_account(where_, Some(where_)) {
                crate::signin::Which::None => {
                    format!("I don't have access to {where_}. Want to give me it?")
                }
                crate::signin::Which::Several(_) => self
                    .access
                    .which_account(where_, None)
                    .ask(where_)
                    .unwrap_or_default(),
                crate::signin::Which::One(account) => {
                    // The grant is actually checked now. This branch
                    // used to announce "signing you into X" having
                    // checked only that the feature was on — the
                    // lookalike-domain check, the grant's own
                    // `Allowed::Nothing`, and the vault state were
                    // all in `may_fill`, which nothing called.
                    //
                    // `you_are_here` is true by construction: this
                    // intent exists because you just asked for it.
                    // The two questions `may_fill` also asks — is
                    // this a login form, was it reached by typing the
                    // address — belong to a browser, and nothing
                    // here has one open, so `may_start` is the
                    // honest half rather than two invented `true`s.
                    let vault_open =
                        self.vault.state() == crate::vault::State::Open;
                    // A credential that has stopped working is worth
                    // saying before the attempt, not after: "sign-in
                    // failed" sends you to check the site, and the
                    // answer is usually that you changed the password
                    // somewhere else.
                    if let Some(g) = self
                        .access
                        .find_account(where_, &account)
                        .filter(|g| g.looks_superseded)
                    {
                        return crate::signin::probably_changed(g);
                    }
                    match self.access.may_start(where_, vault_open, true, &cfg) {
                        Err(refused) => {
                            // Every attempt is recorded, whether it
                            // worked or not. A refusal is a use of
                            // the credential that did not happen, and
                            // the trail is the only way a wrong one
                            // is visible afterwards.
                            self.access.note_use(
                                where_,
                                &account,
                                where_,
                                false,
                                true,
                                crate::store::now(),
                            );
                            refused.say()
                        }
                        Ok(_) => {
                            self.access.note_use(
                                where_,
                                &account,
                                where_,
                                true,
                                true,
                                crate::store::now(),
                            );
                            // Your bank is a site like any other for
                            // signing in. What it isn't is a site
                            // Atlas can do anything else on, and that
                            // line is drawn in finance::allowed
                            // rather than here.
                            let money = self.tools_cfg().finance.clone();
                            let care = if money.is_financial(where_) {
                                " I'll read what's there and nothing else — no \
                                 transfers, no orders."
                            } else {
                                ""
                            };
                            if crate::signin::Access::asks_first(&cfg) {
                                // Asked, and now actually waited on:
                                // this question used to go nowhere.
                                let q = format!(
                                    "Sign you into {where_} as {account}? You've asked \
                                     me to check first each time.{care}"
                                );
                                self.session.ask(&q);
                                self.pending_signin = Some((where_.clone(), account.clone()));
                                q
                            } else {
                                // Autofill, for real (Eric, B4): the
                                // login goes in on the site's own
                                // domain, in Atlas's browser.
                                let started = self.start_sign_in(where_, &account, crate::store::now());
                                format!("{started}{care}")
                            }
                        }
                    }
                }
            }
        }
    }

    fn on_recap(&mut self) -> String {
        let convo = self.session.transcript(20);
        if convo.trim().is_empty() {
            "We haven't said anything yet this session.".into()
        } else {
            format!("Here's our conversation so far:\n{convo}")
        }
    }

    fn on_finish_setup(&mut self) -> String {
        let mut fr = crate::firstrun::FirstRun::load(&self.store);
        match fr.resume() {
            Some(step) => {
                if let Err(e) = fr.save(&self.store) {
                    return format!(
                        "I couldn't write that down ({e}), so it would ask you \
                         again next time. Left alone."
                    );
                }
                format!(
                    "Picking up {}. Start Atlas again and its setup carries on from there.",
                    step.plain()
                )
            }
            // Said plainly rather than starting setup over. Somebody
            // who says this when nothing was skipped meant "is there
            // anything left", and the answer is no.
            None => "Nothing was left unfinished — setup is done.".into(),
        }
    }

    fn on_this_is_me(&mut self) -> String {
        let who = self.tools_cfg().vision.your_face.clone();
        let who = if who.trim().is_empty() { "me".to_string() } else { who };
        self.name_this(&who, crate::store::now())
    }

    fn on_hand_over(&mut self, note: &str) -> String {
        let state = crate::roots::install_state();
        let mut h = crate::handover::Handover::load(&state);
        let said = h.hand_over(note, clock());
        if let Err(e) = h.save(&state) {
            // A handover that did not survive being written down is
            // one that ends the moment Atlas restarts, which is worse
            // than refusing: you would have handed the laptop over
            // believing it was narrowed.
            return format!(
                "I couldn't write that down ({e}), so I can't promise it holds. \
                 Try handing it over again."
            );
        }
        said
    }

    fn on_unlock(&mut self, phrase: &str) -> String {
        let now = clock();
        match self.vault.open(phrase, now, &self.tools_cfg().vault) {
            Ok(()) => {
                // Your sign-in copy follows the setting: made at the
                // first unlock after it's turned on, taken away at
                // the first unlock after it's turned off.
                let sign_in = self.keep_sign_in_copy(now);
                // The first unlock is where the salt and the check
                // value come into existence. Not writing them here
                // means the next start has neither, and a vault with
                // no check value opens for anything.
                let first = !self.vault.proved_it();
                if let Err(e) = self.vault.save(&self.vault_home) {
                    format!(
                        "Open, but I couldn't write the vault to disk ({e}), so this \
                         passphrase won't be remembered past this run."
                    )
                } else if first {
                    format!("Open. That's the passphrase set — it's checked from now on.{sign_in}")
                } else {
                    format!("Open.{sign_in}")
                }
            }
            Err(why) => {
                let mut c = why.chars();
                match c.next() {
                    Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str().trim_end_matches('.')),
                    None => why,
                }
            }
        }
    }

    fn on_pair(&mut self, their_name: &str) -> String {
        let their_name = spoken_name(their_name);
        if their_name.is_empty() {
            return "Who should I pair with?".into();
        }
        let kin_cfg = self.tools_cfg().kin.clone();
        if !kin_cfg.enabled {
            return "Pairing is switched off. Turn on kin in settings first.".into();
        }
        let (Some(my_name), Some(my_host)) =
            (kin_cfg.my_name.clone(), kin_cfg.my_host.clone())
        else {
            return "I need your name and Tailscale address set in config first -- \
                    kin.my_name and kin.my_host -- before I can pair with anyone."
                .into();
        };
        // One directory for pairings, and it is the install's, not
        // the active profile's. See `kin::where_pairings_live`.
        let dir = self.peer_dir.clone();
        let mut pairings = crate::kin::Pairings::load(&dir);
        let token = match crate::server::new_token() {
            Ok(t) => t,
            Err(e) => return format!("Couldn't generate a secure token: {e}"),
        };
        match crate::kin::invite(&mut pairings, their_name, &my_name, &my_host, kin_cfg.port, &token) {
            None => "Your name or host can't contain '|' -- fix that in config and try \
                     again."
                .into(),
            Some(code) => {
                // Onto the live door as well, so "can already reach
                // you" is true now rather than after a restart. The
                // mirror of the revoke below: the door is built once
                // at startup, so a peer written only to the file
                // could not be served -- the pairing completed on
                // both ends and then did not work, with nothing
                // saying why.
                let now_open = match (&self.signal_listener, pairings.peers.last()) {
                    (Some(l), Some(p)) => l.admit_peer(p.clone()),
                    // No listener: nothing is reachable either way,
                    // and the sentence below says so.
                    _ => false,
                };
                match pairings.save(&dir) {
                    Err(e) => format!("Couldn't save the pairing: {e}"),
                    Ok(()) if now_open => format!(
                        "{their_name} can already reach you -- I've remembered them. Send \
                         them exactly this: {code}"
                    ),
                    Ok(()) => format!(
                        "Remembered {their_name}. The door for peers isn't open in this \
                         session, so they can reach me from my next start. Send them \
                         exactly this: {code}"
                    ),
                }
            }
        }
    }

    fn on_accept_pairing(&mut self, code: &str) -> String {
        let code = code.trim();
        if code.is_empty() {
            return "Paste the pairing code someone sent you.".into();
        }
        let kin_cfg = self.tools_cfg().kin.clone();
        if !kin_cfg.enabled {
            return "Pairing is switched off. Turn on kin in settings first.".into();
        }
        let (Some(my_name), Some(my_host)) =
            (kin_cfg.my_name.clone(), kin_cfg.my_host.clone())
        else {
            return "I need your name and Tailscale address set in config first -- \
                    kin.my_name and kin.my_host -- before I can accept a pairing."
                .into();
        };
        // One directory for pairings, and it is the install's, not
        // the active profile's. See `kin::where_pairings_live`.
        let dir = self.peer_dir.clone();
        let mut pairings = crate::kin::Pairings::load(&dir);
        match crate::kin::accept(&mut pairings, code, &my_name, &my_host, kin_cfg.port) {
            Err(e) => format!("Couldn't accept that: {}", e.plain()),
            Ok(accepted) => match pairings.save(&dir) {
                Err(e) => format!("Couldn't save the pairing: {e}"),
                Ok(()) => match accepted.return_block {
                    None => format!("Paired with {}.", accepted.from),
                    Some(block) => format!(
                        "Paired with {}. Send this back so they can reach you too: {block}",
                        accepted.from
                    ),
                },
            },
        }
    }

    fn on_forget_peer(&mut self, name: &str) -> String {
        let name = spoken_name(name);
        if name.is_empty() {
            return "Who should I forget the pairing with?".into();
        }
        // One directory for pairings, and it is the install's, not
        // the active profile's. See `kin::where_pairings_live`.
        let dir = self.peer_dir.clone();
        let mut pairings = crate::kin::Pairings::load(&dir);
        if !pairings.forget(name) {
            return format!("I don't have a pairing with {name}.");
        }
        // The live door, not only the file.
        //
        // `Door` is built once at startup from a cloned peer list. So
        // rewriting `kin_peers.yaml` took effect at the next restart
        // and not before -- while this arm replied "Forgotten. {name}
        // can no longer reach you", and the next `/signal` or
        // `/handoff` from that peer was accepted and delivered. A
        // stated protection the code did not implement, for as long
        // as Atlas stayed up.
        let door_closed = match &self.signal_listener {
            Some(l) => l.forget_peer(name),
            // No listener means nothing is open in the first place,
            // so there is nothing to close and nothing to qualify.
            None => true,
        };
        match pairings.save(&dir) {
            Err(e) => format!(
                "I've stopped letting {name} in, but I couldn't write it down: {e}. \
                 That means it comes back when I restart -- worth running this again."
            ),
            Ok(()) if door_closed => format!(
                "Forgotten. {name} can no longer reach you, and you can no longer reach them."
            ),
            // Honest about the half that did not happen. The file is
            // right, so the next restart is right; until then the
            // open door is still open.
            Ok(()) => format!(
                "Written down, so {name} is gone from my pairings. I couldn't close the \
                 door that's already open, though -- they can still reach me until I \
                 restart."
            ),
        }
    }

    fn on_capture(&mut self, text: &str) -> String {
        let now = clock();
        let cfg = self.tools_cfg().capture.clone();

        // A sentence that already contains the whole record makes an
        // item, not a note. A note you file later is a debt.
        let people: Vec<String> = Vec::new();
        let spoken = crate::capture::read_spoken(text, &cfg.projects, &people);
        // A capture that names a project is the project being
        // worked on — `touch_project` stamps `last_touched` so
        // "what was I on?" has an answer. The `projects` store had
        // no production writer at all before this.
        if let Some(p) = &spoken.project {
            self.memory.touch_project(p, None);
            let _ = self.memory.save(&self.store);
            // If it names a project Atlas is tracking in the workshop,
            // the capture is also an outstanding task for that project
            // — so it shows up in that project's window, not just as a
            // loose note. `title` is what to do, if the capture named
            // one, else the raw text.
            if self.workshop.resolve(p).is_some() {
                let task = spoken.title.clone().unwrap_or_else(|| text.to_owned());
                self.workshop.add_task(p, &task, now);
                let _ = self.workshop.save(&self.store);
            }
        }
        if spoken.is_a_whole_item() {
            // Kept as well as announced (30 Sep 2026: it said
            // `Made "Tuesday tips", due friday.` and kept it nowhere
            // unless the project was one the workshop tracks).
            let id = self.notebook.capture(text, None, now, &cfg);
            self.wd_date_note(id, now);
            self.synclog.append(crate::sync::What::Captured { id: id.to_string(), text: text.to_owned() }, now);
            if let Err(e) = self.notebook.save(&self.store) {
                return format!("I couldn't write that down ({e}). Say it again once that's sorted, because I haven't kept it.");
            }
            crate::capture::made(&spoken)
        } else {
            let id = self.notebook.capture(text, None, now, &cfg);
            // "Call the bank Friday at 10" is dated, so it comes up
            // in Friday's brief (round 11).
            self.wd_date_note(id, now);
            // Recorded for your other devices at the same moment it
            // is written here. A capture is the cleanest thing sync
            // carries -- `What::Captured` can never clash, because
            // adding something is not a change to anything.
            self.synclog.append(
                crate::sync::What::Captured {
                    id: id.to_string(),
                    text: text.to_owned(),
                },
                now,
            );
            // Written down before Atlas says it was. Acknowledging
            // first and saving afterwards leaves a window where the
            // reply is true and the disk is not, and a thought caught
            // before it was gone is exactly the thing not to lose to
            // a window.
            if let Err(e) = self.notebook.save(&self.store) {
                return format!(
                    "I couldn't write that down ({e}). Say it again once \
                     that's sorted, because I haven't kept it."
                );
            }
            // Also remember it as a typed fact, so "what do you know
            // about the wifi" finds it later — the one capture feeding
            // both the note store and the fact book, not two islands
            // that never see each other's contents. A task is an action
            // to do, not a fact about your world, so only non-tasks are
            // filed as knowledge; the kind (a preference, a pointer, or
            // a fact about you) is read from the words.
            if crate::capture::kind_of(text) != crate::capture::Kind::Task {
                // `learn`, not `put`: saying the same thing again
                // strengthens the one fact instead of adding a duplicate,
                // which is what keeps the book bounded through normal use.
                // `trim` then holds the footprint under budget by fading
                // only Atlas's own stale guesses — never anything you
                // stated. See facts::Book::trim.
                self.facts.learn(crate::facts::Fact::stated(text, now), now);
                self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
                let _ = self.facts.save(&self.store);
            }
            // "Turn tasks into outstanding items automatically" --
            // `tasks_become_work` ships `true` and was read by
            // nothing, so a captured task stayed a note and never
            // reached the list Atlas reads back to you. Both halves
            // existed: `capture::kind_of` already says which captures
            // are tasks, and `Backlog` is already what `outstanding()`
            // and the brief report from.
            //
            // Filed as `Unsupported`, which reads "I can't do that one
            // for you yet" and, unlike every other blocker, is never
            // offered back (`is_blocked` returns true for it
            // unconditionally). That is the honest shape: your task on
            // your list, not Atlas volunteering to phone the dentist.
            if cfg.tasks_become_work
                && crate::capture::kind_of(text) == crate::capture::Kind::Task
            {
                self.backlog.record(
                    text,
                    crate::backlog::Blocker::Unsupported("do that one for you".into()),
                    now,
                );
                let _ = self.backlog.save(&self.store);
            }
            match spoken.worth_asking() {
                // One question, and only when it would turn a note
                // into something finished -- and only when you have
                // not said not to. `never_ask_on_capture` ships
                // `true` ("Ask nothing at capture time. This is the
                // whole point") and was read by nothing until 18 Sep
                // 2026, so the default install did the opposite of
                // its own default.
                Some(q) if spoken.title.is_some() && !cfg.never_ask_on_capture => {
                    format!("{} {q}", self.notebook.acknowledge(id))
                }
                _ => self.notebook.acknowledge(id),
            }
        }
    }

    fn on_mail_which_one(&mut self, what: &str) -> String {
        let w = crate::whichone::weigh(
            what,
            crate::whichone::ABOUT_MAIL,
            &self.tools_cfg().whichone,
        );
        crate::whichone::which_did_you_mean(&w, crate::whichone::ABOUT_MAIL)
            .unwrap_or_else(|| "Which did you mean?".into())
    }

    fn on_mail_rules(&mut self) -> String {
        let rules = crate::ledger::trading_rules();
        // The one that costs people money, and why it applies to you
        // rather than in general.
        match rules.first() {
            Some(r) => format!("{} — {}", r.what, r.why_you),
            None => "Nothing I know about that.".into(),
        }
    }

    fn on_mail_statements(&mut self) -> String {
        let cfg = self.tools_cfg().money.clone();
        if !cfg.enabled {
            "Reading your statements is switched off.".into()
        } else {
            // This passed `&[]`. `money::spoken` opens with
            // `if m.by_bucket.is_empty() { return "Nothing to go on." }`
            // -- so with no entries that was the *only* reachable
            // answer, and the reason was that nothing had ever read a
            // statement rather than that there was nothing in it.
            // Same shape as `goingaway::spoken(&[])` and
            // `messaging::spoken(&[], ..)` before it.
            let this: Vec<crate::money::Entry> =
                self.store.load(crate::money::THIS_MONTH);
            if this.is_empty() {
                "I haven't read a statement yet. Hand me your bank's export (.csv) \
                 on the Give page and I'll sort it."
                    .into()
            } else {
                let last: Vec<crate::money::Entry> =
                    self.store.load(crate::money::LAST_MONTH);
                let jump = self.tools_cfg().finance.category_jump as f32;
                let mut changes = crate::money::new_or_grown(&this, &last, jump);
                changes.extend(crate::money::buckets_that_jumped(&this, &last, jump));
                crate::money::spoken(&crate::money::summarise(&this), &changes)
            }
        }
    }

    fn on_mail_messages(&mut self) -> String {
        let cfg = self.tools_cfg().messaging.clone();
        if !cfg.enabled {
            "I'm not connected to any messaging.".into()
        } else {
            // Was `messaging::spoken(&[], ..)`, which on an empty
            // slice returns literally "0 messages, all group chat" --
            // a count of something nothing had read, stated as fact.
            // `imap.rs` reads mail; there is no messaging reader.
            //
            // Then it was a fixed sentence saying so, which was true
            // and useless: `messaging.platforms` is a list you wrote
            // and nothing read, and two of the six things you can put
            // in it can never work at all.
            //
            // There is a reader now. `telegram.rs` reads a bot's
            // messages and `atlas telegram` keeps them, so the
            // sorting that was written and unreachable -- `sort`,
            // `folder_for`, `note_on`, `spoken` -- finally runs on
            // something somebody sent. Whether there is anything to
            // sort decides which answer this is, and the empty case
            // still refuses to give a count: "nothing read yet" and
            // "nothing in it" are different facts and the old code
            // could only ever state the second.
            let kept: Vec<crate::messaging::Message> =
                self.store.load(crate::telegram::KEPT);
            if kept.is_empty() {
                crate::messaging::what_you_asked_for(&cfg.platforms)
            } else {
                let mut answer = crate::messaging::spoken(&kept, &cfg.your_names);
                // Reading the messages is also the only moment Atlas
                // learns who has been in touch. `note_on` reads a
                // sender's messages and decides which folder they
                // belong in; kept here, the contact book grows from
                // real messages instead of being typed in by hand, and
                // a new work approach gets said rather than buried in a
                // count. Anything already personal or unsorted is filed
                // silently.
                answer.push_str(&self.note_the_senders(&kept, &cfg.your_names));
                answer
            }
        }
    }

    fn on_mail_plain(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().mail.clone();
        if what.contains("outlook setup") || what.contains("connect outlook") && what.contains("help") {
            crate::msoauth::SETUP.to_string()
        } else if what.contains("connect") && what.contains("outlook") {
            self.connect_outlook_from_request(what)
        } else if !cfg.enabled {
            "Reading your email is switched off.".into()
        } else if crate::unsub::go_ahead(what) || crate::unsub::go_ahead(&self.last_said.clone()) {
            // The go-ahead after the report, not another look.
            self.unsubscribe_help("unsubscribe from those").unwrap_or_default()
        } else if what.contains("clear") || what.contains("unsubscribe") {
            self.check_unsubscribe()
        } else if what.contains("outreach") {
            self.draft_outreach_from_request(what)
        } else if (what.contains("discard") || what.contains("throw") || what.contains("scrap"))
            && (what.contains("draft") || what.contains("reply"))
        {
            let who = what.rsplit("to ").next().unwrap_or(what).trim();
            self.discard_draft(who)
        } else if what.contains("draft") || what.contains("reply") {
            let who = what.rsplit("to ").next().unwrap_or(what).trim();
            self.read_draft(who)
        } else if what.contains("order") {
            self.read_order(what)
        } else {
            self.check_mail()
        }
    }

    fn on_brief_on(&mut self, who: &str) -> String {
        let cfg = self.tools_cfg().elsewhere.clone();
        if !cfg.enabled {
            "Asking your other Atlases is switched off in your settings.".into()
        } else if who.trim().is_empty() {
            match cfg.names().as_slice() {
                [] => "I don't know any other Atlas to ask. Add one under \
                       `elsewhere` in your settings."
                    .into(),
                names => format!("Which one? I can ask: {}.", names.join(", ")),
            }
        } else {
            match cfg.find(who) {
                None => {
                    let known = cfg.names();
                    if known.is_empty() {
                        format!(
                            "I don't know an Atlas called {}. Add one under \
                             `elsewhere` in your settings.",
                            who.trim()
                        )
                    } else {
                        format!(
                            "I don't know an Atlas called {} — I can ask: {}.",
                            who.trim(),
                            known.join(", ")
                        )
                    }
                }
                Some(e) => match crate::elsewhere::ask(e, &cfg) {
                    Ok(brief) => crate::elsewhere::spoken(&brief),
                    // Said plainly rather than swallowed. A check-in
                    // that goes quiet when the machine is down is
                    // useless exactly when it matters.
                    Err(why) => why,
                },
            }
        }
    }

    fn on_review_post_which_one(&mut self, what: &str) -> String {
        let w = crate::whichone::weigh(
            what,
            crate::whichone::ABOUT_A_POST,
            &self.tools_cfg().whichone,
        );
        crate::whichone::which_did_you_mean(&w, crate::whichone::ABOUT_A_POST)
            .unwrap_or_else(|| "Which did you mean?".into())
    }

    fn on_review_post_stance(&mut self, what: &str) -> String {
        // A post is a case being made, so that's what it's judged as.
        let kind = crate::stance::Kind::Case;
        let s = crate::stance::assess(what, kind);
        crate::stance::brief(&s, kind)
            .unwrap_or_else(|| "It says something and supports it. Nothing I'd add.".into())
    }

    fn on_review_post_grading(&mut self) -> String {
        let cfg = self.tools_cfg().grading.clone();
        if !cfg.enabled {
            "Colour checking is switched off.".into()
        } else {
            // Was `grading::spoken(&[])`, whose empty case is
            // "Grade looks clean." So turning colour checking *on*
            // changed the answer from an honest refusal into a false
            // pass on a file nothing had opened. Nothing in this tree
            // produces grade notes yet -- there is no node-tree
            // reader -- so the honest answer is that, not a verdict.
            "Colour checking is on, but I've no way to read the grade tree yet, \
             so I can't tell you whether it's clean. I'd rather say that than \
             pass a file I never opened."
                .into()
        }
    }

    fn on_review_post_brand(&mut self, what: &str) -> String {
        let asked = crate::editcraft::what_they_asked(what);
        match crate::editcraft::reply_to(asked) {
            Some(r) => format!("{r}\n\n{}", crate::editcraft::THE_PRINCIPLE),
            None => "I don't have a line for that one.".into(),
        }
    }

    fn on_review_post_plain(&mut self, what: &str) -> String {
        let opsec = self.tools_cfg().opsec.clone();
        // Was a fixed, aging string, and `check` was called against
        // `""` rather than the post -- meaning this arm has never
        // actually scanned a real post since it was written. `now()`
        // already exists on every daemon; slicing `iso_utc`'s first
        // ten characters is the same YYYY-MM-DD `still_applies` and
        // `days_left` compare against lexicographically, so nothing
        // about their own logic needed to change.
        let now = crate::store::now();
        let today = crate::digest::iso_utc(now);
        let today = &today[..10.min(today.len())];
        let found = crate::opsec::check(what, &[], &opsec, today);
        let mut said = crate::opsec::spoken(&found, &opsec, today);

        // Prose sits alongside opsec rather than gating on a phrase
        // the way the colour/stance/editcraft arms above do — every
        // review should still catch a dropped apostrophe or a
        // doubled word whether or not that's what was asked about.
        // But a review isn't live dictation either: a `Certain` fix
        // is applied rather than narrated one at a time -- fixing is
        // the job, not a running commentary on it -- and only the
        // ones Atlas genuinely can't tell were intentional are
        // worth asking about. Guessing wrong and silently changing
        // someone's deliberate wording is worse than asking once.
        let prose_cfg = self.tools_cfg().prose.clone();
        let mut fixes = crate::prose::check(what, &prose_cfg);
        // "better then", "more ... then": phrase-level fixes, only ever offered.
        fixes.extend(crate::prose::check_phrases(what));
        let (corrected, fixed_count) = crate::prose::apply_certain(what, &fixes);
        if fixed_count > 0 {
            if !said.is_empty() {
                said.push(' ');
            }
            said.push_str(&format!(
                "Fixed {fixed_count} thing{} in the wording: \"{corrected}\"",
                if fixed_count == 1 { "" } else { "s" }
            ));
        }
        let unsure: Vec<&crate::prose::Fix> =
            fixes.iter().filter(|f| f.kind != crate::prose::Kind::Certain).collect();
        if let Some(first) = unsure.first() {
            if !said.is_empty() {
                said.push(' ');
            }
            said.push_str(&format!(
                "You wrote \"{}\" — {}. Did you mean it, or should I fix it?",
                first.was, first.because
            ));
            if unsure.len() > 1 {
                said.push_str(&format!(" ({} more like that.)", unsure.len() - 1));
            }
        }

        // If a draft is open, the reviewed words become the draft.
        // `DraftPost` created an empty post and asked "what should
        // it say?" — and no arm ever answered: `edit` and
        // `request_approval` had no production caller, so every
        // post ever drafted stayed an empty Draft forever and the
        // corrected text this arm computed was thrown away.
        // `edit` voids any prior approval by design (the approval
        // was for the words you read), and the approval question
        // carries the full final text. Sending stays unbuilt:
        // `delivery::plan` still short-circuits, so nothing here
        // can post — it can only put the words where an eventual,
        // separately-ruled send would find them.
        let open_draft = self
            .publisher
            .posts
            .iter()
            .filter(|p| p.state == crate::publish::PostState::Draft)
            .map(|p| p.id)
            .next_back();
        if let Some(id) = open_draft {
            let final_text =
                if fixed_count > 0 { corrected.as_str() } else { what };
            if self.publisher.edit(id, final_text) {
                if let Some(q) = self.publisher.request_approval(id) {
                    let _ = self.publisher.save(&self.store);
                    // The question now waits for its answer (G2):
                    // before this, a yes went nowhere and no post
                    // could ever be approved.
                    self.session.ask(&q);
                    self.pending_post_approval = Some(id);
                    if !said.is_empty() {
                        said.push(' ');
                    }
                    said.push_str(&q);
                }
            }
        }

        if said.is_empty() {
            "Nothing I'd stop for.".into()
        } else {
            said
        }
    }

    fn on_travel_prep(&mut self) -> String {
        if self.accounts.accounts.is_empty() {
            "I don't know about any of your accounts yet, so I can't tell you \
             which ones would lock you out. The Accounts page is where they go."
                .into()
        } else {
            // Both halves. "Every account is reachable from anywhere"
            // is no comfort with no codes printed, and the codes
            // count is no comfort for the account that has none --
            // and this is the question you ask at an airport.
            let names: Vec<String> =
                self.accounts.accounts.iter().map(|a| a.site.clone()).collect();
            let ccfg = self.tools_cfg().codes.clone();
            let away: crate::goingaway::Away =
                self.store.load(crate::goingaway::AWAY_RECORD);
            let mut said = crate::goingaway::spoken(&self.accounts.accounts);
            said.push(' ');
            said.push_str(&crate::codes::before_you_go(
                &self.code_sets,
                &names,
                &ccfg,
            ));
            if let Some(days) = away.days_until(crate::store::now()) {
                said.push_str(&format!(" You're going in {days} days."));
            }
            said
        }
    }

    fn on_files(&mut self, what: &str) -> String {
        // A question about your files waits a moment for the list
        // on disk (`index::Loading`), then answers honestly either way.
        self.wait_for_index(INDEX_WAIT_FOR_A_QUESTION);
        self.files_request(what)
    }

    fn on_work_on_yourself_what(&mut self) -> String {
        let cfg = self.tools_cfg().self_audit.clone();
        if !cfg.enabled {
            "Looking at myself is switched off.".into()
        } else {
            self.refresh_signals();
            let recs = crate::selfaudit::recommend(&self.signals, cfg.most_at_once);
            let mut said = crate::selfaudit::spoken(&recs);
            // The hollow check, actually run.
            //
            // `hollow.rs` said in its own docstring that it was used
            // three ways -- doctor, the nightly self-audit, and the
            // source ratchet. Only the ratchet was true: `audit`,
            // `judge` and `judge_readings` were called by nothing. A
            // bug-detector that never runs is the bug it was written
            // to find.
            let found = self.hollow_answers();
            if !found.is_empty() {
                said.push(' ');
                said.push_str(&crate::hollow::spoken(&found, &self.tools_cfg().judgment));
            }
            // Speed is the thing a person actually feels, and this
            // answer was silent on it. `refresh_signals` above rebuilds
            // `self.signals` from undo, misunderstandings and unused
            // capabilities alone -- so the `GotSlower` signal pushed
            // during the hourly tidy is overwritten here and never
            // reaches "anything to look at?". Ask the timing window
            // itself: when a turn has measurably slowed, say why in one
            // plain sentence -- `why_slow` is the sentence written for
            // exactly that and had no caller. Gated on `got_slower` so
            // it speaks only when there is a real regression, not on
            // every fast machine.
            if self.timing.got_slower().is_some() {
                said.push(' ');
                said.push_str(&self.timing.why_slow());
            }
            said
        }
    }

    fn on_work_on_yourself_go_on(&mut self) -> String {
        let grants = self.tools_cfg().self_grant.clone();
        match grants.granted() {
            None => crate::selfgrant::asking_for(
                crate::selfgrant::Reach::WhatItSays,
                &grants,
            ),
            Some(reach) => format!(
                "Right — I'll fix {} on my own and tell you after. Anything past that \
                 still comes to you.",
                reach.plain()
            ),
        }
    }

    fn on_unknown(&mut self, raw: &str) -> String {
        match self.known_procedure(raw) {
        Some(how) => how,
        None => match self.answer_from_notes(raw, crate::store::now()) {
            Some(answer) => answer,
            // A question Atlas has no way to answer here is said to
            // be one, rather than "I didn't catch that" -- which tells
            // a person who spoke clearly that they didn't.
            // The reason as far as it's known (29 Sep 2026): this
            // said "there isn't one on this machine yet" whenever no
            // model was loaded -- also when one was there and hadn't
            // started, or didn't fit the memory free at that moment.
            None if self.llm.is_none() && crate::wanted::is_a_question(raw) => {
                let why = self.model_server_trouble.clone().unwrap_or_else(|| {
                    "it isn't loaded yet".to_string()
                });
                format!(
                    "I can't answer that one yet: general questions need my language model, and {why}. \
                     The hub's Health page says where it stands."
                )
            }
            None => "I didn't catch that.".into(),
        },
    }
    }

    /// Errand kinds that call `crew::Control::checkpoint` at safe points, so
    /// a pause actually holds them. The rest run as one step and can only
    /// finish or be called off — said so, rather than claimed paused.
    pub(super) const HOLDS_AT_A_SAFE_POINT: &'static [&'static str] = &[
        "research", "council", "build", "improve", "mail", "unsubscribe", "outreach",
        "outlook-connect", "search-check",
    ];
}

/// How a post is drafted from a channel and a topic.
const DRAFT_SYSTEM: &str = "Write one short social media post in the user's own voice, first person, for the channel named. \
Plain and specific, no hashtag walls (two at most), no emoji unless the channel is Instagram, no preamble or quotation marks: \
only the post itself. LinkedIn: three to five short sentences. X: under 270 characters.";
