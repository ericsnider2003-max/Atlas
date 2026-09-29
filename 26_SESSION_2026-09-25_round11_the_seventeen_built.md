# Round 11: the seventeen ideas, built

**25 September 2026.** Eric: *"Ok implement the 17 new ideas in. Look for ways to improve them and make them soundproof without deteriorating the efficiency and effectiveness of the system."*

Branch **`round11`**, on top of `round10`, with master merged in up to `24bf4b4` (friends with no Tailscale between people). It fast-forwards master.

All seventeen are built, wired to the daemon, saved where they keep anything, and tested end to end through `Daemon::turn`. What each one needs from you before it does anything on your laptop is in §5.

## 1. What was built

Each tool lives in its own module and knows nothing of the daemon. One new module, `workday`, joins them to it: the sentences, the saving, the tick, and the brief.

| # | idea | module | say | what makes it sound |
|---|---|---|---|---|
| 1 | clipboard history | `cliphist` | "turn on clipboard history", "what did I copy", "paste 2" | **Off until you turn it on.** Kept **in memory only**, never on disk. The clipboard is read only when Windows' sequence number moves. It skips copies a password manager marks private (`ExcludeClipboardContentFromMonitorProcessing`, `CanIncludeInClipboardHistory`=0, "Clipboard Viewer Ignore"), anything that looks like a key or a card number, and apps on a never list (KeePass, 1Password, Bitwarden…). Forgotten after 24 hours. |
| 2 | copy text off the screen | `screentext` + `platform::win` | "copy the text off the screen", "copy the text from the total" | Windows' own recognizer (`Windows.Media.Ocr`), on this machine. Noise isn't passed off as text. Asking for one thing ("the total") keeps that line and the one after it. Something secret-looking on screen is warned about before you paste it. |
| 3 | market-day awareness | `marketdays` | "is the market open", "next market holiday" | NYSE holidays and early closes worked out by rule (Easter by computus, weekend shifts, Juneteenth from 2022). **Checked against NYSE's published list through 2028.** CPI dates for 2026 come from BLS; past that it says the table ran out rather than guessing. It says a schedule, never a lean (tested). |
| 4 | waiting-for tracker | `waitingfor`, `mailbook` | "what am I waiting on", "what did I promise", "done 2", "not a promise 3" | Reads mail **you sent** (a new Sent-folder read in the mail check, RFC 6154 `\Sent`). Quoted history is cut off, so someone else's question in a reply isn't yours. A reply from them closes it. A phrasing you call wrong three times stops counting. Promises with a day ("I'll send it Monday") are due that day. |
| 5 | one-step capture | `capture`, `chords` | "note that…", or **Ctrl+Alt+Space** on anything selected | A note that says a sure time is **dated**, and comes up in that day's brief. "Review my notes" goes through the week's notes: "keep 1", "drop 2", "keep all". |
| 6 | launcher | `launcher` | "open spotify", "start up visual studio" | Any Start-menu shortcut, matched by prefix, word, initials ("vsc") or one typo. It opens only a clear winner and asks when two are close. What you pick rises (a 7-day half-life). **An app not in `apps.yaml` still goes through the grants gate first.** |
| 7 | trading-session prompts | `tradeday` | "trading check in", "how has my trading process been" | Asked on its own 30 minutes before the open and 5 minutes after the close, **on trading days only** (early closes at 1:00), once each. It asks about you and your process, never a position. The summary counts and ends with *MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE*. A stray sentence isn't recorded as an answer. |
| 8 | meeting prep | `meetprep` | "prep me for my next meeting", or offered 15 minutes before | Who's in it (read from the title and notes, because calendar events here carry no attendee list), what you last wrote each other, what's open between you, your notes about them. It's offered only when it can name someone, and at most once per meeting. |
| 9 | snippets | `snippets`, `chords` | "save snippet ;sig as Best, Eric", "type my address", or **Ctrl+Alt+E** after typing `;sig` | **No keylogger.** Expansion happens only on request: the chord selects the word before the cursor, reads it through the clipboard, puts your clipboard back, and replaces the word only if it is exactly one of your triggers. It won't type into an app on the dictation never-list. A snippet holding a secret is refused. `{date}` `{time}` `{weekday}` `{year}` are filled in. |
| 10 | find any file | `findfile` | "find the pdf from yesterday about taxes", "open 2" | Type and date are read out of the sentence, and the rest is the name. With no exact match, near names are listed as **"closest" and never opened.** "Open 2" means the list you were just shown, for ten minutes. |
| 11 | PDF tools | `pdfkit` | "merge the pdfs 1 and 2", "extract pages 2-5 from 1", "sign 1" | An in-house reader and writer: classic and object-stream files, the page tree with inherited boxes. **An encrypted file is refused by name.** Every file written is re-parsed and checked before it's saved, **beside the original, never over it**. The signature is your PNG, with its transparency kept. |
| 12 | personal CRM | `people` | "remember Sam's daughter is called Leo", "keep in touch with Priya every month", "Sam's email is…", "who should I catch up with" | A person comes into being the first time you name them. "Last in touch" comes from the mail cache, not from you logging calls. **A first name that could be two people is asked about, never guessed.** Notes with secrets are refused. "I called the bank" isn't filed as a person. |
| 13 | RSS / read-later | `feeds` | "follow theverge.com", "my feeds", "read 1", "save 2" | RSS 2.0, RSS 1.0 and Atom, with the feed found from a page's `<link rel=alternate>`. Tracking parameters are stripped from every link, and `javascript:` links are never kept. **A first read lists only three**, not the archive. A failing feed backs off to once a day and says so. Redirects stop at three hops and never go from https down to http. Feeds are read one at a time on a background thread, so the tick never waits. "Save" puts the link in the tray. |
| 14 | receipts | `receipts` | "keep this receipt" (on screen, or its text copied), "what did I spend at Costco this month" | Merchant, date and total. The total is taken only from a labelled line (SUBTOTAL, TOTAL SAVINGS, CHANGE and TENDERED are traps it skips). An unlabelled total is **asked about** ("probably $7.75, keep it?"). If subtotal plus tax doesn't make the total, it says so. A full card number is scrubbed. The same receipt kept twice counts once. Currencies are summed separately. |
| 15 | habit tracker | `habits` | "new habit: read 20 minutes, 5 times a week", "did my reading", "pause my habits until Monday" | Loop's **strength** score: one miss dents it, it doesn't zero it. "3 a week" is met by 3 in 7 days. Pauses leave the score where it was. **A habit about a body number (weight, calories, a dose) is tracked but never brought up.** |
| 16 | spaced repetition | `srs` | "make a card: capital of Peru \| Lima", "quiz me", then again / hard / good / easy | **FSRS-5** with its published default parameters; a first "good" comes back in 3 days. A missed week comes back as 20 cards, not a wall. Intervals are capped at 18 months. |
| 17 | local translation | `translation` | "translate this into Spanish" (what you copied), "translate good morning to French" | Your local model does it. **Checked**: every number, link and address must come through; the length must be plausible; an echo or an answer instead of a translation is flagged. Short texts are translated back and compared. Long texts are cut at sentence ends. |

**Key chords** (`chords`): Ctrl+Alt+Space captures a selection, Ctrl+Alt+E expands a snippet, and Ctrl+Alt+T copies screen text. They use Windows' `RegisterHotKey`, which is told about one exact chord and can't see typing. They're **off until you turn them on** (`workday.chords.enabled`). A chord another program already owns is reported, not swallowed. Win+letter is refused, because Windows keeps those.

## 2. How they're joined (`workday`)

- **Sentences.** Seventeen new intents run through the full path: parser, policy gate, guest profiles, add-on permissions and the handler. Nothing bypasses the gate.
  - Sentences with an unmistakable shape are read before the phrase table (`workday::read_first`). These include "keep in touch with Priya every month", a follow-up to a list ("open 2", "done 1", "good"), and "follow theverge.com".
  - "I called Sam" and "did my stretch" are taken only if Sam, or the stretch habit, already exists.
  - It costs **3.6 µs a sentence** in a debug build (`round11::reading_every_sentence_first_costs_next_to_nothing`).
- **Lists are read whole.** Atlas shapes speech to two sentences. That would cut "what am I waiting on" to its first two lines and drop the trading line entirely, so these seventeen replies keep their length (`workday::reads_whole`).
- **A yes or a no** after "keep it?" goes to the receipt, not to "Nothing to confirm".
- **Unknown app names** ("open spotify") go to the launcher after the grants gate.
- **The brief** now has your day in it (a new `Source::Day`):
  - promises due, and replies you're owed
  - notes dated today
  - people you meant to be in touch with, and birthdays
  - habits due, and cards due
  - the market's calendar, once you've done a trading check-in (or `market_in_brief: true`)

  A clean install's brief is still empty.
- **The tick.**
  - Clipboard history reads one sequence number per tick, only when it's on.
  - Feeds are read on a background thread, one at a time.
  - Meeting prep and the trading check-in look at the clock once a minute, and speak only when Atlas may interrupt.
  - Nothing runs while paused.
- **Lazy.** Nothing is read off the disk until it's first used.
- **Saved.** Each tool has its own file in `data/state`: `people`, `habits`, `cards`, `snippets`, `feeds`, `receipts`, `trade_journal`, `waiting_taught`, `launcher_uses`, `mailbook`. Clipboard history is the exception: it is never written.
- **Settings.** Everything is in one block, `workday:` in `tools.yaml`.

## 3. Reversed on purpose: "Atlas never watches the clipboard"

That rule held since round 1. It still holds for everything except clipboard history, which exists to do exactly that.

The reversal is yours to make: it ships **off**, and the guard test (`clipboard_rehearse::atlas_never_watches_the_clipboard_in_the_background`) now checks three things:
- the history is off in the code and in the shipped file;
- nothing in `workday` ever saves it;
- the ask-first clipboard reader is unchanged.

## 4. Measured and checked

- **PDF**: 18 real files were parsed, rewritten reversed, and stamped with a half-transparent image. They included TeX Gyre and Latin Modern documentation, a 10-page design showcase, and qpdf object-stream and QDF variants of each.
  - Every output passed `qpdf --check`, and page counts matched `pdfinfo`.
  - The render shows the stamp blended over the text.
  - Parse took 0.3–11 ms; the whole rewrite took 1–21 ms (release).
  - Fixtures of Atlas's own making are in `tests/fixtures/round11/`: classic, object-stream, and an AES-256 encrypted file, which is refused.
- **Market calendar**: checked against NYSE's published 2026–2028 list, including Good Friday, July 3 2026, Christmas Eve 2027 observed, and the unobserved Saturday New Year of 2028.
- **FSRS**: `R(S,S)=0.9`; the first "good" is 3 days (S₀=3.173); intervals grow; a lapse lowers stability.
- **Launcher**: 1,000 candidates ranked 20 times in under 2 s (debug).
- **Tests**:
  - `tests/round11.rs`: 64 tests, one module per idea, plus every helper directly.
  - `tests/round11_on_windows.rs`: 5 tests that run only on Windows (§6).
  - `tests/workday_through_the_daemon.rs`: 17 tests, each driving a tool through `Daemon::turn` and checking the store.

## 5. What each needs on your laptop

- **Windows native run**: see §6 for what was run and what it showed.
- **Mail**: waiting-for and meeting prep fill from the next "check my mail". The Sent folder is found by its `\Sent` flag.
- **To turn on (your choice)**: clipboard history ("turn on clipboard history"), and key chords (`workday.chords.enabled: true`).
- **For signing**: a PNG of your signature at `workday.signature_png`.
- **Translation and the model's side of things**: your local model (already set up for the rest of Atlas).

## 6. Windows

Run natively on your laptop (le3o), a release build of the test suite: **85 of 86 passed**. That's every round 11 test plus the Windows-only ones.

| on Windows | result |
|---|---|
| `Windows.Media.Ocr` on a rendered invoice | read `"Invoice 4471\nTotal 43.20"` exactly, in **41 ms**; the receipt reader took the labelled total, $43.20 |
| a key chord (Ctrl+Alt+Shift+F12) through `RegisterHotKey` | registered; Win+L refused |
| the Start menu walk | **75 shortcuts in 27 ms** (all-users and yours) |
| the clipboard's sequence number | read (65) |
| `read_first` on every sentence | **0.2 µs a sentence** (release) |
| writing and reading the clipboard | **not tested: Windows refused it to this session.** `clip.exe` said "Access is denied", and PowerShell's own `Get-Clipboard` failed the same way from the same shell. This is the remote shell's session, not the code. The private-copy checks (`clipboard_copy`) are compiled and tested on the mock; they're the next thing to watch on your desktop, with "turn on clipboard history". |
| grabbing the front window | returned nothing: this session has no window in front. On your desktop it's the window you're looking at. |

Nothing typed, opened or showed anything during the run.

## Gate

| suite | result |
|---|---|
| personal Atlas, debug targets (lib, bins, `all` and the 26 separate test targets) | **6,709 passed, 0 failed**, 2 ignored (need whisper and piper). After merging master `24bf4b4`: **6,723 passed, 1 failed** (a name that master's new `wire` also uses), fixed by renaming and re-run green: **6,724 passed, 0 failed** |
| *[row removed 28 Sep 2026: trading-system material]* |
| the guards | green: dead capabilities (test-only 251, helper-untested 4, both unchanged), dead methods, dead config, name collisions, the catalogue, every intent reaching the daemon, phrases that parse, the retrospective |

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 7. Also fixed

- **Tests that failed at night.** Found because this round's gates ran across midnight UTC.
  - Every calendar test that says "tomorrow" failed between 00:00 and 04:00 in the test zone, because round 10's "tomorrow after midnight is asked about" rule reads the wall clock. Those tests (`your_own_calendar`, `times_with_other_people`, `round10`, the new daemon tests) now run in a zone where it's morning, when they're run in those hours.
- `round10::a_call_or_a_video_is_not_time_away` failed after 22:00. Its fifty driven minutes crossed midnight, and the report reads "today" off the wall clock. It now runs earlier in the day when it's late.
- `open_path` (new) hands its child process to `unwaited`, so it leaves no zombie. It also opens web addresses, for "open 2" on a feed.
- `http::Response` now carries `Location`, so a redirect can be followed on purpose.

## Sources

- NYSE, *Holidays & Trading Hours*: https://www.nyse.com/trade/hours-calendars
- BLS CPI release schedule (fetch refused, not retried; dates cross-checked from two copies): https://www.bls.gov/schedule/news_release/cpi.htm · https://www.usinflationcalculator.com/inflation/consumer-price-index-release-schedule/ · https://cpiinflationcalculator.com/cpi-release-schedule/
- FSRS algorithm and FSRS-5 defaults: https://github.com/open-spaced-repetition/awesome-fsrs/wiki/The-Algorithm (and fsrs-rs)
- Loop Habit Tracker (strength score): https://github.com/iSoron/uhabits
- Windows.Media.Ocr; PowerToys Text Extractor (for the idea); Clipboard formats that opt out of history: Microsoft Learn, *Clipboard Formats* / `ExcludeClipboardContentFromMonitorProcessing`
- `RegisterHotKey`: Microsoft Learn
- Espanso (read for triggers and variables only, GPL-3.0), Ditto (clipboard history)
- lopdf (read for structure), qpdf (validation only, at test time)
- Monica (personal CRM ideas, AGPL), Miniflux (feed reader behaviour), ClearURLs (tracker list)
- SROIE (ICDAR 2019) receipt layouts
- Mozilla Firefox Translations / Bergamot (local translation shape)
- Commitment detection in email: Microsoft Research; Iqbal & Horvitz 2007; Epstein et al., UbiComp 2015

*MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE*
