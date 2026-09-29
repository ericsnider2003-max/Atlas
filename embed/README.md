# embed — the meaning encoder for Atlas's search-by-meaning

Text on stdin, one 384-number vector on stdout. This is the reference
implementation of the encoder contract in `atlas/src/meaning.rs`: the model is
all-MiniLM-L6-v2, mean-pooled and L2-normalised the way sentence-transformers
does it, so the cosine scores `recall` computes are the ones the model was
trained to make meaningful.

## What's here

```
src/            the program — one dependency (tract-onnx, pure Rust, the same
                major version personal Atlas already builds with), and BERT's
                WordPiece tokenizer written out in ~120 tested lines
models/         vocab.txt, plus GET_THE_MODEL.md — the 86MB onnx file
                itself exceeds the chat delivery cap, so it is one
                hash-pinned curl from huggingface (the hash in that file is
                of the exact copy this bundle was verified against)
dist/embed.exe               Windows x86-64, statically linked, no DLLs
dist/embed-linux-x86_64      Linux build, same source
```

## Verified before shipping (22 Sep 2026)

- 4 tokenizer unit tests green.
- Semantic sanity on real vectors: "what do I know about apples" scores 0.271
  against the orchard note, ~0 against unrelated text; vectors are unit length.
- **End to end through the real daemon**: with this encoder configured, Atlas
  found a note sharing zero words with the question, off the real tick path —
  the same test the fake encoder runs, passed by the real model.
- One invocation is ~0.26s including model load.
- `dist/embed.exe` was run under wine and prints output byte-identical to the
  Linux build. That is not the same as running on a real Windows machine —
  that last step is yours, and it is one command:
  `echo hello | embed.exe --model ... --vocab ...` printing 384 numbers.

## Installing on the Atlas machine

1. Download the model per `models/GET_THE_MODEL.md` and CHECK THE HASH.
2. Copy `dist/embed.exe` to `<atlas install>/tools/embed/embed.exe`, and
   `all-MiniLM-L6-v2.onnx` + `vocab.txt` to `<atlas install>/models/`.
3. In `config/tools.yaml`, uncomment the `meaning.encoder` block (the shipped
   example matches these paths exactly) and set `recall.semantic: true`.
4. `atlas doctor` — the `meaning-search` line should read "on and the encoder
   is installed". Existing notes get their vectors within a couple of minutes
   of idling (two per tick); new notes as they arrive.

## Rebuilding

`cargo build --release` (this machine), or
`cargo build --release --target x86_64-pc-windows-gnu` with mingw-w64 for the
Windows exe. `cargo test` covers the tokenizer.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
