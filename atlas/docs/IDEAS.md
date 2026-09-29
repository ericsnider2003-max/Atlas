# Implementation ideas

> **STALE — a dated list, partly built since.** Several of the twenty-eight ideas here have been built. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Twenty-eight ideas, each with what it does, why it's worth it, what it would
take, and whether your laptop can run it. Ordered by value per unit of effort,
not by how impressive they sound.

**Effort** is rough: *small* is a day, *medium* is a few days, *large* is a
project in its own right.

---

# Tier 1 — high value, low effort, all parts already exist

These are the ones I'd build first. In every case the hard part is already
written and only the connecting piece is missing.

### 1. Clipboard as an input channel
Copy anything — an error message, a paragraph, a table — and say "explain
this", "reply to this", "clean this up". The reply goes back to the clipboard
so you paste it where you were.

**Why it's worth more than it looks:** it sidesteps the entire screen-reading
problem. You've already selected exactly the thing you mean, so there's no
ambiguity about what Atlas should look at, no vision model, no OCR. It's the
cheapest possible "understand what I'm working on".
**Effort:** small. One Win32 call each way.
**On your machine:** yes.

### 2. Dictation
Speak, and the words appear in the focused window. Not a command — actual
typing, with punctuation spoken or inferred.

**Why:** you already have whisper running for commands. This is the same
audio going to a different destination. For anyone who thinks faster than they
type, it's the single highest-frequency use of a voice assistant.
**Effort:** small. Every piece exists; it needs a mode flag and the input
guard so it never types into Discord.
**On your machine:** yes.

### 3. Watch a long job and report when it's done
Start a render, a build, a large download. Walk away. Atlas tells you the
outcome when it finishes.

**Why:** it's the Jarvis beat — *"the simulation completed while you were
out"* — and it costs almost nothing. The journal, the away-brief and the lanes
are all built. Nothing currently watches an external process.
**Effort:** small.
**On your machine:** yes.

### 4. Rehearsal mode
"Show me what you'd do." Atlas runs a whole workflow against the fake OS —
the same one its 637 tests use — and tells you every window it would move and
every file it would touch, without doing any of it.

**Why:** this is the trust-builder. Before you let it near a real workflow you
watch the whole thing play out harmlessly. The simulated OS already exists and
is exercised on every test run; nothing exposes it to you.
**Effort:** small.
**On your machine:** yes.

### 5. Explaining its own decisions
"Why did you put Chrome there?" "Why didn't you post that?" "Why did you use
the laptop mic?"

**Why:** every one of those choices is already recorded — the role resolution,
the approval check, the audio selection all produce a reason string. Nothing
surfaces them. An assistant that can account for itself is one you extend
trust to; one that can't is one you second-guess forever.
**Effort:** small. The data exists; it needs a question to reach it.
**On your machine:** yes.

### 6. Reading the document you're looking at
"Summarise the PDF I just opened." Atlas joins the window title to the file
index, finds the actual file, and reads it.

**Why:** no screenshots, no vision model, no guessing. Both halves are built
and unconnected.
**Effort:** small.
**On your machine:** yes.

---

# Tier 2 — high value, moderate effort

### 7. Atlas working on Atlas
Read its own source, make a change, run its own 637 tests, show you the diff,
apply only if you agree.

**Why:** the sandbox is built and so is the test suite, and that suite is
exactly what makes this reasonable rather than reckless — a change that breaks
something gets caught before you ever see it. It's also the highest-leverage
thing on this list: every subsequent idea gets cheaper.
**Effort:** medium.
**On your machine:** yes, though a 3B local model will be weak at real code.
Best paired with a hosted model for this specific task.

### 8. Learning where things go
Notice that invoices end up in one folder, screenshots get renamed a certain
way, notes follow a pattern. Then put things there without being told.

**Why:** filing is pure friction and entirely mechanical. The index already
watches those folders and the memory system already stores patterns.
**Effort:** medium.
**On your machine:** yes.

### 9. Noticing patterns you haven't
"You've opened that spreadsheet every Tuesday for six weeks — want it in your
Tuesday layout?" "Every time you research something you then draft a post."

**Why:** Atlas already records every command with a timestamp and does nothing
with the sequence. This is genuine anticipation rather than scheduled
anticipation, and it's the difference between a tool that follows rules and one
that seems to know you.
**Effort:** medium. The risk is confident nonsense, so it must offer rather
than act, and stop suggesting things you decline.
**On your machine:** yes.

### 10. Voice notes to structured output
Ramble for two minutes walking to the car. Get a task list, a draft, or a note
with the decisions pulled out.

**Why:** speaking is faster than typing and much worse organised, which is
exactly the gap a model closes. Needs the phone client to be most useful.
**Effort:** medium.
**On your machine:** yes.

### 11. Comparing two documents properly
"What changed between these two contracts?" A real structural diff, not a
summary of each one.

**Why:** summarising two things and hoping you spot the difference is what
most tools do, and it's near-useless for anything that matters.
**Effort:** medium.
**On your machine:** yes.

### 12. Meeting preparation
Ten minutes before a call: the last thread with those people, open items, files
touched since, and what you said you'd do.

**Why:** the trigger layer is built and waiting for a calendar. This is the
highest-value use of anticipation for most people.
**Effort:** medium, plus a calendar connection.
**On your machine:** yes.

### 13. Reading handwriting
Photograph a page of notes, get text back, filed.

**Why:** local OCR is genuinely good now, and the webcam and capture pipeline
are built.
**Effort:** medium.
**On your machine:** yes — OCR is far lighter than image generation.

### 14. Local image generation, including you in the picture
Covered in `MEDIA.md`. Give Atlas a handful of photos of you once; after that
"put me in this scene" is a mask, a prompt and a face embedding.

**Why:** you asked for it specifically, and it's genuinely achievable — just
not fast.
**Effort:** medium.
**On your machine:** yes, but tens of seconds per image on Arc. Background
work, not conversational.

---

# Tier 3 — worth doing, larger

### 15. A second machine
If you get a desktop, Atlas has no story for two of them sharing one thread,
one memory and one backlog.

**Why:** the moment there are two, "where did I leave that" becomes a real
problem. Better designed before there are two than after.
**Effort:** large. The API exists; the merge logic doesn't.

### 16. Driving an app you're looking at, without taking focus
The hidden-desktop approach from `RESOURCES.md`. A second Windows desktop
where Atlas drives its own copy of an app invisibly.

**Why:** it's the only way past the "synthetic input goes to the focused
window" limit. Commercial RPA tools do exactly this.
**Effort:** large, and unpleasant to debug because you can't see it.
**On your machine:** yes.

### 17. Local video generation
**Not possible here.** ~16GB VRAM floor; you have 8GB shared. Listed so it
stays off the plan rather than being rediscovered.

### 18. Speaking while thinking
Say the first sentence while composing the rest, so a long answer doesn't
begin with silence.

**Why:** it's most of what makes an assistant feel fast. Needs streaming from
both the model and the speech engine.
**Effort:** medium-to-large; touches the whole reply path.

### 19. Endpointing
Stop recording when you stop talking, rather than after a fixed eight seconds.

**Why:** the fixed window is the most-copied mistake in voice tutorials and
it will feel slow every single turn. Silence detection is well understood.
**Effort:** medium.

### 20. Its own memory of you, beyond preferences
Not "you like markdown" but "this project matters, that one is parked, this
person is a client". Context that changes what a good answer looks like.

**Why:** the memory system stores facts and has no notion of importance.
**Effort:** medium, and easy to get wrong in a way that's hard to notice.

---

# Tier 4 — genuinely useful, further out

**21. Money-adjacent monitoring, read-only.** Watch balances and flag anomalies
without ever moving anything. The line stays where it is.

**22. A private search across everything you've written.** Notes, drafts,
transcripts, research. Semantic rather than keyword.

**23. Teaching it a workflow by demonstration.** Do something once, say
"remember that", and it records the sequence rather than you writing it out.

**24. Health-aware pacing.** Notice you've been at the desk four hours and say
so, once. Small, and the presence layer already knows.

**25. Multiple voices for different contexts.** Terse for commands, fuller for
research read-back.

**26. A public-facing mode.** Someone else at the desk: no private content,
no personal names, no notifications. The presence layer detects it; nothing
acts on it yet.

**27. Failure post-mortems.** After something goes wrong, Atlas writes up what
it tried and why it failed, so a recurring problem accumulates evidence rather
than being re-diagnosed each time.

**28. Cost awareness.** If a hosted model is ever configured, track what each
request costs and say when something is getting expensive.

---

# Deliberately not on this list

- **Anything that moves money.** Not a gap. A line.
- **Continuous screen reading.** The technology exists; the privacy, battery
  and vision-model costs aren't worth it against event triggers.
- **Replacing ggml.** MIT-licensed and permanent. Rewriting buys no
  independence and costs years.
- **Autonomy for consequential actions.** Every promotion path caps at
  announce-and-do, permanently.
- **Holograms and touchable projections.** Physics, not budget.
- **Distinguishing you from a recording of you.** Neither voice nor face
  solves this. It's why identity never gates permission.

---

# If I had to pick three

**Clipboard input** (#1), because it gives Atlas your context for almost no
work. **Rehearsal mode** (#4), because it's how you come to trust the rest.
**Atlas working on Atlas** (#7), because it makes everything after it cheaper.

---

# Additional ideas, researched after the first pass

## Identity — the Face ID question, answered

You asked whether the Face ID model can be adapted. It can, and the right
adaptation is **not to build face matching at all.**

What makes Face ID trustworthy isn't the camera — it's that the sensor and the
matching live in dedicated secure hardware, and **apps never see your face.**
An app asks the operating system "is this him?" and gets back a yes or no. It
cannot inspect the biometric, cannot store it, cannot be tricked into matching
against something else.

Windows has exactly that, and Atlas can call it. `UserConsentVerifier`
performs a verification using Windows Hello — face, fingerprint or PIN — and
hands back a single result. The developer never has access to any user's
private key; the signing happens inside Windows Hello. It is available to
ordinary desktop apps, not just Store apps.

**So the design is:** Atlas never touches a camera for identity. When
something consequential needs proof, it asks Windows for a Hello prompt. You
get the same experience as unlocking your laptop, backed by the same hardware,
and Atlas holds nothing it could leak.

Three things follow from that:
- It works on any machine that has Hello set up, and degrades honestly on one
  that doesn't — unavailable means "fall back to a spoken yes", never "assume
  it's him".
- A grace window, like `sudo`: prove once, and consequential actions go
  unchallenged for a while rather than prompting every time.
- The webcam face-matching I built earlier stays where it is — **presence**,
  not identity. Knowing someone is at the desk is useful for timing. It is not
  proof of who, and it never gates anything.

**Effort:** small. One WinRT call plus the policy around it.

## Portability, since your friends will run their own

Not multi-user — separate installs. What that actually requires is that
**nothing in a shipped build is specific to one computer**, which was not true
until this round: the config had a username in it, and the dry-run fixture had
particular monitor ids.

Now there are two layers. The generic config ships to anyone and uses
`%LOCALAPPDATA%` placeholders. `machine.yaml` is written by setup on first
run, holds the paths and device names it found, and is never shared. There is
a test asserting no shipped file contains a home directory, so this cannot
quietly regress.

**Still worth adding:**

**29. A single first-run conversation.** Instead of running three batch files
and reading a FAIL list, Atlas walks you through it out loud: finds your apps,
tests each microphone, asks which monitor is which, writes `machine.yaml`.
*Effort: medium. This is what would make it usable by someone who isn't you.*

**30. A shareable build.** One zip, no config editing, no paths to fix.
Everything personal generated on first run. Mostly done; needs the setup
conversation above.

**31. Config validation with plain-language errors.** Already partly there —
a Windows path in quotes now says "use forward slashes". Worth extending: an
unknown app in a startup order, a layout that doesn't exist, a mic name that
doesn't match anything on the machine.
*Effort: small.*

## More ideas worth having

**32. Refusing to answer when it doesn't know.** Atlas currently always says
something. A local 3B model will confidently invent things. A confidence
threshold that produces "I don't know" is worth more than a better model.
*Effort: small. Value: high, and it's the difference between trusting it and
double-checking everything.*

**33. Time-boxing itself.** "Spend ten minutes on this and tell me where you
got to." Bounded effort with a report, rather than either finishing or
failing.
*Effort: small; the lane queue already tracks running work.*

**34. Remembering what didn't work.** When an approach fails, record it
against the problem so the same dead end isn't walked twice next week. The
backlog stores blocked tasks; it doesn't store failed approaches.
*Effort: small.*

**35. A "what changed" report.** After a week: what it learned, what you
approved, what it stopped asking about, what it gave up on. Learning that
happens invisibly is learning you can't correct.
*Effort: small. All the data exists.*

**36. Draft quality separated from send.** Right now a draft is one shot.
Better: generate, critique its own draft against what you've approved before,
revise once, then show you. Costs one extra model call.
*Effort: small.*

**37. Escalating to you rather than guessing.** When confidence is low on
something consequential, don't pick the safest option silently — say which two
options it's between and why. Currently it either asks a yes/no or does the
safe thing.
*Effort: small.*

**38. A panic word.** One phrase that stops everything, drops every queue, and
puts Atlas to sleep. "Pause" suspends; this abandons. Worth having before it
can do anything irreversible.
*Effort: small. Value: entirely about confidence.*

**39. Local encryption of the state folder.** Notes, transcripts and the index
sit in plain JSON. Windows DPAPI ties encryption to your account with no
password to manage.
*Effort: medium.*

**40. Its own changelog.** Atlas writing down what changed about itself each
time you accept a change from the sandbox — so six months in, you can see how
it got to where it is.
*Effort: small, and it compounds.*

## The three I would add next

**#32 refusing to answer when it doesn't know**, because a local model that
invents things quietly is worse than one that admits ignorance loudly.
**#38 a panic word**, because it costs an hour and changes how comfortable you
are giving it more. **#29 the first-run conversation**, because it's the
difference between something you can hand a friend and something only you can
run.

---

# The hub, and where secrets should live

## The hub (built this round)

You were right that this was a gap. Everything I'd added had a switch and
nowhere to flip it.

The hub is a **page, not an app** — served on loopback by the API server that
already existed. That choice matters: nothing new to install, it works from
your phone and iPad over the same connection, and **Atlas still runs with no
window open**. You open it the way you open a router's admin page:
occasionally, on purpose, then close it.

Plain HTML, no JavaScript framework, no external requests — it has to work
offline because Atlas does. Six pages: Status, Settings, Permissions,
Accounts, Activity, Outstanding.

Two things it does that a plain settings screen wouldn't:

**Every switch states its cost.** A toggle that turns on your camera does not
look identical to one that changes how many sentences Atlas speaks. There's a
test asserting anything consequential explains what it costs.

**A Permissions page.** Everything that can reach a sensor, leave the machine,
or act without asking, gathered in one list worth reading once a month. That's
the page that stops permissions accumulating invisibly.

## Where your logins should live

You asked for Atlas to log into sites for you. My answer hasn't changed since
the first audit, but it's worth restating now that you're asking long-term,
because the reasoning is the whole design:

**Atlas should never hold the secret. It should hold a reference.**

Windows already has a credential store — DPAPI-backed, tied to your account,
unlockable by your login, no master password to lose, and audited by people
whose job that is. A homegrown vault written by one person and one assistant
is a worse version of that with none of the scrutiny.

So the shape is:

- Secrets live in **Windows Credential Manager**. Atlas stores a name.
- To log you in, Atlas navigates to the page, asks Windows for the credential,
  and hands it to the browser **without ever seeing it in its own memory**.
- Anything it does hold — which sites you have accounts on, when you last
  logged in — is deliberately boring.
- The **Accounts** page in the hub lists what it can reach and lets you revoke
  any of it, without ever displaying a secret.

That gets you the capability you want, and if this laptop is ever compromised,
Atlas's own files give an attacker a list of website names.

**One thing I'd still argue against:** payment details. Your browser already
does this behind a biometric prompt, in code that's audited and hardened.
Atlas typing a card number into a page is strictly worse, and the only thing
it saves is one tap.

---

# Capabilities worth considering, long term

You asked for anything valuable, so this is broad rather than filtered.
Feasibility on your hardware is noted where it matters.

## Calendar and meetings

**41. Calendar read and write.** "What's my day?", "move my 3pm", "find an
hour with Sam this week." Outlook and Google both expose this. This is the
single biggest unlock on the list — half the ideas below need a calendar to be
useful.
*Effort: medium. Value: very high.*

**42. Meeting prep, automatically.** Ten minutes before a call: last thread
with those people, open items, files touched since. Trigger layer is built.

**43. Joining calls.** Zoom, Teams and Meet all take a URL. Atlas can open the
link at the right moment, mute you on entry, and put the window where you want
it. It should never *speak* in a call — that's a line worth keeping.
*Effort: small once a calendar exists.*

**44. Listening to a call and taking notes.** Technically straightforward —
capture system audio alongside your mic, transcribe both, extract decisions
and actions afterwards. **The hard part is not technical.** In most places
recording someone without telling them ranges from rude to illegal, and it
varies by jurisdiction. If this gets built it should announce itself, default
to your side only, and never record silently.
*Effort: medium. Worth building with the consent behaviour first, not bolted
on after.*

**45. Post-meeting follow-through.** Draft the summary, the follow-up email,
and the calendar items — all as drafts, none sent.

## Accounts and the web

**46. Logging into sites.** As above: reference model, browser fills, Atlas
never sees the secret.

**47. Site routines.** "Check my orders", "download this month's statements",
"is anything waiting for me?" — recorded once, replayed. Site profiles exist.

**48. Watching a page for change.** Price, availability, a status page, a job
board. Cheap, and genuinely useful.

**49. Filling long forms** from things it already knows about you — address,
details, prior answers. Never payment.

**50. Reading your email and triaging it.** Not replying — sorting. "Three
things need you today, the rest can wait."

## Media and entertainment

**51. Streaming control.** Spotify has a proper API; Netflix and the rest are
browser-only, so "put something on" means driving a page. Doable, brittle when
they redesign, and honestly of modest value compared with the rest of this
list.
*Effort: small per service, ongoing maintenance forever.*

**52. Anything playing, paused for a call.** Genuinely useful and much cheaper
than full control — Windows exposes global media keys.

**53. Music matched to a mode.** Focus mode starts the focus playlist. Trivial
once modes and media control both exist.

## Documents and thinking

**54. A private search across everything you've written.** Notes, drafts,
transcripts, research — semantic rather than keyword. This is the thing that
makes months of accumulated notes actually pay off.
*Effort: medium. Needs a local embedding model, which your hardware handles
comfortably — they are far smaller than a language model.*

**55. Reading a long document properly.** Not a summary — answering questions
about it, with the passage it came from.

**56. Turning a rambling voice note into a structured document.**

**57. Tracking a decision over time.** "What did we decide about the VPS, and
when did it change?" Atlas has the thread; nothing mines it.

## Home and hardware

**58. Smart lights and plugs.** Zigbee, no cloud. Worth pairing with modes.

**59. Waking the machine, or putting it to sleep on a schedule.**

**60. Noticing hardware trouble early.** Disk health, battery wear, a fan
running constantly. Some of this exists in the health module.

## Money-adjacent, read-only

**61. Watching balances and flagging anomalies.** Read-only, never moving
anything. The line stays where it is.

**62. Reading statements and receipts into a ledger.** Local files you already
have, categorised. This is Phase 1 of your original spec and still unbuilt.

**63. Subscription watch.** "You're paying for three things you haven't opened
in six months." Needs statement reading plus app usage.

## The ones I'd argue are most valuable

Out of everything on both lists:

1. **Calendar (#41)** — unlocks the most other things.
2. **Private search over your own writing (#54)** — the payoff for everything
   Atlas has been accumulating.
3. **Email triage (#50)** — the highest-frequency drain that a machine can
   genuinely help with.
4. **Statement reading (#62)** — already in your spec, entirely local, no
   credentials needed.
5. **Site routines (#47)** — the "do the boring thing for me" case that made
   you want this in the first place.
