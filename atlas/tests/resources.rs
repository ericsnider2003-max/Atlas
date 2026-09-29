use atlas::backends::{Backend, Capability, Request, Router};
use atlas::lifecycle::{LifecycleConfig, Order, Supervisor};
use atlas::memory::Memory;
use atlas::retention::{classify, plan, survey, usage, Class, Plan, RetentionConfig};
use std::fs;
use std::path::PathBuf;

fn req(cap: Capability, app: &str) -> Request<'_> {
    Request { capability: cap, app, may_steal_focus: false, must_be_your_window: false }
}

// ============ picking the right mechanism ============

#[test]
fn browser_work_prefers_devtools_over_synthetic_clicks() {
    let r = Router::default();
    let c = r.choose(&req(Capability::FillForm, "chrome")).unwrap();
    assert_eq!(c.backend, Backend::Cdp);
}

#[test]
fn reading_a_native_app_prefers_accessibility_over_a_whole_browser() {
    let r = Router::default();
    // UIA is 25MB; the hidden desktop is 300MB. Both can read text.
    let c = r.choose(&req(Capability::ReadText, "notepad")).unwrap();
    assert_eq!(c.backend, Backend::Uia, "cheapest thing that works");
}

#[test]
fn devtools_is_never_chosen_for_a_non_browser_app() {
    let r = Router::default();
    let ladder = r.ladder(&req(Capability::ReadText, "discord"));
    assert!(!ladder.contains(&Backend::Cdp));
}

#[test]
fn nothing_that_steals_focus_is_picked_while_you_are_working() {
    let r = Router::default();
    // Scroll: only CDP (chrome-only), HiddenDesktop, and SendInput can.
    let c = r.choose(&req(Capability::Scroll, "notepad")).unwrap();
    assert_ne!(c.backend, Backend::SendInput, "must not grab the screen");
    assert_eq!(c.backend, Backend::HiddenDesktop);
}

#[test]
fn synthetic_input_becomes_available_once_you_step_away() {
    let r = Router::default();
    let mut q = req(Capability::Scroll, "notepad");
    q.may_steal_focus = true;
    assert!(r.ladder(&q).contains(&Backend::SendInput));
}

#[test]
fn asking_about_your_actual_window_rules_out_separate_copies() {
    let r = Router::default();
    let mut q = req(Capability::ReadText, "chrome");
    q.must_be_your_window = true;
    let c = r.choose(&q).unwrap();
    assert_eq!(c.backend, Backend::Uia, "only UIA reads YOUR window");
}

#[test]
fn when_nothing_can_do_the_job_it_says_so_instead_of_guessing() {
    let r = Router::default();
    let mut q = req(Capability::WaitForElement, "notepad");
    q.must_be_your_window = true;
    assert!(r.choose(&q).is_none());
}

#[test]
fn a_backend_that_keeps_failing_on_one_app_loses_that_app_only() {
    let mut r = Router::default();
    for _ in 0..12 {
        r.record(Backend::Uia, "discord", false);
    }
    // Electron apps expose a useless accessibility tree; Atlas learns that.
    let c = r.choose(&req(Capability::ReadText, "discord")).unwrap();
    assert_ne!(c.backend, Backend::Uia);
    // But UIA is still first choice for Notepad.
    assert_eq!(r.choose(&req(Capability::ReadText, "notepad")).unwrap().backend, Backend::Uia);
}

#[test]
fn one_failure_is_not_enough_to_write_a_backend_off() {
    let mut r = Router::default();
    r.record(Backend::Uia, "notepad", false);
    assert_eq!(r.choose(&req(Capability::ReadText, "notepad")).unwrap().backend, Backend::Uia);
}

#[test]
fn a_distrusted_backend_is_still_used_if_it_is_the_only_option() {
    let mut r = Router::default();
    for _ in 0..30 {
        r.record(Backend::Uia, "weirdapp", false);
    }
    let mut q = req(Capability::ReadWindowTitle, "weirdapp");
    q.must_be_your_window = true;
    // Only UIA reads window titles on your own window. Distrusted or not,
    // refusing outright would be worse than trying.
    assert_eq!(r.choose(&q).unwrap().backend, Backend::Uia);
}

#[test]
fn the_choice_explains_itself() {
    let mut r = Router::default();
    for _ in 0..4 {
        r.record(Backend::Cdp, "chrome", true);
    }
    let why = r.choose(&req(Capability::Navigate, "chrome")).unwrap().why;
    assert!(why.contains("4/4"), "should show its record: {why}");
}

#[test]
fn an_app_update_can_reset_what_atlas_learned_about_it() {
    let mut r = Router::default();
    for _ in 0..12 {
        r.record(Backend::Uia, "discord", false);
    }
    r.learned.forget_app("discord");
    assert_eq!(r.choose(&req(Capability::ReadText, "discord")).unwrap().backend, Backend::Uia);
}

#[test]
fn a_failure_falls_down_the_ladder_rather_than_giving_up() {
    let r = Router::default();
    let ladder = r.ladder(&req(Capability::ReadText, "chrome"));
    assert!(ladder.len() >= 2, "there is always a next thing to try: {ladder:?}");
}

// ============ not holding memory forever ============

#[test]
fn a_helper_is_started_on_demand_and_reused_while_warm() {
    let mut s = Supervisor::new(LifecycleConfig::default());
    assert_eq!(s.acquire("cdp", 180, 100).0, Order::Start("cdp".into()));
    s.release("cdp", 110);
    assert_eq!(s.acquire("cdp", 180, 120).0, Order::Reuse("cdp".into()), "no cold start twice");
}

#[test]
fn an_idle_helper_is_shut_down_and_its_memory_returned() {
    let mut s = Supervisor::new(LifecycleConfig::default());
    s.acquire("uia", 25, 100);
    s.release("uia", 100);
    assert_eq!(s.memory_mb(), 25);
    assert!(s.reap(150).is_empty(), "still within its idle window");
    assert_eq!(s.reap(1000), vec![Order::Stop("uia".into())]);
    assert_eq!(s.memory_mb(), 0, "memory is only reclaimed by exiting");
}

#[test]
fn the_browser_stays_warm_longer_because_research_comes_in_bursts() {
    let mut s = Supervisor::new(LifecycleConfig::default());
    s.acquire("cdp", 180, 100);
    s.release("cdp", 100);
    assert!(s.reap(250).is_empty(), "would have reaped a default helper by now");
    assert!(s.is_running("cdp"));
    assert_eq!(s.reap(500), vec![Order::Stop("cdp".into())]);
}

#[test]
fn a_helper_in_the_middle_of_a_job_is_never_reaped() {
    let mut s = Supervisor::new(LifecycleConfig::default());
    s.acquire("cdp", 180, 100);
    assert!(s.reap(99_999).is_empty(), "still working");
}

#[test]
fn exceeding_the_budget_evicts_the_least_recently_used() {
    let mut s = Supervisor::new(LifecycleConfig { budget_mb: 400, ..Default::default() });
    s.acquire("uia", 25, 100);
    s.release("uia", 100);
    s.acquire("cdp", 180, 200);
    s.release("cdp", 200);

    let (order, evicted) = s.acquire("hidden_desktop", 300, 300);
    assert_eq!(order, Order::Start("hidden_desktop".into()));
    assert!(evicted.contains(&Order::Stop("uia".into())), "oldest goes first: {evicted:?}");
    assert!(s.memory_mb() <= 400);
}

#[test]
fn atlas_refuses_rather_than_exceeding_its_memory_budget() {
    let mut s = Supervisor::new(LifecycleConfig { budget_mb: 200, ..Default::default() });
    s.acquire("cdp", 180, 100); // in use, cannot be evicted
    let (order, _) = s.acquire("hidden_desktop", 300, 110);
    assert!(matches!(order, Order::Refuse(_)), "got {order:?}");
    assert!(s.memory_mb() <= 200, "never goes over");
}

#[test]
fn everything_can_be_shut_down_at_once() {
    let mut s = Supervisor::new(LifecycleConfig::default());
    s.acquire("cdp", 180, 100);
    s.acquire("uia", 25, 100);
    assert_eq!(s.stop_all().len(), 2);
    assert_eq!(s.memory_mb(), 0);
}

// ============ storage that stays bounded ============

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-ret-{tag}"));
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(d.join("tmp")).unwrap();
    fs::create_dir_all(d.join("notes")).unwrap();
    fs::create_dir_all(d.join("logs")).unwrap();
    d
}

#[test]
fn files_are_classified_by_what_they_cost_and_what_they_are_worth() {
    assert_eq!(classify(&PathBuf::from("data/tmp/turn.wav")), Class::Scratch);
    assert_eq!(classify(&PathBuf::from("data/tmp/screen_1.png")), Class::Captures);
    assert_eq!(classify(&PathBuf::from("data/notes/1234-topic.md")), Class::Notes);
    assert_eq!(classify(&PathBuf::from("data/logs/atlas.log")), Class::Logs);
    assert_eq!(classify(&PathBuf::from("data/state/memory.json")), Class::State);
}

#[test]
fn stale_scratch_and_captures_are_deleted_notes_are_not() {
    let d = tmpdir("age");
    fs::write(d.join("tmp/turn.wav"), "x").unwrap();
    fs::write(d.join("tmp/screen_1.png"), "x").unwrap();
    fs::write(d.join("notes/research.md"), "valuable").unwrap();

    let mut items = survey(&d);
    // Pretend everything is a week old.
    for i in &mut items {
        i.modified = 0;
    }
    let plans = plan(&items, &RetentionConfig::default(), 7 * 86400);
    let deleted: Vec<String> = plans.iter().map(|p| match p {
        Plan::Delete { path, .. } => path.file_name().unwrap().to_string_lossy().to_string(),
        Plan::Keep => String::new(),
    }).collect();
    assert!(deleted.contains(&"turn.wav".to_string()));
    assert!(deleted.contains(&"screen_1.png".to_string()));
    assert!(!deleted.contains(&"research.md".to_string()), "notes survive a week");
}

#[test]
fn fresh_files_are_left_alone() {
    let d = tmpdir("fresh");
    fs::write(d.join("tmp/screen_1.png"), "x").unwrap();
    let mut items = survey(&d);
    for i in &mut items {
        i.modified = 1000;
    }
    assert!(plan(&items, &RetentionConfig::default(), 1100).is_empty());
}

#[test]
fn going_over_budget_evicts_scratch_before_captures_and_never_notes() {
    let d = tmpdir("budget");
    fs::write(d.join("tmp/a.wav"), vec![0u8; 400_000]).unwrap();
    fs::write(d.join("tmp/b.png"), vec![0u8; 400_000]).unwrap();
    fs::write(d.join("notes/keep.md"), vec![0u8; 400_000]).unwrap();

    let mut items = survey(&d);
    for i in &mut items {
        i.modified = 1000;
    }
    let cfg = RetentionConfig { total_budget_mb: 1, ..Default::default() };
    let plans = plan(&items, &cfg, 1100);
    let deleted: Vec<String> = plans.iter().filter_map(|p| match p {
        Plan::Delete { path, .. } => Some(path.file_name().unwrap().to_string_lossy().to_string()),
        _ => None,
    }).collect();
    assert!(deleted.contains(&"a.wav".to_string()), "scratch goes first: {deleted:?}");
    assert!(!deleted.contains(&"keep.md".to_string()), "notes are never evicted for space");
}

#[test]
fn usage_is_reported_per_category_so_you_can_see_what_is_growing() {
    let d = tmpdir("usage");
    fs::write(d.join("tmp/a.wav"), vec![0u8; 1000]).unwrap();
    fs::write(d.join("notes/n.md"), vec![0u8; 500]).unwrap();
    let u = usage(&survey(&d));
    assert_eq!(u.scratch, 1000);
    assert_eq!(u.notes, 500);
    assert_eq!(u.total(), 1500);
}

// ============ remembering without hoarding ============

#[test]
fn old_approvals_collapse_to_counts_without_changing_behaviour() {
    let mut m = Memory::default();
    for _ in 0..300 {
        m.record_approval("close_app", true, None);
    }
    for _ in 0..100 {
        m.record_approval("close_app", false, None);
    }
    let rate_before = m.approval_rate("close_app");
    let seen_before = m.times_seen("close_app");

    let dropped = m.compact_approvals(50);
    assert!(dropped > 0);
    assert_eq!(m.approvals.len(), 50, "only recent detail is kept");
    assert_eq!(m.approval_rate("close_app"), rate_before, "the statistics survive");
    assert_eq!(m.times_seen("close_app"), seen_before, "the count survives");
}

#[test]
fn compaction_does_nothing_when_there_is_little_to_compact() {
    let mut m = Memory::default();
    for _ in 0..10 {
        m.record_approval("focus_app", true, None);
    }
    assert_eq!(m.compact_approvals(200), 0);
    assert_eq!(m.approvals.len(), 10);
}

#[test]
fn compaction_keeps_different_action_kinds_separate() {
    let mut m = Memory::default();
    for _ in 0..100 {
        m.record_approval("close_app", true, None);
        m.record_approval("workspace_off", false, None);
    }
    m.compact_approvals(10);
    assert_eq!(m.approval_rate("close_app"), Some(1.0));
    assert_eq!(m.approval_rate("workspace_off"), Some(0.0));
}

#[test]
fn compacted_state_survives_a_save_and_load() {
    let d = std::env::temp_dir().join("atlas-compact-persist");
    let _ = fs::remove_dir_all(&d);
    let store = atlas::store::Store::new(&d);
    let mut m = Memory::default();
    for _ in 0..500 {
        m.record_approval("close_app", true, None);
    }
    m.compact_approvals(20);
    m.save(&store).unwrap();
    assert_eq!(Memory::load(&store).times_seen("close_app"), 500);
}

#[test]
fn the_shipped_config_bounds_both_memory_and_storage() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert!(t.lifecycle.budget_mb > 0, "helpers must have a ceiling");
    assert!(t.lifecycle.idle_timeout_secs > 0, "helpers must be reaped");
    assert!(
        t.lifecycle.keep_warm_secs.get("cdp").copied().unwrap_or(0) > t.lifecycle.idle_timeout_secs,
        "the browser is worth keeping warm longer than the default"
    );
    assert!(t.retention.total_budget_mb > 0);
    assert!(t.retention.scratch_minutes < t.retention.captures_hours * 60);
}

#[test]
fn what_ive_learned_reports_readable_and_unreadable_apps_after_enough_evidence() {
    // The reader the daemon's window-reads feed and the `atlas backends`
    // command prints. One try is not evidence; several failures on an app is.
    let mut r = Router::default();
    // notepad reads cleanly every time.
    for _ in 0..6 {
        r.record(Backend::Uia, "notepad", true);
    }
    // discord never reads.
    for _ in 0..6 {
        r.record(Backend::Uia, "discord", false);
    }
    // slack has only been tried twice — not enough to report yet.
    r.record(Backend::Uia, "slack", true);
    r.record(Backend::Uia, "slack", false);

    let lines = r.what_ive_learned();
    let joined = lines.join("\n");
    assert!(joined.contains("notepad") && joined.contains("reads cleanly"), "{joined}");
    assert!(joined.contains("discord") && joined.contains("can't read it"), "{joined}");
    assert!(
        !joined.contains("slack"),
        "an app with too few tries must not be reported as a verdict: {joined}"
    );
}

#[test]
fn a_fresh_router_has_learned_nothing_to_report() {
    assert!(Router::default().what_ive_learned().is_empty());
}
