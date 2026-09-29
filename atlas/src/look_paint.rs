//! `look`'s design, painted natively.
//!
//! `look` renders the panels as HTML/CSS/SVG. That is the wrong surface for a
//! window Atlas owns: an HTML panel needs either an external browser (a
//! dependency on something outside Atlas) or a bundled web engine, and the
//! rule for this system is in-house and self-contained first. So the design
//! `look` defines — the slate-glass palette, the catenary mark, the row
//! kinds — is reproduced here as values an egui painter can draw, with no new
//! dependency (eframe/egui is already compiled in) and nothing fetched.
//!
//! **This module is the design made testable.** The colours and the mark
//! geometry are pure functions with no window attached, so a test can prove
//! the catenary math and the palette match `look` without a display — which
//! is the only way to verify faithfulness in a headless build. The actual
//! painting (which consumes these) lives with the window, since it needs a
//! live `egui::Ui`.
//!
//! Every constant here is lifted from `look`'s own CSS, named to the line it
//! came from, so the two cannot drift silently: if `look::TOKENS` changes a
//! colour, the test that pins them together fails.

use eframe::egui::Color32;

/// The palette, from `look::TOKENS` and its two siblings: the locked hub
/// design's colourways, so Atlas's own windows, pop-ups and typing box wear
/// what the hub wears. Warm Paper unless Settings chose Ember Dark or the
/// colour-blind-safe set.
///
/// Read through functions rather than constants because the colourway is a
/// choice: `current()` looks at the stored appearance, at most every two
/// seconds, so a change in Settings reaches an open window without a restart.
pub mod palette {
    use super::Color32;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// One colourway, as egui colours. Names follow `look::TOKENS`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Palette {
        /// `--ink` — the panel's ground.
        pub ink: Color32,
        /// `--raised` — a raised strip, the sidebar's cream.
        pub raised: Color32,
        /// `--text` — the main text.
        pub text: Color32,
        /// `--soft` — a supporting line.
        pub soft: Color32,
        /// `--dim` — a heading, an anchor at rest.
        pub dim: Color32,
        /// `--signal` — the accent. Used once per view, on the thing that
        /// changes what you do.
        pub signal: Color32,
        /// `--signal-text` — the accent when it is words: dark enough for
        /// 4.5:1 (WCAG 1.4.3). `signal` is for the mark and other graphics.
        pub signal_text: Color32,
        /// `--warn` — an urgent row's bar and text.
        pub warn: Color32,
        /// `--gone` — a done row, struck through.
        pub gone: Color32,
        /// `--line` — the hairline between rows.
        pub line: Color32,
        /// A dark ground: egui's dark widgets rather than its light ones.
        pub dark: bool,
    }

    const fn rgb(v: u32) -> Color32 {
        Color32::from_rgb((v >> 16) as u8, (v >> 8) as u8, v as u8)
    }

    /// Warm Paper — the lead colourway (`look::TOKENS`).
    pub const WARM_PAPER: Palette = Palette {
        ink: rgb(0xFFFFFF),
        raised: rgb(0xF7F7F5),
        text: rgb(0x37352F),
        soft: rgb(0x5F5D58),
        dim: rgb(0x66645F),
        signal: rgb(0xD9730D),
        signal_text: rgb(0x8F5003),
        warn: rgb(0x8C5A17),
        gone: rgb(0x66645F),
        line: rgb(0xECEAE4),
        dark: false,
    };

    /// Ember Dark (`look::TOKENS_DARK`).
    pub const EMBER_DARK: Palette = Palette {
        ink: rgb(0x0C0F14),
        raised: rgb(0x13181F),
        text: rgb(0xECEFF3),
        soft: rgb(0x97A2AE),
        dim: rgb(0x8E99A5),
        signal: rgb(0xEB9D4A),
        signal_text: rgb(0xEB9D4A),
        warn: rgb(0xE0A44E),
        gone: rgb(0x8E99A5),
        line: rgb(0x232C37),
        dark: true,
    };

    /// Access — colour-blind safe (`look::TOKENS_ACCESS`).
    pub const ACCESS: Palette = Palette {
        ink: rgb(0xFFFFFF),
        raised: rgb(0xF2F3F5),
        text: rgb(0x12151A),
        soft: rgb(0x454B54),
        dim: rgb(0x505763),
        signal: rgb(0x0072B2),
        signal_text: rgb(0x005A8E),
        warn: rgb(0x7A4C00),
        gone: rgb(0x505763),
        line: rgb(0xC9CED6),
        dark: false,
    };

    /// Which colourway an appearance asks for on a computer set up as `os`.
    ///
    /// - Windows high contrast wins over everything: the windows paint in
    ///   the user's own contrast colours, as the hub does under
    ///   `forced-colors`. Their choice of colours is the point of the
    ///   setting (EN 301 549 11.7).
    /// - "Follow this computer" is Ember Dark when the computer is set to
    ///   dark apps, Warm Paper when it's light or can't be read.
    /// - Colour-blind mode is the colour-blind-safe set, whatever the theme.
    pub fn for_appearance_on(a: &crate::appearance::Appearance, os: &crate::oslook::OsLook) -> Palette {
        if let Some(c) = os.contrast {
            return from_contrast(&c);
        }
        if a.cvd != crate::appearance::Cvd::None {
            return ACCESS;
        }
        match a.theme {
            crate::appearance::Theme::Ember => EMBER_DARK,
            crate::appearance::Theme::Access => ACCESS,
            crate::appearance::Theme::System if os.dark == Some(true) => EMBER_DARK,
            crate::appearance::Theme::Warm | crate::appearance::Theme::System => WARM_PAPER,
        }
    }

    /// A palette made from the user's high-contrast colours.
    pub fn from_contrast(c: &crate::oslook::Contrast) -> Palette {
        let lum = |v: u32| ((v >> 16) & 0xFF) * 299 + ((v >> 8) & 0xFF) * 587 + (v & 0xFF) * 114;
        Palette {
            ink: rgb(c.window),
            raised: rgb(c.window),
            text: rgb(c.text),
            soft: rgb(c.text),
            dim: rgb(c.text),
            signal: rgb(c.link),
            signal_text: rgb(c.link),
            warn: rgb(c.highlight),
            gone: rgb(c.gray),
            line: rgb(c.text),
            dark: lum(c.window) < 128_000,
        }
    }

    static CHOSEN: std::sync::Mutex<Option<(Palette, crate::oslook::OsLook)>> = std::sync::Mutex::new(None);
    static READ_AT: AtomicU64 = AtomicU64::new(0);

    /// The colourway in use now, and what the computer asks for: the stored
    /// appearance and the computer's settings, re-read at most every two
    /// seconds so a change in either reaches an open window without a restart.
    pub(super) fn current_with_os() -> (Palette, crate::oslook::OsLook) {
        let now = crate::store::now();
        let stale = now.saturating_sub(READ_AT.load(Ordering::Relaxed)) >= 2 || READ_AT.load(Ordering::Relaxed) == 0;
        let mut g = CHOSEN.lock().unwrap_or_else(|p| p.into_inner());
        if stale || g.is_none() {
            let os = crate::oslook::read();
            let p = for_appearance_on(&crate::appearance::Appearance::load(), &os);
            *g = Some((p, os));
            READ_AT.store(now.max(1), Ordering::Relaxed);
        }
        g.unwrap_or((WARM_PAPER, crate::oslook::OsLook::default()))
    }

    pub(super) fn current() -> Palette {
        current_with_os().0
    }
}

/// The colourway Atlas's own windows paint in right now (see `palette`).
pub fn colourway() -> palette::Palette {
    palette::current()
}

/// egui's own look, in the current colourway: the window ground, text and
/// controls, so nothing Atlas draws natively falls back to egui's default
/// dark grey. Set at the top of every Atlas window's frame.
pub fn visuals() -> eframe::egui::Visuals {
    let p = colourway();
    let mut v = if p.dark { eframe::egui::Visuals::dark() } else { eframe::egui::Visuals::light() };
    v.panel_fill = p.ink;
    v.window_fill = p.ink;
    v.extreme_bg_color = p.raised;
    v.faint_bg_color = p.raised;
    v.override_text_color = Some(p.text);
    v.hyperlink_color = p.signal_text;
    v.selection.bg_fill = p.signal.gamma_multiply(0.25);
    v.selection.stroke.color = p.signal;
    v.widgets.noninteractive.bg_fill = p.ink;
    v.widgets.noninteractive.bg_stroke.color = p.line;
    v.widgets.inactive.bg_fill = p.raised;
    v.widgets.inactive.weak_bg_fill = p.raised;
    v.widgets.inactive.bg_stroke.color = p.line;
    v.widgets.hovered.bg_stroke.color = p.dim;
    v.widgets.active.bg_stroke.color = p.signal;
    v.window_stroke.color = p.line;
    v
}

/// Dress an Atlas window for this frame: the colourway's visuals, the
/// computer's text size (Windows' Settings → Accessibility → Text size), and
/// no animation when the computer is set to show fewer. Called at the top of
/// every Atlas window's frame in place of a bare `set_visuals`.
pub fn dress(ctx: &eframe::egui::Context) {
    let (p, os) = palette::current_with_os();
    // The window's icon follows the appearance too: switched when it changes
    // while the window is open. 0 = not yet set, 1 = light, 2 = dark.
    static ICON: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);
    let want = if p.dark { 2 } else { 1 };
    let had = ICON.swap(want, std::sync::atomic::Ordering::Relaxed);
    if had != 0 && had != want {
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Icon(Some(crate::mark::window_icon_for(p.dark))));
    }
    ctx.set_visuals(visuals());
    if (ctx.zoom_factor() - os.text_scale).abs() > 0.01 {
        ctx.set_zoom_factor(os.text_scale);
    }
    let still = os.reduce_motion;
    ctx.style_mut(|s| s.animation_time = if still { 0.0 } else { 1.0 / 12.0 });
}

/// The colourway and the computer's own settings (dark mode, text size,
/// fewer animations), for a painter that needs both in one frame
/// (`window::paint_mark`).
pub fn palette_and_os() -> (palette::Palette, crate::oslook::OsLook) {
    palette::current_with_os()
}

/// The states the mark can be in. How each moves is `mark::pose`; the
/// shape is `mark` (the Folded A, 27 Sep 2026).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarkState {
    /// Sits in the corner all day: dim, a very slow breath.
    Idle,
    /// The dot rises toward the crease and settles.
    Thinking,
    /// The dot carries the voice; the folded leg answers it.
    Speaking,
    /// The morning brief: the strip rises, folds over, and the dot arrives.
    Waking,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_palette_matches_looks_tokens_exactly() {
        // If look::TOKENS changes a colour, this fails — the two cannot drift.
        let tokens = atlas_tokens();
        for (p, tokens) in [
            (palette::WARM_PAPER, tokens),
            (palette::EMBER_DARK, crate::look::TOKENS_DARK),
            (palette::ACCESS, crate::look::TOKENS_ACCESS),
        ] {
            assert_eq!(p.ink, parse_hex(tokens, "--ink"));
            assert_eq!(p.raised, parse_hex(tokens, "--raised"));
            assert_eq!(p.text, parse_hex(tokens, "--text"));
            assert_eq!(p.soft, parse_hex(tokens, "--soft"));
            assert_eq!(p.dim, parse_hex(tokens, "--dim"));
            assert_eq!(p.signal, parse_hex(tokens, "--signal"));
            assert_eq!(p.signal_text, parse_hex(tokens, "--signal-text"));
            assert_eq!(p.warn, parse_hex(tokens, "--warn"));
            assert_eq!(p.gone, parse_hex(tokens, "--gone"));
            assert_eq!(p.line, parse_hex(tokens, "--line"));
        }
    }

    // --- helpers: read look's own CSS so the pins are against the real source ---

    fn atlas_tokens() -> &'static str {
        crate::look::TOKENS
    }

    fn parse_hex(css: &str, var: &str) -> Color32 {
        // find `--name:#RRGGBB`
        let at = css.find(var).unwrap_or_else(|| panic!("{var} not in TOKENS"));
        let rest = &css[at + var.len()..];
        let hash = rest.find('#').expect("a hex colour");
        let hex = &rest[hash + 1..hash + 7];
        let r = u8::from_str_radix(&hex[0..2], 16).unwrap();
        let g = u8::from_str_radix(&hex[2..4], 16).unwrap();
        let b = u8::from_str_radix(&hex[4..6], 16).unwrap();
        Color32::from_rgb(r, g, b)
    }
}
