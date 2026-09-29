//! The background tick and what hangs off it: readings, `tick` itself, saving
//! after it, watching the index, the wire and sync to your other devices, and
//! the overnight run.
//! 
//! Moved out of `daemon.rs` unchanged on 29 Sep 2026 (docs/refactor-plan-daemon-split.md).

use super::*;

impl<'a> Daemon<'a> {
    pub(super) fn readings(&self) -> Readings {
        crate::health::read_machine()
    }

    pub(super) fn current_work(&self) -> Option<String> {
        // Crew work counts, and so do the windows being worked for you —
        // both are things "Paused. … on hold." should name. (This counted
        // only scheduled jobs that were due, so a render half an hour into
        // running reported as nothing.)
        let n = self.scheduler.due(crate::store::now()).len()
            + self.crew.errands().len()
            + self.working_for_you.iter().filter(|w| !w.held).count();
        match n {
            0 => None,
            1 => Some("1 job".into()),
            n => Some(format!("{n} jobs")),
        }
    }

    // ---------- background ----------

    /// Work Atlas does without being asked. Returns anything worth saying.
    pub fn tick(&mut self, t: u64) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(line) = self.model_warmed.lock().ok().and_then(|mut w| w.take()) {
            self.log.info(&line);
        }
        // The Talk page's waiting words, one turn each (`talk_queue`). A
        // turn that needs the model hands the call to a worker and is
        // finished on a later tick, so the loop -- and the hub -- never waits
        // on the model.
        self.talk_queue_turns(t);
        // A question left standing is stamped here, so its age is known.
        self.expire_stale_question(t);

        // THE HEARTBEAT, FIRST, before anything that can return early.
        //
        // It says one thing -- "this process is alive and holding the lock" --
        // and that is true on every tick regardless of mode, pause state, or
        // whether Atlas has anything to say. It has no business being
        // conditional on any of them.
        //
        // It used to sit 440 lines further down, behind two `return out`s: the
        // pause check and `if !self.modes.may_interrupt(false)`. Both are
        // reachable from shipped state -- `modes::suggested()` ships a focus
        // mode with `Interruptions::Urgent` and an on-a-call mode with
        // `Interruptions::Silent`, and `may_interrupt(false)` is false for
        // both. So:
        //
        //   say "focus mode", or pause Atlas, and the heartbeat stopped while
        //   Atlas was still running. `GONE_AFTER_SECS` is 150. After two and a
        //   half minutes the lock read `Abandoned`, and the next launch -- the
        //   shortcut, ATLAS.bat, a logon task -- took it and ran a SECOND
        //   daemon beside the live one. Both then load `data/state`, change it
        //   in memory, and write the whole thing back, so the second write
        //   erases whatever the first learned.
        //
        // `onlyone.rs`'s own module doc describes that outcome exactly: "The
        // damage is quiet... You notice weeks later that something you told it
        // didn't stick." The guard was built and then placed where the two
        // commonest quiet states switched it off. Found 17 Sep 2026 by reading
        // every `return` between the top of this function and the beat.
        // The heartbeat, and whether it landed.
        //
        // A beat that silently fails to write -- a full disk, a file a sync
        // client has locked -- lets the lock age past GONE_AFTER_SECS while
        // Atlas is running perfectly well. The next start then reads
        // `Abandoned`, takes the lock, and two instances write the same state
        // folder from their own memory. The holder used to have no way to
        // know: the write's result was discarded.
        //
        // Counted rather than reported on the first failure, because one
        // missed write is a blip and the staleness window is five beats wide.
        // Three in a row is a pattern with two beats of margin left.
        if crate::onlyone::OnlyOne::at(&self.store.data_dir()).beat(t) {
            self.missed_beats = 0;
        } else {
            self.missed_beats = self.missed_beats.saturating_add(1);
            const ENOUGH: u32 = 3;
            if self.missed_beats == ENOUGH {
                out.push(format!(
                    "I can't refresh my own instance lock at {}. If that keeps up, \
                     another Atlas started in the next couple of minutes will think \
                     this one is gone, take over, and the two of us will write over \
                     each other's memory. Worth checking the disk isn't full and that \
                     nothing has the file open.",
                    crate::onlyone::OnlyOne::at(&self.store.data_dir()).path().display()
                ));
            }
        }

        if let Some(w) = self.journal_warning.take() {
            self.log.warn(&w);
            out.push(w);
        }

        // Locked, or the screens off, is not asleep (H13a): the work carries
        // on, and the change is logged rather than treated as a night.
        self.note_running_state();
        // Mishearing you often enough to say so (H5), once.
        if let Some(line) = self.heard_note.take() {
            out.push(line);
        }

        // What a restart cut off: windows being worked are picked back up,
        // work that only makes an answer is redone once, and the rest is
        // named (`resume`).
        out.extend(self.pick_up_after_restart(t));
        self.keep_window_jobs();

        // Your settings, if you changed one while Atlas was running. Before
        // the pause check, like the heartbeat: switching something off is
        // exactly what you might do while Atlas is paused. Logged, not
        // spoken — whoever changed it has just seen the change.
        let _ = self.pick_up_settings();

        // Calls: notice one starting or ending, and hand back finished notes.
        out.extend(self.call_notes_tick(t));
        // A window being worked for you.
        // (Windows being worked for you go further down, once Atlas knows
        // whether you're at the keyboard.)

        // Heavyweight helpers that have gone idle. These are real kills, not
        // a walk over an empty list.
        //
        // Also moved up here, and for the same reason as the heartbeat: this
        // is housekeeping, and it was sitting below the pause check. So
        // pausing Atlas -- the thing you do to make it get out of the way --
        // pinned the model server's memory indefinitely, because the one
        // thing that brings an idle helper down never ran while paused. On a
        // laptop that is multiple gigabytes held precisely when you asked
        // Atlas to stand down.
        //
        // Reaped rather than force-stopped, deliberately. `Helpers::stop_all`
        // would take whisper down too, and "paused means only listening" --
        // the listening has to keep working. Reaping respects each helper's
        // own idle timeout, so what goes away is what was not being used.
        // A helper that died on its own comes off the books, so the next
        // time it is needed it is started again rather than trusted.
        for (name, how) in self.helpers.died() {
            self.log.warn(&format!("{name} stopped on its own ({how}); it will be started again when next needed"));
            if name == "model-server" {
                self.model_server_died();
            }
        }
        for said in self.helpers.reap(t) {
            self.log.info(&said);
        }

        // Answer a peer dialing us for a direct same-network sync, every beat —
        // so a phone coming onto the wifi is caught up at once, not only when
        // this device happens to run its own sync pass. Passive receive, so it
        // runs regardless of pause, like the housekeeping above.
        out.extend(self.serve_direct_sync(t));

        // Meaning vectors for notes that don't have one yet, a couple per
        // tick. Housekeeping, so it sits with the other housekeeping — but
        // unlike the heartbeat it spawns a process, and paused means Atlas
        // gets out of the way, so it respects the pause the way the reap
        // above deliberately does not.
        if !self.attention.is_paused() {
            self.embed_backlog();
        }

        // The exit statuses of the processes nobody waits for -- a panel, an
        // app the person opened, the headless browser. Never blocks. On unix,
        // not collecting them leaves one zombie per panel in the process
        // table for the life of the daemon, and a daemon runs for days. Here
        // with the other housekeeping, above the early returns, because a
        // paused Atlas still opens panels. See `unwaited`.
        crate::unwaited::reap();

        // "I can't write things down" is the one piece of housekeeping news
        // that has to reach you rather than the log, and it goes here for the
        // same reason as the two above: it is true regardless of mode, and a
        // paused or focused Atlas that cannot save is still an Atlas that is
        // losing everything you tell it.
        if let Some(trouble) = self.persist_trouble() {
            out.push(trouble);
        }

        // Re-lock the vault when it has been open too long. Above the pause
        // check, and among the security housekeeping, for the same reason as
        // the heartbeat: a vault left open is a vault open whether Atlas is
        // paused, focused, or idle, and `lock_after_mins` (15 by default) is a
        // promise that a walk-away does not leave every secret readable.
        //
        // Until this call the vault was locked in exactly one place --
        // `take_it_back`, immediately after proving identity -- so an ordinary
        // `atlas vault` unlock stayed open for the life of the process. The
        // idle rule the config already carried was honoured by nothing.
        //
        // `screen_locked` is `false` because no platform reading for it exists
        // in this tree yet: unknown reads as "not observed", which degrades to
        // the idle timeout rather than inventing a lock event. When the screen
        // state is plumbed through, it passes here and the second clause of
        // `should_lock` starts firing on its own.
        let vault_cfg = self.tools_cfg().vault.clone();
        if self.vault.should_lock(t, false, &vault_cfg) {
            self.vault.lock();
            self.log.info("Re-locked the vault after it sat open past its idle limit.");
        }

        let signals = self.observe(t);
        self.note_work(&signals, t);

        // Windows being worked for you. Typing waits for a gap in yours
        // (`lanes`): reading and deciding go on meanwhile.
        {
            let lanes = self.lane_cfg();
            let busy = signals.in_conversation || signals.idle_secs < lanes.gap_secs;
            out.extend(self.work_for_you(t, busy));
        }

        // Dictation that nobody is feeding stops itself.
        //
        // Above the pause check on purpose: a microphone that quietly stayed
        // in typing mode because you paused Atlas and walked off is the same
        // failure as a camera left watching an empty room, and `idle_stop_secs`
        // exists precisely so it cannot happen. `idle_check` only fires once,
        // since it flips the state that lets it fire.
        let dcfg = self.dictate_cfg();
        if let Some(d) = self.dictation.as_mut() {
            if d.idle_check(&dcfg, t) {
                self.dictation = None;
                out.push("Stopped dictating — nothing said for a while.".into());
            }
        }

        // Anything written to somebody else, tried again.
        //
        // Above the pause check: pausing Atlas stops it *speaking to you* and
        // stops it starting work, and a message you already wrote and asked
        // to be sent is neither. It is a thing already promised.
        //
        // Silent when nothing changed -- `Round::spoken` returns empty on a
        // round where everybody was simply offline, which is the ordinary
        // case and is not news.
        {
            // Built fresh each tick from the pairings and rooms as they are
            // now, so a peer paired or a room opened a moment ago is already
            // reachable. It owns its snapshot, holding no borrow on `chats`
            // while the courier mutates it.
            // Introductions and group lists first, so a message to a group
            // goes to the people its latest list says are in it.
            self.peer_upkeep(t);
            self.settle_owned_groups();
            if let Some(said) = self.post_release_notices(t) {
                out.push(said);
            }
            let link = {
                let pairings = crate::kin::Pairings::load(&self.peer_dir);
                self.peer_link(&pairings)
            };
            let round = crate::courier::run(
                &mut self.chats,
                &link,
                &mut self.tries,
                &crate::courier::Attempts::default(),
                t,
            );
            if !round.nothing_happened() {
                let _ = self.chats.save(&self.store);
                out.push(round.spoken());
            }

            // Read receipts owed to the people whose messages we've read,
            // sent on the same link and in the same pass as delivery. Silent:
            // a receipt is not news to the person who read the message, only
            // to the person who sent it, and this end is the reader. Saved
            // only when a receipt actually advanced a room's mark, so a
            // failed pass leaves the state to be retried next tick.
            if crate::courier::send_receipts(&mut self.chats, &link, t) > 0 {
                let _ = self.chats.save(&self.store);
            }
        }

        // Paused means paused: no jobs, no posts, no offers. Only listening.
        if self.attention.is_paused() {
            return out;
        }

        // Scheduled jobs.
        for id in self.scheduler.due(t) {
            let Some(job) = self.scheduler.jobs.iter().find(|j| j.id == id).cloned() else { continue };
            let intent = self.parser.parse(&job.command);
            let call = crate::policy::classify_with_policy(&intent, &self.memory, &self.cfg.policy);

            // A scheduled job you never individually approved must not run
            // itself just because a timer fired. Approving the job unlocks it
            // permanently; until then it waits, supervised or not.
            if call.needs_consent() && !job.approved {
                self.scheduler.park_for_approval(id);
                if self.autonomy == Autonomy::Supervised {
                    let q = format!("Scheduled: {}. Go ahead?", job.command);
                    self.session.await_approval(intent.clone(), &q);
                    self.pending_job = Some(id);
                    out.push(q);
                }
                continue;
            }
            let result = self.execute(&intent);
            let ok = !result.starts_with("error");
            self.journal.record_at(Act::Scheduled, &job.command, ok, t);
            self.scheduler.complete(id, t, &result, ok);
            if call != Decision::AutoProceed || !ok {
                out.push(result);
            }
        }

        // --- Add-on sequences that run by themselves ---
        //
        // Approving the add-on is the consent for these, the rule scheduled
        // jobs already follow; each step is still checked as it runs, and a
        // step that asks still asks. One at a time, and never over a sequence
        // already in hand -- a due one waits for the next tick rather than
        // being dropped.
        if self.current_flow.is_none() {
            let reg = crate::plugins::Registry::load_kept(&mut self.plugins_kept, &self.plugins_dir, &self.cfg.commands, &self.store).clone();
            let mut runs = crate::plugins::ScheduleRuns::load(&self.store);
            let before = runs.clone();
            if let Some((id, f, key)) = reg.due(&mut runs, t, local_offset_mins()).into_iter().next() {
                runs.ran(&key, t);
                let said = self.start_flow(f, Some(id), None, t);
                out.push(said);
            }
            if runs != before {
                let _ = runs.save(&self.store);
            }
        }

        // --- Scheduled posts and emails ---
        //
        // The authorization question, stated plainly: **approving a post at
        // schedule time IS the consent to send it at its time.** That is the
        // entire point of scheduling; requiring you to be present at 7am
        // would make the feature pointless.
        //
        // What that consent covers is narrow, and re-verified here: this exact
        // text, to this channel. Editing after approval voids it, the length
        // and media rules are checked again, and a failure never silently
        // retries forever — it lands on the outstanding list.
        // Never waited for on the loop (`status_now`). Until the first
        // answer is in (`Unknown`, a moment after start) nothing here is
        // decided: no post is sent, and the board isn't told the connection
        // failed.
        let reach = self.connectivity.status_now(t);
        let known = reach != Reach::Unknown;
        let online = reach == Reach::Online;
        // The probe already happened; this just writes down what it saw. Left
        // unrecorded, the board could never learn the one thing it is best
        // placed to notice — that an answer was produced with no network.
        if let Some(i) = self.connections.get_mut(crate::integrations::INTERNET).filter(|_| known) {
            if online {
                i.worked(t);
            } else {
                i.failed(t, "no route out");
            }
        }
        let due = if known { self.publisher.due(t, online) } else { Vec::new() };
        for id in due {
            match delivery::plan(&self.publisher, &self.browser_cfg(), id, online) {
                // Sent through Atlas's browser now (G2). The comment that
                // stood here recorded why it used to be held instead: nothing
                // could send, and a post left `Scheduled` came round every tick.
                // A post in flight is skipped until its errand comes back.
                Ok(_) => {
                    let _ = self.send_post(id, t, online);
                }
                Err(delivery::Outcome::Retry(_)) => {}
                Err(delivery::Outcome::Blocked(why)) => {
                    let what = self.publisher.get(id).map(|p| p.describe()).unwrap_or_default();
                    self.backlog.record(&format!("send {what}"), Blocker::Failed(why.clone()), t);
                    self.journal.record_at(Act::Blocked, &format!("{what}: {why}"), false, t);
                    out.push(format!("Couldn't send: {why}"));
                }
                Err(delivery::Outcome::Sent(m)) => out.push(m),
            }
        }
        // Problems you would want to know about before the send time arrives.
        // (`blocked` never counts "no connection", so it needs no answer.)
        for (id, why) in self.publisher.blocked(t, online) {
            if self.warned_posts.contains(&id) {
                continue;
            }
            self.warned_posts.push(id);
            out.push(format!("Heads up — a scheduled post can't go: {why}"));
        }

        // --- Run what's queued ---
        //
        // Background work goes immediately; anything needing the screen waits
        // for a gap. This is what makes "Atlas is busy" stop meaning "you are
        // waiting".
        let lanes = self.lane_cfg();
        for id in self.queue.ready_with(&signals, &lanes, t, self.connectivity.cached()) {
            let Some(task) = self.queue.tasks.iter().find(|x| x.id == id).cloned() else { continue };
            let intent = self.parser.parse(&task.command);
            let call = crate::policy::classify_with_policy(&intent, &self.memory, &self.cfg.policy);

            // Queued work never sneaks past the approval gate.
            if call.needs_consent() {
                self.queue.finish(id, "needs your go-ahead", false);
                self.backlog.record(&task.command, Blocker::NeedsApproval, t);
                continue;
            }
            let result = self.execute(&intent);
            let ok = !result.starts_with("error");
            self.queue.finish(id, &result, ok);
            self.journal.record_at(Act::Scheduled, &task.command, ok, t);
            if !ok || call == Decision::ProceedAndReport {
                out.push(result);
            }
        }

        // A workflow mid-run keeps moving between turns. One paused for
        // your yes stays paused — the ask already happened, and asking
        // every two seconds is the scheduled-post bug this file already
        // fixed once.
        if self
            .current_flow
            .as_ref()
            .is_some_and(|r| r.state == crate::flow::RunState::Running)
        {
            if let Some(line) = self.drive_flow(t) {
                out.push(line);
            }
        }

        // --- Things Atlas couldn't do earlier ---
        let conditions = Conditions {
            online,
            screen_free: signals.idle_secs > 20,
            you_are_here: !self.attention.is_paused(),
            tools: Vec::new(),
        };
        let backlog_cfg = crate::backlog::BacklogConfig::default();
        if let Some(item) = self.backlog.next_offer(&conditions, &backlog_cfg, t) {
            let q = crate::backlog::Backlog::phrase(&item);
            self.session.ask(&q);
            // Remember which item this yes/no is about, so the answer can run
            // it or, on a no, dismiss it -- otherwise "no" only clears the
            // question and the item is raised again next quiet tick.
            self.pending_backlog = Some(item.id);
            out.push(q);
        }

        // --- The machine itself ---
        //
        // A notice waits for a quiet moment; something urgent doesn't. Either
        // way one thing at a time, and not again for a week.
        let hcfg = self.health_cfg();
        let findings = assess_machine(&self.readings(), &hcfg);
        self.health.reconcile(&findings);
        let quiet = signals.idle_secs > 60 && !self.session.is_waiting();
        if let Some(f) = self.health.next(&findings, quiet, &hcfg, t) {
            if self.modes.may_interrupt(f.severity == crate::health::Severity::Urgent) {
                // Routed rather than only spoken. A disk warning is the
                // clearest case for this: it matters most exactly when you are
                // not at the desk to hear it.
                let urgency = match f.severity {
                    crate::health::Severity::Urgent => crate::notify::Urgency::Urgent,
                    _ => crate::notify::Urgency::Routine,
                };
                let note = crate::notify::Note::new("Atlas — your machine", &f.say, urgency, t);
                let sent = self.reach_you(note, t);
                if sent.reached_you() {
                    out.push(f.say.clone());
                }
                // Recorded as offered only when it actually went somewhere a
                // person could see. Journalling a held note as delivered is
                // how "I told you" becomes untrue.
                self.journal.record_at(Act::Offered, &f.say, sent.reached_you(), t);
                if let crate::notify::Sent::Failed(why) = &sent {
                    self.log.warn(&format!("could not notify: {why}"));
                }
            }
        }

        // --- Other machines, watched from the outside ---
        let names: Vec<String> = self.watcher.targets.iter().map(|x| x.name.clone()).collect();
        for name in names {
            if !self.watcher.due(&name, t) {
                continue;
            }
            let addr = self
                .watcher
                .targets
                .iter()
                .find(|x| x.name == name)
                .map(|x| x.address.clone())
                .unwrap_or_default();
            let up = crate::watch::reachable(&addr, 1500);
            match self.watcher.observe(&name, up, t) {
                crate::watch::Alert::None => {}
                crate::watch::Alert::WentDown { say, .. }
                | crate::watch::Alert::CameBack { say, .. }
                | crate::watch::Alert::Flapping { say, .. } => {
                    self.journal.record_at(Act::Offered, &say, true, t);
                    out.push(say);
                }
            }
        }

        // --- Re-plan when the machine changes (`fit.replan_on_change`) ---
        //
        // Hourly. The plan made on the day Chrome had forty tabs open is not
        // the one to keep forever; `fit::worth_replanning` is what decides.
        let fit_cfg = self.tools_cfg().fit.clone();
        if fit_cfg.replan_on_change && t.saturating_sub(self.fit_measured.1) >= 3600 {
            let now_m = crate::fit::measure();
            if crate::fit::worth_replanning(&self.fit_measured.0, &now_m) {
                let before = self.fit.tier;
                self.fit = crate::fit::plan_as_set(&now_m, &fit_cfg);
                self.log.info(&format!("re-planned for this machine: {} -> {}", before.name(), self.fit.tier.name()));
                self.fit_measured.0 = now_m;
            }
            self.fit_measured.1 = t;
        }

        // --- A new Atlas dropped into updates/ ---
        //
        // Checked a few times a day, said once per version: it goes in the
        // next time Atlas starts (`upgrade::swap_checked`, in main).
        if t.saturating_sub(self.last_update_check.0) >= 6 * 3600 {
            self.last_update_check.0 = t;
            match crate::upgrade::waiting(&self.store.install_root()) {
                Some(Ok((_, v))) if self.last_update_check.1 != v => {
                    let say = format!("Atlas {v} is waiting in the updates folder. It goes in the next time I start; the one running now is kept so you can go back.");
                    self.log.info(&say);
                    out.push(say);
                    self.last_update_check.1 = v;
                }
                Some(Err(why)) if self.last_update_check.1 != why => {
                    self.log.warn(&format!("updates folder: {why}"));
                    out.push(format!("There's something in the updates folder I won't use: {why}."));
                    self.last_update_check.1 = why;
                }
                _ => {}
            }
        }

        // --- The waking panel leaves by itself ---
        if let Some(p) = self.wants_panel {
            if crate::panel::faded(p, self.panel_shown_at, t, &self.panel_cfg()) {
                self.wants_panel = None;
            }
        }

        // --- Is the sync folder still syncing? ---
        //
        // Every `cloud.check_every_hours`. It was checked once, at setup; a
        // client that signed out in March looked fine until someone noticed
        // their phone hadn't seen anything since. Said once per finding, and
        // `Trouble::fix` says what to do about it.
        let sync_cfg = self.tools_cfg().sync.clone();
        let cloud_cfg = self.tools_cfg().cloud.clone();
        let every = cloud_cfg.check_every_hours.max(1) as u64 * 3600;
        if sync_cfg.enabled && !sync_cfg.folder.trim().is_empty() && t.saturating_sub(self.last_sync_check) >= every {
            self.last_sync_check = t;
            let mine = format!("{}.bundle", sanitise(&self.synclog.device));
            let others = !self.seen_up_to.is_empty();
            if let Some((trouble, what)) = crate::cloudsync::still_syncing(std::path::Path::new(sync_cfg.folder.trim()), &mine, others, t, &cloud_cfg) {
                let fix = trouble.fix();
                let say = format!("Your sync folder: {what}. {}{}.", fix[..1].to_uppercase(), &fix[1..]);
                self.log.warn(&say);
                out.push(say);
            }
        }

        // Shared-page edits made from the command line (`atlas doc`)
        // wait in an inbox; this is where they join the sync log, which
        // has one writer — here. The files go only once the log holding
        // them is on disk.
        let inbox = self.store.data_dir().join("doc-inbox");
        let taken = crate::yata::take_queued(&inbox, &mut self.synclog, t);
        if !taken.is_empty() && self.store.save("synclog", &Some(self.synclog.clone())).is_ok() {
            for f in taken {
                let _ = std::fs::remove_file(f);
            }
        }

        // --- Your standing watches ---
        //
        // Readings the tick already takes, offered as named things a rule can
        // watch. The rules come from settings and are re-read each tick, so
        // editing one takes effect without a restart; what they remember
        // (pending "for" timers, last firing) is kept apart and survives one.
        let specs = self.tools_cfg().automations.clone();
        if !specs.is_empty() {
            let rules: Vec<crate::automation::Automation> =
                specs.iter().filter_map(|s| crate::automation::Automation::from_spec(s).ok()).collect();
            self.automations.rules = rules;
            let r = self.readings();
            let mut seen: Vec<(String, String)> = vec![
                ("machine.disk_free_gb".into(), format!("{:.1}", r.disk_free_gb)),
                ("machine.ram_used_gb".into(), format!("{:.1}", r.ram_used_gb)),
            ];
            if r.disk_total_gb > 0.0 {
                seen.push(("machine.disk_used_pct".into(), format!("{:.1}", 100.0 * (1.0 - r.disk_free_gb / r.disk_total_gb))));
            }
            if r.ram_total_gb > 0.0 {
                seen.push(("machine.ram_used_pct".into(), format!("{:.1}", 100.0 * r.ram_used_gb / r.ram_total_gb)));
            }
            if let Some(b) = r.battery_percent {
                seen.push(("machine.battery_pct".into(), b.to_string()));
            }
            if let Some(d) = r.days_since_backup {
                seen.push(("machine.days_since_backup".into(), d.to_string()));
            }
            for (name, st) in &self.watcher.status {
                let word = match st.health {
                    crate::watch::Health::Up => "up",
                    crate::watch::Health::Down => "down",
                    crate::watch::Health::Flapping => "flapping",
                    crate::watch::Health::Unknown => "unknown",
                };
                seen.push((format!("watch.{name}"), word.to_string()));
            }
            // "at 7" in a watch means 7 on your clock, and the offset moves
            // twice a year, so it is taken fresh each tick.
            self.automations.utc_offset = self.home_zone().offset_at(t as i64);
            let before = (self.automations.mem.pending.clone(), self.automations.mem.latched.clone());
            let mut fired = Vec::new();
            for (entity, value) in &seen {
                fired.extend(self.automations.observe(entity, value, t as i64));
            }
            fired.extend(self.automations.tick(t as i64));
            for f in &fired {
                let say = format!("{} ({})", f.action, f.because);
                self.journal.record_at(Act::Offered, &say, true, t);
                out.push(say);
            }
            // Saved when something that must survive a restart moved — a
            // timer started or cleared, a watch fired — not every tick.
            let after = (&self.automations.mem.pending, &self.automations.mem.latched);
            if !fired.is_empty() || (&before.0, &before.1) != after {
                let _ = self.store.save("automations", &self.automations.mem);
            }
        }

        // --- Work prepared before you ask for it ---
        let moment = Moment {
            new_files: signals.recent_changes.added.clone(),
            returned: false,
            idle_secs: signals.idle_secs,
            last_said: self.thread.last().map(|e| e.said.clone()).unwrap_or_default(),
            // Never prepare anything while you're mid-task; anticipation that
            // makes the laptop stutter is worse than none.
            can_afford_work: signals.idle_secs > 30 && !signals.in_conversation,
            ..Default::default()
        };
        for rule in self.anticipator.due(&moment, t) {
            let lane = lane_for(&rule.command);
            // A task that cannot run without a connection says so at the
            // moment it is queued. `push_online` and the offline hold in
            // `ready_with` existed for this and were unreachable: every
            // task ever queued had `needs_net: false`, so the connectivity
            // argument the tick passes in could never change the answer.
            if command_needs_connection(&rule.command) {
                self.queue.push_online(&rule.command, lane);
            } else {
                self.queue.push(&rule.command, lane);
            }
            if rule.announce && self.modes.may_interrupt(false) {
                out.push(crate::anticipate::ready_line(&rule));
            }
            self.journal.record_at(Act::Offered, &rule.name, true, t);
        }

        // --- Round 11's tools: key chords, clipboard history, feeds read in
        // the background, meeting prep and the trading check-in. Each costs
        // nothing unless it's switched on and has something to do.
        let may_speak = self.proactive.may_interrupt(&signals, t);
        let paused = self.attention.is_paused();
        let online = self.connectivity.cached() == Reach::Online;
        out.extend(self.workday_tick(t, may_speak, paused, online));

        // --- Calendar reminders coming due ---
        // An event with a reminder lead-time gets spoken once as its start
        // comes within that lead. Repeating events expand, so a daily standup
        // reminds each day; the `reminded` set (id, occurrence start) keeps any
        // one occurrence from firing twice.
        self.reminded.retain(|(_, start)| *start > t);
        for occ in self.calendar.due_reminders(t) {
            let key = (occ.id, occ.start);
            if self.reminded.contains(&key) {
                continue;
            }
            let mins_away = occ.start.saturating_sub(t) / 60;
            let when = if mins_away >= 60 {
                format!("in {} hour{}", mins_away / 60, if mins_away / 60 == 1 { "" } else { "s" })
            } else if mins_away <= 1 {
                "in a moment".to_string()
            } else {
                format!("in {mins_away} minutes")
            };
            out.push(format!("Reminder: \"{}\" {when}.", occ.title));
            self.reminded.insert(key);
            self.journal.record_at(Act::Offered, &format!("reminder: {}", occ.title), true, t);
        }

        // Anything hand tracking has said since the last tick. It runs on its
        // own thread precisely so the pointer does not wait for this — only
        // the reporting does.
        for said in self.hands.as_ref().map(|h| h.heard()).unwrap_or_default() {
            match said {
                crate::handloop::Said::Did(what) => out.push(what),
                crate::handloop::Said::Trouble(why) => out.push(why),
                crate::handloop::Said::HandsGone => {
                    self.steering_until = None;
                    self.carrying = None;
                }
            }
        }

        // Look at the room. Only a gesture answering a pending question ever
        // reaches you from this; presence just informs everything else.
        if let Some(said) = self.look_at_the_room(t) {
            out.push(said);
        }

        // Anything you handed over from another device. Read one per tick;
        // said only when Atlas may speak at all, so a tray filling up while
        // you are in a meeting does not become a queue of interruptions.
        if let Some(said) = self.read_one_handed_thing() {
            if self.proactive.may_interrupt(&signals, t) {
                out.push(said);
            }
        }

        // --- Backup, on its own schedule ---
        //
        // Copying everything in the store can genuinely take a while on a
        // real vault, and this fires unprompted on a timer — nobody is
        // waiting on it the way they are for a spoken command, which makes
        // it the worst possible thing to block the tick. Handed to the
        // crew; `take_crew_news` reports it when it actually finishes.
        let bcfg = self.backup_cfg();
        if due_for_backup(&bcfg, t) && t.saturating_sub(self.last_backup) > 3600 {
            let root = self.store.root().to_path_buf();
            let cfg_for_errand = bcfg.clone();
            let work: crew::Work = Box::new(move |_stop| match back_up(&root, &cfg_for_errand, crate::store::now()) {
                Ok(b) => {
                    prune_backups(&cfg_for_errand);
                    Ok(format!("backed up {} files", b.files.unwrap_or(0)))
                }
                Err(e) => Err(e.to_string()),
            });
            // Only mark the schedule satisfied if the crew actually took
            // it. The bounded waiting list refusing is vanishingly
            // unlikely for one periodic job, but if it happens, leaving
            // `last_backup` alone means the next tick simply tries again
            // rather than the backup silently never running.
            if self.hand_off("backup", t, work, None, SpeakPolicy::ViaWatcher) {
                self.last_backup = t;
            }
        }

        // --- Housekeeping: hourly, cheap, never in your way ---
        // The night's work.
        //
        // `overnight.rs` is a complete state machine -- the window, the
        // give-up-after-N-failures rule, the per-night problem cap, the
        // budget check, the morning brief -- and until now nothing called any
        // of it. The settings were in `tools.yaml` and read by nothing, which
        // is the config-that-lies shape this tree keeps finding.
        //
        // What runs here is the `ask_you_later` brain, which is the shipped
        // default and the only one that is free and needs no network: Atlas
        // works through what it could not finish, writes each one up, and
        // leaves the brief for the morning. The `local`, `hosted` and
        // `delegate` brains drive a solver, cost money or drive another
        // application, and are deliberately still unwired -- that is a
        // decision, not an oversight, and it is named in OUTSTANDING_TASKS.
        //
        // Nothing is ever applied while you are asleep. That is not a setting
        // here: `OvernightConfig::apply_while_asleep` is `#[serde(skip)]` and
        // hard-wired false, and `the_night_never_applies_anything` holds it.
        self.run_the_night(t);

        // --- How you're working, when it's worth a word ---
        //
        // `Noticed` had four variants, each with a sentence and a rule about
        // repeating, and nothing in the tree ever built one. Three settings
        // were thresholds on it — `notice_patterns`,
        // `late_nights_before_saying`, `hours_before_saying` — and a fourth,
        // `quiet_days`, decided how often the same remark could be made. All
        // four were thresholds on something nobody produced.
        //
        // An observation and an offer, never a diagnosis.
        {
            let pcfg = self.tools_cfg().person.clone();
            let hour = crate::localclock::hour_here(t) as u32;
            // A quarter of an hour away is a break; a pause to read something
            // is not. Counting a pause would mean the stretch reset all day
            // and the threshold was never reached.
            const A_BREAK: u64 = 900;
            if signals.idle_secs >= A_BREAK {
                self.working_since = None;
            } else if self.working_since.is_none() {
                self.working_since = Some(t);
                // Recorded on the hour you actually started, which is what
                // decides whether tonight counts as a late one.
                self.person.working_now(hour, t, &self.tools_cfg().judgment);
            }
            let hours_straight = self
                .working_since
                .map(|from| (t.saturating_sub(from) / 3600) as u32)
                .unwrap_or(0);

            if self.proactive.may_interrupt(&signals, t) {
                if let Some(seen) = crate::person::noticing(&self.person, &pcfg, hours_straight, t)
                {
                    let mut said: crate::person::Said =
                        self.store.load(crate::person::SAID_RECORD);
                    if said.may_say(&seen, &pcfg, t) {
                        said.record(&seen, t);
                        if self.store.save(crate::person::SAID_RECORD, &said).is_ok() {
                            out.push(seen.spoken());
                        }
                    }
                }
            }
        }

        // --- Going somewhere your texts won't reach you ---
        //
        // `going_away.remind_days_before` is "remind you this many days
        // before a trip you've told it about" and there was no way to tell it
        // about a trip; `periodic_nudge` takes `days_since_last` and nothing
        // kept a last; `codes.check_days_before` was a threshold on the same
        // missing date. One record answers all three.
        //
        // Both halves are said, because either alone leaves a wrong
        // impression: "every account is reachable" is no comfort with no
        // codes printed, and "ten codes in hand" is no comfort for the
        // account that has none.
        if self.proactive.may_interrupt(&signals, t) {
            let acfg = self.tools_cfg().going_away.clone();
            let ccfg = self.tools_cfg().codes.clone();
            let mut away: crate::goingaway::Away =
                self.store.load(crate::goingaway::AWAY_RECORD);

            // At most once a day, both halves. A trip reminder that fires
            // every tick for a fortnight is one you stop reading, and then
            // stop reading on the fortnight it mattered.
            if away.days_since_asked(t) >= 1 {
                let mut said: Vec<String> = Vec::new();
                if let Some(line) = crate::goingaway::the_trip_is_close(
                    &away,
                    &self.accounts.accounts,
                    &acfg,
                    t,
                ) {
                    said.push(line);
                }
                if crate::codes::worth_raising_now(&ccfg, away.days_until(t)) {
                    let names: Vec<String> =
                        self.accounts.accounts.iter().map(|a| a.site.clone()).collect();
                    said.push(crate::codes::before_you_go(&self.code_sets, &names, &ccfg));
                }
                // No trip, and long enough since anyone asked. Dates change
                // and the preparation takes days, so this is the check that
                // catches a deployment you have not written down yet.
                if said.is_empty() {
                    if let Some(line) = crate::goingaway::periodic_nudge(
                        &self.accounts.accounts,
                        away.days_since_asked(t),
                        &acfg,
                    ) {
                        said.push(line);
                    }
                }
                if !said.is_empty() {
                    // Marked as asked before it is said, so a failure to
                    // write it down is a reminder you do not get rather than
                    // one you get every tick.
                    away.last_checked = t;
                    if self.store.save(crate::goingaway::AWAY_RECORD, &away).is_ok() {
                        out.push(said.join(" "));
                    }
                }
            }
        }

        // --- The envelope, checked on the interval you set ---
        //
        // `after_me.review_every_days` is the only part of that arrangement
        // that has to run on a clock — everything else is decided once, and
        // the failure mode is a plan that was true three years ago and names
        // somebody you have since fallen out with. Nothing read it, and the
        // whole `after_me:` section was in `config::PARSED_AND_NEVER_READ`.
        //
        // Only ever a nudge to go and look. Nothing about this arrangement is
        // acted on by Atlas, and it never holds the passphrase.
        if self.proactive.may_interrupt(&signals, t) {
            let acfg = self.tools_cfg().after_me.clone();
            let arrangement: crate::afterme::Arrangement =
                self.store.load(crate::afterme::RECORD);
            if let Some(said) = arrangement.nudge(&acfg, t) {
                // Marked as reviewed-asked rather than reviewed: saying it
                // once a year is the point, saying it every tick for a year
                // is how it gets ignored.
                let mut asked = arrangement.clone();
                asked.reviewed_at = t;
                if self.store.save(crate::afterme::RECORD, &asked).is_ok() {
                    out.push(said);
                }
            }
        }

        // --- Looking at itself, on the cadence you set ---
        //
        // `self_audit.every_days` and `self_audit.act_without_asking` were
        // both changeable and both inert: `recommend` was only ever reached
        // from `Intent::WorkOnYourself`, which runs because you asked. So
        // "how often to look" described a looking nothing did, and "act
        // without asking" described an asking that was the only way in.
        //
        // Gated on `may_interrupt` before the look rather than after it. The
        // clock is only marked when it actually looked, so a week spent in
        // meetings delays the check rather than silently spending it.
        if self.proactive.may_interrupt(&signals, t) {
            let acfg = self.tools_cfg().self_audit.clone();
            let last: crate::selfaudit::LastLook =
                self.store.load(crate::selfaudit::LOOK_RECORD);
            if crate::selfaudit::time_to_look(&acfg, last.at, t) {
                let _ = self.store.save(
                    crate::selfaudit::LOOK_RECORD,
                    &crate::selfaudit::LastLook { at: t },
                );
                self.refresh_signals();
                let recs = crate::selfaudit::recommend(&self.signals, acfg.most_at_once);
                // The hollow check, on the pass that happens without you
                // asking. It used to run only when you asked -- so the one
                // sweep nobody triggers was the one that skipped the
                // bug-detector, which is the shape `hollow.rs` was written
                // about, one level up.
                //
                // Raised on a judgment rather than a count. Six findings that
                // all say "not wired up" are a backlog; one that says
                // "claimed fine while every number was zero" is worth
                // tonight. `worth_raising` weighs both, including the
                // evidence *against* interrupting, and stays quiet when it
                // cannot tell -- which is the right way round for something
                // that speaks while you did not ask.
                let hollow_found = self.hollow_answers();
                let raise = crate::judgment::worth_raising(&hollow_found, &self.tools_cfg().judgment);
                if raise.settled() == Some(true) {
                    out.push(crate::hollow::spoken(&hollow_found, &self.tools_cfg().judgment));
                }
                if let Some(u) = crate::selfaudit::unprompted(&acfg, &recs) {
                    out.push(u.said);
                    // Only when told not to ask. `work_on_myself` opens the
                    // session and refuses on its own if the pipeline is off,
                    // which is the right place for that answer to live.
                    if let Some(goal) = u.goal {
                        // Its own diagnosis goes straight in (E2): the four
                        // answers are already known, so the work starts at
                        // proving the test fails. Landing still waits for
                        // your OK (`selfgrant`).
                        if let Some(thought) = u.thought {
                            let mut session = crate::selfwork::Session::new(&goal, 0);
                            for answer in [&thought.symptom, &thought.cause, &thought.where_, &thought.proof] {
                                let _ = session.diagnosing.answer(answer);
                            }
                            self.selfwork = Some(session);
                        }
                        let opened = self.work_on_myself(&goal);
                        out.push(opened);
                    }
                }
            }
        }

        if t.saturating_sub(self.last_tidy) >= 3600 {
            self.last_tidy = t;
            // Atlas's own things, fixed without asking (E1).
            let _ = self.fix_my_own_things(t);
            // Routines: asked about once, and run when due (E4).
            if self.proactive.may_interrupt(&signals, t) {
                out.extend(self.routines_on_the_hour(t));
                // A change to itself waiting on your OK (F8).
                if let Some(line) = self.remind_about_staged_change(t) {
                    out.push(line);
                }
            }
            // The hours you were active, kept across a restart.
            let _ = self.store.save("rhythm", &self.rhythm);
            // Changes waiting on you, checked against the code as it is now.
            // One hash per file it writes; said once when a change goes out
            // of date, not every hour after.
            let newly = self.workshop.mark_outdated();
            if !newly.is_empty() {
                let _ = self.workshop.save(&self.store);
                for (project, title, files) in newly {
                    out.push(format!(
                        "The \"{title}\" change for {project} is out of date — {} changed since I wrote it. \
                         Ask me to redo it against the current code before implementing it.",
                        files.join(", ")
                    ));
                }
            }
            // Your `retention.approvals_detailed`, not a literal that happened
            // to equal it. Hoisted rather than inlined because `tools_cfg`
            // borrows `&self` and `compact_approvals` needs `&mut`.
            let keep_detailed = self.tools_cfg().retention.approvals_detailed;
            self.memory.compact_approvals(keep_detailed);
            // The producer selfaudit::Kind::GotSlower never had. A signal
            // nothing can raise is a signal that will never fire.
            if let Some(sig) = self.timing.got_slower() {
                self.signals.push(sig);
            }
            // Anything Atlas worked out for itself and has not seen hold up
            // since is a guess that has been sitting long enough to look like
            // a fact. Surfaced, not deleted — you decide.
            for f in self.facts.guesses_worth_checking(t) {
                self.log.info(&format!(
                    "worth re-checking: {} ({})",
                    f.summary,
                    f.kind.plain()
                ));
            }
            self.backlog.tidy(&backlog_cfg, t);
            self.scheduler.prune();
            // A trash record that cannot be read stops `expire` and `take`
            // dead — both refuse rather than guess an id, which is right, and
            // both used to do it in silence. `LedgerState::trouble()` exists
            // to say which of the two it is and had no caller, so the trash
            // quietly stopped accepting anything and quietly stopped
            // expiring, for as long as the file stayed bad.
            if let Some(why) = self.trash.read_ledger().trouble() {
                self.log.info(&format!(
                    "{why} — nothing new can go to the trash and nothing in it is \
                     expiring until that file is readable again"
                ));
            }
            self.trash.expire(t);
            let _ = self.queue.save(&self.store);
            self.queue.prune();
            // Walking the whole data/ tree and deciding what to reclaim is
            // real filesystem I/O — on a machine that's accumulated a lot
            // of logs and backups, "hourly, cheap, never in your way" is a
            // promise the walk itself can break if it runs on the tick.
            // Self-contained (owns a path, a config, and nothing of the
            // daemon's), so it goes to the crew like the backup does.
            //
            // Resolved against the install's own root now, via
            // `Store::install_root` — retention means the whole `data/`
            // tree (state, backups, notes, logs), which is a sibling of
            // `store.root()` (`data/state`), not something nested under
            // it. Named as an open question in an earlier pass; the
            // concept it was waiting on now exists.
            let root_for_errand = self.store.install_root().join("data");
            // Your budget, not the built-in one.
            //
            // This built `RetentionConfig::default()` inside the errand while
            // the `retention:` section of your tools.yaml sat unread -- so a
            // person who set a 2 GB budget still got the shipped 500 MB, and
            // the pass that *deletes files to stay in budget* was the one
            // ignoring the number. `config::PARSED_AND_NEVER_READ` named it;
            // nothing acted on it.
            let retention_cfg = self.tools_cfg().retention.clone();
            let work: crew::Work = Box::new(move |_stop| {
                let items = crate::retention::survey(&root_for_errand);
                let plans = crate::retention::plan(&items, &retention_cfg, crate::store::now());
                let mut lines = Vec::new();
                // Anything the plan named outside data/ is a bug upstream.
                // Say so rather than quietly skipping it.
                for stray in crate::retention::out_of_bounds(&plans, &root_for_errand) {
                    lines.push(format!("refused to delete outside data/: {}", stray.display()));
                }
                let freed = crate::retention::apply(&plans, &root_for_errand);
                if freed > 0 {
                    lines.push(format!("reclaimed {} MB", freed / (1024 * 1024)));
                }
                // `retention::irreducible` had no caller: the case where notes
                // and learned state alone (never evicted for space, per
                // `plan`'s own rule) already exceed the budget, so no amount
                // of deletion here will fix it. That is a configuration
                // problem, not a cleanup problem, and it was previously
                // invisible — `apply` just quietly freed whatever it could
                // and stopped.
                let usage = crate::retention::usage(&items);
                if crate::retention::irreducible(&usage, &retention_cfg) {
                    lines.push(format!(
                        "notes and learned state alone are {} MB, over the {} MB budget — \
                         deleting scratch/logs/captures won't fix this",
                        (usage.notes + usage.state) / (1024 * 1024),
                        retention_cfg.total_budget_mb
                    ));
                }
                Ok(lines.join("; "))
            });
            self.hand_off("housekeeping", t, work, None, SpeakPolicy::ViaWatcher);
            // `daily::still_keep` had no caller at all: closed days were
            // computed fresh each rollover and handed straight to the brief
            // opener, with nothing archived to ever prune. Now that
            // `daily_history` (above, in `Intent::Outstanding`) actually
            // keeps them, this is the other half -- a real retention pass
            // over what accumulated, same hourly cadence as the rest of
            // this block.
            let daily_cfg = self.tools_cfg().daily.clone();
            let before = self.daily_history.len();
            self.daily_history.retain(|c| crate::daily::still_keep(c, t, &daily_cfg));
            if self.daily_history.len() != before {
                let _ = self.store.save("daily_history", &self.daily_history);
            }
        }
        // Reaping idle helpers moved to the top of `tick`, above the pause
        // check, for the reason given there: it is housekeeping, and pausing
        // Atlas is exactly when you want the memory back.
        // Anything the crew finished, vanished on, or still won't stop for.
        out.extend(self.take_crew_news(t));

        // --- The day's run, when you come in rather than when a clock says ---
        //
        // For one afternoon this fired at `brief.at_hour`, because that was
        // the setting sitting unread and wiring it was the obvious fix. It
        // was the wrong fix, and the reason is worth keeping: **a brief is
        // worth having when you start, and you do not start at the same time
        // every day.** Seven o'clock greets a night session at its fourth
        // hour and misses the morning that began at ten.
        //
        // So arrival decides. `daily::arriving` asks two things -- has there
        // been a real gap since your last turn, and has your day turned since
        // the last brief -- and the day it measures against is the one
        // `Rhythm` worked out from when you actually stop, not the calendar's.
        // A night owl's day ends at four in the morning and nobody had to say
        // so.
        //
        // The case this exists for: you work eleven until six, through the
        // four o'clock rollover. At 4am there is no gap, so this is
        // `StillGoing` and nothing is said. You sleep and come back at two --
        // a gap, and a day that turned -- so the brief arrives then. Held,
        // not missed, which is the rule `morning_brief` already keeps for the
        // night's work.
        //
        // `not_before_hour` is the only thing the clock still decides, and it
        // decides one thing: not before then. Asking works at any hour.
        {
            let bcfg = self.tools_cfg().brief.clone();
            let dcfg = self.tools_cfg().daily.clone();
            let rolls_at = self.rhythm.rolls_at(&dcfg);
            let this_hour = crate::localclock::hour_here(t) as u8;
            let arrival =
                crate::daily::arriving(self.last_turn_of_yours, self.last_brief_at, t, rolls_at, &dcfg);

            if bcfg.enabled
                && arrival == crate::daily::Arrival::Starting
                && this_hour >= bcfg.not_before_hour
            {
                self.last_brief_at = t;
                let _ = self.store.save("last_brief_at", &self.last_brief_at);
                let b = self.brief_now(t);
                // `b.is_empty()` rather than a check on the sentence: an
                // empty brief still *speaks* -- "Nothing needs you. I'll get
                // on with the rest." -- which is the right answer to someone
                // who just asked and the wrong thing to volunteer every
                // morning for the rest of your life.
                if !b.is_empty() {
                    let line = crate::brief::spoken(&b);
                    match self.morning_brief.take() {
                        // The night finished and this is the same morning.
                        // Both, in the order they happened, rather than one
                        // silently overwriting the other -- which is what a
                        // plain assignment here would have done, on exactly
                        // the mornings there was most to say.
                        Some(night) => self.morning_brief = Some(format!("{night} {line}")),
                        None => self.morning_brief = Some(line),
                    }
                }
            }
        }

        // The morning brief, once, the first time Atlas gets to speak after a
        // night. Taken rather than read: a brief said twice is worse than one
        // said late, and the night it describes is already over.
        if let Some(brief) = self.morning_brief.take() {
            out.push(brief);
        }

        // --- Has the index stopped matching the folder? ---
        //
        // Measured hourly, above the speak gate, because noticing and saying
        // are two different things: a drift found while Atlas is not allowed
        // to interrupt still belongs in the log, and a check that only ran on
        // the ticks Atlas may speak would go unrun for a whole focus session.
        // What it finds is held for the raise at the end of the tick.
        if t.saturating_sub(self.last_index_check) >= 3600 {
            self.last_index_check = t;
            let d = self.index_drift();
            if d.is_clean() {
                self.index_drifted = None;
            } else {
                self.log.info(&format!("notes index has drifted: {}", d.plain()));
                self.index_drifted = Some(d);
            }
        }

        // Should Atlas speak first?
        if !self.modes.may_interrupt(false) {
            self.persist_after(t, !out.is_empty());
            return out;
        }
        // Initiative before reaction. A stalled commitment matters more than
        // a folder that filled up, and if both are true you should only hear
        // one of them.
        let hour = crate::localclock::hour_here(t) as u8;
        // Record that the connections were looked at. Without this the
        // board reports its last answer in the present tense.
        // The heartbeat used to be here, and being here was the bug: two
        // `return out`s above it meant a paused or focused Atlas stopped
        // beating while it was still running. It is now the first thing
        // `tick` does. See the note at the top of this function.
        // Once per run, so the notification path knows whether headphones are
        // connected without launching anything itself.
        self.refresh_audio_once();
        self.connections.swept(t);
        // A connection that has genuinely started failing is worth saying out
        // loud once, not only writing to a log nobody opens. `link_broke`
        // existed for exactly this and was called by nothing — which was
        // academic while the board was empty, and is not any more.
        //
        // `Failing` only, deliberately: `Unknown` is the state every
        // dependency starts in, so nudging on it would greet every fresh
        // start with a complaint about connections that are probably fine.
        // Two failures, not one — one failure does not announce that your
        // bank is down. `health()` reports Failing on a single failure, which
        // is right for a status page and wrong for an interruption, so the
        // threshold lives here where the interrupting happens rather than by
        // changing a `health()` other callers already depend on.
        let broken: Vec<crate::nudge::Nudge> = self
            .connections
            .needs_you(t)
            .into_iter()
            .filter(|i| {
                i.health(t) == crate::integrations::Health::Failing && i.failures_running >= 2
            })
            .map(crate::nudge::link_broke)
            .collect();
        // Written when it changes, not on every tick: on Eric's laptop the
        // same "never seen it work" line went to the log four times a
        // second (26 Sep 2026), which buries everything else in it.
        let lines: Vec<String> = self.connections.needs_you(t).iter().map(|i| i.line(t)).collect();
        for l in lines.iter().filter(|l| !self.connections_logged.contains(l)) {
            self.log.info(l);
        }
        self.connections_logged = lines;
        // At most one, and only when Atlas is allowed to speak at all. A
        // machine that has just gone offline has several things fail at once,
        // and hearing about each of them separately is noise. Behind the same
        // interrupt gate as every other nudge — without it this spoke in
        // unattended mode, where the whole contract is that Atlas does not
        // start conversations.
        if let Some(n) = broken
            .into_iter()
            .next()
            .filter(|_| self.proactive.may_interrupt(&signals, t))
        {
            let subject = n.subject.clone().unwrap_or_default();
            if self.nudger.may_raise(&subject, t) {
                self.nudger.raised(&subject, &n.message, t);
                let offer = crate::proactive::from_nudge(&n);
                self.session.ask(&offer.message);
                out.push(offer.message.clone());
                self.pending_offer = Some(offer);
            }
        }
        // Once there is enough history to judge by, worth_saying() names a
        // real problem -- reports that never cover what you ask -- rather
        // than a routine status update, so it goes to the log rather than
        // interrupting you as a nudge -- once each time it changes, not on
        // every tick.
        if let Some(m) = self.tier_mix.worth_saying() {
            if !self.connections_logged.contains(&m) {
                self.log.info(&m);
            }
            self.connections_logged.push(m);
        }
        let nudge = if self.proactive.may_interrupt(&signals, t) {
            self.nudger.consider(t, hour, signals.dwell_secs, signals.idle_secs)
        } else {
            None
        };
        // A daypart greeting on its own says "there is a lot on", which is a
        // notification. With the run attached it says what to start with,
        // which is the difference between being told you are busy and being
        // helped -- `daypart_with_brief`'s own doc, and it had no caller
        // because there was no brief worth attaching.
        //
        // Only when the brief has something in it. A greeting that says
        // "nothing needs you" every morning is the count-shaped notification
        // this module is written against, and `Brief::is_empty` is the check
        // that already knows the difference.
        let nudge = match nudge {
            Some(n) if n.trigger == crate::nudge::Trigger::Daypart => {
                let b = self.brief_now(t);
                match (crate::nudge::Part::from_hour(hour), b.is_empty()) {
                    (Some(part), false) => Some(crate::nudge::daypart_with_brief(part, &b)),
                    _ => Some(n),
                }
            }
            other => other,
        };
        // A nudge and an offer on the same tick are weighed together
        // (`bandit`, by how you've answered each kind) instead of the nudge
        // always taking the floor; either way only one question is asked.
        let from_nudge = nudge.as_ref().map(crate::proactive::from_nudge);
        let nudge_kind = from_nudge.as_ref().map(|o| o.kind.clone());
        let chosen = self.proactive.consider_with(&signals, &self.memory, t, from_nudge);
        // The nudge lost (or nothing was said at all): the nudger must not
        // go on believing it spoke, or its one-shot question and its
        // morning greeting are spent on something you never heard.
        if let Some(n) = &nudge {
            if chosen.as_ref().map(|o| Some(&o.kind)) != Some(nudge_kind.as_ref()) {
                self.nudger.unsaid(n);
            }
        }
        if let Some(offer) = chosen {
            self.session.ask(&offer.message);
            out.push(offer.message.clone());
            self.pending_offer = Some(offer);
        }

        // --- The index, if nothing else took the floor ---
        //
        // Raised last and only when nothing else is already on the table. A
        // stale index is a real fault — Atlas trusts it and stops looking —
        // but it is never the most urgent thing in the room, and two
        // questions asked in one tick means one of them gets the answer meant
        // for the other. Through `may_raise`/`raised` like every other nudge,
        // so a "no" sticks and silence widens the gap.
        //
        // The drift itself was measured before the speak gate above, so it is
        // found and logged even on the ticks where Atlas may not say anything.
        if let Some(d) = self.index_drifted.clone() {
            if let Some(n) = crate::nudge::drifted(&d) {
                let subject = n.subject.clone().unwrap_or_default();
                if self.pending_offer.is_none()
                    && self.proactive.may_interrupt(&signals, t)
                    && self.nudger.may_raise(&subject, t)
                {
                    self.index_drifted = None;
                    self.nudger.raised(&subject, &n.message, t);
                    let offer = crate::proactive::from_nudge(&n);
                    self.session.ask(&offer.message);
                    out.push(offer.message.clone());
                    self.pending_offer = Some(offer);
                }
            }
        }

        self.persist_after(t, !out.is_empty());
        out
    }

    /// The end-of-tick save: now when the tick had something to say, and
    /// otherwise on a minute's sweep.
    ///
    /// `persist` saved every state file at the end of every tick — up to
    /// every two seconds, forever, whether or not a byte had changed, and
    /// one of them is the file index. `Store::save` now skips identical
    /// bytes, but *serialising* everything thirty times a minute to discover
    /// nothing happened is about one percent of a core, permanently, and
    /// that is the entire idle budget. What this risks is up to a minute of
    /// internal bookkeeping on an unclean kill — never anything Atlas told
    /// you (a tick that spoke is a tick that saved) and never anything you
    /// did (a turn saves at its own call sites).
    pub fn persist_after(&mut self, t: u64, spoke: bool) {
        if spoke || self.last_persist == 0 || t.saturating_sub(self.last_persist) >= PERSIST_SWEEP_SECS {
            self.persist();
            self.last_persist = t.max(1);
        }
    }

    /// Take in the file index once it has been read (`index::Loading`).
    /// True when it is here (or there was nothing to read).
    fn settle_index(&mut self) -> bool {
        match self.index_load.settle(&mut self.index) {
            crate::index::Settled::StillLoading => false,
            crate::index::Settled::Took(on_disk) => {
                self.index_on_disk = Some(on_disk);
                true
            }
            crate::index::Settled::KeptNewer | crate::index::Settled::Ready => true,
        }
    }

    /// Wait up to `max` for the file index to be read (tests, and anything
    /// that needs the whole list before it can answer).
    pub fn wait_for_index(&mut self, max: std::time::Duration) -> bool {
        if let crate::index::Settled::Took(on_disk) = self.index_load.wait(&mut self.index, max) {
            self.index_on_disk = Some(on_disk);
        }
        !self.index_load.is_loading()
    }

    /// For a test: the index as if its read from disk hadn't finished.
    #[doc(hidden)]
    pub fn index_loading_for_test(&mut self, l: crate::index::Loading) {
        self.index = Index::default();
        self.index_on_disk = Some(self.index.written_as());
        self.index_load = l;
    }

    /// Is the file index still being read from disk? (A question about
    /// your files waits up to `INDEX_WAIT_FOR_A_QUESTION` for it first.)
    pub fn index_still_loading(&self) -> bool {
        self.index_load.is_loading()
    }

    pub fn observe(&mut self, t: u64) -> Signals {
        // Not before the index on disk is in: a walk compared against the
        // empty stand-in would report every file you have as new.
        let loaded = self.settle_index();
        // A rescan of the index costs disk and battery: none below your
        // battery floor, and none once it's past the size you set
        // (`perf::Throttle::may_scan`; `max_index_entries` was read by nothing).
        let may_scan = loaded && crate::perf::Throttle::new(self.tools_cfg().perf.clone()).may_scan(self.power, self.index.entries.len());
        self.awareness.observe(
            self.plat,
            &mut self.index,
            if may_scan { self.cfg.indexing.as_ref() } else { None },
            self.session.is_waiting(),
            t,
        )
    }

    /// What the model gets to see: the machine, plus what you're doing, plus
    /// the last few exchanges.
    /// Write what happened here into a folder, and take in whatever the other
    /// side left there.
    ///
    /// A folder is the carrier `sync.rs` already names -- `Carry::CloudFolder`,
    /// "it'll land next time both are on" -- and it is the only one that needs
    /// nothing running on the other machine and no network of its own. Point
    /// two Atlases at the same synced folder, a share, or a USB stick, and
    /// they carry each other.
    ///
    /// This replaces a message that said "moving files between your devices
    /// isn't built yet". That message was honest about itself, which is the
    /// better kind of stub, but `sync.rs` was 443 lines of working merge
    /// sitting behind it: the append-only log, the bundle, the version check,
    /// the clash rule that only `Changed` and `Removed` can conflict, and
    /// `already_seen` so a folder read twice does not double anything.
    /// Our own log as bytes ready for the wire — sealed with the household key
    /// when sealing is on, plain JSON otherwise, exactly as the folder path
    /// writes it. `None` only if sealing is on and there is no key to seal with,
    /// which is the same "don't ship it in the clear" refusal the folder makes.
    fn wire_bytes(
        &mut self,
        cfg: &crate::sync::SyncConfig,
        key: Option<&[u8]>,
        now: u64,
    ) -> Option<Vec<u8>> {
        let mut bundle =
            {
                self.note_addon_changes(now);
                crate::sync::make_bundle(&self.synclog, &self.synclog.device.clone(), 0, now)
            };
        bundle.belongs_to = cfg.belongs_to.clone();
        if cfg.encrypt_bundles {
            crate::sync::seal(&bundle, key?).ok().map(String::into_bytes)
        } else {
            serde_json::to_vec(&bundle).ok()
        }
    }

    /// Take a bundle in off the wire: the same acceptance the folder path does
    /// — skip our own, refuse another household's or a newer format, keep only
    /// what we haven't seen, merge, advance the clock, and append. Returns how
    /// many events were taken, any clash lines, and any clock-skew lines. The
    /// transport is just bytes; this is where a wire bundle becomes ours.
    fn take_in_wire(
        &mut self,
        incoming: &[u8],
        cfg: &crate::sync::SyncConfig,
        key: Option<&[u8]>,
        now: u64,
    ) -> (usize, Vec<String>, Vec<String>) {
        let mut clashes = Vec::new();
        let mut skews = Vec::new();
        let Ok(text) = std::str::from_utf8(incoming) else {
            return (0, clashes, skews);
        };
        let bundle = match crate::sync::read_bundle(text, key) {
            Ok(b) => b,
            Err(_) => return (0, clashes, skews),
        };
        if bundle.from_device == self.synclog.device
            || crate::sync::can_open(&bundle).is_err()
            || crate::sync::from_the_same_atlas(&bundle, &cfg.belongs_to).is_err()
        {
            return (0, clashes, skews);
        }
        let fresh: Vec<crate::sync::Event> = bundle
            .events
            .iter()
            .filter(|e| !self.seen_up_to.iter().any(|(d, s)| *d == e.device && e.seq <= *s))
            .cloned()
            .collect();
        if fresh.is_empty() {
            return (0, clashes, skews);
        }
        let merged = crate::sync::merge(&self.synclog.events, &fresh, now);
        if !merged.clashes.is_empty() {
            clashes.push(crate::sync::spoken(&merged, &bundle.from_name));
        }
        if let Some(skew) = self.synclog.note_seen(&fresh, now) {
            skews.push(skew.plain(&bundle.from_name));
        }
        let sealed = crate::sync::peek(text).is_some();
        for said in self.take_in_synced(&fresh, sealed, &bundle.from_name) {
            clashes.push(said);
        }
        let taken = fresh.len();
        for e in fresh {
            match self.seen_up_to.iter_mut().find(|(d, _)| *d == e.device) {
                Some((_, s)) => *s = (*s).max(e.seq),
                None => self.seen_up_to.push((e.device.clone(), e.seq)),
            }
            self.synclog.events.push(e);
        }
        (taken, clashes, skews)
    }

    /// Sync straight to the peers you've named by address — the case a tailnet
    /// (or any VPN, or a reachable public host) exists to serve.
    ///
    /// The same-network path only reaches a peer that answered a LAN broadcast,
    /// so two devices on *different* networks never sync directly even when both
    /// are online and mutually routable — exactly the phone-away-from-home case.
    /// Every `elsewhere` peer already carries a stable `host`; this dials that
    /// host on the fixed sync port and does the same one-round-trip exchange the
    /// LAN path does. `host` being a tailnet IP (100.x on Tailscale/WireGuard,
    /// already in the tree) is what makes it reach across networks — no relay,
    /// no server, nothing hardcoded.
    ///
    /// Offline-first stays intact: the folder path has already run by the time
    /// this is called, so a peer that is asleep or unroutable costs nothing here
    /// — it syncs through the folder when both are next on it, silently, exactly
    /// like a LAN miss. `skip` names peers the LAN path already reached this
    /// pass, so a device that is both on the same wifi *and* named by tailnet
    /// address doesn't get sent to twice. The bundle is re-made per peer so a
    /// single pass carries forward whatever the previous peer just handed us.
    fn dial_configured_peers(
        &mut self,
        cfg: &crate::sync::SyncConfig,
        key: Option<&[u8]>,
        now: u64,
        skip: &[String],
    ) -> Vec<String> {
        let mut lines: Vec<String> = Vec::new();
        let peers = self.tools_cfg().elsewhere.known.clone();
        for p in peers {
            let name = p.name.trim().to_string();
            let host = p.host.trim().to_string();
            if name.is_empty() || host.is_empty() {
                continue;
            }
            if skip.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
                continue;
            }
            // Re-made per peer: if a previous peer this pass handed us events,
            // the next one gets them too, so one pass can chain across a tailnet.
            let Some(bytes) = self.wire_bytes(cfg, key, now) else {
                continue;
            };
            let port = p.sync_port.unwrap_or(crate::transport::SYNC_PORT);
            match crate::transport::exchange(
                &host,
                port,
                &bytes,
                std::time::Duration::from_secs(4),
            ) {
                Ok(reply) => {
                    let (t, cl, sk) = self.take_in_wire(&reply, cfg, key, now);
                    let took = if t > 0 { format!(", took in {t}") } else { String::new() };
                    lines.push(format!(
                        "Synced straight across to {name}{took} — reached by address, \
                         so it works off your local network too."
                    ));
                    lines.extend(cl);
                    lines.extend(sk);
                }
                // Asleep, or not routable right now. The folder carries it; a
                // named peer being unreachable is not worth a line every pass.
                Err(_) => {}
            }
        }
        lines
    }

    /// Record what changed about your add-ons since your other devices were
    /// last told, as ordinary sync events (`plugins::changes_to_carry`).
    fn note_addon_changes(&mut self, now: u64) {
        for (id, field, to) in crate::plugins::changes_to_carry(&self.store, &self.plugins_dir) {
            self.synclog.append(crate::sync::What::Changed { id, field, to }, now);
        }
        // Your groups' lists, and this device's key, for your other devices:
        // what lets your phone manage a group your laptop made.
        let me = self.my_key().unwrap_or_default();
        for (id, field, to) in crate::groups::changes_to_carry(&self.store, &me) {
            self.synclog.append(crate::sync::What::Changed { id, field, to }, now);
        }
    }

    /// Make what your other device did true here too.
    ///
    /// Until this existed, sync carried events and nothing applied them: a
    /// note captured on the phone crossed the wire, sat in this device's log,
    /// and never reached the notebook -- "Atlas on every device" was a log
    /// that travelled. Now a capture lands in the notebook, and an add-on
    /// change lands in the add-ons (`plugins::take_synced`, which only takes
    /// an approval from a sealed bundle). Returns anything worth saying.
    fn take_in_synced(&mut self, fresh: &[crate::sync::Event], sealed: bool, from: &str) -> Vec<String> {
        let mut said = Vec::new();
        let mut notes_changed = false;
        for e in fresh {
            match &e.what {
                crate::sync::What::Captured { text, .. } => {
                    let cfg = self.tools_cfg().capture.clone();
                    self.notebook.capture(text, None, e.at, &cfg);
                    notes_changed = true;
                }
                crate::sync::What::Changed { id, to, .. }
                    if id.starts_with(crate::groups::SYNC_GROUP) || id.starts_with(crate::groups::SYNC_DEVICE) =>
                {
                    let me = crate::peerkey::Identity::load_or_create(&self.peer_dir).ok();
                    if let Some(s) = crate::groups::take_synced(&self.store, me.as_ref(), id, to, sealed) {
                        said.push(s);
                    }
                }
                crate::sync::What::Changed { id, field, to } if id.starts_with(crate::plugins::SYNC_PREFIX) => {
                    if let Some(s) = crate::plugins::take_synced(
                        &self.store,
                        &self.plugins_dir,
                        &self.cfg.commands,
                        id,
                        field,
                        to,
                        sealed,
                        from,
                    ) {
                        said.push(s);
                    }
                }
                _ => {}
            }
        }
        if notes_changed {
            if let Err(e) = self.notebook.save(&self.store) {
                said.push(format!("Notes came over from {from} and I couldn't save them: {e}"));
            }
        }
        said
    }

    /// The household key for sync, loaded and derived the same way the folder
    /// path does — factored out so the per-tick serve can reuse it. Derivation
    /// is Argon2id (~0.5s), so this is called only once a peer has actually
    /// connected, never on an idle tick.
    fn sync_key(&self) -> Option<Vec<u8>> {
        let kept: crate::sync::KeptKey = self.store.load(crate::sync::KEY_FILE);
        if !kept.is_set() {
            return None;
        }
        kept.phrase().and_then(|p| crate::sync::key_from_phrase(&p)).ok()
    }

    /// Listen for a peer dialing us for a direct sync — once, without blocking —
    /// called every tick so a device is reachable continuously, not only while
    /// it is itself running a sync pass. That is the difference between "we
    /// happen to sync when both of us are syncing" and "a phone coming onto the
    /// wifi is answered at once."
    ///
    /// Passive: it only receives a bundle and hands ours back, so it runs even
    /// when Atlas is paused — the same rule the heartbeat and the helper-reap
    /// follow, "paused means only listening," and this is listening. The accept
    /// is non-blocking, so an idle tick spends one syscall and returns; the key
    /// derivation happens only when a peer is actually on the socket. Returns a
    /// line only when something arrived.
    fn serve_direct_sync(&mut self, now: u64) -> Vec<String> {
        let cfg = self.tools_cfg().sync.clone();
        if !cfg.enabled {
            return Vec::new();
        }
        // A bundle is your notes; if Atlas has been handed to someone else,
        // don't serve — the same boundary the folder path draws before writing.
        if self.handover().stance.handed_over() {
            return Vec::new();
        }
        if self.sync_server.is_none() {
            self.sync_server = crate::transport::Server::bind(crate::transport::SYNC_PORT).ok();
        }
        let Some(server) = self.sync_server.take() else {
            return Vec::new();
        };
        let mut lines: Vec<String> = Vec::new();
        let _ = server.poll(std::time::Duration::from_millis(50), |incoming| {
            // A peer actually connected — now the key derivation is worth it.
            let key = self.sync_key();
            let (t, cl, sk) = self.take_in_wire(&incoming, &cfg, key.as_deref(), now);
            if t > 0 {
                lines.push(format!(
                    "Took in {t} from another of your devices, straight across the network."
                ));
            }
            lines.extend(cl);
            lines.extend(sk);
            self.wire_bytes(&cfg, key.as_deref(), now).unwrap_or_default()
        });
        self.sync_server = Some(server);
        lines
    }

    pub(super) fn carry_to_your_other_devices(&mut self, now: u64) -> String {
        let cfg = self.tools_cfg().sync.clone();
        if !cfg.enabled {
            return "Syncing between your devices is switched off in your settings.".into();
        }
        // A bundle is your captures. Whoever is at the machine decides where
        // they go, so this is the one place the question "is that actually
        // you?" has to be asked before anything is written.
        //
        // Handed over means you have said out loud that somebody else is
        // using Atlas. They may talk to it; they may not push your notes into
        // a folder. This was missing from the first version of this function
        // and is the leak it would have caused.
        if self.handover().stance.handed_over() {
            return "Not while somebody else is using Atlas — a bundle is your \
                    own notes, and that's yours to send."
                .into();
        }
        // The best route available (H13i): the folder you set, or else a
        // cloud folder this machine already syncs.
        let mut dir = cfg.folder.trim().to_string();
        if dir.is_empty() {
            match crate::sync::best_folder() {
                Some((p, _)) => dir = p.display().to_string(),
                None => {
                    return "I've nowhere to put it — there's no cloud folder on this machine. Set \
                            `sync.folder` to a folder both machines can see (a share, or a USB stick \
                            you plug in) and I'll carry it through there."
                        .into();
                }
            }
        }
        let (_carry, route) = crate::sync::route_of(std::path::Path::new(&dir));
        self.log.info(&format!("syncing through {dir}: {route}"));
        let dir = std::path::Path::new(&dir);
        if let Err(e) = std::fs::create_dir_all(dir) {
            return format!("I couldn't open {}: {e}", dir.display());
        }

        // Anything left from a pairing that was started and never finished.
        // The window is three minutes and a taken handoff deletes itself, so
        // this is only for the abandoned case -- which is exactly the one
        // nobody would think to tidy up.
        let swept = crate::sync::sweep_handoffs(dir, now)
            + crate::household::sweep_invitations(dir, now);
        if swept > 0 {
            self.log.info(&format!("cleared {swept} expired key handoff(s) from the sync folder"));
        }

        // The household key, derived once for this pass rather than once per
        // bundle -- Argon2id at 64MiB is half a second, and a folder with six
        // bundles in it would otherwise spend three.
        //
        // Loaded whether or not sealing is on: reading is never gated by the
        // switch, so turning it off does not strand bundles already written,
        // and a device that has the key can always open what it is sent.
        let kept: crate::sync::KeptKey = self.store.load(crate::sync::KEY_FILE);
        let key: Option<Vec<u8>> = if kept.is_set() {
            match kept.phrase().and_then(|p| crate::sync::key_from_phrase(&p)) {
                Ok(k) => Some(k),
                Err(why) => {
                    self.log.info(&format!("the household key would not load: {why}"));
                    None
                }
            }
        } else {
            None
        };

        // Take in first, then write out -- so the bundle we leave already
        // includes anything that just arrived, and one pass on each machine
        // is enough to converge rather than two.
        let mut taken = 0usize;
        let mut clashes: Vec<String> = Vec::new();
        let mut refused: Vec<String> = Vec::new();
        // A device whose clock is wildly off — noticed while taking its bundle
        // in, never a reason to refuse it. See `sync::Log::note_seen`.
        let mut skews: Vec<String> = Vec::new();

        // Direct, same network: if the other device dialed us since last pass,
        // serve it now — take its bundle in and hand ours straight back over the
        // socket. Bound lazily and polled once, without blocking: a step at a
        // time, like everything else on this clock. The folder below still runs,
        // so this is pure acceleration when both are on the same wifi.
        if self.sync_server.is_none() {
            self.sync_server = crate::transport::Server::bind(crate::transport::SYNC_PORT).ok();
        }
        if let Some(server) = self.sync_server.take() {
            let key_ref = key.as_deref();
            let _ = server.poll(std::time::Duration::from_millis(200), |incoming| {
                let (t, mut cl, mut sk) = self.take_in_wire(&incoming, &cfg, key_ref, now);
                taken += t;
                clashes.append(&mut cl);
                skews.append(&mut sk);
                self.wire_bytes(&cfg, key_ref, now).unwrap_or_default()
            });
            self.sync_server = Some(server);
        }
        if let Ok(entries) = std::fs::read_dir(dir) {
            let mut paths: Vec<std::path::PathBuf> =
                entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.extension().and_then(|x| x.to_str()) != Some("bundle") {
                    continue;
                }
                let Ok(raw) = std::fs::read_to_string(&p) else { continue };
                // Sealed or plain, through one reader. Two paths here is how
                // a plaintext fallback survives a feature meant to remove
                // one -- and a sealed bundle with no key says which command
                // fixes that rather than reporting an unreadable file.
                let bundle = match crate::sync::read_bundle(&raw, key.as_deref()) {
                    Ok(b) => b,
                    Err(why) => {
                        refused.push(format!("{}: {why}", p.display()));
                        continue;
                    }
                };
                // Ours, on a folder we also write to. Skipping it is what
                // makes a shared folder work at all.
                if bundle.from_device == self.synclog.device {
                    continue;
                }
                if let Err(why) = crate::sync::can_open(&bundle) {
                    refused.push(why);
                    continue;
                }
                // Your other Atlas is not your other machine. A work Atlas and
                // the personal one do not carry each other; they are linked
                // through the business hub, item by item, on purpose.
                if let Err(why) = crate::sync::from_the_same_atlas(&bundle, &cfg.belongs_to) {
                    refused.push(why);
                    continue;
                }
                // Only what we have not already taken from that device.
                // Counting the whole bundle each time would report two new
                // things every morning for a folder that had not changed.
                let (new, _skipped) =
                    crate::sync::already_seen(&bundle.events, &self.seen_up_to);
                if new == 0 {
                    continue;
                }
                let fresh: Vec<crate::sync::Event> = bundle
                    .events
                    .iter()
                    .filter(|e| {
                        !self
                            .seen_up_to
                            .iter()
                            .any(|(d, s)| *d == e.device && e.seq <= *s)
                    })
                    .cloned()
                    .collect();

                let merged = crate::sync::merge(&self.synclog.events, &fresh, now);
                taken += fresh.len();
                if !merged.clashes.is_empty() {
                    clashes.push(crate::sync::spoken(&merged, &bundle.from_name));
                }
                // Advance this device's clock past everything the bundle
                // carried, so anything done after this reconnect sorts after
                // what was just learned — and catch a wrong remote clock while
                // we're here. The events are kept either way.
                if let Some(skew) = self.synclog.note_seen(&fresh, now) {
                    skews.push(skew.plain(&bundle.from_name));
                }
                let sealed = crate::sync::peek(&raw).is_some();
                for said in self.take_in_synced(&fresh, sealed, &bundle.from_name) {
                    clashes.push(said);
                }
                for e in fresh {
                    match self.seen_up_to.iter_mut().find(|(d, _)| *d == e.device) {
                        Some((_, s)) => *s = (*s).max(e.seq),
                        None => self.seen_up_to.push((e.device.clone(), e.seq)),
                    }
                    self.synclog.events.push(e);
                }
            }
        }

        let mut bundle =
            {
                self.note_addon_changes(now);
                crate::sync::make_bundle(&self.synclog, &self.synclog.device.clone(), 0, now)
            };
        // Stamped on the way out, so the other side can tell whose it is
        // without having to know which folder it came from.
        bundle.belongs_to = cfg.belongs_to.clone();
        let name = format!("{}.bundle", sanitise(&self.synclog.device));
        let out = dir.join(name);

        // Sealed, if you asked for that. With sealing on and no key, nothing
        // is written at all: falling back to plaintext would be the switch
        // quietly doing the opposite of what it says, which is the whole
        // class of defect this was found in.
        let mut made_a_key = String::new();
        let written = if cfg.encrypt_bundles {
            // No key yet? Make one. The first version refused here and told
            // you to run a command, which is a wall in front of a switch you
            // just turned on -- and the switch is in the hub, where somebody
            // who does not use a terminal found it.
            //
            // Making one is safe to do unasked precisely because losing it
            // costs nothing durable: a bundle is a courier, every bundle
            // carries the whole log, and the sync page has a button that
            // starts a new key.
            // Cloned so the outer `key` survives for the direct same-network
            // send below, which seals with the same household key.
            let key = match key.clone() {
                Some(k) => Ok(k),
                None => match crate::sync::ensure_key(&self.store, now) {
                    Ok((k, made)) => {
                        if let Some(setup) = made {
                            made_a_key = crate::sync::made_one(&setup);
                            self.log.info("made a household key for sealing bundles");
                        }
                        Ok(k)
                    }
                    Err(why) => Err(why),
                },
            };
            match key {
                Err(why) => Err(why),
                Ok(k) => crate::sync::seal(&bundle, &k)
                    .and_then(|raw| crate::sync::write_whole(&out, raw.as_bytes())),
            }
        } else {
            serde_json::to_string_pretty(&bundle)
                .map_err(|e| e.to_string())
                .and_then(|raw| crate::sync::write_whole(&out, raw.as_bytes()))
        };

        // --- Which route this would have taken ---
        //
        // `mesh::choose` picks between `SameNetwork`, `Mesh`, `Cable` and
        // `Cloud`, and until 19 Sep 2026 every argument it got was a
        // hardcoded literal, so the answer was invariably `Cloud` and the
        // other three were unreachable. `Cloud` is a folder and needs no
        // address, which is why it was the only one that ever worked.
        //
        // `same_network` is a real observation now: `nearby::look` shouts on
        // the local network and `mesh::on_this_network` asks whether the
        // machine you actually sync with answered. Nothing is *sent* that way
        // yet -- the bundle still goes through the folder, which is written
        // above -- so this says what it found rather than claiming a
        // transport that is not built. That is the same line `atlas mesh`
        // takes and it is drawn in the same place.
        //
        // Looked at only when there is somebody to look for: a broadcast on
        // every sync, on a machine with no peers configured, is a packet sent
        // to ask a question nobody is waiting to answer.
        let mcfg = self.tools_cfg().mesh.clone();
        let ncfg = self.tools_cfg().nearby.clone();
        let peers = self.tools_cfg().elsewhere.names().iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let (route, peer_addr) = if peers.is_empty() {
            (None, None)
        } else {
            let found = crate::nearby::look(&ncfg).unwrap_or_default();
            let here = peers.iter().any(|p| crate::mesh::on_this_network(p, &found));
            // The peer that answered — its name (so the configured-address dial
            // below can skip it) and its host (the one we send straight to). Its
            // shout carries its host; the sync port is fixed.
            let addr = found.iter().find_map(|f| {
                peers
                    .iter()
                    .find(|p| p.eq_ignore_ascii_case(&f.name))
                    .map(|p| (p.clone(), f.host.clone(), crate::transport::SYNC_PORT))
            });
            // `mesh_up` stays false: a private network is the one route with
            // nothing behind it, and saying otherwise here would be the
            // hardcoded literal coming back wearing an observation's clothes.
            (Some(crate::mesh::choose(here, false, true, false, &mcfg)), addr)
        };

        let mut said = match written {
            Ok(()) => format!(
                "Left {} thing{} for your other devices in {}{}.{}",
                bundle.events.len(),
                if bundle.events.len() == 1 { "" } else { "s" },
                dir.display(),
                if cfg.encrypt_bundles { ", sealed" } else { "" },
                made_a_key
            ),
            Err(e) => format!("I couldn't write the bundle: {e}"),
        };
        if taken > 0 {
            said.push_str(&format!(" Took in {taken} from the other side."));
        }
        for c in clashes {
            said.push(' ');
            said.push_str(&c);
        }
        for r in refused {
            said.push_str(&format!(" {r}"));
        }
        for s in skews {
            said.push(' ');
            said.push_str(&s);
        }
        // Said last, and only when there was something to notice. On the same
        // wifi the folder is a slow way to move something across the room,
        // and that is worth knowing even while it is still the only way.
        // Peers reached directly this pass, by name — so the configured-address
        // dial below never sends to the same device twice.
        let mut synced_directly: Vec<String> = Vec::new();
        if let Some((crate::mesh::Path::SameNetwork, why)) = route {
            match &peer_addr {
                Some((name, host, port)) => {
                    // Straight across the wifi. Send ours, take theirs back, in
                    // one round trip. The folder above already ran, so a miss
                    // here costs nothing — it just means the folder does it.
                    match self.wire_bytes(&cfg, key.as_deref(), now) {
                        Some(bytes) => match crate::transport::exchange(
                            host,
                            *port,
                            &bytes,
                            std::time::Duration::from_secs(3),
                        ) {
                            Ok(reply) => {
                                let (t, cl, sk) =
                                    self.take_in_wire(&reply, &cfg, key.as_deref(), now);
                                synced_directly.push(name.clone());
                                said.push_str(&format!(
                                    " ({why} — synced straight across to your other Atlas, no folder needed{}.)",
                                    if t > 0 { format!(", took in {t}") } else { String::new() }
                                ));
                                for c in cl {
                                    said.push(' ');
                                    said.push_str(&c);
                                }
                                for s in sk {
                                    said.push(' ');
                                    said.push_str(&s);
                                }
                            }
                            Err(_) => said.push_str(&format!(
                                " ({why} — your other Atlas answered on this network, but the \
                                 direct send didn't land; it's in the folder.)"
                            )),
                        },
                        None => said.push_str(&format!(
                            " ({why} — your other Atlas is on this network; it's in the folder.)"
                        )),
                    }
                }
                None => said.push_str(&format!(
                    " ({why} — your other Atlas is on this network; it's in the folder.)"
                )),
            }
        }

        // Then the peers you've named by address that we did *not* just reach on
        // the local network — the tailnet/VPN/public-host case. This is what
        // lets the phone sync with the laptop when they're on different networks
        // but both online, without a folder in between. Silent when they're
        // asleep; the folder still carries it.
        for line in self.dial_configured_peers(&cfg, key.as_deref(), now, &synced_directly) {
            said.push(' ');
            said.push_str(&line);
        }
        said
    }

    /// One step of the night, at most once an hour.
    ///
    /// Written as "advance by one" rather than "run the whole night in a
    /// loop" on purpose: a loop here would hold the tick for as long as the
    /// night lasts, and the tick is also how Atlas answers you. The night is
    /// hours long and has nobody waiting on it, so it advances a step at a
    /// time like everything else on this clock.
    fn run_the_night(&mut self, t: u64) {
        let cfg = self.tools_cfg().overnight.clone();
        let dcfg = self.tools_cfg().daily.clone();

        // --- Where you are, rather than what time it is ---
        //
        // This was `cfg.in_window(hour)` -- `start_hour` to `stop_hour`, a
        // fixed clock window, and exactly the mistake `brief.at_hour` was. It
        // cannot tell you asleep from you at a desk at two in the morning,
        // and it has no idea you left for work at eight. So the night's work
        // ran on a timetable: it ran while you were up working, and it did
        // not run on the Saturday you were out all day.
        //
        // `daily::whereabouts` asks instead whether you are gone, and gone
        // long enough that starting an hour of work will not be interrupted.
        // Two kinds of gone, because they are different permissions:
        // `Asleep` is your quiet stretch, which `Rhythm` worked out from when
        // you actually stop; `Out` is a gap in hours you are usually about,
        // which can end at any moment.
        //
        // Nothing asks you to say you are stepping away. Being told is a
        // thing people do not do, and a system that needs telling does
        // nothing.
        let where_you_are = crate::daily::whereabouts(
            self.last_turn_of_yours,
            t,
            &self.rhythm,
            &dcfg,
            &self.tools_cfg().judgment,
        );
        // Recorded only while you are gone, so it holds where you were
        // *while the work happened*. Set unconditionally it is overwritten by
        // the first tick after you come back, and the write-up then opens
        // "You're about:" -- which is true of this moment and false of the
        // six hours it is describing. Caught by the night's own test
        // reporting itself that way.
        if where_you_are.free_to_work() {
            self.worked_while = Some(where_you_are);
        }

        // Back at the machine with a night behind us: write it up once, then
        // forget it. Checked before `enabled` so that switching overnight off
        // mid-session still gives you the brief for the work already done
        // rather than swallowing it.
        if !where_you_are.free_to_work() {
            if let Some(line) = crate::inhibit::apply(&mut self.sleep_hold, crate::awake::Hold::Release, "") {
                self.log.info(&line);
            }
            if let Some(session) = self.overnight.take() {
                if !session.results.is_empty() {
                    // Where you were while it ran, not where you are now.
                    // Taken from what was recorded at the time -- asking now
                    // would say "you're about", which is true and is a
                    // different fact.
                    let then = self.worked_while.unwrap_or(crate::daily::Whereabouts::Asleep);
                    let mut brief = crate::overnight::morning_brief(&session, then);
                    if let Some(why) = &session.ended_because {
                        brief.push_str(&format!(" I stopped because I {why}."));
                    }
                    // The long account (H13j): kept as a note, mentioned in
                    // the brief, and there when asked for.
                    let detail = crate::overnight::morning_detail(&session);
                    let _ = self.store.save("overnight_detail", &detail);
                    let ccfg = self.tools_cfg().capture.clone();
                    self.notebook.capture(&detail, Some("overnight"), t, &ccfg);
                    let _ = self.notebook.save(&self.store);
                    brief.push_str(" The full account is in your notes, or ask \"what did you do overnight\".");
                    self.morning_brief = Some(brief);
                }
            }
            return;
        }
        if !cfg.enabled {
            return;
        }
        // The guarantee, read rather than assumed.
        //
        // `apply_while_asleep` is `#[serde(skip)]` and hard-wired false, so
        // this cannot fire today. It is here because a flag nothing reads is
        // a promise nothing keeps: if someone ever makes it settable, the
        // night refuses to run rather than quietly starting to apply things
        // while you are asleep. Defaulting to "do nothing" is the only safe
        // direction for a mistake in this particular setting.
        if cfg.apply_while_asleep {
            self.log.warn(
                "overnight is set to apply changes while you're asleep — refusing to run",
            );
            return;
        }
        if t.saturating_sub(self.last_overnight) < 3600 {
            return;
        }
        self.last_overnight = t;

        // What Atlas could not finish, minus anything that needs a decision
        // only you can make. `worth_doing_overnight` reads the request the
        // way you said it -- "which of these", "should I", "send", "pay" --
        // and leaves those alone. Working on one of those overnight would
        // mean guessing at the answer, which is the one thing the night must
        // not do.
        let queue: Vec<String> = self
            .backlog
            .outstanding()
            .iter()
            .map(|i| i.request.clone())
            .filter(|r| {
                let p = crate::handoff::Problem {
                    goal: r.clone(),
                    ..Default::default()
                };
                crate::overnight::worth_doing_overnight(&p)
            })
            .collect();

        // May the machine be held for the night's work? `keep_awake` and the
        // `awake` module were built for exactly this moment. Since round 5 the
        // answer is acted on (`inhibit`, below). It also gates the work: on
        // battery below the give-up
        // line, starting an hour of work that will die and cost you the
        // morning's charge is worse than not starting it. So the decision
        // gates whether tonight's work begins, and its reason is recorded
        // once rather than discarded.
        let r = crate::health::read_machine();
        let power = crate::awake::Power {
            on_battery: r.on_battery,
            battery_pct: r.battery_percent.map(|p| p as u32).unwrap_or(100),
            // The lid state has no reader on any platform, so it is reported
            // as unknown rather than guessed — the battery branch below does
            // not depend on it.
            lid_closed: false,
            lid_action: crate::awake::LidAction::Unknown,
            external_display: false,
        };
        let held_mins = t.saturating_sub(self.overnight.as_ref().map(|s| s.started_at).unwrap_or(t))
            / 60;
        let (hold, why_not) =
            self.keep_awake(crate::awake::Because::OvernightWork, &power, held_mins as u32);
        // And now it is acted on: the machine is actually held awake while
        // the night runs, and let go when the decision says so.
        if let Some(line) = crate::inhibit::apply(&mut self.sleep_hold, hold, "tonight's work") {
            if line.starts_with("couldn't") {
                self.log.warn(&line);
            } else {
                self.log.info(&line);
            }
        }
        if hold == crate::awake::Hold::Release && !why_not.is_empty() {
            // Said once, when the night declines to start, not on every tick.
            if self.overnight.is_none() {
                self.log.info(&format!("not starting overnight work: {why_not}"));
            }
            return;
        }

        let session = self.overnight.get_or_insert_with(|| crate::overnight::Session::start(t));
        // No hosted brain is wired, so there is no spend to have left. Passed
        // explicitly rather than defaulted so that wiring one later has to
        // come back through here.
        let budget_left = 0.0;
        match session.next(&queue, &cfg, where_you_are, budget_left) {
            crate::overnight::Step::Work(problem) => {
                // `ask_you_later`: the night's work is the writing-up. Recorded
                // as `NeedsYou` because that is what is true -- it is waiting
                // on you, not stuck and not solved. Claiming `Solved` here
                // would be the morning brief lying about a night's work.
                session.record(crate::overnight::Result_ {
                    problem: problem.clone(),
                    outcome: crate::overnight::Outcome::NeedsYou,
                    attempts: 0,
                    sandbox_path: None,
                    tests_passed: None,
                    note: crate::overnight::note_for(&problem, &cfg),
                    dollars: 0.0,
                });
            }
            crate::overnight::Step::Finish(why) | crate::overnight::Step::Abandon(why) => {
                // The night is over, and the brief waits for the morning.
                //
                // It used to be said the moment the queue emptied, which on a
                // short backlog is two in the morning -- a "morning brief"
                // delivered while you are asleep, to nobody, and gone by the
                // time you are up. The session is marked ended and held; the
                // out-of-window branch above is what speaks it.
                session.ended_at = Some(t);
                if session.ended_because.is_none() {
                    session.ended_because = Some(why);
                }
                if let Some(line) = crate::inhibit::apply(&mut self.sleep_hold, crate::awake::Hold::Release, "") {
                    self.log.info(&line);
                }
            }
        }
    }
}
