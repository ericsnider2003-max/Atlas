//! Atlas's settings, in Atlas's own window.
//!
//! Eric, 23 Sep 2026: *"I still don't know how to access the hub to be able to
//! get to my settings for Atlas."* On the laptop there was no way a person
//! would find: the hub is a web page reached by pasting a printed address with
//! a token in it, and by Eric's own ruling (17 Sep) the desktop surface is not
//! a browser — while the native settings panel was a placeholder that said
//! "Settings are up." and listed nothing.
//!
//! This is the real one. Every setting in `settings::registry` — the same list
//! the hub renders — grouped the same way, each with what it does and what it
//! costs, with a control that fits it: a switch, a number in its range, a
//! choice, a name. A change is checked by `Settings::set` (which knows a
//! toggle from a number) and kept in `config/settings.yaml` through
//! `Preferences` — the very path the hub's own form takes, so the two can
//! never disagree about what a setting is. Changes that widen what Atlas may
//! touch or see ask once before they're kept. Anything changed can be put
//! back. A running Atlas picks most changes up within seconds; the few that
//! set something up at the start (`settings::NEEDS_A_RESTART`) wait for a
//! restart, and only then does the page offer one.
//!
//! Reached from the Atlas window's Settings button, from the Start-menu
//! shortcut (`atlas home settings`), and by saying "show me settings".

use crate::settings::{Setting, Settings, Value};
use std::path::Path;
#[cfg(feature = "desktop-ui")]
use std::path::PathBuf;

/// Check a change against the settings list and keep it. The same two steps
/// the hub's form takes: validate with `Settings::set`, then write through
/// `Preferences`.
pub fn keep_setting(config_dir: &Path, key: &str, raw: &str) -> Result<String, String> {
    let mut settings = current_settings(config_dir)?;
    let said = settings.set(key, raw)?;
    let mut prefs = crate::preferences::Preferences::load_checked(config_dir)
        .map_err(|e| format!("I haven't changed anything: {e}. Fix that file or delete it, then try again."))?;
    prefs.set(key, raw);
    prefs.save(config_dir).map_err(|e| format!("I couldn't keep that change: {e}"))?;
    Ok(said)
}

/// What a change said, and when it takes hold. A running Atlas picks most
/// changes up within seconds (`Daemon::pick_up_settings`); the few that set
/// something up at the start wait for a restart, and say so.
pub fn when_it_applies(key: &str, said: &str) -> String {
    if crate::settings::needs_a_restart(key) {
        format!("{said}. That one takes effect when Atlas restarts.")
    } else {
        format!("{said}. Atlas picks it up straight away.")
    }
}

/// Put one setting back to what Atlas ships with.
pub fn put_back(config_dir: &Path, key: &str) -> Result<String, String> {
    let mut settings = current_settings(config_dir)?;
    let said = settings.reset(key)?;
    let mut prefs = crate::preferences::Preferences::load_checked(config_dir)
        .map_err(|e| format!("I haven't changed anything: {e}. Fix that file or delete it, then try again."))?;
    prefs.clear(key);
    prefs.save(config_dir).map_err(|e| format!("I couldn't put that back: {e}"))?;
    Ok(said)
}

/// The settings as they stand now: shipped values with your changes on top.
pub fn current_settings(config_dir: &Path) -> Result<Settings, String> {
    let cfg = crate::config::Config::load(config_dir).map_err(|e| format!("I couldn't read my settings: {e}"))?;
    Ok(crate::settings::registry(&cfg.tools.unwrap_or_default()))
}

/// The raw text a control produces for a value, in the form `Settings::set`
/// reads back.
pub fn raw_of(v: &Value) -> String {
    match v {
        Value::Toggle(b) => if *b { "on".into() } else { "off".into() },
        Value::Number { value, .. } => format!("{value}"),
        Value::Text(s) | Value::Choice { value: s, .. } => s.clone(),
        Value::List(items) => items.join(", "),
    }
}

/// A change waiting for "yes" because it widens what Atlas may do.
#[derive(Debug, Clone, PartialEq)]
pub struct Asking {
    pub key: String,
    pub name: String,
    pub raw: String,
    pub why: String,
}

/// Does this change need asking about? Only when it turns something on or
/// widens it — turning a sensor or a permission *off* never needs a yes.
pub fn needs_a_yes(s: &Setting, raw: &str) -> bool {
    if !s.weight.needs_confirming() {
        return false;
    }
    match s.value {
        Value::Toggle(_) => matches!(raw.trim().to_lowercase().as_str(), "on" | "true" | "yes" | "1"),
        _ => true,
    }
}

/// Why a change is being asked about, in a sentence.
pub fn why_ask(w: crate::settings::Weight) -> &'static str {
    match w {
        crate::settings::Weight::Sensitive => {
            "This turns on a sensor or reaches outside this machine. Keep it?"
        }
        crate::settings::Weight::Permission => "This changes what Atlas may do without asking you. Keep it?",
        _ => "Keep it?",
    }
}

#[cfg(feature = "desktop-ui")]
/// The page's state inside the Atlas window.
pub struct Page {
    config_dir: PathBuf,
    settings: Option<Settings>,
    /// Text being typed into a name or list, per key, until it's kept.
    drafts: std::collections::HashMap<String, String>,
    asking: Option<Asking>,
    said: Vec<String>,
    changed_since_start: bool,
    error: Option<String>,
    /// A key setting waiting for you to press the key.
    capturing: Option<String>,
}

#[cfg(feature = "desktop-ui")]
/// What the page asks the window to do.
pub enum Ask {
    Restart,
}

#[cfg(feature = "desktop-ui")]
impl Page {
    pub fn new(config_dir: PathBuf) -> Page {
        let mut p = Page {
            config_dir,
            settings: None,
            drafts: Default::default(),
            asking: None,
            said: Vec::new(),
            changed_since_start: false,
            error: None,
            capturing: None,
        };
        p.reload();
        p
    }

    fn reload(&mut self) {
        match current_settings(&self.config_dir) {
            Ok(s) => {
                self.settings = Some(s);
                self.error = None;
            }
            Err(e) => self.error = Some(e),
        }
    }

    fn change(&mut self, key: &str, raw: String) {
        let Some(s) = self.settings.as_ref().and_then(|all| all.get(key)).cloned() else { return };
        if needs_a_yes(&s, &raw) {
            self.asking = Some(Asking {
                key: key.to_string(),
                name: s.name.clone(),
                raw,
                why: why_ask(s.weight).into(),
            });
            return;
        }
        self.keep(key, &raw);
    }

    fn keep(&mut self, key: &str, raw: &str) {
        match keep_setting(&self.config_dir, key, raw) {
            Ok(said) => {
                self.said.push(when_it_applies(key, &said));
                self.changed_since_start |= crate::settings::needs_a_restart(key);
            }
            Err(e) => self.said.push(e),
        }
        self.drafts.remove(key);
        self.reload();
    }

    /// Draw it. `running` is whether the background Atlas is up, for the
    /// restart offer. Only in the desktop build: everything else in this
    /// module (keeping a setting, reading them back) is used by the hub too,
    /// and is in the GUI-free core as well.
    pub fn show(&mut self, ui: &mut eframe::egui::Ui, running: bool) -> Option<Ask> {

        use eframe::egui::{self, FontId, RichText};
        let mut ask = None;

        ui.label(RichText::new("Settings").font(FontId::proportional(26.0)).strong().color(crate::look_paint::colourway().text));
        ui.label(
            RichText::new("Everything here is kept the moment you change it, and most of it applies straight away.")
                .font(FontId::proportional(13.0))
                .color(crate::look_paint::colourway().soft),
        );
        ui.add_space(8.0);

        if let Some(e) = &self.error {
            ui.label(RichText::new(e).color(crate::look_paint::colourway().warn));
            return None;
        }

        // The confirm step, above everything so it can't be missed.
        if let Some(a) = self.asking.clone() {
            egui::Frame::none().fill(crate::look_paint::colourway().raised).inner_margin(egui::Margin::same(12.0)).show(ui, |ui| {
                ui.label(RichText::new(format!("{}: {}", a.name, a.raw)).font(FontId::proportional(16.0)).color(crate::look_paint::colourway().text));
                ui.label(RichText::new(&a.why).font(FontId::proportional(13.0)).color(crate::look_paint::colourway().warn));
                ui.horizontal(|ui| {
                    if ui.button("Yes, keep it").clicked() {
                        self.asking = None;
                        self.keep(&a.key, &a.raw);
                    }
                    if ui.button("No, leave it").clicked() {
                        self.asking = None;
                        self.reload();
                    }
                });
            });
            ui.add_space(8.0);
        }

        if self.changed_since_start {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(if running {
                        "Some of your changes take effect when Atlas restarts."
                    } else {
                        "Your changes take effect when Atlas starts."
                    })
                    .color(crate::look_paint::colourway().signal_text),
                );
                if running && ui.button("Restart Atlas now").clicked() {
                    ask = Some(Ask::Restart);
                    self.changed_since_start = false;
                }
            });
            ui.add_space(4.0);
        }
        for line in self.said.iter().rev().take(3) {
            ui.label(RichText::new(line).font(FontId::proportional(13.0)).color(crate::look_paint::colourway().soft));
        }

        // A key being set by pressing it: the next key you press is it.
        if let Some(which) = self.capturing.clone() {
            let events = ui.ctx().input(|i| i.events.clone());
            if let Some(result) = key_pressed(&which, &events) {
                self.capturing = None;
                match result {
                    Some(spec) => {
                        self.drafts.remove(&which);
                        self.change(&which, spec);
                    }
                    None => self.said.push("Left the key as it was.".into()),
                }
            }
        }

        let Some(settings) = self.settings.clone() else { return ask };
        let mut wanted: Vec<(String, String)> = Vec::new();
        let mut back: Option<String> = None;
        for group in settings.groups() {
            ui.add_space(14.0);
            ui.label(RichText::new(&group).font(FontId::proportional(18.0)).color(crate::look_paint::colourway().text));
            if let Some(note) = Settings::group_note(&group) {
                ui.label(RichText::new(note).font(FontId::proportional(12.0)).color(crate::look_paint::colourway().dim));
            }
            for s in settings.in_group(&group) {
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&s.name).font(FontId::proportional(15.0)).color(crate::look_paint::colourway().text));
                    if s.weight.needs_confirming() {
                        ui.label(RichText::new(s.weight.label()).font(FontId::proportional(11.0)).color(crate::look_paint::colourway().warn));
                    }
                });
                ui.label(RichText::new(&s.what).font(FontId::proportional(12.0)).color(crate::look_paint::colourway().soft));
                if !s.cost.trim().is_empty() {
                    ui.label(RichText::new(&s.cost).font(FontId::proportional(11.0)).color(crate::look_paint::colourway().dim));
                }
                ui.horizontal_wrapped(|ui| {
                    match &s.value {
                        Value::Toggle(on) => {
                            let mut v = *on;
                            let label = if v { "On" } else { "Off" };
                            if ui.checkbox(&mut v, label).changed() {
                                wanted.push((s.key.clone(), if v { "on".into() } else { "off".into() }));
                            }
                        }
                        Value::Number { value, min, max } => {
                            let mut v = *value;
                            let r = ui.add(egui::DragValue::new(&mut v).range(*min..=*max).speed(((max - min) / 200.0).max(0.01)));
                            if (r.drag_stopped() || r.lost_focus()) && (v - value).abs() > 1e-9 {
                                wanted.push((s.key.clone(), format!("{v}")));
                            }
                        }
                        Value::Choice { value, options } => {
                            let mut v = value.clone();
                            egui::ComboBox::from_id_source(&s.key).selected_text(v.clone()).show_ui(ui, |ui| {
                                for o in options {
                                    ui.selectable_value(&mut v, o.clone(), o);
                                }
                            });
                            if &v != value {
                                wanted.push((s.key.clone(), v));
                            }
                        }
                        Value::Text(_) if KEY_SETTINGS.contains(&s.key.as_str()) => {
                            let current = raw_of(&s.value);
                            if self.capturing.as_deref() == Some(s.key.as_str()) {
                                ui.label(RichText::new("Press the key now… (Esc to leave it)").color(crate::look_paint::colourway().signal_text));
                            } else {
                                ui.label(RichText::new(pretty_key(&current)).font(FontId::monospace(15.0)).color(crate::look_paint::colourway().text));
                                if ui.button("Set by pressing").clicked() {
                                    self.capturing = Some(s.key.clone());
                                }
                                let draft = self.drafts.entry(s.key.clone()).or_insert_with(|| current.clone());
                                let r = ui.add(egui::TextEdit::singleline(draft).desired_width(160.0).hint_text("or type it"));
                                if r.lost_focus() && *draft != current {
                                    wanted.push((s.key.clone(), draft.clone()));
                                }
                            }
                        }
                        Value::Text(_) | Value::List(_) => {
                            let current = raw_of(&s.value);
                            let draft = self.drafts.entry(s.key.clone()).or_insert_with(|| current.clone());
                            let r = ui.add(egui::TextEdit::singleline(draft).desired_width(260.0));
                            if r.lost_focus() && *draft != current {
                                wanted.push((s.key.clone(), draft.clone()));
                            }
                        }
                    }
                    if s.changed() && ui.small_button("Put back").clicked() {
                        back = Some(s.key.clone());
                    }
                });
            }
        }
        for (key, raw) in wanted {
            self.change(&key, raw);
        }
        if let Some(key) = back {
            match put_back(&self.config_dir, &key) {
                Ok(said) => {
                    self.said.push(when_it_applies(&key, &said));
                    self.changed_since_start |= crate::settings::needs_a_restart(&key);
                }
                Err(e) => self.said.push(e),
            }
            self.drafts.remove(&key);
            self.reload();
        }
        ask
    }
}

/// The settings that are a key, set by pressing it.
pub const KEY_SETTINGS: &[&str] = &["push_to_talk.key", "quick_input.hotkey"];

/// "ctrl+shift+space" as it's shown: "Ctrl + Shift + Space".
pub fn pretty_key(spec: &str) -> String {
    spec.split('+').map(crate::hotkeys::key_word).collect::<Vec<_>>().join(" + ")
}

/// The key a person just pressed, as a setting for `which`. `Some(None)` is
/// Escape (leave it); `None` is nothing pressed yet. Push-to-talk takes the
/// key alone; the typing box takes the key with whatever was held.
#[cfg(feature = "desktop-ui")]
pub fn key_pressed(which: &str, events: &[eframe::egui::Event]) -> Option<Option<String>> {
    use eframe::egui::{Event, Key};
    for e in events {
        if let Event::Key { key, pressed: true, modifiers, .. } = e {
            if *key == Key::Escape {
                return Some(None);
            }
            let name = key.name().to_lowercase().replace(' ', "");
            let spec = if which == "push_to_talk.key" {
                crate::hotkeys::spec_of(false, false, false, false, &name)
            } else {
                crate::hotkeys::spec_of(modifiers.ctrl, modifiers.shift, modifiers.alt, false, &name)
            };
            if let Some(s) = spec {
                return Some(Some(s));
            }
        }
    }
    None
}
