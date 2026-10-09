//! The turn itself (turn_unwatched): from what was said to the reply.
//!
//! Moved out of `turn.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl<'a> Daemon<'a> {
    /// The turn itself.
    pub(super) fn turn_unwatched(&mut self, said: &str, t: u64, how: Arrival) -> String {
        // "List, did you hear me?": the misheard name said to Atlas comes off,
        // so it isn't read as a command of its own (`kws::misheard_name_opening`).
        let unnamed;
        let said = match crate::kws::misheard_name_opening(said) {
            Some(rest) if !rest.is_empty() => {
                unnamed = rest;
                unnamed.as_str()
            }
            _ => said,
        };
        self.last_said = said.to_string();
        // "Get to know me" (`getknow`): an answer goes to the interview. A
        // question or a command of its own ends it, keeping what was said.
        if let Some(reply) = self.interview_turn(said, t) {
            return reply;
        }
        // "Ask the laptop to …" from the phone (item 24).
        if let Some(reply) = self.ask_the_laptop_turn(said, t) {
            return reply;
        }
        // A question Atlas asked long ago is not what this answers.
        self.expire_stale_question(t);
        // The names "go to", "close" and "how's" may take.
        self.parser.know_names(self.names_for_the_parser());
        // Started now, not waited for: a line the parser settles doesn't
        // need it, and one that does waits for it to finish loading
        // (`models::WaitsForServer`).
        self.keep_model_server(t);
        // You did something, and *when*. This is what the gap in
        // `daily::whereabouts` and `daily::arriving` is measured from, so it
        // decides whether Atlas may get on with an hour of work and whether
        // this is the turn that earns the day's brief.
        //
        // Recorded here rather than in `execute`, where `rhythm.saw` is,
        // because `execute` has no time of its own and reaches for `clock()`.
        // That is right in production and wrong everywhere the time is
        // supplied -- a test driving a night at one in the morning had its
        // gap measured against the real wall clock, came out as zero, and
        // Atlas concluded you were sitting there. Found by the night refusing
        // to run at all.
        self.last_turn_of_yours = t;
        let _ = self.store.save("last_turn_of_yours", &self.last_turn_of_yours);
        // Old conversation folded on every path, not only the ones that
        // reach the end of a turn (H12).
        self.fold_if_due(t);
        // Your words, for the speech model's hints (H11). Saved only when a
        // word it could hint with changed, not on every turn.
        // Which project this was about (H13h), so "what have I been on"
        // and the gone-quiet check have something real to go on.
        if let Some(p) = crate::person::project_named(said, &self.person.projects) {
            self.person.touched(&p, t);
            let _ = self.person.save(&self.store);
        }
        let before = self.vocab.hints(crate::improve::HINTS_GIVEN);
        self.vocab.learn(said);
        if self.vocab.hints(crate::improve::HINTS_GIVEN) != before {
            let _ = self.store.save("vocabulary", &self.vocab);
        }
        // Away is either a silence from you, or — where the machine says how
        // long since the keyboard and mouse were touched — a break it saw.
        // Without the second, typing for an hour without speaking read as an
        // hour away, and earned a "welcome back" at the desk you never left.
        let (away_since, gone) = match self.back_from.take() {
            Some((left, back)) => (left, back.saturating_sub(left)),
            None => (self.last_present, t.saturating_sub(self.last_present)),
        };
        let was_away = gone > self.away_after;
        self.last_present = t;
        if was_away {
            // Anything that could not reach you while you were out is said
            // now. This is the half that makes holding honest -- without it,
            // "held" is a nicer word for dropped.
            let cfg = self.notify_cfg();
            // Peeked, not drained. This turn may still end early — while
            // paused, or when `addressing` decides the words were not meant
            // for Atlas — and draining here would lose them for good.
            let waiting = self.outbox.ready(t, &cfg);

            // The structured list `returning.rs` was designed around.
            //
            // This path used to build a pre-formatted string here --
            // `Journal::brief` for what Atlas did, `notify::spoken` for what
            // could not reach you, concatenated -- which is why
            // `returning::welcome` and `returning::full_brief` sat complete,
            // tested and unreachable for as long as they did: they take a
            // `[Happened]`, and there was nowhere in the running program
            // that had one. Assembling the list first, and letting
            // `returning` decide what to say about it, is what actually
            // wires them in. The two decisions that used to live inline
            // below -- whether a quiet overnight still earns a line, and
            // whether to lead with the greeting -- are `welcome`'s own, and
            // are no longer duplicated here.
            let happened = self.happened_while_away(away_since, &waiting);

            // Your hour, on your clock: the `time_zone` you chose, or this
            // machine's own (`tz::home`). Both chats fixed this separately
            // ("Good morning" arrived at midnight in Pacific time).
            let hour = self.home_zone().hour(t as i64);
            let rcfg = self.returning_cfg();
            let welcome = crate::returning::welcome(gone, &happened, hour, &rcfg);
            // What you were in the middle of: the cue that shortens getting
            // back into it (Trafton & Monk 2007: an explicit reminder of
            // where you were beats none; a subtle one does no better).
            let cue = if self.worklog_cfg().enabled && self.worklog_cfg().resume_cue {
                self.worklog.last_context(away_since).filter(|c| c.until + 600 >= away_since).map(|c| c.cue())
            } else {
                None
            };

            // The brief goes on screen too, so you can read it rather than
            // having to catch it. It does not steal focus -- only an urgent
            // panel does that. What is on the panel is now the same list
            // `welcome` reasoned about, rather than a separate assembly
            // that could drift from what was said out loud.
            let lines: Vec<String> = happened.iter().map(|h| h.what.clone()).collect();
            self.show_panel(crate::window::Panel::Brief, "While you were away", lines);

            // `returning::welcome` leans on the greeting to carry "you have
            // just come back" -- with a name set, "Morning, Eric. Disk is
            // nearly full." says it on its own. But the default address is
            // deliberately nothing at all ("a system that calls you 'sir'
            // uninvited is doing a bit"), and without a greeting a bare
            // "Disk is nearly full." is indistinguishable from something
            // happening right now. That distinction is the entire point of a
            // returning brief: not what happened, but that it happened while
            // you were not looking. The framing is restored here rather than
            // inside `returning.rs` because being on the return path is the
            // daemon's knowledge, not the phrasing layer's.
            let unaddressed =
                rcfg.how_to_address() == crate::returning::Address::None && !happened.is_empty();
            let frame = |s: &String| {
                if unaddressed {
                    format!("While you were away: {s}")
                } else {
                    s.clone()
                }
            };

            self.pending_brief = match &welcome {
                crate::returning::Welcome::Straight(s) => Some(frame(s)),
                crate::returning::Welcome::Offer(s) => Some(frame(s)),
                // `returning` calls a sub-`away_after` absence no absence at
                // all and says nothing, which is right about the journal and
                // wrong about the outbox: something held back *because* you
                // were not there is owed either way, and `away_after` is
                // configurable low enough for the two to genuinely disagree
                // (anything under fifteen minutes reads as `Gone::Moment`).
                // Held things are still said; everything else stays quiet.
                crate::returning::Welcome::Nothing if !waiting.is_empty() => {
                    Some(crate::notify::spoken(&waiting))
                }
                crate::returning::Welcome::Nothing => None,
            };
            // A welcome back is a hello: the part-of-day greeting after it
            // would be a second one (`returning::hello_now`).
            if self.pending_brief.is_some() {
                self.last_greeted_at = t;
                let _ = self.store.save("last_greeted_at", &self.last_greeted_at);
            }
            if let Some(cue) = cue {
                self.pending_brief = Some(match self.pending_brief.take() {
                    Some(b) => format!("{b} {cue}"),
                    None => cue,
                });
            }

            // An offer is only an offer if "yes" reaches something. Held so
            // the next turn's bare yes runs `full_brief` against this exact
            // list, rather than against whatever has happened since.
            self.pending_brief_detail = match welcome {
                crate::returning::Welcome::Offer(_) => Some(happened),
                _ => None,
            };
        }
        self.awareness.heard_you(t);
        let said = said.trim();
        if said.is_empty() {
            return String::new();
        }

        // Asking Atlas for a new ability (`growth`), and "what broke <test>"
        // (`bisect`). First, as you said it (5 Oct 2026): above everything
        // that would act on a word inside the request ("I want you to be
        // able to send texts" sending a text), and above the phrase book,
        // which rewrote a second wording of one to "be able to send texts".
        if let Some(reply) = (if self.handover().stance.handed_over() { None } else { self.ability_request(said, t) }).or_else(|| self.what_broke_asked(said, t)) {
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }

        // "Stop" while Atlas is working an app ends that job, there and then.
        if self.operating.is_some() {
            let l = said.to_lowercase();
            let l = l.trim().trim_end_matches(['.', '!']);
            if matches!(l, "stop" | "stop that" | "stop it" | "cancel" | "cancel that" | "never mind" | "stop working on that") {
                if let Some(s) = self.stop_operating() {
                    return s;
                }
            }
        }
        let canceled_reminder = matches!(hear(said), Some(Heard::Cancel)) && self.cancel_pending_reminder_edit();
        let canceled_brief = matches!(hear(said), Some(Heard::Cancel | Heard::Panic)) && self.explicitly_cancel_brief_preparation(t);
        // Pause and resume are heard before anything else, so they work even
        // mid-task and mid-sentence.
        if self.active_file_move_id().is_some() || self.approved_undo.is_some() {
            let lower = said.to_lowercase();
            let plain = lower.trim().trim_end_matches(['.', '!']);
            if matches!(plain, "stop" | "stop that" | "stop it" | "cancel" | "cancel that" | "never mind" | "stop working on that") {
                let undo = self.cancel_approved_undo();
                if self.active_file_move_id().is_none() && undo {
                    return if canceled_reminder { "Canceled the waiting undo and reminder change. No restore was performed; the reminder change was not saved.".into() } else { "Canceled the waiting undo. No restore was performed.".into() };
                }
                self.stop_files();
                return if canceled_reminder { "Stopping the file moves and canceled the waiting reminder change, which was not saved. Unfinished copies keep their originals; I will report the result.".into() } else { "Stopping the file moves. Unfinished copies keep their originals, and remaining files will stay in place. I'll report the result.".into() };
            }
        }
        if canceled_reminder {
            return "Canceled the waiting reminder change. It was not saved.".into();
        }
        match hear(said) {
            // "stop everything" / "halt" / "emergency stop" / "drop
            // everything". THIS WAS NOT HANDLED, and it is the one phrase
            // Atlas teaches you by name: `firstrun.rs:181` ends setup with
            // 'Say "what can you do" any time, "stop everything" if I get
            // something wrong.'
            //
            // `attention::hear` recognised it and returned `Heard::Panic`;
            // this match had no arm for it, so it fell through `_ => {}` and
            // carried on to ordinary parsing, where it means nothing.
            // `Attention::halt` -- which empties the queues, abandons the
            // work and sets `halted` so a later resume does not silently
            // restart it -- had **zero callers in src/**. Built, tested,
            // taught to the user, and wired to nothing.
            //
            // A check that is performed and whose result is discarded is
            // worse than no check: it reads as covered.
            // `attention::halt` is the capability built for this and is what
            // `was_halted()` reads so a later resume does not silently
            // restart abandoned work. And it now reaches the crew too: the
            // note that used to sit here said the daemon "does not keep the
            // ids of what is running" -- it does, `self.crew_links` is
            // iterated for exactly that answer in `Intent::Queued` -- and
            // `ask_everyone_to_stop` needs no ids at all. So "stop
            // everything" finally stops an errand already in flight, not
            // just Atlas asking and starting things. The endings still
            // arrive through `settle`, reported rather than swallowed.
            Some(Heard::Panic) => {
                let canceled_reminder = self.cancel_pending_reminder_edit();
                // A job in an app ends where it is.
                // unheard-ok: returns `Option<String>`, not a Result
                let _ = self.stop_operating();
                // A model turn still thinking is stopped too: its answer
                // would otherwise run its tool when it came back (28 Sep 2026).
                self.drop_pending_turn(t, "Stopped before I answered -- nothing was done.");
                // A request of several steps ends at its next step.
                self.stop_task_loop();
                self.stop_files();
                self.cancel_approved_undo();
                self.cancel_calendar_delivery();
                let asked = self.crew.ask_everyone_to_stop();
                let mut halted = self.attention.halt(t);
                if canceled_reminder { halted.push_str(" Canceled the waiting reminder change; it was not saved."); }
                // `halt`'s own doc says "queues emptied, work abandoned" —
                // and nothing ever emptied this queue, so a later resume
                // quietly restarted the exact work you panicked about.
                // Keyed to `was_halted`, the flag that says a panic
                // happened, not to this arm's position in the file.
                if self.attention.was_halted() {
                    self.queue.tasks.retain(|task| {
                        matches!(
                            task.state,
                            crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed
                        )
                    });
                }
                return if asked > 0 {
                    format!(
                        "{halted} Asked {asked} running errand{} to stop too.",
                        if asked == 1 { "" } else { "s" }
                    )
                } else {
                    halted
                };
            }
            Some(Heard::Pause) => {
                // What was mid-flight is recorded, so resume can name it.
                // `suspended` had no writer: `release()` returned an empty
                // vec at every call site since the day it was written.
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
                self.drop_pending_turn(t, "Paused before I answered -- nothing was done.");
                // A request of several steps holds before its next step.
                self.hold_task_loop(true);
                self.pause_files(true);
                // "Pause" is total: the errands hold too, at their safe
                // points, and nothing they have done is lost.
                let mut msg = self.attention.pause(self.current_work(), t);
                if let Some(status) = self.file_pause_status() { msg.push(' '); msg.push_str(status); }
                let held = self.hold_every_errand();
                return if held > 0 && msg != "Already paused." {
                    format!(
                        "{msg} Holding {held} errand{} where {} — nothing lost.",
                        if held == 1 { "" } else { "s" },
                        if held == 1 { "it is" } else { "they are" }
                    )
                } else {
                    msg
                };
            }
            // "I'm ready" only means resume if something was paused.
            // Otherwise it's you arriving at the desk, which is a different
            // thing entirely.
            Some(Heard::Resume) if self.attention.is_paused() => {
                let m = self.attention.resume(t);
                self.pause_files(false);
                for id in std::mem::take(&mut self.held_by_pause) {
                    self.crew.resume(id);
                }
                // What was suspended at the pause, named on the way back —
                // the return value both call sites used to discard.
                let held = self.attention.release();
                let names: Vec<String> = held
                    .iter()
                    .filter_map(|id| {
                        self.queue.tasks.iter().find(|x| x.id == *id).map(|x| x.command.clone())
                    })
                    .collect();
                let m = if names.is_empty() {
                    m
                } else {
                    format!("{m} Still in hand: {}.", names.join(", "))
                };
                // If a reply was cut off mid-sentence, "carry on" finishes
                // it — that is what the parking in `say_interruptibly` was
                // for, and until now nothing ever asked for the remainder.
                return match self.finish_saying() {
                    Some(rest) => format!("{m} {rest}"),
                    None => m,
                };
            }
            // "Carry on" with nothing paused still finishes an interrupted
            // reply — stopping Atlas mid-answer does not pause the world,
            // so the remainder must be reachable outside a pause too.
            Some(Heard::Resume) if self.unsaid.is_some() => {
                if let Some(rest) = self.finish_saying() {
                    return rest;
                }
            }
            Some(Heard::Status) => return self.attention.status(t),
            _ => {}
        }
        // "Stop" with several errands going: which one. Above the pause gate
        // so an errand can be picked back up while Atlas itself is paused.
        if let Some(reply) = self.errand_control(said, t) {
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }

        // Let a bare stop choose among running errands before consuming it as
        // a calendar-delivery cancellation. Otherwise a pending reminder can
        // hide the actual two-job choice from the owner.
        if matches!(hear(said), Some(Heard::Cancel)) && self.cancel_calendar_delivery() {
            return "Stopped pending calendar booking and reminder delivery. Any event already saved remains on the calendar; unconfirmed delivery will not repeat automatically.".into();
        }
        if canceled_brief && matches!(hear(said), Some(Heard::Cancel)) {
            return "Canceled the brief preparation. Nothing was marked delivered; I won't offer it again today unless you ask.".into();
        }

        // While paused, nothing else gets through — asked of `allows`,
        // the predicate written for exactly this, rather than a second
        // inline reading of the same rule. (Resume and Status returned in
        // the match above; `allows` is what keeps this line and that match
        // telling the same story.)
        if !self.attention.allows(hear(said)) {
            return String::new();
        }

        // While dictating, what you say is content, not commands.
        //
        // This sits above the addressing check deliberately. "Was that meant
        // for Atlas?" is the right question for a wake-word turn and the
        // wrong one here: you started dictation, so everything until you stop
        // it is meant to be typed. Below the pause check, because stopping
        // has to work from inside any mode.
        if self.dictation.is_some() {
            return self.dictated(said, t);
        }

        // Was that even meant for Atlas? Abandoning work because someone
        // walked in is worse than missing one instruction.
        let situation = Situation {
            awaiting_answer: self.session.is_waiting(),
            working: !self.scheduler.due(t).is_empty() || self.queue_is_busy(),
            after_wake_word: how == Arrival::Directed,
            // Atlas spoke to you within the last half minute: this is the
            // conversation carrying on, even when it's about "him" or "them".
            just_spoke: how == Arrival::OpenMic
                && self.last_spoke_at != 0
                && t.saturating_sub(self.last_spoke_at) <= crate::addressing::STILL_TALKING_SECS,
            other_voice: how == Arrival::OpenMic && matches!(self.last_verdict, crate::voiceid::Verdict::NotYou(_)),
            ..Default::default()
        };
        let judged = assess(said, &situation);
        match respond(&judged, &situation) {
            // Silence is never the answer to something you deliberately sent.
            // Even when Atlas genuinely cannot tell, saying so beats saying
            // nothing -- an assistant that ignores you is indistinguishable
            // from one that has crashed.
            Addressed::Ignore if how == Arrival::Directed => {}
            Addressed::Ignore => return String::new(),
            // Another voice answering: the question you were asked stays
            // open for you, so nothing is asked in its place.
            Addressed::Ask(q) if situation.other_voice => return q,
            Addressed::Ask(q) => {
                self.session.ask(&q);
                return q;
            }
            Addressed::Act => {}
        }

        // Some things are not an assistant's to have a go at. A message that
        // reads as a crisis is caught here -- after "was that meant for me?"
        // says yes, and before any mode, saved flow, parser or model can treat
        // it as a command. `person::beyond_me` is the deliberately narrow test
        // for exactly this ("most difficult conversations are just difficult
        // conversations"), and it reached nothing: a line like "I can't go on"
        // fell through to ordinary parsing, where the best case was a blank
        // `Unknown` and the worse case was a phrase-match on the wrong thing.
        // Redirecting to a person -- while still offering to take things off
        // your plate -- is the one right move, and it wins over everything
        // else Atlas might otherwise do with the words.
        if crate::person::beyond_me(said) {
            return crate::person::NOT_A_THERAPIST.to_string();
        }

        // A hard day that is not a crisis. Caught here for the same reason
        // `beyond_me` is -- before any mode, flow, parser or model can treat
        // "rough day, honestly" as a command and search the disk for a file
        // called "rough". The right move is not to redirect and not to
        // perform concern: it is to offer to take real work off your plate,
        // and the things it offers are the outstanding backlog items, the
        // concrete work Atlas could actually pick up rather than an empty
        // gesture. With nothing outstanding it says little and does not
        // pretend otherwise -- which is exactly what `hard_day([])` is for.
        if crate::person::having_a_hard_time(said) {
            let can_take_on: Vec<String> = self
                .backlog
                .outstanding()
                .iter()
                .map(|i| i.request.trim().to_string())
                .collect();
            let reply = crate::person::hard_day(&can_take_on);
            self.thread.append(said, &reply, Some("hard day".to_string()), t);
            self.persist();
            return reply;
        }

        // How you talk (2 Oct 2026, `learning`): "what have you learned
        // about how I talk", "forget that phrase", "what did you
        // misunderstand this week" -- and "no, I meant ...", which runs the
        // words you meant and learns the ones that missed.
        if let Some(reply) = self.how_you_talk_turn(said, t, how) {
            self.persist();
            return reply;
        }

        // "What are you working on": every stream of work, by name
        // (30 Sep 2026, `streams`).
        if crate::streams::asks_what_youre_working_on(said) {
            let reply = self.what_im_working_on();
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }

        // "Use the better model" / "use the faster model" (30 Sep 2026,
        // `deepbrain`): which model talks with you.
        if let Some(better) = crate::deepbrain::asks_for_talk_model(said) {
            let reply = self.choose_talk_model(better);
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }

        // A named mode wins before anything else parses it as a command.
        if let Some(m) = self.modes.match_trigger(said).map(|m| m.name.clone()) {
            if self.modes.active().map(|a| a.name != m).unwrap_or(true) {
                if let Some(tr) = self.modes.enter(&m, &[]) {
                    self.thread.append(said, &tr.say, Some(format!("{m} mode")), t);
                    self.persist();
                    return tr.say;
                }
            }
        }

        // A saved sequence you named earlier.
        //
        // Run as a `flow::Run`, not flattened into the queue. The flatten
        // shape discarded `on_fail` and `produces` entirely — an optional
        // step stopped the chain, a retrying one never retried, and a step
        // using `{name}` from an earlier one ran with the braces still in
        // it. The run also gives the mind something true to say: this is
        // the first place `Mind` has ever been handed work, which is why
        // "what are you doing?" could only ever answer "nothing".
        if let Some(w) = self.flows.match_trigger(said).map(|w| w.name.clone()) {
            if let Some(f) = self.flows.get(&w).cloned() {
                return self.start_flow(f, None, Some(said), t);
            }
        }

        // An add-on's sequence (`plugins`). After your own, so one you saved
        // always wins over one an add-on declared -- and never while Atlas is
        // waiting on an answer from you, so nothing you say to answer it can
        // start an add-on instead.
        if matches!(self.session.pending, Pending::Nothing) {
            // Kept between turns, read again only when the folder changes.
            let found = crate::plugins::Registry::load_kept(&mut self.plugins_kept, &self.plugins_dir, &self.cfg.commands, &self.store)
                .match_trigger(said);
            if let Some((id, f)) = found {
                return self.start_flow(f, Some(id), Some(said), t);
            }
        }

        // Being spoken to like a person, answered like one.
        //
        // This sits above reference resolution, which is where it has to be:
        // "you there" contains a pronoun, so the resolver claimed it and asked
        // "Which one?". It is also above the policy gate, because greeting
        // Atlas is not an action that needs approving -- below it, "hello"
        // came back as "I didn't catch that. Go ahead?", which treats a
        // greeting as a failed command.
        //
        // Checked against what was said rather than a parsed intent: the
        // phrase parser matches loosely enough that half of these never reach
        // `Intent::Unknown`. The word cap and exact-match lists in
        // `social_reply` are what stop a real instruction being swallowed as a
        // pleasantry.
        if let Some(reply) = self.persona_now().social(said, crate::localclock::hour_here(t), t, self.last_turn_failed || self.mid_flow()) {
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }

        // "Argue the other side" -- above reference resolution for the same
        // reason: "argue the other side" names "the other" and the resolver
        // claimed it and asked "Which one?". Only an explicit request
        // (`otherside::is_asked_for`) reaches it, and never for a guest.
        if !self.handover().stance.handed_over() {
            if let Some(reply) = self.other_side(said) {
                self.thread.append(said, &reply, None, t);
                self.persist();
                return reply;
            }
        }

        // "Make it faster" just after an animation: "it" is that animation.
        // Above reference resolution, which would otherwise ask "Which one?"
        // about a referent that is plain from what just happened.
        if !self.handover().stance.handed_over() {
            if let Some(reply) = self.refine_animation(said, t) {
                self.thread.append(said, &reply, None, t);
                self.persist();
                return reply;
            }
        }

        // "Start that research", "do the research I asked for": the research
        // asked for last, run now -- not a chat about research (29 Sep 2026:
        // three of these went to the model, which said "I'm already on it"
        // and started nothing). With nothing asked yet, it says how to ask.
        // "Take that off my outstanding list" (1 Oct 2026: there was no way).
        if let Some(removal) = crate::backlog::removal_asked(said) {
            // Matched against everything the Outstanding page can take off,
            // in its order, not only the backlog (2 Oct 2026), and removed
            // the one way the page's buttons remove (`drop_outstanding`).
            let listed = self.outstanding_removable(t);
            // "Clear my outstanding list" clears the list; it doesn't stop
            // work a worker is in the middle of. Those are stopped by name,
            // or with their own Stop it.
            let listed: Vec<(String, String)> = match removal {
                crate::backlog::Removal::All => listed.into_iter().filter(|(k, _)| !k.starts_with("e:")).collect(),
                _ => listed,
            };
            let titles: Vec<String> = listed.iter().map(|(_, title)| title.clone()).collect();
            let reply = match crate::backlog::pick_for_removal(&removal, &titles) {
                Ok(at) => {
                    let mut off = Vec::new();
                    let mut refused = Vec::new();
                    for n in at {
                        match self.drop_outstanding(&listed[n].0, t) {
                            Ok(o) => off.push(o),
                            Err(why) => refused.push(why),
                        }
                    }
                    let unsaved = off.iter().find_map(|o| o.unsaved.clone());
                    let mut said = match (off.as_slice(), &unsaved) {
                        (_, Some(e)) => format!("I took it off, but couldn't save the list ({e}) -- it may come back after a restart."),
                        ([], None) => String::new(),
                        ([one], None) if one.stopping => format!("Asked \"{}\" to stop -- it's off your outstanding list once it winds down.", one.title),
                        ([one], None) => format!("Done -- \"{}\" is off your outstanding list.", one.title),
                        (many, None) => format!("Done -- cleared {} things off your outstanding list.", many.len()),
                    };
                    for why in refused {
                        if !said.is_empty() {
                            said.push(' ');
                        }
                        said.push_str(&why);
                    }
                    said
                }
                Err(say) => say,
            };
            self.thread.append(said, &reply, None, t);
            self.persist();
            return reply;
        }
        let research_again;
        // "research it again", "look that up again": the topic is the last
        // one, not the word "it" (30 Sep 2026: searched for "it").
        let only_a_pronoun = |a: &str| {
            let rest: Vec<&str> = a
                .split_whitespace()
                .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
                .filter(|w| !matches!(*w, "" | "again" | "more" | "further" | "please" | "up" | "on"))
                .collect();
            !rest.is_empty() && rest.iter().all(|w| matches!(*w, "it" | "that" | "this" | "them" | "those"))
        };
        let parsed_now = self.parser.parse(said);
        let research_of_it = matches!(&parsed_now, Intent::Research(a) if only_a_pronoun(&a.to_lowercase()));
        let said = if research_of_it || (matches!(parsed_now, Intent::Unknown(_)) && crate::references::starts_the_research(said)) {
            match self.referents.last_topic.clone() {
                Some(topic) => {
                    // "again" kept: it asks for a fresh look, not the
                    // answer from last time (`research_from_request`).
                    let again = if said.to_lowercase().split_whitespace().any(|w| w.trim_matches(|c: char| !c.is_alphanumeric()) == "again") { " again" } else { "" };
                    research_again = format!("research {topic}{again}");
                    research_again.as_str()
                }
                None => {
                    let reply = "I haven't been given a topic yet. Say \"research\" and then what to look into, and I'll start on it.".to_string();
                    self.thread.append(said, &reply, None, t);
                    self.persist();
                    return reply;
                }
            }
        } else {
            said
        };

        // "close it" -> "close chrome", or a question if there is no referent.
        //
        // Only when the phrase alone doesn't already say what to do. "undo
        // that" contains a pronoun but is complete on its own, and asking
        // "which one?" about it would be absurd.
        // "Complete" means more than "it parsed". "close it" parses fine and
        // then tries to close an app called "it", so an intent whose argument
        // is itself a pronoun still needs resolving.
        let parsed = self.parser.parse(said);
        let already_clear = !matches!(parsed, Intent::Unknown(_))
            && !argument_of(&parsed)
                .map(|a| crate::references::argument_leans_on_earlier(&a))
                .unwrap_or(false);
        // An answer to a question Atlas asked in its own words ("change it
        // to …", a decision's next move) is that answer, pronouns and all.
        let answering = matches!(self.session.pending, Pending::Clarification(_))
            && (self.pending_post_approval.is_some() || self.pending_post_when.is_some() || self.pending_decision.is_some());
        let said_owned;
        let said = if already_clear || answering {
            said
        } else {
            match resolve(said, &self.referents) {
            Resolution::Unchanged(t) => {
                said_owned = t;
                said_owned.as_str()
            }
            // Only when it makes a command of it ("close it" -> "close
            // chrome"). Free conversation keeps your exact words (1 Oct 2026:
            // "Why is it ..." reached the model, and the thread, as "Why is
            // chrome ...", "say it again" as "save chrome again" -- every
            // sentence after Atlas had once opened an app).
            Resolution::Resolved { text, .. } if matches!(self.parser.parse(&text), Intent::Unknown(_)) => {
                let _ = text;
                said
            }
            // A long sentence's "it" is its own (1 Oct 2026: "research how an
            // AI system can add its own features when it gets approval"
            // reached Atlas as "... when <the last topic> gets approval").
            // Only a short command leans on what came before: "close it",
            // "move it to the other screen".
            Resolution::Resolved { .. } if crate::references::word_count(said) > 6 => said,
            Resolution::Resolved { text, .. } => {
                said_owned = text;
                said_owned.as_str()
            }
            // Not understood as a command either way: "which one?" would
            // be a question about nothing ("I'm so tired of this" got "Which
            // one?", 26 Sep 2026). Taken as said.
            Resolution::Ambiguous(_) if matches!(parsed, Intent::Unknown(_)) => said,
            Resolution::Ambiguous(q) => {
                self.session.ask(&q);
                return q;
            }
            }
        };

        // A parked approval takes precedence: the next thing you say is an
        // answer, not a new command.
        if let Pending::Approval(intent, description) = self.session.pending.clone() {
            // Several waiting (30 Sep 2026), answered together: "yes to
            // both", "no to the second".
            let waiting = self.session.approvals_waiting();
            if let Some(answers) = crate::session::answers_for_several(said, waiting) {
                return self.answer_several(said, answers, t);
            }
            let kind = kind_of(&intent).to_string();
            // A question you were asked is not a question anyone else may
            // answer. Atlas asks "go ahead?", you hand the laptop over, and
            // the next person says "yes" -- so the answer is dropped along
            // with the question.
            //
            // Ahead of `record_approval` on purpose: a yes recorded here
            // becomes a standing grant, and a standing grant is how the
            // *next* one of these gets waved through without being asked at
            // all. A stranger's yes must not teach Atlas anything about what
            // you approve of.
            if let Some(refusal) = self.handed_over_refusal(&intent) {
                self.abandon_dependent_question();
                self.session.drop_approvals();
                self.pending_job = None;
                return refusal;
            }
            // A permission question is answered with a breadth, not just a
            // yes: "yes, always" records an `Always` grant, "just this
            // session" a `Session` one, a bare "yes" a `Once`. Recorded
            // BEFORE the action runs, so the gate's re-check finds the grant
            // and does not ask the same question again. Any app action the
            // grants gate parked comes back through here.
            if let Some((app, action)) = app_action_of(&intent) {
                // "Allow the camera?" answered "I allow the camera", or
                // heard as "I love the camera" (Eric, 1 Oct 2026): neither
                // opens with a yes, so the answer was taken as something
                // new, nothing was kept, and the question came back on every
                // look. The question was "may I look whenever you ask", so a
                // yes to it is kept for good.
                let camera_yes = matches!(intent, Intent::CaptureWebcam) && crate::camera_ask::allows(said);
                let span = if camera_yes { Some(crate::grants::Span::Always) } else { crate::grants::span_from_answer(said) };
                if let Some(span) = span {
                    self.permissions.grant(&app, Some(&action), span, t);
                    let _ = self.store.save("permissions", &self.permissions);
                    // A watch was what was asked for: it starts now.
                    if matches!(intent, Intent::CaptureWebcam) {
                        if let Some((secs, until_stopped)) = self.watch_after_allow.take() {
                            self.session.pending = Pending::Nothing;
                            let reply = self.start_watching(secs, until_stopped, t);
                            self.session.record(said, &intent, &reply);
                            self.persist();
                            return reply;
                        }
                    }
                    self.session.pending = Pending::Nothing;
                    self.memory.record_approval(&kind, true, None);
                    let reply = self.execute_approved_step(&intent);
                    self.session.record(said, &intent, &reply);
                    let reply = self.and_the_next_approval(reply);
                    self.persist();
                    return reply;
                }
                // Not an affirmative answer — fall through to the ordinary
                // no-handling below, which learns from the refusal.
            }
            let yes = is_yes(said);
            self.session.pending = Pending::Nothing;
            // A yes or a no answers it. Anything else is something new: the
            // question is dropped and what you said is taken as itself. It
            // used to be "Left it alone." -- a no -- and the new request was
            // lost with the question (27 Sep 2026).
            if yes || is_no(said) {
                if !yes { self.decline_dependent_approval(&intent, "Approval declined; dependent steps were not run."); }
                self.memory.record_approval(&kind, yes, None);
                let reply = if yes {
                    let r = self.execute_approved_step(&intent);
                    // If this was a scheduled job asking, close it out too.
                    if let Some(jid) = self.pending_job.take() {
                        self.scheduler.approve(jid);
                        self.scheduler.complete(jid, t, &r, !r.starts_with("error"));
                    }
                    r
                } else if let Some(own) = crate::coding_agent::declined(&intent) {
                    // A no to handing it to a coding agent is a no to the
                    // agent, not to the work: Atlas writes it itself (2 Oct
                    // 2026).
                    self.pending_job = None;
                    self.execute(&own)
                } else {
                    self.pending_job = None;
                    // Worth learning once, per its own doc comment -- not a rule
                    // inferred from a single no, just a pattern that starts
                    // counting. `notice` is the same dedup-by-text used for every
                    // other trait, so saying no to the same kind of thing twice
                    // is what makes it `confident()`, not this one refusal alone.
                    let (what, k) = crate::person::learn_from_refusal(&description);
                    self.person.notice(&what, k, t);
                    "Left it alone.".into()
                };
                self.session.record(said, &intent, &reply);
                // The next approval waiting, asked now.
                let reply = self.and_the_next_approval(reply);
                self.persist();
                return reply;
            }
            self.pending_job = None;
            // Something new instead of an answer: everything that was
            // waiting is dropped with the question.
            self.abandon_dependent_question();
            self.session.queued.clear();
        }

        // A workflow paused mid-chain for your yes.
        //
        // Checked before the offer path: a flow awaiting approval and a
        // pending offer are never on the table at once, but if they ever
        // were, the flow asked most recently and the answer belongs to it.
        if let Pending::Clarification(_) = self.session.pending.clone() {
            let awaiting = self
                .current_flow
                .as_ref()
                .is_some_and(|r| r.state == crate::flow::RunState::AwaitingApproval);
            if awaiting {
                self.session.pending = Pending::Nothing;
                // "Always": a yes, and for an add-on's step, a standing one --
                // you decided once, so you are not asked again every run.
                let always = crate::session::is_always(said);
                let mut trusted_note = String::new();
                if always {
                    if let Some(run) = self.current_flow.as_ref() {
                        if let (Some(id), Some(step)) = (run.plugin.clone(), run.current().map(|s| s.command.clone())) {
                            trusted_note = match crate::plugins::trust_step(
                                &self.store,
                                &self.plugins_dir,
                                &self.cfg.commands,
                                &id,
                                &step,
                            ) {
                                Ok(_) => format!("I won't ask about \"{step}\" again. "),
                                Err(why) => format!("{why} "),
                            };
                        }
                    }
                }
                if is_yes(said) || always {
                    // Approval brings the job back to the front, wherever an
                    // intervening request pushed it.
                    self.mind.promote(self.flow_mind);
                    // The yes covers THE STEP IT WAS ASKED ABOUT. Handing it
                    // back to the policy gate would classify the same step
                    // into the same question, forever.
                    self.approved_flow_step(t);
                    let reply =
                        self.drive_flow(t).unwrap_or_else(|| "Carrying on.".into());
                    let reply = format!("{trusted_note}{reply}");
                    self.persist();
                    return reply;
                }
                if let Some(run) = self.current_flow.as_mut() {
                    // Refused mid-chain: the rest is abandoned rather than
                    // half-done, which is `deny`'s whole contract.
                    run.deny();
                }
                let reply = self
                    .drive_flow(t)
                    .unwrap_or_else(|| "Alright, leaving the rest.".into());
                self.persist();
                return reply;
            }
        }

        // A pending offer works the same way.
        let mut offer_dropped = false;
        if let Pending::Clarification(q) = self.session.pending.clone() {
            if let Some(offer) = self.pending_offer.clone() {
                let yes = is_yes(said);
                self.session.pending = Pending::Nothing;
                self.pending_offer = None;
                if yes {
                    self.proactive.record_response(&offer.kind, yes, &mut self.memory);
                    let reply = self.run_command(&offer.command, t);
                    self.persist();
                    return reply;
                }
                if is_no(said) {
                    self.proactive.record_response(&offer.kind, false, &mut self.memory);
                    self.persist();
                    return "Alright.".into();
                }
                // Neither: the offer is dropped unanswered and this is taken
                // as a new request ("want me to look it up?" -- "what about
                // Tuesday?"), rather than read as a no and lost.
                offer_dropped = true;
            }
            // A decision being worked (H9): the answer goes to its move.
            if let Some(waiting) = self.pending_decision.take() {
                self.session.pending = Pending::Nothing;
                let l = said.trim().to_lowercase();
                if ["stop", "cancel", "never mind", "nevermind", "forget it", "not now", "later"].iter().any(|w| l == *w || l.starts_with(&format!("{w} "))) {
                    return "Alright, I've put the decision aside. \"Back to the decision\" picks it up where we were.".into();
                }
                return match waiting {
                    None if is_yes(said) => self.deciding.as_ref().and_then(|d| d.lean().ok()).map(|l| l.written()).unwrap_or_else(|| "There's no working to show yet.".into()),
                    None => "Alright.".into(),
                    Some(m) => {
                        if let Some(d) = self.deciding.as_mut() {
                            d.take_answer(m, said);
                        }
                        self.say_the_decision()
                    }
                };
            }
            // A dropped task: back on the list on a yes (H8).
            if let Some(title) = self.pending_bring_back.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, it stays dropped.".into();
                }
                return self.bring_back(&title, t);
            }
            // A file the scan couldn't check: open it anyway only on a yes (H3).
            if let Some((what, path)) = self.pending_unscanned.take() {
                self.session.pending = Pending::Nothing;
                return self.answer_scan_decision(&what, &path, is_yes(said));
            }
            // An edited video: keep it, and then the original (G8).
            if let Some((original, copy, result)) = self.pending_media_keep.take() {
                self.session.pending = Pending::Nothing;
                crate::heard!(std::fs::remove_file(&copy));
                if !is_yes(said) {
                    crate::heard!(std::fs::remove_file(&result));
                    self.finish_media_decision(false);
                    return "Alright — I've thrown the edit away. Your original is untouched.".into();
                }
                // Removing an original is the consequential kind of media
                // step whoever does it (`media_decision`), so it's asked.
                let d = crate::categories::media_decision(crate::categories::MediaOp::Export, false, true);
                if d.needs_consent() {
                    let q = format!("Kept. Do you want me to get rid of the original ({original})?");
                    self.session.ask(&q);
                    self.pending_media_original = Some(original);
                    return q;
                }
                self.finish_media_decision(true);
                return "Kept.".into();
            }
            if let Some(original) = self.pending_media_original.take() {
                self.finish_media_decision(true);
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, the original stays.".into();
                }
                return match self.trash.take(std::path::Path::new(&original), "replaced by the edited version, on your say-so") {
                    Ok(_) => "Done — the original is in my trash, so \"undo\" brings it back while it's there.".into(),
                    Err(e) => format!("I couldn't remove the original: {e}. It's still where it was."),
                };
            }
            // "Undo X?" — yes carries it out (G6).
            if let Some(id) = self.pending_undo.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Left as it is.".into();
                }
                return self.carry_out_undo(id);
            }
            // A folder's sorting plan, carried out on a yes (29 Sep 2026;
            // any folder since 2 Oct 2026).
            if let Some(plan) = self.pending_desktop.take() {
                self.session.pending = Pending::Nothing;
                if is_no(said) {
                    return "Alright, I left everything where it is.".into();
                }
                if !is_yes(said) {
                    return self.turn_from(said, t, how);
                }
                return self.carry_out_sorting(plan, t);
            }
            // An optimization run's offer: all of it, on one yes.
            if let Some(plan) = self.pending_optimize.take() {
                self.session.pending = Pending::Nothing;
                if is_no(said) {
                    return "Alright, I left everything as it is.".into();
                }
                // Anything else is a new request, not an answer: it's heard
                // as one (1 Oct 2026: a different question was taken as "no").
                if !is_yes(said) {
                    return self.turn_from(said, t, how);
                }
                return self.carry_out_optimize(plan, t);
            }
            // Moving big folders to another drive (G5).
            if let Some(plan) = self.pending_storage.take() {
                self.session.pending = Pending::Nothing;
                if is_no(said) {
                    return "Alright, nothing moved.".into();
                }
                if !is_yes(said) {
                    return self.turn_from(said, t, how);
                }
                return self.carry_out_storage_plan(plan, t);
            }
            // A job in an app waiting on you (`operate`).
            if let Some(answer) = self.operate_answer(said) {
                return answer;
            }
            // A button that can't be undone (G4).
            if let Some((win, name, app)) = self.pending_press.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, I didn't press it.".into();
                }
                return self.press_now(crate::platform::WindowId(win), &name, &app);
            }
            // A post's approval: yes, then when (G2). The approval is only
            // given once there's a time, so a yes never posts on the spot.
            if let Some(id) = self.pending_post_approval.take() {
                self.session.pending = Pending::Nothing;
                // "Change it to …": your edit of the words, asked about again,
                // and learned from (H13g).
                let l = said.to_ascii_lowercase();
                if let Some(new) = ["change it to ", "make it read ", "make it ", "change the words to "]
                    .iter()
                    .find_map(|p| l.find(p).map(|i| said[i + p.len()..].trim().trim_matches('"').to_string()))
                    .filter(|n| !n.is_empty())
                {
                    let before = self.publisher.get(id).map(|p| p.body.clone()).unwrap_or_default();
                    let previous_publisher = self.publisher.clone();
                    if self.publisher.edit(id, &new) {
                        if let Some(q) = self.publisher.request_approval(id) {
                            if let Err(e) = self.publisher.save(&self.store) { self.publisher = previous_publisher; return format!("Those edited words couldn't be saved ({e}); review the previous draft before approving it."); }
                            self.learned_from_your_edit(&before, &new, t);
                            self.session.ask(&q);
                            self.pending_post_approval = Some(id);
                            return q;
                        }
                    }
                    return "I couldn't change that one.".into();
                }
                // "Attach C:\...\photo.jpg": a picture or video goes with it,
                // and the approval is asked again with it named.
                if let Some(path) = crate::publish::file_to_attach(said) {
                    if !std::path::Path::new(&path).is_file() {
                        self.session.ask("Post it?");
                        self.pending_post_approval = Some(id);
                        return format!("I can't find {path}. Say the whole path, like attach C:\\Users\\you\\Pictures\\photo.jpg.");
                    }
                    let previous_publisher = self.publisher.clone();
                    self.publisher.attach(id, &path);
                    if let Some(q) = self.publisher.request_approval(id) {
                        if let Err(e) = self.publisher.save(&self.store) { self.publisher = previous_publisher; return format!("That attachment change couldn't be saved ({e}); nothing was newly approved."); }
                        self.session.ask(&q);
                        self.pending_post_approval = Some(id);
                        return q;
                    }
                    return "I couldn't add that to it.".into();
                }
                if !is_yes(said) {
                    return "Alright — it stays a draft.".into();
                }
                self.pending_post_when = Some(id);
                let q = "When should it go? Right now, or a time like at 6pm or tomorrow at 9.".to_string();
                self.session.ask(&q);
                return q;
            }
            if let Some(id) = self.pending_post_when.take() {
                self.session.pending = Pending::Nothing;
                return self.schedule_post_at(id, said, t);
            }
            // "Go" on a mailbox rehearsal.
            if self.pending_mail_sort {
                self.pending_mail_sort = false;
                self.session.pending = Pending::Nothing;
                let go = is_yes(said) || said.trim().eq_ignore_ascii_case("go");
                return if go {
                    self.apply_mail_plan(None, t)
                } else {
                    self.mail_plans.clear();
                    "Alright, I've left your mailbox as it is.".into()
                };
            }
            // A routine asked about, or one that asks before running.
            if let Some(reply) = self.answer_about_routine(said, t) {
                return reply;
            }
            // A security change read back: your yes makes it, anything
            // else doesn't. `confirmed::answer` is stricter than `is_yes`
            // on purpose: a mumble is a no here.
            if let Some(asked) = self.pending_security.take() {
                self.session.pending = Pending::Nothing;
                return match crate::confirmed::answer(said, &asked) {
                    crate::confirmed::Step::Go { asked } => self.make_security_change(asked, t),
                    crate::confirmed::Step::Dropped => "Left alone.".into(),
                    _ => "I needed a yes or a no on that one, so I've left it alone. Ask me again if you want it.".into(),
                };
            }
            // "Sign you in? You've asked me to check first."
            if let Some((site, account)) = self.pending_signin.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright.".into();
                }
                return self.start_sign_in(&site, &account, t);
            }
            // A window job that asked before going on: yes carries it on,
            // no stops it.
            if let Some(id) = self.pending_window_confirm.take() {
                self.session.pending = Pending::Nothing;
                let yes = is_yes(said);
                if let Some(i) = self.working_for_you.iter().position(|w| w.id == id) {
                    let app = self.working_for_you[i].job.app.clone();
                    if yes {
                        let w = &mut self.working_for_you[i];
                        w.job.confirmed();
                        w.held = false;
                        w.looked = 0;
                        return format!("Carrying on in {app}.");
                    }
                    self.working_for_you[i].job.refused();
                    self.working_for_you.remove(i);
                    return format!("Alright, I've stopped working {app}.");
                }
                return "That one's already finished.".into();
            }
            // "Show it on your only screen?" — asked when a panel would sit
            // over what you're working on, and until now left with nothing
            // to answer it.
            if let Some(panel) = self.pending_panel.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright.".into();
                }
                let monitors = self.plat.monitors().unwrap_or_default();
                let cfg = self.panel_cfg();
                if crate::panel::place_anyway(panel, &monitors, &cfg).is_none() {
                    return "I can't find a screen to put it on.".into();
                }
                self.wants_panel = Some(panel);
                self.draw_panel(panel);
                let content = match panel {
                    crate::panel::Panel::Tasks => self.backlog.summary(),
                    crate::panel::Panel::Mind => self.mind_summary(),
                    _ => String::new(),
                };
                return crate::panel::narration(panel, &content, &cfg).unwrap_or_else(|| "It's up.".into());
            }
            // "What should I have done instead?" -- the answer is the fix,
            // and it belongs to the correction that asked, not to whatever
            // the parser would make of it on its own. Checked before the
            // other pending questions because a correction is the only one of
            // them whose answer is deliberately free text.
            if self.pending_correction.is_some() {
                self.session.pending = Pending::Nothing;
                let now = crate::store::now();
                let reply = self.correction_wanted(said, now);
                self.persist();
                return reply;
            }
            // Same shape, for "do you want me to think about it with you,
            // or just listen?" -- resolved against what was originally
            // said, never against the answer to the meta-question itself.
            // Something that isn't an answer to it -- a new question, a
            // command -- is taken as itself, not as a confused answer
            // ("look into Rome" after "…or just listen?" got "Not sure which
            // you meant", 26 Sep 2026).
            let answered_it = crate::wanted::answer_to_ask(said).is_some();
            if let (Some(original), true) = (self.pending_wanted.clone(), answered_it) {
                self.session.pending = Pending::Nothing;
                self.pending_wanted = None;
                return match crate::wanted::answer_to_ask(said) {
                    Some(crate::wanted::Wanted::Hearing) => {
                        format!("{} {}", crate::wanted::heard(&original), crate::wanted::then_offer())
                    }
                    // Not `run_command` -- the original words would parse as
                    // `Intent::Unknown` again, `wanted::read` would read the
                    // same way a second time, and this would ask the same
                    // question forever. A note lookup is the one thing that
                    // can genuinely answer it; short of that, say so plainly
                    // rather than loop.
                    Some(_) => self.answer_from_notes(&original, t).unwrap_or_else(|| {
                        "I don't have anything specific on that, but go ahead and tell me more."
                            .into()
                    }),
                    None => "Not sure which you meant -- I'll leave it there.".into(),
                };
            }
            if self.pending_wanted.take().is_some() {
                self.session.pending = Pending::Nothing;
            }
            // A backlog offer -- "Earlier you asked me to X but Y. Want me to
            // do it now?" -- and this is the yes or no. A yes runs the
            // original request; a no is the answer `backlog::dismiss` was
            // written for: the item stays on the record but is never raised
            // again. Without this a no only cleared the question, so
            // `next_offer` raised the same task on the next quiet tick and
            // "no" meant "ask me again later" forever. Anything that is
            // neither drops the offer and is handled from scratch.
            if let Some(id) = self.pending_backlog {
                if is_yes(said) {
                    self.pending_backlog = None;
                    self.session.pending = Pending::Nothing;
                    let request = self
                        .backlog
                        .items
                        .iter()
                        .find(|i| i.id == id)
                        .map(|i| i.request.clone());
                    if let Some(request) = request {
                        let reply = self.run_command(&request, t);
                        self.backlog.complete(id);
                        let _ = self.backlog.save(&self.store);
                        self.persist();
                        return reply;
                    }
                } else if is_no(said) {
                    self.pending_backlog = None;
                    self.session.pending = Pending::Nothing;
                    self.backlog.dismiss(id);
                    let _ = self.backlog.save(&self.store);
                    self.persist();
                    return "Alright, I'll leave that one off your list.".into();
                } else {
                    // The reply is about something else. The offer has already
                    // been raised; drop it rather than act on an ambiguous word.
                    self.pending_backlog = None;
                }
            }
            // Atlas asked something and this is the answer. Carry the
            // question into the turn.
            //
            // This was `let _ = q;` -- the question was **discarded**,
            // `pending` was cleared, and the answer fell through to
            // `run_command` to be parsed from scratch by a parser that had no
            // idea a question had been asked. So Atlas would ask "which
            // browser did you mean?", hear "the second one", and start again
            // from nothing.
            //
            // `ask` is the action the schema tells the model to use whenever
            // a request is ambiguous, so this is not a rare path: it is the
            // one the prompt actively steers toward, and it could not
            // complete. Two turns that read as one exchange to you were two
            // unrelated events to Atlas, which is most of what makes a
            // conversation feel like it is not happening.
            //
            // Cleared before running, so a question that somehow asks itself
            // again cannot loop.
            self.session.pending = Pending::Nothing;
            if !q.trim().is_empty() && !offer_dropped {
                if self.answer_dependent_question(&q, said) {
                    return "Thanks. Carrying on with your original request.".into();
                }
                self.answering = Some(q);
            }
        }

        // "Updates on the index and the backup when you want them." — and
        // then you said yes.
        //
        // This sits below every other pending answer on purpose: an offered
        // brief is the weakest claim on a bare yes there is, and anything
        // genuinely awaiting one has already returned above. It is also
        // deliberately *not* routed through `Pending::Clarification` like the
        // others: that machinery parks a question and treats your next
        // utterance as its answer, which is precisely what "when you're
        // ready" promised not to do — the offer rides along on a reply you
        // asked for, and you may well have come back to do something else.
        //
        // `pending_brief.is_none()` is what proves the offer was actually
        // delivered. Both are set on the same turn, and without this check a
        // "yes" that happened to be the first thing said after an absence
        // would answer an offer you had not heard yet, leaving the offer
        // itself queued to surface later against nothing.
        if self.pending_brief.is_none() {
            if let Some(happened) = self.pending_brief_detail.take() {
                if is_yes(said) {
                    return crate::returning::full_brief(&happened);
                }
                if is_no(said) {
                    return "Alright.".into();
                }
                // Anything else means you came back to do something specific,
                // which is the case the offer exists for. The list is dropped
                // rather than held: by the time you next say a bare yes it
                // will be about something else entirely, and answering it
                // with a stale brief is worse than not offering.
            }
        }

        // A bare yes/no with nothing pending is an acknowledgement, not a
        // command. Without this it parses as Unknown, gets gated, and parks a
        // question — which then eats your next real instruction as its answer.
        //
        // Unless one of round 11's tools just asked ("probably $7.75 -- keep
        // it?"): then the yes or no is its answer (`workday::read_first`).
        // Unless Atlas's own last words asked something ("Want me to set a
        // reminder?"): then the yes or no is an answer to that, and goes to
        // the model with the question still in the conversation (30 Sep 2026:
        // it was told "Nothing to confirm.").
        let asked_last = self.thread.recent.last().is_some_and(|e| e.reply.trim_end().ends_with('?'));
        if (is_yes(said) || is_no(said))
            && !asked_last
            && !matches!(self.parser.parse_named(said).0, Intent::Receipt(_) | Intent::TradeDay(_) | Intent::Cards(_))
        {
            return "Nothing to confirm.".into();
        }

        // Asking Atlas for a new ability (Eric, 1 Oct 2026) is heard at the
        // top of the turn now (5 Oct 2026), before any intent.
        // "Watch me for five minutes" (`camwatch`).
        if let Some(reply) = self.watch_request(said, t) {
            return reply;
        }
        // On a call: "show Atlas off", "don't mute my calls" (`callmute`).
        if let Some(reply) = self.call_mute_request(said, t) {
            return reply;
        }

        // Naming a tool as the instrument of the task IS the permission for
        // it — "use Excel to build that sheet" grants Excel for this task, so
        // the gate does not then stop to ask about the very app you just told
        // it to use. `grant_in_instruction` is refusal-aware ("don't use
        // Discord" grants nothing) and only fires on a deliberate lead, not a
        // passing mention.
        if let Some((app, span)) = crate::grants::grant_in_instruction(said, &self.known_app_names()) {
            self.permissions.grant(&app, None, span, t);
        }

        // The Talk page and the voice loop let this turn's model call run on
        // a worker (`pending_turn`); nothing else does.
        self.may_defer = self.defer_turns;
        let reply = self.run_command(said, t);
        self.may_defer = false;
        reply
    }
}
