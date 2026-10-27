use serde::Deserialize;
use serde_json::Value;
use std::path::{Path, PathBuf};

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
            if candidate.exists() {
                let resolved = candidate
                    .canonicalize()
                    .map_err(|e| format!("private model path unavailable: {e}"))?;
                if !resolved.starts_with(&root) {
                    return Err(format!(
                        "private model path escapes private root: {}",
                        path.display()
                    ));
                }
            }
        }
        Ok(())
    }
}
