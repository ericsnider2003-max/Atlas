# Work Items 1–7 status — 2026-10-18

This record refreshes `OPEN_WORK_AUDIT_2026-10-15.md` against the current
canonical candidate. It is intentionally a status and execution boundary
record; it does not merge, commit, delete, archive, install toolchains, or
alter owner/device state.

## Current evidence

- Canonical branch: `candidate/2026-10-17-foundation`.
- Current filing check: green with 59 warnings, up from the obsolete 45-warning
  count in the earlier audit.
- Current working tree includes the installed-candidate evidence and many
  untracked verification/worktree artifacts. These must be classified before
  any integration decision.
- Current status says the installed candidate has passed the latest 14/14
  substantive runtime contracts, but owner acceptance of the operating system
  foundation remains open.

## Items 1–7

1. **Stale filing warnings — active, not closed.** The cold check is green but
   still reports 59 warnings across historical documents and a few recently
   edited records. Safe next action: classify current-vs-historical records in
   small batches, then update only the current entry points. Do not bulk-delete
   or rewrite history.

2. **Foundation integration — blocked on owner-selected integration.** The
   candidate is materially more current than the 2026-10-15 audit, but the
   workspace still contains untracked worktrees and verification outputs. Safe
   next action: produce a file-by-file integration inventory and ask which
   candidate/worktree is authoritative before staging or committing.

3. **Untested capability triage — needs a fresh generated-catalog read.** The
   old 247-row counts are not sufficient evidence for the current candidate.
   Recompute state counts from the current generated catalogue, then group
   rows by shared source path and proof boundary. Do not open 150 independent
   feature tickets.

4. **Source-only gaps — deferred behind the foundation gate.** Configuration
   repair reporting, clock/DST jumps, disk-full behavior, changelog capture,
   file watching, media control, and mesh remain candidates. They should only
   be implemented after the owner-readiness journey confirms the foundation is
   dependable and each gap gets a small acceptance test.

5. **Council pipeline — source work exists, production round-trip remains
   owner/provider dependent.** Keep the current opinion aggregation and prompt
   construction. Do not claim council complete until model calls, dissent,
   recovery, and the user-visible result are exercised through the real
   provider/room policy.

6. **Full-feature build lane — blocked by missing `libclang.dll`.** No LLVM
   installation was found in the checked locations and the prior audit records
   `llama-cpp-sys` as the failing dependency. Installing or selecting a
   toolchain changes the machine and requires an owner decision. Keep default,
   lean, and full-feature proof separately labeled.

7. **CI/release automation — reviewable, not yet closed.** The repository has
   Android, iOS, Windows, and general CI workflows. Review should verify that
   generated capability records, version numbers, artifacts, and device/secret
   gates remain truthful. Do not run signing or release actions without the
   required credentials and owner approval.

## Recommended immediate sequence

1. Owner selects the singular integration target and confirms whether the
   untracked verification/worktree artifacts are disposable evidence or work
   to preserve.
2. Re-run the generated capability state count from that target.
3. Classify the 59 filing warnings in a small reversible documentation batch.
4. Run the installed-candidate verification and owner-readiness journey.
5. Only then promote source-only gaps or full-feature toolchain work.

