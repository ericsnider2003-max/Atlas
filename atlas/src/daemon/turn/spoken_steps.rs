//! A sentence of several instructions, carried out one at a time (CONV1
//! phase 1, 12 Oct 2026).
//!
//! `splitter::spoken_steps` reads the sentence; this runs what it read. Each
//! part goes through `run_command` exactly as if it had been said on its
//! own, so every gate a single request meets -- the policy, a voice that
//! might not be yours, a reading that was inferred, the handover refusals --
//! is met by each part. Nothing here approves anything.
//!
//! What this adds on top:
//!
//! - **Outside text.** Once a part has read something someone else wrote (a
//!   mail, a page, your messages -- `Session::tainted_by_outside`), every
//!   later part that is destructive (`splitter::destructive`: closes,
//!   deletes, overwrites, sends, pays, or anything the policy would ask
//!   about) or that refers back to what was read ("about it", "what you
//!   found") is held for your yes instead of run -- or, for a text, which has
//!   no approval to wait on, not done. Held: closing an app, a message, mail
//!   sorting or deleting, shutting the workspace, a text, and the like. NOT
//!   held: reading, answering, opening an app, moving a window, a reminder of
//!   your own -- whatever is neither destructive nor refers back.
//!   `run_command` clears the flag for your own words, which is right for a
//!   sentence said on its own and wrong inside one: the part after a mail
//!   read is still part of the sentence. So the run keeps its own `tainted`,
//!   started from the session's flag and set by any part that reads outside
//!   text, checks that (never the session flag a harmless part in between
//!   just cleared), and writes it back to the session after every part, so a
//!   mail read inside a compound still counts for the turns after it.
//! - **Stopping.** Each part ends in a `PartOutcome`, read from what the
//!   turn actually did (an approval or question left pending, the typed
//!   receipt `execute` leaves, whether `execute` was reached at all) and only
//!   then from the reply's words, which err toward stopping. The next part
//!   runs only after a `Done`. The reply says what was done, what was not,
//!   and ends with the question if there is one, so your yes answers it.
//! - **The record.** The conversation keeps the sentence once, as you said
//!   it, with the one reply; the parts are named only in that reply.

use super::*;
use crate::splitter::{Reading, SpokenStep};

/// How one part ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PartOutcome {
    /// Done, or handed to the background crew to do.
    Done,
    /// Waiting on your yes or your answer.
    Asked,
    /// Tried and did not happen.
    Failed,
    /// Never carried out: refused, parked for later, held.
    Refused,
}

impl<'a> Daemon<'a> {
    /// Carry out the parts `splitter::spoken_steps` read, in order, and say
    /// in one reply what was done.
    pub(in crate::daemon) fn run_spoken_steps(
        &mut self,
        said: &str,
        steps: Vec<SpokenStep>,
        t: u64,
    ) -> String {
        if let [one] = steps.as_slice() {
            let reply = self.run_command(&one.words, t);
            // The plain request was what ran; what you said is what the
            // conversation keeps ("hey could you open up chrome for me",
            // not "open chrome").
            self.keep_the_words_said(said, &one.words);
            return reply;
        }
        let thread_had = self.thread.recent.len();
        let turns_had = self.session.turns.len();
        let reply = self.carry_out_parts(&steps, t);
        self.keep_as_one_turn(said, &reply, thread_had, turns_had, t);
        reply
    }

    fn carry_out_parts(&mut self, steps: &[SpokenStep], t: u64) -> String {
        let mut done: Vec<String> = Vec::new();
        // Outside text read so far in this run (see the module comment):
        // `run_command` clears the session flag for every part, because each
        // is your own words, so the run cannot rely on it between parts.
        let mut tainted = self.session.tainted_by_outside;
        for (n, step) in steps.iter().enumerate() {
            let rest: Vec<&SpokenStep> = steps.iter().skip(n + 1).collect();
            // "open notepad and then close it": the "it" is what the step
            // before opened, resolved the way a sentence said alone is
            // (`turn_unwatched`), or asked about when nothing is behind it --
            // before anything is held or run, so a held step is held with its
            // real target, never "it".
            let leans = match &step.reading {
                Reading::Command(i) => argument_of(i)
                    .map(|a| crate::references::argument_leans_on_earlier(&a))
                    .unwrap_or(false),
                _ => false,
            };
            let resolved = if leans {
                match resolve(&step.words, &self.referents) {
                    Resolution::Resolved { text, .. } => match self.parser.parse(&text) {
                        Intent::Unknown(_) => step.clone(),
                        i => SpokenStep {
                            words: text,
                            reading: Reading::Command(i),
                        },
                    },
                    Resolution::Ambiguous(q) => {
                        self.session.ask(&q);
                        return finish_spoken_steps(&done, &q, &rest, true);
                    }
                    Resolution::Unchanged(_) => step.clone(),
                }
            } else {
                step.clone()
            };
            // A later part after outside text was read: asked, not run.
            if n > 0 && tainted && leans_on_outside_text(&resolved) {
                let q = match &resolved.reading {
                    Reading::Command(intent) => {
                        let q = format!(
                            "The step before read something someone else wrote. {} -- go ahead?",
                            capital_first(&intent.plain())
                        );
                        self.session.await_approval(intent.clone(), &q);
                        q
                    }
                    // A text has no approval to wait on: it is left for you
                    // to ask for on its own.
                    _ => {
                        let line = format!(
                            "I didn't do \"{}\": the step before read something someone else wrote, so say it on its own if you still want it.",
                            resolved.words
                        );
                        return finish_spoken_steps(&done, &line, &rest, false);
                    }
                };
                return finish_spoken_steps(&done, &q, &rest, true);
            }
            self.execution_receipt = None;
            let executed_before = self.last_executed.take();
            let reply = self.run_command(&resolved.words, t);
            tainted |= self.session.tainted_by_outside
                || matches!(&resolved.reading, Reading::Command(i) if brain::reads_outside_text(i));
            self.session.tainted_by_outside = tainted;
            let outcome = self.outcome_of_part(&resolved.reading, &reply);
            if self.last_executed.is_none() {
                self.last_executed = executed_before;
            }
            let reply = if reply.trim().is_empty() {
                format!("{}: started.", capital_first(&resolved.reading.plain()))
            } else {
                reply.trim().to_string()
            };
            match outcome {
                PartOutcome::Done => done.push(reply),
                PartOutcome::Asked => return finish_spoken_steps(&done, &reply, &rest, true),
                // The last part's own words say how it went.
                _ if rest.is_empty() => done.push(reply),
                PartOutcome::Failed | PartOutcome::Refused => {
                    return finish_spoken_steps(&done, &reply, &rest, false)
                }
            }
        }
        done.join(" ")
    }

    /// How the part just run ended, from what the turn did: a question or
    /// approval left pending; the receipt `execute` leaves; whether
    /// `execute` was reached at all (it isn't for a refusal, a request parked
    /// for later, or one held for approval). Only when none of that says, the
    /// reply's words -- and those err toward stopping.
    fn outcome_of_part(&self, reading: &Reading, reply: &str) -> PartOutcome {
        if !matches!(self.session.pending, Pending::Nothing) {
            return PartOutcome::Asked;
        }
        if reads_as_not_done(reply) {
            return PartOutcome::Failed;
        }
        if let Reading::Command(_) = reading {
            if let Some((_, receipt)) = &self.execution_receipt {
                return match receipt {
                    crate::taskloop::Outcome::Done(_) | crate::taskloop::Outcome::Started(_)
                        if !reads_as_not_done(reply) =>
                    {
                        PartOutcome::Done
                    }
                    crate::taskloop::Outcome::NeedsYou(_) => PartOutcome::Asked,
                    _ => PartOutcome::Failed,
                };
            }
            if self.last_executed.is_none() {
                return PartOutcome::Refused;
            }
            if self.last_turn_failed {
                return PartOutcome::Failed;
            }
            return PartOutcome::Done;
        }
        // A helper (reminder, window move, text, weather) leaves no receipt:
        // it counts as done only when its reply says it was, in the words it
        // uses when it was. Anything else stops the run.
        if says_the_helper_did_it(reading, reply) {
            PartOutcome::Done
        } else {
            PartOutcome::Failed
        }
    }

    /// The turn just recorded under `ran` (the plain request) is recorded
    /// under `said`, the words you actually used.
    fn keep_the_words_said(&mut self, said: &str, ran: &str) {
        if said == ran {
            return;
        }
        let mut changed = false;
        if let Some(e) = self.thread.recent.last_mut().filter(|e| e.said == ran) {
            e.said = said.to_string();
            changed = true;
        }
        if let Some(turn) = self.session.turns.last_mut().filter(|x| x.said == ran) {
            turn.said = said.to_string();
        }
        if changed {
            self.persist();
        }
    }

    /// The parts each wrote themselves into the conversation as they ran;
    /// the conversation keeps the sentence once instead, as you said it,
    /// with the one reply that named every part.
    fn keep_as_one_turn(
        &mut self,
        said: &str,
        reply: &str,
        thread_had: usize,
        turns_had: usize,
        t: u64,
    ) {
        if self.thread.recent.len() >= thread_had {
            self.thread.recent.truncate(thread_had);
        }
        if self.session.turns.len() >= turns_had {
            self.session.turns.truncate(turns_had);
        }
        self.thread.append(said, reply, None, t);
        self.session.turns.push(crate::session::Turn {
            said: said.to_string(),
            action: "several".into(),
            reply: reply.to_string(),
            at: t,
        });
        self.persist();
    }
}

/// Would this part act on what an earlier part read? A destructive part
/// (`splitter::destructive`: closing, deleting, sending, paying, anything
/// the policy asks about) or one that refers back to it ("about it", "what
/// you found") is; reading, answering, opening an app, moving a window or
/// setting a reminder of your own is not.
fn leans_on_outside_text(step: &SpokenStep) -> bool {
    match &step.reading {
        Reading::Command(i) => {
            !brain::safe_after_outside_text(i)
                && (crate::taskloop::refers_back(&step.words) || crate::splitter::destructive(i))
        }
        Reading::Text => true,
        Reading::Reminder | Reading::MoveWindow | Reading::Weather => false,
    }
}

/// The reply a helper gives when it did the thing: "I'll remind you to ...",
/// "Moved chrome to your left screen.", "Text to Sam: ... tap Send", a
/// forecast with numbers in it.
fn says_the_helper_did_it(reading: &Reading, reply: &str) -> bool {
    let l = reply.trim().to_lowercase();
    match reading {
        Reading::Reminder => l.contains("i'll remind you") || l.contains("i\u{2019}ll remind you"),
        Reading::MoveWindow => l.starts_with("moved ") || l.contains("is already on that screen"),
        Reading::Text => l.starts_with("text to "),
        Reading::Weather => l.chars().any(|c| c.is_ascii_digit()),
        Reading::Command(_) => false,
    }
}

/// A reply that says it didn't happen. A fallback only, for when nothing the
/// turn did says so; it errs toward stopping.
fn reads_as_not_done(reply: &str) -> bool {
    let l = reply.to_lowercase().replace('\u{2019}', "'");
    [
        "couldn't",
        "could not",
        "can't",
        "cannot",
        "won't",
        "will not",
        "not allowed",
        "isn't allowed",
        "unable",
        "no such",
        "refuse",
        "failed",
        "didn't",
        "did not",
        "no app",
        "not found",
        "isn't running",
        "unreachable",
        "i don't have",
        "i haven't",
        "switched off",
        "not set up",
        "isn't set up",
        "error",
    ]
    .iter()
    .any(|w| l.contains(w))
}

/// The reply for a run that stopped at a part: what was done, the stopping
/// part's own words, what was left. A question goes last, so the yes or no
/// that follows answers it.
fn finish_spoken_steps(done: &[String], stopped: &str, rest: &[&SpokenStep], asks: bool) -> String {
    let mut out: Vec<String> = done.to_vec();
    let left = (!rest.is_empty()).then(|| {
        let names: Vec<String> = rest.iter().map(|s| format!("\"{}\"", s.words)).collect();
        if asks {
            format!(
                "I haven't done the rest yet ({}) -- ask me again for that once you've answered.",
                names.join(", ")
            )
        } else {
            format!("I stopped there, so I haven't done {}.", names.join(" or "))
        }
    });
    if asks {
        if let Some(l) = left {
            out.push(l);
        }
        out.push(stopped.trim().to_string());
    } else {
        out.push(stopped.trim().to_string());
        if let Some(l) = left {
            out.push(l);
        }
    }
    out.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::splitter::SpokenStep;

    fn step(words: &str, reading: Reading) -> SpokenStep {
        SpokenStep {
            words: words.into(),
            reading,
        }
    }

    #[test]
    fn splitter_steps_that_act_on_outside_text_are_the_ones_held() {
        // Leans on what was read, or sends something as you: held.
        assert!(leans_on_outside_text(&step(
            "message sam about it",
            Reading::Command(Intent::Message("sam about it".into()))
        )));
        assert!(leans_on_outside_text(&step(
            "text sam saying hi there",
            Reading::Text
        )));
        // Your own: opening an app, a reminder, a question.
        assert!(!leans_on_outside_text(&step(
            "open chrome",
            Reading::Command(Intent::OpenApp("chrome".into()))
        )));
        assert!(!leans_on_outside_text(&step(
            "remind me at 5",
            Reading::Reminder
        )));
        assert!(!leans_on_outside_text(&step(
            "what time is it",
            Reading::Command(Intent::Clock)
        )));
    }

    #[test]
    fn splitter_a_helper_is_done_only_when_it_says_so() {
        assert!(says_the_helper_did_it(
            &Reading::Reminder,
            "Right \u{2014} in 20 minutes, I'll remind you to stretch. (#3)"
        ));
        assert!(says_the_helper_did_it(
            &Reading::MoveWindow,
            "Moved chrome to your left screen."
        ));
        assert!(!says_the_helper_did_it(
            &Reading::MoveWindow,
            "I couldn't move it: no window."
        ));
        assert!(!says_the_helper_did_it(
            &Reading::Reminder,
            "When should I remind you to stretch?"
        ));
        for reply in [
            "Chrome didn't move.",
            "There's no app called that.",
            "Not found.",
            "Chrome isn't running.",
            "The phone is unreachable.",
        ] {
            assert!(reads_as_not_done(reply), "{reply}");
        }
    }

    #[test]
    fn splitter_after_outside_text_anything_destructive_is_held() {
        for (words, i) in [
            ("close notepad", Intent::CloseApp("notepad".into())),
            (
                "delete the noise",
                Intent::SortMail("delete the noise".into()),
            ),
            ("close workspace", Intent::WorkspaceOff),
        ] {
            assert!(
                leans_on_outside_text(&step(words, Reading::Command(i))),
                "{words}"
            );
        }
    }

    #[test]
    fn splitter_a_reply_that_did_not_happen_reads_as_not_done() {
        for reply in [
            "I won't do that while someone else has the laptop.",
            "That's not allowed here.",
            "I was unable to reach it.",
            "There's no such app as that.",
            "I couldn't find it.",
        ] {
            assert!(reads_as_not_done(reply), "{reply}");
        }
        assert!(!reads_as_not_done("Opening chrome."));
    }

    #[test]
    fn splitter_a_stopped_run_says_what_was_not_done_and_ends_on_the_question() {
        let rest = step(
            "open discord",
            Reading::Command(Intent::OpenApp("discord".into())),
        );
        let said = finish_spoken_steps(
            &["Opening notepad.".to_string()],
            "Closing chrome. Go ahead?",
            &[&rest],
            true,
        );
        assert!(said.starts_with("Opening notepad."), "{said}");
        assert!(said.contains("open discord"), "{said}");
        assert!(said.ends_with("Go ahead?"), "{said}");
        let failed = finish_spoken_steps(&[], "I couldn't open it.", &[&rest], false);
        assert!(failed.contains("haven't done"), "{failed}");
    }
}
