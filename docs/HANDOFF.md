# Atlas — Full Handoff

> **STALE — superseded.** The first full handoff, written when the tree was a third of its size. Last true on 9 September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

**9 September 2026.** Written so a new chat can pick this up cold — Eric,
and whichever Claude reads this next. Everything below is accurate as of
this document's creation. Where something is uncertain or unverified, it
says so; nothing here is dressed up.

---

## 1. What Atlas is

A local-first personal assistant, written in Rust, running as a single
`atlas.exe` on Eric's Windows laptop. Voice in, voice out, a typed prompt,
a settings hub in the browser, and — as of tonight — the ability to pair
with another Atlas instance over Tailscale. No cloud dependency for the
core loop: speech-to-text (whisper), text-to-speech (piper, with Chatterbox
chosen as the eventual upgrade), and reasoning can all run against a local
model. A hosted model can be plugged in via config for anything that needs
more capability than the local one has.

The whole project runs on one standing rule Eric has stated repeatedly:
**as offline and self-built/self-hosted as possible, for everything.**
Third-party services are studied as concepts to copy, never adopted as
dependencies, unless Eric explicitly says otherwise for a specific case.

**Eric's setup:** Windows laptop, runs lid-closed behind external monitors
("do nothing when closed"), AirPods Pro always connected, a webcam mounted
above the monitors, Intel audio (not Realtek — an early wrong guess).
Folder: `C:\Atlas\Atlas`. iPhone for himself; Android friends get a native
app eventually; iPhone friends get a PWA (no Mac available, and Apple
requires one to build native — see §7).

---

## 2. Where things stand right now

- **Test suite: 2,835 passing, 0 failed, 0 warnings.** Confirmed by running
  in four batches (the suite has grown past what this sandbox's disk can
  compile in one pass — 135 test files, each producing its own linked
  binary). Splitting into batches with a `cargo clean` between each is now
  the standard way to get a true full-suite read here; a linker crash
  midway through a giant single run is almost always disk pressure, not a
  real failure — check `df -h` before concluding otherwise.
- **194 files in `src/`, 56,691 lines.** 135 test files.
- **The Windows build works end to end**, confirmed on Eric's real
  hardware: `atlas doctor` reports all-ok (mic, speakers, both models,
  memory, disk), the interactive prompt reaches real daemon actions, voice
  mode auto-detects the right microphone, and Smart App Control (which was
  blocking the exe) has been turned off on his machine.
- **Two packaged outputs, refreshed after every change:**
  - `Atlas.zip` — the ready-to-run folder (exe + `ATLAS.bat` + config +
    empty `models`/`tools` folders + a `START HERE.txt`). This is what
    Eric drops onto his laptop.
  - `atlas-source-complete.zip` — the entire source tree, buildable on
    Linux/Mac (`cargo build`) and cross-compilable to Windows
    (`cargo build --release --target x86_64-pc-windows-gnu`, needs
    `rustup target add x86_64-pc-windows-gnu` and `mingw-w64` installed).

**A new chat's first move should be: extract `atlas-source-complete.zip`,
run `cargo test` (batched if disk-constrained), confirm the same 2,835/0/0,
and only then start changing anything.**

---

## 3. How the codebase is disciplined (read this before touching code)

This project has a real, working set of self-enforcing rules, built up
over the session and by outside parallel audits. They are not decoration —
several have caught real bugs, including bugs in Claude's own work
tonight. A new session should know these exist and respect them:

- **`tests/wiring.rs`** — every module must either be reachable from
  `main()`, or be named, with a real reason, in `UNWIRED_BASELINE`. The
  baseline can only shrink (a real test enforces this) and the ceiling
  tracks it (also enforced, and enforced a *second*, independent way by
  `tests/ceiling.rs` — see below). Currently 55 modules are deliberately
  unwired; the full list and reasons are in §5.
- **`tests/ceiling.rs`** — re-reads `wiring.rs` as raw text and re-checks
  its own numbers independently, specifically because internal guards have
  gone missing or gone blind before (its own docstring lists three prior
  incidents). It caught a real bug tonight: `wiring.rs`'s own ceiling test
  used `saturating_sub`, which silently passes even when the unwired list
  has grown *past* its ceiling. Both are fixed now, and they agree.
- **`tests/declared.rs`** — every file in `src/` must be declared in
  `lib.rs`, every `pub mod` must have a file, every test's `use atlas::x`
  must name something real. Exists because `links.rs` once had 211 lines
  and 19 tests and was never declared, silently taking down the *entire*
  test suite's ability to compile — and the wiring guard couldn't report
  the problem, because a guard cannot run in a build that doesn't finish.
- **`tests/guards.rs`** — a manifest of specific, named, load-bearing
  safety mechanisms (a constant, a function name, a specific string) that
  must still exist in a specific file, each with a one-line reason for why
  its removal would be bad. Every real fix tonight that mattered got an
  entry here. This is the single most important file to read before
  believing anything is "safe" — check whether it's actually guarded.
- **`tests/retrospective.rs`** — scans the whole test suite for tests that
  assert nothing, or that only prove a string contains a phrase (wording,
  not behavior) — with a baseline that can't grow. Caught several of
  Claude's own shallow tests tonight; each was fixed to assert real
  behavior rather than removed.
- **`src/hollow.rs`** — the pattern-level lens for the single most
  recurring bug class found tonight: **an absence of a finding is not the
  same as an absence of a problem.** A stub that returns
  `Readings::default()` looks exactly like a healthy machine. A catch-all
  arm that says "not wired to an action yet" is telling the truth about
  itself and nobody's listening. `tests/no_quiet_nothings.rs` ratchets
  this across the whole source tree.

**Recurring practical hazard:** several `str_replace` edits tonight
accidentally swallowed a function's own signature line when the `old_str`
was just the signature and the `new_str` didn't re-include it. Caught both
times by the immediate compile error, but worth watching for.

**Recurring practical hazard #2:** near-identical module names have caused
real confusion twice: `signal.rs` (new) vs. pre-existing `signals.rs`
(self-audit findings) — resolved by renaming the new one to `kin.rs`. And
`mending.rs` (Claude's own correction-loop module) vs. an audit's new
`mend.rs` (refuses shortcuts, asks jargon-free questions — a different
concept) — resolved by renaming Claude's to `revise.rs`. **Before naming
a new module, check the existing listing.**

**Recurring practical hazard #3:** this sandbox's disk is small relative
to the project's build artifacts. `cargo clean` liberally, especially
before a cross-compile or a full test run, and prefer running test files
in a handful of `--test` batches over one giant `cargo test` once the
suite is large.

---

## 4. Session history — how tonight went, roughly chronologically

This is long on purpose. It's the "why" behind decisions a fresh session
would otherwise have to re-derive or, worse, accidentally re-litigate.

**Getting Atlas to launch at all.** Started from "Could not find
atlas.exe" — turned out to be two separate zips (`atlas.zip` = source
only, no exe; `atlas-prebuilt.zip` = the runnable one) and confusion about
which to open. Then a real bug: `ATLAS.bat`'s `:get_zip` checked for the
downloaded exe at the top level of the extraction, but both whisper's and
piper's real release archives nest it one folder deeper — so both
downloads "succeeded" and were reported as failed, and voice never
installed. Fixed with a flatten step. Verified against the real upstream
archives.

**The interactive prompt not reaching real actions.** The typed prompt had
its own tiny six-intent dispatcher instead of using the real `Daemon`, so
most things printed "parsed X — not wired to an action yet" — which was
true of the prompt, not of Atlas. Fixed by building the same `Daemon` the
`--daemon` mode uses (minus voice) and routing typed lines through it.

**The readings stub.** `Daemon::readings()` returned `Readings::default()`
— every field zero, forever. Nothing had ever read real memory or disk.
Hidden because `assess()` only reports on values above zero, so an
unread machine looked identical to a healthy one. This is the bug that
`hollow.rs` was written in direct response to. Fixed with real
`GlobalMemoryStatusEx`/`GetDiskFreeSpaceExW` calls, confirmed on Eric's
real hardware afterward.

**The "push me" nudge system.** Eric wanted Atlas to initiate — stalled
goals, drift detection, daypart briefs, occasional check-ins — with very
specific behavioral rules he gave directly: back off on silence, ask why
exactly once and never again, believe a "no" the first time, habits and
health are in scope but medical topics are permanently out (a `const`,
not a setting). Built as `nudge.rs`.

**System-optimization permission.** Eric explicitly granted Atlas
permission to touch the machine for optimizations (previously it could
only ever propose). Built `checks.rs` — 32 real Windows mechanisms sorted
by reversibility, with a permanent no-list (credential dumps, Prefetch
deletion, registry cleaners, disabling Memory Integrity) that a person
would have to argue past, not just override.

**Council, brief, and the business direction.** `council.rs` —
blind-round, multi-persona decision-making, tuned to catch and report
suspicious unanimity. `brief.rs` — the morning coordination run. Both
came from a large research pass over TikTok videos Eric sent (~50 uploaded
+ ~78 links, transcribed) about AI-agent and "build your own Jarvis"
content — most of it was either things Atlas already does better, or
paid courses selling what Atlas already has for free. A few genuinely
good ideas came out of it: the four-write memory loop (live write → sweep
before compression → nightly consolidation → corrections stored as *why*,
not mood), the vault index pattern (one master index read at boot, notes
loaded on demand), and the mic-quality-over-Bluetooth heuristic that
became `audio.rs`.

**The memory/correction loop.** Built as `mending.rs` (later renamed to
`revise.rs`), then substantially extended by an audit into `mend.rs` (a
different, complementary concept — see §3's naming-collision note). The
core rule Eric cares about: one correction is a note, two earns an actual
edit to how Atlas behaves; a correction is stored as *why it was wrong and
what right looks like*, never as "the user was unhappy."

**Voice.** Piper is what's running; Chatterbox is the chosen upgrade
(free, MIT, self-hosted, better quality). `tts.rs` was made pluggable
across engines with a documented, tested fix for a real landmine: piper's
"speed" is an inverted length-scale (bigger number = slower), while
Chatterbox and most others use a normal speed multiplier — silently
swapping engines without translating this would have inverted every speed
preference Eric ever set.

**The mic auto-detection saga.** Doctor originally reported the mic name
as a hardcoded guess ("Realtek") that didn't match Eric's actual hardware
(Intel). Fixed the literal string first — then Eric explained his real
setup (lid closed, AirPods, webcam) and explicitly rejected the manual
fix, asking for real detection instead. Built properly: `audio.rs` now
probes real Windows devices live every time `--voice` starts, and — the
actual insight — **excludes the built-in mic entirely whenever the
laptop's own monitor isn't part of the currently active display layout**,
reusing Atlas's existing monitor-role tracking rather than trying to query
Windows for lid state directly (which isn't reliably queryable). Windows
can keep listing an internal mic even with the lid shut; "it's in the
device list" was never proof it's usable. Verified with 17 tests including
the exact closed-lid scenario.

**Tailscale, `kin.rs`, and cross-Atlas communication.** Eric wants his
phone to reach his desktop Atlas from anywhere (via a Siri Shortcut,
eventually), and separately wants a *second* Atlas — his own, running
on a future self-hosted server — to
be able to notify his personal Atlas of something urgent, **without the
two being linked**. Decided: Tailscale for the network layer (private,
free at this scale, no public exposure). Built `kin.rs`: a structurally
separate, notify-only channel — named peers only (never trust-on-first-
use), its own token namespace entirely apart from the phone's, rate-
limited per peer, and the single hard rule, guarded: **a signal can only
ever become a `Nudge`. There is no code path from "message arrived" to
"command executed."** Proven not just by test but by an actual test that
sends a peer's token at `/say` (something the phone's token could do) and
confirms it goes nowhere.

Then: `SignalListener` — a non-blocking listener checked once per daemon
tick (same pattern as the existing keyboard poll), so the door is actually
*live* while `--daemon` runs, not just built-and-dormant. Proven with real
sockets and real concurrent client threads, not mocked.

Then: `atlas invite` / `atlas accept` — the easy-setup pairing flow Eric
asked for explicitly ("my friends don't want to read a document"). Three
commands total, no file-editing, no manual token pasting — a single
shareable code block exchanged twice completes a full mutual pairing.
Verified end-to-end against the actual compiled binary, not just unit
tests.

**The 8-audit merge, tonight.** Eric periodically sends zipped patches
from a parallel Claude session working the same codebase independently.
Tonight's batch: `mend.rs`, `craft.rs`, `tier.rs`, `goal.rs` (four new,
related modules — see §6), `onlyone.rs` (single-instance lock, heartbeat-
based rather than pid-based, deliberately wired only into `--daemon` and
*not* the quick interactive prompt — locking the prompt too would make it
useless whenever the daemon is already running in the background, which
seemed like a real usability regression the source branch hadn't
considered), two genuine UTF-8 crash fixes (byte-offset string slicing
panicking on any accented character or em dash — one in the chunked-HTTP
decoder used by any web fetch, one in the report-checker), and a
refinement to how Atlas asks a person a question it can't answer alone
(parks as a real to-do item rather than guessing a default). Also found
and fixed the `saturating_sub` bug in `wiring.rs` mentioned in §3.

**Tonight's own wiring push.** Eric pushed hard, in capitals, on not
neglecting to actually wire things in rather than leaving them
tested-but-dormant. Wired `tier.rs` into the daemon's command path (tracks
whether an answer came from a named capability, a cached report, or an
actual model call — and says so if reports never seem to cover what's
asked). Built a real `atlas craft <dir>` command wiring `craft.rs`'s
toolchain-ladder logic to actual process execution (cargo/ruff/pytest,
cheapest signal first, tool's own words reported verbatim) — `goal.rs`
came wired for free as a result, since `craft.rs` already converts a
build spec into a `goal::Goal` internally. `mend.rs` was deliberately
**left unwired** — using it for real means Atlas proposing an actual code
fix, which needs the LLM integration piece, and that felt like a decision
that deserved its own moment, not something to fold in as a side effect
of a merge.

---

## 5. What's wired, what isn't, and why (as of tonight)

**Confirmed wired tonight:** `tier`, `craft`, `goal`, `onlyone`, `kin`
(both the receiving door and the pairing/invite flow), `audio`, `hollow`,
`links`/`integrations` (merged; `integrations.rs` kept, `links.rs`
retired), `checks`, `council`, `brief`, `nudge`, `mending`/`revise`,
`recall` fixes, `decide`, `opportunity`, `contents`, `trace`.

**Deliberately still unwired (55 modules, in `UNWIRED_BASELINE`), with the
general shape of why:**

```
mend        — auto-fix generation needs the LLM piece; deliberately held back
look        adapt       afterme     android     backends    budget
cloudsync   codes       companion   confirmed   consent     content
credentials delegate    diagnose    dictate     edit        editors
endpoint    firstrun    fit         gguf        grade       hearing
household   identity    improve     ios         language    models
ocr         overlay     overnight   person      plainly     profiles
prose       publishing  quickinput  reach       research    recovery
remote      returning   system      uia         voiceid     voiceover
walkthrough wanted      wants       watching    workingset
```

Most of these fall into a few buckets:

- **Hardware that's never run on real Windows yet:** `overlay`, `look`,
  `uia`, `watching`, `voiceid`, `hearing`, `audio`-adjacent bits, `dictate`,
  `endpoint`, `quickinput`, `ocr`. Some may already be closer to ready
  than the list suggests, now that Atlas has actually run cleanly on
  Eric's machine — worth a fresh pass.
- **Second-device / phone-related:** `ios`, `android`, `cloudsync`,
  `codes`, `companion`, `confirmed`, `household`, `recovery`, `remote`,
  `workingset`, `afterme`. Blocked on the phone-connectivity decision,
  which Tailscale + `kin.rs` has now substantially answered — this bucket
  is likely the single best next place to make real progress.
- **Business/content pipeline:** `content`, `publishing`, `prose`,
  `plainly`, `editors`, `reach`, `grade`, `walkthrough`, `wanted`,
  `wants`, `person`, `returning`, `research`. Eric explicitly wants this
  ("Atlas is meant to be a business and content manager") and it connects
  directly to §7's open design conversation.
- **Everything else** is smaller and more situational — worth reading
  each module's own doc comment before assuming why it's unwired; several
  have real, specific rationale already written at the top of the file.

---

## 6. The `craft`/`goal`/`mend`/`tier` family — how they fit together

These four are not independent; they were clearly designed as one system
by the parallel audit session, and tonight's wiring confirmed it:

- **`tier.rs`** decides, cheaply and before anything else, whether a
  request even needs a model: a named capability (run it), something
  already worked out and still fresh (answer from that), or genuinely new
  thinking. Wired into `run_command`.
- **`craft.rs`** is the offline build loop: a fixed, per-language ladder
  of checks (cheapest and most informative first — `cargo check` before
  `clippy` before `test`; a failure that blocks later checks stops the
  ladder there, so a next attempt never works from noise). Wired as
  `atlas craft <dir>`, which actually executes the ladder today and stops
  at diagnosis.
- **`goal.rs`** is what `craft.rs` converts a build spec into internally —
  success criteria, a hard attempt cap (never unlimited), and an explicit
  `runnable_unattended()` check that refuses to run alone if any criterion
  can only be judged by a person.
- **`mend.rs`** is the layer that would sit between "the ladder failed"
  and "here's a fix": it refuses shortcuts by name (widening a type until
  an error disappears, deleting a failing test, swallowing an error) and
  enforces that any question put to Eric is answerable without reading
  code. **Not yet wired to anything that generates a fix**, because that
  generation is the LLM piece.

**The natural next step for this family**, if a new session picks it up:
wire an actual fix-generation loop for `atlas craft` — on a `Next::Fix`,
have the LLM propose a change, run it through `mend::worth_trying` before
applying, and use `mend::Question`/`.parked()` when the fix needs a
decision Atlas can't make alone. This is real, valuable, and squarely
within what Eric has been asking for all along (an app/tool builder), but
it's also the point where autonomy and safety questions genuinely start
to matter, so it deserves the same care `kin.rs` got.

---

## 7. Open design conversation — not yet built, several not yet even discussed

This is the substantial part of what Eric is waiting on. He explicitly
asked for real research and real thought here, and it kept getting pushed
back by the mechanical work (merges, bug fixes, wiring). **A new session
should treat this as the actual priority**, not another backlog item.

### 7.1 Hub navigation and visual design — not started

Eric's words: he doesn't want each business to be a flat separate page in
the hub; he wants a real drill-down structure (a list/index of things,
click into one). He also explicitly asked for **genuine, in-depth research
into modern productivity/business app design** — not a guess — because a
sloppy-feeling tool would subconsciously read as less credible to a
business partner he plans to hand this to. **This research has not
actually been done yet.** The hub today is server-rendered HTML from
`hub.rs`, functional, not designed. This needs real web research (current,
2026 productivity/business app patterns) before any redesign work starts.

### 7.2 The confidence system — conversation started, nothing built

Eric's framing, close to verbatim: he does **not** want the fix for "Atlas
isn't confident enough" to be "raise the bar until it acts less" — that's
not smarter, it's just more restrictive, and it would mean he's back to
doing everything himself. He wants Atlas to become genuinely more capable
so that confidence is *earned and correct* more often, not gated harder.
He explicitly asked: **how do we actually get Atlas to that point of
intellect and confidence?**

The honest answer, sketched but not built: confidence should be *per
task-category*, calibrated against a real track record — this connects
directly to `trace.rs` (already logs every model call, including a
`graded: Option<bool>` field) and `mending`/`revise`'s repeat-rate metric
(does it make the same mistake twice). A category earns a higher
confidence ceiling only after demonstrated correct outcomes in that
category specifically, not globally. This has not been designed in
detail or built.

### 7.3 Business-task graduation — conversation started, nothing built

Directly tied to 7.2: Eric wants Atlas to start on personal tasks, build
a track record, and only then graduate to acting on business tasks — with
business actions needing a visibly higher bar than personal ones, because
a wrong confident guess on the business side costs a partner who never
agreed to that risk. Not designed in detail.

### 7.4 The personal/business firewall — partially specified, explicitly NOT to be finished without Eric

Eric gave real, concrete examples, not a full specification:

- **Allowed on the business side:** work tasks tied to the business,
  sharing calendar *availability* with a partner.
- **Never allowed to leak:** personal files not linked to the business;
  and more generally, sharing "the wrong ones."
- **His explicit instruction, verbatim in spirit: don't set a boundary
  without talking to me first.** This is a hard constraint on process, not
  just outcome — a new session should not invent the specifics of this
  enforcement mechanism unilaterally, even with good intentions.
- **One question asked of Eric and not yet answered:** what should "it
  leaked past the boundary" actually look like if it ever happened —
  silently blocked, flagged to him first, the whole share paused? This is
  still open.

The architectural direction discussed (not committed): a named, scoped
"Business" concept — its own space, own participants (tied to `kin.rs`
peer identities), an explicit allow-listed set of shared categories, an
append-only shared log rather than a mutable shared database (Eric
explicitly agreed with append-only), and — critically — **read-and-
propose only until proven**, before any autonomous action touches shared
data. This is a real, substantial feature that deserves its own careful,
incremental build, the same way `kin.rs` did.

### 7.5 Cross-device link/document sharing — discussed, not built

Eric wants to submit a link, video, or document from the **phone** app and
have Atlas actually use it (open it, read it, act on it), with that
capability traveling to the **desktop** too, and applying to both personal
and business scopes. The underlying pieces already exist —
`browser.rs` drives a real headless Chrome instance; `research.rs`
fetches and extracts from a URL — so this is substantially a wiring and
sync-design task, not new capability from scratch. Not yet designed in
detail: how "phone submits, desktop picks it up" actually syncs (likely
the same Tailscale-reachable-server pattern `kin.rs` already established,
since it's the same person's own devices, not a cross-trust-boundary
case).

### 7.6 Voice-triggered pairing — discussed, safety concern raised, not designed

Eric asked whether, once Atlas is running on both his and a partner's
machine, he could just say something like *"Atlas, invite [name] to
[business]"* — and in the same breath flagged the real risk himself:
misrecognition here could be genuinely bad (wrong person invited, wrong
scope). **Not designed yet.** The shape that was gestured at but not
committed: voice can *start* the flow, but the parsed name/scope must be
read back and explicitly confirmed before any real token is generated or
sent — the same pattern already used elsewhere in the codebase for
consequential actions, not a new invention.

### 7.7 The second Atlas (server-side) — explicitly held open

Eric plans a second Atlas on a future self-hosted server, deliberately
*not linked* to
his personal Atlas but able to communicate with it (this is the direct
motivation for `kin.rs`). **Eric explicitly asked Claude to hold off on
giving objections/ideas until he's ready to have that conversation.**
He has not yet asked to resume it. A new session should not resume this
unprompted — wait for Eric to raise it.

One tension worth having ready for that conversation, already noted:
"not linked" and "my Atlas can work on the server" pull against each
other, and the likely resolution is that Eric's personal Atlas gets its
own *account* on the server (reachable, scoped) rather than any back door
into server-Atlas's own memory or trust — but this hasn't been proposed
to Eric yet.

---

## 8. Standing facts worth not re-deriving

- **Eric is not deeply technical and has said so directly** — he wants
  one clear instruction at a time when something is operational
  (installing, configuring), but has gotten comfortably capable with the
  actual running Atlas (typing commands, running doctor, reading output)
  over the course of this session. Calibrate to where he actually is, not
  where he started.
- **He wants everything genuinely wired, not tested-and-dormant** — said
  forcefully tonight. When a module gets built, the default should be
  "wire it to something real," and leaving it unwired should be a
  deliberate, stated decision with a reason (as `UNWIRED_BASELINE` already
  requires), not a default.
- **He wants real research, not guesses**, especially for anything
  design- or capability-facing (the hub visual design ask is explicit
  about this).
- **Third-party services get researched for concepts, not adopted** —
  this has held throughout: Hermes Agent (free, studied, not adopted),
  ElevenLabs/Cartesia (studied, Chatterbox chosen as the free/local
  answer instead), Tailscale (the one deliberate exception — adopted
  directly, because it's free at this scale, genuinely private, and
  reinventing a mesh VPN isn't a sensible use of build time).
- **Every Windows-facing fix in this session has eventually been verified
  against Eric's real hardware**, not just tested in the sandbox. This
  matters — several real bugs (the archive nesting, the readings stub,
  the wrong mic guess) were invisible in tests and only surfaced on his
  actual machine. Treat "compiles and tests pass" as necessary, not
  sufficient, for anything touching real Windows behavior.

---

## 9. Suggested first moves for a new chat

1. Read this document in full before doing anything else.
2. Extract `atlas-source-complete.zip`, confirm the build and test suite
   match §2 (batch the tests if disk-constrained).
3. Ask Eric directly which of §7's open threads he wants to work on next
   — don't guess. Given his own stated priorities, the hub design
   research (7.1) and the confidence system (7.2/7.3) are the two he's
   pushed for most insistently and gotten least far on.
4. Keep wiring things in as they're built. Check `UNWIRED_BASELINE`
   before adding anything new, and remove entries the moment something
   becomes genuinely reachable — the ratchet only holds if this stays
   disciplined.
5. Don't invent boundary specifics for §7.4 without asking Eric first —
   he has been explicit about this twice now.
