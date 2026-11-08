//! Validated adapters for explicitly installed private reasoning and voice
//! providers. This module owns artifact-bound configuration and the seams that
//! prevent an enabled private model from being silently replaced by another
//! provider.

use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PrivateModelsConfig {
    pub root: PathBuf,
    pub reasoning: PrivateModelSlot,
    pub voice: PrivateModelSlot,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct PrivateModelSlot {
    pub enabled: bool,
    pub manifest: PathBuf,
    pub mode: String,
    pub max_context_chars: usize,
    pub timeout_seconds: u64,
}

impl PrivateModelsConfig {
    /// Validate enabled private providers at the real model-selection seam.
    /// Invalid private configuration is a hard configuration error: Atlas must
    /// not silently fall back to a smaller or online model and lower quality.
    pub fn validate_enabled(&self) -> Result<(), String> {
        if self.root.as_os_str().is_empty() {
            if self.reasoning.enabled || self.voice.enabled {
                return Err(
                    "private model root is required when a private provider is enabled".into(),
                );
            }
            return Ok(());
        }
        let root = self.root.as_path();
        for (kind, slot, expected) in [
            (PrivateModelKind::Reasoning, &self.reasoning, "reasoning"),
            (PrivateModelKind::Voice, &self.voice, "voice"),
        ] {
            if !slot.enabled {
                continue;
            }
            if slot.manifest.as_os_str().is_empty() {
                return Err(format!("enabled private {expected} model has no manifest"));
            }
            let path = manifest_path_under(root, &slot.manifest, expected)?;
            let bytes = std::fs::read(&path)
                .map_err(|e| format!("private {expected} manifest unavailable: {e}"))?;
            let value: Value = serde_json::from_slice(&bytes)
                .map_err(|e| format!("private {expected} manifest is not JSON: {e}"))?;
            let manifest = PrivateModelManifest::from_value(&value)?;
            if manifest.kind != kind {
                return Err(format!(
                    "private {expected} manifest declares the wrong kind"
                ));
            }
            manifest.validate_under(root)?;
        }
        Ok(())
    }

    /// Return the validated reasoning command's executable and deadline.
    ///
    /// The manifest is deliberately read again at the runtime seam. This
    /// keeps selection safe when configuration changes between startup and a
    /// request, and prevents callers from executing an unvalidated path.
    pub fn reasoning_command(
        &self,
    ) -> Result<Option<(PathBuf, PathBuf, u64, usize, String)>, String> {
        if !self.reasoning.enabled {
            return Ok(None);
        }
        if self.root.as_os_str().is_empty() {
            return Err("private model root is required when reasoning is enabled".into());
        }
        let manifest_path = manifest_path_under(&self.root, &self.reasoning.manifest, "reasoning")?;
        let bytes = std::fs::read(&manifest_path)
            .map_err(|e| format!("private reasoning manifest unavailable: {e}"))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("private reasoning manifest is not JSON: {e}"))?;
        let manifest = PrivateModelManifest::from_value(&value)?;
        if manifest.kind != PrivateModelKind::Reasoning {
            return Err("private reasoning manifest declares the wrong kind".into());
        }
        if !self.reasoning.mode.is_empty() && self.reasoning.mode != "validated_json" {
            return Err(format!(
                "private reasoning mode is unsupported: {}",
                self.reasoning.mode
            ));
        }
        manifest.validate_under(&self.root)?;
        if manifest.runtime != PrivateModelRuntime::Command {
            return Err(format!(
                "private reasoning runtime {:?} has no executable adapter",
                manifest.runtime
            ));
        }
        let entrypoint = self.root.join(manifest.entrypoint);
        let entrypoint_sha256 = crate::digest::sha256_file_hex(&entrypoint)
            .map_err(|e| format!("private reasoning entrypoint cannot be hashed: {e}"))?;
        Ok(Some((
            entrypoint,
            self.root.join(manifest.artifact_dir),
            self.reasoning.timeout_seconds.max(1),
            self.reasoning.max_context_chars,
            entrypoint_sha256,
        )))
    }

    /// Return the validated private voice command.
    pub fn voice_command(&self) -> Result<Option<(PathBuf, PathBuf, u64, String)>, String> {
        if !self.voice.enabled {
            return Ok(None);
        }
        if self.root.as_os_str().is_empty() {
            return Err("private model root is required when voice is enabled".into());
        }
        let manifest_path = manifest_path_under(&self.root, &self.voice.manifest, "voice")?;
        let bytes = std::fs::read(&manifest_path)
            .map_err(|e| format!("private voice manifest unavailable: {e}"))?;
        let value: Value = serde_json::from_slice(&bytes)
            .map_err(|e| format!("private voice manifest is not JSON: {e}"))?;
        let manifest = PrivateModelManifest::from_value(&value)?;
        if manifest.kind != PrivateModelKind::Voice {
            return Err("private voice manifest declares the wrong kind".into());
        }
        manifest.validate_under(&self.root)?;
        if manifest.runtime != PrivateModelRuntime::Command {
            return Err(format!(
                "private voice runtime {:?} has no executable adapter",
                manifest.runtime
            ));
        }
        let entrypoint = self.root.join(manifest.entrypoint);
        let entrypoint_sha256 = crate::digest::sha256_file_hex(&entrypoint)
            .map_err(|e| format!("private voice entrypoint cannot be hashed: {e}"))?;
        Ok(Some((
            entrypoint,
            self.root.join(manifest.artifact_dir),
            self.voice.timeout_seconds.max(1),
            entrypoint_sha256,
        )))
    }
}

/// Runtime seam used by model selection; kept as a free function so the
/// provider boundary is visible to wiring audits and callers.
pub fn reasoning_command(
    cfg: &PrivateModelsConfig,
) -> Result<Option<(PathBuf, PathBuf, u64, usize, String)>, String> {
    cfg.reasoning_command()
}

/// Runtime seam used by speech synthesis.
pub fn voice_command(
    cfg: &PrivateModelsConfig,
) -> Result<Option<(PathBuf, PathBuf, u64, String)>, String> {
    cfg.voice_command()
}

fn manifest_path_under(root: &Path, manifest: &Path, kind: &str) -> Result<PathBuf, String> {
    if manifest.is_absolute()
        || manifest
            .components()
            .any(|component| matches!(component, std::path::Component::ParentDir))
    {
        return Err(format!("private {kind} manifest path escapes private root"));
    }
    let root = root
        .canonicalize()
        .map_err(|e| format!("private root unavailable: {e}"))?;
    let resolved = root
        .join(manifest)
        .canonicalize()
        .map_err(|e| format!("private {kind} manifest unavailable: {e}"))?;
    if !resolved.starts_with(&root) {
        return Err(format!("private {kind} manifest path escapes private root"));
    }
    if !resolved.is_file() {
        return Err(format!("private {kind} manifest is not a file"));
    }
    Ok(resolved)
}

pub fn validate_reasoning_action(value: &Value) -> Result<(), String> {
    let object = value
        .as_object()
        .ok_or_else(|| "reasoning output must be a JSON object".to_string())?;
    let action = object
        .get("action")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| "reasoning output needs a non-empty action".to_string())?;
    if !action
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_')
    {
        return Err("reasoning action contains unsupported characters".into());
    }
    if !object.get("arguments").is_some_and(Value::is_object) {
        return Err("reasoning output arguments must be an object".into());
    }
    if !object
        .get("requires_confirmation")
        .is_some_and(Value::is_boolean)
    {
        return Err("reasoning output requires_confirmation must be boolean".into());
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateModelKind {
    Voice,
    Reasoning,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum PrivateModelRuntime {
    #[serde(rename = "f5_tts")]
    F5Tts,
    #[serde(rename = "llama_cpp")]
    LlamaCpp,
    #[serde(rename = "command")]
    Command,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PrivateModelManifest {
    pub schema: u32,
    pub kind: PrivateModelKind,
    pub model_id: String,
    pub runtime: PrivateModelRuntime,
    pub entrypoint: PathBuf,
    pub artifact_dir: PathBuf,
    pub sha256: String,
    pub license: String,
    pub dataset_version: String,
}

impl PrivateModelManifest {
    pub fn from_value(value: &Value) -> Result<Self, String> {
        let manifest: Self = serde_json::from_value(value.clone())
            .map_err(|e| format!("invalid private model manifest: {e}"))?;
        if manifest.schema != 1 {
            return Err(format!(
                "unsupported private model manifest schema: {}",
                manifest.schema
            ));
        }
        if manifest.model_id.trim().is_empty()
            || manifest.sha256.trim().is_empty()
            || manifest.license.trim().is_empty()
            || manifest.dataset_version.trim().is_empty()
        {
            return Err("private model manifest contains an empty required field".into());
        }
        Ok(manifest)
    }

    pub fn validate_under(&self, private_root: &Path) -> Result<(), String> {
        if self.entrypoint.is_absolute() || self.artifact_dir.is_absolute() {
            return Err("private model paths must be relative to the private root".into());
        }
        for path in [&self.entrypoint, &self.artifact_dir] {
            if path
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                return Err(format!(
                    "private model path escapes private root: {}",
                    path.display()
                ));
            }
        }
        let root = private_root
            .canonicalize()
            .map_err(|e| format!("private root unavailable: {e}"))?;
        for path in [&self.entrypoint, &self.artifact_dir] {
            let candidate = root.join(path);
            let metadata = std::fs::metadata(&candidate)
                .map_err(|e| format!("private model path unavailable: {} ({e})", path.display()))?;
            let resolved = candidate
                .canonicalize()
                .map_err(|e| format!("private model path unavailable: {e}"))?;
            if !resolved.starts_with(&root) {
                return Err(format!(
                    "private model path escapes private root: {}",
                    path.display()
                ));
            }
            if path == &self.entrypoint && !metadata.is_file() {
                return Err(format!(
                    "private model entrypoint is not a file: {}",
                    path.display()
                ));
            }
            if path == &self.artifact_dir && !metadata.is_dir() {
                return Err(format!(
                    "private model artifact path is not a directory: {}",
                    path.display()
                ));
            }
        }
        if !self.sha256.chars().all(|c| c.is_ascii_hexdigit()) || self.sha256.len() != 64 {
            return Err("private model sha256 must be a 64-character hexadecimal digest".into());
        }
        let actual = crate::digest::sha256_file_hex(&root.join(&self.entrypoint))
            .map_err(|e| format!("private model entrypoint cannot be hashed: {e}"))?;
        if !actual.eq_ignore_ascii_case(&self.sha256) {
            return Err(format!(
                "private model entrypoint sha256 mismatch: expected {}, got {}",
                self.sha256, actual
            ));
        }
        Ok(())
    }
}
