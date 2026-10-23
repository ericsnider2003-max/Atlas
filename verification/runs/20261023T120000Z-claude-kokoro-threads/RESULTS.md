# Kokoro threads for back-to-back speech: 2026-10-23 (Claude, usage-max routed)

Question: why back-to-back Kokoro calls slowed to about 2x real time (found 22 Oct), and whether thread or power settings fix it.

Method: `evidence/kbench.py` drives Atlas's own sherpa-onnx 1.13.8 C API and the installed `model.int8.onnx` (af_heart) through ctypes, with no rebuild. Each run: 10 back-to-back calls on one 3.6 s sentence; figures are synthesis time / speech length. The laptop was on mains, power plan Balanced. Power plans weren't changed (a system setting); only measured. Other sessions' builds and training ran for parts of the window. Every comparison takes 1 and 4 threads in turns, so both see the same conditions (`evidence/matrix1-3.txt`).

| Comparison | 1 thread (avg) | 4 threads (avg, Atlas until now) |
|---|---|---|
| Near idle (matrix 1) | 0.87 | 1.00 |
| Interleaved round 1 (other builds at times) | 1.43 | 2.51 |
| Round 2 | 1.14 | 1.83 |
| Round 3 | 1.25 | 2.58 |
| Clean pair (no build, CPU < 25% before both) | 1.40 | 2.00 |

- One thread was faster in all five comparisons, and steadier. Four threads drifted to 2-3x after the first call: that is the "back-to-back slowdown". 2 threads sat between; 6 threads about 1.0; 8 threads about 1.2.
- In only 1 of about 30 attempts over 2 hours were both runs preceded by a quiet check, so the "clean" sample is a single pair.
- Likely cause (not proven): onnxruntime's intra-op threads on a hybrid chip (4 performance + 4 low-power cores) pay synchronisation and scheduling costs that outweigh the parallel gain for this small model.

Change shipped: `kokoro::threads_for`. Machines with 8 or more logical processors use 1 thread; smaller machines keep the measured rule (half, 2-4; a 2-core Xeon measured 28 Sep: 2 threads 1.25-1.31x vs 1 thread 1.67x). The 8+ cut-off is extrapolated from this one 8-processor hybrid laptop. A 16-processor all-performance desktop is untested; `KOKORO_THREADS` in `tests/kokoro_voice.rs` lets anyone measure. The recogniser's own thread count was not changed.

Checks: unit test `kokoro::tests::thread_count_follows_what_was_measured`; lib kokoro/asr_server/speech (22); guards dead_/catalogue/wiring/no_quiet_nothings/retrospective (105); splitter (6); kokoro module tests (12). Reviewer: PASS (one documentation suggestion, recorded above). Installed-build `atlas kokoro-check`, alternating new and old under load: 1 thread 2.07 (cold first run), 1.13, 1.12; 4 threads 1.29, 1.23, 1.19. Verifier 14/14. Installed `CF72FAEA...`, rollback `atlas-0.1.0-dev.previous` = `1A950B7A...`.

Not verified: how it sounds (I can't listen), the real daemon speaking a long reply, other machines.
