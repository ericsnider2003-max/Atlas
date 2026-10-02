//! Hands-free talk on the phone (why-stale idea 10, 2 Oct 2026): the Talk
//! page offers it only where the phone's shell can listen on its own, and
//! "that's all" ends it rather than being sent.

#[test]
fn the_talk_page_carries_hands_free_for_a_shell_that_can_converse() {
    let page = atlas::hubpages::talk_page(&[("hi".into(), "Hello.".into())], &[], true);
    // Hidden until the shell says it can (s.converse), so a browser never
    // shows a button that does nothing.
    assert!(page.contains("id=handsfree class=secondary aria-pressed=false hidden"), "{page}");
    assert!(page.contains("if(s.converse&&h){h.hidden=false;"), "only shown when the shell can converse");
    // After Atlas has spoken, listen again; nothing heard turns it off.
    assert!(page.contains("window.atlasSpoke=function(){if(hf()&&s.converse)s.converse();};"));
    assert!(page.contains("window.atlasQuiet=function(){setHf(false);"));
    // "That's all" stops it and is not sent to Atlas.
    assert!(page.contains("/^(that'?s all|stop|stop listening|goodbye|end)[.!]?$/i.test(t.trim())){setHf(false);return;}"));
    assert_eq!(page.matches("id=handsfree").count(), 1);
}

#[test]
fn the_ios_shell_listens_through_headsets_and_answers_the_page() {
    let shell = std::fs::read_to_string("mobile/ios/Atlas/Shell.swift").unwrap();
    assert!(shell.contains(".allowBluetooth") && shell.contains(".allowBluetoothA2DP"), "AirPods' microphone and speaker");
    assert!(shell.contains("case \"converse\": conversing = true; listen()"));
    assert!(shell.contains("window.atlasSpoke && window.atlasSpoke()") && shell.contains("window.atlasQuiet && window.atlasQuiet()"));
    let app = std::fs::read_to_string("mobile/ios/Atlas/AtlasApp.swift").unwrap();
    assert_eq!(app.matches("do:'converse'").count(), 1, "the page can ask the shell to converse");
}
