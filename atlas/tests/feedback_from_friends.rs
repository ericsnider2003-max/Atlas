//! Feedback: a friend tells you something's wrong when *they* decide it is,
//! and hears back what you did about it.
//!
//! Eric, 26 Sep: "I don't want my friends' Atlas to tell me. I want a way for
//! my friends to submit feedback to me ... when the friend makes that
//! determination ... Kind of like a feedback loop."

use atlas::feedback::{
    answer_delivered, answer_feedback, answers_out, compose_feedback, feedback_delivered, feedback_inbox, feedback_outbox,
    feedback_preview, feedback_sent, heard_answer, heard_feedback, queue_feedback, Feedback, FeedbackStatus,
    MAX_FEEDBACK_BYTES, MAX_WORDS_CHARS,
};
use atlas::store::Store;
use atlas::update_apply::{self as apply, FailedBecause, FailureReport};
use std::path::PathBuf;

fn fresh(tag: &str) -> (Store, PathBuf) {
    let root = std::env::temp_dir().join(format!("atlas-feedback-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("data/state")).unwrap();
    (Store::new(root.join("data/state")), root)
}

fn failure() -> FailureReport {
    FailureReport {
        version: "9.1.0".into(),
        sha256: "cc".repeat(32),
        replaces: "0.1.0".into(),
        platform: "windows-x86_64".into(),
        stage: "probation".into(),
        reasons: vec!["3 starts in a row never got through".into()],
        crash: "where: src/hub.rs:88\nwhat: called `Option::unwrap()` on a `None` value".into(),
        at: 7,
        ..Default::default()
    }
}

/// The whole loop, both ends, through the functions the doors and the daemon
/// call: a friend writes it and sends it, you get the report, you answer, and
/// they hear it.
#[test]
fn a_friend_sends_feedback_you_get_a_report_and_they_hear_your_answer() {
    let (friend, _) = fresh("loop-friend");
    let (you, _) = fresh("loop-you");

    // The friend writes it, attaching the update failure only because they chose to.
    let f = compose_feedback("The hub goes blank after the update", Some(failure()), 100).unwrap();
    let shown = feedback_preview(&f);
    println!("LIVE [what the friend sees before sending]\n{shown}");
    assert!(shown.contains("The hub goes blank") && shown.contains("src/hub.rs:88"), "they see what goes");
    queue_feedback(&friend, f.clone(), "Eric");
    let out = feedback_outbox(&friend);
    assert_eq!(out.len(), 1);
    let (to, body, id) = &out[0];
    assert_eq!((to.as_str(), id.as_str()), ("Eric", f.id.as_str()));

    // It arrives at your end over the pairing named "Priya".
    let said = heard_feedback(&you, "Priya", body).expect("filed");
    println!("LIVE [what you're told] {said}");
    assert!(said.contains("Priya") && said.contains("The hub goes blank") && said.contains("number 1"), "{said}");
    feedback_delivered(&friend, id);
    assert!(feedback_outbox(&friend).is_empty(), "delivered once");
    assert!(heard_feedback(&you, "Priya", body).is_none(), "the same feedback twice is filed once");
    let inbox = feedback_inbox(&you);
    assert_eq!((inbox.len(), inbox[0].from.as_str(), inbox[0].status.clone()), (1, "Priya", FeedbackStatus::New));
    // The attached failure goes where fix briefs come from -- and on a
    // friend's say-so alone the build is NOT pulled; that's yours to decide.
    let reports = apply::failure_reports(&you);
    assert_eq!((reports.len(), reports[0].from.as_str()), (1, "Priya"));
    assert!(!apply::is_halted(you.root(), &"cc".repeat(32)), "a friend's feedback doesn't stop your releases");
    let brief = apply::fix_brief(&reports, "9.1.0").unwrap();
    assert!(brief.contains("Priya") && brief.contains("src/hub.rs:88"));

    // You answer; the friend hears it.
    let to = answer_feedback(&you, 1, FeedbackStatus::Fixed("9.1.1".into()), "thanks -- the hub now waits for the palette", 200).unwrap();
    assert_eq!(to, "Priya");
    let answers = answers_out(&you);
    assert_eq!(answers.len(), 1);
    let (ato, abody, aid) = &answers[0];
    assert_eq!(ato, "Priya");
    let said = heard_answer(&friend, "Eric", abody).expect("they hear it");
    println!("LIVE [what the friend is told] {said}");
    assert!(said.contains("fixed in 9.1.1") && said.contains("waits for the palette"), "{said}");
    answer_delivered(&you, aid);
    assert!(answers_out(&you).is_empty());
    let mine = feedback_sent(&friend);
    assert_eq!(mine[0].status, FeedbackStatus::Fixed("9.1.1".into()));
    assert_eq!(mine[0].replies.len(), 1);
    assert!(heard_answer(&friend, "Eric", abody).is_none(), "the same answer twice is said once");
}

#[test]
fn an_answer_is_taken_only_from_whoever_the_feedback_went_to_and_only_about_real_feedback() {
    let (friend, _) = fresh("answers");
    let f = compose_feedback("Typing goes wrong in Notepad", None, 1).unwrap();
    queue_feedback(&friend, f.clone(), "Eric");
    let answer = |id: &str| format!(r#"{{"id":"{id}","status":{{"state":"fixed","detail":"9.9.9"}},"note":"all fixed","at":5}}"#);
    assert!(heard_answer(&friend, "Mallory", &answer(&f.id)).is_none(), "someone else can't close your feedback");
    assert!(heard_answer(&friend, "Eric", &answer("not-something-you-sent")).is_none(), "nor invent one");
    assert!(heard_answer(&friend, "Eric", "not json").is_none());
    assert_eq!(feedback_sent(&friend)[0].status, FeedbackStatus::New);
    assert!(heard_answer(&friend, "eric", &answer(&f.id)).is_some(), "your name for them, in any case");
}

#[test]
fn what_arrives_is_checked_and_the_sender_is_the_pairing_not_the_body() {
    let (you, _) = fresh("checked");
    let mut f = compose_feedback("It crashed", None, 1).unwrap();
    f.from = "Someone Important".into();
    f.status = FeedbackStatus::Fixed("1.0".into());
    let body = serde_json::to_string(&f).unwrap();
    heard_feedback(&you, "Priya", &body).unwrap();
    let got = &feedback_inbox(&you)[0];
    assert_eq!(got.from, "Priya", "who sent it is the pairing's name");
    assert_eq!(got.status, FeedbackStatus::New, "a sender can't mark their own feedback fixed");
    assert!(heard_feedback(&you, "Priya", "{}").is_none(), "no words, no feedback");
    assert!(heard_feedback(&you, "Priya", &"x".repeat(MAX_FEEDBACK_BYTES + 1)).is_none());
    let long = Feedback { id: "big".into(), words: "w".repeat(MAX_WORDS_CHARS + 1), ..Default::default() };
    assert!(heard_feedback(&you, "Priya", &serde_json::to_string(&long).unwrap()).is_none());
    assert!(compose_feedback("   ", None, 1).is_err(), "nothing written, nothing to send");
    // A friend's attached failure is re-sorted on your side.
    let mut a = failure();
    a.because = FailedBecause::ThisMachine;
    let with = Feedback { id: "f2".into(), words: "update failed".into(), attached: Some(a), ..Default::default() };
    heard_feedback(&you, "Priya", &serde_json::to_string(&with).unwrap()).unwrap();
    assert_eq!(apply::failure_reports(&you)[0].because, FailedBecause::TheBuild, "a crash is the build's, whatever the sender said");
}

#[test]
fn you_decide_to_hold_a_build_after_reading_the_feedback() {
    let (you, _) = fresh("hold");
    let bytes = b"a build friends said was broken".to_vec();
    let sha = atlas::update_courier::keep_for_friends(you.root(), &bytes).unwrap();
    assert!(atlas::update_courier::chunk(you.root(), &sha, 0).is_some());
    apply::hold_release(&you, &sha);
    assert!(apply::is_halted(you.root(), &sha));
    assert!(atlas::update_courier::chunk(you.root(), &sha, 0).is_none(), "held: not handed out");
}

#[test]
fn the_feedback_doors_take_it_from_a_known_peer_and_nothing_else() {
    let door = std::sync::Mutex::new(atlas::kin::Door::new(vec![atlas::kin::Peer::new("Priya", "priya-token")]));
    let post = |path: &str, body: String| atlas::server::Request {
        method: "POST".into(),
        path: path.into(),
        query: String::new(),
        token: None,
        token_from_url: false,
        body,
    };
    let body = serde_json::json!({ "body": "{\"id\":\"f1\",\"words\":\"broken\"}" }).to_string();
    match atlas::server::route_feedback(&post("/feedback", body.clone()), &door, "priya-token") {
        Some(atlas::server::Action::PeerFeedback(f)) => assert_eq!(f.from, "Priya", "from the token"),
        other => panic!("{other:?}"),
    }
    match atlas::server::route_feedback(&post("/feedback-answer", body.clone()), &door, "priya-token") {
        Some(atlas::server::Action::PeerFeedbackAnswer(f)) => assert_eq!(f.from, "Priya"),
        other => panic!("{other:?}"),
    }
    assert!(atlas::server::route_feedback(&post("/feedback", body.clone()), &door, "stranger").is_none());
    assert!(atlas::server::route_feedback(&post("/chat", body), &door, "priya-token").is_none(), "wrong door");
    let huge = serde_json::json!({ "body": "x".repeat(MAX_FEEDBACK_BYTES + 1) }).to_string();
    assert!(atlas::server::route_feedback(&post("/feedback", huge), &door, "priya-token").is_none());
}

/// The real program on your side: `atlas feedback` lists what came in, and
/// `atlas feedback reply` queues your answer back.
#[test]
#[cfg(unix)]
fn the_real_feedback_command_lists_and_answers() {
    let (you, root) = fresh("cli");
    let f = compose_feedback("Settings page won't save", Some(failure()), 3).unwrap();
    heard_feedback(&you, "Priya", &serde_json::to_string(&f).unwrap()).unwrap();
    let run = |args: &[&str], input: &str| {
        use std::io::Write;
        let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_atlas"))
            .args(args)
            .env("ATLAS_HOME", &root)
            .env("ATLAS_UPDATE_PROBE", "1")
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        c.stdin.take().unwrap().write_all(input.as_bytes()).unwrap();
        String::from_utf8_lossy(&c.wait_with_output().unwrap().stdout).to_string()
    };
    let out = run(&["feedback"], "");
    println!("LIVE [atlas feedback] {out}");
    assert!(out.contains("1. Priya") && out.contains("Settings page won't save") && out.contains("[update failure attached]"), "{out}");
    let out = run(&["feedback", "reply", "1", "fixed", "9.1.1", "sorted", "in", "the", "next", "one"], "");
    println!("LIVE [atlas feedback reply] {out}");
    assert!(out.contains("Marked fixed in 9.1.1") && out.contains("Priya"), "{out}");
    let queued = answers_out(&you);
    assert_eq!(queued.len(), 1);
    assert!(queued[0].1.contains("sorted in the next one"));
    assert_eq!(feedback_inbox(&you)[0].status, FeedbackStatus::Fixed("9.1.1".into()));
    // With no release channel, there's no one to send to -- said, nothing queued.
    let out = run(&["feedback", "send"], "it broke\nyes\n");
    assert!(out.contains("isn't in anyone's release channel"), "{out}");
    assert!(feedback_outbox(&you).is_empty());
}
