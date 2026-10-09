use atlas::{calendar::Calendar, oauthlink::{self, Provider}, publish::{Channel, PostState, Publisher, SendCheck}, social::apis::{Net, Reply}, tz::Zone};
use std::cell::RefCell;

#[test]
fn approved_media_is_captured_and_same_path_replacement_requires_new_consent() {
    let path = std::env::temp_dir().join(format!("atlas-consent-{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    std::fs::write(&path, b"approved").unwrap();
    let mut publisher = Publisher::default();
    let id = publisher.draft(Channel::X, "review this");
    publisher.attach(id, &path.to_string_lossy());
    assert!(publisher.approve(id));
    let post = publisher.get(id).unwrap();
    let captured = post.verified_media_copies(&|| false).unwrap();
    std::fs::write(&path, b"replaced").unwrap();
    assert_eq!(std::fs::read(&captured.paths[0]).unwrap(), b"approved");
    assert!(post.verified_media_copies(&|| false).is_err());
    let wire = serde_json::to_value(&publisher).unwrap();
    let loaded: Publisher = serde_json::from_value(wire.clone()).unwrap();
    assert!(loaded.get(id).unwrap().verified_media_copies(&|| false).is_err());
    let mut legacy = wire;
    let posts = legacy.get_mut("posts").unwrap().as_array_mut().unwrap();
    posts[0].as_object_mut().unwrap().remove("media_approval");
    posts[0]["state"] = serde_json::json!("ready_to_send");
    posts[0]["approved_text"] = serde_json::json!("review this");
    let loaded: Publisher = serde_json::from_value(legacy).unwrap();
    assert_eq!(loaded.get(id).unwrap().state, PostState::Held);
    assert!(matches!(loaded.check(id, true, None), SendCheck::Hold(_)));
    assert!(post.verified_media_copies(&|| true).is_err());
    struct NoNetwork;
    impl atlas::social::posting::Xrpc for NoNetwork {
        fn call(&self, _: &str, _: Option<&str>, _: &str, _: &[u8]) -> Result<(u16, String), String> { panic!("changed attachments must be blocked before any provider call") }
    }
    std::fs::write(&path, b"approved").unwrap();
    assert!(matches!(atlas::delivery::send_bluesky_unless(&mut publisher, &NoNetwork, "fixture.test", "fake password", id, true, 10, &|| true), atlas::delivery::Outcome::Blocked(_)));
    std::fs::write(&path, b"replaced").unwrap();
    assert!(matches!(atlas::delivery::send_bluesky_unless(&mut publisher, &NoNetwork, "fixture.test", "fake password", id, true, 10, &|| false), atlas::delivery::Outcome::Blocked(_)));
    drop(captured);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn preparation_receipt_never_claims_done_without_owner_review() {
    let valid = atlas::content::review_worker_result("creator_review", "review_ready", "Review these suggestions".into(), None, vec![], vec![]).unwrap();
    assert!(atlas::content::parse_review_worker(&valid).is_ok());
    for (key, value) in [("outcome", serde_json::json!("done")), ("version", serde_json::json!(2)), ("kind", serde_json::json!("publication"))] {
        let mut broken: serde_json::Value = serde_json::from_str(&valid).unwrap();
        broken[key] = value;
        assert!(atlas::content::parse_review_worker(&broken.to_string()).is_err());
    }
    assert!(atlas::content::parse_review_worker("legacy finished text").is_err());
}

#[test]
fn background_approval_receipt_applies_only_to_the_exact_current_draft() {
    let mut publisher = Publisher::default();
    let id = publisher.draft(Channel::X, "reviewed words");
    let expected = publisher.get(id).unwrap().clone();
    let mut worker = publisher.clone();
    assert!(worker.schedule(id, 1234));
    assert!(worker.approve(id));
    let receipt = atlas::publish::PostApprovalReceipt { tag: "atlas.post_approval".into(), version: 1, expected, approved: worker.get(id).unwrap().clone(), requested_at: 1000, requested_send_at: Some(1234) };
    let encoded = serde_json::to_string(&receipt).unwrap();
    let receipt = serde_json::from_str(&encoded).unwrap();
    let mut changed = publisher.clone();
    changed.edit(id, "different words");
    assert!(!changed.apply_approval_receipt(&receipt));
    assert!(publisher.apply_approval_receipt(&receipt));
    assert_eq!(publisher.get(id).unwrap().send_at, Some(1234));
    assert!(!publisher.apply_approval_receipt(&receipt));
}

#[test]
fn mail_send_requires_exact_review_and_never_substitutes_another_mailbox() {
    let root = std::env::temp_dir().join(format!("atlas-mail-owner-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    let store = atlas::store::Store::new(root);
    let draft = atlas::outbox::PendingReply { id: "owner-reviewed".into(), account: "removed mailbox".into(), to_address: "fixture@example.test".into(), to_name: "Fixture".into(), subject: "Review".into(), body: "Original words".into(), kind: atlas::outbox::Kind::Client, critique: vec![], created_at: 100, status: atlas::outbox::Status::Waiting, thread: Default::default() };
    atlas::outbox::Outbox::keep_draft(&store, draft.clone()).unwrap();
    let mut cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    cfg.tools.as_mut().unwrap().mail.accounts = vec![atlas::mail::Account { name: "different mailbox".into(), address: "different@example.test".into(), ..Default::default() }];
    let platform = atlas::platform::mock::MockPlatform::new(vec![]);
    let mut daemon = atlas::daemon::Daemon::new(&cfg, &platform, None, store.clone(), atlas::proactive::Proactive::new(Default::default()));
    let review = daemon.turn("send the reply to Fixture", 1000);
    assert!(review.contains("Nothing sent"), "{review}");
    let mut changed = draft.clone(); changed.body = "Changed words".into();
    atlas::outbox::Outbox::keep_draft(&store, changed).unwrap();
    let review = daemon.turn("send it", 1001);
    assert!(review.contains("Changed words") && review.contains("Nothing sent"), "{review}");
    let refused = daemon.turn("send it", 1002);
    assert!(refused.contains("original mailbox") && refused.contains("Nothing sent"), "{refused}");
    assert_eq!(atlas::outbox::Outbox::load(&store).waiting().len(), 1);
}

fn snapshot(title: Option<&str>) -> String {
    format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{}END:VCALENDAR\r\n", title.map(|t| format!("BEGIN:VEVENT\r\nUID:same-uid\r\nSUMMARY:{t}\r\nDTSTART:20261008T100000Z\r\nDTEND:20261008T110000Z\r\nEND:VEVENT\r\n")).unwrap_or_default())
}
fn titles(calendar: &Calendar) -> Vec<String> {
    serde_json::to_value(calendar).unwrap()["events"].as_array().unwrap().iter().map(|e| e["title"].as_str().unwrap().to_string()).collect()
}

#[test]
fn removal_is_source_scoped_and_same_uid_in_other_calendar_is_preserved() {
    let mut calendar = Calendar::default();
    calendar.import_ics(&snapshot(Some("local invite")), 0, &Zone::utc()).unwrap();
    calendar.reconcile_ics_window("account-one", &snapshot(Some("cancel this")), 0, &Zone::utc(), None).unwrap();
    calendar.reconcile_ics_window("account-two", &snapshot(Some("keep this")), 0, &Zone::utc(), None).unwrap();
    assert_eq!(titles(&calendar).len(), 3);
    calendar.reconcile_ics_window("account-one", &snapshot(None), 0, &Zone::utc(), None).unwrap();
    assert_eq!(titles(&calendar), ["local invite", "keep this"]);
}

#[test]
fn malformed_snapshot_does_not_erase_cache_and_window_preserves_history() {
    let mut calendar = Calendar::default();
    calendar.reconcile_ics_window("one", &snapshot(Some("saved")), 0, &Zone::utc(), None).unwrap();
    let before = serde_json::to_value(&calendar).unwrap();
    assert!(calendar.reconcile_ics_window("one", "this is an error page", 0, &Zone::utc(), None).is_err());
    assert_eq!(serde_json::to_value(&calendar).unwrap(), before);
    calendar.reconcile_ics_window("one", &snapshot(None), 0, &Zone::utc(), Some((0, 1))).unwrap();
    assert_eq!(serde_json::to_value(&calendar).unwrap(), before);
}

struct Pages { replies: RefCell<Vec<Reply>>, paths: RefCell<Vec<String>> }
impl Net for Pages {
    fn get(&self, _: &str, path: &str, _: &[(&str, &str)]) -> Result<Reply, String> { self.paths.borrow_mut().push(path.into()); Ok(self.replies.borrow_mut().remove(0)) }
    fn post_form(&self, _: &str, _: &str, _: &str) -> Result<Reply, String> { Ok(reply(r#"{"access_token":"test","expires_in":3600}"#)) }
    fn post_json(&self, _: &str, _: &str, _: &[(&str, &str)], _: &str) -> Result<Reply, String> { unreachable!() }
}
fn reply(body: &str) -> Reply { Reply { status: 200, body: body.into(), ..Default::default() } }

#[test]
fn google_reads_second_page_and_selected_calendar_and_keeps_recurring_occurrences_distinct() {
    let net = Pages { replies: RefCell::new(vec![
        reply(r#"{"items":[{"id":"one","iCalUID":"series","summary":"first","start":{"dateTime":"2026-10-08T10:00:00Z"},"end":{"dateTime":"2026-10-08T11:00:00Z"}}],"nextPageToken":"next"}"#),
        reply(r#"{"items":[{"id":"two","iCalUID":"series","summary":"second","start":{"dateTime":"2026-10-09T10:00:00Z"},"end":{"dateTime":"2026-10-09T11:00:00Z"}}]}"#)
    ]), paths: Default::default() };
    let text = oauthlink::calendar_snapshot_with_access(&net, Provider::Google, "fake-access", 1_791_450_000, &["work@example.com".into()]).unwrap();
    assert!(net.paths.borrow()[0].contains("work%40example.com"));
    assert!(net.paths.borrow()[1].contains("pageToken=next"));
    let mut calendar = Calendar::default();
    calendar.reconcile_ics_window("google", &text, 0, &Zone::utc(), None).unwrap();
    assert_eq!(titles(&calendar), ["first", "second"]);
}

#[test]
fn a_failed_or_foreign_continuation_never_becomes_an_empty_successful_snapshot() {
    for page in [Reply { status: 503, ..reply("unavailable") }, reply(r#"{"value":[],"@odata.nextLink":"https://evil.example/v1.0/steal"}"#)] {
        let net = Pages { replies: RefCell::new(vec![page]), paths: Default::default() };
        assert!(oauthlink::calendar_snapshot_with_access(&net, Provider::Microsoft, "fake-access", 1_791_450_000, &[]).is_err());
    }
}

#[test]
fn ambiguous_submission_survives_restart_and_cannot_be_approved_edited_or_sent_again() {
    let mut publisher = Publisher::default();
    let id = publisher.draft(Channel::X, "hello"); publisher.approve(id);
    publisher.mark_submission(id, true, "lost confirmation");
    let mut restored: Publisher = serde_json::from_value(serde_json::to_value(&publisher).unwrap()).unwrap();
    assert_eq!(restored.get(id).unwrap().state, PostState::Uncertain);
    assert!(matches!(restored.check(id, true, None), SendCheck::Hold(_)));
    assert!(restored.due(u64::MAX, true).is_empty());
    assert!(!restored.approve(id)); assert!(!restored.edit(id, "retry"));
    assert!(restored.reconcile_submission(id, None));
    assert!(matches!(restored.check(id, true, None), SendCheck::Hold(_)), "absence requires fresh approval");
    assert!(restored.approve(id));
    assert_eq!(restored.check(id, true, None), SendCheck::Go);
}

#[test]
fn provider_receipt_is_kept_and_confirmed_submission_stays_unsendable() {
    let mut publisher = Publisher::default(); let id = publisher.draft(Channel::X, "hello");
    publisher.mark_submission(id, false, "pending");
    assert!(publisher.reconcile_submission(id, Some("https://x.com/person/status/123")));
    assert_eq!(publisher.get(id).unwrap().result.as_deref(), Some("Owner checked publication: https://x.com/person/status/123"));
    assert!(!publisher.approve(id));
}

#[test]
fn owner_reconciliation_form_refuses_unchecked_or_wrong_provider_receipts_and_requires_fresh_approval() {
    use atlas::{config::Config, daemon::Daemon, platform::mock::MockPlatform, proactive::{Proactive, ProactiveConfig}, store::Store};
    let root = std::env::temp_dir().join(format!("atlas-publication-check-{}-{}", std::process::id(), atlas::store::now()));
    let cfg = Config::load(std::path::Path::new("config")).unwrap();
    let platform = MockPlatform::new(vec![]);
    let mut daemon = Daemon::new(&cfg, &platform, None, Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    let id = daemon.publisher.draft(Channel::X, "hello");
    daemon.publisher.mark_submission(id, true, "confirmation lost");
    let page = atlas::connecting::publication_status(&daemon.publisher);
    assert!(page.contains("publication unconfirmed") && page.contains("I won't retry automatically"));
    let fields = |checked: &str, receipt: &str, resolution: &str| vec![("what".into(), "post-reconcile".into()), ("id".into(), id.to_string()), ("checked".into(), checked.into()), ("receipt".into(), receipt.into()), ("resolution".into(), resolution.into())];
    atlas::connecting::post(&mut daemon, &fields("", "https://x.com/me/status/123", "published"));
    assert_eq!(daemon.publisher.get(id).unwrap().state, PostState::Uncertain);
    atlas::connecting::post(&mut daemon, &fields("yes", "https://x.com.evil.example/me/status/123", "published"));
    assert_eq!(daemon.publisher.get(id).unwrap().state, PostState::Uncertain);
    atlas::connecting::post(&mut daemon, &fields("yes", "", "absent"));
    assert_eq!(daemon.publisher.get(id).unwrap().state, PostState::Draft);
    assert!(matches!(daemon.publisher.check(id, true, None), SendCheck::Hold(_)));
    let page = atlas::connecting::publication_status(&daemon.publisher);
    assert!(page.contains("hello") && page.contains("Attachments: None") && page.contains("as soon as a connection is available"));
    let review = atlas::publish::review_fingerprint(daemon.publisher.get(id).unwrap());
    let approval = |checked: &str, review: &str| vec![("what".into(), "post-reapprove".into()), ("id".into(), id.to_string()), ("checked".into(), checked.into()), ("review".into(), review.into())];
    atlas::connecting::post(&mut daemon, &approval("", &review));
    assert_eq!(daemon.publisher.get(id).unwrap().state, PostState::Draft);
    atlas::connecting::post(&mut daemon, &approval("yes", "stale review"));
    assert_eq!(daemon.publisher.get(id).unwrap().state, PostState::Draft);
    atlas::connecting::post(&mut daemon, &approval("yes", &review));
    assert_eq!(daemon.publisher.get(id).unwrap().state, PostState::ReadyToSend);
    // No tick or publication is run: approval is the only action tested.
    drop(daemon); let _ = std::fs::remove_dir_all(root);
}

#[test]
fn explicit_ics_cancellation_removes_only_its_source() {
    let mut calendar = Calendar::default();
    calendar.reconcile_ics_window("one", &snapshot(Some("cancelled")), 0, &Zone::utc(), None).unwrap();
    let cancelled = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VEVENT\r\nUID:same-uid\r\nSTATUS:CANCELLED\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n";
    calendar.reconcile_ics_window("one", cancelled, 0, &Zone::utc(), None).unwrap();
    assert!(titles(&calendar).is_empty());
}

#[test]
fn cancelled_provider_event_removes_previously_cached_meeting() {
    let mut calendar = Calendar::default();
    let active = oauthlink::ics_from_google(r#"{"items":[{"id":"meeting","summary":"cancel me","start":{"dateTime":"2026-10-08T10:00:00Z"},"end":{"dateTime":"2026-10-08T11:00:00Z"}}]}"#).unwrap();
    let cancelled = oauthlink::ics_from_google(r#"{"items":[{"id":"meeting","status":"cancelled"}]}"#).unwrap();
    // The original import boundary retained the cancelled meeting.
    calendar.import_ics(&active, 0, &Zone::utc()).unwrap();
    calendar.import_ics(&cancelled, 0, &Zone::utc()).unwrap();
    assert_eq!(titles(&calendar), ["cancel me"]);
    // A source-owned connection snapshot reconciles the actual absence.
    let mut connected = Calendar::default();
    connected.reconcile_ics_window("google", &active, 0, &Zone::utc(), None).unwrap();
    connected.reconcile_ics_window("google", &cancelled, 0, &Zone::utc(), None).unwrap();
    assert!(titles(&connected).is_empty());
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct LegacyPublisher { posts: Vec<LegacyPost> }
#[derive(serde::Serialize, serde::Deserialize)]
struct LegacyPost { id: u64, state: LegacyState, approved_text: Option<String>, result: Option<String> }
#[derive(serde::Serialize, serde::Deserialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
enum LegacyState { Held, Draft, Scheduled, ReadyToSend, Sent, Failed, Cancelled, AwaitingApproval }

#[test]
fn rollback_sees_held_without_approval_and_keeps_the_additive_submission_fence_on_save() {
    use atlas::store::Store;
    for uncertain in [false, true] {
        let root = std::env::temp_dir().join(format!("atlas-post-rollback-{}-{uncertain}", std::process::id()));
        let store = Store::new(root.clone());
        let mut publisher = Publisher::default(); let id = publisher.draft(Channel::X, "hello"); publisher.approve(id);
        publisher.mark_submission(id, uncertain, "publication unconfirmed"); publisher.save(&store).unwrap();
        let wire = serde_json::to_value(&publisher).unwrap();
        assert_eq!(wire["posts"][0]["state"], "held", "old reader must never see a new enum variant");
        let mut legacy: LegacyPublisher = store.load("posts");
        assert_eq!(legacy.posts.len(), 1);
        assert_eq!(legacy.posts[0].state, LegacyState::Held);
        assert!(legacy.posts[0].approved_text.is_none(), "rollback must have no usable prior approval");
        legacy.posts[0].result = Some("An older version saved this note".into()); store.save("posts", &legacy).unwrap();
        let mut restored = Publisher::load(&store);
        assert_eq!(restored.get(id).unwrap().state, if uncertain { PostState::Uncertain } else { PostState::PendingSubmission });
        assert!(restored.due(u64::MAX, true).is_empty());
        assert!(!restored.approve(id));
        let _ = std::fs::remove_dir_all(root);
    }
}

struct BrokenBluesky { fail_login: bool, lose_reply: bool, submissions: std::cell::Cell<usize> }
impl atlas::social::posting::Xrpc for BrokenBluesky {
    fn call(&self, nsid: &str, _: Option<&str>, _: &str, _: &[u8]) -> Result<(u16, String), String> {
        if nsid == "com.atproto.server.createSession" {
            if self.fail_login { return Err("offline before submission".into()); }
            return Ok((200, r#"{"accessJwt":"fixture","did":"did:plc:fixture"}"#.into()));
        }
        assert_eq!(nsid, "com.atproto.repo.createRecord");
        self.submissions.set(self.submissions.get() + 1);
        if self.lose_reply { Err("connection lost after request".into()) } else { Ok((200, "{}".into())) }
    }
}

#[test]
fn bluesky_lost_create_reply_or_missing_receipt_is_fenced_and_never_repeated() {
    for lose_reply in [false, true] {
        let x = BrokenBluesky { fail_login: false, lose_reply, submissions: Default::default() };
        let mut publisher = Publisher::default(); let id = publisher.draft(Channel::Other("bluesky".into()), "hello"); publisher.approve(id);
        assert!(matches!(atlas::delivery::send_bluesky_unless(&mut publisher, &x, "fixture.bsky.social", "fixture", id, true, 0, &|| false), atlas::delivery::Outcome::Uncertain(_)));
        assert_eq!(publisher.get(id).unwrap().state, PostState::Uncertain);
        assert!(matches!(atlas::delivery::send_bluesky_unless(&mut publisher, &x, "fixture.bsky.social", "fixture", id, true, 0, &|| false), atlas::delivery::Outcome::Blocked(_)));
        assert_eq!(x.submissions.get(), 1);
    }
}

#[test]
fn bluesky_sign_in_failure_before_submission_keeps_safe_retry_available() {
    let x = BrokenBluesky { fail_login: true, lose_reply: false, submissions: Default::default() };
    let mut publisher = Publisher::default(); let id = publisher.draft(Channel::Other("bluesky".into()), "hello"); publisher.approve(id);
    assert!(matches!(atlas::delivery::send_bluesky_unless(&mut publisher, &x, "fixture.bsky.social", "fixture", id, true, 0, &|| false), atlas::delivery::Outcome::Retry(_)));
    assert_eq!(publisher.get(id).unwrap().state, PostState::ReadyToSend);
    assert_eq!(x.submissions.get(), 0);
}
