# The three-chat merge, 26 September 2026

**What was merged:** everything in `Desktop\Atlas Project\4. Awaiting Merge`, into one tree. That folder held work from all three chats:

- **the main chat:** the courier, friends, and Tor;
- **this chat:** rounds 1–11 and the 10 Sep rebuild;
- **the third chat:** the settings window, the hub in the window, your clock, call notes, working a window for you, security, typing correction, and the backlog rulings.

**The result:** branch **`all-merged`**. It is master (`8d507ee`, the main chat's newest, with update self-checks), plus `tenth-sep`, plus the third chat's whole line from 23 Sep to its 25h checkpoint.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

Nothing here is installed on any machine, and nothing here can place a trade.

---

## 1. Take it

```powershell
cd C:\Users\erics\Atlas\atlas-current
git status                                  # no changes to tracked files
git branch master-before-three-chat-merge master
git checkout master
git merge --ff-only all-merged
```

`all-merged` is built on master `8d507ee`, so this is a fast-forward as long as master hasn't moved. (It was built on `22958f1`, and master's next two commits were merged in on the way.) If it has, use `git merge all-merged`. The places to expect conflicts are in §6.

**To undo:** `git reset --hard master-before-three-chat-merge`.

---

## 2. What was in the folder, item by item

Every file was hashed and checked: against its own SHA-256 list where one existed, against git history where it was ours, and against the other copies where there were several. Nothing was set aside unread.

### The third chat (its work had never been merged anywhere)

| Item | What it is | What happened |
|---|---|---|
| `Atlas handoff 2026-09-24\ATLAS_FULL_HANDOFF_2026-09-24b.tar.gz` (2 parts) | The third chat's tree on 24 Sep | Hash matches its `SHA256.txt`. Committed as the first step of its line, on our durable base `4a1164a`. Its base is exactly our base: `_23e_src.txt` lists the base's modules, and it matches ours file for file. |
| `Atlas handoff 2026-09-25\…25b` to `…25g` (2 parts each) | Its checkpoints on 25 Sep | All six hashes match `SHA256.txt` and `SHA256-25e/f/g.txt`. Each is committed in order, so its history is kept step by step. |
| `ATLAS_FULL_HANDOFF_2026-09-25h.tar.gz` (2 parts; one copy loose, one in the subfolder, identical) | Its newest checkpoint: the security group and typing correction built, E/F/G/H in progress | Hash matches `SHA256-25h.txt`. Committed. |
| `ATLAS_FULL_HANDOFF_2026-09-25g.tar.gz` (the loose, whole one) | 25g in one piece | Same bytes as the two 25g parts joined. Nothing extra. |
| `atlas-25b-to-25d/e/f/g/h.patch` (25g and 25h also loose) | Each checkpoint's code change from 25b | Each was applied to 25b. Every one reproduces its archive exactly, so the archives lose nothing that the patches carry. |
| `README*.txt`, `SHA256*.txt` | Joining instructions and fingerprints | Read and used for the checks above. The loose `README-25h.txt` and `SHA256-25h.txt` are identical to the subfolder's. |
| Docs 26–32, 30b, 30c (loose and in the subfolder) | Its session records, the backlog sort, the rulings | 26–32 and 30b are identical wherever they appear, and all are in 25h. **30c** came in three versions. The one in the subfolder (14,250 bytes) is the newest: it adds the H13 lettering from your screenshot. It contains the other two, and it's the one in the tree now. |
| `atlas-sendtest.ps1`, `atlas-chart-test.ps1` | Its laptop probes: the typing-pace measurement behind doc 29, and a known chart for the picture reader | In no archive. Kept, with a note, in `docs/probes-25sep/`. |
| `_23e_src.txt` | The module list of the 23e archive | Used to prove where its line starts (above). Nothing to merge. |
| `Atlas-for-Windows.zip` and `Atlas-for-Windows_2.zip` (identical) | Its Windows build at 25g: `atlas.exe` sha `7c6dd9a0…`, as README-25g says | A built binary with no source in it. Nothing to merge. |
| `atlas-livetest-0925.zip` | Its live-test build from doc 29 (`atlas.exe` sha `69383905…`) | A built binary. Nothing to merge. |

### This chat

| Item | What happened |
|---|---|
| `r11b`–`r11e.bundle`, `round10.bundle`, `round10_1.bundle`, `tenth-sep.bundle` to `tenth-sep4.bundle` | Each bundle's commit (`eac0c12`, `c85ba08`, `30cba72`, `16941fd`, `1f9867f`, `413f564`, `3472b86`, `341d486`, `c215f84`, `bd51961`) was checked to be an ancestor of the merge. |
| `ATLAS_MERGE_HANDOFF_2026-09-26.md` and `.zip` | The round 11 merge guide as it stood before the 10 Sep rebuild. The `.md` is the same bytes as doc 27 at `30cba72`/`16941fd`, and the zip packages that same tree. Superseded by this document. |
| `r11win.zip` | The round 11 Windows test kit: `all.exe`, config and fixtures. Every config and fixture file in it is in git history, byte for byte. |
| `all.zip`, `atlas.zip`, `config.zip` | The round 10 laptop test build. Every config file is in git history, byte for byte. The rest are built binaries. |

### The main chat

Its work arrived through master: `22958f1`, which `tenth-sep` is built on, then `133721d` and `8d507ee` (a new version checks itself and goes back by itself), which were merged in cleanly. Nothing of it was loose in the folder.

---

## 3. What your tree gains from the third chat

These are in its session docs, now in the repo root. Their numbers overlap ours, so the file names say which is which.

| Doc | What |
|---|---|
| `20_SESSION_2026-09-23_settings_in_atlas_window.md` | A Settings page in Atlas's own window, kept the moment you change it, with a Restart button |
| `21_SESSION_2026-09-23_hub_in_the_window_and_windows.md` | The hub inside Atlas's window through Windows' own web view; Windows' blocks answered without a certificate |
| `22_SESSION_2026-09-24_clock_settings_voice_line_and_seeing.md` | Times on your clock; settings that apply without a restart; the voice line moving with Atlas's speech; seeing |
| `23_SESSION_2026-09-24_finished_call_notes_delegation_envelope.md` | Call notes (the others only after they say yes); working the window in front for you; the envelope |
| `24_SESSION_2026-09-24_the_gaps_the_audit_found.md` | The first audit's gaps, fixed |
| `25_SESSION_2026-09-25_window_work_is_an_errand.md` | Window work is an errand like any other |
| `26_SESSION_2026-09-25_new_work_through_the_crew.md` | Window replies, call write-ups and the picture reader go through the crew |
| `27_WSHOBSON_AGENTS_FOR_ATLAS_2026-09-25.md` | Build and security council rooms; replies checked for a chatbot's voice; research figures checked |
| `28_SESSION_2026-09-25_a_no_comes_with_a_way_to_yes.md` | A no names what would make it OK; the activity log sealed; calls graded; search checks itself |
| `29_SESSION_2026-09-25_your_answers_and_the_laptop.md` | The log checked against backups; graded examples kept, scrubbed; the envelope yours alone; typing paced |
| `30_SESSION_…`, `30b_…`, `30c_…`, `31_…`, `32_…` | Restart resumes work; runbooks; hollow finds dead code; the backlog sorted and ruled; the trading split; two-factor, sign-in, making accounts, typing correction |

The code behind these includes 17 new modules: `hubwin`, `settingswin`, `overlaywin`, `webview2_loader`, `speaking`, `callwatch`, `callrec`, `callnotes`, `localclock`, `picture_talk`, `next_up`, `phases`, `resume`, `twofactor`, `webrun`, `astype` and `platform::idle`. It also includes a vendored `webview2-com-sys` and three new Windows-only crates: `wry`, `cpal` and `raw-window-handle`.

---

## 4. Where the chats built the same thing twice, and what the merge did

In seven places two chats solved the same problem separately. Each one was merged into a single thing rather than kept as a pair or dropped. The hub's look (item 6) and the two seals (item 4) were left for you, and you ruled on both on 26 Sep.

1. **Chrome keeping the connection open.** Both chats found that Atlas's own browser could never attach to Chrome, and each fixed it (`http::reply_is_whole` in ours, `http::whole_reply` in the third chat's).
   - **Merged:** one function, `whole_reply`, with our stricter end-of-chunked-body check, run once after each read.
2. **Your idle time.** Both chats added `input_idle_secs`.
   - **Merged:** the third chat's version, which leaves Atlas's own typing out, plus our `quiet_state`.
3. **Typing into a window.** Round 11 typed 64 keys per batch for snippets. The third chat measured Notepad garbling anything faster than about 20 ms per character.
   - **Merged:** the third chat's version: 25 ms per character, a newline as Shift+Enter so a chat reply is never sent half-typed, and read back before Enter.
4. **A sealed activity log.** Both chats built one. Ours is a Merkle log with a checkpoint a day. The third chat's is a hash chain, with its heads kept beside the log and checked against every backup.
   - **Merged:** every entry is now sealed both ways. `atlas journal check` and doctor use the chain and the backups; `verify_seal` checks the daily checkpoints.
   - **Your ruling (26 Sep): keep both.**
5. **Local time.** Ours was the `time_zone` setting, with daylight-saving rules and a zone per event, but unset meant UTC. The third chat's was `localclock`, which reads the machine's clock.
   - **Merged:** one home zone. It is your setting if you've chosen one, and this computer's clock (daylight saving included) if you haven't. The setting now offers "Automatic" first.
   - `localclock` follows the same zone, so the hub and the calendar can't disagree.
   - The calendar kept round 9's parser. The third chat's calendar calls and its eight clock tests go through a thin wrapper, and all pass.
   - A weekly event with no zone of its own now repeats on your clock's weekday, the third chat's fix. It's done without shifting an event that has its own zone twice.
6. **The hub's look.** On 20–21 Sep, with this chat, you locked Warm Paper as the lead colourway. On 23 Sep, the third chat built the command deck as "Eric's design" (`atlas/design/command-deck.html`).
   - **Your ruling (26 Sep):** the command deck is the hub, and Warm Paper is its default colourway, with the settings to change it.
   - **What that means in the code:**
     - Nothing chosen gives Warm Paper, on the deck's layout.
     - The "Aa" menu on every page offers Paper, Light, Dark and Auto. Auto follows the computer's setting.
     - Settings has a new "How it looks" section with colourway (Warm Paper, deck dark, follow this computer, colour-blind safe), accent, colour-blind mode and density. Those choices were stored before but had no page to change them. Picking a colourway there clears the Aa menu's theme, so the one you picked shows.
     - With the system's high-contrast setting, Warm Paper's text was turning white on cream. It now stays dark.
   - Our accent picks, colour-blind modes and density are kept. Each accent has a dark and a light value.
   - **Still to come:** the hub design the third chat was waiting on (for H2, H13f and K) wasn't in the folder. The hub will be rebuilt to it when it arrives; until then the look above is a placeholder, not the finished hub.
7. **Appearance menus.** Ours is in Settings ("How it looks"). The third chat's is the hub's "Aa" menu (theme, text size, contrast, motion).
   - **Merged:** both apply. Where both say the same thing, the "Aa" menu wins, until a colourway is picked in Settings.

---

## 5. Where one chat removed something another still used

The third chat's 25g pass removed 62 functions that its tree never called. Two of them are used by our line:

- `vault::Kind::usable_unattended`: the vault opened on your Windows sign-in (round 5).
- `panel::Panel::transient`: the waking panel fading by itself.

Both are back, with tests. The compiler checked everything else: nothing else either line uses is missing.

The main chat's GUI-free core (`--no-default-features`, for the phone) still builds. Only the drawing parts of the third chat's settings page and overlay sit behind the desktop build, and the rest of each is in the core.

Other smaller changes:

- The third chat's commands are decided for add-ons: every one is refused, because each acts as you or on other people.
- Its tools config reads go through the shared, resolved copy. That copy is rebuilt when your settings change, and so is the crew's memory margin and battery floor.
- Four of its functions now share a bare name with one of ours. Each is recorded, with its reason, in `tests/name_collisions.rs`.

---

## 6. Checks

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  - The first run on the merged tree failed 14 guard tests. Every failure was bookkeeping the guards exist for, and none was behaviour: lists to re-sort, functions newly wired, two now-unused settings, the capability counts, an add-on decision for each of the third chat's commands, and one line that read the hour in a way the third chat's UTC guard flags. The first fix pass left none failing on the rerun; the second rerun is the result above.
- **Windows, natively on your laptop:** the whole `all` suite was run on the merge and, for comparison, on master (`8d507ee`) alone. Each ran in its own folder, beside `atlas-current`, so master's working tree wasn't touched. Master alone: 22 failed. The merge's last run: **5,853 passed, 20 failed**. Nineteen of the twenty fail on master too (below). The twentieth, `the_line_moves::the_watch_reads_the_voice_level_now`, is a timing test from the third chat: under the load of a full native run it read the voice level 150 ms late and caught the quiet end of a 300 ms loud part. Its loud part is now 3 seconds long, so a late read still lands mid-word; nothing in Atlas changed. Rerun on your laptop, it passes.
  - **Found and fixed:** a test that raised a notification made the test program launch itself: `all.exe window …`, which ran every test with "window" in its name. That set off a storm of test runs fighting over the clipboard and the key chords, and it put real toast notifications on your screen. Now only the Atlas program itself opens a panel or shows a toast. This was already on master, and on the third chat's line too.
  - **Fixed:** five tests that failed only because of Windows (CRLF, path separators, Windows paths in YAML).
  - **What still fails there fails on master too, for the same reason:** tests that call Unix tools (`sh`, `cp`, `echo`, `cmd` quirks), the friend tests' loopback connections, and animation GIFs that need the browser. None is caused by the merge. They're named in `OPEN_GAPS.md` §9.
- **The GUI-free core:** `cargo check --no-default-features --lib --bins` is clean.
- **`tests/promised.rs`** now also lists what the third chat's line and this merge promised: 38 more rows. It fails by name if any of it leaves the tree.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

### If master has moved when you merge

- **`atlas/src/capability.rs`:** `MODULES_IN_TREE` is 393, and each module master adds goes on top.
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **`MANIFEST.txt`:** retake it.
- **A new `self.tools_cfg().<section>` read in master's code:** add `.clone()`. The compiler names each one.

---

## 7. What's still open because of the merge, or newly unblocked by it

**Work that isn't in the folder, so isn't here.** 25h was a mid-session checkpoint. Groups E, F, G, H and then D, I and J from the rulings were "in progress" or "next" there. Anything the third chat built after 25h never reached the folder.

**Unblocked now that the merge is done** (each was ruled "waits for the merge"):

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- **H7:** phone calendar sync.
- **H2, H13f, K:** these wait on your hub design.

**Your two design calls, ruled 26 Sep:** the command deck with Warm Paper as its default colourway, and both seals kept.

**On the laptop:** typing correction hasn't had a live run yet, and neither has a real call for call notes (both from doc 32 and doc 29).

**Binaries in the tree are older than the code:**

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- Rebuild them from this tree before shipping them anywhere.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
