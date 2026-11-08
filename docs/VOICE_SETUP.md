# Getting the voice loop running

> **Today (1 Oct 2026): `atlas get everything` sets all of this up** — it
> downloads every piece below, hash-checked, into Atlas's own folder. The
> manual steps further down are kept for reference and for machines without
> the installer.
>
> **Which ear is live.** Speech-to-text is Parakeet TDT 0.6B v2 when it has
> been downloaded (`atlas get hearing`; about a quarter of whisper's time per
> sentence), whisper.cpp otherwise; `stt_engine` in settings can force either.
> `atlas doctor` prints the one in use on its `hearing` line. Parakeet is ©
> NVIDIA under CC BY 4.0 — its credit is in `THIRD_PARTY_NOTICES.md`, with
> Silero VAD (MIT) and the CAM++ speaker model (Apache-2.0).
>
> **The voice.** Kokoro when downloaded, Piper otherwise.

Three free binaries. No subscriptions, no accounts, no cloud. Total install is
about 20 minutes and roughly 1 GB, most of it the speech model.

## What each piece does

| Stage | Tool | Why this one |
|---|---|---|
| record mic | **ffmpeg** | already needed for screen capture; one binary |
| speech → text | **whisper.cpp** | best local accuracy per CPU cycle, runs fine without a GPU |
| text → speech | **piper** | fast, natural, ~60 MB, no GPU |
| playback | **ffplay** | ships with ffmpeg |

None of these are compiled into Atlas. They are named in `config/tools.yaml`
and shelled out to, so replacing any of them is a YAML edit.

## Install

```powershell
winget install Gyan.FFmpeg          # gives you ffmpeg AND ffplay
```

**whisper.cpp** — grab a release build from
`github.com/ggml-org/whisper.cpp/releases`, unzip somewhere permanent, add
that folder to PATH. Then get a model:

```powershell
mkdir models
curl -L -o models\ggml-base.en.bin `
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

`base.en` is the right starting point — roughly 140 MB, near-realtime on a
laptop CPU. Move up to `small.en` only if accuracy is actually the problem;
it is 3× slower for a modest gain on clear desk audio.

**piper** — release binary from `github.com/rhasspy/piper/releases`, plus a
voice:

```powershell
curl -L -o models\en_US-amy-medium.onnx `
  https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/amy/medium/en_US-amy-medium.onnx
curl -L -o models\en_US-amy-medium.onnx.json `
  https://huggingface.co/rhasspy/piper-voices/resolve/main/en/en_US/amy/medium/en_US-amy-medium.onnx.json
```

## Find your microphone's exact name

ffmpeg needs the device string verbatim, including punctuation:

```powershell
ffmpeg -list_devices true -f dshow -i dummy
```

Copy the audio device string into `mic_device` in `config/tools.yaml`.

## Verify, then use

```powershell
atlas doctor          # every line must say ok
atlas --voice
```

`doctor` checks each binary is on PATH and each model file exists, so a
missing piece is named before you are standing there talking to nothing.

## Notes from building it

- **8 seconds of recording per turn** is the default. Fixed-length recording is
  deliberately dumb: no silence detection, no endpointing. It always works.
  Voice-activity detection is a later refinement, not a prerequisite.
- **Press Enter to talk.** Wake words come after the loop is reliable —
  debugging placement through a wake-word false-positive is debugging two
  systems at once.
- **`[BLANK_AUDIO]`** is what whisper emits for silence. Atlas strips bracketed
  annotations before parsing, or every silent turn would parse as a command.
  There is a test for this.
- **Spoken replies are one short line** by design. A confirmation that runs
  three sentences is unusable when you just want to know it worked.

---

# The reasoning layer

Fixed phrases in `commands.yaml` handle the things you say every day. Anything
else goes to a language model, which replies with one JSON object naming an
action. **The model is an enhancement, never a dependency** — if it is down,
slow, or talking nonsense, `boot workspace` still boots your workspace. There
is a test asserting exactly that.

## Local, nothing leaves the laptop

```powershell
winget install Ollama.Ollama
ollama pull llama3.1:8b
```

That is the default in `config/tools.yaml`. An 8B model is plenty for "which
of these actions did he mean", which is the actual job.

## Letting Atlas see

Screenshots and webcam frames are captured by ffmpeg and handed to a
vision-capable model as base64. Uncomment `vision_request` in `tools.yaml`.

- **Local:** `ollama pull llava`. Free, private, and honestly mediocre at
  reading dense screen text on a laptop CPU.
- **Hosted:** more accurate by a wide margin. This is the one place where a
  paid API buys capability you cannot get locally on laptop hardware, so it is
  worth knowing the trade rather than assuming local is always equivalent.

Without a `vision_request`, "view my display" still captures the image and
tells you where it saved it — it just can't describe it.

## Wake word

Set `wake.enabled: true`, then run `atlas --wake`.

With no `detector` configured, Atlas records short clips and runs speech-to-text
on each one looking for the phrase. **This works with nothing extra installed
and it will keep a CPU core busy and drain battery.** It is the honest starting
point, not the destination. Once the loop is proven, add a dedicated detector
(openWakeWord, Porcupine) under `wake.detector` — it blocks on near-zero CPU
until the word is heard, and Atlas will use it instead automatically.

Matching is deliberately loose. Speech-to-text renders your wake word
differently every time — "Atlas", "atlas,", "Hey, Atlas!" — so Atlas strips
punctuation and case before comparing. Tested against all of those.

---

# Push-to-talk, and the typing box

Built 25 Sep 2026 (Eric's ruling H1): the wake word, push-to-talk and a typing
box all work at once, and both keys are yours to choose in settings.

**Push-to-talk** is `push_to_talk.key` (Tab unless you change it), watched from
anywhere in Windows by a low-level keyboard hook (`hotkeys.rs`). Tab is a key
you press hundreds of times a day, so Atlas watches how long it's held:

- **Tap** — given back to the app you're in. Your indent, your field change,
  untouched.
- **Hold past `hold_ms` (350ms)** — Atlas starts recording and keeps the key.
- **Release** — recording stops and what you said is heard.

Below about 250ms ordinary typing starts triggering it. Any key by name works:
`capslock`, `rightctrl`, `scrolllock`, `f13` and so on. `enabled: false` turns
only the key off.

**The typing box** is `quick_input.hotkey` (Ctrl+Shift+Space unless you change
it — no Alt needed). Press it anywhere and a one-line box opens over what you're
doing; Enter sends, Escape closes. It takes Ctrl, Shift or Win with a key, or on
its own a key nobody types with (an F-key, Insert, Pause, Scroll Lock), so it
never fires while you type. If another program already owns the combination,
Atlas says so at start-up and push-to-talk still works.

**Changing either key:** in Settings → Talking to it, press **Set by pressing**
and then the key(s) you want, or type the name. Or say it: "set my typing key to
F9", "set my push to talk key to caps lock". "What are my keys" says both. A key
that can't work is refused before it's kept, and the change takes hold when
Atlas restarts.

Still to try on the laptop: both keys in a real session. The decisions (tap or
hold, what the box does with each key) are tested without a keyboard.

**Off Windows** (merged 26 Sep from round 5's `hotkey.rs`): on Linux the
push-to-talk key is read from the keyboard device, not grabbed, so the app still
gets the key as well; reading it needs your user in the `input` group, and
`doctor` says so when it can't. If that matters, set `push_to_talk.key` to a key
nothing else uses -- `scrolllock`, `pause`, `rightctrl`, or `f13`. There is no
typing-box key off Windows.

---

# Working while Atlas works

## The constraint

On Windows, a synthetic click or keystroke goes to whatever window has focus.
There is no OS-level way around that. If Atlas clicks something, it takes the
foreground — and you lose your place mid-sentence.

Pretending otherwise would mean building something that fights you all day. So
work is split into two lanes.

## Background lane — runs whenever, invisibly

Never touches your screen:

- web research (in a **separate headless Chrome**, not your Chrome window)
- searching inside your indexed documents
- reading and summarizing files
- writing notes

Say "look into X for me" and it starts immediately while you keep typing. You
get told when it's done.

## Foreground lane — waits for a gap

Needs your actual windows, so it queues until you have been quiet for
`lanes.gap_secs` (20 by default):

- opening, closing, focusing, moving windows
- clicking and scrolling
- typing into an application
- screenshots

Say "do it now" and it goes immediately — urgent tasks skip the wait. Only one
foreground task runs at a time, and anything that never gets a gap within an
hour gives up and tells you rather than lurking forever.

## What this means in practice

"Research the IETF QUIC v1 spec and open Chrome to the docs" splits: the
research starts now in the background, and the Chrome window waits until you
pause. That is the honest best available given how Windows input works.

## Genuinely still open

Reading a page without a browser window is solved. **Clicking a button inside
an app you are actively using is not**, and cannot be while you are looking at
it. Two real paths exist if this matters later:

- **UI Automation** — invoke controls by accessibility API rather than by
  synthetic clicks. Works without focus for many apps; unreliable for Chrome
  and Electron.
- **A second Windows desktop** — `CreateDesktop`, drive an app there
  invisibly. This is what commercial RPA tools do. It is real, and it is a
  substantial piece of work.

Neither is built. Both are honest options rather than something I can hand you
this round.
