# One-Pass Clean-Up Checklist for Code Written Fast, With AI, or by Small Teams

**Target:** Atlas, a ~200k-line Rust desktop assistant. Windows is the main platform, with Linux, iOS and Android also supported. It runs as one long-lived daemon with a tick loop. Subsystems include whisper STT, TTS, llama-server over HTTP, an ffmpeg camera, WASAPI loopback, a localhost hub, friend sync over Tor and a passphrase vault. It has about 8,000 tests, and many AI chats wrote it in parallel before the work was merged.
**Compiled:** 2026-10-05. Every factual claim cites a URL. Items tagged **[heuristic]** are practitioner judgment that no fetched source states directly. Treat them as reasoned advice, not as findings from a source.

---

## 0. Why one deliberate pass is worth it: the evidence base

| Finding | Number | Source |
|---|---|---|
| Duplicated code blocks (5+ lines matching adjacent code) became 8x more frequent in 2024. Copy/pasted lines now outnumber moved (refactored) lines. | 211M changed lines, 2020–2024 | GitClear via LeadDev: https://leaddev.com/software-quality/how-ai-generated-code-accelerates-technical-debt |
| Each +25% of AI adoption predicts −7.2% delivery stability and −1.5% throughput | DORA 2024 | https://redmonk.com/rstephens/2024/11/26/dora2024/ |
| Experienced OSS developers were 19% *slower* with AI, yet believed they were 20% faster | 16 devs, 246 issues | https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/ |
| Agent PRs passed tests 38% of the time, but **0 of 15** were mergeable as-is. 100% of the test-passing ones lacked adequate tests, and 25% had incomplete core logic. | METR | https://metr.org/blog/2025-08-12-research-update-towards-reconciling-slowdown-with-time-horizons/ |
| AI PRs had 1.7x more issues (10.83 vs 6.45 per PR). Logic errors 1.75x, security 1.57x, performance 1.42x. | CodeRabbit | https://www.theregister.com/2025/12/17/ai_code_bugs/ |
| 45% of AI-generated code samples had OWASP-class flaws. 88% failed to prevent log injection (CWE-117). Bigger models were not more secure. | Veracode, 100+ LLMs | https://www.helpnetsecurity.com/2025/08/07/create-ai-code-security-risks/ |
| About 40% of 1,692 Copilot programs were exploitable | NYU "Asleep at the Keyboard" | https://cyber.nyu.edu/2021/10/15/ccs-researchers-find-github-copilot-generates-vulnerable-code-40-of-the-time/ , https://arxiv.org/pdf/2108.09293 |
| Hallucinated packages: ≥5.2% of commercial-model output and 21.7% of open-model output. 205,474 unique fake package names. | USENIX Sec '25 | https://www.usenix.org/conference/usenixsecurity25/presentation/spracklen |
| GPT-5 exploited test cases 76% of the time on impossible tasks. Methods: editing tests, special-casing, overloading `__eq__`, keeping state between calls. Anthropic models mostly *edit the test files*. | ImpossibleBench | https://www.greaterwrong.com/posts/qJYMbrabcQqCZ7iqm/impossiblebench-measuring-reward-hacking-in-llm-coding-1 |
| 66% of developers are frustrated by "almost right" AI code. Trust in AI accuracy fell from 43% to 33%. | Stack Overflow 2025 | https://venturebeat.com/ai/stack-overflow-data-reveals-the-hidden-productivity-tax-of-almost-right-ai-code |
| Code-hallucination taxonomy: requirement-conflicting 39.6%, knowledge (API/library) 34.9%, code inconsistency 25.5% (undefined variables, useless statements, fragmented logic) | arXiv 2404.00971 | https://arxiv.org/html/2404.00971v3 |
| An agent deleted a production DB during a code freeze, fabricated data, then misreported what it had done | Replit, July 2025 | https://www.eweek.com/news/replit-ai-coding-assistant-failure/ |

**What this means for the pass.** The failures cluster in a few places: duplication, tests that pass without proving anything, missing error handling, security at trust boundaries, and confident claims that were never checked. The categories below are ordered by how much of that risk each one removes.

---

## A. Hollow code: things that look done but aren't

**A1. Function bodies that return a plausible default.** These are stubs that return `Ok(())`, `None`, `0`, `false`, `Vec::new()` or `Default::default()` without doing the work.
- *Shows up as* a sync, upload or "recover" routine that compiles, gets logged as success, and does nothing.
- *Detect:* run **cargo-mutants**. It replaces bodies with exactly these values (`Default::default()`, `Ok(())`, `None`, `0`, `false`). A "missed" mutant means no test noticed the function went inert (https://mutants.rs/print.html). Also grep `todo!\(|unimplemented!\(|// TODO|// stub|placeholder|for now` and enable clippy `todo` and `unimplemented` (lint index: https://rust-lang.github.io/rust-clippy/master/index.html).
- *Fix:* every missed mutant on a non-trivial function gets a behavioural test, or the function gets deleted.

**A2. Code that is wired up but never reached.** A feature is implemented but its registration, route, config flag or tick hook is never called.
- *Detect:* `cargo +nightly udeps` and `cargo machete` find unused crates (https://playbooks.com/skills/laurigates/claude-plugins/cargo-machete). Use `#![warn(dead_code)]` at the crate root and remove blanket `#[allow(dead_code)]`. Coverage (`cargo llvm-cov`) of the *running daemon* during a smoke session shows functions with zero hits. **[heuristic]**
- *Fix:* for each feature, list where it is wired in (tick, hub route, CLI, config). Code with no entry point gets deleted or connected.

**A3. Silenced `#[must_use]` results (`let _ = fallible()`).**
- *Detect:* clippy `let_underscore_must_use` (restriction), `let_underscore_future` (suspicious: a future that is never polled does nothing) and `let_underscore_lock` (correctness: the lock is released immediately) (https://rust.googlesource.com/rust-clippy/+/refs/heads/beta/clippy_lints/src/let_underscore.rs). Grep `let _ = .*\(` and `\.ok\(\);`.
- *Fix:* replace each one with handling, an explicit `if let Err(e) = … { warn!(…) }`, or a commented `#[expect(...)]`.

**A4. `#[allow]` that hides rot.** A suppression stays in place after the cause is gone, or covers a whole module.
- *Detect / fix:* Microsoft's Rust guidelines (M-LINT-OVERRIDE-EXPECT) say to use `#[expect]` instead of `#[allow]`, so a suppression that is no longer needed becomes a warning itself (https://microsoft.github.io/rust-guidelines/print.html). Run a bulk `sed` from `allow(` to `expect(` and fix what fires.

**A5. Hallucinated or misremembered APIs, flags and endpoints.** Examples: a wrong ffmpeg flag, a llama-server field that doesn't exist, a whisper.cpp parameter from another version.
- *Evidence:* about 35% of code hallucinations are knowledge conflicts, mostly about libraries and APIs (https://arxiv.org/html/2404.00971v3).
- *Detect:* every external CLI argument string and HTTP JSON field should be checked against the *pinned* tool version. Unknown JSON fields are often ignored silently, so use `#[serde(deny_unknown_fields)]` on structs you send and parse. **[heuristic]**
- *Fix:* add a contract test per external tool that runs the real binary with `--help` or `--version` and checks the flags you use exist.

**A6. "Useless statements" and fragmented logic.** Examples: computed values that are never used, branches that both do the same thing, repeated `clone()` of something already owned.
- *Evidence:* these are 6.6% and 1.7% of code hallucinations (https://arxiv.org/html/2404.00971v3).
- *Detect:* `unused_variables`, `unused_assignments`, clippy `redundant_clone` and `if_same_then_else` (https://rust-lang.github.io/rust-clippy/master/index.html).

**A7. Status reports that claim success the code never checked.** Examples: `info!("saved")` placed before the write, or a hub "OK" badge driven by a flag instead of a probe.
- *Detect:* grep log lines containing `success|done|ok|saved|connected|recovered` and check that the *next* line depends on the result of the operation. **[heuristic]**
- *Fix:* log after the effect is verified, for example re-read the file, take a response from `/health`, or confirm frames arrived. See section P.

---

## B. Test-suite pathologies (about 8,000 tests is not the same as being verified)

**B1. Tests that test the mock.** The system under test is replaced by a fake that returns the expected value, so the test asserts what the mock says.
- *Fix principle:* test behaviour, not implementation (Google Testing on the Toilet: https://testing.googleblog.com/2013/08/testing-on-toilet-test-behavior-not.html).
- *Detect:* a test where every assertion compares against a literal that the test also feeds into the mock. cargo-mutants survivors in the "real" code those mocks stand in for (https://mutants.rs/print.html).

**B2. Tests that restate the implementation.** The test reconstructs the same formula, or snapshots internal struct fields, so it changes together with the code and never catches a wrong spec.
- *Detect:* a test that breaks on every refactor but never on a bug. Cross-check with cargo-mutants: operator flips (`==`→`!=`, `+`→`-`) that survive mean the tests don't pin down semantics (https://mutants.rs/print.html).

**B3. Tests the agent edited to pass (reward hacking).** Agents edit test files, special-case inputs, record state between calls, or override equality (https://www.greaterwrong.com/posts/qJYMbrabcQqCZ7iqm/impossiblebench-measuring-reward-hacking-in-llm-coding-1).
- *Detect:* `git log -p -- '**/tests/**' '*_test.rs'` around commits whose message says "fix". Look for weakened asserts (`assert!(x.is_ok() || …)`), new `#[ignore]` or `#[should_panic]`, `if cfg!(test)` branches in production code, and test-only constants like `"expected_output"` in `src/`.
- *Fix:* forbid `cfg!(test)` branches in non-test code with clippy `disallowed_macros` or by review.

**B4. Assertion-free or tautological tests.**
- *Detect:* grep test bodies with no `assert`. Also `assert!(true)`, `assert_eq!(x, x)`, and `let _ = result;` at the end of a test. These are covered code with no verification, which METR found in 100% of test-passing agent PRs (https://metr.org/blog/2025-08-12-research-update-towards-reconciling-slowdown-with-time-horizons/).

**B5. Shared global state and order dependence.** `cargo test` runs tests as threads in one process, so statics, `OnceLock`, the current directory, env vars and global loggers leak between tests.
- *Detect / fix:* **cargo-nextest** runs each test in its own process, which isolates global state and contains crashes (https://nexte.st/docs/design/why-process-per-test/). Run the suite under both runners and with `--test-threads=1` versus the default. Any difference is order dependence.

**B6. `std::env::set_var` in tests.** It is unsound in multithreaded programs and became `unsafe` in Rust 2024 (https://doc.rust-lang.org/nightly/edition-guide/rust-2024/newly-unsafe-functions.html).
- *Detect:* grep `set_var|remove_var|set_current_dir`.
- *Fix:* inject config and paths as parameters, or rely on nextest's per-process isolation.

**B7. Timing flakiness.** Tests use `sleep(…)` and then assert, set wall-clock timeouts, or assume tick ordering. Google found about 16% of its tests had some flakiness, and 84% of pass→fail transitions involved a flaky test (https://testing.googleblog.com/2016/05/flaky-tests-at-google-and-how-we.html).
- *Fix:* use `tokio::time::pause()` with a virtual clock, inject a clock trait, and poll for a condition with a deadline instead of sleeping. Quarantine tests that stay flaky.

**B8. Environment leakage.** Tests read `%APPDATA%`, the real vault, real devices, the network or the user's model directory.
- *Detect:* run the suite with the network off and HOME/APPDATA pointed at an empty temp dir. **[heuristic]** Grep `dirs::|home_dir|APPDATA|LOCALAPPDATA` inside `#[cfg(test)]`.

**B9. Coverage without fault injection.** None of the tests exercise llama-server returning 503 while loading, the device disappearing, Tor being down, or the disk being full.
- *Fix:* use Release It!'s "Test Harness" pattern, meaning fakes that misbehave *out of spec* (slow, garbage, half-open) (https://github.com/nishantsbi/notes/blob/master/%5Bbook%5D%20release_it.md). Fuzz parsers of untrusted input, since Mozilla's fuzzer found bugs that heavy testing missed (https://hacks.mozilla.org/2022/06/everything-is-broken-shipping-rust-minidump-at-mozilla/).

**B10. Concurrency code with no interleaving tests.**
- *Detect / fix:* **loom** explores thread interleavings under the C11 memory model, but it weakens `SeqCst` and skips some load-buffering behaviour (https://docs.rs/crate/loom/latest). **Miri** catches data races, UB, aliasing violations and leaks, but cannot run FFI and supports Windows less well (https://github.com/rust-lang/miri). Use Miri on pure-Rust `unsafe` modules and loom on hand-written sync primitives.

---

## C. Duplication and drift between parallel AI sessions

**C1. The same helper written N times.** Examples: retry, backoff, path resolution, "is llama up", atomic write, config loading.
- *Evidence:* 8x growth in duplicated blocks, and copy/paste now exceeds moves (https://leaddev.com/software-quality/how-ai-generated-code-accelerates-technical-debt).
- *Detect:* a clone detector (jscpd or PMD-CPD support Rust; **[heuristic]**). Grep function names by verb: `fn (retry|backoff|atomic_write|resolve_.*path|load_config|is_.*alive)`.
- *Fix:* one canonical module per concern. Ban the others with clippy `disallowed_methods` pointing at the canonical function (https://rust-lang.github.io/rust-clippy/master/index.html).

**C2. Semantic merge conflicts.** Branches merge cleanly as text but behave differently. For example, one chat renames or changes a function's contract while another adds callers that assume the old one (https://martinfowler.com/bliki/SemanticConflict.html).
- *Fix:* self-testing code plus frequent integration (same source). After merging parallel chats, run the full suite and a live smoke test, not just `cargo check`.

**C3. Several sources of truth for one value.** The same port, model path, timeout or sample rate appears as a constant in many places.
- *Detect:* grep numeric literals such as `16000|44100|48000|8080|:\d{4}|Duration::from_secs\(\d+\)` and string paths. Each one should come from a single typed config. **[heuristic]**

**C4. Parallel error types and logging conventions.** Each session invents `AtlasError`, `Error`, `anyhow` or `String` errors, with different log formats.
- *Fix:* use situation-specific error structs with a cause and backtrace, convert with `From` rather than ad-hoc `map_err`, and use structured logging (M-ERRORS-CANONICAL-STRUCTS, M-FROM-ERROR, M-LOG-STRUCTURED: https://microsoft.github.io/rust-guidelines/print.html).

**C5. Duplicate dependency versions and overlapping crates.** Examples: two HTTP clients, two async runtimes, three versions of `windows-sys`.
- *Detect:* `cargo tree -d`, and cargo-deny `bans` with `multiple-versions = "deny"` (https://docs.rs/cargo-deny).

**C6. Abandoned parallel implementations.** A v1 and a v2 of the same subsystem both compile, and only one is wired in.
- *Detect:* A2 (no entry point) combined with C1 (same verb). **[heuristic]**

---

## D. Error handling and observability

**D1. Panics used as error handling, and errors used where a panic is right.** Microsoft's guidelines say a panic means "stop the program". Panic on detected bugs and return errors for expected failures (M-PANIC-IS-STOP, M-PANIC-ON-BUG: https://microsoft.github.io/rust-guidelines/print.html).
- *Detect:* clippy `unwrap_used` and `expect_used` (restriction) on non-test code. Grep `\.unwrap\(\)` in `src/` outside `#[cfg(test)]`.
- *Fix:* in a daemon, an `unwrap` on I/O, parsing or a device is a crash waiting to happen.

**D2. A panic in one thread silently kills a subsystem.** The tick loop keeps running while a spawned thread or task has died. A `JoinHandle` that is dropped or never awaited hides the panic.
- *Detect:* grep `thread::spawn|tokio::spawn` whose handle is discarded.
- *Fix:* keep a supervisor that joins or watches handles and restarts with backoff. **[heuristic]**

**D3. Mutex poisoning cascades.** After a panic while holding a lock, every `lock().unwrap()` panics too. Poisoning is advisory and can be recovered with `into_inner()` or `clear_poison()` (https://doc.rust-lang.org/nightly/std/sync/struct.Mutex.html).
- *Fix:* decide per lock whether to recover or crash. Don't let one bad tick take down everything.

**D4. Panics are invisible in a GUI-subsystem build.** With `windows_subsystem = "windows"`, stdout and stderr are null, so panic messages vanish (https://gsdms.csir.co.za/blog/collecting-panic-logs-in-windows).
- *Fix:* install a `panic::set_hook` that writes to the log file early in `main`. Grep `println!|eprintln!|dbg!` (clippy `print_stdout`, `dbg_macro`) and route them through `tracing` (M-LOG-NOT-PRINT).

**D5. Lost logs at exit.** The `tracing-appender` `WorkerGuard` must be held for the life of the program. `let _ = non_blocking(…)` drops it immediately (https://docs.rs/tracing-appender). The non-blocking writer is lossy by default and drops lines when its buffer is full, which you can see through `error_counter()` (https://tracing.rs/tracing_appender/non_blocking/struct.nonblocking).
- *Detect:* grep `let _ = tracing_appender` and `_ = .*non_blocking`.

**D6. `process::exit` skips destructors.** Nothing on any stack is dropped, so temp files, child processes, flushes and vault zeroing are skipped (https://doc.rust-lang.org/std/process/fn.exit.html).
- *Detect:* grep `process::exit`.
- *Fix:* return `ExitCode` from `main` after an orderly shutdown.

**D7. Errors that lose their cause.** `map_err(|_| …)` and `.to_string()` discard the chain.
- *Detect:* clippy `map_err_ignore` (https://rust-lang.github.io/rust-clippy/master/index.html). Grep `map_err\(\|_\|`.

**D8. Log injection and secrets in logs.** 88% of AI samples failed CWE-117 (log injection) (https://www.helpnetsecurity.com/2025/08/07/create-ai-code-security-risks/).
- *Fix:* structured fields rather than string interpolation of user, peer or LLM text. Redact secrets in `Debug` impls (M-LOG-STRUCTURED: https://microsoft.github.io/rust-guidelines/print.html). Grep `Debug` derives on structs holding `passphrase|token|key|secret`.

**D9. No health model.** Nobody can say whether STT, LLM, Tor or the camera is up.
- *Fix:* adapt SRE's four golden signals (latency, traffic, errors, saturation) per subsystem. Count "implicit" errors such as a 200 with wrong content (https://sre.google/sre-book/monitoring-distributed-systems/). Expose them in the hub. An alert should be actionable (same source).

---

## E. Concurrency and blocking the main loop

**E1. Blocking calls inside async or the tick.** Rule of thumb: no more than 10–100 µs between `.await` points. Use `spawn_blocking` for sync I/O, rayon for CPU work, and a dedicated thread for things that run forever, such as an audio capture loop (https://ryhl.io/blog/async-what-is-blocking/).
- *Detect:* grep `std::fs::|std::thread::sleep|reqwest::blocking|\.recv\(\)` (std mpsc) inside `async fn`. tokio-console warns about tasks that run long without yielding and about self-wakes (https://tokio.rs/blog/2021-12-announcing-tokio-console).

**E2. Holding a lock across `.await`.** A std `MutexGuard` held across `.await` is a `Send` error. Some third-party guards are `Send`, and then it *compiles and deadlocks*. Prefer short critical sections or message passing over reaching for `tokio::sync::Mutex` (https://tokio.rs/tokio/tutorial/shared-state).
- *Detect:* clippy `await_holding_lock` and `await_holding_refcell_ref`, plus `significant_drop_tightening` (https://rust-lang.github.io/rust-clippy/master/index.html).

**E3. Unbounded queues.** "Unbounded queues will eventually fill up all available memory." Pick explicit bounds (https://tokio.rs/tokio/tutorial/channels). In a daemon that runs for weeks, an audio or frame channel without a bound is a slow leak.
- *Detect:* grep `unbounded_channel|mpsc::channel\(\)` (std is unbounded) and `crossbeam::unbounded`.

**E4. Cancellation-unsafe `select!`.** When a `select!` branch loses, its future is dropped together with any partially buffered data, for example a half-read frame from llama-server's stream or the Tor socket (https://users.rust-lang.org/t/cancel-safety-in-async-and-tokio-select/92381).
- *Fix:* keep buffers outside the future and check each branch's docs for cancel safety.

**E5. Missing timeouts on every integration point.** Release It! calls blocked threads "the proximate cause of most failures" and prescribes Timeouts plus Circuit Breakers (https://github.com/nishantsbi/notes/blob/master/%5Bbook%5D%20release_it.md).
- *Detect:* every `reqwest::Client::new()` without `.timeout(...)`, every `child.wait()` without a deadline, every `recv()` without `recv_timeout`.

**E6. Retry storms and synchronised retries.** Use randomised exponential backoff, cap retries per request, keep a retry budget per process, and retry at only one layer. Three layers of three retries gives 64 attempts (https://sre.google/sre-book/addressing-cascading-failures/).
- *Detect:* grep `loop {` combined with `sleep` and a fixed `Duration`, plus nested retry helpers (see C1).

**E7. Deadlines that aren't propagated.** A voice request should carry one absolute deadline through STT → LLM → TTS and cancel downstream work when the user gives up (https://sre.google/sre-book/addressing-cascading-failures/).

**E8. Lock ordering and re-entrancy across subsystems.** **[heuristic]** Document a global lock order. Detect with `parking_lot`'s `deadlock_detection` feature in debug builds, or with loom for the core primitives (https://docs.rs/crate/loom/latest).

**E9. Wrong clock for intervals.** Use `Instant`, not `SystemTime`, for intervals. Note that whether system suspend counts as elapsed time is *unspecified* and varies by platform. `elapsed()` saturates to zero if the clock goes backwards (https://doc.rust-lang.org/std/time/struct.Instant.html).
- *Fix:* tick logic should tolerate a huge jump after laptop sleep, for example not firing 500 missed ticks at once (tokio `MissedTickBehavior`). **[heuristic]**

---

## F. Resource leaks (processes, threads, handles, files)

**F1. Child processes that are never killed or waited on.** `Child` has no `Drop`. A dropped child keeps running, and on Unix an un-waited child stays a zombie. The std docs advise against dropping `Child` in long-running applications (https://doc.rust-lang.org/std/process/struct.Child.html).
- *Detect:* clippy `zombie_processes` (https://rust.googlesource.com/rust-clippy/+/refs/heads/beta/clippy_lints/src/zombie_processes.rs). Grep `\.spawn\(\)` and follow each handle.
- *Fix:* an RAII wrapper that kills and waits on `Drop`.

**F2. Orphaned llama-server, ffmpeg or whisper processes after Atlas crashes (Windows).** Windows children survive their parent. Put them in a Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so the OS kills them however Atlas exits (https://meziantou.net/killing-all-child-processes-when-the-parent-exits-job-object.htm).
- *Detect:* kill Atlas from Task Manager, then list processes. On the next start, also detect and adopt or kill a stale llama-server holding the port. **[heuristic]**

**F3. Pipe deadlocks with children.** If the child fills its stdout buffer while the parent is blocked writing stdin, or isn't reading stderr, both sides hang. Use `wait_with_output()` or drain each pipe on its own thread (https://docs.w3cub.com/rust/std/process/index).
- *Detect:* grep `Stdio::piped()` on stderr where nothing reads it. ffmpeg writes a lot to stderr.

**F4. ffmpeg killed instead of stopped.** An MP4 killed mid-recording has no `moov` atom and won't play (https://ffmpeg.org/pipermail/ffmpeg-user/2014-June/022266.html).
- *Fix:* stop gracefully (`q` on stdin), then kill after a timeout. Or record to a container that survives truncation (MKV or fragmented MP4). **[heuristic on container choice]**

**F5. Temp files outliving the process.** `NamedTempFile` and `TempDir` cleanup depends on destructors, which don't run on signals, inside statics, or after `process::exit`. Unnamed `tempfile()` relies on the OS instead (https://docs.rs/tempfile/).
- *Fix:* sweep Atlas's own temp prefix at startup (Release It! "Steady State": https://github.com/nishantsbi/notes/blob/master/%5Bbook%5D%20release_it.md).

**F6. Unbounded growth of logs, recordings, transcripts and caches.** "Steady State" means anything that accumulates needs an automatic purge (https://github.com/nishantsbi/notes/blob/master/%5Bbook%5D%20release_it.md). Also "Unbounded Result Sets": "the only sizes you care about are zero, one, or lots" (same source).
- *Detect:* every directory Atlas writes to needs a size or age cap. Check log rotation is configured.

**F7. Threads spawned per event.** Spawning a `std::thread` per request or tick, without a cap, leaks under load. **[heuristic]** Detect: grep `thread::spawn` inside loops or handlers.

**F8. Memory that creeps over weeks.**
- *Detect:* dhat-rs for heap profiling, and its heap-usage tests can assert peak bytes and allocation counts (https://github.com/nnethercote/dhat-rs). Track RSS in the hub over a 24-hour soak. **[heuristic]**

**F9. Windows file locks and transient errors.** Renames and deletes fail intermittently with `ERROR_ACCESS_DENIED`, `ERROR_SHARING_VIOLATION` or `ERROR_FILE_NOT_FOUND`. Go's toolchain retries exactly these errors on Windows (https://go.googlesource.com/build/+/HEAD/maintner/internal/robustio/robustio.go).
- *Fix:* do atomic writes as temp file + `fsync` + rename, with a bounded retry on those three errors.

---

## G. Performance, CPU and idle cost

**G1. Idle wakeups from polling ticks.** A background app shouldn't wake the CPU on timers.
- *Measure:* `wpr -start power -filemode`, leave idle for 5 minutes, `wpr -stop idle.etl`, then look at WPA → CPU Usage (Precise). Context switches and utilisation should be about 0, and "New Thread Stack" shows what woke the CPU (https://learn.microsoft.com/en-in/windows/apps/performance/power).
- *Fix:* make the tick event-driven. Back off the tick rate when idle.

**G2. `timeBeginPeriod(1)` or a high-resolution timer left on.** It raises scheduler frequency and stops the CPU entering power-saving states. Since Windows 10 2004 it applies per process, and Windows 11 ignores it for occluded or minimised windows (https://learn.microsoft.com/en-us/windows/win32/api/timeapi/nf-timeapi-timebeginperiod).
- *Detect:* grep `timeBeginPeriod|NtSetTimerResolution`, including inside dependencies (audio crates).

**G3. Background work not marked as background.** Opt the daemon's non-urgent threads (indexing, sync, transcription backlog) into EcoQoS via `SetProcessInformation` or `SetThreadInformation` (PowerThrottling). Microsoft reports up to 90% lower CPU power (https://devblogs.microsoft.com/performance-diagnostics/introducing-ecoqos/).

**G4. Allocation churn in hot paths.** Audio frames, video frames and token streams are hot paths.
- *Fix:* `Vec::with_capacity`, reuse buffers with `clear()`, avoid `format!` and `clone()` in loops, use `read_line` into a reused `String` instead of `lines()` (https://nnethercote.github.io/perf-book/heap-allocations.html).
- *Detect:* clippy `redundant_clone`, dhat (https://github.com/nnethercote/dhat-rs), and `cargo flamegraph` or the profilers listed at https://nnethercote.github.io/perf-book/profiling.html.

**G5. Release profile left at defaults, or symbols stripped so profiling is blind.** Consider `codegen-units=1`, LTO (reported 10–20%+ gains), mimalloc or jemalloc, and `panic="abort"` (https://nnethercote.github.io/perf-book/build-configuration.html). Note that `panic=abort` stops a panic hook from unwinding to supervisors, so decide deliberately. **[heuristic]**

**G6. Cold start and a cold cache.** A model loading after a restart can't serve full load. SRE advises ramping up gradually (https://sre.google/sre-book/addressing-cascading-failures/).
- *Atlas-specific:* llama-server returns **503 `{"status":"loading model"}`** on `/health` until it is ready, and 200 `ok` after (https://github.com/NousResearch/nous-llama.cpp/blob/master/examples/server/README.md). Gate "LLM ready" on that, not on the process existing.

**G7. Large futures and stack blowups.** Deeply nested async state machines get copied around.
- *Detect:* clippy `large_futures` (https://rust-lang.github.io/rust-clippy/master/index.html). **[heuristic: it matters most on mobile targets]**

---

## H. Configuration and path handling across OSes

**H1. CRLF/LF drift.** Different chats on different OSes commit different line endings. This shows up as whole-file diffs, broken shell scripts, and tests that compare text exactly.
- *Fix:* `* text=auto` in `.gitattributes` normalises to LF in the index, with `*.sh text eol=lf` and `*.bat text eol=crlf`. Attributes override `core.autocrlf` (https://git-scm.com/docs/gitattributes).
- *Detect:* `git ls-files --eol`.

**H2. Hard-coded separators and string paths.** Grep `"/"` and `"\\\\"` concatenated into paths, and `format!("{}/{}"`. Use `Path::join`. **[heuristic]**

**H3. Windows reserved names and invalid characters.** `CON`, `PRN`, `AUX`, `NUL`, `COM1-9`, `LPT1-9` are reserved *even with an extension* (`NUL.txt`). Names can't contain `< > : " / \ | ? *` or end with a space or period. Names are case-insensitive by default (https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file).
- *Fix:* sanitise any filename built from a contact name, transcript title, LLM output or peer data.

**H4. MAX_PATH (260) and the `\\?\` prefix.** Long paths need opt-in or the `\\?\` prefix (https://learn.microsoft.com/en-us/windows/win32/fileio/naming-a-file). `std::fs::canonicalize` returns `\\?\` paths, which many tools and UIs can't handle. The `dunce` crate strips the prefix when it's safe (https://docs.rs/crate/dunce/latest).
- *Detect:* grep `canonicalize\(`.

**H5. Lossy path and OS-string handling.** `to_string_lossy()` and `to_str().unwrap()` on paths corrupt or panic on non-UTF-16-clean Windows names (WTF-8 background: https://internals.rust-lang.org/t/osstr-wtf8-as-bytes-and-to-string-unchecked/12694).
- *Detect:* grep `to_str\(\)\.unwrap|to_string_lossy` in places where paths are used again rather than just displayed.

**H6. Child processes popping console windows from a GUI-subsystem binary.** Set `CommandExt::creation_flags(CREATE_NO_WINDOW = 0x08000000)` on every `Command` (https://users.rust-lang.org/t/command-without-windows/74567). A GUI-subsystem parent with no console passes invalid std handles, so a console child spawned with inherited stdio may produce no output (https://github.com/rust-lang/rust/issues/101645).
- *Detect:* grep `Command::new` and check each call goes through one shared builder (see C1).

**H7. Assuming a service context.** Session 0 isolation separates services from the user's desktop. On Windows 10 and 11, input to session 0 is discarded (https://www.firedaemon.com/post/microsoft-windows-interactive-services-and-session-0-isolation). Microphone, camera, tray and notifications belong in the user session. If Atlas ever runs as a service, those subsystems must move to a user-session helper.

**H8. Environment variables read at random points.** Different chats read `std::env::var` ad hoc, and writes are unsound once threads exist (https://doc.rust-lang.org/nightly/edition-guide/rust-2024/newly-unsafe-functions.html).
- *Fix:* read the environment once into a typed config at startup.

**H9. Config that silently accepts typos.** **[heuristic]** Use `#[serde(deny_unknown_fields)]` on config structs and log the effective config at startup with secrets redacted.

---

## I. Security

**I1. Command injection through spawned commands, especially `.bat`/`.cmd` on Windows.** Rust's `Command` escaped arguments to batch files incorrectly before **1.77.2** (CVE-2024-24576, "BatBadBut", CVSS 10) (https://thehackernews.com/2024/04/critical-batbadbut-rust-vulnerability.html).
- *Detect:* `rustc --version` must be ≥1.77.2. Grep `Command::new\("cmd"|\.bat|\.cmd|powershell|sh -c`.
- *Fix:* never pass LLM, peer or user text as an argument to a shell or batch file. Call executables directly with absolute paths.

**I2. Untrusted model output treated as trusted.** OWASP LLM05 (Improper Output Handling) covers model output reaching shells, paths, SQL or HTML, which leads to RCE, path traversal and XSS. Apply zero trust: validate and encode output like user input (https://genai.owasp.org/llmrisk/llm052025-improper-output-handling/).
- *Atlas-specific:* tool calls from llama, filenames the LLM suggests, and markdown rendered in the hub.

**I3. The lethal trifecta.** Private data (the vault), untrusted content (friend-sync messages, web pages, transcripts of calls) and the ability to communicate externally (Tor sync, HTTP). Together they let an attacker exfiltrate data through prompt injection. Once an agent has taken in untrusted input, that input must not be able to trigger consequential actions (https://simonwillison.net/2025/Jun/16/the-lethal-trifecta/). See also OWASP LLM06 Excessive Agency (https://genai.owasp.org/llmrisk/llm062025-excessive-agency/).
- *Detect:* map which LLM contexts include peer or call content and which tools they can invoke.

**I4. A localhost hub open to DNS rebinding and CSRF.** Services bound to 127.0.0.1 can be reached by malicious web pages through DNS rebinding.
- *Fix:* validate the `Host` header against an allowlist and require authentication (https://github.com/nccgroup/singularity/wiki/%5BAnnouncement-Blog-Post%5D--Singularity-of-Origin:-A-DNS-Rebinding-Attack-Framework). Don't rely on Chrome's Private Network Access: enforcement has been rolled back several times (https://developer.chrome.com/blog/private-network-access-preflight).
- *Detect:* `curl -H "Host: evil.example" http://127.0.0.1:PORT/api/...` must be refused.

**I5. SSRF wherever Atlas fetches a URL it was given.** The URL might come from an LLM tool call, a peer or a link preview.
- *Fix:* prefer an allowlist; resolve the name and reject private, loopback and link-local ranges; disable redirects; don't accept full URLs where components will do (https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html). Watch for the LLM being steered to `http://127.0.0.1:<llama-port>` or to the hub itself.

**I6. Weak vault key derivation.** The OWASP minimum for Argon2id is m=19 MiB, t=2, p=1 (https://cheatsheetseries.owasp.org/cheatsheets/Password_Storage_Cheat_Sheet.html). Vault KDFs should exceed that, since there's no server-side rate limiting.
- *Detect:* grep `argon2|pbkdf2|sha256\(.*pass` and check that parameters and salt are stored with the ciphertext. **[heuristic on "should exceed"]**

**I7. Secrets lingering in memory and logs.** `zeroize` wipes memory reliably with volatile writes, but only the buffer you call it on. Copies made by moves or clones are not covered (https://docs.rs/crate/zeroize).
- *Fix:* wrap secrets in a type that has no `Clone` and a redacting `Debug`. Grep `#[derive(.*Debug.*)]` on secret-bearing structs.

**I8. Leaking build paths and usernames into binaries.** Release binaries embed absolute paths such as `/home/<user>/.cargo/...` in panic messages and debug info. `trim-paths` and `--remap-path-prefix` address this (https://rust-lang.github.io/rfcs/3127-trim-paths.html).
- *Detect:* `strings atlas.exe | grep -i users\\`.

**I9. Generic AI-code security debt.** Veracode measured 45% flawed and CodeRabbit 1.57x more security findings (https://www.helpnetsecurity.com/2025/08/07/create-ai-code-security-risks/ , https://www.theregister.com/2025/12/17/ai_code_bugs/).
- *Fix:* do a deliberate trust-boundary review of every input source (mic/STT text, peer sync, hub HTTP, LLM output, files) rather than a line-by-line skim. **[heuristic]**

**I10. `unsafe` without a stated invariant.** Every `unsafe` block needs a documented reason (M-UNSAFE: https://microsoft.github.io/rust-guidelines/print.html). Run Miri on pure-Rust unsafe code (https://github.com/rust-lang/miri).
- *Detect:* `grep -rn "unsafe" src/ | grep -v SAFETY`, and clippy `undocumented_unsafe_blocks` (https://rust-lang.github.io/rust-clippy/master/index.html).

---

## J. Dependency hygiene and supply chain

**J1. Slopsquatted or hallucinated crates.** 5–22% of LLM package suggestions are hallucinated, and the fake names repeat, which is what makes squatting them viable (https://www.usenix.org/conference/usenixsecurity25/presentation/spracklen).
- *Detect:* for every crate an AI added, check its crates.io age, download count and repository link. cargo-deny `sources` limits crates to trusted registries (https://docs.rs/cargo-deny).

**J2. Known vulnerabilities.** `cargo audit` and cargo-deny `advisories` (https://docs.rs/cargo-deny). Build with **cargo-auditable** so shipped binaries can be scanned later by cargo-audit, trivy, grype or osv-scanner (https://docs.rs/cargo-auditable/).

**J3. Unused dependencies.** `cargo machete` is fast and works on stable but has some false positives. `cargo +nightly udeps` is slower and more accurate (https://playbooks.com/skills/laurigates/claude-plugins/cargo-machete).

**J4. Licence compliance.** cargo-deny `licenses` matters if Atlas is ever distributed or sold (https://docs.rs/cargo-deny). Check whisper, llama and model weight licences separately. **[heuristic]**

**J5. Lockfile discipline.** Commit `Cargo.lock` for applications. Rust's 2023 guidance is to commit it by default (https://blog.rust-lang.org/2023/08/29/committing-lockfiles/). Build releases with `--locked`. **[heuristic on --locked]**

**J6. Pinning external binaries.** llama-server, whisper.cpp, ffmpeg and tor are dependencies too. Record version and hash, and add a contract test (A5). A new llama-server can change JSON or flags without any compile error. **[heuristic]**

---

## K. Audio and media pipelines

**K1. Sample-rate and channel mismatches.** Whisper expects 16 kHz mono float (whisper.rn format notes: https://www.mintlify.com/mybigday/whisper.rn/core-concepts/audio-formats). WASAPI usually delivers the device mix format, commonly 48 kHz stereo float. **[heuristic]**
- *Detect:* assert the format at every boundary (capture → resampler → VAD → STT) and log it once per stream. Grep hard-coded `16000|44100|48000` (C3).

**K2. Loopback capture going silent when nothing is playing.** WASAPI loopback produces no packets while no audio is rendered. GStreamer fixed this by playing silence into a render client so capture keeps flowing (https://gitlab.freedesktop.org/gstreamer/gst-plugins-bad/-/merge_requests/1588). Before Windows 10 1703, event-driven loopback received no events and needed a render-stream workaround (https://learn.microsoft.com/en-us/windows/win32/coreaudio/loopback-recording).
- *Symptom in Atlas:* call recordings with gaps or timestamp drift, or a stalled tick waiting on a buffer.
- *Fix:* fill silence by wall clock, or play silence into a render client.

**K3. Device loss and default-device changes.** Headset unplugged, Bluetooth switches, or a dock changes. The app must handle `IMMNotificationClient::OnDefaultDeviceChanged` and `IAudioSessionEvents::OnSessionDisconnected` (DeviceRemoval), then **re-create the stream** on the new default. Notifications arrive asynchronously and in no fixed order (https://learn.microsoft.com/en-us/windows/win32/coreaudio/relevant-device-notifications-for-stream-routing).
- *Detect:* grep `AUDCLNT_E_DEVICE_INVALIDATED`. If nothing handles it, capture dies until restart.

**K4. Privacy toggles mistaken for silence.** When Windows microphone access is blocked, apps run but "nobody hears you", or they get access errors (https://geekrewind.com/what-breaks-when-you-turn-off-microphone-access-in-windows-11-privacy-settings/).
- *Fix:* tell "blocked" apart from "quiet". If the RMS stays exactly 0.0 for N seconds, raise a health warning. **[heuristic]**

**K5. STT hallucinating on non-speech.** Whisper hallucinated on 40.3% of 301k non-speech clips. 3% of phrases ("thank you for watching"…) made up 67% of the hallucinations. VAD before transcription combined with a bag-of-hallucinations filter cut WER to 6.5–9.4% (https://arxiv.org/html/2501.11378v1).
- *Fix:* gate whisper behind VAD and drop known phantom phrases before they reach the LLM or memory.

**K6. Clock drift between capture streams.** Mic and loopback clocks drift apart over a long call. **[heuristic]** Timestamp from the device clock (QPC position), not from arrival time, and resample to align.

**K7. Audio callback doing real work.** Allocating, locking, logging or doing I/O in the real-time capture thread causes glitches. **[heuristic, consistent with E1 at https://ryhl.io/blog/async-what-is-blocking/]** Hand frames off through a bounded lock-free ring.

**K8. Camera via ffmpeg.** See F1–F4 for lifecycle, pipes and graceful stop. Also re-enumerate devices after a USB replug, the same way as K3. **[heuristic]**

---

## L. Release engineering

**L1. No crash reporting.** Panics are invisible in GUI builds (D4), and native crashes in whisper or llama FFI never reach a panic hook.
- *Fix:* out-of-process minidumps. Mozilla's rust-minidump turned 4 GB core dumps into about 2 MB minidumps (https://hacks.mozilla.org/2022/06/everything-is-broken-shipping-rust-minidump-at-mozilla/). Embark's `minidumper` crate is one implementation (https://docs.rs/crate/minidumper/0.11.0).

**L2. Non-reproducible builds.** Absolute paths, timestamps and an unlocked dependency graph make builds differ (https://rust-lang.github.io/rfcs/3127-trim-paths.html). Use `--locked`, a pinned `rust-toolchain.toml`, and path remapping.

**L3. No embedded build identity.** **[heuristic]** Embed version, git SHA and a dirty flag in the binary, and show them in the hub and in every crash report. Without them you can't match a report to source.

**L4. No rollback.** **[heuristic]** Keep the previous binary and a vault/data schema version. Migrations must be forward-only and backed up before they run, and the old binary must refuse newer data rather than corrupt it. Lessons from the Replit incident: separate dev and prod data, and make restore one click (https://www.eweek.com/news/replit-ai-coding-assistant-failure/).

**L5. Code signing and SmartScreen.** Since 2024, EV certificates no longer grant instant SmartScreen reputation. Every new hash starts at zero, so avoid rotating certificates near a release (https://www.todesktop.com/blog/posts/windows-apps-psa-ev-certs-do-not-grant-immediate-reputation-anymore).

**L6. CI gates that agents can't quietly skip.** **[heuristic]** Gate on `clippy -D warnings` with the restriction lints above, `cargo deny check`, `cargo audit`, `cargo machete`, nextest, and a cargo-mutants run on changed files (`--in-diff`). Microsoft's static-verification list covers clippy, rustfmt, cargo-audit, cargo-hack, cargo-udeps and Miri (M-STATIC-VERIFICATION: https://microsoft.github.io/rust-guidelines/print.html).

**L7. Toolchain floor.** Require Rust ≥1.77.2 because of CVE-2024-24576 (https://thehackernews.com/2024/04/critical-batbadbut-rust-vulnerability.html), and Rust 2024 so `set_var` is `unsafe` (https://doc.rust-lang.org/nightly/edition-guide/rust-2024/newly-unsafe-functions.html).

---

## M. Long-running daemon stability (Release It! and SRE patterns applied)

**M1. Circuit breakers on llama, Tor, STT and camera.** After repeated failures, stop calling the dependency for a while and degrade instead of hammering it (https://github.com/nishantsbi/notes/blob/master/%5Bbook%5D%20release_it.md).

**M2. Bulkheads.** Give each subsystem its own threads or runtime and its own queue, so a stuck camera can't starve voice (same source).

**M3. Fail fast.** If llama is still loading (503), say so immediately instead of making the user wait 60 seconds for a timeout (same source; https://github.com/NousResearch/nous-llama.cpp/blob/master/examples/server/README.md).

**M4. Small queues and dropping stale work.** Keep queues short relative to workers, and consider LIFO or CoDel so stale voice requests get dropped (https://sre.google/sre-book/addressing-cascading-failures/).

**M5. SLA inversion.** Atlas is only as reliable as its least reliable dependency unless it degrades gracefully (https://github.com/nishantsbi/notes/blob/master/%5Bbook%5D%20release_it.md). List which features must still work offline, with Tor down or with no model loaded.

**M6. Soak test.** **[heuristic]** Run the daemon for 24–72 hours with synthetic voice, device unplugs and sleep/resume. Chart RSS, handle count, thread count, child-process count and CPU. All of them should stay flat.

---

## N. Cross-platform and mobile drift

**N1. `cfg(windows)` branches that are never compiled on other targets.** **[heuristic]** Run `cargo check --target` for each target in CI, and use `cargo hack --each-feature` (cargo-hack appears in M-STATIC-VERIFICATION: https://microsoft.github.io/rust-guidelines/print.html). Otherwise the Linux, iOS and Android paths rot quietly.

**N2. Features that are no-ops on some platforms.** A `#[cfg(not(windows))] fn record() -> Result<()> { Ok(()) }` is hollow code (A1) hidden behind `cfg`. Grep `cfg\(not\(` and check what the body does. **[heuristic]**

---

## P. "Fluent but wrong": the assistant claims success without checking

**P1. Reporting success without checking the result.** Coding agents misreport what they did. The Replit agent ran destructive actions during a freeze and fabricated data (https://www.eweek.com/news/replit-ai-coding-assistant-failure/). 66% of developers report "almost right" output (https://venturebeat.com/ai/stack-overflow-data-reveals-the-hidden-productivity-tax-of-almost-right-ai-code). The same pattern appears at runtime when Atlas's LLM says "Done, I've sent it", or "I've set a reminder", based on intent rather than an observed effect.
- *Fix:* every user-visible claim of an action must come from the tool's *verified* result (read-back, exit code, HTTP 2xx plus body check). The LLM should narrate results, not intentions. **[heuristic, applying LLM05 zero-trust: https://genai.owasp.org/llmrisk/llm052025-improper-output-handling/]**

**P2. Green ticks that only check a flag, not a probe.** SRE counts "implicit" errors, such as a 200 with the wrong content (https://sre.google/sre-book/monitoring-distributed-systems/).
- *Fix:* hub status should come from probes, such as `/health` 200 with `ok`, frames received in the last N seconds, or the last transcript timestamp.

**P3. Self-assessment can't be trusted, by humans or models.** Developers believed AI made them 20% faster when it made them 19% slower (https://metr.org/blog/2025-07-10-early-2025-ai-experienced-os-dev-study/). Agents' "tests pass" correlated with 0% mergeable (https://metr.org/blog/2025-08-12-research-update-towards-reconciling-slowdown-with-time-horizons/).
- *Fix:* an item is "done" only when there is an external artifact: a test that fails if the feature is removed (cargo-mutants), a log line from the live daemon, or a screenshot.

**P4. Hallucinated memory and transcripts treated as fact.** STT phantom phrases (K5) and LLM summaries written into memory become "facts". **[heuristic]** Tag the provenance of every stored item (heard, inferred, or user-confirmed) and only let user-confirmed items drive actions.

**P5. Destructive actions without confirmation or undo.** Separate "plan" from "apply", require confirmation for irreversible actions, and keep a backup (the Replit remediation was dev/prod separation, a planning-only mode and one-click restore: https://www.eweek.com/news/replit-ai-coding-assistant-failure/).

---

## Q. Suggested order for the one-pass sweep

1. **Gates first** (L6, L7): clippy restriction lints (`unwrap_used`, `let_underscore_must_use`, `await_holding_lock`, `zombie_processes`, `print_stdout`, `todo`, `undocumented_unsafe_blocks`), cargo-deny, cargo-audit, machete. This turns a large share of the checklist into a list of compiler errors.
2. **Measure test reality** (B): nextest vs cargo test for order dependence, then a cargo-mutants baseline. The missed-mutant list *is* the hollow-code list (A1).
3. **Dedupe** (C): one canonical module per concern, with the rest banned via `disallowed_methods`.
4. **Lifecycle** (F, E5, E3): process wrapper (kill-on-drop + Job Object + `CREATE_NO_WINDOW` + timeouts + drained pipes), bounded channels, timeouts on every client.
5. **Trust boundaries** (I): hub Host check and auth, LLM output → actions, peer content → LLM context, SSRF.
6. **Audio and media** (K): device-change handling, loopback silence, VAD before whisper, format assertions.
7. **Observability and release** (D, L, P): panic hook to file, WorkerGuard, probe-based health, minidumps, build ID.
8. **Soak and idle** (G1, M6): WPR idle trace plus a 48-hour soak, accepted only when the curves stay flat.

---

## Known gaps in this research
- No fetched source covered **Tor-specific** daemon pitfalls (onion-service auth, circuit timeouts). Only the generic integration-point, timeout and SSRF guidance applies.
- No primary source confirmed **whisper.cpp's exact input format** beyond a third-party doc. Check it against the pinned whisper.cpp version.
- Clippy lint names other than the `let_underscore*` family and `zombie_processes` are cited to the lint index, but each lint page wasn't fetched individually.
- iOS and Android lifecycle (background execution limits) was not researched. It needs its own pass.
- The GitClear figures come from LeadDev's write-up, not the GitClear PDF itself.
