//! Dictation, notifications, the work log and quiet time, and coming back:
//! what happened while you were away and the brief.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    /// You're here, as of `t`: no "welcome back" on the next turn. For the
    /// self-test, whose sentences are a minute apart on its own clock while
    /// each one starts a fresh Atlas -- every reply came back with "Before the
    /// break you'd been in..." (1 Oct 2026 report).
    pub fn here_at(&mut self, t: u64) {
        self.last_present = t;
        self.back_from = None;
    }

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

    /// The evening wrap-up (and, on Fridays or when asked, the week), from
    /// your list, the work log and the jobs Atlas ran (`daily::wrap_said`).
    pub(super) fn wrap_now(&mut self, t: u64, week: bool) -> crate::daily::Wrap {
        let off = crate::localclock::offset_secs();
        let today = crate::localclock::midnight(t, off);
        let summary = crate::worklog::summarise(&self.worklog.between(today, t));
        let done = |i: &crate::workspace_view::Item, from: u64| {
            i.status == crate::workspace_view::Status::Done && i.closed_at.is_some_and(|c| c >= from && c <= t)
        };
        let carried: Vec<(String, u32)> = self
            .workspace
            .iter()
            .filter(|i| i.status.live())
            .map(|i| (i.title.clone(), self.carried.iter().find(|(c, _)| *c == i.title).map(|(_, n)| *n).unwrap_or(0)))
            .collect();
        let tomorrow = crate::localclock::day_here(t + 86_400) as u64;
        let week = (week || crate::localclock::weekday(t, off) == 4).then(|| {
            let monday = today - crate::localclock::weekday(t, off) as u64 * 86_400;
            let spans = self.worklog.between(monday, t);
            let mut per_day: std::collections::BTreeMap<u64, u64> = std::collections::BTreeMap::new();
            for s in &spans {
                *per_day.entry(crate::localclock::midnight_here(s.start)).or_default() += s.secs();
            }
            let w = crate::worklog::summarise(&spans);
            crate::daily::Week {
                active_secs: w.active,
                days_worked: per_day.values().filter(|s| **s >= 1800).count() as u32,
                focus_blocks: w.blocks.len(),
                finished: self.workspace.iter().filter(|i| done(i, monday)).count(),
            }
        });
        crate::daily::Wrap {
            finished: self.workspace.iter().filter(|i| done(i, today)).map(|i| i.title.clone()).collect(),
            carried,
            handled: crate::brief::handled_since(&self.scheduler, today),
            active_secs: summary.active,
            top: summary.by_category.clone(),
            longest_focus: summary.blocks.first().map(|b| (b.category.clone(), b.end.saturating_sub(b.start))),
            push: self.facts.get("push them on").and_then(|f| crate::brief::push_piece(&f.summary, tomorrow)),
            week,
        }
    }

    /// "Two hours on the edit", "end the session", "how long is left".
    pub(super) fn worksession_help(&mut self, said: &str, t: u64) -> Option<String> {
        match crate::worksession::heard(said)? {
            crate::worksession::Said::Start { what, secs } => {
                if let Some(s) = self.work_session.as_ref().filter(|s| !s.over(t)) {
                    return Some(format!(
                        "You're already in a session on {} -- {} left. Say \"end the session\" first if you want a new one.",
                        s.what,
                        crate::worklog::duration_words(s.left(t))
                    ));
                }
                let s = crate::worksession::Session::new(&what, secs, t);
                let said = crate::worksession::started(&s);
                self.proactive.quiet_until = s.until;
                self.work_session = Some(s);
                let _ = self.store.save("work_session", &self.work_session);
                Some(said)
            }
            crate::worksession::Said::End => Some(match self.work_session.is_some() {
                true => self.end_work_session(t),
                false => "There's no work session going.".into(),
            }),
            crate::worksession::Said::HowLong => Some(match self.work_session.as_ref() {
                Some(s) if !s.over(t) => format!("{} left on {}.", crate::worklog::duration_words(s.left(t)), s.what),
                _ => "There's no work session going.".into(),
            }),
        }
    }

    /// End the session: how it went, said and kept in your notes.
    pub(super) fn end_work_session(&mut self, t: u64) -> String {
        let Some(s) = self.work_session.take() else { return String::new() };
        let _ = self.store.save("work_session", &self.work_session);
        self.proactive.quiet_until = 0;
        let ended = t.min(s.until);
        let log = crate::worklog::summarise(&self.worklog.between(s.started, ended));
        // What was held back during it, said now and taken off the outbox
        // -- the same hand-over the welcome back does.
        let cfg = self.notify_cfg();
        let held = self.outbox.collect(t, &cfg);
        let held = if held.is_empty() { String::new() } else { crate::notify::spoken(&held) };
        let said = crate::worksession::how_it_went(&s, ended, &log, &held);
        let off = crate::localclock::offset_secs();
        let clock = |u: u64| {
            let l = (u as i64 + off).rem_euclid(86_400) as u64;
            format!("{}:{:02}", l / 3600, (l % 3600) / 60)
        };
        let dir = self.notes_dir();
        let filed = (|| -> crate::error::Result<()> {
            let _guard = self.store.transaction()?;
            std::fs::create_dir_all(&dir)?;
            use std::io::Write;
            let mut f = std::fs::OpenOptions::new().create(true).append(true).open(dir.join("work-sessions.md"))?;
            writeln!(f, "{}", crate::worksession::note_line(&s, ended, &said, &clock))?;
            f.sync_all()?;
            Ok(())
        })();
        match filed { Ok(()) => said, Err(error) => format!("{said} I couldn't save the work-session note ({error}).") }
    }

    /// On the phone app, your answer to the free online models: "use online
    /// models" or "stop using online models" (`phonemode`). Kept, so it's
    /// asked once. `None` anywhere else.
    pub(super) fn phone_online_help(&mut self, said: &str) -> Option<String> {
        if !crate::phonemode::on() {
            return None;
        }
        let yes = crate::phonemode::online_answer(said)?;
        crate::phonemode::set_online_ok(yes);
        let kept = self.store.save(crate::phonemode::ONLINE_ASKED, &yes).is_ok();
        let mut out = if yes {
            "Done: until this phone has a model of its own, your questions go to the free online models. Ask me again.".to_string()
        } else {
            "Done: nothing goes to the online models. Say \"get your own model\" and I'll fetch one onto this phone.".to_string()
        };
        if !kept {
            out.push_str(" I couldn't keep that choice, so I'll ask again next time Atlas starts.");
        }
        Some(out)
    }

    /// "Why did Nvidia move today?" -- a research question with today's date
    /// on it, so the answer is about today's move and not a year-old one
    /// (the general market desk: general trading knowledge only).
    pub(super) fn why_moved_help(&mut self, said: &str, t: u64) -> Option<String> {
        let what = crate::tradeday::why_it_moved(said)?;
        let off = crate::localclock::offset_secs();
        let (y, m, d) = crate::civil::civil_from_days(((t as i64) + off).div_euclid(86_400));
        let month = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"][(m - 1) as usize];
        Some(self.research(&format!("why {what} moved on {d} {month} {y}: the news and figures behind the move")))
    }

    /// The last eight days of the work log, reduced for `daily::one_thing_noticed`.
    pub(super) fn noticed_days(&self, t: u64) -> Vec<crate::daily::DayLog> {
        let off = crate::localclock::offset_secs();
        let today = crate::localclock::midnight(t, off);
        (0..8u64)
            .rev()
            .map(|back| {
                let from = today.saturating_sub(back * 86_400);
                let to = if back == 0 { t } else { from + 86_400 };
                let s = crate::worklog::summarise(&self.worklog.between(from, to));
                let small = self.worklog.between(from, (from + 4 * 3600).min(to)).iter().map(|x| x.secs()).sum();
                crate::daily::DayLog { active_secs: s.active, by_category: s.by_category, focus_blocks: s.blocks.len(), small_hours_secs: small }
            })
            .collect()
    }

    /// "What have you noticed?" -- the one observation, asked for.
    pub(super) fn noticed_help(&mut self, said: &str, t: u64) -> Option<String> {
        let low = said.trim().trim_end_matches(['?', '.', '!']).to_ascii_lowercase();
        if !["what have you noticed", "noticed anything", "what did you notice", "notice anything", "anything you noticed"]
            .iter()
            .any(|p| low.ends_with(p))
        {
            return None;
        }
        Some(crate::daily::one_thing_noticed(&self.noticed_days(t)).unwrap_or_else(|| {
            "Nothing that stands out yet -- I need a week of your work log to compare against.".into()
        }))
    }

    /// "Wrap up my day", "how did my week go".
    pub(super) fn wrapup_help(&mut self, said: &str, t: u64) -> Option<String> {
        let asked = crate::daily::wrap_asked(said)?;
        let w = self.wrap_now(t, asked == crate::daily::WrapAsked::Week);
        Some(crate::daily::wrap_said(&w))
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
    pub(super) fn upkeep_questions(&self, now: u64) -> Vec<String> {
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
    /// Synchronous compatibility entry for callers outside the running loop.
    /// The live loop uses request_brief/poll_brief and never waits for reads.
    pub fn brief_now(&mut self, now: u64) -> crate::brief::Brief {
        match self.prepare_brief_inline(now) {
            Ok(brief) => brief,
            Err(why) => {
                self.log.warn(&why);
                crate::brief::run(&[], &[], &crate::brief::BriefConfig::default())
            }
        }
    }
    /// Prepared text is not proof that speech finished. A canceled attempt
    /// keeps its scheduling cooldown while the receipt remains unchanged.
    pub(crate) fn acknowledge_brief_delivery(&mut self, delivered: &str, now: u64) {
        let Some((line, _)) = self.pending_brief_delivery.as_ref() else { return };
        let compact = |s: &str| s.split_whitespace().collect::<Vec<_>>().join(" ");
        if !compact(delivered).contains(&compact(line)) { return; }
        if !self.commit_brief_receipt(now) { return; }
        let Some((_, prepared_at)) = self.pending_brief_delivery.as_ref() else { return };
        self.last_brief = *prepared_at;
        self.last_brief_at = now;
        self.pending_brief_delivery = None;
        let _ = self.store.save("last_brief_at", &self.last_brief_at);
        crate::heard!(self.store.save("last_brief_source_at", &self.last_brief));
        let _ = self.store.save("brief_prepared", &self.pending_brief_delivery);
    }
}
