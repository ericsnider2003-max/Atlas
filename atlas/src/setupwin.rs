//! Atlas's own window for setting up and starting — the thing a double-click
//! opens.
//!
//! One window, no terminal, no browser (Eric's ruling, 17 Sep 2026: the
//! desktop surface is Atlas's own window). It walks the steps itself and says
//! each one in words as it goes:
//!
//! 1. a home for Atlas (and its Start-menu and desktop shortcuts),
//! 2. the voice pieces and Tor, fetched and checked (`getpieces`),
//! 3. the Windows Firewall rule for your own devices, asked for once
//!    (`doorrule`; Tor, fetched with the pieces, is how friends reach you),
//! 4. getting to know this computer (`atlas adapt`),
//! 5. checking everything (`doctor`), keeping only what you can act on,
//! 6. your phone — a code to scan, over Tailscale (`phonelink`).
//!
//! Then: whether Atlas is running, a button to start it, and a switch to start
//! it with Windows. Everything is safe to run again; opening the window later
//! walks the same steps, and the ones already done say so at once.
//!
//! The mark — Atlas's line — is at the top: drawing in when the window opens,
//! the slow thinking wave while work is going on, still when it's done.

use crate::firstlaunch;
use crate::getpieces;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Where one step stands.
#[derive(Debug, Clone, PartialEq)]
pub enum StepState {
    Waiting,
    /// Working; a fraction when there's a size to measure against.
    Working(Option<f32>),
    Done(String),
    Problem(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub label: String,
    pub state: StepState,
}

/// The phone section.
#[derive(Debug, Clone, PartialEq)]
pub enum Phone {
    Checking,
    Link { url: String, modules: (usize, Vec<bool>) },
    Why(String),
}

/// Everything the window shows, filled in by the worker.
#[derive(Debug, Clone)]
pub struct Progress {
    pub steps: Vec<Step>,
    /// Things `doctor` found that you can do something about, in words.
    pub to_look_at: Vec<String>,
    pub phone: Phone,
    pub finished: bool,
    /// What finishing did next — switched on starting with Windows, started
    /// the background Atlas — in words (`after_setup`).
    pub after: Vec<String>,
}

impl Progress {
    pub fn new() -> Progress {
        let mut steps = vec![Step { label: "A home for Atlas".into(), state: StepState::Waiting }];
        for p in setup_pieces() {
            steps.push(Step { label: format!("{} ({} MB)", capitalise(p.name), p.megabytes()), state: StepState::Waiting });
        }
        steps.push(Step { label: "Letting your own devices reach Atlas".into(), state: StepState::Waiting });
        steps.push(Step { label: "Getting to know this computer".into(), state: StepState::Waiting });
        steps.push(Step { label: "Checking everything".into(), state: StepState::Waiting });
        Progress { steps, to_look_at: Vec::new(), phone: Phone::Checking, finished: false, after: Vec::new() }
    }

    fn working(&self) -> bool {
        !self.finished
    }

    pub fn problems(&self) -> usize {
        self.steps.iter().filter(|s| matches!(s.state, StepState::Problem(_))).count()
    }
}

impl Default for Progress {
    fn default() -> Self {
        Progress::new()
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// "Report a problem with Atlas": the desktop button for feedback (OPEN_GAPS
/// 8.14), the same three steps as `atlas feedback send` and the voice --
/// your words, what will go exactly, then send only on the second press.
/// Kept apart from the drawing so it can be tested without a window.
#[derive(Debug, Clone, Default)]
pub struct FeedbackForm {
    pub open: bool,
    pub words: String,
    /// Attach what was written down about an update that failed here. Only
    /// offered when there is one, and off until you tick it.
    pub attach: bool,
    /// What will be sent, once you've asked to see it.
    pub showing: Option<crate::feedback::Feedback>,
    /// What happened, in words.
    pub said: String,
}

impl FeedbackForm {
    /// Whether there's a failed update to offer attaching.
    pub fn failure_here(store: &crate::store::Store) -> bool {
        crate::update_apply::last_failure(store).is_some()
    }

    /// "Show me what will be sent": builds it, sends nothing.
    pub fn show(&mut self, store: &crate::store::Store, now: u64) {
        let attach = if self.attach { crate::update_apply::last_failure(store) } else { None };
        match crate::feedback::compose_feedback(&self.words, attach, now) {
            Ok(f) => {
                self.showing = Some(f);
                self.said.clear();
            }
            Err(why) => {
                self.showing = None;
                self.said = why;
            }
        }
    }

    /// "Send it": only what was shown, and only after it was shown.
    pub fn send(&mut self, store: &crate::store::Store, peer_dir: &Path) {
        let Some(f) = self.showing.take() else {
            self.said = "Press \"Show me what will be sent\" first.".into();
            return;
        };
        match crate::feedback::send_decided(store, peer_dir, f) {
            Ok(s) => {
                self.said = s.plain();
                self.words.clear();
                self.attach = false;
            }
            Err(why) => self.said = why,
        }
    }

    /// Words changed after it was shown: show it again before sending.
    pub fn edited(&mut self) {
        self.showing = None;
    }
}

/// Everything the setup window fetches, in order: the voice pieces, the
/// language model (`getpieces::pictures`: the same model answers questions
/// and reads screens), then Tor (`getpieces::tor`, OPEN_GAPS 8.3).
///
/// Eric, 27 Sep 2026: "Nothing needed installed when I started Atlas… I
/// can't even get help from Atlas because there is no language model." The
/// model was a separate `atlas get pictures`, a command, so setup finished
/// with an Atlas that could answer nothing.
///
/// Lives in `getpieces` (28 Sep 2026): this module only exists in the desktop
/// build, and the running Atlas asks for the list too, so the phone builds
/// failed to compile while it was defined here.
pub use crate::getpieces::setup_pieces;

/// What the setup needs to know about this install.
#[derive(Debug, Clone)]
pub struct Place {
    pub root: PathBuf,
    pub exe: PathBuf,
    /// Where the hub answers now (`firstlaunch::hub_port_at`).
    pub port: u16,
    /// The setting, for when the hub isn't answering.
    pub configured_port: u16,
    pub token: String,
}

/// Doctor's lines that are only the consequence of a voice piece not being
/// here yet.
const VOICE_FINDINGS: &[&str] =
    &["record (mic)", "stt", "tts", "play", "model 'stt_model'", "speech engine"];

/// Doctor findings a person can act on, in words. The ones about Atlas's own
/// shipped settings are Atlas's to fix, not yours, and apps you don't have
/// are grouped into one line rather than one alarm each.
pub fn for_you_to_look_at(findings: &[crate::doctor::Finding], pieces_missing: bool) -> Vec<String> {
    let mut out = Vec::new();
    let mut missing_apps: Vec<String> = Vec::new();
    let mut extras = false;
    for f in findings.iter().filter(|f| !f.ok) {
        if f.label == "settings that do nothing" {
            continue;
        }
        // When a voice piece didn't arrive, the steps above already say so;
        // the same gap again in doctor's words would be the same news twice.
        if pieces_missing && VOICE_FINDINGS.contains(&f.label.as_str()) {
            continue;
        }
        // The camera features are optional and ship off: one line saying
        // so, not three alarms about models nobody asked for yet.
        if ["vision", "reading", "hands"].contains(&f.label.as_str()) {
            extras = true;
            continue;
        }
        if let Some(app) = f.label.strip_prefix("app '").and_then(|a| a.strip_suffix('\'')) {
            missing_apps.push(app.to_string());
            continue;
        }
        let first = f.detail.split(". ").next().unwrap_or(&f.detail).trim().trim_end_matches('.');
        out.push(format!("{}: {first}.", f.label));
    }
    if extras {
        out.push(
            "The camera features (seeing, reading the screen, following your hands) aren't set \
             up — they're optional, and off until you want them."
                .into(),
        );
    }
    if !missing_apps.is_empty() {
        out.push(format!(
            "I couldn't find {} on this computer — that's fine if you don't use {}.",
            missing_apps.join(", "),
            if missing_apps.len() == 1 { "it" } else { "them" }
        ));
    }
    out
}

/// Walk the steps. Runs on its own thread; the window only reads `progress`.
pub fn walk_the_steps(place: &Place, progress: &Arc<Mutex<Progress>>, tools: &getpieces::Tools) {
    let set = |i: usize, s: StepState| {
        if let Ok(mut p) = progress.lock().or_else(crate::crash::unpoison) {
            if let Some(step) = p.steps.get_mut(i) {
                step.state = s;
            }
        }
    };

    // 1. A home, and the shortcuts the first time. The download mark comes
    // off Atlas's own copy here too, so a copy that was unzipped in place
    // (rather than moved in) also stops being asked about at every start.
    firstlaunch::forget_download_mark(&place.exe);
    let first_time = !firstlaunch::is_set_up(&place.root);
    let mut home = format!("Atlas lives in {}", place.root.display());
    if first_time {
        let made = firstlaunch::make_shortcuts(&place.exe);
        if !made.is_empty() {
            home = format!("{home}. {}", made.join(" "));
        }
    }
    set(0, StepState::Done(home));

    // 2. The pieces. About 3.3 GB in all, so first: is there room? Until 28
    // Sep 2026 nothing looked, and a full disk showed up as a download that
    // failed partway, or an unpack that did.
    let pieces = setup_pieces();
    let mut no_room = getpieces::room_for(&pieces, &place.root, getpieces::free_bytes(&place.root));
    if no_room.is_err() {
        // Half-finished downloads of our own are the first thing to give back.
        if getpieces::clear_unfinished(&pieces, &place.root) > 0 {
            no_room = getpieces::room_for(&pieces, &place.root, getpieces::free_bytes(&place.root));
        }
    }
    let mut said_no_room = false;
    for (n, piece) in pieces.iter().enumerate() {
        let i = n + 1;
        if getpieces::have(piece, &place.root) {
            set(i, StepState::Done("already here".into()));
            continue;
        }
        if let Err(why) = &no_room {
            set(
                i,
                StepState::Problem(if said_no_room { "not fetched: there isn't room yet (see above)".into() } else { why.clone() }),
            );
            said_no_room = true;
            continue;
        }
        set(i, StepState::Working(Some(0.0)));
        let report = |done: u64, total: u64| {
            let f = if total == 0 { 0.0 } else { done as f32 / total as f32 };
            set(i, StepState::Working(Some(f.clamp(0.0, 1.0))));
        };
        match getpieces::fetch(piece, &place.root, tools, &report) {
            Ok(()) => set(i, StepState::Done(format!("here — for {}", piece.for_what))),
            Err(e) => set(i, StepState::Problem(e)),
        }
    }

    // 3. The door your own devices use: the Windows Firewall rule, asked for
    // once (`doorrule`). Friends come through Tor and need no rule.
    let door = pieces.len() + 1;
    set(door, StepState::Working(None));
    let state = place.root.join("data").join("state");
    match crate::doorrule::ensure(&state, &place.exe, &crate::doorrule::run_program) {
        crate::doorrule::Standing::Problem(w) => set(door, StepState::Problem(w)),
        s => set(door, StepState::Done(s.plain())),
    }

    // 4. This computer.
    let adapt = door + 1;
    if place.root.join("config").join("machine.yaml").is_file() {
        set(adapt, StepState::Done("already done".into()));
    } else {
        set(adapt, StepState::Working(None));
        match firstlaunch::run_quietly(&place.exe, &["adapt"]) {
            Ok(out) if out.status.success() => set(adapt, StepState::Done("done".into())),
            Ok(out) => {
                let said = String::from_utf8_lossy(&out.stdout);
                let last = said.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("it stopped partway");
                set(adapt, StepState::Problem(last.trim().to_string()))
            }
            Err(e) => set(adapt, StepState::Problem(format!("I couldn't run it: {e}"))),
        }
    }

    // 4. Checking.
    let check = adapt + 1;
    let mut settings_ok = false;
    set(check, StepState::Working(None));
    crate::getpieces::use_own_tools(&place.root);
    match crate::config::Config::load(&place.root.join("config")) {
        Ok(cfg) => {
            settings_ok = cfg.tools.is_some();
            let cfg = crate::config::Config { tools: cfg.tools.map(|t| t.anchored()), ..cfg };
            let findings = with_platform(|plat| crate::doctor::run(&cfg, cfg.tools.as_ref(), plat));
            let pieces_missing = setup_pieces().iter().any(|p| !getpieces::have(p, &place.root));
            let mut said = for_you_to_look_at(&findings, pieces_missing);
            // Smart App Control first: where it's on or about to be, it's
            // the thing that stops everything else working.
            if let Some(words) = firstlaunch::app_control_words(firstlaunch::app_control()) {
                said.insert(0, words);
            }
            let n = said.len();
            if let Ok(mut p) = progress.lock().or_else(crate::crash::unpoison) {
                p.to_look_at = said;
            }
            set(
                check,
                StepState::Done(if n == 0 {
                    "everything's in order".into()
                } else {
                    format!("{n} thing{} to look at, below", if n == 1 { "" } else { "s" })
                }),
            );
        }
        Err(e) => set(check, StepState::Problem(format!("I couldn't read my settings: {e}"))),
    }

    // 5. The phone.
    let phone = phone_section(place);
    if let Ok(mut p) = progress.lock().or_else(crate::crash::unpoison) {
        p.phone = phone;
    }

    // Only a setup that went through is marked done (29 Sep 2026: a fetch
    // that failed was marked done anyway, so Atlas never tried again and
    // fell back to push-to-talk for good). A step that had a problem is
    // tried again the next time Atlas is opened.
    let problems = progress.lock().or_else(crate::crash::unpoison).map(|p| p.problems()).unwrap_or(0);
    if problems == 0 {
        crate::heard!(firstlaunch::mark_set_up(&place.root));
    }
    let after = after_setup(place, settings_ok, first_time);
    if let Ok(mut p) = progress.lock().or_else(crate::crash::unpoison) {
        p.after = after;
        p.finished = true;
    }
}

/// Once setup is through, on Windows: start with Windows (the first time
/// only — `startup::decided` — so unticking it later sticks), and start the
/// background Atlas if it isn't running, with no window of its own (Eric, 28
/// Sep 2026: no terminal, and nothing has to stay open for Atlas to run).
/// Only from the Atlas program itself: a test harness running these steps
/// must not register or start anything.
fn after_setup(place: &Place, settings_ok: bool, just_set_up: bool) -> Vec<String> {
    let is_atlas = place.exe.file_stem().map(|s| s.to_string_lossy().eq_ignore_ascii_case("atlas")).unwrap_or(false)
        && std::env::current_exe().map(|me| me == place.exe).unwrap_or(false);
    let state = place.root.join("data").join("state");
    let next = crate::startup::after_setup(
        cfg!(windows) && is_atlas,
        settings_ok,
        just_set_up,
        crate::startup::decided(&state),
        firstlaunch::atlas_running(&place.root),
    );
    let mut said = Vec::new();
    if next.register {
        match crate::startup::turn_on(&place.exe, crate::startup::Mode::Background) {
            Ok(words) => {
                crate::heard!(crate::startup::remember(&state, true));
                said.push(format!("{words} You can switch that off below."));
            }
            // Not remembered, so the next setup tries again.
            Err(e) => said.push(format!("I couldn't set Atlas to start when you sign in: {e}.")),
        }
    }
    if next.start {
        match firstlaunch::start_background_watched(&place.exe, &place.root, std::time::Duration::from_secs(4)) {
            Ok(()) => said.push("Starting Atlas in the background.".into()),
            Err(why) => said.push(why),
        }
    }
    said
}

fn with_platform<T>(f: impl FnOnce(&dyn crate::platform::Platform) -> T) -> T {
    #[cfg(windows)]
    {
        f(&crate::platform::win::WindowsPlatform)
    }
    #[cfg(not(windows))]
    {
        f(&crate::platform::mock::MockPlatform::new(vec![]))
    }
}

/// The phone: a code to scan when Tailscale can publish Atlas to your phone,
/// otherwise the one thing to do about it.
fn phone_section(place: &Place) -> Phone {
    let tool = crate::phonelink::tailscale_tool();
    let installed = crate::tools::which(&tool.command).is_some();
    if !installed {
        return Phone::Why(crate::phonelink::say(&crate::phonelink::Serve::NoTailscale));
    }
    let vars = crate::tools::Vars::new();
    match crate::phonelink::publish(place.port, &place.token, &tool, &vars) {
        crate::phonelink::Serve::Published { url } => {
            // Kept, so the hub's "Your devices" page can show the same code
            // for adding another device.
            crate::kept!(crate::roots::store().save(crate::phonelink::LINK_KEY, &url));
            match crate::phonelink::qr_modules(&url) {
            Some(modules) => Phone::Link { url, modules },
            None => Phone::Why("I have your phone's link but couldn't draw its code.".into()),
            }
        }
        other => Phone::Why(crate::phonelink::say(&other)),
    }
}

/// Which page the window opens on: in `firstlaunch`, so a build without the
/// desktop window (the GUI-free core) can still name one.
pub use crate::firstlaunch::First;

/// A window height that fits a screen `screen` tall: at most nine tenths of
/// it (room for the taskbar and title bar), never under the window's least.
fn fit_height(want: f32, screen: f32) -> f32 {
    want.min(screen * 0.9).max(480.0)
}

/// Left by a second "Open Atlas" for the open window to come forward.
const RAISE_FILE: &str = "raise";

/// Open the window. Blocks until it's closed.
pub fn run(place: Place, first: First) -> Result<(), String> {
    // One Atlas window (30 Sep 2026): "Open Atlas" by the clock, a
    // double-click on the icon or the Start menu each opened another. A
    // second one asks the first to come forward, and goes.
    let window_dir = place.root.join("data").join("window");
    crate::heard!(std::fs::create_dir_all(&window_dir));
    let window_lock = crate::onlyone::OnlyOne::at(&window_dir);
    let now = crate::store::now();
    if !window_lock.look(now).can_take() {
        return std::fs::write(window_dir.join(RAISE_FILE), b"")
            .map_err(|e| format!("Atlas's window is already open, and I couldn't ask it to come forward: {e}"));
    }
    crate::heard!(window_lock.take(now));
    let progress = Arc::new(Mutex::new(Progress::new()));
    start_work(&place, &progress);
    let opts = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default().with_icon(crate::mark::window_icon())
            .with_inner_size([560.0, 860.0])
            .with_min_inner_size([420.0, 480.0])
            .with_title("Atlas"),
        ..Default::default()
    };
    let settings = crate::settingswin::Page::new(place.root.join("config"));
    let hub = crate::hubwin::Hub::new(place.port, place.root.join("data").join("webview"));
    let (view, hub_page) = match first {
        First::Home => (View::Home, "/hub".to_string()),
        First::Settings => (View::Settings, "/hub".to_string()),
        First::Hub(page) => (View::Hub, page),
    };
    let place_data = place.root.join("data");
    let app = App {
        view,
        settings,
        hub,
        hub_page,
        hub_go: false,
        size_before_hub: None,
        hub_sized: false,
        restarting: Arc::new(Mutex::new(None)),
        place,
        progress,
        opened: std::time::Instant::now(),
        running: false,
        last_poll: None,
        with_windows: crate::startup::decided(&place_data.join("state")),
        note: None,
        voice: crate::speaking::Watch::new(place_data),
        feedback: FeedbackForm::default(),
        fitted: false,
    };
    // The lock is kept fresh, and a request to come forward answered, from
    // a thread of its own: a minimized window isn't drawn, so its frames
    // can't be what keeps it.
    let closed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let keeper = {
        let (closed, lock, dir) = (closed.clone(), window_lock.clone(), window_dir.clone());
        move |ctx: eframe::egui::Context| {
            std::thread::spawn(move || {
                let mut last = now;
                while !closed.load(std::sync::atomic::Ordering::SeqCst) {
                    let t = crate::store::now();
                    if lock.due(last, t) {
                        lock.beat(t);
                        last = t;
                    }
                    if std::fs::remove_file(dir.join(RAISE_FILE)).is_ok() {
                        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Minimized(false));
                        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
                        ctx.request_repaint();
                    }
                    std::thread::sleep(std::time::Duration::from_millis(400));
                }
            });
        }
    };
    let shown = eframe::run_native(
        "atlas-home",
        opts,
        Box::new(move |cc| {
            keeper(cc.egui_ctx.clone());
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| e.to_string());
    closed.store(true, std::sync::atomic::Ordering::SeqCst);
    window_lock.release();
    shown
}

fn start_work(place: &Place, progress: &Arc<Mutex<Progress>>) {
    if let Ok(mut p) = progress.lock().or_else(crate::crash::unpoison) {
        *p = Progress::new();
    }
    let place = place.clone();
    let progress = Arc::clone(progress);
    std::thread::spawn(move || walk_the_steps(&place, &progress, &getpieces::Tools::default()));
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum View {
    Home,
    Hub,
    Settings,
}

struct App {
    view: View,
    settings: crate::settingswin::Page,
    /// The hub, shown inside this window (`hubwin`).
    hub: crate::hubwin::Hub,
    /// The hub page to open when the Hub page is next shown.
    hub_page: String,
    /// Load `hub_page` afresh on the next frame (set when a button asks for a
    /// particular hub page; otherwise the hub stays where you left it).
    hub_go: bool,
    /// The window's size before the Hub page widened it, to go back to.
    size_before_hub: Option<eframe::egui::Vec2>,
    /// Whether this visit to the Hub page has already been sized.
    hub_sized: bool,
    /// What a restart is doing, from its own thread.
    restarting: Arc<Mutex<Option<String>>>,
    place: Place,
    progress: Arc<Mutex<Progress>>,
    opened: std::time::Instant,
    running: bool,
    last_poll: Option<std::time::Instant>,
    /// Whether the start-with-Windows task is on, once known.
    with_windows: Option<bool>,
    note: Option<String>,
    /// Atlas's voice, so the mark moves while it speaks.
    voice: crate::speaking::Watch,
    /// "Report a problem with Atlas".
    feedback: FeedbackForm,
    /// Whether the opening size has been checked against the screen.
    fitted: bool,
}

impl eframe::App for App {
    fn update(&mut self, ctx: &eframe::egui::Context, win: &mut eframe::Frame) {
        // The locked hub design's colourway, not egui's default grey.
        crate::look_paint::dress(ctx);

        use crate::look_paint::MarkState;
        use eframe::egui::{self, FontId, RichText};

        // Twice a second at least, for what changes without input (the
        // progress of a download, whether Atlas is answering); faster only
        // where the mark is drawn and moving, below (27 Sep 2026: this was
        // thirty frames a second on every page, the hub page included).
        ctx.request_repaint_after(std::time::Duration::from_millis(500));
        let t = self.opened.elapsed().as_secs_f32();
        let snapshot = self.progress.lock().or_else(crate::crash::unpoison).map(|p| p.clone()).unwrap_or_default();

        // Is Atlas answering? Asked every couple of seconds, not every frame.
        if self.last_poll.map(|p| p.elapsed().as_secs_f32() > 2.0).unwrap_or(true) {
            self.running = firstlaunch::atlas_running(&self.place.root);
            // The hub's real port: it may have opened beside a taken one
            // (`server::open_hub`), or moved back.
            let port = firstlaunch::hub_port_at(&self.place.root, self.place.configured_port);
            if port != self.place.port {
                self.place.port = port;
                self.hub.set_port(port);
            }
            self.last_poll = Some(std::time::Instant::now());
            if self.running && self.note.as_deref() == Some(STARTING) {
                self.note = None;
            }
        }

        // The hub is laid out for a wide window (the command deck: Right now
        // beside your cards). The setup window is narrow, so the Hub page
        // widens it — within the screen — and leaving puts it back.
        // 30 Sep 2026: the window opens 860 tall, which on a laptop screen
        // at 150% scaling runs past the taskbar with the buttons under it.
        // Once the screen is known, the window is brought within it.
        if !self.fitted {
            let (now, screen) = ctx.input(|i| (i.screen_rect().size(), i.viewport().monitor_size));
            if let Some(room) = screen.filter(|r| r.y > 0.0) {
                self.fitted = true;
                let fits = fit_height(now.y, room.y);
                if fits + 1.0 < now.y {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(now.x, fits)));
                }
            }
        }
        if self.view == View::Hub && !self.hub_sized {
            self.hub_sized = true;
            let (now, screen) = ctx.input(|i| (i.screen_rect().size(), i.viewport().monitor_size));
            let room = screen.unwrap_or(egui::vec2(1440.0, 900.0));
            let want = egui::vec2(1240.0f32.min(room.x * 0.92), fit_height(now.y.max(860.0), room.y));
            if now.x + 40.0 < want.x {
                self.size_before_hub = Some(now);
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
            }
        } else if self.view != View::Hub && self.hub_sized {
            self.hub_sized = false;
            if let Some(size) = self.size_before_hub.take() {
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
            }
        }

        let frame = egui::Frame::none().fill(crate::look_paint::colourway().ink).inner_margin(egui::Margin::same(28.0));
        egui::CentralPanel::default().frame(frame).show(ctx, |ui| {
            // The two pages: Atlas (setup and whether it's running) and its
            // Settings. Always one click apart.
            ui.horizontal(|ui| {
                for (view, name) in [(View::Home, "Atlas"), (View::Hub, "Hub"), (View::Settings, "Settings")] {
                    let colour = if self.view == view { crate::look_paint::colourway().signal_text } else { crate::look_paint::colourway().soft };
                    let b = egui::Button::new(RichText::new(name).font(FontId::proportional(15.0)).color(colour)).frame(false);
                    if ui.add(b).clicked() {
                        self.view = view;
                    }
                }
            });
            ui.add_space(6.0);
            let restart_note = self.restarting.lock().or_else(crate::crash::unpoison).ok().and_then(|n| n.clone());
            if let Some(n) = &restart_note {
                ui.label(RichText::new(n).font(FontId::proportional(13.0)).color(crate::look_paint::colourway().signal_text));
            }
            if self.view == View::Hub {
                self.show_hub(ui, win);
                return;
            }
            // Any other page: the hub steps aside and keeps its place.
            self.hub.hide();
            if self.view == View::Settings {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    if let Some(crate::settingswin::Ask::Restart) = self.settings.show(ui, self.running) {
                        restart(&self.place, &self.restarting);
                    }
                });
                return;
            }
            egui::ScrollArea::vertical().show(ui, |ui| {
                let state = if t < 0.8 {
                    MarkState::Waking
                } else if snapshot.working() {
                    MarkState::Thinking
                } else {
                    MarkState::Idle
                };
                let (rect, mark_resp) = ui.allocate_exact_size(egui::vec2(48.0, 48.0), egui::Sense::hover());
                mark_resp.widget_info(|| {
                    let what = match state {
                        MarkState::Waking => "Atlas mark: listening",
                        MarkState::Thinking => "Atlas mark: working",
                        _ => "Atlas mark",
                    };
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, what)
                });
                let voice = self.voice.level();
                if voice.is_some() {
                    ctx.request_repaint();
                } else {
                    ctx.request_repaint_after(crate::window::repaint_every(state));
                }
                crate::window::paint_mark(ui.painter(), rect, state, t, voice);
                ui.add_space(12.0);
                let heading = if snapshot.working() {
                    "Setting Atlas up"
                } else if self.running {
                    "Atlas is running"
                } else {
                    "Atlas is ready"
                };
                ui.label(RichText::new(heading).font(FontId::proportional(26.0)).strong().color(crate::look_paint::colourway().text));
                ui.add_space(16.0);

                for step in &snapshot.steps {
                    let (dot, colour, detail) = match &step.state {
                        StepState::Waiting => (Dot::Ring, crate::look_paint::colourway().dim, String::new()),
                        StepState::Working(Some(f)) if *f >= 1.0 => {
                            (Dot::Pulse, crate::look_paint::colourway().signal, "checking it's the right file".into())
                        }
                        StepState::Working(Some(f)) => (Dot::Pulse, crate::look_paint::colourway().signal, format!("{:.0}%", f * 100.0)),
                        StepState::Working(None) => (Dot::Pulse, crate::look_paint::colourway().signal, "working on it".into()),
                        StepState::Done(d) => (Dot::Filled, crate::look_paint::colourway().signal, d.clone()),
                        StepState::Problem(d) => (Dot::Filled, crate::look_paint::colourway().warn, d.clone()),
                    };
                    ui.horizontal_wrapped(|ui| {
                        // Drawn, not a glyph: the built-in font has no tick,
                        // and a box where a tick should be reads as broken.
                        let (r, dot_resp) = ui.allocate_exact_size(egui::vec2(18.0, 18.0), egui::Sense::hover());
                        // The dot's state as a word for screen readers: shape
                        // and colour aren't the only carriers (WCAG 1.1.1, 1.4.1).
                        let word = match &step.state {
                            StepState::Waiting => "Waiting",
                            StepState::Working(_) => "Working",
                            StepState::Done(_) => "Done",
                            StepState::Problem(_) => "Problem",
                        };
                        dot_resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, word));
                        let c = r.center();
                        match dot {
                            Dot::Ring => {
                                ui.painter().circle_stroke(c, 4.5, egui::Stroke::new(1.2_f32, colour));
                            }
                            Dot::Filled => {
                                ui.painter().circle_filled(c, 5.0, colour);
                            }
                            Dot::Pulse => {
                                let a = (0.55 + 0.45 * (t * 3.0).sin()) * 255.0;
                                let pulsing = egui::Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), a as u8);
                                ui.painter().circle_filled(c, 5.0, pulsing);
                            }
                        }
                        ui.label(RichText::new(&step.label).font(FontId::proportional(16.0)).color(crate::look_paint::colourway().text));
                    });
                    if let StepState::Working(Some(f)) = step.state {
                        ui.add(egui::ProgressBar::new(f).desired_height(4.0));
                    }
                    if !detail.is_empty() {
                        let c = if matches!(step.state, StepState::Problem(_)) { crate::look_paint::colourway().warn } else { crate::look_paint::colourway().soft };
                        ui.label(RichText::new(detail).font(FontId::proportional(13.0)).color(c));
                    }
                    ui.add_space(6.0);
                }

                if !snapshot.to_look_at.is_empty() {
                    ui.add_space(10.0);
                    ui.label(RichText::new("Worth a look").font(FontId::proportional(18.0)).color(crate::look_paint::colourway().text));
                    for line in &snapshot.to_look_at {
                        ui.label(RichText::new(line).font(FontId::proportional(13.0)).color(crate::look_paint::colourway().soft));
                    }
                }

                if snapshot.finished {
                    ui.add_space(18.0);
                    if snapshot.problems() > 0 && ui.button("Try the unfinished steps again").clicked() {
                        start_work(&self.place, &self.progress);
                    }
                    ui.add_space(8.0);
                    for line in &snapshot.after {
                        ui.label(RichText::new(line).font(FontId::proportional(13.0)).color(crate::look_paint::colourway().soft));
                    }
                    if self.running {
                        ui.label(RichText::new(RUNNING_WORDS)
                            .font(FontId::proportional(15.0)).color(crate::look_paint::colourway().text));
                    } else if ui.button(RichText::new("Start Atlas").font(FontId::proportional(18.0))).clicked() {
                        self.note = Some(match firstlaunch::spawn_quietly(&self.place.exe, &["--daemon"]) {
                            Ok(child) => {
                                crate::unwaited::dont_wait(child);
                                self.last_poll = None;
                                STARTING.into()
                            }
                            Err(e) => format!("I couldn't start: {e}"),
                        });
                    }
                    // Picked up once setup has switched it on by itself
                    // (`after_setup`), so the box shows ticked.
                    if self.with_windows.is_none() {
                        self.with_windows = crate::startup::decided(&self.place.root.join("data").join("state"));
                    }
                    let mut on = self.with_windows.unwrap_or(false);
                    if ui.checkbox(&mut on, "Start Atlas when I sign in").changed() {
                        let done = if on {
                            crate::startup::turn_on(&self.place.exe, crate::startup::Mode::Background)
                        } else {
                            crate::startup::turn_off()
                        };
                        match done {
                            Ok(words) => {
                                // Your choice, kept: setup never switches it
                                // back on once you've decided.
                                crate::heard!(crate::startup::remember(&self.place.root.join("data").join("state"), on));
                                self.with_windows = Some(on);
                                self.note = Some(words);
                            }
                            Err(e) => self.note = Some(format!("That didn't take: {e}")),
                        }
                    }
                    if let Some(n) = &self.note {
                        ui.label(RichText::new(n).font(FontId::proportional(13.0)).color(crate::look_paint::colourway().soft));
                    }

                    ui.add_space(20.0);
                    self.show_feedback(ui);

                    ui.add_space(20.0);
                    ui.label(RichText::new("Your phone").font(FontId::proportional(18.0)).color(crate::look_paint::colourway().text));
                    match &snapshot.phone {
                        Phone::Checking => {
                            ui.label(RichText::new("Checking…").color(crate::look_paint::colourway().soft));
                        }
                        Phone::Why(why) => {
                            ui.label(RichText::new(why).font(FontId::proportional(14.0)).color(crate::look_paint::colourway().soft));
                        }
                        Phone::Link { url, modules } => {
                            ui.label(RichText::new(
                                "First, on your phone: install the Tailscale app and sign in with the same account as this \
                                 computer (the link only opens on your own devices). Then point the camera at this, open the \
                                 link, and choose Share → Add to Home Screen.",
                            ).font(FontId::proportional(14.0)).color(crate::look_paint::colourway().soft));
                            ui.add_space(8.0);
                            paint_qr(ui, modules, 220.0);
                            ui.add_space(6.0);
                            ui.label(RichText::new(url).font(FontId::monospace(11.0)).color(crate::look_paint::colourway().dim));
                        }
                    }
                }
            });
        });
    }
}

impl App {
    /// "Report a problem with Atlas": write it, see exactly what goes, send.
    fn show_feedback(&mut self, ui: &mut eframe::egui::Ui) {
        use eframe::egui::{self, FontId, RichText};
        let store = crate::roots::store();
        let soft = crate::look_paint::colourway().soft;
        if !self.feedback.open {
            if ui.button(RichText::new("Report a problem with Atlas").font(FontId::proportional(15.0))).clicked() {
                self.feedback.open = true;
            }
            if !self.feedback.said.is_empty() {
                ui.label(RichText::new(&self.feedback.said).font(FontId::proportional(13.0)).color(soft));
            }
            return;
        }
        ui.label(RichText::new("Report a problem with Atlas").font(FontId::proportional(18.0)).color(crate::look_paint::colourway().text));
        ui.label(RichText::new("What's wrong? In your own words. Nothing is sent until you've seen it and pressed Send.")
            .font(FontId::proportional(13.0)).color(soft));
        let edit = ui.add(egui::TextEdit::multiline(&mut self.feedback.words).desired_rows(4).desired_width(f32::INFINITY));
        edit.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::TextEdit, true, "What's wrong"));
        if edit.changed() {
            self.feedback.edited();
        }
        if FeedbackForm::failure_here(&store)
            && ui.checkbox(&mut self.feedback.attach, "Attach what was written down about the update that failed here").changed()
        {
            self.feedback.edited();
        }
        ui.horizontal(|ui| {
            if ui.button("Show me what will be sent").clicked() {
                self.feedback.show(&store, crate::store::now());
            }
            if self.feedback.showing.is_some() && ui.button("Send it").clicked() {
                self.feedback.send(&store, &crate::roots::state_dir());
                self.feedback.open = !self.feedback.said.starts_with("Sending") && !self.feedback.said.starts_with("This is your own");
            }
            if ui.button("Cancel").clicked() {
                self.feedback = FeedbackForm::default();
            }
        });
        if let Some(f) = &self.feedback.showing {
            ui.label(RichText::new(crate::feedback::feedback_preview(f)).font(FontId::monospace(12.0)).color(soft));
        }
        if !self.feedback.said.is_empty() {
            ui.label(RichText::new(&self.feedback.said).font(FontId::proportional(13.0)).color(soft));
        }
    }

    /// The Hub page: the hub itself, in this window, when Atlas is running.
    fn show_hub(&mut self, ui: &mut eframe::egui::Ui, frame: &eframe::Frame) {

        use eframe::egui::{self, FontId, RichText};
        let _ = frame;
        let say = |ui: &mut egui::Ui, s: &str| {
            ui.label(RichText::new(s).font(FontId::proportional(15.0)).color(crate::look_paint::colourway().soft));
        };
        if !self.running {
            self.hub.hide();
            ui.add_space(12.0);
            ui.label(RichText::new("Hub").font(FontId::proportional(26.0)).strong().color(crate::look_paint::colourway().text));
            say(ui, "The hub is Atlas's own pages — what it's doing, what's waiting for you, your devices, \
                     your settings. It's shown here while Atlas is running, and Atlas isn't running right now.");
            ui.add_space(8.0);
            if ui.button(RichText::new("Start Atlas").font(FontId::proportional(18.0))).clicked() {
                self.note = Some(match firstlaunch::spawn_quietly(&self.place.exe, &["--daemon"]) {
                    Ok(child) => {
                        crate::unwaited::dont_wait(child);
                        self.last_poll = None;
                        self.hub_go = true;
                        STARTING.into()
                    }
                    Err(e) => format!("I couldn't start: {e}"),
                });
            }
            if let Some(n) = &self.note {
                say(ui, n);
            }
            return;
        }
        let url = crate::hubwin::page_url(self.place.port, &self.place.token, &self.hub_page);
        if !crate::hubwin::Hub::can_embed() {
            ui.add_space(12.0);
            say(ui, "On Windows the hub shows right here. This system has no built-in web view for Atlas to \
                     borrow, so here is its address on this machine:");
            ui.label(RichText::new(&url).font(FontId::monospace(12.0)).color(crate::look_paint::colourway().dim));
            return;
        }
        if let Some(p) = self.hub.problem() {
            ui.add_space(12.0);
            say(ui, p);
            return;
        }
        let r = ui.available_rect_before_wrap();
        #[cfg(windows)]
        {
            let area = crate::hubwin::Area { x: r.min.x, y: r.min.y, w: r.width(), h: r.height() };
            let go = std::mem::take(&mut self.hub_go);
            self.hub.show(frame, area, &url, go);
        }
        ui.allocate_rect(r, egui::Sense::hover());
    }
}

const STARTING: &str = "Starting…";

/// What the window says once the background Atlas is answering.
pub const RUNNING_WORDS: &str = "Atlas is running in the background. You can close this window — Atlas keeps \
     running, and its icon by the clock opens it again.";

/// The small marker beside each step.
#[derive(Clone, Copy)]
enum Dot {
    Ring,
    Pulse,
    Filled,
}

/// Stop the background Atlas and start it again, off the window's thread, so
/// settings changed since it started take effect.
fn restart(place: &Place, note: &Arc<Mutex<Option<String>>>) {
    let place = place.clone();
    let note = Arc::clone(note);
    let say = move |s: &str| {
        if let Ok(mut n) = note.lock().or_else(crate::crash::unpoison) {
            *n = Some(s.to_string());
        }
    };
    say("Restarting Atlas…");
    std::thread::spawn(move || {
        if !firstlaunch::ask_atlas_to_stop(&place.root, std::time::Duration::from_secs(20)) {
            say("Atlas didn't stop when asked, so I left it running. Your changes take effect at its next start.");
            return;
        }
        match firstlaunch::start_background_watched(&place.exe, &place.root, std::time::Duration::from_secs(4)) {
            Ok(()) => say("Atlas restarted with your changes."),
            Err(why) => say(&format!("Atlas stopped but didn't start again. {why}")),
        }
    });
}

/// Paint a QR code: dark modules on a white square, quiet zone included.
fn paint_qr(ui: &mut eframe::egui::Ui, modules: &(usize, Vec<bool>), size: f32) {
    use eframe::egui::{self, Color32, Rect};
    let (width, dark) = modules;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Label, true, "QR code: scan it with your phone's camera to open Atlas there. The address is also written below.")
    });
    let painter = ui.painter();
    painter.rect_filled(rect, 0.0, Color32::WHITE);
    if *width == 0 {
        return;
    }
    let cell = size / *width as f32;
    for y in 0..*width {
        for x in 0..*width {
            if dark[y * width + x] {
                let min = rect.min + egui::vec2(x as f32 * cell, y as f32 * cell);
                painter.rect_filled(Rect::from_min_size(min, egui::vec2(cell.ceil(), cell.ceil())), 0.0, Color32::BLACK);
            }
        }
    }
}

/// The place this copy of Atlas is running from, ready for the window.
pub fn here(root: &Path, exe: &Path, port: u16) -> Result<Place, String> {
    // The same store the running Atlas reads its hub token from, so the
    // phone's link opens the hub this install actually serves.
    let token = crate::server::token_for(&crate::roots::store()).map_err(|e| e.to_string())?;
    Ok(Place { root: root.to_path_buf(), exe: exe.to_path_buf(), port: firstlaunch::hub_port_at(root, port), configured_port: port, token })
}
