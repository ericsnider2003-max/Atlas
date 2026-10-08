use atlas::index::{AssetClass, Index, IndexConfig};
use atlas::intent::Intent;
use atlas::memory::Memory;
use atlas::config::Config;
use atlas::policy::{classify, classify_with_policy, Decision, PolicyConfig};
use atlas::scheduler::{JobState, Scheduler};
use atlas::session::{is_yes, Pending, Session};
use atlas::store::Store;
use std::fs;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-test-{tag}"));
    let _ = fs::remove_dir_all(&p);
    fs::create_dir_all(&p).unwrap();
    p
}

fn write(dir: &PathBuf, rel: &str, body: &str) {
    let p = dir.join(rel);
    fs::create_dir_all(p.parent().unwrap()).unwrap();
    fs::write(p, body).unwrap();
}

fn idx_cfg(root: &PathBuf) -> IndexConfig {
    serde_yaml::from_str(&format!(
        "roots: [\"{}\"]\nexclude_dirs: [node_modules, AppData]\nexclude_exts: [tmp, log]\nmax_depth: 6\n",
        root.display().to_string().replace(char::from(92), "/")
    ))
    .unwrap()
}

// ================= storage =================

#[test]
fn store_round_trips_and_survives_corruption() {
    let d = tmp("store");
    let s = Store::new(&d);
    let mut m = Memory::default();
    m.prefer("note_format", "markdown");
    m.save(&s).unwrap();
    assert_eq!(Memory::load(&s).preference("note_format"), Some("markdown"));

    // A half-written file must not stop Atlas from starting.
    fs::write(d.join("memory.json"), "{ this is not json").unwrap();
    assert!(Memory::load(&s).preferences.is_empty());
}

#[test]
fn saves_are_atomic_leaving_no_temp_file_behind() {
    let d = tmp("atomic");
    let s = Store::new(&d);
    Memory::default().save(&s).unwrap();
    let names: Vec<String> = fs::read_dir(&d).unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().to_string()).collect();
    assert_eq!(names, vec!["memory.json"]);
}

// ================= indexing =================

#[test]
fn scan_classifies_assets_by_extension() {
    let d = tmp("class");
    write(&d, "report.pdf", "x");
    write(&d, "shot.png", "x");
    write(&d, "clip.mp4", "x");
    write(&d, "main.rs", "x");
    let i = Index::scan(&idx_cfg(&d));
    assert_eq!(i.entries.len(), 4);
    for class in [AssetClass::Document, AssetClass::Image, AssetClass::Video, AssetClass::Code] {
        assert_eq!(i.entries.values().filter(|e| e.class == class).count(), 1, "{class:?}");
    }
}

#[test]
fn excluded_dirs_and_extensions_are_skipped() {
    let d = tmp("exclude");
    write(&d, "keep.md", "x");
    write(&d, "node_modules/junk.js", "x");
    write(&d, "AppData/secret.txt", "x");
    write(&d, "noisy.log", "x");
    let i = Index::scan(&idx_cfg(&d));
    let names: Vec<&str> = i.entries.values().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["keep.md"], "got {names:?}");
}

#[test]
fn dotfiles_are_ignored() {
    let d = tmp("dot");
    write(&d, "visible.md", "x");
    write(&d, ".hidden", "x");
    assert_eq!(Index::scan(&idx_cfg(&d)).entries.len(), 1);
}

#[test]
fn rescan_reports_added_modified_and_removed() {
    let d = tmp("rescan");
    write(&d, "a.md", "one");
    write(&d, "b.md", "one");
    let cfg = idx_cfg(&d);
    let mut i = Index::scan(&cfg);
    assert!(i.rescan(&cfg).is_empty(), "no changes means no churn");

    write(&d, "c.md", "new");
    fs::remove_file(d.join("b.md")).unwrap();
    write(&d, "a.md", "a much longer body so the size differs");

    let c = i.rescan(&cfg);
    assert_eq!(c.added.len(), 1);
    assert_eq!(c.removed.len(), 1);
    assert_eq!(c.modified.len(), 1);
    assert_eq!(c.count(), 3);
}

#[test]
fn search_ranks_exact_over_prefix_over_substring() {
    let d = tmp("search");
    write(&d, "notes.md", "x");
    write(&d, "notes-archive.md", "x");
    write(&d, "my-notes-backup.md", "x");
    let i = Index::scan(&idx_cfg(&d));
    let hits: Vec<&str> = i.search("notes").iter().map(|e| e.name.as_str()).collect();
    assert_eq!(hits, vec!["notes.md", "notes-archive.md", "my-notes-backup.md"]);
}

#[test]
fn search_is_not_fuzzy_a_wrong_confident_hit_is_worse_than_none() {
    let d = tmp("nofuzz");
    write(&d, "invoice.pdf", "x");
    let i = Index::scan(&idx_cfg(&d));
    assert!(i.search("invoce").is_empty());
    assert!(i.search("").is_empty());
}

#[test]
fn recent_returns_newest_first() {
    let d = tmp("recent");
    write(&d, "old.md", "x");
    std::thread::sleep(std::time::Duration::from_millis(1100));
    write(&d, "new.md", "x");
    let i = Index::scan(&idx_cfg(&d));
    assert_eq!(i.recent(1)[0].name, "new.md");
}

#[test]
fn index_persists_across_restarts() {
    let d = tmp("idxsave");
    write(&d, "a.md", "x");
    let store = Store::new(d.join(".atlas"));
    Index::scan(&idx_cfg(&d)).save(&store).unwrap();
    assert_eq!(Index::load(&store).entries.len(), 1);
}

// ================= memory =================

#[test]
fn repeated_sequences_become_habits_not_duplicates() {
    let mut m = Memory::default();
    for _ in 0..3 {
        m.record_workflow("Start Work", vec!["workspace_on".into(), "open_app".into()]);
    }
    m.record_workflow("check mail", vec!["open_app".into()]);
    assert_eq!(m.workflows.len(), 2);
    let h = m.habits(3);
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].trigger, "start work");
    assert_eq!(h[0].times_used, 3);
}

#[test]
fn approval_rate_distinguishes_no_history_from_bad_history() {
    let mut m = Memory::default();
    assert_eq!(m.approval_rate("close_app"), None, "unknown is not the same as risky");
    m.record_approval("close_app", true, None);
    m.record_approval("close_app", false, Some("wrong window".into()));
    assert_eq!(m.approval_rate("close_app"), Some(0.5));
}

#[test]
fn approval_log_is_bounded() {
    let mut m = Memory::default();
    for _ in 0..600 {
        m.record_approval("close_app", true, None);
    }
    assert_eq!(m.approvals.len(), 500);
}

#[test]
fn projects_track_folders_without_duplicating() {
    let mut m = Memory::default();
    m.touch_project("atlas", Some("/work/atlas"));
    m.touch_project("atlas", Some("/work/atlas"));
    m.touch_project("atlas", Some("/work/atlas-docs"));
    assert_eq!(m.projects["atlas"].folders.len(), 2);
}

// ================= four-state confidence model =================

#[test]
fn the_four_states_are_assigned_sensibly() {
    assert_eq!(classify(&Intent::OpenApp("chrome".into())), Decision::AutoProceed);
    assert_eq!(classify(&Intent::WorkspaceOn), Decision::ProceedAndReport);
    assert_eq!(classify(&Intent::WorkspaceOff), Decision::RequireApproval);
    assert_eq!(classify(&Intent::Ask("which one?".into())), Decision::AskClarification);
}

#[test]
fn a_learnable_action_relaxes_after_consistent_approval() {
    let mut m = Memory::default();
    for _ in 0..10 {
        m.record_approval("focus_app", true, None);
    }
    let cfg = PolicyConfig {
        always_ask: vec!["workspace_off".into()],
        learnable: vec!["focus_app".into()],
        min_samples: 5,
    };
    // Pretend focus_app were consequential, to exercise the relax path.
    assert_eq!(
        classify_with_policy(&Intent::FocusApp("chrome".into()), &m, &cfg),
        Decision::AutoProceed,
        "focus is low-risk to begin with"
    );
}

#[test]
fn an_always_ask_action_is_never_relaxed_however_often_you_approve_it() {
    let mut m = Memory::default();
    for _ in 0..100 {
        m.record_approval("workspace_off", true, None);
        m.record_approval("close_app", true, None);
    }
    let cfg = PolicyConfig::default();
    assert_eq!(classify_with_policy(&Intent::WorkspaceOff, &m, &cfg), Decision::RequireApproval);
    assert_eq!(
        classify_with_policy(&Intent::CloseApp("chrome".into()), &m, &cfg),
        Decision::RequireApproval
    );
}

#[test]
fn an_action_on_neither_list_is_never_relaxed_by_default() {
    // Silence is not consent: unlisted means unchanged.
    let mut m = Memory::default();
    for _ in 0..100 {
        m.record_approval("some_new_action", true, None);
    }
    assert_eq!(
        classify_with_policy(&Intent::WorkspaceOff, &m,
            &PolicyConfig { always_ask: vec![], learnable: vec![], min_samples: 5 }),
        Decision::RequireApproval
    );
}

#[test]
fn the_shipped_policy_puts_destructive_actions_on_always_ask() {
    let c = Config::load(Path::new("config")).unwrap();
    assert!(c.policy.always_ask.iter().any(|k| k == "workspace_off"));
    assert!(c.policy.always_ask.iter().any(|k| k == "close_app"));
}

fn history(min_samples: usize) -> PolicyConfig {
    PolicyConfig { min_samples, ..Default::default() }
}

#[test]
fn mixed_history_does_not_relax_anything() {
    let mut m = Memory::default();
    for i in 0..10 {
        m.record_approval("close_app", i % 3 != 0, None);
    }
    assert_eq!(
        classify_with_policy(&Intent::CloseApp("x".into()), &m, &history(5)),
        Decision::RequireApproval
    );
}

#[test]
fn thin_history_does_not_relax_anything() {
    let mut m = Memory::default();
    m.record_approval("close_app", true, None);
    assert_eq!(
        classify_with_policy(&Intent::CloseApp("x".into()), &m, &history(5)),
        Decision::RequireApproval
    );
}

#[test]
fn atlas_never_learns_to_auto_run_things_it_did_not_understand() {
    let mut m = Memory::default();
    for _ in 0..50 {
        m.record_approval("unknown", true, None);
    }
    assert_eq!(
        classify_with_policy(&Intent::Unknown("???".into()), &m, &history(5)),
        Decision::RequireApproval
    );
}

// ================= session =================

#[test]
fn follow_ups_can_resolve_against_the_last_app() {
    let mut s = Session::default();
    s.record("open chrome", &Intent::OpenApp("chrome".into()), "Opening chrome.");
    assert_eq!(s.last_app.as_deref(), Some("chrome"));
    s.record("what's on screen", &Intent::ViewDisplay, "Looking.");
    assert_eq!(s.last_app.as_deref(), Some("chrome"), "non-app turns don't clear it");
}

#[test]
fn ambiguous_noises_are_not_consent() {
    for s in ["uh", "hmm", "maybe", "i guess", "", "no", "wait"] {
        assert!(!is_yes(s), "{s:?} must not count as approval");
    }
    for s in ["yes", "Yeah!", "  ok  ", "do it", "go ahead"] {
        assert!(is_yes(s), "{s:?} should count");
    }
}

#[test]
fn pending_clarification_is_tracked() {
    let mut s = Session::default();
    s.ask("Which browser?");
    assert_eq!(s.pending, Pending::Clarification("Which browser?".into()));
}

#[test]
fn session_history_is_bounded_and_ordered() {
    let mut s = Session::default();
    for i in 0..60 {
        s.record(&format!("cmd {i}"), &Intent::ViewDisplay, "ok");
    }
    assert_eq!(s.turns.len(), 40);
    assert_eq!(s.turns.last().unwrap().said, "cmd 59");
    assert!(s.transcript(2).contains("cmd 59"));
}

#[test]
fn session_steps_feed_workflow_memory() {
    let mut s = Session::default();
    s.record("boot workspace", &Intent::WorkspaceOn, "ok");
    s.record("open chrome", &Intent::OpenApp("chrome".into()), "ok");
    let mut m = Memory::default();
    m.record_workflow("morning", s.steps());
    assert_eq!(m.workflows[0].steps, vec!["workspace_on", "open_app"]);
}

// ================= scheduler =================

#[test]
fn due_returns_only_jobs_whose_time_has_come() {
    let mut s = Scheduler::default();
    let a = s.at("boot workspace", 100);
    let _b = s.at("close workspace", 500);
    assert_eq!(s.due(50), Vec::<u64>::new());
    assert_eq!(s.due(100), vec![a]);
    assert_eq!(s.due(600).len(), 2);
}

#[test]
fn recurring_jobs_reschedule_from_completion_not_from_the_old_due_time() {
    // A laptop asleep for a day must not wake owing 24 runs of an hourly job.
    let mut s = Scheduler::default();
    let id = s.every("index refresh", 3600, 1000);
    s.complete(id, 90_000, "ok", true);
    let j = s.jobs.iter().find(|j| j.id == id).unwrap();
    assert_eq!(j.due, 93_600);
    assert_eq!(j.state, JobState::Pending);
    assert_eq!(s.due(93_599).len(), 0);
}

#[test]
fn one_shot_jobs_are_done_and_do_not_rerun() {
    let mut s = Scheduler::default();
    let id = s.at("boot workspace", 10);
    s.complete(id, 20, "ok", true);
    assert_eq!(s.jobs[0].state, JobState::Done);
    assert!(s.due(9999).is_empty());
}

#[test]
fn a_failed_recurring_job_stops_instead_of_looping_forever() {
    let mut s = Scheduler::default();
    let id = s.every("broken", 60, 0);
    // 29 Sep 2026: one failure no longer stops a repeating job for good (a
    // reminder that hit one bad moment never came again, unsaid). It is tried
    // at its next time -- not at once, so still no loop -- and stops after
    // `FAILS_BEFORE_STOPPING` failures in a row.
    s.complete(id, 100, "exploded", false);
    assert_eq!(s.jobs[0].state, JobState::Pending);
    assert_eq!(s.jobs[0].due, 160, "tried again at once, which is the loop this guards against");
    for t in [160, 220] {
        s.complete(id, t, "exploded", false);
    }
    assert_eq!(s.jobs[0].fails_in_a_row, atlas::scheduler::FAILS_BEFORE_STOPPING);
    assert_eq!(s.jobs[0].state, JobState::Failed);
    assert!(s.due(9999).is_empty());
}

#[test]
fn parked_jobs_never_fire_just_because_time_passed() {
    let mut s = Scheduler::default();
    let id = s.at("close workspace", 10);
    s.park_for_approval(id);
    assert!(s.due(999_999).is_empty(), "waiting for approval is not a timer");
    s.approve(id);
    assert_eq!(s.due(999_999), vec![id]);
}

#[test]
fn cancel_and_prune_keep_the_file_small() {
    let mut s = Scheduler::default();
    let a = s.at("one", 1);
    let b = s.at("two", 2);
    s.complete(a, 5, "ok", true);
    assert!(s.cancel(b));
    assert!(!s.cancel(999));
    s.prune();
    assert!(s.jobs.is_empty());
}

#[test]
fn scheduler_persists() {
    let d = tmp("sched");
    let store = Store::new(&d);
    let mut s = Scheduler::default();
    s.every("index refresh", 900, 0);
    s.save(&store).unwrap();
    let back = Scheduler::load(&store);
    assert_eq!(back.active().len(), 1);
    assert_eq!(back.jobs[0].every, Some(900));
}

#[test]
fn a_windows_path_in_a_quoted_yaml_string_gives_a_useful_error() {
    // "C:\Users\..." fails YAML parsing because \U starts a unicode escape.
    // The raw message — "did not find expected hexadecimal number" — tells a
    // user nothing about what to change.
    let d = std::env::temp_dir().join("atlas-yaml-error");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("indexing.yaml"), "roots: [\"C:\\Users\\erics\\Desktop\"]\n").unwrap();
    fs::write(d.join("apps.yaml"), "apps: {}\nstartup_order: []\nshutdown_order: []\n").unwrap();
    fs::write(d.join("layouts.yaml"), "roles: []\nlayouts: {}\n").unwrap();
    fs::write(d.join("commands.yaml"), "commands: []\n").unwrap();

    let err = Config::load(&d).unwrap_err().to_string();
    assert!(err.contains("forward slashes"), "should say what to do: {err}");
    assert!(err.contains("C:/Users"), "should show the right shape: {err}");
}

#[test]
fn doctor_finds_an_app_even_when_the_executable_is_named_differently() {
    // "not found" when the app is plainly on the taskbar is a useless answer.
    // Installers ship the same app as Claude.exe, AnthropicClaude.exe, or a
    // Store alias, so an exact-filename search is not enough.
    let d = std::env::temp_dir().join("atlas-find-exe");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("AnthropicClaude")).unwrap();
    fs::write(d.join("AnthropicClaude/AnthropicClaude.exe"), "x").unwrap();
    fs::write(d.join("AnthropicClaude/claude-uninstall.exe"), "x").unwrap();

    let hits = atlas::doctor::find_exes(&[d.clone()], "claude.exe", "claude");
    assert_eq!(hits.len(), 1, "got {hits:?}");
    assert!(hits[0].contains("AnthropicClaude.exe"));
}

#[test]
fn an_exact_match_is_preferred_over_a_near_one() {
    let d = std::env::temp_dir().join("atlas-find-exact");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("a")).unwrap();
    fs::create_dir_all(d.join("b")).unwrap();
    fs::write(d.join("a/AnthropicClaude.exe"), "x").unwrap();
    fs::write(d.join("b/claude.exe"), "x").unwrap();

    let hits = atlas::doctor::find_exes(&[d.clone()], "claude.exe", "claude");
    assert!(hits[0].ends_with("claude.exe"), "exact first: {hits:?}");
}

#[test]
fn installers_and_uninstallers_are_never_offered_as_the_app() {
    let d = std::env::temp_dir().join("atlas-find-noise");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    for n in ["claude-setup.exe", "claude_updater.exe", "Uninstall claude.exe"] {
        fs::write(d.join(n), "x").unwrap();
    }
    assert!(atlas::doctor::find_exes(&[d.clone()], "claude.exe", "claude").is_empty());
}

#[test]
fn environment_variables_expand_regardless_of_case() {
    // Windows reports "ProgramFiles"; configs are written %PROGRAMFILES%.
    // A case-sensitive match silently left the placeholder in place, and the
    // app was then reported as missing.
    // The variables are passed in: the process's environment is shared with
    // every other test in this binary. The match is the real one's.
    let env = |n: &str| atlas::doctor::lookup_in(n, [("AtlasTestVar".to_string(), "/somewhere".to_string())]);
    assert_eq!(atlas::doctor::expand_env_with("%ATLASTESTVAR%/app.exe", &env), "/somewhere/app.exe");
    assert_eq!(atlas::doctor::expand_env_with("%atlastestvar%/app.exe", &env), "/somewhere/app.exe");
    assert_eq!(atlas::doctor::expand_env_with("%AtlasTestVar%/app.exe", &env), "/somewhere/app.exe");
}

#[test]
fn an_unknown_variable_is_left_visible_rather_than_blanked() {
    let out = atlas::doctor::expand_env("%NO_SUCH_VAR_HERE%/app.exe");
    assert!(out.contains("NO_SUCH_VAR_HERE"), "a typo should be obvious, not silent: {out}");
    // Behaviour, not just wording: an unknown variable passes through byte for
    // byte -- percent signs and all -- rather than being blanked out.
    assert_eq!(out, "%NO_SUCH_VAR_HERE%/app.exe", "an unknown variable was altered, not left visible");
}

#[test]
fn a_path_with_a_stray_percent_does_not_break() {
    assert_eq!(atlas::doctor::expand_env("100% done"), "100% done");
    assert_eq!(atlas::doctor::expand_env("%"), "%");
}
