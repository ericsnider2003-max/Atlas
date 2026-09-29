# Session, 24 September 2026 (later): what was left half-done, finished

Eric: *"If you haven't finished something then finish it or you have a history of losing it."*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

This covers the three pieces that were still open after the rulings, plus two gaps found underneath them.

## Call notes, end to end

The rulings were "call: yes notes, voices ask then record". Four new modules carry them out:

- **`callwatch`** notices a call. Windows keeps an "apps using your microphone" list in the registry, and an app holding the microphone right now has a stop time of zero.
  - It counts as a call when a call app (Teams, Zoom, Discord, Slack, Webex and others) or a browser holds the microphone.
  - Atlas's own listening is excluded.
  - The parser is tested against real `reg query` output from the laptop. On the laptop, `atlas call now` answered correctly: "No call app is holding the microphone."
- **`callrec`** records two separate files:
  - your side from the microphone;
  - their side from what the laptop plays (WASAPI loopback through `cpal`), so no virtual cable and no Stereo Mix.

  Both are written to disk as they arrive, at 16 kHz mono. Silence is written in wherever Windows sends nothing from loopback, so the two files stay on the same clock.
  - **On the laptop:** `atlas call check 6` recorded 5.9 s of what the laptop played while a Windows sound was playing, and the sound was in it. It recorded 6.1 s from the microphone, silent because nobody was speaking.
- **`callnotes`** runs the steps, and every decision goes through `consent::Recorder`:
  - A call starts, and Atlas notes your side.
  - "Record everyone" gives you the question to ask. It's worded for the chat, because `announce_in_chat` is on.
  - "They said yes" starts recording their side.
  - "They said no", "nobody answered" and "I couldn't ask them" all mean your side only. A yes after any of those counts for nothing.
  - If their side is ever being recorded without a yes, it is stopped and deleted on the spot.
  - When the call ends, both sides are transcribed by whisper with timestamps and merged in order as "You:" and "Them:" lines. They're summarised by the model if there is one, with the transcript passed as quoted material, not instructions. The notes go into your notes folder.
  - Audio is deleted after `keep_audio_days`; the notes are kept.
- **Things you can say:**
  - "take notes on this call"
  - "record everyone"
  - "they said yes" / "they said no"
  - "nobody answered"
  - "I couldn't ask them"
  - "just my side"
  - "are you recording"
  - "stop taking notes"
- **The setting** Call notes (Reaching outside this machine) now describes what it actually does. It is off until you turn it on.
- **Tests:** `tests/call_notes.rs` (13) and `tests/asking_before_recording.rs` (7).
- **`consent` is now wired, and it was the last module nothing reached.** The unwired baseline is empty: every module in the tree is reached by the running program.

## Working the window in front for you

**A gap found first: the Windows platform couldn't type, press keys or read a window at all.** `type_text`, `press` and `read_window` fell through to "not supported here", which means dictation had never worked on Windows either. All three are now built:

- **Typing:** `SendInput` with Unicode characters, so any language or symbol arrives as written. A line break is Shift+Enter, so a half-typed reply never gets sent.
- **Keys:** combos like "ctrl+a" and "enter".
- **Reading:** UI Automation, the tree a screen reader uses, capped at 3,000 elements.
  - **Proved on the laptop:** `atlas window read Notepad.exe` read the text of a test file open in Notepad, even with the screen locked.
  - **Not yet run on the laptop:** typing. It can't be tried while the laptop is locked.

**Daytime delegation** is built on top of that:

- **"Draft a reply to this"** writes the reply into the box and doesn't send it.
- **"Finish this conversation until I'm back"** carries on, up to 12 turns, and replies only when something new has arrived since Atlas last wrote.
- **Unknown apps:** Atlas asks the first time before using an app it doesn't know, and naming an app counts as permission (`grants`). Apps that confirm every message stop and wait for you.
- ~~**You coming back ends it:** saying anything stops it before your request runs, and so does putting another window in front.~~ **Wrong, reversed in doc 25:** it's an errand; only "stop" stops it, and it waits for a gap in your typing instead of quitting.
- **Needs a model:** without one, Atlas says so and types nothing.
- **What's on screen** reaches the model as quoted material, never as instructions.
- **Tests:** `tests/working_a_window_for_you.rs` (7) drives the real daemon against the mock platform. The mock now holds readable windows.

## The envelope, only when asked

Your ruling was: not announced "unless asked". "Where's my envelope?" now reads back what you arranged: the kind of place, and who knows. Atlas never holds the passphrase.

The only thing Atlas says without being asked is the yearly "is this still right?" review reminder, which names no place. A guard checks that. `after_me` is the owner's only: nobody Atlas is handed over to can ask. Tests: `tests/the_envelope_when_asked.rs` (3).

## Also caught

**Live settings missed four reads.** Four settings reads in the daemon were split across lines (`self.cfg\n.tools`), so the earlier change that applies settings without a restart didn't reach them:

- notes folder
- backups
- the model connection
- the browser

They now go through the same live accessor as everything else.

**Guards, moved with reasons:**

- **`TEST_ONLY_MAX`** went 254 → 250 → 253. The drop is the delegate steps now being used. The rise is `consent` joining the count once it was wired: its two wording helpers, `announcement_named` and `script`, aren't used by anything yet.
- **`MODULES_IN_TREE`** 308.
- **A new capability, `callnotes`.** The shakedown now says three things need Eric in person: two camera ones and a real call.
- **The `metrics` reader check** is kept rather than deleted now that the unwired baseline is empty. It counts the quoted lines in the list instead.
- **Handed over:** `delegate` and `call_notes` are things a guest can never start (`NEVER_AS_A_GUEST`). `after_me` is the owner's own.
- **Dependencies:** `cpal` (Windows only) is listed with its reason. The `windows` crate gained the keyboard, UI Automation and variant features.

## One thing on the laptop to know about

The window-reading test opened a test file in Notepad. Closing it afterwards closed the whole Notepad process, and Eric's other open tabs went with it. Notepad keeps its tabs (the TabState folder had 7 entries afterwards), so they should come back the next time Notepad opens. Worth a glance.

## Still open, named

- **Typing into a window, and a real call, have not run on the laptop yet.** Typing can't be tried while the laptop is locked, and a real call needs one. `atlas call check` and `atlas window read` are there for checking.
- **The picture reader** still needs its 3 GB download on the laptop (`atlas get pictures`). That's Eric's to start.
- **The desktop captions** haven't been seen on screen.
- **Hub:** paused, as asked.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
