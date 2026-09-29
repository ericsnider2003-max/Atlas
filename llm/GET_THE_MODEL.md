# The generative model — one download, hash-pinned

The GGUF is not in this archive (491 MB, far over the chat delivery cap).
It is one command, and the hash below is the exact file this path was proven
against on 22 Sep 2026.

## Starter model: Qwen2.5-0.5B-Instruct (Q4_K_M, ~491 MB)

Small and fast — good for a first working install and for the CPU-only case.
It genuinely answers ("2+2 equals 4"; explains a function correctly). Swap in
a larger instruct GGUF later (7B–14B Q4) and Atlas will prefer it when it fits
the RAM budget — no config change, the registry re-scans.

    curl -L -o qwen2.5-0.5b-instruct-q4_k_m.gguf \
      https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf

Expected SHA-256:

    74a4da8c9fdbcd15bd1f6d01d621410d31c6fc00986f5eb687824e7b93d7a9db

Check it — PowerShell: `Get-FileHash .\qwen2.5-0.5b-instruct-q4_k_m.gguf -Algorithm SHA256`
· sh: `sha256sum qwen2.5-0.5b-instruct-q4_k_m.gguf`

If the hash does not match, do not use the file. Put it in `models/`.

## The server

`GET_THE_MODEL` is only the weights. You also need a llama.cpp `llama-server`
(see README step 1). Atlas launches it from the `models.server` config and
talks to it on the configured port.
