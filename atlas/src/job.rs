//! Shared durable-job vocabulary for the local-first foundation.
//!
//! Domains keep their own payloads and side effects; this module owns only the
//! lifecycle envelope. A single transition gate prevents one domain from
//! claiming success after cancellation or turning an unresolved external
//! result into a false completion.

use serde::{Deserialize, Serialize};

const STORE_NAME: &str = "jobs";
/// The start of every crew-errand record's job id (`Daemon::hand_off_as`),
/// so the Hub can tell an errand's record from a research or build job.
pub const ERRAND_PREFIX: &str = "errand:";
/// A durable receipt journal must not grow with every task forever. Active
/// jobs are never evicted; terminal history is trimmed oldest-first once the
/// bounded local journal reaches this limit.
pub const MAX_RECORDS: usize = 1_024;
const MAX_JOURNAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Planned,
    AwaitingApproval,
    Running,
    Checkpointed,
    Cancelling,
    Cancelled,
    Succeeded,
    Failed,
    Uncertain,
    RolledBack,
}

impl State {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Cancelled | Self::Succeeded | Self::Failed | Self::Uncertain | Self::RolledBack
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub job_id: String,
    pub state: State,
    pub stage: String,
    pub detail: String,
    pub provenance: String,
    #[serde(default)]
    pub artifacts: Option<crate::artifact::Envelope>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Record {
    pub job_id: String,
    pub state: State,
    pub stage: String,
    pub checkpoint: Option<String>,
    pub receipt: Option<Receipt>,
    /// Last durable lifecycle/progress update, used by recovery inspection.
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub artifacts: Option<crate::artifact::Envelope>,
    /// The owner pressed "Drop it" on this ended job (20 Oct 2026). Only a
    /// finished record can be dismissed, and it is kept rather than deleted:
    /// it just stops being shown as something still to look at.
    #[serde(default)]
    pub dismissed: bool,
    /// Atlas working on itself (21 Oct 2026): still written down so it ends
    /// as done, failed or uncertain like any other work, but it is closed
    /// already-reviewed so it does not turn up in the owner's lanes.
    #[serde(default)]
    pub internal: bool,
}

impl Record {
    pub fn planned(job_id: impl Into<String>, stage: impl Into<String>) -> Self {
        Self {
            job_id: job_id.into(),
            state: State::Planned,
            stage: stage.into(),
            checkpoint: None,
            receipt: None,
            updated_at: crate::store::now(),
            artifacts: None,
            dismissed: false,
            internal: false,
        }
    }

    pub fn transition(&mut self, next: State, detail: impl Into<String>) -> Result<(), String> {
        if self.state.terminal() {
            return Err(format!("job {} is already terminal", self.job_id));
        }
        if self.state == next {
            self.updated_at = crate::store::now();
            if next == State::Checkpointed {
                self.checkpoint = Some(detail.into());
            }
            return Ok(());
        }
        if !allowed(self.state, next) {
            return Err(format!(
                "cannot move job from {:?} to {:?}",
                self.state, next
            ));
        }
        self.state = next;
        self.updated_at = crate::store::now();
        if next == State::Checkpointed {
            self.checkpoint = Some(detail.into());
        }
        Ok(())
    }

    pub fn finish(
        &mut self,
        state: State,
        detail: impl Into<String>,
        provenance: impl Into<String>,
    ) -> Result<Receipt, String> {
        if !state.terminal() {
            return Err("finish requires a terminal state".into());
        }
        let detail = detail.into();
        self.transition(state, detail.clone())?;
        let receipt = Receipt {
            job_id: self.job_id.clone(),
            state,
            stage: self.stage.clone(),
            detail,
            provenance: provenance.into(),
            artifacts: self.artifacts.clone(),
        };
        self.receipt = Some(receipt.clone());
        if self.internal {
            self.dismissed = true;
        }
        Ok(receipt)
    }

    pub fn cancel(
        &mut self,
        detail: impl Into<String>,
        provenance: impl Into<String>,
    ) -> Result<Receipt, String> {
        let detail = detail.into();
        // Work that has not started has no external side effect to unwind;
        // move it directly to a cancelled receipt. Once execution began,
        // retain the explicit Cancelling phase so recovery can distinguish
        // an in-flight stop from a queued item that never ran.
        if matches!(self.state, State::Running | State::Checkpointed) {
            self.transition(State::Cancelling, detail.clone())?;
        }
        self.finish(State::Cancelled, detail, provenance)
    }

    /// Record progress without changing lifecycle state.
    pub fn heartbeat(&mut self, detail: impl Into<String>) {
        self.updated_at = crate::store::now();
        self.checkpoint = Some(detail.into());
    }

    /// Translate the existing daemon outcome vocabulary into the durable
    /// lifecycle without letting callers skip the transition gate.
    pub fn apply_outcome(
        &mut self,
        outcome: &crate::taskloop::Outcome,
        provenance: impl Into<String> + Clone,
    ) -> Result<Option<Receipt>, String> {
        match outcome {
            crate::taskloop::Outcome::Started(detail) => {
                self.transition(State::Running, detail.clone())?;
                Ok(None)
            }
            crate::taskloop::Outcome::NeedsYou(detail) => {
                self.transition(State::AwaitingApproval, detail.clone())?;
                Ok(None)
            }
            crate::taskloop::Outcome::Done(detail) => Ok(Some(self.finish(
                State::Succeeded,
                detail.clone(),
                provenance.clone(),
            )?)),
            crate::taskloop::Outcome::Failed(detail) => Ok(Some(self.finish(
                State::Failed,
                detail.clone(),
                provenance,
            )?)),
        }
    }

    pub fn stale(&self, now: u64, after_secs: u64) -> bool {
        !self.state.terminal()
            && self.updated_at > 0
            && now.saturating_sub(self.updated_at) >= after_secs
    }
}

/// Read the job journal without defaulting corrupt or unsupported state to an
/// empty list. Recovery and writes use this boundary so a damaged journal is
/// preserved for inspection instead of being replaced by a new empty one.
pub fn load_checked(store: &crate::store::Store) -> crate::error::Result<Vec<Record>> {
    Ok(store
        .load_checked_bounded::<Vec<Record>>(STORE_NAME, 4 * 1024 * 1024)?
        .unwrap_or_default())
}

type Seen = (std::time::SystemTime, u64, Vec<Record>);

/// The journal as `load_checked` reads it, remembered by the file's time and
/// size (20 Oct 2026). The Hub's header pill is drawn on every page and now
/// counts from the same lanes Outstanding shows, which include these records;
/// without this each page view would read and parse up to 4 MB again. One
/// `metadata` call per page tells whether anything was saved since; a changed
/// file is read afresh, and a file that will not read is never remembered, so
/// a damaged journal still says so every time.
pub fn load_cached(store: &crate::store::Store) -> crate::error::Result<Vec<Record>> {
    static SEEN: std::sync::Mutex<Option<std::collections::HashMap<std::path::PathBuf, Seen>>> =
        std::sync::Mutex::new(None);
    let path = store.root().join(format!("{STORE_NAME}.json"));
    let stamp = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok().map(|t| (t, m.len())));
    let Some((time, len)) = stamp else {
        // No file (nothing ever recorded), or one that cannot be inspected:
        // the checked read decides what that means.
        return load_checked(store);
    };
    if let Ok(guard) = SEEN.lock().or_else(crate::crash::unpoison) {
        if let Some((t, l, records)) = guard.as_ref().and_then(|m| m.get(&path)) {
            if *t == time && *l == len {
                return Ok(records.clone());
            }
        }
    }
    let records = load_checked(store)?;
    if let Ok(mut guard) = SEEN.lock().or_else(crate::crash::unpoison) {
        let map = guard.get_or_insert_with(Default::default);
        if map.len() > 32 {
            map.clear();
        }
        map.insert(path, (time, len, records.clone()));
    }
    Ok(records)
}

/// Write down that a long errand has begun, so a restart that cuts it off
/// leaves a `Running` record for `reconcile_after_restart` to turn into an
/// uncertain one (20 Oct 2026: a download in progress vanished across a full
/// restart without a trace, because only research and builds were recorded).
pub fn begin(
    store: &crate::store::Store,
    job_id: &str,
    stage: &str,
    detail: &str,
) -> crate::error::Result<()> {
    begin_as(store, job_id, stage, detail, false)
}

/// As `begin`, saying whether this is Atlas working on itself (`Record::internal`).
pub(crate) fn begin_as(
    store: &crate::store::Store,
    job_id: &str,
    stage: &str,
    detail: &str,
    internal: bool,
) -> crate::error::Result<()> {
    let mut record = Record::planned(job_id, stage);
    record.internal = internal;
    record.transition(State::Running, detail).map_err(|error| {
        crate::error::AtlasError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })?;
    upsert(store, record)
}

/// Close a record opened by `begin` with how it ended. A record that was
/// never begun (housekeeping, which is only written down when it fails) is
/// created and closed in one step. An already-terminal record is left alone.
pub fn end(
    store: &crate::store::Store,
    job_id: &str,
    stage: &str,
    state: State,
    detail: &str,
) -> crate::error::Result<()> {
    finish(store, job_id, stage, state, detail, "crew errand")
}

/// The one way any writer (crew errands, Hub jobs, the remote queue) gives a
/// job its ending (21 Oct 2026). The terminal set is `Succeeded`, `Failed`,
/// `Cancelled`, `RolledBack` and `Uncertain` (the "interrupted, check before
/// repeating" state); a job waiting on the owner is `AwaitingApproval`, which
/// is not terminal and is set with `Record::transition`. Not a terminal state
/// is refused; an already-terminal record is left alone; a record that was
/// never begun is created and closed in one step.
pub fn finish(
    store: &crate::store::Store,
    job_id: &str,
    stage: &str,
    state: State,
    detail: &str,
    provenance: &str,
) -> crate::error::Result<()> {
    if !state.terminal() {
        return Err(crate::error::AtlasError::Config(
            "a job ending must be a terminal state".into(),
        ));
    }
    let mut records = load_checked(store)?;
    let at = match records.iter().position(|r| r.job_id == job_id) {
        Some(at) => at,
        None => {
            let mut fresh = Record::planned(job_id, stage);
            // A record that was never begun has nothing to be cut off: it is
            // closed at once.
            // unheard-ok: Planned to Running is always allowed, so this cannot fail
            let _ = fresh.transition(State::Running, "started");
            records.push(fresh);
            records.len() - 1
        }
    };
    if records[at].state.terminal() {
        return Ok(());
    }
    // A stop goes through `cancel`: Running has no direct edge to Cancelled,
    // only through Cancelling, so `finish` alone left a stopped errand's
    // record Running forever.
    let closed = if state == State::Cancelled {
        records[at].cancel(detail, provenance)
    } else {
        records[at].finish(state, detail, provenance)
    };
    closed.map_err(|error| {
        crate::error::AtlasError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })?;
    save(store, &records)
}

/// Mark an ended job as reviewed, so it leaves the Failed and Interrupted
/// lanes. `false` when there is no such finished record.
pub fn dismiss(store: &crate::store::Store, job_id: &str) -> crate::error::Result<bool> {
    let mut records = load_checked(store)?;
    let Some(record) = records
        .iter_mut()
        .find(|r| r.job_id == job_id && r.state.terminal())
    else {
        return Ok(false);
    };
    record.dismissed = true;
    save(store, &records)?;
    Ok(true)
}

pub fn save(store: &crate::store::Store, records: &[Record]) -> crate::error::Result<()> {
    // Job receipts are written by both the control loop and background
    // workers. A worker must wait briefly for the daemon's grouped state
    // transaction instead of turning a successful action into a false
    // "state busy" research failure.
    let mut value = records.to_vec();
    for record in &value {
        if let Some(envelope) = &record.artifacts {
            envelope
                .validate()
                .map_err(crate::error::AtlasError::Config)?;
        }
        if let Some(receipt) = &record.receipt {
            if let Some(envelope) = &receipt.artifacts {
                envelope
                    .validate()
                    .map_err(crate::error::AtlasError::Config)?;
            }
        }
    }
    bound(&mut value)?;
    let encoded = serde_json::to_vec(&value)
        .map_err(|error| crate::error::AtlasError::Config(format!("job encode: {error}")))?;
    if encoded.len() > MAX_JOURNAL_BYTES {
        return Err(crate::error::AtlasError::Config(
            "durable job journal exceeds the local size bound".into(),
        ));
    }
    let mut last = None;
    // A research worker can be handing a terminal receipt back at the same
    // moment the next foreground request starts. Keep this bounded and local
    // to the receipt journal; do not make every foreground Store save wait.
    for _ in 0..4 {
        match store.keep_briefly(STORE_NAME, &value) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last = Some(error);
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
        }
    }
    match last {
        Some(error) => Err(error),
        None => Err(crate::error::AtlasError::Config(
            "durable job save made no attempt".into(),
        )),
    }
}

pub fn upsert(store: &crate::store::Store, record: Record) -> crate::error::Result<()> {
    let mut records = load_checked(store)?;
    if let Some(existing) = records.iter_mut().find(|old| old.job_id == record.job_id) {
        *existing = record;
    } else {
        records.push(record);
    }
    save(store, &records)
}

fn bound(records: &mut Vec<Record>) -> crate::error::Result<()> {
    if records.len() <= MAX_RECORDS {
        return Ok(());
    }

    let active = records
        .iter()
        .filter(|record| !record.state.terminal())
        .count();
    if active > MAX_RECORDS {
        return Err(crate::error::AtlasError::Config(format!(
            "durable job journal has {active} active records; refusing to evict live state"
        )));
    }

    let keep_active: Vec<Record> = records
        .iter()
        .filter(|record| !record.state.terminal())
        .cloned()
        .collect();
    let mut terminal: Vec<Record> = records
        .iter()
        .filter(|record| record.state.terminal())
        .cloned()
        .collect();
    terminal.sort_by_key(|record| record.updated_at);
    let keep_terminal = MAX_RECORDS - active;
    if terminal.len() > keep_terminal {
        terminal.drain(..terminal.len() - keep_terminal);
    }
    records.clear();
    records.extend(keep_active);
    records.extend(terminal);
    Ok(())
}

/// Convert work that was nonterminal when the process stopped into an
/// explicit uncertain receipt. A restart cannot safely claim that a worker
/// finished, and it must not silently replay an external side effect.
pub fn reconcile_after_restart(store: &crate::store::Store) -> crate::error::Result<Vec<Record>> {
    reconcile_after_restart_except(store, None)
}

/// Reconcile non-terminal jobs while leaving a job created by the currently
/// running daemon alone. The first tick can race with a request started just
/// before it; that request is not evidence of a previous process dying.
fn reconcile_after_restart_except(
    store: &crate::store::Store,
    except_job_id: Option<&str>,
) -> crate::error::Result<Vec<Record>> {
    let mut records = load_checked(store)?;
    let mut changed = Vec::new();
    for record in &mut records {
        if record.state.terminal() || Some(record.job_id.as_str()) == except_job_id {
            continue;
        }
        record
            .finish(
                State::Uncertain,
                "process restarted before a terminal receipt",
                "restart reconciliation",
            )
            .map_err(|error| {
                crate::error::AtlasError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    error,
                ))
            })?;
        changed.push(record.clone());
    }
    if !changed.is_empty() {
        save(store, &records)?;
    }
    Ok(changed)
}

fn allowed(from: State, to: State) -> bool {
    matches!(
        (from, to),
        (
            State::Planned,
            State::AwaitingApproval
                | State::Running
                | State::Failed
                | State::Cancelled
                | State::Uncertain
        ) | (
            State::AwaitingApproval,
            State::Running | State::Succeeded | State::Failed | State::Cancelled | State::Uncertain
        ) | (
            State::Running,
            State::AwaitingApproval
                | State::Checkpointed
                | State::Cancelling
                | State::Succeeded
                | State::Failed
                | State::Uncertain
        ) | (
            State::Checkpointed,
            State::AwaitingApproval
                | State::Running
                | State::Cancelling
                | State::Succeeded
                | State::Failed
                | State::Uncertain
        ) | (State::Cancelling, State::Cancelled | State::Uncertain)
    )
}

#[cfg(test)]
mod tests {
    use super::{load_checked, upsert, Record, State, MAX_RECORDS};
    use crate::brain::Llm;
    use crate::error::Result as AtlasResult;
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};

    /// A deterministic local-provider fixture. It is deliberately kept in the
    /// durable-job tests: provider output is data, while lifecycle truth still
    /// belongs to the shared job transition gate.
    struct ScriptedLocalProvider {
        replies: Mutex<VecDeque<AtlasResult<String>>>,
        calls: Arc<Mutex<usize>>,
    }

    impl ScriptedLocalProvider {
        fn new(
            replies: impl IntoIterator<Item = AtlasResult<String>>,
        ) -> (Self, Arc<Mutex<usize>>) {
            let calls = Arc::new(Mutex::new(0));
            (
                Self {
                    replies: Mutex::new(replies.into_iter().collect()),
                    calls: Arc::clone(&calls),
                },
                calls,
            )
        }
    }

    impl Llm for ScriptedLocalProvider {
        fn complete(&self, _: &str, _: &str) -> AtlasResult<String> {
            *self.calls.lock().unwrap() += 1;
            self.replies.lock().unwrap().pop_front().unwrap_or_else(|| {
                Err(crate::error::AtlasError::Platform(
                    "local fixture exhausted".into(),
                ))
            })
        }
    }

    #[test]
    fn a_job_cannot_claim_success_after_cancellation() {
        let mut job = Record::planned("one", "write");
        job.transition(State::Cancelled, "owner stopped").unwrap();
        assert!(job.finish(State::Succeeded, "done", "local").is_err());
    }

    #[test]
    fn scripted_local_provider_success_becomes_one_durable_receipt() {
        let (provider, calls) = ScriptedLocalProvider::new([Ok("local answer".into())]);
        let mut job = Record::planned("provider-success", "local inference");
        job.transition(State::Running, "provider started").unwrap();
        let answer = provider.complete("system", "user").unwrap();
        let receipt = job
            .finish(State::Succeeded, answer, "local-provider")
            .unwrap();
        assert_eq!(receipt.detail, "local answer");
        assert_eq!(*calls.lock().unwrap(), 1);
        assert!(job.state.terminal());
    }

    #[test]
    fn scripted_local_provider_failure_is_a_failed_receipt_not_a_success() {
        let (provider, calls) = ScriptedLocalProvider::new([Err(
            crate::error::AtlasError::Platform("provider unavailable".into()),
        )]);
        let mut job = Record::planned("provider-failure", "local inference");
        job.transition(State::Running, "provider started").unwrap();
        let error = provider.complete("system", "user").unwrap_err();
        let receipt = job
            .finish(State::Failed, error.to_string(), "local-provider")
            .unwrap();
        assert_eq!(receipt.state, State::Failed);
        assert!(receipt.detail.contains("provider unavailable"));
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn cancelled_local_provider_completion_cannot_replay_after_restart() {
        let (provider, calls) = ScriptedLocalProvider::new([Ok("late answer".into())]);
        let mut job = Record::planned("provider-cancel", "local inference");
        job.transition(State::Running, "provider started").unwrap();
        let receipt = job.cancel("owner stopped", "local-provider").unwrap();
        assert_eq!(receipt.state, State::Cancelled);
        let late = provider.complete("system", "user").unwrap();
        assert!(job
            .finish(State::Succeeded, late, "local-provider")
            .is_err());
        assert_eq!(job.receipt.as_ref().unwrap().state, State::Cancelled);
        assert_eq!(*calls.lock().unwrap(), 1);
    }

    #[test]
    fn local_provider_restart_reconciliation_keeps_late_completion_uncertain() {
        let root =
            std::env::temp_dir().join(format!("atlas-job-provider-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let (provider, calls) = ScriptedLocalProvider::new([Ok("late local answer".into())]);
        let mut job = Record::planned("provider-restart", "local inference");
        job.transition(State::Running, "local provider started")
            .unwrap();
        upsert(&store, job).unwrap();

        let reconciled = super::reconcile_after_restart(&store).unwrap();
        assert_eq!(reconciled.len(), 1);
        assert_eq!(reconciled[0].state, State::Uncertain);
        assert!(reconciled[0]
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.state == State::Uncertain));

        let late = provider.complete("system", "user").unwrap();
        let mut restored = super::load_checked(&store).unwrap().remove(0);
        assert!(restored
            .finish(State::Succeeded, late, "local-provider")
            .is_err());
        assert_eq!(restored.state, State::Uncertain);
        assert_eq!(*calls.lock().unwrap(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn scripted_local_provider_multistage_work_reloads_with_checkpoint_and_artifacts() {
        let root =
            std::env::temp_dir().join(format!("atlas-job-provider-stages-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let (provider, calls) =
            ScriptedLocalProvider::new([Ok("outline".into()), Ok("final answer".into())]);
        let mut job = Record::planned("provider-stages", "local workflow");
        job.artifacts = Some(crate::artifact::Envelope {
            input: vec![crate::artifact::ArtifactRef::new("prompt-1", "prompt", None).unwrap()],
            output: vec![],
            depends_on: vec![],
        });
        job.transition(State::Running, "outline started").unwrap();
        upsert(&store, job.clone()).unwrap();
        assert_eq!(provider.complete("system", "outline").unwrap(), "outline");
        job.transition(State::Checkpointed, "outline complete")
            .unwrap();
        upsert(&store, job.clone()).unwrap();

        let final_answer = provider.complete("system", "final").unwrap();
        job.artifacts
            .as_mut()
            .unwrap()
            .output
            .push(crate::artifact::ArtifactRef::new("answer-1", "answer", None).unwrap());
        let receipt = job
            .finish(State::Succeeded, final_answer, "local-provider")
            .unwrap();
        upsert(&store, job).unwrap();

        let restored = load_checked(&store).unwrap();
        assert_eq!(restored.len(), 1);
        assert_eq!(restored[0].checkpoint.as_deref(), Some("outline complete"));
        assert_eq!(restored[0].receipt, Some(receipt));
        assert_eq!(restored[0].artifacts.as_ref().unwrap().output.len(), 1);
        assert_eq!(*calls.lock().unwrap(), 2);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_checkpoint_is_kept_in_the_shared_record_and_receipt() {
        let mut job = Record::planned("one", "research");
        job.transition(State::Running, "started").unwrap();
        job.transition(State::Checkpointed, "source-1-read")
            .unwrap();
        let receipt = job
            .finish(State::Succeeded, "saved", "local-index")
            .unwrap();
        assert_eq!(job.checkpoint.as_deref(), Some("source-1-read"));
        assert_eq!(receipt.job_id, "one");
    }

    #[test]
    fn terminal_records_cannot_be_reused() {
        let mut job = Record::planned("one", "work");
        job.finish(State::Failed, "no model", "broker").unwrap();
        assert!(job.transition(State::Running, "retry").is_err());
    }

    #[test]
    fn terminal_state_identifies_receipt_states_only() {
        assert!(State::Succeeded.terminal());
        assert!(State::Failed.terminal());
        assert!(State::Cancelled.terminal());
        assert!(State::Uncertain.terminal());
        assert!(!State::Running.terminal());
        assert!(!State::Checkpointed.terminal());
    }

    #[test]
    fn a_stopped_errand_ends_cancelled_not_running_forever() {
        let root = std::env::temp_dir().join(format!("atlas-job-stopped-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        let store = crate::store::Store::new(&root);
        super::begin(&store, "errand:stop", "backup", "started").unwrap();
        super::end(
            &store,
            "errand:stop",
            "backup",
            State::Cancelled,
            "Stopped before it finished.",
        )
        .unwrap();
        let records = super::load_checked(&store).unwrap();
        let record = records.iter().find(|r| r.job_id == "errand:stop").unwrap();
        assert_eq!(record.state, State::Cancelled);
        assert!(record.receipt.is_some());
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
    }

    #[test]
    fn cancellation_has_a_distinct_terminal_receipt() {
        let mut job = Record::planned("one", "work");
        job.transition(State::Running, "started").unwrap();
        let receipt = job.cancel("owner stopped", "taskloop").unwrap();
        assert_eq!(job.state, State::Cancelled);
        assert_eq!(receipt.state, State::Cancelled);
    }

    #[test]
    fn queued_and_approval_waiting_jobs_can_be_cancelled_without_running() {
        for state in [State::Planned, State::AwaitingApproval] {
            let mut job = Record::planned("one", "work");
            if state == State::AwaitingApproval {
                job.transition(state, "owner approval").unwrap();
            }
            let receipt = job
                .cancel("owner stopped before execution", "taskloop")
                .unwrap();
            assert_eq!(job.state, State::Cancelled);
            assert_eq!(receipt.state, State::Cancelled);
        }
    }

    #[test]
    fn records_survive_a_store_reload_and_upsert_does_not_duplicate() {
        let root = std::env::temp_dir().join(format!("atlas-job-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        upsert(&store, Record::planned("one", "research")).unwrap();
        let mut updated = Record::planned("one", "research");
        updated.transition(State::Running, "started").unwrap();
        upsert(&store, updated).unwrap();
        let records = load_checked(&store).unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].state, State::Running);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn checked_job_reads_refuse_corrupt_state_without_replacing_it() {
        let root = std::env::temp_dir().join(format!("atlas-job-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        std::fs::write(root.join("jobs.json"), b"{not-json").unwrap();

        assert!(super::load_checked(&store).is_err());
        assert_eq!(std::fs::read(root.join("jobs.json")).unwrap(), b"{not-json");
        assert!(super::upsert(&store, Record::planned("one", "work")).is_err());
        assert_eq!(std::fs::read(root.join("jobs.json")).unwrap(), b"{not-json");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn restart_reconciliation_never_replays_nonterminal_work() {
        let root = std::env::temp_dir().join(format!("atlas-job-restart-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let mut record = Record::planned("one", "research");
        record.transition(State::Running, "started").unwrap();
        upsert(&store, record).unwrap();
        let changed = super::reconcile_after_restart(&store).unwrap();
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0].state, State::Uncertain);
        assert!(changed[0]
            .receipt
            .as_ref()
            .unwrap()
            .provenance
            .contains("restart"));
        assert!(super::reconcile_after_restart(&store).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn heartbeat_updates_progress_without_claiming_completion() {
        let mut record = Record::planned("one", "research");
        let before = record.updated_at;
        record.heartbeat("source 1 complete");
        assert_eq!(record.state, State::Planned);
        assert_eq!(record.checkpoint.as_deref(), Some("source 1 complete"));
        assert!(record.updated_at >= before);
    }

    #[test]
    fn artifact_envelope_survives_job_completion_and_reload() {
        let mut record = Record::planned("one", "research");
        record.artifacts = Some(crate::artifact::Envelope {
            input: vec![crate::artifact::ArtifactRef::new(
                "source-1",
                "research_source",
                Some("sha256:input".into()),
            )
            .unwrap()],
            output: vec![crate::artifact::ArtifactRef::new(
                "report-1",
                "report",
                Some("sha256:output".into()),
            )
            .unwrap()],
            depends_on: vec!["prior-job".into()],
        });
        record.transition(State::Running, "started").unwrap();
        let receipt = record.finish(State::Succeeded, "saved", "local").unwrap();
        assert_eq!(receipt.artifacts, record.artifacts);

        let encoded = serde_json::to_vec(&record).unwrap();
        let restored: Record = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(restored.artifacts, record.artifacts);
    }

    #[test]
    fn taskloop_outcomes_follow_the_durable_approval_and_receipt_path() {
        let mut record = Record::planned("one", "write");
        assert!(record
            .apply_outcome(
                &crate::taskloop::Outcome::Started("started".into()),
                "local"
            )
            .unwrap()
            .is_none());
        assert!(record
            .apply_outcome(
                &crate::taskloop::Outcome::NeedsYou("approve the write".into()),
                "local"
            )
            .unwrap()
            .is_none());
        assert!(record
            .apply_outcome(&crate::taskloop::Outcome::Done("saved".into()), "local")
            .unwrap()
            .is_some());
        assert_eq!(record.state, State::Succeeded);
        assert!(record
            .apply_outcome(
                &crate::taskloop::Outcome::Failed("late failure".into()),
                "local"
            )
            .is_err());
    }

    #[test]
    fn job_journal_bounds_terminal_history_but_keeps_active_work() {
        let mut records = Vec::with_capacity(MAX_RECORDS + 1);
        for n in 0..MAX_RECORDS {
            let mut record = Record::planned(format!("done-{n}"), "work");
            record.transition(State::Running, "started").unwrap();
            record.finish(State::Succeeded, "done", "test").unwrap();
            record.updated_at = n as u64;
            records.push(record);
        }
        let mut active = Record::planned("still-running", "work");
        active.updated_at = u64::MAX;
        records.push(active);

        super::bound(&mut records).unwrap();

        assert_eq!(records.len(), MAX_RECORDS);
        assert!(records
            .iter()
            .any(|record| record.job_id == "still-running"));
        assert!(!records.iter().any(|record| record.job_id == "done-0"));
    }

    #[test]
    fn public_save_enforces_the_journal_bound() {
        let root =
            std::env::temp_dir().join(format!("atlas-job-save-bound-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let mut records = Vec::with_capacity(MAX_RECORDS + 1);
        for n in 0..=MAX_RECORDS {
            let mut record = Record::planned(format!("done-{n}"), "work");
            record.transition(State::Running, "started").unwrap();
            record.finish(State::Succeeded, "done", "test").unwrap();
            record.updated_at = n as u64;
            records.push(record);
        }

        super::save(&store, &records).unwrap();
        let saved = super::load_checked(&store).unwrap();

        assert_eq!(saved.len(), MAX_RECORDS);
        assert!(!saved.iter().any(|record| record.job_id == "done-0"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn public_save_rejects_a_malformed_artifact_envelope() {
        let root =
            std::env::temp_dir().join(format!("atlas-job-artifact-invalid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let mut record = Record::planned("invalid", "work");
        record.artifacts = Some(crate::artifact::Envelope {
            depends_on: vec![String::new()],
            ..crate::artifact::Envelope::default()
        });
        let error = super::save(&store, &[record]).unwrap_err().to_string();
        assert!(error.contains("artifact dependency identity"), "{error}");
        assert!(!root.join("jobs.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_oversized_job_detail_is_refused_before_persistence() {
        let root = std::env::temp_dir().join(format!("atlas-job-bytes-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let mut record = Record::planned("large", "research");
        record.heartbeat("x".repeat(4 * 1024 * 1024));

        assert!(super::save(&store, &[record]).is_err());
        assert!(!root.join("jobs.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_errand_begun_and_never_ended_is_uncertain_after_a_restart_and_can_be_dismissed() {
        let root = std::env::temp_dir().join(format!("atlas-job-errand-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        super::begin(&store, "errand:backup:1:1", "Backup", "Backup started").unwrap();
        super::begin(
            &store,
            "errand:getting-everything:1:2",
            "Download",
            "started",
        )
        .unwrap();
        super::end(
            &store,
            "errand:getting-everything:1:2",
            "Download",
            State::Succeeded,
            "ok",
        )
        .unwrap();
        // A restart: only the one still Running is cut off.
        let cut = super::reconcile_after_restart(&store).unwrap();
        assert_eq!(cut.len(), 1);
        assert_eq!(cut[0].job_id, "errand:backup:1:1");
        assert_eq!(cut[0].state, State::Uncertain);
        // Ending a record that is already terminal changes nothing.
        super::end(
            &store,
            "errand:backup:1:1",
            "Backup",
            State::Succeeded,
            "late",
        )
        .unwrap();
        let all = load_checked(&store).unwrap();
        assert_eq!(all[0].state, State::Uncertain);
        assert!(!all[0].dismissed);
        assert!(super::dismiss(&store, "errand:backup:1:1").unwrap());
        assert!(load_checked(&store).unwrap()[0].dismissed);
        assert!(!super::dismiss(&store, "no-such-job").unwrap());
        // A chore only written down when it fails is created closed.
        super::end(
            &store,
            "errand:housekeeping:5:3",
            "Housekeeping",
            State::Failed,
            "locked",
        )
        .unwrap();
        let hk = load_checked(&store)
            .unwrap()
            .into_iter()
            .find(|r| r.job_id == "errand:housekeeping:5:3")
            .unwrap();
        assert_eq!(hk.state, State::Failed);
        assert_eq!(hk.receipt.unwrap().detail, "locked");
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn the_cached_read_sees_every_later_save_and_never_hides_a_damaged_file() {
        let root = std::env::temp_dir().join(format!("atlas-job-cache-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        assert!(super::load_cached(&store).unwrap().is_empty());
        super::begin(&store, "one", "Backup", "started").unwrap();
        assert_eq!(super::load_cached(&store).unwrap().len(), 1);
        // Same answer twice is the cache; a save in between is not missed.
        assert_eq!(super::load_cached(&store).unwrap().len(), 1);
        super::begin(&store, "two", "Download", "started").unwrap();
        assert_eq!(super::load_cached(&store).unwrap().len(), 2);
        std::fs::write(root.join("jobs.json"), b"{broken").unwrap();
        assert!(super::load_cached(&store).is_err());
        assert!(super::load_cached(&store).is_err());
        let _ = std::fs::remove_dir_all(root);
    }
    #[test]
    fn one_finish_helper_closes_any_writers_job_and_refuses_a_non_ending() {
        let root = std::env::temp_dir().join(format!("atlas-job-finish-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        super::begin(&store, "a", "Work", "started").unwrap();
        assert!(super::finish(&store, "a", "Work", State::Running, "x", "t").is_err());
        super::finish(&store, "a", "Work", State::Uncertain, "gone quiet", "hub").unwrap();
        super::begin_as(&store, "b", "Own", "started", true).unwrap();
        super::finish(&store, "b", "Own", State::Failed, "no", "crew errand").unwrap();
        let all = load_checked(&store).unwrap();
        assert_eq!(all[0].state, State::Uncertain);
        assert_eq!(all[0].receipt.as_ref().unwrap().provenance, "hub");
        assert!(!all[0].dismissed);
        assert!(all[1].internal && all[1].dismissed);
        // An old file with no `internal` field still reads.
        let old =
            r#"[{"job_id":"o","state":"running","stage":"s","checkpoint":null,"receipt":null}]"#;
        let parsed: Vec<Record> = serde_json::from_str(old).unwrap();
        assert!(!parsed[0].internal);
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
    }
}
