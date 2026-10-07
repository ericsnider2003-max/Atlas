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
        self.cpu_meter.talked();
        // The parser needs your project names to tell "fix the parser in
        // Homelab" (project work) from "change the volume" (not).
        let names: Vec<String> = self.workshop.projects.iter().map(|p| p.name.clone()).collect();
        self.parser.know_projects(names);
        // The people and habits you have, and the list a number would
        // refer to -- so "open 2" and "I called Sam" read right.
        self.workday_known(t);
        self.turn_from(said, t, Arrival::Directed)
    }

    /// Handle something you said, knowing how it arrived -- watched for how
    /// it ends (2 Oct 2026, `learning`): a miss is written down, and a turn
    /// that puts right the one before teaches Atlas your words. A turn inside
    /// a turn (a correction running the words it meant, an answer that is
    /// really a new request) is part of the outer one and isn't watched
    /// again.
    pub fn turn_from(&mut self, said: &str, t: u64, how: Arrival) -> String {
        if self.watching_turn {
            return self.turn_unwatched(said, t, how);
        }
        self.watching_turn = true;
        self.take_outside_changes();
        let (watch, mended) = self.watch_the_turn(said);
        let heard = mended.as_deref().unwrap_or(said);
        // What the turn adds goes to your other devices (item 16).
        let before = self.before_the_turn();
        let reply = self.turn_unwatched(heard, t, how);
        self.carry_what_the_turn_added(before, t);
        self.watching_turn = false;
        self.turn_watched(watch, heard, reply, t)
    }


    /// The helpers both `answer_locally` and `answer_before_the_model` ask
    /// first, in this order. One list (audit Q3): it was written out twice,
    /// and a helper added to one and not the other answered in one path and
    /// went unheard in the other.
    fn asked_of_a_helper(&mut self, raw: &str, t: u64) -> Option<String> {
        self.keeping_track(raw, t)
            .or_else(|| self.writing_help(raw))
            .or_else(|| self.check_writing_help(raw))
            .or_else(|| crate::hunting::fit_asked(self, raw))
            .or_else(|| crate::hunting::applied_asked(self, raw, t))
            .or_else(|| self.phone_online_help(raw))
            .or_else(|| self.askdocs_help(raw))
            .or_else(|| self.wrapup_help(raw, t))
            .or_else(|| self.worksession_help(raw, t))
            .or_else(|| self.why_moved_help(raw, t))
            .or_else(|| self.studio_help(raw, t))
            .or_else(|| self.noticed_help(raw, t))
            .or_else(|| self.research_note_help(raw))
            .or_else(|| self.later_words_help(raw, t))
            .or_else(|| self.drafts_help(raw))
            .or_else(|| self.connect_help(raw))
            .or_else(|| self.muse_help(raw))
            .or_else(|| self.think_hard_help(raw))
            .or_else(|| self.watch_video_help(raw, t))
            .or_else(|| self.one_message_help(raw))
            .or_else(|| self.text_help(raw))
            .or_else(|| self.move_window_help(raw))
            .or_else(|| self.improvements_help(raw))
            .or_else(|| self.compose_help(raw))
            .or_else(|| self.progress_help(raw))
            .or_else(|| self.unsubscribe_help(raw))
            .or_else(|| self.note_asked(raw, t))
            .or_else(|| self.weather_help(raw))
            .or_else(|| self.remind_help(raw, t))
    }

    /// Everything Atlas can answer from what it already holds, before any
    /// model: reminders (B3), what you've said you want (B2), a correction or
    /// fact you've stated, your notes, and the rest. `None` when none of it does.
    fn answer_locally(&mut self, raw: &str, t: u64) -> Option<String> {
        self.asked_of_a_helper(raw, t)
            .or_else(|| self.spot_opportunity(raw))
            .or_else(|| self.learn_stated(raw))
            .or_else(|| self.answer_from_notes(raw, t))
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
        self.asked_of_a_helper(raw, t)
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

// The rest of this module, by what it does (audit Q6, 6 Oct 2026).
mod unwatched;
mod run_command;
mod around_the_turn;

