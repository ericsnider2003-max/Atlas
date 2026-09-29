# Reconciliation with the current main-chat baseline — 12-13 Sep 2026

`ATLAS_RECONCILED_20260913.zip` — the complete tree, not a delta. Your last
upload turned out to be genuinely newer than what I'd been working against
in two directions at once, so this round was a real merge, not a rebuild.

## What the new baseline had that I didn't

- **A real fix to `Intent::ReviewPost`** — it had been sharing a match arm
  with `Research`/`DraftPost`/`Capture`, so a post whose own wording
  happened to contain "it", "this" or "that" got silently hijacked into a
  clipboard/selection question instead of ever being reviewed.
- **`digest.rs`** — the hardcoded three-letter account whitelist (`C`/`D`/`E`)
  is gone; account keys are validated the same structural way as
  instrument and timeframe now, not against one business's fixed list.
- **`opsec.rs`** — the movement-detection phrase list split into
  unambiguous and context-dependent tiers.
- **`prose.rs`** — a grammar-check rule that was silently converting every
  correct "there is" to "there are" is gone.
- **A whole new `market/multiframe.rs` module**, with its own CLI and tests.

All of it is intact in the reconciled tree — none of my work touched these
files, so there was nothing to merge there, just preserve.

## What I had that the baseline didn't

Confirmed precisely: **the pairing round is already merged into the main
chat's tree** (case-insensitive `kin.rs`, the `raw_argument` parser fix,
`Pair`/`AcceptPairing`/`ForgetPeer`). Everything after that — the three
small bug fixes, the business hub (`roster.rs`/`shared_task.rs`), the
hardware-detection fix, `wants`/`Intent::Recommend`, `household`,
`firstrun`, and `identity` — was not yet in what you sent, so all of it is
reapplied here, file by file, around the baseline's own changes rather than
overwriting them.

## Real bugs this reconciliation pass found — none from the merge itself

- **`kin::same_name` needed `pub(crate)` again.** The already-merged pairing
  round predates `roster.rs`, which needs the same comparison `roster.rs`
  wasn't there yet to require.
- **The shipped `config/tools.yaml` had `backup.enabled: false`.** A test
  already asserted it should be `true` ("backups should be on by default —
  nothing else protects this") and was failing quietly. Fixed — and it's
  what makes last round's household/restore protection actually mean
  something in practice.
- **Removing `household`/`firstrun`/`identity`/`wants` from
  `UNWIRED_BASELINE` (module-level) surfaced a second, separate ratchet:**
  `tests/capability_wiring.rs` has its own `CAPABILITY_UNWIRED` list, and
  it still named all four. Updated to match.
- **That same removal exposed real, previously-hidden dead capabilities**
  inside `household.rs`/`identity.rs` — modules skipped entirely by the
  dead-capability sweep while they were still on the unwired list.
  `identity::grace_remaining` was one of them: genuinely built, never
  called. Wired it into a new `atlas household proof` command ("proven for
  another N minutes" / "not currently proven"). Fixing that one was enough
  to bring the count back under the ceiling — no ceiling bump needed.
- **A debug-format leak in my own household code**, caught only because I
  finally ran `hub_is_not_code.rs` against it: `"Already set up as {:?}."`
  on a plain `String`. Fixed to `{}`. This is exactly the class of bug this
  guard exists for — I just hadn't run this specific guard against this
  code before.

## Named, not fixed

- **`household::meets`, `new_pairing`, `saw_another`, `share_with_friend`
  are still genuinely unwired.** These are the "pair a second device of
  your own to this household" flow — a real, separate feature I never
  built. I only wired the identity/config layer
  (`Household::load`/`save`/`is_set`) and the cross-household safety check
  in `restore()`. The dead-capability count has room for these right now,
  but they're real, named, un-fixed work, not swept under anything.

## Verified

- `cargo check --lib --bins` and `cargo check --tests` — the whole
  reconciled tree, clean.
- **142 tests, 0 failures** across every guard file plus the broader
  regression set this pass: `daemon` (18), `bug_sweep` (14, all 4 of its
  own sub-checks including the two ceilings), `integration` (23),
  `brain_and_wake` (33), `connections_wired` (4), `declared` (4),
  `capability_wiring` (4), `hub_is_not_code` (10), `guards` (23),
  `retrospective` (5), `wiring` (4).
- Plus every round's own tests re-confirmed clean in this tree:
  `machine_detection` (7), `roster` (10), `shared_task` (12),
  `server_safety` (31), `kin` (11), `firewall` (19), `pairing` (25),
  `pairing_wiring` (21).

Everything from here is real, current, and merged with what the main chat
actually has — this is the tree to keep building on.
