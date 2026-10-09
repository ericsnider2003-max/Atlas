//! Settings, barriers, open work and Outstanding, accounts and holds.
//!
//! Moved out of `hublive.rs` unchanged (audit Q6, 6 Oct 2026): one file of
//! thousands of lines was where every chat's edits collided.

use super::*;

impl Daemon<'_> {
    pub(super) fn outstanding_page_live(&self, now: u64) -> String {
        let mut html = hub::outstanding_page(&self.open_work(now));
        let mut review = String::new();
        match self.calendar_review_rows() {
            Ok(rows) if !rows.is_empty() => {
                review.push_str("<section aria-labelledby=calendar-recovery><h2 id=calendar-recovery>Calendar outcomes to review</h2><p>A reminder may already have reached you. Retrying can repeat it. Dismiss records your review; it does not claim delivery or remove a saved event.</p>");
                for (id, explanation, token) in rows {
                    review.push_str(&format!("<div class=item><p>{}</p><form method=post action='/hub/calendar/review'><input type=hidden name=id value='{}'><input type=hidden name=token value='{}'>", hub::esc(&explanation), hub::esc(&id), hub::esc(&token)));
                    if id.starts_with("reminder:") {
                        review.push_str("<button name=what value=retry>Retry this reminder, even if already received</button> ");
                    }
                    review.push_str("<button name=what value=dismiss>I've reviewed this; dismiss</button></form></div>");
                }
                review.push_str("</section>");
            }
            Ok(_) => {}
            Err(_) => review.push_str("<p role=alert>Calendar recovery records are unavailable. No previous outcome can be confirmed; refresh or recover the saved records before deciding.</p>"),
        }
        if let Some(at) = html.rfind("</main>") { html.insert_str(at, &review); }
        html
    }

    /// Validate a setting, then keep it, then take it up at once. The one
    /// path every settings form uses (Settings, Sound & voice), so a switch
    /// can't report success and write nothing.
    pub(crate) fn apply_setting(&mut self, key: &str, value: &str) -> String {
        // Validated first, then written. Validating first matters:
        // `Settings::set` is what knows a toggle from a number from a name,
        // and writing an unparseable value into the settings file would turn
        // a switch that did nothing into one that stops Atlas starting.
        let mut settings = crate::settings::registry(&self.tools_cfg());
        // Written where it is watched, so the change that was just kept is the
        // one picked up below (the same folder when Atlas runs for real).
        let dir = self.settings_dir().unwrap_or_else(crate::roots::config_dir);
        let said = settings.set_and_keep(key, value, &dir);
        // What taking it up says wins: it knows a change that waits for the
        // next start from one that is live now (29 Sep 2026: "is now on" was
        // shown for Voice, Push-to-talk and the speaking voice, which only
        // change when Atlas starts again).
        let took = self.pick_up_settings();
        if !took.is_empty() {
            return took.join(" ");
        }
        if crate::settings::needs_a_restart(key) && !said.to_lowercase().contains("couldn") {
            return format!("{said} It takes effect when Atlas next starts.");
        }
        said
    }

    /// A turn typed in the hub: answered like any other, and — under
    /// "hands-free only" — kept on screen rather than said.
    /// Someone found something in the way (Help → Report a barrier). On your
    /// own Atlas it goes on your own feedback list; on a friend's, to whoever
    /// sends them Atlas — only because they pressed send on exactly this text.
    pub(super) fn report_barrier(&mut self, text: &str, now: u64) -> String {
        let words = format!("Accessibility: {text}");
        let f = match crate::feedback::compose_feedback(&words, None, now) {
            Ok(f) => f,
            Err(why) => return why,
        };
        let groups = crate::groups::Groups::load(&self.store);
        let sender = groups.held.values().map(|h| &h.state).find(|g| g.release_channel).map(|c| c.owner.clone());
        let me = crate::peerkey::Identity::load_or_create(&self.peer_dir).ok().map(|i| i.public()).unwrap_or_default();
        match sender {
            Some(owner) if owner != me && !me.is_empty() => {
                let to = crate::kin::Pairings::load(&self.peer_dir).name_of_key(&owner);
                match to {
                    Some(to) => {
                        crate::feedback::queue_feedback(&self.store, f, &to);
                        format!("Sent to {to}. They'll see exactly what you wrote, and nothing else.")
                    }
                    None => "I can't reach whoever gave you Atlas, so it's kept here for now.".to_string(),
                }
            }
            _ => {
                // unheard-ok: returns `Option<String>`, not a Result
                let _ = crate::feedback::heard_feedback(&self.store, "you", &serde_json::to_string(&f).unwrap_or_default());
                "It's on your own list to fix, on your Feedback page.".to_string()
            }
        }
    }

    /// The sidebar's parts only the running Atlas knows: your businesses, and
    /// your name on the brand.
    pub(super) fn with_sidebar_names(&self, html: String) -> String {
        let businesses = crate::roster::Roster::load(&self.store).businesses();
        let html = hub::with_business(html, &businesses);
        let name = match crate::returning::Address::load(&self.store) {
            // A guest's hub doesn't carry the owner's name either.
            _ if self.handover().stance.handed_over() => None,
            crate::returning::Address::Name(n) => Some(n),
            _ if !self.store.exists("address") => {
                let n = self.tools_cfg().persona.address.clone();
                // A title ("sir") is how to address you, not your name.
                (!n.trim().is_empty() && n.chars().next().map(|c| c.is_uppercase()).unwrap_or(false)).then_some(n)
            }
            _ => None,
        };
        hub::with_owner(html, name.as_deref())
    }

    /// Outstanding, in the design's four lanes.
    pub(super) fn open_work(&self, now: u64) -> hub::Open {
        let mut drops = hub::Drops::default();
        let mut waiting: Vec<(String, String, String)> = Vec::new();
        for (what, href, key) in self.home_asks_keyed(now) {
            waiting.push((what, String::new(), href));
            drops.waiting.push(key);
        }
        // A question or a yes it's waiting on carries its reason with it.
        for w in waiting.iter_mut() {
            if let Some(i) = self.backlog.items.iter().find(|i| !i.done && !i.dismissed && sentence(&i.request) == w.0) {
                w.1 = sentence(&i.blocker.explain());
            }
        }
        let mut blocked: Vec<hub::Stopped> = Vec::new();
        for i in self.backlog.items.iter().filter(|i| !i.done && !i.dismissed && !waits_on_you(&i.blocker)) {
            blocked.push(hub::Stopped {
                what: sentence(&i.request),
                tried: format!("To {}.", i.request.trim().trim_end_matches('.')),
                stopped: sentence(&i.blocker.explain()),
                needs: i.blocker.needs(),
                area: None,
            });
            drops.blocked.push(Some(format!("b:{}", i.id)));
        }
        let mut in_progress: Vec<(String, String)> = Vec::new();
        for job in &self.scheduler.jobs {
            if job.in_flight {
                let status = if job.state == crate::scheduler::JobState::Cancelled { "Stopping; its result is not confirmed yet." } else { "Running now; its result is not confirmed yet." };
                in_progress.push((sentence(&job.command), status.into()));
                drops.in_progress.push((job.worker_id.is_some() || job.file_move_id.is_some()).then(|| format!("s:{}", job.id)));
            } else if job.interrupted {
                blocked.push(hub::Stopped { what: sentence(&job.command), tried: format!("To {}.", job.command.trim().trim_end_matches('.')),
                    stopped: job.last_result.clone().unwrap_or_else(|| "Previous outcome is unconfirmed.".into()),
                    needs: "Check the previous result before asking Atlas to try again.".into(), area: None });
                drops.blocked.push(Some(format!("s:{}", job.id)));
            }
        }
        for task in self.queue.tasks.iter().filter(|task| task.interrupted) {
            blocked.push(hub::Stopped {
                what: sentence(&task.command),
                tried: format!("To {}.", task.command.trim().trim_end_matches('.')),
                stopped: task.result.clone().unwrap_or_else(|| "Previous outcome not confirmed.".into()),
                needs: "Check the previous result before asking Atlas to try again.".into(),
                area: None,
            });
            drops.blocked.push(Some(format!("t:{}", task.id)));
        }
        for t in self
            .queue
            .tasks
            .iter()
            .filter(|t| !matches!(t.state, crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed))
        {
            let how = match t.state {
                crate::lanes::TaskState::Running if t.stop_requested => "Stopping; the worker has not finished yet.",
                crate::lanes::TaskState::Running => "Running now.",
                crate::lanes::TaskState::WaitingForGap => "Waiting for a pause in your work.",
                _ => "Next in line.",
            };
            in_progress.push((sentence(&t.command), how.to_string()));
            drops.in_progress.push((t.state != crate::lanes::TaskState::Running || t.worker_id.is_some() || t.file_move_id.is_some()).then(|| format!("t:{}", t.id)));
        }
        for e in self.crew.errands() {
            in_progress.push((e.name.clone(), "Handed to a worker — I check what comes back before you see it.".into()));
            drops.in_progress.push(Some(format!("e:{}", e.id)));
        }
        let today = crate::localclock::midnight(now, crate::localclock::offset_secs());
        let mut carried: Vec<(String, u64)> = Vec::new();
        for i in self.workspace.iter().filter(|i| i.at < today && i.status.live()) {
            carried.push((i.title.clone(), ((today - i.at) / 86_400).max(1)));
            drops.carried.push(Some(format!("w:{}", i.id)));
        }
        hub::Open { waiting, blocked, in_progress, carried, drops }
    }

    /// Everything on the Outstanding page that can be taken off it, in the
    /// page's own order, as (key, what to call it). What "remove the second
    /// one from my outstanding list" counts through, so the second one is
    /// the second one you can see (2 Oct 2026: the spoken path only knew the
    /// backlog, so anything else on the page "wasn't on the list").
    ///
    /// A workspace item can be on the page twice (waiting on you, and
    /// carried over): it's listed once. A backlog item is called what you
    /// said, not the page's tidied sentence, because that's what you'll say
    /// back.
    pub(crate) fn outstanding_removable(&self, now: u64) -> Vec<(String, String)> {
        let o = self.open_work(now);
        let titles = o
            .waiting
            .iter()
            .map(|w| w.0.clone())
            .chain(o.blocked.iter().map(|b| b.what.clone()))
            .chain(o.in_progress.iter().map(|p| p.0.clone()))
            .chain(o.carried.iter().map(|c| c.0.clone()));
        let pad = |v: &Vec<Option<String>>, n: usize| (0..n).map(|i| v.get(i).cloned().flatten()).collect::<Vec<_>>();
        let keys = pad(&o.drops.waiting, o.waiting.len())
            .into_iter()
            .chain(pad(&o.drops.blocked, o.blocked.len()))
            .chain(pad(&o.drops.in_progress, o.in_progress.len()))
            .chain(pad(&o.drops.carried, o.carried.len()));
        let mut out: Vec<(String, String)> = Vec::new();
        for (key, title) in keys.zip(titles) {
            let Some(key) = key else { continue };
            if out.iter().any(|(k, _)| *k == key) {
                continue;
            }
            let title = key
                .strip_prefix("b:")
                .and_then(|id| id.parse::<u64>().ok())
                .and_then(|id| self.backlog.items.iter().find(|i| i.id == id))
                .map(|i| i.request.trim().to_string())
                .unwrap_or(title);
            out.push((key, title));
        }
        out
    }

    /// Take one thing off the Outstanding page, by the key its button (or
    /// `outstanding_removable`) gave it, and keep that. The one way every
    /// path removes -- the hub's buttons, "take it off my outstanding list",
    /// "drop the task" -- so none of them can forget to save, and none can
    /// say gone while it comes back after a restart (2 Oct 2026).
    ///
    /// Each kind goes the way its own model already has for it: a backlog
    /// item is dismissed (and kept with what you dropped, so "bring back
    /// what I dropped" finds it), a workspace item is marked dropped, a
    /// queued task that hasn't started is taken out of the queue, a worker's
    /// errand is asked to stop, and a project change waiting for your yes is
    /// dropped. `Err` is what to say when there was nothing to take off.
    pub(crate) fn drop_outstanding(&mut self, key: &str, now: u64) -> Result<OffTheList, String> {
        let (kind, rest) = key.split_once(':').unwrap_or((key, ""));
        let gone = || "That's already off the list.".to_string();
        match kind {
            "b" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let item = self.backlog.outstanding().into_iter().find(|i| i.id == id).cloned().ok_or_else(gone)?;
                self.backlog.dismiss(id);
                let request = item.request.trim().to_string();
                self.dropped.retain(|d| d.title != request);
                self.dropped.push(crate::daily::Dropped {
                    title: request.clone(),
                    when: now,
                    carried_for: (now.saturating_sub(item.first_seen) / 86_400) as u32,
                    about: None,
                    thinking: Vec::new(),
                });
                let kept = self.backlog.save(&self.store).and_then(|_| self.store.save("dropped", &self.dropped));
                Ok(OffTheList { title: request, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: true })
            }
            "w" => {
                let i = self.workspace.iter_mut().find(|i| i.id == rest && i.status.live()).ok_or_else(gone)?;
                i.status = crate::workspace_view::Status::Dropped;
                i.closed_at = Some(now);
                let title = i.title.clone();
                let kept = self.store.save("workspace", &self.workspace);
                Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: false })
            }
            "s" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let job = self.scheduler.jobs.iter().find(|job| job.id == id).ok_or_else(gone)?;
                if !job.in_flight && !job.interrupted { return Err(gone()); }
                if job.in_flight && job.worker_id.is_none() && job.file_move_id.is_none() {
                    return Err("That scheduled job is running without a cancellable worker; its result is not confirmed yet.".into());
                }
                let title = job.command.trim().to_string();
                let stopping = job.in_flight;
                if !self.cancel_scheduled_job(id) { return Err(gone()); }
                let kept = self.scheduler.save(&self.store);
                Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping, can_bring_back: false })
            }
            "t" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let t = self.queue.tasks.iter().find(|t| t.id == id).ok_or_else(gone)?;
                match t.state {
                    crate::lanes::TaskState::Queued | crate::lanes::TaskState::WaitingForGap => {}
                    crate::lanes::TaskState::Failed if t.interrupted => {}
                    crate::lanes::TaskState::Running => {
                        if let Some(files) = t.file_move_id {
                            let title = t.command.trim().to_string();
                            if let Some(task) = self.queue.tasks.iter_mut().find(|task| task.id == id) { task.stop_requested = true; }
                            self.stop_file_move(files);
                            let kept = self.queue.save(&self.store);
                            return Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping: true, can_bring_back: false });
                        }
                        if let Some(worker) = t.worker_id {
                            let title = t.command.trim().to_string();
                            if let Some(task) = self.queue.tasks.iter_mut().find(|task| task.id == id) { task.stop_requested = true; }
                            let stopping = self.stop_linked_worker(worker, now);
                            let kept = self.queue.save(&self.store);
                            return Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping, can_bring_back: false });
                        }
                        return Err("That one's already running, and a queued job can't be stopped part way. It'll be off the list when it finishes.".into())
                    }
                    _ => return Err(gone()),
                }
                let title = t.command.trim().to_string();
                self.queue.tasks.retain(|t| t.id != id);
                let kept = self.queue.save(&self.store);
                Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: false })
            }
            "e" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let e = self.crew.errands().into_iter().find(|e| e.id == id).ok_or_else(gone)?;
                // Not waited on: the errand stops at its next safe point and
                // its ending comes back through `settle` like any other. One
                // still waiting for a hand is simply dropped.
                let stopping = self.stop_linked_worker(id, now);
                Ok(OffTheList { title: e.name, unsaved: None, stopping, can_bring_back: false })
            }
            "c" => {
                let (id, project) = rest.split_once(':').ok_or_else(gone)?;
                let id: u64 = id.parse().map_err(|_| gone())?;
                let p = self.workshop.projects.iter_mut().find(|p| p.name == project).ok_or_else(gone)?;
                let c = p
                    .changes
                    .iter_mut()
                    .find(|c| c.id == id && c.state == crate::workshop::State::Ready)
                    .ok_or_else(gone)?;
                c.state = crate::workshop::State::Dropped;
                let title = format!("Approve \"{}\" ({})", c.title, project);
                let kept = self.workshop.save(&self.store);
                Ok(OffTheList { title, unsaved: kept.err().map(|e| e.to_string()), stopping: false, can_bring_back: false })
            }
            _ => Err(gone()),
        }
    }

    pub(crate) fn outstanding_lines(&self, now: u64) -> Vec<String> {
        let now_view = crate::workspace_view::shipped()
            .into_iter()
            .find(|v| v.name == "Now");
        let Some(view) = now_view else {
            return Vec::new();
        };
        crate::workspace_view::apply(&self.workspace, &view, now)
            .into_iter()
            .map(|i| i.title.clone())
            .collect()
    }

    pub(super) fn activity_lines(&self, now: u64) -> Vec<String> {
        let since = now.saturating_sub(24 * 3600);
        self.journal
            .since(since)
            .into_iter()
            .rev()
            .map(|e| e.what.clone())
            .collect()
    }

    pub(super) fn connection_lines(&self, now: u64) -> Vec<String> {
        self.connections
            .integrations
            .iter()
            .map(|i| i.line(now))
            .collect()
    }

    /// What would go wrong, from the three modules built for exactly this and
    /// called by nothing.
    ///
    /// `goingaway` works out what would lock him out; `codes` finds accounts
    /// with no recovery codes; `recovery` finds the vault with no way back in.
    /// All three were complete and tested and none had ever been given an
    /// account. Empty inputs are not a problem here — "you have no recovery
    /// codes for anything" is the answer, and a real one.
    pub(super) fn account_safety(&self) -> Vec<(String, String, f32)> {
        let mut out: Vec<(String, String, f32)> = Vec::new();
        let cfg = self.tools_cfg();

        for p in crate::goingaway::plan(&self.accounts.accounts) {
            out.push((
                format!("{} — {}", p.site, p.what),
                p.why.clone(),
                p.urgency,
            ));
        }

        let names: Vec<String> = self
            .accounts
            .accounts
            .iter()
            .map(|a| a.site.clone())
            .collect();
        for g in crate::codes::gaps(&self.code_sets, &names, &cfg.codes) {
            out.push((
                format!("{} — {}", g.site, g.what),
                g.url.clone().unwrap_or_else(|| {
                    "Recovery codes are the one thing that still works when \
                     everything else is locked."
                        .to_string()
                }),
                g.urgency,
            ));
        }

        for g in crate::recovery::gaps(&self.vault_recovery, &cfg.recovery, crate::store::now()) {
            out.push((g.what.clone(), g.why.clone(), g.urgency));
        }
        // Each way back in, with its weakness said plainly (Eric, B5).
        for s in &self.vault_recovery {
            out.push(("A way back into the vault".to_string(), crate::recovery::described(s), 0.0));
        }

        // Worst first, like everything else on this page.
        out.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        out
    }

    /// What the vault is holding, by name and kind only.
    ///
    /// Never a value, and never `get` — reading a secret to render a list
    /// would touch every entry's last-used date and defeat `stale()`, quite
    /// apart from putting the secrets themselves through a page.
    pub(super) fn stored_secrets(&self) -> Vec<(String, String)> {
        self.vault
            .list()
            .into_iter()
            .map(|(name, kind)| (name.to_string(), kind_word(kind).to_string()))
            .collect()
    }

    /// Does Atlas actually have this on this machine?
    ///
    /// The inventory says what *kinds* of credential exist. Whether one is
    /// really here is a different question, and answering it from the
    /// catalogue would have the access page claim Atlas holds a mail password
    /// nobody ever gave it — a page that overstates its own reach is as
    /// useless as one that understates it.
    pub(super) fn holds(&self, c: &crate::credentials::Credential) -> bool {
        match c.kept {
            crate::credentials::Kept::NotHeld => false,
            // A session exists only once there is a browser profile with
            // sites configured for it. An empty list means Atlas has never
            // been signed into anything.
            crate::credentials::Kept::YourBrowserSession => {
                !self.tools_cfg().browser.sites.is_empty()
            }
            crate::credentials::Kept::Vault => self
                .vault
                .list()
                .iter()
                .any(|(name, _)| c.name.contains(name) || name.contains(c.name)),
            crate::credentials::Kept::PlainConfig => true,
        }
    }
}

#[cfg(test)]
mod calendar_review_ui_tests {
    use super::*;
    #[test]
    fn unreadable_recovery_is_visible_and_ambiguous_post_cannot_change_it() {
        let root = std::env::temp_dir().join(format!("atlas-calendar-review-ui-{}-{}", std::process::id(), crate::store::now()));
        let store = crate::store::Store::new(&root);
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(&config, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let path = store.root().join("calendar_reminder_receipts.json");
        std::fs::write(&path, b"{broken").unwrap();
        let page = d.outstanding_page_live(crate::store::now());
        assert!(page.contains("role=alert") && page.contains("Calendar recovery records are unavailable"));
        let reply = crate::hublive::reply(&mut d, crate::server::Action::HubPost {
            path: "/hub/calendar/review".into(), fields: vec![("id".into(), "reminder:synthetic".into()), ("token".into(), "synthetic-stale-token".into()), ("what".into(), "retry".into()), ("what".into(), "dismiss".into())],
        });
        assert!(reply.body.contains("nothing"), "{}", reply.body);
        assert_eq!(std::fs::read(&path).unwrap(), b"{broken");
        drop(d);
        crate::heard!(std::fs::remove_dir_all(root));
    }
}

#[cfg(test)]
mod scheduled_status_tests {
    use super::*;
    #[test]
    fn live_worker_is_running_and_restart_requires_review_without_retry() {
        let root = std::env::temp_dir().join(format!("atlas-scheduled-projection-{}-{}", std::process::id(), crate::store::now()));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let cfg = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor { id: 1, x: 0, y: 0, width: 1280, height: 800, primary: true }]);
        let mut d = Daemon::new(&cfg, &platform, None, store.clone(), crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()));
        let id = d.scheduler.every("prepare the report", 60, 1);
        d.scheduler.start_durably(id, &store).unwrap();
        d.scheduler.jobs[0].worker_id = Some(98765);
        d.scheduler.save(&store).unwrap();
        let (needs_you, day) = crate::brief::from_jobs(&d.scheduler, 2);
        assert!(needs_you.is_empty(), "live work was presented as failed");
        assert!(day.iter().any(|item| item.what.starts_with("Running:")));
        let open = d.open_work(2);
        assert!(open.in_progress.iter().any(|(what, status)| what == &sentence("prepare the report") && status.contains("Running now")));
        assert!(open.drops.in_progress.iter().any(|key| key.as_deref() == Some(format!("s:{id}").as_str())));
        assert!(!open.blocked.iter().any(|item| item.what == sentence("prepare the report")));
        d.scheduler = crate::scheduler::Scheduler::load(&store);
        let prior = d.scheduler.jobs[0].last_result.clone();
        let (needs_you, day) = crate::brief::from_jobs(&d.scheduler, 3);
        assert!(day.is_empty());
        assert!(needs_you.iter().any(|item| item.subject.contains("Check the previous result before retrying") && item.conflicts_with.as_deref().is_some_and(|text| text.contains("unconfirmed"))));
        let open = d.open_work(3);
        assert!(open.blocked.iter().any(|item| item.what == sentence("prepare the report") && item.stopped.contains("unconfirmed")));
        assert!(open.drops.blocked.iter().any(|key| key.as_deref() == Some(format!("s:{id}").as_str())));
        assert!(!open.in_progress.iter().any(|(what, _)| what == &sentence("prepare the report")));
        assert!(d.scheduler.due(1000).is_empty());
        let result = d.drop_outstanding(&format!("s:{id}"), 4).unwrap();
        assert!(!result.stopping && result.unsaved.is_none());
        assert_eq!(d.scheduler.jobs[0].state, crate::scheduler::JobState::Cancelled);
        assert!(!d.scheduler.jobs[0].interrupted);
        assert_eq!(d.scheduler.jobs[0].last_result, prior);
        let restored = crate::scheduler::Scheduler::load(&store);
        assert!(restored.due(1000).is_empty());
    }
}
