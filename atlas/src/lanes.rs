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
#[serde(into = "TaskWire", from = "TaskWire")]
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
    /// A previous execution ended without a confirmed outcome. Kept until
    /// explicitly dismissed, because replaying could duplicate side effects.
    #[serde(default)]
    pub interrupted: bool,
    /// Exact worker whose completion belongs to this queued request.
    #[serde(default)]
    pub worker_id: Option<u64>,
    #[serde(default)]
    pub file_move_id: Option<u64>,
    #[serde(default)]
    pub stop_requested: bool,
}

// Older releases replay Running records at startup. Persist a terminal legacy
// state with an additive fence, so rolling back cannot repeat an uncertain act.
#[derive(Serialize, Deserialize)]
struct TaskWire {
    id: u64,
    command: String,
    lane: Lane,
    state: TaskState,
    created: u64,
    #[serde(default)] result: Option<String>,
    #[serde(default)] urgent: bool,
    #[serde(default)] needs_net: bool,
    #[serde(default)] interrupted: bool,
    #[serde(default)] worker_id: Option<u64>,
    #[serde(default)] file_move_id: Option<u64>,
    #[serde(default)] stop_requested: bool,
    #[serde(default)] in_flight: bool,
    #[serde(default)] origin_waiting: Option<TaskState>,
}
impl From<Task> for TaskWire {
    fn from(t: Task) -> Self {
        let in_flight = t.state == TaskState::Running;
        Self { id: t.id, command: t.command, lane: t.lane,
            state: if in_flight { TaskState::Failed } else { t.state },
            created: t.created,
            result: if in_flight { Some("Previous execution outcome is unconfirmed; check before retrying.".into()) } else { t.result },
            urgent: t.urgent, needs_net: t.needs_net,
            interrupted: t.interrupted || in_flight, worker_id: t.worker_id,
            file_move_id: t.file_move_id, stop_requested: t.stop_requested, in_flight, origin_waiting: None }
    }
}
impl From<TaskWire> for Task {
    fn from(t: TaskWire) -> Self {
        Self { id: t.id, command: t.command, lane: t.lane,
            state: if t.in_flight { TaskState::Running } else { t.state },
            created: t.created, result: t.result, urgent: t.urgent,
            needs_net: t.needs_net, interrupted: t.interrupted,
            worker_id: t.worker_id, file_move_id: t.file_move_id, stop_requested: t.stop_requested }
    }
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

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(into = "QueueWire", from = "QueueWire")]
pub struct Queue {
    pub tasks: Vec<Task>,
    next_id: u64,
    origin_batches: std::collections::BTreeMap<String, OriginBatch>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
struct OriginBatch {
    ids: Vec<u64>,
    #[serde(default)] done: std::collections::BTreeSet<u64>,
    #[serde(default)] blocked: bool,
}

#[derive(Serialize, Deserialize)]
struct QueueWire {
    tasks: Vec<TaskWire>,
    next_id: u64,
    #[serde(default)] origin_batches: std::collections::BTreeMap<String, OriginBatch>,
}
impl From<Queue> for QueueWire {
    fn from(queue: Queue) -> Self {
        let tasks = queue.tasks.into_iter().map(|task| {
            let protected = queue.origin_batches.values().any(|batch| batch.ids.contains(&task.id)) && matches!(task.state, TaskState::Queued | TaskState::WaitingForGap);
            let mut wire = TaskWire::from(task);
            if protected { wire.origin_waiting = Some(wire.state); wire.state = TaskState::Failed; }
            wire
        }).collect();
        Self { tasks, next_id: queue.next_id, origin_batches: queue.origin_batches }
    }
}
impl From<QueueWire> for Queue {
    fn from(wire: QueueWire) -> Self {
        let tasks = wire.tasks.into_iter().map(|mut task| {
            if wire.origin_batches.values().any(|batch| batch.ids.contains(&task.id)) {
                if let Some(state @ (TaskState::Queued | TaskState::WaitingForGap)) = task.origin_waiting { task.state = state; }
            }
            Task::from(task)
        }).collect();
        Self { tasks, next_id: wire.next_id, origin_batches: wire.origin_batches }
    }
}

impl Queue {
    /// A task written down as `Running` and never updated again is not
    /// evidence it finished or did nothing. Never replay it automatically:
    /// the previous action may already have changed files or a service.
    pub fn load(store: &Store) -> Queue {
        Self::load_checked(store).unwrap_or_else(|error| panic!("the saved work queue could not be read safely: {error}"))
    }

    pub fn load_checked(store: &Store) -> Result<Queue> {
        let mut q: Queue = store.load_checked("queue")?.unwrap_or_default();
        for t in q.tasks.iter_mut() {
            if t.state == TaskState::Running {
                t.state = TaskState::Failed;
                t.interrupted = true;
                t.result = Some("Interrupted by a restart; the previous outcome is not confirmed. Check what happened before asking me to try again.".into());
            }
        }
        Ok(q)
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("queue", self)
    }

    pub fn push(&mut self, command: &str, lane: Lane) -> u64 {
        self.push_at(command, lane, now())
    }

    /// Origin and jobs share one durable record. Retained tombstones prevent a
    /// failed routine reservation save or a pruned terminal row from replaying.
    pub fn enqueue_origin_once(&mut self, key: &str, commands: &[String], lane: Lane, t: u64) -> (Vec<u64>, bool) {
        if let Some(batch) = self.origin_batches.get(key) { return (batch.ids.clone(), false); }
        let ids: Vec<_> = commands.iter().map(|command| self.push_at(command, lane, t)).collect();
        self.origin_batches.insert(key.to_owned(), OriginBatch { ids: ids.clone(), ..OriginBatch::default() });
        (ids, true)
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
            interrupted: false,
            worker_id: None,
            file_move_id: None,
            stop_requested: false,
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

    /// Persist the start fence before any action. A failed save restores the
    /// entire in-memory queue, so no caller receives permission to execute.
    pub fn ready_durably(&mut self, store: &Store, s: &Signals, cfg: &LaneConfig, t: u64, reach: crate::connectivity::Reach) -> Result<Vec<u64>> {
        let before = self.clone();
        let ids = self.ready_with(s, cfg, t, reach);
        if *self == before { return Ok(ids); }
        if let Err(e) = self.save(store) {
            *self = before;
            return Err(e);
        }
        Ok(ids)
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
        let mut allowed_origins = std::collections::HashSet::new();
        let mut blocked_origins = std::collections::HashSet::new();
        for batch in self.origin_batches.values_mut() {
            let mut waiting = None;
            for id in &batch.ids {
                if batch.done.contains(id) { continue; }
                match self.tasks.iter().find(|task| task.id == *id) {
                    Some(task) if task.state == TaskState::Done => { batch.done.insert(*id); }
                    Some(task) if task.state == TaskState::Failed || task.interrupted || task.stop_requested => { batch.blocked = true; break; }
                    Some(_) => { waiting = Some(*id); break; }
                    None => { batch.blocked = true; break; }
                }
            }
            if batch.blocked { blocked_origins.extend(batch.ids.iter().copied()); }
            else if let Some(id) = waiting { allowed_origins.insert(id); }
        }
        for task in &mut self.tasks {
            if blocked_origins.contains(&task.id) && matches!(task.state, TaskState::Queued | TaskState::WaitingForGap) {
                task.state = TaskState::Failed; task.interrupted = true;
                task.result = Some("Held because an earlier routine step failed, stopped, or has no confirmed outcome. Check before asking to retry.".into());
            }
        }
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
            if self.origin_batches.values().any(|batch| batch.ids.contains(&task.id)) && !allowed_origins.contains(&task.id) { continue; }
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
        for batch in self.origin_batches.values_mut().filter(|batch| batch.ids.contains(&id)) {
            if ok { batch.done.insert(id); } else { batch.blocked = true; }
        }
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.state = if ok { TaskState::Done } else { TaskState::Failed };
            t.result = Some(result.to_string());
            t.interrupted = false;
            t.worker_id = None;
            t.file_move_id = None;
            t.stop_requested = false;
        }
    }

    pub fn attach_worker(&mut self, id: u64, worker: u64, message: &str) {
        if let Some(task) = self.tasks.iter_mut().find(|task| task.id == id && task.state == TaskState::Running) {
            task.worker_id = Some(worker);
            task.result = Some(message.into());
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
            .retain(|t| t.interrupted || !matches!(t.state, TaskState::Done | TaskState::Failed));
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
    fn routine_origins_survive_partial_reservation_and_pruned_receipts() {
        let store = temp_store(); let mut queue = Queue::default();
        let commands = vec!["first".into(), "second".into()];
        let (ids, added) = queue.enqueue_origin_once("routine:test:day1", &commands, Lane::Background, 100);
        assert!(added); queue.save(&store).unwrap();
        // Queue persisted, independent routine last_day save never happened.
        let mut restarted = Queue::load_checked(&store).unwrap();
        assert!(!restarted.enqueue_origin_once("routine:test:day1", &commands, Lane::Background, 100).1);
        assert_eq!(restarted.tasks.len(), 2);
        restarted.finish(ids[0], "finished", true); restarted.prune(); restarted.save(&store).unwrap();
        let mut restarted = Queue::load_checked(&store).unwrap();
        assert!(restarted.origin_batches["routine:test:day1"].done.contains(&ids[0]));
        assert_eq!(restarted.ready(&Signals::default(), &LaneConfig::default(), 101), vec![ids[1]]);
        assert!(!restarted.enqueue_origin_once("routine:test:day1", &commands, Lane::Background, 100).1);
    }

    #[test]
    fn older_readers_cannot_execute_origin_steps_without_their_dependencies() {
        let mut queue = Queue::default(); queue.enqueue_origin_once("daily", &["one".into(), "two".into()], Lane::Background, 100);
        let wire = serde_json::to_value(&queue).unwrap();
        assert!(wire["tasks"].as_array().unwrap().iter().all(|task| task["state"] == "failed"));
        let restored: Queue = serde_json::from_value(wire).unwrap();
        assert!(restored.tasks.iter().all(|task| task.state == TaskState::Queued));
    }

    #[test]
    fn routine_steps_require_confirmed_predecessor_and_hold_after_failure() {
        let mut queue = Queue::default(); let (ids, _) = queue.enqueue_origin_once("daily", &["one".into(), "two".into()], Lane::Background, 100);
        let signals = Signals::default(); let cfg = LaneConfig { background_slots: 10, ..LaneConfig::default() };
        assert_eq!(queue.ready(&signals, &cfg, 101), vec![ids[0]]);
        assert!(queue.ready(&signals, &cfg, 102).is_empty());
        queue.finish(ids[0], "failed", false);
        assert!(queue.ready(&signals, &cfg, 103).is_empty());
        assert!(queue.tasks.iter().find(|task| task.id == ids[1]).unwrap().interrupted);
    }

    #[test]
    fn missing_routine_predecessor_is_unknown_and_never_skipped() {
        let mut queue = Queue::default(); let (ids, _) = queue.enqueue_origin_once("daily", &["one".into(), "two".into()], Lane::Background, 100);
        queue.tasks.retain(|task| task.id != ids[0]);
        assert!(queue.ready(&Signals::default(), &LaneConfig::default(), 101).is_empty());
        assert!(queue.tasks[0].interrupted);
    }

    #[test]
    fn corrupt_origin_storage_is_not_rebuilt_as_an_empty_queue() {
        let store = temp_store(); std::fs::write(store.root().join("queue.json"), b"{broken").unwrap();
        assert!(Queue::load_checked(&store).is_err());
        assert_eq!(std::fs::read(store.root().join("queue.json")).unwrap(), b"{broken");
    }

    #[test]
    fn a_task_stranded_running_across_a_restart_is_not_repeated() {
        let store = temp_store();
        let mut q = Queue::default();
        q.push("do the thing", Lane::Background);
        // Simulate what a crash or a kill mid-job leaves behind: a task
        // marked Running that nothing will ever update again, because the
        // id that would call `finish` on it is gone.
        q.tasks[0].state = TaskState::Running;
        q.save(&store).unwrap();

        let mut reloaded = Queue::load(&store);
        assert_eq!(
            reloaded.tasks[0].state,
            TaskState::Failed,
            "an unknown previous outcome must never automatically repeat an action"
        );
        assert!(reloaded.tasks[0].result.as_deref().unwrap().contains("not confirmed"));
        assert!(reloaded.ready(&Signals::default(), &LaneConfig::default(), crate::store::now()).is_empty());
        reloaded.save(&store).unwrap();
        assert_eq!(Queue::load(&store).tasks[0].state, TaskState::Failed);
        reloaded.prune();
        assert_eq!(reloaded.tasks.len(), 1, "unknown outcomes remain available for review");
    }

    #[test]
    fn queued_work_cannot_start_without_a_durable_fence() {
        let store = temp_store();
        let mut q = Queue::default();
        q.push("change something", Lane::Background);
        std::fs::create_dir(store.root().join("queue.json")).unwrap();
        assert!(q.ready_durably(&store, &Signals::default(), &LaneConfig::default(), crate::store::now(), crate::connectivity::Reach::Online).is_err());
        assert_eq!(q.tasks[0].state, TaskState::Queued);
        std::fs::remove_dir(store.root().join("queue.json")).unwrap();
        assert_eq!(q.ready_durably(&store, &Signals::default(), &LaneConfig::default(), crate::store::now(), crate::connectivity::Reach::Online).unwrap(), vec![1]);
        let restarted = Queue::load(&store);
        assert!(restarted.tasks[0].interrupted);
        assert_eq!(restarted.tasks[0].state, TaskState::Failed);
    }

    #[test]
    fn an_unchanged_queue_does_not_attempt_a_state_write() {
        let store = temp_store();
        std::fs::create_dir(store.root().join("queue.json")).unwrap();
        let mut queue = Queue::default();
        assert!(queue.ready_durably(&store, &Signals::default(), &LaneConfig::default(), 1, crate::connectivity::Reach::Online).unwrap().is_empty());
    }

    #[test]
    fn running_fence_is_terminal_to_an_older_reader() {
        let mut q = Queue::default();
        q.push("publish once", Lane::Background);
        q.tasks[0].state = TaskState::Running;
        let bytes = serde_json::to_vec(&q).unwrap();
        let legacy: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(legacy["tasks"][0]["state"], "failed");
        assert_eq!(legacy["tasks"][0]["interrupted"], true);
        let current: Queue = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(current.tasks[0].state, TaskState::Running);
        assert!(current.tasks[0].interrupted);
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
