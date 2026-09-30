//! A turn: `turn` and `turn_from`, answering before the model, `run_command`,
//! and the small helpers they lean on, down to finding files.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// How this reached Atlas.
    ///
    /// The distinction the addressing check needed and never got. Saying the
    /// wake word, pressing push-to-talk, or typing into Atlas's own prompt are
    /// all unambiguous: nobody types into Atlas to talk to the person next to
    /// them. Only genuinely ambient audio is worth second-guessing.
    ///
    /// This existed as a field on `addressing::Situation` -- `after_wake_word`
    /// -- and `turn` built that struct with `..Default::default()`, so it was
    /// always false. The wake word was detected upstream and thrown away
    /// before the one thing that needed to know about it, which is why saying
    /// "Atlas" and then "hello" got silence: addressing scored a one-word
    /// greeting as a fragment, called it overheard, and returned nothing.
    pub fn turn(&mut self, said: &str, t: u64) -> String {
        // The parser needs your project names to tell "fix the parser in
        // Homelab" (project work) from "change the volume" (not).
        let names: Vec<String> = self.workshop.projects.iter().map(|p| p.name.clone()).collect();
        self.parser.know_projects(names);
        // The people and habits you have, and the list a number would
        // refer to -- so "open 2" and "I called Sam" read right.
        self.workday_known(t);
        self.turn_from(said, t, Arrival::Directed)
    }

    /// Handle something you said, knowing how it arrived.
    pub fn turn_from(&mut self, said: &str, t: u64, how: Arrival) -> String {
        self.last_said = said.to_string();
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

        // Pause and resume are heard before anything else, so they work even
        // mid-task and mid-sentence.
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
                // A model turn still thinking is stopped too: its answer
                // would otherwise run its tool when it came back (28 Sep 2026).
                self.drop_pending_turn(t, "Stopped before I answered -- nothing was done.");
                // A request of several steps ends at its next step.
                self.stop_task_loop();
                let asked = self.crew.ask_everyone_to_stop();
                let halted = self.attention.halt(t);
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
                // "Pause" is total: the errands hold too, at their safe
                // points, and nothing they have done is lost.
                let msg = self.attention.pause(self.current_work(), t);
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
        if let Some(reply) = self.persona_now().social(said, crate::localclock::hour_here(t) as u8, t, self.last_turn_failed || self.mid_flow()) {
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
        let research_again;
        let said = if matches!(self.parser.parse(said), Intent::Unknown(_)) && crate::references::starts_the_research(said) {
            match self.referents.last_topic.clone() {
                Some(topic) => {
                    research_again = format!("research {topic}");
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
                if let Some(span) = crate::grants::span_from_answer(said) {
                    self.permissions.grant(&app, Some(&action), span, t);
                    let _ = self.store.save("permissions", &self.permissions);
                    self.session.pending = Pending::Nothing;
                    self.memory.record_approval(&kind, true, None);
                    let reply = self.execute(&intent);
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
                self.memory.record_approval(&kind, yes, None);
                let reply = if yes {
                    let r = self.execute(&intent);
                    // If this was a scheduled job asking, close it out too.
                    if let Some(jid) = self.pending_job.take() {
                        self.scheduler.approve(jid);
                        self.scheduler.complete(jid, t, &r, !r.starts_with("error"));
                    }
                    r
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
                if !is_yes(said) {
                    return "Alright, I've left it unopened.".into();
                }
                // Off the loop like the first ask (`file_work_off_the_loop`).
                let job = if what == "unzip" { FileJob::Unzip } else { FileJob::Read };
                return self.file_work_off_the_loop(job, &path, true);
            }
            // An edited video: keep it, and then the original (G8).
            if let Some((original, copy, result)) = self.pending_media_keep.take() {
                self.session.pending = Pending::Nothing;
                let _ = std::fs::remove_file(&copy);
                if !is_yes(said) {
                    let _ = std::fs::remove_file(&result);
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
                return "Kept.".into();
            }
            if let Some(original) = self.pending_media_original.take() {
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
            // The desktop's loose files, filed on a yes (29 Sep 2026).
            if let Some(plan) = self.pending_desktop.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, your desktop stays as it is.".into();
                }
                return self.carry_out_desktop_plan(plan);
            }
            // Moving big folders to another drive (G5).
            if let Some(plan) = self.pending_storage.take() {
                self.session.pending = Pending::Nothing;
                if !is_yes(said) {
                    return "Alright, nothing moved.".into();
                }
                return self.carry_out_storage_plan(plan, t);
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
                let l = said.to_lowercase();
                if let Some(new) = ["change it to ", "make it read ", "make it ", "change the words to "]
                    .iter()
                    .find_map(|p| l.find(p).map(|i| said[i + p.len()..].trim().trim_matches('"').to_string()))
                    .filter(|n| !n.is_empty())
                {
                    let before = self.publisher.get(id).map(|p| p.body.clone()).unwrap_or_default();
                    self.learned_from_your_edit(&before, &new, t);
                    if self.publisher.edit(id, &new) {
                        if let Some(q) = self.publisher.request_approval(id) {
                            let _ = self.publisher.save(&self.store);
                            self.session.ask(&q);
                            self.pending_post_approval = Some(id);
                            return q;
                        }
                    }
                    return "I couldn't change that one.".into();
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
                    Some(_) => self.from_notes(&original, t).unwrap_or_else(|| {
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
        if (is_yes(said) || is_no(said)) && !matches!(self.parser.parse_named(said).0, Intent::Receipt(_) | Intent::TradeDay(_) | Intent::Cards(_)) {
            return "Nothing to confirm.".into();
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

    /// Everything Atlas can answer from what it already holds, before any
    /// model: reminders (B3), what you've said you want (B2), a correction or
    /// fact you've stated, your notes, and the rest. `None` when none of it does.
    fn answer_locally(&mut self, raw: &str, t: u64) -> Option<String> {
        self.remind_help(raw, t)
            .or_else(|| self.spot_opportunity(raw))
            .or_else(|| self.learn_stated(raw))
            .or_else(|| self.from_notes(raw, t))
            .or_else(|| self.ways_in_help(raw))
            .or_else(|| self.decision_help(raw))
            .or_else(|| self.knew_once_help(raw))
            .or_else(|| self.wanted_check(raw))
    }

    /// The part of `answer_locally` that is a real answer from what Atlas
    /// holds, asked before the model: a reminder, a fact you stated or asked
    /// about, your notes. Not the "think it through with you, or just
    /// listen?" check or the want-weighing, which are for when there's no
    /// model to hold a conversation.
    ///
    /// Since 27 Sep 2026 only the things that ARE answers go here: a reminder
    /// set, a correction you opened with, a fact asked for by its exact slot
    /// ("what's the wifi password"), and the decision and ways-in helpers when
    /// the sentence opens with them. Your notes, the fact book's looser
    /// matches and what Atlas once knew go to the model as hints
    /// (`notes_as_hints`) instead of answering on their own -- a note that
    /// shared one word with "what should I eat" was the whole reply.
    fn answer_before_the_model(&mut self, raw: &str, t: u64) -> Option<String> {
        self.remind_help(raw, t)
            .or_else(|| self.learn_stated(raw))
            .or_else(|| self.exact_fact(raw, t))
            .or_else(|| if opens_with_ways(raw) { self.ways_in_help(raw) } else { None })
            .or_else(|| if opens_with_deciding(raw) { self.decision_help(raw) } else { None })
    }

    /// A fact asked for by its exact slot: "what kind of car do I have".
    fn exact_fact(&self, question: &str, now: u64) -> Option<String> {
        if self.handover().stance.handed_over() {
            return None;
        }
        self.facts.slot_answer(question, now).map(|f| f.answer(now))
    }

    /// The date and time, what's running, what setup still lacks, and what
    /// Atlas knows about itself for this question: the model was told none
    /// of it before 27 Sep 2026, so it couldn't say what day it was, what
    /// Atlas can do, or where anything is.
    fn about_now(&self, said: &str, t: u64) -> String {
        let off = crate::localclock::offset_secs();
        let (year, _, _) = crate::hubpages::ymd(crate::localclock::day(t, off));
        let root = self.store.install_root();
        let missing: Vec<&str> = crate::getpieces::setup_pieces()
            .iter()
            .filter(|p| !crate::getpieces::have(p, &root))
            .map(|p| p.name)
            .collect();
        let research = self.tools_ref().is_some_and(|tc| tc.research.enabled);
        format!(
            "Now: {} ({year}).\nRunning here: the language model{}; looking things up on the web is {}.\n{}{}",
            crate::localclock::spoken_now(t, off),
            if missing.is_empty() { String::new() } else { format!("; setup hasn't fetched yet: {}", missing.join(", ")) },
            if research { "on" } else { "off (Settings can turn it on)" },
            // Only for a question about Atlas: headed "answer only from these
            // lines", it made a small model say "not sure" to everything else
            // (27 Sep 2026).
            if crate::capability::is_about_atlas(said) { crate::capability::about_atlas(said, 6) } else { String::new() },
            "",
        )
    }

    pub(super) fn run_command(&mut self, said: &str, _t: u64) -> String {
        // Named modes and flows are already checked above this call, so the
        // RunNamed branch here rarely fires first -- what this adds is the
        // FromReport branch and the running Mix, which is the only way to
        // notice a daemon that is quietly spending a real model call on
        // questions its own reports already answered.
        //
        // reports is empty until something actually populates a cache of
        // prior work Atlas can answer from -- that cache does not exist yet,
        // so FromReport cannot fire in practice today. Tracked honestly
        // rather than faked: worth_saying() already says so once there is
        // enough history to judge by.
        let named: Vec<String> = self
            .modes
            .modes
            .iter()
            .map(|m| m.name.clone())
            .chain(self.flows.workflows.iter().map(|w| w.name.clone()))
            .collect();
        // Finishing a turn whose model call ran on a worker (`pending_turn`):
        // everything before the call already happened when it started.
        let resumed = self.decided_already.is_some();
        if !resumed {
            self.also_asked.clear();
        }
        let tier = crate::tier::tier_for(said, &named, &[], _t);
        if !resumed {
            self.tier_mix.note(&tier);
        }

        // Read before `context` takes it: the conversation path says it too.
        let answering = self.answering.clone();
        let ctx = format!("{}{}", self.context(), self.about_now(said, _t));

        // Atlas's own answers before the model's (Eric, 27 Sep 2026: "Atlas
        // did not seem that smart"). Reminders, facts you've told it, your
        // notes and the rest were asked only when there was NO model -- with
        // one, every unrecognised sentence went straight to the model, so
        // "remind me in 20 minutes" got a "sure" and nothing was set.
        let mut local = if !resumed
            && self.llm.is_some()
            && matches!(self.parser.parse(said), Intent::Unknown(_))
            && !self.handover().stance.handed_over()
        {
            self.answer_before_the_model(said, _t)
        } else {
            None
        };

        // What kind of moment this is, worked out BEFORE the model is asked.
        //
        // `register::read` was called further down, after `run_command` had
        // already produced the reply, so it could only trim what came back --
        // a `Chatting` register let eight sentences through from a model that
        // had been told to produce one short one. Reading it here is what
        // lets the register choose the *instructions* rather than just the
        // scissors.
        //
        // `after_a_failure` keys off the PREVIOUS turn rather than this one,
        // which is the only thing it can do here and is also the more
        // truthful reading: somebody is short with Atlas because the last
        // thing went wrong, not because this one is about to.
        let register = crate::register::read(
            said,
            &crate::register::Moment {
                recent: self.thread.recent.iter().rev().take(3).map(|e| e.said.clone()).collect(),
                after_a_failure: self.last_turn_failed,
                busy: self.queue_is_busy(),
            },
        );
        // One length, decided once, used for BOTH the briefing and the
        // trimming.
        //
        // Three caps stack here: `persona.max_spoken_sentences` from the
        // config, `modes::sentences_for(verbosity)` from the active mode, and
        // `register.length()`. The shaping step further down set
        // `persona.max_spoken_sentences = mode_cap.min(register.length())`,
        // overwriting the config value -- so the number the model was told
        // ("At most {n} sentences", rendered by `Persona::system_prompt`) and
        // the number it was cut at were computed from different things.
        //
        // Told eight and cut at three is the same failure as being told one
        // and allowed eight, which is the thing this whole pass is about. So
        // the cap is resolved here, written onto the persona that goes to the
        // model, and the shaping step reuses it rather than recomputing.
        //
        // `min` of all three: a mode that asks for brevity beats a chatty
        // register, and the config ceiling beats both.
        let mut persona = self.persona_now();
        // Only a mode somebody actually turned on gets to cap this. See
        // `Modes::verbosity_if_set`.
        // `.map(|v| f(v))` rather than `.map(f)`, and not as a style choice:
        // every reachability guard in this tree finds a call by looking for
        // `name(` (`tests/common/mod.rs`'s `called_names`), so a function
        // passed as a VALUE is invisible to all of them. Written the tidy
        // way, this made `modes::sentences_for` -- called right here -- count
        // as reached only by tests.
        let mode_cap = self
            .modes
            .verbosity_if_set()
            .map(|v| crate::modes::sentences_for(v))
            .unwrap_or(usize::MAX);
        persona.max_spoken_sentences =
            persona.max_spoken_sentences.min(mode_cap).min(register.length()).max(1);
        self.this_turn_cap = Some(persona.max_spoken_sentences);
        // Timed here rather than inside `decide`, because what matters is how
        // long *you* waited, which includes reaching the model and parsing
        // what came back -- not the part of it the model spent thinking.
        self.this_turn_register = Some(register);
        let started = std::time::Instant::now();
        // "What do you know about black holes": a notes question the notes
        // can't answer, so the model answers it rather than "Nothing in my
        // notes on black holes" (27 Sep 2026).
        let beyond_the_notes = !resumed && self.llm.is_some() && self.notes_have_nothing_on(said);
        if !resumed {
            self.by_chat = false;
        }
        let decision = match self.decided_already.take() {
            Some(d) => d,
            None => match self.llm.clone() {
                _ if local.is_some() => brain::Decision {
                    intent: Intent::Unknown(said.to_string()),
                    say: String::new(),
                    model: brain::Reached::NotNeeded,
                },
                Some(llm) => {
                    // A request of several parts (30 Sep 2026): worked side
                    // by side, or step by step, rather than one tool and the
                    // rest named as not done.
                    let several = if !resumed && llm.native_chat() && !self.handover().stance.handed_over() {
                        self.several_parts(said)
                    } else {
                        None
                    };
                    let needs_model = several.is_some() || beyond_the_notes || matches!(self.parser.parse(said), Intent::Unknown(_));
                    // Built only when the model will read it: a command the
                    // phrases settle needs none of it.
                    let mut turn = if needs_model {
                        self.conversation_turn(said, _t, register, &persona, answering.as_deref(), &ctx)
                    } else {
                        brain::Turn { said: said.to_string(), one_prompt: ctx.clone(), ..Default::default() }
                    };
                    turn.skip_phrases = beyond_the_notes;
                    self.by_chat = needs_model && llm.native_chat();
                    match several {
                        Some(tasks::Several::SideBySide(parts)) => self.work_side_by_side(llm.clone(), said, parts, _t, register, &persona),
                        Some(tasks::Several::StepByStep) => self.work_through(llm.clone(), said, turn, _t),
                        None => {
                    // The Talk page and the voice loop don't wait here: the
                    // call runs on a worker and the turn is finished when it
                    // comes back (`finish_pending_turn`).
                    if std::mem::take(&mut self.may_defer) && needs_model && self.pending_turn.is_none() {
                        self.start_pending_turn(said, _t, llm, turn, persona.clone(), register);
                        self.this_turn_register = None;
                        self.this_turn_cap = None;
                        return String::new();
                    }
                    // A model call is already out (a Talk page turn still
                    // thinking) and holds the conversation's slot: this one
                    // uses the other slot rather than waiting behind it on the
                    // loop (29 Sep 2026: both named slot 0, so this waited for
                    // the first to finish, and the hub, the typing box and
                    // Pause all went unanswered for minutes).
                    if self.pending_turn.is_some() {
                        turn.aside = true;
                    }
                    // The words so far are the Talk page's to show -- unless a
                    // Talk turn is still thinking, whose words they are
                    // (28 Sep 2026: a voice turn answered meanwhile cleared
                    // and wrote over them).
                    let partial = self.pending_turn.is_none().then(|| self.talk_partial.clone());
                    if let Some(Ok(mut p)) = partial.as_ref().map(|p| p.lock()) {
                        p.clear();
                    }
                    let mut also = Vec::new();
                    let _talking = self.talking_guard();
                    let d = Brain { llm: &*llm, fallback: &self.parser, voice: Some((&persona, register)) }.converse_noting(
                        &turn,
                        &mut |piece| {
                            if let Some(Ok(mut p)) = partial.as_ref().map(|p| p.lock()) {
                                p.push_str(piece);
                            }
                            true
                        },
                        &mut also,
                    );
                    self.also_asked = also;
                    d
                        }
                    }
                }
                None => {
                    let i = self.parser.parse(said);
                    let say = brain::default_say(&i);
                    brain::Decision { intent: i, say, model: brain::Reached::NotNeeded }
                }
            },
        };
        // "I'm on it" with nothing started, by whichever path the model
        // answered (30 Sep 2026, `backed`): the chat path holds such a
        // sentence back itself; this catches the one-prompt path.
        let decision = match &decision.intent {
            Intent::Say(s) if decision.model == brain::Reached::Yes => {
                let s2 = crate::backed::without_unbacked_claims(s, false);
                if s2 != *s {
                    brain::Decision { intent: Intent::Say(s2.clone()), say: s2, model: decision.model }
                } else {
                    decision
                }
            }
            _ => decision,
        };
        let took_ms = self.decided_in_ms.take().unwrap_or(started.elapsed().as_millis() as u64);
        // Only when the model was actually asked. `Reached::NotNeeded` means
        // the phrase parser settled it and nothing was sent, and recording
        // those would make the log say Atlas asks the model about everything
        // -- which is the opposite of what it is for.
        if decision.model != brain::Reached::NotNeeded {
            let failed = match decision.model {
                brain::Reached::No => Some(decision.say.clone()),
                _ => None,
            };
            self.record_model_call(
                "brain",
                took_ms,
                ctx.len() + said.len(),
                decision.say.len(),
                failed,
            );
        }
        // Record what the answer's dependencies actually did, at the moment
        // they did it. This is the only place the model's reachability is
        // observed, so if it is not written down here it is not written down
        // at all.
        match decision.model {
            brain::Reached::Yes => {
                if let Some(i) = self.connections.get_mut(crate::integrations::MODEL) {
                    i.worked(_t);
                }
            }
            brain::Reached::No => {
                if let Some(i) = self.connections.get_mut(crate::integrations::MODEL) {
                    i.failed(_t, "did not answer");
                }
            }
            // The parser handled it. That says nothing about the model either
            // way, and writing down a success it never earned would make the
            // board lie in the direction that matters least but still lie.
            brain::Reached::NotNeeded => {}
        }
        let mut intent = decision.intent.clone();

        // Write down where this turn's answer came from, so "why did you do
        // that?" / "why is that?" can be answered from a real record rather
        // than the empty list it used to read. Whether the model was reached
        // is the invisible choice the person cannot see and would ask about
        // ("why was that slow?"), and the tier carries the reason nothing
        // cheaper served. `decision.model` is what ACTUALLY happened, so the
        // record cannot claim the model was used on a turn the phrase parser
        // settled -- the tier alone is a pre-answer heuristic and would.
        //
        // Skipped for the questions that are themselves about the record --
        // `Intent::Why` and `Intent::History` -- because recording their own
        // routing would make a bare "why" surface the routing of the "why"
        // and answer itself.
        if !matches!(intent, Intent::Why(_) | Intent::History(_)) {
            let d = match decision.model {
                brain::Reached::Yes => crate::why::Decision {
                    at: _t,
                    what: "went to the model".into(),
                    because: match &tier {
                        crate::tier::Tier::Think { why } => why.clone(),
                        _ => "the request needed working out beyond what I had".into(),
                    },
                    instead_of: Some("answering from something I'd already worked out".into()),
                    set_by: None,
                },
                brain::Reached::No => crate::why::Decision {
                    at: _t,
                    what: "asked the model and got nothing back".into(),
                    because: "it did not answer, so I fell back to what I could do without it"
                        .into(),
                    instead_of: None,
                    set_by: None,
                },
                brain::Reached::NotNeeded => match &tier {
                    crate::tier::Tier::RunNamed(name) => crate::why::Decision {
                        at: _t,
                        what: format!("ran \"{name}\" without asking the model"),
                        because: "you named something I can run, so nothing had to be worked out"
                            .into(),
                        instead_of: Some("sending it to the model".into()),
                        set_by: None,
                    },
                    crate::tier::Tier::FromReport { report, as_of } => crate::why::Decision {
                        at: _t,
                        what: format!("answered from \"{report}\" rather than looking again"),
                        because: format!("it already covers this -- worked out {as_of}"),
                        instead_of: Some("sending it to the model".into()),
                        set_by: None,
                    },
                    crate::tier::Tier::Think { .. } => crate::why::Decision {
                        at: _t,
                        what: "answered without the model".into(),
                        because: "the phrasing settled it without anything having to be worked out"
                            .into(),
                        instead_of: Some("sending it to the model".into()),
                        set_by: None,
                    },
                },
            };
            self.decisions.note_full(d);
        }

        // "Summarise this" and "reply to this" are both clipboard commands,
        // and the difference between them is the whole instruction. Carry the
        // sentence through rather than falling back to a generic default.
        if let Intent::UseClipboard(arg) = &intent {
            if arg.trim().is_empty() {
                intent = Intent::UseClipboard(said.to_string());
            }
        }

        // Handed over, and this is one of the things Atlas will not do for
        // somebody who is not you.
        //
        // Above everything below it, and that position is the point. Below
        // the policy gate, a standing grant -- recorded when the machine was
        // yours -- returns `Ok` and the action runs. Below the notes lookup,
        // an unrecognised sentence has already been answered out of your
        // research notes, which is the leak this is for.
        if let Some(refusal) = self.handed_over_refusal(&intent) {
            self.log.info(&format!("refused while handed over: {}", kind_of(&intent)));
            return refusal;
        }

        // An unrecognised line is searched against Atlas's own notes before
        // anything else happens to it.
        //
        // This sits *above* the policy gate on purpose. An Unknown intent is
        // classified as needing approval, so it never reaches `execute` — it
        // becomes "I didn't catch that. Go ahead?" and waits. Reading back
        // something Atlas already wrote down changes nothing and risks
        // nothing, so asking permission for it is noise, and putting the
        // lookup below the gate meant it never ran at all.
        // Being spoken to like a person, answered like one — and above the
        // policy gate, because greeting Atlas is not an action that needs
        // approving. Below the gate it came back as "I didn't catch that. Go
        // ahead?", which treats hello as a failed command.
        let from_notes = match &intent {
            // Not while somebody else has it. `Unknown` is not on either
            // refusal list and must not be -- ordinary conversation lands
            // here, and refusing all of it would leave a guest with an Atlas
            // that says nothing. But *this* branch answers out of your
            // research notes and your own written-down wants, which is the
            // owner's material arriving through a door with no name on it.
            Intent::Unknown(raw) if !self.handover().stance.handed_over() => {
                // Reminders (B3) and stated-want weighing (B2) come first,
                // then a correction/declaration is learned before the notes
                // lookup ("actually my car is a Toyota" updates the fact).
                // All of it is gated by the handed_over() check on this arm.
                let answered = match local.take() {
                    Some(a) => Some(a),
                    None => self.answer_locally(raw, _t),
                };
                // Nothing here could answer it, and it read as a request
                // rather than a stray word or a mishear: Atlas was asked for
                // something it has no way to do. Written down so the next
                // "what am I missing?" can name it -- `recommend()` turns each
                // unsupported request into a concrete suggestion, and this is
                // the source it was built for. Recorded before the approval
                // gate, which for an unknown only ever asks or parks; whether
                // it does either does not change that the ask was made.
                if answered.is_none() && raw.split_whitespace().count() >= 2 {
                    self.wants_seen.asked_for_something_missing(raw);
                    let _ = self.store.save("wants_seen", &self.wants_seen);
                }
                answered
            }
            _ => None,
        };

        // "Do that tonight", "when I'm out".
        //
        // Read from the sentence rather than from a separate command: making
        // you say it twice is the friction that stops a feature being used.
        // Parked above the policy gate on purpose -- nothing is being done
        // yet, so nothing needs approving, and asking "may I?" about a thing
        // you have just asked to be *delayed* is the wrong question.
        //
        // Answering, reading and conversation are exempt: "what did I do
        // tonight" is a question, not an instruction to wait, and parking it
        // would make Atlas mute on the word.
        // Nor a question about the night: "what did you do overnight" was
        // parked until you were out of the way (the capability sweep, 30 Sep
        // 2026).
        if !matches!(
            intent,
            Intent::Unknown(_) | Intent::Ask(_) | Intent::Say(_) | Intent::Why(_) | Intent::Overnight | Intent::Recap
        ) && !asks_about_what_happened(said)
        {
            if let Some(blocker) = crate::backlog::asked_to_wait(said) {
                let id = self.backlog.record(said, blocker, _t);
                let _ = self.backlog.save(&self.store);
                return format!(
                    "Right — I'll hold {} until you're out of the way, and \
                     pick it up then. (#{id})",
                    intent.plain()
                );
            }
        }

        let call = crate::policy::classify_with_policy(&intent, &self.memory, &self.cfg.policy);

        // A voice that only *might* be yours turns a consequential action into
        // a question.
        //
        // "Consequential" is read from `policy::classify`, the intrinsic
        // judgement, rather than from `call` above — `classify_with_policy`
        // can downgrade an action to automatic because you approved one like
        // it before, and a standing grant is exactly what should not carry
        // when Atlas is unsure who is speaking. The grant records that *you*
        // approved it, which is the thing in doubt.
        //
        // This only ever escalates: `handle` never returns permission, so the
        // worst case is being asked a question you did not need to be asked.
        let consequential =
            !matches!(crate::policy::classify(&intent), Decision::AutoProceed);
        let call = match crate::voiceid::handle(
            self.last_verdict,
            consequential,
            &self.tools_cfg().voice_id,
        ) {
            crate::voiceid::Handling::Confirm => Decision::RequireApproval,
            _ => call,
        };

        // And a *reading* that only might be what you said does the same.
        //
        // The gate above grades what the action is. Until this, nothing
        // graded how Atlas came to believe you asked for it -- a dictation
        // matched from the phrase list and a dictation the model guessed out
        // of a mumble were graded identically and both simply happened.
        // `brain.rs` does tell the model to "use ask rather than guessing",
        // but that is an instruction to the very model whose confident
        // wrongness `certainty.rs` exists to catch, and nothing checked
        // whether it obeyed.
        //
        // Intrinsic `classify` again rather than `call`, on exactly the
        // reasoning in the block above: a standing grant records that you
        // approved something like this before, and what is in doubt here is
        // whether you asked for it at all. Escalation only, so the worst case
        // is a question you did not need.
        let understanding =
            crate::understood::Understanding::from_reached(decision.model);
        let call = crate::policy::Decision::max(
            call,
            crate::understood::grade(
                crate::policy::classify(&intent),
                understanding,
                &self.tools_cfg().understood,
            ),
        );
        // And a consequential action the MODEL chose -- a tool call, not
        // your words matching a phrase -- is always asked about first: handing
        // Atlas over, messaging someone, changing code, pressing a button.
        let call = if decision.model == brain::Reached::Yes && brain::model_must_ask(&intent) {
            crate::policy::Decision::max(call, Decision::RequireApproval)
        } else {
            call
        };
        // Another program's tool is asked about every time, unless its
        // server's entry lets this one tool run unasked (`mcp`).
        let call = self.mcp_gate(&intent, call);

        // A sensor may say something. It may not do anything.
        //
        // `Hint::offer` returns words and has no other effect -- it cannot
        // enter a handover and cannot leave one. This is the whole of what an
        // unfamiliar voice is permitted to cause: a sentence, once, telling
        // whoever is there that there is a way to say so out loud.
        if matches!(self.last_verdict, crate::voiceid::Verdict::NotYou(_))
            && !self.offered_handover
        {
            let handed = crate::handover::Handover::load(&crate::roots::install_state());
            if !handed.stance.handed_over() {
                if let Some(offer) = crate::handover::Hint::VoiceUnfamiliar.offer() {
                    self.session.ask(offer);
                    self.offered_handover = true;
                }
            }
        }
        // The turn's own time, for what the action reads the clock for
        // (`now_acting`): what was asked at 23:59 is about that day.
        self.acting_at = Some(_t);
        let reply = match from_notes {
            Some(answer) => answer,
            // A line Atlas didn't understand is answered, not approved.
            // `policy` still classes it as needing approval -- nothing Atlas
            // didn't understand may ever be *run* -- but asking "Go ahead?"
            // about it turned every unrecognised line into a question, and
            // the next thing said was then taken as the yes or no (found
            // testing with a friend, 26 Sep 2026). `execute(Unknown)` only
            // answers: from a written procedure, from your notes, or by
            // saying it can't.
            None if matches!(intent, Intent::Unknown(_)) => self.execute(&intent),
            None => match call {
            Decision::AutoProceed => self.execute(&intent),
            Decision::ProceedAndReport => self.execute(&intent),
            Decision::AskClarification => {
                // When the reason we are asking is that the *reading* was
                // inferred rather than matched, say the reading. The model's
                // own `say` is what it planned to announce while doing the
                // thing, which is the wrong sentence for a question -- and
                // "I didn't catch that" would throw away a reading Atlas
                // actually made and force you to start the sentence over.
                // Naming it lets you correct one word.
                let ask = if understanding == crate::understood::Understanding::Inferred
                    && crate::policy::classify(&intent) == Decision::ProceedAndReport
                {
                    crate::understood::checking(&intent.plain())
                } else {
                    decision.say.clone()
                };
                self.session.ask(&ask);
                ask
            }
            // Never ask about something that would be refused anyway: a
            // question whose "yes" leads to "you can't" is a question wasted,
            // and every wasted one teaches you to answer without reading.
            Decision::RequireApproval if self.refused_before_asking(&intent).is_some() => {
                self.refused_before_asking(&intent).unwrap_or_default()
            }
            Decision::RequireApproval => {
                if self.autonomy == Autonomy::Unattended {
                    // "I'll wait" used to be the whole of this branch, and
                    // nothing waited: no record was kept, so the next time you
                    // looked there was no trace you had been asked. A promise
                    // Atlas does not keep is worse than a refusal.
                    //
                    // `mend`'s rule is that the question has to be answerable
                    // without reading code, and its `parked()` is the backlog
                    // item it becomes -- the one producer of
                    // `Blocker::NeedsYourDecision`, which had no caller.
                    self.park_for_you(&crate::mend::about_approval(said), said, _t)
                } else {
                    // What kind of thing you're approving, when it matters:
                    // anything that leaves the machine, commits you, or
                    // changes Atlas says so (`categories::consent_line`);
                    // something local just asks.
                    let cat = crate::categories::category_of(&intent);
                    // Many commands have no stock phrase; the question still
                    // has to say what it is asking about.
                    let say = match brain::default_say(&intent) {
                        s if s.trim().is_empty() => format!("Just to check -- {}.", intent.plain()),
                        s => s,
                    };
                    // A message goes out as you, and pairing links two Atlases:
                    // neither signs you up to anyone's terms, which is what the
                    // agreement line said for both (the capability sweep, 30
                    // Sep 2026). And one that names nothing asks what first.
                    let q = match (&intent, cat) {
                        (Intent::CreateAccount(w) | Intent::SignIn(w), _) if w.trim().is_empty() => {
                            return "Which site?".into();
                        }
                        (Intent::Message(_), _) => format!("{} -- it goes out as you. Go ahead?", capital_first(&say.trim_end_matches('.').replace("Just to check -- ", ""))),
                        (Intent::Pair(_) | Intent::AcceptPairing(_), _) => format!(
                            "{} -- that Atlas and this one could then reach each other. Go ahead?",
                            capital_first(&say.trim_end_matches('.').replace("Just to check -- ", ""))
                        ),
                        (_, crate::categories::Category::LocalOperational | crate::categories::Category::LocalCreative) => {
                            format!("{say} Go ahead?")
                        }
                        _ => crate::categories::consent_line(cat, say.trim_end_matches('.')),
                    };
                    self.session.await_approval(intent.clone(), &q);
                    q
                }
            }
            },
        };
        self.acting_at = None;

        // A saved file that couldn't be read was set aside since you were
        // last told: said now, once, not only in Diagnose (28 Sep 2026).
        let reply = match crate::store::tell_set_aside(&self.store) {
            Some(line) => format!("{line} {reply}"),
            None => reply,
        };

        // If you have been gone a while, lead with what happened.
        let reply = match self.pending_brief.take() {
            Some(b) => {
                // It is now in the reply, so it has been handed over and can
                // be dropped. This is the only place the outbox is emptied.
                let cfg = self.notify_cfg();
                let _ = self.outbox.collect(_t, &cfg);
                format!("{b} {reply}")
            }
            // No away-brief -- either you never left, or the absence was too
            // short to owe you one. But the conversation itself can still have
            // a gap worth naming: turning back to Atlas after an hour on
            // something else reads as the same thread only if it says where
            // you left off. `resume_line` is asked against the thread as it
            // stood before this turn's `append` below, so the gap is measured
            // to the last thing actually said, and it stays silent unless that
            // gap has passed `gap_secs` and there is a topic to name. It fires
            // once -- the append that follows resets `last_active`, so the
            // next turn's gap is near zero and the line does not repeat.
            None => match self.thread.resume_line(&self.thread_cfg(), _t) {
                Some(r) => format!("{r} {reply}"),
                None => reply,
            },
        };

        // Remember what "it" now means.
        if let Some(app) = crate::session::app_of(&intent) {
            self.referents.last_app = Some(app);
        }
        // And what "that research" means.
        if let Intent::Research(topic) = &intent {
            if !topic.trim().is_empty() {
                self.referents.last_topic = Some(topic.clone());
            }
        }

        // Anything blocked goes on the outstanding list rather than evaporating.
        // Asked through `connectivity::allows`, the predicate written to answer
        // "can this need be met right now?" — it folds the need and the cached
        // reach into one answer, where this line used to spell out the
        // Internet case by hand and the other needs (Local, PrefersInternet)
        // were simply never considered.
        if !self.connectivity.allows(need_of(&intent)) {
            self.backlog.record(said, Blocker::Offline, _t);
        }

        // Everything Atlas says goes through its manner — and the manner
        // depends on the moment. A task gets two sentences; a conversation
        // gets room to actually be one.
        // The register this turn was ANSWERED in -- the same one the model was
        // briefed with, not a second reading taken afterwards. Two readings of
        // one turn could disagree, and then Atlas would be instructed to
        // converse and trimmed as if it were working.
        let register = self.this_turn_register.unwrap_or_else(|| {
            crate::register::read(
                said,
                &crate::register::Moment {
                    recent: self
                        .thread
                        .recent
                        .iter()
                        .rev()
                        .take(3)
                        .map(|e| e.said.clone())
                        .collect(),
                    after_a_failure: self.last_turn_failed,
                    busy: self.queue_is_busy(),
                },
            )
        });
        // The same persona and the same cap the model was briefed with. See
        // the note where `this_turn_cap` is set: two independent computations
        // of "how long" is how Atlas ended up instructed to converse and
        // trimmed as if it were working.
        let mut persona = self.persona_now();
        persona.max_spoken_sentences = self.this_turn_cap.unwrap_or_else(|| {
            self.modes
                .verbosity_if_set()
                .map(|v| crate::modes::sentences_for(v))
                .unwrap_or(usize::MAX)
                .min(register.length())
                .max(1)
        });
        // A list you asked for is read whole. Round 11's tools answer with
        // numbered lists ("what am I waiting on", "what did I copy") that
        // "open 2" refers back to, and the trading check-in must end with
        // its line -- cut to two sentences, the list is useless and the line
        // is gone. Everything else is shaped for speech as before.
        // A reply the model wrote over the chat path was already stopped at
        // the end of a sentence, at the length it was asked for; cutting it
        // again here chopped answers mid-thought. It only loses its filler,
        // with a hard ceiling kept for safety.
        let chatted = decision.model == brain::Reached::Yes && matches!(intent, Intent::Say(_)) && self.by_chat;
        let reply = if crate::workday::reads_whole(&intent) {
            reply.trim().to_string()
        } else if chatted {
            let mut loose = persona.clone();
            loose.max_spoken_sentences = SAFETY_SENTENCES;
            loose.spoken(&reply)
        } else {
            persona.spoken(&reply)
        };

        // An action the phrase parser settled gets its words put in Atlas's
        // voice, here, with no model call -- so the action is not delayed and
        // the sentence is not a table entry.
        //
        // Only on the fast path (`Reached::NotNeeded`): when the model was
        // asked it wrote the reply itself under `prompt_for`, which already
        // carries the tone and the form of address, and dressing that a
        // second time would put ", Eric." on a sentence that had already
        // decided how to end.
        //
        // And only for an action. A fact, a count or a refusal keeps the
        // words it was given: "now" belongs on something being done, not on
        // something being reported.
        let reply = if decision.model == brain::Reached::NotNeeded && brain::is_an_action(&intent) {
            persona.acknowledge_in(&reply, _t, register, self.mid_flow() || crate::wit::fenced_intent(&intent))
        } else {
            reply
        };

        // Deliberately after `persona.spoken`, not before. `shape()` caps the
        // sentence count, so a caveat added earlier is exactly the sentence
        // most likely to be cut — the warning would be silently dropped from
        // the answers that most needed it.
        // A model call that failed with nothing to say is answered with why,
        // not with an empty reply and a caveat about what it is missing (29
        // Sep 2026: the whole reply Eric got was "I should say: the language
        // model is failing -- so this is missing whatever it would have
        // added", and the reason was thrown away).
        let failed_silent = decision.model == brain::Reached::No && reply.trim().is_empty();
        let reply = if failed_silent {
            let why = self.model_server_trouble.clone().unwrap_or_else(|| decision.say.clone());
            self.log.warn(&format!("the model call failed: {}", decision.say));
            // Ours may be running but stuck (a graphics driver hang): the
            // next pass asks it, and restarts it if it doesn't answer.
            self.model_suspect = self.starts_model_server;
            model_failed_words(&why)
        } else {
            reply
        };
        let used = crate::integrations::sources_for(need_of(&intent), decision.model);
        let reply = if failed_silent { reply } else { crate::integrations::mark(&reply, &used, &self.connections, _t) };
        // The model didn't know, or it's the kind of thing that changes by
        // the day: offer to look it up, and a yes does.
        let reply = match self.offer_to_look_it_up(said, &intent, decision.model, &reply) {
            Some(r) => r,
            None => reply,
        };
        // Asked for two things in one breath, the model called two tools
        // and only the first runs: the second is named, not dropped.
        let also = std::mem::take(&mut self.also_asked);
        let reply = if also.is_empty() { reply } else { format!("{} {}", reply.trim(), one_at_a_time(&also)) };
        self.last_register = register;

        // Carried to the NEXT turn, for `register::Moment::after_a_failure`.
        // Read from the reply that was actually produced, which is the only
        // place the outcome is known, and used one turn later, which is where
        // the frustration lands.
        //
        // Only a task can fail. A conversation that mentions failing ("the
        // launch failed because…") left the next turn Rough, with no jokes and
        // no tangents (27 Sep 2026).
        self.last_turn_failed = !matches!(intent, Intent::Say(_) | Intent::Ask(_) | Intent::Unknown(_))
            && (reply.to_lowercase().contains("failed")
                || reply.to_lowercase().starts_with("error")
                || reply.contains("I couldn't")
                || reply.contains("unreachable"));
        self.this_turn_register = None;
        self.this_turn_cap = None;

        // What gets WRITTEN DOWN, which is not always what was said.
        //
        // The three calls below all persist: `session` is in memory but
        // `thread.append` and `journal.record_at` are both saved by
        // `persist()` two lines further on, into `data/state/thread.json` and
        // `data/state/activity.json` -- and `safety::back_up` copies that
        // whole folder into `data/backups/`, so anything here is replicated
        // into every backup too.
        //
        // `said` used to go in verbatim. `config/commands.yaml` ships
        // `"the passphrase is"` as an Unlock phrase that takes an argument,
        // so saying "the passphrase is hunter2" wrote **hunter2 in plaintext
        // into two files in the same directory as the vault it opens.** A
        // stolen laptop then carries the ciphertext and the passphrase side
        // by side, which is the exact thing `vault.rs`'s design exists to
        // prevent, and `typed.rs` opens by saying there is "exactly one thing
        // in Atlas that a microphone must never carry: the vault passphrase".
        // It was carrying it, and then writing it down.
        //
        // Reproduced before fixing: one turn, then `persist()`, then grep the
        // state folder -- it was in `thread.json` and `activity.json`.
        //
        // The description rather than a mask, because `describe` already says
        // what the turn was for and the thread stays readable: you can still
        // see that you unlocked the vault at that point, which is the part
        // worth keeping.
        let for_the_record: String = match &intent {
            Intent::Unlock(_) => {
                format!("[{} — what you said is not written down]", intent.plain())
            }
            _ => said.to_string(),
        };
        let said = for_the_record.as_str();

        // How long, said up front for the long kinds (F6).
        let reply = match self.how_long_up_front(&intent, _t) {
            Some(est) if !reply.is_empty() => format!("{reply} {est}"),
            _ => reply,
        };
        // An old correction about this kind of work (F9).
        let reply = match self.related_old_correction(&intent, said, _t) {
            Some(line) if !reply.is_empty() => format!("{reply} {line}"),
            _ => reply,
        };
        self.session.record(said, &intent, &reply);
        self.note_for_routines(said, &intent, _t);
        self.thread.append(said, &reply, topic_of(&intent), _t);
        // A tool that reads something back (your calendar, what's in your
        // notes, the machine's health) answered with its fixed words; the
        // model puts them into a short natural reply next (`RephraseAsk`).
        // Actions keep their fixed acknowledgements.
        if self.rephrase_ok && decision.model == brain::Reached::Yes && self.by_chat && reads_back(&intent) && worth_rephrasing(&reply) {
            self.rephrase_ask = Some(RephraseAsk { written: reply.clone(), keep_written: crate::workday::reads_whole(&intent) });
        }
        // The same question, come round again. `asked_before` is asked now,
        // with this turn already appended, so it skips the turn just added and
        // looks further back for an identical `said` -- exactly the shape its
        // own doc describes ("repeating an answer verbatim ... makes an
        // assistant feel like it isn't listening"). The answer still gets
        // given in full; a short lead-in just marks that Atlas noticed, rather
        // than replaying the reply as if for the first time. Left off what
        // gets written down: the record keeps the substantive answer, not the
        // conversational nudge.
        //
        // Not in conversation: "tell me another joke" twice is a request for
        // a second one, and a model that answers again answers afresh.
        let reply = match self.thread.asked_before(said) {
            Some(_) if !chatted => format!("You asked this a little earlier -- here it is again. {reply}"),
            _ => reply,
        };
        self.fold_if_due(_t);
        self.journal.record_at(Act::Asked, said, true, _t);
        self.persist();
        reply
    }

    /// Is there work in hand -- queued in a lane, or a workflow mid-run?
    ///
    /// Public because the busy signal is a fact about the daemon that the
    /// outside is entitled to ask, and the tests that hold the signal honest
    /// ask it here rather than reaching into the queue and re-deriving it.
    pub fn queue_is_busy(&self) -> bool {
        // A workflow mid-run is work in hand, exactly as queued work is --
        // it just lives in `current_flow` instead of a lane.
        self.queue.pending() > 0 || self.current_flow.is_some()
    }

    /// What "this" refers to right now.
    ///
    /// Returns a question when it genuinely can't tell, which stops the
    /// intent rather than guessing at it.
    pub(super) fn resolve_subject(&mut self, intent: &Intent) -> Option<String> {
        let arg = match intent {
            Intent::Research(a) | Intent::DraftPost(a) | Intent::Capture(a) | Intent::Files(a) => a,
            // `ReviewPost`'s argument is the actual text being reviewed, not
            // a bare reference to it — "review this post: I love it when
            // things work" has "it" and "this" in it as ordinary English,
            // not as something to resolve. Treating it the same as
            // "explain this" meant any post whose own wording happened to
            // contain "it", "this" or "that" anywhere was silently
            // hijacked into a clipboard/selection question instead of ever
            // being reviewed at all.
            Intent::ReviewPost(_) => return None,
            // "Explain this" with UseClipboard already says where "this" is.
            Intent::UseClipboard(_) => return None,
            _ => return None,
        };
        // Only sentences that actually contain a pronoun need resolving.
        let lower = arg.to_lowercase();
        if !["this", "that", "it", "these", "those"]
            .iter()
            .any(|w| lower.split_whitespace().any(|t| t.trim_matches(',') == *w))
        {
            return None;
        }
        // Only what the daemon genuinely knows. Filling these in with guesses
        // would make the resolver confident about nothing — and leaving the
        // clipboard out made it resolve to nothing at all, which broke
        // "explain this".
        //
        // `only_on_request` (on by default) means Atlas does not reach for
        // what you copied unless your words point at it — "explain this" when
        // "this" could be the clipboard, not every pronoun. The predicate
        // written for exactly this (`refers_to_clipboard`) had no caller, so
        // the setting was dead and the clipboard was silently in scope for
        // every resolution.
        let use_clipboard = !self.clipboard_cfg().only_on_request
            || crate::clipboard::refers_to_clipboard(arg);
        let candidates = crate::subject::Candidates {
            clipboard: if use_clipboard { self.clipboard_text.clone() } else { None },
            ..Default::default()
        };
        match crate::subject::resolve(arg, &candidates) {
            // Two equally likely things is a question, not a coin toss.
            crate::subject::Resolution::Ambiguous { question, .. } => Some(question),
            crate::subject::Resolution::Nothing(why) => Some(why),
            crate::subject::Resolution::Found { .. } => None,
        }
    }

    /// Something learned. Merges with what's already known rather than
    /// adding a second copy.
    ///
    /// The point: asking about the same thing twice, worded differently,
    /// shouldn't cost you what you knew. Two notes means two decay curves and
    /// confidence in a settled fact drifting down because you happened to ask
    /// again.
    pub fn learned(&mut self, says: &str, source: &str, now: u64) -> bool {
        let strengthened = crate::consolidate::learn(&mut self.known, says, source, now);

        let cfg = self.tools_cfg().consolidate.clone();
        // Squeeze the long ones first, drop only if that isn't enough, and
        // keep a line for whatever goes (H10: memory keeps a stub of what it
        // forgot).
        let (_squeezed, dropped) =
            crate::consolidate::make_room(&mut self.known, &mut self.stones, cfg.keep_at_most, now);
        if !dropped.is_empty() {
            let _ = self.store.save("known_stones", &self.stones);
        }
        if let Some(note) = crate::consolidate::dropped_note(&dropped) {
            // Said rather than done quietly — a store that silently forgets is
            // one you stop trusting.
            self.history.note(&note, "knowledge",
                crate::undo::Undo::Cannot("what was dropped is gone".into()), false, now);
        }
        // Trim holds settled facts and things about your own setup out of the
        // budget entirely, because they cannot be looked up again. When those
        // alone exceed the cap, nothing is dropped and the store sits over
        // budget on purpose -- and until now, silently. A cap quietly exceeded
        // is a cap doing nothing, so which of the two it is gets said.
        if let Some(note) =
            crate::consolidate::over_budget_on_purpose(&self.known, cfg.keep_at_most)
        {
            self.history.note(&note, "knowledge",
                crate::undo::Undo::Cannot(
                    "nothing was dropped; the budget is exceeded by facts that can't be relearned"
                        .into(),
                ), false, now);
        }
        strengthened
    }

    /// Turn what Atlas records about itself into signals it can act on.
    ///
    /// The gap this closes: `selfaudit` could rank signals and had nothing
    /// producing them. A self-audit with an empty input list reports that
    /// everything is fine, which is the most misleading possible answer.
    pub fn refresh_signals(&mut self) {
        // What requests have actually used (`used`, 30 Sep 2026). Only
        // abilities there's a way to ask for, and only after two weeks of
        // counting: before that it's empty and `from_unused` says nothing
        // (29 Sep: every working ability was listed as never called, with a
        // "Have a go" that could never be done).
        let never_used = self.used.unasked(crate::store::now());
        let total = crate::capability::all().len() as u32;
        self.signals = crate::signals::gather(
            &self.history,
            self.unknown_count,
            self.utterance_count,
            &self.last_unknown,
            &never_used,
            total,
        );
    }

    /// Anything that was running when the machine stopped.
    ///
    /// Resolved before the brief, not during it — you sat down to get on with
    /// something, and a system that opens with questions about last night has
    /// answered the wrong one. Bounded: anything it can't judge inside the
    /// budget is left alone and mentioned, never asked about.
    pub(super) fn settle_interrupted(&mut self) -> Vec<String> {
        let mut said = Vec::new();
        let jobs = std::mem::take(&mut self.interrupted);
        for (what, checked, reversible) in jobs {
            let decision = crate::awake::on_waking_checked(&checked, reversible, 0);
            said.push(crate::awake::woke_checked(&what, decision, &checked));
        }
        said
    }

    /// Should the machine be kept awake right now?
    ///
    /// Only while something is actually running, and never with the lid shut
    /// or the battery low. Called by whatever is running rather than held
    /// open by the daemon.
    pub fn keep_awake(&self, why: crate::awake::Because, power: &crate::awake::Power, held_mins: u32)
        -> (crate::awake::Hold, String)
    {
        crate::awake::decide(why, power, held_mins, &self.tools_cfg().awake)
    }

    /// Something didn't work. Is there another way?
    ///
    /// Reporting a failure is what a program does. Trying the next route is
    /// what an assistant does, and the module for it has been sitting there
    /// unreachable.
    pub fn another_way(&self, kind: crate::route::Kind, failed: &str) -> Option<String> {
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        let next = crate::route::known_routes()
            .into_iter()
            .filter(|r| r.for_what == kind)
            .filter(|r| r.name != failed)
            .filter(|r| online || !crate::route::needs_internet(r))
            .max_by(|a, b| {
                a.reliability.partial_cmp(&b.reliability).unwrap_or(std::cmp::Ordering::Equal)
            })?;
        Some(next.name)
    }

    /// The apps Atlas has configured, by name — what `grants::check` counts
    /// as "known", and what `grant_in_instruction` matches a named tool
    /// against.
    fn known_app_names(&self) -> Vec<String> {
        self.cfg.apps.apps.keys().cloned().collect()
    }

    /// What Atlas knows about an app, for the permission check. `known` is
    /// simply whether it is in the configured apps; `confirm_each_time` is the
    /// per-app "ask every single time" flag, which today only the no-input
    /// apps (Discord and its kind) carry — a stray keystroke there is public
    /// and permanent, so touching one is always a question.
    pub(super) fn app_facts(&self, app: &str) -> crate::grants::AppFacts {
        let spec = self.cfg.apps.apps.iter().find(|(name, _)| name.eq_ignore_ascii_case(app));
        crate::grants::AppFacts {
            known: spec.is_some(),
            confirm_each_time: spec.map(|(_, s)| s.no_input).unwrap_or(false),
        }
    }

    /// The permission gate for acting on an app. A configured app Atlas
    /// already knows sails straight through — the gate is not a nag, it is
    /// the "I don't know this one, may I?" question rule 1 of the grants
    /// module describes, plus the confirm-every-time apps. On a question it
    /// parks the action as a pending approval so the next thing you say is
    /// the answer; on a yes there, the grant is recorded (with the breadth
    /// you gave — once, this session, or always) before the action runs, so
    /// the re-check finds it and does not ask twice.
    pub(super) fn gate_app(&mut self, app: &str, action: &str, intent: &Intent) -> AppGate {
        let facts = self.app_facts(app);
        match self.permissions.check(app, action, &facts) {
            crate::grants::Verdict::Allowed(_) => {
                // A one-off grant is spent the moment it is used, so the next
                // action on the same app asks again.
                self.permissions.consume(app, action);
                AppGate::Go
            }
            crate::grants::Verdict::Ask(question) => {
                self.session.await_approval(intent.clone(), &question);
                AppGate::Ask(question)
            }
        }
    }

    /// What Atlas can still do with the network unplugged.
    pub fn offline_coverage(&self, kind: crate::route::Kind) -> String {
        let (offline, all) = crate::route::coverage(kind);
        format!("{offline} of {all} ways work with no internet.")
    }

    /// Is this something Atlas already knows how to do?
    ///
    /// Checked before falling back to the model — a procedure Atlas has
    /// written down beats a guess, and it works with the network unplugged.
    pub(super) fn known_procedure(&self, asked: &str) -> Option<String> {
        let book = crate::knowhow::Knowhow::load(&self.store);
        // Offline is the normal case, not the exception, so a procedure that
        // needs the internet is filtered out rather than offered and failed.
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        book.for_request(asked, online)
            .map(|p| crate::knowhow::announce(p, online))
    }

    /// A symptom you describe, matched to a known snag and its fix.
    ///
    /// The other half of `known_procedure`: that answers "how do I do X" from
    /// the procedures' goals; this reads `knowhow::for_symptom`, which scores
    /// what you say against every procedure's *snags* -- the things that
    /// usually go wrong -- and names the likely cause and what to do. Offline
    /// by construction, because the snags ship compiled in. Named
    /// `diagnose_symptom` rather than `diagnose` on purpose: `diagnose.rs`
    /// already owns the bare name `diagnose` for the machine-vitals check, and
    /// the reachability scan matches by bare name.
    pub(super) fn diagnose_symptom(&self, symptom: &str) -> String {
        let book = crate::knowhow::Knowhow::load(&self.store);
        match book.for_symptom(symptom) {
            Some((p, snag)) => format!(
                "That's the sort of thing I've run into ({}): usually {}. \
                 What I'd do: {}. It comes up while trying to {}.",
                snag.looks_like, snag.cause, snag.fix, p.goal
            ),
            None if symptom.trim().is_empty() => {
                "Tell me what you're actually seeing and I'll check whether I know \
                 the cause -- something like \"an app that launches and closes \
                 immediately\"."
                    .into()
            }
            None => format!(
                "I can't match \"{}\" to any snag I've run into before. Describe \
                 what's on screen a bit differently and I'll look again.",
                symptom.trim()
            ),
        }
    }

    /// A task you name, read back as the steps to follow.
    ///
    /// The other side of `known_procedure`: that finds the same procedure with
    /// `for_request` but only says `announce` -- "I know this one, N steps" --
    /// and never the steps. This is the caller `knowhow::as_plan` never had,
    /// so a match becomes a numbered plan you can actually work through.
    /// Offline by construction: the procedures ship compiled in, and one that
    /// needs the internet is filtered out rather than offered and failed.
    ///
    /// Named `walk_me_through` rather than any bare `plan`/`steps` on purpose:
    /// the reachability scan matches by bare name, and this name is its own.
    pub(super) fn walk_me_through(&self, asked: &str) -> String {
        let asked = asked.trim();
        if asked.is_empty() {
            return "Walk you through what? Name the task -- \"walk me through \
                    freeing up memory\", \"how do I open something that won't \
                    launch\"."
                .into();
        }
        let book = crate::knowhow::Knowhow::load(&self.store);
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        match book.for_request(asked, online) {
            Some(p) => {
                let mut s = format!("{} -- here's how:\n{}\n", p.goal, crate::knowhow::checklist(p));
                for (n, step) in crate::knowhow::as_plan(p).into_iter().enumerate() {
                    s.push_str(&format!("  {}. {}\n", n + 1, step));
                }
                s
            }
            // Not one of Atlas's own procedures -- "how do I make pancakes"
            // is a question, and it gets an answer when there's a model to
            // give one, not a pointer at a troubleshooting command (26 Sep
            // 2026).
            None => {
                if let Some(llm) = self.llm.clone() {
                    let system = format!(
                        "{}\n\n{}",
                        self.persona_now().prompt_for(crate::register::Register::Working),
                        crate::brain::TALK
                    );
                    if let Some(said) = llm
                        .complete(&system, &format!("{}\nUser said: how do I {asked}", crate::capability::about_atlas(asked, 6)))
                        .ok()
                        .and_then(|r| crate::brain::spoken_text(&r))
                    {
                        return said;
                    }
                }
                format!(
                    "I don't have steps for \"{asked}\" written down, and questions like that need my \
                     language model, which isn't running here. If something's gone wrong, tell me what \
                     you're seeing -- \"troubleshoot ...\" -- and I'll see if I know the cause."
                )
            }
        }
    }

    /// Before saying something, is it worth saying?
    ///
    /// The one gate everything unprompted goes through. Without it every
    /// feature politely announces itself and the sum is a system that talks
    /// constantly about nothing.
    pub fn worth_saying(&mut self, thing: &crate::interrupt::Thing, doing: crate::interrupt::Doing) -> Option<String> {
        let mut cfg = self.tools_cfg().interrupt.clone();
        // What you wrote in the config, plus what you said out loud. Two
        // lists because they come from two places and neither should silently
        // overwrite the other -- `interrupt::Muted` has the argument.
        cfg.muted.extend(crate::interrupt::Muted::load(&self.store).topics);
        match self.gate.consider(thing, doing, &cfg, clock()) {
            crate::interrupt::Decision::Say(s) => Some(s),
            _ => None,
        }
    }

    /// How sure is the answer, and should it say so?
    pub(super) fn hedge(&self, answer: &str, grounding: crate::certainty::Grounding) -> String {
        let cfg = self.tools_cfg().certainty.clone();
        let (confidence, _, why) = crate::certainty::assess(answer, &grounding, &cfg);
        crate::certainty::phrase(answer, confidence, &why)
    }

    /// The tools config, or its defaults when there isn't one.
    ///
    /// Everything is off by default, so a missing tools.yaml means Atlas does
    /// less rather than more.
    /// Your time zone: the `time_zone` setting if you've chosen one, and
    /// this computer's own clock if you haven't (`tz::home`).
    pub fn home_zone(&self) -> crate::tz::Zone {
        crate::tz::home(self.tools_ref().map(|t| t.time_zone.as_str()).unwrap_or(""))
    }

    /// The zone you chose, or `None` for "this computer's clock": what
    /// `localclock` is told, so the hub and the calendar agree.
    fn chosen_zone(&self) -> Option<crate::tz::Zone> {
        let set = self.tools_ref().map(|t| t.time_zone.trim().to_string()).unwrap_or_default();
        (!set.is_empty() && !set.eq_ignore_ascii_case("automatic")).then(|| crate::tz::home(&set))
    }

    /// Your settings as they stand now: the ones Atlas started with, or the
    /// ones it has picked up since. Every read of a setting goes through here,
    /// which is what lets a change apply without a restart.
    pub(crate) fn tools_ref(&self) -> Option<&crate::voice::ToolsConfig> {
        self.tools_live.as_ref().or(self.cfg.tools.as_ref())
    }

    /// A security, vault or confirmation step is waiting on you: no wit
    /// until it's done (`wit::holds_back`).
    pub(super) fn mid_flow(&self) -> bool {
        self.pending_security.is_some()
            || self.pending_signin.is_some()
            || self.pending_window_confirm.is_some()
            || self.pending_post_approval.is_some()
            || self.pending_press.is_some()
            || self.pending_offer.is_some()
    }

    /// The settings folder being watched, when Atlas runs for real: where a
    /// setting changed by voice is kept (`talkback`, `hunting`).
    pub(crate) fn settings_dir(&self) -> Option<std::path::PathBuf> {
        self.settings_watch.as_ref().map(|(d, _)| d.clone())
    }

    /// Watch this folder's settings, so a change made in the settings window,
    /// the hub, or by hand is picked up while Atlas runs.
    pub fn watch_settings(mut self, config_dir: std::path::PathBuf) -> Self {
        let seen = settings_fingerprint(&config_dir);
        self.settings_stamp = settings_stamp(&config_dir);
        self.settings_watch = Some((config_dir, seen));
        self
    }

    /// Pick up a change to your settings, if there's been one since the last
    /// look. Returns one line per setting that changed, saying whether it
    /// applies now or when Atlas next starts.
    ///
    /// Cheap enough for every tick: two files' sizes and modified times are
    /// looked at, and only when one of those has moved are the files read and
    /// hashed (27 Sep 2026 -- they used to be read and hashed every tick).
    /// Nothing is reloaded unless the contents differ from last time.
    pub fn pick_up_settings(&mut self) -> Vec<String> {
        let Some((dir, seen)) = self.settings_watch.clone() else { return Vec::new() };
        let stamp = settings_stamp(&dir);
        if stamp == self.settings_stamp {
            return Vec::new();
        }
        self.settings_stamp = stamp;
        let now = settings_fingerprint(&dir);
        if now == seen {
            return Vec::new();
        }
        let fresh = match crate::config::Config::load(&dir) {
            Ok(c) => c.tools.map(|t| t.anchored()),
            Err(e) => {
                // Half-written or hand-broken: keep running on what we have,
                // look again next tick, and say why once.
                self.settings_watch = Some((dir, now));
                return vec![format!("I couldn't read my changed settings, so I'm keeping the ones I had: {e}")];
            }
        };
        self.settings_watch = Some((dir.clone(), now));
        // Your settings file is applied quietly over tools.yaml, so one that
        // won't read would otherwise just look like every choice reverting.
        if let Err(e) = crate::preferences::Preferences::load_checked(&dir) {
            let line = format!("I couldn't read your settings file, so I'm using the defaults until it's fixed: {e}");
            self.log.warn(&line);
            return vec![line];
        }
        let Some(mut fresh) = fresh else { return Vec::new() };
        let before = self.tools_ref().cloned().unwrap_or_default();
        // `vars` aren't settings — nothing on the settings page changes them —
        // and some hold what Atlas worked out for itself at the start (the
        // microphone it picked, over the one the file names). So the running
        // ones stay, and only a var the file has newly gained is added.
        let mut vars = before.vars.clone();
        for (k, v) in std::mem::take(&mut fresh.vars) {
            vars.entry(k).or_insert(v);
        }
        fresh.vars = vars;
        let changed = crate::settings::registry(&before).differences(&crate::settings::registry(&fresh));
        // The three parts that keep their own copy of a setting, refreshed so
        // they read the new one.
        self.proactive.cfg = fresh.proactive.clone();
        self.persona = fresh.persona.clone();
        self.eyes.cfg = fresh.presence.clone();
        self.tools_live = Some(fresh);
        // The shared, resolved copy every `tools_cfg()` hands out, and the
        // home zone the clock follows, are rebuilt from the new settings.
        self.tools_resolved = std::sync::Arc::new(resolve_tools(self.tools_ref(), &self.store));
        crate::localclock::set_home_zone(self.chosen_zone());
        let crew_cfg = self.tools_resolved.crew.clone();
        self.crew.set_margins(crew_cfg.keep_free_mb, crew_cfg.battery_floor_percent);
        self.mcp_configure();
        // The wake word switched on or off from the hub or the settings
        // window takes effect now, not at the next start.
        if self.wake_on() != self.tiers.wake_on() {
            let on = self.wake_on();
            self.tiers.set_wake(on);
        }
        let lines: Vec<String> = changed
            .iter()
            .map(|s| {
                let v = s.value.as_display();
                if crate::settings::needs_a_restart(&s.key) {
                    format!("{} will be {v} when I next start.", s.name)
                } else {
                    format!("{} is now {v}.", s.name)
                }
            })
            .collect();
        for l in &lines {
            self.log.info(&format!("settings: {l}"));
        }
        lines
    }

    /// Your tools.yaml, resolved and shared.
    ///
    /// This was `self.cfg.tools.clone().unwrap_or_default()` on every call —
    /// every tool's command and arguments, the whole variable map, several
    /// hundred allocations — at over a hundred call sites, most of which
    /// wanted one field: `let cfg = self.tools_cfg().signin;` copied the lot
    /// to keep one. It is materialised once, handed out by reference count,
    /// and rebuilt only when your settings change (`pick_up_settings`). A
    /// site that needs to own a section clones that one section.
    pub fn tools_cfg(&self) -> std::sync::Arc<crate::voice::ToolsConfig> {
        self.tools_resolved.clone()
    }

    /// "Find me the thing about the budget."
    ///
    /// This used to answer by classifying the words as if they were a
    /// filename — "the budget is a Document. It needs an application that
    /// opens it" — which has the shape of an answer without being one.
    /// `index.search` had existed the whole time and nothing called it.
    ///
    /// The question is run through `asking` first. A spoken question is
    /// mostly scaffolding, and sometimes it points at something rather than
    /// naming it; searching the raw words either dilutes the one term that
    /// mattered or searches for nothing at all and reports that as an answer.
    /// The three things `Intent::Files` was written to do: convert one kind of
    /// file into another, join several, or find one. They share a phrase list
    /// (`convert this`, `join these`, `what is this file`) and, until now, a
    /// single answer -- every one of them went to `find_files` and came back as
    /// a keyword search. "Convert this pdf to text" looked the disk over for
    /// files whose *names* held the words *pdf*, *to* and *text*, which is a
    /// well-formed answer to a question nobody asked.
    ///
    /// A conversion is not a search, so it is answered as one: `files::convert`
    /// knows what each change costs you (a sheet to text loses its formulas)
    /// and what it plainly cannot do (an archive does not become a PDF). Only
    /// what is *not* a recognised conversion falls through to the filename
    /// search, so the existing "find me the note about..." path is untouched.
    pub(super) fn files_request(&self, what: &str) -> String {
        if let Some(answer) = convert_answer(what) {
            return answer;
        }
        self.find_files(what)
    }

    fn find_files(&self, what: &str) -> String {
        let prepared = match crate::asking::prepare(what) {
            Ok(p) => p,
            // Asked, not guessed. An invented referent retrieves confidently
            // and wrongly, and you cannot tell that from a real answer.
            Err(unsearchable) => return unsearchable.ask(),
        };

        // Each term separately: the index matches filenames, and a filename
        // almost never contains every word of a spoken question.
        //
        // Each term gives its own ranked list; they are merged by Reciprocal
        // Rank Fusion (`bm25::rrf`, k = 60), so a file several terms find
        // rises above one only the first term found. Before, the first term's
        // list simply came first, whatever the others said.
        let mut names: Vec<(String, String)> = Vec::new();
        let mut lists: Vec<Vec<u64>> = Vec::new();
        for term in &prepared.terms {
            let mut list = Vec::new();
            for e in self.index.search(term) {
                let at = match names.iter().position(|(p, _)| *p == e.path) {
                    Some(i) => i,
                    None => {
                        names.push((e.path.clone(), e.name.clone()));
                        names.len() - 1
                    }
                };
                list.push(at as u64);
            }
            lists.push(list);
        }
        let found: Vec<(String, String)> = crate::bm25::rrf(&lists, crate::bm25::RRF_K)
            .into_iter()
            .map(|(i, _)| names[i as usize].clone())
            .collect();

        let mut answer = if found.is_empty() {
            // Filenames matched nothing. Before giving up, look *inside* the
            // documents and code -- "the thing about the budget" rarely has
            // the word "budget" in its name. This is the expensive search: it
            // reads files off the disk, so it runs only now, once the cheap
            // pass over filenames has already come back empty. Skipped when
            // there is no indexing config, because then the size cap and the
            // roots that bound the read do not exist.
            let inside: Vec<crate::index::ContentHit> = match self.cfg.indexing.as_ref() {
                Some(idx_cfg) => {
                    // All the terms at once: the content search ranks by
                    // BM25, which scores a chunk holding several of them
                    // above one holding a single word, and that only works
                    // if it sees them together.
                    self.index.search_content(&prepared.terms.join(" "), idx_cfg, 3)
                }
                None => Vec::new(),
            };

            // Says what it looked for. "I couldn't find anything" leaves you
            // unable to tell a bad search from an empty disk.
            let caveat = match self.index.missed.caveat() {
                Some(c) => format!(" {c}"),
                None => String::new(),
            };

            if inside.is_empty() && self.index_load.is_loading() {
                // Not "nothing matches": the list isn't all here yet.
                crate::index::STILL_READING.to_string()
            } else if inside.is_empty() {
                // "Found nothing" and "could not look" are different answers,
                // and the index already knows which one this was.
                format!(
                    "Nothing in the {} indexed files matches {}.{}",
                    self.index.entries.len(),
                    prepared.searched_for(),
                    caveat
                )
            } else {
                // Nothing was *named* for it, but something *says* it. Show
                // the line each was found on, so you can tell a real match
                // from a word that happened to appear.
                let shown: Vec<String> = inside
                    .iter()
                    .take(3)
                    .map(|h| format!("{} -- {}", h.cite, h.excerpt))
                    .collect();
                format!(
                    "No filename matches {}, but it's written inside {}.{}",
                    prepared.searched_for(),
                    shown.join("; "),
                    caveat
                )
            }
        } else {
            // What each one is, not just what it's called. A search that
            // returns five names you can't open is a list, not an answer —
            // and `files::Sort` knows which of them needs something you may
            // not have.
            let shown: Vec<String> = found
                .iter()
                .take(5)
                .map(|(_, n)| {
                    let sort = crate::files::Sort::of(n);
                    if sort.readable_offline() {
                        n.clone()
                    } else {
                        format!("{n} (needs {})", sort.needs())
                    }
                })
                .collect();
            let more = found.len().saturating_sub(shown.len());
            let tail = if more > 0 {
                format!(" and {more} more")
            } else {
                String::new()
            };
            format!(
                "Searching {} — {}{}.",
                prepared.searched_for(),
                shown.join(", "),
                tail
            )
        };

        // Two questions in one utterance retrieve the average of two topics
        // and the best match for neither. Said rather than silently dropped.
        if prepared.is_more_than_one_question() {
            answer.push_str(&format!(
                " That was two questions — I answered the first. The other was: {}",
                prepared.also.join("; ")
            ));
        }
        answer
    }
}

/// The first letter a capital.
fn capital_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// "What did you do overnight?", "how was tonight's backup?": asking about
/// something, not asking for it later. "Can you back up tonight?" is a
/// request, and still waits.
fn asks_about_what_happened(said: &str) -> bool {
    const ASKING: &[&str] = &["what", "what's", "whats", "who", "why", "how", "where", "which", "did", "was", "were", "is", "are", "has", "have"];
    let first = said.split_whitespace().next().unwrap_or("").to_lowercase();
    let first = first.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
    ASKING.contains(&first)
}
