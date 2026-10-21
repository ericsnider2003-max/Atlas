//! Local-first Creative Workspace Core.
//!
//! The first slice is deliberately a durable, format-neutral project contract:
//! assets are immutable references, edits are named revisions, and renders are
//! recorded rather than claimed. Photo, vector, and video front ends can share
//! this contract without sharing unsafe mutable state.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const MAX_ASSETS: usize = 512;
pub const MAX_REVISIONS: usize = 512;
pub const MAX_CAPTION_CUES: usize = 20_000;
pub const MAX_TEXT_BYTES: usize = 64 * 1024;
pub const MAX_RENDERS: usize = 512;
pub const MAX_STAGED_BYTES: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaKind {
    Photo,
    Vector,
    Video,
    Audio,
    Caption,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EditOperation {
    PhotoCrop { x: u32, y: u32, width: u32, height: u32 },
    PhotoAdjust { exposure_milli: i32, saturation_milli: i32 },
    VectorPath { path: String, fill: String, stroke: String },
    VectorText { text: String, x_milli: i32, y_milli: i32 },
    VideoTrim { start_ms: u64, end_ms: u64 },
    VideoSplit { at_ms: u64 },
    VideoCaptionTrack { cues: Vec<CaptionCue> },
}

impl EditOperation {
    pub fn validate(&self) -> Result<(), String> {
        match self {
            Self::PhotoCrop { width, height, .. } if *width == 0 || *height == 0 => {
                Err("photo crop must have non-zero dimensions".into())
            }
            Self::PhotoAdjust { exposure_milli, saturation_milli }
                if !(-10_000..=10_000).contains(exposure_milli)
                    || !(-10_000..=10_000).contains(saturation_milli) =>
            {
                Err("photo adjustment is outside its bounded range".into())
            }
            Self::VectorPath { path, fill, stroke } => {
                validate_text(path, "vector path")?;
                validate_text(fill, "vector fill")?;
                validate_text(stroke, "vector stroke")
            }
            Self::VectorText { text, .. } => validate_text(text, "vector text"),
            Self::VideoTrim { end_ms, start_ms } if end_ms <= start_ms => {
                Err("video trim must move forward in time".into())
            }
            Self::VideoCaptionTrack { cues } => validate_captions(cues),
            _ => Ok(()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Asset {
    pub id: String,
    pub kind: MediaKind,
    pub source_name: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptionCue {
    pub start_ms: u64,
    pub end_ms: u64,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revision {
    pub id: String,
    pub parent: Option<String>,
    pub operation: String,
    pub asset_ids: Vec<String>,
    pub caption_cues: Vec<CaptionCue>,
    pub operations: Vec<EditOperation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderRecord {
    pub revision_id: String,
    pub format: String,
    pub output_name: String,
    pub completed: bool,
    pub warning: Option<String>,
}

/// A deterministic handoff from project history to a concrete renderer.
/// Renderer implementations may be native, WASM, or external-process based;
/// none of them should need to inspect mutable project state directly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenderPlan {
    pub revision_id: String,
    pub format: String,
    pub output_name: String,
    pub asset_ids: Vec<String>,
    pub caption_cues: Vec<CaptionCue>,
    pub deterministic_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderArtifact {
    pub media_type: String,
    pub bytes: Vec<u8>,
    pub deterministic_key: String,
}

/// Reference backend used by tests and headless workflows. It emits a stable
/// manifest rather than pretending to encode pixels or video frames. Native
/// and external codec backends can implement the same plan contract later.
pub trait RendererBackend {
    fn render(&self, project: &CreativeProject, plan: &RenderPlan) -> Result<RenderArtifact, String>;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ManifestRenderer;

impl RendererBackend for ManifestRenderer {
    fn render(&self, project: &CreativeProject, plan: &RenderPlan) -> Result<RenderArtifact, String> {
        let current = project.plan_render(&plan.revision_id, &plan.format, &plan.output_name)?;
        if current != *plan {
            return Err("creative render plan is stale or differs from project history".into());
        }
        let bytes = serde_json::to_vec(plan)
            .map_err(|error| format!("creative render manifest encode failed: {error}"))?;
        Ok(RenderArtifact {
            media_type: format!("application/x-atlas-render-plan+{}", plan.format),
            bytes,
            deterministic_key: plan.deterministic_key.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRenderCommand {
    pub program: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StagedInput {
    pub asset_id: String,
    pub path: std::path::PathBuf,
    pub bytes: u64,
}

/// Builds an auditable ffmpeg invocation without executing an external process.
/// Execution belongs at the application boundary, where cancellation,
/// sandboxing, and tool-version reporting can be enforced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FfmpegCommandBuilder {
    pub executable: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalRenderReport {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolFingerprint {
    pub executable: String,
    pub version: String,
}

impl FfmpegCommandBuilder {
    pub fn execute(&self, plan: &RenderPlan, inputs: &[StagedInput], output_root: &Path, timeout: std::time::Duration) -> Result<ExternalRenderReport, String> {
        let cancel = std::sync::atomic::AtomicBool::new(false);
        self.execute_with_cancel(plan, inputs, output_root, timeout, &cancel)
    }

    pub fn execute_with_cancel(&self, plan: &RenderPlan, inputs: &[StagedInput], output_root: &Path, timeout: std::time::Duration, cancel: &std::sync::atomic::AtomicBool) -> Result<ExternalRenderReport, String> {
        let command = self.command_for(plan).map_err(|error| {
            crate::heard!(Err::<(), _>(error.clone()));
            error
        })?;
        drop(command);
        if cancel.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(ExternalRenderReport { exit_code: None, stdout: String::new(), stderr: String::new(), timed_out: false, cancelled: true });
        }
        if inputs.is_empty() {
            return Err("ffmpeg execution requires at least one staged input".into());
        }
        for input in inputs {
            if !input.path.is_file() {
                return Err("ffmpeg execution received a missing staged input".into());
            }
        }
        std::fs::create_dir_all(output_root).map_err(|error| format!("render output directory unavailable: {error}"))?;
        let output = output_root.join(&plan.output_name);
        let mut command = std::process::Command::new(&self.executable);
        command.arg("-hide_banner").arg("-y");
        for input in inputs {
            command.arg("-i").arg(&input.path);
        }
        command.arg(&output);
        let mut child = command.stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).spawn()
            .map_err(|error| format!("ffmpeg could not start: {error}"))?;
        let started = std::time::Instant::now();
        let mut cancelled = false;
        let timed_out = loop {
            if child.try_wait().map_err(|error| format!("ffmpeg status failed: {error}"))?.is_some() {
                break false;
            }
            if cancel.load(std::sync::atomic::Ordering::Relaxed) {
                child.kill().map_err(|error| format!("ffmpeg cancellation kill failed: {error}"))?;
                cancelled = true;
                break false;
            }
            if started.elapsed() >= timeout {
                child.kill().map_err(|error| format!("ffmpeg timeout kill failed: {error}"))?;
                break true;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        let output_result = child.wait_with_output().map_err(|error| format!("ffmpeg output failed: {error}"))?;
        Ok(ExternalRenderReport {
            exit_code: output_result.status.code(),
            stdout: String::from_utf8_lossy(&output_result.stdout).chars().take(16_384).collect(),
            stderr: String::from_utf8_lossy(&output_result.stderr).chars().take(16_384).collect(),
            timed_out,
            cancelled,
        })
    }
}

impl FfmpegCommandBuilder {
    pub fn fingerprint(&self, timeout: std::time::Duration) -> Result<ToolFingerprint, String> {
        let mut child = std::process::Command::new(&self.executable)
            .arg("-version")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|error| format!("ffmpeg fingerprint could not start: {error}"))?;
        let started = std::time::Instant::now();
        loop {
            if child.try_wait().map_err(|error| format!("ffmpeg fingerprint status failed: {error}"))?.is_some() {
                break;
            }
            if started.elapsed() >= timeout {
                child.kill().map_err(|error| format!("ffmpeg fingerprint timeout kill failed: {error}"))?;
                return Err("ffmpeg fingerprint timed out".into());
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        let output = child.wait_with_output().map_err(|error| format!("ffmpeg fingerprint output failed: {error}"))?;
        if !output.status.success() {
            return Err("ffmpeg fingerprint returned a failure status".into());
        }
        let version = String::from_utf8_lossy(&output.stdout).lines().next().unwrap_or_default().chars().take(512).collect::<String>();
        if version.is_empty() {
            return Err("ffmpeg fingerprint returned no version".into());
        }
        Ok(ToolFingerprint { executable: self.executable.clone(), version })
    }

    pub fn command_for(&self, plan: &RenderPlan) -> Result<ExternalRenderCommand, String> {
        if !matches!(plan.format.as_str(), "mp4" | "webm" | "wav") {
            return Err("ffmpeg backend only accepts video or audio plans".into());
        }
        Ok(ExternalRenderCommand {
            program: self.executable.clone(),
            args: vec!["-hide_banner".into(), "-y".into(), "-i".into(), "<asset-inputs>".into(), plan.output_name.clone()],
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreativeProject {
    pub id: String,
    pub title: String,
    pub assets: BTreeMap<String, Asset>,
    pub revisions: BTreeMap<String, Revision>,
    pub renders: Vec<RenderRecord>,
    pub current_revision: Option<String>,
}

impl CreativeProject {
    pub fn new(id: impl Into<String>, title: impl Into<String>) -> Result<Self, String> {
        let project = Self {
            id: id.into(),
            title: title.into(),
            assets: BTreeMap::new(),
            revisions: BTreeMap::new(),
            renders: Vec::new(),
            current_revision: None,
        };
        project.validate()?;
        Ok(project)
    }

    pub fn import_asset(&mut self, asset: Asset) -> Result<(), String> {
        validate_text(&asset.id, "asset id")?;
        validate_text(&asset.source_name, "asset source name")?;
        validate_text(&asset.digest, "asset digest")?;
        if self.assets.len() >= MAX_ASSETS && !self.assets.contains_key(&asset.id) {
            return Err("creative project has reached its asset limit".into());
        }
        if self.assets.insert(asset.id.clone(), asset).is_some() {
            return Err("creative asset identity already exists".into());
        }
        Ok(())
    }

    pub fn add_revision(&mut self, revision: Revision) -> Result<(), String> {
        validate_text(&revision.id, "revision id")?;
        validate_text(&revision.operation, "revision operation")?;
        if let Some(parent) = &revision.parent {
            if !self.revisions.contains_key(parent) {
                return Err("creative revision parent does not exist".into());
            }
        }
        if revision.asset_ids.iter().any(|id| !self.assets.contains_key(id)) {
            return Err("creative revision refers to an unknown asset".into());
        }
        validate_captions(&revision.caption_cues)?;
        if revision.operations.len() > MAX_ASSETS {
            return Err("creative revision has too many operations".into());
        }
        for operation in &revision.operations {
            operation.validate()?;
        }
        if self.revisions.len() >= MAX_REVISIONS && !self.revisions.contains_key(&revision.id) {
            return Err("creative project has reached its revision limit".into());
        }
        if self.revisions.insert(revision.id.clone(), revision).is_some() {
            return Err("creative revision identity already exists".into());
        }
        self.current_revision = Some(self.revisions.keys().next_back().unwrap().clone());
        Ok(())
    }

    pub fn record_render(&mut self, render: RenderRecord) -> Result<(), String> {
        if !self.revisions.contains_key(&render.revision_id) {
            return Err("creative render refers to an unknown revision".into());
        }
        validate_render_format(&render.format)?;
        validate_output_name(&render.output_name)?;
        if self.renders.len() >= MAX_RENDERS {
            return Err("creative project has reached its render limit".into());
        }
        self.renders.push(render);
        Ok(())
    }

    pub fn plan_render(&self, revision_id: &str, format: &str, output_name: &str) -> Result<RenderPlan, String> {
        let revision = self.revisions.get(revision_id).ok_or_else(|| "creative render refers to an unknown revision".to_string())?;
        validate_render_format(format)?;
        validate_output_name(output_name)?;
        let deterministic_key = format!("{revision_id}:{format}:{output_name}");
        Ok(RenderPlan {
            revision_id: revision_id.into(),
            format: format.into(),
            output_name: output_name.into(),
            asset_ids: revision.asset_ids.clone(),
            caption_cues: revision.caption_cues.clone(),
            deterministic_key,
        })
    }

    /// Copy revision inputs into a renderer-owned directory. Asset references
    /// are treated as relative names under `source_root`; traversal, symlinked
    /// escapes, and oversized batches are rejected before any tool runs.
    pub fn stage_inputs(&self, plan: &RenderPlan, source_root: &Path, staging_root: &Path) -> Result<Vec<StagedInput>, String> {
        if self.plan_render(&plan.revision_id, &plan.format, &plan.output_name)? != *plan {
            return Err("creative render plan is stale or differs from project history".into());
        }
        let source_root = source_root.canonicalize().map_err(|error| format!("source root unavailable: {error}"))?;
        std::fs::create_dir_all(staging_root).map_err(|error| format!("staging directory unavailable: {error}"))?;
        let staging_root = staging_root.canonicalize().map_err(|error| format!("staging directory unavailable: {error}"))?;
        let mut total = 0u64;
        let mut staged = Vec::with_capacity(plan.asset_ids.len());
        for asset_id in &plan.asset_ids {
            let asset = self.assets.get(asset_id).ok_or_else(|| "creative render asset does not exist".to_string())?;
            let relative = Path::new(&asset.source_name);
            if relative.is_absolute() || relative.components().any(|component| matches!(component, std::path::Component::ParentDir)) {
                return Err("creative asset source must stay under the source root".into());
            }
            let source = source_root.join(relative);
            let source = source.canonicalize().map_err(|error| format!("creative asset source unavailable: {error}"))?;
            if !source.starts_with(&source_root) {
                return Err("creative asset source escapes the source root".into());
            }
            let metadata = std::fs::metadata(&source).map_err(|error| format!("creative asset metadata unavailable: {error}"))?;
            if !metadata.is_file() {
                return Err("creative asset source is not a regular file".into());
            }
            total = total.checked_add(metadata.len()).ok_or_else(|| "creative staged input size overflow".to_string())?;
            if total > MAX_STAGED_BYTES {
                return Err("creative staged inputs exceed their size limit".into());
            }
            let name = source.file_name().ok_or_else(|| "creative asset source has no file name".to_string())?;
            let destination = staging_root.join(name);
            std::fs::copy(&source, &destination).map_err(|error| format!("creative asset staging failed: {error}"))?;
            staged.push(StagedInput { asset_id: asset_id.clone(), path: destination, bytes: metadata.len() });
        }
        Ok(staged)
    }

    pub fn validate(&self) -> Result<(), String> {
        validate_text(&self.id, "project id")?;
        validate_text(&self.title, "project title")?;
        if self.assets.len() > MAX_ASSETS || self.revisions.len() > MAX_REVISIONS || self.renders.len() > MAX_RENDERS {
            return Err("creative project exceeds its local bounds".into());
        }
        if let Some(current) = &self.current_revision {
            if !self.revisions.contains_key(current) {
                return Err("current creative revision does not exist".into());
            }
        }
        Ok(())
    }

    /// Persist a project as one owned JSON file. The store layer writes through
    /// its safe replacement path so an interrupted write cannot leave a half
    /// project for the next reopen.
    pub fn save(&self, path: &Path) -> crate::error::Result<()> {
        self.validate().map_err(crate::error::AtlasError::Config)?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| crate::error::AtlasError::Config(format!("creative project encode: {error}")))?;
        crate::store::write_owned_file(path, &bytes)?;
        Ok(())
    }

    /// Reopen a project and validate its references before returning it.
    pub fn load(path: &Path) -> crate::error::Result<Self> {
        let bytes = std::fs::read(path)?;
        let project: Self = serde_json::from_slice(&bytes)
            .map_err(|error| crate::error::AtlasError::Config(format!("creative project decode: {error}")))?;
        project.validate().map_err(crate::error::AtlasError::Config)?;
        Ok(project)
    }
}

fn validate_text(value: &str, label: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > MAX_TEXT_BYTES {
        Err(format!("{label} is empty or too large"))
    } else {
        Ok(())
    }
}

fn validate_captions(cues: &[CaptionCue]) -> Result<(), String> {
    if cues.len() > MAX_CAPTION_CUES {
        return Err("creative caption track exceeds its cue limit".into());
    }
    for cue in cues {
        if cue.end_ms <= cue.start_ms || cue.text.is_empty() || cue.text.len() > MAX_TEXT_BYTES {
            return Err("creative caption cue is invalid".into());
        }
    }
    Ok(())
}

fn validate_render_format(format: &str) -> Result<(), String> {
    if !matches!(format, "png" | "jpeg" | "webp" | "svg" | "mp4" | "webm" | "wav" | "srt" | "vtt") {
        return Err("creative render format is unsupported".into());
    }
    Ok(())
}

fn validate_output_name(name: &str) -> Result<(), String> {
    validate_text(name, "render output name")?;
    if name.contains(['/', '\\']) || name == "." || name == ".." || name.contains("..") {
        return Err("creative render output must be a single safe file name".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo() -> Asset {
        Asset { id: "photo-1".into(), kind: MediaKind::Photo, source_name: "lake.png".into(), digest: "sha256:lake".into() }
    }

    #[test]
    fn a_project_imports_assets_and_keeps_a_named_revision() {
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(photo()).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose photo layout".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![], operations: vec![EditOperation::PhotoCrop { x: 0, y: 0, width: 800, height: 600 }] }).unwrap();
        assert_eq!(project.current_revision.as_deref(), Some("rev-1"));
    }

    #[test]
    fn a_revision_cannot_refer_to_an_unknown_asset_or_parent() {
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        let revision = Revision { id: "rev-1".into(), parent: Some("missing".into()), operation: "edit".into(), asset_ids: vec!["missing".into()], caption_cues: vec![], operations: vec![] };
        assert!(project.add_revision(revision).is_err());
    }

    #[test]
    fn caption_cues_need_forward_time_and_bounded_text() {
        assert!(validate_captions(&[CaptionCue { start_ms: 5, end_ms: 5, text: "bad".into() }]).is_err());
        assert!(validate_captions(&[CaptionCue { start_ms: 0, end_ms: 1_000, text: "hello".into() }]).is_ok());
    }

    #[test]
    fn a_render_must_point_at_a_real_revision() {
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        assert!(project.record_render(RenderRecord { revision_id: "missing".into(), format: "png".into(), output_name: "out.png".into(), completed: true, warning: None }).is_err());
    }

    #[test]
    fn renders_accept_known_formats_and_reject_unsafe_destinations() {
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(photo()).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![], operations: vec![] }).unwrap();
        assert!(project.record_render(RenderRecord { revision_id: "rev-1".into(), format: "png".into(), output_name: "out.png".into(), completed: false, warning: Some("renderer pending".into()) }).is_ok());
        assert!(project.record_render(RenderRecord { revision_id: "rev-1".into(), format: "exe".into(), output_name: "out.exe".into(), completed: true, warning: None }).is_err());
        assert!(project.record_render(RenderRecord { revision_id: "rev-1".into(), format: "png".into(), output_name: "..\\escape.png".into(), completed: true, warning: None }).is_err());
    }

    #[test]
    fn render_plan_is_deterministic_and_carries_revision_inputs() {
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(photo()).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![CaptionCue { start_ms: 0, end_ms: 1_000, text: "Lake".into() }], operations: vec![] }).unwrap();
        let first = project.plan_render("rev-1", "png", "out.png").unwrap();
        let second = project.plan_render("rev-1", "png", "out.png").unwrap();
        assert_eq!(first, second);
        assert_eq!(first.asset_ids, vec!["photo-1"]);
        assert_eq!(first.caption_cues[0].text, "Lake");
        assert!(project.plan_render("missing", "png", "out.png").is_err());
    }

    #[test]
    fn manifest_renderer_rejects_stale_plans_and_emits_repeatable_bytes() {
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(photo()).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![], operations: vec![] }).unwrap();
        let plan = project.plan_render("rev-1", "svg", "out.svg").unwrap();
        let renderer = ManifestRenderer;
        let first = renderer.render(&project, &plan).unwrap();
        let second = renderer.render(&project, &plan).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.media_type, "application/x-atlas-render-plan+svg");
        let stale = RenderPlan { output_name: "changed.svg".into(), ..plan };
        assert!(renderer.render(&project, &stale).is_err());
    }

    #[test]
    fn ffmpeg_command_builder_is_explicit_about_supported_media() {
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(photo()).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![], operations: vec![] }).unwrap();
        let builder = FfmpegCommandBuilder { executable: "ffmpeg".into() };
        let video = project.plan_render("rev-1", "mp4", "out.mp4").unwrap();
        let command = builder.command_for(&video).unwrap();
        assert_eq!(command.program, "ffmpeg");
        assert!(command.args.contains(&"<asset-inputs>".into()));
        let image = project.plan_render("rev-1", "png", "out.png").unwrap();
        assert!(builder.command_for(&image).is_err());
    }

    #[test]
    fn staging_copies_inputs_and_rejects_traversal() {
        let root = std::env::temp_dir().join(format!("atlas-creative-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("source")).unwrap();
        let source = root.join("source").join("lake.png");
        std::fs::write(&source, b"fixture").unwrap();
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(Asset { source_name: "lake.png".into(), ..photo() }).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![], operations: vec![] }).unwrap();
        let plan = project.plan_render("rev-1", "mp4", "out.mp4").unwrap();
        let staged = project.stage_inputs(&plan, &root.join("source"), &root.join("stage")).unwrap();
        assert_eq!(staged[0].bytes, 7);
        assert_eq!(std::fs::read(&staged[0].path).unwrap(), b"fixture");
        project.assets.get_mut("photo-1").unwrap().source_name = "..\\outside.png".into();
        let plan = project.plan_render("rev-1", "mp4", "out.mp4").unwrap();
        assert!(project.stage_inputs(&plan, &root.join("source"), &root.join("stage-2")).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ffmpeg_execution_fails_closed_before_starting_without_inputs() {
        let plan = RenderPlan { revision_id: "rev-1".into(), format: "mp4".into(), output_name: "out.mp4".into(), asset_ids: vec![], caption_cues: vec![], deterministic_key: "rev-1:mp4:out.mp4".into() };
        let builder = FfmpegCommandBuilder { executable: "ffmpeg".into() };
        assert!(builder.execute(&plan, &[], &std::env::temp_dir(), std::time::Duration::from_millis(10)).is_err());
    }

    #[test]
    fn ffmpeg_execution_honors_preflight_cancellation() {
        let plan = RenderPlan { revision_id: "rev-1".into(), format: "mp4".into(), output_name: "out.mp4".into(), asset_ids: vec!["asset-1".into()], caption_cues: vec![], deterministic_key: "rev-1:mp4:out.mp4".into() };
        let cancel = std::sync::atomic::AtomicBool::new(true);
        let builder = FfmpegCommandBuilder { executable: "ffmpeg".into() };
        let report = builder.execute_with_cancel(&plan, &[], &std::env::temp_dir(), std::time::Duration::from_millis(10), &cancel).unwrap();
        assert!(report.cancelled);
        assert!(report.exit_code.is_none());
    }

    #[test]
    fn incomplete_render_records_survive_reopen_for_recovery() {
        let root = std::env::temp_dir().join(format!("atlas-creative-recovery-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("project.json");
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(photo()).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![], operations: vec![] }).unwrap();
        project.record_render(RenderRecord { revision_id: "rev-1".into(), format: "mp4".into(), output_name: "out.mp4".into(), completed: false, warning: Some("cancelled".into()) }).unwrap();
        project.save(&path).unwrap();
        let reopened = CreativeProject::load(&path).unwrap();
        assert_eq!(reopened.renders.len(), 1);
        assert!(!reopened.renders[0].completed);
        assert_eq!(reopened.renders[0].warning.as_deref(), Some("cancelled"));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn ffmpeg_fingerprint_fails_closed_for_missing_tool() {
        let builder = FfmpegCommandBuilder { executable: "atlas-tool-that-does-not-exist".into() };
        assert!(builder.fingerprint(std::time::Duration::from_millis(10)).is_err());
    }

    #[test]
    fn a_project_reopens_with_its_revision_history() {
        let root = std::env::temp_dir().join(format!("atlas-creative-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("project.json");
        let mut project = CreativeProject::new("p-1", "Lake campaign").unwrap();
        project.import_asset(photo()).unwrap();
        project.add_revision(Revision { id: "rev-1".into(), parent: None, operation: "compose".into(), asset_ids: vec!["photo-1".into()], caption_cues: vec![], operations: vec![EditOperation::PhotoAdjust { exposure_milli: 100, saturation_milli: 0 }] }).unwrap();
        project.save(&path).unwrap();
        let reopened = CreativeProject::load(&path).unwrap();
        assert_eq!(reopened.current_revision.as_deref(), Some("rev-1"));
        assert_eq!(reopened.revisions.len(), 1);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn a_corrupt_project_is_refused_on_reopen() {
        let root = std::env::temp_dir().join(format!("atlas-creative-corrupt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&root);
        let path = root.join("project.json");
        std::fs::write(&path, b"not json").unwrap();
        assert!(CreativeProject::load(&path).is_err());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn media_operations_are_bounded_before_they_enter_history() {
        assert!(EditOperation::PhotoCrop { x: 0, y: 0, width: 0, height: 10 }.validate().is_err());
        assert!(EditOperation::VideoTrim { start_ms: 10, end_ms: 5 }.validate().is_err());
        assert!(EditOperation::VectorText { text: "title".into(), x_milli: 0, y_milli: 0 }.validate().is_ok());
    }
}
