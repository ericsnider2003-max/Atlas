//! Proof that the automatic backup no longer blocks the tick.
//!
//! Before this, `Daemon::tick` called `back_up` directly, on the tick
//! thread. Copying a real vault takes real time, and a job taking four
//! minutes was four minutes in which Atlas did not listen, notice, report,
//! or answer. These tests exercise the real `Daemon`, not `crew.rs` in
//! isolation, so a wiring mistake between the two shows up here even if
//! each module's own tests still pass.
//!
//! A backup of an empty fresh store finishes fast enough that a raw "is it
//! still running right after this exact tick" check is a coin flip -- the
//! rest of `tick`'s own work is often slower than the errand it just
//! started. So these assert what actually matters and holds regardless of
//! that race: the work is genuinely handed to the crew (a `long_work` job
//! exists for it), and its result is reported once, for real, after it
//! actually finishes -- never before.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-off-tick-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

/// A fresh install has no prior backup, so `due_for_backup` fires on the
/// very first tick past the one-hour cooldown floor.
const FIRST_TICK: u64 = 4_000;

#[test]
fn a_due_backup_goes_through_the_crew_and_is_watched() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "hand-off");

    let started = Instant::now();
    d.tick(FIRST_TICK);
    let tick_took = started.elapsed();
    assert!(
        tick_took < Duration::from_secs(2),
        "starting a backup must not itself make the tick slow, took {tick_took:?}"
    );

    // Whether or not it has already finished by the time we check (a
    // near-empty store can copy in well under a millisecond), a job must
    // have been registered for it -- that only happens on the crew path.
    assert!(
        d.long_work.jobs.iter().any(|j| j.name == "backup" && j.process == "crew"),
        "a due backup must be registered with long_work, proving it went through the crew"
    );
}

#[test]
fn a_fast_successful_backup_is_logged_but_not_spoken() {
    // `watching`'s whole design is not to announce something that finished
    // fast -- "you were still sitting there" -- so a quick, successful
    // backup must land in the journal and be marked Finished without ever
    // becoming a spoken line. Only a slow one or a failure earns that.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "quiet-success");

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut settled = false;
    let mut t = FIRST_TICK;
    while Instant::now() < deadline {
        let said = d.tick(t);
        assert!(
            !said.iter().any(|l| l.contains("backed up")),
            "a fast, successful backup must never be spoken"
        );
        if d.crew.active() == 0 && d.long_work.jobs.iter().any(|j| j.name == "backup") {
            settled = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
    assert!(settled, "the backup should settle within the deadline");

    let job = d.long_work.jobs.iter().find(|j| j.name == "backup").expect("job recorded");
    assert_eq!(job.outcome, atlas::watching::Outcome::Finished);
    assert!(
        d.journal.events.iter().any(|e| e.what.starts_with("backup:") && e.ok),
        "the journal must carry a record of the backup even though nothing was said aloud"
    );
}

#[test]
fn housekeeping_reclaim_also_runs_through_the_crew() {
    // The retention walk (survey the whole data/ tree, decide what to
    // reclaim) used to run inline in the same hourly block as the backup
    // used to. Same proof, same reasoning: it must be handed to the crew,
    // not run on the tick.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "housekeeping-crew");

    d.tick(FIRST_TICK);
    assert!(
        d.long_work.jobs.iter().any(|j| j.name == "housekeeping" && j.process == "crew"),
        "the hourly retention pass must be registered with long_work, proving it went through the crew"
    );
}

#[test]
fn a_backup_already_in_flight_is_not_started_a_second_time() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "no-double-start");

    d.tick(FIRST_TICK);
    d.tick(FIRST_TICK + 1);
    d.tick(FIRST_TICK + 2);

    // Across three ticks in quick succession, at most one backup job should
    // ever have been created -- whether it already finished or is still
    // running, `due_for_backup` (real prior backup) and the one-hour
    // `last_backup` floor both forbid a second one this soon.
    let backups = d.long_work.jobs.iter().filter(|j| j.name == "backup").count();
    assert_eq!(backups, 1, "only one backup errand should exist across three immediate ticks");
}
