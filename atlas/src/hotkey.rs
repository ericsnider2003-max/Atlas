//! The push-to-talk key, heard wherever you are.
//!
//! `input::HoldToTalk` decided what a hold meant and nothing fed it: the
//! push-to-talk tier waited for Enter in Atlas's own console, which is no
//! use when you are in another window. This is the missing key source.
//!
//! - **Windows:** a low-level keyboard hook (`WH_KEYBOARD_LL`) on its own
//!   thread. The chosen key is held back from the app you're in until Atlas
//!   knows whether it's a hold: a hold starts listening and the app never sees
//!   the key; a quick tap is given back to the app (`SendInput`), so Tab still
//!   tabs. Events Atlas injects are marked and ignored on the way back in.
//! - **Linux:** the kernel's keyboard device (`/dev/input/event*`), read, not
//!   grabbed — so the key still reaches the app as well. Reading it needs the
//!   `input` group; `doctor` says so when it can't.
//!
//! The decisions — what to hold back, when a hold becomes talking, when to
//! give a tap back — live in `Gate`, which is plain logic and tested; the
//! platform code only reports key-down and key-up.

use crate::input::{HoldToTalk, KeyEvent};

/// What the platform side should do with one key event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verdict {
    /// Keep this event from the app.
    pub hold_back: bool,
    /// Give the app the tap it didn't get (down + up), because it turned out
    /// not to be a hold.
    pub give_tap_back: bool,
    /// Tell Atlas.
    pub event: Option<KeyEvent>,
}

/// The decisions, without any platform in them.
pub struct Gate {
    hold: HoldToTalk,
    down: bool,
}

impl Gate {
    pub fn new(hold_ms: u64) -> Gate {
        Gate { hold: HoldToTalk::new("", hold_ms), down: false }
    }

    /// The key went down (or auto-repeated while down).
    pub fn down(&mut self, now_ms: u64) -> Verdict {
        let first = !self.down;
        self.down = true;
        let e = if first { self.hold.down(now_ms) } else { self.hold.poll(now_ms) };
        Verdict { hold_back: true, give_tap_back: false, event: (e == KeyEvent::StartTalking).then_some(e) }
    }

    /// Time passed with the key still down: a hold crosses its threshold here
    /// even when the keyboard sends no repeats.
    pub fn tick(&mut self, now_ms: u64) -> Option<KeyEvent> {
        if !self.down {
            return None;
        }
        let e = self.hold.poll(now_ms);
        (e == KeyEvent::StartTalking).then_some(e)
    }

    /// Held, and not yet long enough to talk: the only time the hold timer
    /// has anything to check (audit Q14: it woke 50 times a second, all day).
    pub fn waiting_for_hold(&self) -> bool {
        self.down && !self.hold.is_talking()
    }

    /// The key came up.
    pub fn up(&mut self, now_ms: u64) -> Verdict {
        if !self.down {
            return Verdict { hold_back: false, give_tap_back: false, event: None };
        }
        self.down = false;
        match self.hold.up(now_ms) {
            KeyEvent::StopTalking => Verdict { hold_back: true, give_tap_back: false, event: Some(KeyEvent::StopTalking) },
            _ => Verdict { hold_back: true, give_tap_back: true, event: None },
        }
    }

}

/// The key named in `push_to_talk.key`, as a Windows virtual-key code.
pub fn windows_vk(name: &str) -> Option<u16> {
    let n = name.trim().to_lowercase().replace(['_', '-', ' '], "");
    Some(match n.as_str() {
        "tab" => 0x09,
        "capslock" | "caps" => 0x14,
        "scrolllock" => 0x91,
        "pause" => 0x13,
        "rightctrl" | "rctrl" => 0xA3,
        "rightalt" | "ralt" | "altgr" => 0xA5,
        "rightshift" | "rshift" => 0xA1,
        "insert" => 0x2D,
        "menu" | "apps" => 0x5D,
        "space" => 0x20,
        f if f.starts_with('f') => {
            let k: u16 = f[1..].parse().ok()?;
            if (1..=24).contains(&k) { 0x6F + k } else { return None }
        }
        c if c.len() == 1 && c.chars().all(|x| x.is_ascii_alphanumeric()) => c.to_ascii_uppercase().as_bytes()[0] as u16,
        _ => return None,
    })
}

/// The same key as a Linux input-event code (linux/input-event-codes.h).
pub fn linux_code(name: &str) -> Option<u16> {
    let n = name.trim().to_lowercase().replace(['_', '-', ' '], "");
    Some(match n.as_str() {
        "tab" => 15,
        "capslock" | "caps" => 58,
        "scrolllock" => 70,
        "pause" => 119,
        "rightctrl" | "rctrl" => 97,
        "rightalt" | "ralt" | "altgr" => 100,
        "rightshift" | "rshift" => 54,
        "insert" => 110,
        "menu" | "apps" => 127,
        "space" => 57,
        f if f.starts_with('f') => {
            let k: u16 = f[1..].parse().ok()?;
            match k {
                1..=10 => 58 + k,
                11 => 87,
                12 => 88,
                13..=24 => 170 + k,
                _ => return None,
            }
        }
        _ => return None,
    })
}

/// One `struct input_event` from a Linux keyboard device: 24 bytes on a
/// 64-bit kernel (a 16-byte time, then type, code, value). `Some((code,
/// value))` for key events: value 1 down, 0 up, 2 auto-repeat.
pub fn parse_linux_event(b: &[u8]) -> Option<(u16, i32)> {
    if b.len() < 24 {
        return None;
    }
    let ty = u16::from_le_bytes([b[16], b[17]]);
    let code = u16::from_le_bytes([b[18], b[19]]);
    let value = i32::from_le_bytes([b[20], b[21], b[22], b[23]]);
    (ty == 1).then_some((code, value))
}

#[cfg(any(windows, target_os = "linux"))]
fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The key, being heard.
pub struct Keys {
    rx: std::sync::mpsc::Receiver<KeyEvent>,
}

impl Keys {
    /// Held down past the threshold since last asked? Never blocks.
    pub fn held(&self) -> bool {
        let mut started = false;
        while let Ok(e) = self.rx.try_recv() {
            started |= e == KeyEvent::StartTalking;
        }
        started
    }

    /// Wait up to `ms` for a hold to start.
    pub fn held_within(&self, ms: u64) -> bool {
        let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
        loop {
            let left = until.saturating_duration_since(std::time::Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(KeyEvent::StartTalking) => return true,
                Ok(_) => continue,
                Err(_) => return false,
            }
        }
    }
}

/// Start hearing the key, or say why it can't be heard on this machine.
pub fn spawn(key: &str, hold_ms: u64) -> Result<Keys, String> {
    let (tx, rx) = std::sync::mpsc::channel();
    platform::spawn(key, hold_ms, tx)?;
    Ok(Keys { rx })
}

#[cfg(windows)]
mod platform {
    use super::*;
    use std::sync::mpsc::Sender;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, GetMessageW, SetWindowsHookExW, KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL, WM_KEYDOWN, WM_KEYUP,
        WM_SYSKEYDOWN, WM_SYSKEYUP,
    };

    /// Marks keys Atlas injected, so the hook lets them through untouched.
    const MINE: usize = 0x4154_4C53; // "ATLS"

    struct Shared {
        vk: u16,
        gate: Gate,
        tx: Sender<KeyEvent>,
    }
    static STATE: Mutex<Option<Shared>> = Mutex::new(None);
    /// The hold timer's thread, woken by the hook when the key goes down.
    static TIMER: std::sync::OnceLock<std::thread::Thread> = std::sync::OnceLock::new();

    unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
            if k.dwExtraInfo != MINE {
                let mut give_back = None;
                let mut swallow = false;
                if let Ok(mut g) = STATE.lock().or_else(crate::crash::unpoison) {
                    if let Some(s) = g.as_mut() {
                        if k.vkCode as u16 == s.vk {
                            let m = wparam.0 as u32;
                            let v = if m == WM_KEYDOWN || m == WM_SYSKEYDOWN {
                                let v = s.gate.down(super::now_ms());
                                if let Some(timer) = TIMER.get() {
                                    timer.unpark();
                                }
                                v
                            } else if m == WM_KEYUP || m == WM_SYSKEYUP {
                                s.gate.up(super::now_ms())
                            } else {
                                Verdict { hold_back: false, give_tap_back: false, event: None }
                            };
                            if let Some(e) = v.event {
                                let _ = s.tx.send(e);
                            }
                            swallow = v.hold_back;
                            if v.give_tap_back {
                                give_back = Some(s.vk);
                            }
                        }
                    }
                }
                if let Some(vk) = give_back {
                    tap(vk);
                }
                if swallow {
                    return LRESULT(1);
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    fn tap(vk: u16) {
        let key = |flags: KEYBD_EVENT_FLAGS| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT { wVk: VIRTUAL_KEY(vk), wScan: 0, dwFlags: flags, time: 0, dwExtraInfo: MINE },
            },
        };
        let inputs = [key(KEYBD_EVENT_FLAGS(0)), key(KEYEVENTF_KEYUP)];
        unsafe {
            SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
        }
    }

    pub fn spawn(key: &str, hold_ms: u64, tx: Sender<KeyEvent>) -> Result<(), String> {
        let vk = windows_vk(key).ok_or_else(|| format!("I don't know a key called \"{key}\""))?;
        *STATE.lock().map_err(|_| "the key state is poisoned")? = Some(Shared { vk, gate: Gate::new(hold_ms), tx });
        let (ok_tx, ok_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || unsafe {
            match SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), None, 0) {
                Ok(_) => {
                    let _ = ok_tx.send(Ok(()));
                    let mut msg = MSG::default();
                    while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
                }
                Err(e) => {
                    let _ = ok_tx.send(Err(format!("Windows wouldn't let me watch the keyboard: {e}")));
                }
            }
        });
        // The timer: a hold crosses its threshold even with no key repeats.
        // Asleep while the key is up; the hook unparks it on a press.
        let timer = std::thread::spawn(|| loop {
            let waiting = STATE.lock().or_else(crate::crash::unpoison).ok().and_then(|g| g.as_ref().map(|s| s.gate.waiting_for_hold())).unwrap_or(false);
            if !waiting {
                std::thread::park_timeout(std::time::Duration::from_secs(5));
                continue;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
            if let Ok(mut g) = STATE.lock().or_else(crate::crash::unpoison) {
                if let Some(s) = g.as_mut() {
                    if let Some(e) = s.gate.tick(super::now_ms()) {
                        let _ = s.tx.send(e);
                    }
                }
            }
        });
        // unheard-ok: a OnceLock already set keeps its first value, which is the one wanted
        let _ = TIMER.set(timer.thread().clone());
        ok_rx.recv().map_err(|_| "the keyboard thread didn't start".to_string())?
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use super::*;
    use std::io::Read;
    use std::sync::mpsc::Sender;

    /// The keyboards: event devices whose capabilities include letter keys.
    fn keyboards() -> Vec<std::path::PathBuf> {
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir("/dev/input/by-path") {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().ends_with("-event-kbd") {
                    out.push(e.path());
                }
            }
        }
        out
    }

    pub fn spawn(key: &str, hold_ms: u64, tx: Sender<KeyEvent>) -> Result<(), String> {
        let code = linux_code(key).ok_or_else(|| format!("I don't know a key called \"{key}\""))?;
        let devs = keyboards();
        if devs.is_empty() {
            return Err("no keyboard device found under /dev/input".into());
        }
        let mut opened = 0;
        let gate = std::sync::Arc::new(std::sync::Mutex::new(Gate::new(hold_ms)));
        // The hold timer, asleep while the key is up: a reader unparks it on
        // a press (audit Q14: it woke 50 times a second to find the key up).
        let timer = {
            let (tx, gate) = (tx.clone(), gate.clone());
            std::thread::spawn(move || loop {
                if !gate.lock().or_else(crate::crash::unpoison).map(|g| g.waiting_for_hold()).unwrap_or(false) {
                    std::thread::park_timeout(std::time::Duration::from_secs(5));
                    continue;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
                if let Ok(mut g) = gate.lock().or_else(crate::crash::unpoison) {
                    if let Some(e) = g.tick(now_ms()) {
                        let _ = tx.send(e);
                    }
                }
            })
            .thread()
            .clone()
        };
        for d in devs {
            let mut f = match std::fs::File::open(&d) {
                Ok(f) => f,
                Err(_) => continue,
            };
            opened += 1;
            let (tx, gate, timer) = (tx.clone(), gate.clone(), timer.clone());
            std::thread::spawn(move || {
                let mut buf = [0u8; 24];
                while f.read_exact(&mut buf).is_ok() {
                    if let Some((c, v)) = parse_linux_event(&buf) {
                        if c != code {
                            continue;
                        }
                        if let Ok(mut g) = gate.lock().or_else(crate::crash::unpoison) {
                            let verdict = if v == 0 { g.up(now_ms()) } else { g.down(now_ms()) };
                            if let Some(e) = verdict.event {
                                let _ = tx.send(e);
                            }
                            if v != 0 {
                                timer.unpark();
                            }
                        }
                    }
                }
            });
        }
        if opened == 0 {
            return Err("I can see the keyboard but can't read it — add yourself to the `input` group".into());
        }
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "linux")))]
mod platform {
    use super::*;
    pub fn spawn(_key: &str, _hold_ms: u64, _tx: std::sync::mpsc::Sender<KeyEvent>) -> Result<(), String> {
        Err("no keyboard hook on this system yet".into())
    }
}
