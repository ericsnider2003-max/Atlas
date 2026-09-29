# Round 9: a working day, not just a conversation

**25 September 2026.** Eric: *"I want to make this a solid system that genuinely helps a work flow, look for anything that helps with that on GitHub or anything that would prevent it."*

## What the research said, and what it found in Atlas

The research came from three places:

- **GitHub:** ActivityWatch (MPL-2.0), an automatic time tracker, and chrono (MIT), a natural-language date parser.
- **Interruption science:** Horvitz, Apacible & Subramani on *bounded deferral* (2005); Iqbal & Bailey on breakpoint-timed notifications (CHI 2008); Trafton & Monk on resuming an interrupted task (2007); Mark's cost-of-interruption studies.
- **Windows' own interruption API:** `SHQueryUserNotificationState`.

Held against those, **Atlas had one blind spot under several failures.** It knew you were there only when you *spoke* to it. It had no idea whether the keyboard and mouse were in use, and no idea whether you were presenting. What that caused, all confirmed in the code:

| found | what it did to a working day |
|---|---|
| "idle" meant *since you last spoke to Atlas* | an offer could land while you were typing mid-sentence, 30 s after you last spoke |
| away meant *not speaking for 15 minutes* | twenty minutes of silent typing counted as away. Notes were held as if you'd gone, and after an hour **pushed to your phone while you sat at the desk**. Speaking after an hour's quiet work got a "welcome back" |
| nothing ticks while the laptop sleeps | the commonest break of all, closing the lid for lunch, passed without a word on your return |
| the away brief cut subjects at three words | "Atlas — your machine" came back as *"I've got an update on Atlas — your when you're ready"* |
| the calendar's time reader was deliberately narrow | "at 3" was **3 in the morning**; "in 20 minutes", "the 14th", "Oct 3", "noon" and "next Tuesday afternoon" weren't read at all |
| nothing checked the OS | Windows says when you're presenting or in a full-screen app, and Atlas never asked |

## What was built

### 1. Atlas can tell working from away (`platform`)

- `input_idle_secs()`: seconds since the last keyboard or mouse input, from `GetLastInputInfo`. The count wraps safely across Windows' 49.7-day tick counter.
- `quiet_state()`: what Windows says about interrupting you now, from `SHQueryUserNotificationState`. The answers are presentation mode, a full-screen app, a full-screen game, locked or away, and the first hour after sign-in.
- Both use crate features that were already on (`Win32_UI_Input_KeyboardAndMouse`, `Win32_UI_Shell`), so no new dependency. Linux and the phone say "can't tell", and everything behaves exactly as before there.

Wired into the daemon:

- **Presence** is now the shorter of "since you spoke" and "since you touched the keyboard or mouse" (`quiet_for`). This fixes the phone push and the false "welcome back".
- **A break** is keyboard silence past `away_after`, *or a gap in the ticks themselves* (the lid was shut).

### 2. Where the time went (`worklog`, new)

This uses ActivityWatch's model, clean-room.

- **How it records.** Every tick is a heartbeat of the focused app and window. A heartbeat identical to the last span, arriving within the pulse time, extends that span. A morning of 1,800 heartbeats became **7 spans**.
- **Away time.** Three minutes of keyboard silence (ActivityWatch's AFK default) counts as away. The span ends when the input stopped, not when Atlas noticed.
- **Categories.** Your rules come first, then built-ins for trading, meetings, chat, mail, coding, writing, sheets, design, video, files and browsing, then the app's own name.
- **Focus blocks.** A focus block is 25 minutes or more in one category. It survives glances under a minute and short keyboard pauses; a longer detour or break ends it.
- **Switches** count changes of category that lasted at least ten seconds.
- **Ask it.** "Where did my time go today?", "…yesterday", "…this week", or `atlas time [yesterday|week]`. It's an owner-only action, so a guest holding the laptop can't hear your day.
- **Private.** It stays on this machine and is never synced. Window titles go through `redact` first, so a key or card number in a title never reaches disk. `keep_titles: false` keeps apps and times only.
- **Saved on its own schedule:** every ten minutes and on shutdown, not every tick. That keeps Atlas within Microsoft's idle-energy guideline of one periodic write per ten minutes. At most ten minutes of the record can be lost to a hard crash.

### 3. Offers wait for a natural break (`proactive`, bounded deferral)

An offer that would land while you're typing mid-task now waits for a **breakpoint**:

- an app switch in the last 30 s;
- coming back to the keyboard after a minute or more away;
- a pause of 20 s or more.

It waits **never longer than `defer_max_secs`** (20 minutes). While Windows says you're presenting, in a full-screen app, away or in the first hour after sign-in, it waits with **no bound**. That's the one exception to bounded deferral, because a pop-up over a presentation can't be taken back.

Where the OS can't report input timing, nothing is held (the old behaviour). Urgent health alerts go their own way and aren't affected. The settings are `defer_while_working` and `defer_max_secs`, in `tools.yaml` under `proactive`.

### 4. Coming back, a cue for what you were in the middle of

When you come back from a break (keyboard silence past `away_after`, or the lid shut), the welcome ends with one line about what you were doing, for example: *"Before the break you'd been in coding for 1 h 30 min — last in "worklog.rs - atlas"."*

Trafton & Monk's review found that an explicit cue shortens getting back into a task, and a subtle one does no better than none. So the cue names the task outright. Switch it off with `worklog.resume_cue: false`.

### 5. Times the way people say them (`when`, new)

The design follows chrono (MIT, read as the reference, clean-room): small parsers for days, dates, clock times, offsets, ranges and lengths, then one step that combines what they found.

- **Coverage.** Casual days, weekdays with this/next/last, ordinals, month names in either order, ISO and US numeric dates, `3pm` / `3:30pm` / `15:00`, noon and midnight, "half past", "quarter to", offsets in words or digits ("an hour and a half", "a couple of hours"), "N units from now", ranges ("from 2 to 4pm", "2-4pm", "between 1 and 2"), lengths ("for 30 minutes"), and "end of day", "end of the week", "end of the month", "this weekend", "first thing".
- **A bare hour** follows chrono's rule: "at 3" means 3 pm, marked as guessed so it can be said back.
- **It isn't sure when two readings are real,** and says why:
  - "next Friday" said on a Wednesday (this Friday, or the one after?);
  - `3/4`;
  - "a few";
  - "next week" with no day;
  - "next weekend".
- **The calendar reads through it.** It takes only sure answers, and still asks when a time comes with no day ("at 5pm"). The same fix applies to recurring events: "every weekday at 7" is now 7 am, where it used to go through the old reader.

## Measured (tests/round9.rs)

| what | result |
|---|---|
| **`when`, 106 phrases** (`tests/when_corpus.rs`) | **106 right, 0 wrong, 0 declined**. The calendar's old reader on the same phrases: **28 right, 22 wrong, 56 unread** |
| `when`, **40 held-out phrases** written after tuning, first run | **38 right, 1 wrong, 1 declined**. The wrong one ("a week from today" read as today) was a general bug and is fixed. The decline (11/5, which is a real date either way) is by design. After: 39 / 0 / 1 |
| the calendar through `when` | 91 right, **0 wrong**, 15 asked (a time with no day, by design) |
| **deferral**, a synthetic 4.6-hour day of typing in bursts, app switches and two breaks, with an offer every 11 minutes | before: 24 offers said, **21 while you were typing**, no wait. Held for a break: 23 said, **4 while typing** (the ones that reached the 20-minute bound), mean wait **311 s**, longest **1,005 s** |
| deferral during an hour of presentation mode | **0 offers landed on it**; the longest wait 3,900 s (held past the bound, as designed) |
| a morning in the work log | every second accounted for, to one heartbeat. Focus blocks found: coding 50 min (survived a 40 s Slack glance), trading 45 min, then 27 min after a 3-minute news break. The mail span ended at the last input, not when away was noticed |
| **90 minutes of silent typing, then a question** | no "welcome back" (before: a welcome, and notes routed as if you'd left) |
| 40 minutes away, then back | *"Before the break you'd been in coding for 1 h 30 min — last in "worklog.rs - atlas"."* |
| **the lid shut for an hour** | *"Before the break you'd been in trading for 40 min — last in "EURUSD,M15 - MetaTrader"."* |
| "where did my time go today", as said to the daemon | *"1 h 30 min at the machine today: coding 1 h 30 min. Longest stretch of focus: 1 h 30 min on coding … You stayed on the one thing."* |
| the away brief's subject | "Atlas — your machine" is now *"an update on your machine"* |
| the real machine | Linux: "can't tell" (as designed). **Windows (le3o): input idle read as 1,077 s, interrupt state `Accepts`** |

**All of the deferral and work-log numbers come from synthetic traces.** Your real day will differ, and `atlas time` will show it once Atlas has run for a day.

## Honest limits

- **Breakpoints are coarse:** app switches, pauses, returns. Iqbal & Bailey detected finer ones (within an app) with trained models. Atlas doesn't.
- **Meetings in a browser tab** read as browsing unless you add a rule. Categories are word matches, not understanding.
- **"at 7" is 7 am and "at 6" is 6 pm** by chrono's rule. It's said back, but a 7 pm dinner needs "7pm" or "tonight at 7".
- **US date order** for numeric dates, and unsure whenever both readings are real.
- **Offers are still off by default** (`proactive.enabled: false`), as before. Deferral decides *when* an offer is said once you turn them on.

## Gate

| suite | result |
|---|---|
| personal Atlas, every debug target (lib, bins, 28 test targets) | merged with `d369458`: **6,473 passed, 3 failed** (two doc guards master itself failed, below, and one `{:?}` in the new `atlas time` line), all fixed. Then master moved to `5dda9b2` (courier step 3) and was merged too: **6,514 passed, 3 failed**, all guard bookkeeping (the catalogue to regenerate, a stale-doc baseline line the main chat's own spec edit had made unnecessary, and `yourchanges::diff` looking like a name collision only because this tree also has a `diff` module). Fixed, and those targets re-run green: **6,517 passed, 0 failed** |
| personal Atlas, release voice (`voice_measured`, `vad_measured`) | **8 passed, 0 failed** |
| *[row removed 28 Sep 2026: trading-system material]* |
| **Windows, natively on le3o** (cross-built `all.exe`, run from `.r9test`) | **8/8 round-9 tests pass.** The real machine answered: seconds since input `Some(1077)`, Windows' interrupt state `Accepts` |
| *[row removed 28 Sep 2026: trading-system material]* |

**Found on master:** `d369458` (the main chat's update-courier steps 1–2) is red on two documentation guards. `UPDATE_COURIER_SPEC.md` was neither on the current-documents list nor marked stale, and it says "Tier 2 — sandboxed code (only if a real need appears)", which `bug_sweep` reads as a doc underselling Atlas. Fixed on `round9` by listing the spec as current, treating its build log like a handoff (its test counts are per-step records), and adding that line to the stale-doc baseline (taken out again once `5dda9b2` reworded it). The spec itself was not touched here.
