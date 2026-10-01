//! The small settings readers the tick and turns share, and help with decisions,
//! opportunities, reminders and ways in.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    pub(super) fn health_cfg(&self) -> HealthConfig {
        self.tools_ref().map(|t| t.health.clone()).unwrap_or_default()
    }
    pub(super) fn thread_cfg(&self) -> ThreadConfig {
        self.tools_ref().map(|t| t.thread.clone()).unwrap_or_default()
    }
    pub(super) fn panel_cfg(&self) -> crate::panel::PanelConfig {
        self.tools_ref().map(|t| t.panels.clone()).unwrap_or_default()
    }

    /// What's outstanding, weighted so the brief can be said in priority
    /// order rather than handed over as a document.
    pub(super) fn brief_items(&self) -> Vec<crate::mind::Item> {
        use crate::mind::{Item, Weight};
        let mut items: Vec<Item> = Vec::new();
        let now = crate::store::now();

        for (_, why) in self.publisher.blocked(now, true) {
            items.push(Item {
                what: "a scheduled post can't go".into(),
                weight: Weight::Urgent,
                because: why,
            });
        }
        for b in self.backlog.outstanding().iter().take(4) {
            items.push(Item {
                what: b.request.clone(),
                weight: Weight::Soon,
                because: "waiting on something".into(),
            });
        }
        items
    }

    pub(super) fn clipboard_cfg(&self) -> crate::clipboard::ClipboardConfig {
        self.tools_ref().map(|t| t.clipboard.clone()).unwrap_or_default()
    }

    pub(super) fn lane_cfg(&self) -> LaneConfig {
        self.tools_ref().map(|t| t.lanes.clone()).unwrap_or_default()
    }

    /// Machine readings. Filled in properly by the platform layer; until then
    /// the assessment logic runs against whatever is known, and unknown
    /// figures simply produce no finding rather than a false one.
    /// Weigh something that might be worth doing.
    ///
    /// Starts every axis unlooked rather than assuming a neutral score, and
    /// marks money as blocked until the accounts are readable -- so the first
    /// thing it says is what it needs, not a number it invented.
    pub fn weigh_opportunity(&self, what: &str, found_via: &str) -> String {
        use crate::opportunity::{money_is_not_auditable_yet, Axis, Opportunity};
        let mut o = Opportunity::new(what, found_via);
        o.note(Axis::Money, money_is_not_auditable_yet());
        o.presented()
    }

    /// "Argue the other side" — the strongest case against a decision, on
    /// request only (`otherside::is_asked_for` is never volunteered).
    ///
    /// On its own the phrase is about the last thing said, so the previous
    /// turn is offered as the decision when the sentence carries none. The
    /// answer names what it could NOT argue for want of evidence
    /// (`otherside::not_raised`, read through `Angle::needs_evidence`) rather
    /// than presenting a case with its gaps hidden.
    pub(super) fn other_side(&self, said: &str) -> Option<String> {
        let earlier = self
            .thread
            .recent
            .iter()
            .rev()
            .map(|e| e.said.as_str())
            .find(|s| s.trim() != said.trim() && !crate::otherside::is_asked_for(s));
        let d = crate::otherside::decision_from(said, earlier)?;
        Some(crate::otherside::argued(&d))
    }

    /// An unrecognised line that is really a decision, worked instead of
    /// answered.
    ///
    /// This is the assistant door `work_a_decision`'s own doc pointed at: a
    /// decision "arrives as a question you are already halfway through
    /// asking", so it lands as `Intent::Unknown` rather than behind an intent
    /// of its own. When the sentence carries a deciding phrase, the reply is
    /// the first move of working it -- the question underneath -- rather than
    /// "I didn't catch that, go ahead?".
    ///
    /// Sits above the policy gate with `from_notes`, and for the same reason:
    /// laying a decision out changes nothing and risks nothing, so asking
    /// permission for it would be noise. Returns `None` when the line is not a
    /// decision, leaving the ordinary path untouched.
    /// "remind me in 20 minutes to stretch", "remind me at 6 to leave",
    /// "remind me every day at 8 to take the pills".
    ///
    /// A reminder is a scheduler JOB, not a calendar entry: the calendar is
    /// what you look at; this is what Atlas *says* when the time comes. The
    /// stored command is `reminder Reminder: <text>`, which the tick re-parses
    /// to `Intent::Say` and simply speaks — so nothing here re-fires (the
    /// scheduler's own `every` handles recurrence).
    /// What's already set: reminders listed, cancelled and snoozed, timers,
    /// and events cancelled or moved (`keeping`). `None` when the sentence
    /// isn't about any of that -- or names an event there isn't.
    pub(super) fn keeping_track(&mut self, said: &str, t: u64) -> Option<String> {
        use crate::keeping::{Ask, Which};
        // The time for a reminder that was waiting for one.
        if let Some(text) = self.reminder_waiting_for_a_time.take() {
            let low = said.to_lowercase();
            let timed = crate::keeping::duration_secs(&low).map(|_| format!("remind me in {} to {text}", low.trim_start_matches("in ").trim()))
                .or_else(|| crate::when::parse(said, self.home_zone().to_local(t as i64).max(0) as u64).map(|_| format!("remind me {} to {text}", said.trim())));
            if let Some(again) = timed {
                return self.remind_help(&again, t);
            }
        }
        let ask = crate::keeping::read(said)?;
        let reminders = |s: &Self| -> Vec<crate::scheduler::Job> {
            let mut v: Vec<crate::scheduler::Job> =
                s.scheduler.active().into_iter().filter(|j| j.command.starts_with("reminder ")).cloned().collect();
            v.sort_by_key(|j| j.due);
            v
        };
        let words_of = |j: &crate::scheduler::Job| -> String {
            let w = j.command.trim_start_matches("reminder ").trim_start_matches("Reminder:").trim();
            // A timer is named by its length: "Your 10-minute timer is done." -> "the 10-minute timer".
            match w.strip_prefix("Your ").and_then(|r| r.strip_suffix(" is done.")) {
                Some(timer) => format!("the {timer}"),
                None => w.to_string(),
            }
        };
        let zone = self.home_zone();
        let lnow = zone.to_local(t as i64).max(0) as u64;
        let when_of = |due: u64| -> String {
            let local = zone.to_local(due as i64).max(0) as u64;
            if due.saturating_sub(t) < 3600 {
                // Rounded up: 9 min 59 s is "in 10 minutes", as a person says it.
                format!("in {}", say_duration((due.saturating_sub(t) + 59) / 60 * 60))
            } else {
                local_moment(local, lnow)
            }
        };
        match ask {
            Ask::ListReminders => {
                let all = reminders(self);
                if all.is_empty() {
                    return Some("No reminders or timers set.".into());
                }
                let listed: Vec<String> = all.iter().take(8).map(|j| format!("{} {} (#{})", words_of(j), when_of(j.due), j.id)).collect();
                let more = if all.len() > 8 { format!(", and {} more", all.len() - 8) } else { String::new() };
                Some(format!("{}: {}{more}.", if all.len() == 1 { "One".to_string() } else { format!("{}", all.len()) }, listed.join("; ")))
            }
            Ask::CancelReminder(which) => {
                let all = reminders(self);
                if all.is_empty() {
                    return Some("There are no reminders or timers set.".into());
                }
                let chosen: Vec<crate::scheduler::Job> = match &which {
                    Which::All => all.clone(),
                    Which::Number(n) => all.iter().filter(|j| j.id == *n).cloned().collect(),
                    Which::About(words) => {
                        let want: Vec<String> = words.split_whitespace().filter(|w| w.len() > 2).map(str::to_lowercase).collect();
                        all.iter().filter(|j| {
                            let text = words_of(j).to_lowercase();
                            !want.is_empty() && want.iter().all(|w| text.contains(w.as_str()))
                        }).cloned().collect()
                    }
                    Which::Last => {
                        if all.len() == 1 {
                            all.clone()
                        } else {
                            all.iter().filter(|j| Some(j.id) == self.last_reminder_set).cloned().collect()
                        }
                    }
                };
                match chosen.len() {
                    0 if matches!(which, Which::Last) => Some(format!(
                        "Which one? {}.",
                        all.iter().take(6).map(|j| format!("{} {} (#{})", words_of(j), when_of(j.due), j.id)).collect::<Vec<_>>().join("; ")
                    )),
                    0 => Some("I can't find a reminder like that. Say \"what reminders do I have\" to hear them.".into()),
                    n if n > 1 && !matches!(which, Which::All) => Some(format!(
                        "That could be {n} of them: {}. Say the number.",
                        chosen.iter().map(|j| format!("{} (#{})", words_of(j), j.id)).collect::<Vec<_>>().join("; ")
                    )),
                    _ => {
                        for j in &chosen {
                            self.scheduler.cancel(j.id);
                        }
                        let _ = self.scheduler.save(&self.store);
                        Some(if chosen.len() == 1 {
                            format!("Cancelled: {}.", words_of(&chosen[0]))
                        } else {
                            format!("Cancelled all {}.", chosen.len())
                        })
                    }
                }
            }
            Ask::Timer(secs) => {
                let id = self.scheduler.at(&format!("reminder Your {} timer is done.", say_duration(secs).replace(' ', "-").trim_end_matches('s')), t + secs);
                self.last_reminder_set = Some(id);
                let _ = self.scheduler.save(&self.store);
                Some(format!("Timer set for {}. (#{id})", say_duration(secs)))
            }
            Ask::Snooze(secs) => {
                let (command, _) = self.last_reminder_fired.clone()?;
                let secs = secs.unwrap_or(crate::keeping::SNOOZE_SECS);
                let id = self.scheduler.at(&command, t + secs);
                self.last_reminder_set = Some(id);
                let _ = self.scheduler.save(&self.store);
                Some(format!("I'll say it again in {}.", say_duration(secs)))
            }
            Ask::CancelEvent(what) => {
                let found = self.event_meant(&what, t)?;
                let title = found.title.clone();
                let when = found.say_when();
                self.calendar.remove(found.id);
                let _ = self.calendar.save(&self.store);
                Some(format!("Taken off your calendar: {title}, {when}."))
            }
            Ask::MoveEvent { what, to } => {
                let found = self.event_meant(&what, t)?;
                let len = found.end.saturating_sub(found.start).max(60);
                // A day and time ("Friday at 2"), or a time on the same day ("4pm").
                let new_when = crate::calendar::resolve_when_in(&to, t, &zone).or_else(|| {
                    let (hours, minute) = crate::keeping::clock_in(&to)?;
                    let local = zone.to_local(found.start as i64).max(0) as u64;
                    let day = local - local % 86_400;
                    let hour = *hours.first()? as u64;
                    let start = zone.to_utc((day + hour * 3600 + minute as u64 * 60) as i64).max(0) as u64;
                    Some(crate::calendar::When { start, end: start + len, all_day: false })
                })?;
                let new_when = if new_when.all_day { new_when } else { crate::calendar::When { end: new_when.start + len, ..new_when } };
                self.calendar.move_to(found.id, new_when);
                let _ = self.calendar.save(&self.store);
                let moved = self.calendar.event(found.id).map(|e| e.say_when()).unwrap_or_default();
                Some(format!("Moved {} to {moved}.", found.title))
            }
        }
    }

    /// "What's the weather", "will it rain tomorrow", "weather in Chicago":
    /// Open-Meteo, answered now (`weather`). Without the internet, said so
    /// rather than guessed.
    pub(super) fn weather_help(&mut self, said: &str) -> Option<String> {
        let asked = crate::weather::about_the_weather(said)?;
        if self.connectivity.cached() == Reach::Offline {
            return Some("I can't check the weather without the internet, and I'd only be guessing.".into());
        }
        let cfg = self.tools_ref().map(|t| t.weather.clone()).unwrap_or_default();
        Some(match crate::weather::answer(&cfg, &asked, self.weather_place.as_ref()) {
            Ok((said, place)) => {
                if asked.place.is_none() {
                    self.weather_place = Some(place);
                }
                said
            }
            Err(why) => format!("I couldn't get the weather: {why}."),
        })
    }

    /// "Where's that note about broker fees", "what did I note yesterday",
    /// "find my note about X", "what did I write down about X": the notebook
    /// searched (30 Sep 2026: `Notebook::find` had no caller, so a thought
    /// written down could never be asked for again).
    pub(super) fn note_asked(&self, said: &str, now: u64) -> Option<String> {
        let t = said.to_lowercase();
        let asking = [
            "where's that note", "wheres that note", "where is that note", "find my note", "find the note", "find that note",
            "what did i note", "what did i write down", "what did i jot", "did i note", "my note about", "my notes about",
            "read my notes", "what's in my notes", "whats in my notes", "what notes do i have", "my last note",
        ]
        .iter()
        .any(|p| t.contains(p));
        if !asking {
            return None;
        }
        if t.contains("my last note") {
            return Some(match self.notebook.notes.last() {
                Some(n) => n.text.clone(),
                None => "Your notebook is empty.".into(),
            });
        }
        let hits = self.notebook.find(said, now);
        if hits.is_empty() {
            return Some("I can't find a note like that. Say \"note that ...\" and I'll keep the next one.".into());
        }
        Some(crate::capture::found(&hits.into_iter().take(3).collect::<Vec<_>>()))
    }

    /// The event these words mean, among the next two weeks' (and today's
    /// earlier ones): by what it's called or when it is. One only.
    fn event_meant(&self, what: &str, t: u64) -> Option<crate::calendar::Event> {
        let zone = self.home_zone();
        let from = t.saturating_sub(12 * 3600);
        let mut found: Vec<crate::calendar::Event> = self
            .calendar
            .occurrences_between(from, t + 14 * 86_400)
            .into_iter()
            .filter(|e| crate::keeping::event_answers_to(&e.title, zone.to_local(e.start as i64).max(0) as u64, what))
            .collect();
        let today_or_tomorrow = what.contains("tomorrow") || what.contains("today");
        if today_or_tomorrow {
            let lnow = zone.to_local(t as i64).max(0) as u64;
            let day = lnow - lnow % 86_400 + if what.contains("tomorrow") { 86_400 } else { 0 };
            found.retain(|e| {
                let l = zone.to_local(e.start as i64).max(0) as u64;
                l >= day && l < day + 86_400
            });
        }
        found.sort_by_key(|e| e.start);
        found.dedup_by_key(|e| e.id);
        // The soonest one it could be: "my 3pm" is the next 3pm.
        found.into_iter().find(|e| e.end > t)
    }

    pub(super) fn remind_help(&mut self, said: &str, t: u64) -> Option<String> {
        // "set a reminder for tomorrow at 9 to call mom" is the same request
        // as "remind me tomorrow at 9 to call mom". (It used to be a calendar
        // phrase, which booked an event called "a reminder" and lost the rest.)
        let low0 = said.to_lowercase();
        let restated;
        // "give me a nudge at 4 to call the dentist" (1 Oct 2026 model
        // ranking: the model reached for signing in somewhere).
        let said = match [
            "set a reminder for ", "set a reminder ", "set reminder for ", "set reminder ", "add a reminder for ", "add a reminder ",
            "give me a nudge ", "nudge me ", "ping me ", "give me a shout ", "give me a reminder ", "send me a reminder ",
        ]
            .iter()
            .find(|p| low0.starts_with(**p))
        {
            Some(p) => {
                restated = format!("remind me {}", said.get(p.len()..).unwrap_or(""));
                restated.as_str()
            }
            None => said,
        };
        let low = said.to_lowercase();
        if !low.starts_with("remind me") && !low.starts_with("remind ") {
            return None;
        }
        // The turn's own time, not the wall clock: a reminder set at `t` is
        // due relative to `t`. (Reading the clock here made a test pass or
        // fail by the hour of day it ran; found by the round-5 Windows run.)
        let now = t;
        // Clock words ("at 6", "every weekday at 7") are read on your clock:
        // resolved against local time, then brought back to UTC.
        let zone = self.home_zone();
        let lnow = zone.to_local(now as i64).max(0) as u64;
        let back = |local: u64| zone.to_utc(local as i64).max(0) as u64;

        // (1) Relative: "in 20 minutes", "in 2 hours".
        if let Some(secs) = relative_secs(&low) {
            let text = reminder_text(said);
            // From the turn's own moment, like every other time here (it read
            // the wall clock itself, so a turn's "in 2 minutes" and its tick
            // disagreed whenever they weren't the same clock).
            let id = self.scheduler.at(&format!("reminder Reminder: {text}"), now + secs);
            self.last_reminder_set = Some(id);
            let _ = self.scheduler.save(&self.store);
            return Some(format!(
                "Right — in {}, I'll remind you to {text}. (#{id})",
                say_duration(secs)
            ));
        }

        // (2) Recurring: "every day at 8", "every week on Monday at 9",
        // "every weekday at 7", "the last Friday of every month at 4".
        let repeat = crate::calendar::repeat_from(said);
        if repeat != crate::calendar::Repeat::Once {
            let text = reminder_text(said);
            // A general rule finds its own first slot (the Friday, not today).
            if let Some(rule) = repeat.rule() {
                let Some(when) = crate::calendar::resolve_recurring_when(said, lnow, &repeat) else {
                    return Some(format!("What time? — \"remind me {} at 9 to …\".", rule.describe()));
                };
                let id = self.scheduler.on_rule(&format!("reminder Reminder: {text}"), &rule, back(when.start), &zone);
                let _ = self.scheduler.save(&self.store);
                return Some(format!("Set — {} I'll remind you to {text}. (#{id})", rule.describe()));
            }
            let Ok(first) = first_occurrence(said, lnow) else {
                return Some("Every day at what time? — \"remind me every day at 8 to …\".".into());
            };
            // `first` is on your clock. A fixed interval would slide an hour
            // at each clock change, so outside UTC every repeat is a cron slot
            // kept on your wall clock instead.
            // Already local seconds (`lnow` above), so the time of day here is
            // yours, not UTC's.
            let local_secs_of_day = first % 86_400;
            let (h, m) = (local_secs_of_day / 3600, (local_secs_of_day % 3600) / 60);
            if !zone.is_utc() {
                let (expr, cadence) = match repeat {
                    crate::calendar::Repeat::Daily => (format!("{m} {h} * * *"), "every day".to_string()),
                    crate::calendar::Repeat::Weekdays => (format!("{m} {h} * * MON-FRI"), "every weekday".to_string()),
                    _ => {
                        let dow = ["MON", "TUE", "WED", "THU", "FRI", "SAT", "SUN"][crate::civil::weekday((first / 86_400) as i64) as usize];
                        (format!("{m} {h} * * {dow}"), "every week".to_string())
                    }
                };
                return Some(match self.scheduler.on_cron(&format!("reminder Reminder: {text}"), &expr, now, &zone) {
                    Ok(id) => {
                        let _ = self.scheduler.save(&self.store);
                        format!("Set — {cadence} at {h:02}:{m:02} {} I'll remind you to {text}. (#{id})", zone.abbreviation_at(now as i64))
                    }
                    Err(why) => format!("I couldn't set that: {why}"),
                });
            }
            let (every, cadence) = match repeat {
                crate::calendar::Repeat::Weekly => (7 * 86_400, "every week"),
                crate::calendar::Repeat::Daily => (86_400, "every day"),
                // Weekday-only is not a fixed interval. It used to be refused
                // here ("I won't pretend to skip weekends"); a cron slot is the
                // honest shape for it, so it is set, and it does skip them.
                crate::calendar::Repeat::Weekdays => {
                    return Some(
                        match self.scheduler.on_cron(&format!("reminder Reminder: {text}"), &format!("{m} {h} * * MON-FRI"), now, &zone) {
                            Ok(id) => {
                                let _ = self.scheduler.save(&self.store);
                                format!("Set — every weekday I'll remind you to {text}. (#{id})")
                            }
                            Err(why) => format!("I couldn't set that: {why}"),
                        },
                    );
                }
                crate::calendar::Repeat::Once | crate::calendar::Repeat::Rule(_) => unreachable!(),
            };
            let id = self.scheduler.every(&format!("reminder Reminder: {text}"), every, first);
            let _ = self.scheduler.save(&self.store);
            return Some(format!("Set — {cadence} I'll remind you to {text}. (#{id})"));
        }

        // (3) Absolute clock time: "at 6", "tomorrow at 9", "Monday at 8".
        // The time is said back, so a misread one is caught while it can
        // still be fixed; a time that could mean two things is asked about,
        // never set on a guess and never dropped in silence.
        match first_occurrence(said, lnow) {
            Ok(first) => {
                let text = reminder_text(said);
                let id = self.scheduler.at(&format!("reminder Reminder: {text}"), back(first));
                let _ = self.scheduler.save(&self.store);
                return Some(format!("Set — {} I'll remind you to {text}. (#{id})", local_moment(first, lnow)));
            }
            Err(Some(why)) => {
                return Some(format!("I haven't set it yet: {why} Say it again with the day and time and I will."));
            }
            Err(None) => {}
        }

        // (4) "remind me to X" with no time named: asked when, and the next
        // thing you say with a time in it sets it (30 Sep 2026: it said
        // "Reminder: X" there and then, and set nothing).
        let text = reminder_text(said);
        self.reminder_waiting_for_a_time = Some(text.clone());
        Some(format!("When should I remind you to {text}? In 20 minutes, at 6, tomorrow at 9 -- whenever."))
    }

    /// Eric states a want or floats an idea; Atlas takes it and weighs it as an
    /// opportunity rather than letting it pass. It surfaces the frame and, this
    /// being the honest part, names what only Eric or his accounts can settle —
    /// it does not declare a verdict, because Money can't be audited and Fit is
    /// his call. Eric's spec: "I mentioned wanting something done and Atlas sees
    /// an opportunity for that thing to happen and suggests it."
    pub(super) fn spot_opportunity(&self, said: &str) -> Option<String> {
        if !reads_as_a_want(said) {
            return None;
        }
        let framed = self.weigh_opportunity(said, "something you said you wanted");
        Some(format!("You mentioned wanting that — worth weighing.\n{framed}"))
    }

    pub(super) fn decision_help(&mut self, said: &str) -> Option<String> {
        let l = said.to_lowercase();
        // Picking a set-aside decision back up.
        if l.contains("back to the decision") || l.contains("that decision again") {
            return Some(if self.deciding.is_some() { self.say_the_decision() } else { "There's no decision on the go.".into() });
        }
        // "Set aside the contract, it pays late" -- while one is being worked.
        if let Some(rest) = l.strip_prefix("set aside ").or_else(|| l.strip_prefix("rule out ")) {
            if let Some(d) = self.deciding.as_mut() {
                let (what, why) = rest.split_once(" because ").or_else(|| rest.split_once(", ")).unwrap_or((rest, "you ruled it out"));
                let name = d.options.iter().find(|o| o.what.to_lowercase().contains(what.trim())).map(|o| o.what.clone());
                if let Some(name) = name {
                    d.set_aside(&name, why.trim());
                    return Some(self.say_the_decision());
                }
            }
        }
        if !crate::decide::wants_working(said) {
            return None;
        }
        let weight = crate::decide::how_much_it_matters(said);
        if weight == crate::decide::Weight::JustPick {
            return Some(self.work_a_decision(said, true));
        }
        self.deciding = Some(crate::decide::Decision::new(said, weight));
        // With a model, the whole pass is drafted at once and a lean comes
        // back as news; without one, it's worked with you, a move a turn.
        // Drafted in the background: the deep model's work (`deepbrain`).
        if let Some(llm) = self.background_llm() {
            let about = said.to_string();
            let work: crew::Work = Box::new(move |_ctl| {
                llm.complete(crate::decide::DRAFTER_PROMPT, &about).map_err(|e| e.to_string())
            });
            if self.hand_off("decide", crate::store::now(), work, Some(said.to_string()), SpeakPolicy::Always) {
                return Some("Let me think that through properly — I'll come back with where I'd lean and why.".into());
            }
        }
        Some(self.say_the_decision())
    }

    /// Where the decision stands, said, and the next question asked.
    pub(super) fn say_the_decision(&mut self) -> String {
        let _ = self.store.save("deciding", &self.deciding);
        let Some(d) = self.deciding.as_ref() else { return "There's no decision on the go.".into() };
        let said = crate::decide::said(d);
        if d.lean().is_ok() {
            self.session.ask(&said);
            self.pending_decision = Some(None);
        } else if let Some(m) = d.next_move() {
            if !said.ends_with("the one.") {
                self.session.ask(&said);
                self.pending_decision = Some(Some(m));
            }
        }
        said
    }

    /// The model's draft of a decision, back from the crew.
    pub(super) fn decision_news(&mut self, ending: &crew::Ending) -> Option<String> {
        let about = self.deciding.as_ref().map(|d| d.about.clone())?;
        let weight = self.deciding.as_ref().map(|d| d.weight)?;
        match ending {
            crew::Ending::Done(Ok(text)) => {
                let drafted = crate::decide::from_draft(&about, weight, text);
                // A draft the model got wrong is worked with you instead.
                if drafted.options.len() >= 2 {
                    self.deciding = Some(drafted);
                }
                Some(self.say_the_decision())
            }
            _ => Some(self.say_the_decision()),
        }
    }

    /// "What are all the ways you could get me that?" — the whole menu of
    /// approaches, in the order Atlas would try them, rather than only the one
    /// it reaches for first.
    ///
    /// `route::plan` picks the single best way in and `another_way` names the
    /// next one when the first is closed; neither answers the person who wants
    /// to see the options laid out before committing to any. `route::all_routes`
    /// builds exactly that ordered list — cheapest-and-most-reliable first, no
    /// repeats — and was proven by `stance_route.rs` and reached by nothing.
    ///
    /// Sits in the `Unknown` chain beside `decision_help`, and for the same
    /// reason: laying the options out changes nothing and risks nothing, so
    /// asking permission for it would be noise. Returns `None` when the line
    /// is not asking for the ways, leaving the ordinary path untouched.
    pub(super) fn ways_in_help(&self, said: &str) -> Option<String> {
        let w = said.trim().to_lowercase();
        let asked = w.contains("all the ways")
            || w.contains("every way")
            || w.contains("what are the ways")
            || w.contains("what ways")
            || w.contains("how else could you")
            || w.contains("how else can you")
            || w.contains("other ways")
            || w.contains("list the ways");
        if !asked {
            return None;
        }
        // Which sort of problem the ways are for. A menu of ways to extract
        // something is a different menu from ways to fix code, so the kind is
        // read from the sentence rather than guessed at. "Find out"/"look up"
        // and anything unclassified fall to the learning menu, which is the
        // general one.
        use crate::route::Kind;
        let kind = if w.contains("fix")
            || w.contains("repair")
            || w.contains("the bug")
            || w.contains("the error")
            || w.contains("the code")
        {
            Kind::Fix
        } else if w.contains("get ")
            || w.contains("data")
            || w.contains("extract")
            || w.contains("pull ")
            || w.contains("download")
            || w.contains("scrape")
            || w.contains("read ")
        {
            Kind::Extract
        } else if w.contains("do ")
            || w.contains("click")
            || w.contains("in the app")
            || w.contains("press ")
            || w.contains("type ")
        {
            Kind::Act
        } else {
            Kind::Learn
        };
        // The full menu, so nothing is dropped for a missing prerequisite:
        // the question is "what are ALL the ways", not "what can you do right
        // now" — whether a route is available today is a separate answer that
        // `offline_coverage` already gives. Feeding in every prerequisite the
        // routes name leaves `all_routes` to order them by cost and reliability.
        let have: Vec<String> = crate::route::known_routes()
            .into_iter()
            .flat_map(|r| r.needs)
            .collect();
        let routes = crate::route::all_routes(
            kind,
            said.trim(),
            &have,
            &crate::learned::Learned::default(),
            &self.tools_cfg().route,
            crate::store::now(),
        );
        if routes.is_empty() {
            return None;
        }
        let ways: Vec<String> = routes
            .iter()
            .enumerate()
            .map(|(i, r)| format!("{}. {}", i + 1, r.name))
            .collect();
        Some(format!(
            "{} ways I'd try, in order — {}.",
            routes.len(),
            ways.join("; ")
        ))
    }

    /// Work a decision rather than answer it.
    ///
    /// Reached from the assistant rather than sitting behind an intent of its
    /// own, because a decision usually arrives as a question you are already
    /// halfway through asking. `decision_help` is that door: it spots a
    /// deciding phrase in an otherwise-unrecognised line and calls this.
    pub fn work_a_decision(&self, about: &str, cheap_and_reversible: bool) -> String {
        use crate::decide::{Decision, Weight};
        let d = Decision::new(
            about,
            if cheap_and_reversible { Weight::JustPick } else { Weight::WorthWorking },
        );
        match d.next_move() {
            None => d.laid_out(),
            Some(m) => m.asks().to_string(),
        }
    }

    /// A message arrived from another, trusted Atlas. Turns it into an
    /// ordinary offer through the exact same door `nudge` already uses --
    /// there is no separate, higher-trust path for this. A signal earns
    /// exactly the same scrutiny as anything Atlas proposes on its own.
    ///
    /// Not yet reachable from the running server: the server today only
    /// runs in `run_hub`'s settings-only mode, which does not drive a live
    /// daemon loop at all. This is the piece that mode is missing, written
    /// and tested now so wiring the two together later is one call, not a
    /// redesign.
    /// The offer currently waiting for a yes or no, if there is one.
    pub fn pending_offer(&self) -> Option<&Offer> {
        self.pending_offer.as_ref()
    }

    /// What you want to be called.
    ///
    /// Falls back to no name rather than to a guessed one -- being addressed
    /// by the wrong name is worse than not being addressed at all.
    ///
    /// This carried the note "not yet called from anywhere -- `mend`'s
    /// question-parking flow is not wired into the daemon loop tonight". It is
    /// now: `park_for_you` is that flow, and this is the name it uses. The
    /// `allow(dead_code)` that went with the note is gone, which means the
    /// compiler will say so if it ever stops having a caller again.
    pub(super) fn called(&self) -> String {
        self.memory.preference("called").unwrap_or("").trim().to_string()
    }
}
