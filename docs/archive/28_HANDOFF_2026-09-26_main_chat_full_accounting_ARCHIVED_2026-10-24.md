# Atlas — full handoff and accounting for the main chat (24–26 September 2026)

**Written 26 September 2026.** This accounts for every piece of work this chat did, from the durable base on 24 September to now: what was asked, what was decided, what was built, what was found broken and fixed, what was built and then removed on your instruction, what was tested and how, and what is still open. Each item points at the evidence: a commit on your laptop, a test, or a document.

**How this was checked.** Early parts of this chat were long enough that the conversation was summarised along the way, so their exact wording isn't kept word for word. The record here is built from what was written down at the time, and every line of it can be checked:

- **The git history** on your laptop. Every commit from 3007af9 onward carries this chat's session ID. The 24 September morning commits (the durable base and the hub design) come from the same chat, before the ID was added to commit messages.
- **The commit messages**, which were written when each change was made.
- **The documents written during the work:**
  - the update-courier spec, including its §20 build log;
  - `20_PHONE_AS_PEER`, `21_TAILNET_SYNC_MOBILE_CORE` and `MASTER_BUILD_PLAN`;
  - the open-gaps register.
- **The test suites**, re-run on the final tree.

Nothing listed as built here is missing from the tree. That was checked file by file against your laptop on 26 September (§12).

---

## 1. Where everything is

| | |
|---|---|
| **Repository** | `C:\Users\erics\Atlas\atlas-current` (a git repo) on your laptop |
| **Branch** | `master`. Its tip is **`014d4b6`** at the time of writing, plus the commit that adds this file. |
| **Everything else is inside `master`** | The other chat's round branches (`round6`, `round7`, `round8`, `round8m`, `round9`, `round10` and `round11`) were checked on 26 Sep with `git merge-base --is-ancestor`. |
| **Undo point for the round-11 merge** | Branch `master-before-round11`, which points at `03d7e7e` |
| **Working tree** | Clean: no uncommitted changes to tracked files |
| **This file** | In the repo root, as `28_HANDOFF_2026-09-26_main_chat_full_accounting.md`, and also at `C:\Users\erics\Atlas\ATLAS_HANDOFF_2026-09-26.md` |
| **The update-courier spec** | `atlas/docs/UPDATE_COURIER_SPEC.md`. §1–19 are the design, §20 is the build log with every gap from A to AP. |
| **Open gaps** | `OPEN_GAPS.md` in the repo root. Section 8 holds this chat's gaps. |
| **Project docs (claude.ai)** | `claude/update-courier-capability-spec-25sep.md`, `claude/open-gaps-register-25sep.md`, `claude/merge-handoff-round11-26sep.md`, and this handoff |

---

## 2. The commit ledger: every commit this chat made, oldest first

"Tests" is what that commit's own full run reported.

| # | Commit | When (UTC) | What | Size | Tests at the time |
|---|---|---|---|---|---|
| *[row removed 28 Sep 2026: trading-system material]* |
| 2 | `ba4da22` | 24 Sep 08:52 | **Hub: the agreed Warm Paper design ported into the palette.** The 29 screens you approved on 20–21 Sep had never been coded. | 2 files | 109 hub/manifest/design tests pass |
| 3 | `952cf93` | 24 Sep 08:59 | **Master build plan**: a completeness map of everything worked on against what's in code | 1 file | — (document) |
| 4 | `5108c81` | 24 Sep 09:13 | **Hub: appearance stored and applied**: theme, accent, colour-blind mode, text size, density (`src/appearance.rs`) | 3 files | 4 appearance + 34 hub tests pass |
| 5 | `3c87755` | 24 Sep 09:30 | **Hub: the header follows the palette**; phone layout checked against the mockups at 1280 px and 390 px | 1 file | 36 hub/nav/phone tests pass |
| 6 | `3007af9` | 24 Sep 20:23 | **Hybrid Logical Clock**: device sync ordered by it, not by wall time (`src/hlc.rs`) | 6 files | sync 58, hlc 10 |
| 7 | `7ec88ee` | 24 Sep 22:20 | **Direct same-network sync**: a bundle sent straight across, no folder (`src/transport.rs`) | 4 files | transport 4, daemon 105 |
| 8 | `a1b19fb` | 24 Sep 23:08 | **Sync listens every tick**, not only while syncing | 2 files | daemon 105 |
| 9 | `b70be92` | 25 Sep 02:50 | **Phone as a peer**: sync by tailnet address, a core that compiles without the desktop GUI, a phone platform layer, ONNX made optional, and eight red guards reconciled | 22 files | `all` 5,421 passed |
| 10 | `0436ddf` | 25 Sep 08:55 | **Update courier step 1**: the release-signature root (ed25519) | 12 files | reported green, but not fully: see #11 |
| 11 | `d369458` | 25 Sep 09:37 | **Update courier step 2**: the signed release manifest. It also **corrects #10's claim**: two guards were red, because the check had only run part of the suite. Every step since runs all targets. | 7 files | 6,264 passed, 0 failed, all 30 targets |
| 12 | `5dda9b2` | 25 Sep 10:44 | **Gaps A–G closed**; **step 3**: your edits survive every update (`src/yourchanges.rs`) | 14 files | all targets green |
| 13 | `715b53c` | 25 Sep 11:43 | **Gaps H–M closed**; **step 4**: add-ons, Tier 1 of the plugin boundary (`src/plugins.rs`) | 21 files | all targets green |
| 14 | `6b7e938` | 25 Sep 17:13 | **Asking less**; **gaps N–R closed**; **step 5**: owned groups and the release channel (`src/peerkey.rs`, `src/groups.rs`, `src/update_courier.rs`) | 29 files | all targets green |
| 15 | `773454a` | 25 Sep 20:11 | **Sharing add-ons by choice**; **gaps S–X closed**; **step 6 begun**: release keygen, sign and announce | 26 files | all targets green |
| 16 | `e6975c4` | 25 Sep 21:17 | **Friends in one step** (`src/friends.rs`); **gaps Y and Z closed** | 27 files | 30 targets green |
| 17 | `24bf4b4` | 26 Sep 02:09 | **No Tailscale between people**: Atlas's own encryption (`src/wire.rs`), router door-opening (`portmap`), friend helpers (`mailbox`) | 22 files | 30 targets green |
| 18 | `03d7e7e` | 26 Sep 04:34 | **Friends through Tor; nobody in the middle** (`src/onion.rs`). `portmap` and `mailbox` removed on your instruction. | 17 files | 30 targets green, plus a real-Tor check and an end-to-end run on a private Tor network |
| *[row removed 28 Sep 2026: trading-system material]* |
| 20 | `58ef3df` | 26 Sep 05:19 | Open-gaps register: the merge recorded, and this chat's gaps added as section 8 | 2 files | — |
| 21 | `014d4b6` | 26 Sep 05:20 | Register: the merge hash corrected after re-authoring | 2 files | — |

**About #19–21:** these were first committed as `be58e36` and `3f87481` under your name. An automatic check asked for them to carry Claude's name instead, so they were re-made with byte-identical content. On your laptop, `master` was moved to the new versions after checking the content matched.

---

## 3. Every decision you made in this chat, in force now

| # | Decision | Where it's recorded / how it's enforced |
|---|---|---|
| D1 | **The hub looks like the Warm Paper design you approved** (29 screens). Ember Dark and Access themes; accents Ember, Blue, Teal, Purple, Forest; a colour-blind mode that always wins. | `ba4da22`, `5108c81`, `3c87755`; `src/appearance.rs` |
| D2 | **The phone is a full Atlas, not a window.** Same Atlas as the laptop, works offline, syncs when the two meet. iOS as a PWA and Android native at first; later iOS native ad-hoc. | `20_PHONE_AS_PEER_2026-09-24.md`; `b70be92` |
| D3 | **How each platform gets Atlas:** one signed, normal installer per platform, and nobody has to turn a security setting off. | Spec §19; the deployment memory |
| D3a | iOS: **native ad-hoc** under your $99/yr Apple Developer account, with each friend's device registered. | |
| D3b | Android: a **native app (APK)**. | |
| D3c | Windows: signed with **Azure Artifact Signing** (about $10/month), so there's no SmartScreen or Smart App Control warning. | |
| D3d | macOS: **notarized** under the same Apple account. | |
| D4 | **Notifications are local.** Waking a closed iPhone app with a push is **yours only**, because it rests on your push key. | Spec §9 |
| D5 | **Add-ons (the plugin boundary):** Tier 1, declarative, now. Tier 2 (WASM code) later. | Spec §5; `src/plugins.rs` |
| D6 | **The release private key lives in your vault.** A recovery key is shown once. | Spec §20; `atlas release keygen` |
| D7 | **Key first, then any friend's copy.** The release key is made in the same sitting as the Apple and Windows signing setup, before any copy goes out. You'll want help with that sitting. | Spec §20 |
| D8 | **Approval mustn't make Atlas ask about everything.** Approving an add-on is the decision; a step can be answered "always"; nothing is asked when the answer couldn't change the outcome. | `6b7e938` |
| D9 | **A group's creator controls it**: who's in it, who can post and who can only read, adding and removing people. | `6b7e938` (step 5) |
| D10 | **Friends can share add-ons in a group or privately.** Others pick them up by choice; nothing installs itself. | `773454a` |
| D11 | **Adding a friend is one step**, like a texting app. No code sent back, no waiting for a confirmation. | `e6975c4` |
| D12 | **Tailscale only joins one person's own devices, never two people's Atlases.** You never have to link anyone to your Tailscale. | `24bf4b4`, `03d7e7e`; the spec |
| D13 | **No server in the middle**, and nothing you pay for to run one. | `03d7e7e` |
| D14 | **No friend's Atlas holds anyone's messages.** You chose "a mutual friend's Atlas" on 26 Sep, saw how it worked, said it felt wrong, and had it removed. | `03d7e7e` |
| D15 | **Friends connect through Tor**, with Arti (Tor rewritten in Rust) to come when it's ready. | `03d7e7e`; gap AL |
| D16 | **When a friend's Atlas is off, the sender's Atlas keeps the message** and sends it when theirs is back, with the original send time. This was already your 17 Sep rule. | `03d7e7e`; the courier |
| D17 | **Work goes step by step through the open list**, starting at step 1 (below). | This handoff, §11 |

---

## 4. What was built, area by area

### 4.1 The hub's design (24 Sep)

- **`ba4da22` — the palette.** Warm Paper by default: cream `#f7f4ee`, warm ink `#37352f`, orange accent `#d9730d`.
  - Ember Dark (`#0c0f14`, ember `#eb9d4a`) is used when you choose it, or when your system is set to dark mode.
  - Access is the Wong colour-blind-safe set.
  - The hard-coded dark colours in the CSS were turned into tokens.
  - The PWA manifest and the offline page open on Warm Paper.
- **`5108c81` — appearance is stored and applied** (`src/appearance.rs`).
  - Theme: System, Warm, Ember or Access. Accent: five choices. Colour-blind: None, Deuter or Tritan. Text size and density.
  - The page shell writes these onto the page, and colour-blind mode overrides the accent.
- **`3c87755` — checked against the mockups** at desktop (1280 px) and phone (390 px) widths.
  - The header had been dark-on-dark on Warm Paper. Fixed.
  - The phone layout folds to one column with 40 px tap targets.
  - Named as not done: a pinned bottom tab bar, and phone-only features that need a native layer.
- **Hub pages added later in this chat:** Add-ons, Your edits (both 715b53c), Groups (6b7e938) and Friends (e6975c4). All four are in the command palette and the navigation, and the navigation guard allows at most six items per group.

### 4.2 The plan

- **`952cf93` — `MASTER_BUILD_PLAN.md`**: a completeness map of what was built and in code, what had to be rebuilt, and what was never coded. The other chat has added its rounds to it since.

### 4.3 The phone and sync between your own devices (24–25 Sep)

- **`3007af9` — Hybrid Logical Clock** (`src/hlc.rs`, 10 tests).
  - Root cause fixed: sync had ordered events by raw wall-clock time. So with a phone's clock a few seconds behind, an edit made *after* taking in the laptop's could sort *before* it, and the merge would quietly settle on the wrong answer.
  - The clock now carries the order. A wildly-ahead remote clock is capped and reported, never dropped.
- **`7ec88ee` — direct same-network sync** (`src/transport.rs`, port 8790, 4 tests). One round trip over a socket is a full two-way sync. It's an extra, never something Atlas depends on: the shared folder still works.
- **`a1b19fb` — the sync listener runs every tick**, even while Atlas is paused, so a phone coming onto the wifi is answered at once.
- **`b70be92` — the phone as a peer:**
  - **Sync by address** (`dial_configured_peers`): a peer on your tailnet is dialled directly. A real socket test proves it.
  - **A core without the desktop GUI:** `desktop-ui` became a feature, so `cargo check --no-default-features` builds the whole core.
  - **`platform::mobile`**: a platform layer for phones.
  - **`onnx` made optional**, for the phone build.
  - **Guards reconciled:** eight guards the previous sync commits had left red were fixed, and two redundant methods were removed.
  - Blocked on hardware (still): the Android cross-compile, a real sync across two networks, and model speed on the phone.
- Documents: `20_PHONE_AS_PEER_2026-09-24.md` and `21_TAILNET_SYNC_MOBILE_CORE_2026-09-24.md`.

### 4.4 The update courier: getting new versions to you and friends (25 Sep)

The design (spec §1–19) was written first, in this chat. It covers:

- **The eight properties an update must have:** no reinstall, no data lost, no customisation lost, no central host, the author proven, offline first, least authority, and simple.
- **The mechanics:** the three-way config merge, the plugin boundary, signing and the key, how each platform installs, notifications, widgets, and re-signing.
- **What the pre-build review found:** a bad update reaching everyone at once, weak rollback on phones, delta updates, versioned plugin API, and the yearly iOS re-signing.

Then the build:

- **Step 1 (`0436ddf`), `src/release.rs`.** ed25519 sign and verify. A placeholder key means "trust nothing", never "accept anything unsigned". New crate: `ed25519-dalek`, justified in the dependency list.
- **Step 2 (`d369458`), the signed manifest.** One signature covers every platform's file.
  - What it records: the release number, which only goes up; the version; the oldest data format it can open; and for each platform, the file name, size and SHA-256.
  - The checks run in a set order: size cap, then the signature *before* anything is read, then format, then order, then platform.
  - 16 tests cover these attacks: replay, path traversal, downgrade, oversized input, and fake entries.
  - New crate: `sha2`.
- **Gaps A–G closed (`5dda9b2`).**
  - A: one data-format number.
  - B: files are hashed while downloading and refused past their signed size.
  - C: the installed release is remembered.
  - D: rolling back needs an approval made on the device itself; nothing arriving from outside can add one.
  - E: one name for each platform.
  - F: the freeze-attack date. A signed "next word by" date means a device notices when it's being held back.
  - G: key rotation, plus an offline recovery key.
- **Step 3 (`5dda9b2`), `src/yourchanges.rs`: your edits survive updates.** A three-way merge of the shipped config files.
  - Hand edits move to `config/local`, and shipped defaults follow new releases.
  - A default that changes under one of your edits is reported once.
  - A bad edit can never stop Atlas starting.
  - A file you'd edited that a release replaces is kept.
- **Gaps H–M (`715b53c`).**
  - H and I: the "Your edits" page and `atlas edits`, with "back to the default" for each edit.
  - J: the label lists turned out to be compiled into the models, so they're no longer written out as files that look editable but do nothing.
  - L: a Settings choice now remembers the default it replaced.
  - K and M: settled.
- **Step 4 (`715b53c`), `src/plugins.rs`: add-ons.** Named sequences of ordinary commands, no code.
  - **Permissions:** grouped into plain categories. A forbidden list (vault, pairing, sync, accounts, handover, self-modification and more) can never be granted.
  - **Nothing runs until you approve it.** The approval records the file's fingerprint.
  - **Every step is checked again as it runs**, so revoking takes effect at once.
  - **Triggers** must be at least two words, must match exactly, and must never collide with a built-in phrase.
- **Asking less (`6b7e938`).** Approving an add-on is the decision. A step can be answered "always". Nothing is asked when the answer couldn't change the outcome.
- **Gaps N–R (`6b7e938`).**
  - N: add-ons and their approvals ride sync; approvals are only taken from sealed bundles.
  - O: add-ons can be sent to a friend.
  - P: add-ons can run on a schedule.
  - Q: add-ons can be removed.
  - R: closed as far as it can be.
- **Step 5 (`6b7e938`):**
  - **`src/peerkey.rs`.** Each Atlas has its own key, introduced over `/hello` and pinned. A different key later is refused, never quietly swapped.
  - **`src/groups.rs`, groups with an owner.** The owner signs a numbered member list with member and reader roles.
    - Forged, claimed, replayed and tampered lists are refused.
    - Readers can't post.
    - Messages that arrive before their group's list wait for it.
    - Someone who leaves is taken off, and the owner can't walk out.
  - **`src/update_courier.rs`, the release channel.** Only the owner posts. Signed notices are checked against the built-in key and said once.
- **Sharing add-ons (`773454a`).** "Shared with you" shelf: nothing is installed or approved just by arriving. It shows who sent it, what it would be allowed to do, and every step. Taking it is one decision, and you can recommend it in a group.
- **Gaps S–X (`773454a`).**
  - S: the owner passes members' messages on to the others.
  - T and U: your devices vouch for each other, and can manage your groups.
  - V: see step 6.
  - W: voice commands for managing groups, which add-ons and guests can never use.
  - X: an older group can be given an owner.
- **Step 6 begun (`773454a`).** `atlas release keygen`, `sign`, `announce` and `show`.
- **Gaps Y, Z and AA.**
  - Y (`e6975c4`): release files travel in checked 256 KB pieces, resume where they stopped, and are kept only if the whole file matches the signed fingerprint.
  - Z (`e6975c4`): your phone posts in your groups as you.
  - AA (`773454a`): the "Shared with you" shelf shows every step, not only the description.

### 4.5 Friends: the path, including what was replaced

1. **`e6975c4`, friends in one step** (`src/friends.rs`).
   - "Add a friend" makes a one-time link: 128 random bits, good for one use, for 7 days.
   - Opening it pairs both Atlases at once, and each pins the other's key.
   - A friend request can go through a group, to that one person only, and is accepted with one press.
   - A friend who's offline is retried for the week the link lasts.
   - There's a hub Friends page, voice commands, and `atlas friend`.
2. **Your objection, 25–26 Sep:** no Tailscale for friends. On your instruction, **Tailscale was restricted to your own devices** (D12).
3. **`24bf4b4`, first version without Tailscale:**
   - **`src/wire.rs`** — kept to this day: Atlas's own encryption. Every request is sealed to the key pinned for the receiver and by the sender's key, with a one-time key per envelope, a freshness window and replay refusal. The answer comes back sealed too. It passes the published HKDF test vector from RFC 5869, and a token taken from someone else's envelope is refused.
   - **`portmap`**: Atlas opening its own door on your router.
   - **`mailbox`**: a mutual friend holding sealed mail.
4. **Your objection, 26 Sep:** friends holding mail felt wrong. I researched how messaging apps do this and set out the options: nobody in between, Tor (as in Briar and Ricochet), or the Keet-style approach, which still needs relays for some connections. **You chose Tor** and asked for `portmap` and `mailbox` to be removed.
5. **`03d7e7e`, friends through Tor** (`src/onion.rs`, all written here):
   - The onion address is made from a separate key derived from Atlas's own key. SHA3-256 is written here and checked against the official test vectors.
   - Tor's SOCKS5 client is written here too.
   - Atlas starts `tor` itself. It forwards to a door that only accepts sealed envelopes.
   - Friends on the same wifi connect straight across.
   - The sender keeps undelivered messages.
   - **Proof:**
     - Tor itself accepted Atlas's keys and served exactly the address Atlas had worked out.
     - Two Atlases with **no network in common** became friends through Tor, on Tor's own private test network (`chutney`).
6. **Removed at your direction** (in `03d7e7e`): `portmap` (the router door), `mailbox` (friends holding mail), and the `/check`, `/where`, `/pass` and `/pickup` doors. Earlier, in `24bf4b4`, the `tailscale serve --tcp` forwarding for friends went too. None of them is anywhere in the tree now, and nothing you'd kept depended on them.

### 4.6 The merge that kept the other chat's work (26 Sep)

- **What I found:** when you asked whether anything had been lost, I found that the other chat's 58 commits (its rounds 6–11) were still only on its own branch. My Tor commit had moved `master`, so the merge its handoff described would have stopped with an error.
- **`2543dbc`:** merged them into `master`. There were three conflicts, all bookkeeping, and each is resolved in the commit message.
- **Tested on the merged tree:**
  - personal Atlas: 6,726 passed, 0 failed, 6 ignored (whisper, piper, a real Tor network);
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  - the real-Tor check passes.
- **Records:** the other chat's merge-handoff document in the project now says it's done, and the open-gaps register was updated.

---

## 5. Root causes found and fixed along the way

These weren't asked for. They were found while building, and each was fixed at its cause.

| Found | What was really wrong | Fixed in |
|---|---|---|
| Sync ordering | Events were ordered by wall clock, so a skewed phone clock could merge to the wrong answer. | `3007af9` |
| Sync never applied anything | Events travelled between devices, but nothing applied them. A note captured on the phone never reached the laptop's notebook. | `6b7e938` |
| Windows local time | Local time on Windows was always read as UTC, so schedules ran at the wrong hour. It's now asked of Windows. Named as not yet proven on your laptop. | `6b7e938` |
| Label lists | They were written out as editable files but compiled into the models, so edits did nothing. | `715b53c` |
| A false "all green" | `0436ddf`'s message said the whole suite passed, but two guards were red. Corrected in the next commit, and every step since runs every target. | `d369458` |
| Duplicate release helpers | `release::sha256_hex` and `verify` duplicated other functions. | `6b7e938` |
| The door between Atlases | It listened on this machine only, with nothing forwarding it, so pairings completed and then never connected. Now the ordinary door serves your own networks and friends arrive over Tor. | `e6975c4`, then `03d7e7e` |
| The door only opened at startup | It only opened if a pairing already existed, so your first friend could never knock. | `e6975c4` |
| Groups with one other person | They weren't recognised as groups: "message the Friends group" failed. | `e6975c4` |
| Hub header | Dark text on a dark bar in Warm Paper. | `3c87755` |
| Guards left red | Eight guards were red after the sync commits. | `b70be92` |
| **A file I overwrote** (25 Sep) | Writing the new per-Atlas key module, I wrote over the existing `identity.rs` by mistake. It was restored from your laptop's copy the same session, the new module was renamed `peerkey.rs`, and you were told. | Before `6b7e938` |
| Shared-name methods | A new method name (`handoff`) made an unrelated setting look used to a guard. Renamed rather than hiding the guard. | `24bf4b4` |
| Your phone in your own group | A device you'd vouched for could change your groups but couldn't post in them as you. | `e6975c4` (gap Z) |
| Disk full during testing | The workspace filled with test logs during a run. Cleared, the run repeated, nothing lost. | — |

---

## 6. Evidence: the test runs

| When | Tree | Result |
|---|---|---|
| Each commit from `d369458` to `03d7e7e` | this chat's `master` | every test target, 0 failed (the targets and their counts are in each commit message) |
| *[row removed 28 Sep 2026: trading-system material]* |
| 26 Sep | real `tor` program | **Tor accepted Atlas's onion keys and served the same address** (`onion::tests::tor_itself_serves_the_address_atlas_worked_out`) |
| 26 Sep | Tor's private test network (chutney, hs-v3-min) | **two Atlases with no network in common became friends through Tor** (`two_atlases_with_no_network_in_common_become_friends_through_tor`, 5 s) |
| Every step | the build without default features (the phone core) | compiles |

**Ignored on purpose, and why:**

- **Tor tests:** they need the `tor` program or a Tor network. Run them with `--ignored`; the doc comments say how.
- **whisper and piper tests:** they need those programs and their models.

**Not provable from here:**

- The public Tor network: the build machine can't reach it.
- Your real routers, phones, or Windows desktop session.

---

## 7. Every gap, A to AP, and where it stands

| Gap | What it was | Status |
|---|---|---|
| A | No single data-format number | **Closed** `5dda9b2` |
| B | Large files checked in memory | **Closed** `5dda9b2` |
| C | Where the installed release is kept | **Closed** `5dda9b2` |
| D | Rolling back must come only from you | **Closed** `5dda9b2` |
| E | One name per platform | **Closed** `5dda9b2` |
| F | Freeze attack | **Closed** `5dda9b2` |
| G | Key rotation and recovery | **Closed** `5dda9b2` (callers: see open item O2) |
| H | First start has no base on record | **Closed** `715b53c` |
| I | Undoing an edit did nothing | **Closed** `715b53c` |
| J | Label lists not covered | **Closed** `715b53c` (root cause: they were never read from disk) |
| K | Comments on edited lines don't travel | **Settled** (the previous file is kept) |
| L | A Settings choice didn't record the default it replaced | **Closed** `715b53c` |
| M | Edits made while running are caught on next start | **Settled** |
| N | Add-ons don't follow you to other devices | **Closed** `6b7e938` |
| O | A friend can't send an add-on | **Closed** `6b7e938` |
| P | Add-ons only start by voice | **Closed** `6b7e938` (schedules) |
| Q | No remove | **Closed** `6b7e938` |
| R | Content choosing the target within what you granted | **Closed as far as it can be** `6b7e938` |
| S | Members only reach members they're paired with | **Closed** `773454a` |
| T | Losing the owner's device loses its groups | **Closed** `773454a` |
| U | You on two devices are two identities | **Closed** `773454a` |
| V | No command to post a release notice | **Closed** `773454a` |
| W | No voice commands for groups | **Closed** `773454a` |
| X | Older groups stay ownerless | **Closed** `773454a` |
| Y | Release files don't travel | **Closed** `e6975c4` |
| Z | Your phone can't post as you | **Closed** `e6975c4` |
| AA | Shared add-ons judged by their description | **Closed** `773454a` |
| AB | Friends on unrelated networks needed Tailscale | **Superseded**: no Tailscale between people (`24bf4b4`), then Tor (`03d7e7e`) |
| AC | A friend's QR code reads as text on the phone | **Open** (O10) |
| AD | Only the announcing Atlas hands out a release | **Open** (O11) |
| AE | Both friends behind shared addresses | **Closed by Tor** `03d7e7e` |
| AF | Helpers see who writes to whom; 30-second delay | **Closed**: there are no helpers (`03d7e7e`) |
| AG | A release can't be fetched through a helper | **Superseded** by Tor (fetching works over Tor; speed is AN) |
| AH | "Delivered" meant "handed to the helper" | **Closed** `03d7e7e` |
| AI | IPv6 behind a router firewall | **Closed**: not needed with Tor |
| AJ | Windows firewall rule for the door | **Open** (O9) |
| AK | Tor must ship in the installer | **Open** (O3) |
| AL | Arti | **Open** (O5) |
| AM | Networks that block Tor | **Open** (O6) |
| AN | Speed through Tor | **Open** (O7) |
| AO | Antivirus and `tor.exe` | **Open** (O8) |
| AP | Phones don't run Tor; they reach their own desktop over their own Tailscale | **By design** |

---

## 8. Things researched and answered in conversation (no code)

- **How standard apps work without a server (26 Sep).**
  - WhatsApp, Signal, iMessage and Telegram all rely on company servers, and on Apple's or Google's push service.
  - Calls try to go direct and fall back to a relay.
  - The "serverless" apps use other users' machines instead: old Skype's supernodes, BitTorrent and Jami's shared lookup networks, and Briar over Tor or Bluetooth.
- **NAT traversal research:**
  - The largest recent measurement found about 70% of hole-punching attempts succeed, and every attempt still needs a third party to introduce the two sides.
  - iOS apps are suspended in the background, so an iPhone can't receive while asleep without Apple's push service.
- **The Tor question (26 Sep):**
  - Nothing to pay and nothing to set up.
  - Its downsides, and which can be fixed in-house: speed partly; the separate program eventually (Arti); blocked networks mostly, with bridges; antivirus flags are a watch item.

---

## 9. What's open, in the order we'll take it

This is the list you asked for on 26 Sep, now numbered.

**Update safety and delivery (step 6 of the build plan):**

1. **O1: the health check and automatic rollback.** A new version must pass a self-check when it starts, or the desktop goes back to the kept previous version. **Done 26 Sep** (spec §20, "Step 6 — O1"); its follow-on, AQ, is to copy the data before a trial once the data format first changes.
2. **O2: install a release with the previous one kept, a manual undo, and key rotation.** The code is built and tested; nothing calls it yet. Also the "update automatically" setting.
3. **O2a: canary rollout.** Your devices update first; friends only after a waiting period.
4. **O2b: a known-good version** the fleet can fall back to.
5. **O2c: delta updates**, sending only changed bytes, with the full build as a fallback.
6. **O2d: the iOS re-signing reminder**, before the ad-hoc profile expires.

**Friends:**

7. **O3 (AK): Tor in the installer** (the Tor Project's expert bundle).
8. **O7 (AN): keep each friend's Tor connection open**, for speed.
9. **O6 (AM): notice a Tor block and switch to bridges** by itself.
10. **O5 (AL): test Arti** inside Atlas; switch when it holds up.
11. **O11 (AD): any device holding a verified release can pass it on.**
12. **O10 (AC): scanning a friend's QR code opens Atlas directly.**
13. **O9 (AJ): the installer adds the Windows firewall rule.**

**Phones (steps 7–9 of the build plan).** These can be written here, but each needs a real phone to prove:

14. Notification categories and their on/off switches.
15. Home-screen widgets.
16. The Android install hook, and iOS updates delivered from your own desktop.

**Waiting on you:**

- O8: watch for antivirus flagging `tor.exe` on first installs.
- **The signing sitting:** the Apple account, Azure signing, then `atlas release keygen`.
- **Tests only you can run:**
  - Tor between your laptop and your phone's hotspot;
  - the Windows local-time fix;
  - identity backup when you use separate profiles;
  - a toast from the hub.
- **A build toolchain on the laptop:** MinGW or Visual Studio Build Tools (`OPEN_GAPS.md` 1.1).

**Other chat's items, in `OPEN_GAPS.md`:**

- the hub brought in line with all 29 screens;
- the two lost passes of the first package;
- `contents::Contents`, which nothing produces yet;
- 247 functions reached only by tests.

---

## 10. The guard numbers, now

**Where the counts stand on `master`:**

| Count | Value |
|---|---|
| `MODULES_IN_TREE` | 376 |
| `UNCLAIMED_MAX` | 171 |
| `TEST_ONLY_MAX` | 251 |
| `HELPER_UNTESTED_MAX` | 4 |

Every change to these numbers has a dated note beside it in the source.

**Crates this chat added,** each justified in `tests/metrics.rs`:

- `ed25519-dalek`: release signing.
- `sha2`: file fingerprints. It was already compiled in.
- `curve25519-dalek`: agreeing a key with a friend. It was already compiled in.

---

## 11. How to check any of this yourself

```powershell
cd C:\Users\erics\Atlas\atlas-current
git log --oneline -25          # the ledger in §2
git show 03d7e7e               # any commit, in full
```

Every test and the full suite run in the cloud workspace, because Atlas can't be built on the laptop until the toolchain in §9 is installed. The byte-for-byte check runs anywhere with Git Bash:

```bash
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
```

---

## 12. Checks made for this handoff (26 Sep)

- **Your laptop matches the build:** every tracked source, test, config and document under `atlas/` on your laptop was checksummed against the build that was tested, and all 769 files matched.
- **Nothing deleted:** from the durable base (`4a1164a`) to `master`, `git diff --name-status` shows no deleted files. `portmap` and `mailbox` were added and removed inside that range, by your decision.
- **Every round branch is contained:** `round6` to `round11` are all inside `master`.
- **The project docs match:** the courier spec, the open-gaps register and the round-11 merge handoff in the claude.ai project match the repo copies.

*MEASUREMENTS. NO VERDICT. ERIC RULES.*
