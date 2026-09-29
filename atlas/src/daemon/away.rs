//! Dictation, notifications, the work log and quiet time, and coming back:
//! what happened while you were away and the brief.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    pub(super) fn phone_cfg(&self) -> crate::phone::PhoneConfig {
        self.tools_ref().map(|t| t.phone.clone()).unwrap_or_default()
    }

    /// The dictation settings, or the type's own defaults.
    pub(super) fn dictate_cfg(&self) -> crate::dictate::DictateConfig {
        self.tools_ref().map(|t| t.dictate.clone()).unwrap_or_default()
    }

    /// The window you are looking at: (what identifies it, what it is called).
    ///
    /// The process is the identity, because `Dictation` compares it every
    /// turn to notice you have moved — and a window *title* changes as you
    /// type into it, which would end dictation on the second sentence of
    /// every document. The title is kept for the block check, since "Slack"
    /// appears there on machines where the process is `electron`.
    ///
    /// Empty when the platform cannot say. That is not the same as "no
    /// window", and dictation treats it as a reason to refuse rather than a
    /// reason to type into whatever is there.
    fn focused_window(&self) -> (String, String) {
        self.plat
            .active_window()
            .ok()
            .flatten()
            .map(|w| (w.process, w.title))
            .unwrap_or_default()
    }

    /// Whether Atlas may type into this window at all.
    ///
    /// Two lists, and they are not the same list. `dictate.never_into` is
    /// about *dictation* — a misheard sentence in a chat window is public,
    /// where in a document it is a typo. `apps.*.no_input` is about Atlas
    /// typing anywhere, for any reason, and was written long before this. An
    /// app on either list is refused; neither is a subset of the other, and
    /// collapsing them would quietly drop whichever was checked second.
    ///
    /// Both are matched against the process AND the title, because an
    /// Electron app is `electron` in one and "Slack" in the other, and
    /// checking only the field that happens to be stable is how a chat window
    /// gets dictated into.
    fn may_dictate_into(&self, process: &str, title: &str) -> bool {
        if process.trim().is_empty() && title.trim().is_empty() {
            return false;
        }
        let cfg = self.dictate_cfg();
        let blocked = |name: &str| {
            let lower = name.to_lowercase();
            crate::workspace::input_blocked_apps(self.cfg)
                .iter()
                .any(|b| !b.is_empty() && lower.contains(&b.to_lowercase()))
                || !crate::dictate::may_type_into(name, &cfg)
        };
        !(blocked(process) || blocked(title))
    }

    /// Start typing what you say.
    pub(super) fn start_dictating(&mut self, first: &str, t: u64) -> String {
        let cfg = self.dictate_cfg();
        if !cfg.enabled {
            return "Dictation is switched off — turn on `dictate.enabled` in tools.yaml.".into();
        }
        let (app, title) = self.focused_window();
        if app.trim().is_empty() {
            // Refusing beats guessing: typing into "whatever is in front"
            // when Atlas cannot name it is how dictated text lands in the
            // wrong window.
            return "I can't tell which window you're in, so I won't start typing into it.".into();
        }
        if !self.may_dictate_into(&app, &title) {
            return crate::dictate::refusal(if title.trim().is_empty() { &app } else { &title });
        }

        self.dictation = Some(crate::dictate::Dictation::start(&app, t));
        // One sentence, carrying both facts. The reply goes through
        // `persona.spoken`, which caps a task register at a sentence or two —
        // so a second sentence saying how to get out was being cut off, which
        // is the worst possible half to lose.
        let opening = format!("Dictating into {app} — say \"stop dictating\" to finish.");
        if first.trim().is_empty() {
            return opening;
        }
        // Anything said after the trigger is the first line, so "type this:
        // dear Sarah" does not need a second breath.
        format!("{opening} {}", self.dictated(first, t))
    }

    /// One utterance, while dictation is on.
    ///
    /// Everything that ends dictation comes back from `Dictation::heard` as an
    /// `Err` carrying the sentence to say — stopping, taking something back,
    /// and moving to another window. Only the first and third actually end it,
    /// which is why the state is read back rather than assumed.
    pub(super) fn dictated(&mut self, said: &str, t: u64) -> String {
        let cfg = self.dictate_cfg();
        let (app, title) = self.focused_window();
        let Some(d) = self.dictation.as_mut() else {
            return String::new();
        };

        let outcome = d.heard(said, &app, &cfg, t);
        let still_on = d.state == crate::dictate::State::On;
        if !still_on {
            self.dictation = None;
        }

        match outcome {
            Err(why) => why,
            Ok(text) if text.trim().is_empty() => String::new(),
            Ok(text) => {
                // Checked again on every line, not only at the start: you can
                // move a window, or an app can rename itself, between one
                // sentence and the next.
                if !self.may_dictate_into(&app, &title) {
                    self.dictation = None;
                    return crate::dictate::refusal(&app);
                }
                match self.plat.type_text(&text) {
                    Ok(()) => String::new(),
                    Err(e) => {
                        // A failure to type is not a quiet one. Dictation that
                        // silently stops reaching the window is worse than
                        // dictation that stops.
                        self.dictation = None;
                        format!("I couldn't type that, so I've stopped: {e}")
                    }
                }
            }
        }
    }

    pub(super) fn notify_cfg(&self) -> crate::notify::NotifyConfig {
        self.tools_ref().map(|t| t.notify.clone()).unwrap_or_default()
    }

    /// The returning config, with a spoken address change layered over it.
    ///
    /// Two sources genuinely disagree here and both are legitimate:
    /// `tools.yaml`'s `returning.address`, and whatever you last told Atlas
    /// out loud (`Intent::AddressAs`, persisted by `Address::save`). The
    /// spoken one wins — it is the more recent and more deliberate of the
    /// two, and "stop calling me that" would otherwise be undone by a config
    /// file on the next restart.
    ///
    /// The check is `store.exists`, not the loaded value: `Address::None` is
    /// both the default for a store that has never been written and the
    /// thing "stop calling me that" deliberately writes, so reading the
    /// value alone cannot tell "never asked" from "asked for nothing", and
    /// treating the second as the first is exactly the bug this avoids.
    /// The persona, with whatever you last told Atlas to call you.
    ///
    /// The precedence is not re-decided here: `returning_cfg` already
    /// resolves `tools.yaml`'s `returning.address` against the spoken
    /// `Intent::AddressAs`, and explains at length why the spoken one wins
    /// ("stop calling me that" must not be undone by a config file on the
    /// next restart). This reuses that answer rather than inventing a second
    /// one, which matters because there were already two unconnected notions
    /// of what Atlas calls you: `ReturnConfig::address`, used when you come
    /// back after an absence, and `Persona::address`, which is the one that
    /// reaches the model -- and which nothing ever set.
    ///
    /// So "call me Eric" was honoured when Atlas greeted you after a gap and
    /// ignored in every actual reply.
    ///
    /// Two more, found 27 Sep 2026 when Atlas started going to Eric's
    /// friends: "stop calling me that" left the settings' name in every
    /// reply (an empty spoken answer was read as no answer), and while Atlas
    /// is handed over the guest was called by the owner's name, because the
    /// guest's profile has no spoken name and the settings' one filled in.
    pub(crate) fn persona_now(&self) -> Persona {
        let mut p = self.persona.clone();
        if self.handover().stance.handed_over() {
            p.address = String::new();
            return p;
        }
        let r = self.returning_cfg();
        if self.store.exists("address") || !r.address.trim().is_empty() {
            p.address = r.address.trim().to_string();
        }
        p
    }

    /// The run loop finished a piece of its own work (a turn, a tick) at
    /// `t`. A long one stalls the ticks, and that gap is Atlas busy with you,
    /// not the laptop shut: the next break check measures from here.
    pub fn done_working(&mut self, t: u64) {
        self.worked_until = self.worked_until.max(t);
    }

    pub(super) fn worklog_cfg(&self) -> crate::worklog::WorkLogConfig {
        self.tools_ref().map(|t| t.worklog.clone()).unwrap_or_default()
    }

    /// How long you've given no sign of being here: since you last spoke,
    /// or — where the machine can say — since you last touched the keyboard
    /// or mouse, whichever is shorter. Speaking is not the only way of being
    /// at the desk; before this, twenty silent minutes of typing counted as
    /// away, held notes as if you'd gone, and after an hour sent them to
    /// your phone while you sat in front of the screen.
    pub(super) fn quiet_for(&self, t: u64) -> u64 {
        let spoke = t.saturating_sub(self.awareness.last_spoke);
        // On a call or watching something counts as here (see
        // `worklog::effective_idle`).
        let idle = crate::worklog::effective_idle(
            &self.worklog_cfg(),
            self.plat.active_window().ok().flatten().as_ref(),
            self.plat.quiet_state(),
            self.plat.input_idle_secs(),
        );
        match idle {
            Some(idle) => spoke.min(idle),
            None => spoke,
        }
    }

    /// One heartbeat into the work log, and what the keyboard and mouse say
    /// about being away (`worklog`). Saved on its own schedule — every ten
    /// minutes and on the way out — because a record that changes every tick
    /// written every tick would break the ten-minute rule `persist_after`
    /// keeps (Microsoft's idle-energy guidance). At most ten minutes of it can
    /// be lost to a hard crash.
    pub(super) fn note_work(&mut self, s: &crate::awareness::Signals, t: u64) {
        let cfg = self.worklog_cfg();
        // A gap in the ticks themselves: the lid was shut, the machine
        // slept. Nothing ticked while you were gone, so the keyboard's
        // silence was never seen — but the gap says it all. The commonest
        // break there is (closing the laptop for lunch) would otherwise pass
        // without a word on your return.
        // A call or a video holds you without a key pressed (see
        // `worklog::effective_idle`); that silence isn't away.
        let idle_now = crate::worklog::effective_idle(&cfg, s.active.as_ref(), s.os_quiet, s.input_idle_secs);
        let since = self.last_beat.max(self.worked_until);
        let gap = t.saturating_sub(since);
        if self.last_beat > 0 && gap > self.away_after {
            let back = idle_now.map(|i| t.saturating_sub(i)).unwrap_or(t).max(since);
            if back.saturating_sub(since) > self.away_after {
                self.back_from = Some((self.input_away.take().unwrap_or(since), back));
            }
        }
        self.last_beat = t;
        if let Some(idle) = idle_now {
            let last_input = t.saturating_sub(idle);
            if idle >= self.away_after {
                self.input_away.get_or_insert(last_input);
            } else {
                if let Some(left) = self.input_away.take() {
                    if last_input.saturating_sub(left) > self.away_after {
                        self.back_from = Some((left, last_input));
                    }
                }
                self.last_present = self.last_present.max(last_input);
            }
        }
        self.worklog.beat(&cfg, t, s.active.as_ref(), idle_now);
        // What an ordinary pause is in each app, learned as you work, so a
        // held offer waits for a pause that's long *for that app*.
        if let Some(w) = s.active.as_ref() {
            self.worklog.note_pause(&w.process, t, s.input_idle_secs);
        }
        if self.awareness.learned_pause.is_empty() || t.saturating_sub(self.worklog.saved_at) >= 600 {
            self.awareness.learned_pause = self.worklog.pause_thresholds();
        }
        if t.saturating_sub(self.worklog.saved_at) >= 600 {
            self.worklog.prune(&cfg, t);
            self.worklog.saved_at = t;
            let _ = self.store.save("worklog", &self.worklog);
        }
    }

    /// "Where did my time go?" — today, yesterday or this week, from the
    /// work log.
    pub(super) fn time_spent(&mut self, what: &str, t: u64) -> String {
        let cfg = self.worklog_cfg();
        if !cfg.enabled {
            return "I'm not keeping a record of where your time goes — it's switched off (worklog.enabled in settings).".into();
        }
        let zone = self.home_zone();
        let lnow = zone.to_local(t as i64).max(0) as u64;
        let today = crate::calendar::start_of_day(lnow);
        let low = what.to_lowercase();
        let (from, to, name) = if low.contains("yesterday") {
            (today - 86_400, today, "yesterday")
        } else if low.contains("week") {
            (today - 6 * 86_400, today + 86_400, "over the last seven days")
        } else {
            (today, today + 86_400, "today")
        };
        let back = |local: u64| zone.to_utc(local as i64).max(0) as u64;
        let spans = self.worklog.between(back(from), back(to));
        // Asked about one thing ("how long was I on YouTube"): that thing.
        if let Some(line) = crate::worklog::time_on(&spans, &low, name) {
            return line;
        }
        let summary = crate::worklog::summarise(&spans);
        let clock = |u: u64| {
            let l = zone.to_local(u as i64).max(0) as u64 % 86_400;
            format!("{}:{:02}", l / 3600, (l % 3600) / 60)
        };
        let blind = !self.worklog.saw_input && self.worklog.blind_beats > 0;
        crate::worklog::say(&summary, name, blind, &clock)
    }

    pub(super) fn returning_cfg(&self) -> crate::returning::ReturnConfig {
        let mut cfg = self.tools_ref().map(|t| t.returning.clone()).unwrap_or_default();
        if self.store.exists("address") {
            match crate::returning::Address::load(&self.store) {
                crate::returning::Address::None => cfg.address = String::new(),
                crate::returning::Address::Name(n) => {
                    cfg.address = n;
                    cfg.address_as = "name".into();
                }
                crate::returning::Address::Title(t) => {
                    cfg.address = t;
                    cfg.address_as = "title".into();
                }
            }
        }
        cfg
    }

    /// What happened while you were gone, as the structured list
    /// `returning.rs` reasons about.
    ///
    /// Two sources, because two different things accumulate while you are
    /// out: the journal (what Atlas did) and the outbox (what Atlas wanted
    /// to tell you and could not). Merged and ordered by when, so the brief
    /// reads as one absence rather than two lists.
    pub(super) fn happened_while_away(
        &self,
        since: u64,
        waiting: &[crate::notify::Note],
    ) -> Vec<crate::returning::Happened> {
        let mut out: Vec<crate::returning::Happened> = Vec::new();

        for e in self.journal.since(since) {
            // Which kinds reach a brief at all, stated positively.
            //
            // `Journal::brief` selected these same three by picking them out
            // one at a time, so the exclusions were implicit and easy to lose
            // — and losing them is not cosmetic. The first version of this
            // function excluded only `Upkeep`, which let `Asked` through, and
            // `Asked` is a verbatim record of what *you* said. Coming back to
            // "I've got an update on call me boss when you're ready" is how
            // that reads: your own instruction from before you left, recited
            // back as news.
            //
            //   `Asked`     — you were there. You said it.
            //   `Upkeep`    — log rotation is not news.
            //   `Offered`   — an offer that reached you, you already heard;
            //                 one that did not is held in the outbox, which
            //                 is merged in below. Counting the journal entry
            //                 too would say it twice.
            //
            match e.kind {
                crate::activity::Kind::Published
                | crate::activity::Kind::Scheduled
                | crate::activity::Kind::Blocked => {}
                crate::activity::Kind::Asked
                | crate::activity::Kind::Upkeep
                | crate::activity::Kind::Offered => continue,
            }
            out.push(crate::returning::Happened {
                what: e.what.clone(),
                // `Blocked` is the journal's own word for "Atlas couldn't,
                // and filed it" — the one kind that is genuinely still
                // waiting on a decision from you.
                //
                // `Offered` deliberately does not count. An offer recorded
                // in the journal may well have been answered already — the
                // journal records that it was made, never that it is still
                // open — and a brief that reads out every offer you have
                // ever been made as though it still needs you is the fastest
                // way to make the whole brief ignorable.
                needs_you: e.kind == crate::activity::Kind::Blocked,
                failed: !e.ok,
                at: e.at,
            });
        }

        for n in waiting {
            out.push(crate::returning::Happened {
                what: n.title.clone(),
                // Of the two flags, this is the honest one for an urgent
                // note. `notify`'s own definition of `Urgent` is "the
                // machine is about to stop working, something is failing, a
                // deadline is passing" — things that want you now. `failed`
                // would additionally claim something already went wrong,
                // which a deadline approaching has not. Both flags route to
                // `Welcome::Straight` either way, so this costs nothing and
                // keeps the list true if anything ever reads the flags apart.
                needs_you: n.urgency == crate::notify::Urgency::Urgent,
                failed: false,
                at: n.at,
            });
        }

        out.sort_by_key(|h| h.at);
        out
    }

    /// Rebuild the searchable library from what is actually on disk.
    ///
    /// `recall.rs` was complete — ranking, rarity weighting, a relative floor
    /// that keeps a weak hit from riding along beside a strong one, and a
    /// `clarity()` check that reports when two notes answer a question two
    /// ways — and nothing ever put a single piece into a `Library`, so none of
    /// it could run. Wiring `research` made that worse rather than better:
    /// Atlas started writing notes to disk that it had no way to find again.
    ///
    /// Read from the filesystem rather than kept as a parallel index, because
    /// a stale index that looks current is the failure this codebase keeps
    /// finding. The notes are small and there are not many.
    /// Upkeep Atlas has decided it should not settle on its own.
    ///
    /// Deliberately short. Housekeeping Atlas can simply do belongs in the
    /// hourly sweep and never in a brief; what belongs here is what it has
    /// stopped and left for you. `guesses_worth_checking` in particular had
    /// nowhere to go but the log, where nothing reads it.
    fn upkeep_questions(&self, now: u64) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(d) = &self.index_drifted {
            out.push(format!("the notes index has drifted — {}", d.plain()));
        }
        for f in self.facts.guesses_worth_checking(now).iter().take(3) {
            out.push(format!("still true? {} ({})", f.summary, f.kind.plain()));
        }
        out
    }

    /// The morning run, built from what is already on this machine.
    ///
    /// The whole point of the redesign: `brief::run(&[], &[], &default())` was
    /// the only call this module ever got, so the brief was a function that
    /// always returned the same empty answer. Everything handed over here is
    /// something Atlas already holds with nothing plugged in.
    pub fn brief_now(&mut self, now: u64) -> crate::brief::Brief {
        // Loaded rather than held: the doorstep is written by the server
        // thread when a friend sends something, so a copy kept on the daemon
        // would be the stale one.
        let inbox = crate::household::Inbox::load(&self.store);
        let mut upkeep = self.upkeep_questions(now);
        // The list for later, once a week (F8).
        let mut later: crate::later::Later = self.store.load(crate::later::RECORD);
        if let Some(line) = later.weekly_line(now) {
            upkeep.push(line);
            let _ = self.store.save(crate::later::RECORD, &later);
        }
        // Ideas saved and never come back to, each named once (F1).
        let mut named: Vec<u64> = self.store.load("ideas_named");
        let ideas = crate::capture::ideas_to_name(&self.notebook, now, &named);
        if let Some(line) = crate::capture::ideas_line(&ideas, now) {
            named.extend(ideas.iter().map(|n| n.id));
            upkeep.push(line);
            let _ = self.store.save("ideas_named", &named);
        }
        // Logins you haven't used: once a month at most, in the brief only.
        let last_said: u64 = self.store.load("quiet_logins_said");
        if now.saturating_sub(last_said) >= crate::signin::QUIET_EVERY_DAYS * 86_400 {
            if let Some(line) = self.access.quiet_line(now) {
                upkeep.push(line);
                let _ = self.store.save("quiet_logins_said", &now);
            }
        }
        // `Unknown` counts as offline here. Listing an email you cannot open
        // costs more than leaving it out for one brief, and it comes back the
        // moment the check succeeds — the same caution `Reach` documents for
        // requirement decisions.
        let online = self.connectivity.cached() == crate::connectivity::Reach::Online;
        let cfg = self.tools_cfg().brief.clone();
        let since = self.last_brief;
        let sources = crate::brief::Sources {
            inbox: &inbox,
            backlog: &self.backlog,
            scheduler: &self.scheduler,
            proposals: &self.proposals,
            publisher: &self.publisher,
            upkeep: &upkeep,
            // No reader in this build. Named rather than omitted so that
            // adding one is a change at exactly this line.
            mail: &[],
            online,
        };
        let mut b = crate::brief::from_here(&sources, &cfg, now, since);
        // Your own day, from what's kept on this machine (round 11): promises
        // due, replies owed, people, habits, notes dated today, the market's
        // calendar. Appended after what the brief already ranked, so none of
        // it pushes a failed job or a friend's note down the list.
        let day = self.day_items(now);
        if b.start_with.is_none() {
            b.start_with = day.iter().find(|i| i.weight == crate::brief::Weight::Urgent).map(|i| i.subject.clone());
        }
        b.yours.extend(day);
        self.last_brief = now;
        b
    }
}
