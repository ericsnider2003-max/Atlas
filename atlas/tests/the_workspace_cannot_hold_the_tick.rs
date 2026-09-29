//! Bringing the workspace up must not take the tick with it.
//!
//! ## What was wrong
//!
//! `workspace_on` walks `apps.yaml`'s `startup_order` and, for each app,
//! waits for a window to appear: `retries` polls, `poll_ms` apart. Those
//! budgets are per-app and nothing added them up. The shipped file has claude
//! at 30 × 500ms and chrome at 40 × 750ms, which is 45 seconds between two of
//! them before the other two are counted.
//!
//! All of it runs on the tick thread, inside one pass of `Daemon::run`. For
//! the length of a slow bring-up that one pass is the whole of Atlas: nothing
//! listens, nothing answers, the dashboard is not served, a message from
//! another Atlas is not collected, and `OnlyOne::beat` does not run. The
//! staleness window is 150 seconds, and a worst-case bring-up was heading
//! towards it — at which point a second Atlas reads the lock as abandoned,
//! takes it, and two of them write the same state folder.
//!
//! And at the end of it the report named only `failed`, so a bring-up that
//! got two apps up and never reached the other two came back
//! "Workspace online."
//!
//! ## What is pinned here
//!
//! A whole-bring-up wall-clock budget; a heartbeat that keeps beating from
//! inside the wait; a way out while waiting; and a report that distinguishes
//! *this app never showed a window* from *we never got to this app*.
//!
//! ## What is NOT fixed, and should be said
//!
//! The bring-up still blocks the tick for as long as its budget allows. Doing
//! better means placing one app per pass and keeping the state between
//! passes, and it cannot go to `crew`: `crew.rs`'s own rule is that
//! background work owns everything it needs, and driving a window needs
//! `&dyn Platform` borrowed from the daemon. So this bounds the stall and
//! makes it survivable rather than removing it.

use atlas::config::Config;
use atlas::platform::mock::MockPlatform;
use atlas::config::AppSpec;
use atlas::platform::{Monitor, Platform, PixelRect, WindowId};
use atlas::workspace::{workspace_on_within, BRINGUP_BUDGET_SECS};
use std::path::Path;
use std::time::Duration;

/// A platform whose `sleep_ms` genuinely sleeps, and whose windows never
/// appear.
///
/// `MockPlatform::sleep_ms` only records the call, which is right for testing
/// what was asked for and useless for testing a wall clock. This wraps it so
/// the waiting is real, at a hundredth of the configured interval — enough
/// for elapsed time to move, fast enough to be a test.
struct SlowToOpen {
    inner: MockPlatform,
    slept: std::cell::RefCell<u64>,
}

impl SlowToOpen {
    fn new() -> Self {
        SlowToOpen {
            inner: MockPlatform::new(vec![
                Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true },
                Monitor { id: 2, x: 1920, y: 0, width: 1920, height: 1040, primary: false },
                Monitor { id: 3, x: 3840, y: 0, width: 1920, height: 1040, primary: false },
            ]),
            slept: std::cell::RefCell::new(0),
        }
    }
}

impl Platform for SlowToOpen {
    fn monitors(&self) -> atlas::error::Result<Vec<Monitor>> {
        self.inner.monitors()
    }
    fn launch(&self, spec: &AppSpec) -> atlas::error::Result<()> {
        self.inner.launch(spec)
    }
    /// Never. That is the case the budget exists for.
    fn find_window(&self, _spec: &AppSpec) -> atlas::error::Result<Option<WindowId>> {
        Ok(None)
    }
    fn place(&self, win: WindowId, rect: PixelRect) -> atlas::error::Result<()> {
        self.inner.place(win, rect)
    }
    fn focus(&self, win: WindowId) -> atlas::error::Result<()> {
        self.inner.focus(win)
    }
    fn close(&self, spec: &AppSpec) -> atlas::error::Result<()> {
        self.inner.close(spec)
    }
    /// The only override that matters: real waiting, at a hundredth of the
    /// configured interval, so elapsed time moves without the test taking
    /// the forty-five seconds the real budget allows.
    fn sleep_ms(&self, ms: u64) {
        *self.slept.borrow_mut() += 1;
        std::thread::sleep(Duration::from_millis((ms / 100).max(1)));
    }
}

impl SlowToOpen {
    /// How many times it actually waited. Read, so "the budget tripped" can
    /// be told apart from "nothing ever waited and the test proved nothing".
    fn waits(&self) -> u64 {
        *self.slept.borrow()
    }
}

fn conf() -> Config {
    Config::load(Path::new("config")).unwrap()
}

/// An install of our own, set before anything asks `roots` anything.
///
/// The bring-up beats the instance lock, and the lock lives under
/// `roots::data_dir()`. Without this that is the checkout's own `data/`, and
/// a `running.lock` left there makes a real `atlas --daemon` in this tree
/// refuse to start for the next 150 seconds -- blaming a test process that
/// finished. `roots` caches its answer in a `OnceLock`, so a second value
/// would be ignored rather than reported, which is why this is a `Once` and
/// why every test goes through it.
fn home() -> std::path::PathBuf {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let p = std::env::temp_dir().join(format!("atlas-bringup-{}", std::process::id()));
    ONCE.call_once(|| {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
    });
    p
}

#[test]
fn the_whole_bringup_has_a_budget_and_keeps_to_it() {
    home();
    let plat = SlowToOpen::new();
    let started = std::time::Instant::now();
    let report =
        workspace_on_within(&conf(), &plat, Duration::from_millis(300)).expect("a report");
    let took = started.elapsed();

    assert!(
        took < Duration::from_secs(3),
        "the bring-up ran {took:?} against a 300ms budget -- the budget is not being \
         checked, and on the shipped apps.yaml that is a minute and a half of Atlas \
         not listening"
    );
    assert!(
        !report.ok(),
        "nothing came up and the report says everything is fine"
    );
    assert!(
        plat.waits() > 0,
        "the bring-up never waited at all, so the budget was not what stopped it \
         and this test proves nothing"
    );
}

#[test]
fn an_app_never_reached_is_not_reported_as_an_app_that_failed() {
    // The half that made the stall invisible. `Report::ok` looked at `failed`
    // alone, so a bring-up that ran out of time before the last two apps
    // reported no failures -- and `report()` in the daemon turned that into
    // "Workspace online."
    home();
    let plat = SlowToOpen::new();
    let report =
        workspace_on_within(&conf(), &plat, Duration::from_millis(200)).expect("a report");

    assert!(
        !report.not_reached.is_empty(),
        "no app was recorded as never reached, so the apps the bring-up gave up \
         before are indistinguishable from apps that were never in the list"
    );
    for name in &report.not_reached {
        assert!(
            !report.failed.iter().any(|(f, _)| f == name),
            "{name} is reported both as failed and as never reached"
        );
    }
    assert!(
        report.plain().contains("ran out of time"),
        "the sentence does not say time ran out: {}",
        report.plain()
    );
}

#[test]
fn a_report_with_apps_left_unreached_is_not_ok() {
    home();
    let plat = SlowToOpen::new();
    let report =
        workspace_on_within(&conf(), &plat, Duration::from_millis(200)).expect("a report");
    assert!(
        !report.ok(),
        "a bring-up that never reached {:?} called itself ok, which is what came \
         out of the daemon as \"Workspace online.\"",
        report.not_reached
    );
}

#[test]
fn the_heartbeat_keeps_beating_while_the_bringup_waits() {
    // The consequence that costs state rather than patience. `Daemon::tick`
    // beats once a pass; a bring-up happens inside one pass; so for its whole
    // length nothing beat. Past GONE_AFTER_SECS a second Atlas takes the lock
    // and both write the same folder.
    home();
    let lock = atlas::onlyone::OnlyOne::at(&atlas::roots::data_dir());
    let _ = std::fs::remove_file(lock.path());

    let plat = SlowToOpen::new();
    let _ = workspace_on_within(&conf(), &plat, Duration::from_millis(400));

    let after = std::fs::read_to_string(lock.path()).unwrap_or_default();
    assert!(
        !after.is_empty(),
        "no heartbeat was written during the wait, so the lock goes stale while the \
         workspace comes up"
    );
    assert!(
        after.trim().parse::<u64>().is_ok(),
        "the heartbeat wrote something that is not a timestamp: {after:?}"
    );
}

#[test]
fn the_budget_is_stated_in_seconds_a_person_would_recognise() {
    // Not a magic number check -- a ceiling check. The point of the budget is
    // that it is comfortably under `onlyone::GONE_AFTER_SECS`, because that
    // is the arithmetic that keeps two Atlases off one state folder even if
    // the heartbeat above were removed.
    assert!(
        BRINGUP_BUDGET_SECS < atlas::onlyone::GONE_AFTER_SECS,
        "the bring-up may now run for {BRINGUP_BUDGET_SECS}s against a {}s staleness \
         window, so a slow morning can look like a dead Atlas to a second one",
        atlas::onlyone::GONE_AFTER_SECS
    );
    assert!(
        BRINGUP_BUDGET_SECS < atlas::onlyone::GONE_AFTER_SECS / 2,
        "the budget wants real headroom under the staleness window, not to sit just \
         inside it"
    );
}
