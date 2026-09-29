# Replacing Ollama

## What Ollama actually is

The important thing the name hides: **Ollama does not do inference.**

Ollama is a Go server that wraps **llama.cpp**. When you type `ollama run`, the
CLI sends an HTTP request to a local server, which reads the GGUF file, works
out how much RAM and VRAM you have, spawns a llama.cpp runner as a child
process, formats your prompt using the model's template, and streams tokens
back. The actual matrix multiplication happens in **ggml**, llama.cpp's tensor
library, on CUDA, Metal, Vulkan or CPU.

So "Ollama" is really five jobs stacked on someone else's engine:

1. Download and store models
2. Parse GGUF metadata
3. Decide what fits in memory and how many layers to offload
4. Format prompts for each model's chat template
5. Expose a REST API

**All five are replaceable in Rust.** The sixth thing — ggml itself — is not,
and this document is honest about why you shouldn't want to.

---

## Built this round

### GGUF reader (`src/gguf.rs`)

A pure-Rust parser for the model file format. Reads architecture, context
length, layer count, attention shape, tokenizer, chat template, and every
tensor's shape and quantization.

Only the header is read — inspecting a 40GB model costs a few kilobytes of I/O,
not 40GB of RAM. Verified by building GGUF files byte by byte in the tests and
reading them back.

Three things it gets right that are easy to get wrong:

- **A corrupt header cannot make Atlas allocate wildly.** Implausible tensor or
  KV counts are rejected before any allocation.
- **Huge token arrays are counted and skipped.** Real vocabularies run past
  100,000 strings; holding them all in memory to answer "how big is this model"
  would be absurd, and parsing must continue correctly past them.
- **The dominant quantization is measured by bytes.** One F32 normalization
  tensor does not make a Q4 model an F32 model.

### Memory fitting (`src/models.rs`)

The calculation Ollama does before loading, done here directly — including the
term people forget:

> A 7B model at Q4 is about 4GB of weights. A 32k context adds several more in
> KV cache. That is why a model that "fits" runs out of memory partway through
> a long conversation.

The cache is computed from the real architecture: layers × KV heads × head
dimension × context × 2 (key and value) × 2 bytes. Model selection picks the
largest model that fits *including* the cache, because a model that pages to
disk is worse than a smaller one that runs. And it explains itself — *"nothing
fits 100MB — the smallest, small-3b, needs ~2400MB"*.

### Prompt templating

Models are trained on a specific chat format. Feeding one the wrong markers
degrades it subtly rather than failing outright, which is worse — you get
mediocre answers and no error. ChatML, Llama 3, Mistral and Gemma are
implemented, detected from the model's own embedded template with the filename
as fallback, each with its correct stop tokens.

### Direct llama.cpp

`llama-server` is a single binary from llama.cpp releases. Atlas builds its own
command line — model path, context clamped to what the model supports, GPU
layers computed from free VRAM, bound to `127.0.0.1` so nothing is ever exposed
— and drives it. **No Ollama, no Go, no wrapper.**

Partial GPU offload is calculated rather than guessed, because offloading more
layers than fit is slower than offloading none: the driver starts paging.

---

## What I would not rebuild, and why

**ggml/llama.cpp is the engine.** Rewriting it means writing quantized matrix
kernels for AVX2, AVX-512, NEON, CUDA, Metal and Vulkan; a tensor graph with
memory planning; and the dequantization paths for a dozen block formats. That
is years of work from many contributors, and the result would be slower.

More to the point, **it would not buy you independence.** llama.cpp is
MIT-licensed. Nobody can revoke it, paywall the copy you have, or change the
terms on you retroactively. It is the same category as the C compiler you would
have used to write your own — a permanent public good, not a vendor.

The dependency worth removing was Ollama: a company's product, with a hosted
tier, a registry it controls, and a roadmap that is not yours. That one is now
gone. You already depend on the same lineage for speech anyway — whisper.cpp
and ggml are by the same author.

---

## Where this leaves you

| layer | before | now |
|---|---|---|
| Model file parsing | Ollama | **Atlas** |
| Memory fitting / layer offload | Ollama | **Atlas** |
| Prompt templating | Ollama | **Atlas** |
| Model registry and selection | Ollama | **Atlas** |
| API surface | Ollama REST | **Atlas, direct** |
| Inference engine | ggml (via Ollama) | ggml (direct) |
| GPU kernels | ggml | ggml |

Ollama stays supported as one engine option — it works, and there is no reason
to break a setup that runs. But it is no longer required, and nothing in Atlas
assumes it.

**Not verified:** launching a real `llama-server` and getting tokens back. The
parsing, fitting, templating and command construction are all tested; the live
process needs the laptop.

## If you ever do want native inference in Rust

The honest path is not writing kernels from scratch. `candle` (Hugging Face's
Rust tensor library) already loads GGUF and runs quantized Llama-family models
with CUDA and Metal support. It would make Atlas a single binary with no child
process at all. It is slower than llama.cpp today and supports fewer
architectures — a real trade, worth revisiting when the rest of Atlas is
running on your laptop rather than before.
