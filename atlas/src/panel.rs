//! Free-standing panels.
//!
//! Not a hub. These appear when asked for, say their piece, and leave. They
//! are furniture on your desk rather than an application you visit, and that
//! shapes every decision here: big type because they are glanced at from a few
//! feet away, almost no controls because they are driven by voice, and a
//! lifetime measured in seconds.
//!
//! Placement follows from the same idea. Anything you need to read while
//! working goes on a second screen if you have one, because covering the thing
//! you are working on to tell you about it is self-defeating. With one screen
//! Atlas asks first, and takes no for an answer by simply speaking instead.

use crate::platform::{Monitor, PixelRect as Rect};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    /// The startup moment: the mark, then your brief, then gone.
    Waking,
    /// Outstanding tasks, as a slim column.
    Tasks,
    /// What it's doing and the thinking behind it, live.
    Mind,
    /// Small, ambient, off to the side. Just breathing.
    Presence,
    /// Settings, when you ask for them.
    Controls,
}

impl Panel {
    /// Does this one go away by itself? (Removed as unused on the third
    /// chat's line; our round wired it into `faded`, so it stays.)
    pub fn transient(&self) -> bool {
        matches!(self, Panel::Waking)
    }
    /// Is it worth covering part of your working screen for?
    pub fn title(&self) -> &'static str {
        match self {
            Panel::Waking => "Atlas",
            Panel::Tasks => "Outstanding",
            Panel::Mind => "Working",
            Panel::Presence => "Atlas",
            Panel::Controls => "Settings",
        }
    }
}

/// Has a panel that leaves by itself been up long enough to go? Only the
/// transient one (`Waking`) ever fades; the rest stay until you move on.
pub fn faded(panel: Panel, shown_at: u64, now: u64, cfg: &PanelConfig) -> bool {
    panel.transient() && now.saturating_sub(shown_at) >= cfg.waking_secs
}

/// Where a panel sits and how big it is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Placement {
    pub monitor: u32,
    pub rect: Rect,
    /// Sits above everything else.
    pub on_top: bool,
    /// No title bar, no border, no browser furniture.
    pub chromeless: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Decision {
    /// Show it here.
    Show(Placement),
    /// One screen, and this would cover your work. Ask first.
    AskFirst(String),
    /// Say it instead.
    SpeakOnly(String),
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PanelConfig {
    pub enabled: bool,
    /// Width of the slim column, in pixels.
    pub column_width: i32,
    /// How long the waking panel stays before fading.
    pub waking_secs: u64,
    /// The small ambient mark, and how big.
    pub presence: bool,
    pub presence_size: i32,
    /// Put the reading panels on a second screen instead of your main one.
    ///
    /// Off by default: you asked for them full height down the right of the
    /// screen you're actually looking at, which is where you'd glance without
    /// turning your head.
    pub prefer_second_screen: bool,
    /// Ask first when a panel would take a real bite out of the screen.
    ///
    /// Judged by how much width it would cost, not by how many screens you
    /// have — 400px is a sixth of a 2560 monitor and worth nothing, but a
    /// fifth of a laptop screen and very much in the way. That's the thing
    /// that matters, so that's what's measured.
    pub ask_above_fraction: f32,
    /// Ask before covering the only screen you have.
    pub ask_on_single_display: bool,
    /// Speak the contents as well as showing them.
    pub speak_as_well: bool,
}

impl Default for PanelConfig {
    fn default() -> Self {
        PanelConfig {
            enabled: true,
            column_width: 400,
            waking_secs: 9,
            presence: true,
            presence_size: 120,
            prefer_second_screen: false,
            // 400px of 2560 is 16% — take it. 400 of 1920 is 21% — ask.
            ask_above_fraction: 0.18,
            ask_on_single_display: true,
            speak_as_well: true,
        }
    }
}

/// The screen you're working on.
fn primary(monitors: &[Monitor]) -> Option<&Monitor> {
    monitors.iter().find(|m| m.primary).or_else(|| monitors.first())
}

/// Somewhere that isn't the screen you're working on.
fn secondary(monitors: &[Monitor]) -> Option<&Monitor> {
    let p = primary(monitors)?;
    monitors.iter().find(|m| m.id != p.id)
}

/// Work out where a panel should go.
pub fn place(panel: Panel, monitors: &[Monitor], cfg: &PanelConfig) -> Decision {
    if !cfg.enabled || monitors.is_empty() {
        return Decision::SpeakOnly("panels are off".into());
    }

    // The waking panel is the one moment worth the middle of your main
    // screen — it is the thing you asked for, and it leaves by itself.
    if panel == Panel::Waking {
        let m = primary(monitors).unwrap();
        let w = 760.min(m.width - 80);
        let h = 460.min(m.height - 80);
        return Decision::Show(Placement {
            monitor: m.id,
            rect: Rect {
                x: m.x + (m.width - w) / 2,
                y: m.y + (m.height - h) / 2,
                width: w,
                height: h,
            },
            on_top: true,
            chromeless: true,
        });
    }

    // The ambient mark never touches the screen you're working on.
    if panel == Panel::Presence {
        let Some(m) = secondary(monitors) else {
            return Decision::SpeakOnly("nowhere to put it that isn't in your way".into());
        };
        let s = cfg.presence_size;
        return Decision::Show(Placement {
            monitor: m.id,
            // Bottom right of the second screen: visible in peripheral
            // vision, never where a window's content is.
            rect: Rect { x: m.x + m.width - s - 28, y: m.y + m.height - s - 28, width: s, height: s },
            on_top: true,
            chromeless: true,
        });
    }

    // A full-height column down the right of the screen you're looking at.
    // Its own window, top to bottom — nothing to scroll a small box for.
    let target = if cfg.prefer_second_screen { secondary(monitors) } else { primary(monitors) };

    match target {
        Some(m) => {
            // On a big monitor a column at the edge costs you nothing. On a
            // laptop screen it's a fifth of your working width, and that's
            // worth asking about.
            if takes_a_real_bite(m, cfg) {
                return Decision::AskFirst(question_for(panel));
            }
            Decision::Show(column(m, cfg))
        }
        // Only reachable when a second screen was asked for and isn't there.
        None if cfg.ask_on_single_display => Decision::AskFirst(question_for(panel)),
        None => Decision::SpeakOnly("nowhere to put it, so I'll read it".into()),
    }
}

/// Would this cost a noticeable slice of the screen?
fn takes_a_real_bite(m: &Monitor, cfg: &PanelConfig) -> bool {
    if m.width <= 0 {
        return false;
    }
    cfg.column_width as f32 / m.width as f32 > cfg.ask_above_fraction
}

fn question_for(panel: Panel) -> String {
    match panel {
        Panel::Tasks => "Want to see the list, or shall I just read it?".into(),
        Panel::Mind => "Want to watch, or shall I talk you through it?".into(),
        _ => "Want me to put it on screen?".into(),
    }
}

/// A column against the right edge, the full height of the screen.
fn column(m: &Monitor, cfg: &PanelConfig) -> Placement {
    Placement {
        monitor: m.id,
        rect: Rect {
            x: m.x + m.width - cfg.column_width,
            y: m.y,
            width: cfg.column_width,
            height: m.height,
        },
        // On top: you asked for it, so it shouldn't disappear behind the
        // window you look at next.
        on_top: true,
        chromeless: true,
    }
}

/// You said yes to the question. Put it on the only screen you have — down the
/// side, so it takes as little as possible.
pub fn place_anyway(panel: Panel, monitors: &[Monitor], cfg: &PanelConfig) -> Option<Placement> {
    let _ = panel;
    Some(column(primary(monitors)?, cfg))
}

/// "Put it on the other screen" — the override, for when you do want it out
/// of the way.
pub fn place_on_second(panel: Panel, monitors: &[Monitor], cfg: &PanelConfig) -> Option<Placement> {
    let _ = panel;
    Some(column(secondary(monitors)?, cfg))
}

/// The command line for a chromeless window.
///
/// A browser in app mode has no tabs, no address bar and no title — it is a
/// bare rectangle of your own content, which is what a panel needs to be.
pub fn window_args(url: &str, p: &Placement) -> Vec<String> {
    vec![
        format!("--app={url}"),
        format!("--window-position={},{}", p.rect.x, p.rect.y),
        format!("--window-size={},{}", p.rect.width, p.rect.height),
        // Its own profile, so it never inherits your tabs, extensions or
        // session.
        "--user-data-dir=data/panel-profile".into(),
        "--no-first-run".into(),
        "--disable-extensions".into(),
        "--disable-features=TranslateUI".into(),
    ]
}

/// What Atlas says as a panel appears. It speaks either way — the panel is for
/// reading at a glance, the words are what you actually take in.
pub fn narration(panel: Panel, contents: &str, cfg: &PanelConfig) -> Option<String> {
    if !cfg.speak_as_well {
        return None;
    }
    match panel {
        Panel::Presence => None,
        Panel::Tasks | Panel::Mind | Panel::Waking => Some(contents.to_string()),
        Panel::Controls => Some("Settings are up.".into()),
    }
}

#[cfg(test)]
mod transient_tests {
    use super::*;

    #[test]
    fn only_the_waking_panel_leaves_by_itself() {
        // Restored in the 26 Sep merge: the third chat's line removed
        // `transient` as unused, and ours had wired it into `faded`.
        assert!(Panel::Waking.transient());
        assert!(!Panel::Controls.transient());
    }
}
