//! Offline labels exercise the production page with synthetic state only.
use atlas::{config::Config, connectivity::Reach, daemon::Daemon, hub::Page,
    platform::{mock::MockPlatform, Monitor}, proactive::{Proactive, ProactiveConfig},
    publish::{Channel, PostState}, server::Action, store::Store};
use std::path::Path;
fn run(tag: &str, reach: Reach, setup: impl FnOnce(&mut Daemon), check: impl FnOnce(&str)) {
    let root = std::env::temp_dir().join(format!("atlas-offline-label-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let c = Config::load(Path::new("config")).unwrap();
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let mut d = Daemon::new(&c, &p, None, Store::new(&root), Proactive::new(ProactiveConfig::default()));
    d.connectivity.set(reach, atlas::store::now());
    setup(&mut d);
    let before = serde_json::to_value(&d.publisher).unwrap();
    let outbox_before = serde_json::to_value(atlas::outbox::Outbox::load(&d.store)).unwrap();
    let html = atlas::hublive::reply(&mut d, Action::Hub(Page::Offline)).body;
    assert_eq!(outbox_before, serde_json::to_value(atlas::outbox::Outbox::load(&d.store)).unwrap());
    assert_eq!(before, serde_json::to_value(&d.publisher).unwrap(), "rendering must not approve, send, or modify publication");
    assert!(!html.contains("Anything queued is going out"));
    assert!(!html.contains("goes by itself"));
    assert!(!html.contains("Delivered"));
    check(&html);
    // Each test owns its disposable root; never points at installed state.
    let _ = std::fs::remove_dir_all(root);
}
fn post(d: &mut Daemon, approval: bool) -> u64 {
    let id = d.publisher.draft(Channel::X, "Synthetic offline label <draft>");
    d.publisher.request_approval(id).unwrap();
    if approval { assert!(d.publisher.approve(id)); }
    id
}
#[test]
fn offline_labels_approved_offline_waits_for_connection_without_sending() {
    run("approved", Reach::Offline, |d| { post(d, true); }, |h| {
        assert!(h.contains("Saved locally")); assert!(h.contains("Waiting for connection"));
        assert!(h.contains("Acceptance times are not recorded here")); assert!(h.contains("&lt;draft&gt;"));
    });
}
#[test]
fn offline_labels_unapproved_offline_waits_for_approval() {
    run("unapproved", Reach::Offline, |d| { post(d, false); }, |h| {
        assert!(h.contains("Awaiting approval")); assert!(!h.contains("Waiting for connection"));
    });
}
#[test]
fn offline_labels_connected_unapproved_still_waits_for_approval() {
    run("online-unapproved", Reach::Online, |d| { post(d, false); }, |h| {
        assert!(h.contains("You're online")); assert!(h.contains("Awaiting approval"));
        assert!(!h.contains("Waiting for connection"));
    });
}
#[test]
fn offline_labels_empty_states_do_not_promise_sending() {
    for reach in [Reach::Online, Reach::Offline, Reach::Unknown] {
        run(&format!("empty-{reach:?}"), reach, |d| { assert!(d.publisher.posts.is_empty()); }, |h| {
            assert!(h.contains("Nothing is waiting here"));
        });
    }
}
#[test]
fn offline_labels_restart_and_failure_keep_real_state() {
    run("restart-failure", Reach::Offline, |d| {
        let id = post(d, false);
        d.publisher.mark_sent(id, "Synthetic rejection", false);
        let saved = serde_json::to_vec(&d.publisher).unwrap();
        d.publisher = serde_json::from_slice(&saved).unwrap();
        assert_eq!(d.publisher.get(id).unwrap().state, PostState::Failed);
    }, |h| { assert!(h.contains("Failed; check the publication result")); assert!(!h.contains("Waiting for connection")); });
}
#[test]
fn offline_labels_unknown_connection_is_not_offline_or_sending() {
    run("unknown", Reach::Unknown, |d| { post(d, true); }, |h| {
        assert!(h.contains("Connection unknown")); assert!(h.contains("Queued; connection and execution checks pending"));
        assert!(!h.contains("Waiting for connection"));
    });
}

#[test]
fn offline_labels_waiting_mail_keeps_authorization_unknown_on_reconnect() {
    for reach in [Reach::Offline, Reach::Online] {
        run(&format!("mail-{reach:?}"), reach, |d| {
            let outbox: atlas::outbox::Outbox = serde_json::from_value(serde_json::json!({"replies": [{
                "id": "synthetic", "account": "local", "to_address": "nobody@example.invalid",
                "to_name": "Synthetic Person", "subject": "Fixture", "body": "No send", "kind": "client",
                "critique": [], "created_at": 1, "status": "waiting"
            }]})).unwrap();
            outbox.save(&d.store).unwrap();
        }, |h| {
            assert!(h.contains("A reply to Synthetic Person"));
            assert!(h.contains("Awaiting approval or standing authorization check"));
            assert!(!h.contains("Waiting for connection"));
        });
    }
}
