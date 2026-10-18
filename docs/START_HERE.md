# Atlas — current starting point

Use this page for a new session. Older root handoffs and gap registers are
historical records, not current instructions.

## Current truth

- [STATUS.md](STATUS.md) — current state, evidence, next command, and blockers.
- [INDEX.md](INDEX.md) — filing index, open/done ledger, and cold-start map.
- [OPEN_WORK_AUDIT_2026-10-15.md](OPEN_WORK_AUDIT_2026-10-15.md) — complete
  open-work inventory, including owner/device boundaries.
- [STALE_RECONCILIATION_2026-10-15.md](STALE_RECONCILIATION_2026-10-15.md) —
  every current filing warning and its safe treatment.
- [FOUNDATION_IMPLEMENTATION_2026-10-15.md](FOUNDATION_IMPLEMENTATION_2026-10-15.md)
  — source-verified foundation work and test evidence.

## Working rules

Work only in the canonical `atlas/` lane. Replica work is separate and must
not be opened, edited, merged, or cleaned up. Preserve existing uncommitted
changes and ask before making a branch or integration decision that changes
ownership.

## Verification

From the repository root:

```powershell
node .filing\filing.mjs status
node .filing\filing.mjs check --cold
```

The current source aggregate recorded in the filing status is 7,166 passed,
0 failed, and 9 ignored. Focused source checks should be run before changing
the capability state or claiming a gap is closed.

## Historical context

Root files such as `00_START_HERE.md`, `NEW_CHAT_START_HERE.md`, `OPEN_GAPS.md`,
and `OUTSTANDING_*.md` are preserved for audit history. They may contain useful
background, but their paths, dates, and task lists are not current authority.

