# Coding from a description, auto-delegation, and a clear-out — 20 Sep 2026

Fourth pass of the day, continuing from
`04_SESSION_2026-09-20_grants_cloudflare_and_a_clearout.md`. Eric's asks:

1. Don't force "do it online" — when online, Atlas checks its own capability
   and delegates to a sub-agent automatically to free itself up. Create the
   intent for it.
2. Increase Atlas's in-house coding: code from a description, "like how I tell
   you what I want and you build it", better and more accurate than now, and
   Atlas fact-checks it for correctness. Delegable in-house and out-of-house.
3. Keep working DEAD_IN_WIRED; remove duplicate functions that don't lose
   capability; keep the code catalogue current.
4. Complete any ruled-on-but-unwired capability.

All built, tested, verified. State table at the end.

MEASUREMENTS. NO VERDICT. ERIC RULES. · OFFLINE-FIRST · THE COMPILER IS THE JUDGE

---

## 1. Coding from a description — `build_it`, fact-checked

The pieces already existed and were tested — `craft` (the real toolchain
ladder: `cargo fmt`/`check`/`clippy`/`test`), `sandbox` (isolate + run),
`selfwork` (land) — but two seams were missing: **nothing turned a description
into code**, and **`Sandbox::create` had no production caller**. This session
built both.

New `src/build_it.rs` owns the fallible half, all pure and unit-tested against
a `MockLlm` with no toolchain:
- `generate(description, lang, llm)` — a first draft, code pulled out of
  whatever the model wraps it in (`extract_code` takes the largest fenced
  block, falls back to the bare reply, keeps an unclosed block).
- `fix_draft(code, failure_output, llm)` — one fix round from the tool's
  verbatim complaint.
- `build_loop(...)` — the whole generate → check → fix loop, with the
  **checking injected** so the contract is testable without cargo. A draft is
  only `Built` when the checker says so; when the fix budget runs out the best
  draft is handed over as `Struggled` **with the failure attached, never as if
  it worked**.

The daemon (`build_from_description`) owns the real check: it writes the draft
into a throwaway `Sandbox` (scaffolding a tiny crate for Rust, a file for
Python) and runs `craft`'s ladder against it — **the compiler is the ground
truth, not the model's confidence**. Failures drive the next fix round; the
verified code (or best draft) is written to a builds directory. New
`Intent::Build` ("build me a script that…", "write a function that…"), routed
through all the intent classifiers.

Why this is *more accurate* than "just ask the model": the model's word is
never trusted — the local compiler's is. A draft from a bigger model online is
still a draft until `cargo check` agrees here.

New capability `build_it`, honest state **Untested** (its parts are tested; the
generate→compile round has never run end-to-end with a live model). 16 unit
tests + 3 daemon tests.

## 2. Auto-delegation — no forced "do it online"

Eric: "when it is online I want Atlas to check its capability and if it can be
delegated to a sub-agent then it does, to free up Atlas." Realized concretely
on the two heavy background capabilities — **build** and **research** — through
`cloudflare_worker()`, which returns a worker only when the machine is online
AND the provider is set up AND the token is in the vault. When it returns one,
the drafting/summarising is delegated to it; when it returns `None`, the local
model does the work unchanged. Either way the job runs as a **crew errand**, so
Atlas itself is freed while the sub-agent works, and the result is **checked
locally** before it reaches you (the compiler for build, `certainty` +
`verify_result` for research). You never say "online" — Atlas decides from its
own capability and connectivity.

The general `dispatch_task` primitive stays the tested seam for delegating an
arbitrary task; a general "do XYZ" intent that routes *any* task this way is
the larger next step, but build and research are the concrete, high-value
realizations now.

## 3. Duplicates removed — capability kept, code cut

- **`endpoint::Turn`** — a dead duplicate of the live `timing::Turn` (per-stage
  turn timing). `timing::Turn` is what the daemon actually builds and reads;
  `endpoint::Turn` was constructed only in a test. Deleted (struct + impl +
  `feels_quick`/`total_ms`/`worst`), the test moved to `timing`'s coverage.
  Deleting it honestly exposed that `timing::total_ms`/`worst` had only *looked*
  reachable through the bare-name collision with the duplicate — now correctly
  listed as test-only.
- **`panel::worth_interrupting_for`** — an orphan with a body identical to
  `panel::transient`. Deleted; no capability lost.

`endpoint::saved_against_fixed` was checked and **kept** — it is a live method
on the real `Endpointer`, not part of the dead `Turn`.

## 4. DEAD_IN_WIRED — checked, and honestly none wireable now

Every remaining `DEAD_IN_WIRED` entry was re-checked against its live
consumers. **None are cleanly wireable today** — each needs a
feature/driver that does not exist yet: `grading`/`editcraft` want a
video/scope measurement feed (their `check`/`too_many_effects` have no
production caller), `clipboard::reply_to_clipboard` wants a platform
clipboard-*write*, `cloudsync::check_every_hours` wants a periodic sync
verifier, `companion::queue_while_offline` wants the phone client,
`consult`/`draft::max_passes`/`enrol::password`/`overnight::attempts_each` want
their loops/drivers, `panel::waking_secs` wants the panel renderer,
`workingset::always_carry_current` wants a devices-meet event. Wiring any of
them now would leave the setting parsed-but-inert — the exact defect the guard
exists to prevent. Recorded rather than faked.

## 5. Ruling completion

Eric's ruling — Atlas codes from a description and **fact-checks it for
correctness** — is completed by `build_it`: the fact-check *is* running the
real compiler and tests. `selfwork::run_tests` (running the suite in a sandbox
for **self**-modification) stays blocked, correctly: its surrounding loop still
has no candidate-generation step, so wiring `run_tests` alone would be a
discarded-result check. `build_it`'s proven `generate`/`fix_draft` machinery is
now the natural seam to complete that self-improvement loop in a focused future
pass — deliberately not rushed at session's end, because Atlas editing its own
tree is exactly where care matters most.

## 6. Verification

- personal Atlas: **0 warnings**, **430 lib**, **315/315 integration files,
  5,196 integration tests, 0 failed** (two new test files). The 2 env-gated
  hearing tests stay green against the real binaries.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- Baselines: `TEST_ONLY_METHODS`/`ORPHAN_METHODS` reconciled for the two
  deletions and the timing exposure; `KNOWN` updated (build_it fully wired,
  timing totals added, two duplicates removed); `MODULES_IN_TREE` 283 → 284
  (build_it); `UNCLAIMED_MAX` held (build_it claimed by the `build_it`
  capability). `CAPABILITIES.md` regenerated.
- **Manifest retaken: 605 files.** As always, this needs Eric's agreement
  before it stands; this file is the record of what changed.

The full plain `cargo test` still doesn't fit this container's disk (argon2
bus-errors at the linker when bundled); the suite ran in bounded batches of
six, swept between — same tests, same totals. One batch hit a transient
compile bus-error under disk pressure (`which_model_fits_here`), which passes
individually; the clean run is 0 failed.

## 7. Open for Eric

- **A general "do XYZ" delegation intent** would route *any* delegable task to
  a sub-agent automatically, not just build and research. The machinery
  (`dispatch_task` + `verify_result`) is built and tested; it waits on that
  intent and on deciding which task kinds are safe to auto-delegate.
- **The self-improvement loop** (`selfwork` generating its own candidate fixes,
  verified by `run_tests` in a sandbox) is now one generation-seam away, with
  `build_it` as the pattern to follow — a focused pass of its own.
- Cloudflare Workers for whole compute jobs (beyond Workers AI inference and
  Browser Rendering) remains the next provider shape; the `online` seam is
  provider-agnostic and ready.
