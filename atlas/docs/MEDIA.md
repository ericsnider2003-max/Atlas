# In-house image and video

Researched before writing this. The short version: **image is genuinely
achievable, video splits into a yes and a no**, and the deciding factor for all
of it is the GPU in your laptop.

---

## Images — yes, and "add me to this" is a solved workflow

Local image generation and editing needs no third party. The pieces:

| need | tool | notes |
|---|---|---|
| Generate from a description | Stable Diffusion / SDXL / Flux | open weights, run locally |
| Edit part of an image | inpainting | mask a region, describe the change |
| **Put you in a picture** | IP-Adapter FaceID | this is the specific one |

**"Add me to this image"** is exactly what IP-Adapter FaceID does. It uses a
face-ID embedding from a face recognition model rather than a generic image
embedding, and combines with LoRA for identity consistency. A portrait variant
takes several photos of you to improve likeness, and image-guided inpainting
works by replacing the text prompt with an image prompt — so Atlas can hold a
reference of you and place you into a scene.

Practically: you give Atlas a handful of photos of yourself once. It builds a
face embedding. After that, "put me in this photo, standing on the left" is a
mask plus a prompt plus your embedding. **No training run, no upload, no
service.**

The honest cost: a GPU. SDXL wants roughly 8–12GB of VRAM to be pleasant.
On CPU it works but takes minutes per image, which turns a conversational
request into a background job. That is survivable — it goes in the background
lane like research does — but it is not instant.

---

## Video — editing yes, generating mostly no

These are two different problems and it matters which one you mean.

### "Take this video, here's my vision, edit it" — achievable

Cutting, reordering, trimming, transitions, colour, speed, captions, audio
levels: all of that is ffmpeg, which Atlas already drives. The intelligence
required is *planning* — turning "make it punchier, cut the dead air, put the
best bit first" into an edit list. That is a language problem, not a graphics
problem, and the local model already handles that class of work.

This is buildable now and I would build it next: describe the vision, Atlas
watches the video (sampled frames plus the audio transcript via whisper, which
you already have), proposes an edit decision list, renders it with ffmpeg, and
shows you the result. Originals untouched, output to a new file.

### "Generate video from a description" — real, but hardware-bound

Open-weight video models genuinely exist now and are not toys. Wan 2.2 is
Apache-2.0 and runs from 8GB VRAM on a 5B GGUF with memory offloading. With
quantization Wan 2.1 1.3B runs at 4–6GB, and LTX Video 2B fits at 6–8GB with
FP8 and tiling — limited to 480p and shorter clips, but usable.

The catch is the honest one: even quantized, generating a 5-second 720p video
at good quality needs about 16GB of VRAM as a floor, and the flagship models
want 32GB+. A 5-second clip at 24fps is 120 frames — video multiplies image
requirements by three to ten times.

**So:** generation is possible on a desktop with a strong GPU, marginal on a
laptop, and out of reach on a laptop without a discrete one. Editing has no
such ceiling.

---

## What this means for the build order

1. **Video editing from a description.** No GPU floor, uses tools already in
   the stack, immediately useful. Build first.
2. **Image generation and inpainting, including your likeness.** Needs a GPU
   to be pleasant, but nothing exotic. Build second.
3. **Video generation.** Design the interface now, defer the implementation
   until the hardware question is answered. It would be dishonest to wire this
   up and let it fail on your laptop.

The policy layer for all of it is already built and tested: local media work is
done-and-reported, sending your content to an outside AI service always
requires approval, and overwriting an original is gated wherever it ran.
Originals are never modified in place.

---

## Your actual hardware — answered

**Intel Arc 140V, 8GB, integrated. No discrete GPU. 15.7GB system RAM.**

That is a Lunar Lake chip, and it changes three things:

**No CUDA.** Every "just install CUDA" instruction you will read online does
not apply. The Intel path is Vulkan or SYCL, both of which llama.cpp supports —
`llama-server` with the Vulkan backend will use this GPU. That is a build flag,
not a limitation.

**The 8GB is shared, not dedicated.** It is carved out of the same 15.7GB the
operating system is using. Your screenshot showed 12.8GB already in use at 82%.
So the real budget for a language model is closer to **3–4GB**, not the 6GB I
had defaulted to — a 7B at Q4 plus its cache would fight Windows for memory and
you would feel it. A 3B or 4B model at Q4_K_M is the honest fit, and the model
registry already picks the largest that fits rather than the largest available.

**You also have an NPU** (Intel AI Boost). It is not useful for llama.cpp today,
but OpenVINO can target it, and it is the right home for small always-on models
— wake word, speech-to-text — because it costs almost no power. Worth revisiting
once the basics run.

### What this means for media, concretely

| | verdict |
|---|---|
| Image generation (SD 1.5 class) | **Yes.** Via OpenVINO or Vulkan. Expect tens of seconds, not seconds. Background-lane work. |
| SDXL / Flux | Marginal. It will run and it will be slow enough to be annoying. |
| "Add me to this photo" | **Yes**, same path — the face embedding is cheap; the generation is the slow part. |
| Video **editing** from your description | **Yes, unaffected.** ffmpeg is CPU work with hardware decode. This is the one to build. |
| Video **generation** | **No.** The floor is ~16GB VRAM for a 5-second 720p clip. Not on this machine. |

That is not a bad hand. Video editing was always the more useful half, and it
is fully open to you. Video generation would need a desktop with a discrete
card, and nothing in Atlas's design assumes it.

## Confirming it yourself later

What GPU is in the target laptop? It decides:

- whether image generation is seconds or minutes per image
- whether video generation is on the table at all
- how much of the model memory budget is left for the language model

`atlas doctor` will report it once you run it on the machine. Before then, you
can find out in about ten seconds:

**Fastest:** press `Ctrl+Shift+Esc`, open the **Performance** tab. Any entry
labelled "GPU 1" alongside the integrated one is a discrete card, and the panel
shows its name and **Dedicated GPU memory** — that number is the VRAM figure
everything above depends on.

**Or in a terminal:**

```powershell
Get-CimInstance Win32_VideoController | Select-Object Name, AdapterRAM
nvidia-smi          # if this command exists at all, you have an NVIDIA GPU
```

Rough reading of the answer:

| what you see | what it means |
|---|---|
| Intel UHD / Iris Xe only, no second GPU | integrated. Image generation in minutes, not seconds. No video generation. |
| NVIDIA RTX with 6–8GB | image generation is comfortable. Video generation only at 480p, short clips. |
| NVIDIA RTX with 12–16GB | image generation is fast. 5-second 720p video is at the floor but real. |
| 24GB+ | everything on this page is open to you. |
