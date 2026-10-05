//! The typing box (Eric's ruling H1): press the key, a one-line box opens
//! over whatever you're doing, type, press Enter, and it's gone.
//!
//! Its own small process (`atlas typebox`), like the overlay, so the box can
//! never stall the background Atlas and the other way round. What you type is
//! printed on one line to its output, which the background Atlas reads as a
//! typed turn — the same door as the console.
//!
//! The box follows `quickinput::QuickInput`: Enter sends, Enter on nothing or
//! Escape closes, Backspace takes a letter back, and a box you opened and
//! wandered away from closes by itself after `quick_input.idle_close_secs`.
//! Its look is plain on purpose: the panels' look waits on the hub's design
//! (H2), and this is the working part.

#[cfg(feature = "desktop-ui")]
use crate::quickinput::{Action, QuickInput};
use crate::quickinput::QuickInputConfig;

/// The line a finished box prints, so the reader can tell a submission from
/// anything else a process might write.
pub const SENT: &str = "ATLAS-TYPED:";

/// Keys, turned into what the box should do. Kept apart from drawing so it's
/// tested without a screen.
#[cfg(feature = "desktop-ui")]
pub fn apply_keys(q: &mut QuickInput, events: &[eframe::egui::Event], t: u64) -> Action {
    use eframe::egui::{Event, Key};
    for e in events {
        match e {
            Event::Text(s) => {
                for c in s.chars() {
                    q.typed(c, t);
                }
            }
            Event::Paste(s) => {
                for c in s.chars().filter(|c| !c.is_control()) {
                    q.typed(c, t);
                }
            }
            Event::Key { key: Key::Backspace, pressed: true, .. } => q.backspace(t),
            Event::Key { key: Key::Enter, pressed: true, .. } => return q.submit(),
            Event::Key { key: Key::Escape, pressed: true, .. } => return q.escape(),
            _ => {}
        }
    }
    q.tick(t)
}

/// A line the box printed, as the typed words, or `None` for anything else.
pub fn typed_line(line: &str) -> Option<String> {
    line.strip_prefix(SENT).map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// The box's window title: how the background Atlas's signal finds it.
pub const TITLE: &str = "Atlas typing box";

/// Open the box. Blocks until it's sent or closed.
///
/// `standby`: started with the background Atlas and kept hidden, shown on
/// "show" arriving on its input, hidden again after each line. Measured on
/// Eric's laptop (26 Sep 2026): a box started fresh on the key press took
/// about four seconds to appear, which is no good for "press and type".
#[cfg(feature = "desktop-ui")]
pub fn run(cfg: QuickInputConfig, standby: bool) -> Result<(), String> {
    let mut q = QuickInput::new(cfg);
    if !standby {
        q.hotkey(None, crate::store::now());
    }
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_icon(crate::mark::window_icon())
            .with_title(TITLE)
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_resizable(false)
            .with_active(!standby)
            .with_transparent(false)
            .with_visible(false)
            .with_inner_size([560.0, 48.0]),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "atlas-typebox",
        opts,
        Box::new(move |cc| {
            // The window by its handle, from eframe (`winpark`).
            let hwnd = crate::winpark::handle_of(cc);
            let (tx, rx) = std::sync::mpsc::channel::<Option<isize>>();
            if standby {
                touch(false);
                std::thread::spawn(|| loop {
                    std::thread::sleep(std::time::Duration::from_secs(5));
                    let idle = crate::store::now().saturating_sub(LAST_USED.load(std::sync::atomic::Ordering::Relaxed));
                    if !ON_SCREEN.load(std::sync::atomic::Ordering::Relaxed) && idle >= HIDDEN_EXIT.as_secs() {
                        std::process::exit(0);
                    }
                });
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || {
                    use std::io::BufRead;
                    for line in std::io::stdin().lock().lines().map_while(|l| l.ok()) {
                        if line.trim() == "show" {
                            // Shown and given the keyboard straight away,
                            // from this thread, rather than waiting for the
                            // window's own loop to wake: on the laptop the
                            // first words typed went to the app underneath.
                            let was_in = front_window();
                            touch(true);
                            show_now(hwnd);
                            if tx.send(was_in).is_err() {
                                break;
                            }
                            ctx.request_repaint();
                        }
                    }
                    // The background Atlas went away: so does the box.
                    std::process::exit(0);
                });
            }
            Ok(Box::new(Box_ { q, frames: if standby { u32::MAX } else { 0 }, hidden_told: 0, hidden_since: None, standby, wake: rx, was_in: front_window(), hwnd }))
        }),
    )
    .map_err(|e| e.to_string())
}

#[cfg(feature = "desktop-ui")]
struct Box_ {
    q: QuickInput,
    /// Frames since it was shown; `u32::MAX` while it waits hidden.
    frames: u32,
    /// Its native window, from eframe (`winpark::handle_of`).
    hwnd: isize,
    /// Frames told "stay hidden" since it was last put away.
    hidden_told: u8,
    /// Since when it has sat hidden; it goes after `HIDDEN_EXIT`.
    hidden_since: Option<std::time::Instant>,
    standby: bool,
    wake: std::sync::mpsc::Receiver<Option<isize>>,
    /// The window you were in, given back when the box goes (Windows
    /// otherwise hands the keyboard to whatever it likes).
    was_in: Option<isize>,
}

#[cfg(feature = "desktop-ui")]
impl eframe::App for Box_ {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        // The locked hub design's colourway, not egui's default grey.
        crate::look_paint::dress(ctx);
        use eframe::egui::ViewportCommand;
        if let Ok(was_in) = self.wake.try_recv() {
            self.q.hotkey(None, crate::store::now());
            self.frames = 0;
            self.hidden_told = 0;
            self.hidden_since = None;
            self.was_in = was_in;
        }
        if self.frames == u32::MAX {
            // Waiting, hidden, for the next key press. Told to stay hidden
            // every time, and painted clear: on Windows the window came up
            // anyway after its first frame, and a frame that paints nothing
            // is a black bar across the screen (Eric, 27 Sep 2026).
            // For a few frames only: each command wakes the window again,
            // and told every frame the hidden box spun a core (29 Sep 2026).
            eframe::egui::CentralPanel::default()
                .frame(eframe::egui::Frame::none().fill(crate::look_paint::colourway().raised))
                .show(ctx, |_| {});
            // Nothing more asked of the loop once it's told: the "show" line
            // wakes it (see `run`), and a hidden window asking to be drawn
            // again every half second kept eframe's loop spinning -- a core
            // at 75% for a box nobody could see (29 Sep 2026).
            if self.hidden_told < crate::overlaywin::HIDE_FRAMES {
                { touch(false); ctx.send_viewport_cmd(ViewportCommand::Visible(false)); }
                self.hidden_told += 1;
                if self.hidden_told < crate::overlaywin::HIDE_FRAMES {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                } else {
                    // Parked: eframe stops waiting for a paint Windows never
                    // sends a hidden window (`winpark`).
                    crate::winpark::park(self.hwnd);
                }
            } else {
                // Hidden and settled for a while: gone, rather than kept
                // alive hidden -- that held 40% of a core on Eric's laptop
                // (1 Oct 2026). The key starts a fresh one (`Standby::show`
                // finds this one gone and the daemon starts another).
                if self.standby && self.hidden_since.get_or_insert_with(std::time::Instant::now).elapsed() >= HIDDEN_EXIT {
                    std::process::exit(0);
                }
                std::thread::sleep(crate::winpark::IDLE_NAP);
            }
            return;
        }
        // Shown, sized and brought to the front on the first frames. Windows
        // only lets a window take the keyboard from the app you're in when
        // the process that saw your key press allows it, which the
        // background Atlas does just before asking.
        if self.frames < 3 {
            if self.frames == 0 {
                ctx.send_viewport_cmd(ViewportCommand::InnerSize(eframe::egui::vec2(560.0, 48.0)));
            }
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.frames += 1;
            ctx.request_repaint();
        } else if ctx.input(|i| i.focused) {
            // It has the keyboard: no more asking for it, so losing it later
            // puts the box away rather than taking it back from you.
            self.frames = self.frames.max(40);
        } else if self.frames < 40 {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
            self.frames += 1;
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
        } else if self.standby && !ctx.input(|i| i.focused) {
            // Gone from under you: you clicked elsewhere, or Windows never
            // let it have the keyboard. Put away rather than left on top of
            // everything with your typing going to the window underneath
            // (29 Sep 2026). The key brings it back.
            { touch(false); ctx.send_viewport_cmd(ViewportCommand::Visible(false)); }
            self.frames = u32::MAX;
            return;
        }
        let events = ctx.input(|i| i.events.clone());
        let done = match apply_keys(&mut self.q, &events, crate::store::now()) {
            Action::Submit(text) => {
                crate::outln!("{SENT}{text}");
                use std::io::Write;
                let _ = std::io::stdout().flush();
                true
            }
            Action::Hide => true,
            _ => false,
        };
        if done {
            give_back(self.was_in.take());
            if self.standby {
                { touch(false); ctx.send_viewport_cmd(ViewportCommand::Visible(false)); }
                self.frames = u32::MAX;
            } else {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
            return;
        }
        let fill = eframe::egui::Frame::none()
            .fill(crate::look_paint::colourway().raised)
            .inner_margin(eframe::egui::Margin::symmetric(14.0, 8.0));
        eframe::egui::CentralPanel::default().frame(fill).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.label(eframe::egui::RichText::new(self.q.placeholder()).monospace().size(18.0).color(crate::look_paint::colourway().signal_text));
                ui.label(eframe::egui::RichText::new(format!("{}|", self.q.buffer)).size(18.0).color(crate::look_paint::colourway().text));
            });
        });
        ctx.request_repaint_after(std::time::Duration::from_millis(250));
    }
}

/// A build without the desktop window (`--no-default-features`, the lean
/// server build) has no box to draw, and says so rather than failing to build.
#[cfg(not(feature = "desktop-ui"))]
pub fn run(_cfg: QuickInputConfig, _standby: bool) -> Result<(), String> {
    Err("this build of Atlas has no windows, so there's no typing box -- type in the console instead".into())
}

/// When the box was last asked for or used, in seconds since the epoch, and
/// whether it's on screen: read by the standby box's own watch thread, which
/// ends it once it has sat hidden for `HIDDEN_EXIT` (1 Oct 2026: a parked
/// eframe window never ran its own check, and the hidden box still held a
/// fifth of a core).
static LAST_USED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static ON_SCREEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn touch(on_screen: bool) {
    LAST_USED.store(crate::store::now(), std::sync::atomic::Ordering::Relaxed);
    ON_SCREEN.store(on_screen, std::sync::atomic::Ordering::Relaxed);
}

/// How long a hidden box waits for its key before it goes.
pub const HIDDEN_EXIT: std::time::Duration = std::time::Duration::from_secs(90);

/// The box kept ready by the background Atlas.
pub struct Standby {
    input: std::process::ChildStdin,
    child: std::process::Child,
}

impl Standby {
    /// Start the hidden box, and hand what's typed into it to `send`.
    pub fn start(send: impl Fn(String) + Send + 'static) -> Result<Standby, String> {
        // The self-test's copy never puts anything on the screen or starts
        // another Atlas (1 Oct 2026: a briefing panel opened on Eric's screen from
        // inside the test).
        if crate::selftest::in_a_test() {
            return Err(crate::selftest::NOT_IN_A_TEST.into());
        }
        let exe = std::env::current_exe().map_err(|e| e.to_string())?;
        let mut child = crate::tools::command(exe)
            .args(["typebox", "--standby"])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| format!("the typing box wouldn't start: {e}"))?;
        crate::childjob::tie(&child);
        let out = child.stdout.take().ok_or("the typing box gave nothing to read")?;
        let input = child.stdin.take().ok_or("the typing box can't be reached")?;
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(out).lines().map_while(|l| l.ok()) {
                if let Some(text) = typed_line(&line) {
                    send(text);
                }
            }
        });
        Ok(Standby { input, child })
    }

    /// Show it. `false` when the box has gone and needs starting again.
    pub fn show(&mut self) -> bool {
        if matches!(self.child.try_wait(), Ok(Some(_))) {
            return false;
        }
        allow_to_front();
        use std::io::Write;
        writeln!(self.input, "show").and_then(|_| self.input.flush()).is_ok()
    }
}

impl Drop for Standby {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

/// Let the box Atlas is about to show come to the front: the background
/// Atlas just received your key press, so Windows lets it hand that on.
fn allow_to_front() {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};
        let _ = AllowSetForegroundWindow(ASFW_ANY);
    }
}

/// The window in front right now, before the box takes it.
#[cfg_attr(not(feature = "desktop-ui"), allow(dead_code))]
fn front_window() -> Option<isize> {
    #[cfg(windows)]
    unsafe {
        let h = windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
        if !h.0.is_null() {
            return Some(h.0 as isize);
        }
    }
    None
}

/// Put you back in the window you were in. The box is in front at this
/// moment, so Windows allows it.
#[cfg_attr(not(feature = "desktop-ui"), allow(dead_code))]
fn give_back(to: Option<isize>) {
    #[cfg(windows)]
    if let Some(h) = to {
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::SetForegroundWindow(windows::Win32::Foundation::HWND(h as *mut std::ffi::c_void));
        }
    }
    #[cfg(not(windows))]
    let _ = to;
}

/// Show the box and give it the keyboard (its window by handle).
#[cfg_attr(not(feature = "desktop-ui"), allow(dead_code))]
fn show_now(hwnd: isize) {
    crate::winpark::unpark(hwnd);
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_SHOW};
        if hwnd != 0 {
            let h = HWND(hwnd as *mut core::ffi::c_void);
            let _ = ShowWindow(h, SW_SHOW);
            let _ = SetForegroundWindow(h);
        }
    }
}
