//! The self-test's copy of Atlas never starts another Atlas (30 Sep 2026).
//!
//! "Show me settings" in the test opened Atlas's window from the copy, which
//! started a background Atlas there that outlived the test: two Atlases on
//! Eric's laptop, and the one answering him lived in a temp folder. Its own
//! process, because it sets an environment variable for the whole process.

#[test]
fn nothing_in_the_test_can_start_another_atlas() {
    std::env::set_var(atlas::selftest::IN_A_TEST, "1");
    assert!(atlas::selftest::in_a_test());
    let me = std::env::current_exe().unwrap();
    let e = atlas::firstlaunch::spawn_quietly(&me, &["--daemon"]).unwrap_err();
    assert!(e.to_string().contains("never starts another Atlas"), "{e}");
    let why = atlas::firstlaunch::start_background_watched(&me, &std::env::temp_dir(), std::time::Duration::from_millis(10)).unwrap_err();
    assert!(why.contains("never starts another Atlas"), "{why}");
    // 1 Oct 2026: a briefing panel opened on the screen from inside the test.
    let panel = atlas::window::open(&atlas::window::Contents::knock("A briefing")).unwrap_err();
    assert_eq!(panel, atlas::selftest::NOT_IN_A_TEST);
    let again = atlas::update_apply::relaunch_self(&me, &["--daemon".to_string()]).unwrap_err();
    assert_eq!(again, atlas::selftest::NOT_IN_A_TEST);
    assert_eq!(atlas::typebox::Standby::start(|_| {}).err().as_deref(), Some(atlas::selftest::NOT_IN_A_TEST));
    std::env::remove_var(atlas::selftest::IN_A_TEST);
    assert!(!atlas::selftest::in_a_test());
}
