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

/// The overlay window's title, which is how it is found to show and park.
pub const OVERLAY_TITLE: &str = "Atlas overlay";

/// Frames told to stay hidden after hiding: enough for Windows' own
/// show-after-first-frame to be undone, and no more.
pub const HIDE_FRAMES: u8 = 5;

/// The part of the screen the overlay draws in, as `(x, y, width, height)`:
/// the mark above the middle, the words and their shade across it.
///
/// Only this band, and only while Atlas speaks (29 Sep 2026). The window
/// used to cover the whole screen, above everything, all the time, relying
/// on transparency to be invisible. On Eric's laptop the transparency didn't
/// take: a black screen over everything from the moment Atlas started, with
/// no taskbar button and every click passing through it, so only Task
/// Manager got rid of it. Now a window that fails to be transparent is a
/// dark caption band for the length of a reply, then gone.
pub fn overlay_band(screen_w: i32, screen_h: i32, cfg: &crate::overlay::OverlayConfig) -> (i32, i32, i32, i32) {
    let cy = screen_h / 2;
    let x = (screen_w as f32 * 0.18) as i32;
    let w = (screen_w as f32 * 0.64) as i32;
    // The mark is 132 high and sits 40 above the middle; a little air above.
    let top = (cy - 132 - 40 - 24).max(0);
    let bottom = (cy + cfg.headline_size * 4).min(screen_h);
    (x, top, w, bottom - top)
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
            .with_title(OVERLAY_TITLE)
            .with_decorations(false)
            .with_transparent(true)
            .with_always_on_top()
            .with_mouse_passthrough(true)
            .with_taskbar(false)
            .with_active(false)
            .with_resizable(false)
            .with_visible(false)
            .with_position([0.0, 0.0])
            .with_inner_size([64.0, 64.0]),
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
        shown: false,
        hidden_told: 0,
        keyed: false,
        hwnd: 0,
        origin: (0.0, 0.0),
        band_w: 0.0,
        atlas: crate::onlyone::Watching::default(),
    };
    let watch_dir = app.data_dir.clone();
    let watch_cfg = app.config_dir.clone();
    let (own_lock, atlas_dir) = (app.only.clone(), app.data_dir.clone());
    eframe::run_native(
        "atlas-overlay",
        opts,
        Box::new(move |cc| {
            // The window by its handle, from eframe: looked up by title it
            // wasn't found on Eric's laptop (`winpark`).
            let hwnd = crate::winpark::handle_of(cc);
            let mut app = app;
            app.hwnd = hwnd;
            // A hidden window's own loop may not wake (Windows sends no
            // paint to a window that isn't shown), so a small thread watches
            // for Atlas starting to speak and wakes it.
            let ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let mut voice = crate::speaking::Watch::new(watch_dir);
                let mut last: Option<u64> = None;
                let mut enabled = overlay_cfg(&watch_cfg).enabled;
                let mut looked = std::time::Instant::now();
                let mut atlas = crate::onlyone::Watching::default();
                let mut checked = std::time::Instant::now();
                loop {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    // Still wanted? Asked here, not only in the window's own
                    // loop, which a hidden window may never run (29 Sep 2026:
                    // an overlay outlived its Atlas, and its lock went stale
                    // so a restarted Atlas could start a second one).
                    if checked.elapsed().as_secs() >= 3 {
                        checked = std::time::Instant::now();
                        let now = crate::store::now();
                        own_lock.beat(now);
                        if !atlas_is_up(&mut atlas, &atlas_dir, now) {
                            own_lock.release();
                            std::process::exit(0);
                        }
                    }
                    // Its switch, as the window itself reads it: switched
                    // off, nothing is ever put on screen.
                    if looked.elapsed().as_secs() >= 3 {
                        enabled = overlay_cfg(&watch_cfg).enabled;
                        looked = std::time::Instant::now();
                    }
                    let started = voice.now().filter(|s| s.still_going(crate::speaking::now_ms())).map(|s| s.started_ms);
                    if enabled && started.is_some() && started != last {
                        show_without_focus(hwnd);
                        ctx.request_repaint();
                    }
                    last = started.or(last);
                }
            });
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string())
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
    /// Whether the window is on screen, and where its band starts.
    shown: bool,
    /// Frames told "stay hidden" since it was last hidden (`HIDE_FRAMES`).
    hidden_told: u8,
    /// Whether Windows has been told the overlay is see-through.
    keyed: bool,
    /// Its native window, from eframe (`winpark::handle_of`); 0 until then.
    hwnd: isize,
    origin: (f32, f32),
    band_w: f32,
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

        // Invisible by Windows itself, not only by the graphics card: the
        // window flags `overlay::window_style` describes were written and
        // never applied (29 Sep 2026). Applied here, once the window exists.
        if !self.keyed {
            self.keyed = see_through(self.hwnd);
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

        // Nothing to say: off the screen entirely, told so every frame (a
        // window Windows shows after its first frame anyway is exactly how
        // the typing box became a black bar, 27 Sep 2026).
        if !alive {
            if self.shown {
                self.shown = false;
                self.hidden_told = 0;
            }
            // Told for a few frames after hiding, not every frame (29 Sep
            // 2026): each command wakes the window again, and told every
            // frame the hidden overlay spun a core at 40% doing nothing.
            egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |_| {});
            // Hidden, nothing more is asked of the window's own loop once it
            // is told: a hidden window gets no paint from Windows, so eframe
            // kept its loop spinning on every "again in 100 ms" -- the idle
            // overlay held a core at 40% (29 Sep 2026). The watcher thread
            // wakes it when Atlas starts to speak, and does the lock and the
            // is-Atlas-still-there check itself.
            if self.hidden_told < HIDE_FRAMES {
                ctx.send_viewport_cmd(ViewportCommand::Visible(false));
                self.hidden_told += 1;
                if self.hidden_told < HIDE_FRAMES {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                } else {
                    // Told enough: parked, so eframe stops waiting for a
                    // paint Windows never sends a hidden window (`winpark`).
                    crate::winpark::park(self.hwnd);
                }
            } else {
                // Hidden and settled, and still being woken: at most a few
                // times a second (`winpark::IDLE_NAP`), the way eframe itself
                // naps a minimized window.
                std::thread::sleep(crate::winpark::IDLE_NAP);
            }
            return;
        }
        let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) else {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
            return;
        };
        let (sw, sh) = (monitor.x as i32, monitor.y as i32);
        if !self.shown {
            let (x, y, w, h) = overlay_band(sw, sh, &self.cfg);
            self.origin = (x as f32, y as f32);
            self.band_w = w as f32;
            crate::winpark::unpark(self.hwnd);
            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(x as f32, y as f32)));
            ctx.send_viewport_cmd(ViewportCommand::InnerSize(egui::vec2(w as f32, h as f32)));
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            self.shown = true;
        }
        let (elements, opacity) = self.stage.frame(sw, sh, &self.cfg);
        let origin = egui::vec2(self.origin.0, self.origin.1);
        let wrap = self.band_w * 0.94;
        egui::CentralPanel::default().frame(egui::Frame::none()).show(ctx, |ui| {
            let painter = ui.painter();
            for e in &elements {
                draw(painter, e, origin, wrap, opacity, level, self.stage.showing.as_ref().map(|o| o.started_ms).unwrap_or(0), now);
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
/// Make the overlay see-through the way Windows does it for any program: a
/// layered window whose black pixels aren't drawn (`LWA_COLORKEY` with
/// `overlay::SEE_THROUGH_KEY`), that clicks pass through, off the taskbar and
/// never focused (`overlay::window_style`).
///
/// Why this and not transparency alone: an OpenGL window is see-through only
/// when the graphics driver hands Windows a picture with an alpha channel,
/// and on Eric's laptop it didn't -- the "transparent" window came out black
/// (29 Sep 2026). A colour key doesn't depend on the driver: Windows itself
/// leaves out every pixel of that colour. The overlay clears to black, so
/// everything it doesn't paint is left out.
///
/// True once applied; false while the window isn't there yet.
fn see_through(hwnd: isize) -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::{COLORREF, HWND};
        use windows::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW, GWL_EXSTYLE, LWA_COLORKEY,
        };
        if hwnd == 0 {
            return false;
        }
        let h = HWND(hwnd as *mut core::ffi::c_void);
        let style = GetWindowLongPtrW(h, GWL_EXSTYLE) as u32 | crate::overlay::window_style();
        SetWindowLongPtrW(h, GWL_EXSTYLE, style as isize);
        return SetLayeredWindowAttributes(h, COLORREF(crate::overlay::SEE_THROUGH_KEY), 0, LWA_COLORKEY).is_ok();
    }
    #[cfg(not(windows))]
    {
        let _ = hwnd;
        true
    }
}

#[cfg(feature = "desktop-ui")]
/// Put the overlay on screen without taking the keyboard, found by its
/// title (the window's own loop may be asleep while it's hidden).
fn show_without_focus(hwnd: isize) {
    // Unparked first: a parked window's loop doesn't run (`winpark`).
    crate::winpark::unpark(hwnd);
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_SHOWNOACTIVATE};
        if hwnd != 0 {
            let _ = ShowWindow(HWND(hwnd as *mut core::ffi::c_void), SW_SHOWNOACTIVATE);
        }
    }
}

#[cfg(feature = "desktop-ui")]
/// Paint one element of `overlay`'s design.
#[allow(clippy::too_many_arguments)]
fn draw(painter: &eframe::egui::Painter, e: &Element, origin: eframe::egui::Vec2, wrap: f32, opacity: f32, level: Option<f32>, started_ms: u64, now_ms: u64) {
    use crate::look_paint::MarkState as Paint;
    use eframe::egui::{self, pos2, vec2, Color32, FontId, Rect};
    let fade = |c: Color32, a: f32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * opacity * 255.0) as u8);
    match e {
        Element::Mark { x, y, size, state } => {
            let rect = Rect::from_min_size(pos2(*x as f32, *y as f32) - origin, vec2(*size as f32, *size as f32));
            let paint = match state {
                crate::overlay::MarkState::Waking => Paint::Waking,
                crate::overlay::MarkState::Thinking => Paint::Thinking,
                crate::overlay::MarkState::Idle => Paint::Idle,
            };
            let t = now_ms.saturating_sub(started_ms) as f32 / 1000.0;
            crate::window::paint_mark(painter, rect, paint, t, level);
        }
        // On Windows the overlay is see-through by colour key, which leaves
        // out exactly-black pixels only: a shade fading to black would stop
        // short of it and show as a dark smudge. The letters' own halo keeps
        // them readable there.
        Element::Shade { .. } if cfg!(windows) => {}
        Element::Shade { x, y, width, height, strength } => {
            // A darker region, not a box: full strength in the middle, nothing
            // at the edge.
            let outer = Rect::from_min_size(pos2(*x as f32, *y as f32) - origin, vec2(*width as f32, *height as f32));
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
            let font = FontId::proportional(*size as f32);
            let body = painter.layout(text.clone(), font.clone(), fade(crate::look_paint::colourway().text, 1.0), wrap);
            let halo = painter.layout(text.clone(), font, fade(crate::look_paint::colourway().ink, 0.75), wrap);
            let at = pos2(*x as f32 - body.size().x / 2.0, *y as f32) - origin;
            // A soft dark halo round every letter, so they read over anything.
            for (dx, dy) in [(-2.0, 0.0), (2.0, 0.0), (0.0, -2.0), (0.0, 2.0), (-1.5, -1.5), (1.5, 1.5), (-1.5, 1.5), (1.5, -1.5)] {
                painter.galley(at + vec2(dx, dy), halo.clone(), Color32::TRANSPARENT);
            }
            painter.galley(at, body, Color32::TRANSPARENT);
        }
        Element::Outline { .. } => {}
    }
}
