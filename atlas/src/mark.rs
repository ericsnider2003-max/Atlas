//! Atlas's mark: the Folded A (Eric, 27 Sep 2026).
//!
//! One strip of paper, folded once. It rises in ink, turns over at a flat
//! crease, and comes down showing its orange back, the way origami paper is
//! coloured on one side. Where it lies over itself the paper darkens: the
//! fold. The dot is the one thing that matters today, and the part that moves.
//!
//! It replaced a dot over an arc (which read as Amazon and Headspace) and,
//! before that, the hanging hairline trace. Everything that draws the mark
//! draws it from here:
//! - the native painter (`window::paint_mark`: the setup window, the panels,
//!   the morning brief and the idle overlay);
//! - the hub (`hub::MARK`);
//! - every generated icon: the web app's, the Windows program's, Android's
//!   launcher and notification icons, and the iPhone and iPad app icon
//!   (`design/mark/make_marks.py` writes them from these numbers, and
//!   `tests/the_mark_is_one_mark.rs` holds every file to them).
//!
//! **The geometry is paper's, not a drawing's.** The two legs are the same
//! strip, 21 units wide measured across the crease. The apex sits *on* the
//! crease, so the legs meet there and never cross, and the crease is a flat
//! edge rather than a point. The fold is exactly where the two legs overlap.
//! The numbers were computed from that rule, not placed by eye, and a test
//! checks the rule still holds.
//!
//! **Sizes:** at 16 px and up the dot stays (Eric: "including the dot"). The
//! fold tone drops out below 32 px, where it would only muddy the crease.
//!
//! **Motion** (`pose`), built from the 150/200/300 ms steps and moving only
//! position, size and opacity:
//! - Idle: dim, a very slow breath.
//! - Thinking: the dot rises toward the crease and settles.
//! - Speaking: the dot carries the voice (the real level when there is one),
//!   and the folded leg answers it.
//! - Waking (the morning brief): the ink leg rises (300), the strip folds
//!   over (300), the crease darkens (150), and the dot arrives (200).

/// The ink leg: the strip's front face, rising left to right. A 100-unit box.
pub const FRONT: [(f32, f32); 4] = [(13.51, 92.0), (34.49, 92.0), (60.49, 10.0), (39.51, 10.0)];
/// The orange leg: the strip's back face, coming down after the fold.
pub const BACK: [(f32, f32); 4] = [(86.49, 92.0), (65.51, 92.0), (39.51, 10.0), (60.49, 10.0)];
/// Where the back lies over the front: the fold, one tone darker.
pub const FOLD: [(f32, f32); 3] = [(60.49, 10.0), (39.51, 10.0), (50.0, 43.09)];
/// The dot: centre and radius.
pub const DOT: (f32, f32, f32) = (50.0, 79.0, 6.5);
/// The crease's height: the flat top edge.
pub const CREASE_Y: f32 = 10.0;

/// The four colours of one colourway: front (ink), back (accent), fold (the
/// accent a step darker), dot (the accent).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Colours {
    pub front: &'static str,
    pub back: &'static str,
    pub fold: &'static str,
    pub dot: &'static str,
}

/// Warm Paper, the lead. The icon's tile is `PAPER`.
pub const WARM_PAPER: Colours = Colours { front: "#37352F", back: "#D9730D", fold: "#A85708", dot: "#D9730D" };
/// Ember Dark. The tile is `EMBER`.
pub const EMBER_DARK: Colours = Colours { front: "#ECEFF3", back: "#EB9D4A", fold: "#C47A2C", dot: "#EB9D4A" };
/// Access (colour-blind safe), on white.
pub const ACCESS: Colours = Colours { front: "#12151A", back: "#0072B2", fold: "#005A8E", dot: "#0072B2" };

/// The app icon's tile on Warm Paper.
pub const PAPER: &str = "#FBFAF7";
/// The app icon's tile on Ember Dark.
pub const EMBER: &str = "#0C0F14";

fn path(points: &[(f32, f32)]) -> String {
    let mut d = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        d.push_str(&format!("{}{x:.2} {y:.2} ", if i == 0 { "M" } else { "L" }));
    }
    d.push('Z');
    d
}

/// The mark as SVG shapes in a 100-unit box, for a page or a file. `small`
/// leaves the fold tone out (32 px and below).
pub fn shapes(c: &Colours, small: bool) -> String {
    let mut s = format!(
        "<path d=\"{}\" fill=\"{}\"/><path d=\"{}\" fill=\"{}\"/>",
        path(&FRONT),
        c.front,
        path(&BACK),
        c.back
    );
    if !small {
        s.push_str(&format!("<path d=\"{}\" fill=\"{}\"/>", path(&FOLD), c.fold));
    }
    s.push_str(&format!("<circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"{}\"/>", DOT.0, DOT.1, DOT.2, c.dot));
    s
}

/// A whole SVG file of the mark, with no tile: what `make_marks.py` writes
/// to `design/mark/`, which a test compares against.
pub fn svg_for_test(c: &Colours) -> String {
    format!("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">{}</svg>\n", shapes(c, false))
}

/// The icon Atlas's own windows carry in the taskbar and title bar, in the
/// colourway they're drawn in: Warm Paper, or Ember when Atlas's appearance
/// is dark (Eric: the colour follows the person's settings). Written by
/// `design/mark/make_marks.py`. `look_paint::dress` swaps it if the
/// appearance changes while a window is open.
#[cfg(feature = "desktop-ui")]
pub fn window_icon() -> std::sync::Arc<eframe::egui::IconData> {
    window_icon_for(crate::look_paint::colourway().dark)
}

#[cfg(feature = "desktop-ui")]
pub fn window_icon_for(dark: bool) -> std::sync::Arc<eframe::egui::IconData> {
    let png: &[u8] = if dark { include_bytes!("../assets/mark-256-dark.png") } else { include_bytes!("../assets/mark-256.png") };
    std::sync::Arc::new(eframe::icon_data::from_png_bytes(png).unwrap_or_default())
}

/// Which way the mark is moving.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Motion {
    Idle,
    Thinking,
    Speaking,
    Waking,
}

/// Where every part of the mark is at `t` seconds into a state: what the
/// painter draws each frame. Scales are 0..1 of full; offsets in mark units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// The whole mark's opacity.
    pub opacity: f32,
    /// How far the ink leg has risen from the base (waking).
    pub front_rise: f32,
    /// How far the folded leg has come down from the crease.
    pub back_drop: f32,
    pub fold_opacity: f32,
    pub dot_opacity: f32,
    /// How far the dot sits above its resting place.
    pub dot_lift: f32,
    pub dot_scale: f32,
    /// Idle is drawn in the dim colour, not the colourway's.
    pub dim: bool,
}

impl Pose {
    pub const STILL: Pose =
        Pose { opacity: 1.0, front_rise: 1.0, back_drop: 1.0, fold_opacity: 1.0, dot_opacity: 1.0, dot_lift: 0.0, dot_scale: 1.0, dim: false };
}

/// Ease-out (the enter curve), as the design's `cubic-bezier(.16,1,.3,1)`
/// closely enough at this size.
fn ease_out(p: f32) -> f32 {
    let p = p.clamp(0.0, 1.0);
    1.0 - (1.0 - p).powi(4)
}

/// A step of `dur` seconds starting at `from`, eased out, 0..1.
fn step(t: f32, from: f32, dur: f32) -> f32 {
    ease_out((t - from) / dur)
}

/// The pose for `motion` at `t` seconds. `voice` is the real voice level
/// (0..1) while speaking, when Atlas has one; without it the dot keeps an
/// irregular rhythm of its own. `still` is Reduce Motion: every state holds
/// its resting pose (idle still dims).
pub fn pose(motion: Motion, t: f32, voice: Option<f32>, still: bool) -> Pose {
    use std::f32::consts::TAU;
    if still {
        return Pose { dim: motion == Motion::Idle, opacity: if motion == Motion::Idle { 0.7 } else { 1.0 }, ..Pose::STILL };
    }
    match motion {
        // A breath over 4.8 s: 0.60..0.85.
        Motion::Idle => Pose { dim: true, opacity: 0.725 - 0.125 * (t / 4.8 * TAU).cos(), ..Pose::STILL },
        // 1.2 s: the dot rises 26 units toward the crease and settles; the
        // fold lightens as it rises.
        Motion::Thinking => {
            let s = 0.5 - 0.5 * (t / 1.2 * TAU).cos();
            Pose { dot_lift: 26.0 * s, fold_opacity: 1.0 - 0.45 * s, ..Pose::STILL }
        }
        Motion::Speaking => {
            let level = voice.unwrap_or_else(|| {
                // Two rates that don't divide into each other, so it never
                // falls into a visible loop.
                let a = (t / 0.30 * TAU).sin();
                let b = (t / 0.43 * TAU).sin();
                (0.5 + 0.25 * a + 0.25 * b).clamp(0.0, 1.0)
            });
            Pose { dot_scale: 0.8 + 0.48 * level, back_drop: 1.0 - 0.035 * level, ..Pose::STILL }
        }
        // 0.95 s: rise 0-300 ms, fold 300-600, crease 600-750, dot 750-950.
        Motion::Waking => {
            let dot = step(t, 0.75, 0.2);
            Pose {
                front_rise: step(t, 0.0, 0.3),
                back_drop: step(t, 0.3, 0.3),
                fold_opacity: step(t, 0.6, 0.15),
                dot_opacity: dot,
                dot_lift: 10.0 * (1.0 - dot),
                ..Pose::STILL
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The mark for the hub: the same shapes, coloured by the page's own tokens,
    /// so each colourway (and a person's accent) carries through. `hub::MARK` is
    /// this written out (the pages build with `format!`); a test holds them together.
    fn for_the_hub() -> String {
        let c = Colours {
            front: "var(--ink)",
            back: "var(--accent)",
            fold: "color-mix(in srgb, var(--accent) 78%, #000)",
            dot: "var(--accent)",
        };
        format!("<svg class=mark viewBox='0 0 100 100' aria-hidden=true>{}</svg>", shapes(&c, false).replace('"', "'"))
    }

    fn area(p: &[(f32, f32)]) -> f32 {
        let mut a = 0.0;
        for i in 0..p.len() {
            let (x1, y1) = p[i];
            let (x2, y2) = p[(i + 1) % p.len()];
            a += x1 * y2 - x2 * y1;
        }
        (a / 2.0).abs()
    }

    #[test]
    fn the_strip_meets_itself_at_a_flat_crease_and_never_crosses() {
        // Both legs span the same stretch of the crease: one strip, folded.
        let top = |p: &[(f32, f32)]| {
            let mut xs: Vec<f32> = p.iter().filter(|(_, y)| (*y - CREASE_Y).abs() < 1e-3).map(|(x, _)| *x).collect();
            xs.sort_by(|a, b| a.partial_cmp(b).unwrap());
            xs
        };
        assert_eq!(top(&FRONT), top(&BACK), "the legs don't meet along one crease");
        assert_eq!(top(&FRONT).len(), 2, "the crease is an edge, not a point");
        // Nothing sits above the crease.
        for (_, y) in FRONT.iter().chain(BACK.iter()).chain(FOLD.iter()) {
            assert!(*y >= CREASE_Y - 1e-3);
        }
        // The legs are mirror images: the same strip, the same width.
        assert!((area(&FRONT) - area(&BACK)).abs() < 0.05);
        // The fold's corners are the crease's ends and the point the legs' inner edges meet.
        assert_eq!(&FOLD[..2], &[BACK[3], BACK[2]]);
        assert!((FOLD[2].0 - 50.0).abs() < 1e-3);
    }

    #[test]
    fn the_dot_sits_clear_of_both_legs() {
        // Distance from the dot's centre to the leg's inner edge, less its radius.
        let clear = |a: (f32, f32), b: (f32, f32)| {
            let (dx, dy) = (b.0 - a.0, b.1 - a.1);
            ((dy * (DOT.0 - a.0) - dx * (DOT.1 - a.1)).abs() / (dx * dx + dy * dy).sqrt()) - DOT.2
        };
        assert!(clear(FRONT[1], FRONT[2]) > 4.0, "the dot crowds the ink leg");
        assert!(clear(BACK[1], BACK[2]) > 4.0, "the dot crowds the orange leg");
    }

    #[test]
    fn small_sizes_keep_the_dot_and_lose_the_fold_tone() {
        let s = shapes(&WARM_PAPER, true);
        assert!(s.contains("<circle"), "Eric: including the dot");
        assert!(!s.contains(WARM_PAPER.fold));
        assert!(shapes(&WARM_PAPER, false).contains(WARM_PAPER.fold));
    }

    #[test]
    fn waking_arrives_in_order_and_settles_whole() {
        let at = |t| pose(Motion::Waking, t, None, false);
        assert_eq!(at(0.0).front_rise, 0.0);
        assert!(at(0.3).front_rise > 0.99 && at(0.3).back_drop < 0.01, "the fold began before the leg had risen");
        assert!(at(0.6).back_drop > 0.99 && at(0.6).fold_opacity < 0.01);
        assert!(at(0.75).dot_opacity < 0.01, "the dot came before the fold");
        let done = at(1.0);
        assert!(done.front_rise > 0.99 && done.back_drop > 0.99 && done.fold_opacity > 0.99 && done.dot_opacity > 0.99);
        assert!(done.dot_lift.abs() < 0.01);
    }

    #[test]
    fn thinking_moves_only_the_dot_and_speaking_follows_the_voice() {
        let top = pose(Motion::Thinking, 0.6, None, false);
        assert!((top.dot_lift - 26.0).abs() < 0.01 && top.front_rise == 1.0 && top.back_drop == 1.0);
        let quiet = pose(Motion::Speaking, 0.0, Some(0.0), false);
        let loud = pose(Motion::Speaking, 0.0, Some(1.0), false);
        assert!(loud.dot_scale > quiet.dot_scale && loud.back_drop < quiet.back_drop);
    }

    #[test]
    fn reduce_motion_holds_every_state_still() {
        for m in [Motion::Thinking, Motion::Speaking, Motion::Waking] {
            for t in [0.0, 0.4, 1.1] {
                let p = pose(m, t, Some(0.9), true);
                assert_eq!(Pose { ..p }, Pose::STILL, "{m:?} moved with Reduce Motion on");
            }
        }
        let idle = pose(Motion::Idle, 2.0, None, true);
        assert!(idle.dim && idle.opacity == pose(Motion::Idle, 0.0, None, true).opacity);
    }

    #[test]
    fn the_hub_draws_the_same_mark() {
        assert_eq!(crate::hub::MARK, for_the_hub(), "hub::MARK has drifted from the mark");
    }

    #[test]
    fn idle_breathes_slowly_and_dim() {
        let lo = pose(Motion::Idle, 0.0, None, false).opacity;
        let hi = pose(Motion::Idle, 2.4, None, false).opacity;
        assert!((lo - 0.6).abs() < 0.01 && (hi - 0.85).abs() < 0.01, "{lo} {hi}");
        assert!(pose(Motion::Idle, 1.0, None, false).dim);
    }
}
