# Atlas as a full offline assistant that keeps the PC usable
Last updated: 2026-10-20 · Updated by: Claude (research session) · Status: research and design — nothing built, nothing downloaded

## Eric's requirement (20 Oct, his words)
> "I need a full Ai system that I can type to or vocally chat with that has the ability to run locally offline without causing me to not be able to use my computer. And that is a key thing. Now it needs to be fully capable, fast, efficient, and accurate."

That document's terms-of-service, licence and cost findings still stand. Read it for those: [OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md](OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md).

## How to read numbers
Every number in this document carries one of these tags.
- **measured**: measured on this laptop, today (20 Oct 2026).
- **recorded**: taken from a dated Atlas note. The note is named.
- **sourced**: taken from a web page. The URL is given.
- **estimate**: worked out by reasoning from other numbers. Not tested.
- **unverified**: seen somewhere, not checked. Treat as a guess.

If a number has no tag, that is a mistake. Report it.

## 1. What this laptop has today
### Hardware (measured)
| Item | Value | Tag |
|---|---|---|
| CPU | Core Ultra 7 256V, 8 cores, 8 threads | measured |
| Memory | 16 GB LPDDR5X-8533, shared with the GPU | measured |
| GPU and NPU | Arc 140V (uses the same memory); Intel AI Boost NPU | measured |
| Free disk | 118 GB | measured |

### Memory speed (measured)
I wrote a small Rust test that reads a 768 MB buffer. It was a throwaway measurement; the test lives in a temp folder. Nothing in Atlas was built or changed.

| Threads | CPU read speed | Tag |
|---|---|---|
| 8 | 40.8 GB/s | measured |
| 4 | 27.6 GB/s | measured |
| 2 | 16.3 GB/s | measured |

The GPU's peak is about 137 GB/s (sourced: https://www.notebookcheck.net/Intel-Arc-Graphics-140V-Benchmarks-and-Specs.854991.0.html).

Why this matters: a model writes one word at a time. Each word needs the whole model read from memory. So memory speed sets the top speed.

### RAM right now, with Atlas not running (measured)
| Item | Value | Tag |
|---|---|---|
| Free RAM (earlier today) | 3.2 GB of 15.7 GB | measured |
| Available RAM (Windows figure, later check) | 2.06 GB | measured |
| Commit | 21.8 of 37.7 GB | measured |

Biggest users, by private memory:

| Program | Private memory | Tag |
|---|---|---|
| Hyper-V VM (vmmem) | 4.1 GB | measured |
| Claude desktop | 2.0 GB | measured |
| WebView2 windows | 1.8 GB | measured |
| ChatGPT | 1.6 GB | measured |
| Node | 1.4 GB | measured |
| Dropbox | 0.8 GB | measured |

The owner of the vmmem VM is not confirmed. It is likely the Claude desktop workspace VM. That is a guess (unverified).

### Atlas with no model loaded (measured)
Test: the daemon in a disposable profile (`verification/runs/20261020T040916Z-claude-typed-acceptance/atlas-home-3`, not committed), watched for 60 s. A throwaway measurement; nothing in Atlas was built or changed.

| Item | Value | Tag |
|---|---|---|
| Daemon | 32 MB | measured |
| Helper | 37 MB | measured |
| Total | about 70 MB | measured |
| CPU while idle | about 0.05 of one core | measured |

So Atlas itself is cheap. The model and the voice are what cost memory.

### What is installed and what is not (measured)
| Present | Not installed |
|---|---|
| llama.cpp b10456 tools, including llama-bench and llama-server (Vulkan) | Any language model |
| Parakeet runtime | Kokoro model |
| sherpa-onnx tools | Piper program |
| Silero VAD | |
| Piper "amy" voice file | |

## 2. What earlier Atlas notes already tell us
### Speed of models already tried (recorded)
Source: `atlas/src/deepbrain.rs:4-12`, 30 Sep 2026, llama.cpp Vulkan b10456, about 450-token prompt (recorded).

| Model | Reads (prompt) | Writes (reply) | Tag |
|---|---|---|---|
| Qwen3-VL 4B Q4_K_M | 334 tok/s | 15-23 tok/s | recorded |
| Qwen3.5 4B Q4_K_M | 138 tok/s | 18-24 tok/s | recorded |
| Qwen3.5 9B IQ4_XS | 80 tok/s | 9-12 tok/s | recorded |

Writing at 18-24 tok/s from a file of roughly 3 GB implies about 60-70 GB/s effective on the GPU (estimate). That is about half the GPU's peak (sourced, above). It is believable.

### Problems seen on 12 Oct (recorded)
Source: `06_AUDIT_AND_GAPS/INVESTIGATIONS_2026-10-12.md` (recorded).

| Finding | Value | Tag |
|---|---|---|
| Talking model server memory | about 4.6 GB | recorded |
| Atlas total at boot | about 5.7 GB | recorded |
| Free RAM left after boot | 0.3 GB | recorded |
| Fixed cost per voice turn | 3.5-5 s | recorded |
| Of which endpointing | 380-1,100 ms | recorded |
| Of which Parakeet | 2.6-3.5 s | recorded |
| Worst end-of-speech to first audio | 25.3 s | recorded |
| Cold model load | 72.3 s | recorded |

The Parakeet cost came from starting it fresh for every utterance (recorded). That is fixable by keeping it running.

### Other recorded facts
| Fact | Tag |
|---|---|
| Draft-model speculation made writing slower: 5.7 vs 22.2 tok/s (`models.rs:353-359`) | recorded |
| Wake word: 60 of 63 hits, 0 of 167 false wakes (`kws.rs:22-25`) | recorded |
| Kokoro is never unloaded; it holds about 200-300 MB | recorded |
| The phrase table answers 162 commands with no model | recorded |
| Atlas already ties llama-server to a job object (`models.rs:981`) | recorded |

## 3. Design
### 3.1 Resource budget (proposed hard rules)
These are rules, not measurements. The numbers are targets.

| Rule | Target |
|---|---|
| Floor rule | A model or voice set loads only if available RAM minus what it adds is still 2 GB or more. Available RAM is read at load time. |
| Normal hard cap | Atlas total 4.5 GB or less, enforced by a job object. The deep tier is the only exception, and only when the floor rule holds. |
| Idle | Atlas core plus wake word is about 0.1 GB, so the idle target is 0.3 GB or less (estimate). Voice models load on the wake word or when a chat opens, and unload after an idle period (proposed 10 min, estimate). This is load-on-demand; no feature is hidden. |
| CPU | Atlas's own work at most 4 threads. EcoQoS for background jobs. Normal priority only during the turn you are in. |
| GPU | Model on the GPU so the CPU stays free |

Every decision (load, unload, step down) is logged with its reason and the measured available RAM. This follows your rule that unproven is broken.

#### Component sheet
All figures are estimates unless tagged. Real values are measured in P1.

| Component | Memory |
|---|---|
| Atlas core | 0.07 GB (measured) |
| Wake word plus VAD | about 0.03 GB |
| Parakeet int8 resident | about 0.6-0.8 GB (unverified) |
| Kokoro | about 0.2-0.3 GB (recorded) |
| Piper amy | about 0.1 GB |
| Whisper base.en (light listening) | about 0.3 GB (148 MB file, recorded `getpieces.rs`) |
| Embeddings | about 0.09 GB |
| 1.7B Q4 plus KV q8_0 at 4K | about 1.3 GB |
| 1.7B at 2K context | about 1.2 GB |
| 4B Q4 plus KV q8_0 at 8K plus overhead | about 3.3 GB |
| 9B IQ4_XS plus KV | about 6.3 GB |

Full voice set (Parakeet + Kokoro + VAD) is about 1.0 GB. Light voice set (whisper base.en + Piper) is about 0.4 GB.

The 12 Oct record of about 4.6 GB (recorded) is not comparable. That server ran Qwen3-VL-4B with a 0.45 GB vision projector and 2 slots (recorded). The real figure must be measured in P1.

#### Your choices that would help
These are yours to make. Atlas will not do them.
- Close the Claude and ChatGPT desktop apps while Atlas runs. That frees about 3.6 GB (measured: 2.0 + 1.6).
- Check what the 4.1 GB VM is (measured). Atlas should not guess.

### 3.2 How Atlas will watch memory (sourced APIs)
| Need | Windows tool | Source |
|---|---|---|
| Run background work gently | EcoQoS: `SetProcessInformation(ProcessPowerThrottling, EXECUTION_SPEED)` | https://learn.microsoft.com/en-us/windows/win32/procthread/quality-of-service |
| Know when memory is short | `CreateMemoryResourceNotification` and `QueryMemoryResourceNotification` | https://learn.microsoft.com/en-us/windows/win32/api/memoryapi/nf-memoryapi-creatememoryresourcenotification |
| Cap memory and CPU | Job objects: `SetInformationJobObject`, CPU-rate control (class 15), extended limits (class 9) | https://learn.microsoft.com/en-us/windows/win32/api/jobapi2/nf-jobapi2-setinformationjobobject |

Limits to know:
- The memory signal is system-wide. It fires only at the extremes (sourced). So Atlas must also read available RAM itself on a timer.
- The exact flag names for the job object are unverified. Check them when building.
- llama-server flags verified in its README (sourced: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md): `--prio N` (-1 low to 3 realtime), `--poll N`, `-t`/`--threads`, `-ngl` (layers on the GPU), `--lora`, `-lm`/`--load-mode` (mlock, or mmap plus mlock).
- Use `--prio -1` and fewer threads. Full GPU offload keeps the CPU free for you.

### 3.3 Model tiers and what loads when (recommended)
| Tier | What | Speed |
|---|---|---|
| 0 | Phrase table and deterministic tools; 0 extra memory | instant |
| 1 | Main model: Qwen3-4B-Instruct-2507 Q4_K_M (or Qwen3.5-4B if it wins the bench; it is multimodal but bigger). KV q8_0 at 4-8K. About 3.3 GB with overhead (estimate). | about 18-24 tok/s, estimated by analogy with Qwen3.5-4B (recorded) |
| 1-small | A 1.7B Q4 with KV q8_0 at 4K, or 2K when tight: about 1.3 GB or 1.2 GB (estimate). To be benched. | about 35-45 tok/s (estimate, about 2x the 4B) |
| 2 | Deep model on request: Qwen3.5-9B IQ4_XS. 5.5 GB file (recorded, `getpieces.rs`); about 6.3 GB with KV (estimate). | 9-12 tok/s (recorded) |
| 3 | Online backup, labelled every time, kept on as Eric wants | n/a |

Speculative decoding is not used: it measured slower (recorded). Granite-4.0-H-Tiny stays an experiment to bench.

Atlas picks from measured available RAM each time a model or voice set loads. Every row also obeys the floor rule.

| Available RAM | Atlas loads | Atlas total (estimate) |
|---|---|---|
| 6.5 GB or more | 4B plus full voice | about 4.4 GB |
| 4.5-6.5 GB | 1.7B plus full voice | about 2.4 GB |
| 3.7-4.5 GB | 1.7B at 2K context plus light voice | about 1.7 GB |
| 2.5-3.7 GB | Light voice plus labelled online replies | about 0.5 GB |
| Under 2.5 GB | See below | |

Under 2.5 GB, nothing is turned off. Atlas says it cannot add an offline model or voice without slowing the PC, names the apps using memory, and offers three choices:
- Online replies (labelled).
- Go ahead anyway (Eric's choice, which breaks the 2 GB floor).
- Close something.

The wake word stays resident throughout.

Today: available RAM was 2.06 GB (measured, Atlas not running). That is the bottom row. With the current apps open, offline replies need memory freed first. Closing the Claude and ChatGPT apps (about 3.6 GB measured) would move the laptop to the 4.5-6.5 GB row (estimate: 2.06 + 3.6 = about 5.7 GB).

#### Deep tier (9B)
Only with 9.5 GB or more available (estimate: about 7.4 GB with full voice, plus the 2 GB floor). It swaps with tier 1: the 4B is unloaded first, and the two never run together. With full voice the total is about 7.4 GB (estimate), above the normal 4.5 GB cap. That is the one allowed exception, and only when the floor rule holds.

Swap time is unmeasured. The recorded cold load was 72.3 s for the 4.6 GB server (recorded). Atlas says "loading the deeper model, about a minute" (estimate).

#### Speed ceiling
Top speed is memory bandwidth divided by bytes read per token (estimate).

| Case | Result | Tag |
|---|---|---|
| 4B Q4 on the GPU | about 18-24 tok/s is in line with the ceiling (Qwen3.5-4B measured it) | recorded |
| 4B Q4 on the CPU, 4 threads (27.6 GB/s) | ceiling about 9-11 tok/s | estimate |
| 1.7B Q4 on the GPU | about 35-45 tok/s, roughly 2x the 4B | estimate |

So the GPU is the right place for the model. It is faster, and it leaves your CPU free.

### 3.4 Candidate models
| Model | Licence | Size | Scores | Notes |
|---|---|---|---|---|
| Qwen3-4B-Instruct-2507 | Apache-2.0 per aggregators (confirm on the card) (unverified) | 4B dense. Q4_K_M about 2.5 GB (estimate) | MMLU-Pro 69.6, GPQA 62.0, IFEval 83.4, BFCL-v3 61.9 (sourced) | https://huggingface.co/Qwen/Qwen3-4B-Instruct-2507 |
| Qwen3.5-4B | Apache-2.0 per third parties (unverified) | about 4B, vision-language. Q6_K 3.81 GB (sourced: https://huggingface.co/bartowski/Qwen_Qwen3.5-4B-GGUF). Q4_K_M about 2.7-3 GB (estimate) | MMLU-Pro 79.1, GPQA Diamond 76.2 (self-reported, sourced) | https://huggingface.co/Qwen/Qwen3.5-4B. Atlas already measured it at 18-24 tok/s (recorded). The vision projector adds RAM. |
| Qwen3.5 9B IQ4_XS | not checked | 5.5 GB (recorded, `getpieces.rs`) | not checked | Deep tier. Writes 9-12 tok/s (recorded). |
| Granite-4.0-H-Tiny | Apache-2.0 (sourced) | 7B total, 1B active (MoE). Q4 about 4 GB (estimate) | not checked | https://www.ibm.com/new/announcements/ibm-granite-4-0-hyper-efficient-high-performance-hybrid-models. Fast decode, large file. |
| Phi-4-mini 3.8B | MIT (unverified) | not checked | MMLU 67.3, GSM8K 88.6 (sourced) | https://arxiv.org/pdf/2503.01743 |
| SmolLM3-3B | Apache-2.0 (secondary source, unverified) | not checked | not checked | Not researched further |
| Gemma 3n E2B / E4B | Gemma terms (Decision 6) | Google says about 2 GB / 3 GB RAM minimum (sourced) | not checked | https://ai.google.dev/gemma/docs/gemma-3n |
| LFM2-8B-A1B | Custom LFM licence | not checked | not checked | Excluded |
| Qwen3-30B-A3B | not checked | about 17 GB (unverified) | not checked | Too big |
| gpt-oss-20b | not checked | about 12-13 GB (unverified) | not checked | Too big |

The two Qwen scores are not comparable head to head. The Qwen3.5-4B scores are self-reported (sourced). Bench both on Eric's own question sets before choosing.

### 3.5 Shrinking the model (quantization)
- Q4_K_M loses the least among the 4-bit options.
- On a 7B model, perplexity (lower is better) is Q4_K_M 7.5692, Q3_K_M 7.6854, IQ3_M 7.7695 (sourced: https://beebopkim.github.io/2024/03/09/Benchmarks-for-lots-of-quantization-types-in-llama-cpp/).
- Small models degrade more than big ones when shrunk (estimate).
- The KV cache (the model's short-term memory) for Qwen3-4B at 8K context is about 1.2 GB at fp16 and about 0.6 GB at q8_0 (estimate).
- Atlas already uses q8_0 KV (recorded).
- Whether q8_0 KV hurts answer quality is an open question.

### 3.6 Router mode
llama-server can manage several models itself. The flags are `--models-dir`, `--models-preset` and `--models-max` (sourced: the README above). The default model count is unverified.

This could do the tier 1 / tier 2 swap for us. Test it before relying on it.

## 4. Voice loop
### 4.1 The chain
Wake word (resident, about 18 MB, recorded) then Silero VAD, then resident Parakeet, then the LLM's first sentence, then Kokoro streaming sentence by sentence. Piper "amy" is the fast fallback.

### 4.2 Target
End of speech to first audio: p50 1.2 s or less, p95 2.5 s or less (proposed; same as the Scorecard).

For comparison, the 12 Oct run saw 25.3 s at worst (recorded).

### 4.3 Rough time budget (estimate)
| Step | Time | Tag |
|---|---|---|
| Endpoint (deciding you stopped) | 300-500 ms | estimate |
| ASR, resident | 300-600 ms | estimate |
| LLM first sentence | 400-800 ms | estimate |
| TTS first audio | 200-400 ms | estimate |
| Total | 1.2-2.3 s | estimate |

The sum is the four rows added together: the forecast is 1.2-2.3 s (estimate). The p50 target of 1.2 s or less needs a resident ASR and endpoint tuning, measured in P1. Measure before promising it.

### 4.4 Voice parts
| Part | Licence and facts | Source |
|---|---|---|
| Parakeet TDT 0.6B v2 | CC-BY-4.0, attribution required. Open ASR WER 6.05%. The card says it needs at least about 2 GB RAM to load the full model. Atlas uses an int8 build, 482 MB on disk (recorded, `getpieces.rs`). Real RAM is unverified. | https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2 |
| Kokoro-82M | Apache-2.0. Warm real-time factor about 0.16 on an M3 Pro and 0.57 with ONNX on an M1 Max (third-party, unverified). No Lunar Lake numbers exist. | https://github.com/hexgrad/kokoro |
| Piper | Atlas uses the original MIT Piper. Its archived status is unverified. The successor `OHF-Voice/piper1-gpl` is GPL-3.0, so do not adopt it (Decision 5). Voice files have their own licences. The amy model card is unchecked. | https://github.com/OHF-Voice/piper1-gpl |
| Silero VAD | MIT (per the brief, unverified) | none |
| Wake word | 60 of 63 hits, 0 of 167 false wakes (recorded, `kws.rs:22-25`) | recorded |

Keeping Parakeet resident should remove most of the 2.6-3.5 s per-utterance cost (estimate; measure it).

Voice memory (estimate, unverified until measured): the full voice set (Parakeet + Kokoro + VAD) is about 1.0 GB. The light set (whisper base.en + Piper) is about 0.4 GB. See the component sheet in 3.1.

Open voice options (not researched): Moonshine, Whisper turbo, Canary, KittenTTS, Kyutai, Supertonic. All unverified. Listed so they are not forgotten.

## 5. Accuracy boosters (low memory)
A small model gets more reliable when it does less guessing.

| Booster | What it does |
|---|---|
| Local memory and retrieval over Eric's notes | Uses existing BM25 plus embeddings (about 90 MB MiniLM, unverified). Switch to rank fusion. |
| Deterministic tools | Maths, dates, files and calendar are computed, not guessed |
| Grammar or JSON-constrained output | Tool calls always come out well formed |
| Ask before acting | No surprise actions |
| Say "I'm not sure" | Then offer to try online |
| Verification step | Check an answer against the source note before speaking it |

These cost little memory. They are where a small model can feel bigger than it is.

## 6. Built in-house on top
- LoRA adapters on the Apache-2.0 base model.
- Trained on Eric's consented examples plus an open-weight teacher model: Qwen3-235B-A22B (Apache-2.0, see the other document). Its output terms still need checking.
- Not trained on Claude or ChatGPT outputs. Their terms bar it. See the other document: [OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md](OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md).
- Cost: about $20-60 per experiment (estimate).
- Adapters are small and load with `--lora` (sourced: the llama-server README above).

## 7. Honest ceiling
### Where it matches or beats online services
- Speed for short turns
- Privacy
- Cost
- Works offline
- Atlas commands
- Eric's own notes

### Where it is good
Everyday chat, writing help, summaries and simple reasoning. For scale, Qwen3-4B-2507 scores MMLU-Pro 69.6 and IFEval 83.4 (sourced).

### Where it is clearly behind Claude and ChatGPT
- Hard multi-step reasoning
- Long documents
- Big coding tasks
- Broad and current knowledge
- Long agentic work

No verified Arena gap number exists. So the size of the gap is not known. The online backup helps exactly in these cases.

## 8. What to download to benchmark fully (Eric approves)
Nothing is downloaded yet. This is the list to approve.

| Item | Size | Tag |
|---|---|---|
| Core: Qwen3-4B-Instruct-2507 Q4_K_M | about 2.5 GB | estimate |
| Core: Qwen3-1.7B Q4 | about 1.1 GB | estimate |
| Core: Parakeet int8 | 0.48 GB | recorded |
| Core: whisper base.en | 0.15 GB | recorded |
| Core: Kokoro model | size unverified; pinned in `getpieces.rs` | unverified |
| Optional: Qwen3.5-4B Q4_K_M | about 3 GB | estimate |
| Optional: Qwen3.5-9B IQ4_XS | 5.5 GB | recorded |
| Optional: Granite-4.0-H-Tiny Q4 | about 4 GB | estimate |

The core set is about 4.3 GB plus Kokoro (estimate). 118 GB is free (measured).

## 9. Phased plan
| Phase | Work | Needs approval |
|---|---|---|
| P0 | No download. Golden sets: intent 200, everyday Q&A 100, 30 recorded voice phrases (all targets, proposed). Write the governor rules. The measurements in this document are already done. | no |
| P1 | Core download. Run `llama-bench` on each candidate, Vulkan vs CPU with 4 threads. Measure RAM at 4K and 8K context. Time the voice loop. Pick the tier 1 model. | yes, the download |
| P2 | Governor: caps, idle unload, step-down, EcoQoS, priority. Resident ASR. Streaming TTS. | yes, code change |
| P3 | Accuracy boosters | yes, code change |
| P4 | Deep-tier swap and online labelling | yes, code change |
| P5 | In-house adapters | yes |

Pass test for P2: A real workday where available RAM never drops below 2 GB and you can keep working. If that fails, P2 is not done.

Feature freeze: P2 onward changes Atlas code. The feature freeze applies (`docs/EXECUTION_EFFICIENCY_PLAN.md`). Eric decides whether this work is allowed through.

## 10. Expected result (estimate)
| Item | Expected | Before |
|---|---|---|
| Atlas memory while you talk to it | about 4.4 GB with the 4B and full voice; less when RAM is tight (estimate, section 3.3) | about 5.7 GB always (recorded) |
| Atlas memory when idle | 0.3 GB or less (estimate) | about 5.7 GB always (recorded) |
| Typed reply starts | about 0.5-1 s (estimate) | not measured |
| Typed reply speed | 4B: about 18-24 tok/s (estimate, by analogy with Qwen3.5-4B, recorded). 1.7B: about 35-45 tok/s (estimate) | same |
| Spoken reply starts | about 1.2-2.3 s (estimate). The p50 target of 1.2 s or less needs resident ASR and endpoint tuning, measured in P1. | 25.3 s worst seen (recorded) |
| Quality | Strong small-model level, with online for the hard cases | n/a |

All of the "expected" column is a forecast until P1 and P2 prove it.

## 11. Open questions
| Question | Why it matters |
|---|---|
| Licence of Qwen3-4B-2507 and Qwen3.5-4B on the model cards | Only aggregators say Apache-2.0 |
| Real Parakeet and Kokoro RAM and latency on the 256V | The voice budget is a guess |
| Vulkan vs SYCL | Could change GPU speed |
| Router-mode defaults | Decides how tier swaps work |
| KV-quant quality | q8_0 may cost accuracy |
| Whose 4.1 GB VM it is | Could free a lot of RAM |
| Amy voice licence | Model card is unchecked |
| Job-object flag names | Needed for the hard cap |
| Real RAM of every item in the component sheet | The sheet is an estimate |
| Model swap time | Unmeasured |
