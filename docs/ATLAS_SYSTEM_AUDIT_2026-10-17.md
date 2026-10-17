# Atlas system audit — implementation versus requested system

**Date:** 2026-10-17  
**Scope:** current canonical repository, current generated capability catalogue, current filing records, GitHub origin/main, installed Windows executable, and historical Claude/ChatGPT handoff evidence.

## Authority and version reality

The only reliable implementation authority is the current Git repository at:

    C:\Users\erics\Atlas\atlas-publish\atlas

Historical handoffs and chat records are requirements and evidence claims. They are not proof that the current source, GitHub, or installed device contains the claimed work.

Current comparison:

- local HEAD: d3d5bca2e6;
- GitHub origin/main: d3d5bca2e6;
- local worktree: 139 tracked files changed plus untracked additions;
- current local changes: not in GitHub;
- installed device: Atlas 0.1.0-dev, SHA-256 B5D6D239A7C7FB4E6F18EFD0BF2400F7BDA71D45133BC064D68C6C06295182BA;
- installed device build: not identified as the current local worktree and not evidence of the current local changes.

The project therefore does not currently have one singular version containing all work claimed by the chats.

## Current capability evidence

The generated catalogue contains 247 Windows capability records. Its current source-backed state distribution is:

| State | Count | Meaning |
|---|---:|---|
| working | 58 | Source and existing evidence say the behavior is dependable enough to expose as working; real full-path proof still needs the independent harness where applicable. |
| switched off | 22 | Implemented or tested enough to exist, but intentionally disabled. |
| dependency/provider waiting | 16 | The code path exists but requires a model, account, token, speech engine, ffmpeg, or other external dependency. |
| built, never run for real | 150 | Code and source tests exist, but the behavior has not been proven on real hardware or through a real installed runtime. |
| not built yet | 1 | The capability is declared but has no reachable implementation. |

The counts are generated from the current catalogue. Earlier audit documents report different totals, so those older numbers are stale evidence and must not be merged into the current status.

## What is implemented in the current system

### Core assistant and conversation

The source contains the daemon, intent parser, conversation/thread state, model routing, certainty/uncertainty handling, answering, follow-up, persona, tone, references, and capability reporting.

Existing evidence is strongest for source routing and policy behavior. Real voice, model, and day-to-day conversation quality remain runtime evidence gaps.

### Local-first memory and knowledge

The source contains facts, memory, consolidation, recall, indexing, local knowledge/file reading, freshness, references, and bounded state/store paths.

The current foundation work has source tests for these paths. Real owner-folder behavior, long-running indexing, recovery, and actual usefulness still require runtime contracts.

### Durable work and recovery

The source contains durable jobs, task loops, receipts/evidence, restart reconciliation, background lanes, pause/cancellation boundaries, Hub projections, and failure reporting.

This is the strongest recent implementation area. It is still not installed-runtime proof until the harness starts the real executable, interrupts work, restarts it, and checks the durable result.

### Hub, settings, and visibility

The source contains the Hub/server path, live status, working/full lists, settings, workspace/project views, navigation, outstanding work, activity, and evidence projections.

Source and synthetic tests exist. Page-by-page installed UI, accessibility, responsiveness, and truthful display of real runtime state are not fully proven.

### Safety, approval, and protected state

The source contains policy gates, grants, consent, vault/credential handling, recovery codes, redaction, approval boundaries, safe/rehearsal modes, undo/recovery structures, and external-action fences.

The project has strong source-level safety work. The missing proof is that real actions, real failures, and real recovery leave the promised evidence without silent side effects.

### Coding and project work

The source contains coding/build/edit/review/sandbox/selfwork paths, project/workshop concepts, explanations, diagnostics, and agent handoff boundaries.

This is present in code, but most of the user-visible workflow is marked built-but-never-run. It needs real disposable-project contracts that verify files, compiler output, diffs, approvals, and rollback.

### Media, desktop, and device interaction

The source contains window/app control, panels, overlays, clipboard, screen text, UIA, notifications, voice, audio, camera, vision, hand tracking, presence, media rendering, video, images, and 3-D paths.

These are precisely the capabilities that cannot be closed by source tests. They need the external harness plus controlled desktop fixtures and real device runs.

### Sync, phone, network, and peers

The source contains sync/bundles, household and profiles, phone/link paths, provider and connectivity records, WireGuard/Tor/peer concepts, messaging, updates, and mobile feature boundaries.

Synthetic wiring and protocol tests exist. Real two-device behavior, mobile suspension/push, live networks/providers, and friend installation remain unproven. The private mesh capability is explicitly not built.

### Installer, update, and self-reporting

The source contains first-launch/setup, install roots, update/release structures, doctor, capability catalogues, self-audit, selftest, regression records, and generated capability output.

The crucial missing part is that selftest is safe rehearsal: it intentionally avoids real background work, real file mutation, network/account effects, and actual UI actions. It cannot be the acceptance system requested by Eric.

## What is not currently in the system or not yet proven

### Missing implementation

- Private direct mesh capability is declared but not built.
- Some council/model/provider paths remain incomplete or dependency-gated.
- Several capabilities are intentionally switched off.
- Full-feature and phone-model build lanes remain blocked by external toolchains or dependencies.

### Implemented but not runtime-proven

The 150 built-but-never-run records are the largest gap. They include substantial parts of:

- voice/hearing/endpointing and speech;
- desktop windows, apps, overlays, notifications, UIA, and clipboard;
- background startup, crash recovery, resource health, scheduling, and overnight work;
- coding/project workflows;
- media/camera/vision/hand/presence;
- sync, phone, network, peer, and update paths;
- installer/setup/update behavior;
- self-test, variation, and capability-reporting behavior itself.

“Built” in these rows means source implementation and source-level evidence, not that Atlas has been demonstrated to work in the installed system.

### Missing verification infrastructure

The system still needs:

1. An independent runner outside Atlas.
2. One immutable candidate manifest and executable hash.
3. A contract for every capability or capability family.
4. Real public-interface drivers: CLI, IPC/HTTP, Hub, UI automation, voice/device drivers where applicable.
5. Observable-effect assertions, including forbidden side effects.
6. Failure injection and restart/recovery tests.
7. Variation generation after a pass.
8. An append-only result ledger retaining every pass and failure.
9. Regression comparison between candidate versions.
10. Reports that distinguish passed, failed, blocked external, inconclusive, rehearsed-only, and not-run.

The first real-runtime runner exists only for candidate identity and machine diagnosis. It is not yet the full system requested.

### Missing singular-version discipline

Claude and ChatGPT produced several generations, branches, worktrees, bundles, installed builds, and handoff documents. The current repository has a large uncommitted local change set, while GitHub and the installed device are on different states.

Until the local source is classified, integrated, built, hashed, and installed as one candidate, claims from different chats cannot be attributed to the same Atlas.

## What must be in the system to satisfy the original request

The requested system is not merely a list of capabilities. It must include four layers:

### 1. Full capability implementation

Keep the entire intended capability set, including offline-first behavior, memory/knowledge, conversation, voice, desktop control, coding/project help, media, safety, local state, sync/devices, phone behavior, network/peer paths, proactive/background work, setup/update, and self-improvement boundaries.

Do not remove a capability because it is difficult to verify. Mark it honestly and keep it in the catalogue.

### 2. Runtime truth system

Every capability needs a real contract that can execute the behavior and inspect its result. Source tests remain necessary but are not sufficient.

### 3. Evidence and regression memory

Every result must remain queryable by candidate, capability, variation, environment, and failure. A later pass must never erase the earlier failure.

### 4. Singular release/candidate discipline

One candidate must have:

- one source revision;
- one build configuration;
- one executable/package;
- one manifest and hash;
- one installed identity;
- one verification report;
- one list of known failures and external blockers.

## Recommended order of work

1. Reconcile the current source and uncommitted work into one candidate without deleting capability work.
2. Build the external verification harness and append-only ledger.
3. Generate capability contracts from the current catalogue and existing intent/module metadata.
4. Run the real candidate through baseline, feature, variation, failure, restart, and recovery campaigns.
5. Fix failures by evidence priority, not by adding more untested features.
6. Promote a capability from built-but-never-run only when the runtime contract passes.
7. Keep external/device/provider blockers visible and separate.

## Audit conclusion

Atlas is not empty and the prior chats did build a substantial system. The honest conclusion is narrower and more useful:

> The implementation layer is large; the singular-version layer and real-runtime verification layer are incomplete. The next required system is the machinery that proves the implementation, preserves every failure, and ties every claim to one exact Atlas build.
