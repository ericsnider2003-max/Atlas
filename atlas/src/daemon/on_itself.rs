//! Atlas working on itself: being told it got something wrong, corrections and
//! lessons, refiling notes, and fixing its own code -- proving a fix in a copy,
//! landing it, or parking it for you.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// You told Atlas it got something wrong.
    ///
    /// One correction is a note; two on separate occasions is a rule. That is
    /// `revise`'s third rule and the one everything hangs on -- a rule made
    /// from one irritable evening is worse than no rule.
    ///
    /// Three things happen here, and the second is the one the flight recorder
    /// was built for:
    ///
    /// 1. The correction is recorded, whether or not it earns an edit. A note
    ///    that was never promoted is still the evidence for the next one.
    /// 2. `trace::blame` ties it to the last model call, so a bad answer can
    ///    be traced back to the exact request that produced it. `grade_last`
    ///    marks that call bad in the same breath.
    /// 3. If it has now been said twice, the edit is *offered* -- never
    ///    written silently. Atlas changing its own instructions without saying
    ///    so is the one kind of self-improvement you cannot audit.
    pub fn got_it_wrong(&mut self, said: &str, t: u64) -> String {
        let about = crate::revise::subject_of(said);
        if about.is_empty() {
            return "What did I get wrong?".into();
        }

        // What Atlas actually did, so the correction records the thing rather
        // than a paraphrase of it.
        let did = self.session.turns.last().map(|t| t.reply.clone()).unwrap_or_default();

        // The model call that produced it, marked before anything else --
        // whether or not this correction ever earns an edit, the call that
        // caused it is worth finding later. This is `trace`'s second stated
        // reason for existing, and it had no caller until now.
        self.trace.blame("brain", &about);
        // Kept, not just marked in memory: the grade outlives a restart.
        if let Some(id) = self.trace.calls.iter().rev().find(|c| c.asked_by == "brain").map(|c| c.id) {
            if id != 0 {
                self.grade_call(id, false, "you corrected it");
                // What you said, and what Atlas did with it: the example a
                // correction makes. The brain's standing instructions are the
                // same every time and aren't kept.
                let words = self.session.turns.last().map(|t| crate::trace::Words {
                    system: String::new(),
                    user: t.said.clone(),
                    reply: t.reply.clone(),
                });
                self.keep_words(id, words.as_ref());
            } else {
                self.trace.grade_last("brain", false);
            }
        }

        // Right after a walk-through, "that didn't work" is about the
        // procedure, not a lesson for the brain: the symptom becomes a snag
        // the procedure knows next time, and a blameless look back is filed.
        let walked = self
            .session
            .turns
            .last()
            .filter(|turn| turn.action == "walk_through")
            .map(|turn| turn.said.clone());
        if let Some(walked) = walked {
            let book = crate::knowhow::Knowhow::load(&self.store);
            if let Some(p) = book.for_request(&walked, true) {
                let snag = crate::knowhow::Snag {
                    looks_like: said.trim().to_string(),
                    cause: "not known yet".into(),
                    fix: "see the look-back note filed when it happened".into(),
                };
                crate::knowhow::Knowhow::learn_and_keep(&self.store, &p.id, snag);
                let note = crate::knowhow::look_back(p, said.trim(), None);
                let dir = self.notes_dir();
                crate::heard!(std::fs::create_dir_all(&dir));
                let path = dir.join(format!("look-back-{}-{t}.md", p.id));
                let filed = std::fs::write(&path, note).is_ok();
                return format!(
                    "Sorry that didn't work. I've added it to what I know goes wrong with {}{} — no blame, just why and what changes. Why do you think it happened?",
                    p.goal,
                    if filed { format!(", and filed a look back at {}", path.display()) } else { String::new() }
                );
            }
        }

        let Some(wanted) = crate::revise::wanted_in(said) else {
            // "That's wrong" is a signal to ask, not a lesson to file. Filing
            // it produces a rule that forbids one thing and teaches none.
            self.pending_correction = Some(said.to_string());
            self.session.ask("What should I have done instead?");
            return "What should I have done instead?".into();
        };

        self.record_correction(&about, &did, &wanted, t)
    }

    /// The answer to "what should I have done instead?".
    pub(super) fn correction_wanted(&mut self, wanted: &str, t: u64) -> String {
        let Some(original) = self.pending_correction.take() else {
            return "Nothing to correct.".into();
        };
        let about = crate::revise::subject_of(&original);
        let did = self.session.turns.last().map(|t| t.reply.clone()).unwrap_or_default();
        if wanted.trim().len() <= 2 {
            return "Alright — I've noted it went wrong, without a rule.".into();
        }
        self.record_correction(&about, &did, wanted.trim(), t)
    }

    /// Record it, and offer the edit when it has earned one.
    fn record_correction(&mut self, about: &str, did: &str, wanted: &str, t: u64) -> String {
        // `Session::started` is this sitting's identity: two corrections in
        // one sitting are one correction, which is `revise`'s third rule --
        // restating a complaint in the same breath is emphasis, not a second
        // occasion.
        let session = self.session.started;
        // Kept as a case the self-test says again every run, so the same
        // mistake can't come back unnoticed (`regressions`).
        // The request that went wrong: the latest turn that isn't the
        // complaint itself or Atlas asking what it should have done.
        let wrong_turn = self
            .session
            .turns
            .iter()
            .rev()
            .find(|t| {
                let r = t.reply.to_lowercase();
                !t.said.trim().is_empty()
                    && t.said.trim() != wanted.trim()
                    && !r.contains("what should i have done")
                    && !looks_like_a_complaint(&t.said)
            })
            .map(|t| (t.said.clone(), t.reply.clone()));
        if let Some((said, wrong)) = wrong_turn {
            let mut cases: Vec<crate::regressions::Case> = self.store.load(crate::regressions::FILE);
            crate::regressions::add(
                &mut cases,
                crate::regressions::Case {
                    said,
                    wrong,
                    wanted: wanted.to_string(),
                    command: String::new(),
                    source: crate::regressions::Source::Correction,
                    at: t,
                },
            );
            let _ = self.store.save(crate::regressions::FILE, &cases);
        }
        let c = crate::revise::Correction::new(about, did, t, session).wanting(wanted);
        let earned = self.mending.heard(c);
        let _ = self.store.save("mending", &self.mending);

        match earned {
            Some(edit) => {
                let n = crate::nudge::offer_to_mend(&edit);
                let offer = crate::proactive::from_nudge(&n);
                self.pending_edit = Some(edit);
                self.session.ask(&offer.message);
                let message = offer.message.clone();
                self.pending_offer = Some(offer);
                message
            }
            // First time. Noted, not promoted, and said plainly -- silence
            // here reads as "it did not hear me", which is how you end up
            // saying it a third time.
            None => "Noted. If you tell me that again I'll make it a rule.".into(),
        }
    }

    /// Write the lesson down, having been told to.
    ///
    /// The edit goes into `Mending::applied`, which `context()` reads on every
    /// turn -- so a `Home::HowTo` lesson is genuinely in front of the model
    /// next time rather than filed somewhere nothing opens.
    /// Whether a lesson is waiting to be written down.
    ///
    /// Exists so a test can assert it reached `apply_lesson`'s real branch
    /// rather than its "nothing waiting" early return. A test for a failure
    /// path that never enters the path passes on every behaviour.
    pub fn pending_edit_for_test(&self) -> Option<&crate::revise::Edit> {
        self.pending_edit.as_ref()
    }

    pub fn apply_lesson(&mut self) -> String {
        let Some(edit) = self.pending_edit.take() else {
            return "Nothing waiting to be written down.".into();
        };
        let where_ = edit.home.plain();
        let becomes = edit.becomes.clone();
        self.mending.applied(edit);
        // Checked, because the sentence below is a promise.
        //
        // This was `let _ = save(...)` followed by "I'll keep to it", said to
        // a person who has just corrected Atlas twice about the same thing.
        // A failed write there is the correction lost **and** the person told
        // it was kept -- so they stop repeating it. That is strictly worse
        // than losing it silently: Atlas would be spending their trust to
        // hide its own failure.
        if let Err(e) = self.store.save("mending", &self.mending) {
            self.log.warn(&format!("couldn't write down a lesson for {where_}: {e}"));
            return format!(
                "I'll keep to \"{becomes}\" for now, but I couldn't write it to {where_}, \
                 so it won't survive a restart — tell me again if I go back on it. ({e})"
            );
        }
        format!("Written to {where_}: \"{becomes}\". I'll keep to it.")
    }

    /// How often Atlas repeated a mistake it had already written down a fix
    /// for. The only number in `revise` that matters.
    /// What Atlas will do on its own, and where it still asks first.
    ///
    /// Read straight from the `earned` record -- the same ledger `should_act`
    /// consults before every action -- so the answer is not a policy stated in
    /// prose but the live rope each kind of work has actually earned.
    /// `may_act_alone` splits the two lists; `rope` says which of the two
    /// self-acting levels a kind reached; `what_would_earn_more` says what is
    /// still standing in the way for the rest.
    pub(super) fn what_i_can_do_alone(&self) -> String {
        use crate::earned::Kind;
        let mut alone: Vec<String> = Vec::new();
        let mut still_ask: Vec<Kind> = Vec::new();
        for kind in Kind::all() {
            if self.earned.may_act_alone(kind) {
                alone.push(format!("{} — {}", kind.title(), self.earned.rope(kind).plain()));
            } else {
                still_ask.push(kind);
            }
        }
        let mut out = String::new();
        if alone.is_empty() {
            out.push_str(
                "Right now I check with you first about everything — I haven't done \
                 enough of any one kind of work yet to have earned more.",
            );
        } else {
            out.push_str("What I'll do on my own:");
            for line in &alone {
                out.push_str(&format!("\n- {line}"));
            }
        }
        if !still_ask.is_empty() {
            if !alone.is_empty() {
                out.push_str("\n\nStill checking with you first:");
            } else {
                out.push('\n');
            }
            for kind in &still_ask {
                // A recent run of wrong answers is literally what is standing
                // in the way, and it is the same signal `rope` demotion reads
                // (`wrong_lately_in >= 2`). Surfacing it here completes the
                // "what's still in the way" handler rather than adding a
                // feature — report text only, no behaviour change.
                let recent = self.earned.wrong_lately(*kind);
                let note = if recent >= 2 {
                    format!(" (wrong {recent} times lately)")
                } else {
                    String::new()
                };
                out.push_str(&format!(
                    "\n- {} — {}{}",
                    kind.title(),
                    self.earned.what_would_earn_more(*kind),
                    note
                ));
            }
        }
        out
    }

    /// "How much do you know?" -- the size of the knowledge store, and the
    /// promise that it does not grow while you are not asking.
    ///
    /// The store is `self.known`: everything research has folded in, merged
    /// rather than duplicated. `consolidate::size_note` turns its count into
    /// the reassurance the module was built to give -- roughly what it costs,
    /// and that searching it is instant -- which until now nothing a person
    /// could say ever reached.
    pub(super) fn knowledge_store_size(&self) -> String {
        crate::consolidate::size_note(self.known.len())
    }

    /// You told Atlas a captured note was filed wrong.
    ///
    /// The one door to `capture::Notebook::correct`, which records a filing
    /// correction -- changing the note's kind, or adding a handle you would
    /// reach for it by -- and marks the note confirmed so the fix outlives the
    /// guess. Capture only ever added notes, so before this the guess it makes
    /// about a note's kind could never be put right by anything a person said.
    ///
    /// The correction lands on the most recent note, which is the one you are
    /// almost always talking about: you file a thought, hear where it went,
    /// and say "no, that's a task" or "put that under the roof job" in the same
    /// breath. Written down before Atlas says it was, for the same reason
    /// capture is: a correction lost to a failed write is a correction you
    /// think you made.
    pub(super) fn refile_note(&mut self, correction: &str) -> String {
        let Some(id) = self.notebook.notes.last().map(|n| n.id) else {
            return "There's no recent note to refile.".into();
        };
        let kind = refile_kind_from(correction);
        let handle = if kind.is_none() { refile_handle_from(correction) } else { None };
        if kind.is_none() && handle.is_none() {
            return "Tell me how to refile it -- as a task, or under a name.".into();
        }
        if !self.notebook.correct(id, kind, handle.as_deref()) {
            return "I couldn't find that note to refile.".into();
        }
        if let Err(e) = self.notebook.save(&self.store) {
            return format!("I refiled it but couldn't write it down ({e}).");
        }
        match (kind, &handle) {
            (Some(k), _) => format!("Refiled -- that's {} now.", k.plain()),
            (None, Some(h)) => format!("Filed that under {h}."),
            _ => "Refiled.".into(),
        }
    }

    pub(super) fn mending_line(&self) -> String {
        let m = &self.mending;
        if m.applied.is_empty() {
            return match m.heard.len() {
                0 => "You haven't had to correct me yet.".into(),
                1 => "One correction noted, not yet a rule.".into(),
                n => format!("{n} corrections noted, none said twice yet."),
            };
        }
        let mut out = format!(
            "{} rule{} from {} correction{}",
            m.applied.len(),
            if m.applied.len() == 1 { "" } else { "s" },
            m.heard.len(),
            if m.heard.len() == 1 { "" } else { "s" }
        );
        if m.repeats_after_fix > 0 {
            out.push_str(&format!(
                ". I've gone back on one {} times — that's the number that matters",
                m.repeats_after_fix
            ));
        } else {
            out.push_str(". I haven't gone back on any of them");
        }
        out.push('.');
        out
    }

    /// Atlas working on Atlas, one turn at a time.
    ///
    /// ## What this replaces
    ///
    /// The arm this came out of read the session, called `may_start`, and
    /// printed whatever it said. `may_start` refuses while `work.thought` is
    /// `None`, and **nothing in the tree ever set `work.thought`** — so the
    /// answer to `WorkOnYourself` was always, for every input, *"Before I
    /// touch anything: no diagnosis — what's actually wrong, and what would
    /// prove it fixed?"*, with no way to answer it. See the note above
    /// `pipeline::Work::record_thought` for the full list of what that made
    /// unreachable.
    ///
    /// ## What happens now
    ///
    /// The diagnosis is collected one answer per turn, because the session
    /// survives turns in the store. Four questions, then Atlas **runs** the
    /// named proving test against the tree as it is and uses the result — a
    /// proof that passes already is refused here, which is the check the whole
    /// pipeline is built around and the one nobody can answer honestly from
    /// memory.
    ///
    /// ## What is still not wired, and why
    ///
    /// After the diagnosis the next stage is Build, and building means
    /// something has to **write a candidate fix**. That does not exist:
    /// `Sandbox::create` has no production caller anywhere, `pending_landing`
    /// is never pushed to, and there is no code-writing step for
    /// `sandbox::plan` to plan from. So `Stage::Build` reports what it is
    /// waiting for rather than pretending to do it. Everything downstream —
    /// `review`, the four landing checks, the grant — is wired and tested and
    /// waiting on that one piece.
    /// Is a diagnosis being collected right now?
    ///
    /// While one is, every `WorkOnYourself` turn is an answer rather than a
    /// command — see the guards on the two arms above `work_on_myself`.
    pub(super) fn mid_self_work(&self) -> bool {
        self.selfwork.as_ref().is_some_and(|s| s.still_needs().is_some())
    }

    pub(crate) fn work_on_myself(&mut self, what: &str) -> String {
        let cfg = self.tools_cfg().pipeline.clone();
        if !cfg.enabled {
            return "Working on myself is switched off.".into();
        }
        // The Self-repair switch in Settings is `self_work.enabled`. Until 1
        // Oct 2026 only `pipeline.enabled` was read here, so switching
        // Self-repair off left it running cargo and drafting fixes (research
        // report, Stage 1 item 2). Every way in -- asked, the weekly look,
        // the hub's "Have a go" -- comes through here.
        if !self.tools_cfg().self_work.enabled {
            return "Self-repair is switched off in Settings, so I'm not working on my own code. \
                    Turn it on there and ask again -- nothing I change lands without your yes."
                .into();
        }

        // Dropping it. Before anything else, because a person who has
        // changed their mind is otherwise answering the next question in a
        // conversation they have already left — and with no way out, a
        // session sits in the store asking the same thing at every
        // `WorkOnYourself` for the life of the install.
        let said = what.trim().to_lowercase();
        let dropping = said == "stop"
            || said == "forget it"
            || said == "never mind"
            || said == "nevermind"
            || said == "leave it"
            || said == "drop it"
            || said == "cancel";
        if dropping {
            return match self.selfwork.as_mut() {
                Some(s) if s.work.stage != crate::pipeline::Stage::Abandoned => {
                    // Marked, not deleted. The goal and the answers already
                    // given stay readable, `what_next` will not pick an
                    // `Abandoned` session up, and `still_needs` stops asking
                    // — which is the part that matters, because a dropped
                    // session that kept asking its next question would be no
                    // different from one that was never dropped.
                    s.work.abandon();
                    let goal = s.goal.clone();
                    self.persist();
                    format!("Dropped \"{goal}\". Nothing was changed.")
                }
                _ => "I wasn't working on anything.".into(),
            };
        }

        // A session part-way through its diagnosis reads this turn as the
        // answer to the question it asked, not as a new goal. Without this
        // the first answer would be taken for a new goal and start the
        // questions again, for ever.
        let mid_diagnosis = self.mid_self_work();
        if !mid_diagnosis {
            // An abandoned session is not resumed, even for the same goal.
            // Asking again for something you dropped is a new decision, and
            // resuming would put it back at whatever stage it was dropped in
            // with no way to get past it — `still_needs` is silent on an
            // abandoned session, so the diagnosis could never be finished.
            let resuming = self.selfwork.as_ref().is_some_and(|s| {
                s.work.stage != crate::pipeline::Stage::Abandoned
                    && (s.goal == what || what.trim().is_empty())
            });
            if !resuming {
                self.selfwork = Some(crate::selfwork::Session::new(what, 0));
            }
        }

        let scfg = self.tools_cfg().self_work.clone();
        let root = crate::selfwork::source_root(&self.tools_cfg().self_work).unwrap_or_default();

        // Collecting the diagnosis.
        {
            let Some(session) = self.selfwork.as_mut() else {
                return "Nothing to work on.".into();
            };
            if session.work.thought.is_none() {
                if mid_diagnosis {
                    if let Some(question) = session.heard(what) {
                        return question.to_string();
                    }
                } else if let Some(question) = session.still_needs() {
                    // A fresh session, asking the first question.
                    return format!("Right — \"{}\". {question}", session.goal);
                }
            }
        }

        // All four answers are in and there is no diagnosis yet, so the
        // proving test decides.
        let needs_the_proof = self
            .selfwork
            .as_ref()
            .is_some_and(|s| s.work.thought.is_none() && s.still_needs().is_none());
        // 30 Sep 2026: an installed Atlas has no source to change, and ran
        // cargo in whatever folder it was started from -- often Windows'
        // own. The diagnosis is kept; the building needs a source checkout.
        if needs_the_proof && !crate::selfwork::is_a_source_checkout(&root) {
            return "I've got the diagnosis down, but there's no copy of my source code on this computer for me to change, \
                    so it's kept on the Improvements page and a fix comes to you as an update. If you do have my source, \
                    say where in Settings (Self-improvement -> where my source is)."
                .to_string();
        }
        if needs_the_proof {
            let named = self
                .selfwork
                .as_ref()
                .and_then(|s| s.proving_test().map(str::to_string))
                .unwrap_or_default();
            // On the crew, not the loop (research report, 1 Oct 2026: this
            // ran cargo on the daemon thread for up to two minutes). Waited
            // on briefly, so a quick answer still comes back in this reply;
            // a slow one is said when it lands (`finish_proof_check`).
            let slot = self.proof_check_done.clone();
            if let Ok(mut s) = slot.lock().or_else(crate::crash::unpoison) {
                *s = None;
            }
            let (n2, scfg2, root2, slot2) = (named.clone(), scfg.clone(), root.clone(), slot.clone());
            let work: crate::crew::Work = Box::new(move |_c: &crate::crew::Control| {
                let outcome = crate::selfwork::run_the_proof(&n2, &scfg2, &root2);
                if let Ok(mut s) = slot2.lock().or_else(crate::crash::unpoison) {
                    *s = Some((n2, outcome));
                }
                Ok(String::new())
            });
            if !self.hand_off("proof-check", crate::store::now(), work, Some(named.clone()), super::SpeakPolicy::ViaWatcher) {
                return "I'm already checking that test -- I'll tell you what it shows.".into();
            }
            let until = std::time::Instant::now() + std::time::Duration::from_secs(PROOF_WAIT_SECS);
            while std::time::Instant::now() < until {
                if let Some(said) = self.finish_proof_check() {
                    return said;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            return format!("Checking whether {named} fails right now -- that runs the test, so it takes a minute. I'll tell you what it shows.");
        }

        // Past the diagnosis. The stage machine decides.
        let Some(session) = self.selfwork.clone() else {
            return "Nothing to work on.".into();
        };

        // Re-checked rather than trusted, even though `record_thought` ran
        // `is_thought_through` before letting the stage advance. A session
        // survives in the store between runs, so the one being read here may
        // have been written by an older version with a weaker check — and
        // this is the gate that says work does not start without a diagnosis
        // that holds up. Cheap, and the alternative is trusting a file.
        if let Err(why) = session.may_start() {
            return format!("Before I touch anything: {why}");
        }

        match session.work.what_next(&cfg) {
            // At Build with nothing built yet, this is the step that used to
            // stop and ask you for the change. Now Atlas drafts it, proves it
            // in a copy of the tree, and — if it holds — records the build and
            // review and stages it for you to land. Any other `Do` (still
            // collecting the diagnosis) just says what the stage needs.
            crate::pipeline::Next::Do(crate::pipeline::Stage::Build)
                if session.work.thought.is_some() =>
            {
                self.attempt_own_fix()
            }
            // The review found something: answer that note, and build again
            // (E2). The refinement has to answer a note the review actually
            // made, or `record_refinement` refuses it; the round count is
            // `max_rounds`' to limit.
            crate::pipeline::Next::Do(crate::pipeline::Stage::Refine) => self.refine_own_fix(),
            crate::pipeline::Next::Do(_) => self.what_the_stage_needs(),
            crate::pipeline::Next::Blocked(why) => why,
            crate::pipeline::Next::Land(what_changes) => self.land_it(&what_changes),
            // Round after round and still not clean: stuck, said plainly (F7).
            crate::pipeline::Next::HandOver(why) => {
                let stuck = crate::route::stuck_on(&format!("fixing \"{}\"", session.goal), &why);
                match session.take_it_elsewhere() {
                    Some(_) => format!("{stuck} I'll take it into a conversation and see if something comes back."),
                    None => stuck,
                }
            }
        }
    }

    /// Draft a fix for the diagnosed cause, prove it in a copy of the tree, and
    /// — if it holds — record the build and review and stage it for you to
    /// land. This is what closes the self-improvement loop: everything before
    /// it (diagnose, prove the test fails) and after it (review against the
    /// cause, land with a backup on your say-so) was already wired; the piece
    /// that was missing was writing a candidate to put through them.
    ///
    /// Nothing here touches your real files. The candidate is written into a
    /// copy of the tree and the proof and the suite run there; only `land_it`,
    /// with your go-ahead, writes to the tree you run.
    ///
    /// The compile-and-test in a copy is a cold build of the whole tree, so it
    /// is slow and runs on your machine — the same real-toolchain step as
    /// `run_the_proof`, and like it, exercised on real hardware rather than in
    /// a unit test.
    /// Answer the review's first blocking note, then draft again with that
    /// note as the instruction.
    fn refine_own_fix(&mut self) -> String {
        let note = self
            .selfwork
            .as_ref()
            .and_then(|s| s.work.review.as_ref())
            .and_then(|r| r.blockers().first().map(|n| (*n).clone()));
        let Some(note) = note else {
            return self.what_the_stage_needs();
        };
        let r = crate::pipeline::Refinement { answers: note.kind, what_changed: format!("redrafted to answer: {}", note.what) };
        if let Some(s) = self.selfwork.as_mut() {
            if let Err(why) = s.work.record_refinement(r) {
                return format!("I can't refine that: {why}.");
            }
        }
        self.attempt_own_fix_with(&note.what)
    }

    fn attempt_own_fix(&mut self) -> String {
        self.attempt_own_fix_with("")
    }

    fn attempt_own_fix_with(&mut self, instruction: &str) -> String {
        let scfg = self.tools_cfg().self_work.clone();
        let Some(thought) = self.selfwork.as_ref().and_then(|s| s.work.thought.clone()) else {
            return "I need the diagnosis before I can write anything.".into();
        };
        let root = crate::selfwork::source_root(&self.tools_cfg().self_work).unwrap_or_default();

        // Which file holds the cause?
        let current = crate::selfwork::files_named(&thought.where_, &root);
        if current.is_empty() {
            return format!(
                "The diagnosis puts the cause in \"{}\", but I can't tell which file that is from \
                 here. Name it — \"it's in src/settings.rs\" — and I'll write the fix.",
                thought.where_
            );
        }

        let Some(llm) = self.llm.clone() else {
            return "I've got the diagnosis and the file, but I need a model to draft the fix and \
                    none is configured."
                .into();
        };

        // Drafting the fix and proving it -- a model call, then the whole
        // suite twice in a copy of the tree -- is minutes of work. It ran
        // here, on the loop, until 1 Oct 2026: Atlas stopped listening for
        // as long as it took, and one hung test hung Atlas (research report,
        // Stage 1 item 1). Now it's an errand on the crew; the result is taken
        // in when it ends (`finish_own_fix`).
        let slot = self.self_fix_done.clone();
        let instruction = instruction.to_string();
        let symptom = thought.symptom.clone();
        let (strategy, handoff, consult) = {
            let tc = self.tools_cfg();
            (tc.strategy.clone(), tc.handoff.clone(), tc.consult.clone())
        };
        let work: crate::crew::Work = Box::new(move |c: &crate::crew::Control| {
            let candidate = crate::selfwork::draft_fix(&thought, &instruction, &current, llm.as_ref())
                .map_err(|why| format!("I couldn't draft a fix for that yet: {why}."))?;
            if c.stopping() {
                return Err("stopped before proving the fix".into());
            }
            let mut candidate = candidate;
            let mut proved = prove_in_a_copy(&thought, &candidate, &scfg, &root);
            // One draft didn't hold: the fix loop works the proving test from
            // several angles, the real test output fed back each time, before
            // giving up (`fixloop`; research report, Stage 2 item 15 -- it
            // was a command-line tool only). Whatever it finds is proved the
            // same way: the whole suite, and the mutation check.
            if proved.is_err() && !c.stopping() && !thought.proof.trim().is_empty() {
                let mut test: Vec<String> = scfg.test_command.split_whitespace().map(String::from).collect();
                test.push(thought.proof.trim().to_string());
                let job = crate::fixloop::Job { folder: root.clone(), test, goal: thought.symptom.clone() };
                let mut counsel = crate::fixloop::ModelCounsel::new(llm.as_ref());
                if let Ok(o) = crate::fixloop::run(&job, &mut counsel, &crate::roots::tmp_dir().join("fix"), &strategy, &handoff, &consult) {
                    if o.solved && !o.changes.is_empty() {
                        let from_loop: Vec<crate::selfwork::Edit> = o
                            .changes
                            .iter()
                            .map(|(path, content)| crate::selfwork::Edit {
                                path: path.clone(),
                                content: content.clone(),
                                reason: format!("worked out in {} attempts by the fix loop", o.attempts),
                            })
                            .collect();
                        let again = prove_in_a_copy(&thought, &from_loop, &scfg, &root);
                        if again.is_ok() {
                            candidate = from_loop;
                            proved = again;
                        }
                    }
                }
            }
            let ok = proved.is_ok();
            if let Ok(mut s) = slot.lock().or_else(crate::crash::unpoison) {
                *s = Some(SelfFixDone { thought, current, candidate, proved });
            }
            if ok { Ok("the fix is proven".into()) } else { Ok("the draft didn't hold".into()) }
        });
        let t = crate::store::now();
        if self.hand_off("self-fix", t, work, Some(symptom.clone()), super::SpeakPolicy::Always) {
            format!(
                "Working on a fix for \"{symptom}\" in the background: drafting it, then proving it in a \
                 copy of my code (the whole test suite, so it takes a while). I'll tell you when it's \
                 ready -- nothing lands without your yes."
            )
        } else {
            "I'm already working on a fix -- I'll tell you when it's ready.".into()
        }
    }

    /// A day's mutation sweep: the next file of Atlas's own source, broken
    /// one function at a time by cargo-mutants on the crew, its survivors
    /// kept (`mutation::KEPT`) for `signals::from_survivors`. Quiet: what it
    /// finds reaches you as a self-repair recommendation, not a sentence.
    pub(super) fn maybe_mutation_sweep(&mut self, t: u64) {
        const EVERY_SECS: u64 = 24 * 3600;
        // Asked every tick, due once a day. Until 2 Oct 2026 this found the
        // source first -- a walk of up to 4000 folders under your home, every
        // tick -- and only then read the date: ~80 ms of CPU every second or
        // two, the daemon's whole idle cost on Eric's laptop (measured: the
        // main thread in 50-90 ms bursts, "the rest" of each tick). Now the
        // date first, from memory; the disk once a quarter hour at most; the
        // source only on the day it's due.
        if t < self.next_sweep_look {
            return;
        }
        if self.rehearsal || !self.tools_cfg().self_work.enabled {
            self.next_sweep_look = t + 900;
            return;
        }
        let last: u64 = self.store.load("mutation_sweep_at");
        if t.saturating_sub(last) < EVERY_SECS {
            self.next_sweep_look = (last + EVERY_SECS).min(t + 900);
            return;
        }
        let root = crate::selfwork::source_root(&self.tools_cfg().self_work).unwrap_or_default();
        if !root.join("Cargo.toml").is_file() || !root.join("src").is_dir() {
            // No source on this computer: look again in an hour, not a tick.
            self.next_sweep_look = t + 3600;
            return;
        }
        let _ = self.store.save("mutation_sweep_at", &t);
        let mut files: Vec<String> = std::fs::read_dir(root.join("src"))
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).filter(|n| n.ends_with(".rs")).collect())
            .unwrap_or_default();
        files.sort();
        if files.is_empty() {
            return;
        }
        let next: usize = self.store.load("mutation_sweep_next");
        let file = format!("src/{}", files[next % files.len()]);
        let _ = self.store.save("mutation_sweep_next", &(next + 1));
        let store = self.store.clone();
        let work: crate::crew::Work = Box::new(move |_c: &crate::crew::Control| {
            if !crate::mutation::available(&root) {
                return Ok("the mutation sweep needs cargo-mutants, which isn't installed".into());
            }
            let out = crate::roots::tmp_dir().join("mutation-sweep");
            crate::heard!(std::fs::remove_dir_all(&out));
            let args: Vec<String> = ["mutants", "-f", file.as_str(), "--no-shuffle", "--jobs", "2", "--timeout", "120", "--output"]
                .iter()
                .map(|s| s.to_string())
                .chain(std::iter::once(out.display().to_string()))
                .collect();
            // unheard-ok: returns `(bool, String)`, not a Result
            let _ = crate::sandbox::run_within("cargo", &args, &[], &root, 3 * 3600, 2000);
            let found = crate::mutation::read_survivors(&out);
            let mut kept: Vec<crate::mutation::Survivor> = store.load(crate::mutation::KEPT);
            kept.retain(|s| s.file != file);
            let n = found.len();
            kept.extend(found);
            store.save(crate::mutation::KEPT, &kept).map_err(|e| e.to_string())?;
            Ok(format!("mutation sweep of {file}: {n} change{} no test noticed", if n == 1 { "" } else { "s" }))
        });
        // unheard-ok: returns `bool`, not a Result
        let _ = self.hand_off("mutation-sweep", t, work, None, super::SpeakPolicy::ViaWatcher);
    }

    /// The proving test's run came back: what it means for the diagnosis,
    /// said. `None` while it's still running (or already said).
    pub(super) fn finish_proof_check(&mut self) -> Option<String> {
        let (named, outcome) = self.proof_check_done.lock().or_else(crate::crash::unpoison).ok()?.take()?;
            let Some(session) = self.selfwork.as_mut() else {
                return Some("Nothing to work on.".into());
            };
            match outcome {
                crate::selfwork::ProofToday::PassesAlready => {
                    // Not stored as a diagnosis, and the cause is not rewound
                    // either: the answers are fine and the *proof* is the
                    // wrong one.
                    session.diagnosing.proof = None;
                    return Some(format!(
                        "{named} passes already, so it isn't testing this — whatever the fix \
                         turns out to be, that test would stay green through it. What would \
                         fail right now?"
                    ));
                }
                crate::selfwork::ProofToday::CouldNotRun(why) => {
                    return Some(format!("I couldn't find out whether {named} fails: {why}."));
                }
                crate::selfwork::ProofToday::Fails
                | crate::selfwork::ProofToday::NotWrittenYet => {
                    let not_there =
                        outcome == crate::selfwork::ProofToday::NotWrittenYet;
                    if let Err(why) = session.accept_diagnosis(true) {
                        return Some(format!("That doesn't hold up yet: {why}"));
                    }
                    let mut said = if not_there {
                        format!(
                            "{named} doesn't exist yet, so there's nothing passing — writing it \
                             and watching it fail is the first thing I'd do. "
                        )
                    } else {
                        format!("{named} fails now, so it's testing the right thing. ")
                    };
                    said.push_str(&self.what_the_stage_needs());
                    Some(said)
                }
            }
    }

    /// A self-fix errand ended: take in what it found, and say it.
    pub(super) fn finish_own_fix(&mut self) -> Option<String> {
        let done = self.self_fix_done.lock().or_else(crate::crash::unpoison).ok()?.take()?;
        let scfg = self.tools_cfg().self_work.clone();
        let SelfFixDone { thought, current, candidate, proved } = done;
        Some(match proved {
            Ok((build, changes, mutation_note)) => {
                let behaviour_note = mutation_note.map(|n| format!(" {n}")).unwrap_or_default();
                let passing = build.tests_after;
                let touched = crate::selfwork::lines_touched(&current, &candidate);
                let review = crate::pipeline::review(&thought, &build, &[]);
                let clean = review.clean();
                self.pending_landing = changes;
                // The behaviour view of what's staged: what will be *different*,
                // read from the tests the change adds and drops rather than from
                // the code. Kept so "what will that change do?" can give the
                // full form, and summarised in a line here before you decide.
                let before: Vec<(String, String)> =
                    current.iter().map(|e| (e.path.clone(), e.content.clone())).collect();
                let after: Vec<(String, String)> =
                    candidate.iter().map(|e| (e.path.clone(), e.content.clone())).collect();
                let effect = crate::plainchange::explain(
                    &crate::plainchange::diff_of(&before, &after),
                    &thought.symptom,
                );
                let behaviour = crate::plainchange::spoken(&effect);
                self.pending_change_effect = Some(effect);
                if let Some(s) = self.selfwork.as_mut() {
                    if let Err(why) = s.work.record_build(build) {
                        return Some(format!("The fix built but didn't hold up: {why}."));
                    }
                    crate::heard!(s.work.record_review(review));
                }
                self.persist();
                if clean {
                    let size = if touched > scfg.large_change_lines {
                        format!(" It's a big change ({touched} lines) — worth reading first.")
                    } else {
                        String::new()
                    };
                    format!(
                        "Done — I fixed \"{}\" where the cause was, {passing} tests pass, and the \
                         review is clean.{size}{behaviour_note} It's staged with a backup; say \"go ahead\" and I'll \
                         land it.\n\n{behaviour}",
                        thought.symptom
                    )
                } else {
                    format!(
                        "I've got a change that passes, but the review isn't clean yet — it may be \
                         fixing the symptom rather than the cause in {}. Worth a look before you \
                         land it.\n\n{behaviour}",
                        thought.where_
                    )
                }
            }
            Err(why) => format!(
                "That draft didn't hold: {why}. The diagnosis still stands — I'll try another angle \
                 next time you set me on it."
            ),
        })
    }

    /// What the stage a session is in is actually waiting for.
    ///
    /// Says the missing piece rather than the stage's name. "building it next"
    /// is what the old arm said, and it was true and useless — nothing was
    /// going to build it.
    fn what_the_stage_needs(&self) -> String {
        let Some(session) = self.selfwork.as_ref() else {
            return "Nothing of mine is in progress.".into();
        };
        match session.work.stage {
            crate::pipeline::Stage::Thought => {
                session.still_needs().unwrap_or("Still thinking that one through.").to_string()
            }
            crate::pipeline::Stage::Build | crate::pipeline::Stage::Refine => {
                // The loop is closed now: the next step is Atlas drafting the
                // fix, proving it in a copy, and staging it. Said as what
                // happens next rather than as the stage's name.
                format!(
                    "The diagnosis holds up: {}. Next I'll write the fix where the cause is, prove \
                     it in a copy of the tree — the proving test passes, nothing else breaks — \
                     and stage it for you. Nothing touches your files until you say land it. Set \
                     me on it and I'll draft it.",
                    session
                        .work
                        .thought
                        .as_ref()
                        .map(|t| t.cause.clone())
                        .unwrap_or_else(|| session.goal.clone())
                )
            }
            crate::pipeline::Stage::Review => {
                "Built. Reviewing it against what the diagnosis said the cause was.".into()
            }
            crate::pipeline::Stage::Implement => {
                "Reviewed and clean. Say go ahead and I'll put it in.".into()
            }
            crate::pipeline::Stage::Done => {
                format!("\"{}\" is done and landed.", session.goal)
            }
            crate::pipeline::Stage::Abandoned => {
                format!("I stopped on \"{}\".", session.goal)
            }
        }
    }

    /// Put a change that passed onto the machine — or say why it didn't.
    ///
    /// This is "if it passes, it gets fixed then and there", and the gate in
    /// front of it is "a fix that doesn't change or limit what Atlas can do".
    /// `selfwork::what_holds_it_back` is that constraint made mechanical:
    /// paths Atlas may not edit, a file you changed meanwhile, the six
    /// paper-over shapes, and anything the review blocked on.
    ///
    /// Every reason is reported, not just the first — a change with three
    /// problems learned one at a time takes three rounds, and the second and
    /// third are usually the informative ones.
    fn land_it(&mut self, what_changes: &str) -> String {
        let Some(session) = self.selfwork.clone() else {
            return "Nothing of mine is waiting to land.".into();
        };
        let scfg = self.tools_cfg().self_work.clone();
        let changes = self.pending_landing.clone();
        if changes.is_empty() {
            // Nothing was built in a sandbox, so there is nothing to copy.
            // Said plainly rather than reporting a successful landing of
            // nothing, which is how "it's done" comes to mean nothing.
            return format!("{what_changes} Nothing is built yet, so there's nothing to land.");
        }

        // The project root, which is where Atlas runs from. `may_edit` reads
        // project-relative paths and `sandbox::plan` produces absolute ones.
        let root = crate::selfwork::source_root(&self.tools_cfg().self_work).unwrap_or_default();
        let held = crate::selfwork::what_holds_it_back(
            &changes,
            session.work.review.as_ref(),
            &scfg,
            &self.tools_cfg().self_grant,
            &root,
        );
        if !held.is_empty() {
            let why: Vec<String> = held.iter().map(|h| h.plain()).collect();
            return why.join(" ");
        }

        // The old version of each file goes here first. A fix Atlas landed on
        // its own has to be something you can put back without asking it.
        let keep = self.store.root().join("landed-over");
        match crate::selfwork::land(&changes, &keep) {
            Ok(n) => {
                self.pending_landing.clear();
                self.pending_change_effect = None;
                // Marked done before being dropped, so that a session
                // persisted between the copy and the clear comes back as
                // landed rather than as one still waiting at Implement — a
                // stage that would land the same change a second time.
                if let Some(s) = self.selfwork.as_mut() {
                    s.work.landed();
                }
                self.persist();
                self.selfwork = None;
                self.journal.record_at(
                    Act::Scheduled,
                    &format!("fixed on its own: {}", session.goal),
                    true,
                    crate::store::now(),
                );
                format!(
                    "{what_changes} Landed — {n} file{}. The previous version of each is in \
                     {} if you want it back.",
                    if n == 1 { "" } else { "s" },
                    keep.display()
                )
            }
            Err(e) => format!("It passed, but I couldn't put it in place: {e}"),
        }
    }

    /// Set a decision aside for you, rather than asking into an empty room.
    ///
    /// Three things have to be true before this is worth doing, and `mend`
    /// already knew all three:
    ///
    /// * **It has to be answerable without reading code.** If it is not, Atlas
    ///   has not understood the problem well enough to be asking, and the
    ///   honest thing is to say so rather than park a question nobody can
    ///   answer.
    /// * **It has to be a real choice.** One option is an announcement; four
    ///   is asking you to design rather than decide.
    /// * **Nothing is guessed in the meantime.** One thing is set aside and
    ///   the rest carries on around it. Silence costs you a parked task rather
    ///   than a night of work built on a wrong assumption.
    ///
    /// The parked item is a `Blocker::NeedsYourDecision`, which the brief
    /// already surfaces and `Blocker::self_clearing` already knows does not
    /// clear on its own -- being back at the machine is not a decision.
    pub fn park_for_you(&mut self, q: &crate::mend::Question, request: &str, t: u64) -> String {
        if let Err(jargon) = q.answerable_without_code() {
            // Said plainly rather than parked. A question Atlas cannot phrase
            // for you is one it should not be asking yet, and burying that in
            // a backlog item would hide the real problem.
            self.log.warn(&format!("couldn't phrase a question plainly: {}", jargon.join(", ")));
            return "I've hit something I can't do without you, and I can't put it in \
                    plain enough terms to ask. I've left it."
                .into();
        }
        if !q.is_a_real_choice() {
            return "That needs your say-so.".into();
        }

        self.backlog.record(request, q.parked(), t);
        // No explicit save: `run_command` persists at the end of the turn and
        // `Drop` persists on the way out. A third call here would be a third
        // declaration of when state is written, and `Store::save` now skips
        // unchanged content anyway -- so it would usually have written nothing
        // while looking like the thing keeping the question alive.
        q.spoken(&self.called())
    }
}

/// How long a "work on yourself" turn waits for the proving test before
/// saying it'll report back.
const PROOF_WAIT_SECS: u64 = 3;

/// What a self-fix errand hands back (`finish_own_fix`).
pub(crate) struct SelfFixDone {
    thought: crate::pipeline::Thought,
    current: Vec<crate::selfwork::Edit>,
    candidate: Vec<crate::selfwork::Edit>,
    proved: std::result::Result<(crate::pipeline::Build, Vec<crate::sandbox::Change>, Option<String>), String>,
}

/// Write a candidate into a copy of the tree, run the proving test and the
/// suite there, and report back a `Build` plus the `Change`s that would
/// land. The real files are never touched.
///
/// Real hardware only: a cold compile of the whole tree. There is no unit
/// test for it for the same reason `run_the_proof` has none — it *is* the
/// toolchain — and the pieces it leans on (`draft_fix`, `files_named`,
/// `pipeline::review`, `land`) are each tested on their own.
fn prove_in_a_copy(
    thought: &crate::pipeline::Thought,
    candidate: &[crate::selfwork::Edit],
    scfg: &crate::selfwork::SelfWorkConfig,
    root: &std::path::Path,
) -> std::result::Result<(crate::pipeline::Build, Vec<crate::sandbox::Change>, Option<String>), String> {
    // Refuse anything the never-list or its own limits protect, before a
    // single byte is written anywhere.
    for e in candidate {
        if let crate::selfwork::Verdict::Refused(why) = crate::selfwork::may_edit(&e.path, scfg) {
            return Err(format!("I'm not allowed to change {}: {why}", e.path));
        }
    }

    let base = crate::roots::tmp_dir().join("selffix");
    let mut sandbox = crate::sandbox::Sandbox::create(&base, "self-fix")
        .map_err(|e| format!("couldn't make a copy to work in: {e}"))?;

    // Copy the compile inputs into the sandbox. Not `target/` — a cold
    // build is the price of isolation.
    copy_compile_inputs(root, &sandbox.root)
        .map_err(|e| format!("couldn't copy the tree: {e}"))?;

    // How many tests pass before the change.
    let before = crate::selfwork::run_tests(&mut sandbox, scfg);
    let tests_before = before.tests_run;

    // Apply the candidate in the copy.
    for e in candidate {
        sandbox
            .write(&e.path, &e.content)
            .map_err(|err| format!("couldn't write {} into the copy: {err}", e.path))?;
    }

    // The proving test passes now?
    let proof = crate::selfwork::run_the_proof(&thought.proof, scfg, &sandbox.root);
    let proof_passes = proof == crate::selfwork::ProofToday::PassesAlready;

    // And nothing else broke — the whole suite, in the copy.
    let after = crate::selfwork::run_tests(&mut sandbox, scfg);
    let nothing_else_broke = after.passed && after.tests_run + 1 >= tests_before;

    let build = crate::pipeline::Build {
        touched: candidate.iter().map(|e| e.path.clone()).collect(),
        tests_before,
        tests_after: after.tests_run,
        proof_passes,
        nothing_else_broke,
    };

    if !proof_passes {
        return Err(format!("{} still doesn't pass with the change", thought.proof));
    }
    if !nothing_else_broke {
        return Err("something else in the suite broke".into());
    }

    // Does any test notice if the new lines are broken? Only the lines the
    // fix changed are mutated (`mutation`). A fix whose new code no test
    // depends on is a fix nothing proves, so it doesn't land.
    let diff: String = candidate
        .iter()
        .map(|e| {
            let before = std::fs::read_to_string(root.join(&e.path)).unwrap_or_default();
            crate::mutation::diff_of(&e.path.replace('\\', "/"), &before, &e.content)
        })
        .collect();
    let mutation_note = match crate::mutation::check_diff(&sandbox.root, &diff) {
        crate::mutation::Checked::AllCaught => None,
        crate::mutation::Checked::Survivors(s) => {
            let list: Vec<String> = s.iter().take(3).map(|x| x.said()).collect();
            return Err(format!(
                "the tests pass whether or not the new code works -- breaking it on purpose went unnoticed at {}",
                list.join("; ")
            ));
        }
        crate::mutation::Checked::NotRun(why) => Some(format!("I couldn't check that the tests would catch this change breaking: {why}.")),
    };

    // The preview: which sandbox file maps onto which real file.
    let mapping: Vec<(String, std::path::PathBuf)> =
        candidate.iter().map(|e| (e.path.clone(), root.join(&e.path))).collect();
    let changes = sandbox
        .plan(&mapping)
        .map_err(|e| format!("couldn't work out what would change: {e}"))?;
    Ok((build, changes, mutation_note))
}

/// "That's wrong", "no, not that": the complaint, not the request it's about.
fn looks_like_a_complaint(said: &str) -> bool {
    let t = said.trim().to_lowercase();
    ["that's wrong", "thats wrong", "that was wrong", "wrong", "no,", "not that", "that's not", "that is not", "you got it wrong", "that was too", "you should have"]
        .iter()
        .any(|p| t.starts_with(p))
}

