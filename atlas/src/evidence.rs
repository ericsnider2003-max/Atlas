//! Bounded, portable evidence bundles for local research and self-audit.
//!
//! This is an evidence layer, not a scheduler. Bundles are immutable values:
//! a later observation is compared with an earlier one instead of rewriting
//! it. Hashes use Atlas's existing SHA-256/sealed-log primitive.

use serde::{Deserialize, Serialize};

pub const MAX_BUNDLE_BYTES: usize = 1_048_576;
pub const MAX_CLAIMS: usize = 256;
pub const MAX_TEXT_BYTES: usize = 16_384;
const MAX_JOURNAL_BYTES: u64 = 64 * 1_048_576;
const STORE_NAME: &str = "evidence";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ObservationKind {
    Static,
    Runtime,
    Installed,
    OwnerDevice,
    Derived,
}

/// Lifecycle state for a claim recorded in an evidence bundle.
///
/// The state is deliberately separate from confidence. Confidence describes
/// how strong an observation is; state answers whether the observation is
/// eligible to be reused. In particular, a low-confidence success can still
/// be a valid observation, while a failed or blocked operation must never be
/// promoted merely because its identity matches a later request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum EvidenceState {
    Proven,
    Partial,
    Blocked,
    Failed,
    Declined,
    /// Used when loading version-1 bundles that predate explicit state.
    /// Unknown evidence remains valid for inspection but is never reusable.
    #[default]
    Unknown,
}

impl EvidenceState {
    fn reusable(self) -> bool {
        matches!(self, Self::Proven)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Bundle {
    pub version: u16,
    pub bundle_id: String,
    pub subject: String,
    pub run_id: String,
    pub source_revision: String,
    pub observed_at: u64,
    pub provider: String,
    pub kind: ObservationKind,
    /// Explicit claim lifecycle. Missing state on a legacy version-1 bundle
    /// deserializes to `Unknown` and is therefore fail-closed for reuse.
    #[serde(default)]
    pub state: EvidenceState,
    pub claims: Vec<String>,
    pub limitations: Vec<String>,
    pub input_hash: String,
    pub output_hash: String,
    pub confidence: u8,
    pub supersedes: Option<String>,
    pub receipt_job_id: Option<String>,
    /// Execution identity for the observation, when the caller has one.
    /// Legacy bundles remain readable but do not gain an invented identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<crate::execution::ExecutionDescriptor>,
    /// Non-sensitive per-request routing facts, such as the actual provider
    /// selected by a local FreeLLMAPI gateway. These are bound into the
    /// v3 identity hash so a receipt cannot later claim a different route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_metadata: Option<crate::execution::ExecutionMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub input_changed: bool,
    pub output_changed: bool,
    pub claims_added: Vec<String>,
    pub claims_removed: Vec<String>,
    pub limitations_changed: bool,
    pub confidence_changed: bool,
    pub state_changed: bool,
}

impl Bundle {
    pub fn create(
        subject: impl Into<String>,
        run_id: impl Into<String>,
        source_revision: impl Into<String>,
        observed_at: u64,
        provider: impl Into<String>,
        kind: ObservationKind,
        claims: Vec<String>,
        limitations: Vec<String>,
        input: &[u8],
        output: &[u8],
        confidence: u8,
    ) -> Result<Self, String> {
        let mut bundle = Self {
            // Version 3 adds per-request routing metadata. Keeping the
            // version explicit preserves validation of older v2 hashes.
            version: 3,
            bundle_id: String::new(),
            subject: subject.into(),
            run_id: run_id.into(),
            source_revision: source_revision.into(),
            observed_at,
            provider: provider.into(),
            kind,
            state: EvidenceState::Proven,
            claims,
            limitations,
            input_hash: hash(input),
            output_hash: hash(output),
            confidence,
            supersedes: None,
            receipt_job_id: None,
            execution: None,
            execution_metadata: None,
        };
        bundle.validate_shape()?;
        bundle.bundle_id = hash(&bundle.identity_bytes());
        Ok(bundle)
    }

    pub fn validate(&self) -> Result<(), String> {
        self.validate_shape()?;
        let expected = hash(&self.identity_bytes());
        if self.bundle_id != expected {
            return Err("evidence bundle identity hash does not match its contents".into());
        }
        let bytes = serde_json::to_vec(self).map_err(|e| format!("evidence encode: {e}"))?;
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err("evidence bundle exceeds the local size bound".into());
        }
        Ok(())
    }

    pub(crate) fn export_json(&self) -> Result<Vec<u8>, String> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|e| format!("evidence encode: {e}"))
    }

    pub(crate) fn import_json(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() > MAX_BUNDLE_BYTES {
            return Err("evidence bundle exceeds the local size bound".into());
        }
        let bundle: Self = serde_json::from_slice(bytes)
            .map_err(|e| format!("evidence bundle is unreadable: {e}"))?;
        bundle.validate()?;
        Ok(bundle)
    }

    pub(crate) fn compare(&self, newer: &Self) -> Result<Difference, String> {
        self.validate()?;
        newer.validate()?;
        if self.subject != newer.subject {
            return Err("evidence comparison requires the same subject".into());
        }
        Ok(Difference {
            input_changed: self.input_hash != newer.input_hash,
            output_changed: self.output_hash != newer.output_hash,
            claims_added: newer
                .claims
                .iter()
                .filter(|claim| !self.claims.contains(claim))
                .cloned()
                .collect(),
            claims_removed: self
                .claims
                .iter()
                .filter(|claim| !newer.claims.contains(claim))
                .cloned()
                .collect(),
            limitations_changed: self.limitations != newer.limitations,
            confidence_changed: self.confidence != newer.confidence,
            state_changed: self.state != newer.state,
        })
    }

    fn set_state(&mut self, state: EvidenceState) -> Result<(), String> {
        self.state = state;
        self.bundle_id = hash(&self.identity_bytes());
        self.validate()
    }

    pub(crate) fn link_job(&mut self, job_id: impl Into<String>) -> Result<(), String> {
        self.receipt_job_id = Some(job_id.into());
        self.bundle_id = hash(&self.identity_bytes());
        self.validate()
    }

    fn validate_shape(&self) -> Result<(), String> {
        if !matches!(self.version, 1..=3) || self.subject.is_empty() || self.run_id.is_empty() {
            return Err("evidence bundle has an invalid identity".into());
        }
        if self.claims.len() > MAX_CLAIMS || self.limitations.len() > MAX_CLAIMS {
            return Err("evidence bundle has too many claims or limitations".into());
        }
        for text in self.claims.iter().chain(self.limitations.iter()).chain([
            &self.subject,
            &self.run_id,
            &self.source_revision,
            &self.provider,
        ]) {
            if text.len() > MAX_TEXT_BYTES {
                return Err("evidence bundle text field exceeds the local size bound".into());
            }
        }
        if self.confidence > 100 {
            return Err("evidence confidence must be between 0 and 100".into());
        }
        if self.receipt_job_id.as_deref().is_some_and(str::is_empty) {
            return Err("evidence receipt job identity cannot be empty".into());
        }
        if let Some(execution) = &self.execution {
            execution.validate()?;
        }
        if let Some(metadata) = &self.execution_metadata {
            metadata.validate()?;
        }
        Ok(())
    }

    fn identity_bytes(&self) -> Vec<u8> {
        if self.version == 1 {
            serde_json::to_vec(&(
                self.version,
                &self.subject,
                &self.run_id,
                &self.source_revision,
                self.observed_at,
                &self.provider,
                self.kind,
                &self.claims,
                &self.limitations,
                &self.input_hash,
                &self.output_hash,
                self.confidence,
                &self.supersedes,
                &self.receipt_job_id,
            ))
        } else if self.version == 2 {
            serde_json::to_vec(&(
                self.version,
                &self.subject,
                &self.run_id,
                &self.source_revision,
                self.observed_at,
                &self.provider,
                self.kind,
                self.state,
                &self.claims,
                &self.limitations,
                &self.input_hash,
                &self.output_hash,
                self.confidence,
                &self.supersedes,
                &self.receipt_job_id,
                &self.execution,
            ))
        } else {
            // Keep the versioned identity representation as a JSON array, but
            // do not use a Rust tuple here: serde only implements tuple
            // serialization through arity 16, while version 3 has 17 fields.
            serde_json::to_vec(&serde_json::json!([
                self.version,
                &self.subject,
                &self.run_id,
                &self.source_revision,
                self.observed_at,
                &self.provider,
                self.kind,
                self.state,
                &self.claims,
                &self.limitations,
                &self.input_hash,
                &self.output_hash,
                self.confidence,
                &self.supersedes,
                &self.receipt_job_id,
                &self.execution,
                &self.execution_metadata,
            ]))
        }
        .unwrap_or_else(|error| {
            debug_assert!(false, "evidence identity serialization failed: {error}");
            Vec::new()
        })
    }
}

/// Build the evidence claim for one durable task-loop outcome.
///
/// Keeping this domain writer beside the evidence contract prevents daemon
/// call sites from inventing their own subject, provenance, or limitation
/// wording. The task result is still only an observation: owner/device proof
/// remains explicitly outside this local runtime claim.
#[allow(dead_code)]
pub(crate) fn task_outcome(
    job_id: &str,
    outcome: &crate::taskloop::Outcome,
    observed_at: u64,
) -> Result<Bundle, String> {
    task_outcome_with_execution(job_id, outcome, observed_at, None)
}

/// As `task_outcome`, retaining the exact execution identity observed by the
/// task-loop caller.
pub(crate) fn task_outcome_with_execution(
    job_id: &str,
    outcome: &crate::taskloop::Outcome,
    observed_at: u64,
    execution: Option<crate::execution::ExecutionDescriptor>,
) -> Result<Bundle, String> {
    let kind = match outcome {
        crate::taskloop::Outcome::Started(_) => ObservationKind::Runtime,
        crate::taskloop::Outcome::NeedsYou(_) => ObservationKind::Derived,
        crate::taskloop::Outcome::Done(_)
        | crate::taskloop::Outcome::Failed(_)
        | crate::taskloop::Outcome::Unknown(_) => ObservationKind::Runtime,
    };
    let confidence = match outcome {
        crate::taskloop::Outcome::Done(_) => 70,
        crate::taskloop::Outcome::Started(_) => 35,
        crate::taskloop::Outcome::NeedsYou(_) => 30,
        crate::taskloop::Outcome::Failed(_) => 20,
        crate::taskloop::Outcome::Unknown(_) => 10,
    };
    let text = outcome.text().to_string();
    // Task-loop evidence is not a reusable answer to a generic question: it
    // is the receipt for this exact durable job. Binding the subject to the
    // job prevents a later task with identical outcome text from reusing an
    // earlier task's receipt and provenance.
    let subject = format!("atlas-task-loop:{job_id}");
    let mut bundle = Bundle::create(
        subject,
        job_id,
        "working-tree",
        observed_at,
        "local-daemon",
        kind,
        vec![text.clone()],
        vec!["owner/device proof is separate".into()],
        text.as_bytes(),
        text.as_bytes(),
        confidence,
    )?;
    bundle.set_state(match outcome {
        crate::taskloop::Outcome::Done(_) => EvidenceState::Proven,
        crate::taskloop::Outcome::Started(_) => EvidenceState::Partial,
        crate::taskloop::Outcome::NeedsYou(_) => EvidenceState::Blocked,
        crate::taskloop::Outcome::Failed(_) => EvidenceState::Failed,
        crate::taskloop::Outcome::Unknown(_) => EvidenceState::Unknown,
    })?;
    bundle.execution = execution;
    bundle.bundle_id = hash(&bundle.identity_bytes());
    bundle.validate()?;
    bundle.link_job(job_id)?;
    Ok(bundle)
}

/// Build the typed local evidence for a saved research result.
pub(crate) fn research_outcome(
    job_id: &str,
    topic: &str,
    source_count: usize,
    saved: bool,
    detail: &str,
    observed_at: u64,
) -> Result<Bundle, String> {
    research_outcome_with_execution(
        job_id,
        topic,
        source_count,
        saved,
        detail,
        observed_at,
        None,
    )
}

/// Build research evidence while retaining the adapter identity already
/// observed by the research worker. The legacy wrapper above remains useful
/// for callers that cannot prove an execution descriptor; those bundles keep
/// the conservative local-research provider label instead of inventing one.
pub(crate) fn research_outcome_with_execution(
    job_id: &str,
    topic: &str,
    source_count: usize,
    saved: bool,
    detail: &str,
    observed_at: u64,
    execution: Option<crate::execution::ExecutionDescriptor>,
) -> Result<Bundle, String> {
    research_outcome_with_execution_metadata(
        job_id,
        topic,
        source_count,
        saved,
        detail,
        observed_at,
        execution,
        None,
    )
}

/// Build research evidence with both the configured execution identity and
/// the non-sensitive routing facts returned by that exact request.
pub(crate) fn research_outcome_with_execution_metadata(
    job_id: &str,
    topic: &str,
    source_count: usize,
    saved: bool,
    detail: &str,
    observed_at: u64,
    execution: Option<crate::execution::ExecutionDescriptor>,
    execution_metadata: Option<crate::execution::ExecutionMetadata>,
) -> Result<Bundle, String> {
    let claim = format!("{detail} ({source_count} source(s))");
    let provider = execution
        .as_ref()
        .map(|descriptor| descriptor.provider.clone())
        .unwrap_or_else(|| "local-research".into());
    let mut bundle = Bundle::create(
        "atlas-research",
        job_id,
        "working-tree",
        observed_at,
        provider,
        ObservationKind::Runtime,
        vec![claim],
        vec!["source freshness and owner/device proof remain separate".into()],
        topic.as_bytes(),
        detail.as_bytes(),
        if saved && source_count > 0 { 75 } else { 25 },
    )?;
    bundle.execution = execution;
    bundle.execution_metadata = execution_metadata;
    bundle.bundle_id = hash(&bundle.identity_bytes());
    bundle.validate()?;
    bundle.set_state(if saved && source_count > 0 {
        EvidenceState::Proven
    } else {
        EvidenceState::Failed
    })?;
    bundle.link_job(job_id)?;
    Ok(bundle)
}

/// Build the durable observation for a generic crew errand. Crew completion
/// is local execution evidence; it is not proof of an external side effect.
/// Keeping this writer beside the existing task/research writers lets the job
/// owner bind a successful errand to a validated bundle without introducing a
/// second receipt store.
pub(crate) fn crew_outcome(
    job_id: &str,
    label: &str,
    state: crate::job::State,
    detail: &str,
    observed_at: u64,
) -> Result<Bundle, String> {
    let (evidence_state, confidence) = match state {
        crate::job::State::Succeeded => (EvidenceState::Proven, 70),
        crate::job::State::Failed => (EvidenceState::Failed, 20),
        crate::job::State::Cancelled => (EvidenceState::Declined, 60),
        crate::job::State::Uncertain => (EvidenceState::Unknown, 10),
        crate::job::State::RolledBack => (EvidenceState::Declined, 50),
        _ => return Err("crew evidence requires a terminal job state".into()),
    };
    let claim = format!("crew errand {label} ended as {state:?}: {detail}");
    let mut bundle = Bundle::create(
        format!("atlas-crew:{job_id}"),
        job_id,
        format!("crew:{label}"),
        observed_at,
        "local-crew",
        ObservationKind::Runtime,
        vec![claim],
        vec![
            "local crew completion does not prove an external provider or owner-device effect"
                .into(),
        ],
        label.as_bytes(),
        detail.as_bytes(),
        confidence,
    )?;
    bundle.set_state(evidence_state)?;
    bundle.link_job(job_id)?;
    Ok(bundle)
}

/// Build typed evidence for a local code/build worker result.
pub(crate) fn build_outcome(
    job_id: &str,
    request: &str,
    verified_saved: bool,
    detail: &str,
    observed_at: u64,
) -> Result<Bundle, String> {
    let claim = if verified_saved {
        format!("verified local build saved: {detail}")
    } else {
        format!("local build did not produce verified saved code: {detail}")
    };
    let mut bundle = Bundle::create(
        "atlas-code-build",
        job_id,
        "working-tree",
        observed_at,
        "local-build",
        ObservationKind::Runtime,
        vec![claim],
        vec!["toolchain and owner review boundaries remain separate".into()],
        request.as_bytes(),
        detail.as_bytes(),
        if verified_saved { 85 } else { 35 },
    )?;
    bundle.set_state(if verified_saved {
        EvidenceState::Proven
    } else {
        EvidenceState::Failed
    })?;
    bundle.link_job(job_id)?;
    Ok(bundle)
}

/// Build typed evidence for the owner's final keep/decline decision on media.
pub(crate) fn media_outcome(
    job_id: &str,
    result: &str,
    kept: bool,
    observed_at: u64,
) -> Result<Bundle, String> {
    let claim = if kept {
        format!("edited media kept: {result}")
    } else {
        "edited media declined; dependent work was not run".to_string()
    };
    let mut bundle = Bundle::create(
        "atlas-media-edit",
        job_id,
        "working-tree",
        observed_at,
        "local-media",
        ObservationKind::OwnerDevice,
        vec![claim],
        vec!["the keep decision is local; external publication is not proven".into()],
        result.as_bytes(),
        result.as_bytes(),
        if kept { 90 } else { 100 },
    )?;
    bundle.set_state(if kept {
        EvidenceState::Proven
    } else {
        EvidenceState::Declined
    })?;
    bundle.link_job(job_id)?;
    Ok(bundle)
}

/// Build typed evidence for a kept media file and bind the receipt to the
/// file's current bytes, not merely to the path that names it.
///
/// A path can be reused, moved, or replaced between the edit and the owner's
/// keep decision. The existing media owner already checks that the result is a
/// file; this stronger variant records the streamed SHA-256 of those bytes so
/// later artifact/evidence consumers can distinguish local custody from a
/// filename or path claim.
pub(crate) fn media_file_outcome(
    job_id: &str,
    result: &str,
    kept: bool,
    observed_at: u64,
) -> Result<Bundle, String> {
    let mut bundle = media_outcome(job_id, result, kept, observed_at)?;
    if kept {
        let output_hash = crate::digest::sha256_file_hex(std::path::Path::new(result))
            .map_err(|error| format!("hash kept media output: {error}"))?;
        bundle.output_hash = output_hash;
        bundle
            .limitations
            .push("kept output bytes were SHA-256 bound at evidence capture".into());
        bundle.bundle_id = hash(&bundle.identity_bytes());
        bundle.validate()?;
    }
    Ok(bundle)
}

/// Build typed evidence for a system self-audit. An inspection is not an
/// approval, code change, or proof that a recommendation was applied.
pub(crate) fn self_audit_outcome(
    job_id: &str,
    signal_count: usize,
    recommendation_count: usize,
    observed_at: u64,
) -> Result<Bundle, String> {
    let claim = format!(
        "self-audit observed {signal_count} signal(s) and produced {recommendation_count} recommendation(s)"
    );
    let mut bundle = Bundle::create(
        "atlas-self-audit",
        job_id,
        "working-tree",
        observed_at,
        "local-self-audit",
        ObservationKind::Derived,
        vec![claim.clone()],
        vec!["recommendations require separate review, approval, and apply proof".into()],
        job_id.as_bytes(),
        claim.as_bytes(),
        65,
    )?;
    bundle.set_state(EvidenceState::Proven)?;
    bundle.link_job(job_id)?;
    Ok(bundle)
}

fn hash(bytes: &[u8]) -> String {
    crate::sealedlog::hex(&crate::sealedlog::leaf_hash(bytes))
}

/// Persist an immutable bundle in Atlas's existing atomic Store. The journal
/// is bounded so repeated self-audits cannot consume the machine forever.
pub(crate) fn upsert(store: &crate::store::Store, bundle: Bundle) -> crate::error::Result<()> {
    let _guard = store.transaction_briefly()?;
    upsert_held(store, bundle)
}

/// Persist terminal task evidence while allowing an in-flight worker snapshot
/// to finish. Completion evidence is part of the durable terminal boundary;
/// refusing it after the short control-loop window can turn a completed task
/// into an unconfirmed result even though the competing write is transient.
pub(crate) fn upsert_waiting(
    store: &crate::store::Store,
    bundle: Bundle,
) -> crate::error::Result<()> {
    let _guard = store.transaction_waiting()?;
    upsert_held(store, bundle)
}

/// Variant for callers that already own the store transaction. Keeping the
/// read/modify/write sequence behind one boundary prevents concurrent workers
/// from replacing one another's evidence with stale snapshots.
pub(crate) fn upsert_held(store: &crate::store::Store, bundle: Bundle) -> crate::error::Result<()> {
    bundle
        .validate()
        .map_err(crate::error::AtlasError::Config)?;
    let mut bundles = store
        .load_checked_bounded_held::<Vec<Bundle>>(STORE_NAME, MAX_JOURNAL_BYTES as u64)?
        .unwrap_or_default();
    if let Some(existing) = bundles
        .iter_mut()
        .find(|existing| existing.bundle_id == bundle.bundle_id)
    {
        *existing = bundle;
    } else {
        bundles.push(bundle);
    }
    if bundles.len() > 512 {
        let excess = bundles.len() - 512;
        bundles.drain(..excess);
    }
    let journal_bytes = serde_json::to_vec(&bundles)
        .map_err(|error| crate::error::AtlasError::Config(format!("evidence encode: {error}")))?;
    if journal_bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err(crate::error::AtlasError::Config(
            "evidence journal exceeds the local size bound".into(),
        ));
    }
    // Evidence closes a task's proof boundary. Wait for a competing worker
    // rather than using the control-loop's brief refusal window, so a short
    // overlap cannot turn a completed task into an unverified result.
    store.save_held(STORE_NAME, &bundles)
}

/// Find the newest valid observation that can answer the same local question.
/// A match never ignores revision or input changes, and malformed stored data
/// is skipped rather than promoted into a reusable claim.
pub(crate) fn reusable(
    store: &crate::store::Store,
    subject: &str,
    source_revision: &str,
    provider: &str,
    input_hash: &str,
) -> Option<Bundle> {
    let mut bundles = load_checked(store).ok()?;
    bundles.sort_by_key(|bundle| std::cmp::Reverse(bundle.observed_at));
    bundles.into_iter().find(|bundle| {
        bundle.subject == subject
            && bundle.source_revision == source_revision
            && bundle.provider == provider
            && bundle.input_hash == input_hash
            && bundle.state.reusable()
            && bundle.validate().is_ok()
    })
}

fn load_checked(store: &crate::store::Store) -> crate::error::Result<Vec<Bundle>> {
    Ok(store
        .load_checked_bounded::<Vec<Bundle>>(STORE_NAME, MAX_JOURNAL_BYTES)?
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::{
        build_outcome, hash, media_file_outcome, media_outcome, research_outcome,
        research_outcome_with_execution, research_outcome_with_execution_metadata,
        self_audit_outcome, task_outcome, task_outcome_with_execution, Bundle, EvidenceState,
        ObservationKind, MAX_BUNDLE_BYTES,
    };

    fn sample() -> Bundle {
        Bundle::create(
            "atlas-self-audit",
            "run-1",
            "abc123",
            42,
            "local",
            ObservationKind::Static,
            vec!["jobs are durable".into()],
            vec!["device proof not run".into()],
            b"source",
            b"new result",
            80,
        )
        .unwrap()
    }

    #[test]
    fn task_outcome_writer_keeps_runtime_provenance_and_job_link() {
        let outcome = crate::taskloop::Outcome::Done("saved locally".into());
        let bundle = task_outcome("task-1", &outcome, 42).unwrap();
        assert_eq!(bundle.subject, "atlas-task-loop:task-1");
        assert_eq!(bundle.provider, "local-daemon");
        assert_eq!(bundle.kind, ObservationKind::Runtime);
        assert_eq!(bundle.state, EvidenceState::Proven);
        assert_eq!(bundle.confidence, 70);
        assert_eq!(bundle.receipt_job_id.as_deref(), Some("task-1"));
        assert_eq!(bundle.claims, vec!["saved locally"]);
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn task_outcomes_with_identical_text_have_distinct_receipt_subjects() {
        let outcome = crate::taskloop::Outcome::Done("saved locally".into());
        let first = task_outcome("task-1", &outcome, 42).unwrap();
        let second = task_outcome("task-2", &outcome, 43).unwrap();
        assert_ne!(first.subject, second.subject);
        assert_ne!(first.bundle_id, second.bundle_id);
        assert_eq!(first.input_hash, second.input_hash);
    }

    #[test]
    fn task_evidence_binds_execution_identity_into_bundle_hash() {
        let outcome = crate::taskloop::Outcome::Done("finished".into());
        let descriptor = crate::execution::ExecutionDescriptor::new(
            "fixture-provider",
            "fixture-model",
            "fixture-runtime",
            "fixture-device",
            "fixture-network",
            "fixture-cache",
        )
        .unwrap();
        let without = task_outcome("descriptor-job", &outcome, 42).unwrap();
        let with =
            task_outcome_with_execution("descriptor-job", &outcome, 42, Some(descriptor.clone()))
                .unwrap();
        assert_eq!(with.execution, Some(descriptor));
        assert_ne!(with.bundle_id, without.bundle_id);
        assert!(with.validate().is_ok());
    }

    #[test]
    fn failed_task_outcome_cannot_look_like_a_completed_observation() {
        let outcome = crate::taskloop::Outcome::Failed("provider unavailable".into());
        let bundle = task_outcome("task-failed", &outcome, 42).unwrap();
        assert_eq!(bundle.confidence, 20);
        assert_eq!(bundle.state, EvidenceState::Failed);
        assert!(bundle.claims[0].contains("provider unavailable"));
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn unknown_task_outcome_is_uncertain_and_low_confidence() {
        let outcome = crate::taskloop::Outcome::Unknown("worker disappeared".into());
        let bundle = task_outcome("task-unknown", &outcome, 42).unwrap();
        assert_eq!(bundle.confidence, 10);
        assert_eq!(bundle.state, EvidenceState::Unknown);
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn research_outcome_writes_source_count_and_limits_confidence() {
        let bundle =
            research_outcome("research-1", "local systems", 2, true, "research saved", 42).unwrap();
        assert_eq!(bundle.subject, "atlas-research");
        assert_eq!(bundle.provider, "local-research");
        assert_eq!(bundle.confidence, 75);
        assert_eq!(bundle.state, EvidenceState::Proven);
        assert!(bundle.claims[0].contains("2 source(s)"));
        assert_eq!(bundle.receipt_job_id.as_deref(), Some("research-1"));
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn research_evidence_preserves_the_worker_execution_identity() {
        let descriptor = crate::execution::ExecutionDescriptor::new(
            "freellmapi-gateway",
            "fixture-model",
            "openai-compatible-gateway",
            "unknown",
            "remote-via-loopback-gateway",
            "unknown",
        )
        .unwrap();
        let bundle = research_outcome_with_execution(
            "research-gateway",
            "local systems",
            2,
            true,
            "research saved",
            42,
            Some(descriptor.clone()),
        )
        .unwrap();
        assert_eq!(bundle.provider, "freellmapi-gateway");
        assert_eq!(bundle.execution, Some(descriptor));
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn research_evidence_binds_gateway_routing_metadata_into_its_identity() {
        let descriptor = crate::execution::ExecutionDescriptor::new(
            "freellmapi-gateway",
            "fixture-model",
            "openai-compatible-gateway",
            "unknown",
            "remote-via-loopback-gateway",
            "unknown",
        )
        .unwrap();
        let metadata = crate::execution::ExecutionMetadata {
            routed_via: Some("fixture-provider/model".into()),
            fallback_attempts: Some("1".into()),
            fallback_trail: Some("fixture-provider/model".into()),
        };
        let bundle = research_outcome_with_execution_metadata(
            "research-routing",
            "local systems",
            2,
            true,
            "research saved",
            42,
            Some(descriptor),
            Some(metadata.clone()),
        )
        .unwrap();
        assert_eq!(bundle.execution_metadata, Some(metadata));
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn research_outcome_keeps_failed_research_below_success_confidence() {
        let bundle = research_outcome(
            "research-failed",
            "local systems",
            0,
            false,
            "provider failed before a saved result",
            42,
        )
        .unwrap();
        assert_eq!(bundle.subject, "atlas-research");
        assert_eq!(bundle.confidence, 25);
        assert_eq!(bundle.state, EvidenceState::Failed);
        assert!(bundle.claims[0].contains("provider failed"));
        assert_eq!(bundle.receipt_job_id.as_deref(), Some("research-failed"));
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn research_save_failure_with_sources_still_has_low_confidence() {
        let bundle = research_outcome(
            "research-save-failed",
            "local systems",
            4,
            false,
            "write-up save failed",
            42,
        )
        .unwrap();
        assert_eq!(bundle.confidence, 25);
        assert_eq!(bundle.state, EvidenceState::Failed);
        assert!(bundle.claims[0].contains("4 source(s)"));
        assert!(bundle.claims[0].contains("save failed"));
    }

    #[test]
    fn build_outcome_distinguishes_verified_saved_code_from_failure() {
        let good = build_outcome(
            "build-1",
            "make a local tool",
            true,
            "verified code saved",
            42,
        )
        .unwrap();
        let bad =
            build_outcome("build-2", "make a local tool", false, "checks failed", 42).unwrap();
        assert_eq!(good.subject, "atlas-code-build");
        assert_eq!(good.provider, "local-build");
        assert_eq!(good.confidence, 85);
        assert_eq!(good.state, EvidenceState::Proven);
        assert!(good.claims[0].contains("verified local build saved"));
        assert_eq!(bad.confidence, 35);
        assert_eq!(bad.state, EvidenceState::Failed);
        assert!(bad.claims[0].contains("did not produce verified saved code"));
        assert!(good.validate().is_ok() && bad.validate().is_ok());
    }

    #[test]
    fn media_outcome_records_the_keep_boundary_without_claiming_publication() {
        let kept = media_outcome("task-1", "edited.png", true, 42).unwrap();
        let declined = media_outcome("task-2", "edited.png", false, 42).unwrap();
        assert_eq!(kept.subject, "atlas-media-edit");
        assert_eq!(kept.kind, ObservationKind::OwnerDevice);
        assert_eq!(kept.confidence, 90);
        assert_eq!(kept.state, EvidenceState::Proven);
        assert!(kept.claims[0].contains("kept"));
        assert_eq!(declined.confidence, 100);
        assert_eq!(declined.state, EvidenceState::Declined);
        assert!(declined.claims[0].contains("declined"));
        assert!(declined.limitations[0].contains("publication"));
    }

    #[test]
    fn kept_media_evidence_binds_file_bytes_not_just_the_result_path() {
        let path = std::env::temp_dir().join(format!(
            "atlas-media-evidence-{}-{}.bin",
            std::process::id(),
            crate::store::now()
        ));
        std::fs::write(&path, b"media bytes").unwrap();
        let bundle = media_file_outcome("task-file", path.to_str().unwrap(), true, 42).unwrap();
        assert_eq!(
            bundle.output_hash,
            crate::digest::sha256_file_hex(&path).unwrap()
        );
        assert_ne!(bundle.output_hash, hash(path.to_string_lossy().as_bytes()));
        assert!(bundle
            .limitations
            .iter()
            .any(|item| item.contains("SHA-256 bound")));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn kept_media_evidence_refuses_a_missing_result_file() {
        assert!(media_file_outcome("task-missing", "missing-media-output.bin", true, 42).is_err());
    }

    #[test]
    fn self_audit_outcome_does_not_claim_recommendations_were_applied() {
        let bundle = self_audit_outcome("self-audit-1", 3, 2, 42).unwrap();
        assert_eq!(bundle.subject, "atlas-self-audit");
        assert_eq!(bundle.provider, "local-self-audit");
        assert_eq!(bundle.kind, ObservationKind::Derived);
        assert_eq!(bundle.state, EvidenceState::Proven);
        assert!(bundle.claims[0].contains("3 signal(s)"));
        assert!(bundle.claims[0].contains("2 recommendation(s)"));
        assert!(bundle.limitations[0].contains("approval"));
        assert_eq!(bundle.receipt_job_id.as_deref(), Some("self-audit-1"));
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn export_import_preserves_a_valid_bundle() {
        let bundle = sample();
        let bytes = bundle.export_json().unwrap();
        assert_eq!(Bundle::import_json(&bytes).unwrap(), bundle);
    }

    #[test]
    fn tampering_is_rejected_before_a_claim_can_be_used() {
        let mut value: serde_json::Value =
            serde_json::from_slice(&sample().export_json().unwrap()).unwrap();
        value["claims"][0] = "invented".into();
        let bytes = serde_json::to_vec(&value).unwrap();
        assert!(Bundle::import_json(&bytes).is_err());
    }

    #[test]
    fn compare_names_changed_inputs_claims_and_limits() {
        let old = sample();
        let mut new = Bundle::create(
            "atlas-self-audit",
            "run-2",
            "def456",
            43,
            "local",
            ObservationKind::Runtime,
            vec!["jobs are durable".into(), "heartbeats exist".into()],
            vec!["owner proof not run".into()],
            b"new source",
            b"result",
            90,
        )
        .unwrap();
        new.link_job("job-1").unwrap();
        let diff = old.compare(&new).unwrap();
        assert!(diff.input_changed && diff.output_changed);
        assert_eq!(diff.claims_added, vec!["heartbeats exist"]);
        assert!(diff.limitations_changed && diff.confidence_changed);
    }

    #[test]
    fn oversized_import_is_refused() {
        assert!(Bundle::import_json(&vec![b'x'; MAX_BUNDLE_BYTES + 1]).is_err());
    }

    #[test]
    fn persisted_bundles_reload_without_duplicates() {
        let root = std::env::temp_dir().join(format!("atlas-evidence-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let bundle = sample();
        super::upsert(&store, bundle.clone()).unwrap();
        super::upsert(&store, bundle).unwrap();
        let saved: Vec<Bundle> = store.load(super::STORE_NAME);
        assert_eq!(saved.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reusable_requires_the_same_subject_revision_and_input() {
        let root =
            std::env::temp_dir().join(format!("atlas-evidence-reuse-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let bundle = sample();
        super::upsert(&store, bundle.clone()).unwrap();
        assert!(super::reusable(
            &store,
            "atlas-self-audit",
            "abc123",
            "local",
            &bundle.input_hash,
        )
        .is_some());
        assert!(super::reusable(
            &store,
            "atlas-self-audit",
            "changed",
            "local",
            &bundle.input_hash,
        )
        .is_none());
        assert!(super::reusable(&store, "other", "abc123", "local", &bundle.input_hash,).is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn failed_evidence_is_never_reusable_even_when_identity_matches() {
        let root = std::env::temp_dir().join(format!(
            "atlas-evidence-failed-reuse-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let bundle = research_outcome(
            "research-failed-reuse",
            "same topic",
            0,
            false,
            "provider failed before a saved result",
            42,
        )
        .unwrap();
        super::upsert(&store, bundle.clone()).unwrap();

        assert!(super::reusable(
            &store,
            &bundle.subject,
            &bundle.source_revision,
            &bundle.provider,
            &bundle.input_hash,
        )
        .is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn legacy_version_one_evidence_is_readable_but_not_reusable() {
        let mut legacy = sample();
        legacy.version = 1;
        legacy.state = EvidenceState::Unknown;
        legacy.bundle_id = hash(&legacy.identity_bytes());
        let bytes = legacy.export_json().unwrap();
        let imported = Bundle::import_json(&bytes).unwrap();
        assert_eq!(imported.version, 1);
        assert_eq!(imported.state, EvidenceState::Unknown);

        let root = std::env::temp_dir().join(format!(
            "atlas-evidence-legacy-reuse-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        super::upsert(&store, imported.clone()).unwrap();
        assert!(super::reusable(
            &store,
            &imported.subject,
            &imported.source_revision,
            &imported.provider,
            &imported.input_hash,
        )
        .is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn corrupt_evidence_journal_is_not_replaced_or_reused() {
        let root =
            std::env::temp_dir().join(format!("atlas-evidence-corrupt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        std::fs::write(root.join("evidence.json"), b"{not-json").unwrap();

        assert!(super::reusable(&store, "atlas-self-audit", "abc123", "local", "nope",).is_none());
        assert!(super::upsert(&store, sample()).is_err());
        assert_eq!(
            std::fs::read(root.join("evidence.json")).unwrap(),
            b"{not-json"
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn reusable_refuses_provider_drift_even_when_source_and_input_match() {
        let root = std::env::temp_dir().join(format!(
            "atlas-evidence-provider-drift-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let bundle = sample();
        super::upsert(&store, bundle.clone()).unwrap();
        assert!(super::reusable(
            &store,
            &bundle.subject,
            &bundle.source_revision,
            "local",
            &bundle.input_hash,
        )
        .is_some());
        assert!(super::reusable(
            &store,
            &bundle.subject,
            &bundle.source_revision,
            "different-provider",
            &bundle.input_hash,
        )
        .is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn an_evidence_bundle_cannot_link_to_an_empty_job() {
        let mut bundle = Bundle::create(
            "subject",
            "run",
            "revision",
            1,
            "local",
            ObservationKind::Static,
            vec!["claim".into()],
            vec![],
            b"input",
            b"output",
            80,
        )
        .unwrap();
        assert!(bundle.link_job("").is_err());
    }

    #[test]
    fn invalid_bundle_is_rejected_before_a_new_journal_is_created() {
        let root =
            std::env::temp_dir().join(format!("atlas-evidence-invalid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let store = crate::store::Store::new(&root);
        let mut bundle = sample();
        bundle.bundle_id = "not-the-identity-hash".into();

        assert!(super::upsert(&store, bundle).is_err());
        assert!(!root.join("evidence.json").exists());
        let _ = std::fs::remove_dir_all(root);
    }
}
