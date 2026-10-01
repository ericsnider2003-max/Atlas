//! Atlas's icon by the clock (the notification area), owned by the
//! background Atlas on Windows.
//!
//! Eric, 28 Sep 2026: *"I don't want a command terminal to be open. When on
//! windows I don't even want to have the hub or the application open for
//! Atlas to run."* With no window open, the icon is how you know Atlas is
//! there, and the one place to reach it from: open it, open the hub in your
//! browser, pause it, or quit it.
//!
//! The menu and what each entry does are plain functions (`tray_menu`,
//! `tray_choice`, `tray_tooltip`) so they are tested anywhere. The Win32 part
//! — `Shell_NotifyIconW`, a hidden window on its own thread to receive the
//! icon's clicks, `TrackPopupMenu` — is Windows-only and has only been
//! cross-compiled here, never clicked.
//!
//! The icon's thread never touches the `Daemon`. Pausing and resuming are
//! *asked for* through a small queue the run loop empties every pass
//! (`tray_asks`), and go the same way as the hub's Pause button (`turn("pause")`);
//! the run loop tells the icon whether it is paused (`tray_paused_now`).
//! Quitting is `goodbye::please_stop`, the same in-process door Ctrl-C uses,
//! so Atlas saves and lets go of its lock on the way out.

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// `desktop:` in tools.yaml.
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DesktopConfig {
    /// Show Atlas's icon by the clock while the background Atlas runs.
    pub tray_icon: bool,
}

impl Default for DesktopConfig {
    fn default() -> Self {
        DesktopConfig { tray_icon: true }
    }
}

/// What an entry in the icon's menu (or a double-click on it) does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    /// Atlas's own window, on the hub.
    OpenAtlas,
    /// The hub in your own browser.
    OpenHubInBrowser,
    /// The same as the hub's "Pause Atlas".
    Pause,
    /// The same as the hub's "Carry on".
    Resume,
    /// Stop the background Atlas, cleanly.
    Quit,
}

pub const ID_OPEN: u32 = 1;
pub const ID_BROWSER: u32 = 2;
pub const ID_PAUSE: u32 = 3;
pub const ID_QUIT: u32 = 4;

/// The menu, top to bottom: (id, words). "Pause" and "Resume" are one entry
/// whose words follow whether Atlas is paused.
///
/// Pause says what it does (28 Sep 2026): the hub's Pause, which this is,
/// holds jobs, posts and offers *and* now turns the microphone off — no wake
/// word, no listening while Atlas speaks — until you resume (`micthread`).
/// It used to leave the microphone on, and the menu said only "Pause Atlas"
/// so as not to promise otherwise.
pub fn tray_menu(paused: bool) -> Vec<(u32, &'static str)> {
    vec![
        (ID_OPEN, "Open Atlas"),
        (ID_BROWSER, "Open the hub in my browser"),
        (ID_PAUSE, if paused { PAUSED_ENTRY } else { RUNNING_ENTRY }),
        (ID_QUIT, "Quit Atlas"),
    ]
}

/// The entry picked, as what to do. Unknown ids (0 is "clicked away") do
/// nothing.
pub fn tray_choice(id: u32, paused: bool) -> Option<TrayAction> {
    match id {
        ID_OPEN => Some(TrayAction::OpenAtlas),
        ID_BROWSER => Some(TrayAction::OpenHubInBrowser),
        ID_PAUSE => Some(if paused { TrayAction::Resume } else { TrayAction::Pause }),
        ID_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

/// The menu entry while Atlas runs.
pub const RUNNING_ENTRY: &str = "Pause Atlas and stop listening";
/// The menu entry while it's paused.
pub const PAUSED_ENTRY: &str = "Resume Atlas and listen again";

/// The words shown when the pointer rests on the icon.
pub fn tray_tooltip(paused: bool) -> &'static str {
    if paused {
        "Atlas — paused, not listening"
    } else {
        "Atlas — running"
    }
}

/// The words on the icon, with a line about the hub when there's something
/// to say about it (its usual port taken: `server::open_hub`). Kept under
/// Windows' 127 characters.
pub fn tray_tip(paused: bool, hub_note: Option<&str>) -> String {
    let head = tray_tooltip(paused).to_string();
    let Some(note) = hub_note.filter(|n| !n.trim().is_empty()) else { return head };
    let mut tip = format!("{head}\n{}", note.trim());
    while tip.encode_utf16().count() > 127 {
        tip.pop();
    }
    tip
}

static PAUSED: AtomicBool = AtomicBool::new(false);
static ASKS: Mutex<Vec<TrayAction>> = Mutex::new(Vec::new());
/// The hub's address with its token, as it is now: the hub may open late,
/// or on another port, after the icon is up (`server::open_hub`).
static HUB: Mutex<String> = Mutex::new(String::new());
/// A line about the hub for the icon's words (`tray_tip`).
static HUB_NOTE: Mutex<Option<String>> = Mutex::new(None);
/// How many times the icon has been taken away on the way out
/// (`take_icon_away`) -- counted on every platform, so the way out can be
/// tested where there is no icon.
static TAKEN_AWAY: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Where "Open the hub in my browser" goes, now.
pub fn tray_hub_address(url: &str) {
    if let Ok(mut h) = HUB.lock() {
        *h = url.to_string();
    }
}

/// What the icon's "Open the hub" would open now (empty: no hub yet).
pub fn tray_hub_now() -> String {
    HUB.lock().map(|h| h.clone()).unwrap_or_default()
}

/// Say something about the hub on the icon's words, or nothing (`None`).
pub fn tray_hub_note(note: Option<String>) {
    if let Ok(mut n) = HUB_NOTE.lock() {
        *n = note;
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn hub_note_now() -> Option<String> {
    HUB_NOTE.lock().ok().and_then(|n| n.clone())
}

/// The words the icon should show now.
#[cfg_attr(not(windows), allow(dead_code))]
fn tray_tip_now() -> String {
    if crate::goodbye::asked_to_stop() {
        return CLOSING_TIP.to_string();
    }
    tray_tip(tray_says_paused(), hub_note_now().as_deref())
}

/// The icon's words while Atlas is on its way out: it stays until the way
/// out has finished (30 Sep 2026: Quit took the icon away at once, while
/// Atlas was still saving, so it looked gone and could be started again
/// over a copy still writing).
pub const CLOSING_TIP: &str = "Atlas — closing, saving your things first…";

/// How long the icon waits for the way out before going anyway.
pub const CLOSING_WAIT_SECS: u64 = 20;

/// Take the icon away now, from any thread, before the process ends
/// (28 Sep 2026).
///
/// An update restart (`Daemon::restart_as`) ends this process with
/// `process::exit` straight after starting the new copy: nothing is
/// dropped, the icon's window never gets `WM_DESTROY`, and Explorer kept
/// the old icon beside the new one's until the pointer passed over it -- a
/// ghost Atlas by the clock after every update. `Shell_NotifyIconW(NIM_DELETE)`
/// may be called from any thread, so the way out calls it itself, whatever
/// happens to the icon's thread after. Harmless when there is no icon.
pub fn take_icon_away() {
    TAKEN_AWAY.fetch_add(1, Ordering::SeqCst);
    #[cfg(windows)]
    win::remove_now();
}

/// An update restart that couldn't start the new copy: this Atlas carries
/// on, so its icon comes back (the icon's timer puts it up again).
pub fn bring_icon_back() {
    #[cfg(windows)]
    win::allow_again();
}

/// How many times `take_icon_away` has run in this process (for tests).
pub fn icon_taken_away_count_for_test() -> usize {
    TAKEN_AWAY.load(Ordering::SeqCst)
}

/// Ask the run loop to do something only it may do (pause, resume).
pub fn tray_ask(a: TrayAction) {
    if let Ok(mut q) = ASKS.lock() {
        q.push(a);
    }
}

/// What the icon has asked for since the last pass. Emptied as it is read.
pub fn tray_asks() -> Vec<TrayAction> {
    ASKS.lock().map(|mut q| std::mem::take(&mut *q)).unwrap_or_default()
}

/// The run loop saying whether Atlas is paused, for the icon's words.
pub fn tray_paused_now(paused: bool) {
    PAUSED.store(paused, Ordering::SeqCst);
}

/// Whether the icon should say paused.
#[cfg_attr(not(windows), allow(dead_code))]
fn tray_says_paused() -> bool {
    PAUSED.load(Ordering::SeqCst)
}

/// The icon, while it's up. Dropping it takes the icon away and ends its
/// thread; Atlas keeps it until its run loop has ended.
pub struct TrayIcon {
    #[cfg(windows)]
    hwnd: isize,
    #[cfg(windows)]
    thread: Option<std::thread::JoinHandle<()>>,
}

/// Put Atlas's icon by the clock. `exe` is Atlas's own program (for "Open
/// Atlas"); `hub` is the hub's address with its token, which the hub trades
/// for a cookie on the first visit and takes out of the address bar — the
/// same way the phone's link opens it. An error off Windows, or when the
/// icon's window couldn't be made (the caller says so). An icon Explorer
/// isn't ready for yet is not an error: it is added as soon as it can be.
pub fn show_icon(exe: std::path::PathBuf, hub: String) -> Result<TrayIcon, String> {
    if !hub.is_empty() {
        tray_hub_address(&hub);
    }
    #[cfg(windows)]
    {
        win::start(exe)
    }
    #[cfg(not(windows))]
    {
        let _ = exe;
        Err("the icon by the clock is a Windows thing".into())
    }
}

impl Drop for TrayIcon {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            win::close(self.hwnd);
            if let Some(t) = self.thread.take() {
                let _ = t.join();
            }
        }
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::AtomicU32;
    use std::sync::OnceLock;
    use windows::core::{w, HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Shell::{
        ShellExecuteW, Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY,
        NOTIFYICONDATAW,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow,
        DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics, KillTimer, LoadIconW, LoadImageW,
        PostMessageW, PostQuitMessage, RegisterClassExW, RegisterWindowMessageW, SetForegroundWindow,
        SetMenuDefaultItem, SetTimer, TrackPopupMenu, TranslateMessage, HICON, HMENU, IDI_APPLICATION,
        IMAGE_ICON, LR_DEFAULTCOLOR, MF_SEPARATOR, MF_STRING, MSG, SM_CXSMICON, SM_CYSMICON, SW_SHOWNORMAL,
        TPM_NONOTIFY, TPM_RETURNCMD, TPM_RIGHTBUTTON, WINDOW_EX_STYLE, WM_APP, WM_CLOSE, WM_CONTEXTMENU,
        WM_DESTROY, WM_ENDSESSION, WM_LBUTTONDBLCLK, WM_NULL, WM_RBUTTONUP, WM_TIMER, WNDCLASSEXW, WS_EX_TOOLWINDOW,
        WS_OVERLAPPED,
    };

    const WM_TRAY: u32 = WM_APP + 1;
    const TIMER: usize = 1;
    /// When the icon first saw Atlas asked to stop (0: not yet).
    static STOP_SEEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    struct Place {
        exe: PathBuf,
    }
    static PLACE: OnceLock<Place> = OnceLock::new();
    static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
    /// The words last put on the icon, so they're changed only when they
    /// change.
    static SHOWN_TIP: Mutex<String> = Mutex::new(String::new());
    /// The icon's window, for `remove_now` from another thread (0: none).
    static HWND_NOW: std::sync::atomic::AtomicIsize = std::sync::atomic::AtomicIsize::new(0);
    /// Taken away for good: the timer doesn't put it back.
    static GONE: AtomicBool = AtomicBool::new(false);
    /// Whether the icon is up. At sign-in Atlas can start before Explorer's
    /// taskbar is ready, and adding the icon then fails: it is tried again
    /// every second until it takes, rather than given up on for the session.
    static ADDED: AtomicBool = AtomicBool::new(false);

    pub(super) fn start(exe: PathBuf) -> Result<TrayIcon, String> {
        if PLACE.set(Place { exe }).is_err() {
            return Err("the icon is already up".into());
        }
        let (tx, rx) = std::sync::mpsc::channel::<Result<isize, String>>();
        let thread = std::thread::Builder::new()
            .name("atlas-tray".into())
            .spawn(move || {
                // SAFETY: every Win32 call below is on this thread, which
                // owns the window and pumps its messages.
                let hwnd = match unsafe { make_window() } {
                    Ok(h) => h,
                    Err(e) => {
                        let _ = tx.send(Err(e));
                        return;
                    }
                };
                HWND_NOW.store(hwnd.0 as isize, Ordering::SeqCst);
                let _ = tx.send(Ok(hwnd.0 as isize));
                unsafe {
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, HWND::default(), 0, 0).0 > 0 {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }
            })
            .map_err(|e| e.to_string())?;
        match rx.recv() {
            Ok(Ok(hwnd)) => Ok(TrayIcon { hwnd, thread: Some(thread) }),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err("the icon's thread ended before it said anything".into()),
        }
    }

    pub(super) fn close(hwnd: isize) {
        // SAFETY: posting to a window that may already be gone just fails.
        unsafe {
            let _ = PostMessageW(HWND(hwnd as *mut _), WM_CLOSE, WPARAM(0), LPARAM(0));
        }
    }

    unsafe fn instance() -> HINSTANCE {
        GetModuleHandleW(None).map(|m| HINSTANCE(m.0)).unwrap_or_default()
    }

    unsafe fn make_window() -> Result<HWND, String> {
        let hinst = instance();
        let class = w!("AtlasTrayIcon");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(wndproc),
            hInstance: hinst,
            lpszClassName: class,
            ..Default::default()
        };
        RegisterClassExW(&wc);
        // Explorer says "TaskbarCreated" to every top-level window when it
        // starts again (a crash, an update): the icon has to be put back
        // then. A message-only window never hears it, so this is an
        // ordinary top-level window that is simply never shown.
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::SeqCst);
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
            class,
            w!("Atlas"),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            HWND::default(),
            HMENU::default(),
            hinst,
            None,
        )
        .map_err(|e| format!("couldn't make the icon's window: {e}"))?;
        add_icon(hwnd);
        SetTimer(hwnd, TIMER, 1000, None);
        Ok(hwnd)
    }

    /// Atlas's mark: the icon compiled into atlas.exe (resource 1,
    /// `windows/atlas.rc`), at the small-icon size; Windows' plain program
    /// icon if that can't be read.
    unsafe fn mark() -> HICON {
        let loaded = LoadImageW(
            instance(),
            PCWSTR(1 as *const u16),
            IMAGE_ICON,
            GetSystemMetrics(SM_CXSMICON),
            GetSystemMetrics(SM_CYSMICON),
            LR_DEFAULTCOLOR,
        );
        match loaded {
            Ok(HANDLE(h)) if !h.is_null() => HICON(h),
            _ => LoadIconW(HINSTANCE::default(), IDI_APPLICATION).unwrap_or_default(),
        }
    }

    fn fill_tip(data: &mut NOTIFYICONDATAW, words: &str) {
        let tip: Vec<u16> = words.encode_utf16().collect();
        let n = tip.len().min(data.szTip.len() - 1);
        data.szTip = [0; 128];
        data.szTip[..n].copy_from_slice(&tip[..n]);
    }

    fn data(hwnd: HWND) -> NOTIFYICONDATAW {
        NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: 1, ..Default::default() }
    }

    unsafe fn add_icon(hwnd: HWND) -> bool {
        if GONE.load(Ordering::SeqCst) {
            return false;
        }
        let words = tray_tip_now();
        if let Ok(mut t) = SHOWN_TIP.lock() {
            *t = words.clone();
        }
        let mut d = data(hwnd);
        d.uFlags = NIF_ICON | NIF_MESSAGE | NIF_TIP;
        d.uCallbackMessage = WM_TRAY;
        d.hIcon = mark();
        fill_tip(&mut d, &words);
        let ok = Shell_NotifyIconW(NIM_ADD, &d).as_bool();
        ADDED.store(ok, Ordering::SeqCst);
        ok
    }

    unsafe fn update_tip(hwnd: HWND) {
        if !ADDED.load(Ordering::SeqCst) {
            add_icon(hwnd);
            return;
        }
        let words = tray_tip_now();
        match SHOWN_TIP.lock() {
            Ok(mut t) if *t != words => *t = words.clone(),
            _ => return,
        }
        let mut d = data(hwnd);
        d.uFlags = NIF_TIP;
        fill_tip(&mut d, &words);
        let _ = Shell_NotifyIconW(NIM_MODIFY, &d);
    }

    unsafe fn remove_icon(hwnd: HWND) {
        let d = data(hwnd);
        let _ = Shell_NotifyIconW(NIM_DELETE, &d);
        ADDED.store(false, Ordering::SeqCst);
    }

    pub(super) fn allow_again() {
        GONE.store(false, Ordering::SeqCst);
    }

    /// `take_icon_away`: the icon removed from Explorer now, from whichever
    /// thread is on its way out, and never put back by the timer.
    pub(super) fn remove_now() {
        GONE.store(true, Ordering::SeqCst);
        let h = HWND_NOW.load(Ordering::SeqCst);
        if h == 0 {
            return;
        }
        // SAFETY: NIM_DELETE names the icon by its window and id; for a
        // window that has already gone it just fails.
        unsafe {
            remove_icon(HWND(h as *mut _));
        }
    }

    unsafe fn show_menu(hwnd: HWND) {
        let paused = tray_says_paused();
        let Ok(menu) = CreatePopupMenu() else { return };
        for (id, words) in tray_menu(paused) {
            if id == ID_QUIT {
                let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
            }
            let _ = AppendMenuW(menu, MF_STRING, id as usize, &HSTRING::from(words));
        }
        let _ = SetMenuDefaultItem(menu, ID_OPEN, 0);
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // Without this the menu stays up when you click elsewhere
        // (the documented TrackPopupMenu quirk for notification icons).
        let _ = SetForegroundWindow(hwnd);
        let picked = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, pt.x, pt.y, 0, hwnd, None);
        let _ = PostMessageW(hwnd, WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        if let Some(a) = tray_choice(picked.0 as u32, paused) {
            act(hwnd, a);
        }
    }

    unsafe fn act(hwnd: HWND, a: TrayAction) {
        let Some(place) = PLACE.get() else { return };
        match a {
            TrayAction::OpenAtlas => {
                // `home`: Atlas's window, which opens on the hub once set up.
                if let Ok(child) = crate::firstlaunch::spawn_quietly(&place.exe, &["home"]) {
                    crate::unwaited::dont_wait(child);
                }
            }
            // No hub address (the hub couldn't open): Atlas's window instead,
            // which says why.
            TrayAction::OpenHubInBrowser if tray_hub_now().is_empty() => act(hwnd, TrayAction::OpenAtlas),
            TrayAction::OpenHubInBrowser => {
                // The address as it is now: the hub may have opened late or
                // on another port since the icon went up.
                let target = HSTRING::from(tray_hub_now().as_str());
                ShellExecuteW(HWND::default(), w!("open"), &target, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL);
            }
            TrayAction::Pause | TrayAction::Resume => tray_ask(a),
            // The icon goes when the way out has finished (`WM_TIMER`).
            TrayAction::Quit => {
                crate::goodbye::please_stop();
                update_tip(hwnd);
            }
        }
    }

    unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
        if msg == WM_TRAY {
            match (lp.0 as u32) & 0xFFFF {
                WM_LBUTTONDBLCLK => act(hwnd, TrayAction::OpenAtlas),
                WM_RBUTTONUP | WM_CONTEXTMENU => show_menu(hwnd),
                _ => {}
            }
            return LRESULT(0);
        }
        let created = TASKBAR_CREATED.load(Ordering::SeqCst);
        if created != 0 && msg == created {
            // Explorer started again: whatever it had is gone.
            ADDED.store(false, Ordering::SeqCst);
            add_icon(hwnd);
            return LRESULT(0);
        }
        match msg {
            WM_TIMER => {
                // Asked to stop some other way (the window's Restart, Ctrl-C):
                // the icon goes first, rather than lingering until the
                // pointer passes over a dead one.
                //
                // 30 Sep 2026: not at once -- when the lock is let go (the
                // way out's last step), or after `CLOSING_WAIT_SECS`.
                if crate::goodbye::asked_to_stop() {
                    let now = crate::store::now();
                    let since = match STOP_SEEN.load(Ordering::SeqCst) {
                        0 => {
                            STOP_SEEN.store(now, Ordering::SeqCst);
                            now
                        }
                        s => s,
                    };
                    let lock = crate::onlyone::OnlyOne::at(&crate::roots::data_dir());
                    if !lock.path().exists() || now.saturating_sub(since) >= CLOSING_WAIT_SECS {
                        let _ = DestroyWindow(hwnd);
                    } else {
                        update_tip(hwnd);
                    }
                } else {
                    update_tip(hwnd);
                }
                LRESULT(0)
            }
            WM_CLOSE => {
                let _ = DestroyWindow(hwnd);
                LRESULT(0)
            }
            // Windows is signing you out or shutting down, and ends this
            // process as soon as this returns: Atlas's way out runs first
            // (state, helpers, the lock), for as long as Windows waits
            // without calling it hung (`goodbye::stop_and_wait`).
            WM_ENDSESSION if wp.0 != 0 => {
                let lock = crate::onlyone::OnlyOne::at(&crate::roots::data_dir());
                let _ = crate::goodbye::stop_and_wait(lock.path(), std::time::Duration::from_secs(4));
                LRESULT(0)
            }
            WM_DESTROY => {
                let _ = KillTimer(hwnd, TIMER);
                remove_icon(hwnd);
                HWND_NOW.store(0, Ordering::SeqCst);
                PostQuitMessage(0);
                LRESULT(0)
            }
            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}
