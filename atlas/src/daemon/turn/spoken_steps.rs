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
//! - **Outside text.** If a part read something someone else wrote (a mail,
//!   a page) and a later part leans on it ("... and message Sam about it") or
//!   is a consequential one, the later part is asked about first -- the same
//!   rule `run_command` applies to a tool the model picks once
//!   `Session::tainted_by_outside` is set. `run_command` clears that flag for
//!   your own words, which is right for a sentence said on its own and wrong
//!   inside one: the part after a mail read is still part of the sentence.
//! - **Stopping.** The first part that needs your OK, asks you something or
//!   fails ends the run. The reply says what was done, what was not, and ends
//!   with the question if there is one, so your yes answers it.

use super::*;
use crate::splitter::{Reading, SpokenStep};

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
        let mut done: Vec<String> = Vec::new();
        let total = steps.len();
        for (n, step) in steps.iter().enumerate() {
            let rest: Vec<&SpokenStep> = steps.iter().skip(n + 1).collect();
            // A later part after outside text was read: asked, not run.
            if n > 0 && self.session.tainted_by_outside && leans_on_outside_text(step) {
                let q = match &step.reading {
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
                            step.words
                        );
                        return finish_spoken_steps(&done, &line, &rest, false);
                    }
                };
                return finish_spoken_steps(&done, &q, &rest, true);
            }
            // "open notepad and then close it": the "it" is what the step
            // before opened, resolved the way a sentence said alone is
            // (`turn_unwatched`), or asked about when nothing is behind it.
            let leans = match &step.reading {
                Reading::Command(i) => argument_of(i)
                    .map(|a| crate::references::argument_leans_on_earlier(&a))
                    .unwrap_or(false),
                _ => false,
            };
            let words = if leans {
                match resolve(&step.words, &self.referents) {
                    Resolution::Resolved { text, .. }
                        if !matches!(self.parser.parse(&text), Intent::Unknown(_)) =>
                    {
                        text
                    }
                    Resolution::Ambiguous(q) => {
                        self.session.ask(&q);
                        return finish_spoken_steps(&done, &q, &rest, true);
                    }
                    _ => step.words.clone(),
                }
            } else {
                step.words.clone()
            };
            let reply = self.run_command(&words, t);
            let asked = !matches!(self.session.pending, Pending::Nothing);
            let failed = !asked && (self.last_turn_failed || reads_as_not_done(&reply));
            let reply = if reply.trim().is_empty() {
                format!("{}: started.", capital_first(&step.reading.plain()))
            } else {
                reply.trim().to_string()
            };
            if asked || (failed && n + 1 < total) {
                return finish_spoken_steps(&done, &reply, &rest, asked);
            }
            done.push(reply);
        }
        done.join(" ")
    }
}

impl<'a> Daemon<'a> {
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
}

/// Would this part act on what an earlier part read? A part that refers back
/// to it ("about it", "what you found") or a consequential one (a message,
/// handing over, changing code) is; reading, answering, opening an app or
/// setting a reminder of your own is not.
fn leans_on_outside_text(step: &SpokenStep) -> bool {
    match &step.reading {
        Reading::Command(i) => {
            !brain::safe_after_outside_text(i)
                && (crate::taskloop::refers_back(&step.words) || brain::model_must_ask(i))
        }
        Reading::Text => true,
        Reading::Reminder | Reading::MoveWindow | Reading::Weather => false,
    }
}

/// A reply that says it didn't happen.
fn reads_as_not_done(reply: &str) -> bool {
    let l = reply.to_lowercase();
    [
        "i couldn't",
        "i could not",
        "couldn't find",
        "i can't",
        "i cannot",
        "failed",
        "i don't have",
        "i haven't set",
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
