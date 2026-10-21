# Building Atlas's own language model: options and recommendation
Last updated: 2026-10-20 · Updated by: Claude (research session) · Status: research — nothing built, nothing downloaded

## The question

What would it realistically take to build and own Atlas's own language model, given what Eric has?

Short answer: training a model from scratch is not realistic. A small custom adapter on top of a small open Apache-2.0 model is realistic, cheap, and keeps Eric's data, adapters and pipeline his own. One rule changes the earlier plan: Claude and ChatGPT answers must not be used as training data.

How to read numbers in this document:
- measured: taken from this laptop or from Atlas files, with the source named.
- recorded: copied from a dated note in Atlas source, not re-measured today.
- sourced: taken from a web page, with the URL.
- estimate: calculated or guessed. Treat as rough.
- unverified: stated in a source or from general knowledge but not checked in this pass.

## Eric's target: small and fully on-device (20 Oct)

Eric narrowed the goal on 20 Oct. He wants a model that:
- runs fully on the user's device,
- is built in-house,
- is efficient, and
- is small in storage and RAM, because the laptop barely copes while Atlas runs.

This section replaces the old Recommendation (kept as a short superseded note at the end).

### Where the memory goes today

Measured 2026-10-20 with Atlas NOT running: 3.2 GB free of 15.7 GB. Commit charge 21.8 GB of 37.7 GB.

| Process | Private memory | Tag |
|---|---|---|
| vmmem, a Hyper-V VM (owner not confirmed; likely the Claude desktop workspace VM) | 4.1 GB | measured 2026-10-20 |
| Claude desktop | 2.0 GB | measured 2026-10-20 |
| msedgewebview2 (54 processes) | 1.8 GB | measured 2026-10-20 |
| ChatGPT | 1.6 GB | measured 2026-10-20 |
| node | 1.4 GB | measured 2026-10-20 |
| Dropbox | 0.8 GB | measured 2026-10-20 |

When Atlas last ran it added about 5.7 GB: the talking model server about 4.6 GB plus atlas.exe 0.8-0.9 GB. Source: `06_AUDIT_AND_GAPS/INVESTIGATIONS_2026-10-12.md` (recorded). The Kokoro voice (TTS) is never unloaded and takes about 200-300 MB (same source).

### Budget proposal (estimate)

| Part | Target |
|---|---|
| Language model, resident | 1.5 GB or less |
| Atlas core | 1 GB or less |
| Atlas total | about 2.5 GB or less, versus about 5.7 GB before |

The model loads on demand and unloads after idle.

Rough file sizes at Q4_K_M, plus extra for the KV cache (the model's working memory). This is arithmetic, unverified:

| Model size | Weights at Q4_K_M |
|---|---|
| 1B | about 0.6 GB |
| 1.7B | about 1.0 GB |
| 3B | about 1.8 GB |
| 4B | about 2.4 GB |

So a 4B model does not fit the budget. A 1-1.7B model does.

### Candidates (model-card figures, sourced)

| Model | Licence | Size | Scores | Source |
|---|---|---|---|---|
| IBM Granite-4.0-H-1B | Apache-2.0 | 1-1.5B | MMLU 59.74, IFEval avg 78.53, GSM8K 69.83, BFCL v3 50.21 | https://huggingface.co/ibm-granite/granite-4.0-h-1b |
| Qwen3-1.7B | Apache-2.0 | 1.7B | BFCL v3 about 55.5 | arXiv 2511.22138, https://arxiv.org/pdf/2511.22138 |
| Microsoft BitNet b1.58 2B4T (ternary) | MIT | about 2B on the card; 0.8B in the widget, unexplained | Non-embedding memory 0.4 GB vs 2.6 GB (Qwen2.5-1.5B) and 2 GB (Llama-3.2-1B). CPU decode 29 ms vs 65 / 48 ms. Energy 0.028 J vs 0.347 / 0.258 J. MMLU 53.17, GSM8K 58.38, IFEval 53.48. | https://huggingface.co/microsoft/bitnet-b1.58-2B-4T |
| Phi-4-mini 3.8B | MIT | 3.8B | MMLU 67.3, GSM8K 88.6 | model card figures; URL not recorded in this pass |

Notes:
- BFCL is a tool-calling benchmark. Higher is better.
- BitNet needs bitnet.cpp (MIT, https://github.com/microsoft/BitNet). It reports x86 speedups of 2.37-6.17x and energy savings of 72-82%. It needs VS2022 and clang 18+. It has no Lunar Lake numbers (unverified).
- llama.cpp has TQ1_0 and TQ2_0 ternary types, but whether they fit BitNet is unverified.
- Phi-4-mini is too big for the budget.
- Gemma 3 uses Google's custom terms (Decision 6). LFM2 uses the LFM Open License (restrictions unverified). Exclude both until checked.
- xLAM-2-1B is cc-by-nc-4.0 (non-commercial research only), so exclude it. The xLAM-2-3B scores 65.74 on BFCL v3 in that paper. That shows a fine-tuned small model can lead on narrow tool calling.

### Architecture (recommended)

1. Deterministic code first. The phrase table already handles 162 commands without a model.
2. One small base (Granite-4.0-H-1B or Qwen3-1.7B, picked by a bench) plus small per-section LoRA adapters, swapped per request. Sections: intent and tool calling, writing voice, memory Q&A.
   - llama-server supports `--lora`, `--lora-init-without-apply` and a per-request `lora` field. Verified in the README: https://github.com/ggml-org/llama.cpp/blob/master/tools/server/README.md
   - Memory per adapter is unknown. Typically megabytes (unverified).
3. Load on demand, unload when idle.
4. An experimental ternary track: BitNet 2B4T via bitnet.cpp.
5. Online access for open-ended knowledge and long writing, clearly labelled, as Eric wants.
6. Speculative decoding (a small helper model guesses ahead) is not part of the plan.
   - The flags exist: `--spec-draft-model`, `--spec-draft-n-max`.
   - Atlas's own 30 Sep test found a draft model made the 4B slower: 5.7 vs 22.2 tok/s (`models.rs:353-359`, recorded).
   - Re-measure before using it.

### Slimming Atlas and the laptop (estimates; Eric decides)

- Biggest wins outside Atlas: the 4.1 GB VM, and the Claude and ChatGPT desktop apps (about 3.6 GB together) when not in use.
- Inside Atlas: the 4.6 GB model becomes about 1-1.5 GB. Unload Kokoro when idle. Do not hide any feature; load on demand instead.

### "Better than anything on the market": the honest line

- A 1-2B local model will NOT beat frontier models (Claude, ChatGPT) at open-ended reasoning, broad knowledge or long-form writing.
- It CAN beat them on Atlas's narrow, defined tasks: intent and command understanding, tool calls, short replies in Eric's voice, and recall from his own notes.
- It wins on speed (no network), privacy (zero bytes sent), cost ($0 per turn), offline availability and memory.
- Measure against the Scorecard (section 5 of `00_SOURCE_OF_TRUTH/deferred-atlas-plans/ATLAS_QUALITY_SPEED_AND_LOCAL_MODELS_PLAN_2026-10-18.md`):
  - rows 1-6: latency
  - rows 11-12: RAM
  - rows 13-15: intent (accuracy at least 95%, wrong-action at most 0.5%)
  - rows 16-18: memory recall
- Run the same golden set on three things:
  - (a) the local small model plus adapter,
  - (b) today's free online services,
  - (c) a frontier model, used only for comparison and not for training.
- "Better" for Atlas means local quality within 2 points of the frontier model, plus faster and fully private.
- Writing rows 32-34 and research answers stay online-assisted.

### What "built in-house" means for one person

| Route | What it is | Cost and time | Verdict |
|---|---|---|---|
| (i) From scratch | nanochat (MIT, https://github.com/karpathy/nanochat) on rented 8xH100. The d20 size is the "~$100" tier. A 2-hour run is about $48 at about $3/GPU-hour (estimate). | Gives GPT-2 grade. A competitive 3B costs about $0.6-0.8M (estimate). | A great learning project, not an assistant. |
| (ii) Realistic in-house | Eric's own data, Atlas's own training pipeline, Atlas's own adapters on an Apache-2.0 base. | First run about $20-60 (estimate). | Recommended. |

With route (ii), Eric owns the adapters, the data and the pipeline. He does not own the base model's copyright.

Training data:
- Claude and ChatGPT outputs are barred by their terms. Both commercial and consumer terms reportedly bar it. The consumer-terms text is a summariser's paraphrase, unverified verbatim.
- Use Eric's consented examples plus an Apache-2.0 open-weight teacher (Qwen3-235B-A22B).
- Claude and Codex may write the pipeline code.

### Recommended path

Route (ii): a small Apache-2.0 base, per-section adapters, on-demand loading, online for hard general questions, and BitNet as an experimental track. Budget: Atlas total at most about 2.5 GB (estimate).

### First small measurable step (no download needed)

1. Build the intent golden set: 200 utterances from Atlas's own `atlas/config/commands.yaml` and phrasing Eric approves.
2. Run it through today's path (phrase table plus free online services). Record accuracy, wrong-action rate and latency as Scorecard rows 13-15.
3. That result is the baseline any in-house model must beat.

Step 2, after Eric approves downloads of about 1-2 GB: bench Granite-4.0-H-1B Q4, Qwen3-1.7B Q4 and BitNet 2B4T on the laptop. Record RAM, tok/s and golden-set accuracy.

### Earlier recommendation (superseded)

The first version of this document recommended Path C on a Qwen3 4B or 8B base, with a first experiment of about $20-60 and a 4B-class GGUF download of about 2.5 GB. The direction of Path C (adapters on an Apache-2.0 base, no Claude or ChatGPT training data, a golden-set gate before use, online for hard questions, Path B only if adapters plateau, Path A not worth it) still stands. The base size changes: 4B does not fit the new memory budget, so the target is a 1-1.7B base.

## What Eric has

Hardware lines below were measured on the laptop on 2026-10-20.

| Item | Value | Tag |
|---|---|---|
| CPU | Intel Core Ultra 7 256V, 8 cores, 8 threads, 2.2 GHz base | measured 2026-10-20 |
| Memory | 16 GB LPDDR5X-8533 in 2 modules, shared with the GPU | measured 2026-10-20 |
| GPU | Intel Arc 140V (driver 32.0.101.8626) | measured 2026-10-20 |
| NPU | Intel AI Boost, status OK | measured 2026-10-20 |
| Free disk | C: drive 118 GB free | measured 2026-10-20 |
| Free RAM | 3.6 GB at one measuring (3.2 GB in the later table above) | measured 2026-10-20 |
| PyTorch | not installed | measured 2026-10-20 |
| Subscriptions | Claude and ChatGPT/Codex | stated by Eric |
| Rented GPUs | possible | stated by Eric |

No language model is installed right now. Verified 2026-10-20: no .gguf file in the user profile. Atlas currently answers through free online services. Source: `verification/runs/20261020T040916Z-claude-typed-acceptance/RESULTS.md`.

The speeds below are an older record, from a time when a model was installed. They come from an Atlas source comment dated 30 Sep 2026 (`atlas/src/deepbrain.rs:4-12`), using llama.cpp Vulkan b10456 and a prompt of about 450 tokens. They are "recorded 30 Sep 2026 in Atlas source", not measured today.

| Model | Reads (tok/s) | Writes (tok/s) | Tag |
|---|---|---|---|
| Qwen3-VL 4B Q4_K_M | 334 | 15-23 | recorded 30 Sep 2026 in Atlas source |
| Qwen3.5 4B | 138 | 18-24 | recorded 30 Sep 2026 in Atlas source |
| Qwen3.5 9B IQ4_XS | 80 | 9-12 | recorded 30 Sep 2026 in Atlas source |

"Reads" means how fast it takes in your prompt. "Writes" means how fast it produces the answer.

## What the charter asks for

From `00_SOURCE_OF_TRUTH/ATLAS_DIRECTION_AND_RECOVERY_CHARTER.md` (lines 15, 19, 65):
- Reduce dependence on paid AI.
- Online is allowed when permitted.
- The intended capability must work offline where installed models support it.

A small local model with custom adapters fits all three.

## Existing research and a correction

- The E6 "make our own models" section of `00_SOURCE_OF_TRUTH/deferred-atlas-plans/ATLAS_QUALITY_SPEED_AND_LOCAL_MODELS_PLAN_2026-10-18.md` (lines 176-188) proposed LoRA adapters on the 4B talk model, distilled from Claude/Codex.
- This document keeps the LoRA idea. It corrects the "distilled from Claude/Codex" part. See the next section.
- The replica and REA repos (github.com/Jakeschincariol/replica-skill, github.com/morluto/rea) are methods for recreating UIs and capabilities. They are not model training. They do not help here.

## Terms of service (decisive)

| Provider | What the terms say | Source |
|---|---|---|
| Anthropic | Commercial Terms section D.4 (effective 17 Jun 2025): customers may not "access the Services to build a competing product or service, including to train competing AI models". | sourced: https://www.anthropic.com/legal/commercial-terms |
| Anthropic | Section B: the customer owns its Outputs. Section D.4 still bars training competing models with them. | sourced: same page |
| Anthropic | Anthropic's help article on using outputs to train models. Treat it as confirming D.4 (snippet only). | sourced (snippet only): https://support.claude.com/en/articles/12326764-can-i-use-my-outputs-to-train-an-ai-model |
| OpenAI | May not "use Output to develop models that compete with OpenAI". From a search snippet; page not fetched verbatim. The URL is the rest-of-world terms. | sourced (snippet only): https://openai.com/policies/row-terms-of-use/ |

What this means in plain terms:
- Both companies' terms bar using their outputs to train competing models. Confirm the OpenAI wording before relying on it.
- So do not use Claude or ChatGPT answers as training data for an Atlas model.
- Claude and Codex can still write the training code, build the data pipeline, and help Eric review data. That is using them as tools, not as a teacher.
- Get a lawyer's view before any commercial release.
- Allowed teachers are open-weight models with permissive licences. Example: Qwen3-235B-A22B, Apache-2.0 (https://qwen.ai/blog?id=qwen3), run on a rented GPU.
- DeepSeek and gpt-oss licences are unverified. Do not use them as teachers until checked.

## What "owned" can mean

| Base model | Licence | Notes | Source |
|---|---|---|---|
| Qwen3 at 0.6B, 1.7B, 4B, 8B, 14B, 32B | Apache-2.0 | Fine-tuned weights can be kept, changed and shared with the licence notice. | sourced from a blog, confirm on the model card (unverified): https://www.interconnects.ai/p/qwen-3-the-new-open-standard |
| Llama 3.1 | Meta community licence | Has a 700M monthly-active-user clause and naming rules. Less clean. | sourced: https://github.com/meta-llama/llama-models/blob/main/models/llama3_1/LICENSE |
| Gemma, OLMo 2, Mistral, SmolLM | not checked | Licence texts unverified in this pass. | unverified |

With a Qwen3 base:
- Eric owns his adapter and his training data.
- Eric does not own the base model's copyright.

Training from scratch is the only path where Eric owns all the weights outright. Even then the data licences apply. FineWeb and FineWeb-Edu are ODC-By 1.0 (https://huggingface.co/datasets/HuggingFaceFW/fineweb).

## Rental prices (per GPU-hour)

| GPU | Price | Tag and source |
|---|---|---|
| H100 80GB | $2.89 (RunPod PCIe) to $3.49 | sourced: https://www.runpod.io/product/cloud-gpus and https://www.thundercompute.com/blog/runpod-pricing-vs-thunder-compute |
| A100 80GB | $1.39-1.59 | sourced: https://hackceleration.com/labs/runpod-pricing |
| RTX 4090 | about $0.34 community cloud, $0.69-0.89 secure cloud | estimate (search results, no single source) |

## Path A: train from scratch (background)

Build a new model from nothing, using public web text.

| Project | Size and cost | Tag and source |
|---|---|---|
| nanochat | GPT-2-grade chat model, about $73, about 3 h on one 8xH100 node. Original target about $100. | sourced: https://github.com/karpathy/nanochat/discussions/481 |
| TinyLlama 1.1B | 3T tokens, 16 A100s, about 90 days, about 34.6k A100-hours | sourced: https://github.com/jzhang38/tinyllama |
| SmolLM3 3B | 11T tokens, 384 H100s, 220k GPU-hours | sourced: https://x.com/eliebakouch/status/1942614640480961003 |
| SmolLM3 experiments | about 161k more GPU-hours | unverified |

Cost estimates. Method: 6 x parameters x tokens in FLOPs, at about 350 TFLOPS effective per H100. All estimates.

| Model and data | GPU-hours | Cost |
|---|---|---|
| 1B model, 20B tokens | about 100 | about $300 |
| 1B model, 3T tokens | about 14k | about $40-50k |
| 3B model, 60B tokens | about 860 | about $2.5-3k |
| Competitive 3B, 11T tokens | 220k reported; about 157k by the 6·N·D method | about $0.6-0.8M, based on SmolLM3's reported 220k GPU-hours at the H100 rates above; the 6·N·D method gives about 157k hours, about $0.45-0.55M |

Data: public web corpora such as FineWeb, in terabytes. Eric's own data is far too small for pretraining.

- Time: days to months depending on size (nanochat about 3 h, TinyLlama about 90 days).
- Ownership: Eric owns all weights, subject to data licences (ODC-By).
- Quality: a cheap from-scratch model is GPT-2 grade and useless as an assistant. Reaching a Qwen3-4B level needs hundreds of thousands of dollars and a team.
- Runs on the laptop: a small 1B model fits easily but is weak.

Verdict: not recommended.

## Path B: continued pretraining or full fine-tune of an open ~8B model (background)

Keep teaching an existing model more text, changing all its weights.

- Full fine-tune needs about 16 bytes per parameter with the Adam optimiser. This is a general rule, unverified here. That gives about 128 GB for 8B, so several 80 GB GPUs. Impossible on the laptop. (estimate)
- 1B tokens on an 8B model: about 40 H100-hours, about $116-140. (estimate)
- 10B tokens: about $1.16-1.40k. (estimate)
- Data: needs 1-10B tokens of domain text. Eric does not have this.
- Tool: torchtune (https://github.com/meta-pytorch/torchtune).
- Time: about 1-2 days of rented compute for 1B tokens, plus data preparation. (estimate)
- Ownership: an Apache-2.0 derivative. Eric does not own the base copyright.
- Quality: a modest gain on domain text, with a risk of forgetting general skills.
- Runs on the laptop: the same size as the base. An 8B at 4-bit is about 5 GB, too big for the new budget. (estimate)

Verdict: not recommended now.

## Path C: LoRA or QLoRA on a small Apache-2.0 base (revised E6; the basis of Eric's target)

A LoRA adapter is a small add-on file that nudges a model. The base model stays unchanged. QLoRA does the same with a compressed base so it fits on smaller GPUs.

Base: originally Qwen3-4B or 8B. Under Eric's 20 Oct target the base becomes a 1-1.7B model (Granite-4.0-H-1B or Qwen3-1.7B).

Training:
- Rent a GPU. QLoRA training on an RTX 4090 costs about $1.5-7 for a 4-8 h run on a 4B base. (estimate)
- VRAM for 4B QLoRA is about 8-16 GB. Sources conflict. A 1-1.7B base should need less (unverified).
  - https://aithinkerlab.com/qwen3-qlora-local-fine-tuning-guide/
  - https://kaitchup.substack.com/p/qwen3-vl-fine-tuning-on-your-computer
- Training on the laptop is unproven. No published fine-tune speeds exist for the Arc 140V. There is a reported PyTorch XPU correctness bug on Lunar Lake (https://github.com/pytorch/pytorch/issues/200713). Train locally only after a small benchmark.

Data, in order:
1. Eric's own consented examples: corrections, approved and rejected Atlas outputs, his writing.
2. Synthetic examples from an open-weight teacher (Qwen3-235B-A22B, Apache-2.0) on a rented GPU.
3. Eric reviews a sample.

A typical useful size is 1k-50k examples. (unverified)

Cost breakdown of a first experiment (all estimates):

| Step | Detail | Cost |
|---|---|---|
| Teacher run | Qwen3-235B-A22B at 4-bit needs about 130 GB, so 2 H100s at about $6-7/h. Generating about 10k examples (about 5M tokens) takes about 2-5 h. | about $15-35 |
| QLoRA training | RTX 4090, 4-8 h | about $1.5-7 |
| Total | | about $20-60 |

- Time: data collection is weeks of normal Atlas use (estimate). Training takes hours.
- Ownership: Eric owns the adapter and the data. The base is Apache-2.0.
- Quality: better than the base on Eric's own tasks, voice and Atlas commands. Not frontier-level on hard general questions.
- Runs on the laptop: base GGUF plus a small adapter. Whether llama.cpp loads LoRA adapters on the pinned build (b10456) is unverified, though the current llama-server README documents the flags.

## Comparison

| Path | Compute and cost | Time | Data and source | What "owned" means | Quality vs online services | Runs on the laptop |
|---|---|---|---|---|---|---|
| A. From scratch | $73 (GPT-2 grade, sourced) up to about $0.6-0.8M (competitive 3B, estimate) | Days to months | Terabytes of public web text. Not Eric's data. | All weights, subject to data licences (ODC-By) | Cheap tier is useless as an assistant. | A small 1B fits easily but is weak |
| B. Full fine-tune of ~8B | About $116-140 (1B tokens) to $1.16-1.40k (10B tokens). Estimate. | About 1-2 days of rented compute plus data preparation | 1-10B tokens of domain text. Eric has none. | Apache-2.0 derivative | Modest gain, risk of forgetting. | Same size as the base (too big for the budget) |
| C. LoRA/QLoRA on a small base | First experiment about $20-60. Estimate. | Data: weeks (estimate). Training: hours. | Eric's consented examples plus open-weight teacher output | Eric owns adapters and data. Base is Apache-2.0. | Better than base on Eric's tasks. Below frontier on hard questions. | Small base (1-1.7B fits the budget) plus small adapters |

## Open questions

- Licences of Gemma, OLMo 2, Mistral and SmolLM; LFM2 restrictions.
- The verbatim OpenAI clause and the consumer-terms text.
- Whether llama.cpp on the pinned build b10456 loads LoRA adapters and swaps them per request.
- Memory cost per adapter.
- Real training speed on the Arc 140V.
- Real RAM, speed and accuracy of Granite-4.0-H-1B, Qwen3-1.7B and BitNet 2B4T on this laptop.
- Whether BitNet 2B4T runs well on Lunar Lake through bitnet.cpp, and why the widget shows 0.8B.
- The exact Unsloth VRAM need for a small base.
- Who owns the 4.1 GB vmmem process.
- Lawyer's view before any commercial release.
