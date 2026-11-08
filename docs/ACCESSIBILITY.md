# Atlas accessibility: the laws, what Atlas does about them, and what isn't done

Written 26 Sep 2026, when Eric asked to "look at accessibility laws for apps and implement all of those". This is not legal advice. It records what the standards ask for and where Atlas stands against each requirement, so that a lawyer or an auditor can pick it up.

## Which laws, and what they point to

Almost every accessibility law for apps points to the same technical standard, WCAG. Atlas targets **WCAG 2.2 level AA**, which is the newest version and a superset of 2.1 AA. It also targets the software clauses of **EN 301 549** (chapters 5, 11 and 12), which cover native windows, platform settings and documentation, not only web pages.

**United States, ADA Title II (state and local government, and anything they provide).**
- The DOJ rule requires WCAG 2.1 AA for web content and mobile apps.
- In April 2026 the compliance dates moved to 26 April 2027 for large entities and 26 April 2028 for small ones.
- It applies to Atlas only if a public body ever provides Atlas.
- Sources: [Federal Register, 20 Apr 2026](https://www.federalregister.gov/documents/2026/04/20/2026-07663/extension-of-compliance-dates-for-nondiscrimination-on-the-basis-of-disability-accessibility-of-web) and [SBA Office of Advocacy](https://advocacy.sba.gov/2026/04/27/doj-extends-compliance-dates-for-state-and-local-governments-to-make-their-websites-accessible/).

**United States, ADA Title III (businesses open to the public).**
- There is no technical rule for Title III.
- Courts and settlements use WCAG 2.1/2.2 AA as the yardstick.
- It applies once Atlas is sold or offered to the public.

**United States, Section 508 (federal agencies and what they buy).**
- The standard is WCAG 2.0 AA, applied to software as well.
- It applies only if Atlas is sold to a federal agency, which would then ask for a conformance report (the "VPAT" at the end of this file).

**European Union, European Accessibility Act (Directive 2019/882).** It has applied since 28 June 2025 to consumer e-commerce, banking, e-books, electronic communications and the apps that deliver them.
- The EAA presumes an app conforms if it meets **EN 301 549**. The version currently cited is v3.2.1, which is WCAG 2.1 AA.
- **v4.1.1 was published in September 2026 and adopts WCAG 2.2.** It becomes the legal reference once the Commission cites it in the Official Journal.
- Microenterprises (fewer than 10 staff and at most €2m turnover) are exempt for services.
- Sources: [AccessibleEU, 7 Sep 2026](https://accessible-eu-centre.ec.europa.eu/content-corner/news/european-accessibility-standard-en-301-549-has-been-updated-2026-09-07_en) and [Davis Wright Tremaine](https://www.dwt.com/insights/2026/09/european-accessibility-act-ict-standards-update).

**Also:**
- The UK Equality Act 2010 (a reasonable-adjustments duty, with WCAG 2.2 AA as the yardstick in practice).
- Canada's Accessible Canada Act and Ontario's AODA (WCAG 2.0 AA).
- Apple's and Google's own app-review accessibility expectations.

**What this means for Atlas today.** Atlas is Eric's own assistant, so none of these laws binds a personal tool used by one person. They start to matter the moment Atlas is given or sold to anyone else. That is why the target is the strictest current bar (WCAG 2.2 AA plus EN 301 549), met now rather than retrofitted later.

## What was built for it

**Hub pages (laptop, tablet, phone, folding phone). Every page gets:**
- a language;
- a title;
- a skip link;
- one `main` landmark;
- a named `nav`;
- `aria-current` on the page you're on;
- headings in order;
- labelled controls;
- status messages in `role=status`/`aria-live`;
- Help in the same place on every page;
- no timed refresh (the Now page updates with a script you can pause, where it used to have a meta refresh, which fails 2.2.1);
- no orientation lock;
- pinch zoom allowed;
- reflow down to 320px;
- touch targets of 44px on touch screens (24px minimum elsewhere);
- visible focus;
- `forced-colors` and `prefers-reduced-motion` followed;
- text and graphics contrast of at least 4.5:1 and 3:1 in every colourway, colour-blind modes included.

**Contrast.** Some of the design's text colours (dim, faint, the accent used as text) were darkened just enough to pass contrast, and the design's hues were kept. The accent itself stays as drawn for the mark and other graphics.

**Screen sizes, by room rather than device name:**
- up to 600px: the phone, with one top row and five tabs;
- 601–1019px: an icon rail (iPhone Duo open, iPad upright, a phone on its side, Android foldables open);
- 1020px and up: the full sidebar;
- hinged dual screens: the page list on one side, the page on the other.

**Atlas's own windows (egui):**
- The `accesskit` bridge (MIT/Apache-2.0, eframe's own) is compiled in on Windows, so Narrator, NVDA and JAWS can see them through UI Automation. It adds 11 crates to the Windows build and nothing elsewhere.
- Custom-painted parts have names: the mark, each setup step's state as a word, and the QR code.
- The windows follow Windows' dark mode (when you choose "Follow this computer"), high-contrast colours, text size and animation setting (`src/oslook.rs`). This is EN 301 549 11.7, using the user's platform settings.

**Sound & voice.** Everything spoken is also shown. You can mute it, set quiet hours, or have replies spoken only when you spoke to Atlas. Everything you can say, you can type.

**Documentation (EN 301 549 chapter 12).**
- Help → "Using Atlas your way", the keyboard list, the accessibility statement, and Report a barrier.
- Reported barriers go on the feedback list.

**Phone apps (`mobile/`).**
- They use native speech recognition that stays on the device, and the system voice.
- They respect Dynamic Type and font scale.
- The live-activity status is an icon and a word.
- Neither app locks orientation.

## How it's checked

**Automated (26 Sep).**
- axe-core 4.13 (WCAG 2.0/2.1/2.2 A and AA, plus best practice) was run in Chromium on all 35 hub pages.
- It ran at ten sizes: a 320×568 phone, an iPhone upright and on its side, iPhone Duo folded and open both ways, iPad and iPad mini upright, iPad on its side, and a 1440 laptop.
- It covered Warm Paper, Ember Dark, Access, and the colour-blind modes on both grounds.
- Result: **0 violations, nothing scrolling sideways.**

**In the test suite.**
- `tests/the_phone_stands_alone.rs` holds every page to its structure (language, skip link, landmark, named nav, tabs, Help, no refresh, zoom allowed, labelled controls).
- `src/look_paint.rs` holds the native palette to the tokens.

**Not done yet:**
- Hands-on testing with NVDA, JAWS and Narrator on Windows, VoiceOver on iPhone and iPad, and TalkBack on Android.
- Testing on a real iPhone Duo. Its widths were worked out from Apple's pixel counts, not measured.
- The Swift and Kotlin phone shells have not been compiled.

## Conformance report (WCAG 2.2 AA, VPAT-style)

"Supports" means built and checked automatically. "Partially" says what's missing.

| Criterion | Level | Status | Notes |
|---|---|---|---|
| 1.1.1 Non-text content | A | Supports | Icons are `aria-hidden` with a text label beside them; painted marks in the native windows have names. |
| 1.2.x Time-based media | A/AA | Not applicable | Atlas plays no video. Recorded calls are turned into transcripts. |
| 1.3.1 Info and relationships | A | Supports | Real headings, lists, tables with headers, labels, landmarks. |
| 1.3.2 Meaningful sequence | A | Supports | DOM order is reading order at every size. |
| 1.3.3 Sensory characteristics | A | Supports | No "the orange button" instructions. |
| 1.3.4 Orientation | AA | Supports | No lock in the manifest, iOS Info.plist or Android manifest. |
| 1.3.5 Identify input purpose | AA | Supports | Every text field carries `autocomplete` (27 Sep): the user's own details get their token; fields holding someone else's details (a client's) get `off`, so the browser doesn't offer the user's own. `tests/updates_and_feedback_in_the_hub.rs` checks every page. |
| 1.4.1 Use of colour | A | Supports | Every status is an icon and a word. |
| 1.4.2 Audio control | A | Supports | Mute, volume, quiet hours; nothing plays on page load. |
| 1.4.3 Contrast (minimum) | AA | Supports | Every colourway, both grounds, colour-blind modes. |
| 1.4.4 Resize text | AA | Supports | Three text sizes, browser zoom, phone font scale, Windows text size. |
| 1.4.5 Images of text | AA | Supports | None. |
| 1.4.10 Reflow | AA | Supports | 320px with no sideways scroll. |
| 1.4.11 Non-text contrast | AA | Supports | Control borders, focus ring and graphics are at least 3:1. |
| 1.4.12 Text spacing | AA | Supports | No fixed heights on text containers. |
| 1.4.13 Content on hover or focus | AA | Supports | Menus open on press, close with Esc, stay while hovered. |
| 2.1.1 Keyboard | A | Supports | Every control is a link, button or form field. |
| 2.1.2 No keyboard trap | A | Supports | |
| 2.1.4 Character key shortcuts | A | Supports | No single-key shortcuts while typing; the talk keys can be changed. |
| 2.2.1 Timing adjustable | A | Supports | No timeouts; live updates can be paused. |
| 2.2.2 Pause, stop, hide | A | Supports | Live updates pause; reduced motion is followed. |
| 2.3.1 Three flashes | A | Supports | Nothing flashes. |
| 2.4.1 Bypass blocks | A | Supports | Skip link. |
| 2.4.2 Page titled | A | Supports | "Atlas — Page". |
| 2.4.3 Focus order | A | Supports | |
| 2.4.4 Link purpose | A | Supports | |
| 2.4.5 Multiple ways | AA | Supports | Sidebar/rail/menu, Search (Find anything), breadcrumbs. |
| 2.4.6 Headings and labels | AA | Supports | |
| 2.4.7 Focus visible | AA | Supports | |
| 2.4.11 Focus not obscured | AA | Supports | The sticky top bar and tabs leave room (`scroll-padding`). |
| 2.5.1 Pointer gestures | A | Supports | Gestures are optional; every one has a button. |
| 2.5.2 Pointer cancellation | A | Supports | Actions fire on release; hold-to-talk also works as press to start and press to stop. |
| 2.5.3 Label in name | A | Supports | Icon-only buttons' names match their visible label where they have one. |
| 2.5.4 Motion actuation | A | Not applicable | Nothing is triggered by shaking or tilting. |
| 2.5.7 Dragging movements | AA | Supports | Arranging cards has buttons; nothing requires a drag. |
| 2.5.8 Target size (minimum) | AA | Supports | 24px everywhere, 44px on touch. |
| 3.1.1 / 3.1.2 Language | A/AA | Supports | `lang=en`. Messages in other languages are shown as written. |
| 3.2.1 / 3.2.2 On focus / on input | A | Supports | Nothing changes until you press. |
| 3.2.3 / 3.2.4 Consistent navigation and identification | AA | Supports | |
| 3.2.6 Consistent help | A | Supports | Help is in the top bar on every page (the phone's top row too). |
| 3.3.1 / 3.3.3 Error identification and suggestion | A/AA | Supports | Errors are said in words with what to do, for example "use the hours and minutes, like 22:00". |
| 3.3.2 Labels or instructions | A | Supports | |
| 3.3.4 Error prevention | AA | Supports | Nothing irreversible without a confirmation; changes land only when approved. |
| 3.3.7 Redundant entry | A | Supports | |
| 3.3.8 Accessible authentication | AA | Supports | Pairing is a link or code you can paste; no puzzles. |
| 4.1.2 Name, role, value | A | Supports | Native elements; `aria-pressed` on hold-to-talk; accesskit in the native windows. |
| 4.1.3 Status messages | AA | Supports | `role=status` / `aria-live=polite`. |

**EN 301 549 beyond WCAG:**

| Clause | Requirement | Status |
|---|---|---|
| 5.2 | Activation of accessibility features | Supports: Aa menu, Settings, and the system's own settings followed. |
| 5.5 | Operable parts | Supports: 44px on touch. |
| 5.9 | Simultaneous user actions | Supports: nothing needs two fingers or two keys at once, except the talk key combination you choose yourself. |
| 11.5 | Interoperability with assistive technology | Partially. Web pages: yes. Native windows: accesskit/UIA, not yet tried with a screen reader. |
| 11.7 | User preferences | Supports: dark mode, high contrast, text size, reduced motion. |
| 11.8 | Authoring tools | Not applicable. |
| 12.1 / 12.2 | Documentation and support | Supports: Help page, this file, and Report a barrier. |
| 6.x | Two-way voice communication | Not applicable today: no calls between people. If calls are added, real-time text comes with them (6.2). |
