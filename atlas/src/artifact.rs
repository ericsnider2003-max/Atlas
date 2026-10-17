//! Shared, bounded artifact edges for durable local work. This module gives
//! jobs, receipts, and research records a path-free way to name inputs,
//! outputs, and dependencies while keeping serialized metadata small and
//! validating every edge before it can be persisted or exposed to the hub.

use serde::{Deserialize, Serialize};
use std::collections::HashSet;

pub const MAX_ARTIFACTS: usize = 64;
pub const MAX_ID_BYTES: usize = 256;
pub const MAX_KIND_BYTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactRef {
    pub id: String,
    pub kind: String,
    pub digest: Option<String>,
}

impl ArtifactRef {
    pub fn new(
        id: impl Into<String>,
        kind: impl Into<String>,
        digest: Option<String>,
    ) -> Result<Self, String> {
        let artifact = Self {
            id: id.into(),
            kind: kind.into(),
            digest,
        };
        artifact.validate()?;
        Ok(artifact)
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.id.is_empty() || self.id.len() > MAX_ID_BYTES {
            return Err("artifact identity is empty or too large".into());
        }
        if self.kind.is_empty() || self.kind.len() > MAX_KIND_BYTES {
            return Err("artifact kind is empty or too large".into());
        }
        if self
            .digest
            .as_deref()
            .is_some_and(|digest| digest.len() > MAX_ID_BYTES)
        {
            return Err("artifact digest is too large".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope {
    pub input: Vec<ArtifactRef>,
    pub output: Vec<ArtifactRef>,
    pub depends_on: Vec<String>,
}

impl Envelope {
    pub fn validate(&self) -> Result<(), String> {
        if self.input.len() > MAX_ARTIFACTS
            || self.output.len() > MAX_ARTIFACTS
            || self.depends_on.len() > MAX_ARTIFACTS
        {
            return Err("artifact envelope exceeds the local item bound".into());
        }
        let mut artifact_ids = HashSet::new();
        for artifact in self.input.iter().chain(self.output.iter()) {
            artifact.validate()?;
            if !artifact_ids.insert(&artifact.id) {
                return Err("artifact identity is duplicated in the envelope".into());
            }
        }
        let mut dependency_ids = HashSet::new();
        if self
            .depends_on
            .iter()
            .any(|id| id.is_empty() || id.len() > MAX_ID_BYTES || !dependency_ids.insert(id))
        {
            return Err("artifact dependency identity is empty, too large, or duplicated".into());
        }
        let bytes = serde_json::to_vec(self)
            .map_err(|error| format!("artifact envelope encode: {error}"))?;
        if bytes.len() > crate::evidence::MAX_BUNDLE_BYTES {
            return Err("artifact envelope exceeds the local byte bound".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_default_envelope_is_valid() {
        assert!(Envelope::default().validate().is_ok());
    }

    #[test]
    fn oversized_artifact_lists_are_refused() {
        let envelope = Envelope {
            input: (0..=MAX_ARTIFACTS)
                .map(|index| ArtifactRef::new(format!("input-{index}"), "file", None).unwrap())
                .collect(),
            ..Envelope::default()
        };
        assert!(envelope.validate().is_err());
    }

    #[test]
    fn malformed_dependency_identity_is_refused() {
        let envelope = Envelope {
            depends_on: vec![String::new()],
            ..Envelope::default()
        };
        assert!(envelope.validate().is_err());
    }

    #[test]
    fn duplicate_artifact_identity_is_refused() {
        let artifact = ArtifactRef::new("same", "file", None).unwrap();
        let envelope = Envelope {
            input: vec![artifact.clone()],
            output: vec![artifact],
            ..Envelope::default()
        };
        assert!(envelope.validate().is_err());
    }

    #[test]
    fn duplicate_dependency_identity_is_refused() {
        let envelope = Envelope {
            depends_on: vec!["job-1".into(), "job-1".into()],
            ..Envelope::default()
        };
        assert!(envelope.validate().is_err());
    }
}
