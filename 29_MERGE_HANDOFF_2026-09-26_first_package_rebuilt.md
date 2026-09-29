# Merge handoff: `tenth-sep` into `master`

**26 September 2026.** One branch, one fast-forward. It finishes the very first package from the improvements chat (10 Sep, "work off the tick"), which only partly reached the tree.

Nothing here is installed on any machine, and nothing here can place a trade.

---

## 1. The short version

| | |
|---|---|
| **Merge** | branch **`tenth-sep`** into **`master`** |
| **Where** | `C:\Users\erics\Atlas\atlas-current` on le3o. The branch is already there. |
| **Built on** | `master` at **`22958f1`** ("Handoff: full accounting of the main chat's work, 24-26 Sep"). Round 11 is already in master (merged as `2543dbc`), so this is only what came after. |
| **Kind of merge** | **fast-forward**, as long as master is still at `22958f1` |
| **Size** | 5 commits; 27 files (6 new, 21 changed); +2,400 / −261 lines |
| *[row removed 28 Sep 2026: trading-system material]* |

```powershell
cd C:\Users\erics\Atlas\atlas-current
git status                              # must show no changes to tracked files
git branch master-before-tenth-sep master
git checkout master
git merge --ff-only tenth-sep
```

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

---

## 2. Why this exists

The 10 Sep package went out as a zip. Its first pass reached the tree (the crew, `why_waiting`, `Store::save` skipping identical writes). Its second and third passes and its four test files never did, and no copy survived anywhere on the laptop. Nothing checked that what a package promised had landed, so nobody noticed for two weeks.

They were rebuilt from the package's design document (`docs/improvements-project/handover-crew-10sep.md`) against today's code, with tests. A guard was added so this can't happen quietly again.

---

## 3. What master gains

**Background work admitted by what it costs** (`atlas/src/crew.rs`):

- Each job says what it needs: the whole machine (a build, a render, the council's model calls), one core, or mostly waiting (a download, a copy, a mail check).
- Only one whole-machine job runs at a time, with nothing else thinking beside it. Two renders side by side each finish later than they would one after the other.
- Waiting work has its own allowance and never queues behind a render.
- One core is always kept free for you.
- Nothing that thinks starts with less memory free than **`crew.keep_free_mb`** (1024).
- On battery under **`crew.battery_floor_percent`** (30%), heavy chores Atlas chose itself wait for the charger. Anything you asked for still runs.
- Urgency ages, so nothing waits forever. A heavy job at the front holds the thinking slots rather than being kept out by small ones.
- The same work asked twice runs once.
- A full queue refuses with a sentence saying so.
- What each job waited and ran is recorded. The Status page shows it under "Work in hand".
- `why_waiting` names the rule holding a job.
- When a slot frees, the tick naps 100 ms instead of up to 2 s.
- Each crew errand's cost is decided in one place: `daemon::crew_job`.

Both settings are under *What it may touch*.

**What Atlas costs while idle:**

- **Saving.** A quiet tick saves on the minute; a tick that said something saves at once (`persist_after`). Before this, every state file was re-serialised every two seconds.
- **Folder scans.** They double their wait while nothing changes, up to 30 minutes, or 5 minutes while you're at the machine. Anything found, or anything you say, sets it back to a minute. Eight quiet hours now cost about 16 walks instead of 480. The Status page shows the count.
- **The tools config.** It's resolved once and shared. Before this it was copied whole at every read, at 80 call sites.
- **The model.** It stays loaded between turns when `fit` says the machine can afford it: 30 minutes, or 60 seconds when it can't. This applies only to a local server. A `keep_alive` you set yourself wins.
- **Running tools.** Atlas checks on them starting at 1 ms, doubling to 200 ms. An hour-long render now costs about 18,000 wake-ups instead of 144,000.

**Found on the way:** nothing read the battery. `health::read_machine` never filled `on_battery` or `battery_percent`, so the overnight run's battery check could never fire. It's now read on Windows, Linux and macOS.

**Left out on purpose:** the 10 Sep progress-line rate limit. Today's render collects the tool's output whole, so there's no per-frame line to limit.

---

## 4. Checks

```powershell
cd C:\Users\erics\Atlas\atlas-current\atlas
cargo test --test all -- crew:: crew_efficiency:: idle_cost:: wants:: promised::
cargo test --test dead_capabilities --test name_collisions --test new_capabilities_are_wired
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
```

To retake the personal-Atlas manifest after a merge that conflicts on it:

```bash
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
```

**`tests/promised.rs` is new, and it's the fix for how this happened.** It lists what each package promised: the file, a line of code that only exists if the work does, and what it is. It covers the 10 Sep package and round 11's modules. A merge that drops any of it fails the test by name. Every future package adds its rows there before it's called done.

---

## 5. Evidence

| suite | result |
|---|---|
| personal Atlas, every debug target, on the merged tree (`3472b86`) | **6,779 / 0**, 4 ignored |
| the same work on round11 before the merge (`16941fd`) | **6,785 / 0**, 2 ignored. The first run caught six things the guards exist for, all fixed before the second: two unused functions, two helpers without a test, a name collision, and a source check still reading the old `tools_cfg`. |
| *[row removed 28 Sep 2026: trading-system material]* |
| Windows cross-compile of the library | clean |
| on le3o natively | **85 / 85** (`--test all`, debug build, in a separate worktree so master's working tree was not touched). The battery was read as on mains at 100%, matching `Win32_Battery`. Round 11's Windows checks passed again: OCR in 49 ms, 75 Start-menu shortcuts in 23 ms, a chord registered, and the clipboard written and put back. That last one had failed from the remote shell before. The first run caught one real bug, fixed in `c215f84`: `promised.rs` didn't allow for CRLF line endings in a Windows checkout. |
| *[row removed 28 Sep 2026: trading-system material]* |

---

## 6. Undo

```powershell
git checkout master
git reset --hard master-before-tenth-sep
```

---

## 7. Where things are

- The full register entry: `OPEN_GAPS.md` §6, "This chat's first package … CLOSED 26 Sep".
- The design it was rebuilt from: `docs/improvements-project/handover-crew-10sep.md`.
- The code: `atlas/src/crew.rs`, `daemon.rs` (`crew_job`, `crew_room`, `persist_after`, `resolve_tools`), `awareness.rs`, `brain.rs` (`with_keep_alive`), `tools.rs` (`poll_gap`), `health.rs` (`read_power`), `hublive.rs`, `settings.rs`.
- The tests: `atlas/tests/crew.rs`, `crew_efficiency.rs`, `idle_cost.rs`, `wants.rs`, `promised.rs`.
