//! Learning how you talk, and keeping track of what went wrong (2 Oct 2026,
//! "Atlas doesn't really understand me").
//!
//! Every turn is watched for how it ended (`turn_from` wraps the turn in
//! `watch_the_turn` / `turn_watched`): whether Atlas didn't understand,
//! asked you back, was corrected or was undone (`misses`), and whether this
//! turn put right the one before -- "no, I meant open Spotify", or the same
//! request said another way straight after a miss -- in which case the
//! wording that missed is kept with the action that was right
//! (`phrasebook`). Before the model guesses at a sentence the phrases don't
//! know, the phrasebook is asked (`learned_route`), and a close match is done
//! without the model.
//!
//! Nothing here learns or routes while Atlas is handed over to somebody else:
//! their words are not yours.

use super::*;
use crate::router::clip_words;

/// A correction ("no, I meant ...") is about a turn at most this old.
const CORRECTS_WITHIN_SECS: u64 = 180;
/// The most of the "about them" lines put in front of the model, in
/// characters: people and projects named, and a learned wording.
const ABOUT_THEM_CHARS: usize = 600;

/// How a turn ended.
#[derive(Debug, Clone)]
pub(super) struct Outcome {
    pub said: String,
    /// The action it ran, or `None` when it ended before any action (a
    /// greeting, a question answered on the way in).
    pub intent: Option<Intent>,
    pub reply: String,
    pub at: u64,
    pub missed: Option<crate::misses::Why>,
    pub by_voice: bool,
    /// The learned wording that routed it, when one did.
    pub routed_by: Option<String>,
}

/// What a turn looked like as it started, for telling what it did.
pub(super) struct Watch {
    recorded: u64,
    asked_already: bool,
    correcting: bool,
    pub by_voice: bool,
}

impl<'a> Daemon<'a> {
    /// A spoken turn's words, kept so the turn knows it came by voice
    /// (`heard_through_this_ear` calls this for every one).
    pub fn heard_by_voice(&mut self, said: &str) {
        self.spoken_turn = Some(said.trim().to_string());
    }

    /// Before a watched turn: what the session looked like, and whether it
    /// came by voice. A spoken sentence heard wrong the same way often enough
    /// is mended here, before anything reads it (`misses::mended_hearing`).
    pub(super) fn watch_the_turn(&mut self, said: &str) -> (Watch, Option<String>) {
        let by_voice = self.spoken_turn.take().is_some_and(|s| s == said.trim());
        let mended = if by_voice { crate::misses::mended_hearing(said, &self.misses) } else { None };
        if let Some(m) = &mended {
            self.log.info(&format!("heard \"{said}\", mended to \"{m}\" from mishearings you corrected before"));
        }
        self.routed_by_phrase = None;
        let watch = Watch {
            recorded: self.session.recorded,
            asked_already: matches!(self.session.pending, Pending::Clarification(_)),
            correcting: self.pending_correction.is_some(),
            by_voice,
        };
        (watch, mended)
    }

    /// After a watched turn: write down a miss, learn from a rephrase, an
    /// undo or an answer to "what should I have done instead?", and keep how
    /// it ended for the next turn. Returns the reply, with a line added when
    /// something was learned.
    pub(super) fn turn_watched(&mut self, w: Watch, said: &str, reply: String, t: u64) -> String {
        let handed_over = self.handover().stance.handed_over();
        let did_something = self.session.recorded > w.recorded;
        let intent = if did_something { self.session.last_intent.clone() } else { None };
        let action = intent.as_ref().map(|i| crate::session::kind_of(i).to_string()).unwrap_or_default();
        let asked_now = !w.asked_already && matches!(self.session.pending, Pending::Clarification(_)) && reply.trim_end().ends_with('?');
        let missed = match action.as_str() {
            "unknown" => Some(crate::misses::Why::NotUnderstood),
            "ask" => Some(crate::misses::Why::AskedBack),
            "" if asked_now && self.pending_correction.is_none() => Some(crate::misses::Why::AskedBack),
            _ => None,
        };
        let routed_by = self.routed_by_phrase.take().map(|(w, _)| w);
        let now = Outcome {
            said: said.trim().to_string(),
            intent: intent.clone(),
            reply: reply.clone(),
            at: t,
            missed,
            by_voice: w.by_voice,
            routed_by,
        };
        self.utterance_count = self.utterance_count.saturating_add(1);
        if handed_over || said.trim().is_empty() {
            self.last_outcome = None;
            return reply;
        }
        let prev = self.last_outcome.clone().filter(|p| t.saturating_sub(p.at) <= CORRECTS_WITHIN_SECS);
        let mut reply = reply;
        let mut keep_prev = false;

        // Written down, with what Atlas did. Counted for the self-audit's
        // "not understood" signal too, which had no writer (`signals`).
        if let Some(why) = missed {
            if why == crate::misses::Why::NotUnderstood {
                self.unknown_count = self.unknown_count.saturating_add(1);
                self.last_unknown = said.trim().to_string();
            }
            let did = match why {
                crate::misses::Why::AskedBack => format!("asked \"{}\"", clip_words(reply.trim(), 60)),
                _ => format!("said \"{}\"", clip_words(reply.trim(), 60)),
            };
            self.note_a_miss(said, &did, why, w.by_voice, t);
        }

        // "That's not what I asked" (`got_it_wrong`), about the turn before.
        if action == "got_it_wrong" {
            if let Some(p) = &prev {
                let did = p.intent.as_ref().map(|i| format!("did {}", i.plain())).unwrap_or_else(|| format!("said \"{}\"", clip_words(&p.reply, 60)));
                self.note_a_miss(&p.said, &did, crate::misses::Why::Corrected, p.by_voice, t);
                if let Some(wording) = &p.routed_by {
                    self.drop_a_wrong_lesson(wording);
                }
            }
            // The answer to "what should I have done instead?" is about the
            // same turn: it stays the one a correction is about.
            keep_prev = true;
        }

        // The answer to "what should I have done instead?": when it is
        // something Atlas can do, that wording means it from now on.
        if w.correcting && self.pending_correction.is_none() {
            if let Some(p) = &prev {
                if let Some(i) = self.parser_action(said) {
                    if let Some(line) = self.learn_wording(&p.said, said, &i, crate::phrasebook::Taught::Answered, true, t) {
                        reply = format!("{} {line}", reply.trim_end());
                    }
                }
            }
        }

        // Undone straight after: what was done was the wrong thing.
        if action == "undo" {
            if let Some(p) = prev.as_ref().filter(|p| p.intent.is_some() && t.saturating_sub(p.at) <= crate::misses::UNDONE_WITHIN_SECS) {
                let did = p.intent.as_ref().map(|i| format!("did {}", i.plain())).unwrap_or_default();
                self.note_a_miss(&p.said, &did, crate::misses::Why::Undone, p.by_voice, t);
                if let Some(wording) = &p.routed_by {
                    self.drop_a_wrong_lesson(wording);
                }
            }
        }

        // A rephrase: the turn before wasn't understood, this one said it
        // differently, soon after, and it worked. Not after "which one?" --
        // the next thing said answers the question, and "close it" doesn't
        // mean "close chrome" for good.
        let rephrased = match (&prev, &intent) {
            (Some(p), Some(i)) => {
                p.missed == Some(crate::misses::Why::NotUnderstood)
                    && t.saturating_sub(p.at) <= crate::misses::REPHRASED_WITHIN_SECS
                    && missed.is_none()
                    && crate::phrasebook::learnable_action(i)
                    && !worked_badly(&now.reply)
                    && crate::phrasebook::meant_instead(said).is_none()
                    && names_something(&p.said)
                    // Atlas asked you for something ("Tell me \"Sam's email
                    // is\" ..."): what came next is the answer, not the same
                    // request said better (2 Oct 2026, merge: "email Sam
                    // saying I'll be late" was learned as "Sam's email is").
                    && !asked_you_for_something(&p.reply)
                    && !did_as_it_was_told(&p.reply, said)
            }
            _ => false,
        };
        if rephrased {
            if let (Some(p), Some(i)) = (&prev, &intent) {
                self.check_the_hearing(p, said, t);
                if let Some(line) = self.learn_wording(&p.said, said, i, crate::phrasebook::Taught::Rephrased, false, t) {
                    self.log.info(&line);
                }
            }
        }

        // A learned wording that did its job and wasn't taken back: a
        // rephrase lesson is sure from now on.
        if let Some(p) = &prev {
            if let Some(wording) = &p.routed_by {
                if !matches!(action.as_str(), "undo" | "got_it_wrong") && crate::phrasebook::meant_instead(said).is_none() {
                    self.phrasebook.stood_up(wording);
                    let _ = self.phrasebook.save(&self.store);
                }
            }
        }

        // "No, I meant ..." already left the corrected words as the turn a
        // next correction is about (`meant_this_instead`).
        if self.last_outcome.as_ref().is_some_and(|o| crate::phrasebook::meant_instead(said).as_deref() == Some(o.said.as_str())) {
            keep_prev = true;
        }
        if !keep_prev {
            self.last_outcome = Some(now);
        }
        reply
    }

    /// "What have you learned about how I talk", "forget that phrase", "what
    /// did you misunderstand this week", and "no, I meant ..." -- answered
    /// here, before anything else reads the words.
    pub(super) fn how_you_talk_turn(&mut self, said: &str, t: u64, how: Arrival) -> Option<String> {
        if let Some(asked) = crate::phrasebook::asked_about_phrasebook(said) {
            if self.handover().stance.handed_over() {
                return Some("That's the owner's to see, not mine to share.".into());
            }
            let reply = match asked {
                crate::phrasebook::Asked::List => self.phrasebook.listing(8),
                crate::phrasebook::Asked::ForgetLatest => match self.phrasebook.latest_wording() {
                    Some(w) => self.forget_phrase_said(&w),
                    None => "There's nothing learned to forget.".into(),
                },
                crate::phrasebook::Asked::Forget(w) => self.forget_phrase_said(&w),
                crate::phrasebook::Asked::ForgetAll => {
                    let n = self.phrasebook.phrases.len();
                    self.phrasebook.phrases.clear();
                    match self.phrasebook.save(&self.store) {
                        Ok(_) if n == 0 => "There was nothing learned to forget.".into(),
                        Ok(_) => format!("Done -- I've forgotten all {n} of the ways you put things. I'll learn them again as we go."),
                        Err(e) => format!("I couldn't save that ({e}), so they may come back after a restart."),
                    }
                }
            };
            self.thread.append(said, &reply, None, t);
            return Some(reply);
        }
        if crate::misses::asks_what_was_missed(said) {
            if self.handover().stance.handed_over() {
                return Some("That's the owner's to see, not mine to share.".into());
            }
            let reply = self.misses_said(t);
            self.thread.append(said, &reply, None, t);
            return Some(reply);
        }
        let meant = crate::phrasebook::meant_instead(said)?;
        self.meant_this_instead(&meant, t, how)
    }

    /// "No, I meant X": X is run as itself, and when it does something, the
    /// words that missed mean that from now on. `None` when there's nothing
    /// recent to correct, or X reads as a fact about your world ("I meant my
    /// car is a Toyota" is the fact book's).
    fn meant_this_instead(&mut self, meant: &str, t: u64, how: Arrival) -> Option<String> {
        if self.handover().stance.handed_over() {
            return None;
        }
        let prev = self.last_outcome.clone().filter(|p| t.saturating_sub(p.at) <= CORRECTS_WITHIN_SECS)?;
        if crate::phrasebook::phrase_key(&prev.said) == crate::phrasebook::phrase_key(meant) {
            return None;
        }
        if matches!(self.parser.parse(meant), Intent::Unknown(_)) && crate::facts::triple(meant).is_some() {
            return None;
        }
        // "What should I have done instead?" -- "I meant open notepad"
        // answers it: the question is closed, not left to take the next
        // sentence as its answer.
        if self.pending_correction.take().is_some() {
            self.session.pending = Pending::Nothing;
        }
        let before = self.session.recorded;
        let reply = self.turn_from(meant, t, how);
        let did = if self.session.recorded > before { self.session.last_intent.clone() } else { None };
        // What happened to the words you corrected.
        let was = prev.intent.as_ref().map(|i| format!("did {}", i.plain())).unwrap_or_else(|| format!("said \"{}\"", clip_words(&prev.reply, 60)));
        if prev.missed.is_some() {
            self.misses.now_known_as(&prev.said, crate::misses::Why::Corrected);
            let _ = self.misses.save(&self.store);
        } else {
            self.note_a_miss(&prev.said, &was, crate::misses::Why::Corrected, prev.by_voice, t);
        }
        if let Some(wording) = &prev.routed_by {
            self.drop_a_wrong_lesson(wording);
        }
        // Heard wrong, it's a hearing miss as well -- and the words as heard
        // are still worth keeping: heard that way again, they route.
        self.check_the_hearing(&prev, meant, t);
        let learned = match &did {
            Some(i) if crate::phrasebook::learnable_action(i) && !worked_badly(&reply) => {
                self.learn_wording(&prev.said, meant, i, crate::phrasebook::Taught::Corrected, true, t)
            }
            _ => None,
        };
        // The corrected words are what the next correction is about.
        self.last_outcome = Some(Outcome {
            said: meant.to_string(),
            intent: did,
            reply: reply.clone(),
            at: t,
            missed: None,
            by_voice: prev.by_voice,
            routed_by: None,
        });
        Some(match learned {
            Some(line) if !reply.trim().is_empty() => format!("{} {line}", reply.trim_end()),
            _ => reply,
        })
    }

    /// The action a sentence is, read by the phrases alone: `None` when they
    /// don't know it or it isn't something to learn.
    fn parser_action(&self, said: &str) -> Option<Intent> {
        match self.parser.parse(said) {
            Intent::Unknown(_) => None,
            i => crate::phrasebook::learnable_action(&i).then_some(i),
        }
    }

    /// Keep `wording` as meaning `intent` (what `meant` turned out to be).
    /// The line to say when it was learned.
    fn learn_wording(&mut self, wording: &str, meant: &str, intent: &Intent, taught: crate::phrasebook::Taught, sure: bool, t: u64) -> Option<String> {
        if self.handover().stance.handed_over() || !crate::phrasebook::learnable_action(intent) {
            return None;
        }
        let (tool, arg) = match crate::phrasebook::intent_as_tool(intent, meant) {
            Some(pair) => pair,
            // Not one command and one argument: kept as the words, which the
            // phrases read the same way again -- or, when they don't, only as
            // a hint for the model.
            None => (String::new(), String::new()),
        };
        let meaning = self.meaning_route.as_ref().and_then(|m| m.text(wording)).unwrap_or_default();
        let phrase = crate::phrasebook::Phrase {
            wording: wording.trim().to_string(),
            meant: meant.trim().to_string(),
            tool,
            arg,
            action: crate::session::kind_of(intent).to_string(),
            taught,
            sure,
            learned_at: t,
            used: 0,
            last_used: 0,
            meaning,
        };
        let routes = crate::phrasebook::phrase_route(&phrase, &self.parser).is_some();
        if !self.phrasebook.keep_phrase(phrase) {
            return None;
        }
        if let Err(e) = self.phrasebook.save(&self.store) {
            self.log.warn(&format!("couldn't keep a learned wording: {e}"));
            return None;
        }
        self.log.info(&format!("learned: \"{}\" means \"{}\" ({})", wording.trim(), meant.trim(), taught.plain()));
        Some(if routes {
            format!("I'll remember \"{}\" means that.", clip_words(wording.trim(), 60))
        } else {
            format!("I'll keep in mind that \"{}\" means that.", clip_words(wording.trim(), 60))
        })
    }

    /// A learned wording led to something you undid or corrected: it goes.
    fn drop_a_wrong_lesson(&mut self, wording: &str) {
        if self.phrasebook.forget_wording(wording).is_some() {
            let _ = self.phrasebook.save(&self.store);
            self.log.info(&format!("dropped the learned wording \"{wording}\": what it led to was undone or corrected"));
        }
    }

    /// "Forget the phrase X", answered.
    fn forget_phrase_said(&mut self, wording: &str) -> String {
        match self.phrasebook.forget_wording(wording) {
            Some(p) => match self.phrasebook.save(&self.store) {
                Ok(_) => format!("Forgotten -- \"{}\" doesn't mean \"{}\" any more.", p.wording.trim(), p.meant.trim()),
                Err(e) => format!("I took it off, but couldn't save the list ({e}) -- it may come back after a restart."),
            },
            None => format!("I haven't learned anything for \"{}\".", wording.trim()),
        }
    }

    /// The Improvements page's Forget button.
    pub(crate) fn forget_phrase_from_hub(&mut self, wording: &str) -> String {
        if self.handover().stance.handed_over() {
            return "Not while this is handed over -- that list is the owner's.".into();
        }
        self.forget_phrase_said(wording)
    }

    /// The Improvements page's part: the wordings learned, each with a
    /// Forget button, and the week's misses with how well Atlas has been
    /// hearing you. Nothing of yours while it's handed over.
    pub(crate) fn how_you_talk_block(&self, t: u64) -> String {
        if self.handover().stance.handed_over() {
            return String::new();
        }
        let esc = crate::hub::esc;
        let mut body = String::from("<section aria-labelledby=talk-h><h2 id=talk-h>How you talk</h2>");
        if self.phrasebook.phrases.is_empty() {
            body.push_str(
                "<p class=note>Nothing learned yet. When I get something wrong, say what you meant -- \"no, I meant open my \
                 calendar\" -- and I'll know those words next time.</p>",
            );
        } else {
            let mut v: Vec<&crate::phrasebook::Phrase> = self.phrasebook.phrases.iter().collect();
            v.sort_by_key(|p| std::cmp::Reverse(p.last_used.max(p.learned_at)));
            body.push_str("<ul class=tight>");
            for p in v {
                body.push_str(&format!(
                    "<li>\"{}\" means \"{}\"{} <form class=inline method=post action=/hub/phrasebook>\
                     <input type=hidden name=wording value='{}'><button class=revoke>Forget</button></form></li>",
                    esc(p.wording.trim()),
                    esc(p.meant.trim()),
                    if p.sure { String::new() } else { " <span class=note>(still checking)</span>".into() },
                    esc(p.wording.trim())
                ));
            }
            body.push_str("</ul>");
        }
        body.push_str(&format!(
            "<h2>What I got wrong this week</h2><p>{}</p></section>",
            esc(&self.misses_said(t))
        ));
        body
    }

    /// Write a miss down and keep the log.
    fn note_a_miss(&mut self, said: &str, did: &str, why: crate::misses::Why, by_voice: bool, t: u64) {
        self.misses.note_miss(crate::misses::Miss { at: t, said: said.trim().to_string(), did: did.to_string(), why, by_voice });
        if let Err(e) = self.misses.save(&self.store) {
            self.log.warn(&format!("couldn't keep the list of misses: {e}"));
        }
    }

    /// A spoken turn that the next one put right: if the difference sounds
    /// like hearing rather than meaning, it's a hearing miss. Seen twice, the
    /// right words go to the speech model's hints and the mishearing is
    /// mended from then on. True when it was the hearing.
    fn check_the_hearing(&mut self, prev: &Outcome, meant: &str, t: u64) -> bool {
        if !prev.by_voice {
            return false;
        }
        let Some((heard, right)) = crate::misses::sounds_misheard(&prev.said, meant) else { return false };
        let times = self.misses.heard_as(&heard, &right, t);
        self.misses.misses.push(crate::misses::Miss {
            at: t,
            said: prev.said.clone(),
            did: format!("heard \"{heard}\" for \"{right}\""),
            why: crate::misses::Why::Misheard,
            by_voice: true,
        });
        self.misses.keep_within(t);
        let _ = self.misses.save(&self.store);
        if times >= crate::misses::MEND_AFTER && self.vocab.heard_wrong_as(&right) {
            let _ = self.store.save("vocabulary", &self.vocab);
            self.log.info(&format!("\"{right}\" heard as \"{heard}\" {times} times: given to the speech model as a hint"));
        }
        true
    }

    /// A learned wording for a sentence the phrases don't know, close
    /// enough to act on without the model: the action, and the wording.
    pub(super) fn learned_route(&mut self, said: &str, t: u64) -> Option<Intent> {
        if self.handover().stance.handed_over() || self.phrasebook.phrases.is_empty() {
            return None;
        }
        // Meaning only when a kept wording has a vector to compare with:
        // otherwise there is nothing to wait the encoder's moment for.
        let meaning = if self.phrasebook.phrases.iter().any(|p| !p.meaning.is_empty()) {
            self.meaning_route.as_ref().and_then(|m| m.sentence(said))
        } else {
            None
        };
        let found = self.phrasebook.closest(said, meaning.as_deref())?;
        if found.score < crate::phrasebook::ROUTE_AT {
            return None;
        }
        let intent = crate::phrasebook::phrase_route(&found.phrase, &self.parser)?;
        self.phrasebook.used_now(&found.phrase.wording, t);
        let _ = self.phrasebook.save(&self.store);
        self.log.info(&format!(
            "\"{}\" read as \"{}\" from what you taught me ({}, {:.2})",
            said.trim(),
            found.phrase.meant,
            found.how,
            found.score
        ));
        self.routed_by_phrase = Some((found.phrase.wording.clone(), found.phrase.sure));
        Some(intent)
    }

    /// Whether this turn's action came from a learned wording not yet sure:
    /// a consequential one is asked about first.
    pub(super) fn routed_unsure(&self) -> bool {
        self.routed_by_phrase.as_ref().is_some_and(|(_, sure)| !sure)
    }

    /// The learned wording that routed this turn, for "why did you do that".
    pub(super) fn routed_wording(&self) -> Option<String> {
        self.routed_by_phrase.as_ref().map(|(w, _)| w.clone())
    }

    /// The week's misses, read back with how well Atlas has been hearing you.
    fn misses_said(&self, t: u64) -> String {
        let hearing = crate::hearing::Hearing::load_from(&crate::roots::store());
        let listening: crate::language::Listening = self.store.load("listening");
        let numbers = crate::misses::HearingNumbers {
            ears: hearing.candidates.iter().filter(|c| c.good_turns + c.bad_turns > 0).map(|c| (c.name.clone(), c.good_turns, c.bad_turns)).collect(),
            typical: listening.typical(),
            struggling: listening.struggling(&self.tools_cfg().language),
        };
        crate::misses::week_report(&self.misses, &numbers, self.phrasebook.phrases.len(), t)
    }

    /// More about you, for the model, from what this sentence names: the
    /// people in it (what you've told Atlas about them -- never anything
    /// medical), the project it's about (what you've said about it), and a
    /// wording of yours it looks like (what you meant last time). Short, and
    /// only what bears on the sentence.
    pub(super) fn about_them_for(&mut self, said: &str, t: u64) -> String {
        if self.handover().stance.handed_over() {
            return String::new();
        }
        let mut lines: Vec<String> = Vec::new();
        let low = format!(" {} ", crate::intent::normalize(said));
        let names_in = |name: &str| -> bool {
            let n = crate::intent::normalize(name);
            n.len() >= 3 && low.contains(&format!(" {n} "))
        };
        // People, by full name or first name.
        let people: Vec<(String, Vec<String>)> = self
            .people_known()
            .by_key
            .values()
            .filter(|c| names_in(&c.name) || c.name.split_whitespace().next().is_some_and(&names_in))
            .take(2)
            .map(|c| {
                let notes: Vec<String> = c
                    .notes
                    .iter()
                    .rev()
                    .filter(|(_, n)| !crate::nudge::is_medical(n))
                    .take(2)
                    .map(|(_, n)| clip_words(n, 80))
                    .collect();
                (c.name.clone(), notes)
            })
            .collect();
        for (name, notes) in people {
            if notes.is_empty() {
                lines.push(format!("{name} is someone they know."));
            } else {
                lines.push(format!("About {name}: {}.", notes.join("; ")));
            }
        }
        // The project it's about, and what they've said about it.
        let project = crate::person::project_named(said, &self.person.projects).or_else(|| {
            self.person.projects.iter().map(|(p, _)| p.clone()).find(|p| names_in(p))
        });
        if let Some(p) = project {
            let pl = p.to_lowercase();
            let said_about: Vec<String> = self
                .facts
                .of_kind(crate::facts::Kind::Project)
                .into_iter()
                .filter(|f| f.summary.to_lowercase().contains(&pl) || f.tags.iter().any(|x| x.to_lowercase() == pl))
                .take(2)
                .map(|f| clip_words(&f.summary, 100))
                .collect();
            let when = self.person.projects.iter().find(|(n, _)| n.eq_ignore_ascii_case(&p)).map(|(_, at)| *at);
            let ago = when.filter(|at| *at > 0 && *at <= t).map(|at| (t - at) / 86_400);
            let mut line = format!("This is about their project {p}");
            if let Some(d) = ago {
                line.push_str(&match d {
                    0 => ", last mentioned today".to_string(),
                    1 => ", last mentioned yesterday".to_string(),
                    d => format!(", last mentioned {d} days ago"),
                });
            }
            if said_about.is_empty() {
                line.push('.');
            } else {
                line.push_str(&format!("; they've said: {}.", said_about.join("; ")));
            }
            lines.push(line);
        }
        // A wording of theirs this looks like: what they meant then.
        if let Some(f) = self.phrasebook.closest(said, None) {
            lines.push(format!(
                "When they said \"{}\" before, they meant \"{}\" ({}).",
                clip_words(&f.phrase.wording, 60),
                clip_words(&f.phrase.meant, 60),
                f.phrase.action
            ));
        }
        let mut out = String::new();
        for l in lines {
            if out.len() + l.len() + 1 > ABOUT_THEM_CHARS {
                break;
            }
            out.push_str(&l);
            out.push('\n');
        }
        out
    }
}

/// Two words or more that aren't only pointing back ("do that one", "the
/// other one"): a sentence that leans on what came before means something
/// different every time, and is never learned as a wording.
fn names_something(said: &str) -> bool {
    crate::intent::normalize(said)
        .split_whitespace()
        .filter(|w| !matches!(*w, "it" | "that" | "this" | "them" | "those" | "one" | "other" | "the" | "a" | "an" | "do" | "please" | "again" | "same"))
        .count()
        >= 2
}

/// A reply that reads as the action failing: not something to learn from.
/// Did this reply ask you to tell Atlas something, in words to say?
fn asked_you_for_something(reply: &str) -> bool {
    let l = reply.to_lowercase();
    l.contains("tell me \"")
}

fn worked_badly(reply: &str) -> bool {
    let l = reply.to_lowercase();
    l.starts_with("error") || l.contains("couldn't") || l.contains("could not") || l.contains("failed") || l.contains("can't find") || l.contains("not found")
}

/// Whether `said` is what the reply before told them to say ("Tell me
/// \"Sam's email is\" and the address"): answering Atlas's own question,
/// not saying the missed thing another way. 2 Oct 2026: "email Sam saying
/// I'll be late" was learned as meaning "Sam's email is ...", and every
/// email after it was taken as a new address.
fn did_as_it_was_told(reply: &str, said: &str) -> bool {
    let said = said.trim().to_lowercase().replace('\u{2019}', "'");
    reply
        .replace(['\u{201c}', '\u{201d}'], "\"")
        .split('"')
        .skip(1)
        .step_by(2)
        .map(|q| q.trim().to_lowercase().replace('\u{2019}', "'"))
        .any(|q| q.chars().count() >= 3 && said.starts_with(&q))
}
