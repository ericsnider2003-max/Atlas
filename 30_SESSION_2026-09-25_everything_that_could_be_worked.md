# Session, 25 September 2026 (evening): everything that could be worked without you

Eric: *"Ok everything you can work now."*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 1. A restart no longer loses what Atlas was doing

**Before:** a restart silently dropped everything in hand. A window being worked for you, research half done, a council mid-debate: none of it came back, and nothing said so.

**Now:**
- **Work you asked for is written down as it starts** (`resume`, in the state folder, so it's in the backups too) and crossed off when it ends.
- **A window being worked is written down whenever it changes.**
- **On the first tick after a start, Atlas deals with what's left over:**

| What was cut off | What Atlas does |
|---|---|
| A window it was working (e.g. a conversation) | Picked back up if the window is still open ("I restarted and picked the conversation in Slack back up"). A paused one stays paused. One whose window has closed is dropped, and Atlas says so. |
| Work whose only result is an answer or a proposal: research, the council, building code, improving a project | Redone once, from your own words: "I stopped in the middle of the council on X, so I've started it again." |
| Something that is already a redo | Named, not tried a third time. If that work is what keeps bringing Atlas down, redoing it would bring it down again. |
| Work that reaches outside the machine or acts as you (mail, unsubscribing, outreach, the Outlook sign-in), or that depended on a moment that has passed (a look at your screen, a call's write-up) | Named, never redone on its own: "… were cut off when I stopped; I haven't redone them on my own — ask again if you still want them." |
| Chores Atlas starts itself (backups, housekeeping, the search check) | Not recorded at all; they come round again on their own. |

**Honest limit:** a *window reply* that was halfway written is not kept. The window job comes back and writes its next reply afresh from what's on screen.

## 2. Atlas's own typing no longer counts as you typing

Windows' "last keyboard or mouse input" includes Atlas's own keystrokes. So after Atlas typed a reply, it looked as though you had just typed. Its next reply then waited for a gap in "your" typing that was really its own.

**Now:**
- Atlas notes when its own typing starts and finishes, and your last input from before it.
- "How long since you last typed" leaves Atlas's typing out.
- Typing a reply and then pressing Enter still counts from *your* last key.
- It copes with Windows' clock wrapping round every 49 days.

**Limit:** if you type *while* Atlas is typing, those keys are counted as Atlas's. Atlas already waits for a gap before it starts, so this is rare.

## 3. Project work in steps that survive a restart

Improving one of your projects now runs in three steps: **the draft, checking it, explaining it.**
- Each step is written to its own file as it finishes (`phases`, in the state folder).
- Asked again, or redone after a restart (§1), the work carries on after the last finished step instead of starting over: "Carrying on from where it stopped — the draft, checking it already done."
- A step that was halfway through is started again; only finished steps are kept.
- A step's file is written to a side file and then swapped in, so a stop in the middle of saving leaves the step unfinished rather than half a file.
- Once the change is filed in the project's queue, its steps are cleared.

The rest of what wshobson's `full-review` asks for was already there:
- the change waits in the queue for your "implement" (the checkpoint);
- a cap on fix rounds (`max_fix_rounds`);
- "verified" only when the project's own tests passed with the change in place (evidence for "done").

Also fixed: every build and project change left a scratch folder behind in the temp directory. They're now cleaned up.

## 4. How-to procedures in runbook form

Atlas's built-in procedures ("walk me through freeing up memory", "how do I make room on the disk") now follow the runbook shape from wshobson's `incident-response`:
- **Every step says how you know it worked and what to do if it didn't** ("find what's holding memory — until something over 200MB and untouched — if not: nothing stands out: say memory is tight from many small things…"). 23 of the 27 steps have an "if not"; the other four are rules or last steps with nothing to fail.
- **A quick version comes first:** what's needed, then the steps in a few words each ("In short: find … → close it → unload the model").
- **Telling Atlas it didn't work, right after a walk-through, now teaches the procedure:**
  - What you saw becomes a known snag for that procedure, so the same symptom is recognised next time.
  - A blameless look back is filed in your notes: what happened, what should have happened, and "why?" asked down to something that can be changed, never who.
  - Atlas asks you why you think it happened, rather than guessing a cause.
- **What's learned is kept.** The procedures' header always said they were "improved by use". They weren't: the learning function had no caller, and nothing it learned was saved. Both are fixed.

## 5. What you hear first when you come back

When you sit back down after a while away, Atlas names the most urgent things, at most two, and counts the rest. It used to name them **in the order they happened**. So a backup that failed at lunchtime was said before the question that's been waiting for your answer for five minutes.

Now one ordering step (`next_up`, the shape of wshobson's recommender pipeline: drop what doesn't belong, score the rest, pick the top few, count the rest) decides:
- a decision waiting on you comes before something that went wrong;
- newer comes before older;
- the rest is still "and N more".

This one place orders the welcome. The other lists (what's queued, what's outstanding) still order themselves as before.

## 6. The built-but-unused backlog: 254 → 184

One of Atlas's own checks counts functions that are built and tested but that nothing in the running program calls. It stood at 254. All of them were read and sorted; the full table is `30b_BACKLOG_TRIAGE_2026-09-25.md`.

| What was found | How many | What happened |
|---|---|---|
| Should be wired, with an obvious place to call it | 9 | 8 wired (below). The ninth belongs on the hub's Recommendations page, and the hub is paused. |
| Already done another way by the running program | 40 | 28 removed, their tests pointed at the live code |
| Nothing needs it | 39 | 34 removed |
| Test helpers | 19 | kept on purpose |
| **Needs your ruling** | **145** | untouched (see below) |

**Wired:**
- The day rhythm now counts days, so the hour your day ends can be learned. It's also saved now; it was forgotten at every restart.
- A research fetch that fails makes Atlas re-check the connection instead of trusting "online" from before.
- Approval questions say what kind of thing you're approving when it leaves the machine, commits you, or changes Atlas. Local actions still just ask.
- The notes index isn't rescanned below your battery floor, or past the size you set. That size setting was read by nothing.
- "Better then" and similar are caught when Atlas reviews writing, and only ever offered, never changed silently.
- "Show it on your only screen?" gets an answer. Before, your yes or no went nowhere.
- `atlas doctor` flags your own folder path in the settings that would go to someone else.
- A build's scratch folder is removed afterwards.

**Also found and fixed while sorting:**
- **Window jobs that asked a question were thrown away.** A window job that needed your yes asked the question and then threw the job away, so answering did nothing. Now it's kept, paused: yes carries it on, no stops it.
- **Outlook setup advice was wrong.** The spoken advice for connecting Outlook said it needs an app password. It needs OAuth (Microsoft removed every password route), which is what `atlas mail setup` said. Corrected, and `atlas mail setup` now uses the same words, so they can't disagree again.
- **The machine-health tuning check could never say anything.** It was handed an empty survey. It now gets the real disk, memory and model-folder sizes. Memory used by each app, startup items, disposable folders and other drives still aren't measured.
- **A memory leak:** listing the credentials needed overnight leaked a copy of the whole list every time.

**Removed, and the rule each one stated.** These functions always gave the same answer and nothing read them, so they enforced nothing. The rule is recorded here instead:

| Removed function | The rule it stated |
|---|---|
| `vault::usable_unattended` | No vault secret is usable while you're away. A secret usable while you're asleep is usable by whoever has the laptop. |
| `awake::needs_the_screen` | Keeping the machine awake never keeps the screen on. |
| `household::implies_belonging` | Being near another Atlas never implies belonging to it. |
| `tts::needs_gpu` | No voice needs a graphics card. |
| `firewall::coming_back` | Business reaching the personal Atlas is always allowed, and recorded nowhere. |
| `firstrun::essential` | No setup step is essential. Skipping one degrades Atlas, never blocks it. |
| `gaze::identity_is_enough_for_sensitive_work` | A face on camera can say you're back at the desk, never that you approved something. |
| `signin::can_change_security` | A sign-in grant never allows changing security settings. |
| `enrol::avoids_ambiguous` | Generated passwords never contain l, o, O, 0, 1 or I, because you'll read one out loud one day. |

**Needs your ruling: the 145.**
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- About 18 touch the vault, security or consent wording.
- The rest are halves of features that were never built. Each one would speak unprompted, act outside the machine, or change what Atlas stores or decides. Examples:
  - a phone calendar bridge;
  - a global push-to-talk key;
  - scanning and PDF text extraction;
  - Atlas driving another AI in a window;
  - overnight work while you sleep;
  - as-you-type correction in other apps.

Each is in the triage table with its reason. None were wired, because each is a decision, not a default.

**Guards:** the ceiling was lowered to 184, with the reasons written beside it. The three that were left with no caller when their only caller went are listed with reasons (the machine-checks no-list has no runner to guard, because Atlas runs no machine checks yet).

## 7. The hollow checks

What the code reader (`hollowcode`, the heart of the hollow tool) now also finds:
- **Never called.** A private function nothing in the file mentions: built, and never used. Only private ones count (Rust without `pub`, Python `_name`, lower-case Go, un-exported JavaScript), because a public one may be called from another file. Rust methods in `impl Trait for …` are skipped, since they're called through the trait.
- **Made-up dependency.** An import of a package the project never lists. It's checked against the nearest `Cargo.toml`, `requirements.txt`, `pyproject.toml` or `package.json`, looking up to three folders above the file. The language's own libraries (Python's standard modules, Node's built-ins, Rust's `std`/`core`) are known. Without a manifest nothing is guessed, because guessing which names are real is the very mistake it's looking for.
- **A "Fix:" line on every finding** (from `doc_gardener`), e.g. "Fix: handle the error, or pass it up — at least log what went wrong."

**Limit:** it reads one file at a time. "Never called" can't see a call from another file, which is why it only judges private functions.

## Tests

- **`tests/after_a_restart.rs` (8 tests):**
  - work is written down and crossed off;
  - after a restart, the council is redone once, mail and research are named, and it's said once;
  - chores aren't recorded;
  - a window job is picked back up;
  - a closed window is dropped and said;
  - Atlas's own typing is left out of your idle time, including across the 49-day clock wrap;
  - phases are found again, and a half-written one isn't;
  - project work carries on from its last finished phase with zero model calls, then clears its phases.
- **`tests/runbooks_and_what_comes_next.rs` (10 tests):**
  - steps say what to do if they fail;
  - the checklist;
  - the look back asks why, not who;
  - learned snags survive a restart;
  - "that's wrong" after a walk-through teaches the procedure and files the note;
  - a decision waiting on you is said before older failures;
  - top-few picking;
  - never-called functions found, trait methods not;
  - made-up packages found against Python, JavaScript and Rust manifests;
  - a manifest found above the file;
  - every finding has a Fix line.
- **Tests moved to the live path:** the tests of removed duplicates now test the function the running program actually uses.

## Still open, and yours

- **The laptop, unlocked:**
  - re-run typing with the fixed build (`atlas window type notepad.exe hello`);
  - "look at my screen";
  - the camera features;
  - a real call.
- **The 145 rulings,** when you want to go through them. The triage table groups them.
- **The merge** and **the hub:** waiting on you.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
