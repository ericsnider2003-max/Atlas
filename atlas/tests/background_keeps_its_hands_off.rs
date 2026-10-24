//! Background work never takes your mouse, keyboard or focus (Plan D, 24 Oct
//! 2026). Charter s7: Atlas must never make the computer unusable. A job
//! Atlas runs on its own -- research, a crew errand, a queued job -- works
//! headless, out of sight, while you keep typing; the only things that may
//! drive the screen are the ones you ask for in the moment.
//!
//! Three proofs: a research job run start to finish while you are "in Word"
//! leaves the fake OS's input log empty; the shipped config fetches and
//! browses headless, in Atlas's own profile; and the modules that run
//! unattended work contain no call that moves focus, clicks, scrolls, types
//! or presses keys.

use atlas::brain::{Llm, LlmConfig};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;
use std::time::{Duration, Instant};

struct StubLlm;
impl Llm for StubLlm {
    fn supports_bounded_chat(&self) -> bool {
        true
    }
    fn chat_until(
        &self,
        _: &atlas::brain::ChatRequest,
        on_text: &mut dyn FnMut(&str) -> bool,
        keep_going: &dyn Fn() -> bool,
    ) -> atlas::error::Result<atlas::brain::ChatReply> {
        let text = "Tide times at Ventura peak just after noon. Wear sandals.";
        if !keep_going() || !on_text(text) {
            return Err(atlas::error::AtlasError::Platform("stopped".into()));
        }
        Ok(atlas::brain::ChatReply {
            text: text.into(),
            ..Default::default()
        })
    }
    fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
        Ok("Tide times at Ventura peak just after noon. Wear sandals.".into())
    }
}

/// Research on, pointed at local stand-ins so no network is needed.
fn cfg() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_mut().expect("tools");
    tools.research.enabled = true;
    tools.research.pages_on_this_machine = true;
    tools.research.search = Some(crate::common::printing("http://example.test/tides"));
    tools.research.fetch = Some(crate::common::printing(
        &"Ventura tide information. ".repeat(20),
    ));
    tools.llm = Some(LlmConfig {
        tool: Default::default(),
        request: r#"{"model":"stub","prompt":"{user}"}"#.into(),
        response_path: "response".into(),
        vision_request: None,
    });
    c
}

/// What would take the screen from you.
fn hands_on(a: &Action) -> bool {
    matches!(
        a,
        Action::Focus(_)
            | Action::Click(..)
            | Action::Scroll(..)
            | Action::Type(_)
            | Action::Press(_)
            | Action::Place(..)
            | Action::Launch(_)
    )
}

#[test]
fn a_research_job_runs_start_to_finish_without_touching_your_screen() {
    let c = cfg();
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    p.focus_on("winword.exe", "Quarterly report.docx");
    let root = std::env::temp_dir().join(format!("atlas-hands-off-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let mut d = Daemon::new(
        &c,
        &p,
        Some(std::sync::Arc::new(StubLlm)),
        Store::new(&root),
        Proactive::new(ProactiveConfig::default()),
    );
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);

    d.turn("research tide times at Ventura", 100);
    assert_eq!(
        d.crew.active(),
        1,
        "the job should be running in the background"
    );
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut t = 101;
    let mut answered = false;
    while Instant::now() < deadline && !answered {
        answered = d
            .tick(t)
            .iter()
            .any(|l| l.to_lowercase().contains("sandals"));
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
    assert!(answered, "the background job never finished");

    let took: Vec<String> = p
        .log
        .borrow()
        .iter()
        .filter(|a| hands_on(a))
        .map(|a| format!("{a:?}"))
        .collect();
    assert!(
        took.is_empty(),
        "background work touched your screen: {took:?}"
    );
    let still = p.active.borrow().clone().expect("a window in front");
    assert_eq!(
        (still.process.as_str(), still.title.as_str()),
        ("winword.exe", "Quarterly report.docx"),
        "your window lost the front"
    );
}

#[test]
fn the_shipped_config_fetches_and_browses_headless_in_its_own_profile() {
    let c = Config::load(Path::new("config")).unwrap();
    let tools = c.tools.as_ref().expect("tools");
    let fetch = tools.research.fetch.as_ref().expect("a page fetcher");
    assert!(
        fetch.args.iter().any(|a| a.starts_with("--headless")),
        "research fetches pages in a visible window: {:?}",
        fetch.args
    );
    let launch = tools.browser.launch.as_ref().expect("a browser launch");
    assert!(
        launch.args.iter().any(|a| a.starts_with("--headless")),
        "the browser Atlas drives is visible: {:?}",
        launch.args
    );
    assert!(
        launch
            .args
            .iter()
            .any(|a| a.starts_with("--user-data-dir=")),
        "the browser Atlas drives shares your own profile: {:?}",
        launch.args
    );
}

/// The modules that run work nobody is watching: the crew and its lanes,
/// research, errands, and queued jobs.
const UNATTENDED: &[&str] = &[
    "src/crew.rs",
    "src/lanes.rs",
    "src/research.rs",
    "src/hubjobs.rs",
    "src/which_errand.rs",
    "src/daemon/errands.rs",
];

#[test]
fn nothing_that_runs_unattended_can_move_focus_click_scroll_or_type() {
    let calls = [
        ".focus(",
        ".click(",
        ".scroll(",
        ".type_text(",
        ".press(",
        ".press_named(",
        ".place(",
        ".launch(",
    ];
    let mut found = Vec::new();
    for file in UNATTENDED {
        let src = std::fs::read_to_string(file).unwrap_or_else(|e| panic!("{file}: {e}"));
        for (n, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or("");
            for c in calls {
                if code.contains(c) {
                    found.push(format!("{file}:{}: {}", n + 1, line.trim()));
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "unattended work reaches for the screen -- route it through something you asked for:\n  {}",
        found.join("\n  ")
    );
}

#[test]
fn the_scan_would_see_a_call_if_one_were_there() {
    // Not a vacuous pass: the same scan over a module that does drive the
    // screen on request (dictation) finds its typing call.
    let src = std::fs::read_to_string("src/daemon/away.rs").unwrap();
    assert!(src
        .lines()
        .any(|l| l.split("//").next().unwrap_or("").contains(".type_text(")));
}

/// 24 Oct 2026 (Plan D hand-off): the guard is wired under the platform, not
/// just checked in the source. A scheduled job or queued task that reaches
/// for focus or a launch is refused and logged; the same words, from your own
/// turn, go through.
#[test]
fn a_background_job_that_reaches_for_your_screen_is_refused_and_you_are_not() {
    let c = Config::load(Path::new("config")).unwrap();
    let mon = || vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }];
    let root = std::env::temp_dir().join(format!("atlas-hands-guard-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let p = MockPlatform::new(mon());
    p.focus_on("winword.exe", "Quarterly report.docx");
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));
    let touched = |p: &MockPlatform| -> usize { p.log.borrow().iter().filter(|a| hands_on(a)).count() };

    // A scheduled job you approved, due now.
    let id = d.scheduler.at("switch to claude", 1);
    d.scheduler.approve(id);
    // A queued task.
    d.queue.push("switch to claude", atlas::lanes::Lane::Background);
    d.tick(10);
    assert_eq!(touched(&p), 0, "background work touched the screen: {:?}", p.log.borrow());
    let still = p.active.borrow().clone().expect("a window in front");
    assert_eq!(still.process, "winword.exe", "your window lost the front");
    // Not vacuous: both ran, and each says why it did nothing.
    let said = d.scheduler.jobs[0].last_result.clone().unwrap_or_default();
    assert!(said.contains("kept my hands off"), "the scheduled job's result: {said:?}");
    let queued = d.queue.tasks[0].result.clone().unwrap_or_default();
    assert!(queued.contains("kept my hands off"), "the queued task's result: {queued:?}");

    // The same words from your own turn proceed: it brings the app up.
    d.turn("switch to claude", 20);
    assert!(touched(&p) > 0, "your own request was refused too: {:?}", p.log.borrow());
}
