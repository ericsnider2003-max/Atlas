//! Item 33 (Eric approved, 1 Oct 2026): Atlas is there every time you sit
//! down, and it records why it stopped.
//!
//! On 30 Sep Atlas was absent for 14 hours (6:37 am to 8:31 pm Pacific):
//! with the lid shut behind two monitors Windows locks and unlocks rather
//! than signing in again, and the task only started Atlas at sign-in. Of 23
//! runs, 19 ended with nothing written down about why.

use atlas::goodbye::Why;
use atlas::startup::{needs_new_triggers, task_xml, Mode, WOKE_QUERY};
use atlas::whystopped::{Ended, Runs};
use std::path::Path;

#[test]
fn the_task_starts_atlas_on_unlock_and_on_waking_as_well_as_at_sign_in() {
    let xml = task_xml(Path::new(r"C:\Users\erics\AppData\Local\Atlas\atlas.exe"), Mode::Background, Some(r"LE3O\erics"));
    assert!(xml.contains("<LogonTrigger>"));
    assert!(xml.contains("<StateChange>SessionUnlock</StateChange>"), "{xml}");
    assert!(xml.contains("<EventTrigger>"));
    // The query is escaped once, as Task Scheduler's own exports carry it.
    assert!(xml.contains("&lt;QueryList&gt;"));
    assert!(xml.contains("Power-Troubleshooter") && xml.contains("EventID=1"));
    assert!(xml.contains("Kernel-Power") && xml.contains("EventID=507"));
    // Starting it while it's running does nothing.
    assert!(xml.contains("<MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>"));
    // The user's own session, in both triggers that name one.
    assert_eq!(xml.matches(r"<UserId>LE3O\erics</UserId>").count(), 3);
}

#[test]
fn the_wake_query_is_one_select_both_events_can_match() {
    assert_eq!(WOKE_QUERY.matches("<Select").count(), 1);
    assert!(WOKE_QUERY.contains(" or "));
    assert!(WOKE_QUERY.starts_with("<QueryList>") && WOKE_QUERY.ends_with("</QueryList>"));
}

#[test]
fn an_older_task_is_brought_up_to_date_in_the_mode_it_had() {
    let old_background = "<Task><Triggers><LogonTrigger/></Triggers><Actions><Exec><Arguments>--daemon</Arguments></Exec></Actions></Task>";
    assert_eq!(needs_new_triggers(old_background), Some(Mode::Background));
    let old_listening = old_background.replace("--daemon", "--wake");
    assert_eq!(needs_new_triggers(&old_listening), Some(Mode::Listening));
    let new = task_xml(Path::new(r"C:\Atlas\atlas.exe"), Mode::Background, None);
    assert_eq!(needs_new_triggers(&new), None);
    assert_eq!(needs_new_triggers(""), None);
}

#[test]
fn a_clean_stop_says_why() {
    let mut r = Runs::default();
    assert!(r.start(1_000, Some(500)).is_none());
    r.stop(2_000, Why::YouClosedIt);
    let last = r.past.last().unwrap();
    assert_eq!(last.ended, Some(Ended::Clean(Why::YouClosedIt)));
    assert_eq!(r.last_stop(2_030).unwrap(), "I last stopped just now: you closed it.");
}

#[test]
fn a_run_that_never_said_goodbye_is_judged_by_whether_the_computer_restarted() {
    // Alive at 5,000; the computer started at 6,000: a restart.
    let mut r = Runs::default();
    r.start(1_000, Some(0));
    r.now.as_mut().unwrap().last_alive = 5_000;
    let line = r.start(9_000, Some(6_000)).expect("said");
    assert!(line.contains("computer restarted"), "{line}");
    assert_eq!(r.past.last().unwrap().ended, Some(Ended::ComputerRestarted));

    // Alive at 5,000; the computer has been up since before then: ended.
    let mut r = Runs::default();
    r.start(1_000, Some(0));
    r.now.as_mut().unwrap().last_alive = 5_000;
    let line = r.start(9_000, Some(100)).expect("said");
    assert!(line.contains("without warning"), "{line}");
    assert_eq!(r.past.last().unwrap().ended, Some(Ended::EndedWithoutWarning));
}

#[test]
fn being_alive_is_written_about_once_a_minute_not_every_tick() {
    let mut r = Runs::default();
    r.start(1_000, None);
    assert!(!r.alive(1_002));
    assert!(!r.alive(1_059));
    assert!(r.alive(1_060));
    assert!(!r.alive(1_061));
    assert!(r.alive(1_125));
}

#[test]
fn only_the_last_thirty_runs_are_kept() {
    let mut r = Runs::default();
    for i in 0..40u64 {
        r.start(i * 100, None);
        r.stop(i * 100 + 50, Why::Asked);
    }
    assert_eq!(r.past.len(), atlas::whystopped::KEEP);
    assert_eq!(r.past.first().unwrap().started, 1_000);
}

#[test]
fn the_record_survives_on_disk() {
    let dir = std::env::temp_dir().join(format!("atlas-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut r = Runs::default();
    r.start(1_000, None);
    r.save(&dir).unwrap();
    let back = Runs::load(&dir);
    assert_eq!(back, r);
    let _ = std::fs::remove_dir_all(&dir);
}

// The stop file saying "updating" is tested in atlas_can_be_told_to_stop.rs,
// beside the other tests of the process-wide stop flag (its `alone()` lock).

#[test]
fn the_self_check_says_how_atlas_last_stopped() {
    // The doctor reads the same record the run loop writes.
    let line = {
        let mut r = Runs::default();
        r.start(1_000, None);
        r.now.as_mut().unwrap().last_alive = 1_200;
        r.start(5_000, Some(100));
        r.last_stop(5_030).unwrap()
    };
    assert!(line.starts_with("I last stopped an hour ago: it was ended without warning"), "{line}");
}
