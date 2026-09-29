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
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// How long a finished job is kept for its page to show.
const KEEP_FINISHED: Duration = Duration::from_secs(10 * 60);
/// How many jobs are remembered at once.
const MOST: usize = 64;

/// Where a job is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Running,
    Done(String),
    Failed(String),
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
    finished: Option<Instant>,
}

/// Every hub job, by number. Shared with the crew threads doing them.
#[derive(Debug, Default)]
pub struct Jobs {
    inner: Mutex<(u64, BTreeMap<u64, Job>)>,
}

impl Jobs {
    fn held(&self) -> std::sync::MutexGuard<'_, (u64, BTreeMap<u64, Job>)> {
        self.inner.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A new job, running. Returns its number.
    pub fn start(&self, page: Page, label: &str) -> u64 {
        let mut g = self.held();
        g.0 += 1;
        let id = g.0;
        g.1.insert(id, Job { page, label: label.to_string(), state: State::Running, seen: false, finished: None });
        forget_old(&mut g.1);
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
                j.finished = Some(Instant::now());
            }
        }
        forget_old(&mut g.1);
    }

    /// Where one job is, for a page that asked about it by number. A
    /// finished job is marked shown.
    pub fn look(&self, id: u64) -> Option<Job> {
        let mut g = self.held();
        let j = g.1.get_mut(&id)?;
        if j.state != State::Running {
            j.seen = true;
        }
        Some(j.clone())
    }

    /// Make a finished job look `by` older, for the tests of forgetting.
    #[doc(hidden)]
    pub fn age_finished(&self, id: u64, by: Duration) {
        let mut g = self.held();
        if let Some(j) = g.1.get_mut(&id) {
            j.finished = j.finished.and_then(|f| f.checked_sub(by));
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
        out
    }
}

/// Keep the list bounded without losing work (28 Sep 2026).
///
/// A finished job is forgotten `KEEP_FINISHED` after it finished, shown or
/// not: an answer nobody came back for used to be kept forever. Past `MOST`,
/// the oldest *finished* jobs go first; a running job is never forgotten --
/// it was, when 64 jobs piled up, and its answer then had nowhere to land.
fn forget_old(jobs: &mut BTreeMap<u64, Job>) {
    jobs.retain(|_, j| !j.finished.is_some_and(|f| f.elapsed() > KEEP_FINISHED));
    while jobs.len() > MOST {
        let Some(oldest_finished) = jobs.iter().find(|(_, j)| j.state != State::Running).map(|(k, _)| *k) else { break };
        jobs.remove(&oldest_finished);
    }
}

/// The notice a page shows for a job it asked about, and whether the page
/// should refresh itself to look again.
pub fn notice_for(job: &Job) -> (String, bool) {
    match &job.state {
        State::Running => (format!("{}… (this page refreshes by itself)", job.label.trim_end_matches(['.', '…'])), true),
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
    ReleaseKey { card: String, recovery: String, said: String },
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
        assert_eq!(notice_for(&jobs.look(id).unwrap()), ("Sent.".to_string(), false));
    }

    #[test]
    fn a_flash_is_for_its_page_once_and_runs_out() {
        let mut l = Vec::new();
        keep_flash(&mut l, Page::Friends, Flash::FriendLink("atlas:x".into()), 100);
        assert!(take_flash(&mut l, Page::Sync, 101).is_none());
        assert_eq!(take_flash(&mut l, Page::Friends, 101), Some(Flash::FriendLink("atlas:x".into())));
        assert!(take_flash(&mut l, Page::Friends, 102).is_none());
        keep_flash(&mut l, Page::Sync, Flash::Said("code".into()), 100);
        assert!(take_flash(&mut l, Page::Sync, 100 + FLASH_SECS + 1).is_none());
    }
}
