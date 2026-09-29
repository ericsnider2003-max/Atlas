//! Eric's rulings of 25 Sep 2026 on Atlas acting on its own (E1–E4):
//!
//! - E1: Atlas fixes its own things without asking.
//! - E2: it starts improving its own code from its own findings; the change
//!   still lands only on his OK.
//! - E3: long unattended jobs, with a limit that isn't ridiculously small.
//! - E4: concrete, ordinary routines run on their own; abstract ones ask first
//!   ("Your usual morning setup?").

use atlas::brain::Llm;
use atlas::build_it::{Check, Outcome, Struggle};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::goal::LongJobConfig;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::routine::{RoutineConfig, Watcher};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-own-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ------------------------------------------------------------------ E1

#[test]
fn a_missing_scratch_folder_is_remade_without_asking_and_logged() {
    let root = scratch("e1");
    let missing = root.join("gone").join("tmp");
    let mut cfg = Config::load(Path::new("config")).unwrap();
    cfg.tools.as_mut().unwrap().work_dir = missing.display().to_string();
    let cfg: &'static Config = Box::leak(Box::new(cfg));
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(cfg, &p, None, Store::new(root.join("state")), Proactive::new(ProactiveConfig::default()));
    assert!(!missing.exists());
    let done = d.fix_my_own_things(1_000);
    assert!(done.contains(&"scratch".to_string()), "{done:?}");
    assert!(missing.is_dir(), "the scratch folder was remade");
}

// ------------------------------------------------------------------ E2

#[test]
fn its_own_finding_carries_its_diagnosis_into_the_work() {
    let cfg: atlas::selfaudit::SelfAuditConfig =
        serde_yaml::from_str("enabled: true\nact_without_asking: true\n").unwrap();
    let rec = atlas::selfaudit::Recommendation {
        symptom: "replies run long".into(),
        cause: "the brevity rule is read after the draft".into(),
        where_: "src/prose.rs".into(),
        proof: "tests::short_replies".into(),
        certainty: 0.8,
        because: "you cut five replies this week".into(),
    };
    let u = atlas::selfaudit::unprompted(&cfg, &[rec]).unwrap();
    let t = u.thought.expect("the diagnosis goes with it");
    assert_eq!(t.cause, "the brevity rule is read after the draft");
    assert!(!t.proof_fails_now, "never assumed: the pipeline runs the proof itself");
}

// ------------------------------------------------------------------ E3

/// A model that gets it right on the `right_on`-th fix, or never.
struct Fixer {
    calls: AtomicU32,
    right_on: u32,
    same_every_time: bool,
}

impl Llm for Fixer {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        let n = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        if self.same_every_time {
            return Ok("```\nfn main() { broken }\n```".into());
        }
        Ok(if n >= self.right_on {
            "```\nfn main() { works }\n```".to_string()
        } else {
            format!("```\nfn main() {{ try {n} }}\n```")
        })
    }
}

fn struggle() -> Struggle {
    Struggle {
        description: "rename files by date".into(),
        lang: atlas::craft::Lang::Rust,
        code: "fn main() { broken }".into(),
        failure: "error[E0425]".into(),
    }
}

fn checker(code: &str) -> Check {
    if code.contains("works") {
        Check::Passed(vec![])
    } else {
        Check::Failed("error[E0425]: not found".into())
    }
}

#[test]
fn a_long_job_keeps_going_until_it_passes_well_past_the_old_three_rounds() {
    let llm = Fixer { calls: AtomicU32::new(0), right_on: 12, same_every_time: false };
    let (outcome, said) = atlas::build_it::keep_building(&struggle(), &llm, &LongJobConfig::default(), checker, || false);
    assert!(matches!(outcome, Outcome::Built { rounds: 12, .. }), "{outcome:?}");
    assert!(said.contains("done, and it checks out"), "{said}");
}

#[test]
fn a_long_job_stops_when_it_is_going_in_circles_or_out_of_attempts_or_told() {
    // The same draft twice: nothing is changing, so stop.
    let llm = Fixer { calls: AtomicU32::new(0), right_on: 0, same_every_time: true };
    let (outcome, said) = atlas::build_it::keep_building(&struggle(), &llm, &LongJobConfig::default(), checker, || false);
    assert!(matches!(outcome, Outcome::Struggled { rounds: 2, .. }), "{outcome:?}");
    assert!(said.starts_with("I seem to be stuck on rename files by date for "), "{said}");

    // Out of attempts.
    let llm = Fixer { calls: AtomicU32::new(0), right_on: 1_000, same_every_time: false };
    let small = LongJobConfig { max_attempts: 5, max_hours: 8 };
    let (_, said) = atlas::build_it::keep_building(&struggle(), &llm, &small, checker, || false);
    assert!(said.contains("gave up after 5"), "{said}");

    // Told to stop.
    let llm = Fixer { calls: AtomicU32::new(0), right_on: 1_000, same_every_time: false };
    let mut asked = 0;
    let (_, said) = atlas::build_it::keep_building(&struggle(), &llm, &LongJobConfig::default(), checker, || {
        asked += 1;
        asked > 3
    });
    assert!(said.contains("stopped when you asked, after 3"), "{said}");
}

#[test]
fn the_limit_is_not_ridiculously_small() {
    let l = LongJobConfig::default();
    assert!(l.max_attempts >= 50 && l.max_hours >= 8);
    let shipped = Config::load(Path::new("config")).unwrap().tools.unwrap().long_jobs;
    assert_eq!((shipped.max_attempts, shipped.max_hours), (50, 8));
}

// ------------------------------------------------------------------ E4

fn cfg_on() -> RoutineConfig {
    RoutineConfig { enabled: true, ..RoutineConfig::default() }
}

fn three_mornings(w: &mut Watcher, steps: &[&str]) {
    for day in 0..3u64 {
        let base = 1_000_000 + day * 86_400;
        for (i, s) in steps.iter().enumerate() {
            w.did(s, base + i as u64 * 60, 8, day as u32);
        }
    }
}

#[test]
fn a_routine_seen_three_times_is_asked_about_once_by_a_name_you_would_use() {
    let mut w = Watcher::default();
    three_mornings(&mut w, &["check my mail", "show my agenda"]);
    let r = w.take_new(&cfg_on()).expect("three times is worth asking");
    assert_eq!(r.name, "morning setup");
    assert!(w.take_new(&cfg_on()).is_none(), "asked once");
    assert_eq!(atlas::routine::starting(&r), "Your usual morning setup?");
}

#[test]
fn concrete_routines_run_on_their_own_and_abstract_ones_ask() {
    assert!(atlas::routine::is_concrete(&["check my mail".into(), "show my agenda".into()]));
    assert!(atlas::routine::is_concrete(&["remind me to stretch at 3".into(), "back up".into()]));
    assert!(!atlas::routine::is_concrete(&["research solar panels".into(), "show my agenda".into()]));
    assert!(!atlas::routine::is_concrete(&["check my mail".into(), "send the report to sam".into()]), "anything that sends asks");
}

#[test]
fn a_due_routine_runs_once_a_day_and_a_no_forgets_it() {
    let mut w = Watcher::default();
    three_mornings(&mut w, &["check my mail", "show my agenda"]);
    let r = w.take_new(&cfg_on()).unwrap();
    assert!(w.confirm(&r.name, true));
    assert_eq!(w.due_today(8, 3, 100).len(), 1);
    assert!(w.due_today(8, 3, 100).is_empty(), "not twice the same day");
    assert_eq!(w.due_today(8, 4, 101).len(), 1, "tomorrow again");

    let mut w = Watcher::default();
    three_mornings(&mut w, &["open spotify", "show my agenda"]);
    let r = w.take_new(&cfg_on()).unwrap();
    w.declined(&r.name);
    assert!(w.routines.is_empty());
    assert!(w.take_new(&cfg_on()).is_none(), "not found and asked again");
}
