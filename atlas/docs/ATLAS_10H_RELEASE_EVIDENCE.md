> **STALE** — historical release evidence; verify the current branch and receipts before relying on it.

# Atlas hardening and release evidence

Updated: 2026-11-08

## Verified gates

- Controlled full Cargo suite on the pushed candidate: `7060 passed; 0 failed; 10 ignored` (791.47s).
- Research-focused cluster: `72 passed; 0 failed`.
- Private-model contract cluster: `10 passed; 0 failed`.
- `cargo fmt --check`: passed.
- `git diff --check`: passed.
- CAS stale-snapshot regression: passed.
- Live local language model: passed against the installed Qwen3-1.7B GGUF via
  the installed llama-server. Conversation/tool routing, online research and
  filing, memory across restart, and Atlas council all passed.
- Live local speech synthesis component: Piper loaded the installed
  `en_US-amy-medium.onnx` and produced a 110,240-byte WAV in 1.5 seconds.
- The pushed candidate branch applies cleanly in the release worktree and passed
  `cargo check --no-default-features`.

## Not yet release evidence

- No `private_models` manifests were present in the repository configuration;
  the successful live language run used Atlas's installed local llama runtime,
  not the optional private-manifest adapter.
- `ATLAS_REAL_MODEL_URL`, `ATLAS_PRIVATE_MODEL_ROOT`, and live voice variables
  were unset; the live verification script supplied the local llama endpoint.
- FreeLLMAPI was listening on loopback port 31415, but `/v1/models` returned
  HTTP 401 (`Invalid API key`). This is an environment/authentication finding,
  not evidence that a private model call succeeded.
- The user's original worktree remains dirty and was not reset. The pushed
  release was assembled in an isolated release worktree; unrelated files were
  not included in the release commits.

## Review disposition

- Thermo-Nuclear/council review found no additional CAS regression after the
  full-suite rerun.
- Approval correlation remains a review item for duplicate identical intents
  in parallel work. No speculative schema change was made without a
  deterministic reproducer.

## Release rule

The release is pushed for review. The optional private-manifest adapter remains
an explicit follow-up because no private manifest was configured; the live
local Qwen/Piper workflow is the evidence for the installed local models.
