//! Atlas tests itself on your machine (30 Sep 2026).
//!
//! Eric: "I need Atlas to test all of this stuff so I know what does and
//! doesn't work instead of just waiting for shit to go wrong."
//!
//! `atlas selftest` (or "test everything") asks Atlas for every command it
//! has, the way it would be said, plus a list of everyday sentences with the
//! tool each should reach, through the front door (`Daemon::turn`) -- on
//! this machine, with its real settings, its real model and its real
//! screens -- and writes what happened to `data/selftest/`.
//!
//! ## Why it can't do damage
//!
//! * It runs in a child process whose install folder is a **scratch copy**
//!   (`ATLAS_HOME`): your settings and state are copied in, the models and
//!   tools folders are linked so what's installed reads as installed. Every
//!   write -- notes, the vault, the handover, backups -- lands in the copy.
//! * The screen is a **stand-in that records** (`SafePlatform`): Atlas reads
//!   your windows and screens for real, and every click, keystroke, launch,
//!   move or close is written down as "would have" instead of done.
//! * Nothing is **approved**: a command that asks "Go ahead?" is reported as
//!   asking, and left there.
//! * Background work isn't started (`Daemon::rehearsal`), and commands that
//!   touch real files, the network or your accounts are **rehearsed**: routed,
//!   checked and reported as what they would do, not done (`tier`).
//!
//! So the report says, for every command: it works, it's switched off (and
//! which switch), it needs something installed, it asks first, it asked you
//! for more, it reached the wrong tool, it's slow, or it's broken and how.

use crate::config::AppSpec;
use crate::error::Result;
use crate::platform::{ActiveWindow, Button, ClipCopy, Grab, Monitor, OsQuiet, PixelRect, Platform, WindowId};
use serde::Serialize;
use std::cell::RefCell;
use std::path::{Path, PathBuf};

/// How far a command is taken in the test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// It only reads and answers: run in full.
    Run,
    /// It acts on the screen only: run in full, the actions recorded rather
    /// than done (`SafePlatform`).
    StandIn,
    /// It touches real files, the network, accounts or Atlas's own install:
    /// routed and checked, reported as what it would do, not done.
    Rehearse,
}

/// The tier of a command, by its kind (`session::kind_of`). Anything not
/// named here is rehearsed: the safe default for a command added later.
pub fn tier(kind: &str) -> Tier {
    const RUN: &[&str] = &[
        "capabilities", "clock", "what_i_have", "time_spent", "how_am_i_doing", "model_trace", "machine_health",
        "self_check", "shakedown", "which_model", "outstanding", "queued", "history", "recap", "act_alone",
        "knowledge_size", "why", "languages", "market_day", "agenda", "messages", "people", "waiting_for",
        "note_review", "dangling", "recommend", "plain_change", "overnight", "money_advice", "creator_advice",
        "translate", "diagnose", "walk_through", "updates", "feedback", "phone_model", "who_is_in", "clip_history",
        "ready", "say", "ask", "unknown", "capture_webcam", "got_it_wrong", "apply_lesson", "rehearse", "ask_the_room",
        "whats_there", "whats_this", "find_file", "habit", "cards", "goals", "later", "trade_day",
        "meeting_prep", "explain_code", "animate", "scene3d", "design_review", "make_picture", "research",
        "read_document", "booking", "schedule", "brief_on", "opportunities", "social", "feeds", "receipt",
        "snippet", "teach_gesture", "set_key", "this_is_me", "name_this", "address_as", "wit", "mute_topic",
        "suggestions", "mail", "review_post", "travel_prep", "edit_media",
        "after_me",
        "drop_task", "files",
        "learn_knowledge", "two_factor", "type_code", "create_account", "sign_in",
        "message", "pair", "accept_pairing", "forget_peer", "name_group", "leave_group", "draft_post",
        "schedule_post", "rebuild_index", "use_mic", "finish_setup",
    ];
    const STAND_IN: &[&str] = &[
        "open_app", "close_app", "focus_app", "workspace_on", "workspace_off", "view_display", "set_mode",
        "show_panel", "dismiss_panel", "pause", "resume", "press_button", "use_clipboard", "screen_text", "launch",
        "operate",
    ];
    // Never run, whatever else is true: they change Atlas's own install,
    // which the scratch copy can't fully hold (the program file itself).
    // Nor what writes your own files, records you, or types for you on its
    // own thread: rehearsed, so a test can't move, convert or capture
    // anything real whatever its settings point at.
    const NEVER: &[&str] = &[
        "self_test", "hand_over", "take_it_back", "unlock", "back_up", "undo", "sync", "capture", "refile", "files", "unzip",
        "edit_photo", "edit_media", "move_big_files", "tidy_desktop", "pdf", "receipt", "finish_setup",
        "work_on_yourself", "implement", "improve", "build_it", "keep_at_it", "delegate", "sort_mail", "dictate",
        "gestures", "call_notes",
    ];
    if NEVER.contains(&kind) {
        Tier::Rehearse
    } else if STAND_IN.contains(&kind) {
        Tier::StandIn
    } else if RUN.contains(&kind) {
        // Run up to the point where something would leave the machine or
        // change your files: that point asks first (policy), or is
        // background work (rehearsed), or is a crew errand (rehearsed).
        Tier::Run
    } else {
        Tier::Rehearse
    }
}

/// The real platform, reading for real and writing nothing: every action is
/// recorded as "would have".
pub struct SafePlatform<'a> {
    real: &'a dyn Platform,
    did: RefCell<Vec<String>>,
}

impl<'a> SafePlatform<'a> {
    pub fn wrapping(real: &'a dyn Platform) -> SafePlatform<'a> {
        SafePlatform { real, did: RefCell::new(Vec::new()) }
    }
    /// What was asked of the screen since the last look, emptied.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut *self.did.borrow_mut())
    }
    fn note(&self, what: String) -> Result<()> {
        self.did.borrow_mut().push(what);
        Ok(())
    }
}

impl Platform for SafePlatform<'_> {
    fn active_window(&self) -> Result<Option<ActiveWindow>> {
        self.real.active_window()
    }
    fn active_window_id(&self) -> Result<Option<WindowId>> {
        self.real.active_window_id()
    }
    fn input_idle_secs(&self) -> Option<u64> {
        self.real.input_idle_secs()
    }
    fn quiet_state(&self) -> Option<OsQuiet> {
        self.real.quiet_state()
    }
    fn monitors(&self) -> Result<Vec<Monitor>> {
        self.real.monitors()
    }
    fn built_in_screen_on(&self) -> Option<bool> {
        self.real.built_in_screen_on()
    }
    fn launch(&self, spec: &AppSpec) -> Result<()> {
        self.note(format!("start {}", spec.launch))
    }
    fn find_window(&self, spec: &AppSpec) -> Result<Option<WindowId>> {
        self.real.find_window(spec)
    }
    fn place(&self, win: WindowId, rect: PixelRect) -> Result<()> {
        self.note(format!("move window {} to {}x{} at {},{}", win.0, rect.width, rect.height, rect.x, rect.y))
    }
    fn focus(&self, win: WindowId) -> Result<()> {
        self.note(format!("bring window {} forward", win.0))
    }
    fn close(&self, spec: &AppSpec) -> Result<()> {
        self.note(format!("close {}", spec.launch))
    }
    fn sleep_ms(&self, _ms: u64) {}
    fn click(&self, x: i32, y: i32, button: Button) -> Result<()> {
        self.note(format!("{button:?} click at {x},{y}"))
    }
    fn scroll(&self, dx: i32, dy: i32) -> Result<()> {
        self.note(format!("scroll {dx},{dy}"))
    }
    fn type_text(&self, text: &str) -> Result<()> {
        self.note(format!("type {} characters", text.chars().count()))
    }
    fn press(&self, combo: &str) -> Result<()> {
        self.note(format!("press {combo}"))
    }
    fn read_window(&self, win: WindowId) -> Result<Option<crate::uia::Node>> {
        self.real.read_window(win)
    }
    fn press_named(&self, _win: WindowId, name: &str) -> Result<bool> {
        self.note(format!("press the {name:?} button"))?;
        Ok(true)
    }
    fn focused_text(&self) -> Result<Option<String>> {
        self.real.focused_text()
    }
    fn focused_is_editable(&self) -> Result<Option<bool>> {
        self.real.focused_is_editable()
    }
    fn session_locked(&self) -> Option<bool> {
        self.real.session_locked()
    }
    fn draw_overlay(&self, _elements: &[crate::overlay::Element]) -> Result<()> {
        Ok(())
    }
    fn move_cursor(&self, x: i32, y: i32) -> Result<()> {
        self.note(format!("move the pointer to {x},{y}"))
    }
    fn window_at(&self, x: i32, y: i32) -> Result<Option<WindowId>> {
        self.real.window_at(x, y)
    }
    fn rect_of(&self, win: WindowId) -> Result<PixelRect> {
        self.real.rect_of(win)
    }
    fn cursor(&self) -> Result<(i32, i32)> {
        self.real.cursor()
    }
    fn clipboard_change(&self) -> Option<u32> {
        self.real.clipboard_change()
    }
    fn clipboard_copy(&self) -> Option<ClipCopy> {
        self.real.clipboard_copy()
    }
    fn grab_window(&self) -> Result<Option<Grab>> {
        self.real.grab_window()
    }
    fn grab_screen(&self, monitor: u32) -> Result<Option<Grab>> {
        self.real.grab_screen(monitor)
    }
    fn monitor_bounds(&self, monitor: u32) -> Option<PixelRect> {
        self.real.monitor_bounds(monitor)
    }
    fn active_monitor(&self) -> Option<u32> {
        self.real.active_monitor()
    }
    fn built_in_monitor(&self) -> Option<u32> {
        self.real.built_in_monitor()
    }
    fn recognise_text(&self, grab: &Grab) -> Result<Option<String>> {
        self.real.recognise_text(grab)
    }
    fn recognise_image_file(&self, path: &str) -> Result<Option<String>> {
        self.real.recognise_image_file(path)
    }
    fn open_path(&self, path: &str) -> Result<()> {
        self.note(format!("open {path}"))
    }
    fn read_clipboard(&self) -> Result<Option<String>> {
        self.real.read_clipboard()
    }
    fn write_clipboard(&self, text: &str) -> Result<()> {
        self.note(format!("put {} characters on the clipboard", text.chars().count()))
    }
}

/// What happened to one command.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case", tag = "is", content = "why")]
pub enum Verdict {
    Works,
    /// It asked "go ahead?" -- left there; nothing is approved in a test.
    AsksFirst,
    /// It asked which or what: the sentence wasn't enough on its own.
    AsksForMore,
    SwitchedOff,
    NeedsInstalling,
    NeedsSettingUp,
    /// Background work it would have started.
    WouldStart,
    /// Routed and checked, not done (`Tier::Rehearse`).
    Rehearsed,
    WrongTool(String),
    /// An everyday sentence the phrases don't place, tested with no model.
    NeedsTheModel,
    Slow(u64),
    Broken(String),
}

impl Verdict {
    pub fn plain(&self) -> String {
        match self {
            Verdict::Works => "works".into(),
            Verdict::AsksFirst => "asks you first".into(),
            Verdict::AsksForMore => "asks for more".into(),
            Verdict::SwitchedOff => "switched off".into(),
            Verdict::NeedsInstalling => "needs something installed".into(),
            Verdict::NeedsSettingUp => "needs setting up".into(),
            Verdict::WouldStart => "would start in the background".into(),
            Verdict::Rehearsed => "rehearsed (not done in a test)".into(),
            Verdict::WrongTool(t) => format!("WRONG TOOL: reached {t}"),
            Verdict::NeedsTheModel => "needs the model (tested without it)".into(),
            Verdict::Slow(ms) => format!("SLOW: {:.1} s", *ms as f64 / 1000.0),
            Verdict::Broken(why) => format!("BROKEN: {why}"),
        }
    }
    /// Something to fix, as opposed to a state you can change yourself.
    pub fn is_a_fault(&self) -> bool {
        matches!(self, Verdict::WrongTool(_) | Verdict::Slow(_) | Verdict::Broken(_))
    }
}

/// The ways a reply is broken, if this one is: what the capability sweep
/// (`tests/every_ability_answers.rs`) checks, here so the machine checks it too.
fn reply_fault(reply: &str) -> Option<&'static str> {
    let r = reply.trim();
    let l = r.to_lowercase();
    if r.is_empty() {
        return Some("said nothing");
    }
    for leak in ["Some(", "None)", "Ok(", "Err(", "Intent::", "{:?}", "\\n", "PathBuf", "AtlasError", "panicked", "unwrap()"] {
        if r.contains(leak) {
            return Some("code in the words");
        }
    }
    for slip in ["? now", "platform:", " a the ", " with  "] {
        if l.contains(slip) {
            return Some("a slip in the words");
        }
    }
    if !r.contains('\n') && r.contains("  ") {
        return Some("a slip in the words");
    }
    for stub in ["not built yet", "isn't built", "not implemented", "todo!", "unimplemented", "coming soon"] {
        if l.contains(stub) {
            return Some("says it isn't built");
        }
    }
    if l.contains("couldn't get an answer from the language model") || l.contains("didn't get a usable answer") {
        return Some("the model didn't answer");
    }
    if l.contains("thinking about what to do next") && l.contains("went wrong") {
        return Some("crashed");
    }
    None
}

/// What one reply says about the command, from its words, the time it took,
/// where it was routed and where it should have gone.
fn judge_reply(reply: &str, ms: u64, reached: &str, expected: Option<&str>, would_start: bool) -> Verdict {
    if let Some(want) = expected {
        if reached != want {
            return Verdict::WrongTool(format!("{reached} (wanted {want})"));
        }
    }
    if let Some(why) = reply_fault(reply) {
        return Verdict::Broken(why.into());
    }
    let l = reply.to_lowercase();
    // What it says about itself is in its first sentence: "57 things work
    // ... 22 are switched off" is an answer, not a switch that's off.
    let first: String = {
        let end = l.find(". ").map(|i| i + 1).unwrap_or(l.len());
        l[..end].to_string()
    };
    let l_all = l.clone();
    let l = first;
    if l_all.starts_with("[rehearsed]") {
        return Verdict::Rehearsed;
    }
    if l.contains("switched off") || l.contains(" is off") || l.contains("it's off") || l.contains("turn on ") {
        return Verdict::SwitchedOff;
    }
    if l.contains("download") || (l.contains("i don't have") && l.contains("yet")) || l.contains("isn't installed") || l.contains("not installed") {
        return Verdict::NeedsInstalling;
    }
    if l.contains("isn't set up") || l.contains("is set up") || l.contains("no mailbox") || l.contains("none is configured") || l.contains("haven't got one configured") {
        return Verdict::NeedsSettingUp;
    }
    if l_all.trim_end().ends_with("go ahead?") || l.starts_with("allow ") || l_all.contains("say yes to allow") {
        return Verdict::AsksFirst;
    }
    // A model answering a sentence takes seconds on a laptop; a command
    // answered without one shouldn't.
    if ms > 15_000 || (ms > 3_000 && reached != "unknown" && reached != "say" && reached != "ask") {
        return Verdict::Slow(ms);
    }
    if would_start {
        return Verdict::WouldStart;
    }
    if l_all.trim_end().ends_with('?') {
        return Verdict::AsksForMore;
    }
    Verdict::Works
}

/// One line of the report.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    pub command: String,
    pub said: String,
    pub reached: String,
    pub tier: Tier,
    pub verdict: Verdict,
    pub reply: String,
    pub ms: u64,
    /// What the screen was asked to do, and what background work it would
    /// have started.
    pub would: Vec<String>,
}

/// Everyday sentences, and the tool each should reach: the ones Eric said,
/// and the same things said without the commands' own words.
pub const EVERYDAY: &[(&str, &str)] = &[
    ("I guess I want you to organize my desktop.", "tidy_desktop"),
    ("look at my screen and tell me what's on it", "view_display"),
    ("I want you to go and do a diagnosis on yourself.", "self_check"),
    ("Please use my camera and look at me.", "capture_webcam"),
    ("what's on my calendar tomorrow", "agenda"),
    ("find the tax pdf from last year", "find_file"),
    ("my laptop is running slow, what's eating the memory", "machine_health"),
    ("how much space have I got left on this thing", "machine_health"),
    ("anything new come in by email", "mail"),
    ("could you find me some freelance gigs", "opportunities"),
    ("how did my last youtube video do", "social"),
    ("open chrome", "open_app"),
    ("note that the plumber comes on thursday", "capture"),
    ("what time is it", "clock"),
    ("draw me a lighthouse at dusk", "make_picture"),
    ("what can you do", "capabilities"),
];

/// Every command's sentence, and the everyday ones.
pub fn sentences(book: &crate::intent::ToolBook) -> Vec<(String, String, Option<String>)> {
    let mut out = Vec::new();
    for e in book.entries() {
        if e.describe.starts_with("Internal:") {
            continue;
        }
        let Some(phrase) = e.phrases.iter().find(|p| !p.trim().is_empty()) else { continue };
        let said = if e.takes_arg && !e.arg_optional { format!("{phrase} the quarterly budget") } else { phrase.clone() };
        out.push((e.name.clone(), said, None));
    }
    for (said, want) in EVERYDAY {
        // Compared by kind (`session::kind_of`): a command's name and its
        // kind aren't always the same word (whats_there is capture_webcam).
        let kind = crate::intent::from_tool(want, &serde_json::json!({ "arg": "x" }), said)
            .map(|i| crate::session::kind_of(&i).to_string())
            .unwrap_or_else(|| want.to_string());
        out.push((format!("everyday: {want}"), said.to_string(), Some(kind)));
    }
    out
}

/// Every sentence through the front door of a fresh Atlas each (so one
/// command's leftovers never decide another's), on `plat`.
pub fn run_all(
    cfg: &crate::config::Config,
    plat: &SafePlatform,
    llm: Option<std::sync::Arc<dyn crate::brain::Llm>>,
    store_dir: &Path,
    t0: u64,
    on_row: &mut dyn FnMut(&Row),
) -> Vec<Row> {
    let book = crate::intent::ToolBook::new(&cfg.commands);
    let parser = crate::intent::Parser::new(&cfg.commands);
    let mut rows = Vec::new();
    for (i, (command, said, expected)) in sentences(&book).into_iter().enumerate() {
        let mut d = crate::daemon::Daemon::new(
            cfg,
            plat,
            llm.clone(),
            crate::store::Store::new(store_dir.to_path_buf()),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        d.rehearsal = true;
        let parsed = crate::session::kind_of(&parser.parse(&said)).to_string();
        let _ = plat.take();
        let started = std::time::Instant::now();
        let reply = match crate::crash::caught("a self-test sentence", || d.turn(&said, t0 + i as u64 * 60)) {
            Ok(r) => r,
            Err(why) => format!("thinking about what to do next went wrong: {why}"),
        };
        let ms = started.elapsed().as_millis() as u64;
        // Where it went: the model's choice when the phrases didn't place it.
        let reached = d.last_reached().unwrap_or_else(|| parsed.clone());
        // Placed by the phrases as wanted and carried out by the command that
        // does it (the camera's "what do you see" runs as capture_webcam):
        // right either way.
        let expected = expected.map(|e| if parsed == e { reached.clone() } else { e });
        let mut would = plat.take();
        let started_bg = !d.rehearsed.is_empty() && d.rehearsed.iter().any(|r| r.starts_with("start "));
        would.extend(std::mem::take(&mut d.rehearsed));
        let k = crate::session::kind_of(&parser.parse(&said));
        let tier = tier(k);
        let mut v = judge_reply(&reply, ms, &reached, expected.as_deref(), started_bg);
        if llm.is_none() && expected.is_some() && reached == "unknown" {
            v = Verdict::NeedsTheModel;
        }
        // An error said as the answer is a fault -- except an app the
        // stand-in screen was asked to start and so never showed: that is
        // the test, not Atlas.
        let starts_an_app = would.iter().any(|w| w.starts_with("start "));
        if reply.trim_start().to_lowercase().starts_with("error") && !(tier == Tier::StandIn && starts_an_app) && !v.is_a_fault() {
            v = Verdict::Broken("an error said as the answer".into());
        }
        // Says it did something when nothing was done or started: the
        // failure Eric hears as "it doesn't actually do it".
        if would.is_empty()
            && d.last_reached().is_none()
            && crate::repeating::sentences(&reply).iter().any(|s| crate::backed::claims_work_started(s))
            && !v.is_a_fault()
        {
            v = Verdict::Broken("says it did something it didn't".into());
        }
        let row = Row { command, said, reached, tier, verdict: v, reply, ms, would };
        on_row(&row);
        rows.push(row);
    }
    // What you've corrected before, said again (`regressions`): the row fails
    // if Atlas gives the answer you said was wrong.
    let cases: Vec<crate::regressions::Case> = crate::store::Store::new(store_dir.to_path_buf()).load(crate::regressions::FILE);
    let base = rows.len() as u64;
    for (i, case) in cases.iter().filter(|c| c.source == crate::regressions::Source::Correction).enumerate() {
        let mut d = crate::daemon::Daemon::new(
            cfg,
            plat,
            llm.clone(),
            crate::store::Store::new(store_dir.to_path_buf()),
            crate::proactive::Proactive::new(crate::proactive::ProactiveConfig::default()),
        );
        d.rehearsal = true;
        let _ = plat.take();
        let started = std::time::Instant::now();
        let reply = match crate::crash::caught("a corrected sentence", || d.turn(&case.said, t0 + (base + i as u64) * 60)) {
            Ok(r) => r,
            Err(why) => format!("thinking about what to do next went wrong: {why}"),
        };
        let ms = started.elapsed().as_millis() as u64;
        let k = crate::session::kind_of(&parser.parse(&case.said));
        let reached = d.last_reached().unwrap_or_else(|| k.to_string());
        let mut would = plat.take();
        would.extend(std::mem::take(&mut d.rehearsed));
        let v = if crate::regressions::repeats_the_mistake(case, &reply) {
            Verdict::Broken(format!("the answer you corrected before -- you wanted: {}", case.wanted))
        } else {
            judge_reply(&reply, ms, &reached, None, !would.is_empty())
        };
        let row = Row { command: format!("corrected: {}", reached), said: case.said.clone(), reached, tier: tier(k), verdict: v, reply, ms, would };
        on_row(&row);
        rows.push(row);
    }
    rows
}

/// The rows that failed, as cases for the install's own regressions
/// (`regressions::FROM_SELFTEST`).
pub fn failing_cases(rows: &[Row], at: u64) -> Vec<crate::regressions::Case> {
    rows.iter()
        .filter(|r| r.verdict.is_a_fault() && !r.command.starts_with("corrected: "))
        .map(|r| crate::regressions::Case {
            said: r.said.clone(),
            wrong: r.reply.clone(),
            wanted: match &r.verdict {
                Verdict::Broken(why) => format!("not: {why}"),
                other => format!("not: {other:?}"),
            },
            command: r.command.clone(),
            source: crate::regressions::Source::SelfTest,
            at,
        })
        .collect()
}

/// The report, for you to read.
pub fn report(rows: &[Row], started: &str, with_model: bool) -> String {
    let faults: Vec<&Row> = rows.iter().filter(|r| r.verdict.is_a_fault()).collect();
    let count = |f: &dyn Fn(&Verdict) -> bool| rows.iter().filter(|r| f(&r.verdict)).count();
    let mut s = format!(
        "# Atlas tested itself -- {started}\n\n{} sentences. {} to fix. {} work. {} switched off. {} need installing or setting up. {} ask first. {} rehearsed (not done in a test).\n\nThe model was {}.\n\n",
        rows.len(),
        faults.len(),
        count(&|v| *v == Verdict::Works || *v == Verdict::WouldStart || *v == Verdict::AsksForMore),
        count(&|v| *v == Verdict::SwitchedOff),
        count(&|v| matches!(v, Verdict::NeedsInstalling | Verdict::NeedsSettingUp)),
        count(&|v| *v == Verdict::AsksFirst),
        count(&|v| *v == Verdict::Rehearsed),
        if with_model { "used" } else { "not used (`--no-model`)" },
    );
    let mut section = |title: &str, keep: &dyn Fn(&Verdict) -> bool| {
        let these: Vec<&Row> = rows.iter().filter(|r| keep(&r.verdict)).collect();
        if these.is_empty() {
            return;
        }
        s.push_str(&format!("## {title} ({})\n\n| command | said | result | reply | would |\n|---|---|---|---|---|\n", these.len()));
        for r in these {
            let reply: String = r.reply.replace('\n', " / ").replace('|', "/").chars().take(220).collect();
            s.push_str(&format!(
                "| {} | {} | {} | {} | {} |\n",
                r.command,
                r.said.replace('|', "/"),
                r.verdict.plain(),
                reply,
                r.would.join("; ").replace('|', "/")
            ));
        }
        s.push('\n');
    };
    section("To fix", &|v| v.is_a_fault());
    section("Switched off, installing or setting up -- yours to change", &|v| matches!(v, Verdict::SwitchedOff | Verdict::NeedsInstalling | Verdict::NeedsSettingUp));
    section("Asks first", &|v| *v == Verdict::AsksFirst);
    section("Works", &|v| matches!(v, Verdict::Works | Verdict::WouldStart | Verdict::AsksForMore));
    section("Rehearsed", &|v| *v == Verdict::Rehearsed);
    s
}

/// One line for the voice and the console.
pub fn summary(rows: &[Row]) -> String {
    let faults = rows.iter().filter(|r| r.verdict.is_a_fault()).count();
    let off = rows.iter().filter(|r| matches!(r.verdict, Verdict::SwitchedOff | Verdict::NeedsInstalling | Verdict::NeedsSettingUp)).count();
    format!(
        "Tested {} sentences: {} to fix, {} waiting on a switch or an install, the rest work or ask first.",
        rows.len(),
        faults,
        off
    )
}

/// A scratch copy of the install at `real`, in `scratch`: settings and state
/// copied, the big read-only folders linked. Returns what couldn't be linked.
pub fn scratch_copy(real: &Path, scratch: &Path) -> std::io::Result<Vec<String>> {
    let _ = std::fs::remove_dir_all(scratch);
    std::fs::create_dir_all(scratch.join("data"))?;
    copy_tree(&real.join("config"), &scratch.join("config"))?;
    let state = real.join("data").join("state");
    if state.is_dir() {
        copy_tree(&state, &scratch.join("data").join("state"))?;
    }
    // Anything else beside them (models, tools, voices) is linked, not copied.
    let mut missed = Vec::new();
    for e in std::fs::read_dir(real)?.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if name == "config" || name == "data" || !e.path().is_dir() {
            continue;
        }
        if link_dir(&e.path(), &scratch.join(&name)).is_err() {
            missed.push(name);
        }
    }
    Ok(missed)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for e in std::fs::read_dir(from)?.flatten() {
        let p = e.path();
        let dest = to.join(e.file_name());
        if p.is_dir() {
            copy_tree(&p, &dest)?;
        } else {
            std::fs::copy(&p, &dest)?;
        }
    }
    Ok(())
}

fn link_dir(from: &Path, to: &Path) -> std::io::Result<()> {
    #[cfg(windows)]
    {
        // A junction needs no special rights, unlike a symbolic link.
        let ok = crate::tools::command("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(to)
            .arg(from)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?
            .success();
        if ok {
            Ok(())
        } else {
            Err(std::io::Error::other("mklink /J failed"))
        }
    }
    #[cfg(not(windows))]
    {
        std::os::unix::fs::symlink(from, to)
    }
}

/// Where reports go: `data/selftest` in the real install.
pub fn reports_dir(real_root: &Path) -> PathBuf {
    real_root.join("data").join("selftest")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_read_the_reply() {
        assert_eq!(judge_reply("Reading your email is switched off.", 10, "mail", None, false), Verdict::SwitchedOff);
        assert_eq!(judge_reply("Sending a message to Sam -- it goes out as you. Go ahead?", 10, "message", None, false), Verdict::AsksFirst);
        assert_eq!(judge_reply("Platform: there's nothing to undo", 10, "undo", None, false), Verdict::Broken("a slip in the words".into()));
        assert_eq!(judge_reply("It's 7:42 PM.", 10, "clock", Some("clock"), false), Verdict::Works);
        assert_eq!(judge_reply("It's 7:42 PM.", 10, "say", Some("clock"), false), Verdict::WrongTool("say (wanted clock)".into()));
        assert_eq!(judge_reply("Done.", 9_000, "clock", None, false), Verdict::Slow(9_000));
        assert_eq!(judge_reply("[rehearsed] would back up", 5, "back_up", None, false), Verdict::Rehearsed);
    }

    #[test]
    fn nothing_that_changes_the_install_runs() {
        for k in ["hand_over", "take_it_back", "unlock", "back_up", "undo", "sync", "a_command_added_later"] {
            assert_eq!(tier(k), Tier::Rehearse, "{k}");
        }
        assert_eq!(tier("open_app"), Tier::StandIn);
        assert_eq!(tier("clock"), Tier::Run);
    }
}

/// Set on the test's own process and everything it starts.
pub const IN_A_TEST: &str = "ATLAS_SELFTEST";

/// Said when something in a test would have started another Atlas.
pub const NO_SECOND_ATLAS: &str = "this is the self-test's copy of Atlas, which never starts another Atlas";

/// Is this process (or the one that started it) the self-test?
///
/// The test runs on a copy of the install (`ATLAS_HOME` pointed at it), and
/// "show me settings" there opened Atlas's window -- which, finding no Atlas
/// running *in the copy*, started a background one there (30 Sep 2026). It
/// outlived the test: a second Atlas with its own hub, typing box,
/// microphone and model server, answering Eric from the temp folder while
/// the real one ran beside it, and the settings he changed went into the
/// copy. Starting another Atlas (`firstlaunch::spawn_quietly`,
/// `open_atlas_window`) is refused while this is set.
pub fn in_a_test() -> bool {
    std::env::var_os(IN_A_TEST).is_some_and(|v| !v.is_empty())
}

