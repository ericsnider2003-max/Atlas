//! Atlas's own window.
//!
//! Four things go here, and nothing else: the brief when you come back, the
//! outstanding list when you ask for it, Atlas's thought process when you ask
//! to see it, and an urgent item it could not say out loud.
//!
//! **Not the hub.** The hub is a web page on localhost and it opens when you
//! ask for it, never on its own. This is a small panel Atlas owns and can put
//! in front of you.
//!
//! ## Why this runs as a separate process
//!
//! Not a stylistic choice. On macOS a window has to be created on the process
//! main thread — that is a hard rule of the platform, not a convention — and
//! Atlas's main thread is the daemon loop, which cannot block for as long as a
//! window is open. Spawning a thread does not fix it, because the restriction
//! is about *which* thread, not about blocking.
//!
//! So the daemon writes the panel to a file and launches `atlas window` with
//! it. That child gets its own main thread, the daemon carries on, and the
//! same code path works on Windows, macOS and Linux without a per-platform
//! branch. It also means a crash in the window cannot take Atlas down.

use serde::{Deserialize, Serialize};

/// Which of the four things this is. The kind decides the styling and whether
/// the window insists on being seen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Panel {
    /// What happened while you were away.
    Brief,
    /// The outstanding list, because you asked for it.
    Outstanding,
    /// How Atlas got to an answer, because you asked to see it.
    Thinking,
    /// Something urgent that could not be said out loud.
    Urgent,
}

impl Panel {
    /// The filename this panel stages under.
    ///
    /// **Taken from the improvements chat's 17 Sep window, and it fixes a real
    /// defect on this side.** This file used to build the path as
    /// `format!("{:?}.json", panel).to_lowercase()` — the `Debug` formatting of
    /// the enum. That puts a Rust variant name into a filesystem path, so
    /// renaming `Panel::Thinking` would silently change the staging filename,
    /// the writer and the reader would disagree, and the window would come up
    /// empty with nothing to say why. An explicit slug is a promise the
    /// compiler keeps.
    ///
    /// The same class of defect — a variant name reaching somewhere a person
    /// sees — is one of the four the other side planted and caught in this
    /// pass. Worth stating that this one was found by diffing rather than by a
    /// test, because no test on either side was watching this path.
    pub fn slug(&self) -> &'static str {
        match self {
            Panel::Brief => "brief",
            Panel::Outstanding => "outstanding",
            Panel::Thinking => "thinking",
            Panel::Urgent => "urgent",
        }
    }

    pub fn heading(&self) -> &'static str {
        match self {
            Panel::Brief => "While you were away",
            Panel::Outstanding => "Outstanding",
            Panel::Thinking => "How I got there",
            Panel::Urgent => "Needs you",
        }
    }

    /// The hub page with the whole of what this panel shows in short.
    /// The urgent one has none: it's a knock, and its detail waits for you.
    pub fn hub_page(&self) -> Option<&'static str> {
        match self {
            Panel::Brief => Some("/hub"),
            Panel::Outstanding => Some("/hub/outstanding"),
            Panel::Thinking => Some("/hub/now"),
            Panel::Urgent => None,
        }
    }

    /// Should this sit on top of what you are doing?
    ///
    /// Only the urgent one. A brief that steals focus while you are typing is
    /// a brief you will come to resent, and the whole point of the window is
    /// that it is less intrusive than talking.
    pub fn insists(&self) -> bool {
        matches!(self, Panel::Urgent)
    }
}

/// Everything the window needs, and nothing it has to go and fetch.
///
/// Deliberately self-contained: the child process gets this file and renders
/// it. It has no store, no config and no daemon, so it cannot disagree with
/// what Atlas meant to show you.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contents {
    pub panel: Panel,
    pub title: String,
    /// One line each. Formatting is deliberately thin — this is a panel, not
    /// a document.
    pub lines: Vec<String>,
    /// Shown small at the bottom. Usually why Atlas could not just say this.
    pub footer: Option<String>,
    /// The hub page this is the short version of: the panel's one-tap door
    /// into the full page (the locked design's pop-up rule, H2).
    #[serde(default)]
    pub open: Option<String>,
}

impl Contents {
    pub fn new(panel: Panel, title: &str, lines: Vec<String>) -> Contents {
        Contents { panel, title: title.into(), lines, footer: None, open: panel.hub_page().map(str::to_string) }
    }

    pub fn because(mut self, why: &str) -> Contents {
        self.footer = Some(why.into());
        self
    }

    /// A knock rather than a disclosure.
    ///
    /// The private case: you are told there is something and roughly what
    /// kind, and the content waits until you ask. Atlas cannot know who else
    /// can see your screen, so the safe version is the one that does not need
    /// to know.
    pub fn knock(title: &str) -> Contents {
        Contents {
            panel: Panel::Urgent,
            title: title.into(),
            lines: vec!["Ask me when you're ready.".into()],
            footer: Some("Kept off the screen because it's private.".into()),
            open: None,
        }
    }
}

/// Write the panel somewhere the child process can read it.
pub fn stage(c: &Contents) -> std::io::Result<std::path::PathBuf> {
    let dir = std::env::temp_dir().join("atlas-panels");
    std::fs::create_dir_all(&dir)?;
    // Named by kind rather than uniquely, so a second urgent item replaces the
    // first instead of stacking windows up behind each other.
    let path = dir.join(format!("{}.json", c.panel.slug()));
    let json = serde_json::to_string(c)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(&path, json)?;
    Ok(path)
}

pub fn read_staged(path: &std::path::Path) -> std::io::Result<Contents> {
    let text = std::fs::read_to_string(path)?;
    serde_json::from_str(&text)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Put a panel in front of the person, without blocking the caller.
///
/// Returns an error rather than panicking when there is no display — a
/// headless machine, a server, or a test run. Atlas still works there; the
/// message falls back to being held and spoken, which is what the outbox is
/// for.
pub fn open(c: &Contents) -> Result<(), String> {
    // The self-test's copy never puts anything on the screen or starts
    // another Atlas (1 Oct 2026: a briefing panel opened on Eric's screen from
    // inside the test).
    if crate::selftest::in_a_test() {
        return Err(crate::selftest::NOT_IN_A_TEST.into());
    }
    if !running_as_atlas() {
        return Err("I'm not running as the Atlas program, so there's no window of mine to open".into());
    }
    let path = stage(c).map_err(|e| format!("couldn't stage the panel: {e}"))?;
    let exe = std::env::current_exe().map_err(|e| format!("couldn't find myself on disk: {e}"))?;
    crate::tools::command(exe)
        .arg("window")
        .arg(&path)
        .spawn()
        // Handed over rather than dropped. `spawn().map(|_| ())` threw the
        // `Child` away on the same line, and on unix that leaves the finished
        // panel in the process table as a zombie for the rest of the session
        // -- one per notification, on a process that runs for days. See
        // `unwaited`.
        .map(crate::unwaited::dont_wait)
        .map_err(|e| format!("couldn't open a window: {e}"))
}

/// Is there any point trying to open a window on this machine?
///
/// Checked rather than assumed. On Linux a missing display is the normal case
/// on a server, and finding out by launching a child that dies immediately
/// would look like the message was delivered.
/// Is this process the Atlas program (`atlas`, `atlas.exe`)? A panel is
/// opened by running Atlas again with `window <file>`; from anything else,
/// that would run the wrong program.
///
/// Found running the whole suite natively on Windows (26 Sep 2026): the
/// test binary is `all.exe`, and a test that raised a notification ran
/// `all.exe window <file>`, which the test harness read as "run every test
/// with `window` in its name". Those ran again, raised more notifications,
/// and fought each other over the clipboard and the key chords. On Linux
/// there's no display in the test environment, so it never showed. The same
/// guard `firstlaunch::open_atlas_window` already had.
pub(crate) fn running_as_atlas() -> bool {
    std::env::current_exe()
        .ok()
        .and_then(|e| e.file_stem().map(|s| s.to_string_lossy().eq_ignore_ascii_case("atlas")))
        .unwrap_or(false)
}

pub fn can_open() -> bool {
    if !running_as_atlas() {
        return false;
    }
    // No desktop UI compiled in (a mobile/headless core build) means there is
    // nothing to spawn — say so, so `open` is never attempted and the message
    // falls back to being held and spoken through the outbox, exactly as when
    // there's no display. The phone shell draws its own panels from the hub.
    #[cfg(not(feature = "desktop-ui"))]
    {
        false
    }
    #[cfg(feature = "desktop-ui")]
    {
        #[cfg(target_os = "linux")]
        {
            std::env::var("DISPLAY").is_ok() || std::env::var("WAYLAND_DISPLAY").is_ok()
        }
        #[cfg(not(target_os = "linux"))]
        {
            true
        }
    }
}

/// Run the window. Called only by `atlas window <file>`, on its own main
/// thread.
///
/// Behind `desktop-ui` (on by default): this and everything below it is the
/// eframe/egui desktop panel, the one part of this module that pulls in the GUI
/// stack. A mobile or headless build drops the feature, so `eframe` is not
/// compiled and this window is not drawn — `Panel`/`Contents`/`open` above stay,
/// because the daemon decides *what* a panel says regardless of who draws it.
#[cfg(feature = "desktop-ui")]
pub fn run(c: Contents) -> Result<(), String> {
    let insists = c.panel.insists();
    let height = 120.0 + (c.lines.len() as f32 * 22.0);
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_icon(crate::mark::window_icon())
            .with_inner_size([420.0, height.min(520.0)])
            .with_always_on_top_if(insists)
            .with_decorations(true)
            .with_resizable(true)
            .with_title(format!("Atlas — {}", c.panel.heading())),
        ..Default::default()
    };
    eframe::run_native(
        "atlas-panel",
        opts,
        Box::new(|_cc| Ok(Box::new(App { c, opened: std::time::Instant::now() }))),
    )
    .map_err(|e| e.to_string())
}

#[cfg(feature = "desktop-ui")]
struct App {
    c: Contents,
    /// When the window opened, so the mark's animation is driven by real
    /// elapsed time rather than a frame counter that would run at whatever
    /// rate the machine happens to paint.
    opened: std::time::Instant,
}

/// Which state the mark should be in for this panel. Brief wakes (the
/// morning brief); Thinking thinks; Urgent
/// holds still (dim); the rest breathe idle. Kept next to the panel so the
/// state is decided once.
#[cfg(feature = "desktop-ui")]
fn mark_state_for(panel: Panel) -> crate::look_paint::MarkState {
    use crate::look_paint::MarkState;
    match panel {
        Panel::Thinking => MarkState::Thinking,
        Panel::Brief => MarkState::Waking,
        _ => MarkState::Idle,
    }
}

/// How often a window showing the mark in `state` needs drawing: often
/// enough that the motion is smooth, no oftener. The idle breath is a 4.8 s
/// swell in opacity, smooth at ten frames a second; thinking and waking move
/// the dot and want thirty. With the computer set to show fewer animations
/// nothing moves at all, and twice a second keeps the content current.
#[cfg(feature = "desktop-ui")]
pub(crate) fn repaint_every(state: crate::look_paint::MarkState) -> std::time::Duration {
    use crate::look_paint::MarkState;
    let (_, os) = crate::look_paint::palette_and_os();
    let ms = if os.reduce_motion {
        500
    } else {
        match state {
            MarkState::Idle => 100,
            _ => 33,
        }
    };
    std::time::Duration::from_millis(ms)
}

/// Paint Atlas's mark (the Folded A, `mark`) into the square at the left of
/// `rect`, in `state`, `t` seconds in. `voice` is how loud Atlas's voice is
/// right now (0..1) while it speaks (`speaking::Speaking::level_at`): then the
/// dot follows the real voice rather than a rhythm of its own. With the
/// computer set to show fewer animations, every state holds still.
#[cfg(feature = "desktop-ui")]
pub(crate) fn paint_mark(
    painter: &eframe::egui::Painter,
    rect: eframe::egui::Rect,
    state: crate::look_paint::MarkState,
    t: f32,
    voice: Option<f32>,
) {
    use crate::look_paint::MarkState;
    use crate::mark::{pose, Motion, BACK, DOT, FOLD, FRONT};
    use eframe::egui::{pos2, Color32, Shape, Stroke};
    let motion = match (state, voice) {
        (_, Some(_)) | (MarkState::Speaking, _) => Motion::Speaking,
        (MarkState::Thinking, _) => Motion::Thinking,
        (MarkState::Waking, _) => Motion::Waking,
        (MarkState::Idle, _) => Motion::Idle,
    };
    let (p, os) = crate::look_paint::palette_and_os();
    let pose = pose(motion, t, voice, os.reduce_motion);
    let side = rect.width().min(rect.height());
    let at = |(x, y): (f32, f32)| pos2(rect.left() + x / 100.0 * side, rect.center().y - side / 2.0 + y / 100.0 * side);
    let tone = |c: Color32, a: f32| Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), (a * pose.opacity * 255.0) as u8);
    let (front, back) = if pose.dim { (p.dim, p.dim) } else { (p.text, p.signal) };
    // The fold: the accent a step darker, the paper lying over itself.
    let fold = if pose.dim { p.soft } else { Color32::from_rgb((back.r() as f32 * 0.78) as u8, (back.g() as f32 * 0.78) as u8, (back.b() as f32 * 0.78) as u8) };
    let base = FRONT[0].1;
    // The ink leg rises from the base; the folded leg comes down from the crease.
    let rise = |pt: (f32, f32), k: f32| (pt.0, base - (base - pt.1) * k);
    let drop = |pt: (f32, f32), k: f32| (pt.0, crate::mark::CREASE_Y + (pt.1 - crate::mark::CREASE_Y) * k);
    let poly = |pts: Vec<(f32, f32)>, c: Color32| Shape::convex_polygon(pts.into_iter().map(at).collect(), c, Stroke::NONE);
    if pose.front_rise > 0.001 {
        painter.add(poly(FRONT.iter().map(|q| rise(*q, pose.front_rise)).collect(), tone(front, 1.0)));
    }
    if pose.back_drop > 0.001 {
        painter.add(poly(BACK.iter().map(|q| drop(*q, pose.back_drop)).collect(), tone(back, 1.0)));
    }
    if pose.fold_opacity > 0.001 && side >= 32.0 {
        painter.add(poly(FOLD.to_vec(), tone(fold, pose.fold_opacity)));
    }
    if pose.dot_opacity > 0.001 {
        let c = at((DOT.0, DOT.1 - pose.dot_lift));
        painter.circle_filled(c, DOT.2 / 100.0 * side * pose.dot_scale, tone(if pose.dim { p.dim } else { back }, pose.dot_opacity));
    }
}

#[cfg(feature = "desktop-ui")]
impl eframe::App for App {
    fn update(&mut self, ctx: &eframe::egui::Context, _f: &mut eframe::Frame) {
        // The locked hub design's colourway, not egui's default grey.
        crate::look_paint::dress(ctx);
        use eframe::egui::{self, FontId, RichText};


        // The panel is a living thing while it's open: the mark breathes and
        // the sweep travels, so egui keeps repainting rather than only on
        // input -- at the rate the motion needs, not as fast as the machine
        // can draw (27 Sep 2026: this was `request_repaint()` every frame,
        // a core kept busy drawing a slow breath). See `repaint_every`.
        ctx.request_repaint_after(repaint_every(mark_state_for(self.c.panel)));
        let t = self.opened.elapsed().as_secs_f32();

        // Slate glass over the desktop, not a hole punched in it: the panel
        // is the raised end of look's gradient, over ink.
        let frame = egui::Frame::none().fill(crate::look_paint::colourway().ink).inner_margin(egui::Margin::same(28.0));
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            // The mint line where the panel meets the screen edge (look's
            // `.panel:before`).
            let top = ui.max_rect().top();
            ui.painter().line_segment(
                [egui::pos2(ui.max_rect().left(), top), egui::pos2(ui.max_rect().right(), top)],
                egui::Stroke::new(1.5_f32, crate::look_paint::colourway().signal),
            );

            let panel = self.c.panel;

            // The head. Brief and Thinking carry the mark; Outstanding leads
            // with a count; Urgent leads with its heading in the warn colour.
            match panel {
                Panel::Brief | Panel::Thinking => {
                    let (rect, mark_resp) = ui.allocate_exact_size(egui::vec2(46.0, 46.0), egui::Sense::hover());
                    mark_resp.widget_info(|| {
                        let what = if panel == Panel::Thinking { "Atlas mark: working" } else { "Atlas mark" };
                        eframe::egui::WidgetInfo::labeled(eframe::egui::WidgetType::Label, true, what)
                    });
                    paint_mark(ui.painter(), rect, mark_state_for(panel), t, None);
                    ui.add_space(14.0);
                    ui.label(RichText::new(&self.c.title).font(FontId::proportional(26.0)).strong().color(crate::look_paint::colourway().text));
                }
                Panel::Outstanding => {
                    // The big count, look's `.count`: the number of things
                    // waiting, with the label beneath it.
                    let waiting = self.c.lines.len();
                    ui.label(RichText::new(waiting.to_string()).font(FontId::proportional(64.0)).color(crate::look_paint::colourway().text));
                    ui.label(RichText::new(if waiting == 1 { "thing waiting" } else { "things waiting" }).font(FontId::proportional(14.0)).color(crate::look_paint::colourway().soft));
                    ui.add_space(20.0);
                }
                Panel::Urgent => {
                    ui.label(RichText::new(&self.c.title).font(FontId::proportional(28.0)).strong().color(crate::look_paint::colourway().warn));
                    ui.add_space(16.0);
                }
            }

            // The rows: a list of things, one hairline between them, no boxes.
            let urgent = matches!(panel, Panel::Urgent);
            for (idx, line) in self.c.lines.iter().enumerate() {
                if idx > 0 {
                    let y = ui.cursor().top();
                    ui.painter().line_segment(
                        [egui::pos2(ui.max_rect().left(), y), egui::pos2(ui.max_rect().right(), y)],
                        egui::Stroke::new(1.0_f32, crate::look_paint::colourway().line),
                    );
                    ui.add_space(1.0);
                }
                ui.add_space(11.0);
                let colour = if urgent { crate::look_paint::colourway().warn } else { crate::look_paint::colourway().text };
                ui.label(RichText::new(line).font(FontId::proportional(18.0)).color(colour));
                ui.add_space(11.0);
            }

            // The footer: small, muted, a hairline above it, pinned to the
            // bottom (look's `.foot`).
            if let Some(f) = &self.c.footer {
                ui.add_space(16.0);
                let y = ui.cursor().top();
                ui.painter().line_segment(
                    [egui::pos2(ui.max_rect().left(), y), egui::pos2(ui.max_rect().right(), y)],
                    egui::Stroke::new(1.0_f32, crate::look_paint::colourway().line),
                );
                ui.add_space(12.0);
                ui.label(RichText::new(f).font(FontId::proportional(13.0)).color(crate::look_paint::colourway().soft));
            }

            // One way out, and it reads as text, not a bevelled button
            // (look's `.act`). Bottom-aligned so it never competes with the
            // content.
            ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
                ui.horizontal(|ui| {
                    let got = ui.add(
                        egui::Button::new(RichText::new("Got it").color(crate::look_paint::colourway().signal_text).font(FontId::proportional(15.0)))
                            .frame(false),
                    );
                    if got.clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                    // The one-tap door into the whole page (H2).
                    if let Some(page) = &self.c.open {
                        ui.add_space(18.0);
                        let open = ui.add(
                            egui::Button::new(RichText::new("Open in the hub").color(crate::look_paint::colourway().signal_text).font(FontId::proportional(15.0)))
                                .frame(false),
                        );
                        if open.clicked() {
                            let _ = crate::firstlaunch::open_atlas_window(&crate::firstlaunch::First::Hub(page.clone()));
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    }
                });
            });
        });
    }
}

/// Helper for `ViewportBuilder`, which has no conditional setter.
#[cfg(feature = "desktop-ui")]
trait AlwaysOnTopIf {
    fn with_always_on_top_if(self, yes: bool) -> Self;
}

#[cfg(feature = "desktop-ui")]
impl AlwaysOnTopIf for eframe::egui::ViewportBuilder {
    fn with_always_on_top_if(self, yes: bool) -> Self {
        if yes {
            self.with_always_on_top()
        } else {
            self
        }
    }
}
