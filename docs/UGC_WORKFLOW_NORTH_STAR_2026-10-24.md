# North star: Atlas runs the faceless-UGC workflow, end to end (24 Oct 2026)

**Status: planning only.** This is a parked roadmap, not a build order. The feature freeze (charter §4) holds. Nothing here starts until Foundation Recovery is accepted, and then each phase starts only when Eric chooses it. Charter §10 is where this belongs: once Atlas reliably replaces the paid-AI workflow, it can become a content system.

## The goal

Eric's UGC work today runs on Claude, ChatGPT and Grok, with a Notion board as the record. Grok carries most of the creative load, so it is a single point of failure. The goal is for Atlas to run the whole flow:

1. Ideation
2. Script
3. Voiceover
4. Edit and assembly
5. Client pitch
6. Posting

Atlas owns the record and the order of work. Eric owns every decision that leaves the machine.

## Approval gates (all phases, never relaxed by a phase)

| Action | Rule |
|---|---|
| Post or publish | Only on Eric's yes to that exact post: final file, caption, account and time. A standing schedule counts only if Eric set it himself, per account (charter §8). |
| Send to a client or brand | Drafts are held for Eric. Sending happens on his yes, or under a standing approval he configured for that thread. Cold outreach goes only to people Eric named. |
| Pay, buy, invoice-and-collect, sign up for a paid tool | Never automatic. Atlas prepares; Eric does it. |
| Clone a voice | Eric's own voice, or a voice whose owner gave written consent on file. Never a client's, a celebrity's or a stranger's. Local only. |
| Use a cloud model (Claude, ChatGPT, Grok) | Only where Eric has turned it on, and named in the job's record. A job must still finish when a provider is off (see the last section). |

Every gate shows the exact thing being approved, not a summary. Every approval is recorded against the job, so "what went out and who said yes" can always be answered.

## Phases

Each phase lists what Atlas owns once the phase is done, what it still needs, and the existing work that feeds it. "Built, never run for real" capabilities come from `docs/CAPABILITIES.md`. Under "unproven is broken", each one counts as missing until a real run proves it.

### Phase 0: one record instead of the Notion board

- **Atlas owns:** the content board. Each piece is an Atlas job (ideas → scripted → voiced → cut → approved → posted), with its assets, versions and approvals attached, shown on the Hub's Outstanding page.
- **Needs:**
  - a one-time import of the Notion board from Notion's own export (Atlas has no Notion connection);
  - a content-job type on the durable jobs (F1) that already exist;
  - the Creative Workspace Core project model (`CREATIVE_WORKSPACE_CORE_SCOPE_2026-10-18.md`).
- **Feeds from:** Plan A "what needs me?" (a piece waiting on Eric shows up there); Plan B taught routines ("when I say edit day…").

### Phase 1: ideation and scripts, with Eric choosing

- **Atlas owns:**
  - trend and niche watching (`social` public feeds, `content`/`reach`);
  - a shortlist of ideas with why each one;
  - first-draft scripts in Eric's house style, learned from scripts he kept.
- **Needs:**
  - a hook/script format per platform;
  - a style memory built from accepted versus rejected drafts;
  - the council (`council`) for a second opinion that disagrees.
- **Feeds from:** Plan G prompt economy (local drafting has to be fast enough to use); Plan C tiers (which model this machine can draft with).

### Phase 2: voiceover

- **Atlas owns:**
  - the voiceover from the approved script (`voiceover`, `kokoro`, Piper);
  - timing against the footage;
  - music ducked under the voice.
- **Needs:**
  - Kokoro/Piper installed and proven on this machine;
  - the optional local voice clone from the Plan C voice track, held to an agreed quality bar before it is offered, and only under the consent gate above;
  - pronunciation fixes kept per brand.
- **Feeds from:** Plan C pre-flight (says whether the voice set fits before anything downloads); Plan D (rendering runs in the background without taking the screen).

### Phase 3: editing and assembly

- **Atlas owns:**
  - first cuts from a description (`edit`, `studio`): dead air cut, captions, thumbnail picks, title ideas;
  - cut checks (`cutcheck`);
  - a frame check before posting (`opsec`).
- **Handoff:** to an editor Eric already owns (`editors`) when the job needs a human pass, with the project and its record going both ways.
- **Needs:**
  - the Creative Workspace video lane (timeline, captions, deterministic render, recovery);
  - per-platform export presets.
- **Feeds from:** Plan D; Plan E (the Hub's job pages stay readable as they grow).

### Phase 4: client pitching

- **Atlas owns:**
  - pitch and follow-up drafts (`outbox`, `outreach`, `draft`), built from the portfolio of posted work and its numbers (`social` analytics);
  - a lead list from the daily gig look (it never applies or replies by itself).
- **Needs:**
  - a portfolio page or pack generated from posted pieces;
  - a per-client thread record;
  - rate cards Eric wrote.
- **Gate:** every send. Atlas never quotes a price Eric hasn't set.

### Phase 5: posting and the loop back

- **Atlas owns:**
  - the posting queue (`publish`): the final file, caption and account, posted at the approved time;
  - the per-account numbers afterwards;
  - "why that one worked", fed into Phase 1's next shortlist.
- **Needs:** per-platform posting, proven for real on each account Eric uses. Until a platform is proven, Atlas prepares the post and Eric posts it.
- **Gate:** every post, or Eric's own per-account schedule.

## How this removes the Grok single point of failure

- **The record lives in Atlas, not in a chat.** Ideas, scripts, versions and approvals sit in Atlas's own job store with backups. Losing a provider, an account or a chat history loses no work.
- **Local first, cloud optional.** Each step has a local path: local model drafts, local voice, local render. A cloud model is an extra opinion, not the only route. If Grok is down or gone, the job continues on the next provider Eric enabled, or on the local model, and the record says which one produced each draft.
- **The council instead of one oracle.** Ideation and script review can put the same question to several seats (local plus whichever providers are on) and bring back where they disagree, rather than taking one model's word.
- **The prompts are Atlas's.** Eric's house style, formats and examples are kept in Atlas, not inside one provider's custom instructions. Moving to another model means pointing Atlas at it, not rebuilding the setup.
- **Proof, not a promise.** Each phase is accepted only when a real piece goes through it with one provider switched off and still finishes.

## Tie-in to the video-analysis plans

| Plan | Role here |
|---|---|
| A: what needs me | Content jobs waiting on Eric surface in the short list |
| B: teachable routines | "Edit day", "posting hour" taught in plain words |
| C: tiers and voice | Says what this machine can run; the optional own-voice clone |
| D: hands off the screen | Renders, fetches and drafts run without taking the mouse |
| E: not slop | The Hub's board and approval pages stay clear as content grows |
| F: read-only what-if | Not used here; the same "label it, never act on it" rule applies to performance forecasts |
| G: prompt economy | Local drafting fast enough to replace a cloud chat |

## Out of scope

- Paid ads, buying followers or engagement, auto-replying to comments, and any payment flow.
- Posting to any account without Eric's per-post or self-set approval.
- Voices of anyone who hasn't consented in writing.
