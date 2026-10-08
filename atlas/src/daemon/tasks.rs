//! A request of several parts (30 Sep 2026): worked through step by step
//! (`taskloop`), or -- when the parts don't lean on each other -- side by
//! side (`streams`). Both carry out a step the same way, through
//! `act_on`: anything that needs your OK is asked, never done; anything long
//! goes to the crew and reports back.

use super::*;

/// Said when a request of several steps comes while another is being
/// worked through: one at a time, so two don't interleave their steps.
const STILL_WORKING: &str = "I'm still working through your last request of several steps. Ask me this again when \
                             it's done, or say \"stop everything\" to drop that one.";

/// What the loop's worker sends the daemon (`work_through`).
pub(crate) enum LoopNews {
    /// Carry out this step and send back what it did.
    Act(brain::ToolCall),
    /// A step finished: said as it happens.
    Progress(String),
    /// The loop is over.
    Done(crate::taskloop::Run),
    /// Carry out this part of a request worked side by side (`from_model`:
    /// the model chose it) and send back what it did.
    ActIntent(Intent, bool),
    /// Every part of a request worked side by side, done or started: the
    /// streams, how many parts the model was asked, and how long it took.
    Parts(Vec<crate::streams::Stream>, usize, u64),
}

/// A request of several steps being worked through off the loop.
pub(crate) struct TaskLoop {
    said: String,
    plan: Vec<String>,
    rx: std::sync::mpsc::Receiver<LoopNews>,
    reply: std::sync::mpsc::Sender<crate::taskloop::Outcome>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
    started: std::time::Instant,
    /// Something written by someone else -- a mail, a web page, a document,
    /// another program's answer -- has been read into this loop. From then
    /// on the model may still read and answer, but anything that sends,
    /// posts, presses, changes or reaches out is asked about first: text
    /// it read can be instructions in disguise (`brain::reads_outside_text`;
    /// 5 Oct 2026 audit, Q2, the "lethal trifecta").
    tainted: bool,
    /// The crew job whose typed completion the dependent worker is awaiting.
    waiting_for: Option<u64>,
    /// Kept-file path retained while the edit and original decisions are pending.
    waiting_for_media: Option<String>,
}

impl TaskLoop {
    /// In words, for a restart to name: what was asked and how far it got.
    pub(crate) fn in_words(&self) -> String {
        if self.plan.is_empty() {
            format!("\"{}\"", self.said)
        } else {
            format!("\"{}\" ({} steps planned)", self.said, self.plan.len())
        }
    }
}

impl Drop for TaskLoop {
    /// Atlas closing, or the loop dropped: the worker stops at its next step.
    fn drop(&mut self) {
        self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
        crate::doorbell::ring();
    }
}

/// The loop's hands, on the worker: each step is carried out by the daemon,
/// on its loop, and the outcome sent back.
struct WorkerHands {
    tx: std::sync::mpsc::Sender<LoopNews>,
    rx: std::sync::mpsc::Receiver<crate::taskloop::Outcome>,
}

impl crate::taskloop::Hands for WorkerHands {
    fn act(&mut self, call: &brain::ToolCall) -> crate::taskloop::Outcome {
        if self.tx.send(LoopNews::Act(call.clone())).is_err() {
            return crate::taskloop::Outcome::Failed("Atlas stopped before I could do that.".into());
        }
        self.rx.recv().unwrap_or_else(|_| crate::taskloop::Outcome::Failed("Atlas stopped before I could do that.".into()))
    }
}

/// Watching the loop from the worker: each step sent to be said, and a
/// pause held -- a stop taken -- between steps.
struct WorkerWatch {
    tx: std::sync::mpsc::Sender<LoopNews>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
    paused: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl crate::taskloop::Watch for WorkerWatch {
    fn step_done(&mut self, n: usize, of: usize, step: &crate::taskloop::Step) {
        if let Some(line) = progress_line(n, of, step) {
            let _ = self.tx.send(LoopNews::Progress(line));
        }
    }

    fn stop(&mut self) -> Option<String> {
        use std::sync::atomic::Ordering;
        loop {
            let seen = crate::doorbell::rung();
            if self.stop.load(Ordering::SeqCst) {
                return Some("you asked me to stop.".into());
            }
            if !self.paused.load(Ordering::SeqCst) {
                return None;
            }
            // Rung by a resume or a stop (audit Q14: this polled 10/s).
            crate::doorbell::wait_after(seen, 60_000);
        }
    }
}

/// A finished step as it's said: what it found, or that it didn't work. A
/// step that asks you something or started long work isn't said here -- the
/// loop ends on it and it is the answer.
fn progress_line(n: usize, of: usize, step: &crate::taskloop::Step) -> Option<String> {
    use crate::taskloop::Outcome;
    match &step.outcome {
        Outcome::Done(text) => {
            let text = crate::router::clip_words(text.trim(), 240);
            Some(if text.is_empty() { format!("Step {n} of {of} done.") } else { format!("Step {n} of {of}: {text}") })
        }
        Outcome::Failed(text) => Some(format!("Step {n} didn't work: {}", crate::router::clip_words(text.trim(), 200))),
        Outcome::NeedsYou(_) | Outcome::Started(_) => None,
    }
}

/// How a request of several parts is worked.
pub(super) enum Several {
    /// The parts don't lean on each other: all at once (`streams`).
    SideBySide(Vec<String>),
    /// They do: one step after another, with the model (`taskloop`).
    StepByStep,
}

impl<'a> Daemon<'a> {
    /// Is this a request of several parts, and how should they be worked?
    /// `None` for one thing -- and for words meant to be kept whole: a note,
    /// a message, a translation, dictation (their "and" is part of the text).
    pub(super) fn several_parts(&self, said: &str) -> Option<Several> {
        let whole = self.parser.parse(said);
        if matches!(
            whole,
            Intent::Capture(_) | Intent::Message(_) | Intent::Dictate(_) | Intent::Learn(_) | Intent::Translate(_)
                | Intent::Delegate(_) | Intent::Snippet(_)
        ) {
            return None;
        }
        let parts = crate::taskloop::parts(said);
        // Two or more of the parts must ask for something: "You're built to
        // be you. Can you generate a report on yourself?" is one request with
        // a remark in front of it.
        let asks = |p: &String| crate::taskloop::starts_with_verb(p) || crate::doing::looks_like_an_action(p);
        if parts.iter().filter(|p| asks(p)).count() < 2 {
            return None;
        }
        // These background requests explicitly depend on their result even
        // when the later wording isn't a phrase-matched command ("research
        // this, then tell me what you found"). Keep the dependent plan.
        if matches!(whole, Intent::Research(_) | Intent::ReadDocument(_) | Intent::Unzip(_) | Intent::EditMedia(_))
            && parts[1..].iter().any(|p| crate::taskloop::refers_back(p))
        {
            return Some(Several::StepByStep);
        }
        // A command the phrases matched, with a second request in its
        // argument ("research local models and check my email"): split only
        // when a later part is itself one of Atlas's commands, a different one.
        if !matches!(whole, Intent::Unknown(_))
            && !parts[1..].iter().any(|p| {
                let i = self.parser.parse(p);
                !matches!(i, Intent::Unknown(_)) && std::mem::discriminant(&i) != std::mem::discriminant(&whole)
            })
        {
            return None;
        }
        // "Use my camera and look at me" is one thing said twice: parts that
        // come to the same command are not several.
        let known: Vec<Intent> = parts.iter().map(|p| self.parser.parse(p)).filter(|i| !matches!(i, Intent::Unknown(_))).collect();
        if known.len() == parts.len() && known.windows(2).all(|w| w[0] == w[1]) {
            return None;
        }
        let tops: Vec<Option<String>> = parts.iter().map(|p| self.router.names_for(p, 1).into_iter().next()).collect();
        if tops.iter().all(|t| t.is_some()) && tops.windows(2).all(|w| w[0] == w[1]) {
            return None;
        }
        if let Some(ps) = crate::streams::independent_parts(said) {
            return Some(Several::SideBySide(ps));
        }
        crate::taskloop::is_multi_step(said).then_some(Several::StepByStep)
    }

    /// A tool call from the loop, as the command it names, carried out.
    pub(super) fn act_on_call(&mut self, call: &brain::ToolCall, said: &str, tainted: &mut bool) -> crate::taskloop::Outcome {
        use crate::taskloop::Outcome;
        let Some(intent) = crate::intent::from_tool(&call.name, &call.arguments, said) else {
            return Outcome::Failed(format!("There's no tool called {} that I may use.", call.name));
        };
        self.act_tainted(&intent, true, tainted)
    }

    /// One step of a loop, under its taint (Q2): once outside text has been
    /// read, a step the model chose is asked about unless it only reads or
    /// answers; and a step that reads outside text taints what follows.
    fn act_tainted(&mut self, intent: &Intent, from_model: bool, tainted: &mut bool) -> crate::taskloop::Outcome {
        let force_ask = from_model && *tainted && !brain::safe_after_outside_text(intent);
        let outcome = self.act_on_asking(intent, from_model, force_ask);
        if brain::reads_outside_text(intent) {
            *tainted = true;
        }
        outcome
    }

    /// Carry out one step. `from_model`: the model chose it (a tool call),
    /// rather than the phrases matching the words -- then a consequential
    /// action is always asked about first (`brain::model_must_ask`), as a
    /// single turn does.
    /// ... and, when `force_ask`, asked about first whatever the policy says
    /// (a loop that has read outside text, Q2).
    pub(super) fn act_on_asking(&mut self, intent: &Intent, from_model: bool, force_ask: bool) -> crate::taskloop::Outcome {
        use crate::taskloop::Outcome;
        if self.handover().stance.handed_over() {
            if let Some(refusal) = self.handed_over_refusal(intent) {
                return Outcome::Failed(refusal);
            }
        }
        let call = crate::policy::classify_with_policy(intent, &self.memory, &self.cfg.policy);
        let call = if (from_model && brain::model_must_ask(intent)) || force_ask { Decision::max(call, Decision::RequireApproval) } else { call };
        let call = self.mcp_gate(intent, call);
        match call {
            Decision::AutoProceed | Decision::ProceedAndReport => {}
            _ => {
                let say = match brain::default_say(intent) {
                    s if s.trim().is_empty() => format!("Just to check -- {}.", intent.plain()),
                    s => s,
                };
                let q = format!("{} Go ahead?", say.trim());
                self.session.await_approval(intent.clone(), &q);
                // Another was already waiting (30 Sep 2026): this one queues
                // behind it, and is said as waiting, not as the question.
                if !matches!(&self.session.pending, crate::session::Pending::Approval(i, _) if i == intent) {
                    return Outcome::NeedsYou(format!(
                        "{} That needs your OK too -- I'll ask once you've answered the one before it.",
                        say.trim()
                    ));
                }
                return Outcome::NeedsYou(q);
            }
        }
        let before = self.crew.active() + self.crew.queued();
        let text = self.execute(intent);
        let after = self.crew.active() + self.crew.queued();
        if after > before {
            Outcome::Started(text)
        } else {
            Outcome::Done(text)
        }
    }

    /// The tools for a request of several parts: the capabilities tool, and
    /// the few each part reads like (`router`), never more than
    /// `router::CEILING`.
    pub(super) fn tools_for_parts(&self, parts: &[String]) -> Vec<serde_json::Value> {
        let mut out = vec![crate::router::meta_spec()];
        let per = (crate::router::CEILING - 1).div_ceil(parts.len().max(1)).max(2);
        let apps: Vec<String> = self.cfg.apps.apps.keys().cloned().collect();
        let apps_note = (!apps.is_empty()).then(|| format!("One of: {}.", apps.join(", ")));
        // Other programs' tools the request reads like (`mcp`), as a single
        // turn offers them.
        if self.mcp.any_on() {
            self.mcp.wake();
            for t in self.mcp.tools_for(&parts.join(" "), crate::mcp::MOST_PER_TURN) {
                if !out.contains(&t) && out.len() < crate::router::CEILING {
                    out.push(t);
                }
            }
        }
        for p in parts {
            for e in self.router.for_turn(p, None, per) {
                let note = matches!(e.name.as_str(), "open_app" | "close_app" | "focus_app").then(|| apps_note.as_deref()).flatten();
                let spec = crate::router::compact_spec(e, note);
                if !out.contains(&spec) && out.len() < crate::router::CEILING {
                    out.push(spec);
                }
            }
        }
        out
    }

    /// A request that takes several steps, worked through with the model:
    /// its plan, each step's result fed back, and a finished-or-blocked
    /// answer (`taskloop::run_watched`).
    ///
    /// ## Off the loop (30 Sep 2026)
    ///
    /// It ran here, on the daemon's loop: four model calls and four steps in
    /// a row, and for all of it the hub went unanswered, Pause waited, and
    /// nothing was said until the end. Now the model's side runs on a worker
    /// thread -- the deep model's work when there is one (`deepbrain`) --
    /// and asks the loop to carry out each step (`LoopNews::Act`), which the
    /// tick does between everything else (`take_task_loop_news`): a step
    /// that needs your OK is asked as before, long work still goes to the
    /// crew. Each step is said as it finishes; Pause holds the loop between
    /// steps and "resume" carries it on; "stop everything" ends it there.
    /// What this returns is the plan, said at once; the answer follows.
    pub(super) fn work_through(&mut self, llm: std::sync::Arc<dyn brain::Llm>, said: &str, mut turn: brain::Turn, t: u64) -> brain::Decision {
        // One loop at a time: a second request of several steps while one
        // runs waits its turn rather than two interleaving their steps.
        if self.task_loop.is_some() {
            let s = STILL_WORKING.to_string();
            return brain::Decision { intent: Intent::Say(s.clone()), say: s, model: brain::Reached::NotNeeded };
        }
        let plan = crate::taskloop::parts(said);
        turn.tools = self.tools_for_parts(&plan);
        turn.stable_tools = 1;
        self.streams = plan
            .iter()
            .map(|p| crate::streams::Stream { part: p.clone(), state: crate::streams::State::Running, said: String::new(), at: t })
            .collect();

        let llm = self.background_llm().unwrap_or(llm);
        // Beside the conversation: you may talk with Atlas while it works.
        turn.aside = true;
        let (news_tx, news_rx) = std::sync::mpsc::channel::<LoopNews>();
        let (reply_tx, reply_rx) = std::sync::mpsc::channel::<crate::taskloop::Outcome>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(self.attention.is_paused()));
        let (w_plan, w_stop, w_paused) = (plan.clone(), stop.clone(), paused.clone());
        let spawned = std::thread::Builder::new().name("atlas-steps".into()).spawn(move || {
            let mut hands = WorkerHands { tx: news_tx.clone(), rx: reply_rx };
            let mut watch = WorkerWatch { tx: news_tx.clone(), stop: w_stop, paused: w_paused };
            let run = crate::taskloop::run_watched(&*llm, &turn, &w_plan, &mut hands, crate::taskloop::MAX_STEPS, &mut watch);
            let _ = news_tx.send(LoopNews::Done(run));
        });
        if let Err(e) = spawned {
            let s = format!("I couldn't start working through that: {e}");
            return brain::Decision { intent: Intent::Say(s.clone()), say: s, model: brain::Reached::No };
        }
        self.task_loop = Some(TaskLoop {
            said: said.to_string(),
            plan: plan.clone(),
            rx: news_rx,
            reply: reply_tx,
            stop,
            paused,
            started: std::time::Instant::now(),
            tainted: false,
            waiting_for: None,
            waiting_for_media: None,
        });
        let steps: Vec<String> = plan.iter().enumerate().map(|(i, p)| format!("{}) {p}", i + 1)).collect();
        let s = format!("Working through that in {} steps: {}. I'll say how each one goes.", plan.len(), steps.join("; "));
        brain::Decision { intent: Intent::Say(s.clone()), say: s, model: brain::Reached::Yes }
    }

    /// Stop the request of several steps being worked through, between
    /// steps ("stop everything", Atlas closing). `true` when one was going.
    pub(super) fn stop_task_loop(&mut self) -> bool {
        match self.task_loop.as_ref() {
            Some(tl) => {
                tl.stop.store(true, std::sync::atomic::Ordering::SeqCst);
                crate::doorbell::ring();
                true
            }
            None => false,
        }
    }

    /// Hold it between steps, or let it carry on.
    pub(super) fn hold_task_loop(&mut self, hold: bool) {
        if let Some(tl) = self.task_loop.as_ref() {
            if tl.paused.swap(hold, std::sync::atomic::Ordering::SeqCst) != hold {
                crate::doorbell::ring();
            }
        }
    }

    /// A matching background result releases only its dependent request.
    /// Call after decoding the result and applying any approval question.
    pub(super) fn finish_background_step(&mut self, id: u64, outcome: crate::taskloop::Outcome) -> bool {
        if let Some(tl) = self.task_loop.as_mut() {
            if tl.waiting_for == Some(id) {
                tl.waiting_for = None;
                let _ = tl.reply.send(outcome);
                return true;
            }
        }
        false
    }

    /// Only the matching edit job may park this dependent request for consent.
    pub(super) fn park_media_decision(&mut self, id: u64) -> bool {
        if let (Some(tl), Some((_, _, result))) = (self.task_loop.as_mut(), self.pending_media_keep.as_ref()) {
            if tl.waiting_for == Some(id) {
                tl.waiting_for = None;
                tl.waiting_for_media = Some(result.clone());
                return true;
            }
        }
        false
    }

    /// Release the retained edit step only after its own decisions are resolved.
    pub(super) fn finish_media_decision(&mut self, kept: bool) {
        if let Some(tl) = self.task_loop.as_mut() {
            if let Some(result) = tl.waiting_for_media.take() {
                let outcome = if kept {
                    crate::taskloop::Outcome::Done(format!("Kept edited file: {result}."))
                } else {
                    crate::taskloop::Outcome::Failed("The edit was declined. Dependent steps were not run.".into())
                };
                let _ = tl.reply.send(outcome);
            }
        }
    }

    /// Is a request of several steps being worked through?
    pub fn working_through_steps(&self) -> bool {
        self.task_loop.is_some()
    }

    /// Once a tick: carry out the steps the loop's worker asks for, and say
    /// each step and the answer as they come. Never waits on the worker.
    pub(super) fn take_task_loop_news(&mut self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        let Some(mut tl) = self.task_loop.take() else { return out };
        // A pause holds it at its next step; a resume lets it go on.
        let paused = self.attention.is_paused();
        if tl.paused.swap(paused, std::sync::atomic::Ordering::SeqCst) != paused {
            crate::doorbell::ring();
        }
        if tl.stop.load(std::sync::atomic::Ordering::SeqCst) && (tl.waiting_for.take().is_some() || tl.waiting_for_media.take().is_some()) {
            let _ = tl.reply.send(crate::taskloop::Outcome::Failed("Stopped while waiting for background work.".into()));
        }
        // A model call already in flight can submit a tool while paused.
        // Leave it on the channel until resume; cancellation still drains it.
        if paused && !tl.stop.load(std::sync::atomic::Ordering::SeqCst) {
            self.task_loop = Some(tl);
            return out;
        }
        let mut finished: Option<crate::taskloop::Run> = None;
        let mut parts_done: Option<(Vec<crate::streams::Stream>, usize, u64)> = None;
        let mut gone = false;
        loop {
            match tl.rx.try_recv() {
                Ok(LoopNews::Act(call)) => {
                    self.last_crew_handoff = None;
                    let outcome = if tl.stop.load(std::sync::atomic::Ordering::SeqCst) {
                        crate::taskloop::Outcome::Failed("Stopped before this step.".into())
                    } else {
                        self.act_on_call(&call, &tl.said, &mut tl.tainted)
                    };
                    // The handoff's exact id also identifies a queued or joined
                    // job. Counting workers cannot distinguish those cases.
                    let waiting = self.last_crew_handoff.take().filter(|id| self.crew_links.get(id)
                        .is_some_and(|l| matches!(l.label, "read-file" | "research" | "edit-media")));
                    if let Some(id) = waiting {
                        out.push(outcome.text().to_string());
                        tl.waiting_for = Some(id);
                    } else {
                        let _ = tl.reply.send(outcome);
                    }
                }
                Ok(LoopNews::ActIntent(intent, from_model)) => {
                    let outcome = if tl.stop.load(std::sync::atomic::Ordering::SeqCst) {
                        crate::taskloop::Outcome::Failed("Stopped before this part.".into())
                    } else {
                        self.act_tainted(&intent, from_model, &mut tl.tainted)
                    };
                    let _ = tl.reply.send(outcome);
                }
                Ok(LoopNews::Progress(line)) => out.push(line),
                Ok(LoopNews::Done(run)) => {
                    finished = Some(run);
                    break;
                }
                Ok(LoopNews::Parts(streams, asked, thought_ms)) => {
                    parts_done = Some((streams, asked, thought_ms));
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    gone = true;
                    break;
                }
            }
        }
        if gone {
            out.push("The request of several steps stopped without finishing -- ask me again.".into());
            for s in self.streams.iter_mut().filter(|s| s.state == crate::streams::State::Running) {
                s.state = crate::streams::State::Failed;
            }
            return out;
        }
        if let Some((streams, asked, thought_ms)) = parts_done {
            self.log.info(&format!(
                "worked {} parts side by side ({} asked of the model at once, {}ms), off the loop, in {}ms",
                streams.len(),
                asked,
                thought_ms,
                tl.started.elapsed().as_millis()
            ));
            let reply = self.parts_answer(&streams);
            self.streams = streams;
            if let Some(e) = self.thread.recent.iter_mut().rev().find(|e| e.said == tl.said) {
                e.reply = reply.clone();
            }
            self.persist();
            out.push(reply);
            return out;
        }
        let Some(run) = finished else {
            self.task_loop = Some(tl);
            return out;
        };
        self.log.info(&format!(
            "worked through {} step(s) of a {}-part request in {}ms, off the loop: {}",
            run.steps.len(),
            tl.plan.len(),
            tl.started.elapsed().as_millis(),
            run.verdict.plain()
        ));
        self.finish_steps(&run);
        // The request's line in the conversation ends with the answer, not
        // the plan it started with.
        if let Some(e) = self.thread.recent.iter_mut().rev().find(|e| e.said == tl.said) {
            e.reply = run.reply.clone();
        }
        self.persist();
        let _ = t;
        out.push(run.reply);
        out
    }

    /// Where each part stands once the loop is over, for "what are you
    /// working on".
    fn finish_steps(&mut self, run: &crate::taskloop::Run) {
        for (i, s) in self.streams.iter_mut().enumerate() {
            s.state = match (run.steps.get(i).map(|x| &x.outcome), run.verdict) {
                (Some(crate::taskloop::Outcome::Done(_)), _) => crate::streams::State::Done,
                (Some(crate::taskloop::Outcome::Started(_)), _) => crate::streams::State::Running,
                (Some(crate::taskloop::Outcome::NeedsYou(_)), _) => crate::streams::State::NeedsYou,
                (Some(crate::taskloop::Outcome::Failed(_)), _) => crate::streams::State::Failed,
                (None, crate::taskloop::Verdict::Finished) => crate::streams::State::Done,
                (None, crate::taskloop::Verdict::Started) => crate::streams::State::Running,
                (None, _) => crate::streams::State::Failed,
            };
            if let Some(step) = run.steps.get(i) {
                s.said = step.outcome.text().to_string();
            }
        }
    }

    /// A request whose parts don't lean on each other, each part worked out
    /// at the same time as the others (`streams`): the phrases first, and
    /// the model -- two parts at once, one per slot -- for the rest; then
    /// each part carried out, and the answers said together.
    pub(super) fn work_side_by_side(
        &mut self,
        llm: std::sync::Arc<dyn brain::Llm>,
        said: &str,
        parts: Vec<String>,
        t: u64,
        register: crate::register::Register,
        persona: &Persona,
    ) -> brain::Decision {
        // What the phrases settle, and the turns the model is asked.
        let mut settled: Vec<Option<Intent>> = Vec::new();
        let mut turns: Vec<Option<brain::Turn>> = Vec::new();
        for (i, p) in parts.iter().enumerate() {
            match self.parser.parse(p) {
                Intent::Unknown(_) => {
                    let mut turn = self.conversation_turn(p, t, register, persona, None, "");
                    // One part per slot: the conversation's, then the other.
                    turn.aside = i % crate::streams::SLOTS == 1;
                    settled.push(None);
                    turns.push(Some(turn));
                }
                known => {
                    settled.push(Some(known));
                    turns.push(None);
                }
            }
        }
        let asked = turns.iter().filter(|x| x.is_some()).count();
        // Off the loop too (30 Sep 2026): the parts' model calls ran here,
        // on the daemon's loop, and the hub and Pause waited for the slower
        // of them. Now a worker asks the model -- two parts at once, one per
        // slot, as before -- and hands each part back to be carried out by
        // the tick (`LoopNews::ActIntent`), since carrying out needs the
        // daemon; the answers are said together when the last is in.
        if self.task_loop.is_some() {
            let s = STILL_WORKING.to_string();
            return brain::Decision { intent: Intent::Say(s.clone()), say: s, model: brain::Reached::NotNeeded };
        }
        let (news_tx, news_rx) = std::sync::mpsc::channel::<LoopNews>();
        let (reply_tx, reply_rx) = std::sync::mpsc::channel::<crate::taskloop::Outcome>();
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let paused = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(self.attention.is_paused()));
        // Answering you: the deep model gives way while these run.
        let talking = self.talking_guard();
        let parser = self.parser.clone();
        let persona = persona.clone();
        let (w_parts, w_stop, w_paused) = (parts.clone(), stop.clone(), paused.clone());
        let spawned = std::thread::Builder::new().name("atlas-parts".into()).spawn(move || {
            let started = std::time::Instant::now();
            let decided: Vec<Option<brain::Decision>> = {
                let (parser, turns, llm, persona) = (&parser, &turns, &*llm, &persona);
                crate::streams::side_by_side(w_parts.len(), &|i| {
                    let turn = turns[i].as_ref()?;
                    let brain = Brain { llm, fallback: parser, voice: Some((persona, register)) };
                    Some(brain.converse(turn, &mut |_| true))
                })
            };
            drop(talking);
            let thought_ms = started.elapsed().as_millis() as u64;
            let mut streams: Vec<crate::streams::Stream> = Vec::new();
            for (i, p) in w_parts.iter().enumerate() {
                let (intent, from_model) = match (&settled[i], decided.get(i).cloned().flatten()) {
                    (Some(known), _) => (known.clone(), false),
                    (None, Some(d)) => (d.intent, d.model == brain::Reached::Yes),
                    (None, None) => (Intent::Unknown(p.clone()), false),
                };
                // Paused: held here; stopped: the rest isn't done.
                loop {
                    let seen = crate::doorbell::rung();
                    if !w_paused.load(std::sync::atomic::Ordering::SeqCst) || w_stop.load(std::sync::atomic::Ordering::SeqCst) {
                        break;
                    }
                    crate::doorbell::wait_after(seen, 60_000);
                }
                let (state, said) = match intent {
                    Intent::Say(s) => (crate::streams::State::Done, crate::backed::without_unbacked_claims(&s, false)),
                    Intent::Unknown(_) => (crate::streams::State::Failed, format!("I couldn't work out how to {p}.")),
                    _ if w_stop.load(std::sync::atomic::Ordering::SeqCst) => (crate::streams::State::Failed, format!("Stopped before I could {p}.")),
                    i => {
                        let outcome = if news_tx.send(LoopNews::ActIntent(i, from_model)).is_ok() {
                            reply_rx.recv().unwrap_or_else(|_| crate::taskloop::Outcome::Failed("Atlas stopped before I could do that.".into()))
                        } else {
                            crate::taskloop::Outcome::Failed("Atlas stopped before I could do that.".into())
                        };
                        match outcome {
                            crate::taskloop::Outcome::Done(s) => (crate::streams::State::Done, s),
                            crate::taskloop::Outcome::Started(s) => (crate::streams::State::Running, s),
                            crate::taskloop::Outcome::NeedsYou(s) => (crate::streams::State::NeedsYou, s),
                            crate::taskloop::Outcome::Failed(s) => (crate::streams::State::Failed, s),
                        }
                    }
                };
                streams.push(crate::streams::Stream { part: p.clone(), state, said, at: t });
            }
            let _ = news_tx.send(LoopNews::Parts(streams, asked, thought_ms));
        });
        if let Err(e) = spawned {
            let s = format!("I couldn't start on those: {e}");
            return brain::Decision { intent: Intent::Say(s.clone()), say: s, model: brain::Reached::No };
        }
        self.streams = parts
            .iter()
            .map(|p| crate::streams::Stream { part: p.clone(), state: crate::streams::State::Running, said: String::new(), at: t })
            .collect();
        self.task_loop = Some(TaskLoop {
            said: said.to_string(),
            plan: parts.clone(),
            rx: news_rx,
            reply: reply_tx,
            stop,
            paused,
            started: std::time::Instant::now(),
            tainted: false,
            waiting_for: None,
            waiting_for_media: None,
        });
        let s = match parts.len() {
            2 => format!("Doing both at once: {}, and {}.", parts[0], parts[1]),
            n => format!("Doing all {n} at once: {}.", parts.join("; ")),
        };
        brain::Decision { intent: Intent::Say(s.clone()), say: s, model: brain::Reached::Yes }
    }

    /// The answers of a request worked side by side, said together
    /// (`streams::merged`).
    ///
    /// Two or more waiting for your OK (30 Sep 2026): their questions aren't
    /// said one after another -- only the last would have been answerable.
    /// The rest are said, then how many need your OK and the first of them
    /// (`Session::asking_line`); the others are asked in turn.
    fn parts_answer(&self, streams: &[crate::streams::Stream]) -> String {
        let asking = streams.iter().filter(|s| s.state == crate::streams::State::NeedsYou).count();
        if asking < 2 {
            return crate::streams::merged(streams);
        }
        let others: Vec<crate::streams::Stream> =
            streams.iter().filter(|s| s.state != crate::streams::State::NeedsYou).cloned().collect();
        let rest = crate::streams::merged(&others);
        match self.session.asking_line() {
            Some(q) => format!("{rest} {q}").trim().to_string(),
            None => crate::streams::merged(streams),
        }
    }

    /// After an approval is answered: the next one waiting asked now, after
    /// what the answer did. Not when what was done asked something of its
    /// own -- that is the question now, and the queue waits behind it.
    pub(super) fn and_the_next_approval(&mut self, reply: String) -> String {
        if self.session.is_waiting() {
            return reply;
        }
        match self.session.ask_the_next() {
            Some(q) => format!("{} {q}", reply.trim()).trim().to_string(),
            None => reply,
        }
    }

    /// Several approvals answered at once ("yes to both", "no to the
    /// second"): each one done or left, in the order asked, and whatever was
    /// not answered still asked.
    pub(super) fn answer_several(&mut self, said: &str, answers: Vec<Option<bool>>, t: u64) -> String {
        let all = self.session.all_approvals();
        self.session.pending = crate::session::Pending::Nothing;
        self.session.queued.clear();
        let mut lines: Vec<String> = Vec::new();
        let mut still: Vec<(Intent, String)> = Vec::new();
        for (k, (intent, description)) in all.into_iter().enumerate() {
            let Some(yes) = answers.get(k).copied().flatten() else {
                still.push((intent, description));
                continue;
            };
            if let Some(refusal) = self.handed_over_refusal(&intent) {
                lines.push(refusal);
                continue;
            }
            self.memory.record_approval(crate::session::kind_of(&intent), yes, None);
            let line = if yes {
                let r = self.execute(&intent);
                // The first was what a scheduled job was waiting on.
                if k == 0 {
                    if let Some(jid) = self.pending_job.take() {
                        self.scheduler.approve(jid);
                        self.scheduler.complete(jid, t, &r, !r.starts_with("error"));
                    }
                }
                r
            } else {
                if k == 0 {
                    self.pending_job = None;
                }
                let (what, kind) = crate::person::learn_from_refusal(&description);
                self.person.notice(&what, kind, t);
                format!("Left that alone: {}.", intent.plain().trim_end_matches('.'))
            };
            self.session.record(said, &intent, &line);
            lines.push(line);
        }
        // What wasn't answered keeps its place, behind anything the answers
        // themselves asked.
        for (i, d) in still {
            self.session.await_approval(i, &d);
        }
        let mut reply = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        if let Some(q) = self.session.asking_line() {
            reply = format!("{reply} Still waiting: {q}").trim().to_string();
        }
        self.persist();
        reply
    }

    /// "What are you working on": each part of the last request still going,
    /// and every errand the crew has in hand.
    pub(super) fn what_im_working_on(&self) -> String {
        let errands: Vec<String> = self
            .errand_candidates()
            .iter()
            .filter(|c| self.crew.errands().iter().any(|e| e.id == c.id))
            .map(|c| crate::which_errand::describe(c))
            .collect();
        crate::streams::working_on(&self.streams, &errands)
    }
}
