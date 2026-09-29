# Session, 24 September 2026: the clock, live settings, the moving line, and seeing

**Full suite: 30 targets, 6,301 passed, 0 failed, 0 warnings** (after the rulings below). The real-model tests were run separately with their kits: seeing 7/7, and the picture reader on a real chart.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

Eric's list, worked in order with item 2 (the real laptop install) skipped:

- **1. The clock.** Done.
- **3. Live settings.** Done.
- **4. The moving line and the desktop overlay.** Done.
- **5. Real local vision.** Done, and run on real pictures.
- **6. The four decisions.** Laid out below.
- **7. Housekeeping.** Done: gate, build, docs and package.

## 1. The clock reads your time, not UTC's

Atlas stored every moment as UTC, which is right. The mistake was that it also *showed* times and decided "which day" in UTC. On Eric's laptop (Pacific, UTC−7), 5 pm read as midnight. After 5 pm, "tomorrow" pointed two days ahead. The greeting said "good morning" in the evening.

`localclock` now asks the machine for its offset: Windows directly (`GetTimeZoneInformation`), `date +%z` elsewhere.

- **Everything now uses it:**
  - the calendar, which now stores real moments and reads them on your clock
  - the greeting and the daily brief
  - the nudge timing, your working rhythm and "today"
  - late nights, the workspace view, and bookings
- **Tests.** The test suite pins the offset to 0 in `.cargo/config.toml`, so a test written for noon still means noon. `tests/your_clock.rs` has 8 tests, including "5 pm Pacific is still today" and "tomorrow is tomorrow".
- **Known limit.** Across a daylight-saving change, a time is read with today's offset until the next start.

## 3. Settings apply without a restart

Before this, the running Atlas kept the settings it started with. A switch moved in the settings window, the hub, or by hand was saved to disk and did nothing until the next start.

Now the running Atlas watches `tools.yaml` and `settings.yaml` and picks up changes by itself.

- **When it looks.** On every heartbeat tick, and before every request. The voice loop and the typed prompt don't tick, so the request check covers them.
- **How it reads them.** Every read of a setting in the daemon goes through one accessor. The three parts that keep their own copy of a setting are refreshed on a change: speaking first, the persona, and desk presence.
- **What still waits for a restart.** Eleven settings set something up at the start (`settings::NEEDS_A_RESTART`), each checked against where it is read:
  - voice
  - wake word
  - the typing key
  - phone access
  - the phone companion
  - the voice's sound and speed
  - end of speech
  - the model memory budget
  - identity checks and trusted devices

  The settings window now says which kind each change was. It offers **Restart Atlas now** only when a change actually needs one.
- **Safe on bad input.** A half-written or broken settings file keeps the settings Atlas already had and says so once. What Atlas worked out for itself at the start, such as the microphone it picked, survives a reload.
- **Tests.** `tests/settings_apply_live.rs` has 11 tests that drive the real daemon.

## 4. The line moves when Atlas speaks, and the words go on the desktop

**The moving line.** The line (the mark) was drawn to follow a real voice level, but nothing produced one. Now it follows the real voice:

- Piper writes the whole reply to a WAV file before it plays.
- `speaking` reads the loudness of every 30 ms of that file and writes it next to Atlas's data, together with the moment playback starts. The file is removed when playback ends.
- The Atlas window's line follows it: it swells on loud syllables and settles in pauses.
- A file left behind by a crash can't keep the line moving, because a level exists only inside the speech's own length.

**The desktop overlay (`atlas overlay`).** This is a window with no frame, no background and no taskbar button. It sits above everything, and every click passes through it.

- **When it shows.** When Atlas starts speaking, the line arrives, the words type in over a soft shade (so they read over a white page), and then it fades.
- **How it runs.** The background Atlas starts it on Windows. It ends itself when the background Atlas ends, and only one runs at a time.
- **The setting.** New: **How it talks back → Desktop captions**. It applies live.
- **Tests.** `tests/the_line_moves.rs` has 9 tests, using a real WAV file with silent, loud and quiet sections.
- **Not yet seen on screen.** The overlay window itself has not been watched on the laptop.

## 5. Real local vision, run on real pictures for the first time

The eight OpenCV zoo models had never run. The 11 Sep note said so ("no model has been run"). This session downloaded them, pinned their fingerprints, and ran Atlas's own code on OpenCV's sample pictures (`tests/seeing_real_pictures.rs`, which runs when `ATLAS_SEEING_KIT` is set).

That found two real bugs.

- **The object and face models were fed the wrong colour order.** OpenCV trains and runs them blue-first. A bowl of oranges came back as "a bed" and "a vase". Fed blue-first, it came back as a dozen oranges, scored 0.55 to 0.77. The person and the football were found either way, which is how the bug hid.
- **Reading the screen squashed whole lines into one word.** The finder returns whole lines, and the reader reads a 100×32 strip. "meeting moved to three" came back as "metirgmoediatives". `words_in` now splits a line at the gaps between words before reading. The result: "meeting moved to three invoice total 4250", word for word.

After the fixes, all of these pass on real pictures:

- a face found where it is
- a person and a ball named
- oranges named
- a baboon described as a baboon and not taken for a face
- a face shown once and known again, mirrored and re-cropped
- a stranger not named

**Charts.** The fixed-list models can't say what a chart shows. Eric's laptop has 15.7 GB of memory (Core Ultra 7 256V, with the Arc 140V sharing it), so charts are handled by a local picture reader:

- **The model.** Qwen3-VL 4B Instruct, 4-bit, 2.5 GB, plus its 0.45 GB picture encoder. llama.cpp's `llama-mtmd-cli` runs it once per question, so it uses about 3 GB while it answers and nothing otherwise. The memory budget is asked first.
- **What it said.** Run through Atlas's own code on a known chart (sales Jan–Jun: 120, 135, 128, 160, 190, 175), it answered: *"Sales started at 120 units in January, increased to 135 in February, dipped slightly to 130 in March, rose to 160 in April, peaked at 190 in May, and then fell to 175 in June."* The low, the peak and the shape are right. March and April were read about 2–5 units off.
- **Speed.** 37 seconds here on two slow cores. The laptop build uses Vulkan, so the Arc graphics should be faster.
- **A bug found on the first run.** llama.cpp sizes its memory for the model's 262,144-token default context. The first run was killed for running out of memory. Atlas now asks for 4,096.
- **What it answers.** "Look at my screen", "what's on my screen" and "what does this chart show?" now take a screenshot, ask the local model about it, and delete the screenshot. The running Atlas used to answer this with *"Capture runs through the voice layer."* and looked at nothing.
- **Setting.** New: **What it can see → Reading pictures**.
- **Tests.** `tests/reading_pictures.rs` has 7 tests. The real-model one runs when `ATLAS_PICTURE_KIT` is set.

**Downloads, pinned.**

- `atlas get seeing` fetches the eight zoo models, about 133 MB. ATLAS.bat used to fetch these with no check at all; menu item 8 now calls this.
- `atlas get pictures` fetches the llama.cpp Vulkan build b10456 and the two Qwen files, about 3 GB.
- `atlas get` streams its fingerprint check now. It used to hold the whole file in memory twice, which would have been 5 GB for the 2.5 GB model.
- **Named, not proven.** The llama.cpp zip's fingerprint was read from GitHub's release page, not measured. This container can't reach that repository. The first `atlas get pictures` checks it, and if it doesn't match, the file is refused and Atlas says so.

## 6. The four decisions

Each of these acts on your accounts, your calls, your apps or your estate, so each needs your ruling.

- **`confirmed` (security changes).** Today Atlas reads the change back and waits for your yes, but **you** do the clicking (`atlas walkthrough`). The decision: may Atlas ever click the security switch itself after the read-back and your yes, or does it stay "Atlas finds the page, you make the change"?
- **`consent` (call recording).** This is the only one of the four still unwired. The decision: should call notes exist at all? If yes, is it only your own microphone (legal everywhere, and needs nobody's permission) or the whole call? The whole call means Atlas announces it first, records nothing if the announcement can't be delivered, and stops if anyone objects.
- **`delegate` (working another app for you).** It is built with a turn budget, a stop condition, standing down the moment you speak, and never sending without a confirmation. It is reachable only through the overnight "delegate" mode, which is off. The decision: may Atlas type into another app for you, and which apps?
- **`afterme` (if something happens to you).** Recording your arrangement and the yearly "is this still right?" reminder are wired. Atlas never acts on it and never holds the passphrase. The decision: does it stay that way, or should Atlas ever send the "there's an envelope in the safe" message itself after a period of silence you set?

## Eric's rulings on the four (24 Sep), and what was built

- **Security changes: yes.** Atlas may make the change itself after the read-back and Eric's yes.
  - **The switch:** `walkthrough.atlas_clicks` is now a real setting, **Settings → Security switches**. It is off until Eric turns it on, and it asks once when turned on.
  - **What Atlas will press:** only a visible control labelled for that change, and only when there is exactly one on the page. It stops at any password box and never types anything. Anything else hands the page back to Eric with its address.
  - **Tested** in a real headless Chrome with pages that have one switch, two, a password box, and a hidden switch.
  - **Found on the way:** Atlas's own browser could never attach to Chrome. Its HTTP client read until the connection closed, and Chrome's DevTools endpoint keeps it open. It now stops once the whole reply has arrived.
  - **Not yet tested:** real sites. Most security pages ask for the password again, and Atlas stops there by design.
- **Call notes: yes. Other voices: ask, then record.** Recording anyone besides Eric now asks the call a question and waits for a yes.
  - No answer, or one no, means only Eric's side is noted. A yes that comes before the question has landed counts for nothing.
  - The old announce-and-object mode is still there, but asking is what ships.
  - **Not wired yet:** call capture itself. Nothing detects a call, records one, or transcribes one, so the switch stays off. Recording the other side needs a way to capture Windows' own sound output, which Atlas doesn't have yet.
- **Working another app: yes; which apps, undecided.** The per-app rule already fits this: Atlas asks before using an app it doesn't know, and naming the app counts as permission.
  - **Not wired yet:** a daytime "finish this until I'm back". Today it is only reachable through overnight mode.
- **If something happens to you:** "Not sure I want it to announce where it's at unless asked." Unchanged: Atlas never announces it and never sends anything. Open question: who may ask Atlas where it is, and when.

## Housekeeping

- **Guards.** Updated with reasons:
  - `TEST_ONLY_MAX` 255 → 254 (`live_height` is now used)
  - `MODULES_IN_TREE` 302 → 305
  - a new capability, `picture_talk`
  - `speaking` joins `speak`, and `overlaywin` joins `overlay`
- **CAPABILITIES.md.** Regenerated.
- **ATLAS.bat.** Kept as CRLF. A scripted edit this session briefly damaged it. It was rebuilt and checked line by line against the last delivered copy: the only differences are the intended seeing change.

## Still open, named

- **The desktop overlay has run on the laptop but hasn't been seen.** It was tested in a throwaway folder under `%TEMP%`, since removed:
  - The zip's fingerprint matched.
  - `atlas overlay` started as the window "Atlas overlay" and stayed up through a reply being spoken.
  - It closed by itself within seconds of the background Atlas's lock going away.
  - It uses about 130 MB while idle, mostly its drawing surface. That is worth trimming later.

  What it looks like on screen is still unseen: the laptop was locked, so there was nothing to capture.
- **The picture reader has not run on the laptop.** It needs the 3 GB download there (`atlas get pictures`). That is Eric's to start, because it is a large download to his machine.
- **Daylight saving.** Times are read with today's offset until the next start.
- **The hub.** Paused, as asked.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
