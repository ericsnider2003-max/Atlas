# Session — 23 September 2026 (late): pausing one errand, and knowing which

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
Follows `15_SESSION_2026-09-23_remainder_wired.md`.

Eric's ruling (B1, doc 13 §3): a single-errand control **pauses, it doesn't
erase**. He added a requirement: *"if I say stop and Atlas is in the process
of several tasks I need a way to determine and tell Atlas which task is
getting paused and which ones continue. Maybe we can use some sort of context
here?"*

## What was missing

Errands used to run a closure on a thread with a single stop flag. There was
no way to pause one, and nine of the eleven errand kinds never checked the
flag anyway. A bare "stop" fell through the command parser and did nothing.
"Pause" froze Atlas itself but left every background errand running.

## What was built

**Errands that can hold (`crew`).** Each errand now gets a `Control` instead
of the bare flag. `Control::checkpoint()` is a safe point: if the errand is
paused, it parks right there. It keeps everything it has done so far, because
that work is still on its own thread's stack. When it is resumed, it carries
on from the next step. It does not start over. A test proves this: an errand
that counts to 60, paused partway and then resumed, reports exactly "60
steps". Nothing is lost and nothing is repeated. Other parts of the new
behaviour:

- **Slots:** a parked errand gives up its hand, so queued work can run while it
  waits.
- **Queued errands:** an errand paused before it ever started keeps its place
  in the queue until it is resumed.
- **Stop:** stopping a held errand wakes it up and ends it.
- **`Crew::errands()`:** reports each errand as Running, Pausing (heading for
  its next safe point), Holding, Waiting, or paused while waiting.

**Safe points in the errands that take a while:**

- research: between sources, and before the write-up
- council: between seats
- web and code builds, and project changes: between rounds
- mail check and unsubscribe: between accounts
- outreach: before drafting
- Outlook sign-in: each time it checks back

Backup and housekeeping run as a single step. They can't hold, and Atlas says
so instead of claiming it paused them.

**Which one did you mean (`which_errand`).** When you say "stop" (or "pause
the …", "hold …", "put … on hold"), Atlas decides which errand you meant in
this order. It moves to the next step only if the one before settles nothing:

1. **Named.** "stop the research", "pause the Postgres one". It matches words
   for the errand's kind or for what it was asked about.
2. **Numbered.** "the second one", "the last one", counted oldest first, the
   same order Atlas reads them out in.
3. **Only one is running.** There's nothing to choose between.
4. **The conversation.** The last few things you said name exactly one of
   them.
5. **Just started.** One errand began in the last 90 seconds and no other
   did. "Stop" straight after asking for something means that thing.
6. **Ask.** Atlas replies "Two things are going: 1) the research on …, 2) the
   build. Which should I pause — a number, a name, or 'all'?" Your next line
   answers it: "2", "the build", "both", or "never mind".

Every answer says what Atlas paused, why it picked that one when it was a
guess, and what is still going. For example: *"Paused the research on
Postgres pricing — it holds at its next safe point with nothing lost (it's
what you were just talking about). Still going: the build. If I picked the
wrong one, say 'no, the …' and I'll swap them."* If you say "no, the build"
right after that reply, Atlas puts the research back to work and pauses the
build instead.

**Guesses only ever pause.** Steps 4 and 5 are used only to pause, because a
pause loses nothing and can be undone. "Cancel" / "call off" / "scrap" /
"kill" / "drop" end an errand for good. They need a name, a number, "all",
or an answer to Atlas's question, and they are never decided from context.

**The other controls:**

- **Resume:** "carry on with the research" resumes that one errand. A bare
  "carry on" resumes the only paused errand, or asks which one if several are
  paused.
- **Whole-Atlas "pause":** this is still total, and it now holds every errand
  as well. The "carry on" that ends it releases only those errands. One you
  paused on its own stays paused.
- **"What's queued":** now names errands on hold and whether each one is
  holding or still heading for a safe point.
- **Emergency stop:** "stop everything", the emergency phrase, is unchanged.
  It still abandons everything, as Atlas teaches it at setup.

## Named gap

A pause does not survive Atlas closing. A held errand lives on its own
thread, and the thread ends with the process. Atlas no longer loses them
silently: on the way out it names every paused errand and asks you to request
it again. For a pause to survive a restart, each errand kind needs its
progress written down in a form it can pick back up (research: the sources
already read; council: the seats already answered). That would be a
per-errand feature. It isn't built.

## Guards

- **`crew::ask_to_stop`:** it now has a real caller, so it is off both
  `ORPHAN_METHODS` (8 → 7) and KNOWN. `TEST_ONLY_MAX` 256 → 255.
- **New module:** `which_errand` is claimed at birth by a new `which_errand`
  capability, which also claims `attention`. `MODULES_IN_TREE` 292 → 293,
  `UNCLAIMED_MAX` 174 → 173, and `docs/CAPABILITIES.md` was regenerated.
- **`crew::stopping`:** has a real caller. A council stopped between seats now
  reports "stopped before every seat answered" instead of a room that failed to
  agree.
- **Bare-name false positive:** a local closure named `ids` made
*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
  shows as having no caller, which is correct.

## Tests

`tests/errand_pause.rs` has 17 tests covering:

- **The crew:** hold then resume with nothing lost, stop while held, a held
  errand giving up its hand, and a queued errand paused before it started.
- **The rules:** named, numbered, all, conversation, just-started, ask, never
  cancelling on a guess, and resume only looking at paused errands.
- **The wording:** what was paused, what's still going, how to correct a guess,
  and an errand that can't hold.
- **Through the daemon:** named, asked then answered, a wrong guess swapped,
  whole-Atlas pause and resume, and a named cancel that leaves the other
  errand alone.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
