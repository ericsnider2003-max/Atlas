# Start here (updated 27 Sep 2026, second session)

> **28 Sep 2026 — this repository is personal Atlas only.** The separate Atlas for the trading work and the files that ran beside it, their history, the root `docs/` exports of the Project chats and the generated `catalogs/` left this repository on this date; lines in the records below about that work were replaced with a marker. Everything removed is kept verbatim in a private extract outside this repository, and the git history before this commit still contains it. `atlas/tests/personal_atlas_is_its_own.rs` keeps it out.


- **Newest line:** branch `session-0927b` (on top of `handoff-0927`). Desktop folder: `Atlas Project\7. Handoff 2026-09-27b (Tor, voice, phone model)`.
- **Read first:** `39_SESSION_2026-09-27_tor_ships_voice_and_the_phones_own_model.md` `40_SESSION_2026-09-27_the_hub_catches_up.md` `41_SESSION_2026-09-27_before_the_sandbox.md` `42_THE_MARK_2026-09-27.md` and `43_SENDING_AN_UPDATE_FROM_THE_HUB_2026-09-27.md`, then `OUTSTANDING_2026-09-27b.md` and `OPEN_GAPS.md`.
- **New build switches:** `--features phone-llm` (phone builds; `phone-llm-metal` on iPhone and iPad). The desktop build is unchanged without them.
- **Tests that need things this workspace doesn't always have:** `ATLAS_TOR_BUNDLE=<expert bundle folder> cargo test --test all tor_ships -- --ignored` (real tor); `ATLAS_PHONE_TEST_MODEL=<gguf> cargo test --features phone-llm --test the_phone_thinks_for_itself -- --nocapture` (the phone's engine).

What follows is the 26b guide, kept as it was.

# Atlas — full handoff for a new chat (26 September 2026, "26b")

Give the new chat this file, then the archive.

## 1. What's in this folder

| file | what |
|---|---|
| *[row removed 28 Sep 2026: trading-system material]* |
| `atlas-26a-to-26b.patch` | everything that changed from 26a (the answers fix, its record, the catalogue) |
| `atlas-answers-fix-code-only.patch` | just the code of the answers fix (20 files) — for the merged line in `Atlas\atlas-current`, which has the same faults; git reported three files that need merging by hand: `src/main.rs`, `tests/which_model_fits_here.rs` and `config/tools.yaml` |
| `SHA256-26b.txt` | checksums for the whole archive and each part |
| `atlas-25b-to-26b.patch` | this line's changes as a patch onto 25b, the archive Eric's other two chats work from (checked to apply cleanly) |
| `test-fixtures-26b.tar.gz` | the eight binary test documents the patch can't carry (they're also in the full archive) |
| `Atlas-for-Windows-26b.zip` | the Windows `atlas.exe` built from this exact tree (also inside the archive, in `builds/`) |
| `CATALOG_*.md`, `CODE_CATALOG_…`, `MODULE_REFERENCE_…` | the catalogue, loose, so it can be read without unpacking |
| `OUTSTANDING_2026-09-26.md` | everything not done, loose |

**Rejoin the archive:**
- Windows: `copy /b ATLAS_FULL_HANDOFF_2026-09-26b.tar.gz.part00 + ATLAS_FULL_HANDOFF_2026-09-26b.tar.gz.part01 ATLAS_FULL_HANDOFF_2026-09-26b.tar.gz`
- Linux/macOS: `cat ATLAS_FULL_HANDOFF_2026-09-26b.tar.gz.part* > ATLAS_FULL_HANDOFF_2026-09-26b.tar.gz`

Then check it against `SHA256-26b.txt` and unpack. It makes one folder, `merged/`.

## 2. Read, in this order (all inside `merged/`)

1. `00_START_HERE.md` — the map of the handoff and the measured results.
2. `01_STATE_OF_PLAY.md` §0 — where things stand today.
3. `OUTSTANDING_2026-09-26.md` — what's left and what each item waits on.
4. `32b_SESSION_2026-09-26_answers_not_documents.md` — the most recent fix; `32_SESSION_2026-09-25_the_rulings_built.md` — the work before it.
5. `catalogs/README.md` — then whichever catalogue you need.

## 3. Measured for this handoff

| what | result |
|---|---|
| personal Atlas, `cd merged/atlas && cargo test --no-fail-fast` | 31 targets, **6,490 passed, 0 failed**, 4 ignored (2 need speech tools on the machine, 2 are doc-comment examples) |
| *[row removed 28 Sep 2026: trading-system material]* |
| Windows build, `cargo build --release --target x86_64-pc-windows-gnu` | clean, no warnings |

Full-suite run time here: about 12 minutes; the release Windows build about 7.

## 4. Standing rules for whoever picks this up

- **Merged on 27 Sep** (doc 37): this line is on master as branch `friend-ready`. Read doc 37 first.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **The hub resumed 27 Sep (doc 40).** Held rulings: A, H2, H7, H13f (see OUTSTANDING).
- **Build process:** research first, build and test everything buildable, name what's blocked and why. Don't stop at a plan.
- **Guards:** the test suite fails if code is added that nothing calls, a setting that nothing reads, or a phrase Atlas tells you to say that nothing hears. A new test file needs a `mod` line in `atlas/tests/all.rs` (autotests is off). A new voice command goes in `atlas/config/commands.yaml` and `Intent`.
- **Laptop:** the live-test install is `C:\Users\erics\AtlasLiveTest-0925` (26b, running, with llama-server and the 4B model started by hand). Don't touch `C:\Users\erics\Atlas\atlas-current` — that's the other chats'.

## 5. Regenerating the catalogue

From `merged/`: `python3 atlas/docs/genreports.py`, `python3 atlas/docs/gencatalog_extra.py`,
and from `merged/atlas/`: `python3 docs/genref.py`. Regenerate; don't hand-edit.
