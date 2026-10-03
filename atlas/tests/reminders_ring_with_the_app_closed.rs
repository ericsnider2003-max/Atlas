//! Item 15 (Eric's yes, 1 Oct 2026): phone reminders ring with the app
//! closed. iOS stops Atlas when the app leaves the screen, so the reminders
//! still to come are listed for the app to hand to iOS as local
//! notifications -- the phone's own scheduler, nothing online.

use atlas::scheduler::Scheduler;

#[test]
fn only_reminders_still_to_come_are_listed_soonest_first() {
    let mut s = Scheduler::default();
    let now = 1_790_000_000;
    s.at("reminder Reminder: stretch", now + 1_200);
    s.at("reminder call the bank", now + 600);
    s.at("reminder already rang", now - 60);
    s.at("open spotify", now + 300); // not a reminder
    s.at("reminder next month", now + 40 * 24 * 3600); // handed over on a later visit
    let up = atlas::phonealarms::upcoming(&s, now);
    let texts: Vec<&str> = up.iter().map(|v| v["text"].as_str().unwrap()).collect();
    assert_eq!(texts, vec!["call the bank", "stretch"]);
    assert_eq!(up[0]["due"], now + 600);
    assert!(up[0]["id"].as_u64().is_some());
}

#[test]
fn no_more_than_ios_keeps() {
    let mut s = Scheduler::default();
    let now = 1_790_000_000;
    for i in 0..100 {
        s.at(&format!("reminder r{i}"), now + 60 + i);
    }
    assert_eq!(atlas::phonealarms::upcoming(&s, now).len(), atlas::phonealarms::MOST);
}

#[test]
fn the_app_hands_them_over_on_leaving_and_takes_them_back_on_return() {
    // The Swift side, read for what it must do: hand over in background(),
    // take back in begin(), and decode `upcoming` from live.json.
    let live = std::fs::read_to_string("mobile/ios/Atlas/LiveActivity.swift").unwrap();
    let core = std::fs::read_to_string("mobile/ios/Atlas/AtlasCore.swift").unwrap();
    let bg = live.split_once("func background()").unwrap().1.split("\n    }\n").next().unwrap();
    assert!(bg.contains("Reminders.handOver("), "{bg}");
    let begin = live.split_once("func begin()").unwrap().1.split("\n    }\n").next().unwrap();
    assert!(begin.contains("Reminders.takeBack()"), "{begin}");
    assert!(live.contains("UNTimeIntervalNotificationTrigger"));
    assert!(core.contains("let upcoming: [Upcoming]?"));
}
