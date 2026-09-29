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
