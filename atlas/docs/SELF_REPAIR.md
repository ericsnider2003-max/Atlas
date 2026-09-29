# When Atlas gets stuck

> **STALE — the numbers describe a much smaller tree.** It says 938 tests. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

## The loop

1. Something fails — a test, a command, a capability that stopped working.
2. Atlas reads the actual error and forms a theory.
3. It writes a change **into the sandbox**, never your files.
4. It runs its own 938 tests.
5. If they pass, it shows you the diff and asks whether to keep it.
6. If they fail, it takes a **different angle** and tries again.
7. When the angles run out, it writes up a brief.

## Two limits that sound alike and aren't

- **`strategy.max_angles`** (12) — how many different approaches Atlas tries on
  **one problem** before writing it up. This is the one that went from 3 to 12.
  **Not yours to tune yet:** `strategy.rs` reads `max_angles` correctly, but
  the `strategy:` section of your `tools.yaml` never reaches it, and
  `selfwork`'s `begin`/`after` have no production caller — so the ladder is
  not driven. The number below is the built-in one.
- **`overnight.give_up_after_failures`** (3) — how many **separate problems**
  can get nowhere in a row before Atlas stops for the night. Three different
  problems all failing means something is wrong with the setup, not that the
  problems are hard.

They're different things, and only the first one changed.

## Twelve attempts, not three

"Three attempts then give up" was a bad rule. Three attempts only means
anything if they are three *different* attempts, and left to itself a model
will retry the same idea with the wording changed and call it a second try.

So Atlas works through a ladder of genuinely distinct approaches, each used
once:

| | angle |
|---|---|
| 1 | Read the error again, word for word — the answer is often in the message |
| 2 | Check the thing being blamed is the thing that changed |
| 3 | Test the *assumption* the theory rested on, not the theory |
| 4 | Find somewhere in the codebase that does this correctly, and compare |
| 5 | Add logging and look, instead of reasoning |
| 6 | Cut it down to the smallest thing that still fails |
| 7 | Bisect the failing path |
| 8 | Look at what calls this |
| 9 | Look at what this calls |
| 10 | Question whether the *test* is wrong rather than the code |
| 11 | Write the obvious brute-force version — maybe the cleverness was the problem |
| 12 | Revert everything and start from the last working state |

Cheap and often-right first; revert-and-rethink last. It stops early if three
attempts in a row hit the identical error, because at that point the angles
aren't reaching the problem and eight more won't change that.

Everything learned along the way is kept, even when the campaign fails. That's
the useful half of a failed attempt, and it's what makes the brief worth
reading rather than a list of shrugs.

## The handoff

Step 7 is the part worth explaining.

## The brief

A bad request for help gets a question back. A good one gets an answer first
time. Atlas writes the good kind:

- **What it was trying to do** — intent before symptom
- **What happens** — the error, verbatim
- **What it already tried** — each theory, each change, each outcome
- **What it ruled out** — so nobody suggests it again
- **Its best guess**
- **The test output**
- **Only the code that matters**, with line numbers

Then you paste it into a chat, paste the answer back, and Atlas pulls the code
out — including which file each block belongs to — applies it to the sandbox,
and runs the tests. If they pass you get a diff. If they don't, it says so
rather than pretending.

It handles the two normal non-answers too: a question back isn't a failure, and
an explanation without code gets a follow-up asking for the change itself.

## Where the brief goes

By default, to you. You paste it, paste the answer back, and Atlas takes over
again.

You can also delegate that step, which is what `brain: delegate` does. Name a
window that already has the conversation in it, and Atlas carries it on the
same way it would if you'd stepped out of the room mid-thread. It is the same
`delegate` machinery, pointed at a different app.

### What it says when it gets there

It opens with the context and the ask, then attaches the write-up:

> Pause all other tasks. I ran into an issue while running diagnostics — the
> details are in the write-up below. I've already tried a number of approaches;
> they're listed with what each one showed. What I need is the change itself so
> I can apply it and run the tests.

Then it waits for the **whole** reply. A reply still arriving looks like a
short reply, and acting on half of one is how you apply the first paragraph of
a two-paragraph fix. Atlas waits for the text to stop changing for a couple of
seconds before reading it.

### An attempt is spent on a solution, not on a message

This is the part that makes the budget mean something.

| what comes back | costs an attempt? | what Atlas does |
|---|---|---|
| Code to try | **yes** | applies it in the sandbox, runs the tests, reports what happened |
| A question | no | answers it and carries on |
| "Show me what's in X" | no | pastes it |
| An explanation, no code | no | asks what specifically to change |
| Still arriving | no | waits |

So a conversation can run for a dozen messages and use two attempts, which is
how it should be — most of a good debugging conversation is establishing what's
actually true, not proposing fixes.

Every result is reported back, pass or fail, with the real error. That's what
makes it a conversation rather than a series of guesses.

A separate ceiling stops a discussion that's going nowhere politely:
`max_exchanges`, default 20.

### The three bounds

- **`delegate_turns_each`** — solutions tested on one problem. Default 6.
- **`delegate_turns_total`** — across the whole night. Default 40.
- **A named window.** With none set, it stops. It continues a thread that
  already exists and never goes looking for somewhere to start one.

The turn budget is what makes this a delegated task rather than something else,
which is why it's small and why running out of it ends the night rather than
escalating.

Two things I have deliberately not built, and won't: anything that retries
around a rate limit, and anything that spreads work to get more throughput than
a person would. Those would turn a delegated conversation into a workaround.
Where the line sits precisely is Anthropic's call — their terms are the
authority, not my judgement — but that is where I'd put it.

**It's also better this way, for a reason unrelated to any of that.** You see
the problem before it's asked about and the fix before it's applied. Atlas has
done the tedious part — reproducing, narrowing, gathering context, ruling
things out — and left you the ten seconds that need a person.

## What it costs

Nothing. The diagnosis, the sandbox, the tests and the brief are all local.
The only paid path is the one in `COST.md`, and it stays off unless you turn
it on.
