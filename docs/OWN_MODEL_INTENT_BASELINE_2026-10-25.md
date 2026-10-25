# Own model: the intent baseline to beat (25 Oct 2026)

Last updated: 2026-10-25 · Status: measured baseline, research only

This is the first measurable step of the `own-model` item (`OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md`, "First small measurable step"). It answers one question: how well does Atlas understand commands today **with no language model at all**? Any local or in-house model has to beat these numbers to earn its memory.

## The fixed test sets

They are already in the tree, locked, and run on every test pass. They were written from ordinary spoken sentences, each paired with the exact ordered steps it asks for (`atlas/tests/free_form_corpus.rs`):

- **Corpus** (133 sentences): the set the router was tuned against.
- **Held-out** (32 sentences): never tuned against. This is the honest measure of how well it handles phrasings it hasn't seen.

A sentence passes only if every step reads as the intended command, in order. A reading that would *do something else* is counted separately as a **wrong action**, which is worse than "I didn't catch that".

These two sets stand in for the planned 200-utterance golden set. Topping them up to 200 with phrasing Eric approves is the next step below.

## Result: candidate `46df31e6`, phrase table plus splitter, no model

| Set | Understood | Accuracy | Wrong actions | Wrong-action rate |
|---|---|---|---|---|
| Corpus | 130 / 133 | 97.7% | 1 | 0.75% |
| Held-out | 22 / 32 | 68.8% | 1 | 3.1% |

Command: `cargo test --test all -- --test-threads=1 free_form_the_corpus_pass free_form_the_held_out --nocapture`. Measured 25 Oct 2026.

For reference, the whole sentence read once (the router before 12 Oct): corpus 33/133 (20 wrong), held-out 6/32 (6 wrong).

## Against the scorecard (rows 13-15: accuracy ≥ 95%, wrong actions ≤ 0.5%)

- **Corpus:** passes accuracy, just misses on wrong actions (0.75%).
- **Held-out:** fails both. The table is tuned to the sentences it has seen; new phrasing is understood about two times in three.

**The gap a model has to close is held-out, not corpus:** from 68.8% to ≥ 95% on unseen phrasing, without adding wrong actions. A model that only matches the corpus number adds memory cost for nothing.

## Not measured here

- **Latency per utterance.** These tests time the whole run (about 9 s for 11 tests), not each sentence. Scorecard rows 1-6 need their own timing harness.
- **The model path.** The installed build's language model was not running when this was measured (see below), so there is no "phrase table + Qwen3-1.7B" number yet.
- **Online services:** comparison (b) in the options doc.

## Found on the way: the local model isn't running

The installed laptop's `atlas.log` shows Qwen3-1.7B loaded and answered on 23 Oct. Every start since then logged "couldn't start the model server: no language model in the models folder fits this machine". The model's measured cost is 2,508 MB against a 2,048 MB floor that must stay free. The laptop usually has about 3-5 GB free, so the memory rule often refuses it. The model is on disk, but in practice it is off.

## Next steps

1. Re-measure with the model running, when the laptop has room. Same two sets, through the model path.
2. Top the sets up to 200 sentences from `atlas/config/commands.yaml` and phrasing Eric approves, still keeping a held-out part.
3. Add per-sentence timing.
4. Then bench the small bases (Granite-4.0-H-1B, Qwen3-1.7B Q4, BitNet 2B4T) against this table. Downloads need Eric's yes.
