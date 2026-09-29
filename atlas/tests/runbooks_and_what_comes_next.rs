//! Runbook shape for Atlas's procedures (wshobson's `incident-response`):
//! each step says how you know it worked and what to do if it didn't; a
//! quick checklist comes first; and when a procedure doesn't work, what went
//! wrong is kept and a blameless look back is filed.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::intent::Intent;
use atlas::knowhow::{as_plan, checklist, look_back, Knowhow, Snag};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-runbook-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_step_says_what_to_do_when_it_doesnt_work() {
    let k = Knowhow::shipped();
    let mem = k.procedures.iter().find(|p| p.id == "free-up-memory").unwrap();
    let plan = as_plan(mem);
    assert!(plan[0].contains("(until something over 200MB and untouched)"), "{}", plan[0]);
    assert!(plan[0].contains("— if not: nothing stands out"), "{}", plan[0]);
    // Every step that can fail in a way worth handling says how.
    let with = k.procedures.iter().flat_map(|p| &p.steps).filter(|s| s.if_it_fails.is_some()).count();
    assert!(with >= 20, "only {with} steps say what to do if they fail");
    let quick = checklist(k.procedures.iter().find(|p| p.id == "research-something").unwrap());
    assert!(quick.starts_with("Needs: the internet."), "{quick}");
    assert!(quick.contains(" → "), "{quick}");
}

#[test]
fn a_look_back_asks_why_and_never_who() {
    let k = Knowhow::shipped();
    let p = &k.procedures[0];
    let note = look_back(p, "memory refilled straight away", Some("something restarts itself"));
    assert!(note.starts_with("# Look back: make room when memory is tight"));
    assert!(note.contains("**What happened:** memory refilled straight away"));
    assert!(note.contains("1. something restarts itself\n2. Why did that happen? (not known yet)"), "{note}");
    assert!(note.contains("no blame"));
    let open = look_back(p, "x", None);
    assert!(open.contains("1. Why did it happen? (not known yet)"), "{open}");
}

#[test]
fn what_goes_wrong_is_kept_across_a_restart() {
    let store = Store::new(scratch("kept"));
    let snag = Snag { looks_like: "the fan screams".into(), cause: "a build".into(), fix: "wait".into() };
    assert!(Knowhow::learn_and_keep(&store, "free-up-memory", snag.clone()));
    assert!(!Knowhow::learn_and_keep(&store, "free-up-memory", snag), "kept twice");
    let k = Knowhow::load(&store);
    let mem = k.procedures.iter().find(|p| p.id == "free-up-memory").unwrap();
    assert!(mem.snags.iter().any(|s| s.looks_like == "the fan screams"));
    assert_eq!(k.for_symptom("the fan screams").map(|(p, _)| p.id.as_str()), Some("free-up-memory"));
}

#[test]
fn saying_it_didnt_work_after_a_walk_through_teaches_the_procedure() {
    let root = scratch("walk");
    let mut cfg = Config::load(Path::new("config")).unwrap();
    cfg.tools.as_mut().unwrap().research.notes_dir = root.join("notes").display().to_string();
    let cfg: &'static Config = Box::leak(Box::new(cfg));
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(cfg, &p, None, Store::new(root.join("state")), Proactive::new(ProactiveConfig::default()));
    let walked = d.turn("walk me through freeing up memory", 100);
    assert!(walked.contains("here's how:") && walked.contains("In short:"), "{walked}");
    let said = d.got_it_wrong("that's wrong, the memory filled right back up", 101);
    assert!(said.contains("look back"), "{said}");
    assert!(said.contains("Why do you think it happened?"), "{said}");
    let k = Knowhow::load(&Store::new(root.join("state")));
    assert!(k.procedures[0].snags.iter().any(|s| s.looks_like.contains("filled right back up")));
    let notes: Vec<_> = std::fs::read_dir(root.join("notes")).unwrap().flatten().collect();
    assert_eq!(notes.len(), 1);
    let _ = Intent::WalkThrough(String::new());
}

// ---------------------------------------------------------------- what comes first

#[test]
fn what_needs_you_now_is_said_before_what_went_wrong_earlier() {
    use atlas::returning::{welcome, Happened, ReturnConfig, Welcome};
    let h = |what: &str, needs_you: bool, failed: bool, at: u64| Happened { what: what.into(), needs_you, failed, at };
    let happened = vec![
        h("the backup failed", false, true, 1_000),
        h("the lunchtime post failed", false, true, 2_000),
        h("the council wants your call on the phone link", true, false, 9_000),
    ];
    let cfg = ReturnConfig { name_at_most: 2, ..Default::default() };
    let Welcome::Straight(s) = welcome(4 * 3600, &happened, 15, &cfg) else { panic!() };
    // Before: "the backup failed, and the lunchtime post failed, and 1 more".
    assert!(s.find("council").unwrap() < s.find("lunchtime").unwrap(), "the decision waiting on you should come first: {s}");
    assert!(s.contains("the lunchtime post failed"), "the newer failure should come before the older one: {s}");
    assert!(s.contains("and 1 more"), "{s}");
    assert!(!s.contains("backup"), "{s}");
}

#[test]
fn the_top_few_are_picked_and_the_rest_counted() {
    let p = atlas::next_up::top(&[1, 5, 3, 5, 2], |x| *x != 2, |x| *x as f64, 2);
    assert_eq!(p.chosen, vec![5, 5]);
    assert_eq!(p.passed_over, 2);
    let none = atlas::next_up::top(&[1, 2], |_| false, |x: &i32| *x as f64, 3);
    assert!(none.chosen.is_empty() && none.passed_over == 0);
}

// ---------------------------------------------------------------- hollow: never called, made-up packages, Fix:

#[test]
fn a_private_function_nothing_calls_is_found_and_a_used_one_isnt() {
    use atlas::hollowcode::{read, Shape, Tongue};
    let rust = "fn used() -> u32 { 1 }\nfn forgotten() -> u32 { 2 }\npub fn api() -> u32 { used() }\n\nimpl Llm for X {\n    fn complete(&self) {}\n}\n";
    let found = read(rust, Tongue::Rust);
    let never: Vec<_> = found.iter().filter(|f| f.shape == Shape::NeverCalled).collect();
    assert_eq!(never.len(), 1, "{found:?}");
    assert!(never[0].code.contains("forgotten"));
    let py = "def _helper():\n    return 1\n\ndef _unused():\n    return 2\n\ndef public():\n    return _helper()\n";
    let found = read(py, Tongue::Python);
    let never: Vec<_> = found.iter().filter(|f| f.shape == Shape::NeverCalled).map(|f| f.line).collect();
    assert_eq!(never, vec![4], "{found:?}");
}

#[test]
fn a_package_the_project_never_lists_is_flagged_against_its_manifest() {
    use atlas::hollowcode::{made_up_dependencies, Tongue};
    let py = "import os\nimport requests\nimport totally_real_ai_sdk\nfrom .local import thing\n";
    let found = made_up_dependencies(py, Tongue::Python, "requests==2.31\n");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].code.contains("totally_real_ai_sdk"));
    let js = "const fs = require('fs');\nimport x from 'lodash/get';\nimport y from '@acme/ui/button';\nimport z from 'left-padder';\nimport w from './mine';\n";
    let found = made_up_dependencies(js, Tongue::JavaScript, r#"{"dependencies": {"lodash": "4", "@acme/ui": "1"}}"#);
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].code.contains("left-padder"));
    let rs = "use std::fs;\nuse serde::Deserialize;\nuse tokio_magic::run;\nuse crate::x;\n";
    let found = made_up_dependencies(rs, Tongue::Rust, "[dependencies]\nserde = \"1\"\n");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].code.contains("tokio_magic"));
}

#[test]
fn a_manifest_is_found_near_the_file() {
    let root = scratch("manifest");
    std::fs::create_dir_all(root.join("src/deep")).unwrap();
    std::fs::write(root.join("requirements.txt"), "requests\n").unwrap();
    std::fs::write(root.join("src/deep/app.py"), "import requests\n").unwrap();
    assert_eq!(atlas::hollowcode::manifest_near(&root.join("src/deep/app.py")).as_deref(), Some("requests\n"));
}

#[test]
fn every_finding_says_how_to_fix_it() {
    use atlas::hollowcode::{read, spoken, Tongue};
    let py = "def save(x):\n    try:\n        write(x)\n    except: pass\n";
    let said = spoken("save.py", Tongue::Python, &read(py, Tongue::Python));
    assert!(said.contains("Fix: handle the error, or pass it up"), "{said}");
}
