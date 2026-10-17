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
    pub claims: Vec<String>,
    pub limitations: Vec<String>,
    pub input_hash: String,
    pub output_hash: String,
    pub confidence: u8,
    pub supersedes: Option<String>,
    pub receipt_job_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Difference {
    pub input_changed: bool,
    pub output_changed: bool,
    pub claims_added: Vec<String>,
    pub claims_removed: Vec<String>,
    pub limitations_changed: bool,
    pub confidence_changed: bool,
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
            version: 1,
            bundle_id: String::new(),
            subject: subject.into(),
            run_id: run_id.into(),
            source_revision: source_revision.into(),
            observed_at,
            provider: provider.into(),
            kind,
            claims,
            limitations,
            input_hash: hash(input),
            output_hash: hash(output),
            confidence,
            supersedes: None,
            receipt_job_id: None,
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
        })
    }

    pub(crate) fn link_job(&mut self, job_id: impl Into<String>) -> Result<(), String> {
        self.receipt_job_id = Some(job_id.into());
        self.bundle_id = hash(&self.identity_bytes());
        self.validate()
    }

    fn validate_shape(&self) -> Result<(), String> {
        if self.version != 1 || self.subject.is_empty() || self.run_id.is_empty() {
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
        Ok(())
    }

    fn identity_bytes(&self) -> Vec<u8> {
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
pub(crate) fn task_outcome(
    job_id: &str,
    outcome: &crate::taskloop::Outcome,
    observed_at: u64,
) -> Result<Bundle, String> {
    let kind = match outcome {
        crate::taskloop::Outcome::Started(_) => ObservationKind::Runtime,
        crate::taskloop::Outcome::NeedsYou(_) => ObservationKind::Derived,
        crate::taskloop::Outcome::Done(_) | crate::taskloop::Outcome::Failed(_) => {
            ObservationKind::Runtime
        }
    };
    let confidence = match outcome {
        crate::taskloop::Outcome::Done(_) => 70,
        crate::taskloop::Outcome::Started(_) => 35,
        crate::taskloop::Outcome::NeedsYou(_) => 30,
        crate::taskloop::Outcome::Failed(_) => 20,
    };
    let text = outcome.text().to_string();
    let mut bundle = Bundle::create(
        "atlas-task-loop",
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
    let claim = format!("{detail} ({source_count} source(s))");
    let mut bundle = Bundle::create(
        "atlas-research",
        job_id,
        "working-tree",
        observed_at,
        "local-research",
        ObservationKind::Runtime,
        vec![claim],
        vec!["source freshness and owner/device proof remain separate".into()],
        topic.as_bytes(),
        detail.as_bytes(),
        if saved && source_count > 0 { 75 } else { 25 },
    )?;
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
    bundle.link_job(job_id)?;
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
    bundle.link_job(job_id)?;
    Ok(bundle)
}

fn hash(bytes: &[u8]) -> String {
    crate::sealedlog::hex(&crate::sealedlog::leaf_hash(bytes))
}

/// Persist an immutable bundle in Atlas's existing atomic Store. The journal
/// is bounded so repeated self-audits cannot consume the machine forever.
pub(crate) fn upsert(store: &crate::store::Store, bundle: Bundle) -> crate::error::Result<()> {
    bundle
        .validate()
        .map_err(crate::error::AtlasError::Config)?;
    let mut bundles = load_checked(store)?;
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
    store.save(STORE_NAME, &bundles)
}

/// Find the newest valid observation that can answer the same local question.
/// A match never ignores revision or input changes, and malformed stored data
/// is skipped rather than promoted into a reusable claim.
pub(crate) fn reusable(
    store: &crate::store::Store,
    subject: &str,
    source_revision: &str,
    input_hash: &str,
) -> Option<Bundle> {
    let mut bundles = load_checked(store).ok()?;
    bundles.sort_by_key(|bundle| std::cmp::Reverse(bundle.observed_at));
    bundles.into_iter().find(|bundle| {
        bundle.subject == subject
            && bundle.source_revision == source_revision
            && bundle.input_hash == input_hash
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
        build_outcome, media_outcome, research_outcome, self_audit_outcome, task_outcome, Bundle,
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
        assert_eq!(bundle.subject, "atlas-task-loop");
        assert_eq!(bundle.provider, "local-daemon");
        assert_eq!(bundle.kind, ObservationKind::Runtime);
        assert_eq!(bundle.confidence, 70);
        assert_eq!(bundle.receipt_job_id.as_deref(), Some("task-1"));
        assert_eq!(bundle.claims, vec!["saved locally"]);
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn failed_task_outcome_cannot_look_like_a_completed_observation() {
        let outcome = crate::taskloop::Outcome::Failed("provider unavailable".into());
        let bundle = task_outcome("task-failed", &outcome, 42).unwrap();
        assert_eq!(bundle.confidence, 20);
        assert!(bundle.claims[0].contains("provider unavailable"));
        assert!(bundle.validate().is_ok());
    }

    #[test]
    fn research_outcome_writes_source_count_and_limits_confidence() {
        let bundle =
            research_outcome("research-1", "local systems", 2, true, "research saved", 42).unwrap();
        assert_eq!(bundle.subject, "atlas-research");
        assert_eq!(bundle.provider, "local-research");
        assert_eq!(bundle.confidence, 75);
        assert!(bundle.claims[0].contains("2 source(s)"));
        assert_eq!(bundle.receipt_job_id.as_deref(), Some("research-1"));
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
        assert!(good.claims[0].contains("verified local build saved"));
        assert_eq!(bad.confidence, 35);
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
        assert!(kept.claims[0].contains("kept"));
        assert_eq!(declined.confidence, 100);
        assert!(declined.claims[0].contains("declined"));
        assert!(declined.limitations[0].contains("publication"));
    }

    #[test]
    fn self_audit_outcome_does_not_claim_recommendations_were_applied() {
        let bundle = self_audit_outcome("self-audit-1", 3, 2, 42).unwrap();
        assert_eq!(bundle.subject, "atlas-self-audit");
        assert_eq!(bundle.provider, "local-self-audit");
        assert_eq!(bundle.kind, ObservationKind::Derived);
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
        assert!(
            super::reusable(&store, "atlas-self-audit", "abc123", &bundle.input_hash).is_some()
        );
        assert!(
            super::reusable(&store, "atlas-self-audit", "changed", &bundle.input_hash).is_none()
        );
        assert!(super::reusable(&store, "other", "abc123", &bundle.input_hash).is_none());
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

        assert!(super::reusable(&store, "atlas-self-audit", "abc123", "nope").is_none());
        assert!(super::upsert(&store, sample()).is_err());
        assert_eq!(
            std::fs::read(root.join("evidence.json")).unwrap(),
            b"{not-json"
        );
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
