# Atlas — open items

> **STALE — a dated list, partly closed since.** Several items here have been built; the standing offline-first rule at the top still holds. Last true on 8 September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

**8 September 2026.** Everything discussed and not finished, in one place, so
nothing depends on remembering a conversation.

> **The standing rule, set by you, that overrides every recommendation in this
> document: as offline and self-built/self-hosted as possible, for everything.**
> Where a hosted service appears below it is named as a *concept to copy*, never
> as a dependency to adopt. If a section ever reads otherwise, this line wins. Companion to `BUILD_PLAN.md` and
`BUILD_PLAN_v2.md`. Nothing here is done unless it says so.

Status key: **DONE** · **NEXT** · **PLANNED** · **BLOCKED** · **DECIDE**

---

## 0. Where the numbers actually are

| | then | now |
|---|---|---|
| Tests passing | 2,220 | **2,479** |
| Unreachable modules | 96 → 63 | **55** |
| Ratchet ceiling | 63 | **55** |
| Modules Claude has written | — | 4 (`nudge`, `checks`, `council`, `brief`) |
| A second developer's patches merged | — | finance, retention, faithful, consent |
| Windows build | prebuilt only | **reproducible from source in Claude's sandbox** |

---

## 1. Done this project

- **DONE** `nudge` — initiative. Stalled goals, drift, daypart briefs, random
  check-ins. Backs off on silence, asks why exactly once, believes a refusal
  first time. Habits in scope; medicine permanently out via `NEVER_NUDGES_ABOUT`.
- **DONE** `checks` — 32 Windows system checks with real mechanisms, sorted by
  reversibility. Restore point before anything changes. Four things permanently
  refused (credential dumps, Prefetch deletion, registry cleaners, disabling
  Memory Integrity).
- **DONE** `council` — blind rounds, verdict names the split, dissent survives,
  unanimity on the blind round reported as a warning.
- **DONE** `brief` — the morning run. Leads with what to do, caps at 12 lines,
  cannot send.
- **DONE** wiring: `answering` `chain` `draft` `mail` `otherside` `presence`
  `routine` `timebox` are now reachable. 63 → 55.
- **DONE** `ATLAS.bat` archive-flatten fix (whisper and piper both landed one
  folder too deep, which is why voice never installed).

---

## 2. Immediate — before Windows

- **NEXT** Daemon patches: `nudge` and `brief` are reachable but the daemon
  only calls `nudge` on the proactive tick. `brief` needs a scheduled morning
  run and `council` needs a way in from a spoken request.
- **NEXT** Quick wins still unwired and cheap: `recall` `research` `content`
  `prose` `plainly` `person` `wants` `wanted` `returning` `grade` `reach`.
  Each needs a caller, nothing else.
- **NEXT** Get it running on the laptop. Two folders deep. Everything else in
  this document is theory until this happens.

---

## 3. Memory — agreed, not built

- **PLANNED** **The four-write loop.**
  1. Live write during work, rule-gated: *if it will not change future
     behaviour, it does not get saved.* No diary.
  2. The sweep — a cheap model extracts and **updates in place**, and it runs
     **before context compression**, not at conversation end, because
     compression destroys detail permanently. Idempotent.
  3. The nightly sleep pass — stronger model merges duplicates, resolves
     contradictions, newest wins, compacts ten notes into one rule.
  4. Corrections, stored as *why it was wrong and how to do it right* — never
     "the boss didn't like it."
  - The structural point: **if the mistake was in how a task is done, the fix
    goes in the skill file, not the memory diary.** The skill is read every
    time; the diary may never be read again.
  - Scoreboard: does it make the same mistake twice. That is the only metric.
  - Wires: `learned` `consolidate` `recall` `knowhow` `grading` `selfaudit`
    `overnight`.
- **PLANNED** **Vault index pattern.** An index note per folder, a bullet index
  atop every daily note, one master index at the root. Boot reads *only* the
  master index; everything else loads on demand. Wires `recall` `knowhow`
  `index` `addressing`.
- **PLANNED** Memory guard rails: every memory carries a date and a source;
  "you said it" outranks "I inferred it"; nothing stored as fact unless
  confirmed by you or by result; money and identity writes wait for approval;
  everything in plain files you can open and delete.

---

## 4. Self-driving pipeline — agreed, not built

- **PLANNED** Atlas drives its own five stages and consults only when needed.
  Wires `selfwork` `selfgrant` `grants` `consent` `diagnose` `chain` `budget`.
- **Floor, unchanged:** adding something new, or changing its own limits,
  always comes to you, and its limits can never be self-granted.
- **Amended by you:** system optimisations *may* touch the machine. Scoped by
  `checks` — read-only and reversible run freely, one-way and judgement calls
  still gate.

---

## 5. Content and business — the modules are wired; the judgement items below are still planned

**Updated 14 Sep 2026.** `content`, `reach`, `grade`, `plainly`, `publishing`,
`voiceover`, `editors` and `edit` all have callers now — `atlas content` and
`atlas video`. What remains open here is the four PLANNED items below, which
are about what Atlas should conclude from a body of posts, not about whether
the code can be reached.


- **PLANNED** Weekly report: reads engagement, separates **saved and shared**
  from viewed, names what to **stop**, not only what to start.
- **PLANNED** Study outliers not averages — a video that beat its own channel
  mean by 10× is a repeatable concept; a channel's overall best is not.
- **PLANNED** Hyper-specific beats broad. "Editing till 3am for 200 views"
  outperforms "grow on Instagram."
- **PLANNED** Capture-at-the-moment beats edit-later (the OBS replay-buffer
  idea generalises: the clip you want is the thirty seconds that just happened).
- Draws on `content` `publishing` `prose` `plainly` `voiceover` `editors`
  `reach` `grade` `draft` `edit` `research` — all reachable as of 14 Sep except
  `research`, which is a separate item.

---

## 6. Voice

- **DONE (already installed)** piper. Free, local, no account. `tts.rs` already
  changes voice conversationally.
- **PLANNED** Replace the engine. piper is a 2023 VITS model and is the weak
  link.
  - **Chatterbox** — MIT, 0.5B, zero-shot cloning. Resemble's own blind study
    put it ahead of ElevenLabs 65.3% to 24.5%. *Vendor-run study — their own
    product, their own test. Treat with salt.* Free, self-hosted, approved.
  - **Kokoro-82M** — Apache 2.0, 54 voices, top open MOS (4.2), CPU-capable.
    The lower-risk option.
- **BLOCKED on a decision** `tts.rs` hardcodes piper's file format in `Voice`.
  Making the engine pluggable is the contained job that unblocks both.

---

## 7. Generation — what Atlas can and cannot host

- **PLANNED, realistic** Voice generation. Already local. See §6.
- **PLANNED, realistic** Image generation. `stable-diffusion.cpp` / SDXL-Turbo
  or SD 1.5 via ONNX will run on the Arc 140V. Minutes per image, not seconds.
- **DECIDE** Video generation. **Not realistic on this hardware.** Runway,
  Veo and Sora class models need datacentre GPUs. An 8GB integrated card with
  15.7GB shared system RAM will not do it. Anything that claims otherwise is
  calling a cloud.
- **PLANNED, very realistic** **App and page building — the Base44-shaped
  thing.** See §8.

---

## 8. The app builder

Base44 (Wix, $80M, June 2025) turns a plain-English prompt into a full-stack
app: frontend, backend, database schema, auth, hosting. Free tier is 25
credits; paid from $16/month. **It uses Claude Sonnet for the generation.**

Atlas's version is narrower and more useful to you, because the hard part of
Base44 is hosting, and you do not need hosting:

- **PLANNED** `build a page` / `build me a tool` → local model writes a single
  self-contained HTML file, Atlas opens it in Chrome on the side monitor.
- **PLANNED** Iterate by talking. "Make the header smaller", "add a column."
  Wires `edit` `draft` `prose`.
- **PLANNED** Persist to SQLite locally rather than a hosted Postgres.
- **PLANNED** For anything real, the generation quality is model-bound. With
  `models/llm.gguf` (Qwen 2.5 7B or 14B) results will be modest; the honest
  ceiling on your hardware is small tools and pages, not applications.
- Wires `edit` `publish` `publishing` `content` `browser` `workspace_view`.

---

## 9. LLM management — approved, explained

**What it is, plainly:** a flight recorder for every request Atlas makes to a
model. Each call gets logged with the prompt, the response, how long it took,
how many tokens, and — where you can judge it — whether the answer was any
good.

**Why you need it,** in the order the pain arrives:

1. **You cannot debug what you cannot see.** When Atlas gives a wrong answer
   next month, the question is *what did it actually ask the model, and with
   what context loaded?* Without a trace that is unanswerable and you are back
   to guessing.
2. **It closes the four-write loop.** §3's scoreboard is "does it make the
   same mistake twice." You cannot count that without a record of the first
   time.
3. **It is how a local model earns trust.** You will want to know whether the
   7B is actually good enough, or whether a particular kind of question needs
   the 14B. That is a measurement, not an opinion.
4. **It is the only honest way to compare prompt changes.** Otherwise every
   prompt tweak is a vibe.

**Helicone**, since you asked: a proxy that sits between an app and a model
provider and logs everything passing through, with a one-line setup. **It went
into maintenance mode after Mintlify acquired it in March 2026** — self-hosted
issues are unfixed. The live self-hostable equivalents are Arize Phoenix and
Langfuse (both open source, both free).

**But you should not use any of them.** They are proxies for cloud API traffic
across a team. Atlas is one process, on one machine, calling a model on the
same disk. The whole value is a local append-only trace file plus a reader —
a few hundred lines, no Docker, no service, works offline by construction.

- **PLANNED** `src/trace.rs`: append-only JSONL of every model call — prompt,
  response, tokens, duration, which model, which module asked.
- **PLANNED** Bind it to `grading` so a graded outcome points back at the exact
  call that produced it.
- **PLANNED** `atlas trace` — what did it ask, when, and what did it cost.
- Wires `metrics` `grading` `perf` `brain`.

---

## 10. Prompting — from the carousels

- **PLANNED** The eight-part skeleton: task, tone, background data, detailed
  rules, examples, conversation history, immediate request, think step by step.
  Atlas should construct its own model prompts this way rather than ad hoc.
- **PLANNED** The don't-say/say-better list, as rules in `prose`:
  - "improve it" → "improve it against these three criteria"
  - "make it shorter" → "summarise in three sentences without losing X"
  - "explain to me" → "act as [role]" / "give me non-obvious angles"
  - "give me a summary" → "break this open" (a summary of a contract makes a
    confusing document shorter, not clearer)
- **PLANNED** "Interview me relentlessly" — end a build prompt by making the
  model interrogate you for what you left out. Wires `walkthrough` `wanted`
  `wants`.

---

## 11. Our own Hermes

Hermes Agent is free and open source (Nous Research). Atlas already is the
harness. What Hermes has that Atlas does not:

- **PLANNED** `messaging` — Telegram first. One gateway, so you can send Atlas
  a voice note like you'd text a friend, from anywhere.
- **DECIDE** The phone. Shortcuts vs PWA vs native app. **Open since the first
  build plan and blocking 11 modules.** This is the oldest undecided thing in
  the project.
- **PLANNED** Skill format. Read `agentskills.io` before finalising Atlas's,
  so skills are portable rather than bespoke.
- **NOT RECOMMENDED** switching to Hermes. Atlas has the Windows layer, the
  guards, the wiring ratchet, the DPAPI vault, and no Python runtime.

---

## 12. Trading knowledge

General trading knowledge only. Anything specific to one trading system was
taken out of personal Atlas on 26 Sep 2026 and lives with that system.

Built and shipped, unrun:

- `market` — swings, BOS/CHoCH/sweeps, ATR, efficiency, fair value gaps,
  liquidity pools, session hours.
- `levels` — where a stop and a target fall for a given direction, and a
  refusal with a reason when neither pays.
- `live` — the same reading one bar at a time, with a settled/forming line.
- `untrusted` — nothing it reads may instruct it.
- `claims` — the referee. Claims that can be ruled on against the bars.
- `together` — three trades that are one bet.
- `stale` and `standdown` — a trade that has gone nowhere, and when to have no
  view at all.

Still blocked on the reference material: contract specs, options mechanics,
trader tax, forex conventions. That is knowledge *about* instruments, and it is
what `levels` needs to size a futures or options position correctly rather than
generically.

**Order blocks** are deliberately not built. Three common definitions disagree
and I am not guessing which one you trade.

- **BLOCKED** Atlas has no market knowledge *about instruments*. Prerequisite
  is fetching the ~33MB
  reference material (contract specs, options mechanics, trader tax, forex
  conventions, crypto mechanics). The shelf is built; the content is not.
- **Hard scope:** read-only. Atlas places no trades and has no write path to
  any trading system.

---

## 13. Still blocked, and why

| Blocked | Why | Who unblocks it |
|---|---|---|
| 12 hardware modules (`overlay` `look` `uia` `watching` `presence` `voiceid` `hearing` `audio` `dictate` `endpoint` `quickinput` `ocr`) | never run on Windows | you, by running it |
| 6 model modules (`gguf` `models` `backends` `adapt` `fit` `improve`) | `models/llm.gguf` is 4.4GB and `ATLAS.bat` never downloads it | you, by fetching it |
| 11 device modules (`ios` `android` `cloudsync` `codes` `companion` `confirmed` `household` `recovery` `remote` `workingset` `afterme`) | no second device | the phone decision, §11 |
| Email against a real inbox | needs an app password, or AgentMail's free tier (3 inboxes, 3,000/month) | you |
| OCR | exe expects `tools/tesseract/tesseract.exe`; nothing downloads it | a line in `ATLAS.bat` |
| `config/apps.yaml` | described as all guesses pending real capture | one `doctor` run |
| Video generation | hardware | nothing, on this laptop |

---

## 14. Unread, and honestly so

- 1 of 78 TikTok links was a photo post — unreadable by the tool.
- 4 links were music with no speech, including the most recent one.
- **All 127 videos were read by audio only.** Screen recordings where the
  detail is on-screen text the narrator never says aloud are partly unread.
  OCR on frames is possible on request.
- Of the tools named across the corpus, only ten were actually researched.
  The rest are creators' claims, repeated.
- **Nothing built this project has run on Windows.** 2,424 tests pass and it
  cross-compiles clean. Compiling and working are different things.

---

## 15. Updates — how Atlas changes after it is running

**The question that has been costing you files.** You have been deleting
everything after a failed run because you did not know which copy was current.
That stops here.

### How it works today

Atlas is a single `atlas.exe` plus a folder of data. There is no updater. An
update is: **replace one file.**

- `atlas.exe` — the program. Replaced wholesale, every time.
- `config/` — your settings. **Never overwritten by an update.**
- `models/`, `tools/` — the 260MB of speech files. Downloaded once. **Never
  re-downloaded by an update.** These are the expensive ones and they are safe.
- `data/` — memory, decisions, history. **Never touched by an update.**

So the rule you have been missing: **never delete `models/`, `tools/`,
`config/` or `data/`.** Those are yours. Only `atlas.exe` is ever replaced,
and every failed run so far has been an `atlas.exe` problem or a folder-layout
problem, never a data problem.

### Does it have to shut down?

Yes, and there is no way around it on Windows. A running `.exe` is locked by
the OS; you cannot overwrite it in place. Close the Atlas window, swap the
file, start it again. Seconds, not minutes.

### The update procedure

1. Close Atlas.
2. Drop the new `atlas.exe` into `C:\Atlas\atlas`, overwriting.
3. Right-click → Properties → **Unblock** (every download is stamped afresh).
4. Start `ATLAS.bat`, pick **2** to check what survived.

`config/`, `data/`, `models/`, `tools/` all carry over untouched.

### What is missing, and should be built

- **PLANNED** `atlas version` — prints the build date and test count, so you
  can tell in one second whether the exe in a folder is the current one. **The
  single highest-value thing on this list right now**, because it removes the
  entire class of "am I running the old one" doubt.
- **PLANNED** `atlas.exe --self-check` at every start: verify `models/`,
  `tools/`, `config/` and `data/` are present and sane, and say plainly what is
  missing rather than failing later.
- **PLANNED** An `updates/` folder Atlas watches. Drop a new exe in, Atlas
  notices, tells you it is there, and swaps on next start.
- **DECIDE** Whether Atlas ever fetches its own updates. It cannot self-update
  without a network fetch and a write to its own binary, which is exactly the
  category that always comes to you. Recommended: it *notices* and *asks*, and
  never fetches on its own.

---

## 16. The second Atlas — server-side

**Noted, not designed. This needs its own conversation before any code.**

What you have said so far:

- In a few months there will be a **modified Atlas on a self-hosted server**,
  focused on running the server itself.
- The two must **not be linked** — server-Atlas is not to be an extension of
  your Atlas.
- But they must be able to **communicate**.
- And your Atlas must be able to **work on the server**.
- This is where Phoenix/Langfuse-shaped thinking earns its place: a shared
  trace and evaluation surface two separate systems can both write to without
  either one reaching into the other.

**Claude owes you objections and ideas on this. Not yet — asked to hold.**
The tension worth flagging early, so it is not a surprise later: *not linked*
and *my Atlas can work on the server* are in some tension, and the resolution
is probably that your Atlas gets an account on the server rather than a
back door into server-Atlas's memory. That is the conversation.

---

## 17. Video generation — hardware-gated, in-house

Revised from §7. **In-house first, always.**

- **PLANNED** Build the in-house generator against a local model
  (`stable-diffusion.cpp` / a video-capable local pipeline), not a cloud call.
- **PLANNED** **Hardware gate.** Atlas measures the machine. Below the bar the
  feature is **off and cannot be turned on** — not a warning, not a slow path,
  not available. Above the bar the user may enable it.
  - This is the same shape as `checks`: capability decides, not preference.
  - Your current laptop is below the bar. A future upgrade, or a friend's
    machine, may not be.
- **PLANNED** The bar itself needs measuring rather than guessing — VRAM,
  system RAM, and whether the GPU has usable compute. `perf` and `checks`
  already read most of this.

---

## 18. The app builder — in-house, offline

Promoted from §8 to a committed task.

- **PLANNED** `build me a page` / `build me a tool` → local model writes a
  single self-contained HTML file → Atlas opens it on the side monitor.
- **PLANNED** Iterate by talking: "smaller header", "add a column".
- **PLANNED** Local SQLite for anything that needs to persist. No hosted
  database, ever.
- **PLANNED** No account, no hosting, no deploy step. The file is the artifact
  and it lives on your disk.
- Quality is model-bound: on current hardware the honest ceiling is small tools
  and pages, not applications.

---

## 19. Council — remaining work

`council` is built and tested. What is not done:

- **PLANNED** Its own intent. It currently has no spoken way in; `Rehearse`
  already means "dry-run against the mock platform" and was left alone rather
  than hijacked.
- **PLANNED** The `Open` round. Blind is implemented and enforced; the
  follow-up round where seats read each other and may revise is defined in the
  type and not yet driven.
- **PLANNED** Wiring seats to real model calls. Today `blind_prompts` returns
  the prompts; nothing sends them.
- **PLANNED** Custom rooms. `default_room` is five seats; you should be able to
  define your own and keep them.

---

## 20. Voice — decided

- **DECIDED** Chatterbox. Free, MIT, self-hosted, no account.
- **PLANNED** Make the engine pluggable in `tts.rs` (it hardcodes piper's file
  format in `Voice`).
- **PLANNED** Remove piper once Chatterbox is proven — it is dead weight on
  disk after that, but not before.
