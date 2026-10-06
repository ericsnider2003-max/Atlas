//! Carrying out a command (run_command).
//!
//! Moved out of `turn.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl<'a> Daemon<'a> {
    pub(in crate::daemon) fn run_command(&mut self, said: &str, _t: u64) -> String {
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
        // A wording of yours Atlas has learned (2 Oct 2026, `phrasebook`):
        // "play some tunes", after you once said "no, I meant open
        // Spotify", is done without asking the model to guess again.
        let learned = if !resumed && matches!(self.parser.parse(said), Intent::Unknown(_)) {
            self.learned_route(said, _t)
        } else {
            None
        };
        let mut local = if !resumed
            && learned.is_none()
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
        // Three rough turns running: stop retrying, find the wrong
        // assumption (`persona::spiral_line`).
        self.rough_in_a_row = if register == crate::register::Register::Rough { self.rough_in_a_row + 1 } else { 0 };
        let mut persona = self.persona_now();
        persona.spiral = crate::persona::spiral_line(self.rough_in_a_row).is_some();
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
                _ if learned.is_some() => {
                    let i = learned.clone().unwrap_or(Intent::Unknown(said.to_string()));
                    let say = brain::default_say(&i);
                    brain::Decision { intent: i, say, model: brain::Reached::NotNeeded }
                }
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
            // Done because of a wording you taught it: said as that, so "why
            // did you do that?" names the lesson (and "forget that phrase"
            // is the way out).
            let d = match self.routed_wording() {
                Some(w) if learned.is_some() => crate::why::Decision {
                    at: _t,
                    what: format!("took it as {}", intent.plain()),
                    because: format!("you taught me that \"{w}\" means that -- \"forget that phrase\" undoes it"),
                    instead_of: Some("asking the model to guess".into()),
                    set_by: None,
                },
                _ => d,
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
        // A wording learned only from a rephrase isn't sure yet: anything
        // consequential it leads to is asked about first (`phrasebook`).
        let call = if learned.is_some() && self.routed_unsure() && !matches!(crate::policy::classify(&intent), Decision::AutoProceed) {
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

        // If you have been gone a while, lead with what happened -- unless
        // this reply asks you something. "That's wrong" was answered "While
        // you were away: I've got an update on your machine when you're
        // ready. What should I have done instead?" (self-test, 1 Oct 2026):
        // the question you need to answer, buried under news. The brief
        // waits for the next reply instead.
        let asks_back = reply.trim_end().ends_with('?');
        let reply = match if asks_back { None } else { self.pending_brief.take() } {
            Some(b) => {
                // It is now in the reply, so it has been handed over and can
                // be dropped. This is the only place the outbox is emptied.
                let cfg = self.notify_cfg();
                // unheard-ok: returns `Vec<Note>`, not a Result
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
            None if asks_back => reply,
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
        // Only when the answer IS the same again (30 Sep 2026: "what
        // reminders do I have" after setting three more was told "here it
        // is again" in front of a list that had changed).
        let reply = match self.thread.asked_before(said) {
            Some(before) if !chatted && before.reply.trim() == reply.trim() => {
                format!("You asked this a little earlier -- here it is again. {reply}")
            }
            _ => reply,
        };
        self.fold_if_due(_t);
        self.journal.record_at(Act::Asked, said, true, _t);
        self.persist();
        reply
    }
}
