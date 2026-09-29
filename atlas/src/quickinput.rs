//! Somewhere to type when speaking isn't working.
//!
//! The gap this fills: voice fails — the room is loud, you have a cold, the
//! mic dropped — and there is nowhere to type. Opening Notepad to talk to your
//! assistant is absurd, and a terminal buried behind three windows is barely
//! better.
//!
//! The answer is a key you press anywhere. A single-line box appears over
//! whatever you're doing, you type, press Enter, and it disappears. Nothing to
//! find, nothing to alt-tab to, no window management.
//!
//! Two ways of showing that box, because one is available today and the other
//! is nicer:
//!
//! * **Console** — bring Atlas's own window forward and focus it. Works
//!   immediately, no new UI code.
//! * **Overlay** — a small borderless window drawn over everything, dismissed
//!   on Escape. Better, and needs real Win32 work.

use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Surface {
    /// Raise and focus the existing Atlas window.
    Console,
    /// A borderless one-line box over everything.
    Overlay,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct QuickInputConfig {
    pub enabled: bool,
    /// The key that summons it, anywhere in Windows.
    pub hotkey: String,
    pub surface: Surface,
    /// Close it if you type nothing for this long.
    pub idle_close_secs: u64,
}

impl Default for QuickInputConfig {
    fn default() -> Self {
        QuickInputConfig {
            enabled: true,
            // Ctrl+Shift+Space: no Alt needed (Eric's keyboard has none),
            // no default Windows binding, and not a key you press while
            // typing. Changed in Settings by pressing the key you want.
            hotkey: "ctrl+shift+space".into(),
            surface: Surface::Console,
            idle_close_secs: 30,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Hidden,
    /// Waiting for you to type.
    Open,
    /// You pressed Enter; the text is on its way.
    Submitted,
}

/// What the caller should do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Nothing,
    Show,
    Hide,
    /// Run this, then hide.
    Submit(String),
    /// Say this first — it opened on its own and you should know why.
    ShowWithReason(String),
}

impl Action {
    /// For anywhere this is reported rather than acted on.
    pub fn plain(&self) -> String {
        match self {
            Action::Nothing => "nothing to do".into(),
            Action::Show => "opening the box".into(),
            Action::Hide => "closing the box".into(),
            Action::Submit(t) => format!("sending \"{t}\""),
            Action::ShowWithReason(why) => format!("opening the box: {why}"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuickInput {
    pub cfg: QuickInputConfig,
    pub state: State,
    pub buffer: String,
    opened_at: u64,
    last_keystroke: u64,
    /// Restored after the box closes, so you land back where you were.
    pub previous_focus: Option<String>,
}

impl Default for QuickInput {
    fn default() -> Self {
        QuickInput::new(QuickInputConfig::default())
    }
}

impl QuickInput {
    pub fn new(cfg: QuickInputConfig) -> QuickInput {
        QuickInput {
            cfg,
            state: State::Hidden,
            buffer: String::new(),
            opened_at: 0,
            last_keystroke: 0,
            previous_focus: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.state == State::Open
    }

    /// The hotkey was pressed. Pressing it again while open closes it, so the
    /// same key both summons and dismisses.
    pub fn hotkey(&mut self, focused_app: Option<String>, t: u64) -> Action {
        if !self.cfg.enabled {
            return Action::Nothing;
        }
        match self.state {
            State::Open => {
                self.close();
                Action::Hide
            }
            _ => {
                self.previous_focus = focused_app;
                self.state = State::Open;
                self.buffer.clear();
                self.opened_at = t;
                self.last_keystroke = t;
                Action::Show
            }
        }
    }

    pub fn typed(&mut self, c: char, t: u64) {
        if self.state != State::Open {
            return;
        }
        self.buffer.push(c);
        self.last_keystroke = t;
    }

    pub fn backspace(&mut self, t: u64) {
        if self.state == State::Open {
            self.buffer.pop();
            self.last_keystroke = t;
        }
    }

    pub fn submit(&mut self) -> Action {
        if self.state != State::Open {
            return Action::Nothing;
        }
        let text = self.buffer.trim().to_string();
        self.close();
        if text.is_empty() {
            // Enter on an empty box means "go away", not "run nothing".
            return Action::Hide;
        }
        Action::Submit(text)
    }

    pub fn escape(&mut self) -> Action {
        if self.state != State::Open {
            return Action::Nothing;
        }
        self.close();
        Action::Hide
    }

    /// Close a box you opened and wandered off from.
    pub fn tick(&mut self, t: u64) -> Action {
        if self.state != State::Open {
            return Action::Nothing;
        }
        // Only idle if nothing has been typed — a half-finished thought is
        // worse to lose than an empty box is to leave open.
        if self.buffer.is_empty() && t.saturating_sub(self.last_keystroke) >= self.cfg.idle_close_secs {
            self.close();
            return Action::Hide;
        }
        Action::Nothing
    }

    fn close(&mut self) {
        self.state = State::Hidden;
        self.buffer.clear();
    }

    /// The prompt shown in the box. Short, and it says what it is.
    pub fn placeholder(&self) -> &'static str {
        "atlas >"
    }
}

/// Parse a key setting into modifier flags and a key, for RegisterHotKey.
///
/// Returns (modifiers, virtual key). MOD_ALT 1, MOD_CONTROL 2, MOD_SHIFT 4,
/// MOD_WIN 8. No particular modifier is required (Eric, 26 Sep 2026: "I
/// don't have an Alt button"): Ctrl, Shift or Win on their own are enough,
/// and a key nobody types with — an F-key, Insert, Pause, Scroll Lock, the
/// menu key — works with no modifier at all. What's refused is a key that
/// would fire while you type (a letter, a digit, Space, Tab, Enter) with
/// nothing held, and a modifier on its own, which Windows can't register.
pub fn parse_hotkey(spec: &str) -> Option<(u32, u32)> {
    let mut mods = 0u32;
    let mut key = None;
    for part in spec.to_lowercase().split('+') {
        match part.trim() {
            "" => continue,
            "alt" => mods |= 1,
            "ctrl" | "control" => mods |= 2,
            "shift" => mods |= 4,
            "win" | "super" | "windows" => mods |= 8,
            k => key = Some(crate::hotkeys::key_code(k)?),
        }
    }
    let key = key?;
    // Ctrl, Alt, Shift and Win themselves can't be a registered hotkey.
    if (0xA0..=0xA5).contains(&key) {
        return None;
    }
    if mods == 0 && !crate::hotkeys::safe_alone(key) {
        return None;
    }
    Some((mods, key))
}
