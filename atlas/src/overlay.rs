//! Drawing on the desktop itself.
//!
//! Panels in a browser window can't do what you asked for. A browser always
//! paints a background, so "transparent" isn't available at any setting — the
//! best it manages is a dark rectangle. For text that appears *over* your
//! desktop with nothing behind it, the window has to be a layered window with
//! a real alpha channel, drawn by Atlas.
//!
//! So this is a native overlay: no browser, no URL, no chrome, no background.
//! Just letters and a mark appearing over whatever you were looking at, and
//! then gone.
//!
//! ## Reading over anything
//!
//! The problem with transparent text is that your desktop might be a white
//! document, and white text on white is nothing. Two things fix it, and both
//! are cheap: every glyph gets a soft dark halo, and behind the text sits a
//! wide, very faint gradient that darkens the area without reading as a box.
//! Together they hold contrast over a photograph, a spreadsheet or a terminal.
//!
//! ## Typing
//!
//! Text arrives a character at a time. It reads as something being said rather
//! than something being displayed, and — more usefully — the movement draws
//! your eye to it, so a line that appears while you're looking elsewhere still
//! gets noticed.

use serde::{Deserialize, Serialize};

/// One thing drawn on the overlay.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Element {
    /// The Atlas mark.
    Mark { x: i32, y: i32, size: i32, state: MarkState },
    /// Text that types itself in.
    Typed { x: i32, y: i32, text: String, size: i32, align: Align },
    /// The faint darkening behind text, so it reads over anything.
    Shade { x: i32, y: i32, width: i32, height: i32, strength: f32 },
    /// An outline around the thing your hand is currently over.
    ///
    /// Added because a hand pointing at something with no mark on screen is a
    /// hand you have to guess with. Eric was explicit: he does not want to
    /// watch himself, he wants to see *what he is selecting* — so the feedback
    /// belongs on the thing, not on a picture of him.
    Outline {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
        /// Thicker once you have hold of it, so picking up and hovering are
        /// not the same picture.
        holding: bool,
    },
}

/// Draw a ring around what the hand is over.
///
/// Separate from the speaking overlay's own `frame` because it is live: it
/// follows the pointer every look rather than belonging to a phase of a
/// sentence being said.
pub fn around(rect: (i32, i32, i32, i32), holding: bool) -> Element {
    let (x, y, width, height) = rect;
    Element::Outline { x, y, width, height, holding }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MarkState {
    Idle,
    Thinking,
    Waking,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Align {
    Left,
    Centre,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OverlayConfig {
    pub enabled: bool,
    /// Characters per second while typing. Fast enough not to be a wait,
    /// slow enough to read as typing.
    pub typing_cps: f32,
    /// Pause on a full stop, so a sentence lands.
    pub sentence_pause_ms: u64,
    /// How dark the shade behind text goes, at its centre.
    pub shade_strength: f32,
    /// Text size for the waking line.
    pub headline_size: i32,
    /// Frames per second while anything is moving.
    pub fps: u32,
    /// Hold the finished text this long before fading.
    pub hold_secs: u64,
    pub fade_ms: u64,
}

impl Default for OverlayConfig {
    fn default() -> Self {
        OverlayConfig {
            enabled: true,
            typing_cps: 42.0,
            sentence_pause_ms: 260,
            shade_strength: 0.55,
            headline_size: 34,
            fps: 60,
            hold_secs: 6,
            fade_ms: 700,
        }
    }
}

/// How far through the typing we are.
#[derive(Debug, Clone, PartialEq)]
pub struct Typing {
    pub full: String,
    /// Characters shown so far.
    pub shown: usize,
    /// Extra delay owed, from a full stop.
    pause_until_ms: u64,
}

impl Typing {
    pub fn new(text: &str) -> Typing {
        Typing { full: text.to_string(), shown: 0, pause_until_ms: 0 }
    }

    pub fn done(&self) -> bool {
        self.shown >= self.full.chars().count()
    }

    /// What's on screen right now.
    pub fn visible(&self) -> String {
        self.full.chars().take(self.shown).collect()
    }

    /// Advance to a given moment. Driven by elapsed time rather than by frame
    /// count, so a dropped frame doesn't slow the typing down.
    pub fn at(&mut self, elapsed_ms: u64, cfg: &OverlayConfig) {
        if self.done() {
            return;
        }
        if elapsed_ms < self.pause_until_ms {
            return;
        }
        let chars = self.full.chars().count();
        let target = ((elapsed_ms as f32 / 1000.0) * cfg.typing_cps) as usize;
        let target = target.min(chars);
        if target <= self.shown {
            return;
        }
        // Land on a sentence end and hold there for a beat.
        let upto: Vec<char> = self.full.chars().take(target).collect();
        for (i, c) in upto.iter().enumerate().skip(self.shown) {
            if matches!(c, '.' | '?' | '!') && i + 1 < chars {
                self.shown = i + 1;
                self.pause_until_ms = elapsed_ms + cfg.sentence_pause_ms;
                return;
            }
        }
        self.shown = target;
    }
}

/// What the overlay is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    /// The mark arriving.
    Arriving,
    Typing,
    /// Everything shown, waiting.
    Holding,
    Fading,
    Gone,
}

#[derive(Debug, Clone)]
pub struct Overlay {
    pub phase: Phase,
    pub typing: Typing,
    pub started_ms: u64,
    typing_started_ms: Option<u64>,
    hold_started_ms: Option<u64>,
    /// 0 to 1.
    pub opacity: f32,
}

/// The mark arrives before the words. About half a second — long enough to
/// register, short enough that you aren't waiting for it.
const ARRIVE_MS: u64 = 480;

impl Overlay {
    pub fn begin(text: &str, now_ms: u64) -> Overlay {
        Overlay {
            phase: Phase::Arriving,
            typing: Typing::new(text),
            started_ms: now_ms,
            typing_started_ms: None,
            hold_started_ms: None,
            opacity: 0.0,
        }
    }

    /// Move it on. Returns true while there is still something to draw.
    pub fn tick(&mut self, now_ms: u64, cfg: &OverlayConfig) -> bool {
        let since_start = now_ms.saturating_sub(self.started_ms);

        match self.phase {
            Phase::Arriving => {
                self.opacity = (since_start as f32 / ARRIVE_MS as f32).min(1.0);
                if since_start >= ARRIVE_MS {
                    self.phase = Phase::Typing;
                    self.typing_started_ms = Some(now_ms);
                    self.opacity = 1.0;
                }
            }
            Phase::Typing => {
                let since_typing = now_ms.saturating_sub(self.typing_started_ms.unwrap_or(now_ms));
                self.typing.at(since_typing, cfg);
                if self.typing.done() {
                    self.phase = Phase::Holding;
                    self.hold_started_ms = Some(now_ms);
                }
            }
            Phase::Holding => {
                let held = now_ms.saturating_sub(self.hold_started_ms.unwrap_or(now_ms));
                if held >= cfg.hold_secs * 1000 {
                    self.phase = Phase::Fading;
                    self.hold_started_ms = Some(now_ms);
                }
            }
            Phase::Fading => {
                let fading = now_ms.saturating_sub(self.hold_started_ms.unwrap_or(now_ms));
                self.opacity = 1.0 - (fading as f32 / cfg.fade_ms as f32).min(1.0);
                if fading >= cfg.fade_ms {
                    self.phase = Phase::Gone;
                    self.opacity = 0.0;
                }
            }
            Phase::Gone => return false,
        }
        true
    }

    /// Cut it short — you spoke, or you dismissed it.
    pub fn dismiss(&mut self, now_ms: u64) {
        if self.phase != Phase::Gone {
            self.phase = Phase::Fading;
            self.hold_started_ms = Some(now_ms);
        }
    }

    /// Everything to draw this frame, in paint order.
    pub fn frame(&self, screen_w: i32, screen_h: i32, cfg: &OverlayConfig) -> Vec<Element> {
        if self.phase == Phase::Gone {
            return Vec::new();
        }
        let cx = screen_w / 2;
        let cy = screen_h / 2;
        let mark_size = 132;
        let text = self.typing.visible();

        let mut out = vec![Element::Mark {
            x: cx - mark_size / 2,
            y: cy - mark_size - 40,
            size: mark_size,
            state: if self.phase == Phase::Arriving { MarkState::Waking } else { MarkState::Thinking },
        }];

        if !text.is_empty() {
            // The shade goes down first and is much wider than the text, so
            // it fades out well before its edge — no rectangle, just a darker
            // region.
            let w = (screen_w as f32 * 0.62) as i32;
            let h = cfg.headline_size * 5;
            out.push(Element::Shade {
                x: cx - w / 2,
                y: cy - h / 3,
                width: w,
                height: h,
                strength: cfg.shade_strength,
            });
            out.push(Element::Typed {
                x: cx,
                y: cy,
                text,
                size: cfg.headline_size,
                align: Align::Centre,
            });
        }
        out
    }
}

/// The window flags a transparent overlay needs.
///
/// Layered gives a real alpha channel. Transparent makes clicks pass straight
/// through to whatever is underneath, so it can never take focus or get in the
/// way. ToolWindow keeps it out of the taskbar and out of Alt-Tab — it isn't
/// an app you switch to.
pub const WS_EX_LAYERED: u32 = 0x0008_0000;
pub const WS_EX_TRANSPARENT: u32 = 0x0000_0020;
pub const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
pub const WS_EX_TOPMOST: u32 = 0x0000_0008;
pub const WS_EX_NOACTIVATE: u32 = 0x0800_0000;

pub fn window_style() -> u32 {
    WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE
}

/// The colour Windows makes invisible in the overlay: pure black, which is
/// what the overlay clears to. Every pixel left this colour is not drawn at
/// all and passes clicks through, whatever the graphics card makes of
/// transparency (`overlaywin::see_through`). As a Windows COLORREF.
pub const SEE_THROUGH_KEY: u32 = 0x0000_0000;
