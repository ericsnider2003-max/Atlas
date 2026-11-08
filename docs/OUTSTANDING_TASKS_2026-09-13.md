# Atlas (personal/business) — outstanding tasks, 13 September 2026

> **STALE — superseded.** A dated task list. Last true on 13 September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Companion to `HANDOVER_2026-09-13.md`. Organized by who unblocks it, since
that determines what happens next — not severity.

---

## 1. Needs a ruling from Eric

- **`household::share_with_friend` — what should it actually do?** The
  function exists (build a one-way `Handoff`: a note or file sent to a
  friend's Atlas, explicitly not carrying your household's identity with
  it) but has no real caller, and unlike device-pairing there's no obvious
  transport to reuse — `kin.rs`'s peer channel is the closest fit, but
  sending *content* rather than a signal/nudge is a different shape of
  message than anything that channel currently carries. Needs a decision
  on what the first real use of this looks like before it's worth wiring
  blind.
- **`returning.rs`'s deeper integration.** `welcome()` (the
  Straight/Offer/Nothing decision, tiered by how long you were away) and
  `full_brief()` (the answer to saying yes to an offered brief) are real,
  tested, and unused. The existing away-briefing path in `daemon.rs`
  predates `returning.rs` and works off a pre-formatted string from
  `Journal::brief()`, not the structured `Happened` list these two
  functions need. Wiring them properly means restructuring that path —
  real, valuable work, but a bigger and riskier change than anything
  attempted blind this session. Worth doing; needs a green light given the
  existing path is live and tested today.
- **Which of the ~30 remaining `UNWIRED_BASELINE` modules to tackle next,
  and roughly in what order.** Several fall into shapes that need a
  decision before building rather than more wiring effort:
  - `confirmed.rs` / `consent.rs` — both need genuine live, multi-turn
    daemon state (a pending security-confirmation or a live-recording
    state), not a one-line call. `confirmed.rs` additionally needs a real
    browser-automation backend to actually *make* the security-page change
    it confirms — without one, wiring the confirm/read-back protocol alone
    would let someone say "yes" to nothing happening.
  - `quickinput.rs` — the pure state machine (open/close/type/submit) is
    wireable today; the actual OS-level global hotkey registration
    (`parse_hotkey`'s target) needs real Win32 work this sandbox can't
    verify. Worth asking whether a partial wiring (console-surface only,
    no hotkey) is wanted, or whether to hold for the hotkey work.
  - `android.rs` / `ios.rs` / `companion.rs` — mobile-app scope entirely;
    needs a decision on whether a real app shell is in scope for this
    chat or belongs elsewhere.
  - `gguf.rs` / `models.rs` — a real local-LLM-serving layer (registry,
    launch a server, completion endpoints), a genuinely large integration
    on top of the existing external-command `ShellLlm` path. Worth
    scoping as its own project rather than folding into a wiring pass.
  - `budget.rs` / `overnight.rs` — hosted-model spend control and batch
    overnight work; both need a real hosted-model integration to attach
    to, which doesn't exist in this tree yet either.
- **Cross-instance business-hub linking.** Scoped in an earlier session
  (a small, named, deliberately-populated shared space; a `never_in`-style
  scope guard keeping Atlas off a business's real project folder while
  leaving the deliberate shared space readable) but explicitly held back
  from being built until Eric says when. Still not built.
- **The personal/business firewall's actual trigger.** `firewall.rs`
  itself is complete and tested. Nothing in the product yet *attempts* a
  personal-to-business crossing outside the manual CLI simulation
  (`atlas shared check`) — `roster.rs`/`shared_task.rs` gave it a real
  first door for tasks, but the broader question of what else should
  cross it (shared calendar? client list? research tagged to a business?)
  is still open.

## 2. Real work, no ruling needed — just the next wiring pass

- The remaining ~30 `UNWIRED_BASELINE` modules not covered above:
  `mend`, `look`, `adapt`, `afterme`, `backends`, `cloudsync`, `dictate`,
  `edit`, `editors`, `gguf`, `grade`, `hearing`, `language`, `models`,
  `plainly`, `publishing`, `reach`, `remote`, `voiceover`, `walkthrough`,
  `workingset`, and others — see `docs/MODULE_REFERENCE_2026-09-13.md`'s
  quick index for the complete, current list (search for `**no**`). Each
  is a real, tested, standalone capability; each needs its own integration
  decision, not a guess, per the pattern that's held throughout this
  session.
- `household::meets`/`new_pairing`/`saw_another` are wired now, but the
  device-pairing flow has only been exercised by unit tests and the CLI
  path — never against two genuinely separate physical devices. First real
  run should be exactly that.
- `atlas reclaim` (from an earlier session) has never run against a real
  filesystem either — carried over as a standing caution, not new.

## 3. Blocked on real hardware, unverifiable in this sandbox

- **Voice-lock is unproven on real audio.** The policy layer
  (`identity.rs`'s neighbor concept — actual speaker verification, a
  separate earlier-session feature) is fully tested; no speaker encoder
  has ever processed a real recording.
- **`identity.rs`'s `Hello` status is always reported `Unavailable`.** No
  Windows Hello detection exists anywhere in this tree; building and
  testing one needs real Windows hardware.
- **`firstrun.rs`'s microphone enumeration is unimplemented.**
  `audio::parse_devices` is correct and tested against fixture text; no OS
  command anywhere actually calls it to produce real device names.
- **macOS is unverified.** Windows and Linux cross-compiles are checked
  routinely; there has never been a macOS toolchain available to this
  sandbox to confirm the same.
- **The Windows DPAPI credential path** (an earlier session's work)
  compiles but has never run on real Windows hardware.

## 4. Standing cautions carried forward, not new this session

- The `DEAD_CAPABILITY_CEILING` ratchet (currently 288) will keep moving
  in both directions as more `UNWIRED_BASELINE` modules get wired — see
  `HANDOVER_2026-09-13.md`'s explanation of the mechanism. This is
  expected, not a regression, as long as every move is written down with
  a real reason, which `tests/bug_sweep.rs`'s own comment history (and
  this session's additions to it) already models.
- This sandbox's disk is the binding constraint on getting real
  `cargo test` numbers for the full tree; the personal-island tool is the
  reliable workaround, not a full substitute — anything not yet ported
  into the island only has a compile-check behind it until it is.
