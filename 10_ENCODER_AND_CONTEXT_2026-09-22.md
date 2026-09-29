# The model shipped, and learning #6 v1 — 22 September 2026

Follows `09_MEANING_SEARCH_2026-09-22.md`. Two things: the "waiting on the
model" gap is closed — the encoder is in this archive and the model is one
hash-pinned command away, both verified together end to end — and contextual
recall v1 is built to Eric's two rulings.

## 1. The encoder (`embed/`) — nothing is waiting on a model any more

"Waiting on the model" was never a hard blocker; it was three undone things:
the weights on disk, a wrapper program that runs them, and a Windows build.
All three are now done:

- **`embed/`** — a standalone crate: text on stdin, 384 numbers on stdout.
  One dependency (tract-onnx, pure Rust, the same major version personal
  Atlas already builds with) plus BERT's WordPiece tokenizer written out in
  ~120 tested lines. Model: all-MiniLM-L6-v2 (86MB), the exact model `fit.rs` already plans
  for — too big for the chat delivery cap, so `embed/models/GET_THE_MODEL.md`
  carries a one-command, hash-pinned download of the exact file this bundle
  was verified against. Mean-pooled, L2-normalised — the
  sentence-transformers recipe, so the cosines mean what the model was
  trained for.
- **Verified**: 4 tokenizer tests; semantic sanity on real vectors (related
  0.27, unrelated ~0.0, unit norm); ~0.26s per invocation including model
  load; and **end to end through the real daemon** — the real model, via the
  real tick path, found the orchard note from "what do I know about apples",
  the same zero-shared-words test the fake encoder runs.
- **Windows**: `embed/dist/embed.exe`, statically linked (no DLLs), run under
  wine with output byte-identical to the Linux build. Honest limit: wine is
  not a real Windows box — the first run on Eric's machine is the last
  unverified step, and it is one command printing 384 numbers.
- Install steps are in `embed/README.md`; the shipped `tools.yaml` example
  matches its paths exactly. Copy two folders, uncomment one block, flip
  `recall.semantic: true`, run `atlas doctor`.

## 2. Contextual recall v1 (learning #6), per Eric's rulings

Eric decided 22 Sep: context from **topic/word overlap now** (embeddings
strengthen it later through the same seam), and **silent bias only** — no
volunteering.

Built as one rule applied in both recall paths: **context re-ranks what a
question found; it never finds.** Words from the last three turns
(`recall::context_terms_from`, minus the question's own words) reach
`Library::search_in_context` as a capped multiplier on pieces the query
already matched (`1 + context_weight·s/(s+0.25)`, default weight 0.25 — sized
to clear `clarity`'s 0.9 tie band, far under the gap a genuinely better match
makes), and `facts::Book::recall_in_context` as a +1/word, max +3 bonus keyed
only to facts already scored. A zero base times any nudge is zero, so a note
cannot surface because you mentioned its topic an hour ago.

Wired at all three daemon recall sites: `facts_answer`, `WhatIHave`, and
`from_notes`. `tests/contextual_recall.rs` (4 tests, through the daemon where
it matters): a question two notes answer equally is settled by one turn of
conversation; two turns about bicycles do NOT make an unrelated question
surface the bicycle note; the fact-book tie breaks the same way with nothing
new appearing; and the nudge reorders equals but cannot overturn a clearly
better match.

## Guard movements, honest direction only

`facts::recall` (the context-free delegate) moved ONTO `TEST_ONLY_METHODS`
with its reason — production always has a thread, so the daemon's three call
sites go through `recall_in_context`; the ranking tests keep using the bare
form. No ceiling was raised. `context_weight` shipped in `tools.yaml` with a
reader.

## Verified by running

Full suite: **30 targets, 6,098 passed, 0 failed** (6,086 at the 22 Sep
baseline → +8 meaning, +4 contextual). Every isolated guard binary re-run
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## Still open

- Learning #6's embedding upgrade: blend a context vector (embedded recent
  turns) into the same nudge once the encoder is installed — the seam exists.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  encoder's first run on real Windows. All are one-command checks on the
  machines they belong to.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
