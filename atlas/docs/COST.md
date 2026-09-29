# What a hosted model would actually cost

> **STALE — the numbers describe a much smaller tree.** It says 891 tests. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Real numbers, using the rates in `config/tools.yaml`. Check them against
<https://platform.claude.com/docs/en/about-claude/pricing> — they change, which
is why they live in config rather than in the code.

Current rates, dollars per million tokens: **Haiku 4.5 $1/$5 · Sonnet 5 $2/$10
· Opus 5 $5/$25.** Batch processing halves both. A cache read costs a tenth of
a fresh input token.

---

## The numbers

A realistic overnight task: the Atlas source as context (~120k tokens), a short
instruction, a patch back (~3k tokens out).

| | cost |
|---|---|
| One change, overnight, codebase cached | **$0.03** |
| One change, right now, no batch discount | $0.06 |
| First request of the night (fills the cache) | $0.17 |
| **Twenty changes in one night** | **$0.71** |
| Twenty a night, every night for a month | **$21** |
| The same month on Haiku | $11 |
| The same month with no caching and no batching | **$164** |

That last row is the point. **The difference between doing this carefully and
doing it naively is about eight times the money** — $21 against $164 for
identical work.

---

## Where the saving comes from

**1. Local first.** Most tasks never reach a hosted model. Changing a
threshold, adding a phrase, adjusting config — the 3B on your machine does
those for nothing. Only real code goes out.

**2. The cheapest model that can do the job.** Reading a diff and summarising a
failure is Haiku work. Designing a change across files is Sonnet work. Atlas
picks per task rather than sending everything to the expensive one, and your
ceiling setting overrides its judgement.

**3. The codebase is cached.** The source barely changes between requests, so
after the first one it costs a tenth. This is the single biggest lever, and
it's why the first request of the night costs six times the rest.

**4. Overnight work is batched.** Half price for anything that can wait, which
is exactly what "while I sleep" means. Results come back within 24 hours,
usually within one or two.

They stack. A cached batch request costs roughly a twentieth of a naive one.

---

## The caps are a stop, not a warning

`daily_cap` and `monthly_cap` refuse work rather than notifying you afterwards.
The shipped defaults are **$1 a day and $15 a month**, which at these rates is
around 500 overnight changes. Raise them if that's ever the limit you hit.

Atlas tells you what a night would cost before it starts — *"20 tasks on
Sonnet, codebase cached, overnight rate. About $0.71."* — and you can ask what
it has spent at any time.

---

## How the three options fit together

You asked for the first two integrated so the third costs as little as
possible. That's the design:

**Option 1 — I write the code.** Free. Atlas builds it, runs its 891 tests,
shows you the diff. This stays the main path.

**Option 2 — the local model.** Free. Handles anything trivial without ever
reaching for the network, and it's the filter that keeps option 3 small.

**Option 3 — hosted, overnight.** Everything above only applies to what gets
past options 1 and 2. Batched, cached, capped, and logged.

The important part is that they're not alternatives you choose between. Option
2 decides what option 3 ever sees, which is why the realistic monthly figure is
$21 and not $164.

---

## What I'd actually suggest

Start with the cap at **$5 a month** and `ceiling: haiku`. That's enough for
Atlas to do real work on itself overnight while you find out whether it's
useful. If it is, raise the ceiling to Sonnet and the cap to $15.

There are also **$5 of free credits** on a new API account, which at these
rates is around 150 overnight changes — enough to answer the question before
spending anything.
