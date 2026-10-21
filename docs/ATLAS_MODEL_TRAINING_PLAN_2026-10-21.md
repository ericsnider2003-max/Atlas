# Building and training Atlas's own model: the breakdown
Last updated: 2026-10-21 · Updated by: Claude (research session) · Status: plan — nothing exported, downloaded or trained

Related docs:
- docs/OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md
- docs/OFFLINE_ASSISTANT_DESIGN_2026-10-20.md
- docs/ATLAS_EFFICIENCY_AND_ADAPTATION_DESIGN_2026-10-20.md

Tags used for every number: measured (today), recorded (an Atlas file), sourced (a URL), estimate, proposed.

## One-screen summary

- Data: Eric's own typed messages (about 650 short ones already on the laptop, measured), plus open datasets with clean licences, plus Atlas's own command list.
- Not used: any AI reply from Claude or ChatGPT. Not used: datasets made by OpenAI models, or by an unknown maker unless a sample check passes.
- Stage 1: a small intent and command model (laptop CPU, $0, minutes to an hour, estimate).
- Teacher run for paraphrases and replies: about $12-35 on 2 rented H100s (estimate).
- Stage 2: LoRA fine-tune of the 1.7B or 4B base on a rented GPU (about $2-10, 2-6 h, estimate).
- Stage 3: preference tuning from Eric's approvals, later (a few hundred to a few thousand pairs, estimate).
- First full cycle: about $15-45, budget $20-60 to allow one rerun (estimate). Eric's review time is the long part: a few evenings (estimate).
- Nothing leaves the laptop without Eric's OK, and only after private info is removed (proposed).
- Step 1: build a local extractor for Eric's typed messages, with private info removed. Eric checks 50.
- Step 2: build a 200-message held-out test and measure today's Atlas on it.
- Step 3: Eric approves the first small downloads (MASSIVE, CLINC150, the base model) and Stage 1 is trained on the laptop.

## What Eric asked for

Eric said:

> "realistically for training it can use how i talk to you. Not necessarily your responses but how a person interacts. On top of that it can use examples from the internet, along with the REA and other like git hub repos."

This plan follows that in three parts.

| Part | What it is | Section |
|---|---|---|
| How Eric talks | His side of his conversations, not the AI replies | Source A |
| Examples from the internet | Open datasets with clean licences | Source B |
| REA and other GitHub repos | Methods and Atlas's own repo; honest about what they give | Source C |

## Source A: Eric's own side of his conversations

### Why this is allowed

- Anthropic Consumer Terms section 4 (effective 8 Oct 2025, sourced https://www.anthropic.com/legal/consumer-terms): "you retain any right, title, and interest that you have in the Inputs you submit".
- Outputs are assigned to the user only "subject to your compliance" (sourced, same page).
- The competing-model restriction applies to outputs: Anthropic Commercial Terms §D.4 bar using the services "to train competing AI models" (sourced https://www.anthropic.com/legal/commercial-terms); the matching consumer-terms clause is unverified.
- So the rule is simple: use Eric's messages, never the AI replies.
- The OpenAI terms page was blocked (HTTP 403) when checked.
- A secondary source says outputs are assigned subject to compliance. This is unverified.
- Treat ChatGPT the same way: Eric's messages only.
- Caution: Eric's messages sometimes contain pasted AI text, handoffs or docs.
- Those parts are filtered out (see the cleaning pipeline).

### Already on the laptop

Counts only. No message text was read.

| Source | Location | Size | Typed messages | Median length |
|---|---|---|---|---|
| Claude Code transcripts | `C:\Users\erics\.claude\projects` | 787 files, 288 MB (measured) | 504 unique (measured) | 255 words (measured) |
| Codex sessions | `C:\Users\erics\.codex\sessions` | 51 files, 648 MB (measured) | 611 unique (measured) | 14 words (measured) |

Length buckets (all measured today):

| Source | 60 words or fewer | 61-300 words | Over 300 words |
|---|---|---|---|
| Claude Code | 100 | 155 | 249 |
| Codex | 556 | 55 | 0 |

- The long Claude Code messages are probably pasted handoffs and dispatch context (estimate; their text was not read). The cleaning step drops pasted blocks either way.
- About 656 short, command-like messages in total (measured: 100 + 556). This is the best kind for training.
- 200 of them are locked away as the test set, leaving about 450 real messages for training (estimate).

### Exports Eric can request

| Service | How | Notes |
|---|---|---|
| Claude | Settings > Privacy > Export data. Web or desktop app only. | The emailed link expires in 24 h (sourced https://support.claude.com/en/articles/9450526-how-can-i-export-my-claude-data). The page does not state the file format. "conversations.json" is unverified. |
| ChatGPT | Settings > Data controls > Export. | Emailed ZIP, reportedly with conversations.json. Keep only messages where author.role is "user". Secondary sources, unverified. The official page was blocked. |

- These exports would add older conversations that are not on the laptop.
- Nothing has been requested yet.
- Eric decides whether to request them.

### Cleaning pipeline (proposed)

All of it runs locally. No text is sent anywhere.

| Step | What happens |
|---|---|
| 1 | Keep only the user's turns. |
| 2 | Drop pasted content: blocks over 300 words, code fences, quoted AI text, handoff blocks, "Context from Dispatch" blocks. |
| 3 | Remove duplicates. |
| 4 | Remove private info (below). |
| 5 | Eric reviews a sample. |

Step 4 in detail:
- Tool: Microsoft Presidio (MIT, sourced https://github.com/microsoft/presidio).
- Plus spaCy named-entity recognition (MIT).
- Names, emails, phones, addresses, account numbers and keys become placeholders such as `<PERSON>` or `<EMAIL>`.
- Placeholders keep the sentence shape, which is what the model needs.

Step 5 in detail:
- Eric reads a sample in a reviewable file.
- If the sample shows leaks, the filters are tightened and the run is repeated.
- Nothing moves on until the sample is clean.

### Labelling (proposed)

Each cleaned message gets four labels.

| Label | Meaning |
|---|---|
| Intent | One of Atlas's commands from `atlas/config/commands.yaml` (about 162, recorded), or "chat", "question" or "other". |
| Slots | The parts the command needs: file, person, time, app and so on. |
| Expected Atlas action | The tool call or the reply type. |
| Tone and style notes | Short, blunt, polite, and so on. |

How it is done:
- A local open model pre-labels the messages.
- Eric confirms or corrects in batches.
- Never use Claude or ChatGPT to write the label content that becomes a training target.
- Claude or Codex may write the labelling tool code. Code is not training data.

### Speech

- For the voice side, Eric's own recordings can be added later, with his consent.
- None are recorded now.

## Source B: open datasets

Every row below comes from the dataset's own card or page, at the URL given (sourced).

### Permissive and clean: the ones to use

| Dataset | Licence | Size | Contents | Source |
|---|---|---|---|---|
| Amazon MASSIVE | CC-BY-4.0 | 11,514 train / 2,033 dev / 2,974 test per language; 52 languages; 60 intents; 55 slots | Crowd-worker voice-assistant commands | https://huggingface.co/datasets/AmazonScience/massive |
| SLURP | Text CC BY 4.0. Audio CC BY-NC 4.0, so text only. | not recorded | Voice-assistant commands | https://github.com/pswietojanski/slurp |
| CLINC150 | CC-BY-3.0 | Small set 7,600 train; plus set 15,250 train | 150 intents plus out-of-scope | https://huggingface.co/datasets/clinc/clinc_oos |
| SNIPS | CC0 on the official mirror (from a search snippet; recheck) | not recorded | Built-in intents | https://huggingface.co/datasets/sonos-nlu-benchmark/snips_built_in_intents |
| Schema-Guided Dialogue | CC-BY-SA-4.0 (share-alike) | 22,825 dialogues, 17 domains | Task dialogues | https://huggingface.co/datasets/google-research-datasets/schema_guided_dstc8 |
| Dolly-15k | CC-BY-SA-3.0 | 15,011 | Human-written, no AI | https://huggingface.co/datasets/databricks/databricks-dolly-15k |
| HelpSteer3 | CC-BY-4.0 | 132,937 rows | Responses from permissively licensed models only, per the card | https://huggingface.co/datasets/nvidia/HelpSteer3 |
| BFCL | Apache-2.0 | not recorded | Hand-written executable functions. Use for TESTING tool calls, not training. | https://huggingface.co/datasets/gorilla-llm/Berkeley-Function-Calling-Leaderboard |

Notes:
- Share-alike licences (Schema-Guided Dialogue, Dolly) may require the same licence on derived data. Check before shipping any model or data made from them.
- BFCL is kept out of training so it stays a fair test.

### Usable with checks

| Dataset | Licence | Size | Caveat | Source |
|---|---|---|---|---|
| OASST2 | Apache-2.0 | about 135k messages | Authorship unstated. Use the human-ranked parts. | https://huggingface.co/datasets/OpenAssistant/oasst2 |
| glaive-function-calling-v2 | Apache-2.0 | 112,960 | Generator unknown | https://huggingface.co/datasets/glaiveai/glaive-function-calling-v2 |
| Hermes function-calling-v1 | Apache-2.0 | Small subsets | Generator unknown. Includes Glaive. | https://huggingface.co/datasets/NousResearch/hermes-function-calling-v1 |
| ToolACE | Apache-2.0 | 11.3k | Generator unknown | https://huggingface.co/datasets/Team-ACE/ToolACE |
| When2Call | CC-BY-4.0 | 15k SFT plus 9k preference | Teaches when NOT to call a tool | https://huggingface.co/datasets/nvidia/When2Call |
| Magpie Llama-3.1 | llama3.1 licence | 300k | Llama-made. Licence flow-down applies. | https://huggingface.co/datasets/Magpie-Align/Magpie-Llama-3.1-Pro-300K-Filtered |
| smoltalk | Apache-2.0 for new subsets. Older subsets follow upstream. | not recorded | Exclude the openhermes subset. | https://huggingface.co/datasets/HuggingFaceTB/smoltalk |

What "with checks" means:
- Where the generator is unknown, the data may have been made by a model whose terms forbid training rivals.
- Before use, sample the rows and look for tell-tale AI phrasing.
- If the check is not clean, leave it out.

### Excluded

| Dataset | Reason |
|---|---|
| UltraChat | "Generated by ChatGPT" |
| Alpaca | OpenAI-made, non-commercial |
| OpenHermes-2.5 | GPT-4, likely |
| ToolBench | gpt-3.5, research only |
| LMSYS-Chat-1M | Custom licence, contains proprietary-model outputs |
| WildChat | GPT responses, likely |
| Tulu-3 mix | Contains a GPT-4 subset. Left out; could be reconsidered later with that subset removed and rechecked. |
| xLAM-60k | Card says research purposes only |
| SLURP audio | Non-commercial licence |

Rule: when unsure, leave it out.

### Mapping public data to Atlas

- MASSIVE and CLINC150 use their own intent names.
- Each one is mapped by hand (proposed) to the nearest Atlas intent in `atlas/config/commands.yaml`.
- Intents with no Atlas match are dropped, or become "other".
- CLINC150's out-of-scope examples help the model say "I don't know" instead of guessing.
- When2Call does the same job for tool calls: it teaches when not to call a tool.
- Wrong actions are the costly mistake. The Scorecard allows at most 0.5% (recorded).

## Source C: REA, Replica and similar repos (honest)

### REA

- Repo: https://github.com/morluto/rea, MIT licence.
- About 350k stars per the GitHub API today (measured).
- Its description: "Reverse engineer anything with agents, from app behavior down to native binaries".
- What it gives: a method and agent workflows. Observe app behaviour, record evidence, recreate.
- What it does not give: a training dataset. It has none.
- What Atlas can do with it: turn its task types into tool schemas.
- Also: make synthetic "observation then action" practice examples for desktop tasks.
- What it cannot do: teach language or Eric's style.

### Replica

- Repo: https://github.com/Jakeschincariol/replica-skill, MIT licence.
- Its description: "Eleven free Claude skills that clone any app".
- What it gives: prompt procedures for reverse-engineering, rebuilding, testing and fixing apps.
- The procedure text is MIT, so it can become task descriptions and checklists.
- These are skills written for Claude. They read as human-written instructions rather than model outputs (unverified).
- No data.

### Atlas's own repo

- This is the best "repo" source.
- `atlas/config/commands.yaml` is the intent list.
- The phrasebook and intent tests show how commands are phrased and what is expected.
- The tool definitions are the exact schema the model must learn.

### Out of scope

- Code-understanding data (large code corpora) is out of scope for the assistant model.
- Coding stays with the coder model or the cloud (see the 18 Oct plan, S10).

## Synthetic data from a local or open model

### Teacher

- An open-weight model whose licence allows it: Qwen3-235B-A22B (Apache-2.0) on a rented GPU.
- Or the chosen base model itself on the laptop, for paraphrases.

### Uses

- Paraphrase Eric's real messages into variants (about 10 per message, estimate). Only messages with private info already removed, and only with Eric's OK if they go to a rented machine. The laptop's own base model can do this step instead, with nothing leaving the laptop.
- Write replies in Atlas's voice for his real inputs.
- Make tool-call examples from Atlas's schemas.

### Checks on every synthetic example

| Check | Pass condition |
|---|---|
| Parses | The JSON or call is well formed. |
| Tool exists | The named tool is in Atlas's tool list. |
| Slots match | Required slots are present and of the right type. |
| Human review | Eric reviews a sample. |

- Never use Claude or ChatGPT outputs.
- Synthetic examples are never placed in the test set.

## Data volumes (estimates)

| Stage | Data |
|---|---|
| 1 Intent and command model | About 50-100 examples per intent times about 162 intents gives about 8k-16k. Made from: Eric's real labelled messages (about 650) plus MASSIVE and CLINC mapped to Atlas intents plus paraphrases. Held-out test: 200 of Eric's real messages never used in training. |
| 2 LoRA on the base | About 5k-20k examples. Tool calls (Atlas schemas plus When2Call; ToolACE and glaive only if their sample check passes). Dialogue (Dolly, OASST2 human parts, HelpSteer3). Eric-style replies (his inputs plus teacher replies he approves). |
| 3 Preference tuning (later) | A few hundred to a few thousand pairs of approved versus rejected answers from Eric's real use. |

A worked example of the Stage 1 maths (estimate):
- 162 intents times 50 examples is 8,100.
- 162 intents times 100 examples is 16,200.
- About 650 real messages cover only a fraction of 162 intents.
- So most examples come from public data and paraphrases.
- Eric's real messages matter most for the test set and for style.

## Training stages

### Stage 1: intent and command model

Option A:
- SetFit (Apache-2.0, sourced https://github.com/huggingface/setfit).
- Built on the MiniLM encoder Atlas already ships (about 90 MB, recorded).
- Trains on the laptop CPU in minutes (estimate).
- Its published example uses 8 examples per class (sourced).
- Output: an intent label.

Option B:
- Fine-tune Qwen3-0.6B to output intent plus slots as JSON.
- Heavier, but returns slots directly.

Where it sits:
- The output sits in front of the phrase table's miss path (proposed).
- The phrase table stays first. The model only handles what the table misses.

### Stage 2: LoRA SFT on the base

- Base: the 1.7B or 4B Apache-2.0 model chosen in docs/OFFLINE_ASSISTANT_DESIGN_2026-10-20.md.
- Method: QLoRA with Unsloth (Apache-2.0) or TRL.
- Export: the adapter, loaded by llama.cpp with `--lora`.

### Stage 3: evaluation, then later preference tuning

- Every candidate goes through the promotion gate (see Evaluation).
- Later: preference tuning (DPO) from Eric's approvals and rejections.

### The three stages side by side

| Stage | Input | Output | Runs on |
|---|---|---|---|
| 1 | Labelled short messages | Intent and slots | Laptop CPU |
| 2 | Tool-call, dialogue and style examples | A LoRA adapter | Rented GPU |
| 3 | Approved versus rejected answers | A refined adapter | Rented GPU, later |

## Hardware, cost, time (estimates)

| Stage | Where | Cost | Time |
|---|---|---|---|
| Cleaning and labelling | Laptop | $0 | The long part: a few evenings of Eric reviewing batches |
| Stage 1 | Laptop CPU | $0 | Minutes to an hour |
| Teacher run | 2 rented H100s at $2.89-3.49 per GPU-hour (sourced in docs/OWN_LANGUAGE_MODEL_OPTIONS_2026-10-20.md), so about $6-7/h for the pair | About $12-35 | 2-5 h |
| Stage 2 QLoRA | Rented RTX 4090 ($0.34-0.89/h, estimate) or A100 80GB ($1.39-1.59/h, sourced in the same doc) | About $2-10 | 2-6 h |
| Eval | Laptop | $0 | Under an hour |

- First full cycle: about $15-45; budget $20-60 to allow one rerun (estimate).
- Qwen3-235B-A22B at 4-bit is about 130 GB (estimate), which fits in 2 × 80 GB but is tight. If it does not fit, use a smaller open teacher or 3 GPUs.

### Why Stage 2 is rented

- Laptop GPU training is unproven.
- There are no published fine-tune numbers for the Arc 140V.
- A PyTorch XPU correctness bug is reported on Lunar Lake (sourced https://github.com/pytorch/pytorch/issues/200713).
- So Stage 2 uses a rented GPU until the laptop path is proven.

### Data handling for rented machines (proposed)

- The cleaned dataset goes to a rented machine only with Eric's OK.
- It is sent encrypted.
- It is deleted after the run.
- The rental provider is named to Eric before anything is sent.
- The adapter comes back. The data does not stay.

## Keeps learning without bloating (proposed)

- Approved interactions go into a capped local example store.
- "Approved" means Eric accepted the answer, or corrected it and then accepted.
- Cap: 20,000 examples, about 50 MB (estimate).
- The store is deduplicated.
- Retraining is a batch job when 200 or more new approved examples exist (proposed). It is not continuous.
- Keep only the current adapter and the last-good adapter.
- All deletions or evictions are report-then-ask (Eric's rule).
- Raw transcripts stay where they are. Atlas does not copy them.
- The design that ties this to the rest of Atlas is in docs/ATLAS_EFFICIENCY_AND_ADAPTATION_DESIGN_2026-10-20.md.

## Evaluation against the Scorecard

### The gate

A new model or adapter is used only if both are true:
- It beats the current one on the golden sets.
- It does not slow things down.

### What is checked

| Check | Scorecard rows | Target |
|---|---|---|
| Intent accuracy | 13 | 95% or more |
| Compound commands | 14 | 90% or more |
| Wrong action | 15 | 0.5% or less |
| Speed | 1-6 | No slower than now |
| Memory | 11-12 | No higher than now |
| Recall | 16-18 | No worse than now |
| Tool-call accuracy | BFCL-style tests | Beats the current path |
| Eric's blind preference on 50 replies | 18 Oct plan | 70% or more |

- Targets for rows 13-15 are recorded in the Scorecard.
- The 70% preference target is from the 18 Oct plan (recorded).

### Fair testing

- Run the same tests on today's online path for comparison.
- Never train on test items.
- The 200 held-out messages are locked before any training.
- BFCL is never used in training.
- A failed model is reported with the numbers. It is not hidden.

## First 3 concrete steps

### Step 1: build the extractor

- Local only. No model download. Needs Presidio and spaCy with its small English model installed (a small install, Eric's OK).
- Pull only Eric's typed messages from the Claude Code and Codex logs on the laptop.
- Drop pasted blocks.
- Run private-info removal.
- Save a reviewable file.
- Eric checks 50 messages.

### Step 2: baseline

- Build the 200-message held-out test set.
- Build the intent label list from commands.yaml.
- Measure today's Atlas (phrase table plus online path) on it. The online path only sees messages with private info removed, and only with Eric's OK; otherwise measure the phrase table alone.
- Labels for the test set are written by Eric (with the tool's help), so no local model download is needed yet.
- That result is the baseline every later model must beat.

### Step 3: first training

- Eric approves the first downloads. MASSIVE and CLINC150 are small. The base model comes from the OFFLINE doc.
- Train the Stage 1 intent model on the laptop.
- Compare it with the baseline from Step 2.

## Risks

| Risk | What reduces it |
|---|---|
| Private info leaks into training data | Local-only cleaning, placeholders, Eric reviews a sample |
| Pasted AI text slips into the data | Length and code-fence filters, quoted-text filter, sample review |
| Too few real examples | Public data and paraphrases, with real messages kept for test and style |
| A licence turns out wrong | Leave-out rule when unsure; recheck cards before download |
| Model is worse than today's path | Promotion gate; the old path stays |
| Laptop GPU training fails | Rented GPU for Stage 2 |
| Costs grow | Each rental is a separate approval; first cycle estimated at $20-60 |

## Open questions

- Export formats are unverified.
- The OpenAI terms text could not be read.
- The licences of HWU64, MultiWOZ, ATIS and Taskmaster are not checked.
- The OASST2 authorship is unstated.
- The generators behind the glaive and ToolACE data are unknown.
- Whether to send cleaned data to a rented GPU at all, or wait until laptop training works.
- SNIPS being CC0 comes from a search snippet and needs a recheck on the card.

