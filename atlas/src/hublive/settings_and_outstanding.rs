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
        if let Some(at) = html.rfind("</main>") {
            html.insert_str(at, &review);
        }
        html
    }

    /// How many things are in each state, from `open_work`'s lanes and
    /// nothing else (see `hub::Counts`).
    pub fn counts(&self, now: u64) -> hub::Counts {
        hub::Counts::of(&self.open_work(now))
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
        let sender = groups
            .held
            .values()
            .map(|h| &h.state)
            .find(|g| g.release_channel)
            .map(|c| c.owner.clone());
        let me = crate::peerkey::Identity::load_or_create(&self.peer_dir)
            .ok()
            .map(|i| i.public())
            .unwrap_or_default();
        match sender {
            Some(owner) if owner != me && !me.is_empty() => {
                let to = crate::kin::Pairings::load(&self.peer_dir).name_of_key(&owner);
                match to {
                    Some(to) => {
                        if let Err(e) = crate::feedback::queue_feedback(&self.store, f, &to) {
                            return format!("Not sent: {e}.");
                        }
                        format!(
                            "Sent to {to}. They'll see exactly what you wrote, and nothing else."
                        )
                    }
                    None => "I can't reach whoever gave you Atlas, so it's kept here for now."
                        .to_string(),
                }
            }
            _ => {
                // unheard-ok: returns `Option<String>`, not a Result
                let _ = crate::feedback::heard_feedback(
                    &self.store,
                    "you",
                    &serde_json::to_string(&f).unwrap_or_default(),
                );
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
                (!n.trim().is_empty()
                    && n.chars().next().map(|c| c.is_uppercase()).unwrap_or(false))
                .then_some(n)
            }
            _ => None,
        };
        hub::with_owner(html, name.as_deref())
    }

    /// Outstanding, in the design's lanes: Waiting on you, Blocked, Failed,
    /// Interrupted, In progress, Carried over and Done today.
    ///
    /// The one place that works out what is open (20 Oct 2026). Everything
    /// that says how many things are in a state -- the header pill, the
    /// sidebar count, Home's "to act", Home's brief and "Nothing underway",
    /// the lane headings -- reads `hub::Counts::of` this, so they cannot
    /// disagree. It reads memory plus one cached durable-job file
    /// (`job::load_cached`), so it is cheap enough for the pill on every page.
    pub(super) fn open_work(&self, now: u64) -> hub::Open {
        /// How long a failed scheduled job stays in the Failed lane: this page
        /// cannot drop it, so it ages out. Everything else that failed stays
        /// until dismissed.
        const RECENT_SECS: u64 = 24 * 3600;
        /// What an interrupted item says about itself, so nobody reads a
        /// restart as a failure or as a success.
        const CUT_OFF: &str = "Cut off by a restart — not known to have finished.";
        /// How many of today's finished things the Done lane shows.
        const DONE_SHOWN: usize = 10;
        let clock = crate::store::now();
        let mut failed: Vec<hub::Ended> = Vec::new();
        let mut interrupted: Vec<hub::Ended> = Vec::new();
        let mut done: Vec<hub::Ended> = Vec::new();
        let mut drops = hub::Drops::default();
        let mut waiting: Vec<(String, String, String)> = Vec::new();
        for (what, href, key) in self.home_asks_keyed(now) {
            waiting.push((what, String::new(), href));
            drops.waiting.push(key);
        }
        // A question or a yes it's waiting on carries its reason with it.
        for w in waiting.iter_mut() {
            if let Some(i) = self
                .backlog
                .items
                .iter()
                .find(|i| !i.done && !i.dismissed && sentence(&i.request) == w.0)
            {
                w.1 = sentence(&i.blocker.explain());
            }
        }
        let mut blocked: Vec<hub::Stopped> = Vec::new();
        for i in self
            .backlog
            .items
            .iter()
            .filter(|i| !i.done && !i.dismissed && !waits_on_you(&i.blocker))
        {
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
        let mut queued: Vec<(String, String)> = Vec::new();
        for job in &self.scheduler.jobs {
            if job.in_flight {
                let status = if job.state == crate::scheduler::JobState::Cancelled {
                    "Stopping; its result is not confirmed yet."
                } else {
                    "Running now; its result is not confirmed yet."
                };
                in_progress.push((sentence(&job.command), status.into()));
                drops.in_progress.push(
                    (job.worker_id.is_some() || job.file_move_id.is_some())
                        .then(|| format!("s:{}", job.id)),
                );
            } else if job.interrupted {
                // Cut off by a restart: its own lane (20 Oct 2026), said as
                // what it is rather than as something Atlas is stuck on.
                interrupted.push(hub::Ended {
                    what: sentence(&job.command),
                    detail: format!(
                        "{} {CUT_OFF}",
                        job.last_result
                            .clone()
                            .unwrap_or_else(|| "Previous outcome is unconfirmed.".into())
                    ),
                    when: String::new(),
                    at: job.due,
                });
                drops.interrupted.push(Some(format!("s:{}", job.id)));
            } else if job.state == crate::scheduler::JobState::Failed
                && job.due.saturating_add(RECENT_SECS) >= clock
            {
                failed.push(hub::Ended {
                    what: sentence(&job.command),
                    detail: job
                        .last_result
                        .clone()
                        .unwrap_or_else(|| "It failed and said nothing more.".into()),
                    when: crate::localclock::hhmm_here(job.due),
                    at: job.due,
                });
                // A scheduled job is not taken off a list by this page: that
                // would cancel its schedule. It leaves after a day.
                drops.failed.push(None);
            }
        }
        for task in self.queue.tasks.iter().filter(|task| task.interrupted) {
            interrupted.push(hub::Ended {
                what: sentence(&task.command),
                detail: format!(
                    "{} {CUT_OFF}",
                    task.result
                        .clone()
                        .unwrap_or_else(|| "Previous outcome not confirmed.".into())
                ),
                when: String::new(),
                at: task.created,
            });
            drops.interrupted.push(Some(format!("t:{}", task.id)));
        }
        // A failed task stays until it is dropped (21 Oct 2026); only a
        // scheduled job, which this page cannot take off, still ages out.
        for task in self
            .queue
            .tasks
            .iter()
            .filter(|task| task.state == crate::lanes::TaskState::Failed && !task.interrupted)
        {
            failed.push(hub::Ended {
                what: sentence(&task.command),
                detail: task
                    .result
                    .clone()
                    .unwrap_or_else(|| "It failed and said nothing more.".into()),
                when: crate::localclock::hhmm_here(task.created),
                at: task.created,
            });
            drops.failed.push(Some(format!("t:{}", task.id)));
        }
        let today = crate::localclock::midnight(clock, crate::localclock::offset_secs());
        for task in self
            .queue
            .tasks
            .iter()
            .filter(|task| task.state == crate::lanes::TaskState::Done && task.created >= today)
        {
            done.push(hub::Ended {
                what: sentence(&task.command),
                detail: task.result.clone().unwrap_or_default(),
                when: crate::localclock::hhmm_here(task.created),
                at: task.created,
            });
        }
        // Durable job receipts (`job`). Work cut off by a restart is
        // `Uncertain`: it has no safe automatic retry, so it sits in
        // Interrupted until the owner has checked the real-world outcome and
        // dismisses it (20 Oct 2026: it now has a Drop it button; before, it
        // could never leave the page). A job that ended in failure in the last
        // day is in Failed, one that finished today is in Done, and one that
        // has gone quiet for a while without ending is Blocked.
        const STALE_AFTER_SECS: u64 = 15 * 60;
        let now = clock;
        // Errands the crew still has in hand: their `errand:` records are
        // Running because they are, so they are neither stale nor lost.
        let live_errands: std::collections::HashSet<String> =
            self.live_errand_jobs().into_iter().collect();
        let durable_jobs = match crate::job::load_cached(&self.store) {
            Ok(jobs) => jobs,
            Err(error) => {
                blocked.push(hub::Stopped {
                    what: "durable job journal".into(),
                    tried: "Read the saved job receipts.".into(),
                    stopped: format!("The saved job state could not be read: {error}"),
                    needs: "Inspect the preserved job file before asking Atlas to retry work."
                        .into(),
                    area: None,
                });
                drops.blocked.push(None);
                Vec::new()
            }
        };
        for job in durable_jobs {
            use crate::job::State as S;
            // Atlas working on itself is written down so it ends as done,
            // failed or uncertain like anything else, but it is not the
            // owner's to look at (21 Oct 2026): no lane shows it.
            if job.internal {
                continue;
            }
            let name = durable_name(&job);
            let said = job.receipt.as_ref().map(|r| r.detail.clone());
            let artifact_note = job
                .artifacts
                .as_ref()
                .map(|envelope| {
                    format!(
                        " Artifact edges recorded: {} input, {} output, {} dependencies.",
                        envelope.input.len(),
                        envelope.output.len(),
                        envelope.depends_on.len()
                    )
                })
                .unwrap_or_default();
            match job.state {
                S::Uncertain if !job.dismissed => {
                    interrupted.push(hub::Ended {
                        what: name,
                        detail: format!(
                            "{} {CUT_OFF}{artifact_note}",
                            said.unwrap_or_else(|| "The previous outcome is uncertain.".into())
                        ),
                        when: crate::localclock::hhmm_here(job.updated_at),
                        at: job.updated_at,
                    });
                    drops.interrupted.push(Some(format!("j:{}", job.job_id)));
                }
                // A failure stays until it is dismissed (21 Oct 2026), however
                // old: a request that failed overnight is still one the owner
                // has not looked at. Its time is shown beside it.
                S::Failed if !job.dismissed => {
                    failed.push(hub::Ended {
                        what: name,
                        detail: format!(
                            "{}{artifact_note}",
                            said.unwrap_or_else(|| "It failed and said nothing more.".into())
                        ),
                        when: crate::localclock::hhmm_here(job.updated_at),
                        at: job.updated_at,
                    });
                    drops.failed.push(Some(format!("j:{}", job.job_id)));
                }
                S::Succeeded if !job.dismissed && job.updated_at >= today => {
                    done.push(hub::Ended {
                        what: name,
                        detail: said.unwrap_or_default(),
                        when: crate::localclock::hhmm_here(job.updated_at),
                        at: job.updated_at,
                    });
                }
                // Waiting on the owner's yes or no (21 Oct 2026): its own lane,
                // not Blocked after fifteen quiet minutes. A saved approval is
                // re-asked after a restart, never replayed, so it stays here
                // until it is answered.
                S::AwaitingApproval => {
                    waiting.push((
                        name,
                        job.checkpoint
                            .clone()
                            .or(said.clone())
                            .unwrap_or_else(|| "Asked, and not answered yet.".into()),
                        hub::Page::Outstanding.href().to_string(),
                    ));
                    // A saved approval can be dropped; that closes it (`job::dismiss`).
                    drops.waiting.push(
                        job.job_id
                            .starts_with(crate::job::APPROVAL_PREFIX)
                            .then(|| format!("j:{}", job.job_id)),
                    );
                }
                // Heavy self-repair work (`heavywork`): running, or paused and
                // waiting for you to be away. It writes its record every
                // minute, so it only falls through to Blocked if Atlas died.
                // Only a Running record is in progress; one waiting or paused
                // (Checkpointed) is queued, with its reason (22 Oct 2026).
                s if !s.terminal()
                    && job.job_id.starts_with(crate::heavywork::JOB_PREFIX)
                    && !job.stale(now, STALE_AFTER_SECS) =>
                {
                    if s == S::Running {
                        in_progress.push((
                            name,
                            job.checkpoint
                                .clone()
                                .unwrap_or_else(|| "Running while you're away.".into()),
                        ));
                        drops.in_progress.push(None);
                    } else {
                        queued.push((
                            name,
                            job.checkpoint
                                .clone()
                                .unwrap_or_else(|| "Waits until you step away.".into()),
                        ));
                        drops.queued.push(None);
                    }
                }
                s if !s.terminal()
                    && job.stale(now, STALE_AFTER_SECS)
                    && !live_errands.contains(&job.job_id) =>
                {
                    blocked.push(hub::Stopped {
                        what: format!("{} job ({})", job.stage, job.job_id),
                        tried: format!("Started the {} job.", job.stage),
                        stopped: said
                            .map(|d| format!("{d}{artifact_note}"))
                            .unwrap_or_else(|| {
                                format!(
                                    "No durable progress has been recorded recently.{artifact_note}"
                                )
                            }),
                        needs: "Check the worker before asking Atlas to try it again; it may still be running."
                            .into(),
                        area: None,
                    });
                    drops.blocked.push(None);
                }
                _ => {}
            }
        }
        // A hub button's job that failed (`hubjobs`), including one a restart
        // cut off, which comes back as a failure with its own explanation.
        for (_id, label, why, ended) in self.hub_jobs.failures() {
            failed.push(hub::Ended {
                what: sentence(&label),
                detail: why,
                when: ended.map(crate::localclock::hhmm_here).unwrap_or_default(),
                at: ended.unwrap_or(clock),
            });
            // Not droppable here: the page that started it already showed it,
            // and it leaves by itself.
            drops.failed.push(None);
        }
        for t in self.queue.tasks.iter().filter(|t| {
            !matches!(
                t.state,
                crate::lanes::TaskState::Done | crate::lanes::TaskState::Failed
            )
        }) {
            let drop = (t.state != crate::lanes::TaskState::Running
                || t.worker_id.is_some()
                || t.file_move_id.is_some())
            .then(|| format!("t:{}", t.id));
            if t.state == crate::lanes::TaskState::Running {
                let how = if t.stop_requested {
                    "Stopping; the worker has not finished yet."
                } else {
                    "Running now."
                };
                in_progress.push((sentence(&t.command), how.to_string()));
                drops.in_progress.push(drop);
            } else {
                // Not started yet: queued, never "in progress" (22 Oct 2026).
                let why = if t.state == crate::lanes::TaskState::WaitingForGap {
                    "Waiting for a pause in your work."
                } else if t.needs_net {
                    "Waiting for an internet connection."
                } else {
                    "Next in line."
                };
                queued.push((sentence(&t.command), why.to_string()));
                drops.queued.push(drop);
            }
        }
        for e in self.crew.errands() {
            in_progress.push((
                e.name.clone(),
                "Handed to a worker — I check what comes back before you see it.".into(),
            ));
            drops.in_progress.push(Some(format!("e:{}", e.id)));
        }
        let mut carried: Vec<(String, u64)> = Vec::new();
        for i in self
            .workspace
            .iter()
            .filter(|i| i.at < today && i.status.live())
        {
            carried.push((i.title.clone(), ((today - i.at) / 86_400).max(1)));
            drops.carried.push(Some(format!("w:{}", i.id)));
        }
        // Failed, newest first, each with its own Drop it key kept beside it.
        let mut keyed: Vec<(hub::Ended, Option<String>)> = failed
            .into_iter()
            .enumerate()
            .map(|(i, e)| (e, drops.failed.get(i).cloned().flatten()))
            .collect();
        keyed.sort_by_key(|k| std::cmp::Reverse(k.0.at));
        let (failed, failed_keys): (Vec<_>, Vec<_>) = keyed.into_iter().unzip();
        drops.failed = failed_keys;
        // Done, newest first, a handful: it is a receipt for today, not a log.
        done.sort_by_key(|d| std::cmp::Reverse(d.at));
        done.truncate(DONE_SHOWN);
        // What Atlas is thinking through for you (`mind`), when no other
        // source already shows it: until 22 Oct 2026 only the Now page read
        // this, so Home and Outstanding could say nothing was underway while
        // Now showed a task, and the other way round.
        for w in self.mind.work.iter().filter(|w| !w.stage.finished()) {
            let what = sentence(&w.asked);
            if in_progress
                .iter()
                .chain(queued.iter())
                .any(|(t, _)| *t == what)
            {
                continue;
            }
            if w.stage == crate::mind::Stage::Waiting {
                let why = w
                    .blocked_on
                    .as_deref()
                    .map(sentence)
                    .unwrap_or_else(|| "Waiting on something outside my control.".into());
                queued.push((what, why));
                drops.queued.push(None);
            } else {
                let step = w
                    .thoughts
                    .last()
                    .map(|t| sentence(&t.text))
                    .unwrap_or_default();
                let stage = sentence(w.stage.label());
                let how = if step.is_empty() {
                    stage
                } else {
                    format!("{}: {step}", stage.trim_end_matches('.'))
                };
                in_progress.push((what, how));
                drops.in_progress.push(None);
            }
        }
        hub::Open {
            waiting,
            blocked,
            in_progress,
            queued,
            carried,
            drops,
            failed,
            interrupted,
            done,
        }
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
            .chain(o.carried.iter().map(|c| c.0.clone()))
            .chain(o.failed.iter().map(|f| f.what.clone()))
            .chain(o.interrupted.iter().map(|f| f.what.clone()));
        let pad = |v: &Vec<Option<String>>, n: usize| {
            (0..n)
                .map(|i| v.get(i).cloned().flatten())
                .collect::<Vec<_>>()
        };
        let keys = pad(&o.drops.waiting, o.waiting.len())
            .into_iter()
            .chain(pad(&o.drops.blocked, o.blocked.len()))
            .chain(pad(&o.drops.in_progress, o.in_progress.len()))
            .chain(pad(&o.drops.carried, o.carried.len()))
            .chain(pad(&o.drops.failed, o.failed.len()))
            .chain(pad(&o.drops.interrupted, o.interrupted.len()));
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
                let item = self
                    .backlog
                    .outstanding()
                    .into_iter()
                    .find(|i| i.id == id)
                    .cloned()
                    .ok_or_else(gone)?;
                self.backlog.dismiss(id);
                // Dropping the download ask is a no to the pieces it named.
                self.decline_what_was_asked(&item.request, now);
                let request = item.request.trim().to_string();
                self.dropped.retain(|d| d.title != request);
                self.dropped.push(crate::daily::Dropped {
                    title: request.clone(),
                    when: now,
                    carried_for: u32::try_from(now.saturating_sub(item.first_seen) / 86_400)
                        .unwrap_or(u32::MAX),
                    about: None,
                    thinking: Vec::new(),
                });
                let kept = self
                    .backlog
                    .save(&self.store)
                    .and_then(|_| self.store.save("dropped", &self.dropped));
                Ok(OffTheList {
                    title: request,
                    unsaved: kept.err().map(|e| e.to_string()),
                    stopping: false,
                    can_bring_back: true,
                })
            }
            "w" => {
                let i = self
                    .workspace
                    .iter_mut()
                    .find(|i| i.id == rest && i.status.live())
                    .ok_or_else(gone)?;
                i.status = crate::workspace_view::Status::Dropped;
                i.closed_at = Some(now);
                let title = i.title.clone();
                let kept = self.store.save("workspace", &self.workspace);
                Ok(OffTheList {
                    title,
                    unsaved: kept.err().map(|e| e.to_string()),
                    stopping: false,
                    can_bring_back: false,
                })
            }
            "s" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let job = self
                    .scheduler
                    .jobs
                    .iter()
                    .find(|job| job.id == id)
                    .ok_or_else(gone)?;
                if !job.in_flight && !job.interrupted {
                    return Err(gone());
                }
                if job.in_flight && job.worker_id.is_none() && job.file_move_id.is_none() {
                    return Err("That scheduled job is running without a cancellable worker; its result is not confirmed yet.".into());
                }
                let title = job.command.trim().to_string();
                let stopping = job.in_flight;
                if !self.cancel_scheduled_job(id) {
                    return Err(gone());
                }
                let kept = self.scheduler.save(&self.store);
                Ok(OffTheList {
                    title,
                    unsaved: kept.err().map(|e| e.to_string()),
                    stopping,
                    can_bring_back: false,
                })
            }
            "t" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let t = self
                    .queue
                    .tasks
                    .iter()
                    .find(|t| t.id == id)
                    .ok_or_else(gone)?;
                match t.state {
                    crate::lanes::TaskState::Queued | crate::lanes::TaskState::WaitingForGap => {}
                    // An ended-badly request leaves the Failed lane by being
                    // taken out of the queue, like an interrupted one.
                    crate::lanes::TaskState::Failed => {}
                    crate::lanes::TaskState::Running => {
                        if let Some(files) = t.file_move_id {
                            let title = t.command.trim().to_string();
                            if let Some(task) =
                                self.queue.tasks.iter_mut().find(|task| task.id == id)
                            {
                                task.stop_requested = true;
                            }
                            self.stop_file_move(files);
                            let kept = self.queue.save(&self.store);
                            return Ok(OffTheList {
                                title,
                                unsaved: kept.err().map(|e| e.to_string()),
                                stopping: true,
                                can_bring_back: false,
                            });
                        }
                        if let Some(worker) = t.worker_id {
                            let title = t.command.trim().to_string();
                            if let Some(task) =
                                self.queue.tasks.iter_mut().find(|task| task.id == id)
                            {
                                task.stop_requested = true;
                            }
                            let stopping = self.stop_linked_worker(worker, now);
                            let kept = self.queue.save(&self.store);
                            return Ok(OffTheList {
                                title,
                                unsaved: kept.err().map(|e| e.to_string()),
                                stopping,
                                can_bring_back: false,
                            });
                        }
                        return Err("That one's already running, and a queued job can't be stopped part way. It'll be off the list when it finishes.".into());
                    }
                    _ => return Err(gone()),
                }
                let title = t.command.trim().to_string();
                self.queue.tasks.retain(|t| t.id != id);
                let kept = self.queue.save(&self.store);
                Ok(OffTheList {
                    title,
                    unsaved: kept.err().map(|e| e.to_string()),
                    stopping: false,
                    can_bring_back: false,
                })
            }
            "e" => {
                let id: u64 = rest.parse().map_err(|_| gone())?;
                let e = self
                    .crew
                    .errands()
                    .into_iter()
                    .find(|e| e.id == id)
                    .ok_or_else(gone)?;
                // Not waited on: the errand stops at its next safe point and
                // its ending comes back through `settle` like any other. One
                // still waiting for a hand is simply dropped.
                let stopping = self.stop_linked_worker(id, now);
                Ok(OffTheList {
                    title: e.name,
                    unsaved: None,
                    stopping,
                    can_bring_back: false,
                })
            }
            "j" => {
                // An ended durable job (`job`), dismissed rather than deleted.
                let title = crate::job::load_cached(&self.store)
                    .ok()
                    .and_then(|jobs| jobs.into_iter().find(|j| j.job_id == rest))
                    .map(|j| durable_name(&j))
                    .ok_or_else(gone)?;
                // 22 Oct 2026: a saved approval is only a record of a question
                // that is still waiting in the session or the scheduler, and
                // `persist` writes the record again from them. Dismissing the
                // record alone brought it back; the question goes too.
                if rest.starts_with(crate::job::APPROVAL_PREFIX) {
                    let key = rest.rsplit_once(':').map(|(k, _)| k).unwrap_or(rest);
                    let waiting = self.session.all_approvals();
                    let keep: Vec<_> = waiting
                        .iter()
                        .filter(|(_, d)| crate::job::approval_key("chat", d) != key)
                        .cloned()
                        .collect();
                    if keep.len() != waiting.len() {
                        self.session.drop_approvals();
                        for (i, d) in keep {
                            self.session.await_approval(i, &d);
                        }
                    }
                    let parked: Vec<u64> = self
                        .scheduler
                        .jobs
                        .iter()
                        .filter(|j| j.state == crate::scheduler::JobState::AwaitingApproval)
                        .filter(|j| {
                            crate::job::approval_key("sched", &format!("{}:{}", j.id, j.command))
                                == key
                        })
                        .map(|j| j.id)
                        .collect();
                    for id in parked {
                        self.scheduler.cancel(id);
                    }
                }
                let kept = crate::job::dismiss(&self.store, rest);
                match kept {
                    Ok(true) => Ok(OffTheList {
                        title,
                        unsaved: None,
                        stopping: false,
                        can_bring_back: false,
                    }),
                    Ok(false) => Err(gone()),
                    Err(e) => Ok(OffTheList {
                        title,
                        unsaved: Some(e.to_string()),
                        stopping: false,
                        can_bring_back: false,
                    }),
                }
            }
            "c" => {
                let (id, project) = rest.split_once(':').ok_or_else(gone)?;
                let id: u64 = id.parse().map_err(|_| gone())?;
                let p = self
                    .workshop
                    .projects
                    .iter_mut()
                    .find(|p| p.name == project)
                    .ok_or_else(gone)?;
                let c = p
                    .changes
                    .iter_mut()
                    .find(|c| c.id == id && c.state == crate::workshop::State::Ready)
                    .ok_or_else(gone)?;
                c.state = crate::workshop::State::Dropped;
                let title = format!("Approve \"{}\" ({})", c.title, project);
                let kept = self.workshop.save(&self.store);
                Ok(OffTheList {
                    title,
                    unsaved: kept.err().map(|e| e.to_string()),
                    stopping: false,
                    can_bring_back: false,
                })
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

    /// The last day of what Atlas did without being watched, newest first,
    /// each with its state (from the entry's kind and outcome) and its time.
    /// Atlas's own work on its own code (`Kind::SelfWork`) is left out: it is
    /// kept in the journal but is not the owner's business.
    pub(super) fn activity_lines(&self, now: u64) -> Vec<(hub::ActivityState, String, String)> {
        let since = now.saturating_sub(24 * 3600);
        self.journal
            .since(since)
            .into_iter()
            .rev()
            .filter(|e| !e.kind.internal())
            .map(|e| {
                (
                    hub::activity_state(e.kind, e.ok, &e.what),
                    e.what.clone(),
                    crate::localclock::hhmm_here(e.at),
                )
            })
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
            out.push((format!("{} — {}", p.site, p.what), p.why.clone(), p.urgency));
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
            out.push((
                "A way back into the vault".to_string(),
                crate::recovery::described(s),
                0.0,
            ));
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

/// What a durable job is called on the page: an errand's own words, or its
/// stage with "job" after it. The job number is for the receipt, not for you.
fn durable_name(job: &crate::job::Record) -> String {
    if job.job_id.starts_with(crate::job::ERRAND_PREFIX)
        || job.job_id.starts_with(crate::job::APPROVAL_PREFIX)
        || job.job_id.starts_with("chat:")
        || job.job_id.starts_with("spoken:")
        || job.job_id.starts_with(crate::heavywork::JOB_PREFIX)
    {
        job.stage.clone()
    } else {
        format!("{} job", job.stage)
    }
}

#[cfg(test)]
mod calendar_review_ui_tests {
    use super::*;
    #[test]
    fn unreadable_recovery_is_visible_and_ambiguous_post_cannot_change_it() {
        let root = std::env::temp_dir().join(format!(
            "atlas-calendar-review-ui-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        let store = crate::store::Store::new(&root);
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(
            &config,
            &platform,
            None,
            store.clone(),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let path = store.root().join("calendar_reminder_receipts.json");
        std::fs::write(&path, b"{broken").unwrap();
        let page = d.outstanding_page_live(crate::store::now());
        assert!(
            page.contains("role=alert")
                && page.contains("Calendar recovery records are unavailable")
        );
        let reply = crate::hublive::reply(
            &mut d,
            crate::server::Action::HubPost {
                path: "/hub/calendar/review".into(),
                fields: vec![
                    ("id".into(), "reminder:synthetic".into()),
                    ("token".into(), "synthetic-stale-token".into()),
                    ("what".into(), "retry".into()),
                    ("what".into(), "dismiss".into()),
                ],
            },
        );
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
        let root = std::env::temp_dir().join(format!(
            "atlas-scheduled-projection-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let cfg = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(vec![crate::platform::Monitor {
            id: 1,
            x: 0,
            y: 0,
            width: 1280,
            height: 800,
            primary: true,
        }]);
        let mut d = Daemon::new(
            &cfg,
            &platform,
            None,
            store.clone(),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let id = d.scheduler.every("prepare the report", 60, 1);
        d.scheduler.start_durably(id, &store).unwrap();
        d.scheduler.jobs[0].worker_id = Some(98765);
        d.scheduler.save(&store).unwrap();
        let (needs_you, day) = crate::brief::from_jobs(&d.scheduler, 2);
        assert!(needs_you.is_empty(), "live work was presented as failed");
        assert!(day.iter().any(|item| item.what.starts_with("Running:")));
        let open = d.open_work(2);
        assert!(open
            .in_progress
            .iter()
            .any(|(what, status)| what == &sentence("prepare the report")
                && status.contains("Running now")));
        assert!(open
            .drops
            .in_progress
            .iter()
            .any(|key| key.as_deref() == Some(format!("s:{id}").as_str())));
        assert!(!open
            .blocked
            .iter()
            .any(|item| item.what == sentence("prepare the report")));
        d.scheduler = crate::scheduler::Scheduler::load(&store);
        let prior = d.scheduler.jobs[0].last_result.clone();
        let (needs_you, day) = crate::brief::from_jobs(&d.scheduler, 3);
        assert!(day.is_empty());
        assert!(needs_you.iter().any(|item| item
            .subject
            .contains("Check the previous result before retrying")
            && item
                .conflicts_with
                .as_deref()
                .is_some_and(|text| text.contains("unconfirmed"))));
        let open = d.open_work(3);
        // 20 Oct 2026: work a restart cut off is in its own Interrupted lane,
        // not Blocked: nothing is stuck, nobody knows how far it got.
        assert!(open
            .interrupted
            .iter()
            .any(|item| item.what == sentence("prepare the report")
                && item.detail.contains("unconfirmed")
                && item.detail.contains("Cut off by a restart")));
        assert!(!open
            .blocked
            .iter()
            .any(|item| item.what == sentence("prepare the report")));
        assert!(open
            .drops
            .interrupted
            .iter()
            .any(|key| key.as_deref() == Some(format!("s:{id}").as_str())));
        assert!(!open
            .in_progress
            .iter()
            .any(|(what, _)| what == &sentence("prepare the report")));
        assert!(d.scheduler.due(1000).is_empty());
        let result = d.drop_outstanding(&format!("s:{id}"), 4).unwrap();
        assert!(!result.stopping && result.unsaved.is_none());
        assert_eq!(
            d.scheduler.jobs[0].state,
            crate::scheduler::JobState::Cancelled
        );
        assert!(!d.scheduler.jobs[0].interrupted);
        assert_eq!(d.scheduler.jobs[0].last_result, prior);
        let restored = crate::scheduler::Scheduler::load(&store);
        assert!(restored.due(1000).is_empty());
    }
}

/// One source for the counts (20 Oct 2026): the header pill, Home and
/// Outstanding read the same lanes, and a restart's cut-off work is in them.
#[cfg(test)]
mod one_count_tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    fn scratch(tag: &str) -> crate::store::Store {
        let root = std::env::temp_dir().join(format!(
            "atlas-one-count-{tag}-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        let _ = std::fs::remove_dir_all(&root);
        crate::store::Store::new(&root)
    }

    #[test]
    fn the_pill_home_and_outstanding_agree_on_blocked_running_and_failed() {
        let store = scratch("agree");
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(
            &config,
            &platform,
            None,
            store.clone(),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let now = crate::store::now();
        // One backlog item Atlas is stuck on (not a question for you).
        d.backlog.record(
            "fetch the invoices",
            crate::backlog::Blocker::Failed("the site is down".into()),
            now,
        );
        // One running download, in the crew's hands.
        let release = Arc::new(AtomicBool::new(false));
        let hold = release.clone();
        d.crew
            .hand(
                "getting-everything",
                now,
                Box::new(move |_| {
                    while !hold.load(Ordering::SeqCst) {
                        std::thread::sleep(std::time::Duration::from_millis(5));
                    }
                    Ok("done".into())
                }),
            )
            .unwrap();
        // One backup that failed, as `take_crew_news` writes it down.
        crate::job::end(
            &store,
            "errand:backup:1:1",
            "Backup",
            crate::job::State::Failed,
            "the file is locked by another process",
        )
        .unwrap();

        let o = d.open_work(now);
        let c = d.counts(now);
        assert_eq!(c, hub::Counts::of(&o));
        assert_eq!(
            (c.waiting, c.blocked, c.running, c.failed, c.interrupted),
            (0, 1, 1, 1, 0)
        );

        // The pill: nothing is waiting on YOU, and it says what stopped.
        assert_eq!(d.waiting_count(now), 0);
        assert_eq!(
            d.pill_counts(now),
            (0, "1 blocked · 1 failed or cut off".to_string())
        );
        let (w, extra) = d.pill_counts(now);
        let page = hub::with_counts(d.outstanding_page_live(now), w, &extra);
        assert!(
            page.contains("Nothing waiting · 1 blocked · 1 failed or cut off</a>"),
            "{page}"
        );

        // Outstanding's headings are the same numbers.
        for (lane, n) in [
            ("Waiting on you", 0),
            ("Blocked", 1),
            ("In progress", 1),
            ("Failed", 1),
            ("Interrupted", 0),
        ] {
            assert!(
                page.contains(&format!("{lane} <span class=n>{n}</span>")),
                "{lane} should read {n}: {page}"
            );
        }
        assert!(page.contains("the file is locked by another process"));

        // Home: not "quiet", and not "Nothing underway" beside a download.
        let deck = d.deck(now, 0);
        assert_ne!(deck.now, "Nothing underway.", "a download is running");
        assert!(deck.now.contains("getting-everything"), "{}", deck.now);
        assert!(!deck.brief.contains("It's quiet"), "{}", deck.brief);
        assert!(
            deck.brief
                .contains("2 things have stopped: 1 blocked, 1 failed"),
            "{}",
            deck.brief
        );
        assert!(deck.asks.is_empty());
        let card = d.card_outstanding(now);
        assert!(card.contains("<b>0</b><span>To act</span>"), "{card}");
        assert!(card.contains("<b>1</b><span>Blocked</span>"), "{card}");
        assert!(card.contains("Fetch the invoices"), "{card}");

        release.store(true, Ordering::SeqCst);
        d.errands_done_for_test();
    }

    #[test]
    fn what_waits_on_you_is_the_same_number_everywhere_and_a_blocked_item_is_not_in_it() {
        let store = scratch("waits");
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(
            &config,
            &platform,
            None,
            store,
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let now = crate::store::now();
        d.backlog.record(
            "send the contract",
            crate::backlog::Blocker::NeedsApproval,
            now,
        );
        d.backlog.record(
            "fetch the invoices",
            crate::backlog::Blocker::Failed("the site is down".into()),
            now,
        );
        assert_eq!(d.waiting_count(now), 1, "the pill counts the Waiting lane");
        let deck = d.deck(now, 0);
        assert_eq!(deck.asks.len(), 1);
        assert!(
            deck.brief.starts_with("One thing wants you."),
            "{}",
            deck.brief
        );
        let page = d.outstanding_page_live(now);
        assert!(
            page.contains("Waiting on you <span class=n>1</span>"),
            "{page}"
        );
        assert!(page.contains("Blocked <span class=n>1</span>"), "{page}");
        let card = d.card_outstanding(now);
        assert!(card.contains("<b>1</b><span>To act</span>"), "{card}");
    }

    #[test]
    fn a_running_job_record_is_interrupted_after_a_restart_and_shown_as_cut_off() {
        let store = scratch("restart");
        // The previous run: a download began and the process died.
        crate::job::begin(
            &store,
            "errand:getting-everything:1:1",
            "Download of what Atlas still needs",
            "started",
        )
        .unwrap();
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        // Making the Daemon is the start-up: the reconciliation is saved
        // before any page can be drawn.
        let d = Daemon::new(
            &config,
            &platform,
            None,
            store.clone(),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let saved = crate::job::load_checked(&store).unwrap();
        assert_eq!(saved[0].state, crate::job::State::Uncertain);
        let now = crate::store::now();
        let o = d.open_work(now);
        assert_eq!(o.interrupted.len(), 1);
        assert_eq!(o.interrupted[0].what, "Download of what Atlas still needs");
        assert!(
            o.interrupted[0]
                .detail
                .contains("Cut off by a restart — not known to have finished."),
            "{}",
            o.interrupted[0].detail
        );
        assert_eq!(
            o.drops.interrupted,
            vec![Some("j:errand:getting-everything:1:1".to_string())]
        );
        assert!(o.failed.is_empty() && o.blocked.is_empty());
        assert_eq!(d.counts(now).interrupted, 1);
        let page = d.outstanding_page_live(now);
        assert!(
            page.contains("Interrupted <span class=n>1</span>"),
            "{page}"
        );
        assert!(page.contains("class='item st-interrupted'"), "{page}");
    }

    #[test]
    fn a_failed_job_can_be_dismissed_and_stays_off_the_page() {
        let store = scratch("dismiss");
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(
            &config,
            &platform,
            None,
            store.clone(),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let now = crate::store::now();
        crate::job::end(
            &store,
            "errand:housekeeping:1:1",
            "Housekeeping",
            crate::job::State::Failed,
            "locked",
        )
        .unwrap();
        crate::job::begin(&store, "errand:backup:1:2", "Backup", "started").unwrap();
        crate::job::end(
            &store,
            "errand:backup:1:2",
            "Backup",
            crate::job::State::Succeeded,
            "Finished.",
        )
        .unwrap();
        let o = d.open_work(now);
        assert_eq!(o.failed.len(), 1);
        assert_eq!(o.done.len(), 1, "a backup that finished today is in Done");
        assert_eq!(o.done[0].what, "Backup");
        let key = o.drops.failed[0].clone().unwrap();
        assert_eq!(key, "j:errand:housekeeping:1:1");
        assert!(d.outstanding_removable(now).iter().any(|(k, _)| *k == key));
        let off = d.drop_outstanding(&key, now).unwrap();
        assert_eq!(off.title, "Housekeeping");
        assert!(d.open_work(now).failed.is_empty());
        assert!(d.drop_outstanding(&key, now).is_err() || d.counts(now).failed == 0);
        // Kept, not deleted.
        assert!(crate::job::load_checked(&store)
            .unwrap()
            .iter()
            .any(|r| r.job_id == "errand:housekeeping:1:1" && r.dismissed));
    }

    /// 22 Oct 2026: dropping a saved approval drops the question too, so the
    /// next `persist` does not write the record again.
    #[test]
    fn a_dropped_approval_does_not_come_back_after_persist() {
        let store = scratch("drop-approval");
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(
            &config,
            &platform,
            None,
            store.clone(),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let now = crate::store::now();
        d.session.await_approval(
            crate::intent::Intent::CloseApp("Chrome".into()),
            "Close Chrome? Go ahead?",
        );
        d.persist();
        let o = d.open_work(now);
        let key = o
            .drops
            .waiting
            .iter()
            .flatten()
            .find(|k| k.starts_with("j:approval:"))
            .cloned()
            .expect("the approval is a record");
        d.drop_outstanding(&key, now).unwrap();
        assert!(!d.session.is_waiting(), "the question itself is dropped");
        d.persist();
        let o = d.open_work(now);
        assert!(
            !o.drops
                .waiting
                .iter()
                .flatten()
                .any(|k| k.starts_with("j:approval:")),
            "it came back: {}",
            o.drops
                .waiting
                .iter()
                .flatten()
                .cloned()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    #[test]
    fn lanes_skip_internal_keep_old_failures_and_give_needs_you_its_own_lane() {
        let store = scratch("lanes");
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let d = Daemon::new(
            &config,
            &platform,
            None,
            store.clone(),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let now = crate::store::now();
        // Atlas working on itself, failed: not the owner's to look at.
        crate::job::begin_as(&store, "errand:self-fix:1:1", "Self fix", "started", true).unwrap();
        crate::job::finish(
            &store,
            "errand:self-fix:1:1",
            "Self fix",
            crate::job::State::Failed,
            "no",
            "t",
        )
        .unwrap();
        // A request that failed three days ago is still there, undismissed.
        let mut old = crate::job::Record::planned("chat:1:1", "open a thing");
        old.transition(crate::job::State::Running, "started")
            .unwrap();
        old.finish(crate::job::State::Failed, "it did not open", "t")
            .unwrap();
        old.updated_at = now - 3 * 86_400;
        crate::job::upsert(&store, old).unwrap();
        // A saved approval: its own lane, with a way to drop it.
        crate::job::sync_approvals(
            &store,
            &[(
                crate::job::approval_key("chat", "Close Chrome? Go ahead?"),
                "Close Chrome? Go ahead?".into(),
            )],
        )
        .unwrap();
        let o = d.open_work(now);
        assert_eq!(
            o.failed.len(),
            1,
            "{}",
            o.failed
                .iter()
                .map(|f| f.what.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert_eq!(o.failed[0].what, "open a thing");
        assert!(!o.failed[0].when.is_empty(), "a failure says when");
        assert!(
            o.waiting.iter().any(|w| w.0.contains("Close Chrome")),
            "{}",
            o.waiting
                .iter()
                .map(|w| w.0.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
        assert!(
            o.blocked.is_empty(),
            "a question waiting on you is not Blocked"
        );
        assert!(o
            .drops
            .waiting
            .iter()
            .any(|k| k.as_deref().is_some_and(|k| k.starts_with("j:approval:"))));
    }

    #[test]
    fn activity_leaves_out_atlas_working_on_its_own_code_and_marks_the_rest() {
        let store = scratch("activity");
        let config = crate::config::Config::load(std::path::Path::new("config")).unwrap();
        let platform = crate::platform::mock::MockPlatform::new(Vec::new());
        let mut d = Daemon::new(
            &config,
            &platform,
            None,
            store,
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        let now = crate::store::now();
        d.journal.record_at(
            crate::activity::Kind::SelfWork,
            "mutation-sweep: mutation sweep of src/audio.rs: 2 changes no test noticed",
            true,
            now,
        );
        d.journal.record_at(
            crate::activity::Kind::Upkeep,
            "backup: the file is locked",
            false,
            now,
        );
        d.journal.record_at(
            crate::activity::Kind::Scheduled,
            "sent the weekly report",
            true,
            now,
        );
        let lines = d.activity_lines(now);
        assert_eq!(lines.len(), 2, "only the two the owner should see");
        assert!(lines
            .iter()
            .all(|(_, what, _)| !what.contains("mutation sweep")));
        assert_eq!(lines[0].0, hub::ActivityState::Done, "newest first");
        assert_eq!(lines[1].0, hub::ActivityState::Failed);
        let page = hub::activity_page(&lines);
        assert!(page.contains("st-failed") && page.contains("st-done"));
        assert!(!page.contains("mutation sweep"));
    }
}
