//! Shared execution identity carried through Atlas's existing reply, job, and
//! evidence envelopes.
//!
//! This is deliberately a value object, not a provider registry or a second
//! capability catalogue. It records what the current adapter can identify and
//! uses `unknown` for facts the adapter cannot prove.

use serde::{Deserialize, Serialize};

pub const UNKNOWN: &str = "unknown";
const MAX_FIELD_BYTES: usize = 256;

/// Non-sensitive routing facts returned by an adapter or local gateway.
///
/// This is deliberately separate from `ExecutionDescriptor`: the descriptor
/// identifies the configured execution context, while this records what a
/// particular request reported about routing. All fields are optional because
/// local providers and older adapters may not expose them.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionMetadata {
    #[serde(default)]
    pub routed_via: Option<String>,
    #[serde(default)]
    pub fallback_attempts: Option<String>,
    #[serde(default)]
    pub fallback_trail: Option<String>,
}

impl ExecutionMetadata {
    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("routed_via", &self.routed_via),
            ("fallback_attempts", &self.fallback_attempts),
            ("fallback_trail", &self.fallback_trail),
        ] {
            if let Some(value) = value {
                if value.len() > MAX_FIELD_BYTES {
                    return Err(format!("execution metadata {name} is too long"));
                }
                if value.chars().any(char::is_control) {
                    return Err(format!("execution metadata {name} contains control text"));
                }
            }
        }
        Ok(())
    }
}

/// The execution context that gives a local result its meaning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionDescriptor {
    pub provider: String,
    pub model: String,
    pub runtime: String,
    pub device: String,
    pub network: String,
    pub cache: String,
}

impl Default for ExecutionDescriptor {
    fn default() -> Self {
        Self::unknown()
    }
}

impl ExecutionDescriptor {
    pub fn unknown() -> Self {
        Self {
            provider: UNKNOWN.into(),
            model: UNKNOWN.into(),
            runtime: UNKNOWN.into(),
            device: UNKNOWN.into(),
            network: UNKNOWN.into(),
            cache: UNKNOWN.into(),
        }
    }

    pub fn new(
        provider: impl Into<String>,
        model: impl Into<String>,
        runtime: impl Into<String>,
        device: impl Into<String>,
        network: impl Into<String>,
        cache: impl Into<String>,
    ) -> Result<Self, String> {
        let descriptor = Self {
            provider: provider.into(),
            model: model.into(),
            runtime: runtime.into(),
            device: device.into(),
            network: network.into(),
            cache: cache.into(),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    pub fn validate(&self) -> Result<(), String> {
        for (name, value) in [
            ("provider", &self.provider),
            ("model", &self.model),
            ("runtime", &self.runtime),
            ("device", &self.device),
            ("network", &self.network),
            ("cache", &self.cache),
        ] {
            if value.trim().is_empty() {
                return Err(format!("execution descriptor {name} cannot be empty"));
            }
            if value.len() > MAX_FIELD_BYTES {
                return Err(format!("execution descriptor {name} is too long"));
            }
            if value.chars().any(char::is_control) {
                return Err(format!("execution descriptor {name} contains control text"));
            }
        }
        Ok(())
    }

    pub fn is_unknown(&self) -> bool {
        self == &Self::unknown()
    }
}

#[cfg(test)]
mod tests {
    use super::{ExecutionDescriptor, ExecutionMetadata, UNKNOWN};

    #[test]
    fn unknown_is_explicit_and_round_trips() {
        let descriptor = ExecutionDescriptor::default();
        assert_eq!(descriptor.provider, UNKNOWN);
        let bytes = serde_json::to_vec(&descriptor).unwrap();
        assert_eq!(
            serde_json::from_slice::<ExecutionDescriptor>(&bytes).unwrap(),
            descriptor
        );
    }

    #[test]
    fn empty_and_control_text_are_rejected() {
        assert!(
            ExecutionDescriptor::new("", "model", "runtime", "device", "network", "cache").is_err()
        );
        assert!(ExecutionDescriptor::new(
            "provider", "model\n", "runtime", "device", "network", "cache"
        )
        .is_err());
    }

    #[test]
    fn routing_metadata_round_trips_and_rejects_control_text() {
        let metadata = ExecutionMetadata {
            routed_via: Some("free/provider".into()),
            fallback_attempts: Some("1".into()),
            fallback_trail: Some("local->free/provider".into()),
        };
        metadata.validate().unwrap();
        let bytes = serde_json::to_vec(&metadata).unwrap();
        assert_eq!(
            serde_json::from_slice::<ExecutionMetadata>(&bytes).unwrap(),
            metadata
        );

        let invalid = ExecutionMetadata {
            routed_via: Some("provider\n".into()),
            ..Default::default()
        };
        assert!(invalid.validate().is_err());
    }
}
