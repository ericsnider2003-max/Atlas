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
    struct Speaker(bool);
    impl atlas::daemon::Mouth for Speaker {
        fn speak(&self, _: &str) -> atlas::error::Result<()> {
            if self.0 { Ok(()) } else { Err(std::io::Error::other("disposable speaker failure").into()) }
        }
    }
    fn prepare(d: &mut atlas::daemon::Daemon<'_>, now: u64) -> atlas::brief::Brief {
        let answer = d.turn("what's outstanding", now);
        assert!(answer.contains("preparing"), "actual live brief route: {answer}");
        let until = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some(result) = d.poll_brief() { return result.expect("actual immutable brief preparation"); }
            assert!(std::time::Instant::now() < until, "brief worker must complete within its existing budget");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
    let mut cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let tools = cfg.tools.as_mut().unwrap(); tools.hunt.enabled = false;
    tools.sound.muted = false; tools.sound.speak_replies = "always".into(); tools.sound.quiet_hours = false;
    let platform = atlas::platform::mock::MockPlatform::new(Vec::new());
    let root = std::env::temp_dir().join(format!("atlas-application-delivery-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let store = atlas::store::Store::new(&root);
    let mut a = Applications::default();
    a.take(Heard::Applied { to: "Acme".into(), role: String::new() }, 0);
    a.take(Heard::Applied { to: "Brightside".into(), role: String::new() }, 0);
    a.take(Heard::Moved { to: "Brightside".into(), stage: Stage::Heard }, 100);
    store.save(atlas::applied::FILE, &a).unwrap();
    let mut d = atlas::daemon::Daemon::new(&cfg, &platform, None, store.clone(), atlas::proactive::Proactive::new(Default::default()));
    d.tiers.tier = atlas::input::Tier::Voice;
    // The compatibility entry exercises the exact logical one-week boundary;
    // the due offer below uses the real public asynchronous command path.
    let before = d.brief_now(FOLLOW_UP_AFTER_SECS - 1);
    assert!(!before.yours.iter().any(|item| item.from == "Application"));
    let due = prepare(&mut d, FOLLOW_UP_AFTER_SECS + 200);
    let applications: Vec<_> = due.yours.iter().filter(|item| item.from == "Application").collect();
    assert_eq!(applications.len(), 1);
    assert!(applications[0].subject.starts_with("No word from Acme in a week"));
    assert!(!store.load::<Applications>(atlas::applied::FILE).all[0].nudged, "preparation is not delivery");
    let line = atlas::brief::spoken(&due);
    d.say_volunteered_with(&Speaker(false), &line, &mut || None);
    assert!(!store.load::<Applications>(atlas::applied::FILE).all[0].nudged, "failed playback cannot consume the follow-up");
    d.say_volunteered_with(&Speaker(true), &line, &mut || None);
    assert!(store.load::<Applications>(atlas::applied::FILE).all[0].nudged);
    assert!(!store.load::<Applications>(atlas::applied::FILE).all[1].nudged, "an answered application is not a due follow-up");
    let later = prepare(&mut d, FOLLOW_UP_AFTER_SECS * 3);
    assert!(!later.yours.iter().any(|item| item.from == "Application"), "successfully delivered follow-up must stay once-only");
    drop(d); std::fs::remove_dir_all(root).unwrap();
}
