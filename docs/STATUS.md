# Status

Last updated: 2026-10-17 (singular candidate committed, rebuilt, installed, and runtime-verified)
Owner: (owner)

Current: Canonical Atlas foundation work is active on `atlas/`; F0 evidence paths, F1 durable jobs, restart reconciliation, Hub artifact visibility for stale/uncertain work, bounded artifact/dependency envelopes, research/task-loop/local-coding/kept-media/self-audit receipt edges, and F5 local-knowledge/file safety paths are source-verified. The fresh serialized default aggregate passed 7,175, failed 0, and ignored 9 across 7,184 tests; the post-fix serialized lean aggregate passed 7,021, failed 0, and ignored 8 across the no-default feature boundary. The feature-boundary regressions found in the prior lean run were repaired and are included in these clean runs. F7 sync and handoff validation is complete. Default and lean all-target compile gates, plus strict library Clippy gates, are green. The candidate's `.cargo/config.toml` now makes one test worker the normal verification contract because process-global clock/settings and real-tool tests are not safe to overlap; the five regressions observed under the unsafely parallel invocation all pass under the canonical command.

Owner readiness correction: Eric is not satisfied that this work has produced a dependable operating system after more than a month. The current project must be treated as **not ready to call an operating system** until the installed laptop build demonstrates the minimum operating foundation in `docs/OWNER_READINESS_RESET_2026-10-16.md`. Green source tests and design artifacts are supporting evidence, not acceptance. Feature expansion is paused while the foundation is installed, exercised, recovered from failure, and accepted by the owner.

## In progress

<!-- One line per workstream: what, where (its folder), state, and the exact next step or command. The AGENTS.md card shows these lines; run `node .filing/filing.mjs render` after editing. -->

- Singular candidate — branch `candidate/2026-10-17-foundation`, source revision `32b7f8be` — canonical Atlas source/tests/config are committed; current filing, handoff, and verification evidence are separate docs-only commits on the same branch. The release candidate was rebuilt from that source revision and installed at `C:\Users\erics\AppData\Local\Atlas\atlas.exe`; candidate and installed SHA-256 are both `FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E`. The latest real-process run is `verification/runs/20261017T224847069Z-d381b129/` with 14/14 substantive contracts passed: identity, resource diagnosis requiring memory and disk observations, enqueue, authenticated Hub health/page/live surfaces, daemon cold/restart liveness, post-daemon replay, queue replay, approval-gated start, completion, completed-state replay, and missing-request failure. Daemon working-set samples were recorded in the run ledger. Parallel worktrees and disposable artifacts are not part of the candidate.

- Operating foundation reset — `docs/OWNER_READINESS_RESET_2026-10-16.md` — owner-raised readiness correction; feature expansion paused; candidate installation, live Hub/Settings navigation, backup-lock diagnosis/repair, installed-path runtime proof, one real-profile backup, daemon/UI reconnection after restart, and a disposable-profile restore campaign are complete. The live backup `data/backups/state-1792227445/` has a v2 manifest with 128 files, 12 directories, and 3 owned roots; 125/128 direct hashes matched after restore, with two startup-mutated state files and one intentionally preserved trash ledger recorded. Remaining acceptance work is policy normalization for live-state restore evidence, resource/dependency readiness, the rest of the minimum operating-foundation journey, and owner acceptance.

Next verification command: `cargo +stable-x86_64-pc-windows-msvc check --locked --manifest-path atlas/Cargo.toml --features phone-llm` after LLVM/Clang provides `clang.dll` or `libclang.dll`; then independently hash all 128 files restored by the singular installed candidate. Installed runtime, live hardware, and provider readiness remain separate evidence boundaries.

## Decisions

<!-- Owner decisions that explain the layout or "why not X". Dated, one line each. -->
