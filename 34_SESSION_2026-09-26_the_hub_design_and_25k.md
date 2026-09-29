# Session — 26 Sep 2026: the hub design, and 25k

## What Eric said

"One of the chats when it was doing a merge still didn't pick up the hub design and was still defaulting to the old one.... that is an issue."

## What had actually happened

- **The design never reached the code.** The hub design was locked with Eric on 20–21 Sep, on the pinned canvas "Atlas Hub — Command Deck" (30 artboards), with a written spec in the Atlas Improvments project (`claude/hub-design-decisions-20sep.md`). None of it had been brought into the tree. Only the palette was ported (24 Sep). A second canvas, "Atlas Hub" (24 Sep), redrew the same direction with the phone screens, and it wasn't in the tree either.
- **The only design file in the tree was the wrong one.** It was `design/command-deck.html`, the 20 Sep mock-up delivered "awaiting your steer" *before* the design was locked: a dark icon rail with cards. So every chat merging code took it for "Eric's design".
- **The merge made the mock-up the hub.** The 26 Sep three-chat merge asked Eric to choose between "the command deck" and "Warm Paper", as if the only question were colour. It then recoloured the mock-up in Warm Paper and pinned the hub to the mock-up in `tests/command_deck.rs`.
- **The mock-up was missing the design's substance:**
  - the labelled Notion-style sidebar and the Personal / business groups;
  - the Brief carrying what's waiting on you;
  - Outstanding's Tried / Stopped / Needs;
  - Now as a Plan → Doing → Delegated → Rerouted → Checked → Now → Next stream.
- **Atlas's own windows had never followed any design.** The Atlas window, the pop-up panels, the overlay and the typing box were still in the pre-design slate-and-mint palette, on egui's default dark grey.

## What was done

- **The design is in the tree.**
  - `atlas/design/hub/locked-2026-09-21/` holds all 30 artboards and the canvas index.
  - `atlas/design/hub/refined-2026-09-24/` holds the 14 artboards of the 24 Sep redraw.
  - `atlas/design/hub/SPEC.md` is the written spec, with the token table and what is and isn't built.
  - The mock-up moved to `atlas/design/superseded/`, and the spec says in bold that it is not the design.
- **The hub is rebuilt to it** (`src/hub.rs`, `src/hublive.rs`):
  - **Tokens.** Warm Paper, Ember Dark and Access use the artboards' exact values. Warm Paper is the page's *base*, so a page with no attributes is still the design; "match system" is now written as `data-theme=auto`.
  - **Sidebar.** Brand ("Eric's Atlas"), Search, Now, Personal (Home, Calendar, Projects, Outstanding with its count, Board, History), People, then a business group (only once a business exists), More, and Settings at the foot.
  - **Top bar.** A breadcrumb: "Personal / Home".
  - **Home.**
    - The Brief, in Atlas's words, with what's waiting on you inside it, as links, and the first one as a button.
    - Today, each moment's state an icon and a word.
    - Right now, or Business at a glance when a business exists.
    - The first-run welcome.
    - The arrangeable cards under all that, defaulting to Projects, Health and What I did. "Waiting on you" is no longer a card.
  - **Outstanding.** Four lanes: Waiting on you (questions and yeses), Blocked (Tried / Stopped / Needs, with `Blocker::needs()` new in `backlog.rs`), In progress (queue and crew), and Carried over (with days). The rule is on the page.
  - **Now.** The `mind` stream, which the page never read before, mapped to Plan / Doing / Delegated / Rerouted / Checked / Waiting / Stuck / Now / Next.
    - The rail has the time on this, the fallback, and "Running on your machine".
    - **Pause / Carry on** (`POST /hub/pause`) takes the same path as saying it.
    - **Plain / Detailed** works with no script.
- **Atlas's own windows wear it.** `look::TOKENS` is now Warm Paper, with `TOKENS_DARK` and `TOKENS_ACCESS` beside it. `look_paint::colourway()` follows Settings' colourway (colour-blind mode wins), re-read every two seconds. Every window sets `look_paint::visuals()` before it draws, and the typing box's own dark grey is gone.
- **A guard.** `tests/the_hub_design_is_the_locked_one.rs` replaces `tests/command_deck.rs`. It checks:
  - the canvas's 30 artboards are in the tree;
  - the hub's tokens equal the artboards' own;
  - the sidebar, breadcrumb and names are the design's;
  - Home's order, Outstanding's triple, Now's stream, and the native colourway.

  A merge that brings the mock-up back fails there by name.

## 25k

The Atlas Project chat's 25k arrived in Awaiting Merge after the first merge. It is the keys, tested on the laptop and fixed:

- the typing box is instant;
- focus is handed back;
- remapped keys are accepted;
- `atlas keys` works;
- the log no longer floods;
- "what time is it" is answered.

It is imported as delivered (`chat3-25k`, its hash checked) and merged. The lean build still compiles, with the typing box gated behind the desktop window. Its new `clock` command is decided for add-ons (never), and `hotkeys::heard_as` is listed as a name collision with its reason. This is the source of the key fix that was only an exe before.

## Not done, named

- **Still to build from the artboards:**
  - Messages;
  - the business Overview, Clients and Partners pages;
  - Documents;
  - Sound & voice;
  - Trusted recipients;
  - the Table/Board/Calendar views of shared tasks;
  - the phone's tab bar and live activity.
- **"Match system" in the native windows** gives Warm Paper, because they can't read Windows' light/dark yet.
- **Not seen on the laptop yet.** The hub was rendered and screenshotted here (Chromium), but the egui windows can't be seen from the cloud. Eric's eyes on the laptop are the check.
