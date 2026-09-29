# Session, 25 September 2026 (later): the new work goes through the systems Atlas already has

Eric: *"look for gaps and fix them based off what you find in the system."*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## How I looked

I held the work from 22–25 September up against the rules Atlas already follows everywhere else, the same way doc 25 did for window work:

- **`crew`: the tick never waits.** Slow work runs as a crew errand. It can be named, stopped and held, and "what's queued" lists it.
- **Every model call is recorded** (`record_model_call`).
- **Anything that goes out as you is on the record** (the activity journal, as *Published*).
- **Everything Atlas does as you outside itself has its own switch** in Settings, under "Reaching outside this machine".

Four pieces of the new work broke those rules.

## What was wrong, and what changed

**1. Writing a window reply stopped Atlas.** The model was called on the tick itself. A local model on a laptop can take up to a minute, and for that minute Atlas didn't listen, notice anything, or answer.

- Writing the reply is now a crew errand.
- The reply comes back to its window job and is typed in the next gap in your typing.
- The first reply you ask for goes as soon as your hands are off the keyboard (2 seconds). The 20-second gap is for replies you didn't just ask for. Without this exception, having just spoken to Atlas would count as being busy.
- "Cancel the Slack one" also stops a reply that is still being written.
- The model call is recorded.

**2. Call write-ups ran on a thread of their own.** Nothing could name, stop or hold them, "what's queued" couldn't see them, and the summary's model call went unrecorded.

- A write-up is now a crew errand called "the write-up of the Zoom call".
- The summary's model call is recorded.
- The notes are logged in the activity journal.
- A failure says the audio is kept.

**3. The picture reader ran for up to five minutes inside your request.** Atlas couldn't hear anything until it finished.

- It's now a crew errand: "Looking at your screen — I'll tell you in a moment."
- The answer is said when it's ready.
- The reader's memory is handed back to the memory budget when it finishes.
- Its model call is recorded.
- "Cancel the looking" ends the reader straight away.

**4. The test for (3) found a real hang.** A reader that was stopped, or ran out its time limit, was then waited on for its output. Anything it had started kept that output open, so "stopped" still meant waiting the whole minute out. Its output is now handed back without waiting for it.

**5. Replies sent as you weren't on the record.** Every send is now a *Published* line in the activity journal ("replied in Slack: …"). A reply that couldn't be typed is recorded as *Blocked*.

**6. There was no switch for Atlas writing as you.** There is now: **Working your apps** in Settings, under "Reaching outside this machine", as a Permission.

- It's on, per your ruling ("app: yes it should").
- Turning it off makes Atlas say so rather than act.
- The turn limit for carrying on a conversation is now a setting (`delegate.max_turns`, default 12), instead of a number written into the code.

**7. Smaller gaps:**

- "Pause" now counts windows being worked as work it's holding.
- `which_errand` has names for the new errands, so "stop"/"cancel" can pick them: "the look at your screen", "the write-up of the Zoom call", "the conversation in Slack".
- `atlas window idle` shows how long it's been since the keyboard or mouse was used. That's what "wait for a gap" measures, so you can check it on the laptop.

## Guards

- `picture_talk::ask` is gone; its only caller now uses `ask_until`, which can be stopped.
- `for_the_window` takes your settings, and its settings-free twin is removed.

Both were caught by the guards that flag code nothing calls and names shared between modules.

## Tests

`tests/work_goes_through_the_crew.rs` (7 tests) checks that:

- a model taking 0.8 s doesn't hold up asking or ticking;
- a sent reply is on the record as *Published*;
- the switch exists, is on by default, and "off" means off;
- the turn limit comes from Settings;
- a finished call is written up by the crew, with the summary's model call recorded and the notes logged in the journal;
- the new errands can be named;
- the picture reader stops within half a second of being told to, rather than after its minute.

The window-work tests now tick until the crew has written the reply, which is how it runs for real.

## Found and named, not changed

- **A fresh Atlas's first tick takes about 0.9 s** (start-up looking around). It happens once and is the same with or without a model.
- **Window jobs don't survive a restart.** Neither do crew errands. After a restart you'd ask again.
- **Atlas's own typing counts as keyboard use,** so after it types, its next reply waits one gap.
- **The laptop is connected again but was locked.** Typing into a window and a real call still haven't run there. `atlas window idle`, `atlas window read <app>` and `atlas call check` are there for when it's unlocked.

## Still open

- **The picture reader** needs its 3 GB download (`atlas get pictures`), which is yours to start.
- **The envelope:** who else may ask about it is still your call.
- **The hub:** paused.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
