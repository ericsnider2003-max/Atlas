# Atlas — the whole system in one place

> **STALE — the numbers describe a much smaller tree.** It says 64 modules, 13,900 lines and 596 tests; the tree has 281 modules, 127,245 lines of source and 5,637 tests. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

**64 modules · 13,900 lines of Rust · 596 tests · 4 dependencies · builds clean
for Windows and Linux.**

---

## What it is

A local-first workspace assistant. You speak, it acts, it answers out loud.
It runs as a native process on your laptop — no browser tab, no window, nothing
listening on a public port, nothing to open before you use it.

Everything that makes it useful day to day works with the network cable pulled
out. The internet only adds reach.

---

## When something is wrong

The switches stay reachable when Atlas isn't. **`setup\RUN-SETTINGS.bat`**
opens the settings page directly and starts Atlas in a settings-only mode if it
isn't running — no microphone, no model, no daemon loop.

That matters because the whole point of a voice assistant is that you talk to
it, which is no help when the broken thing is the listening. Every permission,
every toggle and every threshold is on that page in plain language, so you
never have to remember what you turned on.

## How you reach it

Three tiers, with automatic fallback and automatic recovery.

1. **Voice** — a wake word, or hold Tab. Tab is watched for a *hold*, so
   ordinary tabbing passes straight through.
2. **Push-to-talk** — takes over after three consecutive voice failures.
3. **Typing** — always live, never a mode you switch into.

Every demotion is announced. It climbs back after five clean turns, so a
temporarily noisy room doesn't demote you permanently.

**Interrupting it:** only "stop", "pause", or "hold on" cut it off. A cough, a
colleague, or "mm-hmm" never will. Whatever gets cut off is never recorded as
having been said.

**Knowing it's you:** speaker verification decides whether Atlas *listens*,
never whether it's *allowed* — a recording of you sounds like you, so anything
consequential still needs a spoken yes.

---

## What it does

| area | state |
|---|---|
| Open, close, focus, arrange windows across monitors | built, runs on your desk |
| Voice in and out, entirely local | built, needs three binaries installed |
| Conversation, reasoning, follow-ups | built |
| Search files by name and by content | built |
| Web research → written note | built |
| Video editing from a description | built |
| Draft and schedule social posts and email | built |
| Browser control without API keys | built, unverified live |
| Watch machine health and another machine | built |
| Named modes: focus, call, research | built |
| Image generation, "put me in this photo" | possible on your hardware, not built |
| Video generation | not possible on your hardware |

---

## The ideas it's built on

These are the decisions that shaped everything else.

**Nothing is trusted twice.** Approval is checked by the publisher, again by
the delivery layer, and once more immediately before a post goes out. Editing
after approval voids the approval — otherwise "yes, send that" attaches to text
you never read.

**Time passing is not consent.** A scheduled post is re-checked at send time,
not trusted because it was approved once. But approving it *is* the consent to
send it at its time — needing you awake at 7am would make scheduling pointless.

**Learning only ever relaxes, never to silent.** Approve something consistently
and Atlas stops asking, but always announces. Anything on the `always_ask` list
is never promoted however often you agree. You control that list.

**Confidence decides, not rules.** Four states: do it, do it and say so, ask
which, ask permission. Unclear input while Atlas is working becomes a question
rather than a guess, because interrupting wrongly throws away work.

**Was that even for me?** Speech is assessed before it reaches the command
path. Half a phone call beside your desk returns silence and the job keeps
running.

**Two lanes, so you never wait.** Research and file work run in the background
immediately. Anything needing your screen queues until you're idle — because
synthetic clicks always go to the focused window, and there's no way around
that at the OS level.

**Store pointers, not payloads.** Atlas never copies your documents. The index
holds paths; content is re-read on demand. 200,000 files is about 40MB.

**Nothing is forgotten, nothing is nagged.** Anything Atlas couldn't do is
filed with its reason and offered back when the blocker clears — with escalating
gaps, and it gives up after four unanswered reminders.

**Nothing is gone.** Everything removed or overwritten goes to a trash folder
with a ledger. Undo refuses rather than overwriting newer work. Daily backups,
seven kept.

**One conversation, forever.** No sessions. Old exchanges are compressed into a
running summary rather than dropped. Coming back names what you were doing
instead of greeting you.

---

## What it depends on

Four Rust crates: `serde`, `serde_yaml`, `serde_json`, `thiserror`. No async
runtime, no HTTP client, no websocket crate — the WebSocket client for Chrome
and the HTTP layer are about 300 lines of `std::net`.

Four external programs it drives, all free and local: **ffmpeg** (audio and
video), **whisper.cpp** (speech to text), **piper** (speech), **Chrome**
(headless, for research and posting). Plus **llama.cpp** for reasoning — Atlas
reads GGUF files, works out what fits in memory, and drives `llama-server`
directly. No Ollama in the path.

Nothing is compiled in. Swapping any engine is a YAML edit.

---

## Where it stands

**407 capabilities built and tested. 41 not started. 19 blocked on hardware.**

Verified on your machine: it builds, tests pass, both monitors are found and
correctly assigned, Chrome/Discord/Notepad located, Claude identified as a
Store app.

Still needed from you:
1. The Claude app id — `setup\find-claude.bat` prints it
2. ffmpeg, whisper.cpp, piper and two models — about 20 minutes, `VOICE_SETUP.md`

Biggest remaining gaps, in order: draining the work queue (commands reach it,
nothing executes from it yet), the send call that clicks Post, screen reading
for "finish this conversation for me", and an iPhone client on the API that's
already built and tested.

---

## The honest part

Roughly a third of this has never run against the thing it controls. The
Windows layer compiles and the logic is tested against a simulated OS, but
"compiles" and "works" are different words. Chrome control, screen reading,
voice identity and presence detection all need a live model or a live app.

Every one of those is marked 🔒 in `CAPABILITIES.md` rather than counted as
done. Nothing in this brief claims otherwise.
