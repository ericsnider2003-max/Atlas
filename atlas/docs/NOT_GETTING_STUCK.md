# How Atlas avoids getting stuck, and how it adapts to any machine

You asked for three things that turn out to be separable. Here's how each one
works.

---

## 1. It fits the machine it lands on

Nothing is tuned to your laptop. Atlas measures and picks a plan.

**It plans for the machine, not for the state it's in.** This was wrong in the
first version and you were right to push on it. Measuring free memory on a day
with Teams and Spotify idling decides your laptop is small when it isn't — so
Atlas counts what it could *free* as well as what's free, and asks before
closing anything.

On your laptop that is the difference between a 1.5B model and a 3B one. Same
machine, same day; the smaller answer was an artefact of what happened to be
open.

It still never claims more than a third of the machine however idle it looks,
because you're going to open things.

**Integrated graphics don't count as extra memory.** Your Arc 140V shows 8GB,
but it's shared with the system rather than its own. Anything under 4GB of
dedicated VRAM is treated as zero, which is the honest answer.

Some worked examples:

| machine | what it gets |
|---|---|
| Your laptop — 16GB, integrated | speech, meaning-search, a small model, no vision |
| A friend's desktop — 64GB, real GPU | everything, a 14B model, vision, 4 things at once |
| An old laptop — 8GB, spinning disk | speech and search, model kept warm because reloading from that disk is worse |
| A netbook — 4GB | speech only, one thing at a time, rules instead of reasoning |
| Nothing spare at all | text only, and it says so |

**Every choice degrades rather than fails.** No memory for a model doesn't mean
Atlas stops; it means the rule-based paths do the work. And it says what it
can't do here *up front* rather than letting you discover it: "I can't look at
your screen and understand it — I can read text off it."

**It replans when the machine changes.** A plan made on a day you had forty
tabs open isn't the plan you want forever.

---

## 2. Offline is narrower, not useless

The reason most assistants are helpless offline is that all their competence
lives in a model. Atlas keeps competence in two other places as well.

**Procedures it ships with.** Not facts about the world — those go stale and
are what the internet is for — but **how to get things done**: the steps, what
has to be true first, what usually goes wrong, and what to do when it does.
Freeing memory, fixing a microphone that stopped working, finding a file you
can't place, an app that won't start, a filling disk, making sense of a
document. **Six of the seven work with no connection at all.**

The valuable part is the snags, because that's what separates a procedure from
a list. Atlas knows that an app which "launches and closes immediately" is
usually an updater stub — Discord and Chrome both do it — so the answer is to
wait two seconds and look for the real window, not to try again. It knows that
a microphone which "worked yesterday and not today" usually means a headset
connected and the device names shifted, so the answer is to re-measure rather
than trust the saved name.

**Rules that don't need a model.** The punctuation reader that tells "a period
drama" from "that's the end period". The route chooser. The draft critic. The
statement categoriser. The tax knowledge. None of these improve with a model
attached, and all of them work at 4GB.

**And it learns snags.** When something fails in a new way, that failure is
added to the procedure, so the same surprise happens once.

---

## 3. It doesn't get stuck

There are **eighteen known ways in** across four kinds of problem — getting
data out, making something happen, finding something out, fixing something.
When one is closed, Atlas picks another, weighing what it costs against how
often it works.

Two properties matter here:

**Every kind of problem has at least one offline route**, and there's a test
asserting it. Being offline narrows the options; it never closes them.

**Being online is strictly better, never differently better.** Nothing is
possible only when disconnected, so a connection can only add.

And the last route in every list is *ask you*. Which means "stuck" only ever
happens when even that has been ruled out — at which point Atlas says what it
tried, what the closest thing was, and asks you to show it, so next time it
knows.

Underneath that sits the ladder: **twelve genuinely different approaches** to a
problem that resists, stopping early if three in a row hit the same error,
because at that point the angles aren't reaching it.

---

## Why this doesn't cost speed

Quality and speed only fight when everything gets the same treatment.

Routes are scored on cost *and* reliability, so the thorough options exist but
come last. Reading a file that's already there takes a second; screenshot-and-OCR
takes forty. Most problems are solved by the first thing tried and never reach
the expensive end at all. One `impatience` setting slides the balance if you'd
rather have thorough over quick.

The same principle runs through the rest: the model is kept loaded rather than
reloaded, cheap signals are preferred over expensive ones — the accessibility
tree costs nothing where a screenshot costs a second and a vision model costs
ten — and work that can wait is batched.

---

## 4. How Atlas gets better on the hardware you already have

Not a bigger model. The honest answers are about spending what you have more
cleverly, and about accumulating things that cost no memory at all. Most of it
runs while you sleep.

### Free, automatic, and compounding

**Route reliability becomes a measurement.** The eighteen routes ship with
estimated hit rates that I guessed. Every real attempt is scored — this route,
on this site, worked or didn't. After a dozen attempts Atlas knows better than
I did, and the estimate is replaced by evidence, weighted by how much evidence
there is. One success doesn't mean 100%; twenty-five does.

This is the largest single gain available and it costs nothing. It's how Atlas
stops trying things that don't work *in your world* specifically.

**New failures join the procedures.** Something breaks in a way Atlas hasn't
seen, and the symptom, cause and fix are written into the procedure. The same
surprise happens once.

**Your vocabulary improves transcription.** "QUIC", product names, the names of
people you talk about — collected as you use them and handed to the speech
model as hints. A word said once is a typo; said twice it's a word. The list
stays short deliberately: a long one makes transcription worse, because
everything starts sounding like something on it. This weighs a kilobyte and is
the cheapest large gain there is.

**Work moves off the path you're waiting on.** Indexing, embedding, statement
reading and self-work all happen while you aren't waiting for them. Nothing
about capability changes; everything about how it feels does.

### Costs a little, worth a lot

**The model stays warm** rather than being read from disk each turn — the
single biggest latency win available, and honest about costing two gigabytes.

**Meaning vectors are computed overnight** rather than at search time.

**The right-sized model does the job.** Classifying, routing and summarising go
to the small one; only writing and reasoning reach the large one. Most requests
never touch the expensive path.

### Needs a decision from you

**Trimming what you don't use.** Voices you rejected, languages you don't
speak, indexes of folders you deleted. Atlas lists them with what dropping each
one costs you — usually nothing, since a voice re-downloads in a minute — and
the space goes to something you use.

**Distilling.** When a hosted model solves something, what it decided is kept
as an example, and similar problems later are answered locally from the
pattern. This reduces what you'd ever pay for, and it's off by default because
it means keeping more.

### Ask it

"How are you getting on?" gets a real answer: *"140 attempts scored, 3 new
failures understood, 22 of your words learned. None of it cost you anything."*
