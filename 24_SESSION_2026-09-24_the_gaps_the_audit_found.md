# Session, 24 September 2026 (last): the gaps an audit found, fixed

Eric: *"Can we make it better, identify and fix gaps."*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

Three reviews read the day's new work line by line:

- call notes
- working a window for you
- the security switch
- the desktop captions
- the picture reader

Each finding was checked against the code before anything changed. The ones that turned out to be real are fixed, and each fix has a test in `tests/the_gaps_the_audit_found.rs` (23 tests). **Full suite: 30 targets, 6,348 passed, 0 failed, 0 warnings.** The Windows build is clean (0 warnings). The real-Chromium security-switch test passes.

## The biggest one: speaking to Atlas passed whisper a broken command

Every spoken turn sent whisper-cli the literal text `{task_opt}`, `{lang_opt}` and `{lang_val}` as arguments.

- The shipped speech command has had those placeholders since the language settings were wired in.
- Only the daemon's video and call path filled them in. The microphone path (`Voice::vars`) never did.

There is now one function, `language::insert_whisper_vars`, and every caller uses it. The test builds the shipped command and checks nothing is left in braces.

## Call notes

- **A "no" after a "yes" now stops their side and deletes it.** Before, "they said no", "nobody answered" or "I couldn't ask them" after a yes left their side recording. Now, after every step, Atlas checks the recorder: if it says theirs may not be recorded, the recording is stopped and deleted. Your side carries on, and "are you recording?" answers truthfully.
- **"Stop taking notes" stays stopped.** The call watch used to see the call "start" again five seconds later, which undid your stop. A new call is noticed only once this one has ended.
- **"Take notes on this call" with no call app seen lasts until you say stop.** It used to end at the next five-second look. If the watch then does see a call app, that call ends when the app lets go of the microphone.
- **Paused, handed over, or Call notes switched off mid-call** (pausing was changed in doc 25: it now holds the recording rather than ending it):
  - The recording stops.
  - What was taken so far is written up.
  - Nothing new starts.
  - "Stop" and "are you recording" work even with the setting off.
- **Headsets that record at 8 kHz** used to produce files that played at double speed. They are now brought up to 16 kHz properly.
- **Long calls.** Transcription used to be cut off at the tool's two-minute limit, which left notes saying nothing was said. The limit now grows with the length of the call; an hour gets about two hours.
- **Transcription failures are reported.** A failed transcription used to be written up as silence; now you're told it failed.
- **Two calls in the same minute** get two notes files ("… Zoom (2).md") instead of the second writing over the first.
- **An unplugged headset closes its file properly.** A recording thread whose owner is gone stops.
- **No transcriber set up:** you're still told what happened on the call.

## Working a window for you

- ~~**Anything you say stands it down first.**~~ **Wrong, and reversed in doc 25:** a new request no longer stops it. It's an errand, stopped only by "stop".
- **Saying yes to an app Atlas doesn't know works.** Before, your yes went unrecorded and the same question came back. Now it is recorded against that app and the job goes ahead.
- **Apps you've set to confirm every message** get one draft left in the box for you to send. Before, they stopped dead.
- **It doesn't answer itself or a clock.** Three changes:
  - After sending, Atlas waits two seconds before taking its "nothing new" snapshot.
  - "New" is judged by what's on screen after Atlas's own last reply.
  - Times, "seen", "delivered" and "typing…" are ignored.
- **A model with nothing to say** now means "wait for them", not "give up".
- **It never types into the wrong place:**
  - Right before typing, Atlas checks again that the same window is in front.
  - It also checks that the cursor is in a text box. On Windows this goes through UI Automation.
  - If either check fails, nothing is typed and Atlas says why.
- ~~**Paused, or handed to someone else:** it stands down.~~ **Reversed in doc 25:** it holds, with nothing lost, and carries on when you're back.
- **Your instruction carries authority; the screen doesn't.** What you asked for, and what Atlas has already written, now go in the model's instructions. Only the screen text goes in as quoted material, which the model treats as someone else's words.
- **Windows typing.**
  - A Shift+Enter is never split across two batches of keystrokes.
  - If Windows refuses keystrokes partway through, Shift, Ctrl, Alt and Windows are let go, so no key is left held down.
- **Windows reading.**
  - UI Automation now has a 2-second limit per call, so a hung app can't hang Atlas.
  - Windows are read newest-first. If the size limit is reached, it's the oldest messages that go unread, not the one you're being asked to answer.

## The security switch

- **Atlas checks the page before pressing.** It must be on the same site you said yes to. A sign-in page (Google accounts, Microsoft login, Apple ID and similar) is handed back to you. So is a page that redirected to somewhere else.
- **Errors while a page is still loading** are retried until the time limit rather than reported straight away.
- **After pressing,** Atlas says to look at the page, because some sites ask you to confirm again, and gives you the link.

## Settings, clock, captions, pictures

- **Saving a setting is all-or-nothing.** The settings file is written in full, then swapped in, so a power cut never leaves half a file.
- **A settings file you edited by hand into something unreadable is kept.** It's saved beside the new one as `settings.unreadable.yaml` instead of being overwritten, and Atlas tells you it can't read it rather than quietly reverting everything.
- **The clock offset is re-read every minute,** so the night the clocks change is picked up while Atlas is running.
- **Captions:**
  - They show even when the voice's audio format gives no loudness levels.
  - The line moves for 32-bit float WAVs and "extensible" WAVs, not only 16-bit.
  - The drawing side doesn't re-read an unchanged file 20 times a second.
  - A long reply shows only its first 280 characters, cut at a word.
  - The line starts 250 ms later, to match when sound actually comes out of the speakers.
- **Pictures:**
  - A screenshot is shrunk to at most 1600 px wide before the picture reader sees it.
  - The reader is stopped if it takes more than five minutes.
  - A half-written screenshot is deleted.

## Guards, moved with reasons

- **`TEST_ONLY_MAX` 253 → 254.** The new function is `callrec::silent`, a recorder of silence. It's the only way to drive the consent steps on a machine with no sound devices, and it should never replace a real recording.
- **`callrec::silent` is listed in KNOWN** with the same reason.
- **The old `press_the_one` is gone.** Everything uses `press_the_one_at`, which does the site check.
- **`delegate::is_noise` was renamed** `is_screen_noise` so its name doesn't clash with another module's `is_noise`.
- **`language::insert_whisper_vars`** takes the model's facts from `model_facts`, so the language module's own type is still what callers use.
- **Two older tests were updated because the behaviour changed on purpose:**
  - The delegation context test now checks that Atlas's own earlier reply is in the instructions, not in the quoted screen text.
  - The security-switch wording test now expects the "have a look at the page" line.

## Still open, named

- **Not yet run on the laptop:**
  - typing into a window
  - the text-box check
  - a real call
  - the desktop captions on screen

  The laptop was locked, and the connection to it dropped during this session.
- **The picture reader** needs its 3 GB download (`atlas get pictures`). That's yours to start.
- **The envelope:** who else may ask about it is still your call.
- **The hub:** paused, as asked.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
