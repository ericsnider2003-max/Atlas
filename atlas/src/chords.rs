//! Key chords Atlas answers to anywhere -- Ctrl+Alt+Space captures what's
//! selected as a note, Ctrl+Alt+E expands the snippet word you just typed,
//! Ctrl+Alt+T copies the text off the window in front -- without ever
//! watching your typing.
//!
//! **How, and why this way.** Windows' `RegisterHotKey` asks the OS to tell
//! Atlas when one exact chord is pressed, and nothing else. Unlike the
//! push-to-talk key (`hotkey`, a low-level hook that sees every key so it can
//! hold one back), a registered chord can't see what you type. That's the
//! property snippet expansion needs: Espanso-style expansion reads every
//! keystroke; this reads none. When the expand chord comes, Atlas selects
//! the word before the cursor (Ctrl+Shift+Left), copies it through the
//! clipboard, puts your clipboard back as it was, and replaces the word only
//! if it is exactly one of your triggers (`snippets::Snippets::exact`).
//!
//! A chord another program already owns fails to register, and that's said
//! (`doctor`), not swallowed.
//!
//! **Sources:** Microsoft's `RegisterHotKey` documentation (MOD_NOREPEAT so
//! a held chord fires once); PowerToys' Keyboard Manager read for which
//! chords Windows itself reserves (Win+letter), which is why the defaults
//! use Ctrl+Alt.

/// Modifier bits as `RegisterHotKey` takes them.
pub const MOD_ALT: u32 = 0x1;
pub const MOD_CONTROL: u32 = 0x2;
pub const MOD_SHIFT: u32 = 0x4;
pub const MOD_WIN: u32 = 0x8;
pub const MOD_NOREPEAT: u32 = 0x4000;

/// A chord: modifiers and one key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chord {
    pub mods: u32,
    pub vk: u16,
}

/// "ctrl+alt+space" -> a chord. At least one of ctrl/alt/win is required, so
/// a chord can never be an ordinary keystroke; Win+letter is refused because
/// Windows keeps those.
pub fn read_chord(text: &str) -> Result<Chord, String> {
    let mut mods = 0;
    let mut key = None;
    for part in text.split('+').map(|p| p.trim().to_ascii_lowercase()) {
        match part.as_str() {
            "ctrl" | "control" => mods |= MOD_CONTROL,
            "alt" => mods |= MOD_ALT,
            "shift" => mods |= MOD_SHIFT,
            "win" | "super" | "meta" => mods |= MOD_WIN,
            "" => return Err(format!("\"{text}\" has an empty part")),
            k => {
                if key.is_some() {
                    return Err(format!("\"{text}\" has two keys; a chord is modifiers and one key"));
                }
                key = Some(crate::platform::virtual_key(k).ok_or_else(|| format!("I don't know the key \"{k}\""))?);
            }
        }
    }
    let vk = key.ok_or_else(|| format!("\"{text}\" has no key, only modifiers"))?;
    if mods & (MOD_CONTROL | MOD_ALT | MOD_WIN) == 0 {
        return Err(format!("\"{text}\" needs Ctrl, Alt or Win, or it would fire while you type"));
    }
    if mods & MOD_WIN != 0 && (b'A' as u16..=b'Z' as u16).contains(&vk) {
        return Err(format!("\"{text}\": Windows keeps Win+letter for itself"));
    }
    Ok(Chord { mods, vk })
}

/// What a chord is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Does {
    Capture,
    Expand,
    CopyText,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ChordsConfig {
    /// Off until you turn it on: a chord that fires unexpectedly is worse
    /// than one you have to enable.
    pub enabled: bool,
    pub capture: String,
    pub expand: String,
    pub copy_text: String,
}

impl Default for ChordsConfig {
    fn default() -> Self {
        ChordsConfig {
            enabled: false,
            capture: "ctrl+alt+space".into(),
            expand: "ctrl+alt+e".into(),
            copy_text: "ctrl+alt+t".into(),
        }
    }
}

impl ChordsConfig {
    /// The chords, parsed; the ones that don't parse come back as problems.
    /// Two jobs on one chord is a problem too.
    pub fn chords(&self) -> (Vec<(Does, Chord)>, Vec<String>) {
        let mut ok: Vec<(Does, Chord)> = Vec::new();
        let mut bad = Vec::new();
        for (does, text) in [(Does::Capture, &self.capture), (Does::Expand, &self.expand), (Does::CopyText, &self.copy_text)] {
            if text.trim().is_empty() {
                continue;
            }
            match read_chord(text) {
                Ok(c) if ok.iter().any(|(_, o)| *o == c) => bad.push(format!("{text} is set for two things")),
                Ok(c) => ok.push((does, c)),
                Err(e) => bad.push(e),
            }
        }
        (ok, bad)
    }
}

/// The word the expand chord selected, checked: one word, no spaces, short.
/// Anything else means the selection wasn't a trigger and nothing is typed.
pub fn plausible_trigger(selected: &str) -> Option<&str> {
    let s = selected.trim();
    (!s.is_empty() && s.chars().count() <= 32 && !s.contains(char::is_whitespace)).then_some(s)
}

/// Run the expand chord against the focused app: select the word before the
/// cursor, read it through the clipboard, restore the clipboard, and type
/// the snippet over it if the word is a trigger. Returns the trigger used.
pub fn expand(p: &dyn crate::platform::Platform, snippets: &crate::snippets::Snippets, local_now: u64) -> crate::error::Result<Option<String>> {
    let before = p.read_clipboard()?;
    // A sentinel, so a failed copy isn't mistaken for the old clipboard.
    p.write_clipboard("\u{2063}")?;
    p.press("ctrl+shift+left")?;
    p.press("ctrl+c")?;
    p.sleep_ms(60);
    let got = p.read_clipboard()?;
    match &before {
        Some(b) => p.write_clipboard(b)?,
        None => p.write_clipboard("")?,
    }
    let Some(word) = got.as_deref().filter(|g| *g != "\u{2063}").and_then(|w| plausible_trigger(w)) else {
        // Nothing selected, or a copy that didn't happen: put the cursor
        // back where it was.
        p.press("right")?;
        return Ok(None);
    };
    match snippets.exact(word) {
        Some(text) => {
            // The word is selected, so typing replaces it.
            p.type_text(&crate::snippets::fill(text, local_now))?;
            Ok(Some(word.to_string()))
        }
        None => {
            p.press("right")?;
            Ok(None)
        }
    }
}

/// Register the chords on a thread of their own and send what fires.
/// Returns the chords that couldn't be registered (another program has
/// them). Global chords are built for Windows; elsewhere every chord is
/// reported as not registered, so the reply says so.
pub fn start_chords(chords: Vec<(Does, Chord)>, tx: std::sync::mpsc::Sender<Does>) -> Vec<Does> {
    #[cfg(windows)]
    {
        use windows::Win32::UI::Input::KeyboardAndMouse::{RegisterHotKey, HOT_KEY_MODIFIERS};
        use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};
        let (rtx, rrx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("atlas-chords".into())
            .spawn(move || {
                let mut failed = Vec::new();
                for (i, (does, c)) in chords.iter().enumerate() {
                    // SAFETY: a thread-owned hotkey (no window); unregistered
                    // when the thread's message queue goes away with it.
                    let ok = unsafe { RegisterHotKey(None, i as i32 + 1, HOT_KEY_MODIFIERS(c.mods | MOD_NOREPEAT), c.vk as u32) };
                    if ok.is_err() {
                        failed.push(*does);
                    }
                }
                let _ = rtx.send(failed);
                let mut msg = MSG::default();
                // SAFETY: the standard message loop on this thread's queue.
                while unsafe { GetMessageW(&mut msg, None, 0, 0) }.as_bool() {
                    if msg.message == WM_HOTKEY {
                        let id = msg.wParam.0;
                        if let Some((does, _)) = id.checked_sub(1).and_then(|i| chords.get(i)) {
                            if tx.send(*does).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .ok();
        rrx.recv_timeout(std::time::Duration::from_secs(2)).unwrap_or_default()
    }
    #[cfg(not(windows))]
    {
        let _ = tx;
        chords.into_iter().map(|(d, _)| d).collect()
    }
}
