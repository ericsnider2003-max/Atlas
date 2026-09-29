# Gaps you haven't raised

> **STALE — a dated list, partly closed since.** Several gaps here have been closed. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Researched, then reasoned through. These are things that decide whether Atlas
feels finished or feels like a prototype, and none of them have come up yet.

---

## A. You asked for this in your very first message and I never built it

**Speaker verification.** Your opening message said you wanted a system that
"only works on my commands... Voice/recognition?" I answered that voice ID is
weak as authentication — which is true — and then never built it even as the
convenience filter it should be.

The honest design: not a lock, a filter. A voice embedding of you, compared
against each utterance. A confident non-match means Atlas stays quiet rather
than refusing loudly. It stops the TV and houseguests triggering it. It does
**not** stop a recording of you, so it must never be the thing standing between
a stranger and a consequential action — that stays with the approval gate.

Free and local: `pyannote` embeddings, or a small ECAPA-TDNN model. Roughly a
day's work, and it closes a request you made at the very start.

---

## B. Things that decide whether it feels finished

| gap | why it matters |
|---|---|
| **Endpointing** | Recording for a fixed 8 seconds is *the* most-copied mistake in voice tutorials, and it cuts people off mid-word. The fix is voice-activity detection with a silence threshold. Too short and you get cut off mid-thought; too long and it feels laggy. Production targets sit around a 200–400ms turn gap. |
| **Latency budget** | Nothing in Atlas measures how long a turn takes. The bar for feeling conversational is well under a second end to end, and you cannot tune what you do not measure. Per-stage timing — record, transcribe, think, speak — is a small change with a large payoff. |
| **Model warm-up** | Loading the speech model per turn adds seconds every single time. Load once at start, warm it before you need it. |
| **Streaming speech** | Atlas composes the whole reply before saying a word, so a long answer starts with silence. Speaking the first sentence while composing the rest removes that. |
| **Conversation windowing** | Session history grows unbounded; both cost and time-to-first-word grow with it. Window it or summarise older turns. |
| **Pronunciation** | Local TTS will mangle product names, tickers, and acronyms. A small pronunciation dictionary fixes it and is trivial to build. |

---

## C. Operational things that only hurt once

- **No backup of learned state.** Months of approval history, workflows and
  scheduled posts live in one folder with no copy. A bad disk loses all of it.
- **No undo.** Everything destructive is gated, but nothing is reversible after
  the fact. A trash folder for anything Atlas deletes or overwrites would cost
  little.
- **No disk-full handling.** Writes fail silently into `let _ =`.
- **Time zones and DST.** A post scheduled for 9am will drift by an hour twice
  a year, and Atlas stores plain Unix seconds with no zone.
- **No first-run setup.** `doctor` diagnoses, but nothing walks you through a
  first configuration.
- **No self-repair.** When a helper dies, nothing restarts it.
- **Clock jumps.** Sleeping the laptop moves the clock forward hours; scheduled
  work should notice rather than firing a backlog at once.

---

## D. Security posture, given this runs all day with a microphone

- **No encryption at rest.** Notes, transcripts and the index sit in plain JSON.
  Windows EFS or a passphrase-derived key would cover a stolen laptop.
- **Nothing distinguishes you from a recording of you.** Relevant if voice ever
  gates anything consequential — it currently does not, and it should stay that
  way.
- **No rate limit on consequential actions.** A confused loop could try to post
  fifty times. A ceiling per hour is cheap insurance.
- **The audit exists now but is not tamper-evident.** Fine for personal use;
  worth knowing.
- **Isolation from a trade-execution machine is still only advice.** It was in the first audit
  and remains unenforced — nothing stops a future connector reaching it.

---

## E. Capabilities you'd probably want and haven't mentioned

- **Calendar and meeting awareness.** "What's next?", moving a call, not
  interrupting during one. Atlas has scheduling but no notion of *your* diary.
- **Reading a document you point at.** "Summarise the PDF I just opened" —
  needs Layer 2 awareness, which is built as a concept but not wired.
- **Clipboard as an input channel.** Copy something, say "explain this."
  Trivial to add and constantly useful.
- **Dictation.** Not commands — actually typing what you say into the focused
  window. You have every piece of this already.
- **Named workspace modes.** "Trading mode", "writing mode" — different app
  sets and layouts. The layout engine already supports it; nothing names them.
- **A daily brief.** You already have research, scheduling and the journal.
- **Multi-machine.** If you get a desktop, Atlas has no story for two of them
  sharing state.

---

## What I need from you to finish INTEGRATION.md

Four things, in order of how much they unblock:

1. **A pass on the laptop.** `cargo test`, then `atlas doctor`. That single run
   converts five 🔒 items into either working or a specific bug list: Windows
   window management, live Chrome, the UIA walker, the Tab keyboard hook, and
   `llama-server`.
2. **Your real app paths and monitor geometry**, which `doctor` prints for
   pasting.
3. **A decision on speaker verification** — worth the day, or not?
4. **Whether the trading machine is the same laptop.** It changes the
   isolation advice from "keep them apart" to something concrete.

Nothing above is blocked on you *today* — I can keep building any of section B,
C, or E. But the 🔒 list only moves when the code meets the hardware.
