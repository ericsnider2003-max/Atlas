//! Shared durable-job vocabulary for the local-first foundation.
//!
//! Domains keep their own payloads and side effects; this module owns only the
//! lifecycle envelope. A single transition gate prevents one domain from
//! claiming success after cancellation or turning an unresolved external
//! result into a false completion.

use serde::{Deserialize, Serialize};

const STORE_NAME: &str = "jobs";
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
pub fn reconcile_after_restart_except(
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
}
