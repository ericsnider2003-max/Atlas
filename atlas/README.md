# Atlas

A local-first workspace assistant for Windows. This repo is the working
foundation for the Atlas POA specification set.

**Status:** the orchestration core is written, compiles, and passes 18 tests on
any OS. The Windows syscall layer (`src/platform/win.rs`) is the one unverified
file — see below.

## Run it right now, without the target laptop

```bash
cargo test                              # 18 tests, no Windows required
cargo run -- --dry-run "boot workspace"
cargo run -- --dry-run "shutdown workspace"
cargo run -- --dry-run --yes "shutdown workspace"
cargo run -- --dry-run                  # interactive
cargo run -- doctor                     # what is actually on this machine
cargo run -- --wake                     # hands-free
```

Dry-run swaps in `MockPlatform`, a fake OS that reports monitors, simulates
slow-starting apps, and records every action. That is how the whole startup
sequence is testable months before you touch the laptop.

## What is actually implemented

| Area | State |
|---|---|
| YAML config with cross-validation at load | ✅ tested |
| Intent parsing, longest-phrase-wins | ✅ tested |
| Monitor roles resolved by geometry, not by ID | ✅ tested |
| Fallback when displays are unplugged | ✅ tested |
| `workspace_on` — launch, wait for window, place | ✅ tested |
| Partial-failure reporting (one app dying ≠ abort) | ✅ tested |
| Approval gate, unknown intents default to blocked | ✅ tested |
| External tool runner (expansion, stdin, exit codes) | ✅ tested |
| Voice loop: record → STT → act → TTS → play | ✅ tested end to end |
| Screen capture via ffmpeg gdigrab | ✅ tested |
| `atlas doctor` — environment discovery | ✅ working |
| Windows syscalls (`EnumWindows`, `SetWindowPos`, …) | ⚠️ **written, never compiled** |
| Reasoning layer (LLM tool-calling, JSON protocol) | ✅ tested |
| Falls back to fixed phrases when the model is down | ✅ tested |
| Wake word (loose matching, pluggable detector) | ✅ tested |
| Webcam capture + vision model plumbing | ✅ tested |
| Indexing, research, scheduler | ❌ not started |
| Credential vault, payment autofill | ❌ **and should not be** — see `docs/AUDIT.md` §4 |

## Architecture in one paragraph

Everything the OS touches sits behind the six-method `Platform` trait in
`src/platform/mod.rs`. `MockPlatform` implements it for tests; `WindowsPlatform`
implements it for real. Orchestration (`workspace.rs`), role resolution
(`layout.rs`), parsing (`intent.rs`), and policy (`policy.rs`) never call an
OS API directly, so they are portable and testable. If Rust turns out to be the
wrong call for Win32 work, one file changes.

## Config

- `config/layouts.yaml` — logical monitor roles + fractional window rects
- `config/apps.yaml` — what to launch, how to find it, where it goes
- `config/commands.yaml` — phrase → intent
- `config/tools.yaml` — external binaries for voice and capture

No code changes needed to add an app, a layout, or a phrase.

## Read next

- `docs/AUDIT.md` — root-cause analysis of the eight-document spec set
- `docs/FIRST_BOOT.md` — the ordered checklist for your first hour on the laptop
- `docs/INSTALLING.md` — installing it, and updating it without losing anything
- `docs/VOICE_SETUP.md` — installing ffmpeg / whisper.cpp / piper
