# Operating-foundation trace — 2026-10-18

This maps the eight owner-readiness requirements to current implementation and evidence. It is the control list for the next code/test slices.

| Requirement | Current implementation | Existing proof | First missing proof/slice |
|---|---|---|---|
| Starts and stays alive | daemon lifecycle, startup, restart reconciliation | installed daemon start/restart contracts | owner-visible launch and normal-session proof |
| Does not take over machine | doctor/resource diagnosis, working-set sampling, bounded lanes | installed memory/disk diagnosis and daemon samples | measure during a real useful request; fix only if observed |
| Normal request has truthful visible result | CLI remote queue and Hub projections | enqueue, Hub, completion contracts | connect a normal typed/spoken request to the same visible completion path |
| Background work finishes or reports durable failure | durable queue/job state, receipts, replay | start/done/replay/missing-request contracts | failure injection with user-visible recovery explanation |
| Restart preserves work | persisted queue and replay | forced-stop/restart/replay contracts | interrupted in-flight work and uncertainty classification |
| Hub shows working/blocked/failed | health/page/live endpoints and projections | authenticated HTTP shape contracts | page/state matrix with real transitions |
| Safe backup/recovery | backup manifest, restore journal, rollback structures | disposable restore and real-profile backup evidence | normalize live-state restore policy and run owner-safe recovery journey |
| External actions require approval | policy/approval gates and receipts | source tests plus queue approval contract | one installed external-action denial/approval contract |

## First implementation slice

The first source slice is **durable failure visibility after interruption**. It is the narrowest gap that improves the operating loop without adding a new feature: inject an interrupted background request, ensure it becomes an explicit uncertain/failed receipt, expose that state through the existing Hub projection, and prove it with a focused test plus the independent runtime harness.

## Scope guard

This trace does not delete or supersede any roadmap. It selects one vertical slice from the preserved plans; all other requirements remain queued in `PLAN_COVERAGE_REGISTER_2026-10-18.md`.

