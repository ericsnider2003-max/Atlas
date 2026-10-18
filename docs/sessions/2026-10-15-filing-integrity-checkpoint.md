# Filing integrity checkpoint — 2026-10-15

## Scope

Canonical `atlas-publish/atlas` only. Replica work remains excluded and is
recorded as dropped in the filing ledger (`i90e9`), with lesson `Lfd0b`.

## Verified

- `node .filing/filing.mjs status` reports the canonical F0–F8 state and the
  next capability-path silent-discard scan.
- `node .filing/filing.mjs check --cold` exits 0: filing is green with 45
  inherited historical `STALE` warnings; strict mode is not claimed green.
- `.filing/ledger.jsonl` exists and contains the durable-job proof, restart
  recovery proof, replica-scope correction, and later F7/F8 notes.
- `docs/INDEX.md` indexes the canonical F0/F1 proof documents and the session
  records.
- `AGENTS.md` is generated from the current filing state; it was regenerated
  after the current status update.
- No reset, checkout, broad delete, or overwrite was used. The dirty working
  tree contains concurrent changes and generated verification targets; those
  remain preserved.

## Handoff rule

Before any further functional edit, rerun `node .filing/filing.mjs status` and
`node .filing/filing.mjs check --cold`. After each meaningful change, append a
ledger item/note and render the generated filing cards.
