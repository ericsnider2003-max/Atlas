# Roadmap

> **STALE — a dated list, partly closed since.** Several items here have been built. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Everything, ordered. Highest value first, and within that, cheapest first.

**Rule of thumb used throughout:** value per unit of effort, with a bias
toward things that unlock other things. A capability that makes five later
ones cheaper beats a flashier one that stands alone.

Legend: **S** a day · **M** a few days · **L** a project · 🔒 needs the laptop

---

## Phase 0 — finish what's started

Half-built things cost more than unbuilt ones, because they look done.

| | Item | Size | Why first |
|---|---|---|---|
| 0.1 | Whisper install fixed | S | The last FAIL in doctor. Nothing voice works without it. |
| 0.2 | ~~Drain the work queue~~ | ✅ | Background work runs immediately; screen work waits for a gap; nothing sneaks past the gate. |
| 0.3 | The send call for posts | S | Approval, plan and gating all built; nothing clicks Post. 🔒 needs a live browser. |
| 0.4 | ~~Wire the hub to the server~~ | ✅ | Pages served as HTML, settings saved by form, redirect after save. |
| 0.5 | ~~Clipboard and rehearsal by voice~~ | ✅ | "Explain this" and "rehearse boot workspace" both work. Quick input and gestures still need the OS hooks. 🔒 |
| 0.6 | Verify the Windows layer live | 🔒 | Window management compiles but has never run. |

## Phase 1 — the things you'd feel tomorrow

Small, and each one changes a daily moment.

| | Item | Size |
|---|---|---|
| 1.1 | ~~Dictation~~ | ✅ |
| 1.2 | ~~Explain its decisions~~ | ✅ |
| 1.3 | ~~Watch a long job~~ | ✅ |
| 1.4 | ~~Refuse to answer when it doesn't know~~ | ✅ |
| 1.5 | ~~A panic word~~ | ✅ |
| 1.6 | **Read the document you're looking at** — the subject resolver finds it; nothing reads it yet | S |
| 1.7 | ~~Endpointing~~ | ✅ |
| 1.8 | ~~Latency measurement~~ | ✅ |

**Phase 1 is done bar 1.6.** Along the way: voice selection by voice,
accents and translation, concurrent work, the panels, and settings that stay
reachable when Atlas isn't.

## Phase 2 — the big unlocks

Each of these makes several later items possible.

| | Item | Size | Unlocks |
|---|---|---|---|
| 2.1 | **Calendar** — read and write | M | Meeting prep, call joining, daily brief, anticipation |
| 2.2 | ~~Private search over everything you've written~~ | ✅ | Words with no model; meaning when one is there |
| 2.3 | ~~Atlas working on Atlas~~ | ✅ | Sandboxed, test-gated, cannot edit its own permissions |
| 2.4 | ~~The first-run conversation~~ | ✅ | Spoken, skippable, nothing blocks |
| 2.5 | ~~Statement reading~~ | ✅ | Local files, no credentials, with the tax rules that catch traders out |

## Phase 3 — the work you actually wanted it for

| | Item | Size |
|---|---|---|
| 3.1 | **Email triage** — sorting, not replying. "Three things need you today" | M |
| 3.2 | **Site routines** — "check my orders", recorded once, replayed | M |
| 3.3 | **Meeting prep** — last thread, open items, files touched | M |
| 3.4 | **Draft, critique, revise** — one extra model call, much better drafts | S |
| 3.5 | **Logging into sites** — reference model, browser fills, Atlas never sees it | M |
| 3.6 | **Account auditing between statements** — read-only guard already built | M |
| 3.7 | **Cross-app chains** — "research X, write it up, open it" | M |
| 3.8 | **Escalate rather than guess** — name the two options it's between | S |
| 3.9 | **Time-boxing** — "spend ten minutes and tell me where you got to" | S |
| 3.10 | **Remember what didn't work**, so the same dead end isn't walked twice | S |

## Phase 4 — media

| | Item | Size | Note |
|---|---|---|---|
| 4.1 | **Video editing from a description** | — | Built. Needs a live render. 🔒 |
| 4.2 | **Transcript-driven cuts** — "remove where I repeat myself" | M | You already have whisper |
| 4.3 | **Image generation** | M | Tens of seconds on Arc. Background work |
| 4.4 | **Putting you in a picture** — IP-Adapter FaceID | M | Needs 4.3 |
| 4.5 | **Reading handwriting** — OCR is built; this is the workflow around it | S | |
| 4.6 | **Media control** — pause everything for a call | S | Windows exposes global media keys |
| 4.7 | ~~Video generation~~ | — | Not possible on this laptop |

## Phase 5 — presence and awareness

| | Item | Size |
|---|---|---|
| 5.1 | **Speaking while thinking** — no silent gap before a long answer | M |
| 5.2 | **Noticing patterns** — "you open that every Tuesday" | M |
| 5.3 | **Learning where files go**, so things land where you'd have put them | M |
| 5.4 | **Adaptive verbosity** in practice — signals exist, nothing uses them | S |
| 5.5 | **A public-facing mode** — someone at your desk, nothing private on screen | S |
| 5.6 | **Health-aware pacing** — four hours at the desk, mentioned once | S |
| 5.7 | **A "what changed" weekly report** — learning you can't see is learning you can't correct | S |

## Phase 6 — reach

| | Item | Size |
|---|---|---|
| 6.1 | **iPhone / iPad client** — the API is built and tested | M |
| 6.2 | **Voice notes to structured output** — needs 6.1 to be worth much | M |
| 6.3 | **Joining calls** — open the link, mute on entry, place the window | S after 2.1 |
| 6.4 | **Call notes** — consent model built; the capture isn't | M |
| 6.5 | **Smart lights and plugs** — Zigbee, no cloud | M |
| 6.6 | **Watching a page for change** — price, status, availability | S |
| 6.7 | **Waking or sleeping the machine on a schedule** | S |

## Phase 7 — hardening

Unglamorous, and the difference between a demo and something you rely on.

| | Item | Size |
|---|---|---|
| 7.1 | **Encryption at rest** — DPAPI, no password to manage | M |
| 7.2 | **Rate limit on consequential actions** — a confused loop can't post fifty times | S |
| 7.3 | **Timezone and DST** for scheduling — posts drift an hour twice a year | S |
| 7.4 | **Clock jumps** — waking from sleep shouldn't fire a day of backlog at once | S |
| 7.5 | **Disk-full handling** — writes currently fail into `let _ =` | S |
| 7.6 | **Self-repair** — restart a helper that died | S |
| 7.7 | **Its own changelog** — what changed about Atlas, and when | S |
| 7.8 | **Failure post-mortems** — evidence accumulates instead of re-diagnosing | S |
| 7.9 | **Config validation** with plain-language errors | S |

## Phase 8 — larger, later

| | Item | Size |
|---|---|---|
| 8.1 | **A second machine** sharing one thread and memory | L |
| 8.2 | **Hidden desktop** — drive an app without taking focus | L |
| 8.3 | **UI Automation live** — read windows without screenshots | 🔒 M |
| 8.4 | **Backend router in practice** — choose CDP / UIA / SendInput at runtime | M |
| 8.5 | **Teaching by demonstration** — do it once, say "remember that" | L |
| 8.6 | **Native in-process inference** via candle | L |
| 8.7 | **Streaming service control** | M, and brittle forever |
| 8.8 | **Subscription watch** — needs statements plus app usage | M |

---

## Deliberately never

- Anything that moves money.
- Continuous screen reading.
- Replacing ggml.
- Autonomy for consequential actions beyond announce-and-do.
- Holograms; distinguishing you from a recording of you.

---

## The order in one line

Finish what's half-built → the eight small things you'd feel tomorrow →
calendar and search → the actual work → media → awareness → reach → hardening.

**Done so far:** 0.2 the queue drains, 0.4 the hub is served, 0.5 clipboard
and rehearsal are reachable by voice. Register-aware character landed alongside.

**Phase 2 is done bar the calendar.** Next: Phase 3. Calendar (2.1) waits for a
source I can test against. Phase 1 — dictation, explaining its own decisions, watching a long
job, refusing to answer when it doesn't know, and a panic word. Then 0.1 and
0.6 when the laptop is free.

---

# Making it work better on this hardware

A pass over every phase asking what your machine actually changes. The
constraint that matters throughout: **an Arc 140V with 8GB shared out of 15.7GB
total, and Windows already using most of it.** Roughly 3.5GB is genuinely
available. That single number reshapes several items.

## What changes

**Phase 1 — endpointing (1.7) moves up.** Fixed 8-second recording isn't just
slow, it's *expensive here*: every turn transcribes 8 seconds whether you spoke
for one or seven. Endpointing cuts the average clip to two or three seconds,
which cuts transcription cost by more than half. It's a latency fix and a
resource fix at once.

**Phase 2 — private search (2.2) is cheaper than it sounds.** Embedding models
are 50–100MB, not gigabytes. They run comfortably alongside everything else,
and they run *well* on the NPU. This is the one place your Lunar Lake chip is
genuinely an advantage rather than a constraint.

**Phase 2 — Atlas working on itself (2.3) needs a bigger model than fits.** A
3B model writing Rust is not a good use of anyone's afternoon. Two honest
options: use a hosted model for this one task, or accept that it proposes small
changes only. I'd do the first and keep everything else local.

**Phase 4 — image generation (4.3) should target SD-Turbo, not SD 1.5.** One to
four steps instead of twenty. On Arc that's the difference between roughly ten
seconds and roughly a minute. Same VRAM, far better fit.

**Phase 5 — speaking while thinking (5.1) matters more here than on a fast
machine.** A 3B model on shared memory takes a noticeable moment. Speaking the
first sentence while composing the rest hides most of that, and it costs no
memory at all.

## Cheap wins that apply everywhere

**Keep one model loaded.** Loading a 2GB model per turn is seconds of disk read
every time and the single biggest avoidable cost in the whole system. Load once,
keep it warm, evict only under real pressure. The lifecycle supervisor already
tracks idle helpers — it just needs a policy that favours keeping the model.

**Cap the context hard.** Time-to-first-word grows with the conversation, and
on shared memory it grows faster. The thread already folds; the fold threshold
should come down.

**Prefer the accessibility tree over screenshots.** Reading a window's text
costs nothing. A screenshot plus OCR costs a second. A vision model costs ten.
Same answer, three orders of magnitude apart.

**Batch the indexer.** Metadata-first already avoids reading file contents;
scanning in bursts while idle avoids competing with you for disk.

**Quantise the speech model to Q5 and use `tiny.en` when the room is quiet.**
`base.en` earns its size in noise, not in silence.

## Three questions answered

**Recordings.** No, and they aren't kept. The audio is a means to a transcript
and is deleted the moment the transcript exists — seconds, not hours. A folder
of recordings of yourself is a liability with no upside, and keeping them "just
in case" is how they end up in a backup. Keeping them is a setting you'd have
to turn on deliberately.

**Storage.** Not a problem, because the biggest things are also the most
movable. Models are ~900MB, never change once downloaded, and are re-downloadable
— which makes your D: drive with 220GB free exactly where they belong. Captures
and video scratch go with them. What stays on C: is small and matters: notes,
what Atlas has learned, and the backups. Atlas now works out that plan itself
and explains what moves and what doesn't.

**Paying for a bigger model.** For Atlas to write real Rust, honestly, yes —
a 3B model editing a 15,000-line codebase is not a good use of an afternoon.
But there are three ways to have this and only one costs money:

1. **Keep doing what we're doing.** I write the code, Atlas builds it, runs its
   own 874 tests, and shows you the diff before anything is applied. That's the
   sandbox's actual purpose and it costs nothing extra.
2. **Local model for small changes only.** Config edits, a new phrase, a
   threshold. A 3B handles those.
3. **A hosted model for this one task.** Pennies per change, and the only part
   of Atlas that would ever leave your machine.

I'd do 1 and 2, and reach for 3 only if you wanted Atlas working on itself
while you're asleep.

## What this hardware simply won't do

- Video generation. ~16GB VRAM floor.
- A 7B+ model at usable speed alongside Windows.
- Continuous vision. Even a small vision model per frame is unaffordable.

None of that changes the plan; it's why the plan looks like it does.
