# Memory, storage, and choosing a backend

> **STALE — the numbers describe a much smaller tree.** It says 195 tests. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Three problems that all come down to the same thing: Atlas is a guest on your
laptop and has to act like one.

---

## 1. Does that 180MB come back?

**Only when the process exits.** A running headless Chrome holds its memory
until it is killed. Windows will page it out under pressure, which is *worse* —
now you pay disk I/O every time it wakes up.

So Atlas doesn't hope. It manages:

- **Nothing heavy starts until it is needed.** No browser process exists until
  you ask for something that needs one.
- **Idle helpers are reaped.** 90 seconds by default; the browser gets 300
  because research arrives in bursts and a cold start is about a second.
- **A helper mid-job is never killed**, however long it runs.
- **There is a hard ceiling** — 600MB across all helpers. Exceed it and the
  least recently used one is stopped, even if it hasn't timed out.
- **If nothing can be freed, Atlas refuses** rather than going over. It is not
  entitled to unbounded memory on your machine.

Steady-state cost with nothing happening: the Atlas process itself, a few MB.
Everything else is transient.

---

## 2. Teaching it which backend to use

You said you didn't know how to teach it this. You don't have to — it isn't a
training problem, it's a matching problem plus feedback.

Each backend **declares what it can do**:

| backend | can do | memory | steals focus | acts on |
|---|---|---|---|---|
| **CDP** (DevTools) | read, click, scroll, type, fill, navigate, wait | 180MB | no | Chrome only |
| **UIA** (accessibility) | read text, read title, click, type | 25MB | rarely | your open windows |
| **Hidden desktop** | everything | 300MB | never | its own copy |
| **SendInput** | click, scroll, type | 0MB | **always** | whatever is focused |

A request carries its constraints: what capability is needed, which app, whether
Atlas may take the screen right now, and whether it must act on *your* window
rather than a fresh copy. Backends that can't satisfy those are eliminated
outright. What remains is scored on **proven reliability against measured cost**.

Then it learns. Every attempt is recorded per backend *and per app*, because a
backend can be excellent in one place and useless in another — UIA reads Notepad
perfectly and returns an undifferentiated blob for Discord. After enough
failures on Discord specifically, UIA stops being first choice there and stays
first choice for Notepad.

Three deliberate constraints on that learning:

- **One failure is not a verdict.** Nothing is demoted until there are at least
  four attempts. Otherwise a single stumble swaps a 25MB backend for a 300MB one.
- **Cost is part of the score, not a tiebreaker.** An untried backend carries an
  optimistic prior, so ranking on success rate alone would let a heavyweight
  option outrank a light one that failed once. Both go in the same number.
- **Nothing is written off permanently.** If a distrusted backend is the only
  one that can do the job, Atlas tries it anyway. Refusing outright is worse.

`forget_app("discord")` resets what was learned — useful after an app updates
and its accessibility support changes.

Every choice explains itself: *"Cdp: 4/4 successful on chrome (100%)"*.

---

## 3. Storage that doesn't grow forever

**The governing principle: store pointers, not payloads.**

Atlas never copies your documents into its own store. The index holds a path, a
size, and a timestamp — roughly 200 bytes per file, so 200,000 files is about
40MB. When you ask what's in a document, Atlas re-reads the original. Recall
costs nothing to keep, because the data is already on your disk. Duplicating it
would have been the expensive mistake.

What actually grows is what Atlas *generates*:

| class | example | policy |
|---|---|---|
| Scratch | turn wav files | deleted after 10 minutes |
| Captures | screenshots, webcam frames | deleted after 24 hours |
| Logs | atlas.log | rotates at 4MB, two files max |
| Notes | research briefs | kept a year — small text, high value |
| State | memory, index, queue | compacted, never bulk-deleted |

A 500MB total ceiling sits over all of it. Age limits alone usually keep things
well under. If the total is still over, eviction is oldest-first by class:
scratch, then captures, then logs. **Notes and learned state are never evicted
for space.** If those alone exceeded the budget you'd have a configuration
problem, not a cleanup problem, and Atlas reports that rather than quietly
deleting what it learned.

### Recall without hoarding

Approval history is the one thing that grows purely from use. After 200
records, older ones collapse into running totals per action kind. The ratio and
the count are preserved exactly — which is all anything actually consults — and
the per-event detail is dropped. Behaviour is identical before and after; only
the space changes. Tested: 400 records compact to 50, and both the approval
rate and the total count come out unchanged.

Conversation history works the same way: turns fold into workflow memories
(trigger, steps, times used) rather than being kept verbatim.

---

## What is built vs. what is designed

**Built and tested here:** the routing logic, the learning, the lifecycle
supervisor, the retention planner, and approval compaction. 195 tests.

**Not built:** the individual drivers. CDP, UIA, and the hidden desktop are
declared in the registry with their real costs and constraints, and the router
will select between them correctly — but the code that actually speaks DevTools
Protocol, walks an accessibility tree, or calls `CreateDesktop` does not exist
yet. The scaffolding is ready for them; they are each a real piece of work.

The selector itself is wired as of 16 Sep, on its reading side only: the router
records which apps the accessibility path can read, persists what it learns, and
`atlas backends` reports it. The acting side waits on the `delegate` ruling.
