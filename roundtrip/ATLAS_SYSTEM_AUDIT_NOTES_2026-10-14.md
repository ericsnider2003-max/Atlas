# Atlas system audit notes — 2026-10-14

## Scope and confidence

This is a read-only continuation of the capability research. It inspects the current Atlas source, its open-gap register, and the available audit profile. It is not a production-readiness sign-off: the formal roundtrip runner could profile the repository but could not produce a baseline because the copied environment did not have the required Python/Node runtime tooling.

The goal is to identify what must be strengthened for a fast, private, multimodal, proactive Atlas with a JARVIS/EVE-style interaction model. Findings are split into confirmed source or evidence gaps and items that need an installed-device measurement. None of these findings means the whole project has to wait.

## Bottom line

The target is a realistic product direction when translated into engineering behavior: natural voice interaction, durable memory, grounded answers, device and workspace actions, multimodal understanding, media creation, coding and system analysis, market analysis, proactive jobs, and graceful recovery. Atlas already contains meaningful foundations for many of these.

The biggest current risk is not capability design. It is proof and operational integration: several critical behaviors are implemented in source but have not yet been demonstrated on the actual laptop, phone, or always-on startup path. Those can be worked on in parallel with capability development.

## Confirmed findings

### A1 — Always-on recovery is not end-to-end proven (high)

`atlas/src/crash.rs` explicitly documents that the crash layer does not restart Atlas, register a service, or retry the failed operation. It records evidence and reports on the next start. That is a sensible inner-layer boundary, but the outer watchdog, Windows startup/task integration, restart behavior, and recovery after a real crash have not been proven in the open-gap register.

Impact: without a verified outer supervisor, Atlas can be intelligent yet feel unreliable after a panic, update, sleep/wake event, or process exit.

Fix direction: keep the crash evidence layer; add and test a narrowly scoped supervisor contract for startup, bounded restart, crash-loop backoff, safe-mode launch, and user-visible recovery state. Prove it with a real restart into a new build and a forced crash.

### A2 — Real-device latency and battery behavior are unknown (high)

The phone model path is now linked in source, but the open-gap register says it has not been measured on a real iPhone or Android phone. The available two-core laptop measurements are approximately 16 tokens/s for the 0.6B model and 6.5 tokens/s for the 1.7B model, with Atlas answers taking roughly 12–16 seconds and 38–59 seconds respectively.

Impact: a capability can be technically local yet unusable if model selection, prompt loading, or wake-to-first-token time is poor.

Fix direction: create a latency budget and benchmark harness for wake, recognition, routing, first token, completion, tool call, and spoken response. Measure cold/warm model paths, concurrent work, memory pressure, battery draw, and degraded-network behavior on representative hardware. Use those measurements to drive model routing, prompt caching, keep-warm policy, and graceful degradation.

### A3 — The laptop build/install path is not yet self-hosting (high)

The open-gap register reports that the active Windows toolchain cannot build because GNU `dlltool` is missing, with MSVC Build Tools/toolchain switching as the alternative. A self-improving Atlas cannot reliably compile, test, package, and restart itself until the canonical local build path is deterministic.

Impact: coding, repair, updates, and capability expansion can exist in source but stall at the last mile.

Fix direction: choose one supported Windows toolchain, bootstrap it with a preflight diagnostic, make the build path reproducible, and verify a full edit → compile → test → package → restart loop. Preserve the sandbox and compiler-as-authority design already present in `build_it` and `fixloop`.

### A4 — Capability count overstates installed capability until reachability is demonstrated (high)

The catalog contains 247 concrete capability rows. The open-gap register says the 247 functions are built and tested but reached only by tests (`TEST_ONLY_MAX`). This is valuable coverage, but it is not equivalent to user-reachable behavior through voice, chat, Hub, phone, or automation.

Impact: Atlas can appear broad in documentation while still failing to invoke the right operation from a natural request.

Fix direction: maintain a capability truth table with separate states for implemented, routed, permissioned, installed, hardware-tested, and repeatably verified. Add one user-shaped acceptance path per capability family and measure route accuracy, clarification rate, completion, and recovery.

### A5 — Mobile delivery is built but not installed and operated on the real devices (high)

The phone apps have been built and signed, but the open-gap register says nothing has been installed yet. Tailnet sync, iOS compilation on the actual delivery path, Android NDK cross-compile, install flow, and phone operation remain hardware checks.

Impact: cross-device continuity, local model performance, notifications, and offline-first behavior remain assumptions until exercised.

Fix direction: run a small device matrix: laptop, iPhone/iPad, Android, and one degraded/offline scenario. Record install, update, wake, sync conflict, notification, model download/resume, and recovery evidence.

### A6 — Voice and room behavior are calibrated on synthetic speakers, not the owner’s environment (medium/high)

Wake phrase, voice lock, hearing thresholds, and room behavior are currently measured on synthesized speakers. The open-gap register explicitly calls for real voice takes and room calibration.

Impact: the JARVIS/EVE experience fails at the first interaction if wake detection, barge-in, echo cancellation, or speaker identity is unreliable.

Fix direction: test real voice, distance, fan/TV noise, multiple speakers, interruption, false wakes, and privacy mute. Keep a visible local-only mode and a manual fallback.

### A7 — Vision, accessibility, and sensor claims need hands-on proof (medium)

Automated accessibility checks do not replace NVDA, JAWS, Narrator, VoiceOver, or TalkBack use. The same applies to room vision, OCR, camera permissions, and gestures: source support is not device proof.

Fix direction: make accessibility and permission-denial paths part of the acceptance matrix, not a late polish pass. A JARVIS-like system must remain controllable without voice and must fail safely when camera, microphone, or screen access is denied.

### A8 — The local coding model is an explicit missing dependency for self-improvement (medium/high)

The repair loop is designed around sandboxed changes, tests, time limits, and verification, but the open-gap register says real use still needs a local coding model or an explicitly permitted delegate. The architecture is a good foundation; the missing proof is model quality, routing, and end-to-end execution.

Fix direction: benchmark a small local coding model first, then add escalation only when evidence shows it is needed. Require patch review, targeted tests, rollback, and a human approval boundary for changes to permissions, networking, startup, data stores, and financial logic.

### A9 — Performance tuning exists, but the system needs a repeatable performance lab (medium/high)

Atlas already has model budgets, keep-warm behavior, prompt-cache measurements, bounded queues, cancellation, and timeout logic. These are useful foundations, but they need a single repeatable lab that measures the whole turn rather than isolated code paths.

Fix direction: publish p50/p95/p99 timings and resource ceilings for voice-only, text-only, vision, tool calls, multi-step jobs, model swaps, and concurrent subagents. Treat regressions as capability failures because delay destroys practical usefulness.

### A10 — Self-improvement needs an explicit observe → propose → verify → mutate boundary (medium)

The source contains pieces of this pattern in the build and repair loops, but the full system-wide contract is not yet proven for “any system I throw at it,” including Atlas itself.

Fix direction: normalize every improvement job into an evidence bundle: system inventory, goal, hypothesis, proposed diff/configuration, sandbox test, security/data-impact review, approval, apply, rollback point, and post-change measurement. This lets Atlas improve systems without granting it an unbounded self-editing loop.

### A11 — Formal audit evidence is incomplete (medium)

The roundtrip profiler identified mixed Rust/Python/JavaScript/C++/shell/batch components and the relevant risk packs, but no baseline was generated because the copied audit environment lacked runtime tooling. Therefore readiness, test totals, and reliability claims must remain provisional.

Fix direction: install or expose the repository’s supported audit runtimes, rerun the baseline, then verify the highest-risk paths: money analysis data handling, private data storage, startup/recovery, update integrity, scripts, and web/desktop boundaries.

## Foundations worth preserving

- Local model selection and memory-aware budgeting.
- Bounded queues, cancellation, timeouts, and keep-warm logic.
- Sandboxed build/fix loops where the compiler and tests are authoritative.
- Crash evidence collection that avoids pretending recovery happened when it did not.
- Offline-first and signed/update-oriented design work.
- Capability catalog and explicit open-gap register.
- Separation between market analysis and trade execution.
- Privacy-oriented local storage, vault/recovery concepts, and permission-aware device actions.

## Parallel workstreams — no artificial prerequisite chain

1. Continue capability research and competitor-derived behavior mapping.
2. Build the performance lab and local-model bake-off.
3. Repair the Windows build/install path.
4. Prove the outer supervisor and safe restart path.
5. Install and measure the phone paths.
6. Exercise real voice, camera, accessibility, and offline scenarios.
7. Connect the coding/self-improvement loop to a real local coding model.
8. Convert the 247-row catalog into installed, routed, and hardware-proven acceptance evidence.

Media generation, market analysis, coding assistance, business workflows, local subagents, and system diagnostics do not need to wait for every device test. They should be marked honestly by evidence state while the proof work proceeds.

## Product translation of the fictional target

The useful target is not a movie claim of unlimited intelligence. It is a system that is quick to talk to, remembers context, sees and hears with permission, acts across the owner’s devices, explains uncertainty, runs durable jobs, creates media locally, analyzes code/business/markets, improves systems through verified changes, and recovers visibly when something fails. Atlas can be built toward that target; the audit shows where the current implementation needs operational proof or strengthening.

