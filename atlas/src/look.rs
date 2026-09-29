//! How the panels look.
//!
//! ## The brief
//!
//! These are read at a glance from a few feet away, over the top of whatever
//! you're doing, for a few seconds. That is a different problem from a settings
//! page, and it drives everything below: type large enough to take in without
//! leaning forward, almost no chrome, and one thing on screen that moves.
//!
//! ## Palette
//!
//! Cool slate glass, warm off-white text, a single mint signal.
//!
//! Deliberately *not* the two obvious routes. Iron Man cyan is the cliché the
//! moment you say Jarvis, and near-black with one acid accent is what every
//! dark interface does. Slate is bluer and softer than black, so it reads as
//! glass laid over the desktop rather than a hole punched in it, and mint sits
//! far enough from both cyan and orange to be its own thing.
//!
//! ## Type
//!
//! One family, two cuts. Windows 11 ships Segoe UI Variable, whose Display cut
//! is drawn for large sizes and Text cut for small — which is exactly the
//! distinction these panels need. Nothing is fetched: a panel has to render
//! with the network unplugged, so a web font was never an option, and the
//! constraint turns out to pick a better face than a downloaded one would.
//!
//! ## The mark
//!
//! The Folded A (27 Sep 2026), specified in `mark`: one strip of paper folded
//! once, with the dot that matters today. Its earlier forms, an arc bearing a
//! point and then a hanging hairline trace, are gone.

//! ## Superseded rendering
//!
//! This module once rendered the panels as HTML/CSS/SVG. That was replaced by
//! `look_paint` + `window`, which paint the same design natively with egui —
//! no external browser, no bundled web engine, self-contained and offline,
//! which is the rule for this system. The hub (`hub`) renders its own HTML
//! views for the browser/phone and never used these functions. So the HTML
//! renderers had no remaining home and were retired.
//!
//! What stays here is the **design spec** the native painter is measured
//! against: the palette (`TOKENS`). `look_paint`'s tests pin themselves to
//! it, so the two renderers cannot drift — change a colour here and the
//! painter's test fails. This is a specification, not dead code. The mark's
//! own geometry is `mark`.

///
/// Since 26 Sep 2026 these are the locked hub design's colourways (the pinned
/// canvas "Atlas Hub — Command Deck", 20-21 Sep; its Panels artboard draws
/// the pop-ups in the same Warm Paper as the hub). They replace the panels'
/// own earlier palette — cool slate glass with a single mint — which came
/// before the design and kept Atlas's window, pop-ups and typing box in a
/// look Eric never chose. Text colours (`--soft`, `--dim`, `--warn`, `--gone`,
/// `--signal-text`) are the artboards' hues darkened just enough to reach
/// WCAG 2.2's 4.5:1 on their ground (26 Sep 2026), the same adjustment the
/// hub's own tokens got; `--signal` stays the artboards' accent for the mark
/// and other graphics, which need 3:1. `TOKENS` is Warm Paper, the lead; `TOKENS_DARK` is
/// Ember Dark; `TOKENS_ACCESS` is the colour-blind-safe set.
pub const TOKENS: &str = "\
:root{
  --ink:#FFFFFF;
  --raised:#F7F7F5;
  --line:#ECEAE4;
  --text:#37352F;
  --soft:#5F5D58;
  --dim:#66645F;
  --signal:#D9730D;
  --signal-text:#8F5003;
  --warn:#8C5A17;
  --gone:#66645F;
  --pad:28px;
}";

/// Ember Dark, for the panels.
pub const TOKENS_DARK: &str = "\
:root{
  --ink:#0C0F14;
  --raised:#13181F;
  --line:#232C37;
  --text:#ECEFF3;
  --soft:#97A2AE;
  --dim:#8E99A5;
  --signal:#EB9D4A;
  --signal-text:#EB9D4A;
  --warn:#E0A44E;
  --gone:#8E99A5;
}";

/// Access — colour-blind safe, for the panels.
pub const TOKENS_ACCESS: &str = "\
:root{
  --ink:#FFFFFF;
  --raised:#F2F3F5;
  --line:#C9CED6;
  --text:#12151A;
  --soft:#454B54;
  --dim:#505763;
  --signal:#0072B2;
  --signal-text:#005A8E;
  --warn:#7A4C00;
  --gone:#505763;
}";
