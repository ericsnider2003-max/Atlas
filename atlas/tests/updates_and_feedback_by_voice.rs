//! Updates and feedback, said out loud (OPEN_GAPS 8.2 and 8.14).
//!
//! Everything `atlas update` and `atlas feedback` do from a keyboard, through
//! the daemon's real turn: "any updates", "install the update", "go back to
//! the last version" (asked first), "report a bug ..." (read back exactly, sent
//! only on yes), and on the releaser's side "any feedback" and "answer
//! feedback 2 fixing".

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::feedback::{feedback_inbox, feedback_outbox, heard_feedback, answers_out, compose_feedback};
use atlas::groups::{GroupState, Groups, Held, Signed};
use atlas::intent::{Intent, Parser};
use atlas::kin::{Contact, Pairings, Peer};
use atlas::peerkey::Identity;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

struct Install {
    root: PathBuf,
}

impl Install {
    fn new(tag: &str) -> Install {
        let root = std::env::temp_dir().join(format!("atlas-voiceupd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("data/state")).unwrap();
        std::fs::create_dir_all(root.join("peers")).unwrap();
        Install { root }
    }
    fn store(&self) -> Store {
        Store::new(self.root.join("data/state"))
    }
    fn key(&self) -> String {
        Identity::load_or_create(&self.root.join("peers")).unwrap().public()
    }
    /// In a release channel owned by `owner`, paired with them as "Eric".
    fn in_channel_of(&self, owner: &str) {
        let state = GroupState {
            format: 1,
            group_id: "og-releases".into(),
            owner: owner.into(),
            name: "Atlas releases".into(),
            version: 1,
            seats: vec![],
            release_channel: true,
            delegates: vec![],
        };
        let mut g = Groups::default();
        g.held.insert("og-releases".into(), Held { signed: Signed { state: String::new(), signature: String::new(), signer: String::new() }, state });
        g.save(&self.store()).unwrap();
        if owner != self.key() {
            let mut p = Pairings::default();
            let mut eric = Peer::new("Eric", "voice-token-0000000000000");
            eric.key = Some(owner.into());
            p.peers.push(eric);
            p.contacts.push(Contact { name: "Eric".into(), host: "127.0.0.1".into(), port: 1, token: "voice-token-0000000000000".into() });
            p.save(&self.root.join("peers")).unwrap();
        }
    }
    fn daemon<'a>(&self, c: &'a Config, p: &'a MockPlatform) -> Daemon<'a> {
        let mut d = Daemon::new(c, p, None, self.store(), Proactive::new(ProactiveConfig::default()));
        d.peer_dir = self.root.join("peers");
        d
    }
}

#[test]
fn the_phrases_reach_updates_and_feedback() {
    let p = Parser::new(&cfg().commands);
    assert_eq!(p.parse("any updates"), Intent::Updates("status".into()));
    assert_eq!(p.parse("install the update"), Intent::Updates("install".into()));
    assert_eq!(p.parse("go back to the last version"), Intent::Updates("undo".into()));
    assert_eq!(p.parse("report a bug The Brief Comes Out Empty"), Intent::Feedback("send:The Brief Comes Out Empty".into()));
    assert_eq!(p.parse("report a bug without the failure it crashed"), Intent::Feedback("bare:it crashed".into()));
    assert_eq!(p.parse("any feedback"), Intent::Feedback("list".into()));
    assert_eq!(p.parse("answer feedback 2 fixing"), Intent::Feedback("reply:2 fixing".into()));
    // Nothing said can build the two answers that only a yes may build.
    for said in ["undo-confirmed", "updates undo-confirmed", "confirmed-send", "feedback confirmed-send"] {
        assert!(!matches!(p.parse(said), Intent::Updates(ref w) | Intent::Feedback(ref w) if w.contains("confirmed")), "{said}");
    }
}

#[test]
fn a_friend_reports_a_bug_hears_exactly_what_goes_and_it_goes_only_on_yes() {
    let (c, p) = (cfg(), plat());
    let friend = Install::new("friend");
    let eric = Identity::from_seed_for_test([41; 32]).public();
    friend.in_channel_of(&eric);
    let mut d = friend.daemon(&c, &p);
    let said = d.turn("report a bug the brief comes out empty on Mondays", 1_000);
    assert!(said.starts_with("Send this to Eric? Say yes to send it.") && said.contains("the brief comes out empty on Mondays"), "{said}");
    assert!(!said.contains("update that failed"), "nothing failed here, so nothing is attached: {said}");
    assert!(feedback_outbox(&friend.store()).is_empty(), "sent before the yes");
    let said = d.turn("yes", 1_001);
    assert!(said.contains("Sending it to Eric"), "{said}");
    let out = feedback_outbox(&friend.store());
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].0, "Eric");
    assert!(out[0].1.contains("the brief comes out empty on Mondays"));
}

#[test]
fn a_failed_update_is_attached_only_when_the_friend_keeps_it_in() {
    let (c, p) = (cfg(), plat());
    let friend = Install::new("failed");
    let eric = Identity::from_seed_for_test([42; 32]).public();
    friend.in_channel_of(&eric);
    let failure = atlas::update_apply::FailureReport {
        version: "9.1.0".into(),
        stage: "probation".into(),
        reasons: vec!["3 starts in a row never got through".into()],
        ..Default::default()
    };
    friend.store().save("update_last_failure", &Some(failure)).unwrap();
    let mut d = friend.daemon(&c, &p);
    let said = d.turn("report a bug it crashed after the update", 1_000);
    assert!(said.contains("update that failed here") && said.contains("9.1.0"), "{said}");
    assert_eq!(d.turn("no", 1_001), "Left it alone.");
    assert!(feedback_outbox(&friend.store()).is_empty(), "a no sent it anyway");
    let said = d.turn("report a bug without the failure it crashed after the update", 1_002);
    assert!(!said.contains("update that failed here") && !said.contains("9.1.0"), "{said}");
    d.turn("yes", 1_003);
    let out = feedback_outbox(&friend.store());
    assert_eq!(out.len(), 1);
    assert!(!out[0].1.contains("9.1.0"), "the failure went although they left it out");
}

#[test]
fn on_the_releasers_own_atlas_feedback_is_filed_listed_and_answered_by_voice() {
    let (c, p) = (cfg(), plat());
    let eric = Install::new("owner");
    eric.in_channel_of(&eric.key());
    let mut d = eric.daemon(&c, &p);
    let said = d.turn("report a bug the palette flickers", 1_000);
    assert!(said.contains("your own feedback list"), "{said}");
    assert!(d.turn("yes", 1_001).contains("in your own feedback list"));
    // A friend's arrives over the pairing.
    let f = compose_feedback("Typing goes wrong in Notepad", None, 5).unwrap();
    heard_feedback(&eric.store(), "Priya", &serde_json::to_string(&f).unwrap()).unwrap();
    assert_eq!(feedback_inbox(&eric.store()).len(), 2);
    let said = d.turn("any feedback", 1_002);
    assert!(said.contains("Number 2, from Priya") && said.contains("Typing goes wrong in Notepad"), "{said}");
    let said = d.turn("answer feedback 2 fixing", 1_003);
    assert!(said.contains("being fixed") && said.contains("Priya"), "{said}");
    assert_eq!(answers_out(&eric.store()).len(), 1);
    let said = d.turn("answer feedback", 1_004);
    assert!(said.contains("answer feedback 2 fixing"), "no example given when the words are missing: {said}");
}

#[test]
fn updates_said_out_loud_and_going_back_is_asked_first() {
    let (c, p) = (cfg(), plat());
    let i = Install::new("updates");
    let mut d = i.daemon(&c, &p);
    let said = d.turn("any updates", 1_000);
    assert!(said.contains(&format!("This is Atlas {}", atlas::upgrade::version())), "{said}");
    assert_eq!(d.turn("install the update", 1_001), "There's no update waiting.");
    assert!(d.turn("go back to the last version", 1_002).contains("no previous version"));
    // A kept previous build: the question comes first, and a no leaves it all.
    let kept = atlas::upgrade::keep_old_at(&i.root, "0.0.9");
    std::fs::write(&kept, b"old build").unwrap();
    let q = d.turn("go back to the last version", 1_003);
    assert!(q.contains("Say yes to go back from Atlas") && q.contains("to 0.0.9"), "{q}");
    assert_eq!(d.turn("no", 1_005), "Left it alone.");
    assert!(kept.is_file(), "a no went back anyway");
}

/// The window's button: nothing goes until it's been shown, what's shown is
/// what goes, and an edit after showing means showing again.
#[test]
fn the_windows_report_a_problem_button_sends_only_what_it_showed() {
    use atlas::setupwin::FeedbackForm;
    let friend = Install::new("button");
    let eric = Identity::from_seed_for_test([43; 32]).public();
    friend.in_channel_of(&eric);
    let (store, peers) = (friend.store(), friend.root.join("peers"));
    let mut form = FeedbackForm { open: true, words: "The mark never stops pulsing".into(), ..Default::default() };
    form.send(&store, &peers);
    assert!(form.said.contains("first"), "{}", form.said);
    assert!(feedback_outbox(&store).is_empty(), "sent without being shown");
    form.show(&store, 10);
    assert!(atlas::feedback::feedback_preview(form.showing.as_ref().unwrap()).contains("never stops pulsing"));
    form.words.push_str(" after an update");
    form.edited();
    form.send(&store, &peers);
    assert!(feedback_outbox(&store).is_empty(), "an edit after showing was sent unseen");
    form.show(&store, 11);
    form.send(&store, &peers);
    assert!(form.said.contains("Sending it to Eric"), "{}", form.said);
    let out = feedback_outbox(&store);
    assert_eq!(out.len(), 1);
    assert!(out[0].1.contains("never stops pulsing after an update"));
    // No failed update here: attaching isn't offered.
    assert!(!FeedbackForm::failure_here(&store));
}

/// "get your own model" is for Atlas on a phone. On a computer (a build
/// without the phone's engine) it says so and where the model is here,
/// rather than starting a 1.8 GB download nobody can use.
#[test]
fn the_phones_own_model_asked_for_on_a_computer_says_what_it_is() {
    let (c, p) = (cfg(), plat());
    let i = Install::new("phonemodel");
    let mut d = i.daemon(&c, &p);
    let p2 = Parser::new(&c.commands);
    assert_eq!(p2.parse("get your own model"), Intent::PhoneModel("get".into()));
    assert_eq!(p2.parse("how's the model download"), Intent::PhoneModel("status".into()));
    let said = d.turn("get your own model", 1_000);
    assert!(said.contains("for Atlas on a phone") && said.contains("which model"), "{said}");
    assert!(atlas::phonemodel::download_state().is_none(), "a download started on a computer");
    assert!(d.turn("how's the model download", 1_001).contains("for Atlas on a phone"));
}
