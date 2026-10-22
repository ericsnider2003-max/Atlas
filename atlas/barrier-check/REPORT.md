# Accessibility report — Atlas Hub (2026-10-15)

## Verdict: PASS WITH REMAINING WORK for WCAG 2.2 AA
Tested: static ✓ · lint ✗ · rendered ✗ · tree ✗ · axe ✗ · real screen reader ✗ · device ✗

People with low vision may lose contrast on a primary button while hovered or have difficulty seeing gesture strokes. The source corrections are implemented; two findings still need the listed rendered-theme checks before they can be verified. Open the installed Hub and complete the manual checks below; no rendered-browser or screen-reader result is claimed here.

## Do this first — biggest quick win
Item 1 in the fix order: **Verify primary button hover label contrast** (BC-02). The source correction is implemented; the remaining action is to confirm the rendered hover state in each theme. Details under Findings.

## What to do now
Complete the manual checks for items 1–2 in the fix order, then record the rendered result and update finding status. The source corrections are already implemented; source tests do not substitute for visual verification.

## Fix order
| # | When | What to fix, in plain words | Findings | Who it blocks | Severity | Effort | Hours |
|---|---|---|---|---|---|---|---|
| 1 | Do now | Primary button hover must preserve label contrast | BC-02 | — | Major | S | 0.25–1 |
| 2 | Do now | Gesture pictogram strokes should retain contrast | BC-03 | — | Major | S | 0.25–1 |

Total: 2 open findings (Blocker 0 · Major 2 · Minor 0) in 2 fixes, about 0.5–2 h — one estimate per root cause, filed under the worst barrier it removes (major 0.5–2 h). Source corrections are implemented, but both findings remain open pending manual rendered checks. Static CSS leads were manually reviewed against semantic markup and theme tokens. No screen-reader or rendered-browser test was available; do not claim a complete accessibility pass.

## What passed
The shell provides a skip link and main landmark; the appearance control uses native details/summary with a labelled navigation region. Text inputs have an explicit visible focus outline and Windows Highlight override, primary controls pair foreground and background theme tokens, and the state switch exposes its role and checked state.

## What was checked
| Layer | Ran | Detail |
|---|---|---|
| Static scan (check.mjs) | ✓ | 1 file(s) |
| Project linters (lint-bridge.mjs) | ✗ | No accessibility linters installed in this project (eslint-plugin-jsx-a11y, eslint-plugin-vuejs-accessibility, svelte-check). COVERAGE LOST: AST-level JSX/Vue/Svelte rules — only the regex scanner ran. Recommend adding one (references/ci.md); nothing was installed. |
| Rendered pages (render-check.mjs) | ✗ | not run |
| Accessibility tree (announce.mjs) | ✗ | not run |
| axe-core second engine | ✗ | not run |
| Real screen reader | ✗ | not used — nobody listened |
| Real device | ✗ | not tested on a device |
Rendered hover/focus states, browser accessibility tree, axe, keyboard traversal, zoom/reflow, installed-device behavior, and real screen-reader output were not checked because browser/render and assistive-technology runtimes were unavailable in this pass.

## Findings

### BC-02 — Primary button hover must preserve label contrast
- **Who it blocks:** On the light theme, the primary button hover brightening reduced white text contrast below 4.5:1.
- **Fix:** Darken the fill slightly on hover and verify theme pairings.
- **Severity and effort:** Major · S (0.25–1 h)
- **For your developer:** assets/hub/hub.css:422 — WCAG 1.4.3 Contrast (Minimum) (Level AA) — found by reading
- **Status:** Source fix implemented; manual rendered check pending — Changed brightness from 1.08 to .92; default light-token contrast computes 5.19:1, with paired dark/access tokens also above 4.5:1. Added source regression test, Hub design suite 22/22. — not machine-verifiable: found by reading or on a device — no script can confirm the fix

### BC-03 — Gesture pictogram strokes should retain contrast
- **Who it blocks:** Opacity on the gesture card and SVG motion marks could reduce meaningful graphic strokes below 3:1.
- **Fix:** Remove decorative opacity from the pictogram and its motion/repetition strokes.
- **Severity and effort:** Major · S (0.25–1 h)
- **For your developer:** assets/hub/hub.css:498, src/handshape.rs:1424 — WCAG 1.4.11 Non-text Contrast (Level AA) — found by reading
- **Status:** Source fix implemented; manual rendered check pending — Removed parent opacity .9 and SVG path opacity .75; the accent token resolves to at least 3.28:1 in the light canvas, with darker/high-contrast pairings also above 3:1. Regression test passes. — not machine-verifiable: found by reading or on a device — no script can confirm the fix

### BC-01 — Text fields need a high-contrast keyboard focus outline
- **Who it blocks:** Keyboard users relying on high-contrast mode may not see where text-field focus is.
- **Fix:** Use an explicit outline in normal and forced-colors modes.
- **Severity and effort:** Advisory (not a WCAG failure) · S (0.25–1 h)
- **For your developer:** assets/hub/hub.css:436 — Advisory: Windows High Contrast removes box-shadow focus rings (Level advisory) — found by scanner (check.mjs: focus-shadow-only)
- **Status:** Fixed — needs a person to test with assistive technology — Removed outline:none, added a 3px :focus-visible outline and a Highlight system-color override; focused regression test passes. — not machine-verifiable: found by reading or on a device — no script can confirm the fix

## Checked and not a problem
- BC-04 Scanner contrast lead at 227: False positive: the scanner substitutes the background token for the separate foreground token. Source pairs --on-accent with --accent-fill; measured paper 4.52:1, dark 8.34:1, Access 5.19:1. Hover brightness was reviewed and corrected separately.
- BC-05 Scanner contrast lead at 227: False positive: the scanner substitutes the background token for the separate foreground token. Source pairs --on-accent with --accent-fill; measured paper 4.52:1, dark 8.34:1, Access 5.19:1. Hover brightness was reviewed and corrected separately.
- BC-06 Scanner contrast lead at 293: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-07 Scanner contrast lead at 303: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-08 Scanner contrast lead at 303: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-09 Scanner contrast lead at 421: False positive: the scanner substitutes the background token for the separate foreground token. Source pairs --on-accent with --accent-fill; measured paper 4.52:1, dark 8.34:1, Access 5.19:1. Hover brightness was reviewed and corrected separately.
- BC-10 Scanner contrast lead at 421: False positive: the scanner substitutes the background token for the separate foreground token. Source pairs --on-accent with --accent-fill; measured paper 4.52:1, dark 8.34:1, Access 5.19:1. Hover brightness was reviewed and corrected separately.
- BC-11 Scanner contrast lead at 422: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-12 Scanner contrast lead at 422: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-13 Scanner contrast lead at 498: False positive for WCAG 1.4.3: this is a meaningful SVG graphic, not text. After removing the CSS and SVG opacity, its foreground token exceeds the 3:1 non-text threshold.
- BC-14 Scanner contrast lead at 613: False positive for WCAG 1.4.3: this styles the non-text switch track, not text. Its selected-state boundary uses accent-fill against accent-dim (3.88:1) and state is also conveyed by knob position and adjacent wording.
- BC-15 Scanner contrast lead at 711: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-16 Scanner contrast lead at 711: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-17 Scanner contrast lead at 726: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-18 Scanner contrast lead at 726: False positive for WCAG 1.4.3: this declaration colors a border/background or decorative landing outline, not the text foreground. The actual text inherits the ink token; any meaningful control/graphic indicator was reviewed against the 3:1 non-text threshold.
- BC-19 Scanner contrast lead at 730: False positive for WCAG 1.4.3: accent-color styles the native range control indicator, not text. The accent token contrast is at least 3:1 for non-text indicators; rendered control appearance still needs a browser check.

## Before you ship — 5-minute check
No screen reader was used by this audit. Use NVDA with Firefox on Windows, or VoiceOver with Safari on a Mac (Cmd+F5). You do not need to know code.
1. Turn on NVDA with Firefox on Windows, or VoiceOver with Safari on a Mac (Cmd+F5), open the main screen and move through it one item at a time (Tab key on a computer, swipe right on a phone). Every button should say what it does.
2. Keyboard-tab through text fields in the installed Hub, including Windows forced-colors. (BC-01)
3. Render primary buttons at hover in each theme and confirm label readability. (BC-02)
4. Inspect gesture examples in light, dark, and Access themes. (BC-03)
5. Complete the main task once using only the keyboard (or only the screen reader). Note anywhere you get stuck or hear something unclear.
6. Make text larger (browser zoom 200%, or the phone's largest text size) and check nothing is cut off.

## Glossary
- **Focus:** The control that currently receives keyboard input.
- **Contrast:** The visual difference between text or a meaningful graphic and its background.
- **Screen reader:** Assistive software that exposes interface names, roles, states, and content as speech or text.
- **WCAG:** The Web Content Accessibility Guidelines used to evaluate accessibility barriers.
