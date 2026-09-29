# Speed, free conversation, and two deep scans (27–28 Sep 2026)

Eric, after round two of testing day: "Atlas felt slow when clicking around and trying to talk to it, and that is not acceptable." He also wanted to talk freely rather than in set phrases, the still-open list closed, GitHub (open source, over 1k stars) searched for improvements, and later: "everything you found needs to be fixed", and "on Windows I don't want the hub or the application open for Atlas to run."

This work is on branch `merge-0928`, pushed to GitHub as `session-0928`.

## How it was done

- **Six read-only scans:**
  - why it's slow
  - why conversation feels scripted
  - refactors
  - dead ends and defects
  - the still-open list
  - GitHub research
- **Five implementation agents in their own worktrees:**
  - speed
  - talk
  - hubfix
  - pages
  - refactor
- **Merged, then a second pair of scans:**
  - the core
  - apps, setup and updates
- **A second GitHub round.**
- **Four more agents:**
  - core2
  - apps2
  - mcp2
  - bg (Atlas running in the background on Windows)

The build machine has 2 CPUs and 8 GB of memory. A compile of Atlas takes about 25 minutes, which is why this took most of a day. From now on, builds are pushed as soon as they're merged.

## Why it was slow (root causes, all fixed)

1. **The hub answered one request per trip round the main loop.**
   - A trip included a nap of up to 2 s.
   - A click is 4 or more requests.
   - Fix: the hub now runs on its own threads, one per connection. The daemon answers everything waiting in 20–50 ms slices.
   - Measured here: 8 requests at once took 3.5–4 s before and 21–33 ms after.
2. **The wake word was listened for while it was off.**
   - Every trip recorded 3 s of audio and started whisper from scratch.
   - That made every hub request take 6–8 s.
   - Fix: when the wake word is off, it is truly off, and Atlas starts in push-to-talk.
3. **The model always ran on the processor.**
   - Graphics memory was hard-coded to 0, even though setup downloads the Vulkan build.
   - Fix: `models.gpu_layers: auto` puts all layers on the graphics card when the GPU build is present.
   - The model used for talking is capped at about 5B (`talk_ceiling_b`).
4. **The prompt changed near its start on every turn**, so llama.cpp couldn't reuse it.
   - Fix: the prompt is ordered from stable parts to changing parts.
   - Fix: llama-server runs with `--jinja -np 2 --cache-reuse 256`, and conversation is pinned to slot 0.
5. **Spoken replies recorded 1 s and ran whisper before every sentence.**
   - Fix: nothing is recorded between sentences. You cut in by holding the talk key.
6. **Disk work on every tick and turn** (settings files, add-ons, the file index, saves).
   - Fix: these are now checked by modification time and cached, and the index walk runs on its own thread.
7. **Slow buttons froze Atlas** (sending to friends, adding a friend, the phone code).
   - Fix: they now run on the crew, and the page shows "Sending…" and then the result.
8. **Timing:** hub requests over 150 ms, slow ticks, and per-turn stage times are now logged (numbers only, never words).

## Free conversation (fixed)

- **Real chat turns.** The model now receives the conversation as separate turns over `/v1/chat/completions`, not pasted text. It keeps the last 6 exchanges within about 1,200 tokens, plus a folded summary.
- **Every command is a tool.**
  - All 149 entries in commands.yaml have a description, and the model can call any of them.
  - Each turn offers the model about 12 core tools plus up to 6 retrieved by BM25.
  - `brief_on` works again.
  - Consequential tools the model chooses ask first.
- **Greedy phrases fixed.** "how's it going", "wait, what do you mean", "what should I eat", "go to bed" and similar now reach the model, while "pause" and "open chrome" still work.
- **Notes become hints.** Your notes and facts are hints in the prompt, not canned read-backs. Your name, stored facts, the next calendar events and today's reminders are also in the prompt.
- **Streaming.** Replies stream in: they're spoken sentence by sentence, and the Talk page fills in as they arrive.
- **Replies aren't lost or chopped.** No Talk reply vanishes. A parked question expires after 10 minutes, and a sentence that isn't yes or no runs as a new request. "Write me a poem" isn't cut to 2 sentences.
- **Look it up.** When Atlas doesn't know, it offers to look it up.
- **Read results come back as sentences.** Agenda, find-file, machine health and similar are reworded by the model, and it's asked to call a tool rather than announce one ("I'll check your calendar").
- **Tool calls hold up.**
  - A tool call split across stream pieces never leaks into speech.
  - A missing argument gets a question.
  - The prompt is fitted to the context.
  - A partial stream is never spoken twice.

**Tested live** against the real Qwen3-VL-4B on this 2-CPU box with no graphics card:
- It held a follow-up conversation and opened Chrome through a tool.
- The first reply took about 48 s to start; a follow-up took 10–15 s.
- The laptop's Arc graphics should be much faster. **That hasn't been measured.**

## The still-open list (closed)

- **Buttons say what happened.** Every hub button now says what it did. A form with a missing field goes back and says what's missing.
- **Slow work runs in the background.** Sending to friends, add-on sharing, adding a friend from the hub and the phone code all run on the crew.
- **New pages for the vault and handover.** On Accounts: vault passphrase set/change/recovery key, and "Take it back". A banner shows while the machine is handed over.
- **Sync.** Choose a folder, start or join a household, and use a key from another device. A fresh install never needs the terminal.
- **Free up space.** On Status, with a survey on the crew; Atlas only moves the files you tick.
- **Security holes closed:**
  - Passphrase forms are refused except over this machine or Tailscale.
  - Two handover holes: a release key made while handed over, and the household key handed out while handed over.

## Scan 1 defects (fixed)

- **Phone calendar sync failed silently.** The 16 KB limit is now 2 MB.
- **Oversized forms** now get a page with a way back.
- **Settings-only mode** now actually saves.
- **"Make a new key"** now asks first.
- **First-run links** go to the right pages.
- **Improvements** act on the idea, not its place in the list.
- **External links** open in your browser.
- **`reachable_from`** also binds loopback.
- **Poisoned locks** no longer kill routes.
- **Accented names** no longer crash.
- **Debug text** is no longer shown to you.
- **Non-ASCII in notices** is no longer garbled (UTF-8 URL encoding).

## Scan 2 defects (fixed)

**Core:**
- A voice turn no longer takes over a Talk turn; nothing runs twice.
- A click answered "busy" no longer runs later.
- Pause and stop now stop a model turn in flight.
- A connection sending one byte at a time can no longer hold the hub. Each read is bounded, and one address is capped at 16 connections.
- Chat isn't switched off by a 503 or a context overflow.
- Scheduled posts, the agenda, scheduling and bookings now use local time, including across daylight-saving changes.
- A briefly locked file is retried before it's set aside, and you're told if it was.
- The model-server check runs off the loop.
- Hub pages poll `/hub/changed.json` instead of reloading every 3 s.

**Updates, setup and phones:**
- **Updates never installed** (critical).
  - Cause: every build called itself 0.1.0, so a courier update was deleted as "already running" and then recorded as installed.
  - Fix: builds are now identified by their SHA-256, and CI stamps `0.1.<run number>`.
- **Setup:**
  - Running Setup again over a running Atlas now stops it and replaces it.
  - It never downgrades silently.
  - Errors show in a message box.
- **Downloads:**
  - A stalled download is retried and resumed.
  - Setup checks disk space first.
  - A half-unpacked piece is fetched again.
- **Phones:**
  - Start no longer blocks the phone's main thread or starts Atlas twice.
  - iOS: shares aren't lost, the hub restarts after the app was in the background, and the Live Activity goes stale.
  - Android: notification permission is asked for, polling doesn't pile up, and the hub address is cleared when the service stops.
- **Tor** exits with its Atlas.
- **"Send an update"** skips files that aren't builds and finds OneDrive's Downloads folder.
- **Android** builds with a pinned Gradle wrapper.
- **CI** caches work, and artifacts are named by version.

## Atlas runs with nothing open on Windows (Eric's request, 28 Sep)

- After setup, Atlas turns on start-at-sign-in and starts itself in the background. Closing the window never stops it.
- The sign-in task is now built from Task Scheduler XML. The old task had Windows' default **72-hour limit, which would have killed Atlas after 3 days**. The new one has no time limit, runs on battery, allows one copy, and runs as you, not as administrator. If Task Scheduler refuses, the per-user Run entry is used instead.
- **An icon by the clock**, with Open Atlas, Open the hub in my browser, Pause/Resume Atlas, and Quit Atlas. Double-clicking it opens Atlas. It comes back if Explorer restarts. The setting is `desktop.tray_icon`.
- **No console anywhere:** taskkill, powershell and clip now start without a window. Only `atlas.exe` ships.

## From GitHub (over 1k stars, verified 27–28 Sep)

**Applied:**
- **llama.cpp** (129k★): `--jinja` tool calling, `--cache-reuse`, `-np 2`, the Vulkan build on the Arc graphics, and optional speculative decoding. Speculative decoding uses `models.draft` with a verified Qwen3-0.6B draft, or `models.speculate` for the draft-free modes. Both are off by default.
- **pipecat / RealtimeTTS** (16k★ / 4k★): sentence-by-sentence streaming to speech.
- **mem0 / Letta** (66k★ / 25k★): what you've told Atlas is always in the prompt, and notes are retrieved as hints.
- **MCP**, from the rust-sdk (4k★) and servers (91k★):
  - Atlas is now an MCP client, with its own small stdio client (rmcp needs tokio).
  - At most 3 server tools per turn, and every call asks first.
  - Results are treated as data, and tools are never offered while handed over.
  - Examples in tools.yaml, commented out: Filesystem, Time, Fetch, playwright-mcp (38k★), and terminator (1.6k★, Windows UI automation).
- **mozilla/readability** (11k★): hidden elements are skipped, and pages come out as Markdown.
- **SearXNG** (38k★): an optional `research.searxng_url`.
- **himalaya** (6.7k★): an optional mail backend that decodes encoded subjects and bodies.
- **windows-rs** (12.7k★): `Windows.Media.Ocr` as the screen-reading fallback when the picture reader is missing.

**Next** (researched, not yet built):
- Silero VAD and smart-turn through `ort`, for cutting in by voice and knowing when you've finished.
- The official `sherpa-onnx` crate, for Kokoro speech and "who said what" in call notes.
- The wake word on its own thread.

**Avoid:**
- piper1-gpl (GPL)
- khoj and 01 (AGPL)
- ipex-llm (archived, with security warnings)
- screenpipe (non-commercial licence)
- SYCL on Arc (about 3× slower than Vulkan)

## Tests

- **Round 1, merged:** 35 targets, 7,145 passed.
- **Round 2, merged:** 7,216 passed, with 8 bookkeeping failures from merging.
  - Seven were fixed on the merge: counts, lists, a source check that followed the threaded server, and the unused `sync_page` folded into the tests.
  - The eighth was a real regression: the model server wasn't restarted after being let go. It was fixed (`model_start_tried` is now cleared on a successful start).
- **Windows cross-check:** passes on each branch.

## Not verified, or still open

- **Nothing here has run on Windows:** the tray icon, the scheduled-task XML, Windows OCR, the Arc graphics speed, or a real MCP server.
- **The phone and CI changes haven't been built yet.** The first `ios.yml` / `android.yml` / `windows.yml` run is their test. The Windows job now fails if the version block isn't `0.1.<run>`.
- **Cutting in by voice** only works with the talk key held. Voice detection during playback is the next step (Silero).
- **The wake word, when on,** still blocks the loop for each 3 s recording.
- **Splitting `daemon.rs`:** the plan is in `docs/refactor-plan-daemon-split.md`, and the tests are ready for it. It's not done yet.
- **"Pause"** from the icon pauses jobs and offers, like the hub's Pause. It doesn't mute the microphone.

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
