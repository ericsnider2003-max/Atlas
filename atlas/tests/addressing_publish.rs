use atlas::addressing::{assess, respond, Directed, Kind, Response, Situation};
use atlas::publish::{Channel, PostState, Publisher, SendCheck};

fn working() -> Situation {
    Situation { working: true, ..Default::default() }
}
fn idle() -> Situation {
    Situation::default()
}

// ================= was that meant for Atlas? =================

#[test]
fn the_wake_word_settles_it_outright() {
    let s = Situation { after_wake_word: true, ..working() };
    let a = assess("stop", &s);
    assert_eq!(a.directed, Directed::AtAtlas);
    assert!(a.confidence > 0.95);
}

#[test]
fn saying_its_name_settles_it_too() {
    assert_eq!(assess("atlas stop that", &working()).directed, Directed::AtAtlas);
    assert_eq!(assess("hold on atlas", &working()).directed, Directed::AtAtlas);
}

#[test]
fn a_plain_instruction_reads_as_directed_at_atlas() {
    let a = assess("open chrome", &idle());
    assert_eq!(a.directed, Directed::AtAtlas);
    assert_eq!(a.kind, Kind::Command);
}

#[test]
fn half_a_phone_call_is_not_a_command() {
    // The failure this exists to prevent: abandoning work because you took
    // a call.
    for said in [
        "yeah no I told him we would sort it out",
        "she said the meeting moved to Thursday",
        "no yeah anyway so I was like whatever",
    ] {
        let a = assess(said, &working());
        assert_eq!(a.directed, Directed::Overheard, "{said:?} -> {a:?}");
        assert_eq!(respond(&a, &working()), Response::Ignore, "must keep working");
    }
}

#[test]
fn talking_about_someone_else_is_not_talking_to_atlas() {
    let a = assess("he wants them to send her the file", &working());
    assert_eq!(a.directed, Directed::Overheard);
}

#[test]
fn another_persons_name_pushes_it_towards_overheard() {
    let s = Situation { other_names: vec!["Sarah".into()], ..working() };
    let a = assess("sarah can you close that window", &s);
    assert_ne!(a.directed, Directed::AtAtlas, "{a:?}");
}

#[test]
fn a_stray_fragment_does_not_stop_a_running_task() {
    let a = assess("um", &working());
    assert_ne!(respond(&a, &working()), Response::Act);
}

#[test]
fn an_unclear_utterance_asks_rather_than_abandoning_work() {
    // Interrupting wrongly throws work away; ignoring you is merely annoying.
    // So while working, unclear becomes a question.
    let a = assess("the file on the desktop somewhere", &working());
    assert_eq!(a.directed, Directed::Unclear, "{a:?}");
    match respond(&a, &working()) {
        Response::Ask(q) => assert!(q.contains("talking to me")),
        o => panic!("expected a question, got {o:?}"),
    }
}

#[test]
fn the_same_unclear_utterance_is_acted_on_when_nothing_is_running() {
    let a = assess("the file on the desktop somewhere", &idle());
    assert_eq!(respond(&a, &idle()), Response::Act, "nothing to lose");
}

#[test]
fn the_answer_to_atlas_own_question_is_always_for_atlas() {
    let s = Situation { awaiting_answer: true, ..working() };
    let a = assess("the second one", &s);
    assert_eq!(a.directed, Directed::AtAtlas);
    assert!(a.why.contains("asked you"));
}

#[test]
fn questions_and_commands_are_told_apart() {
    assert_eq!(assess("what is on my screen", &idle()).kind, Kind::Question);
    assert_eq!(assess("close chrome now", &idle()).kind, Kind::Command);
    assert_eq!(assess("the weather is nice today", &idle()).kind, Kind::Statement);
}

#[test]
fn every_assessment_explains_itself() {
    let a = assess("yeah no I told him", &working());
    assert!(!a.why.is_empty(), "you should be able to see why it decided");
}

// ================= posting and scheduling =================

fn tweet(p: &mut Publisher) -> u64 {
    p.draft(Channel::X, "Shipping something new today.")
}

#[test]
fn a_draft_is_never_sendable_on_its_own() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    assert_eq!(p.get(id).unwrap().state, PostState::Draft);
    assert!(p.due(u64::MAX, true).is_empty(), "drafts never go out by themselves");
}

#[test]
fn approval_shows_you_the_exact_text_you_are_approving() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    let q = p.request_approval(id).unwrap();
    assert!(q.contains("Shipping something new today."));
    assert!(q.contains("X"));
}

#[test]
fn an_approved_post_can_go_and_an_unapproved_one_cannot() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    assert!(matches!(p.check(id, true, None), SendCheck::Hold(_)));
    p.approve(id);
    assert_eq!(p.check(id, true, None), SendCheck::Go);
}

#[test]
fn editing_after_approval_voids_the_approval() {
    // Otherwise "yes, send that" attaches to text you never read.
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.approve(id);
    assert_eq!(p.check(id, true, None), SendCheck::Go);

    p.edit(id, "Completely different text.");
    match p.check(id, true, None) {
        SendCheck::Hold(why) => assert!(why.contains("changed since you approved")),
        o => panic!("{o:?}"),
    }
    assert_eq!(p.get(id).unwrap().state, PostState::Draft);
}

#[test]
fn attaching_media_also_voids_approval() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.approve(id);
    p.attach(id, "photo.png");
    assert!(matches!(p.check(id, true, None), SendCheck::Hold(_)));
}

#[test]
fn approving_one_post_does_not_approve_the_next() {
    let mut p = Publisher::default();
    let a = tweet(&mut p);
    let b = p.draft(Channel::X, "A second, different post.");
    p.approve(a);
    assert_eq!(p.check(a, true, None), SendCheck::Go);
    assert!(matches!(p.check(b, true, None), SendCheck::Hold(_)));
}

#[test]
fn a_scheduled_post_waits_for_its_time() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.schedule(id, 10_000);
    p.approve(id);
    assert_eq!(p.get(id).unwrap().state, PostState::Scheduled);
    assert!(p.due(9_999, true).is_empty());
    assert_eq!(p.due(10_000, true), vec![id]);
}

#[test]
fn time_passing_is_not_consent() {
    // A scheduled post is re-checked at send time, not trusted because it
    // was approved once.
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.schedule(id, 10_000);
    p.approve(id);
    p.edit(id, "sneaky replacement");
    assert!(p.due(999_999, true).is_empty(), "the edit voided it");
}

#[test]
fn a_post_too_long_for_the_platform_is_held_not_truncated() {
    let mut p = Publisher::default();
    let id = p.draft(Channel::X, &"x".repeat(400));
    p.approve(id);
    match p.check(id, true, None) {
        SendCheck::Hold(why) => assert!(why.contains("120 characters too long"), "got: {why}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn platform_limits_differ_and_are_overridable() {
    let mut p = Publisher::default();
    let id = p.draft(Channel::LinkedIn, &"x".repeat(400));
    p.approve(id);
    assert_eq!(p.check(id, true, None), SendCheck::Go, "400 chars is fine on LinkedIn");
    assert!(matches!(p.check(id, true, Some(100)), SendCheck::Hold(_)), "override applies");
}

#[test]
fn a_platform_that_needs_an_image_will_not_post_without_one() {
    let mut p = Publisher::default();
    let id = p.draft(Channel::Instagram, "A caption with no picture.");
    p.approve(id);
    match p.check(id, true, None) {
        SendCheck::Hold(why) => assert!(why.contains("needs an image")),
        o => panic!("{o:?}"),
    }
    let path = std::env::temp_dir().join(format!("atlas-instagram-media-{}-{}.png", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
    p.attach(id, &path.display().to_string());
    assert!(!p.approve(id), "a missing attachment must not acquire consent");
    assert!(matches!(p.check(id, true, None), SendCheck::Hold(_)));
    let image = std::fs::read("tests/fixtures/round11/ocr_invoice.png").unwrap();
    std::fs::write(&path, &image).unwrap();
    assert!(p.approve(id), "approve the actual disposable image bytes");
    let captured = p.get(id).unwrap().verified_media_copies(&|| false).unwrap();
    assert_eq!(captured.paths.len(), 1);
    assert_ne!(std::path::Path::new(&captured.paths[0]), path.as_path());
    assert_eq!(std::fs::read(&captured.paths[0]).unwrap(), image);
    assert_eq!(p.check(id, true, None), SendCheck::Go);
    assert_eq!(std::fs::read(&path).unwrap(), image, "approval must preserve the source image");
    std::fs::remove_file(&path).unwrap();
    assert!(p.get(id).unwrap().verified_media_copies(&|| false).is_err(), "lost media must be refused before provider access");
}

#[test]
fn nothing_goes_out_while_offline_but_it_is_not_lost() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.approve(id);
    assert!(p.due(u64::MAX, false).is_empty());
    assert_eq!(p.get(id).unwrap().state, PostState::ReadyToSend, "still queued");
    assert_eq!(p.due(u64::MAX, true), vec![id]);
}

#[test]
fn problems_you_would_want_to_know_about_are_surfaced() {
    let mut p = Publisher::default();
    let long = p.draft(Channel::X, &"x".repeat(400));
    p.schedule(long, 100);
    p.approve(long);
    let blocked = p.blocked(200, true);
    assert_eq!(blocked.len(), 1);
    assert!(blocked[0].1.contains("too long"));
}

#[test]
fn a_missing_connection_is_not_reported_as_a_problem_with_the_post() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.schedule(id, 100);
    p.approve(id);
    assert!(p.blocked(200, false).is_empty(), "offline is not the post's fault");
}

#[test]
fn a_post_can_be_cancelled_right_up_until_it_goes_but_not_after() {
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.approve(id);
    assert!(p.cancel(id));
    assert!(p.due(u64::MAX, true).is_empty());

    let b = p.draft(Channel::X, "another");
    p.approve(b);
    p.mark_sent(b, "posted", true);
    assert!(!p.cancel(b), "gone is gone");
}

#[test]
fn emails_are_scheduled_the_same_way_as_posts() {
    let mut p = Publisher::default();
    let id = p.draft(
        Channel::Email { to: "sam@example.com".into(), subject: "Thursday".into() },
        "Moving our call to 2pm, does that work?",
    );
    p.schedule(id, 5_000);
    p.approve(id);
    assert_eq!(p.due(5_000, true), vec![id]);
    assert!(p.get(id).unwrap().describe().contains("sam@example.com"));
}

#[test]
fn emails_have_no_character_ceiling() {
    let mut p = Publisher::default();
    let id = p.draft(
        Channel::Email { to: "a@b.com".into(), subject: "Long".into() },
        &"x".repeat(50_000),
    );
    p.approve(id);
    assert_eq!(p.check(id, true, None), SendCheck::Go);
}

#[test]
fn an_empty_post_is_never_sent() {
    let mut p = Publisher::default();
    let id = p.draft(Channel::X, "   ");
    p.approve(id);
    assert!(matches!(p.check(id, true, None), SendCheck::Hold(_)));
}

#[test]
fn you_can_ask_what_is_queued_to_go_out() {
    let mut p = Publisher::default();
    assert!(p.summary(0).contains("Nothing queued"));
    let id = tweet(&mut p);
    p.schedule(id, 3_600);
    p.approve(id);
    p.draft(Channel::LinkedIn, "waiting on you");
    let s = p.summary(0);
    assert!(s.contains("1 scheduled") && s.contains("1 awaiting"), "got: {s}");
    assert!(s.contains("60 minutes"), "got: {s}");
}

#[test]
fn the_queue_survives_a_restart() {
    let d = std::env::temp_dir().join("atlas-publish-test");
    let _ = std::fs::remove_dir_all(&d);
    let store = atlas::store::Store::new(&d);
    let mut p = Publisher::default();
    let id = tweet(&mut p);
    p.schedule(id, 9_000);
    p.approve(id);
    p.save(&store).unwrap();

    let back = Publisher::load(&store);
    assert_eq!(back.due(9_000, true), vec![id], "approval and schedule both survived");
}
