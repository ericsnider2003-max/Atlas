//! Parking a hidden helper window so it costs nothing (29 Sep 2026).
//!
//! The overlay and the typing box are eframe windows kept hidden until
//! needed. eframe keeps a window's pending redraw until Windows delivers the
//! paint, and Windows never paints a hidden window, so eframe polled for it
//! without pause: on Eric's laptop each hidden helper held a third of a core
//! doing nothing. eframe drops the pending redraw only for a *minimized*
//! window. So a hidden helper is marked minimized too -- its style bit, set
//! directly, so nothing appears on screen -- and the mark is taken off again
//! just before it is shown.
//!
//! The window is named by its handle, which eframe gives when it creates
//! it (`handle_of`): looked up by title, it wasn't found at all on Eric's
//! laptop, so neither parking nor showing it by title did anything.

/// How long a hidden, settled helper window's loop naps each time it is
/// woken anyway (29 Sep 2026: measured on Eric's laptop, each hidden helper
/// was woken continuously and held a third of a core).
pub const IDLE_NAP: std::time::Duration = std::time::Duration::from_millis(200);

/// The native window handle of an eframe window (its creation context or
/// frame), or 0 when there isn't one.
#[cfg(all(windows, feature = "desktop-ui"))]
pub fn handle_of(w: &impl raw_window_handle::HasWindowHandle) -> isize {
    match w.window_handle().map(|h| h.as_raw()) {
        Ok(raw_window_handle::RawWindowHandle::Win32(h)) => h.hwnd.get(),
        _ => 0,
    }
}

/// Elsewhere there is no window style to set: nothing to name.
#[cfg(not(all(windows, feature = "desktop-ui")))]
pub fn handle_of<T>(_w: &T) -> isize {
    0
}

/// Mark the window `hwnd` minimized (it stays hidden).
pub fn park(hwnd: isize) {
    set_minimized_bit(hwnd, true);
}

/// Take the mark off, before the window is shown.
pub fn unpark(hwnd: isize) {
    set_minimized_bit(hwnd, false);
}

fn set_minimized_bit(hwnd: isize, on: bool) {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{GetWindowLongPtrW, SetWindowLongPtrW, GWL_STYLE};
        if hwnd == 0 {
            return;
        }
        let h = HWND(hwnd as *mut core::ffi::c_void);
        let style = GetWindowLongPtrW(h, GWL_STYLE);
        let want = with_minimized_bit(style, on);
        if want != style {
            let _ = SetWindowLongPtrW(h, GWL_STYLE, want);
        }
    }
    #[cfg(not(windows))]
    let _ = (hwnd, on);
}

/// `style` with Windows' minimized bit (WS_MINIMIZE, 0x2000_0000) set or clear.
pub fn with_minimized_bit(style: isize, on: bool) -> isize {
    const WS_MINIMIZE: isize = 0x2000_0000;
    if on {
        style | WS_MINIMIZE
    } else {
        style & !WS_MINIMIZE
    }
}
