# Atlas POA — Architectural Audit & Root-Cause Analysis

> **STALE — a historical argument.** An audit of the eight-document specification set that preceded this codebase. Section 4 argues for deleting the vault; the vault was built instead, deliberately. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

**Audit target:** the eight-document Atlas / Workspace Assistant specification set
**Verdict:** the specification is not the problem. The *process producing it* is.

---

## 0. The finding that subsumes the rest

**Eight documents. Zero lines of running code.**

Read the set in order and the pattern is unmistakable. Every document ends by
proposing the next document:

- `Workspace_Assistant_Implementation_Report` §15 → "I recommend the next
  artifact be: *Starter Repo Blueprint Pack*"
- `Starter_Repo_Blueprint_Pack` §16 → "The most logical next step is for me to
  generate *Phase 1 implementation files*"
- `Atlas_POA_task_and_milestone` → three more planning options
- `Atlas_Personal_Operating_Assistant_True_Master` → a fourth restatement of
  the same baseline

**Root cause:** `Workspace_Assistant_Implementation_Report` §2 records the
actual mechanism — *"This application cannot create downloadable files."*
Every deliverable was therefore forced into copy-paste prose, and prose is the
one artifact type that can always be produced without ever being tested. The
planning loop was not a discipline problem. It was a tooling constraint that
made planning the only reachable output.

**That constraint no longer applies.** This repo compiles and `cargo test`
passes 18 assertions. That single fact is worth more than another 3,000 words
of specification.

---

## 1. No single source of truth

Six documents claim to be the baseline:

| Document | Self-description |
|---|---|
| `Atlas_POA` | "Current Unified Baseline" |
| `Atlas_Personal_Operating_Assistant` | "Official System Definition" |
| `..._Baseline` | "Baseline" |
| `..._Master` | "Master" |
| `..._True_Master` | "Consolidated Updated Baseline" |
| `Atlas_POA_task_and_milestone` | "architectural source of truth" |

A file named `True_Master` is a symptom, not a version. When a developer asks
"which document is authoritative?", there is no answer — which means there is
no specification, only a corpus.

**Fix:** `docs/SPEC.md` in this repo is the only spec. The eight originals move
to `docs/archive/` as history. If something is not in SPEC.md or enforced by a
test, it is not a requirement.

---

## 2. Contradiction: monitor roles vs. hard-coded IDs

- `True_Master` §10.3, stated as a **Core Rule**: *"Use logical display roles,
  not hard-coded Windows monitor numbers."*
- `Starter_Repo_Blueprint_Pack` §11.3, `config/monitor_layout.yaml`:
  ```yaml
  monitors:
    right: { id: 1, role: primary_workspace }
    left:  { id: 2, role: support_workspace }
  ```

The config violates the rule the spec calls core. Windows monitor numbering is
not stable across docking, driver updates, or replugging in a different order.
This would have produced the classic failure: everything works, you undock for
a week, you come back, and Claude opens on the laptop panel while Discord is
placed at coordinates that no longer exist.

**Fixed in this repo.** `config/layouts.yaml` declares roles that claim
monitors *by geometry* at runtime (`primary` / `leftmost` / `rightmost`), with
`fallback_to` chains so an undocked laptop collapses all three roles onto the
built-in panel instead of erroring. Three tests cover this, including
`roles_do_not_depend_on_windows_monitor_numbering`.

---

## 3. Scope: ~25 subsystems for one unfunded person

The spec set commits to: desktop control, browser workflow automation,
authenticated site login, credential vault, payment autofill, finance ingestion,
finance reporting, audit generation, file indexing, document indexing, media
indexing, research pipeline, note generation, scheduler, five separate memory
stores, conversation sessions, proactive assistance, local image analysis,
local image *generation*, local video analysis, local video *generation*, style
learning, webcam presence detection, and future gesture tracking.

Across 13 milestones and 14 phases.

**Root cause:** the spec grew by accretion. Each document added subsystems;
no document ever removed one. There is no section anywhere in 18,000 words
titled "what we are not building."

Local video generation with style learning is a funded research team's roadmap.
Shipping it alongside a personal finance auditor and a credential vault, solo,
means shipping none of them.

**Recommendation:** cut to what earns its keep daily. Workspace control, voice
in/out, file awareness, research-to-note. Everything else goes to
`docs/NOT_BUILDING.md` with a date. Things can graduate back out of that file.
Nothing should sit in a roadmap for two years pretending to be planned work.

---

## 4. 🔴 The credential and payment vault should be deleted, not built

This is the most dangerous item in the document set.

`True_Master` §12.2 and `Baseline` §4 specify that Atlas will store and autofill
**usernames, passwords, PINs, cardholder name, card number, expiry, CVV, and
billing address**, and will navigate authenticated bank sites.

Combine the three properties the spec asks for simultaneously:

1. A homegrown secrets store, written by a solo developer, unaudited.
2. Browser automation that types those secrets into web pages.
3. A **voice-triggered** front end with a wake word and proactive behavior.

The failure modes are not exotic. A phishing page that looks like your bank gets
your real card number typed into it by your own assistant. A site profile with a
stale selector fills your password into a search box that logs queries. A
misheard wake-word activation triggers a form fill you did not ask for. The
approval gate in the spec only fires at *submit* — by then the CVV is already in
the DOM.

Storing a CVV at all is prohibited under PCI-DSS even for merchants. You are not
a merchant, so no auditor will stop you — which is exactly why this needs to be
stopped at design time.

**Correct architecture: Atlas never possesses a secret.**

- Credentials live in Windows Credential Manager (DPAPI, machine+user bound) or
  a real password manager. Atlas holds a *reference*, never a value.
- Atlas navigates to the login page, focuses the field, and stops. You complete
  authentication, with Windows Hello or your manager's own UI.
- Payment autofill: not implemented. Not phase 9, not later. The browser's own
  autofill already does this behind a biometric prompt and is a hardened,
  audited code path. Delegating to it is not a compromise; it is strictly the
  better design.
- Finance: read-only ingestion of CSV and PDF exports you place in a watched
  folder. No bank site automation, ever.

**Separately, and non-negotiably:** Atlas must have no network path to any
machine that executes trades. A voice-triggered, always-on, GUI-automating process
with a large experimental attack surface does not belong on the same VLAN as
anything that can move capital.

---

## 5. Rust is defensible for the core and wrong for the automation layer

The implementation report already contains the counter-argument in its own
Gap 2 and Gap 4: Rust costs materially more work, and you are not writing the
code yourself. The report then recommends Rust anyway.

The honest read: the hardest problem here is Win32 window management, and that
problem is roughly the same difficulty in every language — it is `EnumWindows`,
`GetWindowThreadProcessId`, and `SetWindowPos` no matter what calls them. Rust
does not make it worse. But C# gets `System.Windows.Automation` for free, and
that is the library that actually matters for "focus this app and type into it."

**Kept Rust, since that is your stated decision** — but the entire OS surface is
behind one trait (`src/platform/mod.rs`, six methods). Swapping the
implementation, or shelling out to a C# helper for UI Automation specifically,
means rewriting one file. That option is preserved rather than foreclosed.

---

## 6. Automating the Claude desktop GUI is designed to break

`Workspace_Assistant_Implementation_Report` §5.2 specifies "paste this into
Claude" and "open Claude and enter this prompt" as core V1 commands, and
Phase 7 lists "Claude UI automation."

Driving another application's GUI by simulated keystrokes breaks on every
update to that application — a layout change, a new modal, a slower render.
There is an API for this. Use it, and keep the GUI for humans.

---

## 7. Push-to-talk is bound to Tab

`settings.yaml`: `push_to_talk_key: "Tab"`. Tab is a text-navigation and
field-advance key. Bound globally, it will fire every time you indent code or
move between form fields — which is constantly, in a workspace whose entire
purpose is text.

Use a key with no editing role (F13–F24 if your keyboard exposes them,
otherwise a mouse side button or a chorded modifier).

---

## 8. Not one acceptance criterion was machine-checkable

Every "Exit Criteria" and "Acceptance Criteria" block across all eight documents
is prose a human eyeballs — *"Atlas can place them correctly."* Correctly by
what measure? Checked how? By whom, how often?

**Fixed.** 18 executable tests, all passing, runnable on any machine including
this one — which is not the target laptop. That directly answers the
implementation report's Gap 3 ("you cannot test right now"). You could test
the entire orchestration layer the whole time; it just needed the OS behind a
seam.

---

## Priority matrix

| Priority | Action | Status |
|---|---|---|
| **P0** | Delete the credential/payment vault from the spec; delegate to OS + password manager | Spec change — yours to make |
| **P0** | Network-isolate Atlas from any trade-execution machine | Infrastructure |
| **P0** | Collapse six baselines into one `SPEC.md`; archive the rest | Started in this repo |
| **P1** | Replace hard-coded monitor IDs with runtime role resolution | ✅ Done, 3 tests |
| **P1** | Make workspace_on wait for windows and survive one app failing | ✅ Done, 3 tests |
| **P1** | Make the approval gate executable, defaulting unknown → blocked | ✅ Done, 3 tests |
| **P1** | Validate config cross-references at load, not at 7am | ✅ Done, 2 tests |
| **P2** | Cut scope to daily-use features; open `NOT_BUILDING.md` | Yours to make |
| **P2** | Rebind push-to-talk off Tab | Config change |
| **P2** | Replace Claude GUI automation with the API | Design change |
| **P3** | Verify and fix `src/platform/win.rs` on the target laptop | Blocked on hardware |
