//! Windows implementation.
//!
//! Cfg-gated to Windows. Cross-compiled for Windows on every build, and the
//! parts that can be are run on Eric's laptop; the tests on Linux run against
//! the mock platform, so a change here is proved on Windows or not at all.

use super::{ActiveWindow, Monitor, PixelRect, Platform, WindowId};
use crate::config::AppSpec;
use crate::error::{AtlasError, Result};
use std::process::Command;

use windows::Win32::Foundation::{BOOL, HWND, LPARAM, RECT, TRUE};
use windows::Win32::Graphics::Gdi::{
    EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_FORMAT, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible, SetForegroundWindow,
    SetWindowPos, ShowWindow, HWND_TOP, SWP_NOZORDER, SW_RESTORE,
};

/// Set in MONITORINFO.dwFlags for the primary display. Defined here because
/// the constant moved between windows-crate versions and pinning to it makes
/// the build fragile for one bit.
const MONITORINFOF_PRIMARY: u32 = 1;

pub struct WindowsPlatform;

/// Real pixels on every monitor, for the whole process: per-monitor DPI
/// awareness (v2), set once, first thing (`main`, and `platform::here`).
///
/// **Why (Eric, 29 Sep 2026: "I think Atlas is only seeing one of my
/// monitors").** Atlas never declared itself DPI-aware -- no manifest entry,
/// no call. Windows then *virtualises* every coordinate it hands Atlas to
/// the primary monitor's scale: on a laptop at 150% or 200% beside monitors
/// at 100%, `EnumDisplayMonitors`, `GetWindowRect` and a copy from the
/// screen's DC all come back scaled, so a capture of a window's rectangle
/// grabbed the wrong part of the desktop -- part of the right window, part of
/// another, or part of the wrong monitor -- and whatever was read was from
/// there. Per-monitor aware, every rectangle is in real pixels on every
/// monitor, whatever each one's scale. The hub's window library (tao/wry)
/// asks for the same awareness when it starts, so nothing else changes.
pub fn become_dpi_aware() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| unsafe {
        use windows::Win32::UI::HiDpi::{SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2};
        // Refused when the awareness was already set (by a manifest, or by
        // a window library first) -- which is as good.
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            // Before Windows 10 1703: system-wide awareness at least.
            let _ = windows::Win32::UI::WindowsAndMessaging::SetProcessDPIAware();
        }
    });
}

/// One monitor as Windows describes it.
#[derive(Clone)]
struct WinMon {
    id: u32,
    /// Where windows go: the taskbar left out.
    work: RECT,
    /// The whole screen.
    full: RECT,
    primary: bool,
    /// "\\.\DISPLAY1": how the display settings name it.
    device: String,
}

struct MonAcc(Vec<WinMon>);

unsafe extern "system" fn mon_cb(h: HMONITOR, _: HDC, _: *mut RECT, lp: LPARAM) -> BOOL {
    use windows::Win32::Graphics::Gdi::MONITORINFOEXW;
    let acc = &mut *(lp.0 as *mut MonAcc);
    let mut info = MONITORINFOEXW::default();
    info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
    if GetMonitorInfoW(h, &mut info as *mut MONITORINFOEXW as *mut MONITORINFO).as_bool() {
        let end = info.szDevice.iter().position(|c| *c == 0).unwrap_or(info.szDevice.len());
        acc.0.push(WinMon {
            id: h.0 as usize as u32,
            work: info.monitorInfo.rcWork,
            full: info.monitorInfo.rcMonitor,
            primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
            device: String::from_utf16_lossy(&info.szDevice[..end]),
        });
    }
    TRUE
}

/// Every monitor, in real pixels (`become_dpi_aware`).
fn all_monitors() -> Vec<WinMon> {
    become_dpi_aware();
    let mut acc = MonAcc(Vec::new());
    unsafe {
        let _ = EnumDisplayMonitors(None, None, Some(mon_cb), LPARAM(&mut acc as *mut MonAcc as isize));
    }
    acc.0
}

/// A rectangle of the screen as it looks now, copied in-process: red, green,
/// blue, top row first.
unsafe fn grab_rect(r: RECT) -> Option<(u32, u32, Vec<u8>)> {
    use windows::Win32::Graphics::Gdi::{
        BitBlt, CreateCompatibleBitmap, CreateCompatibleDC, DeleteDC, DeleteObject, GetDC, GetDIBits, ReleaseDC, SelectObject,
        BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CAPTUREBLT, DIB_RGB_COLORS, SRCCOPY,
    };
    let (w, h) = ((r.right - r.left).clamp(1, 16384), (r.bottom - r.top).clamp(1, 16384));
    let screen = GetDC(None);
    let mem = CreateCompatibleDC(screen);
    let bmp = CreateCompatibleBitmap(screen, w, h);
    let old = SelectObject(mem, bmp);
    let copied = BitBlt(mem, 0, 0, w, h, screen, r.left, r.top, SRCCOPY | CAPTUREBLT).is_ok();
    let mut info = BITMAPINFO::default();
    info.bmiHeader = BITMAPINFOHEADER {
        biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
        biWidth: w,
        biHeight: -h, // top row first
        biPlanes: 1,
        biBitCount: 32,
        biCompression: BI_RGB.0,
        ..Default::default()
    };
    let mut bgra = vec![0u8; (w as usize) * (h as usize) * 4];
    let lines = if copied { GetDIBits(mem, bmp, 0, h as u32, Some(bgra.as_mut_ptr() as *mut _), &mut info, DIB_RGB_COLORS) } else { 0 };
    SelectObject(mem, old);
    let _ = DeleteObject(bmp);
    let _ = DeleteDC(mem);
    ReleaseDC(None, screen);
    if lines == 0 {
        return None;
    }
    let rgb: Vec<u8> = bgra.chunks_exact(4).flat_map(|p| [p[2], p[1], p[0]]).collect();
    Some((w as u32, h as u32, rgb))
}

struct WinAcc {
    processes: Vec<String>,
    title_hints: Vec<String>,
    found: Option<HWND>,
}

unsafe extern "system" fn win_cb(hwnd: HWND, lp: LPARAM) -> BOOL {
    let acc = &mut *(lp.0 as *mut WinAcc);
    if acc.found.is_some() || !IsWindowVisible(hwnd).as_bool() {
        return TRUE;
    }

    let mut pid: u32 = 0;
    GetWindowThreadProcessId(hwnd, Some(&mut pid));
    if pid == 0 {
        return TRUE;
    }

    let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
        return TRUE;
    };
    let mut buf = [0u16; 512];
    let mut len = buf.len() as u32;
    let ok = QueryFullProcessImageNameW(
        handle,
        PROCESS_NAME_FORMAT(0),
        windows::core::PWSTR(buf.as_mut_ptr()),
        &mut len,
    )
    .is_ok();
    let _ = windows::Win32::Foundation::CloseHandle(handle);
    if !ok {
        return TRUE;
    }

    let path = String::from_utf16_lossy(&buf[..len as usize]).to_lowercase();
    let matches_proc = acc
        .processes
        .iter()
        .any(|p| path.ends_with(&p.to_lowercase()));
    if !matches_proc {
        return TRUE;
    }

    if !acc.title_hints.is_empty() {
        let mut tbuf = [0u16; 512];
        let n = GetWindowTextW(hwnd, &mut tbuf);
        let title = String::from_utf16_lossy(&tbuf[..n as usize]).to_lowercase();
        if !acc
            .title_hints
            .iter()
            .any(|h| title.contains(&h.to_lowercase()))
        {
            return TRUE;
        }
    }

    acc.found = Some(hwnd);
    TRUE
}

impl Platform for WindowsPlatform {
    fn active_window(&self) -> Result<Option<ActiveWindow>> {
        unsafe {
            let hwnd = windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow();
            if hwnd.0.is_null() {
                return Ok(None);
            }
            let mut tbuf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut tbuf);
            let title = String::from_utf16_lossy(&tbuf[..n as usize]);

            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            let mut process = String::new();
            if let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut buf = [0u16; 512];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(
                    h, PROCESS_NAME_FORMAT(0),
                    windows::core::PWSTR(buf.as_mut_ptr()), &mut len,
                ).is_ok() {
                    let full = String::from_utf16_lossy(&buf[..len as usize]);
                    process = full.rsplit('\\').next().unwrap_or(&full).to_string();
                }
                let _ = windows::Win32::Foundation::CloseHandle(h);
            }
            Ok(Some(ActiveWindow { process, title }))
        }
    }

    fn active_window_id(&self) -> Result<Option<WindowId>> {
        let hwnd = unsafe { windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow() };
        Ok((!hwnd.0.is_null()).then(|| WindowId(hwnd.0 as u64)))
    }

    /// Type text into whatever has focus, as characters rather than keys,
    /// so any language and symbol arrives as written. A line break is
    /// Shift+Enter: plain Enter sends the message in most chat apps, and a
    /// reply must never go out half-typed.
    fn type_text(&self, text: &str) -> Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        // Grouped: a Shift+Enter is one group and is never split across two
        // `SendInput` calls, or a batch boundary could leave Shift held and
        // turn the next line into capitals or, worse, send the message.
        let mut groups: Vec<Vec<INPUT>> = Vec::new();
        let key = |vk: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } },
        };
        for line_or_break in text.replace("\r\n", "\n").split_inclusive('\n') {
            let (line, brk) = match line_or_break.strip_suffix('\n') {
                Some(l) => (l, true),
                None => (line_or_break, false),
            };
            for unit in line.encode_utf16() {
                let mut pair = Vec::with_capacity(2);
                for up in [false, true] {
                    pair.push(INPUT {
                        r#type: INPUT_KEYBOARD,
                        Anonymous: INPUT_0 { ki: KEYBDINPUT {
                            wVk: VIRTUAL_KEY(0),
                            wScan: unit,
                            dwFlags: if up { KEYEVENTF_UNICODE | KEYEVENTF_KEYUP } else { KEYEVENTF_UNICODE },
                            time: 0,
                            dwExtraInfo: 0,
                        } },
                    });
                }
                groups.push(pair);
            }
            if brk {
                groups.push(vec![key(VK_SHIFT, false), key(VK_RETURN, false), key(VK_RETURN, true), key(VK_SHIFT, true)]);
            }
        }
        send_groups(&groups)
    }

    /// A combo like "enter", "ctrl+a" or "ctrl+shift+v".
    fn press(&self, combo: &str) -> Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        let keys = combo
            .split('+')
            .map(|k| vk_for(k.trim()).ok_or_else(|| AtlasError::Platform(format!("I don't know the key \"{k}\""))))
            .collect::<Result<Vec<VIRTUAL_KEY>>>()?;
        let mk = |vk: VIRTUAL_KEY, up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: vk, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } },
        };
        let mut inputs: Vec<INPUT> = keys.iter().map(|k| mk(*k, false)).collect();
        inputs.extend(keys.iter().rev().map(|k| mk(*k, true)));
        send_groups(&[inputs])
    }

    /// The backspaces as one `SendInput` burst -- Windows never puts your own
    /// keys in the middle of one call, and a run of the same key can't come
    /// out garbled the way a burst of different characters does in Notepad
    /// (`send_groups_now`) -- then the text, typed as usual (30 Sep 2026,
    /// `astype`: a backspace at a time, 25 ms apart, left room for your next
    /// key to land in the middle of Atlas's fix).
    fn replace_typed(&self, delete: usize, text: &str) -> Result<()> {
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        if delete > 0 {
            let key = |up: bool| INPUT {
                r#type: INPUT_KEYBOARD,
                Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VK_BACK, wScan: 0, dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) }, time: 0, dwExtraInfo: 0 } },
            };
            let burst: Vec<INPUT> = (0..delete).flat_map(|_| [key(false), key(true)]).collect();
            send_groups(&[burst])?;
        }
        self.type_text(text)
    }

    /// A window's accessibility tree, through UI Automation: what a screen
    /// reader hears. Capped in depth and size so a huge page can't stall
    /// Atlas.
    fn read_window(&self, win: WindowId) -> Result<Option<crate::uia::Node>> {
        unsafe {
            let ua = automation()?;
            let root = ua
                .ElementFromHandle(HWND(win.0 as *mut std::ffi::c_void))
                .map_err(|e| AtlasError::Platform(format!("couldn't read that window: {e}")))?;
            let walker = ua.ControlViewWalker().map_err(|e| AtlasError::Platform(format!("UI Automation: {e}")))?;
            let mut budget = 3000usize;
            let started = std::time::Instant::now();
            Ok(Some(uia_node(&walker, &root, 0, &mut budget, started)))
        }
    }

    /// Is the keyboard's focus in something you can type into? Checked
    /// before typing a reply, so it never lands as keyboard shortcuts in a
    /// window whose text box isn't selected.
    fn press_named(&self, win: WindowId, name: &str) -> Result<bool> {
        use windows::Win32::UI::Accessibility::{
            IUIAutomationInvokePattern, PropertyConditionFlags_IgnoreCase, TreeScope_Descendants, UIA_InvokePatternId,
            UIA_NamePropertyId,
        };
        unsafe {
            let ua = automation()?;
            let root = ua
                .ElementFromHandle(HWND(win.0 as *mut std::ffi::c_void))
                .map_err(|e| AtlasError::Platform(format!("couldn't read that window: {e}")))?;
            let cond = ua
                .CreatePropertyConditionEx(UIA_NamePropertyId, &windows::core::VARIANT::from(name), PropertyConditionFlags_IgnoreCase)
                .map_err(|e| AtlasError::Platform(format!("UI Automation: {e}")))?;
            let Ok(el) = root.FindFirst(TreeScope_Descendants, &cond) else { return Ok(false) };
            if !el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(false) {
                return Ok(false);
            }
            match el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
                Ok(p) => Ok(p.Invoke().is_ok()),
                Err(_) => Ok(false),
            }
        }
    }

    /// A control found by its place in the tree (`read_window`'s order:
    /// the control view, children first to last) and acted on through the
    /// pattern that does what's asked. `Ok(false)` when it has no such
    /// pattern, so the caller can click it instead.
    fn act_on(&self, win: WindowId, path: &[usize], act: &crate::uia::UiAct) -> Result<bool> {
        use crate::uia::UiAct;
        use windows::Win32::UI::Accessibility::{
            IUIAutomationExpandCollapsePattern, IUIAutomationInvokePattern, IUIAutomationSelectionItemPattern,
            IUIAutomationTogglePattern, IUIAutomationValuePattern, UIA_ExpandCollapsePatternId, UIA_InvokePatternId,
            UIA_SelectionItemPatternId, UIA_TogglePatternId, UIA_ValuePatternId,
        };
        unsafe {
            let ua = automation()?;
            let mut el = ua
                .ElementFromHandle(HWND(win.0 as *mut std::ffi::c_void))
                .map_err(|e| AtlasError::Platform(format!("couldn't read that window: {e}")))?;
            let walker = ua.ControlViewWalker().map_err(|e| AtlasError::Platform(format!("UI Automation: {e}")))?;
            for &i in path {
                let mut child = walker
                    .GetFirstChildElement(&el)
                    .map_err(|_| AtlasError::Platform("that control isn't there any more".into()))?;
                for _ in 0..i {
                    child = walker
                        .GetNextSiblingElement(&child)
                        .map_err(|_| AtlasError::Platform("that control isn't there any more".into()))?;
                }
                el = child;
            }
            if !el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(false) {
                return Err(AtlasError::Platform("that control is greyed out".into()));
            }
            let done = match act {
                UiAct::Invoke => match el.GetCurrentPatternAs::<IUIAutomationInvokePattern>(UIA_InvokePatternId) {
                    Ok(p) => p.Invoke().is_ok(),
                    Err(_) => false,
                },
                UiAct::SetValue(v) => match el.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId) {
                    Ok(p) => p.SetValue(&windows::core::BSTR::from(v.as_str())).is_ok(),
                    Err(_) => false,
                },
                UiAct::Toggle => match el.GetCurrentPatternAs::<IUIAutomationTogglePattern>(UIA_TogglePatternId) {
                    Ok(p) => p.Toggle().is_ok(),
                    Err(_) => false,
                },
                UiAct::Select => match el.GetCurrentPatternAs::<IUIAutomationSelectionItemPattern>(UIA_SelectionItemPatternId) {
                    Ok(p) => p.Select().is_ok(),
                    Err(_) => false,
                },
                UiAct::Expand => match el.GetCurrentPatternAs::<IUIAutomationExpandCollapsePattern>(UIA_ExpandCollapsePatternId) {
                    Ok(p) => p.Expand().is_ok(),
                    Err(_) => false,
                },
                UiAct::Focus => el.SetFocus().is_ok(),
            };
            Ok(done)
        }
    }

    fn focused_text(&self) -> Result<Option<String>> {
        use windows::Win32::UI::Accessibility::{IUIAutomationTextPattern, UIA_TextPatternId, UIA_ValueValuePropertyId};
        unsafe {
            let ua = automation()?;
            let Ok(el) = ua.GetFocusedElement() else { return Ok(None) };
            // A password box is never read, whatever else it is.
            if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(true) {
                return Ok(None);
            }
            let value = el
                .GetCurrentPropertyValue(UIA_ValueValuePropertyId)
                .ok()
                .and_then(|v| windows::core::BSTR::try_from(&v).ok())
                .map(|b| b.to_string())
                .unwrap_or_default();
            if !value.is_empty() {
                return Ok(Some(value));
            }
            // Rich boxes (mail bodies, Word) expose their text as a document.
            if let Ok(p) = el.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId) {
                if let Ok(r) = p.DocumentRange() {
                    if let Ok(t) = r.GetText(20_000) {
                        return Ok(Some(t.to_string()));
                    }
                }
            }
            Ok(Some(value))
        }
    }

    fn focused_is_editable(&self) -> Result<Option<bool>> {
        use windows::Win32::UI::Accessibility::{UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_ValueIsReadOnlyPropertyId};
        unsafe {
            let ua = automation()?;
            let Ok(el) = ua.GetFocusedElement() else { return Ok(None) };
            let kind = el.CurrentControlType().ok();
            let read_only = el
                .GetCurrentPropertyValue(UIA_ValueIsReadOnlyPropertyId)
                .ok()
                .and_then(|v| bool::try_from(&v).ok());
            let typeable = matches!(kind, Some(k) if k == UIA_EditControlTypeId || k == UIA_DocumentControlTypeId);
            Ok(Some(typeable && read_only != Some(true)))
        }
    }

    /// Windows keeps the moment of the last keyboard or mouse input, for
    /// the whole session — Atlas's own typing included, which is left out
    /// here (`platform::idle_of_yours`).
    fn session_locked(&self) -> Option<bool> {
        // The input desktop can't be opened for switching while the lock
        // screen owns it: the standard way to tell a locked session.
        use windows::Win32::System::StationsAndDesktops::{CloseDesktop, OpenInputDesktop, DESKTOP_ACCESS_FLAGS, DESKTOP_CONTROL_FLAGS};
        const DESKTOP_SWITCHDESKTOP: u32 = 0x0100;
        unsafe {
            match OpenInputDesktop(DESKTOP_CONTROL_FLAGS(0), false, DESKTOP_ACCESS_FLAGS(DESKTOP_SWITCHDESKTOP)) {
                Ok(h) => {
                    let _ = CloseDesktop(h);
                    Some(false)
                }
                Err(_) => Some(true),
            }
        }
    }

    fn input_idle_secs(&self) -> Option<u64> {
        let last = last_input_tick()?;
        let now = unsafe { windows::Win32::System::SystemInformation::GetTickCount() };
        let own = OWN_INPUT.lock().ok().and_then(|o| *o);
        Some(crate::platform::idle::idle_of_yours(last, own, now))
    }

    fn quiet_state(&self) -> Option<super::OsQuiet> {
        use super::OsQuiet;
        use windows::Win32::UI::Shell::SHQueryUserNotificationState;
        let state = unsafe { SHQueryUserNotificationState() }.ok()?;
        Some(match state.0 {
            1 => OsQuiet::Away,
            2 => OsQuiet::FullScreen,
            3 => OsQuiet::Game,
            4 => OsQuiet::Presenting,
            6 => OsQuiet::QuietTime,
            7 => OsQuiet::StoreApp,
            _ => OsQuiet::Accepts,
        })
    }

    fn built_in_screen_on(&self) -> Option<bool> {
        use windows::Win32::Devices::Display::{
            GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED, DISPLAYCONFIG_PATH_INFO, QDC_ALL_PATHS,
        };
        unsafe {
            let (mut np, mut nm) = (0u32, 0u32);
            if GetDisplayConfigBufferSizes(QDC_ALL_PATHS, &mut np, &mut nm).is_err() {
                return None;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
            if QueryDisplayConfig(QDC_ALL_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None).is_err() {
                return None;
            }
            paths.truncate(np as usize);
            let outputs: Vec<(bool, bool)> = paths
                .iter()
                // Every path, available or not: with the lid shut Windows may
                // list the built-in panel as unavailable, and that is still a
                // laptop whose screen is off, not a desktop with none.
                .map(|p| {
                    let t = p.targetInfo.outputTechnology;
                    let internal = t == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                        || t == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
                        || t == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED;
                    // DISPLAYCONFIG_PATH_ACTIVE
                    (internal, p.flags & 1 != 0)
                })
                .collect();
            crate::layout::built_in_screen_from(&outputs)
        }
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        // Work area, not full bounds -- excludes the taskbar.
        Ok(all_monitors()
            .into_iter()
            .map(|m| Monitor {
                id: m.id,
                x: m.work.left,
                y: m.work.top,
                width: m.work.right - m.work.left,
                height: m.work.bottom - m.work.top,
                primary: m.primary,
            })
            .collect())
    }

    fn monitor_bounds(&self, monitor: u32) -> Option<PixelRect> {
        all_monitors().into_iter().find(|m| m.id == monitor).map(|m| PixelRect {
            x: m.full.left,
            y: m.full.top,
            width: m.full.right - m.full.left,
            height: m.full.bottom - m.full.top,
        })
    }

    fn grab_screen(&self, monitor: u32) -> Result<Option<super::Grab>> {
        let Some(m) = all_monitors().into_iter().find(|m| m.id == monitor) else { return Ok(None) };
        Ok(unsafe { grab_rect(m.full) }.map(|(width, height, rgb)| super::Grab { width, height, rgb, title: String::new() }))
    }

    fn active_monitor(&self) -> Option<u32> {
        use windows::Win32::Graphics::Gdi::{MonitorFromWindow, MONITOR_DEFAULTTONEAREST};
        use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
        become_dpi_aware();
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return None;
            }
            let h = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            (!h.0.is_null()).then(|| h.0 as usize as u32)
        }
    }

    /// The monitor showing the laptop's own panel: the display path whose
    /// output is internal, matched to a monitor by the name the display
    /// settings give its source ("\\.\DISPLAY1").
    fn built_in_monitor(&self) -> Option<u32> {
        use windows::Win32::Devices::Display::{
            DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, QueryDisplayConfig, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
            DISPLAYCONFIG_DEVICE_INFO_HEADER, DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED,
            DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED, DISPLAYCONFIG_PATH_INFO,
            DISPLAYCONFIG_SOURCE_DEVICE_NAME, QDC_ONLY_ACTIVE_PATHS,
        };
        let names: Vec<String> = unsafe {
            let (mut np, mut nm) = (0u32, 0u32);
            if GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut np, &mut nm).is_err() {
                return None;
            }
            let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); np as usize];
            let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); nm as usize];
            if QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), None).is_err() {
                return None;
            }
            paths.truncate(np as usize);
            paths
                .iter()
                .filter(|p| {
                    let t = p.targetInfo.outputTechnology;
                    t == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL
                        || t == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED
                        || t == DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED
                })
                .filter_map(|p| {
                    let mut src = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
                    src.header = DISPLAYCONFIG_DEVICE_INFO_HEADER {
                        r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                        size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                        adapterId: p.sourceInfo.adapterId,
                        id: p.sourceInfo.id,
                    };
                    if DisplayConfigGetDeviceInfo(&mut src.header) != 0 {
                        return None;
                    }
                    let end = src.viewGdiDeviceName.iter().position(|c| *c == 0).unwrap_or(32);
                    Some(String::from_utf16_lossy(&src.viewGdiDeviceName[..end]))
                })
                .collect()
        };
        super::builtin_among(&all_monitors().iter().map(|m| (m.id, m.device.clone())).collect::<Vec<_>>(), &names)
    }

    fn launch(&self, spec: &AppSpec) -> Result<()> {
        let expanded = expand_env(&spec.launch);
        if spec.store {
            // A packaged app is started through the shell by app id.
            // Launching its exe from WindowsApps directly is blocked.
            Command::new("explorer.exe")
                .arg(format!("shell:AppsFolder\\{expanded}"))
                .spawn()
                .map_err(|e| AtlasError::Platform(format!("launch store app {expanded}: {e}")))?;
            return Ok(());
        }
        Command::new(&expanded)
            .args(&spec.args)
            .spawn()
            .map_err(|e| AtlasError::Platform(format!("launch {}: {}", expanded, e)))?;
        Ok(())
    }

    fn find_window(&self, spec: &AppSpec) -> Result<Option<WindowId>> {
        let mut acc = WinAcc {
            processes: spec.process_names.clone(),
            title_hints: spec.title_hints.clone(),
            found: None,
        };
        unsafe {
            let _ = EnumWindows(Some(win_cb), LPARAM(&mut acc as *mut WinAcc as isize));
        }
        Ok(acc.found.map(|h| WindowId(h.0 as u64)))
    }

    fn place(&self, win: WindowId, r: PixelRect) -> Result<()> {
        let hwnd = HWND(win.0 as *mut std::ffi::c_void);
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            SetWindowPos(hwnd, HWND_TOP, r.x, r.y, r.width, r.height, SWP_NOZORDER)
                .map_err(|e| AtlasError::Platform(format!("SetWindowPos: {e}")))?;
        }
        Ok(())
    }

    fn focus(&self, win: WindowId) -> Result<()> {
        use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
        use windows::Win32::UI::WindowsAndMessaging::{BringWindowToTop, GetForegroundWindow};
        let hwnd = HWND(win.0 as *mut std::ffi::c_void);
        unsafe {
            let _ = ShowWindow(hwnd, SW_RESTORE);
            if SetForegroundWindow(hwnd).as_bool() && GetForegroundWindow() == hwnd {
                return Ok(());
            }
            // Windows' foreground lock: only the program you last used may
            // change what's in front, so Atlas working in the background was
            // refused ("Windows wouldn't bring it to the front" on Eric's
            // laptop, 25 Sep 2026, with a PowerShell window in front). For the
            // moment of the switch Atlas joins the input of the window that
            // is in front — the documented way for a helper acting for you —
            // and lets go straight after. No key is pressed to get round it:
            // an Alt tap would land in *your* window and open its menu.
            let fg = GetForegroundWindow();
            let fg_thread = GetWindowThreadProcessId(fg, None);
            let me = GetCurrentThreadId();
            let joined = fg_thread != 0 && fg_thread != me && AttachThreadInput(me, fg_thread, TRUE).as_bool();
            let _ = BringWindowToTop(hwnd);
            let _ = SetForegroundWindow(hwnd);
            if joined {
                let _ = AttachThreadInput(me, fg_thread, BOOL(0));
            }
        }
        Ok(())
    }

    fn close(&self, spec: &AppSpec) -> Result<()> {
        for p in &spec.process_names {
            // /F is a hard kill. See docs/DECISIONS.md — graceful close is
            // WM_CLOSE per-window and is deliberately not implemented yet.
            // Through `tools::command` (no console window flashing up from the
            // windowless background Atlas; 28 Sep 2026).
            let _ = crate::tools::command("taskkill").args(["/IM", p, "/F"]).output();
        }
        Ok(())
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }

    fn open_path(&self, path: &str) -> Result<()> {
        // explorer.exe opens a file with its registered app and a .lnk as the
        // shortcut it is. The path is one argument, never a shell string.
        // A web address opens in your browser the same way.
        let web = path.starts_with("https://") || path.starts_with("http://");
        if !web && !std::path::Path::new(path).exists() {
            return Err(AtlasError::Platform(format!("{path} isn't there any more")));
        }
        let child = Command::new("explorer.exe")
            .arg(path)
            .spawn()
            .map_err(|e| AtlasError::Platform(format!("couldn't open {path}: {e}")))?;
        crate::unwaited::dont_wait(child);
        Ok(())
    }

    fn clipboard_change(&self) -> Option<u32> {
        use windows::Win32::System::DataExchange::GetClipboardSequenceNumber;
        // 0 means the process can't see the clipboard (another desktop).
        Some(unsafe { GetClipboardSequenceNumber() }).filter(|n| *n != 0)
    }

    fn clipboard_copy(&self) -> Option<super::ClipCopy> {
        use windows::core::w;
        use windows::Win32::Foundation::HGLOBAL;
        use windows::Win32::System::DataExchange::{
            CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW,
        };
        use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
        const CF_UNICODETEXT: u32 = 13;
        unsafe {
            // The formats a password manager sets to say "don't keep this".
            // Checked before any text is read, so a private copy never
            // enters this process's memory at all.
            let exclude = RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing"));
            let viewer_ignore = RegisterClipboardFormatW(w!("Clipboard Viewer Ignore"));
            let history = RegisterClipboardFormatW(w!("CanIncludeInClipboardHistory"));
            if OpenClipboard(None).is_err() {
                return None;
            }
            let mut private = (exclude != 0 && IsClipboardFormatAvailable(exclude).is_ok())
                || (viewer_ignore != 0 && IsClipboardFormatAvailable(viewer_ignore).is_ok());
            if !private && history != 0 && IsClipboardFormatAvailable(history).is_ok() {
                // A DWORD: 0 means "not in history".
                if let Ok(h) = GetClipboardData(history) {
                    let g = HGLOBAL(h.0);
                    let p = GlobalLock(g) as *const u32;
                    if !p.is_null() {
                        private = *p == 0;
                        let _ = GlobalUnlock(g);
                    }
                }
            }
            let out = if private {
                super::ClipCopy::Private
            } else if IsClipboardFormatAvailable(CF_UNICODETEXT).is_err() {
                super::ClipCopy::NotText
            } else {
                match GetClipboardData(CF_UNICODETEXT) {
                    Ok(h) => {
                        let g = HGLOBAL(h.0);
                        let p = GlobalLock(g) as *const u16;
                        if p.is_null() {
                            super::ClipCopy::NotText
                        } else {
                            // Bounded by the block's own size, then by the
                            // terminating NUL -- never read past either.
                            let max = GlobalSize(g) / 2;
                            let slice = std::slice::from_raw_parts(p, max);
                            let end = slice.iter().position(|c| *c == 0).unwrap_or(max);
                            let text = String::from_utf16_lossy(&slice[..end]);
                            let _ = GlobalUnlock(g);
                            super::ClipCopy::Text(text)
                        }
                    }
                    Err(_) => super::ClipCopy::NotText,
                }
            };
            let _ = CloseClipboard();
            Some(out)
        }
    }

    fn grab_window(&self) -> Result<Option<super::Grab>> {
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowRect};
        become_dpi_aware();
        unsafe {
            let hwnd = GetForegroundWindow();
            if hwnd.0.is_null() {
                return Ok(None);
            }
            let mut r = RECT::default();
            if GetWindowRect(hwnd, &mut r).is_err() {
                return Ok(None);
            }
            let mut tbuf = [0u16; 512];
            let n = GetWindowTextW(hwnd, &mut tbuf);
            let title = String::from_utf16_lossy(&tbuf[..n as usize]);
            // From the screen, over the window's rectangle: what you see,
            // which is what you meant.
            Ok(grab_rect(r).map(|(width, height, rgb)| super::Grab { width, height, rgb, title }))
        }
    }

    fn recognise_text(&self, grab: &super::Grab) -> Result<Option<String>> {
        use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::Streams::DataWriter;
        // Windows' own recogniser, on this machine: it needs the OCR
        // language pack for your display language, which Windows installs
        // with the language. None when it can't be made.
        let run = || -> windows::core::Result<String> {
            let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
            let bgra: Vec<u8> = grab.rgb.chunks_exact(3).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
            let writer = DataWriter::new()?;
            writer.WriteBytes(&bgra)?;
            let buffer = writer.DetachBuffer()?;
            let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, grab.width as i32, grab.height as i32)?;
            let result = engine.RecognizeAsync(&bitmap)?.get()?;
            let mut lines = Vec::new();
            for line in result.Lines()? {
                lines.push(line.Text()?.to_string());
            }
            Ok(lines.join("\n"))
        };
        match run() {
            Ok(text) => Ok(Some(text)),
            Err(e) => Err(AtlasError::Platform(format!("Windows' text recognition didn't run: {e}"))),
        }
    }

    /// Windows' own recogniser, line by line with where each line is.
    fn recognise_lines(&self, grab: &super::Grab) -> Result<Vec<(String, super::PixelRect)>> {
        use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::Streams::DataWriter;
        let run = || -> windows::core::Result<Vec<(String, super::PixelRect)>> {
            let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
            let bgra: Vec<u8> = grab.rgb.chunks_exact(3).flat_map(|p| [p[2], p[1], p[0], 255]).collect();
            let writer = DataWriter::new()?;
            writer.WriteBytes(&bgra)?;
            let buffer = writer.DetachBuffer()?;
            let bitmap = SoftwareBitmap::CreateCopyFromBuffer(&buffer, BitmapPixelFormat::Bgra8, grab.width as i32, grab.height as i32)?;
            let result = engine.RecognizeAsync(&bitmap)?.get()?;
            let mut out = Vec::new();
            for line in result.Lines()? {
                let text = line.Text()?.to_string();
                let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
                for w in line.Words()? {
                    let r = w.BoundingRect()?;
                    x0 = x0.min(r.X);
                    y0 = y0.min(r.Y);
                    x1 = x1.max(r.X + r.Width);
                    y1 = y1.max(r.Y + r.Height);
                }
                if x1 > x0 && y1 > y0 {
                    out.push((text, super::PixelRect { x: x0 as i32, y: y0 as i32, width: (x1 - x0) as i32, height: (y1 - y0) as i32 }));
                }
            }
            Ok(out)
        };
        run().map_err(|e| AtlasError::Platform(format!("Windows' text recognition didn't run: {e}")))
    }

    fn recognise_image_file(&self, path: &str) -> Result<Option<String>> {
        use windows::Graphics::Imaging::{
            BitmapAlphaMode, BitmapDecoder, BitmapInterpolationMode, BitmapPixelFormat, BitmapTransform,
            ColorManagementMode, ExifOrientationMode,
        };
        use windows::Media::Ocr::OcrEngine;
        use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};
        // A photo handed over, read by the same on-device recognizer as the
        // screen (28 Sep 2026): Windows decodes it (JPEG, PNG, HEIC where the
        // codec is installed), turns it by its EXIF orientation, shrinks it
        // under the engine's size limit, and reads it.
        let bytes = std::fs::read(path).map_err(|e| AtlasError::Platform(format!("couldn't open the picture: {e}")))?;
        let run = || -> windows::core::Result<String> {
            let engine = OcrEngine::TryCreateFromUserProfileLanguages()?;
            let stream = InMemoryRandomAccessStream::new()?;
            let writer = DataWriter::CreateDataWriter(&stream)?;
            writer.WriteBytes(&bytes)?;
            writer.StoreAsync()?.get()?;
            writer.FlushAsync()?.get()?;
            writer.DetachStream()?;
            stream.Seek(0)?;
            let decoder = BitmapDecoder::CreateAsync(&stream)?.get()?;
            let (w, h) = (decoder.OrientedPixelWidth()?, decoder.OrientedPixelHeight()?);
            let most = OcrEngine::MaxImageDimension()?;
            let transform = BitmapTransform::new()?;
            if w > most || h > most {
                let scale = most as f64 / w.max(h) as f64;
                transform.SetScaledWidth(((decoder.PixelWidth()? as f64) * scale) as u32)?;
                transform.SetScaledHeight(((decoder.PixelHeight()? as f64) * scale) as u32)?;
                transform.SetInterpolationMode(BitmapInterpolationMode::Fant)?;
            }
            let bitmap = decoder
                .GetSoftwareBitmapTransformedAsync(
                    BitmapPixelFormat::Bgra8,
                    BitmapAlphaMode::Premultiplied,
                    &transform,
                    ExifOrientationMode::RespectExifOrientation,
                    ColorManagementMode::DoNotColorManage,
                )?
                .get()?;
            let result = engine.RecognizeAsync(&bitmap)?.get()?;
            let mut lines = Vec::new();
            for line in result.Lines()? {
                lines.push(line.Text()?.to_string());
            }
            Ok(lines.join("\n"))
        };
        match run() {
            Ok(text) => Ok(Some(text)),
            Err(e) => Err(AtlasError::Platform(format!("Windows' text recognition didn't read it: {e}"))),
        }
    }

    // `type_text` and `press` are above. Round 11 had its own pair here (for
    // snippets: 64 keys a batch, a newline as Enter); the third chat's, kept,
    // types one character at a time 25 ms apart (Notepad garbled anything
    // faster, measured on Eric's laptop), makes a newline Shift+Enter so a
    // chat reply is never sent half-typed, and leaves Atlas's own typing out
    // of your idle time. Merged 26 Sep 2026.

    fn read_clipboard(&self) -> Result<Option<String>> {
        // Windows ships Get-Clipboard; no crate needed. PowerShell appends a
        // trailing newline, which we trim so a copied one-liner reads as one
        // line. Not compiled here (see the file header) — the shape matches the
        // POSIX side and the tested mock.
        let out = crate::tools::command("powershell")
            .args(["-NoProfile", "-Command", "Get-Clipboard -Raw"])
            .output()
            .map_err(|e| AtlasError::Platform(format!("couldn't read the clipboard: {e}")))?;
        if !out.status.success() {
            return Ok(None);
        }
        let text = String::from_utf8_lossy(&out.stdout);
        let text = text.strip_suffix("\r\n").or_else(|| text.strip_suffix('\n')).unwrap_or(&text);
        Ok(Some(text.to_string()))
    }

    fn write_clipboard(&self, text: &str) -> Result<()> {
        use std::io::Write;
        // clip.exe takes stdin and puts it on the clipboard — the OS's own
        // tool, present on every Windows.
        let mut child = crate::tools::command("clip")
            .stdin(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| AtlasError::Platform(format!("couldn't reach clip.exe: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| AtlasError::Platform(format!("couldn't write to the clipboard: {e}")))?;
        }
        match child.wait() {
            Ok(s) if s.success() => Ok(()),
            Ok(_) => Err(AtlasError::Platform("clip.exe refused the text".into())),
            Err(e) => Err(AtlasError::Platform(format!("clip.exe didn't finish: {e}"))),
        }
    }
}

fn expand_env(s: &str) -> String {
    let mut out = s.to_string();
    for (k, v) in std::env::vars() {
        out = out.replace(&format!("%{}%", k), &v);
    }
    out
}


/// Random bytes from the system RNG.
///
/// `BCryptGenRandom` with `BCRYPT_USE_SYSTEM_PREFERRED_RNG`, which is the
/// documented way to get cryptographic randomness on Windows without opening
/// an algorithm handle first.
pub fn random_bytes(n: usize) -> crate::error::Result<Vec<u8>> {
    use crate::error::AtlasError;

    #[link(name = "bcrypt")]
    unsafe extern "system" {
        fn BCryptGenRandom(
            h_algorithm: *mut core::ffi::c_void,
            pb_buffer: *mut u8,
            cb_buffer: u32,
            dw_flags: u32,
        ) -> i32;
    }
    const USE_SYSTEM_PREFERRED_RNG: u32 = 0x0000_0002;

    let mut buf = vec![0u8; n];
    let status = unsafe {
        BCryptGenRandom(
            core::ptr::null_mut(),
            buf.as_mut_ptr(),
            buf.len() as u32,
            USE_SYSTEM_PREFERRED_RNG,
        )
    };
    if status != 0 {
        return Err(AtlasError::Platform(format!(
            "BCryptGenRandom failed with 0x{status:08x}"
        )));
    }
    Ok(buf)
}


/// Encrypt with Windows' own data protection.
///
/// `CryptProtectData` ties the ciphertext to the current Windows user account.
/// It is the same mechanism the credential manager and every browser on the
/// machine uses for local secrets, and it replaces the stand-in cipher that
/// was here — which was XOR against a key stretched by an affine map, i.e. not
/// encryption.
///
/// What this gives you: a copied vault file is useless on another machine or
/// under another Windows account. What it does not give you: protection from
/// something already running as you. Nothing local can, and pretending
/// otherwise is the failure the old code made.
///
/// The extra entropy is mixed in so that the vault passphrase still matters —
/// otherwise anything running as your user could unprotect the file without
/// knowing it.
pub fn protect(plain: &[u8], extra: &[u8]) -> crate::error::Result<Vec<u8>> {
    dpapi(plain, extra, true)
}

pub fn unprotect(sealed: &[u8], extra: &[u8]) -> crate::error::Result<Vec<u8>> {
    dpapi(sealed, extra, false)
}

#[repr(C)]
struct DataBlob {
    cb_data: u32,
    pb_data: *mut u8,
}

fn dpapi(input: &[u8], extra: &[u8], encrypting: bool) -> crate::error::Result<Vec<u8>> {
    use crate::error::AtlasError;

    #[link(name = "crypt32")]
    unsafe extern "system" {
        fn CryptProtectData(
            p_data_in: *mut DataBlob,
            sz_description: *const u16,
            p_optional_entropy: *mut DataBlob,
            p_reserved: *mut core::ffi::c_void,
            p_prompt_struct: *mut core::ffi::c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
        fn CryptUnprotectData(
            p_data_in: *mut DataBlob,
            pps_description: *mut *mut u16,
            p_optional_entropy: *mut DataBlob,
            p_reserved: *mut core::ffi::c_void,
            p_prompt_struct: *mut core::ffi::c_void,
            dw_flags: u32,
            p_data_out: *mut DataBlob,
        ) -> i32;
    }
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn LocalFree(h: *mut core::ffi::c_void) -> *mut core::ffi::c_void;
    }

    // Never write the plaintext to the swap file while we hold it.
    const CRYPTPROTECT_UI_FORBIDDEN: u32 = 0x1;

    let mut inp = DataBlob { cb_data: input.len() as u32, pb_data: input.as_ptr() as *mut u8 };
    let mut ent = DataBlob { cb_data: extra.len() as u32, pb_data: extra.as_ptr() as *mut u8 };
    let mut out = DataBlob { cb_data: 0, pb_data: core::ptr::null_mut() };

    let ok = unsafe {
        if encrypting {
            CryptProtectData(
                &mut inp,
                core::ptr::null(),
                &mut ent,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        } else {
            CryptUnprotectData(
                &mut inp,
                core::ptr::null_mut(),
                &mut ent,
                core::ptr::null_mut(),
                core::ptr::null_mut(),
                CRYPTPROTECT_UI_FORBIDDEN,
                &mut out,
            )
        }
    };
    if ok == 0 {
        return Err(AtlasError::Platform(
            if encrypting {
                "Windows refused to encrypt that".into()
            } else {
                // The honest message. A wrong passphrase and a file from
                // another machine fail identically here, and saying which
                // would tell an attacker something.
                "couldn't open it — wrong passphrase, or this vault came from another machine \
                 or another Windows account"
                    .to_string()
            },
        ));
    }
    let result = unsafe { std::slice::from_raw_parts(out.pb_data, out.cb_data as usize).to_vec() };
    unsafe { LocalFree(out.pb_data as *mut core::ffi::c_void) };
    Ok(result)
}

/// Send keystrokes, and say so if Windows took fewer than were sent — which
/// is what happens when the window in front belongs to a program running as
/// administrator and Atlas isn't.
/// Atlas's own typing, so "how long since you last typed" can leave it out
/// (`platform::idle_of_yours`).
static OWN_INPUT: std::sync::Mutex<Option<super::OwnInput>> = std::sync::Mutex::new(None);

fn last_input_tick() -> Option<u32> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe { GetLastInputInfo(&mut info).as_bool().then_some(info.dwTime) }
}

fn send_groups(groups: &[Vec<windows::Win32::UI::Input::KeyboardAndMouse::INPUT>]) -> Result<()> {
    let now = || unsafe { windows::Win32::System::SystemInformation::GetTickCount() };
    if let (Some(last), Ok(mut own)) = (last_input_tick(), OWN_INPUT.lock()) {
        *own = Some(crate::platform::idle::own_input_starts(last, *own, now()));
    }
    let sent = send_groups_now(groups);
    if let Ok(mut own) = OWN_INPUT.lock() {
        if let Some(o) = own.as_mut() {
            o.to = now();
        }
    }
    sent
}

fn send_groups_now(groups: &[Vec<windows::Win32::UI::Input::KeyboardAndMouse::INPUT>]) -> Result<()> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT};
    // In batches, since very long texts sent in one call can be dropped,
    // but only ever between whole groups.
    let mut batch: Vec<INPUT> = Vec::new();
    let flush = |batch: &mut Vec<INPUT>| -> Result<()> {
        if batch.is_empty() {
            return Ok(());
        }
        let sent = unsafe { SendInput(batch, std::mem::size_of::<INPUT>() as i32) };
        let all = sent as usize == batch.len();
        batch.clear();
        if all {
            Ok(())
        } else {
            release_modifiers();
            Err(AtlasError::Platform(
                "Windows didn't take all the keystrokes — the window in front may be running as administrator".into(),
            ))
        }
    };
    // One character at a time, with a gap. Measured on Eric's laptop, 25 Sep
    // 2026, in Windows 11 Notepad: sent in one burst, the first few
    // characters landed and every one after came out as the *last* character
    // ("Atlas ................" for "Atlas live typing test … sent."); one
    // per call with no gap did the same; 10 ms between was off by one at the
    // edges; 20 ms and up came out exactly. The app reads each character
    // after the fact, so a burst hands it the newest one over and over.
    // `KEY_GAP_MS` is above the measured edge, and a reply is read back
    // before it's sent (`delegate::type_into_window`), so a machine slower
    // than this one still never sends a garbled line.
    for g in groups {
        batch.extend_from_slice(g);
        flush(&mut batch)?;
        std::thread::sleep(std::time::Duration::from_millis(KEY_GAP_MS));
    }
    Ok(())
}

/// The pause between characters when typing into another app. See
/// `send_groups`.
const KEY_GAP_MS: u64 = 25;

/// Let go of Shift, Ctrl, Alt and Windows, in case a batch that pressed one
/// was only partly taken. A key left held down makes everything you type
/// afterwards wrong.
fn release_modifiers() {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let up: Vec<INPUT> = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LWIN]
        .iter()
        .map(|vk| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: *vk, wScan: 0, dwFlags: KEYEVENTF_KEYUP, time: 0, dwExtraInfo: 0 } },
        })
        .collect();
    unsafe {
        let _ = SendInput(&up, std::mem::size_of::<INPUT>() as i32);
    }
}

/// UI Automation, with a limit on how long one call into another program
/// may take: an app that's hung would otherwise hang Atlas with it.
unsafe fn automation() -> Result<windows::Win32::UI::Accessibility::IUIAutomation> {
    use windows::core::Interface;
    use windows::Win32::System::Com::{CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED};
    use windows::Win32::UI::Accessibility::{CUIAutomation, CUIAutomation8, IUIAutomation, IUIAutomation2};
    let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
    let ua: IUIAutomation = match CoCreateInstance::<_, IUIAutomation>(&CUIAutomation8, None, CLSCTX_INPROC_SERVER) {
        Ok(ua) => ua,
        Err(_) => CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| AtlasError::Platform(format!("UI Automation isn't available: {e}")))?,
    };
    if let Ok(ua2) = ua.cast::<IUIAutomation2>() {
        let _ = ua2.SetConnectionTimeout(2_000);
        let _ = ua2.SetTransactionTimeout(2_000);
    }
    Ok(ua)
}

/// The virtual key for a key's name in a combo.
fn vk_for(name: &str) -> Option<windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY> {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    let n = name.to_lowercase();
    Some(match n.as_str() {
        "ctrl" | "control" => VK_CONTROL,
        "shift" => VK_SHIFT,
        "alt" => VK_MENU,
        "win" | "windows" => VK_LWIN,
        "enter" | "return" => VK_RETURN,
        "tab" => VK_TAB,
        "esc" | "escape" => VK_ESCAPE,
        "backspace" => VK_BACK,
        "delete" | "del" => VK_DELETE,
        "space" => VK_SPACE,
        "home" => VK_HOME,
        "end" => VK_END,
        "up" => VK_UP,
        "down" => VK_DOWN,
        "left" => VK_LEFT,
        "right" => VK_RIGHT,
        "pageup" => VK_PRIOR,
        "pagedown" => VK_NEXT,
        _ if n.len() == 1 => {
            let c = n.chars().next()?.to_ascii_uppercase();
            if c.is_ascii_alphanumeric() { VIRTUAL_KEY(c as u16) } else { return None }
        }
        _ if n.starts_with('f') && n[1..].parse::<u16>().map(|k| (1..=12).contains(&k)).unwrap_or(false) => {
            VIRTUAL_KEY(VK_F1.0 + n[1..].parse::<u16>().ok()? - 1)
        }
        _ => return None,
    })
}

/// One element and its children, as Atlas's own tree.
unsafe fn uia_node(
    walker: &windows::Win32::UI::Accessibility::IUIAutomationTreeWalker,
    el: &windows::Win32::UI::Accessibility::IUIAutomationElement,
    depth: usize,
    budget: &mut usize,
    started: std::time::Instant,
) -> crate::uia::Node {
    use windows::Win32::UI::Accessibility::UIA_ValueValuePropertyId;
    *budget = budget.saturating_sub(1);
    let role = el.CurrentControlType().map(|t| crate::uia::Role::from_control_type(t.0)).unwrap_or(crate::uia::Role::Other);
    let name = el.CurrentName().map(|b| b.to_string()).unwrap_or_default();
    let value = el
        .GetCurrentPropertyValue(UIA_ValueValuePropertyId)
        .ok()
        .and_then(|v| windows::core::BSTR::try_from(&v).ok())
        .map(|b| b.to_string())
        .unwrap_or_default();
    let enabled = el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(true);
    let rect = el
        .CurrentBoundingRectangle()
        .ok()
        .filter(|r| r.right > r.left && r.bottom > r.top)
        .map(|r| [r.left, r.top, r.right - r.left, r.bottom - r.top]);
    let mut children = Vec::new();
    // From the last child backwards: in a chat or a mail thread the newest
    // part is at the end, and when the budget runs out it's the oldest
    // that goes unread, not the message you're being asked to answer.
    if depth < 25 && *budget > 0 {
        let mut next = walker.GetLastChildElement(el).ok();
        while let Some(child) = next {
            if *budget == 0 || started.elapsed() > std::time::Duration::from_secs(8) {
                *budget = 0;
                break;
            }
            children.push(uia_node(walker, &child, depth + 1, budget, started));
            next = walker.GetPreviousSiblingElement(&child).ok();
        }
        children.reverse();
    }
    crate::uia::Node { role, name, value, enabled, children, rect }
}
