//! Errands and the crew: choosing and controlling errands, handing work off,
//! keeping unfinished work and window jobs across a restart, and crew news.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// Every errand in hand, as `which_errand` chooses between them.
    pub(super) fn errand_candidates(&self) -> Vec<crate::which_errand::Candidate> {
        let mut all = self.crew_candidates();
        all.extend(self.window_job_candidates());
        all
    }

    /// The crew's errands, as `which_errand` chooses between them.
    fn crew_candidates(&self) -> Vec<crate::which_errand::Candidate> {
        self.crew
            .errands()
            .into_iter()
            .filter(|e| self.crew.in_hand(e.id))
            // A window job's reply being written is part of that window
            // job, which is a candidate of its own ("the conversation in
            // Slack"); listing both would ask which of two names for one
            // thing you meant.
            .filter(|e| e.name != "conversation-reply")
            .map(|e| {
                let link = self.crew_links.get(&e.id);
                crate::which_errand::Candidate {
                    id: e.id,
                    label: e.name.clone(),
                    topic: link.and_then(|l| l.topic.clone()),
                    started: e.started,
                    paused: matches!(
                        e.state,
                        crew::State::Pausing | crew::State::Holding | crew::State::WaitingPaused
                    ),
                    can_hold: Self::HOLDS_AT_A_SAFE_POINT.contains(&e.name.as_str())
                        || matches!(e.state, crew::State::Waiting | crew::State::WaitingPaused),
                }
            })
            .collect()
    }

    /// Hold every running errand for a whole-Atlas pause. Returns how many.
    pub(super) fn hold_every_errand(&mut self) -> usize {
        // Window jobs hold by themselves while Atlas is paused
        // (`work_for_you`); counted here so the answer is the whole truth.
        let windows = self.working_for_you.iter().filter(|w| !w.held).count();
        let held: Vec<(u64, bool)> = self
            .crew_candidates()
            .into_iter()
            .filter(|c| !c.paused && c.can_hold)
            .map(|c| (c.id, c.label == "housekeeping"))
            .collect();
        for (id, _) in &held {
            self.crew.pause(*id);
        }
        self.held_by_pause.extend(held.iter().map(|(id, _)| *id));
        // Atlas's own hourly tidy holds too, but it isn't one of yours to
        // count (2 Oct 2026: "Holding 2 errands" with one asked for, when the
        // reclaim sweep happened to be mid-survey).
        held.iter().filter(|(_, own)| !own).count() + windows
    }

    /// "Stop", "pause the research", "carry on with the backup", "cancel the
    /// second one", "no, the backup" — single-errand control, Eric's B1
    /// ruling: stopping one errand pauses it and loses nothing, and with
    /// several going Atlas works out which one was meant (`which_errand`),
    /// says what it did and what carries on, and asks when it can't tell.
    ///
    /// `None` when the line isn't about an errand, so everything else still
    /// gets it.
    pub(super) fn errand_control(&mut self, said: &str, t: u64) -> Option<String> {
        use crate::which_errand::{Pick, Verb};
        const ANSWER_WITHIN: u64 = 120;
        let cands = self.errand_candidates();
        let recent: Vec<String> =
            self.thread.recent.iter().rev().take(3).map(|e| e.said.clone()).collect();

        // 1. The answer to "which one?"
        if let Some((verb, ids, asked)) = self.errand_question.take() {
            if t.saturating_sub(asked) <= ANSWER_WITHIN {
                let lower = said.trim().to_lowercase();
                if ["never mind", "nevermind", "none", "neither", "forget it", "leave them", "none of them"]
                    .iter()
                    .any(|p| lower.trim_matches(|c: char| !c.is_alphanumeric()) == *p)
                {
                    return Some("Leaving them all as they are.".into());
                }
                let among: Vec<crate::which_errand::Candidate> =
                    cands.iter().filter(|c| ids.contains(&c.id)).cloned().collect();
                let target = crate::which_errand::verb_and_target(said).map(|(_, r)| r).unwrap_or_else(|| said.to_string());
                if let Pick::These(chosen, why) = crate::which_errand::pick(verb, &target, &among, &[], t) {
                    return Some(self.apply_errand_pick(verb, &chosen, why, &cands, t));
                }
            }
        }

        // 2. "No, the backup" — swap the last guess.
        if let Some((verb, ids, at)) = self.last_errand_pick.clone() {
            // Only straight after the pick: a "no" to anything said since
            // belongs to that, not to the errands.
            let straight_after = self.thread.recent.last().map(|e| e.at == at).unwrap_or(false);
            if verb != Verb::Cancel && straight_after && t.saturating_sub(at) <= ANSWER_WITHIN {
                if let Some(target) = crate::which_errand::correction(said) {
                    let others: Vec<crate::which_errand::Candidate> =
                        cands.iter().filter(|c| !ids.contains(&c.id)).cloned().collect();
                    // Undo first, so the swap leaves nothing half-done.
                    for id in &ids {
                        if *id >= WINDOW_JOB_IDS {
                            match verb {
                                Verb::Pause => self.control_window_job(Verb::Resume, *id),
                                Verb::Resume => self.control_window_job(Verb::Pause, *id),
                                Verb::Cancel => {}
                            }
                            continue;
                        }
                        match verb {
                            Verb::Pause => self.crew.resume(*id),
                            Verb::Resume => self.crew.pause(*id),
                            Verb::Cancel => false,
                        };
                    }
                    let undone: Vec<String> = cands
                        .iter()
                        .filter(|c| ids.contains(&c.id))
                        .map(crate::which_errand::describe)
                        .collect();
                    let back = match verb {
                        Verb::Pause => format!("Sorry — {} is back at work.", undone.join(" and ")),
                        _ => format!("Sorry — {} is paused again.", undone.join(" and ")),
                    };
                    let fresh = self.errand_candidates();
                    let pool: Vec<crate::which_errand::Candidate> =
                        fresh.into_iter().filter(|c| others.iter().any(|o| o.id == c.id)).collect();
                    self.last_errand_pick = None;
                    return Some(match crate::which_errand::pick(verb, &target, &pool, &[], t) {
                        Pick::These(chosen, why) => {
                            let cands = self.errand_candidates();
                            format!("{back} {}", self.apply_errand_pick(verb, &chosen, why, &cands, t))
                        }
                        Pick::Ask(ask) => {
                            let among: Vec<&crate::which_errand::Candidate> =
                                pool.iter().filter(|c| ask.contains(&c.id)).collect();
                            self.errand_question = Some((verb, ask, t));
                            format!("{back} {}", crate::which_errand::question(verb, &among))
                        }
                        Pick::Nothing => back,
                    });
                }
            }
        }

        // 3. A fresh "stop …" / "pause …" / "carry on with …" / "cancel …".
        let (verb, target) = crate::which_errand::verb_and_target(said)?;
        if verb == Verb::Cancel && target.trim().is_empty() {
            return None; // "cancel" alone is about the last thing said, not an errand
        }
        match crate::which_errand::pick(verb, &target, &cands, &recent, t) {
            Pick::These(chosen, why) => Some(self.apply_errand_pick(verb, &chosen, why, &cands, t)),
            Pick::Ask(ids) => {
                let among: Vec<&crate::which_errand::Candidate> = cands.iter().filter(|c| ids.contains(&c.id)).collect();
                let q = crate::which_errand::question(verb, &among);
                self.errand_question = Some((verb, ids, t));
                Some(q)
            }
            Pick::Nothing => None,
        }
    }

    fn apply_errand_pick(
        &mut self,
        verb: crate::which_errand::Verb,
        chosen: &[u64],
        why: crate::which_errand::Why,
        cands: &[crate::which_errand::Candidate],
        t: u64,
    ) -> String {
        use crate::which_errand::Verb;
        for id in chosen {
            if *id >= WINDOW_JOB_IDS {
                self.control_window_job(verb, *id);
                continue;
            }
            match verb {
                Verb::Pause => {
                    self.crew.pause(*id);
                    self.held_by_pause.retain(|h| h != id);
                }
                Verb::Resume => {
                    self.crew.resume(*id);
                    self.held_by_pause.retain(|h| h != id);
                }
                Verb::Cancel => self.crew.ask_to_stop(*id),
            }
        }
        self.errand_question = None;
        self.last_errand_pick = Some((verb, chosen.to_vec(), t));
        let picked: Vec<&crate::which_errand::Candidate> = cands.iter().filter(|c| chosen.contains(&c.id)).collect();
        let others: Vec<&crate::which_errand::Candidate> = cands.iter().filter(|c| !chosen.contains(&c.id)).collect();
        crate::which_errand::answered(verb, &picked, why, &others)
    }

    /// "Stop the Slack one" — a window job paused (held, nothing lost),
    /// picked back up, or called off.
    fn control_window_job(&mut self, verb: crate::which_errand::Verb, id: u64) {
        use crate::which_errand::Verb;
        match verb {
            Verb::Pause => {
                if let Some(w) = self.working_for_you.iter_mut().find(|w| w.id == id) {
                    w.held = true;
                    w.job.pause();
                }
            }
            Verb::Resume => {
                if let Some(w) = self.working_for_you.iter_mut().find(|w| w.id == id) {
                    w.held = false;
                    w.job.resume();
                    // Look again straight away rather than in ten seconds.
                    w.looked = 0;
                }
            }
            Verb::Cancel => {
                if let Some(i) = self.working_for_you.iter().position(|w| w.id == id) {
                    let mut w = self.working_for_you.remove(i);
                    if let Some(c) = w.composing {
                        self.crew.ask_to_stop(c);
                    }
                    let said = w.job.called_off();
                    self.log.info(&said);
                }
            }
        }
    }

    /// Give an errand to the crew and register it with `long_work` in one
    /// step, so a new crew citizen is one call instead of three. Returns
    /// whether it was actually taken — `false` only when the bounded
    /// waiting list is full, which callers on a repeating schedule should
    /// treat as "try again next time" rather than "failed".
    pub(super) fn hand_off(
        &mut self,
        name: &'static str,
        t: u64,
        work: crew::Work,
        topic: Option<String>,
        speak: SpeakPolicy,
    ) -> bool {
        self.hand_off_as(name, t, work, topic, speak).is_some()
    }

    /// As `hand_off`, giving back the crew's id for the errand, for a caller
    /// that needs to know which ending is its own. Work already in hand
    /// (`crew_job`'s key) is joined rather than run twice, and the id given
    /// back is that run's.
    pub(super) fn hand_off_as(
        &mut self,
        name: &'static str,
        t: u64,
        work: crew::Work,
        topic: Option<String>,
        speak: SpeakPolicy,
    ) -> Option<u64> {
        // Testing itself: background work is named, not started (`selftest`).
        if self.rehearsal {
            drop(work);
            self.rehearsed.push(match &topic {
                Some(t) => format!("start {name} ({t}) in the background"),
                None => format!("start {name} in the background"),
            });
            return Some(u64::MAX - self.rehearsed.len() as u64);
        }
        let job = crew_job(name, topic.as_deref(), &speak);
        let crew_id = match self.crew.hand_job(job, t, work) {
            Ok(crew::Taken::Joined(crew_id)) => {
                // The same work is already in hand, so this ask is answered
                // by that run. If this ask is one somebody is waiting on and
                // the run was a quiet chore, the answer is now said out loud.
                // Its `resume` record is the first ask's; nothing to add.
                if let Some(link) = self.crew_links.get_mut(&crew_id) {
                    if speak == SpeakPolicy::Always {
                        link.speak = SpeakPolicy::Always;
                    }
                }
                self.log.info(&format!("{name}: already in hand, joined that run"));
                return Some(crew_id);
            }
            Ok(taken) => taken.id(),
            Err(why) => {
                self.log.warn(&why);
                return None;
            }
        };
        let watch_id = self.long_work.watch(name, "crew", t);
        // Work you asked for is written down until it ends, so a restart
        // can say what it cut off and redo what's safe to (`resume`).
        if crate::resume::what_to_do_with(name) != crate::resume::After::Skip {
            let redone = self.redoing.take().unwrap_or(0);
            self.unfinished.insert(
                crew_id,
                crate::resume::Unfinished {
                    label: name.to_string(),
                    topic: topic.clone(),
                    asked: self.last_said.clone(),
                    started: t,
                    redone,
                },
            );
            self.keep_unfinished();
        }
        self.crew_links.insert(crew_id, CrewLink { watch_id, label: name, topic, speak });
        Some(crew_id)
    }

    /// A hub button's slow part, on the crew: registered as a job its page
    /// can show, and the browser sent straight back to that page with the
    /// job's number (`hubjobs`). `work` gives the answer to show and a tag
    /// for `after_hub_errand`; it writes its answer itself, so the page has it
    /// even while the tick is paused.
    pub(crate) fn hub_errand(
        &mut self,
        name: &'static str,
        page: crate::hub::Page,
        extra: &str,
        label: &str,
        work: Box<dyn FnOnce() -> (std::result::Result<String, String>, String) + Send>,
        after: impl FnOnce(u64) -> HubAfter,
    ) -> crate::server::Reply {
        let job = self.hub_jobs.start(page, label);
        let jobs = self.hub_jobs.clone();
        let crew_work: crew::Work = Box::new(move |_| {
            let (said, tag) = work();
            jobs.finish(job, said.clone(), false);
            Ok(serde_json::json!({ "tag": tag, "said": said }).to_string())
        });
        let sep = if extra.is_empty() { "" } else { "&" };
        match self.hand_off_as(name, clock(), crew_work, None, SpeakPolicy::ViaWatcher) {
            Some(id) => {
                self.hub_after.insert(id, after(job));
                crate::hub::back_with(page.href(), &format!("{extra}{sep}job={job}"), label)
            }
            None => {
                let busy = "Atlas has too much waiting to start that now. Try again in a minute.";
                self.hub_jobs.finish(job, Err(busy.into()), true);
                crate::hub::back_with(page.href(), extra, busy)
            }
        }
    }

    /// A hub errand ended: the daemon's side of it. Returns anything worth
    /// saying out loud (nothing yet: a friend's Atlas answering a retry is
    /// told as `friend_upkeep` always told it, as a note).
    fn after_hub_errand(&mut self, after: HubAfter, ending: &crew::Ending, t: u64) -> Vec<String> {
        let (tag, said): (String, std::result::Result<String, String>) = match ending {
            crew::Ending::Done(Ok(json)) => {
                let v: serde_json::Value = serde_json::from_str(json).unwrap_or_default();
                let said = match (v["said"]["Ok"].as_str(), v["said"]["Err"].as_str()) {
                    (Some(s), _) => Ok(s.to_string()),
                    (_, Some(e)) => Err(e.to_string()),
                    _ => Err("That finished without saying how it went.".to_string()),
                };
                (v["tag"].as_str().unwrap_or_default().to_string(), said)
            }
            crew::Ending::Done(Err(e)) => (String::new(), Err(e.clone())),
            crew::Ending::Stopped => (String::new(), Err("That was stopped before it finished.".into())),
            crew::Ending::Vanished => (
                String::new(),
                Err("That stopped partway through. It's a fault in Atlas, not something you did; try again.".into()),
            ),
        };
        match after {
            HubAfter::Sent { job, doc, to } => {
                if tag == "sent" {
                    self.tray.sent_to(doc, &to, t);
                    let _ = self.tray.save(&self.store);
                }
                self.hub_jobs.finish(job, said, false);
            }
            HubAfter::FriendAdd { job, keep } => {
                let said = if tag.is_empty() { said } else { Ok(self.after_friend_knock(&keep, &knock_of(&tag))) };
                self.hub_jobs.finish(job, said, true);
            }
            HubAfter::FriendRetry { keep } => {
                let outcome = knock_of(&tag);
                if tag == "taken" || tag == "refused" {
                    let store = self.friend_store();
                    let mut out = crate::friends::Outbox::load(&store);
                    out.pending.retain(|x| x.hello.invite != keep.hello.invite);
                    if let Err(e) = out.save(&store) {
                        self.log.warn(&format!("couldn't save the friends still being reached: {e}"));
                    }
                }
                let said = match outcome {
                    crate::friends::Knock::Taken => format!("You and {} are friends now -- their Atlas came online.", keep.name),
                    crate::friends::Knock::Refused => {
                        self.unfriend_half(&keep.name);
                        format!(
                            "{}'s Atlas turned your friend link down -- it had been used or run out. Ask them for a new one.",
                            keep.name
                        )
                    }
                    crate::friends::Knock::Unreachable(_) => return Vec::new(),
                };
                // Told the way `friend_upkeep`'s own news is: kept in the
                // activity, and a routine note to you.
                self.journal.record_at(crate::activity::Kind::Upkeep, &said, true, t);
                let _ = self.reach_you(crate::notify::Note::new("Friends", &said, crate::notify::Urgency::Routine, t), t);
            }
            HubAfter::AddonShare { job, name, group } => {
                self.addon_shared_in_group(&name, group.as_ref());
                self.hub_jobs.finish(job, said, false);
            }
            HubAfter::PhoneCode { job, slot, stop } => {
                self.take_phone_code(&slot, &stop);
                self.hub_jobs.finish(job, said, false);
            }
        }
        Vec::new()
    }

    /// A phone's code that came up on the crew, now the one shown -- unless
    /// it was stopped while it was coming up.
    pub(crate) fn take_phone_code(
        &mut self,
        slot: &std::sync::Mutex<Option<crate::phoneadd::Showing>>,
        stop: &std::sync::atomic::AtomicBool,
    ) {
        let got = slot.lock().unwrap_or_else(std::sync::PoisonError::into_inner).take();
        if let Some(showing) = got {
            if stop.load(std::sync::atomic::Ordering::Relaxed) {
                showing.stop.store(true, std::sync::atomic::Ordering::Relaxed);
                return;
            }
            if let Some(old) = self.phone_code.replace(showing) {
                old.stop.store(true, std::sync::atomic::Ordering::Relaxed);
            }
        }
    }

    /// For a test: let every errand the crew has in hand finish, taking what
    /// each came to as the tick would. Gives up after thirty seconds.
    pub fn errands_done_for_test(&mut self) -> Vec<String> {
        let started = std::time::Instant::now();
        let mut said = Vec::new();
        loop {
            said.extend(self.take_crew_news(clock()));
            if (self.crew.active() == 0 && self.crew.queued() == 0) || started.elapsed().as_secs() > 30 {
                return said;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
    }

    /// The Status page's "Look for space" (`hubvault`): the disk survey,
    /// on the crew so the tick never waits for a walk of the disk. Reported
    /// through the watcher; not written down for `resume`, which skips it.
    pub(crate) fn hand_off_survey(&mut self, t: u64, work: crew::Work) -> Option<u64> {
        self.hand_off_as("reclaim", t, work, None, SpeakPolicy::ViaWatcher)
    }

    /// `social`'s errands (29 Sep 2026): `asked` is something you asked for
    /// and is said when it ends; otherwise a chore the watcher decides on.
    pub(crate) fn hand_off_social(&mut self, name: &'static str, t: u64, work: crew::Work, topic: Option<String>, asked: bool) -> Option<u64> {
        self.hand_off_as(name, t, work, topic, if asked { SpeakPolicy::Always } else { SpeakPolicy::ViaWatcher })
    }

    /// Write down the work in hand (`resume::RECORD`).
    fn keep_unfinished(&self) {
        let list: Vec<&crate::resume::Unfinished> = self.unfinished.values().collect();
        let _ = self.store.save(crate::resume::RECORD, &list);
    }

    /// Write down the windows being worked, when they've changed.
    pub(super) fn keep_window_jobs(&mut self) {
        let list: Vec<crate::resume::SavedWindow> = self
            .working_for_you
            .iter()
            .map(|w| crate::resume::SavedWindow {
                id: w.id,
                job: w.job.clone(),
                win: w.win.0,
                after_mine: w.after_mine.clone(),
                started: w.started,
                held: w.held,
            })
            .collect();
        let text = serde_json::to_string(&list).unwrap_or_default();
        if text != self.saved_windows {
            let _ = self.store.save(crate::resume::WINDOWS, &list);
            self.saved_windows = text;
        }
    }

    /// The first tick after a start: what the last run left unfinished.
    pub(super) fn pick_up_after_restart(&mut self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        // Questions you hadn't answered and a request being worked through
        // when Atlas last stopped: named once, never acted on from an old
        // yes. A day later they're stale and dropped quietly.
        if !self.left_waiting_read {
            self.left_waiting_read = true;
            let left: Vec<super::running::LeftWaiting> = self.store.load(super::running::LEFT_WAITING);
            let _ = self.store.save(super::running::LEFT_WAITING, &Vec::<super::running::LeftWaiting>::new());
            let fresh: Vec<_> = left.into_iter().filter(|l| t.saturating_sub(l.at) < 86_400).collect();
            let asked: Vec<String> = fresh.iter().filter(|l| l.asked).map(|l| l.what.trim_end_matches(['?', '.']).to_string()).collect();
            let doing: Vec<String> = fresh.iter().filter(|l| !l.asked).map(|l| l.what.clone()).collect();
            if !asked.is_empty() {
                out.push(format!(
                    "Before I restarted, {} waiting on your yes: {}. A yes from before doesn't carry over -- ask again if you still want {}.",
                    if asked.len() == 1 { "this was" } else { "these were" },
                    asked.join("; "),
                    if asked.len() == 1 { "it" } else { "them" }
                ));
            }
            if !doing.is_empty() {
                out.push(format!("I was partway through {} when I stopped. Ask again and I'll start it fresh.", doing.join(" and ")));
            }
            // A workflow in hand. One waiting on your yes is put back and
            // asked again -- the step is checked afresh when you answer
            // (`approved_flow_step`); one that was running is named, not
            // carried on, since its next step may be one to ask about.
            let flow: Option<crate::flow::Run> = self.store.load(super::running::FLOW_LEFT);
            let _ = self.store.save(super::running::FLOW_LEFT, &None::<crate::flow::Run>);
            if let Some(run) = flow.filter(|r| t.saturating_sub(r.started) < 86_400 && !r.finished()) {
                match run.state {
                    crate::flow::RunState::AwaitingApproval if self.current_flow.is_none() => {
                        let step = run.current().map(|s| s.command.clone()).unwrap_or_default();
                        let q = format!(
                            "Before I restarted, the \"{}\" workflow was waiting for your yes to: {step}. Go ahead?",
                            run.workflow
                        );
                        self.current_flow = Some(run);
                        self.session.ask(&q);
                        out.push(q);
                    }
                    crate::flow::RunState::Running => out.push(format!(
                        "I was on step {} of {} of the \"{}\" workflow when I stopped. Say \"run {}\" to start it again.",
                        run.position + 1,
                        run.steps.len(),
                        run.workflow,
                        run.workflow
                    )),
                    _ => {}
                }
            }
        }
        // Windows first: a conversation you left Atlas carrying on is picked
        // back up if its window is still open. You set it going; a restart
        // isn't a reason to ask again.
        for saved in std::mem::take(&mut self.left_windows) {
            let still_there = matches!(self.plat.read_window(crate::platform::WindowId(saved.win)), Ok(Some(_)));
            let win = if still_there {
                Some(crate::platform::WindowId(saved.win))
            } else {
                let spec = crate::config::AppSpec::for_process(&format!("{}.exe", saved.job.app));
                self.plat.find_window(&spec).ok().flatten()
            };
            match win {
                Some(win) if !self.working_for_you.iter().any(|w| w.win == win) => {
                    let app = saved.job.app.clone();
                    self.next_window_job = self.next_window_job.max(saved.id + 1);
                    self.working_for_you.push(WorkingForYou {
                        id: saved.id,
                        job: saved.job,
                        win,
                        after_mine: saved.after_mine,
                        looked: 0,
                        started: saved.started,
                        held: saved.held,
                        waiting_for_gap: false,
                        composing: None,
                        composed_from: None,
                        ready: None,
                    });
                    out.push(if saved.held {
                        format!("The conversation in {app} is still paused from before I restarted.")
                    } else {
                        format!("I restarted and picked the conversation in {app} back up.")
                    });
                }
                Some(_) => {}
                None => out.push(format!(
                    "I was working the conversation in {} when I stopped, and that window's gone now, so I've left it.",
                    saved.job.app
                )),
            }
        }
        let left = std::mem::take(&mut self.left_over);
        if left.is_empty() {
            return out;
        }
        let _ = self.store.save(crate::resume::RECORD, &Vec::<crate::resume::Unfinished>::new());
        let (again, ask) = crate::resume::sort(left);
        let describe = |u: &crate::resume::Unfinished| {
            crate::which_errand::describe(&crate::which_errand::Candidate {
                id: 0,
                label: u.label.clone(),
                topic: u.topic.clone(),
                started: u.started,
                paused: false,
                can_hold: false,
            })
        };
        if let Some(line) = crate::resume::said(&again, &ask, describe) {
            out.push(line);
        }
        for u in again {
            // Your own words again, through the same door they came in by.
            // Marked as a redo, so if this is what keeps bringing Atlas down
            // it isn't tried a third time.
            self.redoing = Some(u.redone + 1);
            let said_before = self.last_said.clone();
            let reply = self.turn(&u.asked, t);
            self.redoing = None;
            self.last_said = said_before;
            if !reply.trim().is_empty() {
                out.push(reply);
            }
        }
        out
    }

    pub(super) fn backup_cfg(&self) -> BackupConfig {
        self.tools_ref()
            .map(|t| t.backup.clone())
            .unwrap_or_default()
            .resolved(&self.store.install_root())
    }
    fn watching_cfg(&self) -> watching::WatchConfig {
        self.tools_ref().map(|t| t.watching.clone()).unwrap_or_default()
    }
    /// Called once a tick, right alongside `helpers.reap`. Never blocks —
    /// `crew::Crew::settle` doesn't, and neither does anything below it.
    ///
    /// What `outcome_of` maps an ending to is decided in exactly one place
    /// here, rather than at every call site that hands work to the crew —
    /// the same reason `contents.rs`'s and `revise.rs`'s single-source
    /// rules exist.
    pub(super) fn take_crew_news(&mut self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        for news in self.crew.settle(t) {
            let Some(link) = self.crew_links.remove(&news.id) else { continue };
            if self.unfinished.remove(&news.id).is_some() {
                self.keep_unfinished();
            }
            // A hub button's slow part (`hubjobs`): its answer is on its page
            // already; what's left is the daemon's own follow-up.
            if let Some(after) = self.hub_after.remove(&news.id) {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                out.extend(self.after_hub_errand(after, &news.ending, t));
                continue;
            }
            // The proving test's run, when it outlasted the turn that asked.
            if link.label == "proof-check" {
                if let Some(said) = self.finish_proof_check() {
                    self.long_work.update(link.watch_id, outcome_of(&news.ending), &said, t);
                    out.push(said);
                }
                continue;
            }
            // A self-fix drafted and proven on the crew: taken in and said.
            if link.label == "self-fix" {
                let said = match (self.finish_own_fix(), &news.ending) {
                    (Some(said), _) => said,
                    (None, crew::Ending::Done(Err(e))) => format!("I couldn't work on that fix: {e}"),
                    (None, _) => "The fix I was working on stopped before it finished.".into(),
                };
                self.long_work.update(link.watch_id, outcome_of(&news.ending), &said, t);
                out.push(said);
                continue;
            }
            // The next step of a job in an app (`operate`).
            if link.label == "operate" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.operate_news(news.id, &news.ending) {
                    out.push(said);
                }
                continue;
            }
            // A call written up: its envelope unpacked into what's said,
            // and the summary's model call recorded like every other.
            if link.label == "call-notes" {
                if let crew::Ending::Done(Ok(json)) = &news.ending {
                    if let Ok(w) = serde_json::from_str::<crate::callnotes::WrittenUp>(json) {
                        if let Some(asked) = &w.summary {
                            self.record_model_call("call-notes", w.summary_ms, w.prompt_chars, w.reply_chars, asked.clone().err());
                        }
                        let name = w.path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                        // What the call left behind: your follow-ups on the later list.
                        let mut later: crate::later::Later = self.store.load(crate::later::RECORD);
                        let added = w.follow.yours.iter().filter(|y| later.add(y, t)).count();
                        if added > 0 {
                            let _ = self.store.save(crate::later::RECORD, &later);
                        }
                        let after = crate::callnotes::follow_ups_said(&w.follow, added);
                        let said = format!("Your call notes are written: {name}, in your notes folder.{}{after}", if after.is_empty() { "" } else { " " });
                        self.long_work.update(link.watch_id, outcome_of(&news.ending), &said, t);
                        self.journal.record_at(Act::Upkeep, &format!("call notes: {name}"), true, t);
                        out.push(said);
                        continue;
                    }
                }
                if let crew::Ending::Done(Err(e)) = &news.ending {
                    self.long_work.update(link.watch_id, outcome_of(&news.ending), e, t);
                    out.push(format!("I couldn't write up the call: {e}. The audio is kept in data/calls."));
                    continue;
                }
            }
            // The picture reader finished: its memory back to the budget,
            // its model call recorded, and its answer said.
            if link.label == "pictures" {
                self.helpers.finished("picture reader");
                if let crew::Ending::Done(Ok(json)) = &news.ending {
                    if let Ok(o) = serde_json::from_str::<PictureOutcome>(json) {
                        self.record_model_call("pictures", o.took_ms, o.prompt_chars, o.answer.len(), o.failed.clone());
                        let said = match o.failed {
                            None => o.answer,
                            Some(why) => format!("I looked, but {why}."),
                        };
                        self.long_work.update(link.watch_id, outcome_of(&news.ending), &said, t);
                        out.push(said);
                        continue;
                    }
                }
            }
            // A search measurement: kept, and said against the last one.
            if link.label == "search-check" {
                if let crew::Ending::Done(Ok(json)) = &news.ending {
                    if let Ok(check) = serde_json::from_str::<crate::recall::SearchCheck>(json) {
                        let mut kept: Vec<crate::recall::SearchCheck> = self.store.load("search_checks");
                        let said = check.said(kept.last());
                        kept.push(check);
                        let keep_from = kept.len().saturating_sub(50);
                        let _ = self.store.save("search_checks", &kept[keep_from..].to_vec());
                        self.long_work.update(link.watch_id, outcome_of(&news.ending), &said, t);
                        out.push(said);
                        continue;
                    }
                }
            }
            // Codes, sign-ins, security changes and sign-ups.
            if matches!(link.label, "code" | "sign-in" | "security-change" | "sign-up") {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.two_factor_news(link.label, &news.ending, t) {
                    out.push(said);
                }
                continue;
            }
            // Old conversation, summarised (H12). Nothing said.
            if link.label == "fold" {
                self.folding = false;
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                match &news.ending {
                    crew::Ending::Done(Ok(json)) => {
                        if let Ok((n, summary)) = serde_json::from_str::<(usize, String)>(json) {
                            self.thread.fold_first(n, &summary);
                        }
                    }
                    // The model failed: folded plainly, so the thread still
                    // doesn't grow without end.
                    _ => {
                        let cfg = self.thread_cfg();
                        let old: Vec<crate::thread::Exchange> = self.thread.foldable(&cfg).to_vec();
                        let s = crate::thread::plain_fold(&self.thread.summary, &old);
                        self.thread.fold_first(old.len(), &s);
                    }
                }
                self.persist();
                continue;
            }
            // A gesture shown to the camera (H6).
            if link.label == "teach-gesture" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.gesture_news(&news.ending) {
                    out.push(said);
                }
                continue;
            }
            // A decision drafted by the model (H9).
            if link.label == "decide" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.decision_news(&news.ending) {
                    out.push(said);
                }
                continue;
            }
            // A window job's reply: routed to its window, not spoken.
            if link.label == "edit-media" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                match &news.ending {
                    crew::Ending::Done(Ok(json)) => {
                        if let Ok((original, copy, result, described)) = serde_json::from_str::<(String, String, String, String)>(json) {
                            let q = format!("Here it is: {result} — {described} Keep it?");
                            self.session.ask(&q);
                            self.pending_media_keep = Some((original, copy, result));
                            out.push(q);
                        }
                    }
                    crew::Ending::Done(Err(e)) => out.push(format!("I couldn't edit it: {e}. Your original is untouched.")),
                    _ => {}
                }
                continue;
            }
            // A file read or unpacked off the loop (`file_work_off_the_loop`).
            if link.label == "read-file" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                match &news.ending {
                    crew::Ending::Done(Ok(json)) => match serde_json::from_str::<FileDone>(json) {
                        Ok(done) => out.push(self.file_done(done)),
                        Err(e) => out.push(format!("I read it, but lost what I found on the way back ({e}). Ask me again.")),
                    },
                    crew::Ending::Done(Err(e)) => out.push(format!("I couldn't get to that file: {e}")),
                    crew::Ending::Stopped => out.push("Stopped -- I didn't finish with that file.".into()),
                    crew::Ending::Vanished => out.push("That file's reading stopped without finishing. Ask me again.".into()),
                }
                continue;
            }
            if link.label == "move-files" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.moved_news(&news.ending) {
                    out.push(said);
                }
                continue;
            }
            if link.label == "post" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.post_news(&news.ending, t) {
                    out.push(said);
                }
                continue;
            }
            if matches!(link.label, "mail-sort" | "mail-sort-apply") {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.mail_sort_news(link.label, &news.ending) {
                    out.push(said);
                }
                continue;
            }
            // Your accounts' numbers and what you watch (`social::glue`).
            if link.label.starts_with("social-") {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.social_news(link.label, &news.ending, t) {
                    out.push(said);
                }
                continue;
            }
            // "Look for space" on the Status page (`hubvault`).
            if link.label == "reclaim" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                if let Some(said) = self.reclaim_news(&news.ending, t) {
                    out.push(said);
                }
                continue;
            }
            if link.label == "conversation-reply" {
                self.long_work.update(link.watch_id, outcome_of(&news.ending), "", t);
                out.extend(self.reply_written(news.id, &news.ending));
                continue;
            }
            let (mut result, mut ok) = match &news.ending {
                crew::Ending::Done(Ok(s)) => (s.clone(), true),
                crew::Ending::Done(Err(e)) => (e.clone(), false),
                crew::Ending::Stopped => ("stopped before finishing".to_string(), true),
                crew::Ending::Vanished => ("vanished without finishing".to_string(), false),
            };

            // The council errand always returns `Ok` — even a deadlock or
            // too few seats reached is a real, successful completion of
            // the errand, not a technical failure — carrying a JSON
            // envelope rather than the spoken text directly, so every
            // seat's call can still be recorded individually the way the
            // synchronous version always did. Unpacked here, first, so
            // everything below (the watched job, the spoken line, the
            // journal entry) sees the actual words rather than the
            // envelope.
            if link.label == "council" && ok {
                match serde_json::from_str::<CouncilOutcome>(&result) {
                    Ok(outcome) => {
                        for c in &outcome.calls {
                            let id = self.record_model_call(
                                "council",
                                c.took_ms,
                                c.prompt_chars,
                                c.reply_chars,
                                c.failed.clone(),
                            );
                            if let Some((good, why)) = &c.grade {
                                self.grade_call(id, *good, why);
                                self.keep_words(id, c.words.as_ref());
                            }
                        }
                        if outcome.needs_guidance {
                            if let Some(q) = &link.topic {
                                self.backlog.record(
                                    &format!("ask the room: {q}"),
                                    crate::backlog::Blocker::NeedsYourDecision {
                                        question: q.clone(),
                                        set_aside: "a verdict on this".into(),
                                    },
                                    crate::store::now(),
                                );
                            }
                        } else if outcome.technical_failure {
                            if let Some(q) = &link.topic {
                                self.backlog.record(
                                    &format!("ask the room: {q}"),
                                    crate::backlog::Blocker::Failed(outcome.text.clone()),
                                    crate::store::now(),
                                );
                            }
                            ok = false;
                        }
                        result = outcome.text;
                    }
                    Err(_) => {
                        // Shouldn't happen — the envelope is built and
                        // read in the same binary — but a result that
                        // fails to parse must not be spoken as if it were
                        // the raw JSON.
                        result = "The room answered, but I couldn't make sense of its own report. \
                                  Check atlas trace."
                            .into();
                        ok = false;
                    }
                }
            }
            self.long_work.update(link.watch_id, outcome_of(&news.ending), &result, t);

            // The device flow's own errand always returns `Ok` carrying
            // the refresh token on success — stored in the vault here,
            // on the tick thread, since the errand itself can't touch
            // `self.vault`. `link.topic` carries the address it was for.
            // A finished project-improvement errand carries a JSON envelope
            // describing the proposed change. Filed into the project's queue
            // here, on the tick thread, because the errand cannot reach
            // `self.workshop` — the same handoff shape as council's outcome.
            // The change lands as Ready (waiting on you), never applied.
            if link.label == "improve" && ok {
                match serde_json::from_str::<ImproveOutcome>(&result) {
                    Ok(o) => {
                        let now = crate::store::now();
                        if let Some(id) = self.workshop.propose(
                            &o.project,
                            &o.title,
                            &o.what,
                            o.files,
                            o.verified,
                            &o.note,
                            now,
                        ) {
                            self.workshop.note_bases(&o.project, id, o.bases);
                        }
                        // Filed: the phases it was built in aren't needed now.
                        crate::phases::Phases::for_work(self.store.root(), "improve", &format!("{}\n{}", o.project, o.what)).close();
                        let _ = self.workshop.save(&self.store);
                        result = o.summary;
                    }
                    Err(_) => {
                        result =
                            "I built something for that, but couldn't file it cleanly — ask me to try again."
                                .into();
                        ok = false;
                    }
                }
            }

            if link.label == "outlook-connect" && ok {
                let (address, client_id) = match link.topic.as_deref().map(|t| t.split_once(' ').unwrap_or((t, ""))) {
                    Some((a, c)) => (Some(a.to_string()), c.to_string()),
                    None => (None, String::new()),
                };
                if let Some(address) = &address {
                    let vault_name = format!("outlook {address}");
                    let now = crate::store::now();
                    match self
                        .vault
                        .put(&vault_name, crate::vault::Kind::Login, &result, now)
                        .and_then(|()| {
                            self.vault
                                .save(&crate::roots::install_state())
                                .map_err(|e| format!("sealed it but couldn't write it: {e}"))
                        }) {
                        Ok(()) => {
                            result = match self.add_connected_account(address, &client_id, &vault_name) {
                                Ok(on) if on => format!("Connected {address}. I'll read it with your other mail from now on."),
                                Ok(_) => format!(
                                    "Connected {address}. Reading your mail is switched off, though -- turn it on in Settings, under Mail, and I'll read this one too."
                                ),
                                Err(e) => format!("Connected {address}, but I couldn't add it to your accounts ({e}), so I won't read it yet."),
                            };
                        }
                        Err(e) => {
                            result =
                                format!("Got the connection for {address}, but couldn't save it to the vault: {e}");
                            ok = false;
                        }
                    }
                }
            }

            // Side effects that used to run inline inside `research()` and
            // must now happen here, back on the tick thread, once the
            // crew errand that used to do this work synchronously has
            // actually finished.
            // 30 Sep 2026: stopped research counts as `ok` (you stopped it;
            // nothing failed), and so "stopped before finishing" was kept as
            // what Atlas had learned about the topic -- in the library and
            // the fact book. Stopped research learns nothing and blames
            // nothing.
            let stopped = matches!(news.ending, crew::Ending::Stopped);
            if link.label == "research" && !stopped {
                let now = crate::store::now();
                // Seconds, not milliseconds -- `News::started`/`finished`
                // are on the same clock every other long-running job in
                // this file is measured on (`watching::Job::ran_for`),
                // which is coarser than the `Instant`-based timing the old
                // synchronous call used, but consistent with how every
                // other crew errand's duration is already recorded.
                let took_ms = news.finished.saturating_sub(news.started).saturating_mul(1000);
                let prompt_chars = link.topic.as_deref().map(str::len).unwrap_or(0);
                let id = self.record_model_call(
                    "research",
                    took_ms,
                    prompt_chars,
                    if ok { result.len() } else { 0 },
                    if ok { None } else { Some(result.clone()) },
                );
                // Graded on the one thing that can be checked without a
                // model: every figure it gave is in a page it read.
                if ok {
                    let invented = result.contains(crate::research::UNCONFIRMED);
                    self.grade_call(id, !invented, if invented { "stated a figure that isn't in its sources" } else { "" });
                }
                if ok {
                    if let Some(i) = self.connections.get_mut(crate::integrations::INTERNET) {
                        i.worked(now);
                    }
                    // The finding lands in the knowledge store, merged
                    // rather than duplicated, so asking the same thing
                    // next week is answered from here instead of costing
                    // a second search. `learned` was built for exactly
                    // this handoff and had no caller.
                    let source = match &link.topic {
                        Some(topic) => format!("research: {topic}"),
                        None => "research".to_string(),
                    };
                    self.learned(&result, &source, now);
                    let _ = self.store.save("known", &self.known);
                    // Immediately, not on next start — a note you cannot
                    // find until you restart is a note you will not know
                    // you have.
                    self.reload_library();
                    // And into the one fact book, so "what do you know about
                    // <topic>" answers from what Atlas researched as well as
                    // from what you stated — a single indexed place for
                    // everything it knows, not a separate research silo. A
                    // researched finding is external reference knowledge: kept
                    // (never an evictable guess), merged on re-research, and
                    // decaying slowly the way a looked-up fact should.
                    let summary: String = result
                        .lines()
                        .find(|l| !l.trim().is_empty())
                        .unwrap_or("")
                        .chars()
                        .take(140)
                        .collect();
                    let fact_name = link.topic.clone().unwrap_or_else(|| summary.clone());
                    if !summary.trim().is_empty() {
                        self.facts.learn(
                            crate::facts::Fact::new(
                                &fact_name,
                                &summary,
                                &result,
                                crate::facts::Kind::Reference,
                                now,
                            ),
                            now,
                        );
                        self.facts.trim(crate::facts::MEMORY_BUDGET_BYTES, now);
                        let _ = self.facts.save(&self.store);
                    }
                } else {
                    if let Some(i) = self.connections.get_mut(crate::integrations::INTERNET) {
                        i.failed(now, &result);
                    }
                    // A fetch that failed is evidence the connection may be
                    // gone: the next check looks again rather than trusting
                    // "online" from before.
                    self.connectivity.invalidate();
                    if let Some(topic) = &link.topic {
                        self.backlog.record(
                            &format!("research {topic}"),
                            crate::backlog::Blocker::Failed(result.clone()),
                            now,
                        );
                    }
                    // A failure names the next-most-reliable route of the
                    // same kind, filtered by whether the machine is even
                    // online — `another_way` and the whole `route` module
                    // were decision-shaped and reached nothing.
                    match (self.another_way(crate::route::Kind::Learn, "research"), &link.topic) {
                        (Some(next), _) => result.push_str(&format!(" I could try {next} instead.")),
                        // Nothing else to try: said as being stuck (F7).
                        (None, Some(topic)) => {
                            result.push(' ');
                            result.push_str(&crate::route::stuck_on(&format!("researching {topic}"), "every way in failing"));
                        }
                        (None, None) => {}
                    }
                }
            }

            match link.speak {
                // The answer to something you asked for is said the
                // moment it's ready, not filtered through "was this worth
                // interrupting for" — that filter is for chores you never
                // asked about in the first place.
                SpeakPolicy::Always => {
                    out.push(result.clone());
                    if let Some(j) = self.long_work.jobs.iter_mut().find(|j| j.id == link.watch_id) {
                        j.reported = true;
                    }
                }
                SpeakPolicy::ViaWatcher => {}
            }

            // A quiet, successful "nothing to do" run of something like
            // housekeeping is not a fact worth a journal line every time it
            // fires; a real result (non-empty) or a failure always is.
            if !result.is_empty() || !ok {
                self.journal.record_at(Act::Upkeep, &format!("{}: {result}", link.label), ok, t);
            }
        }
        for (_id, name) in self.crew.still_wont_stop(t) {
            out.push(format!("{name} was asked to stop and hasn't yet."));
        }
        out.extend(self.long_work.to_report(&self.watching_cfg(), t));
        self.long_work.prune(&self.watching_cfg(), t);
        out
    }
}

/// Mail accounts Atlas connected itself (Outlook's sign-in), kept apart from
/// tools.yaml -- which `atlas update` replaces -- and laid over it when the
/// settings are resolved (`resolve_tools`).
pub const CONNECTED_ACCOUNTS: &str = "mail_accounts_connected";

impl Daemon<'_> {
    /// Add an account that has just been connected, and use it now. Returns
    /// whether reading mail is switched on.
    fn add_connected_account(&mut self, address: &str, client_id: &str, vault_name: &str) -> std::result::Result<bool, String> {
        let name = address.split('@').next().unwrap_or(address).to_string();
        self.keep_connected_account(crate::mail::Account {
            name,
            address: address.to_string(),
            password_from_vault: vault_name.to_string(),
            oauth: true,
            client_id: client_id.to_string(),
            ..Default::default()
        })
    }

    /// Keep an account connected from the hub or by Outlook's sign-in, in
    /// place of any by the same address, and use it now. Returns whether
    /// reading mail is switched on.
    pub(crate) fn keep_connected_account(&mut self, account: crate::mail::Account) -> std::result::Result<bool, String> {
        let mut kept: Vec<crate::mail::Account> = self.store.load(CONNECTED_ACCOUNTS);
        kept.retain(|a| !a.address.eq_ignore_ascii_case(&account.address));
        kept.push(account);
        self.store.save(CONNECTED_ACCOUNTS, &kept).map_err(|e| e.to_string())?;
        self.tools_resolved = std::sync::Arc::new(resolve_tools(self.tools_ref(), &self.store));
        Ok(self.tools_cfg().mail.enabled)
    }

    /// Take a connected account off (`connecting`): `false` when it wasn't
    /// one Atlas connected (one listed in tools.yaml stays yours to edit).
    pub(crate) fn drop_connected_account(&mut self, address: &str) -> std::result::Result<bool, String> {
        let mut kept: Vec<crate::mail::Account> = self.store.load(CONNECTED_ACCOUNTS);
        let before = kept.len();
        kept.retain(|a| !a.address.eq_ignore_ascii_case(address));
        if kept.len() == before {
            return Ok(false);
        }
        self.store.save(CONNECTED_ACCOUNTS, &kept).map_err(|e| e.to_string())?;
        // Rebuilt from the file and what's left, so it's gone from what runs now.
        self.tools_resolved = std::sync::Arc::new(resolve_tools(self.tools_ref(), &self.store));
        Ok(true)
    }
}
