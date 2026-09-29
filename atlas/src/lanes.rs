//! Two lanes of work, so Atlas can be busy without making you wait.
//!
//! The constraint that shapes this: on Windows, synthetic clicks and
//! keystrokes go to whatever window has focus. Anything Atlas does that needs
//! *your* windows will take focus from you. There is no way around that at the
//! OS level.
//!
//! So work is split. Background work never touches your screen — web research
//! in a separate headless browser, searching indexed files, reading documents,
//! writing notes. It runs whenever, while you keep working. Foreground work —
//! clicking, scrolling, typing into a window, rearranging your layout — is
//! queued and waits until you are actually idle, or until you tell it to go
//! now.
//!
//! The result is that "do some research on X" runs immediately and invisibly,
//! and "fill in that form" waits for a gap.

use crate::awareness::Signals;
use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Never touches your screen. Runs any time.
    Background,
    /// Needs your windows. Waits for a gap.
    Foreground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Queued,
    /// Ready but waiting for you to stop typing.
    WaitingForGap,
    Running,
    Done,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: u64,
    pub command: String,
    pub lane: Lane,
    pub state: TaskState,
    pub created: u64,
    #[serde(default)]
    pub result: Option<String>,
    /// You said do it now — skip the idle wait.
    #[serde(default)]
    pub urgent: bool,
    /// Needs the internet. Waits for a connection instead of failing.
    #[serde(default)]
    pub needs_net: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct LaneConfig {
    /// Seconds of no keyboard, no mouse, no speech before foreground work is
    /// allowed to take over the screen.
    pub gap_secs: u64,
    /// How many background tasks may run at once.
    pub background_slots: usize,
    /// Give up on a foreground task that never got a gap, after this long.
    pub foreground_ttl_secs: u64,
}

impl Default for LaneConfig {
    fn default() -> Self {
        LaneConfig { gap_secs: 20, background_slots: 2, foreground_ttl_secs: 3600 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Queue {
    pub tasks: Vec<Task>,
    next_id: u64,
}

impl Queue {
    /// A task written down as `Running` and never updated again is not
    /// evidence it finished — with nothing to make the tick block until a
    /// job is done, `Running` used to last three lines inside one tick and
    /// never reach disk. Now that background work can genuinely run for
    /// minutes, a kill or a reboot mid-job leaves a task stranded: `Running`
    /// is skipped by `ready_with` (not `Queued`) and never reached by
    /// `finish` (nothing holds its id anymore). Swept back to `Queued` on
    /// load — it never got a chance, so it is not `Failed` either.
    pub fn load(store: &Store) -> Queue {
        let mut q: Queue = store.load("queue");
        for t in q.tasks.iter_mut() {
            if t.state == TaskState::Running {
                t.state = TaskState::Queued;
            }
        }
        q
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("queue", self)
    }

    pub fn push(&mut self, command: &str, lane: Lane) -> u64 {
        self.push_at(command, lane, now())
    }

    /// Time is injected so expiry is testable without waiting an hour.
    pub fn push_at(&mut self, command: &str, lane: Lane, t: u64) -> u64 {
        self.next_id += 1;
        self.tasks.push(Task {
            id: self.next_id,
            command: command.to_string(),
            lane,
            state: TaskState::Queued,
            created: t,
            result: None,
            urgent: false,
            needs_net: false,
        });
        self.next_id
    }

    /// Queue something that cannot run without a connection.
    pub fn push_online(&mut self, command: &str, lane: Lane) -> u64 {
        let id = self.push(command, lane);
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.needs_net = true;
        }
        id
    }

    /// Are you in the middle of something?
    fn busy(s: &Signals, cfg: &LaneConfig) -> bool {
        s.in_conversation || s.idle_secs < cfg.gap_secs
    }

    /// What may start right now.
    ///
    /// Background work ignores whether you are busy — that is the entire point
    /// of it. Foreground work only goes when the screen is genuinely free,
    /// unless you marked it urgent.
    pub fn ready(&mut self, s: &Signals, cfg: &LaneConfig, t: u64) -> Vec<u64> {
        self.ready_with(s, cfg, t, crate::connectivity::Reach::Online)
    }

    /// As `ready`, but nothing needing the internet starts while offline.
    /// Those tasks stay queued — they are not failed, and not forgotten.
    pub fn ready_with(
        &mut self,
        s: &Signals,
        cfg: &LaneConfig,
        t: u64,
        reach: crate::connectivity::Reach,
    ) -> Vec<u64> {
        let online = reach == crate::connectivity::Reach::Online;
        let busy = Queue::busy(s, cfg);
        let running_bg = self
            .tasks
            .iter()
            .filter(|x| x.state == TaskState::Running && x.lane == Lane::Background)
            .count();
        let mut slots = cfg.background_slots.saturating_sub(running_bg);

        // A foreground task already running owns the screen until it finishes.
        let fg_running = self
            .tasks
            .iter()
            .any(|x| x.state == TaskState::Running && x.lane == Lane::Foreground);

        let mut out = Vec::new();
        let mut fg_taken = fg_running;

        for task in self.tasks.iter_mut() {
            if !matches!(task.state, TaskState::Queued | TaskState::WaitingForGap) {
                continue;
            }
            if task.needs_net && !online {
                continue; // waits for a connection, keeps its place
            }
            match task.lane {
                Lane::Background => {
                    if slots == 0 {
                        continue;
                    }
                    slots -= 1;
                    task.state = TaskState::Running;
                    out.push(task.id);
                }
                Lane::Foreground => {
                    if fg_taken {
                        continue;
                    }
                    if busy && !task.urgent {
                        task.state = TaskState::WaitingForGap;
                        continue;
                    }
                    fg_taken = true;
                    task.state = TaskState::Running;
                    out.push(task.id);
                }
            }
        }

        // Don't hoard screen work forever if a gap never comes. Waiting for a
        // connection is different — that is not Atlas's fault and not
        // something a timeout should punish.
        for task in self.tasks.iter_mut() {
            if task.needs_net {
                continue;
            }
            if task.state == TaskState::WaitingForGap
                && t.saturating_sub(task.created) > cfg.foreground_ttl_secs
            {
                task.state = TaskState::Failed;
                task.result = Some("never got a free moment on screen".into());
            }
        }
        out
    }

    pub fn finish(&mut self, id: u64, result: &str, ok: bool) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.state = if ok { TaskState::Done } else { TaskState::Failed };
            t.result = Some(result.to_string());
        }
    }

    /// Jobs parked until the connection comes back.
    pub fn waiting_for_network(&self) -> Vec<&Task> {
        self.tasks
            .iter()
            .filter(|t| t.needs_net && matches!(t.state, TaskState::Queued | TaskState::WaitingForGap))
            .collect()
    }

    pub fn waiting_for_gap(&self) -> Vec<&Task> {
        self.tasks.iter().filter(|t| t.state == TaskState::WaitingForGap).collect()
    }

    pub fn pending(&self) -> usize {
        self.tasks
            .iter()
            .filter(|t| matches!(t.state, TaskState::Queued | TaskState::WaitingForGap | TaskState::Running))
            .count()
    }

    pub fn prune(&mut self) {
        self.tasks
            .retain(|t| !matches!(t.state, TaskState::Done | TaskState::Failed));
    }
}

/// Which lane a command belongs in. Anything that has to drive your windows is
/// foreground; everything else can happen out of sight.
pub fn lane_for(command: &str) -> Lane {
    let c = command.to_lowercase();
    const FOREGROUND: &[&str] = &[
        "workspace", "open ", "close ", "focus", "switch to", "move ",
        "click", "scroll", "type ", "paste", "view my display", "look at my screen",
    ];
    if FOREGROUND.iter().any(|k| c.contains(k)) {
        Lane::Foreground
    } else {
        Lane::Background
    }
}

#[cfg(test)]
mod stranded_running_tests {
    use super::*;
    use crate::store::Store;

    fn temp_store() -> Store {
        // One folder per call: two tests starting in the same second used
        // to share one and race each other (seen 26 Sep under load).
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "atlas_lanes_test_{}_{}_{}",
            std::process::id(),
            crate::store::now(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Store::new(dir)
    }

    #[test]
    fn a_task_stranded_running_across_a_restart_is_queued_again_not_lost() {
        let store = temp_store();
        let mut q = Queue::default();
        q.push("do the thing", Lane::Background);
        // Simulate what a crash or a kill mid-job leaves behind: a task
        // marked Running that nothing will ever update again, because the
        // id that would call `finish` on it is gone.
        q.tasks[0].state = TaskState::Running;
        q.save(&store).unwrap();

        let reloaded = Queue::load(&store);
        assert_eq!(
            reloaded.tasks[0].state,
            TaskState::Queued,
            "a stranded Running task must come back as Queued, not stay Running forever"
        );
    }

    #[test]
    fn a_genuinely_finished_task_is_not_disturbed_by_load() {
        let store = temp_store();
        let mut q = Queue::default();
        q.push("do the thing", Lane::Background);
        q.finish(1, "done", true);
        q.save(&store).unwrap();

        let reloaded = Queue::load(&store);
        assert_eq!(reloaded.tasks[0].state, TaskState::Done);
    }
}
