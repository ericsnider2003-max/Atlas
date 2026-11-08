# Where Atlas is, and what's left

> **STALE — the numbers describe a much smaller tree.** It says 2,220 tests and 64 unreachable modules; the tree has 5,637 tests and one module on `UNWIRED_BASELINE`. Last true on 14 September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Written from the code rather than from memory — every number here comes from a
test that runs.

**2,220 tests. Zero warnings. Windows cross-build clean.**

---

## The honest number: 64 modules still unreachable

Down from 96 when the wiring audit first ran. It can only go down — the ratchet
fails the build if it grows. But 64 is 64, and pretending otherwise is the
thing that got us here.

They split into four groups, and only one is work I can do from here.

### Ready to wire — 34

Nothing blocking these but a caller. Roughly in order of what you'd notice:

`recall` `research` `draft` `edit` `content` `publishing` `grade` `reach`
`prose` `plainly` `voiceover` `editors` — the writing and content chain.

`person` `wanted` `wants` `answering` `returning` `routine` — the ones that
make it feel like it knows you.

`overnight` `delegate` `timebox` `diagnose` `budget` `chain` — background work
and not getting stuck.

`system` `mail` `credentials` `identity` `consent` `profiles` `firstrun`
`language` `metrics` `otherside` — everything else.

### Needs your machine — 12

`overlay` `look` `uia` `watching` `presence` `voiceid` `hearing` `audio`
`dictate` `endpoint` `quickinput` `ocr`

These compile and are tested and have never touched Windows. They can't be
honestly wired until they've run once — that's what "built, never run for real"
means in the capability list, and it isn't a formality.

### Needs a model — 6

`gguf` `models` `backends` `adapt` `fit` `improve`

Blocked on the local model being installed. `INSTALL.bat` fetches everything
except this one, because 4.4GB shouldn't stand between you and a working
install.

### Needs a second device — 12

`ios` `android` `companion` `cloudsync` `mesh` `sync` `remote` `workingset`
`vault` `signin` `confirmed` `codes` `walkthrough` `accounts` `household`
`afterme` `recovery`

Real code, no second device to talk to.

---

## What's actually blocking you

**One install unblocks five capabilities.** `whisper` turns hearing,
endpointing, dictation, accent handling and translation from *waiting* into
*working*. Nothing else unblocks more than one.

That's the whole first-boot path. Until it runs once, everything about voice is
theory.

---

## Phases

**Done and wired:** the day as a unit, the workspace and board, capture that
files itself, undo across everything, the interruption gate, subject
resolution, certainty, procedures, sign-in with per-site grants, the account
audit, travel preparation, the five-stage self-work loop, self-audit, standing
grants, outsourcing when stuck, the local reference shelf.

**Done, waiting on hardware:** windows, panels, the overlay, dictation, wake
word, screen reading.

**Done, waiting on a device:** everything to do with the phone.

**Not started:**

| | why it's not done |
|---|---|
| The phone app | needs a decision on Shortcuts vs PWA vs native — see `GETTING_IT_ON_A_PHONE.md` |
| In-depth market knowledge | the shelf is built; the material isn't fetched |
| Meeting prep, site routines | need real calendars and sites to test against |
| Email triage against a real inbox | needs an app password |

---

## The next three things, in order

**1. Run `INSTALL.bat` and `RUN-DOCTOR.bat`.** Everything about voice is
unverified until then, and a first run always finds things a test can't.

**2. Wire the 34 that are ready.** That's the largest honest chunk of remaining
work and it needs nothing from you.

**3. Fetch the reference material.** About 33MB for all the trading knowledge —
contract specs, options mechanics, trader tax, forex conventions, crypto
mechanics. The encyclopaedia is 4GB and it's the least useful part.

---

## What holds the line

Five checks run on every build and fail it:

- Nothing unreachable can be added without being declared
- Nothing claiming to be non-configurable can be set from YAML
- No test asserts nothing
- Documentation-shaped tests can't grow past 37
- Every module with behaviour has a test that calls it

And the capability list is bound to the wiring audit, so it can't claim
something works when nothing can reach it. That was the worst drift in the
system and it can't come back.
