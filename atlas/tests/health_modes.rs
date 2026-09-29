use atlas::health::{assess, summary, HealthConfig, Readings, Reporter, Severity};
use atlas::modes::{sentences_for, suggested, Interruptions, Mode, Modes};
use atlas::watch::{Alert, Health, Target, Watcher};

fn cfg() -> HealthConfig {
    HealthConfig::default()
}
fn healthy() -> Readings {
    Readings {
        disk_free_gb: 300.0,
        disk_total_gb: 1000.0,
        ram_used_gb: 6.0,
        ram_total_gb: 15.7,
        battery_percent: Some(90),
        on_battery: false,
        battery_health_percent: Some(95),
        days_since_backup: Some(1),
        uptime_days: 2,
        pending_updates: 0,
    }
}

// ================= watching your machine =================

#[test]
fn a_healthy_machine_produces_nothing_to_say() {
    assert!(assess(&healthy(), &cfg()).is_empty());
    assert!(summary(&healthy(), &[]).contains("All fine"));
}

#[test]
fn a_nearly_full_disk_is_urgent_and_a_tight_one_is_a_notice() {
    let tight = Readings { disk_free_gb: 15.0, ..healthy() };
    assert_eq!(assess(&tight, &cfg())[0].severity, Severity::Notice);

    let full = Readings { disk_free_gb: 3.0, ..healthy() };
    assert_eq!(assess(&full, &cfg())[0].severity, Severity::Urgent);
}

#[test]
fn memory_pressure_is_noticed() {
    // 12.8 of 15.7 is roughly where your machine sits today.
    let tight = Readings { ram_used_gb: 14.5, ..healthy() };
    let f = assess(&tight, &cfg());
    assert!(f.iter().any(|x| x.id == "ram"), "got {f:?}");
}

#[test]
fn battery_only_matters_when_you_are_actually_on_battery() {
    let low_plugged = Readings { battery_percent: Some(10), on_battery: false, ..healthy() };
    assert!(!assess(&low_plugged, &cfg()).iter().any(|f| f.id == "battery"));

    let low_unplugged = Readings { battery_percent: Some(10), on_battery: true, ..healthy() };
    assert!(assess(&low_unplugged, &cfg()).iter().any(|f| f.id == "battery"));
}

#[test]
fn a_battery_that_has_lost_capacity_is_flagged_separately() {
    let worn = Readings { battery_health_percent: Some(62), ..healthy() };
    assert!(assess(&worn, &cfg()).iter().any(|f| f.id == "battery_health"));
}

#[test]
fn never_having_backed_up_is_flagged_not_overlooked() {
    // The easy one to miss: no backup at all reads as "no data" rather than
    // as the worst case.
    let never = Readings { days_since_backup: None, ..healthy() };
    let f = assess(&never, &cfg());
    assert!(f.iter().any(|x| x.id == "backup"), "got {f:?}");
    assert!(f.iter().any(|x| x.say.contains("never been backed up")));
}

#[test]
fn urgent_findings_are_sorted_ahead_of_notices() {
    let bad = Readings {
        disk_free_gb: 2.0,
        days_since_backup: Some(60),
        uptime_days: 40,
        ..healthy()
    };
    let f = assess(&bad, &cfg());
    assert_eq!(f[0].severity, Severity::Urgent);
}

#[test]
fn a_notice_waits_for_a_quiet_moment_but_urgent_does_not() {
    let mut r = Reporter::default();
    let notice = assess(&Readings { disk_free_gb: 15.0, ..healthy() }, &cfg());
    assert!(r.next(&notice, false, &cfg(), 100).is_none(), "not while you're working");
    assert!(r.next(&notice, true, &cfg(), 200).is_some());

    let mut r2 = Reporter::default();
    let urgent = assess(&Readings { disk_free_gb: 2.0, ..healthy() }, &cfg());
    assert!(r2.next(&urgent, false, &cfg(), 100).is_some(), "urgent interrupts");
}

#[test]
fn the_same_problem_is_not_repeated_for_a_week() {
    // A monitor that mentions a full disk hourly gets ignored, and then the
    // genuinely urgent one is ignored too.
    let mut r = Reporter::default();
    let f = assess(&Readings { disk_free_gb: 2.0, ..healthy() }, &cfg());
    assert!(r.next(&f, true, &cfg(), 100).is_some());
    assert!(r.next(&f, true, &cfg(), 100_000).is_none());
    assert!(r.next(&f, true, &cfg(), 100 + 8 * 86_400).is_some());
}

#[test]
fn only_one_thing_is_said_at_a_time() {
    let mut r = Reporter::default();
    let many = assess(
        &Readings { disk_free_gb: 2.0, ram_used_gb: 15.3, uptime_days: 40, ..healthy() },
        &cfg(),
    );
    assert!(many.len() >= 3);
    let first = r.next(&many, true, &cfg(), 100);
    assert!(first.is_some());
    assert_eq!(first.unwrap().severity, Severity::Urgent, "worst first");
}

#[test]
fn fixing_a_problem_lets_it_be_reported_again_later() {
    let mut r = Reporter::default();
    let f = assess(&Readings { disk_free_gb: 2.0, ..healthy() }, &cfg());
    r.next(&f, true, &cfg(), 100);
    r.reconcile(&[]); // disk freed up
    assert!(r.next(&f, true, &cfg(), 200).is_some(), "a recurrence is worth saying again");
}

// ================= watching the other machine =================

fn watcher() -> Watcher {
    let mut w = Watcher::default();
    w.add(Target {
        name: "homelab".into(),
        address: "10.0.0.5:443".into(),
        enabled: true,
        ..Default::default()
    });
    w
}

#[test]
fn one_dropped_packet_is_not_an_outage() {
    let mut w = watcher();
    w.observe("homelab", true, 0);
    assert_eq!(w.observe("homelab", false, 120), Alert::None);
    assert_eq!(w.health("homelab"), Health::Up);
}

#[test]
fn three_failures_in_a_row_is() {
    let mut w = watcher();
    w.observe("homelab", true, 0);
    w.observe("homelab", false, 120);
    w.observe("homelab", false, 240);
    match w.observe("homelab", false, 360) {
        Alert::WentDown { say, .. } => assert!(say.contains("stopped responding")),
        o => panic!("{o:?}"),
    }
    assert_eq!(w.down_for("homelab", 400), Some(40));
}

#[test]
fn coming_back_is_reported_once() {
    let mut w = watcher();
    w.observe("homelab", true, 0);
    for t in 1..=3 {
        w.observe("homelab", false, t * 120);
    }
    match w.observe("homelab", true, 500) {
        Alert::CameBack { .. } => {}
        o => panic!("{o:?}"),
    }
    assert_eq!(w.observe("homelab", true, 620), Alert::None, "not every check");
}

#[test]
fn something_flapping_is_called_out_as_flapping() {
    // Worse than plainly down, and easy to miss if you only alert on
    // transitions.
    let mut w = Watcher::default();
    w.add(Target {
        name: "homelab".into(),
        address: "x:1".into(),
        fails_before_down: 1,
        enabled: true,
        ..Default::default()
    });
    let mut last = Alert::None;
    for i in 0..6 {
        last = w.observe("homelab", i % 2 == 0, i * 60);
    }
    assert!(matches!(last, Alert::Flapping { .. }), "got {last:?}");
}

#[test]
fn a_disabled_target_is_never_probed() {
    let mut w = Watcher::default();
    w.add(Target { name: "off".into(), address: "x:1".into(), enabled: false, ..Default::default() });
    assert!(!w.due("off", 999_999));
}

#[test]
fn the_watcher_never_holds_anything_but_a_host_and_a_port() {
    // If this laptop were compromised, the watch config must give an attacker
    // nothing they couldn't learn by port-scanning.
    let json = serde_json::to_string(&watcher()).unwrap();
    assert!(!json.contains("password") && !json.contains("token") && !json.contains("key\":"));
    assert!(json.contains("10.0.0.5:443"));
}

#[test]
fn you_can_ask_how_the_other_machine_is() {
    let mut w = watcher();
    w.observe("homelab", true, 0);
    assert!(w.summary().contains("All 1 up"));
    assert!(Watcher::default().summary().contains("Not watching"));
}

// ================= named modes =================

fn modes() -> Modes {
    let mut m = Modes::default();
    for x in suggested() {
        m.add(x);
    }
    m
}

#[test]
fn saying_the_mode_name_switches_to_it() {
    let m = modes();
    assert_eq!(m.match_trigger("go into focus mode").unwrap().name, "focus");
    assert_eq!(m.match_trigger("i'm on a call").unwrap().name, "call");
    assert!(m.match_trigger("open chrome").is_none());
}

#[test]
fn call_mode_makes_atlas_stop_talking() {
    // Half the value of saying "call mode" is that it shuts up.
    let mut m = modes();
    m.enter("call", &[]);
    assert!(!m.may_interrupt(false));
    assert!(!m.may_interrupt(true), "not even for urgent things");
}

#[test]
fn focus_mode_allows_only_things_worth_missing() {
    let mut m = modes();
    m.enter("focus", &[]);
    assert!(!m.may_interrupt(false));
    assert!(m.may_interrupt(true));
}

#[test]
fn entering_a_mode_says_what_to_open_and_close() {
    let mut m = modes();
    let t = m.enter("research", &[]).unwrap();
    assert!(t.open.contains(&"chrome".to_string()));
    assert!(t.rules_on.contains(&"new document landed".to_string()));
    assert_eq!(t.say, "research mode.");
}

#[test]
fn leaving_restores_what_was_open_before() {
    // A mode you can't get out of cleanly is a mode you stop using.
    let mut m = modes();
    m.enter("research", &["discord".to_string(), "claude".to_string()]);
    let out = m.leave().unwrap();
    assert!(out.close.contains(&"chrome".to_string()));
    assert!(out.open.contains(&"discord".to_string()), "put back what was there");
    assert!(m.active().is_none());
}

#[test]
fn leaving_also_undoes_the_rule_changes() {
    let mut m = modes();
    m.enter("research", &[]);
    let out = m.leave().unwrap();
    assert!(out.rules_off.contains(&"new document landed".to_string()));
}

#[test]
fn with_no_mode_active_atlas_behaves_normally() {
    let m = Modes::default();
    assert!(m.may_interrupt(false));
    // Was `assert_eq!(m.verbosity(), 2)`. `verbosity()` is gone: it was
    // `verbosity_if_set().unwrap_or(2)`, and that default, used as a ceiling
    // in `run_command`, capped every conversation at three sentences on a
    // fresh install. The test's point stands -- no mode on means no mode
    // preference -- and this is that, without the placeholder.
    assert_eq!(m.verbosity_if_set(), None);
}

#[test]
fn a_custom_mode_can_be_added() {
    let mut m = Modes::default();
    m.add(Mode {
        name: "trading".into(),
        open: vec!["chrome".into()],
        close: vec![],
        interruptions: Interruptions::Urgent,
        rules_on: vec![],
        rules_off: vec![],
        lights: None,
        verbosity: 1,
        triggers: vec!["trading mode".into()],
    });
    assert_eq!(m.match_trigger("switch to trading mode").unwrap().name, "trading");
}

// ================= how much to say =================

#[test]
fn atlas_is_terse_when_you_are_deep_in_something() {
    assert_eq!(sentences_for(1), 1);
}

#[test]
fn asking_for_an_explanation_always_wins() {
    assert_eq!(sentences_for(3), 6);
}
