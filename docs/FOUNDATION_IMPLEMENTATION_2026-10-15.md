# Foundation implementation handoff — 2026-10-15

## Current phase

The foundation verification pass is complete. Implementation begins with F0:
unified capability truth, evidence, availability, resource cost, and fallback
records. The authoritative catalog remains generated from
`atlas/src/capability.rs`; the replica classification ledger is
`replica/ATLAS_CAPABILITY_CLASSIFICATION.csv`.

## Evidence completed before implementation

- 247 catalog rows have unique stable classification IDs.
- 13 capability areas are represented.
- Every row has a primary classification, dependency, rollback, source proof,
  installed proof, owner proof, and status field.
- Serial unit and integration repository gates pass.
- Existing capability honesty, wiring, dead-capability, hollow-code, and
  reachability tests remain part of the canonical integration gate.

## Implementation order

1. F0: typed capability evidence record and one read path for catalog, Hub,
   voice, and status answers.
2. F1: bounded durable jobs/receipts and recovery-compatible state envelope.
3. F2: watchdog, leases, safe mode, restart backoff, and recovery visibility.
4. F3–F5: resource broker, typed job protocol, safe file/knowledge paths.
5. F6–F8: device lanes, sync, and capability-specific adapters.

Each step must preserve declared features, keep offline fallbacks explicit, and
record source, installed, and owner/device evidence separately.

## Owner decisions deferred until needed

Hardware profiles, local model assets, browser-action policy, first provider
adapters, optional online retrieval/export, real voice observations, live
publication sandbox, and multi-device conflict policy remain explicitly
unresolved in the replica execution plan. They do not block the F0/F1 local
spine work.

## F0 progress

Added `CapabilityRecord`, `Availability`, `EvidenceLevel`, and derived
resource-cost/fallback data to `atlas/src/capability.rs`. The record is derived
from the existing catalog entry so there is no second capability description
to drift. Untested and dependency-blocked entries remain explicitly unproven.

Focused F0 tests pass, as do the catalog and dead-capability gates. The
capability-wiring filter was verified separately; the combined Cargo command
was corrected after Cargo rejected multiple positional filters.

The status summary now counts from `CapabilityRecord` availability and
evidence rather than independently reinterpreting raw `State` values. This
keeps status, prompt, and voice truth on the same envelope while retaining the
catalog as the description source.

Follow-up proof: 3 evidence tests, 3 working-status tests, and 60
finishing/ability integration tests pass in the isolated verification target.

## F1 progress

Added `atlas/src/job.rs` and exported it through the crate. The first shared
lifecycle envelope now defines planned, approval, running, checkpointed,
cancelling, cancelled, succeeded, failed, uncertain, and rolled-back states;
it rejects transitions after terminal state and retains checkpoint/provenance
in a receipt. Domain payloads remain outside the envelope.

The module now persists lifecycle envelopes through the existing atomic
`Store` as `jobs.json`, with `load`, `save`, and idempotent `upsert` helpers.
This keeps domain payloads in their owning modules and avoids introducing a
second database or broker. Five focused F1 tests pass, including reload and
duplicate-safe upsert behavior.

The capability-wiring ratchet initially found `job` as an unwired capability.
That was corrected with an explicit shared-infrastructure exemption, alongside
`store` and `error`; the dedicated wiring gate now passes all four tests.

Verification used an isolated task-scoped Cargo target. The
existing target directory was locked by a Windows linker process, so the
verification used `CARGO_TARGET_DIR=target_f1_verify` and did not delete or
overwrite existing build artifacts. The original transition-table defect
(immediate planned failure was rejected) was corrected and retested.

The existing `atlas/src/daemon/tick.rs` diff was inspected and preserved as a
pre-existing user change; it is unrelated to F0/F1 and was not reverted.

The durable lifecycle is now bound to the daemon's background research errand:
the record enters `Running` before worker handoff, reaches `Succeeded` only
after the full write-up is saved, and reaches `Failed` when research or the
write-up save fails. This reuses the existing bounded worker and note-storage
boundaries instead of adding a parallel execution path.

Focused background-research proof passes: the successful and failure-blocking
flows both remain green after the journal integration. A corrected isolated
`cargo check` is the next source-level verification.

Restart reconciliation is now implemented in `job::reconcile_after_restart`
and called from the daemon's first restart pickup pass. Every persisted
nonterminal record becomes `Uncertain` with a restart provenance receipt; the
pass is idempotent and never replays the work automatically. The existing
restart suite remains green: 17 tests passed, alongside 6 focused job tests
and `cargo check`.

Uncertain receipts are now visible in the Hub Outstanding page's blocked lane.
They have explicit tried/stopped/needs text and no dismissal control, so an
owner must inspect the real-world outcome before retrying or clearing them.
The focused Hub integration suite remains green: 11 passed, 1 intentionally
ignored rendering test.

The watchdog audit confirmed the existing crew already provides bounded stop
deadlines, vanished-worker detection, cooperative checkpoints, bounded queue
admission, and nonblocking tick settlement. No duplicate watchdog was added;
the next missing locally provable control is a durable receipt-aware stalled
worker status path beyond the existing in-memory crew report.

F2 progress: durable job records now carry a backward-compatible persisted
`updated_at` timestamp, a heartbeat/checkpoint operation that does not change
the lifecycle state, and stale inspection semantics for future status/recovery
surfaces. This keeps the existing crew watchdog authoritative for live worker
control while giving restart/status code durable progress evidence. Seven job
unit tests and `cargo check` pass.

Stale inspection is now visible in the Hub Outstanding blocked lane with a
conservative 15-minute threshold. Stale jobs are review-only: Atlas does not
cancel, dismiss, or retry them automatically. The Hub suite remains green
(11 passed, 1 intentionally ignored).

F3 admission audit: the existing `fit` planner and `crew` scheduler already
form the active local resource boundary. `fit` selects models from measured
memory/cores/GPU and installed assets; `crew` enforces slots, whole-machine
exclusion, memory floor, battery policy, waiting-worker limits, and bounded
queue growth. No second broker was added. Pressure proof is green: 26 model
fit tests and 13 crew admission/watchdog tests passed.

The model supervisor audit also confirmed model startup is already governed by
the fit plan and helper ownership, with restart backoff, device-loss safe mode,
loopback-only model endpoints, and rule/text fallback when local model work is
not admitted. Three model-without-curl tests pass. The zero-test result from
the broader process-test filter was recorded as a filter mismatch, not claimed
as coverage.

F4 progress: `job::Record::apply_outcome` now translates the existing
`taskloop::Outcome` vocabulary through the durable state gate: Started becomes
Running, NeedsYou becomes AwaitingApproval, and Done/Failed create terminal
receipts. A focused test caught and corrected the legal direct
AwaitingApproval-to-Succeeded transition. Eight job tests and `cargo check`
pass; cancellation and terminal reuse remain protected.

The F4 adapter is now used by both live multi-step execution paths. A
task-loop creates a Store-backed `taskloop-*` record before its worker starts,
and the daemon finalizes that same record from the real `Run::verdict` for
finished, blocked, background-started, out-of-step, and stopped runs. The
existing worker, approval, and stream state remain authoritative; the journal
is an evidence/receipt layer rather than a second scheduler. The first
verification exposed only an adapter-type mismatch (the repository's loader
is infallible and its Store writes return `AtlasError`); that was corrected
without changing behavior. The eight focused job tests pass and the full
source check passes after the correction.

The dead-capability ratchet caught that `job::heartbeat` was initially only
test-reached. It is now called at each live task-loop progress event, so the
durable record carries real progress evidence while the existing task-loop
checkpoint remains authoritative. The same pass exposed missing direct tests
for the new `capability::records` projection and `job::State::terminal`
contract; focused tests were added. No dead-code exemption was added.

F4 locally provable gate: capability wiring 4/4, dead-capability ratchet 5/5,
Hub integration 59/59, capability reachability 3/3, and reliability/task-loop
coverage 68/68. Isolated `cargo check` and `git diff --check` pass.

The serial library unit gate also passes: 1,272 passed, 0 failed, 14 ignored,
in 276.91 seconds. The ignored cases are explicitly hardware, endpoint,
internet, or coordinated-environment proofs; none failed. The filing cold
check remains green with 45 inherited historical stale-reference warnings,
which are documented baseline debt rather than a new failure from F4.

REA-derived evidence/snapshot slice: added `atlas/src/evidence.rs` with bounded
immutable bundles, SHA-256 identity/input/output hashes, static/runtime/
installed/owner-device/derived observation kinds, validation, tamper-rejecting
import/export, snapshot comparison, job linking, and a 512-entry Store-backed
journal. Task-loop finalization now persists and verifies a bundle tied to its
durable job. Evidence tests pass 6/6, the dead-capability ratchet passes 5/5,
task-loop reliability remains 60/60, `cargo check --lib` passes, and diff
whitespace is clean. The production path keeps the evidence layer bounded and
does not add a scheduler or external dependency. Cancellation now produces a
distinct durable terminal receipt (`Cancelled`) instead of being recorded as a
generic failure; task-loop stop handling uses that transition.

Snapshot reuse is now read-only and strict: the newest valid bundle is reused
only when subject, source revision, and input hash all match. Malformed or
mismatched records are ignored. The reuse contract has a dedicated test;
evidence tests now pass 6/6 and the dead-capability ratchet remains 5/5.

## Next verification

The approval/cancellation receipt visibility gate is locally green: job
terminal receipts are distinct, uncertain/stale jobs remain visible in Hub,
and completed jobs stay out of Outstanding. The F5 safety slice is also green:
file recovery 13/13, file finding 7/7, and actual filing 8/8. The remaining
F5 boundary is owner-selected real-folder inspection; synthetic and repository
fixtures are passing without claiming proof about an unselected user folder.

F7 sync/handoff validation is now complete. Continue with the locally provable
adapter-specific media, market, research, and browser checks, while preserving
the owner-only boundary for real hardware and folders.

The adapter pass is green for the locally provable lanes: photo editing 26/26,
video studio 5/5, video understanding 4/4, market desk 2/2, market-feed spike
analysis 4/4, research wiring 6/6, browser fallback 11/11, research off the
tick 6/6, browser HTTP 16/16, offline/CDP 26/26, self-improvement routing
12/12, coding 12/12 plus model selection 27/27, and trading-number ownership
6/6 in its dedicated isolated Cargo target. Total adapter pass: 163/163. No generated media or trade
execution authority is inferred from these tests; they prove local analysis,
bounded transformations, provenance, approval gates, and honest degradation.

The complete explicit Cargo-target verification then passed 8,915 tests across
44 target groups, with 0 failures and 26 intentionally ignored long-running
measurements (`EXIT_CODE=0`). This includes the consolidated `all` target,
dedicated vault/global-state/security targets, and the measured VAD/voice lanes.

The first F8 vertical slice is also green: calendar boundary/behavior 16/16,
publication routing and policy 66/66, delivery-edit receipts 33/33, media
posting 12/12, mail provider parsing 3/3, and finance consent/field guards
50/50, for 180/180 focused tests. These prove approval invalidation, queue
survival, offline holding, provider-specific limits, receipt honesty, and the
no-money-movement boundary; live provider and owner-device proof remain separate.

Hub job receipts now use the same local-first durability boundary: the daemon
loads a bounded `hub_jobs` snapshot, persists start/finish/seen transitions,
and converts a job found running after restart into an explicit retryable
failure. This avoids a second scheduler and preserves the existing page job
IDs/UI behavior. The restart regression is 3/3, with Hub UI/window coverage
20/20.

The generic restart path was also hardened: persistence of unfinished errands,
window jobs, and consumed waiting-state records now goes through the kept/error
reporting path instead of discarding Store failures. Verification is green for
silent-discard detection 3/3, restart recovery 17/17, system recovery 58/58,
and handoff coverage 5/5.

A follow-up scan found one more restart-loss edge in the same path: clearing
consumed unfinished work after pickup also discarded Store errors. That write
now uses the kept/error reporting path; no replay policy changed. The focused
no-silent-discard and restart suites remain green at 3/3 and 17/17.

The feedback/outbox boundary was hardened next: inbox, sent, reply-outbox,
and delivery-removal saves now either report persistence failure or return a
retryable result instead of claiming that a message was received, answered,
or delivered when its durable record was not saved. Feedback coverage is 5/5
and the Hub error-surface coverage is 18/18.

The same audit hardened four user-visible result stores in the daemon: sent
tray entries, proposed workshop changes, researched knowledge, and derived
facts now use kept/error reporting on persistence failure. The operation can
still return its computed result, but Atlas no longer hides that the durable
record failed. No-silent-discard remains 3/3 and research wiring 6/6.

F6 verification has now passed voice measured 4/7 executed (3 long-running
measurement cases remain intentionally ignored), VAD 1/1, and phone-standalone
15/15. The feature-gated `phone-llm` lane remains externally blocked in this
environment because `llama-cpp-sys-2` requires `libclang.dll` for bindgen and
no LLVM/Clang installation was found in the checked locations; no source
failure was inferred from that build prerequisite.

The first full serial aggregate run exposed seven concrete issues. The two new
foundation modules are now explicitly classified as plumbing, the pinned tree
count is 495, the generated capability document is synchronized, research job
receipt writes use a bounded worker-safe retry, and the off-tick backup tests
now exercise the intentional 120-second startup deferral. Verification of the
previous failures is green: catalogue 27/27, silent-discard checks 3/3,
off-the-tick 10/10, research-off-the-tick 6/6, and durable job tests 10/10.
The final serial aggregate suite is green: 7,136 passed, 0 failed, and 9
ignored in 979.96 seconds.

The post-scan aggregate remains green after the complete production
silent-discard cleanup: 7,136 passed, 0 failed, and 9 ignored in 992.29
seconds. The next local gate is worker/resource cleanup verification; the
owner-selected real-folder inspection remains intentionally outside automated
proof because no folder has been selected.

Cleanup ratchets after the status correction are green: dead configuration
6/6, dead methods 5/5, hollow-code analysis 20/20, and registered wiring
18/18. The strict `capability_wiring.rs` guard is now registered in
`tests/all.rs`; its four tests pass, so capability reachability is part of the
canonical aggregate rather than an unrun side file.

The expanded canonical aggregate was then rerun after that registration:
7,140 passed, 0 failed, and 9 ignored in 1,104.49 seconds (serialized).
This is the current full-suite proof for the foundation and capability-wiring
ratchet.

The next full run exposed one timing-sensitive off-the-tick backup observation
after 7,139 passes. The backup passed in isolation; the test's five-second
observation window had too little margin under serialized Windows load. The
test-only window was widened to 15 seconds with that boundary documented, and
the definitive aggregate then passed 7,140/7,140 with 9 ignored in 1,095.19
seconds.

The follow-up compiler hygiene pass removed two new panic assertions from the
foundation. Evidence tests pass 13/13 and durable-job tests pass 16/16;
`cargo check --all-targets` remains green. Repository-wide strict Clippy is
not yet a valid clean gate: it reports 33 pre-existing findings across legacy
examples, tests, and market/server code outside this foundation slice. That
baseline is filed rather than masked or mixed into unrelated cleanup.

The capability-path audit then closed a real consumer gap: live Hub status now
renders the same evidence-backed capability summary used by voice and
capability answers, instead of relying only on machine and crew facts.
Capability honesty passed 9/9, routine behavior 22/22, and the strict wiring
guard 4/4 after the change.

The authoritative warm-cache aggregate was rerun after the final persistence
logging and capability-summary fixes: 7,140 passed, 0 failed, and 9 ignored in
1,138.81 seconds (serialized, `target_after_all_foundation_gates`). The
capability-specific follow-up then passed 35/35, and `cargo check
--all-targets` passed. The filing system records the exact command and result;
the only remaining foundation boundary is owner/device or externally selected
folder verification.

The next canonical aggregate exposed a Windows serialized-load timing issue in
the schedule durability tests: 7,139 passed, 1 failed, and 9 ignored. The
failure was not a product correctness failure; the worker had not settled
before a five-second observation deadline. The failing schedule was reproduced
in isolation, and the matching restart-receipt observation was widened to the
same 15-second test-only margin. The definitive post-fix aggregate then passed
7,140, failed 0, ignored 9 in 1,437.95 seconds (`target_after_latest_capability_projection`).
Both schedule tests passed independently after the change, `cargo check
--all-targets` passed, and `git diff --check` passed with only the known CRLF
normalization warning for `atlas/docs/CAPABILITIES.md`.

The definitive canonical aggregate after the live research-aware shakedown
projection, regression coverage, generated-target cleanup, and all F0–F8
source gates passed 7,141 tests with 0 failures and 9 ignored in 1,468.29
seconds, serialized through `target_after_final_foundation_gate`. The extra
pass is the direct enabled-research commissioning regression. The ignored
tests remain real hardware/model/network observations and are not promoted to
installed or owner/device evidence.

The post-aggregate durability hardening added a 1,024-record bound to the
generic job journal, preserves all active jobs, and trims only the oldest
terminal receipts. Checked reads now protect upsert, restart reconciliation,
task-loop finalization/heartbeat, and Hub outstanding-job reporting from
malformed state being treated as an empty journal. The full serialized
aggregate was rerun afterward: 7,141 passed, 0 failed, and 9 ignored in
1,410.64 seconds. The read-only classification validator also passes all 247
replica ledger rows against the generated catalog; it does not promote
source-only, installed, or owner/device evidence.

The scheduler durability follow-up applies the same invariant at the public
`ScheduleRuns::save` boundary: claim history is capped at 1,024, active claims
are never evicted, and the oldest terminal receipts are pruned deterministically.
Exact-capacity histories may rotate through that boundary. The loaded add-on
suite passed 19/19, the dedicated schedule-bound library proof passed, and the
current serialized aggregate remains 7,141 passed, 0 failed, and 9 ignored in
1,416.74 seconds.

The feature-boundary audit also fixed the lean build's ONNX-only NPU test
reference and gated its desktop-only setup test. `cargo check
--no-default-features --lib` and strict lean library Clippy both pass; the
default all-target check passes. A complete lean all-target test build still
needs broader existing test-module gates for desktop-only `eframe`, `setupwin`,
and `look_paint` imports, which remains test-harness cleanup rather than a
production library failure. Full-feature Clippy requires the owner/toolchain's
`libclang.dll` for `llama-cpp-sys` and is recorded as externally unavailable.
The follow-up lean all-target verification closed the earlier test-harness
note: both `cargo check --no-default-features --all-targets` and
`cargo test --no-default-features --all-targets --no-run` pass, with the
desktop/ONNX-only test modules excluded by their explicit feature boundaries.

## Post-gate persistence hardening

After the canonical aggregate, the local persistence audit continued without
changing the F0–F8 contract. Evidence, plugin decisions/offers, feedback
journals, paired-device journals, and the mail/reply outbox now use checked
bounded reads at their persistence boundaries. Corrupt or oversized state is
refused or treated as an empty safe view according to each existing API, and
the source bytes are preserved. The focused regressions pass: evidence 7/7,
plugins 30/30, feedback integration 26/26 (one intentionally ignored), phone
unit coverage 6/6, and outbox 20/20. Lean all-target compilation and lean
library Clippy with warnings denied also pass. The latest canonical aggregate
before the outbox-only follow-up remains 7,141 passed, 0 failed, 9 ignored;
the outbox follow-up has its own 20/20 proof and does not alter the aggregate
contract.

The subsequent local-knowledge pass extended the bounds to the sharded facts
store (8 MiB per shard and legacy file, 2 MiB for aliases), memory (8 MiB),
semantic vectors (64 MiB), notebook (16 MiB), conversations (16 MiB), account
book (8 MiB), backlog (8 MiB), and provider-routing outcomes (4 MiB). The
canonical serialized aggregate was rerun afterward: 7,141 passed, 0 failed,
and 9 ignored in 1,009.09 seconds. Lean all-target compilation, strict lean
library Clippy, formatting, and diff checks also pass. The ignored cases remain
explicit hardware/model/network observations rather than installed-device
claims.
