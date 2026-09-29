# Merge handoff: `round11` into `master`

**26 September 2026.** Everything the main Atlas chat, or you, needs to merge this chat's work into `master`, check it, and undo it if needed. It's written to be followed without reading anything else first. The deeper documents are listed in §9.

Nothing here is installed on any machine, and nothing here can place a trade.

---

## 1. The short version

| | |
|---|---|
| **Merge** | branch **`round11`** into **`master`** |
| **Where** | `C:\Users\erics\Atlas\atlas-current` on le3o. Both branches are already there. |
| **`round11` tip** | see `git log -1 round11` (this document is in it; the code was final at `eac0c12`) |
| **`master` it was built on** | `24bf4b4` ("Friends reach each other with no Tailscale between people") |
| **Kind of merge** | **fast-forward**, as long as master is still at `24bf4b4`. `master` is already inside `round11`, so no conflicts are possible. |
| **Size** | 58 commits; 423 files (311 new, 112 changed); +98,289 / −1,127 lines. About 29,000 of the added lines are the password-guessing word lists in `atlas/config/guessable/`. |
| *[row removed 28 Sep 2026: trading-system material]* |

```powershell
cd C:\Users\erics\Atlas\atlas-current
git status                              # must show no changes to tracked files
git branch master-before-round11 master # a way back, kept until you're sure
git checkout master
git merge --ff-only round11             # refuses rather than surprise you
```

If `--ff-only` refuses, master has moved since `24bf4b4`. Go to §3.

---

## 2. Before you merge

1. **The main chat is idle.** It commits to `master` in this same folder. A merge while it's mid-edit leaves its uncommitted work sitting on top of 423 changed files.
2. **`git status` is clean for tracked files.** Two untracked folders are expected and harmless:
   - `.r10test/` is left over from round 10's laptop run. You can delete it.
   - `Claude outputs/` holds files the desktop app delivered.
   - Neither is in git, and the merge doesn't touch them.
3. **No stash is needed.** `git stash list` was empty on 26 Sep.
4. **Make the backup branch** (the `git branch master-before-round11 master` line in §1). It costs nothing and makes §7 one command.

---

## 3. If master has moved

Use a normal merge instead of `--ff-only`:

```powershell
git checkout master
git merge round11
```

Every merge between the two lines so far (eleven of them, the latest being `24bf4b4`) has conflicted **only on guard bookkeeping**, never on behaviour. Here is where it happens and how to settle it.

| file | what conflicts | how to settle it |
|---|---|---|
| `atlas/src/capability.rs` | `MODULES_IN_TREE`, and its comment log | Keep round 11's number (**377**) and add master's new modules on top. Round 11 on `24bf4b4` is 377 (see the comment block above the constant). If master added `N` modules, it's `377 + N`. Add a one-line comment naming them. |
| `atlas/docs/CAPABILITIES.md` | generated text | Take either side, then regenerate: `atlas catalog --markdown > docs/CAPABILITIES.md` (from `atlas/`, with the debug build). |
| `atlas/tests/catalogue.rs` | `UNCLAIMED_MAX` | Round 11 left it at 171. If master adds unclaimed modules, add them. |
| `atlas/tests/dead_capabilities.rs` | `TEST_ONLY_MAX` (251), `HELPER_UNTESTED_MAX` (4) | These are exact counts, not ceilings. Take the higher of the two sides, run the test, and set each to what it measures. |
| *[row removed 28 Sep 2026: trading-system material]* |

**Things master's new code may trip over:**

- **`http::Response` has a new field, `location`.** Round 11 added it so feeds can follow redirects on purpose. Any new `http::Response { status, body }` literal in master needs `location: None`. The compiler names each one. `24bf4b4` had two, in `kin.rs`, both fixed in `0d68250`.
- **A new intent in master needs one more line.** `workday::read_first` doesn't change the checklist: `plugins.rs` still needs the intent in `NEVER` or `PERMISSIONS`, and `profiles.rs` or `tests/handed_over.rs` still needs it on exactly one list.
- **Bare function names now taken by round 11.** `tests/name_collisions.rs` fails if master adds a public function whose bare name matches one of round 11's with no module-qualified caller. It happened twice with `24bf4b4` (`split_url`, `addresses`). The fix is to rename one side, or add it to `NAME_COLLISION_ONLY` with the reason.
- **`commands.yaml`**: round 11 appended 17 command blocks at the end. A new master phrase that equals one of them fails `tests/commands_are_distinct.rs`, which names the phrase.
- **`config/tools.yaml`**: round 11 appended a `workday:` block at the end, and edited the `clipboard:` comment (not its keys). Both sides' blocks should be kept.

**A working rule for conflicts:** behaviour files (`daemon.rs`, `intent.rs`, `brain.rs`) have always merged cleanly by hand. Keep both sides' arms and match lines, then let the compiler and the guards say what's missing.

---

## 4. What master gains

This chat's rounds 1–11, which master has never had. Each round has its own session document (§9).

| round | when | what |
|---|---|---|
| 1 | 21 Sep | 21 open-source pieces ported in house: repeat rules (RFC 5545), civil dates, cron, stemming and BM25, chunking, typo tolerance, mail threading (JWZ), vCard, HLC clocks, rate limits, Readability, and more |
| 2–3 | 22–23 Sep | 15 gap-filling ports, and 6 written-down gaps closed |
| 4 | 23 Sep | the gaps rounds 1–3 left open, closed |
| *[row removed 28 Sep 2026: trading-system material]* |
| 6 | 24 Sep | the rendering arms: GIF/MP4 from SVG animations, an in-house 3-D renderer; opt-in diarization |
| 7 | 24 Sep | moving 3-D: keyframes, easing, a camera; the renderer rebuilt and checked against Blender 4.2 |
| 8 | 25 Sep | 3-D models from OBJ/STL/glTF files, glass, glow, patterns, a denoiser; people who speak once; the open-gaps register |
| 9 | 25 Sep | the working day: telling working from away, `worklog` (where the time went), offers held for a natural break, a resume cue, `when` (times in words) |
| 10 | 25 Sep | 19 gaps from a second look fixed (times read sure-but-wrong, silent reminders, a crash on "İ"); natural pauses learned per app; the Windows passphrase read in-process; animations refined by word |
| 11 | 25–26 Sep | round 10's 17 ideas built: clipboard history, text off the screen, the market's calendar, waiting-for, dated capture and review, a launcher, the trading check-in, meeting prep, snippets, find any file, PDF tools, people, feeds, receipts, habits, flashcards, translation, and key chords; your day in the brief |

**By area:**
- `atlas/src`: 65 new modules and 66 changed.
- `atlas/tests`: 152 files.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- `atlas/config`: `commands.yaml`, `tools.yaml`, and the new `guessable/` word lists (MIT, licence file beside them).
- `atlas/docs`: 15 files.
- The handoff set: 6 files in `docs/handoff`, 80 notes in `docs/improvements-project/`, and the session documents `21_…` to `26_…`.

**Build changes:**
- **No new crates.** `atlas/Cargo.toml` only adds Windows API features: cryptography, power, keyboard, notifications, the clipboard, memory, OCR, imaging, streams and collections.
- Two new test targets: `vad_measured` and `voice_measured`.
- The Windows build was cross-compiled and run on le3o. It needs nothing installed.

**Settings:**
- New keys in `tools.yaml`: `time_zone`, `pronounce`, `worklog:` and `workday:`.
- All have defaults, so an existing `config/settings.yaml` keeps working unchanged.
- `upgrade::YOURS` still protects `settings.yaml`, `machine.yaml` and `data/state`.

**New data files** in `data/state`, each created only when first used:
- `people`, `habits`, `cards`, `snippets`, `feeds`, `receipts`, `trade_journal`, `waiting_taught`, `launcher_uses`, `mailbook`, `clipboard_history_on`, and the round 9 work log.
- Clipboard history itself is **never written to disk**.

**Behaviour a master user will notice:**
1. **The brief has your day in it** (a new `Source::Day`): promises due, replies owed, dated notes, people to catch up with, birthdays, habits and cards due, and the market's calendar once you've done a trading check-in. A clean install's brief is still empty.
2. **Round 11's lists are read whole.** They aren't cut to two spoken sentences. Every other reply is shaped for speech as before.
3. **"Open spotify" for an app not in `apps.yaml`** goes to the launcher (Start-menu shortcuts), after the grants gate still asks about an unknown app.
4. **A bare "yes" or "no"** after a round 11 question (a receipt, a trading check-in, a flashcard) is taken as the answer, not "Nothing to confirm".
5. **"Atlas never watches the clipboard" has one exception**, and it's off until you turn it on: clipboard history. The guard test checks it ships off and is never saved.
6. **The mail check also reads your Sent folder** (the one marked `\Sent`), since the last check, for waiting-for and meeting prep. The reading is capped at 300 letters a check, and at 3,000 kept for 60 days, as excerpts with secrets scrubbed.

---

## 5. After merging: check it

From `atlas-current`:

```powershell
cd atlas
cargo build
cargo test --lib --bins --test all          # the bulk: about 6,400 tests
cargo test --test dead_capabilities --test name_collisions --test new_capabilities_are_wired --test handed_over
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
cargo test                                  # 814
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
```

Or, from Git Bash at the root: `./verify.sh` (the full suite, both crates), or `./verify.sh --quick` (about 2 minutes).

**Expected:** 0 failed everywhere. 2 ignored (they need whisper and piper). The full debug suite takes 25–30 minutes on a 2-core machine, and much less on le3o.

**Round 11's Windows-only tests** (`tests/round11_on_windows.rs`) run with `cargo test --test all round11_on_windows`.
- They type nothing, open nothing, and show nothing.
- The clipboard test writes to your clipboard and puts back what was there.
- It needs a normal desktop session. From the remote shell, Windows refused the clipboard (§6).

**If you changed anything while merging, regenerate what's generated:**

```bash
# from the repo root, Git Bash
ATLAS_HOME=/tmp/ah ATLAS_CONFIG=$PWD/atlas/config ./atlas/target/debug/atlas catalog --markdown > atlas/docs/CAPABILITIES.md
python3 docs/handoff/catalog.py
python3 docs/handoff/extract_refs.py   # then splice .refs_modules.md, .refs_urls.md, .deps.md into REFERENCES.md §2/§4/§5
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
git ls-files | grep -v '^MANIFEST.txt$' | LC_ALL=C sort | sed 's|^|./|' | xargs -d '\n' sha256sum > MANIFEST.txt
```

---

## 6. Evidence on the tip

| suite | result |
|---|---|
| personal Atlas, every debug target (lib, bins, `all`, the 26 separate targets) | **6,709 / 0** on `6de1501`; after merging `24bf4b4`, **6,723 / 1** (a name `wire` also uses), renamed, re-run **6,724 / 0** |
| *[row removed 28 Sep 2026: trading-system material]* |
| guards | dead capabilities (test-only 251, helper-untested 4, unchanged), dead methods, dead config, name collisions, the catalogue, every intent reaching the daemon, phrases that parse, the retrospective: all green |
| round 11 on le3o, release build | **85 / 86** |
| the Windows run's details | Windows OCR read a rendered invoice exactly in 41 ms. A `RegisterHotKey` chord registered, and Win+L was refused. 75 Start-menu shortcuts were walked in 27 ms. The clipboard's sequence number was read. Sentence pre-reading costs 0.2 µs. |
| the one miss | writing to the clipboard: Windows refused it to the remote shell's session. PowerShell's own `Get-Clipboard` failed the same way. Re-run from your desktop. |
| PDF writer | 18 real PDFs rewritten and stamped; every output passed `qpdf --check`, with page counts matching `pdfinfo` |
| *[row removed 28 Sep 2026: trading-system material]* |

---

## 7. Undo

If anything is wrong after a fast-forward:

```powershell
git checkout master
git reset --hard master-before-round11   # back to 24bf4b4 exactly
```

The `round11` branch stays, so nothing is lost. Once you're satisfied, delete the backup branch with `git branch -d master-before-round11`.

**Data written by the new features** stays in `data/state` and is ignored by older code. Each file is plain JSON, and you can delete it on its own if you want it gone.

---

## 8. After the merge: the branches

- **`round6`–`round10` and `round8m`** are steps on the way. Each is inside `round11`, and nothing needs them. `git branch -d round6 round7 round8 round8m round9 round10` removes them once master has `round11`; git refuses any that isn't merged.
- **Keep `round11`** until you've run the checks in §5.
- **Future rounds from this chat** will branch from master again.

---

## 9. Where everything is

| read | for |
|---|---|
| `00_START_HERE.md` | the whole handoff: what's where, how to build, test and run, the rules |
| `OPEN_GAPS.md` | every gap still open, why, what closes it, who has to act |
| `MASTER_BUILD_PLAN.md` | the plan of record and the completeness map |
| `26_SESSION_…round11…` | round 11 in detail: each tool, what makes it sound, measurements, the Windows run |
| `25_…` to `21_…` | rounds 10 to 6; rounds 1–5 are in `docs/improvements-project/` |
| `atlas/docs/CAPABILITIES.md` | what Atlas can do, where each capability runs, in what state (generated) |
| `docs/handoff/MODULES.md` | every source and test file with its purpose line (generated) |
| `docs/handoff/REFERENCES.md` | every algorithm, paper, spec, open-source project read, crate licence, external program, model and data file |
| `catalogs/` | code catalogue and the dead-capability ledger (generated) |
| `MANIFEST.txt` | SHA-256 of every tracked file |

**Checked for completeness (26 Sep):**
- All **85** documents from this chat's project are in `docs/improvements-project/`, exported from the project, from the first (10 Sep) to this one.
- Every source and test file those documents name is in the tree. The very first package (10 Sep) had only partly landed: its second and third passes (crew efficiency, idle cost) and four test files were missing. They were rebuilt from its design on 26 Sep, before this merge, and are in round11 with their tests. See `OPEN_GAPS.md` §6.
- `tests/promised.rs` now lists what every package promised, and fails by name if any of it leaves the tree. Run it after the merge like everything else; it is part of the gate.

**What's open, in brief** (the full list is in `OPEN_GAPS.md`):
- Yours to turn on: clipboard history, key chords, and a signature PNG for "sign the pdf".
- Waiting-for and meeting prep fill from the next "check my mail".
- The Windows clipboard test needs re-running from your desktop.
- Meeting prep reads people from event titles, because imported events don't keep attendees.
- CPI dates run out after December 2026.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
