//! A fake OS. Lets the workspace sequence be tested and dry-run anywhere.

use super::{ActiveWindow, Button, Monitor, PixelRect, Platform, WindowId};
use crate::config::AppSpec;
use crate::error::Result;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Launch(String),
    Place(String, PixelRect),
    Focus(String),
    Close(String),
    Sleep(u64),
    Click(i32, i32, Button),
    Scroll(i32, i32),
    Type(String),
    Press(String),
}

pub struct MockPlatform {
    monitors: Vec<Monitor>,
    /// How many find_window polls each app needs before its window "appears".
    appears_after: HashMap<String, u32>,
    polls: RefCell<HashMap<String, u32>>,
    launched: RefCell<HashSet<String>>,
    running: RefCell<HashMap<String, WindowId>>,
    pub log: RefCell<Vec<Action>>,
    pub active: RefCell<Option<ActiveWindow>>,
    next_id: RefCell<u64>,
    /// A real in-memory clipboard, so read/write can be driven end to end in a
    /// test the way a live OS clipboard would be.
    clipboard: RefCell<Option<String>>,
    /// Seconds since keyboard/mouse input, as a test sets it (None: unknown).
    pub input_idle: RefCell<Option<u64>>,
    /// What the fake OS says about interrupting, as a test sets it.
    pub quiet: RefCell<Option<super::OsQuiet>>,
    /// The fake clipboard's sequence number and what a copy holds, for the
    /// history (`None`: the platform can't say, as on Linux).
    pub clip_seq: RefCell<Option<u32>>,
    pub clip_copy: RefCell<Option<super::ClipCopy>>,
    /// The window a capture would return, and what the OS's text engine
    /// would read off it.
    pub grab: RefCell<Option<super::Grab>>,
    pub ocr: RefCell<Option<String>>,
    /// What the OS's text engine would read off a picture file.
    pub ocr_file: RefCell<Option<String>>,
    /// What the next Ctrl+C in the focused app would copy (the selected
    /// word, for the snippet chord). Taken by the press.
    pub copy_gives: RefCell<Option<String>>,
    /// The window in front, by handle, and what each window says — so
    /// reading a window and typing into it can be driven in a test.
    pub front: RefCell<Option<WindowId>>,
    pub screens: RefCell<HashMap<u64, crate::uia::Node>>,
    /// What `focused_is_editable` answers; `None` is "can't tell".
    pub focus_editable: RefCell<Option<bool>>,
    /// What `session_locked` answers.
    pub locked: RefCell<Option<bool>>,
    /// `focus` doesn't bring the window forward.
    pub focus_refused: RefCell<bool>,
    /// Typing comes out wrong in the window — what Windows 11 Notepad did
    /// with a burst of keystrokes on Eric's laptop (the last character,
    /// repeated). Off: what's typed shows in the window in front, the way a
    /// text box does.
    pub garbles: RefCell<bool>,
    /// The text box you're typing in, when a test sets one: typing appends
    /// to it and backspace takes the last character off, the way a real one
    /// does.
    pub typing_box: RefCell<Option<String>>,
    /// How many reads of the typing box, after Atlas types into it, still
    /// show it as it was: a real app takes keys in a moment after they're
    /// sent (30 Sep 2026, `astype::read_back`).
    pub box_lags_reads: RefCell<u32>,
    lag_left: RefCell<u32>,
    box_as_shown: RefCell<Option<String>>,
    /// Whole screens can be captured: each is a picture of its monitor's
    /// size in one colour (its id's), titled "screen <id>".
    pub screen_pictures: RefCell<bool>,
    /// The monitor the window in front is on; the laptop's own screen.
    pub active_screen: RefCell<Option<u32>>,
    pub laptop_screen: RefCell<Option<u32>>,
    /// What the text engine reads off a picture with this title (a screen's
    /// "screen <id>"), before `ocr`.
    pub ocr_by_title: RefCell<HashMap<String, String>>,
    /// Where windows are, by handle (`rect_of`).
    pub window_rects: RefCell<HashMap<u64, PixelRect>>,
}

impl MockPlatform {
    pub fn new(monitors: Vec<Monitor>) -> Self {
        MockPlatform {
            monitors,
            appears_after: HashMap::new(),
            polls: RefCell::new(HashMap::new()),
            launched: RefCell::new(HashSet::new()),
            running: RefCell::new(HashMap::new()),
            log: RefCell::new(Vec::new()),
            active: RefCell::new(None),
            next_id: RefCell::new(1),
            clipboard: RefCell::new(None),
            input_idle: RefCell::new(None),
            quiet: RefCell::new(None),
            clip_seq: RefCell::new(None),
            clip_copy: RefCell::new(None),
            grab: RefCell::new(None),
            ocr: RefCell::new(None),
            ocr_file: RefCell::new(None),
            copy_gives: RefCell::new(None),
            front: RefCell::new(None),
            screens: RefCell::new(HashMap::new()),
            focus_editable: RefCell::new(None),
            locked: RefCell::new(None),
            focus_refused: RefCell::new(false),
            garbles: RefCell::new(false),
            typing_box: RefCell::new(None),
            box_lags_reads: RefCell::new(0),
            lag_left: RefCell::new(0),
            box_as_shown: RefCell::new(None),
            screen_pictures: RefCell::new(false),
            active_screen: RefCell::new(None),
            laptop_screen: RefCell::new(None),
            ocr_by_title: RefCell::new(HashMap::new()),
            window_rects: RefCell::new(HashMap::new()),
        }
    }

    /// Seed the fake OS clipboard, standing in for the person having copied
    /// something. What `read_clipboard` then hands back.
    pub fn set_clipboard(&self, text: &str) {
        *self.clipboard.borrow_mut() = Some(text.to_string());
    }

    /// What is on the fake OS clipboard now — for a test to check that a
    /// write-back actually landed.
    pub fn clipboard_now(&self) -> Option<String> {
        self.clipboard.borrow().clone()
    }

    /// The typing box is about to change under Atlas's keys: with
    /// `box_lags_reads` set, the next reads still show it as it was.
    fn box_about_to_change(&self) {
        let lag = *self.box_lags_reads.borrow();
        if lag > 0 {
            if *self.lag_left.borrow() == 0 {
                *self.box_as_shown.borrow_mut() = self.typing_box.borrow().clone();
            }
            *self.lag_left.borrow_mut() = lag;
        }
    }

    /// Everything typed, in order.
    pub fn typed(&self) -> Vec<String> {
        self.log.borrow().iter().filter_map(|a| if let Action::Type(t) = a { Some(t.clone()) } else { None }).collect()
    }

    /// Simulate a slow-starting app (Discord, Claude).
    pub fn with_slow_app(mut self, process: &str, polls: u32) -> Self {
        self.appears_after.insert(process.to_string(), polls);
        self
    }

    pub fn focus_on(&self, process: &str, title: &str) {
        *self.active.borrow_mut() = Some(ActiveWindow {
            process: process.into(),
            title: title.into(),
        });
    }

    /// Change what a window says, as a new message arriving would.
    pub fn set_window_text(&self, id: u64, text: &str) {
        let node = crate::uia::Node {
            role: crate::uia::Role::Document,
            name: String::new(),
            value: text.to_string(),
            enabled: true,
            children: Vec::new(),
            rect: None,
        };
        self.screens.borrow_mut().insert(id, node);
    }

    pub fn actions(&self) -> Vec<Action> {
        self.log.borrow().clone()
    }

    fn key(spec: &AppSpec) -> String {
        spec.process_names
            .first()
            .cloned()
            .unwrap_or_else(|| spec.launch.clone())
    }
}

impl Platform for MockPlatform {
    fn active_window(&self) -> Result<Option<ActiveWindow>> {
        Ok(self.active.borrow().clone())
    }

    fn active_window_id(&self) -> Result<Option<WindowId>> {
        Ok(*self.front.borrow())
    }

    fn read_window(&self, win: WindowId) -> Result<Option<crate::uia::Node>> {
        Ok(self.screens.borrow().get(&win.0).cloned())
    }

    fn press_named(&self, win: WindowId, name: &str) -> Result<bool> {
        let found = self
            .screens
            .borrow()
            .get(&win.0)
            .and_then(|n| n.by_name(name).map(|c| c.enabled))
            .unwrap_or(false);
        if found {
            self.log.borrow_mut().push(Action::Press(format!("button:{name}")));
        }
        Ok(found)
    }

    fn focused_text(&self) -> Result<Option<String>> {
        let left = *self.lag_left.borrow();
        if left > 0 {
            *self.lag_left.borrow_mut() = left - 1;
            return Ok(self.box_as_shown.borrow().clone());
        }
        Ok(self.typing_box.borrow().clone())
    }

    fn focused_is_editable(&self) -> Result<Option<bool>> {
        Ok(*self.focus_editable.borrow())
    }

    fn input_idle_secs(&self) -> Option<u64> {
        *self.input_idle.borrow()
    }

    fn quiet_state(&self) -> Option<super::OsQuiet> {
        *self.quiet.borrow()
    }

    fn session_locked(&self) -> Option<bool> {
        *self.locked.borrow()
    }

    fn monitors(&self) -> Result<Vec<Monitor>> {
        Ok(self.monitors.clone())
    }

    fn launch(&self, spec: &AppSpec) -> Result<()> {
        let k = Self::key(spec);
        self.log.borrow_mut().push(Action::Launch(k.clone()));
        self.polls.borrow_mut().insert(k.clone(), 0);
        self.launched.borrow_mut().insert(k);
        Ok(())
    }

    fn find_window(&self, spec: &AppSpec) -> Result<Option<WindowId>> {
        let k = Self::key(spec);
        if let Some(id) = self.running.borrow().get(&k) {
            return Ok(Some(*id));
        }
        // An app nobody started has no window. Checking this must not consume
        // a poll, or the retry accounting in the tests would be off by one.
        if !self.launched.borrow().contains(&k) {
            return Ok(None);
        }
        let needed = *self.appears_after.get(&k).unwrap_or(&0);
        let mut polls = self.polls.borrow_mut();
        let seen = polls.entry(k.clone()).or_insert(0);
        if *seen >= needed {
            let mut n = self.next_id.borrow_mut();
            let id = WindowId(*n);
            *n += 1;
            self.running.borrow_mut().insert(k, id);
            Ok(Some(id))
        } else {
            *seen += 1;
            Ok(None)
        }
    }

    fn place(&self, win: WindowId, rect: PixelRect) -> Result<()> {
        let name = self
            .running
            .borrow()
            .iter()
            .find(|(_, v)| **v == win)
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| format!("win{}", win.0));
        self.log.borrow_mut().push(Action::Place(name, rect));
        Ok(())
    }

    fn focus(&self, win: WindowId) -> Result<()> {
        let name = self
            .running
            .borrow()
            .iter()
            .find(|(_, v)| **v == win)
            .map(|(k, _)| k.clone())
            .unwrap_or_else(|| format!("win{}", win.0));
        self.log.borrow_mut().push(Action::Focus(name));
        // As Windows does: the window comes to the front, unless it has been
        // set to refuse (a window running as administrator, say).
        if !*self.focus_refused.borrow() {
            *self.front.borrow_mut() = Some(win);
        }
        Ok(())
    }

    fn close(&self, spec: &AppSpec) -> Result<()> {
        let k = Self::key(spec);
        self.running.borrow_mut().remove(&k);
        self.polls.borrow_mut().remove(&k);
        self.launched.borrow_mut().remove(&k);
        self.log.borrow_mut().push(Action::Close(k));
        Ok(())
    }

    fn sleep_ms(&self, ms: u64) {
        self.log.borrow_mut().push(Action::Sleep(ms));
    }

    fn click(&self, x: i32, y: i32, button: Button) -> Result<()> {
        self.log.borrow_mut().push(Action::Click(x, y, button));
        Ok(())
    }
    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        self.log.borrow_mut().push(Action::Scroll(dx, dy));
        Ok(())
    }
    fn type_text(&self, text: &str) -> Result<()> {
        self.log.borrow_mut().push(Action::Type(text.to_string()));
        self.box_about_to_change();
        if let Some(b) = self.typing_box.borrow_mut().as_mut() {
            b.push_str(text);
            return Ok(());
        }
        // What's typed shows in the window in front — or, garbling, the last
        // character over and over.
        if let Some(front) = *self.front.borrow() {
            if let Some(node) = self.screens.borrow_mut().get_mut(&front.0) {
                let shown = if *self.garbles.borrow() {
                    text.chars().last().map(|c| c.to_string().repeat(text.chars().count())).unwrap_or_default()
                } else {
                    text.to_string()
                };
                node.value.push('\n');
                node.value.push_str(&shown);
            }
        }
        Ok(())
    }
    fn press(&self, combo: &str) -> Result<()> {
        self.log.borrow_mut().push(Action::Press(combo.to_string()));
        if combo.eq_ignore_ascii_case("ctrl+c") {
            if let Some(t) = self.copy_gives.borrow_mut().take() {
                *self.clipboard.borrow_mut() = Some(t);
            }
        }
        if combo == "backspace" {
            self.box_about_to_change();
            if let Some(b) = self.typing_box.borrow_mut().as_mut() {
                b.pop();
            }
        }
        Ok(())
    }

    fn read_clipboard(&self) -> Result<Option<String>> {
        Ok(self.clipboard.borrow().clone())
    }
    fn open_path(&self, path: &str) -> Result<()> {
        self.log.borrow_mut().push(Action::Type(format!("open:{path}")));
        Ok(())
    }
    fn clipboard_change(&self) -> Option<u32> {
        *self.clip_seq.borrow()
    }
    fn clipboard_copy(&self) -> Option<super::ClipCopy> {
        self.clip_copy.borrow().clone()
    }
    fn grab_window(&self) -> Result<Option<super::Grab>> {
        Ok(self.grab.borrow().clone())
    }
    fn recognise_text(&self, grab: &super::Grab) -> Result<Option<String>> {
        if let Some(t) = self.ocr_by_title.borrow().get(&grab.title) {
            return Ok(Some(t.clone()));
        }
        Ok(self.ocr.borrow().clone())
    }
    fn grab_screen(&self, monitor: u32) -> Result<Option<super::Grab>> {
        if !*self.screen_pictures.borrow() {
            return Ok(None);
        }
        let Some(m) = self.monitors.iter().find(|m| m.id == monitor) else { return Ok(None) };
        let (w, h) = (m.width.max(1) as u32, m.height.max(1) as u32);
        let c = [(monitor * 40 % 256) as u8, (monitor * 90 % 256) as u8, (monitor * 150 % 256) as u8];
        let rgb = c.iter().copied().cycle().take((w * h * 3) as usize).collect();
        Ok(Some(super::Grab { width: w, height: h, rgb, title: format!("screen {monitor}") }))
    }
    fn active_monitor(&self) -> Option<u32> {
        *self.active_screen.borrow()
    }
    fn rect_of(&self, win: WindowId) -> Result<PixelRect> {
        self.window_rects
            .borrow()
            .get(&win.0)
            .copied()
            .ok_or_else(|| crate::error::AtlasError::Platform("no such window".into()))
    }
    fn built_in_monitor(&self) -> Option<u32> {
        *self.laptop_screen.borrow()
    }
    fn recognise_image_file(&self, _path: &str) -> Result<Option<String>> {
        Ok(self.ocr_file.borrow().clone())
    }
    fn write_clipboard(&self, text: &str) -> Result<()> {
        *self.clipboard.borrow_mut() = Some(text.to_string());
        Ok(())
    }
}
