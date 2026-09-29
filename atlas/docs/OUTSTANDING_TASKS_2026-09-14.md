# Atlas (personal/business) — outstanding tasks, 14 September 2026

> **STALE — superseded.** A dated task list. Last true on 14 September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

Supersedes `OUTSTANDING_TASKS_2026-09-13.md`. Companion to
`HANDOVER_2026-09-14.md`. Organized by who unblocks it, since that determines
what happens next — not severity.

Ten items from the previous list are **done** and no longer appear:
`endpoint.rs` and `hearing.rs` are wired, plus:
`returning.rs`'s deeper integration, `household::share_with_friend`, the
two-declarations-of-a-default question, the file half of a handoff,
`adapt.rs`'s wiring, microphone enumeration, the install/update hassle, and
`MODULE_REFERENCE`'s regeneration.

---

## 1. Needs a ruling from Eric

- **~~Is 3500 or 6000 the right default for `models.memory_budget_mb`?~~
  Withdrawn — it is not a decision.** `models.rs` is unwired, and
  `ModelsConfig` appears in exactly one place in the tree: as a field on
  `ToolsConfig`, which is config-only presence. Nothing calls `choose`,
  `best_fit` or `explain`, so neither number controls anything. What runs
  reasoning is `ShellLlm` shelling out to ollama, which manages its own
  memory. When `models.rs` is wired the budget should be **derived from the
  machine** via `fit.rs`, which already reads real memory — the same
  correction already made to `wants.rs` — rather than typed into a config
  file at all. Tracked under `models.rs` below, not as a standalone ruling.

- **~~Three separate lists track "is this module wired"~~ — kept, deliberately,
  and now they say so.** They check three different things and
  `capability_wiring.rs` skips type-less modules *because* `wiring.rs` covers
  them. Merging would over-exempt. What was wrong was discovery order: every
  failure message now names the other two lists. Section 20. An attempt to
  compute all three answers in one place was written and deleted — it got a
  different answer on its first run, because it was a cheaper copy of a rule
  that already lives where it is needed.
- **The real duplication was the deadness rule itself** — `calls`/`whole_word`
  byte-identical in two test files that keep two lists which must agree. Now
  `tests/common/mod.rs`, read by both, with a mutation proving one edit moves
  both readers.
- **~~Three separate lists~~ (original note): `UNWIRED_BASELINE` in
  `tests/wiring.rs`, `CAPABILITY_UNWIRED` in `tests/capability_wiring.rs`, and
  `KNOWN` in `tests/new_capabilities_are_wired.rs`.** Wiring `models` needed a
  deletion from all three, and the third only surfaced on a full run after the
  first two were green. They do not disagree today, but three copies of one
  fact is how two of them came to disagree before. **Worth collapsing, and
  that is Eric's call** — it is the same judgement as retiring the summed
  ceiling.

- **`AskClarification` still evaporates when you are away.** The approval
  branch is fixed (section 22); this one is the same hole. `session.ask` parks
  the question in the session, which is gone if you walk away. `mend` has the
  shape for it and `about_ambiguity` was written and then **deleted**, because
  the daemon has one clarification sentence and not two readings — building the
  question would have meant inventing the options. Needs the ambiguity
  resolution to produce its candidates before the question can be honest.

- **Which of the ~26 remaining `UNWIRED_BASELINE` modules to tackle next,
  and roughly in what order.** Unchanged from the previous list. Several
  need a decision before building rather than more wiring effort:
  - `confirmed.rs` / `consent.rs` — both need genuine live, multi-turn
    daemon state. `confirmed.rs` additionally needs a real
    browser-automation backend to actually *make* the security-page change
    it confirms; without one, wiring the confirm/read-back protocol alone
    would let someone say "yes" to nothing happening.
  - `quickinput.rs` — the pure state machine is wireable today; the OS-level
    global hotkey registration needs real Win32 work this sandbox can't
    verify. Worth asking whether a console-surface-only partial wiring is
    wanted, or whether to hold for the hotkey work. (Note that
    `quick_input.enabled` is now correctly `true` in the shipped config,
    which it was not before — the module is still unwired either way.)
  - `android.rs` / `ios.rs` / `companion.rs` — mobile-app scope entirely.
  - `gguf.rs` / `models.rs` — a real local-LLM-serving layer; a large
    integration on top of the existing external-command `ShellLlm` path.
    Worth scoping as its own project rather than folding into a wiring pass.
  - `budget.rs` / `overnight.rs` — both need a real hosted-model
    integration to attach to, which does not exist in this tree.
- **Cross-instance business-hub linking.** Scoped in an earlier session,
  explicitly held back pending Eric's own say-so. Still not built. Note
  that `atlas share` (this session) is a first, deliberately narrow
  crossing between two instances — a note, one way, carrying nothing —
  which may or may not be the right foundation to build the business
  version on. Worth looking at before the linking work starts.
- **The personal/business firewall's actual trigger.** `firewall.rs` is
  complete and tested. Nothing in the product yet *attempts* a
  personal-to-business crossing outside the manual CLI simulation
  (`atlas shared check`); `roster.rs`/`shared_task.rs` gave it a first door
  for tasks, but what else should cross (shared calendar? client list?
  research tagged to a business?) is still open.
- **Should a file handoff ever resume, or is one-shot enough?** The file
  half is built and capped at 8MB, sent in a single POST with a 120-second
  timeout. A transfer that dies at 90% starts again from nothing. Fine at
  8MB over Tailscale; worth revisiting if the cap ever rises.

## 1a. Blocked behind something bigger, found this round

- **~~`nudge::drifted` cannot be wired yet.~~ Done.** The missing half of
  `contents.rs` is built: `from_folder`, `says_for`, `parse`, `save`, `load`,
  `rebuild`, `master_path`, `names_on_disk`. The daemon writes a master index
  at `data/index.md`, reads it back rather than re-deriving it, checks it
  hourly, and offers a rebuild — and the phrase it offers parses to a real
  `Intent::RebuildIndex`, so saying yes does something. `atlas index` is the
  same thing on demand. Section 13 of the handover has the reasoning.
  What is still **not** built: per-folder index notes, the bullet index at the
  top of each daily note, and anything that actually splits an index past
  `MAX_LINES` (it only says it should be split).
- **~~The morning brief has no inputs.~~ Redesigned.** Mail is one `Source`
  among seven now; the brief reads handoffs at the door, requests Atlas could
  not finish, scheduled work that failed or is due, proposed times nobody
  answered, posts waiting on a yes, and its own upkeep — all offline.
  `BriefConfig` is on `ToolsConfig` for the first time (it had no switch at
  all), ships on, and `daypart_with_brief` is wired. Section 15 of the
  handover. Still open underneath it:
  - **`mind::speak_brief` is a second brief.** `Intent::Ready` builds its own
    list from the publisher and backlog. It overlaps what `brief.rs` now does.
    Merging them means deciding whether the waking line and the morning run
    are one thing or two — **a design call, left for Eric.**
  - **`money::summarise(&[])` and `messaging::spoken(&[], ...)`** still have
    the empty-input shape. Same fix; same question underneath about where the
    data comes from without a third party.
  - A mail reader would now unlock exactly three named functions
    (`brief::from_mail`, `vet_draft`, `as_chain`) rather than the module.
- **The endpointing threshold wants one real session.** `silence_below_db` is
  −38 in the shipped config. A single full-scale click measures −36 by RMS, so
  it clears the threshold; the cost is bounded (a click can only reset the
  silence timer, never end a turn) but whether −38 is right against a real
  desk with a real microphone is not answerable from here.

## 1b. The dead-capability list — nine functions nothing calls

Enumerated by name in `tests/dead_capabilities.rs`, each with what it is
waiting on. Two need a ruling before they can be wired; the rest need an
integration decision that is real work rather than a question:

- **~~`selfwork::run_tests` needs a ruling~~ — the ruling was already given,
  and I had it wrong.** The boundary is **fix vs improvement**, not "may it
  run tests": a fault is fixed in the sandbox and lands if it passes and does
  not change or limit what Atlas can do; a gap becomes a build plan put to
  Eric. Section 23. The landing gate is built. What is still needed:
  - **Nothing fills the stages.** Thought needs a theory and a proving test,
    Build needs a change written into the sandbox. Both want a model, and
    there is no llama-server here.
  - **`pending_landing` is never populated** — the step that runs the tests in
    the sandbox and turns a pass into planned changes.
  - **Path B is not built**: no build plan is scoped, no gaps looked for, and
    `Step::Propose` has no producer.
  - **`self_work.enabled` and `pipeline.enabled` both ship `false`**, so the
    gate has never refused a real change.
- **`selfwork::run_tests`** — Atlas running its own test suite in the
  sandbox. Deliberately unwired: it wants a ruling before it has a caller.
- **`vault::seal_bytes` / `unseal_bytes`** — Windows DPAPI as a second layer
  underneath the cross-platform AEAD path that `put` already uses. Whether
  defence-in-depth on Windows is wanted is an open decision, and it cannot be
  tested anywhere but real Windows hardware.
The nine, measured rather than counted by hand — earlier revisions of this
file said "nine" while the list held eleven, because the prose was counting
bullet groups and not names:

- `brief::as_chain`, `brief::from_mail`, `brief::vet_draft` — all three want a
  **mail reader**, and only that. `from_mail` builds items from raw messages,
  `vet_draft` critiques a reply before you see it, `as_chain` makes the run
  resumable. The brief itself is no longer blocked on mail; these three still
  are, and they are the honest measure of what an inbox would unlock.
- `daily::still_keep`, `why::account`, `retention::irreducible` — each needs a
  caller that does not exist yet, described per entry in that file.
- `selfwork::run_tests`, `vault::seal_bytes`, `vault::unseal_bytes` — the two
  rulings named above.

(`nudge::what_i_know_of` and `nudge::trace_line` both came off this list this
session: the first is reached from `atlas index` and from being asked what
Atlas has written down, the second from `atlas trace` and `Intent::ModelTrace`.)

The other three groups (roughly 147 reached only by tests, 122 module-internal
helpers with tests, 29 without) are counted with ceilings rather than
enumerated. The 147 is the real backlog: built, proven, never called.

## 2. Real work, no ruling needed — just the next wiring pass

- **~~`revise.rs` is the next honest piece.~~ Wired.** Section 18. The
  blocker under it turned out to be that `context()` carried *nothing learned*
  — so there was nowhere a lesson could live where it would be read again.
  `revise::standing` is that place. `trace::blame`/`grade_last` got their
  callers. Still open underneath it:
  - **`Edit::replacing` is always `None`** — every lesson is an addition,
    nothing supersedes an earlier line by naming it. `proposal` already has
    the wording for the replacing case.
  - **`stale_notes` still has no caller** — corrections that never earned an
    edit and have gone quiet. `upkeep_questions` in the brief is the home.
  - **All three `Home` variants resolve to one list.** Splitting them
    (instructions into the prompt, preferences into `person`, dated facts into
    `facts.rs`) needs `facts` read into context too.
  - **No model has ever read a standing instruction.** Whether a local 7B
    obeys a line in its context is unmeasured; `repeat_rate` will say.
- **~~`council.rs` / `nudge::convene` needs a ruling.~~ Ruled and wired.**
  Eric's call: the room runs on **hardware specifics**, where Atlas has
  measured numbers, with "ask the room" about anything as the fallback.
  `Intent::AskTheRoom`, `council::hardware_room`, `council::parse_opinion`.
  Section 17. Still open underneath it:
  - **`Round::Open` is not run** — a second five model calls. Worth it only if
    the blind round turns out to be too thin in practice, which needs real use.
  - **No real model has convened a room.** Five sequential calls on a local 7B
    is a long silence and the number is unknown; `atlas trace` will show it.
  - **`parse_opinion` has never read a real model's prose.** Built for what
    local models actually do rather than for JSON, but that is a prediction.

- The remaining ~30 `UNWIRED_BASELINE` modules not covered above: `mend`,
  `look`, `adapt`, `afterme`, `backends`, `cloudsync`, `dictate`, `edit`,
  `editors`, `grade`, `hearing`, `language`, `plainly`, `publishing`,
  `reach`, `remote`, `voiceover`, `walkthrough`, `workingset`, and others.
  See `docs/MODULE_REFERENCE_2026-09-13.md`'s quick index for the current
  list (search for `**no**`). Each is a real, tested, standalone
  capability; each needs its own integration decision.
- **`returning::welcome`'s remaining rough edge.** With an address set, the
  greeting carries "you have just come back"; without one, `daemon.rs`
  prefixes "While you were away:". That is two ways of saying the same
  thing, decided at the call site. It is correct and tested, but the
  phrasing layer would be a better home for it if `returning.rs` ever grows
  a notion of "this is a return" of its own.
- **Regenerate `MODULE_REFERENCE` after any wiring round.** Done for
  14 Sep; the generator is kept at `docs/genref.py`, so it is a re-run.

## 3. Never yet run for real — the standing "first live run" list

- **No real model has ever been recorded.** The flight recorder is wired at
  both call sites and tested against fake models. There is no ollama in this
  container, so `typical_ms` has never measured a real local model. "Is the 7B
  good enough, or does this question need the 14B" is the question the module
  exists to answer, and it needs a few days of real use on Eric's machine
  before it can. `atlas trace` is how to look.
- **The notes index has never described a real note.** Every test builds its
  own folder. `data/notes` is empty in this tree and `research` — the only
  thing that writes notes today — ships off, so `says_for` has never read a
  note Eric wrote. The first real run is `atlas index rebuild` against a
  folder with something in it, and the thing to look at is whether the
  one-sentence summaries are actually worth reading.

- **`atlas share` between two genuinely separate machines** — notes and
  files both. Exercised by unit tests and by a real loopback socket, never
  between two hosts over Tailscale. First real run should be exactly that,
  with a friend's instance on their own hardware. Worth sending a file of a
  few megabytes specifically: the 120-second timeout and the per-endpoint
  body cap have only ever been exercised over loopback, where nothing is
  slow.
- **Device pairing** (`atlas household pair/join`) — same caution, carried
  over unchanged from the previous list.
- **`atlas reclaim`** against a real filesystem — carried over.
- **The streaming listen loop has never read a real microphone.** The level
  meter, the wav header and the ffmpeg invocation are all tested; the loop
  around them has only ever run against synthetic PCM, because this container
  has no sound hardware. First real run is a voice turn that ends when you
  stop talking rather than after eight seconds.
- **`hearing`'s calibration has never measured a real device.** The decision
  logic, the persistence and the parsing of ffmpeg's `volumedetect` output are
  tested against fixture text; no microphone has been recorded from.
- **`atlas update` on a real install.** The preserve-everything guarantee is
  asserted against a constructed install in `tests/updating_without_reinstalling.rs`,
  including a real overwrite of the shipped files. It has never been run
  against an install someone actually uses.

## 4. Blocked on real hardware, unverifiable in this sandbox

Unchanged from the previous list:

- Voice-lock is unproven on real audio. The policy layer is fully tested;
  no speaker encoder has ever processed a real recording.
- `identity.rs`'s `Hello` status is always reported `Unavailable`. No
  Windows Hello detection exists anywhere in this tree.
- **Device names have never been read off real hardware.** This entry used
  to say microphone enumeration was unimplemented and that no OS command
  called `audio::parse_devices`. That was wrong in a way worth recording:
  a command did call it, hardcoded to Windows dshow, so detection had never
  run on Linux or macOS and fell back to a Windows device name that cannot
  work there. Enumeration is now per-platform (`-sources alsa`,
  `-f avfoundation`, dshow) with each parser tested against that platform's
  real output format, and `firstrun` asks for real. What remains blocked:
  this container has no sound devices, so no name has ever made the round
  trip from hardware to ffmpeg and back. Names must be byte-exact or
  recording fails silently, so this wants one run on a real Windows machine
  and one on a real Linux one.
- macOS is unverified. Windows and Linux cross-compiles are checked
  routinely; there has never been a macOS toolchain here.
- The Windows DPAPI credential path compiles but has never run on real
  Windows hardware.

## 5. Standing cautions

- **~~The `DEAD_CAPABILITY_CEILING` total~~ — removed**, at Eric's call, along
  with `bug_sweep.rs`'s duplicate copy of the deadness rule and the
  `helper_tested` bucket. Section 16. What ratchets now: the named `ORPHANS`
  list, and **exact** counts for `test_only` (143) and `helper_untested` (33).
- A guard must not be able to satisfy itself from its own source. Three
  instances this session; see 16.
- The removed ratchet will keep moving in
  both directions as modules get wired. Expected, not a regression, as long
  as every move is written down with a real reason and confirmed by
  measuring rather than estimating — `tests/bug_sweep.rs`'s comment history
  models this.
- **`cargo clean` is not free late in a session.** After a clean, rebuilding
  all 214 test binaries at full parallelism was **OOM-killed silently** —
  no error in the log, no `rustc` left running, just a truncated run. Twice.
  What worked: `cargo build --tests -j 2` first, then `cargo test -j 2`, and
  when even that got killed part-way, running the remaining binaries in two
  explicit `--test` batches and de-duplicating the results.
  - The tell is the same shape as the disk one: **a run that stops with no
    failure and no error**. Check `ps aux | grep rustc` — zero processes and a
    partial log means killed, not finished.
- **The sandbox-disk caution is reinstated, with the actual cause.** A full
  run late in a long session produced **21 failures across unrelated modules**
  — backups, gestures, notes, config, the index, the correction loop. Every
  one of them wrote a file. Nothing was wrong with any of them: the container
  had 8GB left, `target/` had grown to **22GB** across a dozen incremental
  full-suite runs, and writes were failing.
  - The signature to recognise: **broad failures in modules you did not touch,
    all of them file-writing.** A logic bug is narrow and thematic; this is
    wide and mechanical.
  - The fix is `cargo clean -p atlas` (22GB → 1.2GB) and clearing `/tmp/atlas-*`
    test dirs. Both are safe mid-session.
  - Do this *before* concluding anything from a failing full run late in a
    session, and re-run one failing binary alone first — it passes, which is
    the tell.
- **The earlier "sandbox-disk caution is withdrawn" note was right about the
  wrong thing.** The full suite links and runs
  here: 213 binaries, 4,255 tests. Everything should be verified against
  `cargo test --no-fail-fast` from now on, not against an island slice.
  Note that plain `cargo test` stops at the first failing *binary*, which
  is how a tree with 18 failures reported only 1 on the first run.
- `config/tools.yaml` is now guarded against silently drifting off by
  `tests/shipped_config.rs`. If a section genuinely should ship off against
  a default of on, name it in `DELIBERATELY_OFF` with the reason rather
  than editing the assertion. Same for a value that ships different from
  its default: `DELIBERATELY_DIFFERENT` in `tests/hub_settings.rs`.
- **The backlog of built-but-never-called capabilities is now a named list**,
  not a count: `tests/new_capabilities_are_wired.rs`. A new one fails the
  build by name; one that gets wired and is left on the list also fails. The
  list may grow — it may not grow silently.
- **A path that looks per-install and is not.** Three of these in one session:
  `data/index.md` written to the repo root by every test that built a
  `Daemon`; the model-call log written to a fixed `data/logs` and then to the
  store's *parent*, both shared across the whole suite; and `atlas trace`
  reading a different directory from the one the daemon writes. Rule:
  anything a `Daemon` writes derives its path from `store.root()`, and any CLI
  command that reads it derives the same way. Two of the three failed no test
  at all until one was written for them.
- **A guard can pass on the prose that explains it.** `tests/guards.rs`'s
  entry for the notes-index command phrase matched the comment above the line
  rather than the line, so deleting the phrase left the guard green. The
  needle now carries the trailing comma that only the real line has. This is
  the same failure `bug_sweep.rs` fixed the day before, in a different file:
  when writing a guard whose needle is a phrase you also explain in prose,
  anchor it on syntax the prose cannot contain.
- **The dead-capability detector has been corrected four times in one day**
  (non-recursive walk, substring matching, stem-keyed modules, and
  call-versus-reference). Treat any number it produces as provisional until
  the rule has been read. Every correction so far has moved the count in both
  directions at once.
- **Defaults live in one place now** — each config struct's own `Default`.
  `settings.rs`'s registry derives from it and has no way to declare one.
  If you find yourself adding a default beside a live value in that file,
  that is the bug this session removed.
