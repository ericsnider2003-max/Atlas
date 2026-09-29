use atlas::diagnose::{detail, diagnose, needs_permission, report, self_fixable, yours, Impact, Remedy, Vitals};
use atlas::safety::{Trash, TrashConfig};
use atlas::sandbox::Sandbox;
use atlas::tools::ExternalTool;
use atlas::wants::{ask, recommend, Cost, Machine, Observations};
use std::fs;
use std::path::PathBuf;

fn base(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-self-{tag}"));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn sh(script: &str) -> ExternalTool {
    let (command, first) = if cfg!(windows) { ("cmd", "/c") } else { ("sh", "-c") };
    ExternalTool { command: command.into(), args: vec![first.into(), script.into()], ..Default::default() }
}

// ================= somewhere for Atlas to be wrong =================

#[test]
fn atlas_can_write_and_read_its_own_work() {
    let b = base("write");
    let s = Sandbox::create(&b, "fix the parser").unwrap();
    s.write("src/thing.rs", "fn main() {}").unwrap();
    assert_eq!(s.read("src/thing.rs").unwrap(), "fn main() {}");
    assert_eq!(s.files().len(), 1);
}

#[test]
fn nothing_can_be_written_outside_the_sandbox() {
    // The single property the whole idea rests on.
    let b = base("escape");
    let s = Sandbox::create(&b, "x").unwrap();
    for bad in ["../outside.txt", "../../etc/passwd", "a/../../b.txt"] {
        assert!(s.write(bad, "x").is_err(), "{bad} should have been refused");
    }
    assert!(s.write("/tmp/absolute.txt", "x").is_err(), "absolute paths too");
}

#[test]
fn a_climbing_path_is_refused_rather_than_normalised() {
    let b = base("climb");
    let s = Sandbox::create(&b, "x").unwrap();
    let e = s.resolve("../../thing").unwrap_err().to_string();
    assert!(e.contains("climb out"), "got: {e}");
}

#[test]
fn atlas_can_run_its_work_and_see_whether_it_passed() {
    let b = base("run");
    let mut s = Sandbox::create(&b, "x").unwrap();
    let ok = s.run(&sh("echo it worked"), &Default::default(), 4000);
    assert!(ok.passed);
    assert!(ok.output.contains("it worked"));
    assert!(s.settled());

    let bad = s.run(&sh("exit 3"), &Default::default(), 4000);
    assert!(!bad.passed);
    assert!(!s.settled(), "the latest attempt is what counts");
    assert_eq!(s.attempts.len(), 2, "every attempt is kept");
}

#[test]
fn a_wall_of_compiler_output_is_trimmed_but_keeps_both_ends() {
    let long = format!("error: first thing\n{}\nsummary at the end", "noise\n".repeat(5000));
    let t = atlas::sandbox::trim_output(&long, 400);
    assert!(t.len() < 700);
    assert!(t.contains("error: first thing"), "the first error must survive");
    assert!(t.contains("summary at the end"), "and the summary");
}

#[test]
fn the_first_error_can_be_picked_out_for_saying_aloud() {
    let b = base("firsterr");
    let mut s = Sandbox::create(&b, "x").unwrap();
    let a = s.run(&sh("echo warning: fine && echo error: linker not found && exit 1"), &Default::default(), 4000);
    assert_eq!(a.first_problem().as_deref(), Some("error: linker not found"));
}

#[test]
fn nothing_reaches_your_machine_without_an_explicit_yes() {
    let b = base("promote");
    let s = Sandbox::create(&b, "x").unwrap();
    s.write("out.txt", "new content").unwrap();
    let target = b.join("real/out.txt");
    let plan = s.plan(&[("out.txt".into(), target.clone())]).unwrap();

    let trash = Trash::new(TrashConfig { dir: b.join("trash").display().to_string(), keep_days: 30 });
    assert!(Sandbox::promote(&plan, &trash, false).is_err(), "must refuse without approval");
    assert!(!target.exists());

    assert_eq!(Sandbox::promote(&plan, &trash, true).unwrap(), 1);
    assert_eq!(fs::read_to_string(&target).unwrap(), "new content");
}

#[test]
fn accepting_a_change_is_still_undoable() {
    let b = base("undoable");
    let s = Sandbox::create(&b, "x").unwrap();
    s.write("out.txt", "the new version").unwrap();
    let target = b.join("out.txt");
    fs::write(&target, "the version you had").unwrap();

    let trash = Trash::new(TrashConfig { dir: b.join("trash").display().to_string(), keep_days: 30 });
    let plan = s.plan(&[("out.txt".into(), target.clone())]).unwrap();
    Sandbox::promote(&plan, &trash, true).unwrap();

    assert_eq!(fs::read_to_string(&target).unwrap(), "the new version");
    assert_eq!(trash.ledger().len(), 1, "the old one is recoverable");
}

#[test]
fn the_plan_says_what_would_change_before_you_agree() {
    let b = base("describe");
    let s = Sandbox::create(&b, "x").unwrap();
    s.write("a.rs", "x").unwrap();
    s.write("b.rs", "y").unwrap();
    let existing = b.join("b.rs");
    fs::write(&existing, "old").unwrap();

    let plan = s
        .plan(&[("a.rs".into(), b.join("a.rs")), ("b.rs".into(), existing)])
        .unwrap();
    let said = Sandbox::describe(&plan);
    assert!(said.contains("1 new, 1 replaced"), "got: {said}");
    assert!(said.ends_with("Apply?"));
}

#[test]
fn promoting_a_file_that_was_never_written_is_an_error_not_a_silent_skip() {
    let b = base("missing");
    let s = Sandbox::create(&b, "x").unwrap();
    assert!(s.plan(&[("never.rs".into(), b.join("never.rs"))]).is_err());
}

#[test]
fn throwing_the_work_away_touches_nothing_else() {
    let b = base("discard");
    let keep = b.join("yours.txt");
    fs::write(&keep, "untouched").unwrap();
    let s = Sandbox::create(&b, "x").unwrap();
    s.write("scratch.rs", "x").unwrap();
    let root = s.root.clone();
    s.discard().unwrap();
    assert!(!root.exists());
    assert!(keep.exists(), "your files are not in the sandbox");
}

// ================= checking on itself =================

fn healthy() -> Vitals {
    Vitals {
        config_loaded: true,
        state_writable: true,
        disk_free_gb: 200.0,
        ram_used_fraction: 0.5,
        backups_ever: 3,
        ..Default::default()
    }
}

#[test]
fn a_healthy_atlas_says_so_plainly() {
    assert!(diagnose(&healthy()).is_empty());
    assert_eq!(report(&[]), "I'm running properly.");
}

#[test]
fn something_atlas_created_is_fixed_without_asking() {
    // Its own scratch folder. No permission needed to remake it.
    let v = Vitals { scratch_dir_missing: true, ..healthy() };
    let s = diagnose(&v);
    assert_eq!(self_fixable(&s).len(), 1);
    assert!(needs_permission(&s).is_empty());
}

#[test]
fn anything_touching_your_machine_is_offered_not_done() {
    let v = Vitals { backups_ever: 0, ram_used_fraction: 0.97, ..healthy() };
    let s = diagnose(&v);
    let offers = needs_permission(&s);
    assert_eq!(offers.len(), 2);
    for o in offers {
        match &o.remedy {
            Remedy::Offer { what_changes, .. } => {
                assert!(!what_changes.is_empty(), "an offer must say what it would change")
            }
            other => panic!("{other:?}"),
        }
    }
}

#[test]
fn a_missing_program_is_yours_to_install_and_atlas_says_which() {
    let v = Vitals { missing_tools: vec!["whisper-cli".into()], ..healthy() };
    let s = diagnose(&v);
    assert_eq!(yours(&s).len(), 1);
    assert!(s[0].what.contains("whisper-cli"));
}

#[test]
fn a_fatal_problem_leads_and_crowds_out_everything_else() {
    let v = Vitals {
        state_writable: false,
        missing_tools: vec!["piper".into()],
        backups_ever: 0,
        ..healthy()
    };
    let s = diagnose(&v);
    assert_eq!(s[0].impact, Impact::Fatal);
    let said = report(&s);
    assert!(said.contains("can't write"), "got: {said}");
    assert!(!said.contains("piper"), "one thing at a time when it's serious: {said}");
}

#[test]
fn unreadable_state_is_reported_as_handled_not_as_data_loss() {
    let v = Vitals { preserved_files: 2, ..healthy() };
    let s = diagnose(&v);
    assert!(s[0].what.contains("set aside"));
    assert!(matches!(s[0].remedy, Remedy::Self_ { .. }));
}

#[test]
fn a_problem_atlas_does_not_understand_says_so_rather_than_guessing() {
    let v = Vitals { recent_failures: 22, ..healthy() };
    let s = diagnose(&v);
    assert!(matches!(s[0].remedy, Remedy::Unknown));
    assert!(detail(&s).contains("don't know what's causing this"));
}

#[test]
fn the_detailed_report_labels_each_problem_by_how_bad_it_is() {
    let v = Vitals { state_writable: false, scratch_dir_missing: true, ..healthy() };
    let d = detail(&diagnose(&v));
    assert!(d.contains("[STOPS ME]"));
    assert!(d.contains("[degraded]"));
}

// ================= noticing what it lacks =================

fn laptop() -> Machine {
    Machine::default()
}

#[test]
fn nothing_is_recommended_without_evidence() {
    let obs = Observations::default();
    assert!(recommend(&obs, &laptop()).is_empty());
    assert_eq!(ask(&[]), "Nothing I'm missing.");
}

#[test]
fn a_fast_stage_prompts_nothing() {
    let mut obs = Observations::default();
    for i in 0..10 {
        obs.time("transcribe", 900, i);
    }
    assert!(recommend(&obs, &laptop()).is_empty());
}

#[test]
fn a_slow_stage_prompts_a_recommendation_with_the_measurement_attached() {
    let mut obs = Observations::default();
    for i in 0..10 {
        obs.time("transcribe", 6000, i);
    }
    let r = recommend(&obs, &laptop());
    assert!(!r.is_empty());
    assert!(r[0].because.contains("6.0 seconds"), "got: {}", r[0].because);
}

#[test]
fn the_typical_time_ignores_a_single_cold_start() {
    let mut obs = Observations::default();
    for i in 0..9 {
        obs.time("think", 800, i);
    }
    obs.time("think", 60_000, 10);
    assert_eq!(obs.typical("think"), Some(800), "median, not mean");
    assert!(recommend(&obs, &laptop()).is_empty());
}

#[test]
fn recommendations_are_filtered_against_what_this_machine_can_do() {
    let mut obs = Observations::default();
    obs.asked_for_something_missing("generate a video of the product");
    let r = recommend(&obs, &laptop());
    let video = r.iter().find(|x| x.want.contains("video")).unwrap();
    assert!(!video.possible_here, "8GB of shared VRAM cannot generate video");
    assert_eq!(video.cost, Cost::Large);

    let big = Machine { vram_gb: 24.0, has_nvidia: true, ..laptop() };
    let r2 = recommend(&obs, &big);
    assert!(r2.iter().find(|x| x.want.contains("video")).unwrap().possible_here);
}

#[test]
fn it_suggests_the_npu_only_because_this_machine_has_one() {
    let mut obs = Observations::default();
    for i in 0..5 {
        obs.time("transcribe", 6000, i);
    }
    assert!(recommend(&obs, &laptop()).iter().any(|r| r.want.contains("NPU")));
    let no_npu = Machine { has_npu: false, ..laptop() };
    assert!(!recommend(&obs, &no_npu).iter().any(|r| r.want.contains("NPU")));
}

#[test]
fn gpu_offload_is_suggested_for_arc_but_framed_as_vulkan_not_cuda() {
    let mut obs = Observations::default();
    for i in 0..5 {
        obs.time("think", 9000, i);
    }
    let r = recommend(&obs, &laptop());
    let gpu = r.iter().find(|x| x.id == "llm-gpu").unwrap();
    assert!(gpu.possible_here, "Arc can be offloaded to, just not via CUDA");
    assert!(gpu.want.contains("Vulkan"));
}

#[test]
fn a_hosted_model_is_offered_but_its_real_cost_is_stated() {
    let mut obs = Observations::default();
    for i in 0..5 {
        obs.time("think", 9000, i);
    }
    let r = recommend(&obs, &laptop());
    let hosted = r.iter().find(|x| x.id == "llm-hosted").unwrap();
    assert!(hosted.benefit.contains("breaks offline"), "the trade must be named");
    assert_eq!(hosted.cost, Cost::Small);
}

#[test]
fn repeated_failures_of_one_capability_are_noticed() {
    let mut obs = Observations::default();
    for _ in 0..4 {
        obs.failed("posting to x");
    }
    obs.failed("something else");
    let r = recommend(&obs, &laptop());
    assert_eq!(r.len(), 1, "one failure is not a pattern");
    assert!(r[0].because.contains("4 times"));
}

#[test]
fn atlas_raises_one_thing_at_a_time_cheapest_first() {
    let mut obs = Observations::default();
    for i in 0..5 {
        obs.time("think", 9000, i);
    }
    let said = ask(&recommend(&obs, &laptop()));
    assert!(said.contains("typically takes"), "the evidence is in the sentence: {said}");
    assert!(said.len() < 220, "still speakable: {said}");
}

#[test]
fn when_only_impossible_things_remain_it_says_so_rather_than_nagging() {
    let mut obs = Observations::default();
    obs.asked_for_something_missing("generate a video");
    let said = ask(&recommend(&obs, &laptop()));
    assert!(said.contains("different hardware"), "got: {said}");
}

#[test]
fn measurements_are_bounded() {
    let mut obs = Observations::default();
    for i in 0..900 {
        obs.time("think", 100, i);
    }
    assert!(obs.timings.len() <= 500);
}

// ============ drafting a candidate fix from the diagnosis ============

fn a_thought() -> atlas::pipeline::Thought {
    atlas::pipeline::Thought {
        symptom: "the settings page is slow".into(),
        cause: "it re-reads every file on each keystroke".into(),
        where_: "src/settings.rs".into(),
        proof: "settings_page_is_fast".into(),
        proof_fails_now: true,
        not_doing: vec![],
    }
}

#[test]
fn a_fix_is_drafted_from_the_diagnosis_and_the_current_file() {
    use atlas::brain::{Llm, MockLlm};
    use atlas::selfwork::{draft_fix, Edit};
    let llm = MockLlm("```rust\nfn render() { /* cached */ }\n```".into());
    let current =
        vec![Edit { path: "src/settings.rs".into(), content: "fn render() {}".into(), reason: String::new() }];
    let out = draft_fix(&a_thought(), "cache the reads", &current, &llm as &dyn Llm).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].path, "src/settings.rs");
    assert!(out[0].content.contains("cached"), "the model's fix is carried: {:?}", out[0].content);
    // The reason records what it was fixing.
    assert!(out[0].reason.contains("re-reads"));
}

#[test]
fn a_model_that_returns_the_file_unchanged_is_not_a_fix() {
    use atlas::brain::{Llm, MockLlm};
    use atlas::selfwork::{draft_fix, Edit};
    let llm = MockLlm("```\nfn render() {}\n```".into());
    let current =
        vec![Edit { path: "src/settings.rs".into(), content: "fn render() {}".into(), reason: String::new() }];
    let err = draft_fix(&a_thought(), "", &current, &llm as &dyn Llm).unwrap_err();
    assert!(err.contains("unchanged"), "an unchanged file is refused: {err}");
}

#[test]
fn lines_touched_tells_a_small_fix_from_a_rewrite() {
    use atlas::selfwork::{lines_touched, Edit};
    let before = vec![Edit {
        path: "a.rs".into(),
        content: "one\ntwo\nthree\n".into(),
        reason: String::new(),
    }];
    // One line changed.
    let small = vec![Edit { path: "a.rs".into(), content: "one\nTWO\nthree\n".into(), reason: String::new() }];
    assert_eq!(lines_touched(&before, &small), 2, "one line out, one line in");
    // Whole thing rewritten.
    let big = vec![Edit { path: "a.rs".into(), content: "x\ny\nz\nw\n".into(), reason: String::new() }];
    assert!(lines_touched(&before, &big) >= 6);
}
