use atlas::capability::{
    all, blocked, can, full, in_area, offline_count, since, summary, what_would_unblock_most, Area,
    State,
};
use atlas::routine::{ask_about, sends_something, starting, RoutineConfig, Watcher};

// ================= knowing what it can do =================

#[test]
fn built_but_never_run_is_not_the_same_as_working() {
    // Pretending otherwise is how you find out at the worst moment.
    assert!(!State::Untested.usable());
    assert!(!State::Blocked.usable());
    assert!(!State::Off.usable());
    assert!(State::Working.usable());
    assert_eq!(State::Untested.plain(), "built, never run for real");
}

#[test]
fn every_capability_that_is_not_working_says_what_it_is_waiting_on() {
    for c in all() {
        if c.state == State::Blocked || c.state == State::Planned {
            assert!(c.needs.is_some(), "{} doesn't say what it needs", c.id);
        }
    }
}

#[test]
fn things_blocked_on_one_install_are_grouped_because_that_is_one_job() {
    // Most of what was "blocked on whisper" turned out to be blocked on being
    // wired at all, which is a more honest and more useful answer.
    let by_need = what_would_unblock_most();
    assert!(!by_need.is_empty());
    let (need, count) = by_need[0];
    assert!(count >= 2, "{need} unblocks {count}");
}

#[test]
fn the_summary_is_honest_about_all_four_states() {
    let s = summary();
    assert!(s.contains("work right now"));
    assert!(s.contains("switched off"));
    assert!(s.contains("waiting on"));
    assert!(s.contains("never run on your machine"), "got: {s}");
}

#[test]
fn asking_whether_it_can_do_something_gets_a_useful_no() {
    // Not just "no". `recall` used to be the example of a working capability
    // here, and it was advertising a state its unreachable code can't
    // support — the honesty test caught it. `doctor` genuinely works.
    let (yes, _) = can("doctor").unwrap();
    assert!(yes, "doctor is reachable and really does run");

    let (no, why) = can("tune").unwrap();
    assert!(!no);
    assert!(why.contains("switched off") && why.contains("settings"));

    // `dictate` said "not built yet" until 19 Sep 2026, and that was one of
    // 27 capabilities left at `Planned` after being wired -- it has six
    // production call sites in `daemon.rs`. What it is actually waiting on is
    // whisper, which nothing fetches, so it is `Blocked` and says which thing
    // is missing. That is a more useful no: "not built yet" leaves you
    // nothing to do, and "not until whisper is installed" tells you what to
    // go and get.
    let (no, why) = can("dictate").unwrap();
    assert!(!no);
    assert!(why.contains("whisper"), "a blocked capability must name what it waits on: {why}");

    // `mesh` is the standing example of genuinely not built, now that
    // `plainchange` is wired. It is on CAPABILITY_UNWIRED with a reason, which
    // is what `Planned` requires.
    let (_, why) = can("mesh").unwrap();
    assert!(why.contains("not built yet"), "unreachable, so it says so: {why}");

    // "overlay" used to be the never-run-for-real example, but it's
    // unreachable too, so it now honestly reports as not built. "layout" is
    // the real example of code that compiles and has never touched Windows.
    let (_, why) = can("layout").unwrap();
    assert!(why.contains("never run on a real machine"), "and it doesn't pretend: {why}");
}

#[test]
fn asking_about_something_it_has_never_heard_of_returns_nothing() {
    assert!(can("book me a flight").is_none());
}

#[test]
fn most_of_what_atlas_does_works_with_the_network_unplugged() {
    let (offline, total) = offline_count();
    assert!(offline as f32 / total as f32 > 0.8, "{offline} of {total}");
}

#[test]
fn the_full_list_is_grouped_by_area_and_marks_the_state() {
    let f = full();
    assert!(f.contains("HEARING YOU"));
    assert!(f.contains("WORKING THINGS OUT"));
    assert!(f.contains("! waiting"), "with a key");
    assert!(f.contains("? never run for real"));
}

#[test]
fn what_is_new_is_answerable() {
    let recent = since(18);
    assert!(!recent.is_empty());
    assert!(recent[0].added >= recent[recent.len() - 1].added, "newest first");
    assert!(recent.iter().any(|c| c.id == "interrupt"));
}

#[test]
fn every_area_has_something_in_it() {
    for area in [Area::Hearing, Area::Speaking, Area::Files, Area::Thinking, Area::Itself] {
        assert!(!in_area(area).is_empty(), "{area:?} is empty");
    }
}

#[test]
fn nothing_is_listed_twice() {
    let mut ids: Vec<&str> = all().iter().map(|c| c.id).collect();
    let n = ids.len();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), n);
}

#[test]
fn the_blocked_list_names_the_thing_and_the_reason_together() {
    for (c, need) in blocked() {
        assert!(!need.is_empty(), "{} is blocked on nothing", c.id);
    }
}

// ================= learning what you do the same way every time =================

fn cfg() -> RoutineConfig {
    RoutineConfig { enabled: true, ..Default::default() }
}

/// Monday morning, three weeks running.
fn three_mondays() -> Watcher {
    let mut w = Watcher::default();
    for week in 0..3u64 {
        let base = week * 7 * 86_400;
        w.did("open the trading dashboard", base, 8, 0);
        w.did("check overnight fills", base + 60, 8, 0);
        w.did("open the calendar", base + 120, 8, 0);
    }
    w
}

#[test]
fn a_sequence_repeated_three_times_is_spotted() {
    let mut w = three_mondays();
    let found = w.find(&cfg());
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].steps.len(), 3);
    assert_eq!(found[0].seen, 3);
}

#[test]
fn twice_is_a_coincidence_and_is_not_mentioned() {
    // Asking after two is how a system becomes tiresome.
    let mut w = Watcher::default();
    for week in 0..2u64 {
        let base = week * 7 * 86_400;
        w.did("open the dashboard", base, 8, 0);
        w.did("check fills", base + 60, 8, 0);
    }
    assert!(w.find(&cfg()).is_empty());
}

#[test]
fn it_notices_it_is_always_the_same_day_and_time() {
    let mut w = three_mondays();
    let r = &w.find(&cfg())[0];
    assert_eq!(r.usual_hour, 8);
    assert_eq!(r.usual_weekday, Some(0));
}

#[test]
fn something_you_do_daily_is_not_pinned_to_a_weekday() {
    let mut w = Watcher::default();
    for day in 0..4u64 {
        let base = day * 86_400;
        w.did("open the dashboard", base, 8, day as u32);
        w.did("check fills", base + 60, 8, day as u32);
    }
    let r = &w.find(&cfg())[0];
    assert_eq!(r.usual_weekday, None, "it's daily, not Mondays");
}

#[test]
fn a_long_gap_ends_a_sequence_rather_than_joining_two() {
    let mut w = Watcher::default();
    w.did("open the dashboard", 0, 8, 0);
    w.did("check fills", 60, 8, 0);
    // Two hours later is a different thing you did.
    w.did("write the report", 7200, 10, 0);
    let seqs = w.find(&cfg());
    assert!(seqs.iter().all(|r| !r.steps.contains(&"write the report".to_string())));
}

#[test]
fn atlas_asks_rather_than_deciding_your_monday_for_you() {
    // A system that silently starts doing your routine is unnerving even when
    // it's right.
    let mut w = three_mondays();
    let r = &w.find(&cfg())[0];
    assert!(r.worth_asking());
    assert!(!r.confirmed);
    assert!(!r.due(8, 0), "nothing runs before you've said yes");

    let said = ask_about(r, &cfg());
    assert!(said.contains("Monday around 8am"));
    assert!(said.contains("3 times now"));
    assert!(said.contains("Want me to do it?"));
    assert!(said.contains("then"), "and lists the steps: {said}");
}

#[test]
fn a_routine_that_sends_something_is_flagged_before_you_agree_to_it() {
    // Not after it has posted something wrong.
    assert!(sends_something(&["draft the update".into(), "post it to LinkedIn".into()]));
    assert!(!sends_something(&["open the dashboard".into(), "check fills".into()]));

    let mut w = Watcher::default();
    for week in 0..3u64 {
        let base = week * 7 * 86_400;
        w.did("draft the weekly update", base, 17, 4);
        w.did("send it to the team", base + 60, 17, 4);
    }
    let r = &w.find(&cfg())[0];
    assert!(ask_about(r, &cfg()).contains("stop before anything that sends"));
}

#[test]
fn once_confirmed_it_becomes_due_at_the_usual_time() {
    let mut w = three_mondays();
    w.routines = w.find(&cfg());
    let name = w.routines[0].name.clone();
    assert!(w.confirm(&name, false));
    assert_eq!(w.due_now(8, 0).len(), 1);
    assert!(w.due_now(15, 0).is_empty(), "not at the wrong time");
    assert!(w.due_now(8, 3).is_empty(), "not on the wrong day");
}

#[test]
fn one_you_have_not_automated_still_asks_each_time() {
    let mut w = three_mondays();
    w.routines = w.find(&cfg());
    let name = w.routines[0].name.clone();
    w.confirm(&name, false);
    assert!(starting(&w.routines[0]).trim_end().ends_with('?'));

    w.confirm(&name, true);
    assert!(starting(&w.routines[0]).contains("say stop if not"));
}

#[test]
fn watching_for_routines_is_off_until_you_turn_it_on() {
    let mut w = three_mondays();
    assert!(w.find(&RoutineConfig::default()).is_empty());
}
