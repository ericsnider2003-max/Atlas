# Atlas next-chat handoff — full operational context

**Prepared:** 2026-10-17  
**Purpose:** prevent the next chat from losing the project’s actual state, repeating failed approaches, treating chat claims as proof, or damaging useful work.

## Read this first

The next chat must read these files in this order:

1. AGENTS.md
2. docs/STATUS.md
3. docs/INDEX.md
4. docs/ATLAS_SYSTEM_AUDIT_2026-10-17.md
5. docs/ATLAS_FULL_VALIDATION_PLAN.md
6. docs/ATLAS_VERIFICATION_HARNESS.md
7. docs/OPEN_WORK_AUDIT_2026-10-15.md
8. docs/CAPABILITY_EVIDENCE_AUDIT_2026-10-15.md
9. docs/CAPABILITIES.md

The next chat must treat older handoffs and session records as historical evidence, not current authority. Do not start by reading every historical document. Start with the current files above and only open older records when tracing a specific claim.

## Owner’s governing requirements

Eric’s requirements are:

- Atlas must retain the full intended capability set. Do not remove capabilities to make the project easier to test.
- Atlas must be independently testable. Eric cannot manually exercise the entire product.
- Testing must execute the real Atlas system, not only source code, mocks, rehearsals, handler reachability, or synthetic paths.
- Everything must be tested: code, features, settings, UI, inputs, integrations, devices, recovery, performance, and anything else required for the system to work.
- Every setting must be tested on, off, default, invalid, changed live, persisted, and in relevant combinations.
- Every pass and failure must be logged and retained.
- Every passing capability must be tested again with meaningful variations.
- Failures must remain visible until a later candidate proves the fix.
- Atlas must be tested against one exact candidate version with a recorded build identity and hash.
- Atlas is not ready to be called an operating system until a dependable installed build is proven.
- Claims from Claude and ChatGPT are implementation/history evidence, not owner acceptance and not runtime proof.

## Current repository identity

Canonical repository:

    C:\Users\erics\Atlas\atlas-publish

Canonical source:

    C:\Users\erics\Atlas\atlas-publish\atlas

Remote:

    origin = https://github.com/ericsnider2003-max/Atlas.git

Current local candidate branch and HEAD:

    candidate/2026-10-17-foundation @ bf3569f62a9b087a9745608a3df7a085a7a18757

Canonical source commit:

    32b7f8be

Current filing/evidence commit:

    bf3569f6

Current GitHub origin/main after fetch:

    d3d5bca2e69bb6284ac513d306c736d3f8c125ca

Important consequence: the candidate branch is local and has not been pushed to GitHub. Its source and evidence are committed; remaining work must continue on this candidate branch or a deliberately derived candidate branch.

## Dirty worktree state

The candidate tree currently contains:

- two candidate commits on top of origin/main;
- canonical source, tests, required config, current filing, and the latest verification ledger committed;
- separate untracked historical docs, worktrees, continuation targets, disposable restore data, and older verification runs.

Do not run git reset --hard, git checkout --, broad deletion, or blind cleanup.

Do not commit the entire dirty tree as one undifferentiated change. First classify:

1. canonical Atlas source;
2. canonical Atlas tests;
3. required config/assets/generated files;
4. verification harness;
5. filing/current documentation;
6. historical documents;
7. disposable worktrees/build outputs/scratch artifacts.

Preserve valuable source and tests. Clean only confirmed disposable artifacts through a recoverable, reviewed operation.

## Installed device state

Installed executable:

    C:\Users\erics\AppData\Local\Atlas\atlas.exe

Installed version:

    atlas 0.1.0-dev

Installed SHA-256:

    FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E

Installed file modified:

    2026-10-17 01:46:11 -0700

The installed executable is the current release candidate and was hash-matched after installation. The previous installed binaries are preserved as `atlas.exe.before-candidate-20261017T074730Z` and `atlas.exe.before-candidate-20261017T084611Z`.

At the last check no Atlas process was running.

Current worktree candidate built on 2026-10-17:

    C:\Users\erics\Atlas\atlas-publish\atlas\target\release\atlas.exe

Current candidate SHA-256:

    FA61A885DE97051B38F0A53BB94D84C43FA4F97AB659DE5F6383EF5DB418F71E

This candidate passed the backup-focused integration slice (29 tests) and the installed-path independent runtime slice
in `verification/runs/20261017T084811763Z-1682e9bf/` (identity, doctor command, remote queue enqueue, and separate-
process remote queue replay). It is installed and tested; the minimum operating-foundation journey remains open.

The installed UI was subsequently launched explicitly with `atlas.exe home`, recovered from its local connection page,
and navigated through the live Hub and the long Settings surface. The Hub and Settings pages rendered, but Talk exposed
repeated backup lock failures (`ERROR_LOCK_VIOLATION`) and a mutation-sweep busy message. The backup retry repair is now
installed and the four-contract runtime slice passes again. Live backup was then proven through Talk after restarting Atlas
to clear a stale call-recording object: `data/backups/state-1792227445/` was published with a v2 manifest containing 128
files, 12 directories, and 3 owned roots. This closes the live-backup proof boundary; live restore remains intentionally
unrun because it is destructive to the owner profile. A standalone daemon restart followed by `atlas.exe home` then
reconnected the live Hub successfully; the post-restart Home view rendered `On, listening` and showed the completed
`back up` and `stop recording` requests.

## Current capability evidence

The generated current catalogue is docs/CAPABILITIES.md and is generated from the source capability records. It contains 247 Windows capabilities:

- 58 working;
- 22 switched off;
- 16 waiting on dependencies/providers;
- 150 built but never run for real;
- 1 not built: private mesh.

The exact waiting-on reasons include missing whisper, ffmpeg, piper, Kokoro, multilingual models, picture maker, tesseract, language model, and Cloudflare account/token dependencies.

The catalogue is the current generated evidence view. Older documents report different totals and must not be merged into the current counts.

## What is implemented in source

The current tree contains substantial source implementation for:

- daemon, intent parsing, conversation, thread state, model routing, certainty, follow-up, persona, and capability reporting;
- memory, facts, consolidation, recall, indexing, local knowledge and file safety;
- durable jobs, task loops, evidence/receipts, restart reconciliation, lanes, pause/cancellation boundaries, and Hub projections;
- Hub/server/live status, settings, workspace/project views, navigation, activity, outstanding work, and evidence visibility;
- policy, approval, grants, consent, vault, credentials, recovery, redaction, undo, and external-action safety boundaries;
- coding, build, edit, review, sandbox, selfwork, diagnostics, and project/workshop paths;
- window/app control, panels, overlays, clipboard, screen text, UIA, notifications, audio, voice, camera, vision, hand tracking, presence, and media paths;
- sync, bundles, household/profiles, phone/link paths, providers/connectivity, WireGuard/Tor/peer structures, messaging, updates, and mobile feature boundaries;
- installer/setup, doctor, self-audit, capability catalogues, internal selftest, and generated reports.

This is source presence and source-test evidence. It is not proof that every behavior works on the installed device.

## What is not complete or not proven

The main unresolved areas are:

1. 150 source-backed capabilities have not been run through real runtime contracts.
2. The internal selftest is rehearsal-oriented and cannot be the acceptance system.
3. There is no complete external capability contract inventory.
4. There is no full settings matrix.
5. There is no complete UI surface/state/accessibility inventory.
6. There is no full variation and failure-injection campaign.
7. The candidate-to-installed hash chain now exists for the current release candidate; the broader operating proof is still incomplete.
8. Full-feature and phone-model lanes remain dependency/toolchain gated.
9. Real desktop, voice, provider, account, phone, sync, update, and recovery behavior remains incompletely proven.
10. The private mesh capability is not built.
11. Filing cold-start facts still have gaps and 45 historical stale warnings remain.
12. Current local changes have not been integrated into one reviewed candidate commit.

## Work already done in this session

The following files were added or updated:

- docs/ATLAS_SYSTEM_AUDIT_2026-10-17.md
- docs/ATLAS_FULL_VALIDATION_PLAN.md
- docs/ATLAS_VERIFICATION_HARNESS.md
- verification/Invoke-AtlasVerification.ps1
- filing ledger/index entries for the audit, validation plan, and harness.

The external runner has successfully executed the real installed executable in a disposable ATLAS_HOME for:

- candidate identity via --version;
- machine diagnosis via doctor.

The baseline run was saved under:

    verification/runs/20261017T065119122Z-070056e7/

The latest installed-path run `verification/runs/20261017T200545786Z-500d1a37/` proves the harmless durable-queue slice: four contracts passed against the exact installed candidate hash. A disposable-profile restore using the real 128-file live backup also passed the sampled manifest-backed restore contract; this does not prove the broader feature set, UI, settings matrix, full restore hash set, or owner acceptance.

## Required validation architecture

The full verifier must be independent of Atlas’s internal selftest. It must:

- launch the exact candidate executable;
- use real public interfaces;
- run in disposable but real environments;
- observe visible results and durable state changes;
- inspect filesystem/database/process/network/UI side effects;
- verify forbidden side effects;
- inject failures and restart at durable boundaries;
- capture logs, screenshots, traces, resource measurements, and cleanup;
- append every result to a persistent ledger;
- rerun every pass with meaningful variations;
- compare candidate results against prior candidates.

Required result states:

- passed;
- failed;
- blocked_external;
- inconclusive;
- not_run;
- rehearsed_only.

Do not collapse blocked or rehearsed evidence into passed.

## Required test dimensions

The acceptance campaign must cover:

- code/build/format/clippy/feature flags/packaging/signing;
- every capability and alternate user wording;
- settings default/on/off/invalid/live/persisted/combined;
- every UI screen, panel, control, form, loading/empty/success/error/blocked state;
- keyboard, text, voice, Hub, CLI, IPC, and HTTP entry points;
- first launch, warm state, idle, background work, pause, failure, crash, restart, update, rollback, restore, and corruption;
- offline/online, network loss, provider loss, missing model/tool, permission errors, locked files, disk pressure, memory pressure, and multi-display behavior;
- safety, approval, redaction, destructive actions, undo, partial failure, and duplicate requests;
- alternate phrasing, repeated requests, ambiguity, malformed input, concurrency, and failure injection;
- real desktop/device/provider/account runs where safely available.

## Filing-system rules

The filing system is active:

- AGENTS.md is a generated facts card;
- docs/STATUS.md is owner-written current status;
- docs/INDEX.md is generated index/ledger view;
- .filing/ledger.jsonl is append-only;
- run node .filing/filing.mjs render after status/index-affecting changes;
- run node .filing/filing.mjs check after changes;
- do not hand-edit AGENTS.md, docs/INDEX.md, or the ledger;
- use survey before migration;
- use archive for superseded documents;
- preserve archived bytes;
- use the merge lane before a real merge.

The filing check is currently green with 45 inherited stale warnings. Do not claim a fully clean filing state until those warnings are reconciled or explicitly documented as retained historical material.

## Exact next work order

1. Read this handoff and the current files listed at the top.
2. Freeze new feature additions.
3. Audit the 139 local changes by subsystem and ownership.
4. Run source integrity checks and targeted affected tests.
5. Create a recoverable snapshot of the complete local state.
6. Refactor the candidate boundary so source, tests, required assets, verification, and filing records are distinguishable from artifacts.
7. Integrate only the intended canonical changes into one candidate revision.
8. Build the candidate and produce a manifest plus SHA-256.
9. Install the exact candidate and verify the installed hash.
10. Expand the external runner into capability contracts, settings tests, UI tests, variations, failure injection, and persistent reporting.
11. Run the complete campaign.
12. Fix failures by evidence priority and rerun affected variations.
13. Update the catalogue only from actual evidence.
14. Record what remains blocked, unbuilt, rehearsed-only, or not run.

## Non-negotiable prohibitions

- Do not remove capabilities to make testing easier.
- Do not treat source tests as runtime acceptance.
- Do not treat Atlas selftest rehearsal as real feature proof.
- Do not overwrite old failures with later passes.
- Do not claim the installed build contains local changes without a matching hash.
- Do not merge or delete historical/source material blindly.
- Do not reset or discard the dirty worktree.
- Do not let a new chat invent a new source of truth.
