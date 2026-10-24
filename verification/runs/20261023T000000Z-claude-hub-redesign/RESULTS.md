# Hub redesign — results (23 Oct 2026, Claude)

- Branch: `feat/hub-redesign-2026-10-22` (worktree `wt-hub`), on top of candidate e1957f45.
- Plan and brief: `docs/HUB_REDESIGN_2026-10-22.md`.
- Phone decision record: `docs/PLATFORM.md`.
- Final build: release `atlas.exe` at bcb7a8b8, SHA-256 prefix 3358ecc3b69b69d7 (in `wt-hub/atlas/target/release`).
- Nothing is installed. The installed build and the candidate branch are unchanged; installing waits for Dispatch to sequence it with the reliability session.

## Method

- **Same profile before and after.** One disposable ATLAS_HOME (`hub-measure` profile), with models and tools linked to the installed copies. Before: the installed release (A5656D40, 22 Oct). After: this branch's release build.
- **Phone profiles.** Headless Edge, 360 px wide, with the service worker bypassed so the throttling applies.
  - Slow 3G: 400 ms round trip, 50 KB/s, CPU 6× slower.
  - Slow 4G: 150 ms round trip, 200 KB/s, CPU 4× slower.
- **Script:** `phonebench.mjs`, kept in the session scratchpad.
- **Accessibility:** barrier-check `render-check` (1280 px and 320 px, light and dark, forced colours) and `announce --axe`, signed in. Playwright 1.55.0 (Apache-2.0) and axe-core 4.10.3 (MPL-2.0) drove the installed Edge. They are test tools only and do not ship.

## Before → after

| | Before | After |
|---|---|---|
| Home over the wire | 95.6 KB (uncompressed, stylesheet inline) | 8.4 KB; the stylesheet is 15.7 KB, sent once and cached for a year |
| Settings over the wire | 169 KB | 21.4 KB |
| Settings height on a phone | 23,676 px (about 32 screens) | 2,002 px (areas open one at a time; cards no longer repeat the tabs) |
| Home, slow 3G: first paint / loaded | 1,816 / 2,406 ms | 516–728 / 591–647 ms over three runs |
| Settings, slow 3G: first paint / loaded | 1,984 / 4,045 ms | 536–588 / 869–1,039 ms typical; 1,460 / 2,260 ms once, when Atlas was busy |
| Home, slow 4G: first paint | 544 ms | 236–364 ms |
| Sidebar links | about 70, plus a 60-link phone menu | 8 (Search and the six sections) |
| Status on the three status pages | three different stories | one work record; a test fails the build if two pages disagree |

### Budget check

- **Under 30 KB: PASS** on every page. Settings is the largest at 21.4 KB. A first visit adds the 15.7 KB stylesheet once.
- **First view under 1 s on slow 3G: PASS most of the time, with one condition.** Most first paints land at 0.5–0.75 s. The outliers (1.0–1.6 s) match Atlas's own log, where the request "waited 438–579 ms" for Atlas's main loop, and Settings takes about 300 ms to build. Page weight is not the cause. The loop wait is the remaining work, and it belongs with the reliability lane ("never wait in a queue").

## What changed (commits)

1. `137f3406`: `docs/PLATFORM.md` rewritten for a standalone-first phone (owner decision, 22 Oct). The old record is archived unchanged.
2. `6fbe1a7c`: one work record (`hub::Open`) with a Queued lane, read by Home, Now, Outstanding, the header count and `live.json`. Adds `atlas/tests/the_status_is_one_truth.rs`.
3. `20c16c26`: six sections with tabs, Home led by live work lanes, a cached gzip stylesheet, gzip pages, plain-words busy note, and accessibility fixes.
4. `0743cf34`, `c55533fd`: fixes from two independent reviews.
   - Business tabs keep `?b=`.
   - Live updates trust the version of the page they fetched.
   - Running work is named before queued work.
   - One build of the record per page (seven before).
   - The compressed stylesheet is built once.
   - `gzip;q=0` is honoured.
   - A panic while drawing can no longer leave a stale record behind.
5. `9ee4c60c`: Settings areas open one at a time, with no script.
6. `2e6d6195`, `bcb7a8b8`: self-review fixes.
   - Today lists things done, not things said.
   - No "Ctrl K" hint on a phone.
   - No Settings cards that repeat the tabs.

## Gates

| Gate | Result |
|---|---|
| One-truth tests (4) | PASS |
| Hub, server and status suites (`hublive` 61, `the_phone_stands_alone` 15, `hub_navigation` 24, `the_hub_design_is_the_locked_one` 18, `server_safety` 38, `getting_things_done` 29, `mind_hub` 31, `access_and_settings` 22, lib hub/server 73, and others) | PASS |
| Project checks (`dead_methods`, `dead_capabilities`, `new_capabilities_are_wired`, `no_silent_discards`, `no_quiet_nothings`, `asks_are_specific`, `personal_atlas_is_its_own`) | PASS |
| Compressed-reply tests (they catch the header bug that once sent unstyled pages) | PASS |
| barrier-check, rendered: 8 pages × 5 configurations | PASS, 0 findings. BC-01 (focus) and BC-02 (target size) were fixed and verified by the same check. |
| axe-core and the accessibility tree: 8 pages | PASS, 0 violations |
| barrier-check report `verify-report` | PASS. Verdict: "PASS (automated + code review), AT test pending". |
| Real screen reader | NOT RUN. No one listened with a screen reader. |
| Real phone | NOT RUN. Edge emulation only. |
| Independent review (usage-max reviewer, 2 passes) | 7 defects found, all fixed, each with a test where a script can check it. |
| Full all-targets campaign | NOT RUN. Only the targets above were run. |

## Self-critique against the brief (Jarvis, EDITH and Donna)

- **It already knows:** met for work status. The first screen shows needs you, failed, working, queued and done, live. It is not yet proactive beyond status. Anticipating needs is product work outside this redesign.
- **It never contradicts itself:** met and enforced by tests.
- **It is honest about time:** met on the laptop. The phone's "queued until the laptop is reachable" state is designed (`docs/PLATFORM.md`), but the outbox is not built yet.
- **It is quick on a bad phone:** met on weight. Speed is met except when Atlas's main loop is busy; that is named above with evidence.
- **It is discreet:** improved.
  - The busy notice is gone when the view is fresh.
  - Empty lanes take one line.
  - Spoken notices stay off Today.
  - The hidden Ctrl-K panel still adds about 4 KB and 50 links to each page. It is a desktop feature, not shown until opened.

## Open, with owners

1. The main-loop wait (0.4–0.6 s) and the 300 ms Settings build. Owner: the reliability lane.
2. The standalone phone: model on install, an outbox to the laptop, sync over the private network, sync visible on the phone. Listed in `docs/PLATFORM.md`.
3. A development build overflows its stack on `/hub/settings`. This predates the redesign; release builds are fine. It is offered as its own task.
4. A 5-minute check with a real screen reader and a real phone (barrier-check report, "5-minute check").
5. Merge and install: Dispatch to sequence with the reliability session.
