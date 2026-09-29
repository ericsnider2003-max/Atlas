# What costs money: nothing

> **STALE — the numbers describe a much smaller tree.** It says 1,094 tests. Last true on early September 2026.
>
> The current state of the tree is `HANDOFF_2026-09-19.md`. The capability catalogue is
> `CAPABILITIES.md`, generated from `capability::all()` and held to the code by
> `tests/catalogue.rs`. Every module's own words are in
> `MODULE_REFERENCE_2026-09-26.md`.
>
> This file is kept as the record of that date rather than edited to match later
> work — a history that gets rewritten stops being a history.

You've asked twice, which means I haven't answered it plainly enough. So,
plainly:

**Everything Atlas does costs nothing. There is no subscription, no account, no
API key, and no free tier that runs out.**

The word "model" has caused this, and it's a fair confusion. In this project a
model is **a file on your disk**, like a font or a video codec. It is not a
service you connect to.

---

## Every piece, and what it costs

| | what it is | licence | cost |
|---|---|---|---|
| **whisper.cpp** | turns speech into text | MIT | free |
| **ggml speech models** | the files it uses | free download | free |
| **piper** | turns text into speech | MIT | free |
| **piper voices** | how Atlas sounds | free download | free |
| **ffmpeg** | audio and video | LGPL | free |
| **llama.cpp** | the reasoning model runner | MIT | free |
| **GGUF models** | the reasoning models | free download | free |
| **Chrome** | headless, for research | free | free |
| **Tesseract** | reading text from images | Apache | free |
| **Atlas itself** | 1094 tests of it | yours | free |

No account is needed for any of them. The downloads are plain files from
Hugging Face and GitHub — no key in the URL, and there's a test asserting that.

## Translation costs nothing either

This is the one that sounds like it should. It doesn't, for a specific reason:
Whisper transcribes and translates **in the same pass**, with one flag. There
is no second model, no second run, and nothing sent anywhere. Swapping
`ggml-base.en.bin` for `ggml-base.bin` is the same file size and gains ninety-odd
languages.

## The one thing that could cost money, and it's off

A hosted model, only for work a small local model can't do — writing real code
against a large codebase. It's in `docs/COST.md`, roughly $21 a month if you
ran it every night, and **it is switched off**. Nothing enables it but you.

Everything else in this document runs on your laptop with the network cable
pulled out.

---

## Changing how Atlas sounds

Seven free voices, and you change them by saying so:

- *"a bit slower"* · *"you sound robotic"* · *"that's too dramatic"*
- *"use a British voice"* · *"try something deeper"* · *"use Amy"*
- *"can you sound more natural"* — offers the best one, and tells you it's
  114MB and slower
- *"use a different voice"* — plays three, in their own voices, saying
  something it would actually say

Adjustments are small on purpose. You'll say it again if it isn't enough, and
overshooting is more annoying than undershooting. Nothing can be pushed into
being unusable — there's a floor and a ceiling on every setting.
