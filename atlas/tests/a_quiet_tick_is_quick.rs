//! **A quiet tick is quick (30 Sep 2026).**
//!
//! The laptop's log had ticks of 200 ms to 1.8 s with nothing going on, and
//! every tick is time the hub and the typing box wait. The tick now names
//! its slowest parts in the log when it's slow (`timing::Laps`); this holds
//! a quiet tick on the stand-in platform well under the loop's own "slow"
//! line, so what's slow on a real machine is the machine's calls, not
//! Atlas's own work.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::Path;

#[test]
fn ticks_after_the_first_take_well_under_the_slow_line() {
    let dir = std::env::temp_dir().join(format!("atlas-quiet-tick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut c = Config::load(Path::new("config")).unwrap();
    // The shipped config indexes the person's own Desktop, Documents and
    // Downloads; on a real machine that walk is hundreds of milliseconds of
    // the machine's own work, not what this test measures.
    c.indexing = None;
    let p = MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }]);
    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(dir.clone()),
        Proactive::new(ProactiveConfig::default()),
    );
    let t0 = 1_790_700_000u64;
    let first = std::time::Instant::now();
    d.tick(t0);
    eprintln!("first tick: {:?}", first.elapsed());
    let mut laps = Vec::new();
    for i in 1..12 {
        let s = std::time::Instant::now();
        d.tick(t0 + i * 2);
        laps.push(s.elapsed());
    }
    laps.sort();
    // The median, not the worst: one scheduler hiccup or a backup holding the
    // state under parallel load must not fail the run, but a tick that is
    // slow every time (what the laptop logged) still does.
    let median = laps[laps.len() / 2];
    eprintln!(
        "median of the next eleven: {median:?}, worst {:?}",
        laps[laps.len() - 1]
    );
    // Debug build, a shared machine: generous, and still a fifth of what
    // the laptop logged.
    crate::common::assert_prompt(
        median,
        std::time::Duration::from_millis(400),
        "a quiet tick (median)",
    );
    let _ = std::fs::remove_dir_all(&dir);
}
