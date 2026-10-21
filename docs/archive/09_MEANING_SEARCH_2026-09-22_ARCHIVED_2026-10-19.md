# Learning #4 closed — meaning search wired end to end (22 Sep 2026)

The semantic half of `recall` had everything except a source of vectors:
`Piece.embedding` was only ever `None`, `set_embedding`/`unembedded` sat on the
dead-methods list, and `semantic: false` shipped with a comment saying "turn on
once an embedding model is installed". This session built the missing source
and wired it the whole way through.

## What was built

- **`src/meaning.rs`** — the encoder seam, the same external-program shape as
  `speaker.rs`: the encoder is named in config, text goes in on stdin (and as
  `{text}`), numbers come back, `parse_embedding` is reused. Plus
  **`Remembered`**: vectors persisted content-keyed (FNV-1a, spelled out
  because `DefaultHasher` is seeded per process), so the library — which is
  rebuilt from the notes folder on every load — rehydrates unchanged notes for
  free, re-embeds edited ones, and prunes vectors for deleted ones.
- **Daemon wiring** — `embed_backlog` drains unembedded notes two per tick
  (housekeeping section, respects pause); `query_meaning` embeds the question
  at ask time inside `from_notes` and hands it to `Library::search`. Every gap
  — semantic off, no encoder, encoder failed — degrades to word search
  silently, which is the design the ranking was built around.
- **`doctor`** — a `meaning-search` line with three states, parallel to
  voice-lock: off by choice, on and installed, or on WITHOUT an encoder — the
  last is `recall::needs_a_model`'s first caller and names exactly what to
  install. None reads as a failure, because word search is complete alone.
- **`config/tools.yaml`** — a `meaning:` section with a commented-out example
  encoder entry naming the model `fit.rs` already plans for
  (all-MiniLM-L6-v2, ~90MB).
- **Tests** — `tests/meaning_search.rs` (registered in `all.rs`), five
  behavioral tests through the daemon with a fake encoder that maps related
  words to the same vector: a note sharing NO words with the question is found
  by meaning, and its control proves word search alone misses it; vectors
  survive a restart with no encoder present; the toggle is respected; an
  edited note loses its stale vector and gets a fresh one. Plus three unit
  tests in the module.

## Guards moved, all in the honest direction

| ceiling | before | after | why |
|---|---:|---:|---|
| `TEST_ONLY_MAX` | 285 | **282** | `recall::set_embedding`, `unembedded`, `needs_a_model` gained production callers |
| `HELPER_UNTESTED_MAX` | 5 | **4** | `reload_library` gained a direct test |
| `MODULES_IN_TREE` | 291 | **292** | `meaning.rs`, claimed by the `recall` capability (UNCLAIMED unchanged at 174) |

`dead_methods` and `new_capabilities_are_wired` lists shrank by the same
entries, dated in place. `docs/CAPABILITIES.md` regenerated; the `recall`
entry now says both halves honestly: words always, meaning once a model is
installed.

## Verified by running

Full suite: **30 targets, 6,094 passed, 0 failed** (was 6,086 — the +8 are the
new tests). All isolated guard binaries re-run (`handed_over`, `guards`,
`capability_wiring`, `new_capabilities_are_wired`, `dead_capabilities`,
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
files.

## The one step left, and whose it is

Everything is built and tested with a stub. What remains is the model asset on
Eric's machine: an embedding wrapper + all-MiniLM-L6-v2 (~90MB), named under
`tools.meaning.encoder`, then `recall.semantic: true`. Until then Atlas
searches by words and `atlas doctor` names the gap. Learning **#6 (contextual
recall)** remains open — design question with Eric.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
