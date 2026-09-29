# Integration audit

> **STALE — the numbers describe a much smaller tree.** It says 49 modules and 426 tests. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Prompted by a fair question: *"how does a scheduled post go out if I'm not
there?"* It didn't. Chasing that found something larger.

---

## The finding

**49 modules built. 426 tests passing. The daemon called 18 of them.**

Ten modules were wired to nothing at all — reachable only from their own tests:
`addressing`, `attention`, `backends`, `categories`, `delegate`, `delivery`,
`edit`, `flow`, `references`, `uia`. Several more (`publish`, `backlog`,
`lanes`, `research`, `retention`, `lifecycle`, `browser`, `connectivity`,
`grants`) were referenced by config but never invoked at runtime.

**Root cause:** each round produced a subsystem and its tests, and each round's
tests passed, so nothing ever failed to signal the gap. Unit tests verify that
a part works. They cannot tell you the part is never called. Test count was
measuring the wrong thing — and that gap between "tested" and "connected" is
precisely what would have made it feel sloppy in use: every individual thing
correct, the whole thing inert.

---

## Fixed this round

### Scheduled posting, unattended

The authorization rule, stated plainly:

> **Approving a post at schedule time IS the consent to send it at its time.**

That is the entire point of scheduling. Requiring you present at 7am would make
the feature pointless. What that consent covers is deliberately narrow and is
re-verified at send time: *this exact text, to this channel.* Editing after
approval voids it. Length and media rules are re-checked. A blocked post lands
on the outstanding list rather than retrying forever.

Also added: a post that *cannot* go is flagged **once**, ahead of its send
time — not every hour until you deal with it.

### Pause reaches the whole system

"Pause" and "hold on" are now heard before anything else, so they work
mid-task. While paused the tick does nothing at all — no jobs, no posts, no
proactive offers — while still listening for "I'm ready". Suspended jobs are
handed back on resume.

### Overheard speech no longer interrupts work

Every utterance is assessed for whether it was aimed at Atlas before it reaches
the command path. A phone call beside the desk returns silence and the job
keeps running.

### Follow-ups resolve

"Close it" resolves against the last app acted on. A pronoun with no referent
asks instead of guessing.

### Blocked work is filed

Anything needing the internet while offline lands on the outstanding list
automatically. "What's outstanding?" and "what's queued?" both answer.

### Housekeeping actually runs

Hourly: approval history compacted, backlog tidied, finished jobs pruned, stale
captures deleted under the retention budget, idle helpers reaped.

---

## Two real bugs the integration tests caught

Neither was visible from any unit test.

**Connectivity never recorded an assumed-offline state.** `assume_offline`
returned `Offline` but returned *early*, without storing it — so `cached()`
stayed `Unknown` and blocked work was silently never filed. The feature looked
correct in isolation and did nothing in place.

**A cached probe overrode explicit intent.** Telling Atlas the connection state
worked until the 30-second cache expired, then the probe took over. Now pinning
survives cache expiry, which also handles a captive-portal network where the
TCP probe succeeds while nothing actually works.

---

## Second pass — most of it now reachable

The first pass wired 28 of 49 modules. This one took it to 43. Everything
below is now reachable from something you can say:

| module | how you reach it |
|---|---|
| `thread` | every turn — one conversation, folded not dropped |
| `persona` | every reply — filler stripped, capped, no markdown aloud |
| `modes` | "focus mode", "I'm on a call", "research mode" |
| `health` | "how's the machine", and unprompted when urgent |
| `watch` | automatic, alerts when another machine drops |
| `anticipate` | automatic, queues prepared work |
| `safety` | "undo that", "back up", plus a daily backup |
| `lanes` | every command routes to a lane |
| `flow` | say the name of a sequence you saved |
| `activity` | "while you were away" on return |

### Four real bugs this pass found

None were visible from any unit test.

**`queue_busy()` was a stub returning `false`.** I flagged it as a stub last
time. It meant the "are you working" signal used by addressing and the
foreground lane was always wrong.

**The persona ate leading numbers.** Stripping markdown list markers dropped
any digit at the start of a line, so *"1 scheduled, 0 awaiting you"* became
*"scheduled, 0 awaiting you"*. The count is the whole sentence.

**Trash used a fixed relative path.** Two Atlas instances, or two runs from
different folders, shared one trash folder and one ledger.

**Pronoun resolution ran too early.** "Undo that" contains a pronoun but is a
complete instruction; Atlas answered "Which one?". Fixed by checking whether
the *argument* is a pronoun rather than whether the sentence contains one —
so "close it" still asks, and "undo that" doesn't.

## Still unwired — ranked

These remain built and tested but not reachable from a spoken command.

| priority | module | what it needs |
|---|---|---|
| **1** | `delivery` → `browser` | the send call itself. Plan is built and gated; nothing clicks Post yet. |
| **1** | `delegate` | needs UIA or capture for screen reading, plus typing. "Finish the conversation for me". |
| **1** | queue drain | commands reach the lane queue; nothing executes them from there yet. |
| **2** | `edit` | an `edit_video` intent and the planner call. Fully built underneath. |
| **2** | `research` | reaches an intent but runs inline rather than in the background lane. |
| **3** | `grants` | asking before using an unknown app. Every app is currently treated as configured. |
| **3** | `backends` router | nothing yet chooses between CDP / UIA / SendInput at runtime. |
| **3** | `server` | built and tested; the daemon does not start it yet. |
| **4** | `uia`, `voiceid`, `presence` | all need a live model or the COM walker. 🔒 |
| **4** | `categories` | classification exists; nothing consults it before an action. |

---

## Quality gaps worth naming

Things that would feel rough even once everything is connected.

1. **No barge-in.** You cannot interrupt Atlas mid-sentence; you have to wait
   for it to finish talking. This is the single most noticeable roughness in
   any voice assistant.
2. **`queue_busy()` is a stub returning false.** Written as a placeholder while
   wiring; it makes the "are you working" signal less accurate than it should be.
3. **Fixed 8-second recording.** No endpointing, so short answers wait the full
   window. Deliberate for reliability, but it will feel slow.
4. **No streaming speech.** Atlas composes the whole reply before speaking, so
   long answers have a silent gap first.
5. **Grant prompts have no memory across restarts** beyond `Always`.
6. **Nothing reports what it did while you were away.** After an unattended
   stretch there is no "here's what happened" summary.
7. **No undo.** Every destructive action is gated, but nothing can be reversed
   after the fact.

---

## What changed about how I'm measuring

Test count was the wrong metric. The new integration suite tests the *assembled*
system: a scheduled post firing with nobody present, a phone call not
interrupting a job, "close it" resolving, pause stopping the tick, everything
surviving a restart. Those are the tests that fail when something is built but
not plugged in.
