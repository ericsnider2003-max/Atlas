# wshobson/agents: what's in it, and how Atlas uses it

Eric, 25 Sep 2026: *"look at a very specific GitHub repo and see how we can use it in our Atlas system. Look at everything in there."*

**The repo:** https://github.com/wshobson/agents, commit `4236bb91` (13 Sep 2026), MIT licence. It has 1,174 files:

- 92 plugins, plus 2 more pulled in from other repos
- 202 agents
- 183 skills
- 105 commands
- adapter tooling for seven coding tools
- a Python evaluator, `plugin-eval`

I read all of it, split into four parts reviewed in parallel. Every agent, skill, command, hook, script and tool file was opened, not just named. Nothing from the repo was installed or run.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## What it actually is

It is **prompt content for coding assistants** (Claude Code, Codex, Cursor and others). Almost all of it is Markdown:

- **Agents** are system prompts: a `name`, `description` and `model` header, then a persona.
- **Skills** are how-to guides, loaded only when relevant, with a `references/` folder for detail.
- **Commands** are workflow scripts written as prompts.

There is very little executable code in it. What there is:

- hooks in `protect-mcp` and `review-agent-governance`
- a few shell scripts
- the Python in `plugin-eval` and `tools/`

**Quality varies a lot.** Most of the 202 agents come from one template: "Elite X expert… Use PROACTIVELY", followed by 100–300 lines of technology names. They say what the agent knows about, not how to do anything. Many are near-copies across plugins: `code-reviewer` appears 8 times and `backend-architect` 7. The worthwhile material is a small set of skills and commands with specific, checkable rules.

**Almost nothing targets what Atlas is:** Rust, Windows, voice, and a small local model. It is written for large cloud models doing web and cloud development.

## How Atlas can use it

There are four ways, and one hard line.

### 1. Rebuilt into Atlas's own code (done in this session, tested)

I rewrote four ideas into Atlas. Nothing was copied in wholesale, and attribution is in `atlas/THIRD_PARTY_NOTICES.md`.

| Idea | Where it came from | What Atlas does with it now |
|---|---|---|
| Pre-build risk checklist | `before-you-build` | **"Should I build this?" gets its own room** (`council::build_room`). Five seats each argue one way a working thing still fails: demand, the first user, money, reach, trust. They answer blind, like every room. "Ask the room: should I build the hollow CLI?" now convenes it. |
| STRIDE threat model | `security-scanning` | **"Is this safe?" gets its own room** (`council::security_room`). Five seats: impostor, tamperer, eavesdropper, wrecker, climber. Useful for the phone link, guest handover and the WireGuard path. |
| Machine-writing tells | `avoid-ai-writing` | **Replies written as you are read back before they go out.** Atlas's existing draft checker gained a *chatbot voice* fault ("I hope this helps", "Certainly!", "Feel free to reach out", "As an AI"), a *blank* fault ("[Your Name]"), and more filler words. A window reply that trips one is rewritten once, and the rewrite is kept only if it's better. **A reply with a blank in it is never sent:** it goes in the box and the job pauses for you. Chat is casual, so "maybe" or a question back is not flagged. |
| Claim-checked notes | `documentation-standards/grounded-vault`, `content-marketing/search-specialist` | **A research figure must be in the pages it came from** (`research::figures_not_in`). No model is needed. Each number is matched whole ("15" isn't found inside "150"), and single digits and names like "B2" or "4B" are ignored. A figure that isn't found is said as unconfirmed ("5.9 isn't in any of the pages I read") and listed under its own heading in the saved note. The research prompt now also says to copy figures exactly and name the source for key claims. |

**Every model call these make is recorded.** The window reply and its rewrite are recorded separately.

### 2. Install into your Claude Code chats to help build Atlas (your call)

These don't belong inside Atlas; they help whoever is writing Atlas. In Claude Code:

```
/plugin marketplace add wshobson/agents
/plugin install conductor
```

They aren't in your Cowork plugin catalog, so installing them is a Claude Code step. Worth installing, in order:

- **conductor.** Context-driven development: product and tech-stack docs, "tracks" of `spec.md` plus `plan.md` with `[ ]`/`[~]`/`[x]`, and test-first work. Reverts only by `git revert`, and it asks for a literal YES first. It has no Rust style guide; add one.
- **operating-kit.** Session-start / session-end agents. They read the state doc, check it against what is actually live, and flag a MISMATCH before any work starts. Point `{{STATE_DOC}}` at `claude/atlas-handoff-current.md`. This is the discipline that stops the other two chats working from stale docs.
- **agent-teams.** Parallel review and debugging:
  - One owner per file.
  - Findings need `file:line` evidence.
  - Debugging weighs competing explanations against the evidence.
  - It needs Claude Code's experimental agent-teams switch.
- **systems-programming**, the `rust-async-patterns` skill only: don't hold locks across `.await`, JoinSet, cancellation.
- **tdd-workflows** (`tdd-cycle`): red, green, refactor with checkpoints.
- **c4-architecture.** Run it once over the Atlas tree for architecture docs.
- **avoid-ai-writing**, for the handoff docs.
- **ui-design**, only when the phone/iPad app or the colony viewer is being built.

**Don't install the whole marketplace.** Most agents say "Use PROACTIVELY", so they fire on their own and fill the context.

### 3. Design ideas for later Atlas work (named, not built)

These are good ideas that need your rulings or a larger piece of work. Each is listed with where it comes from.

- **Grade Atlas's recorded model calls, so they can one day tune its small model** (`llm-finetuning`). The plugin's hard rule: *"No eval harness, no fine-tune."* Read about 100 recorded calls, sort the failures into a handful of kinds, write one pass/fail check per kind, and hold out a test set. Two sources of ready-made labels already exist: your edits to window drafts, and your yes/no on the improve queue. Personal data must be scrubbed before any row is used. **Your call whether Atlas should keep those labels.**
- **Tamper-evident activity journal** (`signed-audit-trails`, `protect-mcp`). Each entry carries the hash of the one before and is signed with an ed25519 key, so a changed record shows. Their code is not usable: it fails open, signs less than it claims, and pulls an npm package on every call. The idea is small in Rust.
- **Improve queue as resumable phases** (`comprehensive-review/full-review`, `ship-mate`, `superself`):
  - Each phase writes a file, and a `state.json` records where the work is.
  - Explicit checkpoints for you, and a cap on review loops.
  - "Done" needs evidence.
  - Corrections replace an entry rather than editing it.
- **Search that measures itself** (`llm-application-dev`):
  - Keyword plus meaning search merged by rank, because pure meaning search misses names and codes.
  - A fixed set of known questions from the fact book, scored with precision@k, MRR and nDCG every time the embedding model changes.
  - Never mix two embedding models in one index.
- **Notes that go stale on their own** (`grounded-vault`). A note about one of your repos records the commit it was written at. `git diff --stat <sha>..HEAD -- <paths>` then says whether it's out of date, with no model call.
- **Runbook fields for `knowhow`** (`incident-response`): prerequisite, "if this fails", expected output on every step; a quick checklist at the top; a blameless 5-whys after a failure.
- **Ordering what comes next** (`machine-learning-ops` recsys skill): fetch candidates → add details → filter → score → pick the top few → log without blocking. That is a clean shape for "which errand or notice goes first".

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

**quantitative-trading** is thin. It has two short agents and two skills, and its example code has real bugs:

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*
- In the vectorised version, `num_trades` counts days in the market, not trades, and `win_rate` is per day.

*[Removed 28 Sep 2026: trading-system material, kept in the private archive.]*

## Don't install these, and why

- **file-conversion** uploads the whole file, base64-encoded, to changethisfile.com. The plugin author owns that service. This conflicts with Atlas's "what leaves the machine" rules; ffmpeg, LibreOffice or pandoc do the same job locally.
- **protect-mcp / review-agent-governance / signed-audit-trails:**
  - They run `npx protect-mcp` on every tool call, which downloads from npm.
  - They allow everything if the policy file is missing.
  - The agent can approve itself with `touch ./.review-approved`.
  - Receipts cover only the tool name, though the docs say more.
  - Their hook setup edits `settings.json`, and the signed-audit-trails setup uses a hook format that wouldn't fire.
- **block-no-verify:** its "global setup" snippet overwrites `~/.claude/settings.json` completely, and its regex misses `git commit -n`. Atlas's build sandbox could refuse hook-bypass flags itself if that's ever wanted.
- **pensyve** (external memory plugin): not pinned to a commit, has lifecycle hooks, and its code isn't in this repo to review.
- **HOL Guard** (external): pinned, but unreviewed and able to change hook settings.
- **brand-landingpage, meigen-ai-design, hermes-tweet, social-publishing** each depend on a third-party service with an API key:
  - brand-landingpage: Google Stitch
  - meigen-ai-design: MeiGen Cloud (`/gen` skips confirmation)
  - hermes-tweet: Xquik
  - social-publishing: SocialClaw
- **plugin-eval** grades a prompt's *form*, and its deeper layers are weaker than they look:
  - The LLM "triggering" score is the judge marking its own guesses.
  - Its Monte Carlo "quality" is output length.
  - Its Elo module is only reached by its own tests.

  Worth one note for **hollow**: it is a live example of exactly what hollow detects, a working module that nothing uses.

## For hollow

- **skill-forge-essentials' ai-debt-detector** is a five-point checklist, not a tool. Its "orphans" and "hallucinated dependencies" categories are worth adding to hollow's rules.
- **doc_gardener.py** attaches a "Fix:" line to every finding, which is a good output pattern.
- **plugin-eval's dead Elo** is a public example hollow would catch.
- **startup-business-analyst** has market-sizing and pricing skills for hollow's business side. The new "should I build this?" room is the quicker first pass.

## Tests

`tests/what_atlas_took_from_wshobson_agents.rs` (9 tests) checks that:

- the two rooms are chosen for the right questions, can disagree, and answer blind;
- asking the room about building something actually convenes the build room, and its seats are asked;
- a chatbot's voice and a blank are faults, while a normal casual reply is not;
- a reply that sounds like a chatbot is rewritten before it's typed, with both calls made;
- a reply with a blank is put in the box, never sent, and the job pauses;
- figures are matched whole, and names and single digits are ignored;
- end to end, a research answer with a made-up figure is said as unconfirmed and the figure is filed under its own heading in the note.

---

MEASUREMENTS. NO VERDICT. ERIC RULES. · NOT FINANCIAL ADVICE
