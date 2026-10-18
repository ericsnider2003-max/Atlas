# REA and replica adaptation notes — 2026-10-16

## Scope

This is a clean-room research pass against the current Atlas source and the
public REA projects. It records transferable behavior and architecture, not
source code, branding, private APIs, or proprietary assets.

## Public patterns reviewed

- [REA / Reverse Engineer Anything](https://github.com/zleo-code/REA): an
  agent-facing investigation loop that separates decompilation, understanding,
  and reconstruction and returns evidence plus limitations.
- [rea-dev-tools](https://github.com/aliyenidede/rea): durable project-local
  memory, explicit plans, review agents, and branch/workflow rules so an agent
  can resume without relying on conversation history.
- [Relay-agent](https://github.com/miter37/Relay-agent): durable tasks composed
  into explicit project DAGs with named artifact inputs and outputs, human/CLI
  parity, and safe delivery from an isolated working copy.
- [drover](https://github.com/sylvester-francis/drover): rerunnable
  plan/act/observe loops, model-call governance, and an offline fake-model path
  that exercises durability without network credentials.
- [Aegis agent](https://github.com/timwhitez/aegis-agent): local provider
  adapters, durable sessions, compaction, live steering, and explicit safety
  boundaries.

## Atlas mapping

| Research pattern | Atlas foundation | Remaining proof or work |
| --- | --- | --- |
| Evidence and limitations travel with every conclusion | `evidence.rs`, typed capability records, receipts | Add domain-specific claim writers; the shared job envelope now carries bounded artifact/dependency edges. |
| Plans and memory survive a session boundary | `.filing/`, `docs/STATUS.md`, `job.rs`, Hub persistence | Research, task-loop, local coding, kept-media, and self-audit inspections now persist bounded input/output edges; later domain-specific producers can reuse the same envelope. |
| Tasks form inspectable artifact edges | `artifact.rs`, durable jobs, and existing receipt paths | Thread domain artifact IDs through individual multi-step producers; the common bounded envelope is now persisted on job records and receipts. |
| Investigation, implementation, and verification are separate roles | sandbox/fix-loop foundations and capability evidence | Enforce the boundary in self-improvement: inspect -> propose -> sandbox -> verify -> approve -> apply -> rollback. |
| Offline fake/local providers exercise the full loop | offline-first paths and provider-aware model routing | **Proven locally:** deterministic provider fixtures now cover success, failure, cancellation, checkpoint/reload, restart reconciliation, and rejection of late completion; no network is required for recovery proof. |
| Human/CLI/UI paths share one execution history | Hub, voice, status, and shared capability projection | Hub outstanding now surfaces bounded artifact-edge counts for stale/uncertain durable jobs; voice/CLI continue to consume the shared receipt state. |

## Findings that change implementation priority

1. The next durable-job improvement should be an artifact/evidence envelope,
   not another capability-specific queue. This reduces duplication across
   coding, media generation, system diagnostics, and market-analysis review.
2. Atlas should keep reverse-engineering outputs explicitly observational. A
   finding must state what was observed, how it was observed, what remains
   uncertain, and which local implementation decision it supports.
3. Local-provider fixtures are a high-value speed and reliability test. They
   let the system prove cancellation, restart, routing, and receipts without
   spending network time or credentials.
4. The current 247-row capability catalog must not be treated as reachability
   proof. The existing evidence states are the right direction; the next gap is
   user-shaped route evidence and installed-device measurements.

## Decision

Preserve the current typed capability/evidence and durable-job foundation.
The shared bounded artifact/evidence envelope is now integrated with durable
job records and receipts, and the deterministic local-provider acceptance
matrix proves the offline recovery path end to end. The next source-only slice
is domain-specific claim writers and explicit enforcement of the
inspect -> propose -> sandbox -> verify -> approve -> apply -> rollback
self-improvement boundary before adding more parallel agent roles. This keeps
offline behavior, resumability, auditability, and latency improvements aligned
across the requested Atlas capability families.

