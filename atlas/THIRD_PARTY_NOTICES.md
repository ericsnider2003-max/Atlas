# Third-party notices

## wshobson/agents

Atlas adapts ideas and short lists from the wshobson/agents repository
(https://github.com/wshobson/agents, commit 4236bb91, 13 Sep 2026), MIT
licensed. Nothing from it is installed, run or loaded at runtime; the pieces
below were rewritten into Atlas's own code.

| In Atlas | Adapted from |
|---|---|
| `council::build_room` — the "should I build this?" room | `plugins/before-you-build/skills/before-you-build/SKILL.md` (risk checklist) |
| `council::security_room` — the "is this safe?" room | `plugins/security-scanning/skills/stride-analysis-patterns` (STRIDE) |
| `draft` — the `Chatbot` and `Blank` faults, extra filler words, `for_replies` | `plugins/avoid-ai-writing/skills/avoid-ai-writing/references/pattern-catalog.md`, `word-tiers.md` |
| `research::figures_not_in` — figures must appear in the sources | `plugins/documentation-standards/skills/grounded-vault/SKILL.md` |
| research brief prompt — exact figures, name the source | `plugins/content-marketing/agents/search-specialist.md` |

```
MIT License

Copyright (c) 2024 Seth Hobson

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## accesskit (via eframe), Windows only — added 26 Sep 2026

Atlas's own windows are drawn with eframe/egui, which were already in the tree. On 26 Sep 2026 eframe's `accesskit` feature was turned on for Windows builds only, so screen readers (Narrator, NVDA, JAWS) can read those windows through Windows' UI Automation. This is what EN 301 549 11.5 and Section 508 ask for.

- It adds 11 crates to the Windows build: `accesskit`, `accesskit_consumer`, `accesskit_windows`, `accesskit_winit`, and their helpers.
- It adds nothing to the Linux, macOS, phone or lean builds.
- The licences are MIT or Apache-2.0 (https://github.com/AccessKit/accesskit).
- Nothing is fetched at runtime.

## Building the phone apps — added 26 Sep 2026

These are used only to build the phone apps. None of them runs inside Atlas on a laptop, and nothing is fetched at runtime.

- **OpenSSL, built from source into the Android app.** This is `native-tls`'s `vendored` feature, which brings the `openssl-src` crate (OpenSSL 3, Apache-2.0) into Android builds only. Android has no system TLS library an app can link. iOS uses Apple's Security framework and Windows uses its own, so neither needs it.
- **XcodeGen (MIT)**, installed on the cloud Mac for each iPhone build, turns `mobile/ios/project.yml` into the Xcode project. It isn't in the repository or in the app.
- **apksigner (Apache-2.0, Google's Android build-tools)** signs the Android app on Eric's machine. It's one file kept beside the signing key, not in the repository.

## The Kokoro voice (sherpa-onnx, ONNX Runtime, Kokoro, espeak-ng) — added 28 Sep 2026

Not in atlas.exe. Downloaded only when you choose the Kokoro voice (Sound &
voice, or `atlas get kokoro`), into `tools/kokoro/` and `models/kokoro/`,
pinned by SHA-256, and opened at run time (`src/kokoro.rs`).

| Piece | Licence | From |
|---|---|---|
| sherpa-onnx 1.13.8 C library (`sherpa-onnx-c-api`) | Apache-2.0 | https://github.com/k2-fsa/sherpa-onnx |
| ONNX Runtime (shipped inside sherpa-onnx's archive) | MIT | https://github.com/microsoft/onnxruntime |
| Kokoro-82M v1.0 weights and voices (sherpa-onnx's int8 export) | Apache-2.0 | https://huggingface.co/hexgrad/Kokoro-82M |
| espeak-ng, compiled into the sherpa-onnx library, and its `espeak-ng-data` (in the model download) | **GPL-3.0-or-later** | https://github.com/espeak-ng/espeak-ng |

The structure layouts in `src/kokoro.rs` that Atlas hands the library are
copied from the `sherpa-onnx-sys` 1.13.8 crate (Apache-2.0).

espeak-ng is how sherpa-onnx pronounces words its dictionaries don't have.
It is GPL code, running inside Atlas's process when Kokoro speaks. Atlas
neither links nor ships it — you download it separately — but if Atlas is
ever distributed *with* the Kokoro download bundled, that bundle carries
espeak-ng's GPL obligations (its source offer), exactly as `piper1-gpl`
would have. The same is already true of piper: the piper 2023.11.14-2
Windows zip that Atlas's setup fetches contains `espeak-ng.dll` and
`espeak-ng-data` (checked 28 Sep 2026).

## Hearing and voice models (Parakeet, Silero VAD, CAM++) — added 1 Oct 2026

Downloaded by `atlas get` (pinned by SHA-256) and run on this machine; none
is shipped inside atlas.exe.

| Piece | Licence | From |
|---|---|---|
| Parakeet TDT 0.6B v2 speech-to-text model, by NVIDIA, as sherpa-onnx's int8 ONNX export (`sherpa-onnx-nemo-parakeet-tdt-0.6b-v2-int8`) | **CC-BY-4.0** | https://huggingface.co/nvidia/parakeet-tdt-0.6b-v2 (export: https://github.com/k2-fsa/sherpa-onnx) |
| Silero VAD v6.2.3 voice-activity model (`silero_vad_16k_op15.onnx`) | MIT | https://github.com/snakers4/silero-vad |
| 3D-Speaker CAM++ speaker model, English VoxCeleb (`3dspeaker_speech_campplus_sv_en_voxceleb_16k.onnx`, sherpa-onnx export) | Apache-2.0 | https://github.com/modelscope/3D-Speaker (export: https://github.com/k2-fsa/sherpa-onnx) |

**Attribution required by CC-BY-4.0 (Parakeet).** "Parakeet TDT 0.6B v2" ©
NVIDIA, licensed under the Creative Commons Attribution 4.0 International
licence (https://creativecommons.org/licenses/by/4.0/). Changes: converted to
ONNX and quantised to 8-bit integers by the sherpa-onnx project; Atlas uses
it unmodified from that export. Atlas shows this notice, and `atlas doctor`
names the hearing model in use, wherever the model is.
