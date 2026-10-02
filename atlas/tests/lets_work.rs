//! "Let's work" sessions (why-stale idea 3, 1 Oct 2026).

use atlas::worksession::{heard, how_it_went, Said, Session, LONGEST_SECS};

#[test]
fn a_session_is_heard_with_what_and_how_long() {
    assert_eq!(heard("Let's work on the edit for two hours."), Some(Said::Start { what: "the edit".into(), secs: 7200 }));
    assert_eq!(heard("two hours on the bakery budget"), Some(Said::Start { what: "the bakery budget".into(), secs: 7200 }));
    assert_eq!(heard("work session on the Etsy listings for 45 minutes"), Some(Said::Start { what: "the etsy listings".into(), secs: 2700 }));
    assert_eq!(heard("let's work on the thumbnails for six hours"), Some(Said::Start { what: "the thumbnails".into(), secs: LONGEST_SECS }));
    assert_eq!(heard("end the session"), Some(Said::End));
    assert_eq!(heard("let's work"), None, "no length, no session");
    assert_eq!(heard("remind me in two hours to call mum"), None);
}

#[test]
fn it_checks_in_once_halfway_and_says_what_the_record_shows() {
    let mut s = Session::new("the edit", 7200, 1000);
    assert!(!s.check_in_due(1000 + 3000));
    assert!(s.check_in_due(1000 + 3600));
    s.checked_in = true;
    assert!(!s.check_in_due(1000 + 4000));
    assert!(s.over(1000 + 7200));
    let log = atlas::worklog::summarise(&[]);
    assert_eq!(how_it_went(&s, 1000 + 3600, &log, "Your build finished."), "That's the session on the edit: 1 h (ended early). While you worked: Your build finished.");
    assert_eq!(how_it_went(&s, 1000 + 7200, &log, ""), "That's the session on the edit: 2 h.");
}

#[test]
fn by_voice_it_holds_offers_and_ends_with_a_note() {
    use atlas::daemon::Daemon;
    use atlas::platform::{mock::MockPlatform, Monitor};
    use atlas::proactive::{Proactive, ProactiveConfig};
    let mut c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let notes = std::env::temp_dir().join(format!("atlas-lets-work-notes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&notes);
    if let Some(t) = c.tools.as_mut() {
        t.research.notes_dir = notes.display().to_string();
    }
    let plat = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-lets-work-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut d = Daemon::new(&c, &plat, None, atlas::store::Store::new(dir), Proactive::new(ProactiveConfig::default()));
    let t = 1_790_776_800;
    let said = d.turn("let's work on the edit for an hour", t);
    assert!(said.starts_with("1 h on the edit. I'll hold anything that isn't urgent"), "{said}");
    assert_eq!(d.proactive.quiet_until, t + 3600);
    assert!(d.turn("let's work on the budget for an hour", t + 60).starts_with("You're already in a session on the edit"));
    assert_eq!(d.turn("how long is left", t + 600), "50 min left on the edit.");
    let said = d.turn("end the session", t + 1200);
    assert!(said.starts_with("That's the session on the edit: 20 min (ended early)."), "{said}");
    assert_eq!(d.proactive.quiet_until, 0);
    let kept = std::fs::read_to_string(notes.join("work-sessions.md")).unwrap();
    assert!(kept.contains(", the edit: That's the session on the edit"), "{kept}");
}
