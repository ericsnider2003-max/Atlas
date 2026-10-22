//! Hub buttons whose work goes over the network, run off the daemon's thread.
//!
//! Every hub request is answered on the daemon's own thread, so a button
//! whose work waits on the network (sending a document over Tor, knocking on
//! a friend's Atlas, reaching a phone through Tailscale) used to freeze all
//! of Atlas until it finished: no voice, no other page, no tick. Now the
//! handler does its quick local checks, hands the slow part to the crew, and
//! comes straight back to the page with a job number in the address. The page
//! says what is happening and refreshes itself until the job has an answer,
//! then shows that answer once.
//!
//! The crew thread writes the answer here itself, so a page shows it even
//! while the daemon's tick is paused; whatever the daemon must do afterwards
//! with its own state (log the send on the document, keep a friend to try
//! again) happens when the crew's news is taken (`take_crew_news`).
//!
//! Also here: [`Flash`], for text that must never be in an address (a friend
//! link, an invitation code, a recovery key). It is held in memory, shown on
//! the next visit to its page, and gone after that or after five minutes.

use crate::hub::Page;
use crate::store::Store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

/// How long a job that went well is kept for its page to show.
const KEEP_FINISHED: Duration = Duration::from_secs(10 * 60);
/// How long a failed or interrupted job is kept (21 Oct 2026): at least a
/// day, counted from when it failed, so a restart does not start the clock
/// again and a failure is not gone before the owner has seen it.
const KEEP_FAILED: Duration = Duration::from_secs(24 * 60 * 60);
/// What a job that was running when Atlas stopped says. It may well have
/// finished, so it is not offered for a replay.
pub const INTERRUPTED: &str = "Interrupted: Atlas stopped while this was running; check whether it finished before running it again.";
/// How many jobs are remembered at once.
const MOST: usize = 64;
const STORE_NAME: &str = "hub_jobs";
const STORE_LIMIT: u64 = 256 * 1024;

/// Where a job is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum State {
    Running,
    Done(String),
    Failed(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PersistedJob {
    id: u64,
    page: Page,
    label: String,
    state: State,
    seen: bool,
    /// When it ended, in wall-clock seconds, so a restart keeps the real time
    /// (older files have none).
    #[serde(default)]
    finished_at: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct Job {
    /// The page that started it, and the one that shows its answer.
    pub page: Page,
    /// What it is doing, in words: "Sending \"plan\" to Sam".
    pub label: String,
    pub state: State,
    /// Its answer has been shown.
    pub seen: bool,
    /// Wall-clock seconds when it ended.
    finished: Option<u64>,
}

/// Every hub job, by number. Shared with the crew threads doing them.
#[derive(Debug)]
pub struct Jobs {
    inner: Mutex<(u64, BTreeMap<u64, Job>)>,
    store: Option<Store>,
}

impl Default for Jobs {
    fn default() -> Self {
        Self {
            inner: Mutex::new((0, BTreeMap::new())),
            store: None,
        }
    }
}

impl Jobs {
    /// Load the bounded Hub receipt cache for the daemon. A job that was
    /// running when Atlas stopped cannot be replayed safely, so it returns as
    /// an explicit failure rather than disappearing or claiming success.
    pub fn from_store(store: &Store) -> Self {
        Self::from_store_at(store, crate::store::now())
    }

    /// As `from_store`, with the clock given so a test can move it.
    fn from_store_at(store: &Store, now: u64) -> Self {
        let (persisted, can_persist) =
            match store.load_checked_bounded::<Vec<PersistedJob>>(STORE_NAME, STORE_LIMIT) {
                Ok(Some(value)) => (value, true),
                Ok(None) => (Vec::new(), true),
                Err(error) => {
                    // A checked-read failure means the source bytes are
                    // valuable recovery evidence. Do not turn the safe empty
                    // view into destructive overwrite on startup.
                    crate::kept!(Err::<Vec<PersistedJob>, _>(error));
                    (Vec::new(), false)
                }
            };
        let (active, mut terminal): (Vec<_>, Vec<_>) = persisted
            .into_iter()
            .partition(|item| item.state == State::Running);
        // Running jobs are converted to explicit restart failures, but none
        // may be evicted: losing one could hide work whose external effect is
        // unknown. Only terminal receipts use the bounded newest window.
        let terminal_keep = MOST.saturating_sub(active.len());
        terminal.reverse();
        let mut jobs = BTreeMap::new();
        let mut next = 0;
        for item in active
            .into_iter()
            .chain(terminal.into_iter().take(terminal_keep))
        {
            next = next.max(item.id);
            let finished = match item.state {
                State::Running => Some(now),
                _ => item.finished_at.or(Some(now)),
            };
            let state = match item.state {
                State::Running => State::Failed(INTERRUPTED.into()),
                state => state,
            };
            jobs.insert(
                item.id,
                Job {
                    page: item.page,
                    label: item.label,
                    state,
                    seen: item.seen,
                    finished,
                },
            );
        }
        // A failure counts its day from when it failed, not from this boot.
        forget_old_at(&mut jobs, now);
        let out = Self {
            inner: Mutex::new((next, jobs)),
            store: Some(store.clone()),
        };
        if can_persist {
            out.persist();
        }
        out
    }

    fn held(&self) -> std::sync::MutexGuard<'_, (u64, BTreeMap<u64, Job>)> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A new job, running. Returns its number.
    pub fn start(&self, page: Page, label: &str) -> u64 {
        let mut g = self.held();
        g.0 += 1;
        let id = g.0;
        g.1.insert(
            id,
            Job {
                page,
                label: label.to_string(),
                state: State::Running,
                seen: false,
                finished: None,
            },
        );
        forget_old(&mut g.1);
        drop(g);
        self.persist();
        id
    }

    /// The job's answer. A job already finished keeps its first answer
    /// unless `over` is set (the daemon's follow-up changed the outcome).
    pub fn finish(&self, id: u64, result: Result<String, String>, over: bool) {
        let mut g = self.held();
        if let Some(j) = g.1.get_mut(&id) {
            if j.state == State::Running || over {
                j.state = match result {
                    Ok(s) => State::Done(s),
                    Err(s) => State::Failed(s),
                };
                j.finished = Some(crate::store::now());
            }
        }
        forget_old(&mut g.1);
        drop(g);
        self.persist();
    }

    /// Where one job is, for a page that asked about it by number. A
    /// finished job is marked shown.
    pub fn look(&self, id: u64) -> Option<Job> {
        let mut g = self.held();
        let j = g.1.get_mut(&id)?;
        if j.state != State::Running {
            j.seen = true;
        }
        let out = j.clone();
        let changed = out.seen;
        drop(g);
        if changed {
            self.persist();
        }
        Some(out)
    }

    /// Make a finished job look `by` older, for the tests of forgetting.
    #[doc(hidden)]
    pub fn age_finished(&self, id: u64, by: Duration) {
        let mut g = self.held();
        if let Some(j) = g.1.get_mut(&id) {
            j.finished = j.finished.map(|f| f.saturating_sub(by.as_secs()));
        }
    }

    /// How many jobs are remembered.
    pub fn len(&self) -> usize {
        self.held().1.len()
    }

    /// Nothing remembered.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Every hub job that ended badly and is still remembered, newest first,
    /// as (number, what it was doing, why it failed). Reads only; nothing is
    /// marked shown. Outstanding's Failed lane uses it (20 Oct 2026: a hub
    /// button's job that failed was only ever a one-off line on its own page).
    pub fn failures(&self) -> Vec<(u64, String, String)> {
        let g = self.held();
        g.1.iter()
            .rev()
            .filter_map(|(id, job)| match &job.state {
                State::Failed(why) => Some((*id, job.label.clone(), why.clone())),
                _ => None,
            })
            .collect()
    }

    /// Answers for `page` not shown yet, marked shown.
    pub fn unseen_for(&self, page: Page) -> Vec<String> {
        let mut g = self.held();
        let mut out = Vec::new();
        for j in g.1.values_mut() {
            if j.page != page || j.seen {
                continue;
            }
            match &j.state {
                State::Done(s) | State::Failed(s) => {
                    j.seen = true;
                    out.push(s.clone());
                }
                State::Running => {}
            }
        }
        drop(g);
        if !out.is_empty() {
            self.persist();
        }
        out
    }

    fn persist(&self) {
        let Some(store) = &self.store else { return };
        let g = self.held();
        let records: Vec<PersistedJob> =
            g.1.iter()
                .map(|(id, job)| PersistedJob {
                    id: *id,
                    page: job.page,
                    label: job.label.clone(),
                    state: job.state.clone(),
                    seen: job.seen,
                    finished_at: job.finished,
                })
                .collect();
        drop(g);
        let encoded = match serde_json::to_vec(&records) {
            Ok(encoded) => encoded,
            Err(error) => {
                crate::kept!(Err::<(), _>(crate::error::AtlasError::Config(format!(
                    "Hub job encode: {error}"
                ))));
                return;
            }
        };
        if encoded.len() as u64 > STORE_LIMIT {
            crate::kept!(Err::<(), _>(crate::error::AtlasError::Config(
                "Hub job journal exceeds the local size bound".into(),
            )));
            return;
        }
        crate::kept!(store.save(STORE_NAME, &records));
    }
}

/// Keep the list bounded without losing work (28 Sep 2026).
///
/// A finished job is forgotten `KEEP_FINISHED` after it finished, shown or
/// not: an answer nobody came back for used to be kept forever. A failed or
/// interrupted one is kept `KEEP_FAILED`, counted from the wall-clock time it
/// failed and kept across restarts (21 Oct 2026). Past `MOST`,
/// the oldest *finished* jobs go first; a running job is never forgotten --
/// it was, when 64 jobs piled up, and its answer then had nowhere to land.
fn forget_old(jobs: &mut BTreeMap<u64, Job>) {
    forget_old_at(jobs, crate::store::now());
    while jobs.len() > MOST {
        let Some(oldest_finished) = jobs
            .iter()
            .find(|(_, j)| j.state != State::Running)
            .map(|(k, _)| *k)
        else {
            break;
        };
        jobs.remove(&oldest_finished);
    }
}

/// The time part of `forget_old`, with the clock given so a test can move it.
fn forget_old_at(jobs: &mut BTreeMap<u64, Job>, now: u64) {
    jobs.retain(|_, j| {
        let keep = if matches!(j.state, State::Failed(_)) {
            KEEP_FAILED
        } else {
            KEEP_FINISHED
        };
        !j.finished
            .is_some_and(|f| now.saturating_sub(f) > keep.as_secs())
    });
}

/// The notice a page shows for a job it asked about, and whether the page
/// should refresh itself to look again.
pub fn notice_for(job: &Job) -> (String, bool) {
    match &job.state {
        State::Running => (
            format!(
                "{}… (this page refreshes by itself)",
                job.label.trim_end_matches(['.', '…'])
            ),
            true,
        ),
        State::Done(s) | State::Failed(s) => (s.clone(), false),
    }
}

/// Text shown once on a page and never put in an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Flash {
    /// A sentence that carries something private (an invitation code, a
    /// new key's words).
    Said(String),
    /// A friend link just made.
    FriendLink(String),
    /// A release key just made: its card, and the recovery key shown this
    /// once.
    ReleaseKey {
        card: String,
        recovery: String,
        said: String,
    },
}

/// How long a flash waits for its page.
pub const FLASH_SECS: u64 = 5 * 60;

/// Keep `flash` for `page`, replacing any older one for the same page.
pub fn keep_flash(list: &mut Vec<(Page, Flash, u64)>, page: Page, flash: Flash, now: u64) {
    list.retain(|(p, _, until)| *p != page && *until > now);
    list.push((page, flash, now + FLASH_SECS));
}

/// The flash for `page`, taken: it is shown this once.
pub fn take_flash(list: &mut Vec<(Page, Flash, u64)>, page: Page, now: u64) -> Option<Flash> {
    list.retain(|(_, _, until)| *until > now);
    let at = list.iter().position(|(p, _, _)| *p == page)?;
    Some(list.remove(at).1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_job_runs_then_its_answer_is_shown_once() {
        let jobs = Jobs::default();
        let id = jobs.start(Page::Documents, "Sending \"plan\" to Sam");
        assert_eq!(jobs.look(id).unwrap().state, State::Running);
        assert!(jobs.unseen_for(Page::Documents).is_empty());
        let (n, again) = notice_for(&jobs.look(id).unwrap());
        assert!(again && n.starts_with("Sending \"plan\" to Sam…"), "{n}");
        jobs.finish(id, Ok("Sent.".into()), false);
        jobs.finish(id, Err("late".into()), false);
        assert_eq!(jobs.unseen_for(Page::Documents), vec!["Sent.".to_string()]);
        assert!(jobs.unseen_for(Page::Documents).is_empty(), "shown twice");
        assert_eq!(
            notice_for(&jobs.look(id).unwrap()),
            ("Sent.".to_string(), false)
        );
    }

    #[test]
    fn a_finished_hub_job_survives_restart_and_running_work_is_explicitly_interrupted() {
        let dir = std::env::temp_dir().join(format!(
            "atlas-hubjobs-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let jobs = Jobs::from_store(&store);
        let finished = jobs.start(Page::Documents, "Sending a document");
        jobs.finish(finished, Ok("Sent.".into()), false);
        let running = jobs.start(Page::Friends, "Contacting a friend");

        let restored = Jobs::from_store(&store);
        assert_eq!(
            restored.look(finished).unwrap().state,
            State::Done("Sent.".into())
        );
        assert_eq!(
            restored.look(running).unwrap().state,
            State::Failed(INTERRUPTED.into())
        );
        assert!(INTERRUPTED.contains("check whether it finished"));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_failure_keeps_its_own_finish_time_across_restarts_and_lasts_a_day() {
        let dir = std::env::temp_dir().join(format!(
            "atlas-hubjobs-failtime-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let jobs = Jobs::from_store(&store);
        let failed = jobs.start(Page::Friends, "Knocking");
        jobs.finish(failed, Err("No answer.".into()), false);
        let done = jobs.start(Page::Documents, "Sending");
        jobs.finish(done, Ok("Sent.".into()), false);
        let running = jobs.start(Page::Friends, "Still going");
        let failed_at = jobs.held().1[&failed].finished.unwrap();
        let t0 = crate::store::now();

        // An hour later, after a restart: the failure is still listed, with
        // the time it really failed; the success has run out its ten minutes.
        let hour = Jobs::from_store_at(&store, t0 + 3_600);
        assert_eq!(hour.held().1[&failed].finished, Some(failed_at));
        assert_eq!(hour.failures().len(), 2, "failure and interrupted one");
        assert!(hour.look(done).is_none(), "a success outlived ten minutes");
        assert_eq!(
            hour.look(running).unwrap().state,
            State::Failed(INTERRUPTED.into())
        );

        // A second restart does not restart the clock.
        let again = Jobs::from_store_at(&store, t0 + 3_700);
        assert_eq!(again.held().1[&failed].finished, Some(failed_at));
        assert_eq!(again.failures().len(), 2);

        // Past a day from the failure it is gone.
        let later = Jobs::from_store_at(&store, failed_at + 24 * 3_600 + 60);
        assert!(later.look(failed).is_none(), "a failure outlived a day");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_hub_job_file_from_before_finish_times_still_loads() {
        let dir = std::env::temp_dir().join(format!(
            "atlas-hubjobs-old-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let jobs = Jobs::from_store(&store);
        let id = jobs.start(Page::Friends, "Knocking");
        jobs.finish(id, Err("No answer.".into()), false);
        let text = std::fs::read_to_string(dir.join("hub_jobs.json")).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&text).unwrap();
        for job in value["data"].as_array_mut().unwrap() {
            job.as_object_mut().unwrap().remove("finished_at");
        }
        let old = serde_json::to_string(&value).unwrap();
        assert!(!old.contains("finished_at"), "{old}");
        std::fs::write(dir.join("hub_jobs.json"), old).unwrap();
        let restored = Jobs::from_store(&store);
        assert_eq!(restored.failures().len(), 1);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_corrupt_hub_receipt_is_not_overwritten_on_restart() {
        let dir = std::env::temp_dir().join(format!(
            "atlas-hubjobs-corrupt-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let bytes = b"{not-a-hub-receipt";
        std::fs::write(dir.join("hub_jobs.json"), bytes).unwrap();

        let restored = Jobs::from_store(&store);
        assert!(restored.is_empty());
        assert_eq!(std::fs::read(dir.join("hub_jobs.json")).unwrap(), bytes);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restart_keeps_all_running_jobs_even_when_the_journal_is_over_capacity() {
        let dir = std::env::temp_dir().join(format!(
            "atlas-hubjobs-window-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let jobs = Jobs::from_store(&store);
        for n in 0..(MOST + 1) {
            jobs.start(Page::Documents, &format!("job {n}"));
        }

        let restored = Jobs::from_store(&store);
        assert_eq!(restored.len(), MOST + 1);
        assert!(restored.look(1).is_some(), "running job was evicted");
        assert!(restored.look((MOST + 1) as u64).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn restart_keeps_the_newest_terminal_receipts_when_over_capacity() {
        let dir = std::env::temp_dir().join(format!(
            "atlas-hubjobs-terminal-window-{}-{}",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);
        let jobs = Jobs::from_store(&store);
        for n in 0..(MOST + 1) {
            let id = jobs.start(Page::Documents, &format!("job {n}"));
            jobs.finish(id, Ok(format!("done {n}")), false);
        }

        let restored = Jobs::from_store(&store);
        assert_eq!(restored.len(), MOST);
        assert!(
            restored.look(1).is_none(),
            "oldest terminal receipt survived"
        );
        assert!(restored.look((MOST + 1) as u64).is_some());
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_flash_is_for_its_page_once_and_runs_out() {
        let mut l = Vec::new();
        keep_flash(
            &mut l,
            Page::Friends,
            Flash::FriendLink("atlas:x".into()),
            100,
        );
        assert!(take_flash(&mut l, Page::Sync, 101).is_none());
        assert_eq!(
            take_flash(&mut l, Page::Friends, 101),
            Some(Flash::FriendLink("atlas:x".into()))
        );
        assert!(take_flash(&mut l, Page::Friends, 102).is_none());
        keep_flash(&mut l, Page::Sync, Flash::Said("code".into()), 100);
        assert!(take_flash(&mut l, Page::Sync, 100 + FLASH_SECS + 1).is_none());
    }
}
