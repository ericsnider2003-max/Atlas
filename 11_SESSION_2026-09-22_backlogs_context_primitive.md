# Session — 22 September 2026 (late): dead backlogs, #6 finished, primitive proven

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
Nothing here is installed anywhere and nothing can place a trade. Full suite
after everything: **30 targets, 6,102 passed, 0 failed**, 0 warnings,
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 1. How this ran

A crew of two subagents worked the dead backlogs in isolated source copies
(no cargo in the subagents — two concurrent 250-crate builds would exhaust
the disk, the documented wall), and the main thread was the single serial
compile-and-guard gate. Genuine parallel drafting, serial integration.

## 2. Dead-backlog crew

- **Prose-only tests: 41 → 15.** 26 tests given real behavioral assertions
  (a refusal that also proves it didn't start the process it refused; a
  "top of the list" that proves the item ranked highest; an opaque bank line
  that proves it sorted to Unknown rather than a guessed bucket; and so on).
  The remaining 15 are message-builder constants and source-scan lints where
  the wording *is* the behavior — deliberately left, not forced.
- **Orphan methods: 25 → 20.** Five deleted as dead weight after confirming
  nothing references them: `reference::perishable`, `backends::spec`,
  `perf::current_interval`, `stale::to_stop`, `bars::has_time`. Each left
  both `ORPHAN_METHODS` and (by the coupling the crew found) `KNOWN` in the
  same change. `events::is_window` was NOT cut — an inline test references it.
- **Dead-in-wired config (19) and YAML-keys-with-no-reader (11):** every
  entry classified; all are legitimately blocked (scaffolding for unbuilt
  features, or readers that would be hollow). Recorded-reason comments added;
  nothing force-wired. This is the honest outcome, not a miss.

## 3. #6 finished — contextual recall by meaning

#6 v1 (topic/word overlap, silent bias) already shipped. This adds the
embedding half Eric's ruling anticipated: `search_in_context` now also takes
a **context vector** — the recent turns embedded as one vector — and lifts a
piece whose own vector points the same way, so the conversation can connect
by *meaning* with no shared words. Words and meaning are taken at their max
(two ways of asking one question), still capped at `1 + context_weight`, and
still silent: context re-ranks what the query found, never introduces. It
degrades cleanly — no model, no encoder, or a failure all fall back to the
word-overlap nudge, which falls back to no nudge.

Proven end to end with the real encoder: an apple-orchard conversation lifted
the "Orchard yield" note over "Bond yield" with no shared words. Unit tests
in `tests/contextual_recall.rs` pin both the lift and the invariant (with
semantic off, the context vector changes nothing). The fact book keeps the
topic-overlap path (facts carry no embeddings) — named, not a gap.

## 4. Model-call primitive — already built; now proven

The finding: the "call a model, get text back" primitive is **not missing**.
`brain::Llm` + `ShellLlm` (the external tool-slot, `tool.run(prompt)`) exist
and are wired into the daemon's decide loop, the council (blind + open
rounds), `explain`, `build_it` and `overnight`. The 17 Sep "exists nowhere,
blocks three things" note is stale — this landed in the 18–22 Sep work. No
duplicate was built.

The one real gap the old tests named ("the behaviour needs a running server")
is now closed: `tests/model_call_over_a_real_process.rs` proves the primitive
against a real subprocess — `complete()` carries text from a live process
back through `response_path`; a real consumer (`explain::in_plain_english`)
drafts through it; and a non-zero model process surfaces as an error, not an
empty answer. A stub stands in for the model, so this proves the *seam*, not
a real generative model — `explain`/`build_it` stay "Untested" until a local
LLM runs on real hardware (the same kind of install step as the embedding
model; see `embed/README.md`'s pattern).

## 5. Guard movement (all honest direction)

`ORPHAN_METHODS` 25→20, `TEST_ONLY_MAX` 282→277, `HELPER_UNTESTED_MAX` 5→4
(from earlier), `PROSE_ONLY_BASELINE` 41→15. Nothing grew. Catalogs
regenerated from the constants (`catalogs/`).

## 6. Still open (named)

- `explain` / `build_it` / council: run against a **real local LLM** on the
  Atlas machine — the seam is proven, the model install is the step.
- overnight / sub-agents as fuller consumers, and the Cloudflare-gated
  `delegate_online` — design/asset work, not a missing primitive.
- The standing hardware first-runs: engine `.exe`, `embed.exe`, the model
  installs.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
