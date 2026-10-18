# Canonical Atlas work filing bundle — 2026-10-15

## Scope and protection

This bundle covers the canonical `atlas-publish/atlas` lane only. Replica work
is excluded, its ledger item is dropped, and lesson `Lfd0b` records that the
replica belongs to another chat. No reset, checkout, broad delete, rebase, or
overwrite was used against the shared dirty tree.

## Verified work filed today

- F0 capability evidence records are shared by catalog, voice/status, and Hub
  consumers; research state is honored and area totals use the same projection.
- F1 durable jobs persist lifecycle envelopes, bounded records, checkpoints,
  receipts, restart uncertainty, and safe recovery visibility.
- Task-loop, routine-question, workflow-restart, phone handoff, and other
  persistence paths report save failures without changing safe retry/defer or
  no-replay behavior.
- Hub capability status exposes the shared evidence-backed summary.
- Scheduler history is bounded while preserving active claims and newest
  terminal receipts; aggregate-loaded observation is stabilized by terminal
  evidence and bounded settlement waits.
- Unchecked durable readers were removed so production cannot bypass checked
  recovery boundaries.
- The daemon `reached` state now reports save failures through the persistence
  failure ledger.

## Verification evidence

- Definitive serialized integration aggregate: **7,141 passed, 0 failed, 9
  ignored**, `cargo test --test all -- --test-threads=1`.
- Focused durable-job/add-on/foundation aggregate: **61/61 passed**.
- Journal-bound focused gate: **42 passed, 0 failed**.
- Add-on focused gate: **19 passed**.
- Capability validator: **247 rows, unique IDs validated**.
- All-target Cargo checks: passed in the concurrent canonical verification
  pass.
- Filing cold check: exit 0, green, with 45 inherited historical stale
  warnings. Strict filing is not claimed green.

## Still in progress / not claimed

- Current `cargo clippy --no-default-features --all-targets -- -D warnings`
  and related no-default-feature checks are running; no result is claimed yet.
- Real-device, owner-device, provider, and external-dependency measurements
  remain unexercised in this workspace.
- The shared tree remains dirty with concurrent changes and generated target
  directories; those are preserved and not independently attributed here.

## Next handoff

Run `node .filing/filing.mjs status` and `node .filing/filing.mjs check --cold`
before the next edit. Then collect the active clippy/check results, file them,
and continue the remaining capability/persistence audit.
