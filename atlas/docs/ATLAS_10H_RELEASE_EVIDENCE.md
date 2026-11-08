> **STALE** — historical release evidence; verify the current branch and receipts before relying on it.

# Atlas hardening and release evidence

Updated: 2026-11-08

## Verified gates

- Controlled full Cargo suite: `7058 passed; 0 failed; 10 ignored`.
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
- Candidate hardening patch applies cleanly to a fresh detached `HEAD` worktree;
  the isolated candidate also passed `cargo check --no-default-features`.

## Not yet release evidence

- No `private_models` manifests were present in the repository configuration;
  the successful live language run used Atlas's installed local llama runtime,
  not the optional private-manifest adapter.
- `ATLAS_REAL_MODEL_URL`, `ATLAS_PRIVATE_MODEL_ROOT`, and live voice variables
  were unset; the live verification script supplied the local llama endpoint.
- FreeLLMAPI was listening on loopback port 31415, but `/v1/models` returned
  HTTP 401 (`Invalid API key`). This is an environment/authentication finding,
  not evidence that a private model call succeeded.
- The current worktree has 241 staged paths and 542 unstaged paths. The
  hardening files are mixed with unrelated/pre-existing changes and must be
  separated before a release commit or push.

## Review disposition

- Thermo-Nuclear/council review found no additional CAS regression after the
  full-suite rerun.
- Approval correlation remains a review item for duplicate identical intents
  in parallel work. No speculative schema change was made without a
  deterministic reproducer.

## Release rule

Do not commit or push until the private-model live evidence exists, the E2E
workflow has a live receipt, and the intended release paths are isolated from
the unrelated worktree changes.
