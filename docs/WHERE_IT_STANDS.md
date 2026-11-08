# Where Atlas stands

> **STALE — the numbers describe a much smaller tree.** It says 108 modules, 27,000 lines and 1,261 tests. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

*A breakdown of what exists, what's blocked, and what's left.*

---

## In one paragraph

Atlas is a local-first assistant for your Windows laptop, written entirely in
Rust across **108 modules and 27,000 lines**, covered by **1,261 tests**. It
depends on four crates — three of them for reading YAML and JSON. There is no
async runtime, no HTTP library, no web framework; the HTTP server and WebSocket
handling are about 300 lines of hand-written `std::net`. It costs nothing to
run and works with the network unplugged.

---

## The shape of it

**Four crates.** `serde`, `serde_yaml`, `serde_json`, `thiserror`. Every dependency
is a supply-chain risk and a thing that can break your build in two years, so
there are as few as the job allows.

**Offline by construction.** Exactly one intent is classified as needing the
internet: research. A test asserts every other one is local. The internet is an
enhancement, never a dependency.

**Nothing costs money.** whisper.cpp, piper, ffmpeg, llama.cpp and Tesseract are
all MIT or LGPL; the models are plain downloads with no account. A test asserts
no download URL contains a key or token. The only paid path is a hosted model
for overnight code work, and it is off.

**944 lines of config**, all in plain language with the cost of each setting
stated next to it.

---

## What it can do

### Hearing and speaking
- Wake word, continuous listening, push-to-talk
- **Endpointing** — stops when you stop talking, and how long a silence has to
  be depends on what you said: "yes" ends in 380ms, "I want you to" waits 1.1s
- **Dictation** into the focused window, with punctuation read in context, so
  "a period drama" survives intact
- **Seven voices**, changed by asking: "a bit slower", "use a British voice",
  "you sound robotic"
- **Accents** — measures how well it's hearing you and suggests a bigger model
  only on evidence
- **Translation** — ninety languages, in the same pass, for nothing
- Recordings are deleted the moment a transcript exists

### Knowing what you mean
- **"Explain this"** with an empty clipboard resolves against what you selected,
  what's in front of you, what you just saved, and what you were last discussing
- Two equally likely candidates produce a question, not a guess
- **Refuses to answer when it doesn't know** — catches invented specifics and
  claims about your machine it never checked

### Doing things
- Window arrangement, app launching, layouts
- **Changing your machine** — wallpaper, files, appearance settings — sorted by
  how hard each is to undo, never touching security, accounts or networking
- **Looking after the laptop** — what's holding memory, what's slowing startup,
  what's safe to clear, with the actual numbers
- Posting with approval that is re-verified at the moment of sending
- **Routing around failure** — eighteen known ways in; when one is closed it
  picks another, weighing cost against how often it works

### Thinking
- **Twelve distinct approaches** to a stuck problem, not three retries
- **Remembers what didn't work**, with lessons that expire by cause
- **Time-boxed** — over budget it asks rather than killing the work, and
  running out still returns what it had
- **Explains its decisions** — "because the left screen is widest, and that's
  set in config/layouts.yaml"
- **Search over everything you've written**, working with no model installed

### Writing
- Catches what's wrong: throat-clearing, hedging, filler, repetition, monotony
- Judges whether it **says anything**: a claim, support, the objection answered
- Support is weighed rather than counted — something measured beats a citation

### Money
- Reads statements locally, no credentials
- Knows the rules that catch traders: wash sales, Section 1256, trader tax
  status, the mark-to-market deadline, the capital loss limit
- Says plainly that this is knowledge and not advice

### Itself
- Works on its own source in a sandbox, runs all 1,261 tests, and **explains
  changes as behaviour rather than as a diff**
- Cannot edit the files that decide what it's allowed to do
- A change that passes by deleting tests is refused, with the count

### What you see
- A **transparent overlay** drawn on the desktop — no window, no background —
  with text typed a character at a time
- Free-standing panels, full height down the right of your screen
- The mark is a **line under tension**, hanging in a catenary: still when idle,
  one wave when thinking, irregular and warmer when speaking
- Settings reachable when Atlas isn't, via `RUN-SETTINGS.bat`

---

## What's blocked, and on what

**On your machine.** Everything here compiles clean for Windows and has never
executed there:

- The Win32 overlay drawing — layered window, typed text, the mark
- Live window management through the real Windows API
- Chrome control, the accessibility tree walker, the Tab keyboard hook
- Voice identification and presence — needs models installed

**On one file.** Whisper. `setup/install-voice.bat` fetches it, and until it
runs Atlas cannot hear you. Everything works typed in the meantime.

**On a source I can test against.** The calendar. Every other Phase 2 item is
done; this one needs a real calendar to be worth writing.

---

## Where the phases stand

| | |
|---|---|
| **Phase 0** — foundations | done |
| **Phase 1** — the basics of being useful | done bar reading the document you're looking at |
| **Phase 2** — knowing your world | done bar the calendar |
| **Phase 3** — doing real work | draft/critique, time-boxing, learning from failure, routing done |
| **Phases 4–8** | media, presence, an iPhone client, hardening |

---

## The honest gaps

**It has never heard your voice.** Every hearing and speaking path is tested
against fixtures, not against you. The first real conversation will find things.

**The Windows layer is compiled, not run.** It cross-compiles with zero errors
and zero warnings, which catches type errors and nothing else.

**The local model isn't installed**, so everything that would use one falls back
to rules. Those rules are good — the punctuation reader, the route chooser, the
draft critic all work without a model — but they are rules.

**1,261 tests is not the same as correct.** They're written as sentences about
behaviour, which makes them honest about what they check. They still only check
what I thought to check.
