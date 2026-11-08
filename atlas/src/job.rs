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
/// A request waiting on the owner's yes or no (21 Oct 2026). One persisted
/// `AwaitingApproval` record per pending approval, so it shows in the Hub's
/// needs-you lane and survives a restart. It is only ever a reminder: after a
/// restart the question is asked again and an old yes is never replayed.
pub const APPROVAL_PREFIX: &str = "approval:";
/// A durable receipt journal must not grow with every task forever. Active
/// jobs are never evicted; terminal history is trimmed oldest-first once the
/// bounded local journal reaches this limit.
pub const MAX_RECORDS: usize = 1_024;
const MAX_JOURNAL_BYTES: usize = 4 * 1024 * 1024;
static NEXT_JOB_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Create a durable job id that remains unique across same-second launches
/// and process restarts.
pub fn unique_id(prefix: &str) -> String {
    let sequence = NEXT_JOB_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    format!(
        "{prefix}-{}-{}-{sequence}",
        crate::store::now(),
        std::process::id()
    )
}

/// A side effect may already have happened when its terminal receipt cannot
/// be saved. Callers use this wording in the reply/Hub projection so the
/// owner is not told to repeat an action whose outcome is only unconfirmed.
pub(crate) fn completion_receipt_warning(reply: &str) -> String {
    format!(
        "{} I couldn't save the completion receipt, so check before repeating.",
        reply.trim_end()
    )
}

pub(crate) fn completion_receipt_text(reply: String, receipt_saved: bool) -> String {
    if receipt_saved {
        reply
    } else {
        completion_receipt_warning(&reply)
    }
}

pub(crate) fn completion_receipt_result(
    result: Result<String, String>,
    receipt_saved: bool,
) -> Result<String, String> {
    if receipt_saved {
        result
    } else {
        result
            .map(|text| completion_receipt_warning(&text))
            .map_err(|text| completion_receipt_warning(&text))
    }
}

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
    /// Capability-policy revision that authorized the side effect, when the
    /// work crossed an explicit capability boundary.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_revision: Option<String>,
    /// Adapter identity preserved with the terminal receipt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<crate::execution::ExecutionDescriptor>,
    /// Validated evidence bundle bound to this terminal receipt, when the
    /// writer had an evidence-producing completion path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_bundle_id: Option<String>,
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
    /// Monotonic per-record revision used to reject same-state stale
    /// snapshots. Older journals deserialize as revision zero.
    #[serde(default)]
    pub revision: u64,
    #[serde(default)]
    pub artifacts: Option<crate::artifact::Envelope>,
    /// Optional admission evidence carried by this same durable record.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy_revision: Option<String>,
    /// Adapter identity bound before execution and retained through recovery.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<crate::execution::ExecutionDescriptor>,
    /// Evidence bundle bound before a success receipt is made durable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence_bundle_id: Option<String>,
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
    fn bump_revision(&mut self) {
        self.revision = self.revision.saturating_add(1);
    }

    pub fn planned(job_id: impl Into<String>, stage: impl Into<String>) -> Self {
        Self {
            job_id: job_id.into(),
            state: State::Planned,
            stage: stage.into(),
            checkpoint: None,
            receipt: None,
            updated_at: crate::store::now(),
            revision: 0,
            artifacts: None,
            policy_revision: None,
            execution: None,
            evidence_bundle_id: None,
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
            self.bump_revision();
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
        self.bump_revision();
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
            policy_revision: self.policy_revision.clone(),
            execution: self.execution.clone(),
            evidence_bundle_id: self.evidence_bundle_id.clone(),
        };
        self.receipt = Some(receipt.clone());
        if self.internal {
            self.dismissed = true;
        }
        self.bump_revision();
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
        self.bump_revision();
    }

    /// Bind the policy revision before execution. A terminal record cannot be
    /// retroactively attributed to a different authority boundary.
    pub fn bind_policy_revision(&mut self, revision: impl Into<String>) -> Result<(), String> {
        if self.state.terminal() {
            return Err("cannot bind a capability policy to a terminal job".into());
        }
        let revision = revision.into();
        if revision.trim().is_empty() {
            return Err("capability policy revision cannot be empty".into());
        }
        if self
            .policy_revision
            .as_deref()
            .is_some_and(|existing| existing != revision)
        {
            return Err("job already has a different capability policy revision".into());
        }
        self.policy_revision = Some(revision);
        self.updated_at = crate::store::now();
        self.bump_revision();
        Ok(())
    }

    /// Bind the adapter identity before execution. A later adapter cannot
    /// silently replace the identity on a durable job or its receipt.
    pub fn bind_execution_descriptor(
        &mut self,
        descriptor: crate::execution::ExecutionDescriptor,
    ) -> Result<(), String> {
        if self.state.terminal() {
            return Err("cannot bind an execution descriptor to a terminal job".into());
        }
        descriptor.validate()?;
        if self
            .execution
            .as_ref()
            .is_some_and(|existing| existing != &descriptor)
        {
            return Err("job already has a different execution descriptor".into());
        }
        self.execution = Some(descriptor);
        self.updated_at = crate::store::now();
        self.bump_revision();
        Ok(())
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
            crate::taskloop::Outcome::Unknown(detail) => Ok(Some(self.finish(
                State::Uncertain,
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
    // Terminal job receipts are worker-side durability, not a control-loop
    // observation. Wait for an in-flight state writer so a successful task is
    // never downgraded to an unconfirmed result merely because another
    // journal update is finishing.
    let _guard = store.transaction_waiting()?;
    let mut records = crate::store::Store::load_checked_bounded_held::<Vec<Record>>(
        store,
        STORE_NAME,
        MAX_JOURNAL_BYTES as u64,
    )?
    .unwrap_or_default();
    let at = match records.iter().position(|r| r.job_id == job_id) {
        Some(at) => at,
        None => {
            let mut fresh = Record::planned(job_id, stage);
            // A record that was never begun has nothing to be cut off: it is
            // closed at once.
            // unheard-ok: Planned to Running is always allowed, so this cannot fail
            crate::heard!(fresh.transition(State::Running, "started"));
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
    save_held(store, &records)
}

/// Finish a job only after a validated evidence bundle for the same job has
/// been persisted. The evidence-first order means a crash cannot leave a
/// successful generic job without its proof reference.
pub(crate) fn finish_with_evidence(
    store: &crate::store::Store,
    job_id: &str,
    stage: &str,
    state: State,
    detail: &str,
    provenance: &str,
    evidence: crate::evidence::Bundle,
) -> crate::error::Result<()> {
    if !state.terminal() {
        return Err(crate::error::AtlasError::Config(
            "a job ending must be a terminal state".into(),
        ));
    }
    if evidence.receipt_job_id.as_deref() != Some(job_id) {
        return Err(crate::error::AtlasError::Config(
            "job evidence must reference the same job".into(),
        ));
    }
    if state == State::Succeeded && evidence.state != crate::evidence::EvidenceState::Proven {
        return Err(crate::error::AtlasError::Config(
            "a succeeded job requires proven evidence".into(),
        ));
    }
    evidence
        .validate()
        .map_err(crate::error::AtlasError::Config)?;
    // The evidence and terminal job transition form one durable worker
    // operation; waiting preserves the receipt instead of reporting a false
    // persistence failure under concurrent state activity.
    let _guard = store.transaction_waiting()?;
    let mut records = crate::store::Store::load_checked_bounded_held::<Vec<Record>>(
        store,
        STORE_NAME,
        MAX_JOURNAL_BYTES as u64,
    )?
    .unwrap_or_default();
    let at = match records.iter().position(|r| r.job_id == job_id) {
        Some(at) => at,
        None => {
            let mut fresh = Record::planned(job_id, stage);
            crate::heard!(fresh.transition(State::Running, "started"));
            records.push(fresh);
            records.len() - 1
        }
    };
    if records[at].state.terminal() {
        return Ok(());
    }

    crate::evidence::upsert_held(store, evidence.clone())?;
    records[at].evidence_bundle_id = Some(evidence.bundle_id.clone());
    let closed = if state == State::Cancelled {
        records[at].cancel(detail, provenance)
    } else {
        records[at].finish(state, detail, provenance)
    };
    closed.map_err(|error| {
        crate::error::AtlasError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, error))
    })?;
    save_held(store, &records)
}

/// Mark an ended job as reviewed, so it leaves the Failed and Interrupted
/// lanes. `false` when there is no such finished record. A saved approval
/// still waiting is first closed as cancelled (you dropped the question).
pub fn dismiss(store: &crate::store::Store, job_id: &str) -> crate::error::Result<bool> {
    let mut records = load_checked(store)?;
    if let Some(waiting) = records
        .iter_mut()
        .find(|r| r.job_id == job_id && r.state == State::AwaitingApproval)
    {
        waiting
            .finish(
                State::Cancelled,
                "you dropped the question; nothing was done",
                "dropped from the Hub",
            )
            .map_err(|error| {
                crate::error::AtlasError::Io(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    error,
                ))
            })?;
    }
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

/// When this process began, for telling an approval it has itself handled
/// from one an earlier run left.
fn process_start() -> u64 {
    static START: std::sync::OnceLock<u64> = std::sync::OnceLock::new();
    *START.get_or_init(crate::store::now)
}

/// A short stable key for a question, so the same one asked again is the same
/// waiting approval.
pub fn approval_key(kind: &str, what: &str) -> String {
    let kind = kind.trim();
    let mut h: u64 = 0xcbf29ce484222325;
    // The namespace is part of identity, not presentation. A chat question
    // and a scheduled-job question can have identical wording but different
    // owners and lifecycles; collapsing them would let one approval record
    // stand in for the other after a restart.
    for b in kind.trim().bytes().chain([0u8]) {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    for b in what.trim().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{APPROVAL_PREFIX}{kind}-{h:016x}")
}

/// The pre-namespace form used by approvals saved before the identity
/// boundary was fixed. It is read only by `sync_approvals` so an upgrade does
/// not create a second open question for the same pending work.
fn legacy_approval_key(kind: &str, what: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in what.trim().bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{APPROVAL_PREFIX}{}-{h:016x}", kind.trim())
}

fn approval_kind_from_key(key: &str) -> Option<&str> {
    let rest = key.strip_prefix(APPROVAL_PREFIX)?;
    let kind_len = rest.len().checked_sub(17)?; // '-' plus the 16-digit hash
    let (kind, suffix) = rest.split_at(kind_len);
    (suffix.starts_with('-')
        && suffix[1..].len() == 16
        && suffix[1..].bytes().all(|b| b.is_ascii_hexdigit()))
    .then_some(kind)
}

/// Make the saved approvals match what is waiting now (21 Oct 2026). `wanted`
/// is (key from `approval_key`, the question). A waiting one gets a
/// `NeedsYou` (`AwaitingApproval`) record; one that is no longer waiting is
/// closed `Cancelled` ("answered or withdrawn"). A record an earlier run left
/// is kept until the question is asked again and answered, you drop it, or it
/// is a day old: this run has no yes to give it, and must never replay one.
/// Returns whether anything was written.
pub fn sync_approvals(
    store: &crate::store::Store,
    wanted: &[(String, String)],
) -> crate::error::Result<bool> {
    let now = crate::store::now();
    let mut records = load_checked(store)?;
    let mut changed = false;
    for (key, question) in wanted {
        let stage: String = question.chars().take(160).collect();
        let open = records
            .iter_mut()
            .find(|r| r.job_id.starts_with(key.as_str()) && !r.state.terminal());
        match open {
            // Asked again after a restart: the record is this run's now.
            Some(r) if r.updated_at < process_start() => {
                r.updated_at = now;
                changed = true;
            }
            Some(_) => {}
            None => {
                // Upgrade an open record written by the question-only key
                // scheme. Matching the kind prefix and displayed question
                // prevents unrelated approvals from being merged.
                let legacy =
                    approval_kind_from_key(key).map(|kind| legacy_approval_key(kind, question));
                let parked_legacy = approval_kind_from_key(key) == Some("parked");
                let migrated = legacy.and_then(|legacy_key| {
                    records.iter_mut().find(|r| {
                        (r.job_id.starts_with(legacy_key.as_str())
                            || (parked_legacy && r.job_id.starts_with("chat:")))
                            && r.state == State::AwaitingApproval
                            && (!parked_legacy
                                || r.checkpoint.as_deref()
                                    == Some("parked for your decision; nothing was done"))
                            && r.stage == stage
                    })
                });
                if let Some(r) = migrated {
                    r.job_id = format!("{key}:{now}");
                    r.updated_at = now;
                    r.bump_revision();
                    changed = true;
                } else {
                    let mut r = Record::planned(format!("{key}:{now}"), stage);
                    // unheard-ok: Planned to AwaitingApproval is always allowed
                    let _ = r.transition(State::AwaitingApproval, "waiting for your yes or no");
                    r.checkpoint = Some(
                        "Waiting for your yes or no. If Atlas restarts it asks again; an old yes is never replayed."
                            .into(),
                    );
                    records.push(r);
                    changed = true;
                }
            }
        }
    }
    for r in records.iter_mut().filter(|r| {
        (r.job_id.starts_with(APPROVAL_PREFIX)
            || (r.job_id.starts_with("chat:")
                && r.checkpoint.as_deref() == Some("parked for your decision; nothing was done")))
            && r.state == State::AwaitingApproval
            && !wanted.iter().any(|(k, _)| r.job_id.starts_with(k.as_str()))
    }) {
        let ours = r.updated_at >= process_start();
        let old = r.updated_at.saturating_add(86_400) < now;
        if ours || old {
            // unheard-ok: AwaitingApproval to Cancelled is always allowed
            let _ = r.finish(
                State::Cancelled,
                "answered, or no longer waiting; nothing is replayed from it",
                "approval sync",
            );
            changed = true;
        }
    }
    if changed {
        save(store, &records)?;
    }
    Ok(changed)
}

fn prepared(records: &[Record]) -> crate::error::Result<Vec<Record>> {
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
    Ok(value)
}

pub fn save(store: &crate::store::Store, records: &[Record]) -> crate::error::Result<()> {
    let value = prepared(records)?;
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

fn save_held(store: &crate::store::Store, records: &[Record]) -> crate::error::Result<()> {
    let value = prepared(records)?;
    store.save_held(STORE_NAME, &value)
}

pub fn upsert(store: &crate::store::Store, record: Record) -> crate::error::Result<()> {
    upsert_inner(store, record, None)
}

/// Persist a job only if the caller's snapshot was based on `expected_revision`.
/// Workers that mutate a copied record must use this boundary; the ordinary
/// revision number alone is not a compare-and-swap token.
pub fn upsert_cas(
    store: &crate::store::Store,
    record: Record,
    expected_revision: u64,
) -> crate::error::Result<()> {
    upsert_inner(store, record, Some(expected_revision))
}

fn upsert_inner(
    store: &crate::store::Store,
    record: Record,
    expected_revision: Option<u64>,
) -> crate::error::Result<()> {
    // Hold the state lock across the read, lifecycle decision, and write. A
    // load followed by an unlocked save is still a stale-snapshot race even
    // when the replacement itself has a timestamp guard.
    let _guard = store.transaction_briefly()?;
    let mut records = crate::store::Store::load_checked_bounded_held::<Vec<Record>>(
        store,
        STORE_NAME,
        4 * 1024 * 1024,
    )?
    .unwrap_or_default();
    if let Some(existing) = records.iter_mut().find(|old| old.job_id == record.job_id) {
        if let Some(expected) = expected_revision {
            if expected != existing.revision {
                return Err(crate::error::AtlasError::Config(format!(
                    "stale job compare-and-swap for {}: expected revision {}, stored {}",
                    record.job_id, expected, existing.revision
                )));
            }
        }
        // A delayed worker must not replace a newer lifecycle record. In
        // particular, accepting an old non-terminal snapshot after a terminal
        // receipt would erase the evidence that the work ended. Keep this
        // guard at the shared job boundary so every caller gets the same
        // conflict behavior. The revision guard below also covers two workers
        // that update the same lifecycle state within one clock second.
        if record.revision < existing.revision {
            return Err(crate::error::AtlasError::Config(format!(
                "stale job upsert for {}: incoming revision {} is older than stored revision {}",
                record.job_id, record.revision, existing.revision
            )));
        }
        if record.revision == existing.revision && &record != existing {
            return Err(crate::error::AtlasError::Config(format!(
                "job revision conflict for {} at revision {}",
                record.job_id, record.revision
            )));
        }
        if record.updated_at < existing.updated_at {
            return Err(crate::error::AtlasError::Config(format!(
                "stale job upsert for {}: incoming update {} is older than stored update {}",
                record.job_id, record.updated_at, existing.updated_at
            )));
        }
        if existing.state != record.state
            && (existing.state.terminal() || !allowed(existing.state, record.state))
        {
            return Err(crate::error::AtlasError::Config(format!(
                "stale job upsert for {}: cannot replace {:?} with {:?}",
                record.job_id, existing.state, record.state
            )));
        }
        *existing = record;
    } else {
        records.push(record);
    }
    save_held(store, &records)
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
    // 22 Oct 2026: oldest first, but what is already reviewed or ended well
    // (dismissed, Cancelled, Succeeded) goes before anything you have not
    // looked at: an undismissed Failed or Uncertain is what you still need to
    // see, and must not be evicted by a run of routine successes.
    let disposable = |record: &Record| {
        record.dismissed || matches!(record.state, State::Cancelled | State::Succeeded)
    };
    terminal.sort_by_key(|record| (!disposable(record), record.updated_at));
    let keep_terminal = MAX_RECORDS - active;
    if terminal.len() > keep_terminal {
        terminal.drain(..terminal.len() - keep_terminal);
    }
    terminal.sort_by_key(|record| record.updated_at);
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
        // A question waiting on you is not work that was cut off: it stays
        // for you to see and is asked again, never answered for you.
        if record.state == State::AwaitingApproval {
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
    use super::{load_checked, unique_id, upsert, upsert_cas, Record, State, MAX_RECORDS};
    use crate::brain::Llm;
    use crate::error::Result as AtlasResult;
    use crate::execution::ExecutionDescriptor;
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
    fn an_unconfirmed_completion_cannot_sound_clean() {
        let ok = super::completion_receipt_result(Ok("sent".into()), false).unwrap();
        let err = super::completion_receipt_result(Err("failed".into()), false).unwrap_err();
        assert!(ok.contains("completion receipt"), "{ok}");
        assert!(err.contains("completion receipt"), "{err}");
        assert!(ok.contains("check before repeating"), "{ok}");
        assert!(err.contains("check before repeating"), "{err}");
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
    fn execution_identity_survives_receipt_and_restart_reconciliation() {
        let descriptor = ExecutionDescriptor::new(
            "fixture-provider",
            "fixture-model",
            "fixture-runtime",
            "fixture-device",
            "fixture-network",
            "fixture-cache",
        )
        .unwrap();
        let root = std::env::temp_dir().join(format!(
            "atlas-job-execution-descriptor-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);

        let mut running = Record::planned("descriptor-restart", "local inference");
        running
            .bind_execution_descriptor(descriptor.clone())
            .unwrap();
        running
            .transition(State::Running, "fixture started")
            .unwrap();
        upsert(&store, running).unwrap();

        let reconciled = super::reconcile_after_restart(&store).unwrap();
        assert_eq!(reconciled[0].execution, Some(descriptor.clone()));
        assert_eq!(
            reconciled[0].receipt.as_ref().unwrap().execution,
            Some(descriptor.clone())
        );

        let mut finished = Record::planned("descriptor-finished", "local inference");
        finished
            .bind_execution_descriptor(descriptor.clone())
            .unwrap();
        finished
            .transition(State::Running, "fixture started")
            .unwrap();
        let receipt = finished
            .finish(State::Succeeded, "fixture complete", "fixture")
            .unwrap();
        assert_eq!(receipt.execution, Some(descriptor));
        let bytes = serde_json::to_vec(&finished).unwrap();
        let restored: Record = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(restored.receipt, finished.receipt);
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
    fn unique_job_ids_do_not_collide_within_a_second() {
        let first = unique_id("research");
        let second = unique_id("research");
        assert_ne!(first, second);
        assert!(first.starts_with("research-"));
        assert!(second.starts_with("research-"));
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
    fn stale_worker_cannot_replace_a_terminal_job_record() {
        let root =
            std::env::temp_dir().join(format!("atlas-job-stale-upsert-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);

        let mut running = Record::planned("one", "research");
        running.transition(State::Running, "started").unwrap();
        upsert(&store, running.clone()).unwrap();
        let stale_worker_snapshot = running;

        let mut completed = stale_worker_snapshot.clone();
        completed
            .finish(State::Succeeded, "saved", "worker-a")
            .unwrap();
        upsert(&store, completed).unwrap();

        let error = upsert(&store, stale_worker_snapshot).unwrap_err();
        assert!(error.to_string().contains("stale job upsert"));
        assert_eq!(load_checked(&store).unwrap()[0].state, State::Succeeded);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn same_state_stale_worker_cannot_replace_a_newer_checkpoint_revision() {
        let root = std::env::temp_dir().join(format!(
            "atlas-job-same-state-revision-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);

        let mut running = Record::planned("same-state", "research");
        running.transition(State::Running, "started").unwrap();
        // Simulate two workers completing within one wall-clock second. The
        // timestamp alone must not decide which same-state snapshot wins.
        running.updated_at = 100;
        upsert(&store, running.clone()).unwrap();

        let stale_worker = running.clone();
        let mut fresh_worker = running.clone();
        fresh_worker.heartbeat("source 1 complete");
        fresh_worker.updated_at = 100;
        upsert(&store, fresh_worker).unwrap();

        let mut late = stale_worker;
        late.checkpoint = Some("late stale checkpoint".into());
        late.updated_at = 100;
        let error = upsert(&store, late).unwrap_err();
        assert!(error.to_string().contains("revision"));
        assert_eq!(
            load_checked(&store).unwrap()[0].checkpoint.as_deref(),
            Some("source 1 complete")
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn compare_and_swap_rejects_a_stale_snapshot_even_with_a_higher_revision() {
        let root = std::env::temp_dir().join(format!("atlas-job-cas-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);

        let mut running = Record::planned("cas", "research");
        running.transition(State::Running, "started").unwrap();
        upsert(&store, running.clone()).unwrap();
        let expected = running.revision;

        let mut newer = running.clone();
        newer.heartbeat("fresh checkpoint");
        upsert_cas(&store, newer, expected).unwrap();

        let mut stale = running;
        stale.heartbeat("stale checkpoint one");
        stale.heartbeat("stale checkpoint two");
        let error = upsert_cas(&store, stale, expected).unwrap_err();
        assert!(error.to_string().contains("compare-and-swap"));
        assert_eq!(
            load_checked(&store).unwrap()[0].checkpoint.as_deref(),
            Some("fresh checkpoint")
        );
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
    fn restart_reconciliation_keeps_any_waiting_approval_without_replaying_it() {
        let root = std::env::temp_dir().join(format!(
            "atlas-job-parked-approval-restart-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let mut parked = Record::planned("chat:parked:1", "Shutdown workspace");
        parked
            .transition(
                State::AwaitingApproval,
                "parked for your decision; nothing was done",
            )
            .unwrap();
        super::upsert(&store, parked).unwrap();

        let changed = super::reconcile_after_restart(&store).unwrap();
        assert!(
            changed.is_empty(),
            "a parked approval must be re-asked, not interrupted"
        );
        assert_eq!(
            load_checked(&store).unwrap()[0].state,
            State::AwaitingApproval
        );
        let _ = std::fs::remove_dir_all(root); // unheard-ok: scratch test directory
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
    fn a_legacy_job_record_without_revision_defaults_safely() {
        let mut value = serde_json::to_value(Record::planned("legacy", "research")).unwrap();
        value.as_object_mut().unwrap().remove("revision");
        let restored: Record = serde_json::from_value(value).unwrap();
        assert_eq!(restored.revision, 0);
    }

    #[test]
    fn an_admitted_policy_revision_is_carried_into_the_durable_receipt() {
        let mut record = Record::planned("one", "write");
        record.bind_policy_revision("policy-rev-3").unwrap();
        record.transition(State::Running, "started").unwrap();
        let receipt = record.finish(State::Succeeded, "saved", "local").unwrap();
        assert_eq!(receipt.policy_revision.as_deref(), Some("policy-rev-3"));
        let encoded = serde_json::to_vec(&record).unwrap();
        let restored: Record = serde_json::from_slice(&encoded).unwrap();
        assert_eq!(restored.policy_revision.as_deref(), Some("policy-rev-3"));
        assert_eq!(
            restored.receipt.unwrap().policy_revision.as_deref(),
            Some("policy-rev-3")
        );
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
    fn unknown_taskloop_outcome_closes_as_uncertain() {
        let mut record = Record::planned("uncertain-task", "read");
        let receipt = record
            .apply_outcome(
                &crate::taskloop::Outcome::Unknown("worker disappeared".into()),
                "local",
            )
            .unwrap()
            .unwrap();
        assert_eq!(receipt.state, State::Uncertain);
        assert_eq!(record.state, State::Uncertain);
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

    /// 22 Oct 2026: an old undismissed Failed record outlives newer successes.
    #[test]
    fn the_bound_evicts_succeeded_before_an_unreviewed_failure() {
        let mut records = Vec::new();
        let mut failed = Record::planned("old-failed", "work");
        failed.transition(State::Running, "started").unwrap();
        failed.finish(State::Failed, "broke", "test").unwrap();
        failed.updated_at = 0;
        records.push(failed);
        for n in 1..=MAX_RECORDS {
            let mut record = Record::planned(format!("done-{n}"), "work");
            record.transition(State::Running, "started").unwrap();
            record.finish(State::Succeeded, "done", "test").unwrap();
            record.updated_at = n as u64;
            records.push(record);
        }
        super::bound(&mut records).unwrap();
        assert_eq!(records.len(), MAX_RECORDS);
        assert!(records.iter().any(|r| r.job_id == "old-failed"));
        assert!(!records.iter().any(|r| r.job_id == "done-1"));
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

    #[test]
    fn a_waiting_approval_is_a_needs_you_record_that_survives_a_restart_and_is_never_replayed() {
        let root = std::env::temp_dir().join(format!("atlas-job-approval-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let key = super::approval_key("chat", "Close Chrome? Go ahead?");
        let wanted = vec![(key.clone(), "Close Chrome? Go ahead?".to_string())];
        assert!(super::sync_approvals(&store, &wanted).unwrap());
        // The same question again is the same record.
        assert!(!super::sync_approvals(&store, &wanted).unwrap());
        let all = load_checked(&store).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].state, State::AwaitingApproval);
        assert!(all[0].job_id.starts_with(super::APPROVAL_PREFIX));
        // A restart is not the end of a question: it is neither turned into
        // an interrupted job nor answered.
        assert!(super::reconcile_after_restart(&store).unwrap().is_empty());
        assert_eq!(
            load_checked(&store).unwrap()[0].state,
            State::AwaitingApproval
        );
        // Answered or withdrawn in this run: closed.
        assert!(super::sync_approvals(&store, &[]).unwrap());
        assert_eq!(load_checked(&store).unwrap()[0].state, State::Cancelled);
        // Dropping a waiting one from the Hub closes it and keeps it.
        let other = super::approval_key("sched", "1:backup");
        super::sync_approvals(&store, &[(other, "Scheduled: backup. Go ahead?".into())]).unwrap();
        let id = load_checked(&store)
            .unwrap()
            .into_iter()
            .find(|r| !r.state.terminal())
            .unwrap()
            .job_id;
        assert!(super::dismiss(&store, &id).unwrap());
        let closed = load_checked(&store)
            .unwrap()
            .into_iter()
            .find(|r| r.job_id == id)
            .unwrap();
        assert!(closed.state == State::Cancelled && closed.dismissed);
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
    }

    #[test]
    fn approval_namespaces_do_not_share_identity_when_the_question_matches() {
        let question = "Close Chrome? Go ahead?";
        assert_ne!(
            super::approval_key("chat", question),
            super::approval_key("sched", question),
            "approval identity must include its owner namespace"
        );
    }

    #[test]
    fn approval_sync_migrates_an_open_legacy_record_without_duplication() {
        let root = std::env::temp_dir().join(format!(
            "atlas-job-approval-migration-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let question = "Close Chrome? Go ahead?";
        let legacy = super::legacy_approval_key("chat", question);
        let mut old = Record::planned(format!("{legacy}:123"), question);
        old.transition(State::AwaitingApproval, "waiting for your yes or no")
            .unwrap();
        super::upsert(&store, old).unwrap();

        let key = super::approval_key("chat", question);
        assert!(super::sync_approvals(&store, &[(key.clone(), question.to_string())]).unwrap());
        let records = load_checked(&store).unwrap();
        assert_eq!(records.len(), 1, "upgrade must not create a duplicate");
        assert!(records[0].job_id.starts_with(&key));
        assert_eq!(records[0].state, State::AwaitingApproval);
        let _ = std::fs::remove_dir_all(root); // unheard-ok: scratch test directory
    }

    #[test]
    fn approval_sync_migrates_an_old_parked_chat_projection() {
        let root =
            std::env::temp_dir().join(format!("atlas-job-parked-migration-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let request = "delete the old backups";
        let mut old = Record::planned("chat:1790000000:p1", request);
        old.transition(
            State::AwaitingApproval,
            "parked for your decision; nothing was done",
        )
        .unwrap();
        super::upsert(&store, old).unwrap();

        let key = super::approval_key("parked", request);
        assert!(super::sync_approvals(&store, &[(key.clone(), request.to_string())]).unwrap());
        let records = load_checked(&store).unwrap();
        assert_eq!(records.len(), 1);
        assert!(records[0].job_id.starts_with(&key));
        assert_eq!(records[0].state, State::AwaitingApproval);
        let _ = std::fs::remove_dir_all(root); // unheard-ok: scratch test directory
    }

    #[test]
    fn parked_migration_does_not_capture_a_running_chat_job() {
        let root = std::env::temp_dir().join(format!(
            "atlas-job-parked-migration-running-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let request = "delete the old backups";
        let mut running = Record::planned("chat:1790000000:running", request);
        running
            .transition(State::Running, "worker started")
            .unwrap();
        running.checkpoint = Some("parked for your decision; nothing was done".into());
        super::upsert(&store, running).unwrap();

        let key = super::approval_key("parked", request);
        super::sync_approvals(&store, &[(key.clone(), request.to_string())]).unwrap();
        let records = load_checked(&store).unwrap();
        assert!(records
            .iter()
            .any(|r| { r.job_id == "chat:1790000000:running" && r.state == State::Running }));
        assert!(records
            .iter()
            .any(|r| { r.job_id.starts_with(&key) && r.state == State::AwaitingApproval }));
        let _ = std::fs::remove_dir_all(root); // unheard-ok: scratch test directory
    }

    #[test]
    fn parked_cleanup_does_not_cancel_an_unrelated_chat_waiting_job() {
        let root = std::env::temp_dir().join(format!(
            "atlas-job-parked-cleanup-boundary-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root); // unheard-ok: scratch test directory
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let mut waiting = Record::planned("chat:unrelated", "a different question");
        waiting
            .transition(State::AwaitingApproval, "waiting for a different owner")
            .unwrap();
        super::upsert(&store, waiting).unwrap();

        super::sync_approvals(&store, &[]).unwrap();
        assert_eq!(
            load_checked(&store).unwrap()[0].state,
            State::AwaitingApproval
        );
        let _ = std::fs::remove_dir_all(root); // unheard-ok: scratch test directory
    }
}
