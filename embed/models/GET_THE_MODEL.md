# The model file — one download, hash-pinned

`all-MiniLM-L6-v2.onnx` (86MB) is not in this archive because the chat
delivery path caps files at 30MB. Everything else — the encoder exe, the
vocab, the source — is here. The model is one command, and the hash below is
the proof you got the exact file this bundle was verified against (it was
downloaded, run end to end through the daemon, and hashed on 22 Sep 2026):

    curl -L -o all-MiniLM-L6-v2.onnx https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/main/onnx/model.onnx

Expected SHA-256:

    6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452

Check it (PowerShell): `Get-FileHash all-MiniLM-L6-v2.onnx -Algorithm SHA256`
Check it (sh):         `sha256sum all-MiniLM-L6-v2.onnx`

If the hash does not match, do not use the file.
