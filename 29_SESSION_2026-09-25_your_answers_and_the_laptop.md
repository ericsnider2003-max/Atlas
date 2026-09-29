# Session, 25 September 2026: your answers, and what the laptop showed

Eric: *"Log in the backup. If the prompts add value yes. What is the 3gb question. Laptop is unlocked. What's the envelope question? Keep the hub paused. Wait for the merge I'm still working."* Then: *"Envelope: no one else can ask Atlas."* and *"Implement the picture related applications."*

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## 1. The log is checked against the backups

- **Every backup already copied the log's heads** (`activity-anchors.jsonl`), because it copies the whole state folder.
- **They're now read back.** `atlas journal check`, `atlas doctor` and Atlas's start-up check all compare the log with the heads kept beside it *and* with the heads in every backup. The answer says how many backups it was checked against.
- **Why it matters.** Someone with access to your files could rewrite the log, its seals and its anchor file together, and the laptop's own copy would still agree with itself. A backup made before that happened doesn't agree, so the check says the log was altered. A test does exactly this.
- **One more hole closed.** A log could be replaced with one numbered to start *after* every recorded head, so the check would have skipped all of them. Only the newest 400 entries are kept, so a recorded head can only legitimately be missing once 400 more have been written after it. Any sooner, and the log is called altered.
- **An empty log** now says "nothing sealed in it yet". Before, it said "intact … last seal ." with nothing after the full stop.
- **Honest limit:** backups live in `data\backups` on the same laptop unless the backup folder is pointed at another drive or a synced folder. The check is only as far from the laptop as the backups are.

## 2. The words of graded calls are kept

**What adds value is a *graded* call.** That's an example with a label: "this reply needed fixing, because it sounded like a chatbot", "this seat wouldn't commit". A new model can be tested against those, and one day a model could be tuned on them. A prompt with no grade adds nothing the lengths don't already say, so it isn't kept.

**What's kept, in `model-examples.jsonl` beside the call log:**

| Kind | What's kept |
|---|---|
| **Window replies** | The draft and its rewrite, each with the conversation it answered. |
| **Council seats** | Each seat's brief and answer. |
| **A turn you corrected** | What you said and what Atlas did. The brain's standing instructions are the same every time, so they're left out. |

**Research isn't kept.** Its prompt is the pages it read, tens of thousands of characters a time. Its one grade (a figure not in its sources) is checked without the words anyway.

**Before a line is written,** emails, phone numbers, long numbers (cards, accounts), web-address query strings, key-shaped codes and whatever follows "password" or "PIN" are taken out. Times, prices, years and short numbers stay.

**The call log itself still keeps no words.**

**Where you see it:**
- The switch is in Settings, "Keep graded examples", under *Your accounts and secrets*. It's on, per your yes.
- `atlas trace examples` gives the counts by kind, good and bad, and the file's location.
- The newest 2,000 are kept.

**Honest limits:**
- **Names and street addresses are not removed,** and no shape-based scrubber can do that reliably.
- **A window reply's example includes what the other person wrote.** It stays on this laptop and in your backups; nothing sends it anywhere.

## 3. The envelope is yours alone

Your ruling: *no one else can ask Atlas.* Four ways someone else could reach the question, all four closed:

1. **Atlas handed over to someone:** refused, as before.
2. **A guest profile:** now refused too (`profiles::ONLY_YOU_MAY_ASK`). Before, it could ask, and got that profile's own empty answer.
3. **A voice that isn't yours:** "That's only for the person this Atlas belongs to."
4. **A voice Atlas isn't sure of:** "I couldn't be sure that was your voice … ask me again, or type it."

A typed question on your own profile is answered.

**Fixed along the way:** whose voice it was used to stay remembered after that one utterance. A stranger's voice would still have been "the last voice" when you next typed. It's now cleared after each spoken turn.

**Honest limit:** with no voiceprint enrolled, Atlas can't tell voices apart, and anyone at your unlocked laptop on your profile is taken to be you.

## 4. On the laptop

**How I tested:**
- The new build went into a fresh folder, `C:\Users\erics\AtlasLiveTest-0925`.
- Your other chats' folder (`C:\Users\erics\Atlas\atlas-current`) wasn't touched.
- Typing only ever went into a scratch file I made, `typing-test.txt`, never into anything of yours.

**Typing into a window: a real bug, found and fixed.**
- The first live test typed "Atlas live typing test on 25 September. Nothing was sent." into Notepad. What landed was **"Atlas ................"**: the first few characters, then the last one over and over.
- I measured it with a probe on your laptop:

| How it was sent | What landed |
|---|---|
| Everything at once (how Atlas did it) | Garbled |
| One character per send, no pause | Garbled |
| One character per send, 10 ms apart | Off by one at the start and end |
| One character per send, 20, 30 or 50 ms apart | Exact |

- **Fix, part one:** Atlas now types one character at a time, 25 ms apart. A 300-character reply takes about 8 seconds.
- **Fix, part two:** before pressing Enter, Atlas reads the box back. A reply that didn't land as written is left in the box and **never sent**: "what I typed didn't come out right in the box, so I didn't send it — have a look." A test holds this.
- **A second bug:** Windows refused to bring the window to the front when another program was in front ("Windows wouldn't bring it to the front"). Atlas now joins input with the window in front for the moment of the switch, which is the documented way for a helper acting for you. It doesn't press Alt to get round it, because Alt would open *your* window's menu.
- **Not re-run with the fixed build yet.** By then the laptop had locked itself (the lock screen was up after 5½ hours with no input). Windows won't bring anything to the front or accept typing on a lock screen. **Next time it's unlocked:**
  - `atlas window type notepad.exe hello there`: types into Notepad and never presses Enter;
  - `atlas window read notepad.exe`: reads it back.

**The call check ran.** Microphone and laptop sound both recorded 5 seconds, and both were silent, because nothing was playing and no one was speaking. A real call is still needed to hear it work.

**`atlas window idle`** read the real idle time correctly.

## 5. The picture features: installed and working

Both are free:
- **The picture reader:** Qwen3-VL 4B (Apache 2.0) and llama.cpp (MIT).
- **The "seeing" models:** from OpenCV's model zoo, Apache 2.0.

Both run on the laptop, and nothing is sent anywhere.

| Download | What it's for | Result |
|---|---|---|
| `atlas get pictures` (about 3 GB) | Reading charts and screens | Downloaded, checked against its SHA-256, "Everything's here." |
| `atlas get seeing` (about 135 MB) | Finding faces, telling them apart, naming things, finding hands and fingers, finding and reading words on screen | Downloaded, checked, "Everything's here." |

**The picture reader, live.** I drew a bar chart on the laptop: monthly sales of 120, 140, 95, 160, 175 and 190, January to June. Then I asked `atlas picture chart-test.png "what does this chart show?"`. The answer, in 20 seconds:

> This chart shows the number of units sold each month from January to June. The sales figures are: January 120, February 140, March 95, April 160, May 175, and June 190. The chart is a bar graph with months on the horizontal axis and sales numbers on the vertical axis.

Every figure is right.

**Reading words, live.** `atlas screen chart-test.png` took 2 seconds:
- It got the six values and the six months right, except 95, which it read as "9s".
- It ran the title together as "monthilysalestumits".
- So the fast word reader is rough on titles, and the picture reader is the one to trust for "what does this say".

**New command: `atlas picture <file> [question]` or `atlas picture screen [question]`.** It asks the same reader that "look at my screen" uses. A screenshot it takes is deleted once it's been read.

**Where they're installed:** the models are in the test folder (`AtlasLiveTest-0925\models`, plus the llama.cpp program). When the merged Atlas goes into its real folder, either move `models\` and the picture-reader folder across, or run `atlas get pictures` and `atlas get seeing` there. Both pick up where they left off and skip what's already present.

**Not tested live yet:**
- "Look at my screen" (needs the laptop unlocked).
- Faces, things and hands on the camera (need the camera, and you in front of it).

## Tests

`tests/your_answers_of_the_25th.rs`, 11 tests:

**Log**
- A log and its anchors rewritten together still disagree with a real backup.
- A log numbered to skip past its recorded heads is caught, while one that really rolled 400 entries on is fine.
- An empty log says so.

**Examples**
- Personal details are scrubbed by shape, and plain times, prices and years are left alone.
- Only graded calls are kept, the oldest go first, and the call log still has no words.
- A rewritten window reply is kept as one bad and one good example, with no email address in it.
- With the switch off, nothing is kept.
- A council seat that wouldn't commit is kept with why.

**Envelope**
- A guest profile can't ask.
- A voice that isn't yours, or might not be, is told no; yours, or typed, is answered.

**Typing**
- Typing that comes out wrong in the box is never sent.

Also: the mock platform now shows typed text in the window, the way a real text box does, and can be set to garble it like Notepad did.

## Still open

- **Unlock the laptop** for the re-run of typing and a look at the screen, and have a real call for the call notes.
- **The merge:** waiting on you.
- **The hub:** paused.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
