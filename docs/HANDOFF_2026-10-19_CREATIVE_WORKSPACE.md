# Atlas Creative Workspace Handoff — 2026-10-19

## Purpose

This handoff records the current state of the Atlas-owned creative-workspace
implementation and the remaining work needed before owner-facing acceptance.
The scope is photo, vector, and video creation/editing support with captions,
rendering, export, persistence, recovery, and an eventual UI.

## Current verdict

Goals 1 and 2 are evidenced. The creative source slice is locally verified but
is explicitly deferred from the current release: Goal 3 remains gated until
Goal 2 has owner acceptance. Its module is tracked as unwired in the ratchets;
no production caller or current runtime-support claim is being made. The
remaining release gate is owner-facing foundation acceptance, followed by a
deliberate decision to resume creative integration.

Do not describe Atlas as an Adobe replacement yet. The current code provides a
durable, format-neutral project contract, bounded media operations, safe input
staging, a deterministic manifest renderer, an auditable ffmpeg command plan,
bounded ffmpeg execution, tool fingerprinting, cancellation, and reopen
recovery. It does not yet provide a completed editor UI or full native photo,
vector, and video editing experience.

## Verified evidence

- Discovered test baseline: **9,030 tests across 45 targets**.
- The prior full serial all-target campaign reached the final target and zero-test
  example without observed failure output.
- `cargo check --manifest-path atlas\\Cargo.toml --all-targets`: **PASS** after
  the creative changes.
- Current focused creative suite:
  `cargo test --manifest-path atlas\\Cargo.toml creative:: --lib -- --test-threads=1`
  — **16 passed, 0 failed**.
- Local ffmpeg probe: version `9.0.1-full_build-www.gyan.dev`.
- Real fixture: ffmpeg rendered and decoded a 32x32 blue MP4 successfully;
  output size was 1,700 bytes.
- Filing check: `node .filing\\filing.mjs check --cold` is green with 59
  historical warnings. Those warnings are known stale-history references and
  are not silently removed in this handoff.
- Lean all-target campaign (2026-10-19): **7,005 passed, 16 failed, 8
  ignored**. The creative wiring/catalogue/deadness/silent-discard failures
  were resolved; the remaining failures are camera/audio/video tool
  availability. The proprietary-plan fingerprint was resolved by preserving
  the plan at `C:\\Users\\erics\\Atlas\\00_SOURCE_OF_TRUTH\\deferred-atlas-plans\\`
  outside the personal publish tree. This is not a green Goal 2 campaign.
- Dependency-rehearsal rerun (2026-10-20), with the installed bundled tools
  directory available: audio enumeration **8/8**, camera/looking **14/14**,
  and video studio **5/5**. The installed real-process foundation verifier is
  **14/14** in run `20261020T032512135Z-3e56479a` against the rebuilt installed
  candidate.

## Implemented code

Primary implementation:

- `atlas/src/creative.rs`
  - `MediaKind`, assets, captions, edit operations, revisions, render records.
  - Local JSON save/load with validation and safe owned-file replacement.
  - Photo crop/adjust bounds.
  - Vector path/text bounds.
  - Video trim/split bounds.
  - Caption cue validation.
  - Supported output-format validation.
  - Traversal-safe output names.
  - Deterministic `RenderPlan`.
  - `RendererBackend` and repeatable `ManifestRenderer` fixture backend.
  - `FfmpegCommandBuilder` for `mp4`, `webm`, and `wav`.
  - Renderer-owned input staging with source-root containment, regular-file
    checks, symlink/traversal escape rejection, and 512 MiB batch limit.
  - Bounded ffmpeg execution with timeout and capped output capture.
  - Tool fingerprinting with bounded version capture.
  - Preflight and in-flight cancellation.
  - Persistence of incomplete render records for reopen/retry recovery.

- `atlas/src/lib.rs`
  - Exposes `pub mod creative`.

## Documentation filed

- `docs/GOALS_AND_VERIFICATION_2026-10-18.md`
- `docs/VERIFICATION_CAMPAIGN_2026-10-18.md`
- `docs/TEST_COUNT_RECONCILIATION_2026-10-18.md`
- `docs/WORK_ITEMS_1_7_STATUS_2026-10-18.md`
- `docs/OUTSTANDING_2026-10-18.md`
- `docs/CREATIVE_WORKSPACE_CORE_SCOPE_2026-10-18.md`
- `docs/FILING_CANDIDATE_CLASSIFICATION_2026-10-18.md`
- this handoff

## Remaining work in order

1. Add an application-level job controller around `FfmpegCommandBuilder` so
   staging, execution, cancellation, output verification, and render-record
   updates are one recoverable workflow.
2. Add real photo/vector fixtures and verify output dimensions/content, not only
   the video fixture. Keep fixtures small and deterministic.
3. Add caption export fixtures for SRT/VTT and verify cue ordering and bounds.
4. Add interrupted-output cleanup/retry behavior and verify that partial files
   cannot be recorded as completed renders.
5. Integrate the project contract into Atlas's owner-facing UI: import asset,
   select revision, apply an edit, preview, caption, export, cancel, reopen.
6. Perform owner acceptance on Windows for photo, vector, video, captioning,
   export, recovery, keyboard/accessibility, and error states. This must be
   marked by the owner; headless tests cannot self-certify it.

Implementation defaults for the deferred creative slice: local-first files,
no automatic publishing or external account actions, no Adobe dependency, and
the order job controller → deterministic photo/vector/caption fixtures →
interrupted-output recovery → owner-facing UI. These are implementation
defaults, not claims of current support.

## Exact restart commands

From `C:\Users\erics\Atlas\atlas-publish`:

```powershell
cargo test --manifest-path atlas\\Cargo.toml creative:: --lib -- --test-threads=1
cargo check --manifest-path atlas\\Cargo.toml --all-targets
node .filing\\filing.mjs render
node .filing\\filing.mjs check --cold
```

For the installed verification campaign:

```powershell
pwsh -NoProfile -File .\\verification\\Invoke-AtlasVerification.ps1 -AtlasExe 'C:\Users\erics\AppData\Local\Atlas\atlas.exe'
```

## Known outstanding items

- Creative Workspace Core is deferred from the current release until the
  foundation campaign is green or Eric explicitly accepts an exception. The
  source contract remains preserved and tested; it is not claimed as a live
  Atlas capability.

- The two audited Cargo build-cache directories remain because deletion was
  blocked by environment destructive-action policy:
  `target_capability_wiring_continuation/` and `target_foundation_continuation/`.
- Strict all-target Clippy is not a release gate yet because existing test-target
  lints remain (`unwrap_used`, `expect_used`, and doc-comment formatting).
- Filing cold check has 59 historical warnings; do not bulk-delete or rewrite
  history without an owner decision.
- Usage-max remains observation-only with stale provider observations; no current
  allowance percentage should be inferred from it.
- The installed bundle contains `C:\\Users\\erics\\AppData\\Local\\Atlas\\tools\\ffmpeg\\ffmpeg.exe`.
  The matching FFmpeg 9.0.2 `ffprobe.exe` is now installed beside it. Audio
  enumeration passes with the bundled directory available; the source now
  anchors that directory for bare media calls and strips the internal
  `platform:` prefix from camera-facing errors. The corrected source has been
  rebuilt with the installed MSVC Rust toolchain and installed.

## Handoff completion condition

The creative-workspace goal is complete only when the owner has exercised the UI
flows on the target Windows install and accepted the resulting photo, vector,
video, caption, export, cancellation, and recovery behavior. Until then, the
core is verified but the product is not owner-accepted.
