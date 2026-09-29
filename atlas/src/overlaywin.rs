//! Atlas's words on the desktop itself, while it speaks.
//!
//! `overlay` has held the design since the start — the mark arriving, the
//! words typing themselves in over whatever you're looking at, a faint shade
//! so they read over a white page, then gone — and nothing drew it (doc 19,
//! still open in doc 21). Eric, 24 Sep 2026, item 4: the desktop overlay.
//!
//! This draws it: a window with no frame, no background and no taskbar
//! button, above everything, that every click passes straight through. It is
//! its own small process (`atlas overlay`), started by the background Atlas
//! on Windows, so a stall in either can't freeze the other. It follows
//! `speaking`: when Atlas starts saying something, the mark arrives, the
//! words type in and the line moves with the voice; when it has finished and
//! the words have been read, it fades and the desktop is untouched again.
//!
//! It goes away by itself when the background Atlas does, and only one runs
//! at a time.

use crate::overlay::{Element, Overlay, OverlayConfig, Phase};
use crate::speaking::Speaking;

/// What the overlay is showing, decided from what's being said. Kept apart
/// from the drawing so it can be tested without a screen.
#[derive(Debug, Default)]
pub struct Stage {
    pub showing: Option<Overlay>,
    /// Which utterance is showing, by when it started, so one reply is
    /// shown once however many times the file is read.
    last_started: Option<u64>,
}

impl Stage {
    /// Move on to `now_ms`. Returns true while there's something to draw.
    pub fn step(&mut self, speaking: Option<&Speaking>, now_ms: u64, cfg: &OverlayConfig) -> bool {
        if let Some(s) = speaking {
            if Some(s.started_ms) != self.last_started && s.still_going(now_ms) && cfg.enabled {
                self.last_started = Some(s.started_ms);
                self.showing = Some(Overlay::begin(&crate::speaking::caption(&s.text), now_ms));
            }
        }
        let alive = match self.showing.as_mut() {
            Some(o) => o.tick(now_ms, cfg),
            None => false,
        };
        if !alive {
            self.showing = None;
        }
        alive
    }

    /// Switched off while showing: fade out rather than vanish mid-word.
    pub fn stand_down(&mut self, now_ms: u64) {
        if let Some(o) = self.showing.as_mut() {
            o.dismiss(now_ms);
        }
    }

    /// Everything to draw, and how opaque.
    pub fn frame(&self, w: i32, h: i32, cfg: &OverlayConfig) -> (Vec<Element>, f32) {
        match &self.showing {
            Some(o) if o.phase != Phase::Gone => (o.frame(w, h, cfg), o.opacity),
            _ => (Vec::new(), 0.0),
        }
    }
}

/// Should the overlay keep running? Only while the background Atlas holds
/// its lock — the overlay has nothing to show without it.
///
/// Watched over time, not decided on one look (28 Sep 2026): straight after
/// the laptop wakes the lock reads abandoned until Atlas's first tick beats
/// it, and the overlay used to close itself for good in that moment
/// (`onlyone::Watching`).
pub fn atlas_is_up(watching: &mut crate::onlyone::Watching, data_dir: &std::path::Path, now: u64) -> bool {
    watching.still_there(&crate::onlyone::OnlyOne::at(data_dir).look(now), now)
}

/// Where the overlay finds what's being said and its own switch.
pub struct Folders {
    /// Atlas's data folder: `speaking.json` and the background Atlas's lock.
    pub data: std::path::PathBuf,
    /// Atlas's settings, for `overlay.enabled`.
    pub config: std::path::PathBuf,
}

#[cfg(feature = "desktop-ui")]
/// Run the overlay. Blocks until the background Atlas goes away.
pub fn run(folders: Folders) -> Result<(), String> {
    let Folders { data: data_dir, config: config_dir } = folders;
    let only = crate::onlyone::OnlyOne::at(&data_dir.join("overlay"));
    let _ = std::fs::create_dir_all(data_dir.join("overlay"));
    match only.take(crate::store::now()) {
        Ok(_) => {}
        Err(_) => return Ok(()), // one is already showing; nothing to do
    }
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_icon(crate::mark::window_icon())
            .with_title("Atlas overlay")
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_active(false)
            .with_resizable(false)
            .with_position([0.0, 0.0])
            .with_inner_size([1280.0, 720.0]),
        ..Default::default()
    };
    let app = App {
        stage: Stage::default(),
        voice: crate::speaking::Watch::new(data_dir.clone()),
        cfg: overlay_cfg(&config_dir),
        config_dir,
        data_dir,
        only,
        last_check: std::time::Instant::now(),
        sized: false,
        atlas: crate::onlyone::Watching::default(),
    };
    eframe::run_native("atlas-overlay", opts, Box::new(|_cc| Ok(Box::new(app)))).map_err(|e| e.to_string())
}

#[cfg(feature = "desktop-ui")]
/// The overlay's settings as they stand now.
fn overlay_cfg(config_dir: &std::path::Path) -> OverlayConfig {
    crate::config::Config::load(config_dir)
        .ok()
        .and_then(|c| c.tools.map(|t| t.overlay))
        .unwrap_or_default()
}

#[cfg(feature = "desktop-ui")]
struct App {
    stage: Stage,
    voice: crate::speaking::Watch,
    cfg: OverlayConfig,
    config_dir: std::path::PathBuf,
    data_dir: std::path::PathBuf,
    only: crate::onlyone::OnlyOne,
    last_check: std::time::Instant,
    sized: bool,
    /// The background Atlas's lock, watched over time rather than trusted on
    /// one look: straight after a sleep it reads abandoned for the moment
    /// before Atlas beats again (`onlyone::Watching`).
    atlas: crate::onlyone::Watching,
}

#[cfg(feature = "desktop-ui")]
impl eframe::App for App {
    fn clear_color(&self, _visuals: &eframe::egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn update(&mut self, ctx: &eframe::egui::Context, _f: &mut eframe::Frame) {
        // The locked hub design's colourway, not egui's default grey.
        crate::look_paint::dress(ctx);
        use eframe::egui::{self, ViewportCommand};

        // Cover the whole screen once its size is known.
        if !self.sized {
            if let Some(size) = ctx.input(|i| i.viewport().monitor_size) {
                ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(0.0, 0.0)));
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(size));
                self.sized = true;
            }
        }

        // Every few seconds: still wanted, still switched on, still the one.
        if self.last_check.elapsed().as_secs() >= 3 {
            self.last_check = std::time::Instant::now();
            let now = crate::store::now();
            self.only.beat(now);
            if !atlas_is_up(&mut self.atlas, &self.data_dir, now) {
                ctx.send_viewport_cmd(ViewportCommand::Close);
                return;
            }
            self.cfg = overlay_cfg(&self.config_dir);
            if !self.cfg.enabled {
                self.stage.stand_down(crate::speaking::now_ms());
            }
        }

        let now = crate::speaking::now_ms();
        let level = self.voice.level();
        let speaking = self.voice.now().cloned();
        let alive = self.stage.step(speaking.as_ref(), now, &self.cfg);

        let screen = ctx.screen_rect();
        let (elements, opacity) = self.stage.frame(screen.width() as i32, screen.height() as i32, &self.cfg);
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let painter = ui.painter();
            for e in &elements {
                draw(painter, e, opacity, level, self.stage.showing.as_ref().map(|o| o.started_ms).unwrap_or(0), now);
            }
        });

        if alive {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000 / self.cfg.fps.clamp(10, 60) as u64));
        } else {
            // Nothing on screen: look for speech ten times a second, nothing more.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }
}

#[cfg(feature = "desktop-ui")]
/// Paint one element of `overlay`'s design.
fn draw(painter: &eframe::egui::Painter, e: &Element, opacity: f32, level: Option<f32>, started_ms: u64, now_ms: u64) {
    use crate::look_paint::MarkState as Paint;
    use eframe::egui::{self, pos2, vec2, Color32, FontId, Rect};
    let fade = |c: Color32, a: f32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * opacity * 255.0) as u8);
    match e {
        Element::Mark { x, y, size, state } => {
            let rect = Rect::from_min_size(pos2(*x as f32, *y as f32), vec2(*size as f32, *size as f32));
            let paint = match state {
                crate::overlay::MarkState::Waking => Paint::Waking,
                crate::overlay::MarkState::Thinking => Paint::Thinking,
                crate::overlay::MarkState::Idle => Paint::Idle,
            };
            let t = now_ms.saturating_sub(started_ms) as f32 / 1000.0;
            crate::window::paint_mark(painter, rect, paint, t, level);
        }
        Element::Shade { x, y, width, height, strength } => {
            // A darker region, not a box: full strength in the middle, nothing
            // at the edge.
            let outer = Rect::from_min_size(pos2(*x as f32, *y as f32), vec2(*width as f32, *height as f32));
            let inner = outer.shrink2(vec2(outer.width() * 0.3, outer.height() * 0.3));
            let dark = fade(crate::look_paint::colourway().ink, *strength);
            let clear = Color32::TRANSPARENT;
            let mut m = egui::Mesh::default();
            for (p, c) in [
                (outer.left_top(), clear), (outer.right_top(), clear), (outer.right_bottom(), clear), (outer.left_bottom(), clear),
                (inner.left_top(), dark), (inner.right_top(), dark), (inner.right_bottom(), dark), (inner.left_bottom(), dark),
            ] {
                m.colored_vertex(p, c);
            }
            for (a, b, c) in [(4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4), (1, 2, 6), (1, 6, 5), (2, 3, 7), (2, 7, 6), (3, 0, 4), (3, 4, 7)] {
                m.add_triangle(a, b, c);
            }
            painter.add(egui::Shape::mesh(m));
        }
        Element::Typed { x, y, text, size, .. } => {
            let wrap = painter.clip_rect().width() * 0.6;
            let font = FontId::proportional(*size as f32);
            let body = painter.layout(text.clone(), font.clone(), fade(crate::look_paint::colourway().text, 1.0), wrap);
            let halo = painter.layout(text.clone(), font, fade(crate::look_paint::colourway().ink, 0.75), wrap);
            let at = pos2(*x as f32 - body.size().x / 2.0, *y as f32);
            // A soft dark halo round every letter, so they read over anything.
            for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0), (-1.5, -1.5), (1.5, 1.5), (-1.5, 1.5), (1.5, -1.5)] {
                painter.galley(at + vec2(dx, dy), halo.clone(), Color32::TRANSPARENT);
            }
            painter.galley(at, body, Color32::TRANSPARENT);
        }
        Element::Outline { .. } => {}
    }
}
