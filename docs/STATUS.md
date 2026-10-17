# Status

Last updated: 2026-10-16 (default and lean full aggregates verified)
Owner: (owner)

Current: Canonical Atlas foundation work is active on `atlas/`; F0 evidence paths, F1 durable jobs, restart reconciliation, Hub artifact visibility for stale/uncertain work, bounded artifact/dependency envelopes, research/task-loop/local-coding/kept-media/self-audit receipt edges, and F5 local-knowledge/file safety paths are source-verified. The fresh serialized default aggregate passed 7,175, failed 0, and ignored 9 across 7,184 tests; the post-fix serialized lean aggregate passed 7,021, failed 0, and ignored 8 across the no-default feature boundary. The feature-boundary regressions found in the prior lean run were repaired and are included in these clean runs. F7 sync and handoff validation is complete. Default and lean all-target compile gates, plus strict library Clippy gates, are green.

Owner readiness correction: Eric is not satisfied that this work has produced a dependable operating system after more than a month. The current project must be treated as **not ready to call an operating system** until the installed laptop build demonstrates the minimum operating foundation in `docs/OWNER_READINESS_RESET_2026-10-16.md`. Green source tests and design artifacts are supporting evidence, not acceptance. Feature expansion is paused while the foundation is installed, exercised, recovered from failure, and accepted by the owner.

## In progress

<!-- One line per workstream: what, where (its folder), state, and the exact next step or command. The AGENTS.md card shows these lines; run `node .filing/filing.mjs render` after editing. -->

- Canonical foundation — `atlas/` — F0–F8 adapter proof, Store-backed Hub receipts, restart persistence reporting, feedback/outbox honesty, result-store error reporting, the registered capability-wiring guard, F5 local-knowledge/file safety, and synthetic device/sync/provider gates are green; live summary, full-list, and working-list consumers now share the typed research-aware evidence projection. Lean/mobile test modules are feature-gated and compile cleanly. The release candidate was rebuilt with bounded Windows backup-lock retries, installed, and hash-matched at `C:\Users\erics\AppData\Local\Atlas\atlas.exe` (SHA-256 `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`, version `0.1.0-dev`); the backup-focused slice passed 29 tests and the installed candidate passed the four-contract runtime harness, while owner-selected real-folder, device, provider, and full operating-foundation observations remain separate boundaries.

- Operating foundation reset — `docs/OWNER_READINESS_RESET_2026-10-16.md` — owner-raised readiness correction; feature expansion paused; candidate installation, live Hub/Settings navigation, backup-lock diagnosis/repair, installed-path runtime proof, one real-profile backup, and daemon/UI reconnection after restart are complete. The live backup `data/backups/state-1792227445/` has a v2 manifest with 128 files, 12 directories, and 3 owned roots. Remaining acceptance work is live restore rehearsal in a disposable profile, resource/dependency readiness, and the rest of the minimum operating-foundation journey.

Next verification command: `cargo +stable-x86_64-pc-windows-msvc check --locked --manifest-path atlas/Cargo.toml --features phone-llm` after LLVM/Clang provides `clang.dll` or `libclang.dll`; installed runtime, live hardware, and provider readiness remain separate evidence boundaries.

## Decisions

<!-- Owner decisions that explain the layout or "why not X". Dated, one line each. -->
