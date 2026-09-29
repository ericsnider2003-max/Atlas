# Round 10: the gaps a second look found, closed, and what to build next

**25 September 2026.** Eric: *"Fix the gaps, look for more gaps. Give me ideas for new capabilities."*

Branch **`round10`**, on top of `round9`, with master merged in up to `773454a` (update courier steps 4–6, which landed during this round). It fast-forwards master.

## 1. Looking for more gaps

A read-only audit went through round 9's code paths (`when`, `worklog`, `awareness`, `proactive`, the calendar, reminders), then across the tree: config keys, intent phrases, panics on the request path. It traced **18 real problems**. Each one was confirmed by following the code.

The worst were silent. A time was booked **sure and wrong**, and the reply didn't say the time, so nobody would notice.

| # | found | what it did |
|---|---|---|
| 1 | the booking reply looked up "the next event", not the one just added | with anything booked earlier, the reply said *"On the calendar: "dentist", ."*, with no time at all |
| 2 | a range start took the end's pm even past the end | "11-1pm" became 23:00; "10-12pm" became 22:00 |
| 3 | an offset of days was dropped when the sentence held "at" | "remind me in 3 days at 5" fired **today** at 5 |
| 4 | "tonight at 1" / "tonight at 12" | read as 13:00 and 12:00 today, already past, so the reminder fired at once |
| 5 | "2pm-4pm" | only the day survived, and it became an all-day event |
| 6 | the `improve` (project work) phrases included "on the", "change the", "fix the", "add to", "increase" | "change the volume" or "add to my shopping list" was asked *"Which project is this for?"* |
| 7 | a reminder with an unsure time | set nothing and said "Reminder: X" as if it had |
| 8 | a meeting or a video with no typing | counted as away: a 40-minute call became 3 minutes of "meetings", then "Before the break…" |
| 9 | a long answer or render stalls the ticks | read as the lid shut: a false break and a false welcome |
| 10 | "quarter to one" | 00:45 |
| 11 | a past time | booked without a word |
| 12 | "for 2 hours, remind me an hour before" | the reminder was set 2 hours before |
| 13 | the 20-minute cap on held offers | never fired in the daemon (the gate is asked several times a tick and the second ask reset it); a presentation's wait also carried over, so the first keystroke after one let an offer straight through |
| 14 | "how long was I on YouTube?" | gave the whole-day report |
| 15 | "set a reminder for tomorrow at 9 to call mom" | made a calendar event called "a reminder" and lost "call mom" |
| 16 | "İlkay proposed…" | lower-casing "İ" adds a byte, so a slice could land inside a character, and the conversation path isn't caught: **the process exited** |
| 17 | the clock set back | the focus report could underflow (a panic in debug builds, a nonsense figure in release) |
| 18 | "in 2 months" was 60 days; "tomorrow" said at 00:30 | Jan 31 + 2 months landed on Apr 1; after midnight "tomorrow" skipped a day |

Checked and clean: every config key has a reader; no phrase belongs to two intents; there's no `todo!`/`unimplemented!` in shipped code; and the work log's day boundaries, pruning and closing on shutdown are right.

**Found while fixing (a 19th):** `ports_live::recur_calendar_the_last_friday_of_every_month` failed every last Friday of a month after 16:00 UTC. It booked against the wall clock and counted from a fixed Wednesday. It happened to be 16:47 on the last Friday of September when it ran.

## 2. What was fixed

All 19. Each has a test in `tests/round10.rs` that fails on round 9.

- **Times** (`when`):
  - Ranges split on the dash before am/pm, including en dashes.
  - A range start takes the end's pm only if it stays before the end.
  - Day-sized offsets always set the day, and months are calendar months, clamped (Jan 31 + 1 month is Feb 28).
  - "Tonight": 12 is midnight and 1–4 are the small hours after it.
  - "Quarter to one" gets its am/pm before the quarter is taken off.
  - A time already past is asked about, unless the words look back ("yesterday", "last", "ago"), or the event is still running.
  - "Tomorrow" after midnight is asked about.
- **Bookings:**
  - The reply names the time it booked (`Calendar::event(id)`).
  - The title leaves out the time however it was said: "dentist in 3 days at 4pm" is "dentist".
  - A meeting's length is kept apart from its reminder lead.
- **Reminders:**
  - Said back with the time: *"Set — on Mon 2026-09-28 at 17:00 I'll remind you to call mom."*
  - An unsure time is asked about, giving the reason ("is 3/4 month/day or day/month?").
  - A day with no hour is asked about too ("what time on 2026-09-28?").
  - "Set a reminder for…" is a reminder, not a calendar event.
- **Project work:** a general phrase ("fix the", "change the", "on the") reaches `improve` only when a project is named ("project", or one of your registered projects). "Improve", "refactor" and "work on the" always do.
- **Presence:**
  - A call or a video (by the window's category), or Windows saying a full-screen app or a presentation has the screen, isn't away for up to 2 hours of input silence (`worklog::effective_idle`).
  - The same rule stops notes being pushed to your phone mid-meeting.
  - A long turn or tick marks when Atlas was busy (`done_working`), so the break check measures from there.
- **Held offers:** one answer per tick, so the 20-minute bound fires. Measured: an hour of steady typing gives the first chance at 1,200 s, then one more.
- **"How long was I on X":** time on that one thing: a category, an app, or a word in the window titles.
- **Crashes:**
  - The conversation path and the hub are now caught as the tick is. A panic costs that exchange and a sentence, not the process.
  - Eight places that cut the original text at a position found in its lower-cased copy now lower-case ASCII only, which keeps every byte where it was.
- **Clock set back:** the work log waits for the clock to pass where its record already reaches. The report's arithmetic saturates.

## 3. The open gaps that could be closed here

| gap | what was built | measured |
|---|---|---|
| **5.4 breakpoints are coarse** | A natural pause is learned **per app** from your own pauses (`WorkLog::note_pause` / `pause_thresholds`). The threshold is longer than 9 in 10 of that app's pauses, kept between 8 and 90 s, and trusted after 80 pauses. Until then, the fixed 20 s stands. | A synthetic day in three apps (an editor, a trading terminal where watching a chart isn't a break, mail), learned on one day and judged on another (762 pauses). Fixed 20 s: precision 0.27, **197 false breaks**. Learned: thresholds editor 25 s, terminal 68 s, mail 17 s; precision 0.87, **11 false breaks**; recall 1.00 both ways. |
| **3.9 hidden passphrase on Windows** | The console's own input (`CONIN$`), with `ENABLE_ECHO_INPUT` cleared via `GetConsoleMode`/`SetConsoleMode`, read with `ReadConsoleW` and zeroed after. The mode comes back on drop. Nothing leaves the process. PowerShell's `Read-Host` is now only the fallback when there's no console. `atlas doctor` reports which is in use, and really switches the echo off and on to find out. | See §Gate (run natively on le3o) |
| **§6 animation has no refine** | "Make it faster / slower / twice as fast / a bit slower", "make it red / bluer", "bigger / smaller", said within the hour after an animation: an in-house edit of that SVG (`motion::refine`). It scales every SMIL and CSS time, swaps the main colour (never the background), and scales the canvas with its `viewBox`. The result is checked again and saved beside the original as `.v2`, `.v3`. No model. | *"Made it faster (3 s → 1.5 s), red (was #1e88e5). Saved to …/animation.v2.svg (the one before is still there). It still checks out: renders, right size, right length."* |
| **5.3 the denoiser's gain is modest** (round 8's next idea: adaptive sampling) | More rays where a pixel's first rays disagree, stratified on a grid twice as fine. **Off unless `"adaptive": true`**, because of what it measured. | See the table below |

**Adaptive sampling measured** (swept 0.02–0.5, against a 64-ray reference). It gains a little on noisy light and loses on fine patterns:

| scene | uniform | adaptive (0.08) |
|---|---|---|
| night and a lamp (round 8's) | draft 4 rays → 3.31 levels off; good 9 → 2.17; best 16 → 1.56 | 5.3 rays → 2.64; uniform at that cost ≈ 2.85, so about 7% better |
| held out: daylight, checker floor, glass | draft 4 → 5.9; good 9 → 3.41; best 16 → 2.37 | 9.3 rays → 4.75; uniform at that cost ≈ 3.34, so **worse** |

Why it loses: a distant checker's pixel is wrong while its few rays agree, so nothing flags it. Round 8's claim that adaptive sampling "would beat any filter here" is **not borne out** as built. The next idea would be to flag a pixel by how much it differs from its neighbours, which would catch aliasing.

## 4. New capability ideas

Researched on GitHub and in the literature. Ranked by everyday value against build cost. All are in-house and offline-first. References with GPL/AGPL or commercial licences are **for ideas only**; the licence is named each time.

| # | idea | what it does for you | reference (licence) | how, in house | reuses | size |
|---|---|---|---|---|---|---|
| 1 | **Clipboard history** | Everything you copy, searchable, pasted again with a hotkey; tagged by the app it came from | Ditto (GPL-3, ideas only) | `AddClipboardFormatListener` → `WM_CLIPBOARDUPDATE`. **Skips password-manager copies** that set `ExcludeClipboardContentFromMonitorProcessing` / `CanIncludeInClipboardHistory=0`. Encrypted, time-limited | vault, BM25, typos, sealed log | S |
| 2 | **Copy text from anything on screen** | Drag a box over a chart label, an error dialog or a video frame; the text goes to the clipboard | PowerToys Text Extractor (MIT) | `Windows.Media.Ocr`: on-device, 25 languages, returns words with positions; capture with `BitBlt`/Graphics.Capture | pngcodec, clipboard (#1) | S |
| 3 | **Market-day awareness** | NYSE holidays and 1 pm early closes, BLS releases at 8:30 ET, FOMC days, on the calendar and in the morning brief: *"CPI at 8:30, FOMC tomorrow."* **Schedule only, no interpretation** | BLS `.ics` feed; Fed and NYSE published calendars | Fetched once a year, then offline | RFC 5545, calendar, brief | S |
| 4 | **Waiting-for tracker** | Threads where you're owed a reply after N days, and your own "I'll send it Friday" promises still open | Microsoft Research on commitment detection (it doesn't carry across mailboxes, so it learns from your confirm/dismiss) | Rules plus the local model over Sent and Inbox | IMAP, mailthread, reminders, deferral | M |
| 5 | **One-step capture** | A global hotkey or the wake phrase drops a thought into an inbox, with times parsed; a weekly review sorts it into reminder, event, task or note | GTD's capture step; self-tracking research: people lapse from upkeep, so capture must be one step | `RegisterHotKey` and a small input box | `when`, reminders, voice | S |
| 6 | **Launcher in the hub** | Alt+Space: apps, files and Atlas actions in one box ("workspace trading", "remind me…") | PowerToys Run (MIT), Flow Launcher | Launching is easy; ranking is the work | BM25, typos, workspace_on | M |
| *[row removed 28 Sep 2026: trading-system material]* |
| 8 | **Meeting prep** | 15 minutes before an event: the latest threads with the people in it, last meeting's notes, open promises (#4) | — | Assembled locally, said at a breakpoint | calendar, mailthread, diarized notes, deferral | M |
| 9 | **Snippets** | `;addr`, `;sig`, `;date` expand anywhere | Espanso (GPL-3, ideas only) | `WH_KEYBOARD_LL` plus `SendInput`. A keystroke hook is a keylogger by construction: a small in-memory buffer only, never logged. Can't type into admin windows (UIPI), and says so | vault, clipboard | M |
| 10 | **Find any file** | Filename search across drives in a blink; contents later | Everything (NTFS USN journal), Recoll | USN journal needs admin or a service; text out of PDF/DOCX is what makes it large | BM25, stemmer, chunker, filing | M/L |
| 11 | **PDF tools** | Merge, split, reorder, stamp a signature image | lopdf (MIT) as a reference | Object renumbering; malformed files | filing, vault | M |
| 12 | **Personal CRM** | Last contact per person (from mail), birthdays, "reach out every N weeks" | Monica (AGPL, ideas only) | Local and encrypted: it holds data about other people | IMAP, recur, reminders | M |
| 13 | **Read-later and feeds** | Readable offline copies with trackers stripped | Miniflux (Apache-2.0), Wallabag | Fetch online, read offline | readable, BM25, phone sync | M |
| 14 | **Receipts** | Photograph on the phone; vendor, date and total pulled out; filed by month | Tesseract (Apache-2.0) or Windows OCR (#2) | OCR plus the local model | phone sync, filing | M |
| 15 | **A forgiving habit tracker** | A strength that decays slowly, so one missed day isn't a reset; pauses (holidays) built in | Loop Habit Tracker (GPL-3, ideas only) | Small | reminders, worklog | S |
| 16 | **Spaced repetition** | Notes or vocabulary as review cards, scheduled by FSRS | fsrs-rs (BSD-3) | The algorithm is small and published | reminders, chunker | S/M |
| 17 | **Local translation** | Private translation of pages and mail | Mozilla Bergamot models (MPL-2.0), ~40 MB a language pair | Marian inference is the large part | local model runtime | L |
| — | *Deferred:* screen recall (Recall, Screenpipe, OpenRecall) | — | Screenpipe went source-available in 2026; OpenRecall is AGPL with encryption still "planned"; Windows Recall was shown capturing card numbers | The work log already gives most of the value. At most, opt-in OCR for an allow-listed app | — | — |

**What would stop these helping** (from the same research, and why round 9's deferral matters):

- **Interruptions cost more than they save.** After an email alert, getting back into the task averaged about 16 minutes (Iqbal & Horvitz, CHI 2007). Every new capability should go through bounded deferral, not pop up on its own.
- **Capture friction and upkeep make people stop.** Keep capture to one step, fill in from what Atlas already sees, and make pausing a feature.
- **Secrets leak into capture tools.** Honour password managers' clipboard exclusions; keep any keystroke buffer in memory only.
- **Windows privilege boundaries break automation** (UIPI): say when an action can't reach an admin window, rather than failing quietly.
- **Smart detection doesn't transfer** between mailboxes: build in confirm/dismiss feedback from the start.

My pick for round 11: **#1, #2, #3 and #5**. They're small and offline, they reuse what's there, and they meet a trader's working day at its commonest moments: copying, reading off the screen, the market calendar, and catching a thought before it's lost.

## Gate

| suite | result |
|---|---|
| personal Atlas, every debug target (lib, bins, 28 test targets) | full run: **6,527 passed, 7 failed**. All 7 were guard or test bookkeeping from this round's own changes: two calendar unit tests said at exactly midnight, where "tomorrow" is now asked about; a test with no assertion; a test that only checked wording; the general `improve` phrases, which now need a project named; and one helper made `pub` without need. All fixed, and the failing targets (`--lib`, `all`, `dead_capabilities`, `name_collisions`, `new_capabilities_are_wired`, `guards`) re-run: **6,233 passed, 0 failed** |
| **after merging master `6b7e938`** (courier steps 4–5, which landed during the round) | full run on the merged tree: **6,475 passed, 2 failed**. The first failure: the main chat's new add-on permission table didn't decide two intents this chat had added (`time_spent`, now NEVER for add-ons, because it reads out your day; and `scene3d`, now under "making" beside `animate`). The second was `CAPABILITIES.md` to regenerate. Both fixed, and re-run green |
| *[row removed 28 Sep 2026: trading-system material]* |
| personal Atlas, release voice (`voice_measured`, `vad_measured`) | **8 passed, 0 failed** |
| adaptive sampling (release) | measured as in §3; the test holds both the win and the loss |
| *[row removed 28 Sep 2026: trading-system material]* |
| **Windows, natively on le3o** (cross-built `all.exe`) | **25/25 round-10 and round-9 tests pass.** The typed passphrase: *"in this process: the console's echo is switched off while you type"*, so the in-process read works on the real console. The animation refine also filmed a GIF there (36 frames) through Edge. The machine reported input idle 31,018 s and Windows' state `Away`, since you weren't at it |
| *[row removed 28 Sep 2026: trading-system material]* |

---

*MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE*
