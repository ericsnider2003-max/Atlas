//! The floor, daemon side (20 Oct 2026): the everyday jobs done with no
//! language model, each recorded as a visible job and checked, and what is
//! said at once -- with a choice -- when a question needs the model and the
//! memory to load it isn't there. The plain half is `crate::floor`.

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

static SEQ: AtomicU64 = AtomicU64::new(1);

const MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

impl<'a> Daemon<'a> {
    /// "20 October 2026", on your clock, for the moment of the turn.
    fn floor_today(&self) -> String {
        let t = self.now_acting();
        let (y, m, d) =
            crate::hubpages::ymd(crate::localclock::day(t, crate::localclock::offset_secs()));
        format!("{d} {} {y}", MONTHS[(m.clamp(1, 12) - 1) as usize])
    }

    /// "2026-10-20 14-05-09": a stamp that is also a legal folder name.
    fn floor_stamp(&self) -> String {
        let t = self.now_acting();
        let off = crate::localclock::offset_secs();
        let (y, m, d) = crate::hubpages::ymd(crate::localclock::day(t, off));
        let hhmm = crate::localclock::hhmm(t, off).replace(':', "-");
        format!("{y:04}-{m:02}-{d:02} {hhmm}-{:02}", t % 60)
    }

    /// Something that changes a thing, done and written down: a durable job
    /// record (begun, then ended Succeeded or Failed with the words that are
    /// said) and a Hub job on the Status page. `work` does it and checks it.
    fn floor_job(
        &mut self,
        kind: &str,
        stage: &str,
        what: &str,
        work: impl FnOnce() -> crate::floor::Done,
    ) -> crate::floor::Done {
        let t = self.now_acting();
        let id = format!(
            "{}floor-{kind}:{t}:{}",
            crate::job::ERRAND_PREFIX,
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        if let Err(e) = crate::job::begin(&self.store, &id, stage, what) {
            self.log
                .warn(&format!("couldn't write down that \"{what}\" started: {e}"));
        }
        let hub = self.hub_jobs.start(crate::hub::Page::Status, what);
        let done = work();
        let state = if done.ok {
            crate::job::State::Succeeded
        } else {
            crate::job::State::Failed
        };
        let detail: String = done.said.chars().take(300).collect();
        if let Err(e) = crate::job::end(&self.store, &id, stage, state, &detail) {
            self.log
                .warn(&format!("couldn't write down how \"{what}\" ended: {e}"));
        }
        self.hub_jobs.finish(
            hub,
            if done.ok {
                Ok(done.said.clone())
            } else {
                Err(done.said.clone())
            },
            false,
        );
        done
    }

    /// "Create a text file named x.txt in my Desktop folder containing ...".
    pub(super) fn on_create_file(&mut self, raw: &str) -> String {
        let today = self.floor_today();
        let spec = match crate::floor::read_create(raw, &today) {
            Ok(s) => s,
            Err(question) => return question,
        };
        let Some(words) = spec.folder.clone() else {
            return format!(
                "Where should I put {}? Say it like: create a text file named {} in my Documents folder.",
                spec.name, spec.name
            );
        };
        let Some(folder) = crate::floor::folder_named(&words) else {
            return format!(
                "I couldn't find a folder called \"{words}\", so I didn't make {}. Name one that exists, like \"my Documents folder\" or a full path.",
                spec.name
            );
        };
        let what = format!("Making {} in {}", spec.name, folder.display());
        self.floor_job("create-file", "Make a file", &what, move || {
            crate::floor::create_file(&spec, &folder)
        })
        .said
    }

    /// "Read atlas-test.txt on my Desktop": changes nothing, so no job.
    pub(super) fn on_read_file(&mut self, raw: &str) -> String {
        // "Is the backup finished?" asks about a job, not a file.
        if let Some(ask) = crate::floor::status_question(raw) {
            return self.on_job_status(&ask);
        }
        let Some((name, folder)) = crate::floor::read_file_ask(raw) else {
            return "Which file? Say it like: read notes.txt on my Desktop.".into();
        };
        let folder = match folder {
            Some(w) => match crate::floor::folder_named(&w) {
                Some(p) => Some(p),
                None => return format!("I couldn't find a folder called \"{w}\"."),
            },
            None => None,
        };
        match crate::floor::locate(&name, folder.as_deref()) {
            Ok(p) => crate::floor::read_file(&p).said,
            Err(why) => why,
        }
    }

    /// "Is the backup of src finished?" (21 Oct 2026), answered from the Hub's
    /// job list and the durable job records -- no model. Running, done with
    /// when and where, failed with why, or that there is no such job.
    /// "Is it done?" right after a job you started or one that just ended:
    /// "it" is that job, answered from the job records with no model. With no
    /// recent job in this session it is not read here (small talk, or the
    /// what's-running answer, take it as before).
    pub(super) fn job_context_status_help(&mut self, raw: &str) -> Option<String> {
        let ask = crate::floor::status_question_about_it(raw)?;
        let now = crate::store::now();
        let recent = self.hub_jobs.snapshot().iter().any(|j| {
            j.finished_at()
                .map_or(true, |at| now.saturating_sub(at) <= 600)
        });
        recent.then(|| self.on_job_status(&ask))
    }

    fn on_job_status(&mut self, ask: &crate::floor::StatusAsk) -> String {
        use crate::floor::{JobView, Life};
        let mut views: Vec<JobView> = self
            .hub_jobs
            .snapshot()
            .into_iter()
            .map(|j| JobView {
                label: j.label.clone(),
                ended: j.finished_at(),
                life: match &j.state {
                    crate::hubjobs::State::Running => Life::Running,
                    crate::hubjobs::State::Done(s) => Life::Done(s.clone()),
                    crate::hubjobs::State::Failed(s) => Life::Failed(s.clone()),
                },
            })
            .collect();
        // The durable records too, for work the Hub never listed (Atlas's own
        // backup of its state). The floor's own jobs are in both: the Hub's
        // copy carries the fuller label.
        for r in crate::job::load_checked(&self.store).unwrap_or_default() {
            if r.internal || r.job_id.starts_with("errand:floor-") {
                continue;
            }
            let said = r.receipt.as_ref().map(|x| x.detail.clone()).unwrap_or_default();
            let life = match r.state {
                crate::job::State::Succeeded => Life::Done(said),
                crate::job::State::Failed => Life::Failed(said),
                crate::job::State::Uncertain => Life::Failed(format!(
                    "it was cut off and is not known to have finished. {said}"
                )),
                crate::job::State::Cancelled | crate::job::State::RolledBack => {
                    Life::Failed(format!("it was stopped before it finished. {said}"))
                }
                crate::job::State::AwaitingApproval => continue,
                _ => Life::Running,
            };
            let ended = r.state.terminal().then_some(r.updated_at);
            views.push(JobView {
                label: r.stage.clone(),
                life,
                ended,
            });
        }
        crate::floor::status_said(
            ask,
            &views,
            crate::store::now(),
            crate::localclock::offset_secs(),
        )
    }

    /// "Move x.txt from my Desktop to my Documents folder".
    pub(super) fn on_move_file(&mut self, raw: &str) -> String {
        let Some((name, from, to)) = crate::floor::read_move(raw) else {
            return "Say it like: move notes.txt from my Desktop to my Documents folder.".into();
        };
        let from = match from {
            Some(w) => match crate::floor::folder_named(&w) {
                Some(p) => Some(p),
                None => {
                    return format!(
                        "I couldn't find a folder called \"{w}\", so nothing was moved."
                    )
                }
            },
            None => None,
        };
        let src = match crate::floor::locate(&name, from.as_deref()) {
            Ok(p) => p,
            Err(why) => return format!("{why} Nothing was moved."),
        };
        let Some(dest) = crate::floor::folder_named(&to) else {
            return format!("I couldn't find a folder called \"{to}\", so nothing was moved.");
        };
        let what = format!("Moving {name} to {}", dest.display());
        self.floor_job("move-file", "Move a file", &what, move || {
            crate::floor::move_file(&src, &dest)
        })
        .said
    }

    /// "Back up my Documents folder to D:\Backups". With no place said, asks,
    /// and the answer comes back to `floor_answer`.
    pub(super) fn on_back_up_folder(&mut self, raw: &str) -> String {
        let Some((folder_words, dest_words)) = crate::floor::read_backup(raw) else {
            return "Which folder? Say it like: back up my Documents folder to D:\\Backups.".into();
        };
        let Some(src) = crate::floor::folder_named(&folder_words) else {
            return format!(
                "I couldn't find a folder called \"{folder_words}\", so I didn't back anything up."
            );
        };
        if !src.is_dir() {
            return format!(
                "{} isn't there, so I didn't back anything up. Check the path, or say \"find the folder {}\" and I'll look for it.",
                src.display(),
                src.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
            );
        }
        match dest_words {
            Some(w) => match crate::floor::folder_named(&w) {
                Some(dest) => self.start_folder_backup(src, dest),
                None => format!("I couldn't find a place called \"{w}\" to put the backup. Name a folder or a full path."),
            },
            None => {
                // No place said: read it against what is known first (21 Oct
                // 2026): the last backup's destination, or a folder that is
                // open. One clear reading starts the backup and says why; a
                // choice is one question offering them; nothing is today's
                // question, which already says what is missing. The source
                // itself is never offered as its own destination.
                let ctx = self.resolution_context();
                let r = crate::understood::resolve("folder", "where the backup should go", &ctx);
                let q = match &r {
                    crate::understood::Resolution::Clear { value, .. }
                        if !value.eq_ignore_ascii_case(&src.display().to_string())
                            && std::path::Path::new(value).is_dir()
                            && !crate::floor::dest_inside_source(
                                &src,
                                std::path::Path::new(value),
                            ) =>
                    {
                        let dest = std::path::PathBuf::from(value);
                        let said = crate::understood::stated(
                            &format!("Backing up to {}", dest.display()),
                            &r,
                        )
                        .unwrap_or_default();
                        let started = self.start_folder_backup(src, dest);
                        return format!("{said} {started}");
                    }
                    crate::understood::Resolution::Choose { question, .. } => question.clone(),
                    _ => format!(
                        "Where should I put the backup of {}? Say a folder, like \"D:\\Backups\" or \"my Documents folder\". I'll make a new dated folder inside it and never overwrite anything there.",
                        src.display()
                    ),
                };
                self.floor.pending = Some(crate::floor::Pending::BackupDest { folder: src });
                self.session.ask(&q);
                q
            }
        }
    }

    /// The copy itself, on the crew so the loop is not held: a job record
    /// begun here and ended by the worker with how it really ended.
    fn start_folder_backup(&mut self, src: std::path::PathBuf, dest: std::path::PathBuf) -> String {
        let t = self.now_acting();
        let plan = match crate::floor::plan_backup(&src, &dest, &self.floor_stamp()) {
            Ok(p) => p,
            Err(why) => return why,
        };
        if self.rehearsal {
            return format!(
                "(Rehearsal) I would back up {} into {}.",
                src.display(),
                plan.target.display()
            );
        }
        let stage = "Back up a folder";
        let what = format!("Backing up {} to {}", src.display(), plan.target.display());
        let id = format!(
            "{}floor-folder-backup:{t}:{}",
            crate::job::ERRAND_PREFIX,
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        if let Err(e) = crate::job::begin(&self.store, &id, stage, &what) {
            self.log
                .warn(&format!("couldn't write down that \"{what}\" started: {e}"));
        }
        let hub = self.hub_jobs.start(crate::hub::Page::Status, &what);
        let store = self.store.clone();
        let jobs = self.hub_jobs.clone();
        let (job_id, worker_plan) = (id.clone(), plan.clone());
        let work: crew::Work = Box::new(move |c| {
            let done = crate::floor::run_backup(&worker_plan, &|| c.stopping());
            let state = if done.ok {
                crate::job::State::Succeeded
            } else {
                crate::job::State::Failed
            };
            let detail: String = done.said.chars().take(300).collect();
            // The record is closed before the errand reports, so nothing can
            // read "finished" beside a job still shown running.
            crate::heard!(crate::job::end(&store, &job_id, stage, state, &detail));
            jobs.finish(
                hub,
                if done.ok {
                    Ok(done.said.clone())
                } else {
                    Err(done.said.clone())
                },
                false,
            );
            if done.ok {
                Ok(done.said)
            } else {
                Err(done.said)
            }
        });
        // A topic of its own, so two asks are two backups and each record is
        // ended by its own worker.
        if self.hand_off(
            "folder-backup",
            t,
            work,
            Some(id.clone()),
            SpeakPolicy::Always,
        ) {
            format!(
                "Backing up {} in the background into a new folder inside {}. It's in Outstanding while it runs; I'll tell you when it's done and checked (file count and a hash of every file).",
                src.display(),
                dest.display()
            )
        } else {
            crate::heard!(crate::job::end(
                &self.store,
                &id,
                stage,
                crate::job::State::Failed,
                "Atlas had too much background work to start it."
            ));
            self.hub_jobs.finish(
                hub,
                Err("Too much background work to start it.".into()),
                false,
            );
            "I'm swamped with background work right now, so the backup didn't start. Ask again in a moment.".into()
        }
    }

    /// Your never-close list (`tune.keep`), so a program you told Atlas to
    /// keep is never offered for closing to free memory (merge review, 21 Oct
    /// 2026). Naming it yourself ("close X") still closes it.
    fn never_close(&self) -> Vec<String> {
        self.tools_ref()
            .map(|tc| tc.tune.keep.clone())
            .unwrap_or_default()
    }

    /// "What's using my memory": the five biggest and the totals, in a
    /// moment, from the process list and the memory counters.
    pub(super) fn on_whats_using_memory(&mut self) -> String {
        let procs = crate::tune::snapshot();
        let top = crate::floor::biggest(&procs, std::process::id(), &self.never_close());
        let (total, free) = crate::floor::memory_now();
        crate::floor::memory_said(&top, total, free)
    }

    /// Closing an app Atlas knows: refused for Windows and Atlas itself, then
    /// closed, then looked for -- the words say what was seen afterwards.
    pub(super) fn on_close_app(&mut self, app: &str) -> String {
        if !crate::tune::may_close(app, &[]) {
            return format!("I won't close {app}: it's part of Windows or Atlas itself.");
        }
        let (cfg, plat) = (self.cfg, self.plat);
        let name = app.to_string();
        let what = format!("Closing {app}");
        self.floor_job("close-app", "Close an app", &what, move || {
            if let Err(e) = workspace::close_app(cfg, plat, &name) {
                return crate::floor::Done::no(format!("I couldn't close {name}: {e}."));
            }
            let Ok(spec) = cfg.apps.get(&name) else {
                return crate::floor::Done::no(format!("I asked {name} to close but have no way to check it."));
            };
            for _ in 0..12 {
                match plat.find_window(spec) {
                    Ok(None) => return crate::floor::Done::yes(format!("Closed {name}. Checked: its window is gone.")),
                    Ok(Some(_)) => std::thread::sleep(std::time::Duration::from_millis(250)),
                    Err(e) => {
                        return crate::floor::Done::no(format!(
                            "I asked {name} to close but couldn't check that it did: {e}."
                        ))
                    }
                }
            }
            crate::floor::Done::no(format!(
                "I asked {name} to close, but its window is still there. It may be asking whether to save; I left it."
            ))
        })
        .said
    }

    // ------------------------------------------------ a question needing the model

    /// What is known about the model right now, for `crate::floor::decide`. `None` when
    /// this isn't Atlas's own model to judge: a model of your own is set, or
    /// Atlas isn't the one starting it.
    fn floor_facts(&self) -> Option<crate::floor::Facts> {
        if let Some(f) = &self.floor.facts {
            return Some(f());
        }
        let tc = self.tools_ref()?;
        if tc.llm.is_some() || !self.starts_model_server {
            return None;
        }
        let up_elsewhere = self
            .model_server_seen
            .lock()
            .or_else(crate::crash::unpoison)
            .ok()
            .and_then(|g| *g)
            .is_some_and(|(_, up)| up);
        if self.helpers.is_running("model-server") || up_elsewhere {
            return None;
        }
        let (registry, _) =
            crate::models::Registry::scan_reporting(&crate::models::Registry::dir_for(&tc.models));
        let (total_mb, free_mb) = crate::floor::memory_now();
        let machine = crate::fit::machine_from(
            &crate::health::read_machine(),
            crate::fit::Graphics::default(),
            false,
        );
        // 21 Oct 2026: when the memory tiers apply and say no model fits, that
        // is the answer -- the 1.7B and what it needs -- not a fallback to the
        // 4B, which `decide` would call Proceed with about 3.5 GB free.
        let model = match crate::models::pick(&registry, &tc.models, &machine) {
            Some(m) => {
                let tuned = crate::models::tuned_for(m, &tc.models, &machine);
                Some((m.id.clone(), crate::models::footprint_mb(m, &tuned)))
            }
            None => crate::models::tier_no_fit(&registry, &tc.models, &machine).or_else(|| {
                registry.choose_for(&tc.models, u64::MAX).map(|m| {
                    let tuned = crate::models::tuned_for(m, &tc.models, &machine);
                    (m.id.clone(), crate::models::footprint_mb(m, &tuned))
                })
            }),
        };
        let procs = crate::tune::snapshot();
        let closable = crate::floor::biggest(&procs, std::process::id(), &self.never_close())
            .into_iter()
            .filter(|u| u.closable)
            .collect();
        Some(crate::floor::Facts {
            loaded: false,
            model,
            free_mb,
            total_mb,
            online_allowed: tc.models.online_second,
            offline_known: self.connectivity.cached() == Reach::Offline,
            closable,
        })
    }

    /// An unrecognised request that would go to the model, when the model
    /// isn't loaded and can't be: the answer at once, with the choice, and the
    /// request kept so "answer online" or "close <app>" carries on from here.
    /// `None` means ask the model as usual.
    pub(super) fn floor_when_the_model_cannot_load(&mut self, said: &str) -> Option<String> {
        if self.floor.bypass || said.split_whitespace().count() < 2 {
            return None;
        }
        let facts = self.floor_facts()?;
        match crate::floor::decide(&facts) {
            crate::floor::Choice::Proceed => None,
            // With the free online model standing in and labelled as such,
            // the existing path answers; only with nothing at all to answer
            // with is it said plainly here.
            crate::floor::Choice::NoModel => self.llm.is_none().then(crate::floor::no_model_said),
            crate::floor::Choice::Low(ask) => {
                let q = crate::floor::say(&ask);
                self.floor.pending = Some(crate::floor::Pending::LowMemory {
                    request: said.to_string(),
                    online: ask.online,
                    need_mb: ask.need_mb,
                });
                self.session.ask(&q);
                Some(q)
            }
        }
    }

    /// Whatever was said after Atlas asked the floor's question. `None`: it
    /// wasn't an answer, and is heard as a new request.
    pub(super) fn floor_answer(
        &mut self,
        pending: crate::floor::Pending,
        said: &str,
        t: u64,
        how: Arrival,
    ) -> Option<String> {
        // The answer is of this turn's moment, not the wall clock's.
        let before = self.acting_at.replace(t);
        let reply = self.floor_answer_at(pending, said, t, how);
        self.acting_at = before;
        reply
    }

    fn floor_answer_at(
        &mut self,
        pending: crate::floor::Pending,
        said: &str,
        t: u64,
        how: Arrival,
    ) -> Option<String> {
        match pending {
            crate::floor::Pending::BackupDest { folder } => {
                if matches!(crate::floor::follow_up(said), crate::floor::Follow::Cancel) {
                    return Some("Alright, no backup.".into());
                }
                let words = crate::floor::backup_destination(said)?;
                Some(match crate::floor::folder_named(&words) {
                    Some(dest) => self.start_folder_backup(folder, dest),
                    None => format!("I couldn't find a place called \"{words}\". Nothing was backed up."),
                })
            }
            crate::floor::Pending::LowMemory {
                request,
                online,
                need_mb,
            } => match crate::floor::follow_up(said) {
                crate::floor::Follow::Online if online => Some(self.answer_online(&request, t, how)),
                crate::floor::Follow::Online => Some(
                    "I can't answer online right now: the online fallback is off or I'm offline. Say \"close <app>\" from the list and I'll load the model, or free some memory yourself and ask again."
                        .into(),
                ),
                crate::floor::Follow::Close(name) => Some(self.ask_to_close_for_memory(request, &name, online, need_mb)),
                crate::floor::Follow::Cancel => Some("Alright, I've dropped that question. Ask again whenever you like.".into()),
                crate::floor::Follow::Other => None,
            },
            crate::floor::Pending::CloseForMemory {
                request,
                name,
                mb,
                need_mb,
            } => {
                if !crate::floor::yes(said) {
                    return Some("Alright, I haven't closed anything.".into());
                }
                Some(self.close_for_memory(request, &name, mb, need_mb, t, how))
            }
        }
    }

    /// The question put to the model again, by the free online one, once, and
    /// labelled as online.
    fn answer_online(&mut self, request: &str, t: u64, how: Arrival) -> String {
        let online: Arc<dyn Llm> = self
            .floor
            .online
            .clone()
            .unwrap_or_else(|| Arc::new(crate::freeonline::FreeOnline::new()));
        let saved = self.llm.replace(online);
        let (bypass, defer) = (self.floor.bypass, self.may_defer);
        self.floor.bypass = true;
        self.may_defer = false;
        let said = self.turn_from(request, t, how);
        self.floor.bypass = bypass;
        self.may_defer = defer;
        self.llm = saved;
        format!("Online answer (from a free online model, not this computer's own): {said}")
    }

    /// "close Chrome", in answer to the choice: found among the programs that
    /// may be closed, and confirmed first -- closing loses unsaved work.
    fn ask_to_close_for_memory(
        &mut self,
        request: String,
        name: &str,
        online: bool,
        need_mb: u64,
    ) -> String {
        // "close it" names nothing: ask which, never guess (closing the wrong
        // program loses its unsaved work).
        if crate::floor::names_nothing(name) {
            self.floor.pending = Some(crate::floor::Pending::LowMemory {
                request,
                online,
                need_mb,
            });
            self.session.ask("Which app should I close?");
            return "Which app should I close? Say its name from the list.".into();
        }
        // The programs that were offered are the ones that can be picked;
        // anything else running is looked up only to say why it isn't.
        let offered: Vec<crate::floor::Use> = match self.floor_facts() {
            Some(f) => f.closable,
            None => crate::floor::biggest(
                &crate::tune::snapshot(),
                std::process::id(),
                &self.never_close(),
            )
            .into_iter()
            .filter(|u| u.closable)
            .collect(),
        };
        // Matched by whole name, then whole words, then a prefix.
        let all = if crate::floor::pick_program(name, &offered).is_some() {
            Vec::new()
        } else {
            crate::floor::biggest(
                &crate::tune::snapshot(),
                std::process::id(),
                &self.never_close(),
            )
        };
        match crate::floor::pick_program(name, &offered)
            .or_else(|| crate::floor::pick_program(name, &all))
        {
            Some(u) if u.closable => {
                let q = format!(
                    "Closing {} ({}) closes all of its windows, and anything unsaved in them is lost. Go ahead?",
                    u.name,
                    crate::floor::mb_text(u.mb)
                );
                self.floor.pending = Some(crate::floor::Pending::CloseForMemory {
                    request,
                    name: u.name.clone(),
                    mb: u.mb,
                    need_mb,
                });
                self.session.ask(&q);
                q
            }
            Some(u) => {
                self.floor.pending = Some(crate::floor::Pending::LowMemory {
                    request,
                    online,
                    need_mb,
                });
                self.session.ask("Which app should I close?");
                format!(
                    "I won't close {}: it's part of Windows, Atlas, or something another program depends on. Pick one from the list.",
                    u.name
                )
            }
            None => {
                self.floor.pending = Some(crate::floor::Pending::LowMemory {
                    request,
                    online,
                    need_mb,
                });
                self.session.ask("Which app should I close?");
                format!("I can't see a running program called \"{name}\". Pick one from the list.")
            }
        }
    }

    /// Close it, look at the memory again, and -- if the model fits now --
    /// load it and answer the request that started this.
    fn close_for_memory(
        &mut self,
        request: String,
        name: &str,
        mb: u64,
        need_mb: u64,
        t: u64,
        how: Arrival,
    ) -> String {
        let closer = self.floor.closer.clone();
        let target = name.to_string();
        let what = format!("Closing {name} to free memory");
        let done = self.floor_job(
            "close-for-memory",
            "Close an app",
            &what,
            move || match closer {
                Some(c) => c(&target),
                None => close_program(&target, mb),
            },
        );
        if !done.ok {
            // Still short, and the choice still stands.
            self.floor.pending = Some(crate::floor::Pending::LowMemory {
                request,
                online: self
                    .floor_facts()
                    .map(|f| f.online_allowed && !f.offline_known)
                    .unwrap_or(false),
                need_mb,
            });
            self.session
                .ask("Which app should I close, or answer online?");
            return format!("{} The model still isn't loaded.", done.said);
        }
        // Looked at again, not assumed.
        match self
            .floor_facts()
            .map(|f| (crate::floor::decide(&f), f.free_mb))
        {
            Some((crate::floor::Choice::Low(ask), free)) => {
                let q = crate::floor::say(&ask);
                self.floor.pending = Some(crate::floor::Pending::LowMemory {
                    request,
                    online: ask.online,
                    need_mb: ask.need_mb,
                });
                self.session.ask(&q);
                format!(
                    "{} Memory free now: {}. That's still not enough. {q}",
                    done.said,
                    crate::floor::mb_text(free)
                )
            }
            _ => {
                // Room now: the model is started and the request asked again.
                self.model_start_tried = None;
                self.model_rested = false;
                self.model_look_at = 0;
                let answer = self.turn_from(&request, t, how);
                format!(
                    "{} Memory is enough now, so I loaded the model. {answer}",
                    done.said
                )
            }
        }
    }

    /// Test seams: the facts, the closing and the online model stood in for,
    /// so the choice is exercised without a real model, memory or program.
    pub fn floor_with_facts_for_test(
        &mut self,
        facts: Arc<dyn Fn() -> crate::floor::Facts + Send + Sync>,
        closer: Arc<dyn Fn(&str) -> crate::floor::Done + Send + Sync>,
        online: Option<Arc<dyn Llm>>,
    ) {
        self.floor.facts = Some(facts);
        self.floor.closer = Some(closer);
        self.floor.online = online;
    }
}

/// Close one running program by name: refused for Windows and Atlas's own
/// processes, asked to close politely, waited for, and looked for again.
fn close_program(name: &str, mb: u64) -> crate::floor::Done {
    let procs = crate::tune::snapshot();
    let family = crate::tune::atlas_family(&procs, std::process::id());
    let loads = crate::tune::sample_load(&procs, &procs, 1, 1);
    let Some(load) = loads
        .into_iter()
        .find(|l| l.name.eq_ignore_ascii_case(name))
    else {
        return crate::floor::Done::no(format!(
            "{name} isn't running any more, so there was nothing to close."
        ));
    };
    if load.system
        || !crate::tune::may_close(&load.name, &[])
        || load.pids.iter().any(|p| family.contains(p))
    {
        return crate::floor::Done::no(format!(
            "I won't close {name}: it's part of Windows or Atlas itself."
        ));
    }
    let result = crate::tune::close_loads(&[load], std::time::Duration::from_secs(6));
    match result.first() {
        Some(crate::tune::Closed::Asked | crate::tune::Closed::Ended | crate::tune::Closed::Gone) => {
            let left = crate::tune::snapshot()
                .iter()
                .filter(|p| p.name.eq_ignore_ascii_case(name))
                .count();
            if left == 0 {
                crate::floor::Done::yes(format!("Closed {name} (it was holding about {}). Checked: no process of it is left.", crate::floor::mb_text(mb)))
            } else {
                crate::floor::Done::no(format!("I asked {name} to close, but {left} of its processes are still running."))
            }
        }
        Some(crate::tune::Closed::LeftOpen) => crate::floor::Done::no(format!(
            "{name} is still open -- it's probably asking whether to save. I left it rather than lose your work."
        )),
        Some(crate::tune::Closed::Failed(e)) => crate::floor::Done::no(format!("I couldn't close {name}: {e}.")),
        None => crate::floor::Done::no(format!("I couldn't close {name}.")),
    }
}
