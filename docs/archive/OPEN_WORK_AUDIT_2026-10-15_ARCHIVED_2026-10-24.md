# Atlas open-work audit — 2026-10-15

Last updated: 2026-10-20

## Scope and authority

This is a read-only inventory of work that can be continued in the canonical
`atlas-publish/atlas` lane. Replica work is explicitly excluded and was not
opened, edited, merged, or cleaned up.

Current authority is the filing status and current source, not older backlog
documents. The filing check is green; it reports 45 inherited historical
`STALE` warnings. The repository has no current filing-ledger items marked
open, but the source, runtime, external-boundary, and documentation inventories
below still contain work.

Evidence used: `docs/STATUS.md`, `docs/INDEX.md`, current Rust source and
tests, Cargo/workflow configuration, git/worktree state, and the filing cold
check. The latest canonical aggregate recorded in the filing status is 7,166 passed,
0 failed, and 9 ignored; the focused synthetic sync/handoff/provider/phone-link
and no-silent-discard suites recorded 142 passed.

## Workable in the repository now

These items can be advanced without a physical phone, live provider account,
release key, or owner decision. They are ordered by leverage.

1. **Reconcile the 45 stale filing warnings.** Review each historical status,
   handoff, gap, and roadmap document; mark it historical, replace it with a
   current statement, or archive it only after the owner confirms the intended
   record. Do not bulk-delete or silently rewrite history.

2. **Integrate the current uncommitted foundation work.** The canonical lane
   has 76 uncommitted items, including source, tests, generated capability
   records, filing records, and continuation targets. Review the diff by
   subsystem, identify the intended source of truth, then commit or deliberately
   park the work. Existing branches and worktrees also require an owner-selected
   integration decision; they must not be cleaned up automatically.

3. **Finish source-level capability verification for the remaining untested
   areas.** The capability catalogue contains 247 entries: 58 Working, 150
   Untested, 22 Off, 16 Blocked, and 1 Planned. Convert the untested entries
   into grouped source tests or explicit external-boundary records, rather than
   treating every catalogue row as a separate feature ticket.

4. **Implement the remaining source-only product gaps, after a current-source
   audit.** Good candidates are configuration validation and repair reporting,
   clock/DST/jump and disk-full behavior, changelog/postmortem capture, page or
   file change watching, media-control plumbing, and the still-planned private
   network mesh. Each needs a small acceptance test and a capability-state
   update.

5. **Complete the council pipeline.** `council` is Blocked in the current
   catalogue. The source has opinion aggregation and prompt construction, but
   the remaining work is the production round-trip: build/open a round, make
   model calls, collect responses, handle dissent, and return a user-visible
   result. The spoken entry point and custom-room policy need an owner decision
   before implementation is final.

6. **Resolve the full-feature build lane.** Default and lean library/all-target
   checks are green. Full-feature validation remains unavailable because
   `llama-cpp-sys` needs `libclang.dll`; the phone-LLM lane also needs its
   external toolchain/device. Add or document a supported LLVM/Clang setup and
   keep the phone lane separately gated.

7. **Review CI and release automation for source/record drift.** The Android,
   iOS, and Windows workflows contain deliberate owner/secrets/device gates.
   The repository work is to keep those gates truthful, ensure artifacts and
   version numbers are recorded, and add checks for configuration and generated
   capability documentation where missing.

8. **Turn the historical roadmap into a current decision backlog.** Re-audit
   the old phase items against the catalogue and current modules, keeping only
   items that still represent a desired behavior. Candidates include email and
   social adapters, media workflows, adaptive awareness, update courier hardening,
   and richer cross-device orchestration; many are presently external-boundary
   or owner-decision work rather than unstarted code.

## Open, but blocked on owner/device/external state

These can be prepared in source, tests, and runbooks, but cannot be honestly
closed by repository work alone:

- Run the installed Windows executable and doctor/self-test against real
  owner-selected folders and real desktop UI, browser, UIA, toast, clipboard,
  firewall, and signing behavior.
- Install and test Android/iOS builds on real phones, including background
  suspension, notifications, screen readers, device sizes, phone-link behavior,
  and on-device model performance.
- Exercise live mail, calendar, social, cloud/delegation, Cloudflare, Tor, and
  other provider paths with the owner’s accounts, tokens, network, and policy.
- Supply release/signing credentials and run the real restart/update path,
  including Azure/Apple/TestFlight decisions and signed hold notices.
- Resolve owner choices for delegation defaults, online intent, spoken unlock,
  council rooms, enrollment, device/platform priorities, and friend installs.

## Capability states requiring attention

The one Planned capability is `mesh`.

Blocked capabilities are `wake`, `endpoint`, `dictate`, `accents`, `translate`,
`speak`, `kokoro`, `delegate_online`, `vad`, `pronounce`, `reason`, `ocr`,
`addressing`, `council`, `imagemake`, and `voiceover`.

Off capabilities are `cutin`, `system`, `tune`, `loginseal`, `telegram`, `mail`,
`unsub`, `strategy`, `selfwork`, `goingaway`, `cliphist`, `nudge`, `anticipate`,
`reference`, `finance`, `budget`, `booking`, `opsec`, `identity`, `phone`,
`presence`, and `hunt`.

The 150 Untested entries are not automatically implementation gaps: most have
source/fixture coverage but lack real runtime or external-boundary evidence.
They should be handled through the grouped work in the first section.

## Historical records, not current open-item authority

These remain useful for context but are stale or explicitly historical and
must not be used as an unfiltered task list:

- `atlas/docs/OPEN_ITEMS.md`
- `atlas/docs/WHERE_IT_STANDS.md`
- `atlas/docs/ROADMAP.md`
- `OPEN_GAPS.md`
- `OUTSTANDING_2026-09-26.md` and older merge/session handoffs

One example of superseded history is the old claim that `Contents` was never
constructed: current `atlas/src/contents.rs`, `atlas/src/daemon.rs`, upkeep,
and execution paths now implement that flow and the focused tests cover it.

## Recommended next work order

1. Reconcile stale filing records and select the intended integration target
   for the 76 uncommitted canonical changes.
2. Run a focused source audit of the 150 Untested capabilities and promote only
   genuine repo-workable gaps into small issues with acceptance tests.
3. Close the full-feature toolchain/documentation gap and finish council/mesh
   source work where owner decisions are already settled.
4. Hand the external-boundary checklist to the owner for device/provider runs.

