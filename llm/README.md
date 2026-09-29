# The local generative model — install path

This is the sibling of `embed/`, for the *generative* model (not the
embedder). It is what turns `explain`, `build_it`, the council, and the
daemon's own decide loop from "Untested" into working, because those are all
already wired to the model-call primitive (`brain::Llm` + `ShellLlm`) — the
only thing missing on a fresh machine is a model for them to call.

## The tree does the hard part for you

You do **not** hand-write a model connection. Atlas's `models::Registry`
scans the `models/` folder, picks the largest GGUF that fits this machine's
RAM budget, reads that file's own chat template, and builds the connection
itself (`main.rs::model_connection` → `models::llm_config_for`). It talks to
a llama.cpp server on `127.0.0.1:<port>/completion` and reads the reply at
JSON path `content`. So the install is two assets and one config block.

## Install (Windows, the Atlas machine)

1. **A llama.cpp server.** Download a `llama-server` build from
   github.com/ggml-org/llama.cpp releases (the `-bin-win-...` zip for your
   CPU/GPU). Put `llama-server.exe` (and its DLLs) in `tools/llama/`.
2. **A model.** Get a GGUF per `GET_THE_MODEL.md` (starter: Qwen2.5-0.5B, tiny
   and fast — swap in a bigger one later and Atlas will prefer it if it fits).
   Put it in `models/`.
3. **Config.** In `config/tools.yaml`, set the `models:` block (see
   `tools-yaml-models-block.example`) so Atlas knows the folder, the port, and
   how to launch the server. No `tools.llm` needed — leaving it unset lets the
   self-building path run; setting it always wins if you'd rather be explicit.
4. `atlas doctor` — the model line should report the chosen model. Ask Atlas
   to explain a file, and it drafts through the model.

## Proven here (22 Sep 2026)

This was not left as an assertion. In the build container a real
Qwen2.5-0.5B-Instruct GGUF was served on the exact `/completion` contract the
tree uses, and the tree's **own** code path was run against it —
`Registry::scan` → `choose_for` → `llm_config_for` → `ShellLlm`:

- `complete("what is 2+2?")` → **"2+2 equals 4."**
- `explain::in_plain_english("fn add(a,b){a+b}")` →
  **"This function takes two integers, adds them together, and returns the sum."**

So the seam, the config-builder, and a real consumer all work against a live
model. What is left is running it on *your* hardware — the same kind of step
as the embedding model. `serve_reference.py` is the test harness that speaks
the two endpoints (`/health`, `/completion`) Atlas calls; on a real machine
llama.cpp's `llama-server` serves those same two, and Atlas can't tell the
difference — which is why proving against the harness proves the path.

## Notes

- **Offline-first still holds.** With no model installed, everything degrades
  exactly as before — the model-gated capabilities simply say they need a
  model. Nothing breaks.
- **Online fallback is separate.** `tools.cloudflare` (the delegate worker) is
  the optional stronger/online path; it is orthogonal to this local install.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
