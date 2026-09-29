# Testing day, round two (27 Sep 2026, night)

Eric's second run, on the GitHub-signed build: a black bar across the screen, "look for improvements" ending in a raw error page, `{"error":"denied"}` on his iPhone, a command prompt when Atlas opens, and "Atlas did not seem that smart". Two agents swept the code while the first fixes went in: one for dead ends, one for why Atlas answers badly. Everything below is on `session-0927b` at 5823d3a, pushed to GitHub.

## Root causes and fixes

| what Eric saw | root cause | fixed by |
|---|---|---|
| Atlas "not smart", "can't answer that here" | The model is checked against `min(free×0.55, total/3)` of memory. On a 16 GB laptop with a browser open, the 4B model "doesn't fit", so no model is chosen. That check is made once, at start, and never again. | `models::pick`: when nothing fits what's free, the best model that fits half the machine's memory (unless a memory limit was set by hand). `keep_model_server` looks for a model again every minute. |
| Knew nothing about itself | The model's prompt had no date, no capability list and no hub pages. | Every question carries `about_now`: the date and time, what setup hasn't fetched yet, whether web lookup is on, and `capability::about_atlas`. That last is the hub pages and capabilities matching the question, with "answer only from these, name the page". |
| Reminders and facts ignored once a model exists | That whole chain ran only when there was *no* model. | `answer_before_the_model` checks reminders, stated facts, notes, ways-in, decisions and once-known things before the model. The "think with you or just listen?" question stays for when there's no model. |
| "Can you …" requests fell through | "can you" is the capabilities phrase. | `polite_rest` strips "can you / could you / please … for me" and parses what's left. |
| "What's outstanding in your setup" gave the to-do list | Phrase match on "whats outstanding". | Those phrasings now go to the self-check, which reports what setup still has to fetch and whether the model is loaded. |
| Rambling and retries | llama-server's default sampling. | Qwen's own settings for Instruct models: temperature 0.7, top_p 0.8, top_k 20, presence_penalty 1.5, and `cache_prompt`. |
| Web lookup | Off by default. | **On by default (Eric's ruling).** New phrases: "look up", "search for", "google". Settings turns it off. |
| A command prompt on opening | A console program, and child programs opening consoles of their own. | `windows_subsystem = "windows"`. The console is attached when Atlas is typed in a terminal. Every child process goes through `tools::command`, with no window. |
| Black bar | The standby typing box never painted while hidden, and Windows showed it anyway. | It's told to stay hidden on every frame and paints its colour. |
| Improvements → raw `no such endpoint` | `/hub/recommendations/go` was never routed. | Routed. "Have a go" opens self-work where it can; on an installed copy it sends the idea on as feedback. "Not worth it" drops it. The page says what happened. |
| `{"error":"denied"}` on the phone | The token-for-cookie hop was a 303. A visit started from the camera is cross-site, and browsers keep a `Strict` cookie off a redirect it began. | A page that moves itself on (meta refresh plus `location.replace`), which starts on the site. Readable pages for denied, not found and failures, each with **Back to Atlas**. |
| (sweep) Talk froze Atlas while the model answered | The turn ran inside the request. | `talk_queue`: the page shows "thinking…" at once and fills in on the next tick. |
| (sweep) Give → Send the file always failed | Form-encoded body sent to a JSON route. | JSON. |
| (sweep) A friend's document needed a terminal to accept | Only `atlas handoffs keep`. | "Waiting from friends" on the Documents page, with **Keep** and **Bin it**. |
| (sweep) About 25 messages told people to type commands | Messages written for a terminal. | Point to a page or a phrase instead. Four are kept on purpose: the vault passphrase, handover back, and the sync key are CLI-only paths with no page yet, and so is remote start. |

## Guards added

- **Every hub form posts somewhere the server answers.** A source scan, so the next dead button fails the suite.
- **A browser without the token gets the words; the app and peers get the bare answer.**
- **A page or form nothing answers leaves a way back.**
- **Talk comes straight back, and the reply lands on the next tick.**
- **Phone model and self-check:** setup and the model are in the self-check.

## Measured

- Full suite: **34 targets, 7,057 passed**. 11 failed on the first run, every one a test holding the old behaviour: research off, the 303, commands in messages, and where the notes lookup sits. Each was updated and its target re-run green.
- The Windows target builds clean (`cargo check --target x86_64-pc-windows-gnu`).

## Still open

- **Settings and several buttons still don't say what happened** (`hublive` Settings redirect, `after_button`).
- **Still done inside the request, holding Atlas up:** sending to friends (Documents Send, add-on share, adding a friend over Tor), and the phone code's `tailscale` calls.
- **Free-form requests still only reach 14 actions**, out of the roughly 100 `parse_decision` accepts. The list should come from `commands.yaml`.
- **No hub page yet** for the vault passphrase, handover back, the sync key or freeing disk.
- **Sync on a fresh install** answers "`atlas household init`".

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
