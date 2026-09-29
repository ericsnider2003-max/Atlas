# Session, 25 September 2026: working a window is an errand, like the rest

Eric: *"there are systems in place for Atlas not to be stopped, things to be worked in the background, and a crew system for multiple pieces of work to happen at the same time. … determine where the confusion came from and what you messed up, then fix it."*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

**Full suite: 30 targets, 6,349 passed, 0 failed, 0 warnings. The Windows build is clean.**

## What already existed

Atlas already had three systems for running work without it being stopped:

- **`crew`** runs errands side by side, on their own threads. There are rules for them:
  - "Pausing holds, it does not erase."
  - Stopping one errand pauses it (Eric's B1 ruling, doc 13 §3, built in doc 16).
  - With several errands going, `which_errand` works out which one "stop" means from the name, the number, or the conversation. If it can't tell, it asks.
- **`lanes`** splits work into two kinds. Background work runs any time. Foreground work, meaning anything that types or clicks in your windows, waits for a gap in your own activity instead of taking the screen from you.
- **`attention`** handles "Pause". It holds every errand where it is, and "I'm ready" carries them on.
- **`addressing`**, in its own words: *"Abandoning work because someone walked in is worse than missing one instruction."*

## What I got wrong, and why

I built "work the window in front for you" (doc 23) as a one-off job slot beside all of that, not inside it. I took its rules from the older `delegate` module, whose header said **"you speaking ends it immediately — you are back, it stands down."** That module was written before the crew could run work side by side, and I followed it instead of the newer rules for the whole system.

That caused four specific failures:

1. **Any request ended it.** Doc 23 stopped the job when you asked for anything else. In the audit (doc 24) I made that stricter: every turn stopped it before doing anything else. That is the bug you hit.
2. **Putting another window in front ended it**, even though foreground work is supposed to wait for a gap, not quit.
3. **Pausing Atlas ended it**, where pausing is meant to hold. I made the same mistake in the audit with call notes: pausing mid-call ended the recording and wrote the notes up, instead of holding.
4. **"Stop the Slack one" couldn't reach it.** The job wasn't one of the errands `which_errand` chooses between, and "what's queued" didn't list it.

Then, when you asked why, I explained it with a keyboard-conflict story and proposed to build things that already existed. I answered from memory of my own change instead of reading the code.

**On permission.** The `grants` rules are yours:

1. Ask about an app Atlas doesn't know.
2. Naming the app is the permission.
3. Some apps confirm every message.

I applied rule 1 to the window you had pointed at and told Atlas to work. That is rule 2: you named it by putting it in front and saying "reply to this". So Atlas asked a question it shouldn't have.

## What changed

**Window work is an errand** (`daemon::WorkingForYou`). There can be several at once, one per window, each with an id in the same id space as the crew's errands (`WINDOW_JOB_IDS`).

- **Asking Atlas for something else doesn't stop it.** The stop-on-any-request logic was removed from `turn_from` and `execute_timed`.
- **"Stop" pauses it, with nothing lost.** Window jobs are now among the errands `which_errand` picks from, labelled as a *conversation* with the app as its topic:
  - "Stop the Slack one" pauses that job and "carry on with the Slack one" picks it back up.
  - "Cancel the Slack one" ends it.
  - A bare "stop" with two going asks which one, numbered.
- **Pausing Atlas holds it.** Nothing is typed while Atlas is paused or handed to someone else, and it carries on when you're back. "Pause" counts it among the errands it's holding.
- **It works alongside you:**
  - It reads the window in the background; UI Automation doesn't need the window in front.
  - When there's something to reply to and you're typing, it waits.
  - When you stop, it brings its window forward. It checks that the window is really in front and that the cursor is in a text box, types, and puts your window back.
  - "What's queued" says where each job stands: watching, waiting for a gap, or paused.
- **"Waiting for a gap" now counts your typing.** `lanes` has always defined a gap as "no keyboard, no mouse, no speech", but only speech was measured, so foreground work could find a "gap" while you were typing. The platform now reports real input idle time (Windows `GetLastInputInfo`), and `awareness` uses whichever of the two is shorter. This fixes the queued foreground lane too, not just window jobs. One known side effect: Atlas's own typing counts as input, so after it types, its next reply waits one gap (20 seconds by default).
- **No permission question for the window you point at** (rule 2). Apps you've set to confirm every message still never get a send from Atlas; it leaves a draft there (rule 3).
- **Call notes: pausing holds the recording.**
  - While paused, nothing is captured. Silence is written in its place, so both sides stay on the same clock.
  - "Are you recording?" says it's paused.
  - "I'm ready" records again, and the call's notes carry on as one set of notes.
  - Switching Call notes off or handing Atlas over still ends the recording. Someone else at the laptop may be on their own call, which is not yours to note.
- **The `delegate` module's header now says what's true.** `user_returned` is replaced by `called_off`, which only "cancel" uses.
- **Docs 23 and 24 are corrected where they stated the wrong rule.** The old lines are struck through, with a pointer to this doc.

## Tests

`tests/working_a_window_for_you.rs` drives the real daemon against the mock platform. The mock now brings a window forward when Atlas focuses it, and reports input idle time. The tests check that:

- the window you point at needs no permission question;
- asking for something else doesn't stop the job;
- it waits while you type, then types, and gives your window back;
- "stop the somechat one" pauses the job, and "carry on" picks it back up;
- "pause" holds the job and "I'm ready" carries it on;
- two windows are worked side by side, and a bare "stop" asks which one.

`tests/the_gaps_the_audit_found.rs` now also checks that a pause holds a call recording rather than ending it, and that a pause holds a conversation being carried on.

`attention_grants_delegate` was updated. "You speaking ends it" was replaced by "calling it off ends it".

## Still open, named

- **Not yet run on the laptop:** typing into a window, the input-idle reading, and a real call. The laptop connection dropped during the last session.
- **The picture reader** still needs its 3 GB download, which is yours to start.
- **The envelope:** who else may ask about it is still your call.
- **The hub:** paused, as asked.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
