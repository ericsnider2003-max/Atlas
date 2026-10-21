# Core models: download, wiring and benchmark (2026-10-21)

Owner decision (Eric, verbatim): "stick to smaller one for now". The 16.2 GB 8-piece ask was declined and the smaller core set approved. Machine: Core Ultra 7 256V, 16 GB, Intel Arc 140V (Vulkan, shared memory), llama.cpp build b10456 in `tools\llama`. Real profile: `%LOCALAPPDATA%\Atlas`. Atlas itself was never started as a daemon; it is stopped (tasklist at the end: no atlas, llama-server, whisper-server, sherpa or curl left).

## 1. The decline and the approval (real profile, `data/state`)

Done through the code, not by hand-editing JSON: a one-off example binary (`examples/core_decision.rs`, since deleted; a copy is in the scratchpad `core/core_decision.rs.txt`) called `getpieces::Approved::grant`, the new `Approved::decline`, and `Backlog::dismiss`, with `ATLAS_HOME` set to the real profile. `backlog.json` was copied to the scratchpad `state_backup/` first.

- `pieces_approved` now holds 6 granted keys: Qwen3VL-4B gguf, Qwen3-1.7B gguf, the sherpa hearing server, Parakeet, Kokoro engine, Kokoro model.
- `declined` holds 7 keys, never asked about again and never fetched: wake-word spotter (17,626,723 B), multilingual listening model ggml-base.bin (147,951,465), deep brain Qwen3.5-9B IQ4_XS (5,501,202,464), picture model z_image_turbo (3,683,370,944), its text encoder (2,497,281,120), its decoder ae.safetensors (335,304,388), coding model qwen2.5-coder-7b (4,683,073,536). Parakeet, the 8th piece of the old ask, is approved.
- Backlog item 2 (the "Atlas can fetch 8 missing pieces (about 16.2 GB)" ask) is `dismissed: true`.
- Code change so the no sticks. Root cause: a dropped ask was only a no "until the list of missing pieces changes", and approving Parakeet changes that list, so a fresh ask of about 15.7 GB would have been filed. Now: `Approved.declined`, `Approved::decline/declines`, and `decide_top_up_and_finish` skips declined pieces; a later grant replaces an earlier no. CAVEAT: the installed `atlas.exe` is an older build that does not know the `declined` field; it ignores it and could ask again for the other pieces until it is rebuilt with this change.

## 2. Pieces (pinned in `src/getpieces.rs` / `src/kokoro.rs`; fetched by `atlas get core`, which uses Atlas's own verify-and-land path, run at BelowNormal priority)

| Piece | Source | Bytes | SHA-256 | Landed |
|---|---|---|---|---|
| 4B talk model Qwen3VL-4B-Instruct Q4_K_M | huggingface.co/Qwen/Qwen3-VL-4B-Instruct-GGUF | 2,497,281,664 | 66358cb18bb6b3b1b6675aa412c7a88ef01d228f481184d13668e5201c730a0a (re-hashed with certutil afterwards: match) | `models/Qwen3VL-4B-Instruct-Q4_K_M.gguf` |
| 1.7B small model Qwen3-1.7B Q8_0 (NEW pin) | huggingface.co/Qwen/Qwen3-1.7B-GGUF @ 90862c4b9d2787eaed51d12237eafdfe7c5f6077 | 1,834,426,016 | 061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a (HF LFS record; certutil: match) | `models/Qwen3-1.7B-Q8_0.gguf` |
| Parakeet TDT 0.6B v2 int8 | github k2-fsa/sherpa-onnx asr-models | 482,468,385 | 157c157bc51155e03e37d2466522a3a737dd9c72bb25f36eb18912964161e1ad (verified by Atlas before unpacking; marker `.atlas-piece` holds it) | `models/parakeet/` (encoder, decoder, joiner int8; tokens) |
| Kokoro v1.0 int8 | github k2-fsa/sherpa-onnx tts-models | 132,303,094 | 4c3052abaa60943a341f193888cf6abd68787dae6ab8ae5c925a706caa247e4e (marker present) | `models/kokoro/model.int8.onnx` (114,203,756 B unpacked) |
| sherpa server, Kokoro runtime, whisper base.en | already on disk | | | `tools/sherpa`, `tools/kokoro`, `models/ggml-base.en*.bin` |

The official `Qwen/Qwen3-1.7B-GGUF` repo publishes only Q8_0 (no Q4_K_M), 1.83 GB (under the 1.9 GB limit), so Q8_0 is what was pinned, not the design's roughly 1.1 GB Q4. The core set as pinned totals 4,977,783,967 B (about 5.0 GB, not the design's 4.3 GB); `atlas get core` now says about 5 GB. "The 4B" is the shipped Qwen3-VL-4B; the design names Qwen3-4B-Instruct-2507, which is not shipped and was not fetched.

Download notes: the network sustained about 1.0-1.2 MB/s in total (two parallel streams each got half, so parallelism did not help); the whole fetch took about 80 minutes. A stale 510,505,505-byte `the-Parakeet-hearing-model.part` left from 20 Oct (larger than the real 482,468,385-byte asset, confirmed by a HEAD request) made the first Parakeet attempt "finish" instantly, fail the hash and be deleted; the retry fetched cleanly. Disk: 20 GB free before, 6.7 GB after (drive C: is 100% full, partly from other use).

## 3. Tier table as implemented (`src/models.rs`: `memory_tier`, `pick_by_tier`, `tuned_for`)

Applies only when both the 4B and the 1.7B are in the models folder, nothing is named in `prefer`, `talk` is faster or empty, and no memory limit is set. Decided on available RAM at pick time (`Machine::free_ram_mb`).

| Available RAM | Loads | Context |
|---|---|---|
| 6,656 MB (6.5 GB) or more | 4B (the ordinary pick; the 8B only where the machine has room) | configured (8192 default) |
| 4,608-6,655 MB | 1.7B | 4096 |
| 3,789-4,607 MB | 1.7B, only if it leaves the PC 2,048 MB | 2048 |
| below that, or when the floor check fails | no model (the floor: phrase table, plain tools, labelled online replies) | |

The floor check uses `estimate_memory * 1.2`, because `estimate_memory` came out 14-19% under the measured working sets below. With the Q8_0 1.7B the tight tier therefore only really exists for 4,473-4,607 MB; a Q4 1.7B would restore the design's 3.7 GB rung. `daemon/model.rs` applies the tier's context when it starts the server (`tuned_for`), and `daemon/floor.rs` sizes its footprint with it. Unit tests with injected free memory: 12,000 and 6,700 give the 4B; 6,600, 5,000 and 4,608 give the 1.7B; 4,500 gives the 1.7B at 2K; 4,400, 3,900, 2,500 and 500 give no model; a named model, a memory limit and a missing small model leave the ordinary pick alone.

## 4. Benchmarks (this laptop, shipped tools only)

The laptop was shared with other work during the runs (other sessions compiling Rust, browsers), and numbers moved run to run. Two priority settings are shown for llama-bench because the variance was large. Threads: 4 (leaves 4 cores). Vulkan = Intel Arc 140V, `-ngl 99`, flash attention, KV q8_0. CPU = `--device none`.

llama-bench, tokens per second (run A BelowNormal priority, run B Normal priority):

| Model | Backend | pp256 A | tg64 A | pp256 B | tg64 B | llama-bench peak private |
|---|---|---|---|---|---|---|
| 4B Q4_K_M | CPU | 66.6 | 14.6 | 36.6 | 13.1 | 2,023 MB (plus the mmap'd 2.3 GiB file) |
| 4B Q4_K_M | Vulkan | 934.5 | 27.0 | 791.3 | 22.3 | 3,040-3,069 MB |
| 1.7B Q8_0 | CPU | 118.7 | 25.5 | 32.3 | 19.4 | 305 MB (plus the mmap'd 1.7 GiB file) |
| 1.7B Q8_0 | Vulkan | 2,355 | 36.7 | 1,829 | 35.4 | 2,391-2,444 MB |

llama-server with the exact arguments `models::server_args` builds (`-np 2 --kv-unified --cache-reuse 256 --cache-ram 256 -ctk/-ctv q8_0 --jinja`, Vulkan `-ngl 999`, thinking off as `chat_body` sends it), one short chat, 3 requests, 21-25 token prompt. Peak working set and private bytes sampled every 0.3 s from start to stop. Ranges are the BelowNormal and Normal priority runs together:

| Model | Context | Load to /health | Idle WS | Peak WS | Peak private | Generation tok/s | Min available during |
|---|---|---|---|---|---|---|---|
| 4B | 8192 (current default) | 2.3-6.6 s | 3,542 MB | 3,588-3,595 MB | 3,647-3,649 MB | 21.4-29.1 | 2,462-2,843 MB (starting from 6,040-6,495) |
| 4B | 4096 | 1.9-2.6 s | 3,228 MB | 3,275 MB | 3,333 MB | 21.8-29.8 | 2,765-3,275 MB (from 6,097-6,595) |
| 1.7B | 4096 | 1.9-4.4 s | 2,477 MB | 2,517 MB | 2,560 MB | 30.2-37.3 | 3,554-3,933 MB (from 6,109-6,501) |
| 1.7B | 2048 | 1.8-1.9 s | 2,353 MB | 2,393 MB | 2,436 MB | 31.7-36.9 | 3,642-4,061 MB (from 6,080-6,484) |

Whole-request time for a 21-25 token prompt and 30-96 generated tokens: 4B 2.1-3.4 s, 1.7B 0.84-1.7 s. Replies were sensible (4B: "A laptop's memory, typically referring to RAM ..."). Note found while benchmarking: Qwen3-1.7B is a thinking model and answered with an empty reply (all 96 tokens spent thinking) until `chat_template_kwargs.enable_thinking=false` was sent; `models::chat_body` already sends that for every call, so Atlas is fine, but any raw use of the 1.7B must too.

Voice loop (Normal priority, the same 20 synthetic command clips u01-u10, n01-n10):

| Piece | Result |
|---|---|
| Parakeet int8 via `sherpa-onnx-offline`, 2 threads, one process per clip exactly as `parakeet.rs` runs it (includes model load) | p50 2,273 ms, p95 2,456 ms, min 2,163, max 2,825 (40 runs); peak private 711 MB; the first four clips transcribed correctly ("What time is it?", "Open Notepad.", "Set a timer for five minutes.", "Quit Atlas") |
| whisper base.en q5_1 via `whisper-server` (60 requests) | p50 1,433 ms, p95 1,483 ms; WER 7.1% (u clips), 2.4% (n clips); 122 MB WS / 651 MB private after load. An earlier run of the same script in quieter conditions gave p50 686 ms; one with other builds loading the machine gave p50 1,509 ms |
| Kokoro (`atlas kokoro-check`, af_heart, 4 threads), one 3.37 s sentence | load 1.5 s; synthesis 5.7-6.9 s at Normal priority (1.7-2.05x the speech length, slower than real time); at BelowNormal 14.3-70.4 s (4.3-20.9x); peak private 360 MB |

Kokoro on this CPU does not keep up with real time under load. Parakeet (2.3 s) plus a 1.7B reply (about 1 s) plus Kokoro (about 6 s for a 3-second reply) makes the voice loop roughly 9 s before any streaming. Whisper q5_1 is faster than Parakeet per clip here but less accurate (3 of 42 words wrong on the u clips).

## 5. Free memory and PC usability

`\Memory\Available MBytes` was sampled every 1-2 s throughout (one CSV per phase in the scratchpad `core/`).

- Starting from 5.5-6.6 GB available, loading the 4B (Vulkan server) took the minimum to 2,462-2,843 MB at 8K context and 2,765-3,275 MB at 4K. The 2,048 MB floor held. This bears out the design's 6.5 GB rule: a 4B load takes about 3.6-3.7 GB.
- The 1.7B at 4K or 2K kept 3,554-4,061 MB free.
- ONE real violation: `llama-bench` of the 4B on CPU only (the mmap'd 2.3 GiB file plus 2.0 GB private) starting from 5,476 MB available fell to 1,443 MB (run A, starting from 6,293, fell to 2,290). A CPU-only 4B at 5.5 GB available breaks the floor; the tier table sends that case to the 1.7B.
- Over the whole 85-minute download and benchmark window the minimum was 322 MB at 09:41, and 165 samples were under 2,048 MB, in episodes at 09:05, 09:11-09:13, 09:22, 09:40-09:42 and 10:25. The 09:40-09:42 and 09:11-09:13 dips coincided with Rust test compiles (mine at BelowNormal `-j 2`, plus other sessions' rustc processes of 1.3-2.6 GB each running at the same time), not with the model, hearing or voice pieces; the 10:25 dip is the CPU-only 4B llama-bench above. The curl download itself held 9 MB. The evidence cannot separate my compile from the other sessions', so it is reported rather than excused.
- Parakeet (minimum 5,217 MB), whisper (6,387) and Kokoro (4,797 and 5,143) never went near the floor.

Method: llama-server was started on 127.0.0.1 only and killed after each run; nothing was started on the real profile as a daemon; no real button was pressed. Scripts are in the scratchpad `core/` (`phase.ps1`, `sampler.ps1`, `lb.ps1`, `server_bench.ps1`, `pk.ps1`, `kok.ps1`).

## 6. Tests

Run on the final code, `--test-threads=1`:

- lib tests `getpieces`, `models::`, `fit::`: 23 passed, 0 failed (includes `models::memory_tiers` x4 and the two new `getpieces` tests).
- `--test guards`: 5 passed (442 s); `--test name_collisions`: 5 passed (59 s); `--test one_word_is_not_an_address`: 3 passed (263 s). 0 failed.

A first guards run at BelowNormal priority was killed after it crawled (the debug-build source scan at background priority made almost no progress); the passing run is the Normal-priority one.
