//! Keeping track of what's already set (30 Sep 2026): reminders listed,
//! cancelled and snoozed, timers, and calendar events cancelled or moved.
//! Before this a reminder could be set and never seen again, "set a timer"
//! went nowhere, and an event could be booked but not cancelled or moved.

use atlas::daemon::Daemon;
use atlas::keeping::{read, Ask, Which};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-keeping-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

/// Wednesday 30 Sep 2026, 14:00 UTC.
const NOW: u64 = 1_790_776_800;

#[test]
fn the_words_are_read() {
    assert_eq!(read("what reminders do I have?"), Some(Ask::ListReminders));
    assert_eq!(read("list my reminders"), Some(Ask::ListReminders));
    assert_eq!(read("cancel all my reminders"), Some(Ask::CancelReminder(Which::All)));
    assert_eq!(read("cancel reminder 4"), Some(Ask::CancelReminder(Which::Number(4))));
    assert_eq!(read("delete the reminder about the dentist"), Some(Ask::CancelReminder(Which::About("the dentist".into()))));
    assert_eq!(read("cancel that timer"), Some(Ask::CancelReminder(Which::Last)));
    assert_eq!(read("set a timer for 10 minutes"), Some(Ask::Timer(600)));
    assert_eq!(read("set a timer for ten minutes"), Some(Ask::Timer(600)));
    assert_eq!(read("start a 25 minute timer"), Some(Ask::Timer(1500)));
    assert_eq!(read("timer for half an hour"), Some(Ask::Timer(1800)));
    assert_eq!(read("snooze"), Some(Ask::Snooze(None)));
    assert_eq!(read("snooze for 5 minutes"), Some(Ask::Snooze(Some(300))));
    assert_eq!(read("move my 3pm to 4pm"), Some(Ask::MoveEvent { what: "3pm".into(), to: "4pm".into() }));
    assert_eq!(read("reschedule the dentist to friday at 2pm"), Some(Ask::MoveEvent { what: "dentist".into(), to: "friday at 2pm".into() }));
    assert_eq!(read("cancel my 3pm"), Some(Ask::CancelEvent("3pm".into())));
    assert_eq!(read("cancel the dentist appointment"), Some(Ask::CancelEvent("dentist".into())));
    // Not these.
    assert_eq!(read("cancel that"), None);
    assert_eq!(read("cancel the post"), None);
    assert_eq!(read("what time is it"), None);
    assert_eq!(read("remind me in 20 minutes to stretch"), None, "setting one is remind_help's");
}

#[test]
fn an_event_answers_to_its_time_or_its_name() {
    use atlas::keeping::event_answers_to;
    // 15:00 local on some day.
    let three_pm = 1_790_776_800 - 1_790_776_800 % 86_400 + 15 * 3600;
    assert!(event_answers_to("Standup", three_pm, "3pm"));
    assert!(event_answers_to("Standup", three_pm, "3"));
    assert!(event_answers_to("Standup", three_pm, "15:00"));
    assert!(!event_answers_to("Standup", three_pm, "4pm"));
    assert!(event_answers_to("Dentist with Dr Lee", three_pm, "dentist"));
    assert!(!event_answers_to("Dentist with Dr Lee", three_pm, "gym"));
}

#[test]
fn reminders_are_listed_cancelled_and_timers_set() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("reminders")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.turn("what reminders do I have", NOW), "No reminders or timers set.");
    let set = d.turn("remind me in 20 minutes to stretch", NOW);
    assert!(set.contains("stretch"), "{set}");
    let set2 = d.turn("remind me in 2 hours to call mom", NOW);
    assert!(set2.contains("call mom"), "{set2}");
    let timer = d.turn("set a timer for 10 minutes", NOW);
    assert!(timer.starts_with("Timer set for 10 minutes"), "{timer}");
    let list = d.turn("what reminders do I have", NOW);
    assert!(list.starts_with("3:"), "{list}");
    assert!(list.contains("stretch in 20 minutes") && list.contains("call mom") && list.contains("the 10-minute timer in 10 minutes"), "{list}");
    let cut = d.turn("cancel the reminder to call mom", NOW);
    assert_eq!(cut, "Cancelled: call mom.");
    let last = d.turn("cancel that timer", NOW);
    assert_eq!(last, "Cancelled: the 10-minute timer.", "the last one set was the timer");
    let left = d.turn("list my reminders", NOW);
    assert!(left.starts_with("One:") && left.contains("stretch"), "{left}");
    assert_eq!(d.turn("cancel all reminders", NOW), "Cancelled: stretch.");
    assert_eq!(d.turn("what reminders do I have", NOW), "No reminders or timers set.");
}

#[test]
fn a_reminder_with_no_time_asks_when_and_the_answer_sets_it() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("no-time")), Proactive::new(ProactiveConfig::default()));
    let asked = d.turn("remind me to water the plants", NOW);
    assert!(asked.starts_with("When should I remind you to water the plants"), "{asked}");
    let set = d.turn("in 30 minutes", NOW);
    assert!(set.contains("water the plants") && set.contains("30 minutes"), "{set}");
    assert!(d.turn("what reminders do I have", NOW).contains("water the plants"));
}

#[test]
fn a_reminder_that_fired_can_be_snoozed() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("snooze")), Proactive::new(ProactiveConfig::default()));
    let _ = d.turn("remind me in 1 minutes to stretch", NOW);
    // Nothing fired yet: "snooze" isn't taken.
    let _ = d.tick(NOW + 120);
    assert!(d.last_reminder_fired.as_ref().is_some_and(|(c, _)| c.contains("stretch")), "it fired");
    let snoozed = d.turn("snooze", NOW + 130);
    assert_eq!(snoozed, "I'll say it again in 10 minutes.");
    let list = d.turn("what reminders do I have", NOW + 131);
    assert!(list.contains("stretch in 10 minutes"), "{list}");
}

#[test]
fn an_event_is_cancelled_and_moved_by_voice() {
    let c = cfg();
    let p = plat();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("calendar")), Proactive::new(ProactiveConfig::default()));
    let booked = d.turn("schedule dentist tomorrow at 3pm", NOW);
    assert!(booked.to_lowercase().contains("dentist"), "{booked}");
    let _ = d.turn("schedule gym tomorrow at 6pm", NOW);
    let moved = d.turn("move the dentist to 4pm", NOW);
    assert!(moved.starts_with("Moved dentist to"), "{moved}");
    let agenda = d.turn("what's on tomorrow", NOW);
    assert!(agenda.to_lowercase().contains("dentist"), "{agenda}");
    let gone = d.turn("cancel the gym", NOW);
    assert!(gone.starts_with("Taken off your calendar: gym"), "{gone}");
    // Something that isn't an event is left for everything else to answer.
    assert!(!d.turn("cancel the concert", NOW).starts_with("Taken off"));
}
