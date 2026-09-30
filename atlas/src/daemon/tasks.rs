//! A request of several parts (30 Sep 2026): worked through step by step
//! (`taskloop`), or -- when the parts don't lean on each other -- side by
//! side (`streams`). Both carry out a step the same way, through
//! `act_on`: anything that needs your OK is asked, never done; anything long
//! goes to the crew and reports back.

use super::*;

/// The daemon as the loop's hands.
struct DaemonHands<'d, 'a> {
    d: &'d mut Daemon<'a>,
    said: String,
}

impl crate::taskloop::Hands for DaemonHands<'_, '_> {
    fn act(&mut self, call: &brain::ToolCall) -> crate::taskloop::Outcome {
        self.d.act_on_call(call, &self.said)
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
    pub(super) fn act_on_call(&mut self, call: &brain::ToolCall, said: &str) -> crate::taskloop::Outcome {
        use crate::taskloop::Outcome;
        let Some(intent) = crate::intent::from_tool(&call.name, &call.arguments, said) else {
            return Outcome::Failed(format!("There's no tool called {} that I may use.", call.name));
        };
        self.act_on(&intent, true)
    }

    /// Carry out one step. `from_model`: the model chose it (a tool call),
    /// rather than the phrases matching the words -- then a consequential
    /// action is always asked about first (`brain::model_must_ask`), as a
    /// single turn does.
    pub(super) fn act_on(&mut self, intent: &Intent, from_model: bool) -> crate::taskloop::Outcome {
        use crate::taskloop::Outcome;
        if self.handover().stance.handed_over() {
            if let Some(refusal) = self.handed_over_refusal(intent) {
                return Outcome::Failed(refusal);
            }
        }
        let call = crate::policy::classify_with_policy(intent, &self.memory, &self.cfg.policy);
        let call = if from_model && brain::model_must_ask(intent) { Decision::max(call, Decision::RequireApproval) } else { call };
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
    /// answer (`taskloop::run`).
    pub(super) fn work_through(&mut self, llm: std::sync::Arc<dyn brain::Llm>, said: &str, mut turn: brain::Turn, t: u64) -> brain::Decision {
        let plan = crate::taskloop::parts(said);
        turn.tools = self.tools_for_parts(&plan);
        turn.stable_tools = 1;
        self.streams = plan
            .iter()
            .map(|p| crate::streams::Stream { part: p.clone(), state: crate::streams::State::Running, said: String::new(), at: t })
            .collect();
        let run = {
            let mut hands = DaemonHands { d: self, said: said.to_string() };
            crate::taskloop::run(&*llm, &turn, &plan, &mut hands, crate::taskloop::MAX_STEPS)
        };
        self.log.info(&format!(
            "worked through {} step(s) of a {}-part request: {}",
            run.steps.len(),
            plan.len(),
            run.verdict.plain()
        ));
        // Where each part stands now, for "what are you working on".
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
        brain::Decision { intent: Intent::Say(run.reply.clone()), say: run.reply, model: brain::Reached::Yes }
    }

    /// A request whose parts don't lean on each other, each part worked out
    /// at the same time as the others (`streams`): the phrases first, and
    /// the model -- two parts at once, one per slot -- for the rest; then
    /// each part carried out, and the answers said together.
    pub(super) fn work_side_by_side(
        &mut self,
        llm: std::sync::Arc<dyn brain::Llm>,
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
        let started = std::time::Instant::now();
        let talking = self.talking_guard();
        let decided: Vec<Option<brain::Decision>> = {
            let parser = &self.parser;
            let turns = &turns;
            let llm = &*llm;
            crate::streams::side_by_side(parts.len(), &|i| {
                let turn = turns[i].as_ref()?;
                let brain = Brain { llm, fallback: parser, voice: Some((persona, register)) };
                Some(brain.converse(turn, &mut |_| true))
            })
        };
        drop(talking);
        self.log.info(&format!(
            "worked {} parts side by side ({} asked of the model at once) in {}ms",
            parts.len(),
            asked,
            started.elapsed().as_millis()
        ));
        let mut streams: Vec<crate::streams::Stream> = Vec::new();
        for (i, p) in parts.iter().enumerate() {
            let (intent, from_model) = match (&settled[i], decided.get(i).cloned().flatten()) {
                (Some(known), _) => (known.clone(), false),
                (None, Some(d)) => (d.intent, d.model == brain::Reached::Yes),
                (None, None) => (Intent::Unknown(p.clone()), false),
            };
            let (state, said) = match intent {
                Intent::Say(s) => (crate::streams::State::Done, crate::backed::without_unbacked_claims(&s, false)),
                Intent::Unknown(_) => (crate::streams::State::Failed, format!("I couldn't work out how to {p}.")),
                i => match self.act_on(&i, from_model) {
                    crate::taskloop::Outcome::Done(s) => (crate::streams::State::Done, s),
                    crate::taskloop::Outcome::Started(s) => (crate::streams::State::Running, s),
                    crate::taskloop::Outcome::NeedsYou(s) => (crate::streams::State::NeedsYou, s),
                    crate::taskloop::Outcome::Failed(s) => (crate::streams::State::Failed, s),
                },
            };
            streams.push(crate::streams::Stream { part: p.clone(), state, said, at: t });
        }
        let reply = crate::streams::merged(&streams);
        self.streams = streams;
        brain::Decision { intent: Intent::Say(reply.clone()), say: reply, model: brain::Reached::Yes }
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
