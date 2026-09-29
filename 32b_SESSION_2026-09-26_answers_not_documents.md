# Session, 26 September 2026: Atlas answers, instead of replying with documents

Eric: *"Semi-big issue found when my Atlas was being tested by a friend. Atlas would only respond in reports… literally making a document to reply."*

The friend's machine isn't reachable from here, so there were no logs from it. The cause was found by driving Atlas's own conversation path with real questions ("how are you", "what's the capital of France", "tell me a joke"…). That was done in the cloud with a real local model, and then on Eric's laptop with the real Qwen3-VL 4B through llama-server.

Seven faults were found, all on the path an ordinary question takes. Each is fixed and tested.

## What was wrong

1. **The model connection was a web browser.**
   - When Atlas builds its own connection to the model it downloads, it borrowed `research.fetch`. That ships as a hidden Chrome with `--dump-dom`.
   - So every question went to the model as a bare page load, and the model's "answer" came back as an HTML page. This is the most likely cause of the friend's "document" replies.
   - The model server now gets its own POST through `curl`, which every supported Windows has.
2. **Whatever the model returned was quoted back raw.**
   - If a reply wasn't the JSON Atlas asks for, you got "I couldn't work that out (no JSON in model reply: <html>…)".
   - Now nothing that isn't speech is ever read out: no web page, JSON or code block. Markdown decoration is stripped, and a reply that loops or runs on is cut back.
   - When the reply is unusable, the model is asked once more, plainly, to just answer.
3. **The shipped settings pointed at Ollama.**
   - A hand-written `llm:` always wins over the model Atlas downloads for itself. So on every install that model was never used, and without Ollama every question failed.
   - The `llm:` block is now commented out, with a note saying how to use Ollama instead.
4. **"How do I …" matched Atlas's own internal procedures on any shared word.**
   - "How do I make pancakes" shared "make" with "make sense of a document you're looking at". So Atlas read out its own steps for reading a document.
   - Matching is now by whole words, ignoring ordinary ones.
   - A how-to Atlas has no procedure for is answered by the model.
5. **Anything Atlas didn't recognise was treated as an action needing approval.**
   - It replied "I didn't catch that. Go ahead?", then took the next thing said as the yes or no ("Left it alone.").
   - Ordinary questions without a question mark got "Do you want me to think about it with you, or just listen?", with the same hijack.
   - "I'm so tired of this" got "Which one?" (the word "this" was taken as pointing at something).
   - Now an unrecognised line is answered, and never run. A question is recognised by how it starts, not only by a question mark. A new request after one of Atlas's questions is taken as itself.
6. **Typing at `atlas` never reached the model.**
   - The typing prompt's Atlas was built with no model at all.
   - The gate in front of it printed "blocked: 'unknown' requires explicit approval" for every ordinary question.
   - The one-shot path also only looked at `llm:`.
   - All three now use the same model connection as the voice path.
7. **On the laptop, Atlas said no model fits.**
   - It offered "mmproj-…" (half of the picture model, not something that can talk) as "the smallest I have". It also measured free memory while llama-server was already holding its model, so it concluded nothing fits.
   - Projectors are no longer counted as models. A server that's already running is used.
   - "What model" now says "Qwen3VL-4B-Instruct-Q4_K_M — it's running now, on this machine."

**Also fixed on the way:**
- The model would sometimes repeat its previous answer, or copy an old "I can't answer that" line from further up the conversation. Atlas now notices and asks again without the conversation.
- Curly quotes showed as junk in a Windows console. The console is now set to UTF-8.
- An action the model made up but nobody asked for (switching to Chrome in reply to "what's the capital of France") is dropped. The answer that came with it is kept.

**Found because of the fix:** two guest-safety tests said "you're talking to someone else now". That wasn't a handover phrase, so they had only passed because the line was swallowed by "Go ahead?". The phrase now really hands Atlas over. The tests moved to their own file (`tests/guests_at_the_daemon.rs`), with their own install, and pass for the real reason.

## Proved on the laptop

The real Qwen3-VL 4B ran through llama-server on the laptop, with the new `atlas.exe` (sha `86518b1a7490…`) in `AtlasLiveTest-0925`.

| asked | Atlas said |
|---|---|
| what's 12 times 7 | 12 times 7 is 84. |
| why is the sky blue | The sky appears blue because sunlight is scattered by the atmosphere — a phenomenon called Rayleigh scattering… |
| how do I make pancakes | Mix flour, milk, eggs, and sugar in a bowl. Heat a griddle, pour batter, flip when bubbles form. |
| recommend a good movie | suggested *The Last of Us Part I* — a game, not a film: the model's mistake, but an answer |
| give me some ideas for a birthday party | How about a themed party — like a 70s disco, a cozy movie night, or a backyard picnic with games?… |
| i'm bored | You're bored? That's a vibe I can work with. Want to pick a movie, a joke, or maybe just a quick task…? |

With no model at all, a question now gets one plain sentence saying general questions need the language model, and `atlas doctor` says what's missing. It no longer gets a report, "Go ahead?" or "Which one?".

## Tests

- `tests/answers_like_a_person.rs` (12 tests):
  - the model server is POSTed to, and no browser is involved;
  - web pages and JSON are never read out;
  - an unusable reply is asked again plainly;
  - a made-up action is dropped and its answer kept;
  - a repeated line is asked again without the conversation;
  - a how-to only gets a procedure that's about it;
  - questions aren't approvals and don't eat the next turn;
  - with a model, a question is answered in words;
  - the shipped settings use Atlas's own model;
  - typing reaches the model;
  - an old "can't answer" line from the conversation isn't taken as the answer.
- `tests/guests_at_the_daemon.rs` (3 tests).
- `tests/gguf_models.rs`: a projector is not a model.
- **Whole suite:** 6,490 passed, 0 failed.

## Honest limits

- **The friend's machine wasn't seen.** The fixes cover every path found that turns a question into a document or a non-answer. Whether their Atlas was using the browser connection, Ollama, or no model is not known.
- **Answer quality is the model's.** The 0.5B starter model tested in the cloud often answered "I'm sorry, but I can't assist with that". The 4B on the laptop answers well. A friend's machine needs the 4B (from `atlas get pictures`) or larger, and llama-server running.
- **Starting llama-server is still separate.** On the laptop it was started by hand for the test. Whether the running Atlas starts it by itself on a fresh machine was not checked here.
- **The merged line in `Atlas\atlas-current`** (the other chat's three-way merge) has the same faults: it has the same `research.fetch` connection and the shipped `llm:`. This fix is written against 26a. `atlas-answers-fix-code-only.patch` holds just the code (20 files). I checked it against the merged line with `git apply --check` (read-only, nothing changed there). It reported three files that don't apply and need merging by hand because the merged line changed them too: `src/main.rs`, `tests/which_model_fits_here.rs` and `config/tools.yaml`.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
