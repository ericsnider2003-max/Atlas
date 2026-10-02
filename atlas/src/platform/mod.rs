//! Everything OS-specific lives behind this trait, so the orchestration logic
//! can be tested on any machine — including one that is not the target laptop.

use crate::config::AppSpec;
use crate::error::Result;

pub mod mock;
// Compiled everywhere (like `mock`) so a desktop test can exercise it, but
// selected by `here()` only on a phone.
pub mod mobile;
#[cfg(unix)]
pub mod posix;
#[cfg(windows)]
pub mod win;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Monitor {
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
    pub primary: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct WindowId(pub u64);

/// A key's Windows virtual-key code, for `press`: "ctrl", "shift", "alt",
/// "win", "enter", "tab", "esc", "backspace", "delete", arrows, "f1".."f12",
/// a letter or a digit. `None` for anything else, so a typo'd combination is
/// refused rather than guessed.
pub fn virtual_key(name: &str) -> Option<u16> {
    let n = name.to_ascii_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" => 0x11,
        "shift" => 0x10,
        "alt" => 0x12,
        "win" | "windows" | "super" => 0x5B,
        "enter" | "return" => 0x0D,
        "tab" => 0x09,
        "esc" | "escape" => 0x1B,
        "backspace" => 0x08,
        "delete" | "del" => 0x2E,
        "space" => 0x20,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        _ => {
            if let Some(f) = n.strip_prefix('f').and_then(|x| x.parse::<u16>().ok()).filter(|f| (1..=12).contains(f)) {
                return Some(0x70 + f - 1);
            }
            let mut cs = n.chars();
            match (cs.next(), cs.next()) {
                (Some(c), None) if c.is_ascii_alphanumeric() => c.to_ascii_uppercase() as u16,
                _ => return None,
            }
        }
    })
}

/// One copy on the system clipboard, read in-process for the history
/// (`cliphist`). `private` is the copying app's own word that it mustn't be
/// kept: password managers set Windows' `ExcludeClipboardContentFromMonitor
/// Processing` or `CanIncludeInClipboardHistory = 0`, or the older
/// `Clipboard Viewer Ignore`. A private copy is never read at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipCopy {
    Text(String),
    Private,
    /// Something that isn't text (a picture, files).
    NotText,
}

/// The pixels of a window, top row first, three bytes (R, G, B) a pixel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Grab {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
    /// The window's title, so what was read can say where from.
    pub title: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

// ---------------------------------------------------------------------------
// Which screen, in words and in pixels (29 Sep 2026: "I think Atlas is only
// seeing one of my monitors"). Pure, so the mock platform can prove the
// arithmetic the Windows capture relies on.
// ---------------------------------------------------------------------------

/// The monitor a rectangle (a window) is mostly on: the one it overlaps
/// most; the nearest when it overlaps none.
pub fn monitor_under(monitors: &[Monitor], r: PixelRect) -> Option<u32> {
    let overlap = |m: &Monitor| {
        let w = (r.x + r.width).min(m.x + m.width) - r.x.max(m.x);
        let h = (r.y + r.height).min(m.y + m.height) - r.y.max(m.y);
        i64::from(w.max(0)) * i64::from(h.max(0))
    };
    let apart = |m: &Monitor| {
        let (cx, cy) = (i64::from(r.x) + i64::from(r.width) / 2, i64::from(r.y) + i64::from(r.height) / 2);
        let (mx, my) = (i64::from(m.x) + i64::from(m.width) / 2, i64::from(m.y) + i64::from(m.height) / 2);
        (cx - mx).pow(2) + (cy - my).pow(2)
    };
    let best = monitors.iter().max_by_key(|m| overlap(m))?;
    if overlap(best) > 0 {
        return Some(best.id);
    }
    monitors.iter().min_by_key(|m| apart(m)).map(|m| m.id)
}

/// The whole desktop's extent: every monitor's rectangle together. Windows
/// calls it the virtual screen; its corner can be left of or above the
/// primary monitor's (negative x or y).
pub fn virtual_screen(rects: &[PixelRect]) -> Option<PixelRect> {
    let left = rects.iter().map(|r| r.x).min()?;
    let top = rects.iter().map(|r| r.y).min()?;
    let right = rects.iter().map(|r| r.x + r.width).max()?;
    let bottom = rects.iter().map(|r| r.y + r.height).max()?;
    Some(PixelRect { x: left, y: top, width: right - left, height: bottom - top })
}

/// Pictures of each screen, put together as the screens sit: the result is
/// the virtual screen, black where no monitor is. `None` for no pictures.
pub fn stitch(parts: &[(PixelRect, Grab)]) -> Option<Grab> {
    if parts.len() == 1 {
        return Some(parts[0].1.clone());
    }
    let rects: Vec<PixelRect> = parts.iter().map(|(r, g)| PixelRect { x: r.x, y: r.y, width: g.width as i32, height: g.height as i32 }).collect();
    let all = virtual_screen(&rects)?;
    let (w, h) = (all.width.max(1) as usize, all.height.max(1) as usize);
    let mut rgb = vec![0u8; w * h * 3];
    for ((r, g), placed) in parts.iter().zip(&rects) {
        let _ = r;
        let (ox, oy) = ((placed.x - all.x) as usize, (placed.y - all.y) as usize);
        let gw = g.width as usize;
        for row in 0..g.height as usize {
            let from = &g.rgb[row * gw * 3..(row + 1) * gw * 3];
            let at = ((oy + row) * w + ox) * 3;
            rgb[at..at + gw * 3].copy_from_slice(from);
        }
    }
    Some(Grab { width: w as u32, height: h as u32, rgb, title: format!("all {} screens", parts.len()) })
}

/// A screen named the way you'd name it: "your laptop screen", "the left
/// screen", "the middle screen", "the right screen" -- or, for one monitor,
/// "your screen". Left to right by where they sit; one above another by
/// "top"/"bottom".
pub fn describe_screen(monitors: &[Monitor], id: u32, built_in: Option<u32>) -> String {
    if monitors.len() <= 1 {
        return "your screen".into();
    }
    if Some(id) == built_in {
        return "your laptop screen".into();
    }
    let Some(me) = monitors.iter().find(|m| m.id == id) else { return "a screen".into() };
    // Placed among the others -- the laptop's own screen left out, since it
    // has its own name: with it on the right, the two monitors beside it are
    // still "left" and "right".
    let others: Vec<&Monitor> = monitors.iter().filter(|m| Some(m.id) != built_in).collect();
    if others.len() == 1 {
        return "your monitor".into();
    }
    let mut xs: Vec<i32> = others.iter().map(|m| m.x + m.width / 2).collect();
    xs.sort();
    xs.dedup();
    let my_x = me.x + me.width / 2;
    if xs.len() == 1 {
        // One above another.
        let top = others.iter().map(|m| m.y).min().unwrap_or(me.y);
        return if me.y == top { "the top screen".into() } else { "the bottom screen".into() };
    }
    let pos = xs.iter().position(|x| *x == my_x).unwrap_or(0);
    let n = xs.len();
    let side = if pos == 0 {
        "left"
    } else if pos == n - 1 {
        "right"
    } else if n == 3 {
        "middle"
    } else {
        return format!("screen {} from the left", pos + 1);
    };
    format!("the {side} screen")
}

/// Which monitor (by id) shows the laptop's own panel: the monitor whose
/// device name ("\\\\.\\DISPLAY1") is one the display settings give an
/// internal output's source. Windows names both sides the same way; this is
/// the matching, kept here so it's proved off Windows too.
pub fn builtin_among(monitors: &[(u32, String)], builtin_sources: &[String]) -> Option<u32> {
    monitors.iter().find(|(_, dev)| builtin_sources.iter().any(|s| s.eq_ignore_ascii_case(dev))).map(|(id, _)| *id)
}

/// Which screens a request means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScreenPick {
    /// The one with the window you're in.
    Active,
    /// Every screen.
    All,
    /// One screen, named ("the left screen", "my laptop screen").
    This(u32),
}

/// What "look at my screen" means in what was said: "all my screens",
/// "both monitors", "every screen" -> all of them; "the left screen", "my
/// laptop screen" -> that one; anything else -> the one you're working on.
pub fn screens_asked_for(said: &str, monitors: &[Monitor], built_in: Option<u32>) -> ScreenPick {
    let s = said.to_lowercase();
    let plural = ["screens", "monitors", "displays"].iter().any(|w| s.contains(w));
    if (plural && ["all", "both", "every", "each", "my"].iter().any(|w| s.split(|c: char| !c.is_alphanumeric()).any(|x| x == *w)))
        || s.contains("every screen")
        || s.contains("every monitor")
        || s.contains("whole desktop")
        || s.contains("entire desktop")
    {
        return ScreenPick::All;
    }
    if monitors.len() > 1 {
        for m in monitors {
            let name = describe_screen(monitors, m.id, built_in);
            // "the left screen" -> "left"; "your laptop screen" -> "laptop".
            let key = name.trim_start_matches("the ").trim_start_matches("your ").trim_end_matches(" screen");
            if !key.is_empty() && s.split(|c: char| !c.is_alphanumeric()).any(|x| x == key) {
                return ScreenPick::This(m.id);
            }
        }
    }
    ScreenPick::Active
}

/// What you are actually looking at right now. Layer 1 awareness: cheap
/// enough to poll continuously, unlike screenshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActiveWindow {
    pub process: String,
    pub title: String,
}

/// What the operating system says about interrupting you right now
/// (Windows' `SHQueryUserNotificationState`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OsQuiet {
    /// Nothing in the way.
    Accepts,
    /// A full-screen app, or presentation settings applied.
    FullScreen,
    /// Presentation mode switched on to block pop-ups.
    Presenting,
    /// A full-screen (exclusive) game or video.
    Game,
    /// Locked, the screen saver, or another user's session.
    Away,
    /// The first hour after sign-in to a new account or an upgrade.
    QuietTime,
    /// A Store app has the screen.
    StoreApp,
}

impl OsQuiet {
    /// In words.
    pub fn plain(self) -> &'static str {
        match self {
            OsQuiet::Accepts => "Windows says it's fine to interrupt",
            OsQuiet::FullScreen => "a full-screen app has the screen",
            OsQuiet::Presenting => "presentation mode is on",
            OsQuiet::Game => "a full-screen game or video has the screen",
            OsQuiet::Away => "the machine is locked or showing the screen saver",
            OsQuiet::QuietTime => "Windows is in its first-hour quiet time",
            OsQuiet::StoreApp => "a Store app has the screen",
        }
    }

    /// Whether this state rules out an unasked interruption.
    pub fn holds_offers(self) -> bool {
        matches!(self, OsQuiet::FullScreen | OsQuiet::Presenting | OsQuiet::Game | OsQuiet::Away | OsQuiet::QuietTime)
    }
}

pub trait Platform {
    /// A cloud folder this machine already syncs, for bundles when no
    /// `sync.folder` is set (`sync::best_folder`, H13i). Behind the platform
    /// so a test's mock machine has none: the real one is Eric's Dropbox.
    fn cloud_folder(&self) -> Option<std::path::PathBuf> {
        crate::sync::best_folder().map(|(p, _)| p)
    }

    /// The focused window, or None if nothing is focused.
    fn active_window(&self) -> Result<Option<ActiveWindow>> {
        Ok(None)
    }
    /// The focused window's handle, for coming back to it — `None` where the
    /// platform can't say.
    fn active_window_id(&self) -> Result<Option<WindowId>> {
        Ok(None)
    }

    /// Seconds since the last keyboard or mouse input anywhere on the machine,
    /// or `None` where the platform can't say. Different from "since you last
    /// spoke to Atlas": this is how Atlas tells working quietly from away.
    /// On Windows, Atlas's own typing is left out (`platform::idle`), so a
    /// reply it just typed doesn't count as you being busy. (Both chats added
    /// this method, 25 Sep 2026; merged into one.)
    fn input_idle_secs(&self) -> Option<u64> {
        None
    }

    /// What the OS says about interrupting you now, or `None` where it
    /// can't say.
    fn quiet_state(&self) -> Option<OsQuiet> {
        None
    }

    /// Enumerated fresh every time. Displays get unplugged.
    fn monitors(&self) -> Result<Vec<Monitor>>;
    /// Is this machine's own screen on -- a laptop with its lid open?
    /// `Some(false)`: it has one and it is off (the lid shut behind other
    /// monitors). `None`: it has none (a desktop), or this can't be told.
    fn built_in_screen_on(&self) -> Option<bool> {
        None
    }
    fn launch(&self, spec: &AppSpec) -> Result<()>;
    /// None means "not up yet" — the caller retries.
    fn find_window(&self, spec: &AppSpec) -> Result<Option<WindowId>>;
    fn place(&self, win: WindowId, rect: PixelRect) -> Result<()>;
    fn focus(&self, win: WindowId) -> Result<()>;
    fn close(&self, spec: &AppSpec) -> Result<()>;
    fn sleep_ms(&self, ms: u64);

    // --- Input synthesis. Every one of these goes to the FOCUSED window, so
    // callers must own the foreground lane before using them (see lanes.rs).

    fn click(&self, x: i32, y: i32, button: Button) -> Result<()> {
        let _ = (x, y, button);
        Err(crate::error::AtlasError::Platform("clicking is not supported here".into()))
    }
    /// Positive dy scrolls up, matching a wheel.
    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        let _ = (dx, dy);
        Err(crate::error::AtlasError::Platform("scrolling is not supported here".into()))
    }
    fn type_text(&self, text: &str) -> Result<()> {
        let _ = text;
        Err(crate::error::AtlasError::Platform("typing is not supported here".into()))
    }
    /// A combo like "ctrl+f" or "enter".
    fn press(&self, combo: &str) -> Result<()> {
        let _ = combo;
        Err(crate::error::AtlasError::Platform("key presses are not supported here".into()))
    }
    /// Take `delete` characters back from where you're typing and type
    /// `text` in their place (`astype`'s fix of the word just finished). On
    /// Windows it is one burst that none of your own keys can land in the
    /// middle of; elsewhere, backspaces and then the text.
    fn replace_typed(&self, delete: usize, text: &str) -> Result<()> {
        for _ in 0..delete {
            self.press("backspace")?;
        }
        self.type_text(text)
    }
    /// A handle to just the pointer parts, safe to hand to another thread.
    ///
    /// Hand tracking runs on its own thread and must not borrow this one —
    /// see `handloop`. The real platforms hold no state for pointer work, so
    /// this builds a fresh one rather than sharing, which is what keeps a lock
    /// out of the hottest loop in the program.
    ///
    /// `None` from a platform that cannot drive the pointer, so hand control
    /// reports that plainly instead of starting a thread that does nothing.
    fn pointer_handle(&self) -> Option<Box<dyn crate::handloop::Pointer>> {
        None
    }

    /// Read a window's own accessibility tree.
    ///
    /// The alternative is a screenshot and text recognition, which is slower,
    /// wrong more often, and cannot tell a button from a label. This is how a
    /// pinch selects a *thing* rather than a point.
    ///
    /// `None` from a platform that cannot do it, so callers fall back rather
    /// than fail.
    fn read_window(&self, _win: WindowId) -> Result<Option<crate::uia::Node>> {
        Ok(None)
    }

    /// Press the control called `name` in a window, through the
    /// accessibility tree (Invoke), not by guessing a spot to click. `false`
    /// when there's no such control or it can't be pressed.
    fn press_named(&self, _win: WindowId, _name: &str) -> Result<bool> {
        Ok(false)
    }

    /// Do `act` to the control at `path` in the window's tree -- each number
    /// a child's place among its parent's children, as `read_window` gave
    /// them. `Ok(false)`: that control doesn't do that (the caller clicks
    /// it instead); `Err`: the tree couldn't be reached.
    fn act_on(&self, _win: WindowId, _path: &[usize], _act: &crate::uia::UiAct) -> Result<bool> {
        Ok(false)
    }

    /// The words on a picture of a window, line by line, each with where it
    /// is in the picture: what can be clicked in an app that doesn't show
    /// its controls to Windows (`operate`).
    fn recognise_lines(&self, _grab: &Grab) -> Result<Vec<(String, PixelRect)>> {
        Ok(Vec::new())
    }

    /// The text of the box you're typing in, when it can be read. Never a
    /// password box: `None` for one, and `None` when it can't tell.
    fn focused_text(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Whether the focused element takes typing. `None` where the platform
    /// can't tell, and callers go ahead as before.
    fn focused_is_editable(&self) -> Result<Option<bool>> {
        Ok(None)
    }

    /// The session is locked (Win+L, or the lock screen came up). `None`
    /// where the platform can't tell (H13a).
    fn session_locked(&self) -> Option<bool> {
        None
    }

    /// Draw Atlas's own marks on top of the desktop.
    ///
    /// An empty list clears them. Default does nothing, so a platform without
    /// an overlay silently draws nothing rather than refusing to run — the
    /// gesture still works, you just don't get the ring.
    fn draw_overlay(&self, _elements: &[crate::overlay::Element]) -> Result<()> {
        Ok(())
    }

    /// Put the pointer somewhere.
    ///
    /// The counterpart to `cursor`, which could read the pointer and never
    /// move it — so nothing could point at anything, which is the first thing
    /// a hand needs to do.
    fn move_cursor(&self, _x: i32, _y: i32) -> Result<()> {
        Err(crate::error::AtlasError::Platform("moving the pointer isn't built on this platform".into()))
    }

    /// Which window is under this point.
    ///
    /// Needed to pick something up by reaching for it. Without it a hand can
    /// only move whatever happens to be focused, which is a remote control
    /// again rather than reaching out and taking hold of a thing.
    fn window_at(&self, _x: i32, _y: i32) -> Result<Option<WindowId>> {
        Ok(None)
    }

    /// Where a window currently is.
    ///
    /// `place` could move a window and nothing could ask where it was, so
    /// nothing could put it back. That gap is why every gesture so far had to
    /// be irreversible or refused.
    fn rect_of(&self, _win: WindowId) -> Result<PixelRect> {
        Err(crate::error::AtlasError::Platform("reading a window's position isn't built on this platform".into()))
    }

    fn cursor(&self) -> Result<(i32, i32)> {
        Ok((0, 0))
    }

    /// Read the system clipboard, on request only — never watched in the
    /// background (see `clipboard.rs`, which never polls it). `None` means this
    /// platform has no way to read it, so the caller can say that plainly
    /// rather than reporting an empty clipboard it never actually saw.
    /// A number the OS bumps on every copy (Windows' clipboard sequence
    /// number), so the history can notice a copy without reading anything.
    /// `None`: this platform can't say, and there is no history here.
    fn clipboard_change(&self) -> Option<u32> {
        None
    }

    /// The copy now on the clipboard, read in-process and only when
    /// `clipboard_change` moved -- never on a timer by itself.
    fn clipboard_copy(&self) -> Option<ClipCopy> {
        None
    }

    /// The pixels of the window in front, for reading the text off it
    /// (`screentext`). `None`: this platform can't capture a window.
    fn grab_window(&self) -> Result<Option<Grab>> {
        Ok(None)
    }

    /// The pixels of one whole monitor (by `Monitor::id`), taskbar and all.
    /// `None`: this platform can't capture a screen itself (the configured
    /// capture tool is used instead).
    fn grab_screen(&self, _monitor: u32) -> Result<Option<Grab>> {
        Ok(None)
    }

    /// Where a monitor is on the desktop, whole (`monitors()` gives the part
    /// windows go in, without the taskbar).
    fn monitor_bounds(&self, monitor: u32) -> Option<PixelRect> {
        self.monitors().ok()?.into_iter().find(|m| m.id == monitor).map(|m| PixelRect { x: m.x, y: m.y, width: m.width, height: m.height })
    }

    /// Every screen in one picture, laid out as they sit (`stitch`).
    fn grab_all_screens(&self) -> Result<Option<Grab>> {
        let mut parts = Vec::new();
        for m in self.monitors()? {
            if let Some(g) = self.grab_screen(m.id)? {
                let at = self.monitor_bounds(m.id).unwrap_or(PixelRect { x: m.x, y: m.y, width: m.width, height: m.height });
                parts.push((at, g));
            }
        }
        Ok(stitch(&parts))
    }

    /// The monitor the window in front is on (`Monitor::id`). `None`: can't
    /// be told here.
    fn active_monitor(&self) -> Option<u32> {
        None
    }

    /// The laptop's own screen among `monitors()`, when this machine has one
    /// and it's on. `None`: a desktop, the lid shut, or can't be told.
    fn built_in_monitor(&self) -> Option<u32> {
        None
    }

    /// The OS's own text recognition, where it has one (Windows ships an
    /// on-device engine, `Windows.Media.Ocr`). `None`: no such engine here.
    fn recognise_text(&self, _grab: &Grab) -> Result<Option<String>> {
        Ok(None)
    }

    /// The same recognizer on a picture file (a photo handed over, a
    /// scan): decoded by the OS, turned the right way up, and read. `None`:
    /// no such engine here.
    fn recognise_image_file(&self, _path: &str) -> Result<Option<String>> {
        Ok(None)
    }

    /// Open a file with the app the OS uses for its type (a found file, a
    /// Start-menu shortcut). Refused here where no such opener exists.
    fn open_path(&self, path: &str) -> Result<()> {
        let _ = path;
        Err(crate::error::AtlasError::Platform("opening files isn't supported here".into()))
    }

    fn read_clipboard(&self) -> Result<Option<String>> {
        Ok(None)
    }

    /// Put text on the system clipboard, so an answer can land back where you
    /// were pasting. The default *refuses* rather than silently doing nothing,
    /// so the caller can tell you the truth about whether it got there — the
    /// whole point of the write-back is that it is real, not claimed.
    fn write_clipboard(&self, _text: &str) -> Result<()> {
        Err(crate::error::AtlasError::Platform(
            "writing the clipboard isn't built on this platform".into(),
        ))
    }
}


/// The platform layer for whatever this is running on.
///
/// Chosen at compile time, but the *answer to what it can do* is available at
/// runtime through `portable`, so nothing has to guess. Handing Atlas to
/// someone on a Mac means this returns `Posix` and everything that doesn't
/// need window management works immediately.
pub mod idle;
pub use idle::OwnInput;

pub fn here() -> Box<dyn Platform> {
    #[cfg(windows)]
    {
        win::become_dpi_aware();
        Box::new(win::WindowsPlatform)
    }
    // A phone is `unix`, so it must be carved out of the Posix branch below —
    // otherwise `here()` would hand a phone to the X11/Wayland layer, which is
    // exactly the wrong answer. Android and iOS both take the mobile layer.
    #[cfg(any(target_os = "android", target_os = "ios"))]
    {
        Box::new(mobile::MobilePlatform)
    }
    #[cfg(all(unix, not(windows), not(target_os = "android"), not(target_os = "ios")))]
    {
        Box::new(posix::Posix::here())
    }
    #[cfg(not(any(windows, unix)))]
    {
        Box::new(mock::Mock::default())
    }
}

/// Which platform this actually is, at runtime.
///
/// Not a guess and not a config setting — the compiler knows, and this makes
/// it available to everything that wants to say what will and won't work.
pub fn what_am_i() -> crate::portable::Platform {
    use crate::portable::Platform as P;
    if cfg!(windows) {
        P::Windows
    } else if cfg!(target_os = "macos") {
        P::Mac
    } else if cfg!(target_os = "android") {
        P::Android
    } else if cfg!(target_os = "ios") {
        P::Ios
    } else {
        P::Linux
    }
}
