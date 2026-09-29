# Merge, 26–27 September 2026: the Atlas Project chat's 25k–26b, and Atlas for friends

Eric: *"Atlas isn't going to just be used by me. I am sending Atlas to my friends to run on their own devices… see what can be worked on … in the Awaiting Merge folder … I am not trying to lose any of the features I have been working on for several days."*

## 1. The result

The branch to take is **`friend-ready`**. It is master (`c502f1d`, doc 36, phones without a Mac), plus:

| commit | what |
|---|---|
| `0689e9e` | The Atlas Project chat's **26a**: the full code catalogue and its generators, the 25 Sep rulings record completed, the outstanding list. |
| `b462956` | Its **26b**: Atlas answers instead of replying with documents (doc `32b`). |
| `625f4c9` | One merge fix: the guest tests 26b moved need master's newer `Elsewhere` (`sync_port`). |
| `d1a6d7c` | **Atlas on a friend's machine** (§4), and the merge's test bookkeeping. |
| (top) | This record, and the start-here pointers. |

The first run of the whole suite on the merged tree, before `d1a6d7c`, failed only on the three tests in §2 and a count of untested helpers. Nothing was merged away to make it pass.

Nothing is lost:

- The lost-work check (`tests/nothing_from_any_version_was_lost.rs`) now also covers 25k–26b. That adds 57 rows: every new file, `pub` item, test, intent and command since 25j.
- All **3,212 rows** from all three chats are in the tree.

The as-delivered history is kept on **`project-line`**: 25j (`c069ce1`, already in the repo), then 25k (`f8c8e68`), 26a (`bcb2273`) and 26b (`0523505`). Each archive was imported exactly as delivered, and its SHA-256 matched its `SHA256-*.txt`:

- 25k `091fa540…`
- 26a `3b02a3f7…`
- 26b: the tree this chat built and handed over

## 2. How it was merged

- 25k was already in master: the other chat merged it as `775e995`.
- So only what came after 25k was brought over: 26a and 26b, each cherry-picked onto master. That's a three-way merge, with the 25k tree as the base.
- Git merged every code file by itself except `tests/which_model_fits_here.rs`. It counts the doors that have a model: master had added `atlas fix` and 26b had added two more. The resolved count is **six**: the definition, the daemon, `atlas voice`, `atlas fix`, the one-shot `handle`, and the typing prompt.
- `src/main.rs` and `config/tools.yaml` merged cleanly. doc 32b had warned about both, when the patch was checked against the older merged line.
- For `00_START_HERE.md`, `01_STATE_OF_PLAY.md` and `MANIFEST.txt`, master's copy was kept. 26a's versions of them describe the unmerged line.

**Three of master's tests had been written against things 26b changed on purpose.** Each was updated to test the new behaviour:

| test | was | now |
|---|---|---|
| `idle_cost::the_shipped_request_still_parses_with_the_keep_alive_in_it` | read the shipped `llm:` (Ollama) | 26b ships that block commented out, because a hand-written `llm:` beat Atlas's own model everywhere. The test now reads the commented Ollama example and checks that it still takes the keep-alive. It also checks that the shipped settings don't name a connection again. |
| `hublive::the_deck_greets_you_by_the_time_on_your_wall_clock` | expected "Eric" from the shipped settings | The shipped settings now greet you with no name. A name set in the settings is used. |
| `guests_at_the_daemon` (moved in 26b) | built `Elsewhere` without `sync_port` | built as master's struct is now |

## 3. The Awaiting Merge folder, item by item

Doc 33 §2 accounts for everything that was in the folder on 26 Sep morning. These are the items it didn't cover, or that arrived after it:

| item | what happened |
|---|---|
| `Atlas handoff 2026-09-25\25k\` | Imported as `f8c8e68`; hash checked. Already in master as `775e995`. |
| `Atlas handoff 2026-09-26a\` | Imported as `bcb2273`; hash checked. Merged as `0689e9e`. |
| `Atlas Project\Atlas handoff 2026-09-26b\` (beside the folder) | Imported as `0523505`. Merged as `b462956`. |
| `ATLAS_MERGED_2026-09-26.zip` (7:41 PM) | The other chat's merge at `87884f7`. Every branch and tag in its bundle was checked: all are ancestors of master, except `c069ce1` (25j). That one came in by content, and the lost-work check proves it. |
| `ATLAS_FULL_HANDOFF_2026-09-26.zip` | The main chat's handoff at `133721d`, which is an ancestor of master. Its record is doc 28. |
| *[row removed 28 Sep 2026: trading-system material]* |
| `32_SESSION_…_the_rulings_built_1.md` (loose) | An in-between copy. It differs from the tree's only in two lines about Ctrl+Alt+A, which the tree's copy replaces (no Alt key). |
| `30c_…_1.md` (loose) | The older 12,066-byte version. Doc 33 found it contained in the kept one. |
| `36_PHONES_WITHOUT_A_MAC_2026-09-26.md` (loose) | Byte for byte the tree's. |
| `Atlas-0.1.0-android.apk`, `Atlas-for-Windows-25k/26a/26b.zip`, `keys-fix-in-progress\*.zip` | Built programs; their source is in the tree. Nothing to merge. |
| `_for_merge\atlas-current-all.bundle` | The laptop repo, bundled for this merge (every branch). |

## 4. Atlas on a friend's machine

Each of these only ever worked because the machine was Eric's laptop, set up by hand. Each is fixed and tested in `tests/atlas_on_a_friends_machine.rs` (10 tests, in its own test target because it sets up an install).

**1. Atlas never started its own model server.**
- On the laptop, llama-server had been started by hand for the 26b test.
- Only "which model" ever tried to start it, and that was refused every time: the helpers' budget is 600 MB, sized for a browser and a camera, and a model is 2–4 GB.
- The shipped setting names `llama-server`, which isn't on a fresh Windows PATH.

So on a friend's machine every question went to a server that wasn't there. Now:

- The typing prompt, `--daemon` and `atlas voice` start the server on the first turn, in the background.
- A question waits for it to finish loading (at most 2 minutes, and only right after Atlas started it).
- It stays warm through a conversation and is let go after 30 minutes of quiet.
- It's found in `tools\llama\`, where `atlas get pictures` puts it. A server someone started by hand is left alone.
- How big a model may be is still decided by the memory the machine has free, not by the helpers' budget.
- Tested with a stand-in server: started once, with the model and `127.0.0.1`, owned by Atlas, not started twice, started again after being let go. A test's Atlas never starts one.

**2. Everyone was called Eric.**
- The shipped `persona.address` was "Eric". It's now empty.
- First run now ends: *Say "call me" and your name if you'd like me to use it.*
- On Eric's own laptop, a guest (after "you're talking to someone else now") was still called Eric: in replies, in the deck's greeting and on the sidebar. Now the guest gets no name.
- "Stop calling me that" never reached the model, because an empty spoken answer was read as no answer. It reaches the model now.

**3. What the model and the friend were told still said "Eric".**
- Working a window for you told the model to write "on Eric's behalf, in Eric's voice".
- A friend's Atlas that stopped getting updates said "worth checking with Eric".
- Both now speak of whoever uses Atlas.

**4. `atlas doctor` said nothing about the language model**, the one thing every general question needs. It now has a "talking" line. Depending on what it finds, that line:
- names the model and says it will start when needed;
- says the program that runs it is missing; or
- says there's no model, and that `atlas get pictures` fetches one (about 3 GB).

## 5. Measured

| what | result |
|---|---|
| personal Atlas, `cargo test --no-fail-fast` | 34 targets, **7,136 passed, 0 failed**, 6 ignored (speech tools not on this machine, doc-comment examples) |
| *[row removed 28 Sep 2026: trading-system material]* |
| Windows build, `cargo build --release --target x86_64-pc-windows-gnu` | clean, no warnings; `atlas.exe` sha256 `1ce42738…d1c477`, in `Atlas-for-Windows-friend-ready.zip` |

## 6. For Eric

**Taking it on the laptop.** With the other chat idle, in `C:\Users\erics\Atlas\atlas-current`:

```
git fetch "<path>\atlas-friend-ready.bundle" friend-ready:friend-ready project-line:project-line
git checkout master
git merge --ff-only friend-ready
```

`friend-ready` is built on master `c502f1d`, so this is a fast-forward. If master has moved, use `git merge friend-ready` instead.

**Your own name.** Your laptop's settings now ship with no name. Say "call me Eric" once. The spoken name is kept, and it wins over the settings from then on.

**For a friend:**
1. Install.
2. Run `atlas get` (hearing and speaking).
3. Run `atlas get pictures` (the language model and the program that runs it, about 3 GB).

`atlas doctor` says what's still missing.

## 7. Not done, and why

- **A friend's machine hasn't been seen.** The server starting itself was tested with a stand-in, not with the real llama-server on a fresh Windows. The laptop can show it: stop the hand-started server, then ask Atlas a question.
- **A one-shot `atlas "question"` doesn't start the server.** It gets the plain line that the language model isn't running. Starting a 3 GB server for one question and leaving it running isn't a good trade.
- **The phone Atlas (`mobile.rs`) has no language model.** Phones can't run llama-server. General questions there get the same plain line; using the laptop's or your server's model from the phone is a separate piece of work.
- The hub is paused. Rulings A, H2, H7, H13f and K are still held (see `OUTSTANDING_2026-09-26.md`).

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
