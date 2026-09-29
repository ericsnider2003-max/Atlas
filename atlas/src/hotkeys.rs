//! Keys that reach Atlas from anywhere in Windows (Eric's ruling H1, 25 Sep
//! 2026): the wake word, push-to-talk *and* a typing box, all three, with the
//! keys set per person and none locked in.
//!
//! Two keys, both from settings:
//!
//! * **Push-to-talk** (`push_to_talk.key`, held for `push_to_talk.hold_ms`).
//!   Hold it and speak; let go and Atlas hears it. A quick tap is given back
//!   to the app you're in, so a push-to-talk key of Tab still types a tab.
//!   That's what `input::HoldToTalk` was written for: the key is held back
//!   until it's clear which you meant, and only a hold is kept.
//! * **The typing box** (`quick_input.hotkey`, e.g. `ctrl+shift+space`). Press it
//!   and a one-line box opens over whatever you're doing (`typebox`).
//!
//! On Windows the push-to-talk key is watched by a low-level keyboard hook,
//! which is the only way to hold a key back from the app you're in, and the
//! typing-box combination is registered with `RegisterHotKey`, which Windows
//! refuses if another program already owns it — said, rather than silently
//! not working. Elsewhere there are no global keys, and `start` says so.

use crate::input::{HoldToTalk, KeyEvent};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};

/// What a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pressed {
    /// Push-to-talk is held: start listening.
    TalkStart,
    /// Let go: stop listening and hear what was said.
    TalkStop,
    /// Open the typing box.
    TypingBox,
}

impl Pressed {
    /// What happened, in words, for the log.
    pub fn plain(&self) -> &'static str {
        match self {
            Pressed::TalkStart => "push-to-talk held",
            Pressed::TalkStop => "push-to-talk let go",
            Pressed::TypingBox => "typing-box key pressed",
        }
    }
}

/// A single key's Windows virtual-key code, by the name you'd write in
/// settings. Wider than `quickinput::parse_hotkey`'s list because a
/// push-to-talk key is usually one nobody types with: Caps Lock, Right Ctrl,
/// Scroll Lock, a spare F-key.
pub fn key_code(name: &str) -> Option<u32> {
    let k = name.trim().to_lowercase().replace([' ', '_', '-'], "");
    let code = match k.as_str() {
        "tab" => 0x09,
        "space" | "spacebar" => 0x20,
        "capslock" | "caps" => 0x14,
        "scrolllock" => 0x91,
        "pause" | "break" => 0x13,
        "insert" | "ins" => 0x2D,
        "rightctrl" | "rctrl" | "rightcontrol" => 0xA3,
        "leftctrl" | "lctrl" | "leftcontrol" => 0xA2,
        "rightalt" | "ralt" | "altgr" => 0xA5,
        "rightshift" | "rshift" => 0xA1,
        "leftshift" | "lshift" => 0xA0,
        "backtick" | "`" | "grave" => 0xC0,
        "menu" | "apps" | "contextmenu" => 0x5D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        f if f.len() > 1 && f.starts_with('f') && f[1..].chars().all(|c| c.is_ascii_digit()) => {
            let n: u32 = f[1..].parse().ok()?;
            if !(1..=24).contains(&n) {
                return None;
            }
            0x70 + n - 1
        }
        c if c.len() == 1 && c.chars().all(|c| c.is_ascii_alphanumeric()) => c.to_ascii_uppercase().as_bytes()[0] as u32,
        _ => return None,
    };
    Some(code)
}

/// A key that can be the typing-box key with nothing held: one nobody types
/// with. F1–F24, Insert, Pause, Scroll Lock and the menu key.
pub fn safe_alone(vk: u32) -> bool {
    (0x70..=0x87).contains(&vk) || matches!(vk, 0x2D | 0x13 | 0x91 | 0x5D)
}

/// A key setting written the way it's shown and stored ("ctrl+shift+space"),
/// from what was pressed. `None` when it couldn't be a key.
pub fn spec_of(ctrl: bool, shift: bool, alt: bool, win: bool, key: &str) -> Option<String> {
    let k = key.trim().to_lowercase().replace(' ', "");
    key_code(&k)?;
    let mut parts = Vec::new();
    if ctrl {
        parts.push("ctrl".to_string());
    }
    if alt {
        parts.push("alt".to_string());
    }
    if shift {
        parts.push("shift".to_string());
    }
    if win {
        parts.push("win".to_string());
    }
    parts.push(k);
    Some(parts.join("+"))
}

/// Is this a usable setting for `which` ("push_to_talk.key" or
/// "quick_input.hotkey")? `Err` says why not, in words.
pub fn check_setting(which: &str, spec: &str) -> Result<(), String> {
    if which == "push_to_talk.key" {
        if spec.contains('+') {
            return Err("push-to-talk is one key you hold — pick a single key, like capslock or rightctrl".into());
        }
        return key_code(spec).map(|_| ()).ok_or_else(|| format!("I don't know a key called \"{spec}\""));
    }
    crate::quickinput::parse_hotkey(spec).map(|_| ()).ok_or_else(|| {
        format!(
            "\"{spec}\" can't be the typing-box key — it needs Ctrl, Shift or Win with a key, or a key nobody types with on its own (an F-key, Insert, Pause, Scroll Lock)"
        )
    })
}

/// The keys as settings name them, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keys {
    pub talk: Option<(u32, u64)>,
    /// (modifiers, key) for `RegisterHotKey`.
    pub typing: Option<(u32, u32)>,
    /// What was wrong with a key that was set, said at start-up.
    pub problems: Vec<String>,
}

impl Keys {
    pub fn from_settings(ptt: &crate::voice::PttConfig, typing: &crate::quickinput::QuickInputConfig) -> Keys {
        let mut problems = Vec::new();
        let talk = if !ptt.enabled || ptt.key.trim().is_empty() {
            None
        } else {
            match key_code(&ptt.key) {
                Some(vk) => Some((vk, ptt.hold_ms.max(150))),
                None => {
                    problems.push(format!("I don't know the key \"{}\" for push-to-talk", ptt.key));
                    None
                }
            }
        };
        let typing = if !typing.enabled || typing.hotkey.trim().is_empty() {
            None
        } else {
            match crate::quickinput::parse_hotkey(&typing.hotkey) {
                Some(k) => Some(k),
                None => {
                    problems.push(check_setting("quick_input.hotkey", &typing.hotkey).err().unwrap_or_default());
                    None
                }
            }
        };
        Keys { talk, typing, problems }
    }

    /// For start-up: how to reach Atlas without the wake word.
    pub fn said(&self, ptt: &crate::voice::PttConfig, typing: &crate::quickinput::QuickInputConfig) -> Option<String> {
        let mut ways = Vec::new();
        if self.talk.is_some() {
            ways.push(format!("hold {} to talk", pretty(&ptt.key)));
        }
        if self.typing.is_some() {
            ways.push(format!("press {} to type", pretty(&typing.hotkey)));
        }
        if ways.is_empty() {
            return None;
        }
        Some(format!("You can also {}.", ways.join(", or ")))
    }
}

fn pretty(key: &str) -> String {
    key.split('+').map(key_word).collect::<Vec<_>>().join("+")
}

/// One key's name as a person writes it: "tab" → "Tab", "capslock" →
/// "Caps Lock", "f9" → "F9", "q" → "Q".
pub fn key_word(k: &str) -> String {
    let k = k.trim();
    let named = match k.to_lowercase().replace([' ', '_', '-'], "").as_str() {
        "ctrl" | "control" => "Ctrl",
        "alt" => "Alt",
        "shift" => "Shift",
        "win" | "windows" | "super" => "Win",
        "tab" => "Tab",
        "space" | "spacebar" => "Space",
        "capslock" | "caps" => "Caps Lock",
        "scrolllock" => "Scroll Lock",
        "pause" | "break" => "Pause",
        "insert" | "ins" => "Insert",
        "rightctrl" | "rctrl" | "rightcontrol" => "Right Ctrl",
        "leftctrl" | "lctrl" | "leftcontrol" => "Left Ctrl",
        "rightalt" | "ralt" | "altgr" => "Right Alt",
        "rightshift" | "rshift" => "Right Shift",
        "leftshift" | "lshift" => "Left Shift",
        "backtick" | "`" | "grave" => "`",
        "menu" | "apps" | "contextmenu" => "Menu",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Page Up",
        "pagedown" => "Page Down",
        _ => "",
    };
    if !named.is_empty() {
        return named.to_string();
    }
    k.to_uppercase()
}

/// What the hook does with the push-to-talk key. Kept apart from Windows so
/// the decisions are tested without a keyboard.
#[derive(Debug)]
pub struct Gate {
    hold: HoldToTalk,
    down: bool,
}

/// Keep the key from the app, or let it through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hook {
    Swallow,
    Pass,
}

impl Gate {
    pub fn new(hold_ms: u64) -> Gate {
        Gate { hold: HoldToTalk::new("", hold_ms), down: false }
    }

    /// The key went down (or repeated). Held back until it's clear.
    pub fn key_down(&mut self, now_ms: u64) -> (Hook, Option<Pressed>) {
        if !self.down {
            self.down = true;
            let e = self.hold.down(now_ms);
            return (Hook::Swallow, (e == KeyEvent::StartTalking).then_some(Pressed::TalkStart));
        }
        // Auto-repeat while held.
        (Hook::Swallow, self.tick(now_ms))
    }

    /// Checked on a timer, so talking starts at `hold_ms`, not at the
    /// keyboard's first auto-repeat half a second in.
    pub fn tick(&mut self, now_ms: u64) -> Option<Pressed> {
        if !self.down {
            return None;
        }
        (self.hold.poll(now_ms) == KeyEvent::StartTalking).then_some(Pressed::TalkStart)
    }

    /// The key came up. The bool is "give the app the tap it was denied".
    pub fn key_up(&mut self, now_ms: u64) -> (Option<Pressed>, bool) {
        if !self.down {
            return (None, false);
        }
        self.down = false;
        match self.hold.up(now_ms) {
            KeyEvent::StopTalking => (Some(Pressed::TalkStop), false),
            _ => (None, true),
        }
    }

    pub fn is_talking(&self) -> bool {
        self.hold.is_talking()
    }
}

/// The running keys: what they did, and whether push-to-talk is held now.
pub struct Hotkeys {
    rx: Receiver<Pressed>,
    gate: Arc<Mutex<Gate>>,
    /// A key Windows wouldn't give us, said at start-up; the other still works.
    pub problems: Vec<String>,
}

impl Hotkeys {
    /// The next thing a key did, without waiting.
    pub fn poll(&self) -> Option<Pressed> {
        self.rx.try_recv().ok()
    }

    /// Push-to-talk is held right now. What a listen checks to know when to
    /// stop.
    pub fn held(&self) -> bool {
        self.gate.lock().map(|g| g.is_talking()).unwrap_or(false)
    }

    /// For tests and for a platform that feeds keys some other way.
    pub fn from_parts(rx: Receiver<Pressed>, gate: Arc<Mutex<Gate>>) -> Hotkeys {
        Hotkeys { rx, gate, problems: Vec::new() }
    }
}

/// `atlas keys`: say what each key press is seen as, for `secs` seconds.
/// Returns the closing line.
pub fn try_them(
    ptt: &crate::voice::PttConfig,
    typing: &crate::quickinput::QuickInputConfig,
    secs: u64,
    say: &mut dyn FnMut(String),
) -> String {
    let keys = Keys::from_settings(ptt, typing);
    let h = match start(&keys) {
        Ok(h) => h,
        Err(why) => return format!("No keys to try: {why}."),
    };
    for p in &h.problems {
        say(format!("({p})"));
    }
    say(format!(
        "Press your keys now — {}. I'll show what I see for {secs} seconds; nothing is heard or opened.",
        keys.said(ptt, typing).unwrap_or_default().trim_start_matches("You can also ").trim_end_matches('.')
    ));
    let until = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    let mut seen = 0;
    while std::time::Instant::now() < until {
        while let Some(ev) = h.poll() {
            seen += 1;
            say(heard_as(ev, ptt, typing));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if seen == 0 {
        "Nothing came through as a hold or a press. A quick tap of the push-to-talk key goes to your app on purpose — hold it. If you did, another program may be holding the key: pick another in Settings → Keys.".into()
    } else {
        format!("{seen} key event{} seen. The keys work.", if seen == 1 { "" } else { "s" })
    }
}

/// What a key event is, said.
pub fn heard_as(ev: Pressed, ptt: &crate::voice::PttConfig, typing: &crate::quickinput::QuickInputConfig) -> String {
    match ev {
        Pressed::TalkStart => format!("{} held — Atlas would start listening now.", pretty(&ptt.key)),
        Pressed::TalkStop => format!("{} let go — Atlas would stop and hear what you said.", pretty(&ptt.key)),
        Pressed::TypingBox => format!("{} pressed — the typing box would open.", pretty(&typing.hotkey)),
    }
}

/// Start watching the keys. `Err` says why there are none.
pub fn start(keys: &Keys) -> Result<Hotkeys, String> {
    if keys.talk.is_none() && keys.typing.is_none() {
        return Err("no push-to-talk or typing-box key is set".into());
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let gate = Arc::new(Mutex::new(Gate::new(keys.talk.map(|(_, h)| h).unwrap_or(350))));
    let mut problems = keys.problems.clone();
    problems.extend(start_platform(keys, tx, gate.clone())?);
    let mut h = Hotkeys::from_parts(rx, gate);
    h.problems = problems;
    Ok(h)
}

#[cfg(not(windows))]
fn start_platform(_keys: &Keys, _tx: Sender<Pressed>, _gate: Arc<Mutex<Gate>>) -> Result<Vec<String>, String> {
    Err("global keys are only watched on Windows".into())
}

#[cfg(windows)]
fn start_platform(keys: &Keys, tx: Sender<Pressed>, gate: Arc<Mutex<Gate>>) -> Result<Vec<String>, String> {
    win::start(keys.clone(), tx, gate)
}

#[cfg(windows)]
mod win {
    use super::{Gate, Hook, Keys, Pressed};
    use std::sync::mpsc::Sender;
    use std::sync::{Arc, Mutex, OnceLock};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    use windows::Win32::UI::WindowsAndMessaging::*;

    struct Shared {
        vk: u32,
        tx: Sender<Pressed>,
        gate: Arc<Mutex<Gate>>,
        started: std::time::Instant,
    }

    static SHARED: OnceLock<Shared> = OnceLock::new();

    fn now_ms(s: &Shared) -> u64 {
        s.started.elapsed().as_millis() as u64
    }

    /// Marks the taps Atlas puts back itself, so the hook lets those through
    /// and nothing else: a key sent by a remapping program (a keyboard
    /// without the key you want, say) is still a key you pressed.
    const ATLAS_TAP: usize = 0x4154_4C53;

    fn tap(vk: u32) {
        let key = |up: bool| INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk as u16),
                    wScan: 0,
                    dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                    time: 0,
                    dwExtraInfo: ATLAS_TAP,
                },
            },
        };
        unsafe {
            SendInput(&[key(false), key(true)], std::mem::size_of::<INPUT>() as i32);
        }
    }

    unsafe extern "system" fn hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if code >= 0 {
            if let Some(s) = SHARED.get() {
                let k = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
                // What we put back ourselves goes straight through; a key
                // injected by anything else is treated as pressed.
                let ours = (k.flags.0 & LLKHF_INJECTED.0) != 0 && k.dwExtraInfo == ATLAS_TAP;
                if k.vkCode == s.vk && !ours {
                    let msg = wparam.0 as u32;
                    let t = now_ms(s);
                    let mut g = match s.gate.lock() {
                        Ok(g) => g,
                        Err(_) => return CallNextHookEx(None, code, wparam, lparam),
                    };
                    if msg == WM_KEYDOWN || msg == WM_SYSKEYDOWN {
                        let (h, ev) = g.key_down(t);
                        if let Some(e) = ev {
                            let _ = s.tx.send(e);
                        }
                        if h == Hook::Swallow {
                            return LRESULT(1);
                        }
                    } else if msg == WM_KEYUP || msg == WM_SYSKEYUP {
                        let (ev, give_back) = g.key_up(t);
                        drop(g);
                        if let Some(e) = ev {
                            let _ = s.tx.send(e);
                        }
                        if give_back {
                            tap(s.vk);
                        }
                        return LRESULT(1);
                    }
                }
            }
        }
        CallNextHookEx(None, code, wparam, lparam)
    }

    pub fn start(keys: Keys, tx: Sender<Pressed>, gate: Arc<Mutex<Gate>>) -> Result<Vec<String>, String> {
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Vec<String>>();
        let timer_gate = gate.clone();
        let timer_tx = tx.clone();
        std::thread::spawn(move || unsafe {
            let mut problems = Vec::new();
            if let Some((vk, _)) = keys.talk {
                let _ = SHARED.set(Shared { vk, tx: tx.clone(), gate, started: std::time::Instant::now() });
                if let Err(e) = SetWindowsHookExW(WH_KEYBOARD_LL, Some(hook), None, 0) {
                    problems.push(format!("Windows wouldn't let me watch the push-to-talk key ({e})"));
                }
            }
            const TYPING_ID: i32 = 0xA71A;
            if let Some((mods, vk)) = keys.typing {
                let m = HOT_KEY_MODIFIERS(mods) | MOD_NOREPEAT;
                if let Err(e) = RegisterHotKey(HWND::default(), TYPING_ID, m, vk) {
                    // Only "already registered" (1409) means another program
                    // has the key; anything else is said as Windows said it
                    // (29 Sep 2026: every failure was blamed on another program).
                    problems.push(crate::hotkeys::typing_key_refused(e.code().0 as u32 & 0xFFFF, &e.to_string()));
                }
            }
            let _ = ready_tx.send(problems);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, HWND::default(), 0, 0).as_bool() {
                if msg.message == WM_HOTKEY && msg.wParam.0 as i32 == TYPING_ID {
                    let _ = tx.send(Pressed::TypingBox);
                }
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        });
        // Talking starts at `hold_ms`, checked on a timer rather than waiting
        // for the keyboard's own repeat.
        if keys.talk.is_some() {
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_millis(20));
                let Some(s) = SHARED.get() else { continue };
                let t = now_ms(s);
                let ev = timer_gate.lock().ok().and_then(|mut g| g.tick(t));
                if let Some(e) = ev {
                    if timer_tx.send(e).is_err() {
                        break;
                    }
                }
            });
        }
        ready_rx.recv().map_err(|_| "the key watcher didn't start".to_string())
    }
}

/// Why Windows refused the typing-box key, from its error code: 1409
/// (ERROR_HOTKEY_ALREADY_REGISTERED) is another program holding it.
pub fn typing_key_refused(code: u32, message: &str) -> String {
    if code == 1409 {
        "another program already uses the typing-box key — pick another in settings".into()
    } else {
        format!("Windows wouldn't give me the typing-box key ({message}) — pick another in settings")
    }
}
