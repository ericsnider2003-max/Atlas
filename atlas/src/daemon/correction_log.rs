//! Writing the correction log from a turn (23 Oct 2026, `corrlog`).
//!
//! `learning::turn_watched` already knows how every turn ended; the calls
//! here turn that into lines: an approval or rejection of a question Atlas
//! asked, an undo, a correction, and a question it asked because it was
//! unsure, answered. Nothing here changes what a turn says or does, and a
//! write that fails is logged once and goes no further.

use super::*;
use crate::corrlog::{Asked, Context, Facts, From, Kind, Prev};

/// What a turn looked like before it ran, for the log.
pub(super) struct Before {
    /// What each question waiting for a yes or no is about: command and target.
    pub approvals: Vec<(String, Option<String>)>,
    pub unsure: Option<(String, Vec<String>)>,
    pub app: String,
    pub recent: Vec<String>,
    pub reads: u64,
}

impl<'a> Daemon<'a> {
    /// Atlas is asking because it is unsure. `None` for the options keeps
    /// what `context_question` found a moment ago.
    pub(super) fn note_unsure(&self, question: &str, options: Option<Vec<String>>) {
        let mut u = self.corr.unsure.borrow_mut();
        let kept = match (options, u.take()) {
            (Some(o), _) => o,
            (None, Some((_, o))) => o,
            (None, None) => Vec::new(),
        };
        *u = Some((question.to_string(), kept));
    }

    /// Taken before the turn runs. Nothing is looked up while logging is off.
    pub(super) fn log_before(&mut self) -> Before {
        let waiting = self.session.pending.clone();
        let unsure = self.corr.unsure.borrow_mut().take().filter(
            |(q, _)| matches!(&waiting, Pending::Clarification(w) if q.is_empty() || w == q),
        );
        if !self.corr.on {
            return Before {
                approvals: Vec::new(),
                unsure: None,
                app: String::new(),
                recent: Vec::new(),
                reads: 0,
            };
        }
        // The program's file name only: a window title can hold anything.
        let app = self
            .plat
            .active_window()
            .ok()
            .flatten()
            .map(|w| w.process)
            .unwrap_or_default();
        let recent: Vec<String> = self
            .session
            .turns
            .iter()
            .rev()
            .map(|t| t.action.clone())
            .filter(|a| {
                !matches!(
                    a.as_str(),
                    "unknown" | "say" | "ask" | "" | "undo" | "got_it_wrong"
                )
            })
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        Before {
            approvals: {
                let mut a: Vec<(String, Option<String>)> = self
                    .session
                    .all_approvals()
                    .iter()
                    .map(|(i, _)| Self::name_and_detail(i))
                    .collect();
                // "Undo X?" is asked and answered on its own (`pending_undo`).
                if let Some(id) = self.pending_undo {
                    let what = self
                        .history
                        .done
                        .iter()
                        .find(|d| d.id == id)
                        .map(|d| d.what.clone());
                    a.insert(0, ("undo".to_string(), what));
                }
                a
            },
            unsure,
            app,
            recent,
            reads: self.session.outside_reads,
        }
    }

    /// How the sentence was read, as best it can be told afterwards.
    pub(super) fn read_as(
        &self,
        said: &str,
        by_phrase: bool,
        did: bool,
        asked: bool,
    ) -> &'static str {
        if asked {
            "ask"
        } else if !did {
            "none"
        } else if by_phrase || !matches!(self.parser.parse(said), Intent::Unknown(_)) {
            "phrase"
        } else if self.llm.is_some() {
            "model"
        } else {
            "floor"
        }
    }

    /// The command's name and its target, short.
    fn name_and_detail(i: &Intent) -> (String, Option<String>) {
        let name = crate::session::kind_of(i).to_string();
        let detail = match crate::phrasebook::intent_as_tool(i, "") {
            Some((_, arg)) if !arg.trim().is_empty() => Some(arg),
            Some(_) => None,
            None => Some(i.plain()),
        };
        (name, detail)
    }

    fn name_of(i: Option<&Intent>) -> (Option<String>, Option<String>) {
        match i {
            Some(i) => {
                let (n, d) = Self::name_and_detail(i);
                (Some(n), d)
            }
            None => (None, None),
        }
    }

    fn write_log(&mut self, f: Facts) {
        if !self.corr.on {
            return;
        }
        let Some(rec) = crate::corrlog::build(&f) else {
            return;
        };
        if let Some(why) = self.corr.write(&rec) {
            self.log.warn(&why);
        }
    }

    fn prev_of(p: Option<&learning::Outcome>) -> Prev {
        match p {
            Some(p) => Prev {
                said: p.said.clone(),
                reply: p.reply.clone(),
                intent: p
                    .intent
                    .as_ref()
                    .map(|i| crate::session::kind_of(i).to_string()),
            },
            None => Prev::default(),
        }
    }

    /// A correction that wasn't settled by the turn after it is written as
    /// it was: nothing known to be right.
    pub(super) fn flush_held_correction(&mut self) {
        if let Some(h) = self.corr.held.take() {
            if self.corr.on {
                if let Some(why) = self.corr.write(&h) {
                    self.log.warn(&why);
                }
            }
        }
    }

    /// "That's not what I asked": held until the next turn says what was
    /// right, or written as it is when it doesn't.
    pub(super) fn hold_a_correction(
        &mut self,
        said: &str,
        p: &learning::Outcome,
        outside: bool,
        t: u64,
    ) {
        self.flush_held_correction();
        let (did_intent, did_detail) = Self::name_of(p.intent.as_ref());
        let f = Facts {
            kind: Kind::Correction,
            ts: t,
            said: said.to_string(),
            prev: Self::prev_of(Some(p)),
            context: p.ctx.clone(),
            did_intent,
            did_detail,
            right_intent: None,
            right_detail: None,
            from: From::OwnerSaid,
            ask: None,
            outside,
        };
        if self.corr.on {
            self.corr.held = crate::corrlog::build(&f);
        }
    }

    /// The held correction, with what turned out to be right.
    pub(super) fn settle_held_correction(&mut self, right: &Intent) {
        let Some(mut h) = self.corr.held.take() else {
            return;
        };
        let (n, d) = Self::name_and_detail(right);
        h.right.intent = Some(n);
        h.right.detail = d.map(|d| crate::corrlog::redact_text(&d, 80).0);
        h.right.from = From::OwnerAnswer;
        if self.corr.on {
            if let Some(why) = self.corr.write(&h) {
                self.log.warn(&why);
            }
        }
    }

    /// "No, I meant X": what was done, and what X did.
    pub(super) fn log_a_meant_correction(
        &mut self,
        said: &str,
        prev: &learning::Outcome,
        right: Option<&Intent>,
        outside: bool,
        t: u64,
    ) {
        let (did_intent, did_detail) = Self::name_of(prev.intent.as_ref());
        let (right_intent, right_detail) = Self::name_of(right);
        self.write_log(Facts {
            kind: Kind::Correction,
            ts: t,
            said: said.to_string(),
            prev: Self::prev_of(Some(prev)),
            context: prev.ctx.clone(),
            did_intent,
            did_detail,
            right_intent,
            right_detail,
            from: From::OwnerSaid,
            ask: None,
            outside,
        });
    }

    /// Approvals and rejections, an undo, and an answered question: the
    /// lines a finished turn leaves. `p` is the turn before.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn log_the_turn(
        &mut self,
        b: &Before,
        said: &str,
        action: &str,
        intent: Option<&Intent>,
        asked_now: bool,
        missed: bool,
        p: Option<&learning::Outcome>,
        ctx: &Context,
        t: u64,
    ) {
        if !self.corr.on {
            return;
        }
        let outside = self.session.tainted_by_outside || self.session.outside_reads > b.reads;
        let ctx_for =
            |p: Option<&learning::Outcome>| p.map(|p| p.ctx.clone()).unwrap_or_else(|| ctx.clone());

        // Yes or no to what it asked before acting.
        if !b.approvals.is_empty() {
            let answers: Vec<((String, Option<String>), bool)> =
                match crate::session::answers_for_several(said, b.approvals.len()) {
                    Some(a) => b
                        .approvals
                        .iter()
                        .zip(a)
                        .filter_map(|(a, yes)| yes.map(|y| (a.clone(), y)))
                        .collect(),
                    None => {
                        let yes = crate::session::is_yes(said)
                            || crate::grants::span_from_answer(said).is_some();
                        if yes || crate::session::is_no(said) {
                            vec![(b.approvals[0].clone(), yes)]
                        } else {
                            Vec::new()
                        }
                    }
                };
            for ((name, detail), yes) in answers {
                self.write_log(Facts {
                    kind: if yes { Kind::Approval } else { Kind::Rejection },
                    ts: t,
                    said: said.to_string(),
                    prev: Self::prev_of(p),
                    context: ctx_for(p),
                    did_intent: Some(name.clone()),
                    did_detail: detail.clone(),
                    right_intent: yes.then_some(name),
                    right_detail: if yes { detail } else { None },
                    from: if yes { From::Approval } else { From::Rejection },
                    ask: None,
                    outside,
                });
            }
            return;
        }

        // Undone straight after.
        if action == "undo" {
            if let Some(p) = p.filter(|p| {
                p.intent.is_some() && t.saturating_sub(p.at) <= crate::misses::UNDONE_WITHIN_SECS
            }) {
                let (did_intent, did_detail) = Self::name_of(p.intent.as_ref());
                self.write_log(Facts {
                    kind: Kind::Undo,
                    ts: t,
                    said: said.to_string(),
                    prev: Self::prev_of(Some(p)),
                    context: p.ctx.clone(),
                    did_intent,
                    did_detail,
                    right_intent: None,
                    right_detail: None,
                    from: From::Undo,
                    ask: None,
                    outside,
                });
            }
            return;
        }

        // A question it asked because it was unsure, and the answer settled it.
        if let (Some((q, opts)), Some(i)) = (&b.unsure, intent) {
            if !asked_now && !missed && !matches!(action, "ask" | "unknown" | "got_it_wrong") {
                let (name, detail) = Self::name_and_detail(i);
                self.write_log(Facts {
                    kind: Kind::Clarified,
                    ts: t,
                    said: said.to_string(),
                    prev: Self::prev_of(p),
                    context: Context {
                        source: "ask".into(),
                        ..ctx.clone()
                    },
                    did_intent: None,
                    did_detail: None,
                    right_intent: Some(name),
                    right_detail: detail,
                    from: From::OwnerAnswer,
                    ask: Some((q.clone(), opts.clone())),
                    outside,
                });
            }
        }
    }

    /// The words about the log itself: forgetting, stopping, starting.
    pub(super) fn the_log_asked(&mut self, a: Asked) -> String {
        let said = |n: usize, what: &str| match n {
            0 => format!("There was nothing {what} in the corrections log."),
            1 => format!("Done -- I've taken 1 line {what} out of the corrections log."),
            n => format!("Done -- I've taken {n} lines {what} out of the corrections log."),
        };
        let failed = |e: std::io::Error| format!("I couldn't change the corrections log ({e}).");
        match a {
            Asked::Last => match self.corr.forget_that() {
                Ok(n) => said(n, "from just now"),
                Err(e) => failed(e),
            },
            Asked::About(x) => match self.corr.forget_about(&x) {
                Ok(n) => said(n, &format!("about \"{x}\"")),
                Err(e) => failed(e),
            },
            Asked::Everything => match self.corr.forget_everything() {
                Ok(0) => "The corrections log was already empty.".into(),
                Ok(n) => format!("Done -- I've emptied the corrections log ({n} lines)."),
                Err(e) => failed(e),
            },
            Asked::Stop => match self.corr.set_on(&self.store, false) {
                Ok(()) => "Alright -- I've stopped keeping the log of your corrections. What's in it stays until you say \"forget everything\"; \"start learning again\" turns it back on.".into(),
                Err(e) => format!("I've stopped it for now, but couldn't save that ({e}), so it may come back after a restart."),
            },
            Asked::Start => match self.corr.set_on(&self.store, true) {
                Ok(()) => "Alright -- I'm keeping the log of your corrections again.".into(),
                Err(e) => format!("It's on, but I couldn't save that ({e}), so it may be off again after a restart."),
            },
        }
    }
}
