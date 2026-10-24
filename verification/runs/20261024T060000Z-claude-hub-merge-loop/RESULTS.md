# Hub redesign merged; Hub pages no longer wait on the main loop: 2026-10-24 (Claude, usage-max routed)

Branch `merge/hub-redesign-2026-10-23` = candidate `725d0f5c` + `feat/hub-redesign-2026-10-22` (`148d3722`, merged with no textual conflicts), then fd90e9ec, 89a937da, 156736cc, 4392f90f.

## Merge
- Clean merge, no semantic conflicts (module count, regenerated docs, names).
- Seven tests were stale against the redesign (linked stylesheet, eight-link sidebar, tab counts); the tests were fixed, not the code.
- Item-6 behaviour survived the single work record: needs-you lane, failures stay until dismissed, dropping an approval cancels it (tests `a_failed_job_can_be_dismissed_and_stays_off_the_page`, `a_dropped_approval_does_not_come_back_after_persist`, `running_and_failed_work_agree_on_every_surface`).

## Main-loop blocking (root cause and fix)
- Only Home, Now and Outstanding had a published copy, so Settings, Help, Permissions, Trusted, live.json and changed.json queued behind the loop's current tick. Every request the loop handled, even a plain view, also threw the copy away.
- Settings re-read every config file on each draw.
- Fix: those pages and both JSON endpoints are published each pass and served from the copy. Only a request that can change something discards it. Settings re-reads config only when a file's name, size or modified time changes.
- One-time notices (finished-job answers, flashes) keep a page on the live path until said once. The 24 h set-aside banner is built into the copy.
- Not on the copy (still via the loop): Sound, Messages, Connections, Updates, anything with a query.
- Debug build: `build.rs` sets an 8 MiB main stack (MSVC bins only). `tests/the_hub_settings_page_in_a_debug_daemon.rs` fails without it.

## Measured
Disposable home, release daemon, 12 threads keeping the loop busy, 60 requests per page (p50 / p95 / max ms):

| Page | Before | After |
|---|---|---|
| /hub | 26.6 / 49.7 / 53.8 | 1.7 / 21.5 / 26.0 |
| /hub/settings | 33.8 / 53.2 / 56.8 | 3.3 / 23.5 / 26.0 |
| /hub/live.json | 25.0 / 45.5 / 73.8 | 1.5 / 16.7 / 25.0 |

Real profile, installed `988E068A...`, 40 requests per page, 250 ms apart (`evidence/hubtime-new.json`). The log shows real ticks of up to 448 ms in the same session:

| Page | p50 / p95 / max ms |
|---|---|
| /hub | 3.9 / 6.0 / 23.7 |
| /hub/now | 4.1 / 5.4 / 14.6 |
| /hub/outstanding | 2.9 / 4.2 / 4.6 |
| /hub/settings | 9.8 / 24.9 / 25.2 |
| /hub/live.json | 2.3 / 3.6 / 4.4 |
| /hub/sound (via loop) | 5.7 / 8.5 / 25.9 |

The redesign session had reported occasional 1.0-1.6 s loads. No old-build run on the real profile in the same session, so this is not a same-session before/after.

## Checks
- Review: usage-max reviewer found one defect: the status-page copies skipped one-time notices, including the set-aside banner. Fixed in 4392f90f with `the_three_status_pages_copies_carry_what_the_live_pages_say`.
- Full `--all-targets` on 89a937da: 9,356 passed, 7 failed. 3 env (camera/ffmpeg); 1 timing (6/6 alone); 3 stale tests, fixed.
- After 4392f90f: Hub/server/status/one-truth/guard filters 511 passed; lib hub/hublive/server 80; `hub_settings` 39; debug-daemon test 1. No full run after 4392f90f.
- Installed `988E068A...`, rollback `atlas-0.1.0-dev.previous` = `487512C3...`. Verifier 14/14 first run.

## Not verified
- The new notice test was not run against the old code to show it fails.
- Cold first view and first view after a change still go through the loop.
- The redesign session's 300 ms Settings build was not reproduced (9-13 ms before here).
- A real-profile stop took about 38 s once, while memory was nearly full (normally about 2 s); not investigated.
