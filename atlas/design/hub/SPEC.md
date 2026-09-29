# The Atlas hub design — locked with Eric, 20–21 Sep 2026

**This is the hub design.** It was locked with Eric on 20–21 Sep 2026 in the "Atlas Improvments" project, on the pinned canvas **"Atlas Hub — Command Deck"**. That canvas is kept here, artboard by artboard, in `locked-2026-09-21/`.

`refined-2026-09-24/` holds the same direction redrawn on 24 Sep (canvas "Atlas Hub"), with the phone screens worked through. Where the two differ, the locked canvas and this spec win.

`../superseded/command-deck.html` is **not** the design. It is the 20 Sep mock-up, delivered "awaiting your steer", which came *before* Eric locked the design. It was the only design file in the tree until 26 Sep, so every chat that merged code took it for the design. The 26 Sep merge then recoloured it in Warm Paper and called that the hub. Eric caught it the same day: "one of the chats when it was doing a merge still didn't pick up the hub design and was still defaulting to the old one." `tests/the_hub_design_is_the_locked_one.rs` now fails if the hub drifts back to the mock-up.

## Structure: the combined direction (1 + 2)

- **Notion's calm structure with a living home.** A Notion-style left sidebar navigates everything, and each area is a page.
  - The sidebar has the brand ("Eric's Atlas"), then Search and Now, then a **Personal** group.
  - **Settings** sits at the foot of the sidebar.
  - The **Home** page is a curated, living dashboard: the Brief, Today, and Business at a glance. It is never a blank page.
- **One hub, with a Business section** that appears only once a business is added. The business shows as a teamspace group in the sidebar, for example "Northwind LLC · business". This was chosen over a top workspace switcher and over fully separate spaces.
- **Database views** for lists that deserve them. Shared tasks render as a real Table/Board/Calendar view, with owner avatars and status.
- **Messages is its own area**, never doubled on Home.
- **"Waiting on you" rides inside the Brief**, not as a separate home panel.
- **The top bar is a breadcrumb**: "Personal / Home".
- Desktop is the first-class design; the phone adapts from it.

## Home holds

- **Right now and today**: the day, in order, each moment's state shown as an icon and a word.
- **The Brief**: Atlas's narrative summary, which carries the waiting-on-you items and the first thing to do as a button.
- **Business at a glance**.

Nothing unneeded, and never blank. On first run, Home is a calm welcome with three or four guided steps (connect a calendar, add a mailbox, hand Atlas a file). Each step can be skipped, and all of it works offline.

## Colour: three colourways, Warm Paper is the lead

These token values are copied from the artboards.

| | Warm Paper (the default) | Ember Dark | Access (colour-blind safe) |
|---|---|---|---|
| ground | `#ffffff` | `#0c0f14` | `#ffffff` |
| sidebar | `#f7f7f5` | `#0e1218` | `#f2f3f5` |
| edge | `#eceae4` | `#232c37` | `#c9ced6` |
| ink | `#37352f` | `#eceff3` | `#12151a` |
| dim / faint | `#787774` / `#9b9a97` | `#97a2ae` / `#616c78` | `#454b54` / `#6b7280` |
| accent | `#d9730d` | `#eb9d4a` | `#0072B2` |
| ok / wait / info | `#4f8a5b` / `#b5762a` / `#337ea9` | `#5cc79a` / `#e0a44e` / `#6aa8f0` | `#0072B2` / `#b5730a` / `#0072B2` |

- The accent colour can be picked: Ember, Blue, Teal, Purple or Forest.
- Access is also what colour-blind mode switches any colourway to.
- The theme is one set of tokens that the whole hub reads. Every page turns over together, desktop and phone.

## Accessibility: built in, not bolted on

- **Status never relies on colour alone.** Every status is an icon and a word (check = Done, clock = Working, warning = Waiting), so it reads in greyscale.
- **Colour-blind mode** has three settings: Off, Deuteranopia/Protanopia (red–green), and Tritanopia (blue–yellow).
- **Stored on the machine and applied everywhere**: text size (3 steps), density (comfortable/compact), and theme (Warm Paper / Ember Dark / Access / match system).
- Home cards can be rearranged or hidden.

## The two work/thinking views

- **Now: Atlas's thought process, live.** It is a legible stream, not a raw log: Plan → Doing → Delegated → Rerouted → Checked → Now → Next.
  - It shows a step being handed to a worker, and that worker's result being checked before Eric sees it.
  - It shows verification as its own step.
  - It shows rerouting, instead of cutting a corner, when a path fails.
  - A right rail shows the time spent and the fallback route.
  - A "Plain / Detailed" toggle controls how much reasoning is shown.
- **Outstanding: the open backlog**, personal and business together.
  - Its lanes are **Waiting on you**, then **Blocked**, then **In progress** (with who's building it), then **Carried over** (with a day count).
  - Each Blocked item carries the honest triple: what it **Tried**, what **Stopped** it, and what it **Needs** from you.
  - The rule is shown on the page: nothing rots quietly, anything carried over a week is raised in the brief, and "drop it" means gone.

## Pop-ups: the same two, without opening the hub

- **Desktop.**
  - An ambient "here's what I'm doing" panel (the thought process, condensed).
  - A "ready for you" panel: an outstanding item with its action.
  - A collapsed pill as the always-there minimal state.
  - Each can be dismissed, and each is a one-tap door into the full hub page.
  - They are drawn in the same colourway as the hub (the Panels artboard is Warm Paper).
- **Phone.** A live-activity card, plus a Dynamic-Island-style pill for the thought process, and an actionable notification (Implement / Read / Later).
- **Interrupt rule (locked).** Pop-ups appear only when Eric asks, or when it's urgent. The choices are "Only when I ask", "When it's urgent" (the default) and "Anything ready".

## Screens: phone, folding phone, tablet, laptop (26 Sep 2026)

Eric, 26 Sep: Atlas stands alone on phones; it has to work on an iPad in both orientations and on new shapes like the iPhone Duo; and the tabs must be even. The pages are laid out by the room there is, not by device name:

- **Up to 600px: the phone.** This is the 24 Sep phone artboards.
  - The top bar is one row: the mark, Search, Talk (microphone), Help, Aa and Menu.
  - At the bottom are the design's **five** tabs, evenly spaced: Home, Projects, Messages, Business, Settings.
  - What's waiting on you is a badge on Home.
  - Talk is not a tab. A sixth, raised Talk tab was tried and sat off-centre, and Eric caught it.
- **601–1019px: the rail.** The sidebar becomes a slim column of icons with their names under them. This is for an iPhone Duo unfolded, an iPad upright, a phone on its side, or an Android foldable open.
- **1020px and up: the full sidebar.** This is for an iPad on its side and the laptop.
- **Touch screens at any width** get 44px targets and no keyboard hints. On Apple keyboards the hint reads ⌘K.
- **A screen split by a hinge** (the Viewport Segments API) puts the page list on one side and the page on the other.

## Contrast adjustments for WCAG 2.2 AA (26 Sep 2026)

The artboards' text greys and the accent used as text sat below 4.5:1 on their grounds. They were darkened in hue:

- **Warm Paper:**
  - dim text `#5f5d58`
  - faint text `#66645f`
  - accent as text `#8f5003`
  - accent as a button fill `#b26206`
  - wait `#8c5a17`
- **Ember Dark:** faint `#8e99a5`.
- **Access:** faint `#505763`, wait `#7a4c00`.
- **Colour-blind modes on a dark ground** use Okabe-Ito's light blue `#56b4e9` and orange `#e69f00`.

The accent itself (`#d9730d` / `#eb9d4a` / `#0072b2`) is unchanged for the mark and other graphics, which need 3:1. The native windows' palette (`look::TOKENS`) got the same values. See `docs/ACCESSIBILITY.md`.

## What is built, and what isn't (26 Sep 2026)

**Built:**

- The Warm Paper, Ember Dark and Access tokens, with Warm Paper as the page's base, so a page with no settings is still the design.
- The labelled sidebar, with the Personal group, a business group once a business exists, More, and Settings at the foot.
- The breadcrumb top bar.
- Home: the Brief carrying what's waiting on you, Today with icon-and-word states, Right now or Business at a glance, the first-run welcome, and the arrangeable cards under them.
- Outstanding in four lanes, with Tried/Stopped/Needs.
- Now as a stream with its rail, the Plain/Detailed toggle and Pause.
- Atlas's own windows (the Atlas window, pop-up panels, overlay and typing box) painted in the current colourway instead of the old slate-and-mint.

**Built on 26 Sep (second pass):**

- **Messages.**
- **The business pages:** Overview, Clients and Partners.
- **Documents.**
- **Sound & voice.**
- **Trusted recipients.**
- **Shared tasks** as Table, Board and Calendar.
- **The calendar's week grid.**
- **Give, Talk, Offline and Help & accessibility.**
- **The phone layout:** the five tabs and a one-row top bar, plus the rail for tablets and foldables.
- **The phone live activity**, in source: an iOS Live Activity and Dynamic Island, and an Android ongoing notification. Both read `/hub/live.json`.
- **Native windows following Windows** when "Follow this computer" is chosen: its dark mode, high contrast, text size and animation.

**The mark (27 Sep):** the Folded A (`src/mark.rs`, doc 42). `hub::MARK` draws it in the page's own tokens, and it replaces the ringed dot.

**Built 27 Sep (second session):**

- **Updates** (Your devices): what's installed, what's been heard, Install, "Go back to the last version" (asks first), how updates arrive (automatic, ask, off). On the releaser's Atlas, the builds friends reported failing, with **Hold** per build.
- **Feedback** (Your devices): write it, see exactly what will be sent, then Send. On the releaser's Atlas, the reports that came in, each with an answer box.
- **Partners** show Online (heard in the last 15 minutes), Offline with when they were last heard, or not heard from yet. Every answer from a paired device, and every paired device reaching this door, is noted.
- **Documents** carry a Send column; what's sent is logged on the item, so it shows as "Sent to …" instead of Private.
- **Connections** show the phone's own model when the app is built with it, with **Get this phone's own model**.
- **`autocomplete`** on every text field: `email`/`name`/`tel` where the field is the user's own, `off` where it holds someone else's (a client's) or isn't personal data.

**Still not built**, named in `OPEN_GAPS.md`:

- **The phone shells** (`mobile/ios`, `mobile/android`) are written but have never been compiled.
- **Invoices** on Business Overview: there is no invoice module.
