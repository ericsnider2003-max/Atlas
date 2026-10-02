//! The application tracker: "I applied for … at …", what moved, the list,
//! and the one nudge after a quiet week.

use atlas::applied::{heard, Applications, Heard, Stage, FOLLOW_UP_AFTER_SECS};

#[test]
fn an_application_is_heard_with_its_role_and_company() {
    assert_eq!(
        heard("I applied for a video editor at Brightside Media."),
        Some(Heard::Applied { to: "Brightside Media".into(), role: "video editor".into() })
    );
    assert_eq!(heard("I applied to Acme"), Some(Heard::Applied { to: "Acme".into(), role: String::new() }));
    assert_eq!(heard("i applied to three jobs today and they were all remote ones"), None);
    assert_eq!(heard("Acme said no"), Some(Heard::Moved { to: "Acme".into(), stage: Stage::No }));
    assert_eq!(heard("I have an interview with Acme on Friday"), Some(Heard::Moved { to: "Acme".into(), stage: Stage::Interview }));
    assert_eq!(heard("where are my applications?"), Some(Heard::List));
    assert_eq!(heard("they said no"), None);
    assert_eq!(heard("what's the weather"), None);
}

#[test]
fn what_moved_is_kept_and_listed_open_first() {
    let mut a = Applications::default();
    let day = 86_400;
    assert!(a.take(Heard::Applied { to: "Acme".into(), role: "editor".into() }, 0).contains("editor at Acme"));
    a.take(Heard::Applied { to: "Brightside".into(), role: String::new() }, day);
    assert!(a.take(Heard::Moved { to: "acme".into(), stage: Stage::No }, 2 * day).contains("closed"));
    let said = a.listed(3 * day);
    assert_eq!(said.lines().next(), Some("1 open, 1 closed."));
    assert!(said.lines().nth(1).unwrap().starts_with("- Brightside: applied, no word yet (applied 2 days ago)"), "{said}");
    assert!(a.take(Heard::Moved { to: "Nowhere".into(), stage: Stage::Heard }, 0).starts_with("I don't have an application to Nowhere"));
}

#[test]
fn a_quiet_week_is_mentioned_once_and_never_after_they_reply() {
    let mut a = Applications::default();
    a.take(Heard::Applied { to: "Acme".into(), role: String::new() }, 0);
    a.take(Heard::Applied { to: "Brightside".into(), role: String::new() }, 0);
    a.take(Heard::Moved { to: "Brightside".into(), stage: Stage::Heard }, 100);
    assert_eq!(a.due_follow_ups(FOLLOW_UP_AFTER_SECS - 1).len(), 0);
    let due = a.due_follow_ups(FOLLOW_UP_AFTER_SECS + 200);
    assert_eq!(due.len(), 1);
    assert!(due[0].starts_with("No word from Acme in a week"));
    assert_eq!(a.due_follow_ups(FOLLOW_UP_AFTER_SECS * 3).len(), 0);
}
