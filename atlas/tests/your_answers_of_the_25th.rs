//! Eric's answers, 25 Sep 2026:
//!
//! - "Log in the backup." The activity log is checked against the heads kept
//!   in every backup, not only the file beside it — so a log and anchor file
//!   rewritten together on the laptop still disagree with the backups.
//! - "If the prompts add value yes." The words of a model call are kept only
//!   when the call was graded, only for kinds whose grade comes from
//!   something that happened, scrubbed, behind a switch that ships on.
//! - "Envelope: no one else can ask Atlas." Not a guest profile, not a
//!   handover, not a voice that isn't yours or might not be.

use atlas::activity::{Journal, Kind, Sealed};
use atlas::brain::Llm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-answers25-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

// ---------------------------------------------------------------- log in the backup

fn backup_cfg(dir: &Path) -> atlas::safety::BackupConfig {
    serde_yaml::from_str(&format!("dir: '{}'\n", dir.display())).unwrap()
}

#[test]
fn a_log_and_its_anchors_rewritten_together_still_disagree_with_the_backup() {
    let root = scratch("forged");
    let state = root.join("state");
    std::fs::create_dir_all(&state).unwrap();
    let store = Store::new(state.clone());
    let mut j = Journal::default();
    for i in 0..5 {
        j.record_at(Kind::Published, &format!("sent reply {i}"), true, 1_000 + i);
    }
    j.save(&store).unwrap();
    let cfg = backup_cfg(&root.join("backups"));
    atlas::safety::back_up(&state, &cfg, 2_000).expect("backup");
    assert!(root.join("backups/state-2000/activity-anchors.jsonl").is_file(), "the heads weren't in the backup");

    // Untouched: intact, and checked against the backup too.
    let (sealed, from) = atlas::activity::check_with_backups(&j, &state, &cfg);
    assert!(matches!(sealed, Sealed::Intact { sealed: 5, .. }), "{sealed:?}");
    assert_eq!(from, 1);
    assert!(atlas::activity::said_with_backups(&sealed, from).contains("in 1 backup"));

    // Rewritten on the laptop, seals and anchors and all: entry 3 now says
    // something it didn't.
    let mut forged = Journal::default();
    for i in 0..5 {
        let what = if i == 3 { "nothing was sent".to_string() } else { format!("sent reply {i}") };
        forged.record_at(Kind::Published, &what, true, 1_000 + i);
    }
    std::fs::remove_file(atlas::activity::anchor_path(&state)).unwrap();
    forged.save(&store).unwrap();
    let local_only = forged.check(&atlas::activity::anchors(&state));
    assert!(matches!(local_only, Sealed::Intact { .. }), "the laptop's own copy can't see it: that's the point");
    let (sealed, _) = atlas::activity::check_with_backups(&forged, &state, &cfg);
    assert!(matches!(sealed, Sealed::Broken { .. }), "the backup's heads didn't catch the rewrite: {sealed:?}");
}

#[test]
fn a_log_numbered_to_skip_past_what_was_recorded_is_caught() {
    let mut j = Journal::default();
    for i in 0..5 {
        j.record_at(Kind::Published, &format!("e{i}"), true, i);
    }
    let heads = vec![(4u64, j.events[4].hash.clone())];
    // Replaced with a log that starts after the recorded head.
    // Set field by field: since the merge (26 Sep) the journal also carries a
    // private Merkle seal, so it can't be built with `..Default::default()`
    // from outside the crate.
    let mut skipped = Journal::default();
    skipped.next_seq = 50;
    skipped.record_at(Kind::Published, "fresh", true, 10);
    assert!(matches!(skipped.check(&heads), Sealed::Broken { .. }), "{:?}", skipped.check(&heads));
    // Genuinely rolled off the front, 400 later: fine.
    let mut long = j.clone();
    for i in 0..420 {
        long.record_at(Kind::Published, &format!("later {i}"), true, 100 + i);
    }
    assert!(matches!(long.check(&heads), Sealed::Intact { .. }), "{:?}", long.check(&heads));
}

#[test]
fn an_empty_log_says_so_rather_than_intact_with_no_seal() {
    let s = atlas::activity::said_with_backups(&Journal::default().check(&[]), 0);
    assert_eq!(s, "The activity log has nothing sealed in it yet, so there's nothing to check.");
}

// ---------------------------------------------------------------- graded examples

#[test]
fn personal_details_are_taken_out_by_their_shape() {
    use atlas::trace::scrub;
    assert_eq!(scrub("mail sam@example.com today"), "mail [email] today");
    assert_eq!(scrub("call me on 555 123 4567 after 3"), "call me on [number] after 3");
    assert_eq!(scrub("card 4111-1111-1111-1111 ok"), "card [number] ok");
    assert_eq!(scrub("my password is hunter2"), "my password is [secret]");
    assert_eq!(scrub("see https://x.test/a?token=abc"), "see https://x.test/a?[…]");
    assert_eq!(scrub("key sk_live_51Habcdefghijklmnop1234"), "key [key]");
    // What isn't personal stays: times, prices, years, short numbers.
    let plain = "Friday at 3:30 costs $1,200 in 2026, room 12";
    assert_eq!(scrub(plain), plain);
    assert_eq!(scrub("two\nlines"), "two\nlines");
}

#[test]
fn only_graded_calls_are_kept_and_the_oldest_go_first() {
    let dir = scratch("examples");
    let log = atlas::trace::log_path(&dir);
    let mut t = atlas::trace::Trace::default();
    let words = atlas::trace::Words { system: "s".into(), user: "reach me at a@b.co".into(), reply: "ok".into() };
    let mut c = atlas::trace::Call::new("council", "m", 5).finished(1, "p", "r");
    c.id = t.next_id();
    t.record(c);
    assert!(atlas::trace::example_of(&t, 1, &words).is_none(), "an ungraded call was kept");
    t.grade(1, false, Some("wouldn't commit"));
    let ex = atlas::trace::example_of(&t, 1, &words).unwrap();
    assert_eq!(ex.user, "reach me at [email]");
    assert_eq!(ex.why.as_deref(), Some("wouldn't commit"));
    for _ in 0..5 {
        assert!(atlas::trace::keep_example(&log, &ex, 3));
    }
    assert_eq!(atlas::trace::examples(&log).len(), 3);
    // The call log itself still has no words.
    assert!(atlas::trace::append(&log, &t.calls[0]));
    let raw = std::fs::read_to_string(&log).unwrap();
    assert!(!raw.contains("reach me"), "{raw}");
}

/// Writes like a chatbot, then like a person.
struct Chatty;
impl Llm for Chatty {
    fn complete(&self, system: &str, _user: &str) -> Result<String> {
        if system.starts_with("You are tightening") {
            Ok("Friday at 3 works. See you then.".into())
        } else {
            Ok("Certainly! Friday at 3 works. I hope this helps!".into())
        }
    }
}

fn settle(d: &mut Daemon, p: &MockPlatform) {
    let mut t = atlas::store::now() + 50;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline
        && !p.actions().iter().any(|a| matches!(a, atlas::platform::mock::Action::Type(_)))
    {
        let _ = d.tick(t);
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn window_reply(cfg: Config, tag: &str) -> (PathBuf, Vec<atlas::trace::Example>) {
    let cfg: &'static Config = Box::leak(Box::new(cfg));
    let p = plat();
    let root = scratch(tag);
    let mut d = Daemon::new(cfg, &p, Some(Arc::new(Chatty)), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    p.focus_on("SomeChat.exe", "");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(7));
    p.set_window_text(7, "Sam (sam@example.com): does Friday at 3 work?");
    let _ = d.execute_timed(&Intent::Delegate("draft a reply to this".into()), "draft a reply to this");
    settle(&mut d, &p);
    let log = atlas::trace::log_path(root.as_path());
    (root, atlas::trace::examples(&log))
}

#[test]
fn a_rewritten_window_reply_is_kept_as_a_bad_and_a_good_example() {
    let (_, kept) = window_reply(Config::load(Path::new("config")).unwrap(), "reply-kept");
    let replies: Vec<_> = kept.iter().filter(|e| e.asked_by == "conversation-reply").collect();
    assert_eq!(replies.len(), 2, "{kept:?}");
    assert!(!replies[0].good && replies[0].reply.contains("Certainly!"), "{:?}", replies[0]);
    assert!(replies[1].good && replies[1].reply.contains("See you then"), "{:?}", replies[1]);
    assert!(kept.iter().all(|e| !e.user.contains("sam@example.com")), "an email address was kept");
}

#[test]
fn off_means_no_words_are_kept() {
    let mut cfg = Config::load(Path::new("config")).unwrap();
    assert!(cfg.tools.as_ref().unwrap().trace.keep_examples, "Eric said yes; it ships on");
    assert!(atlas::settings::registry(cfg.tools.as_ref().unwrap()).get("trace.keep_examples").is_some());
    cfg.tools.as_mut().unwrap().trace.keep_examples = false;
    let (root, kept) = window_reply(cfg, "reply-off");
    assert!(kept.is_empty(), "{kept:?}");
    assert!(!atlas::trace::examples_path(&atlas::trace::log_path(&root)).exists());
}

/// A council seat that won't commit.
struct Undecided(Mutex<u32>);
impl Llm for Undecided {
    fn complete(&self, _s: &str, _u: &str) -> Result<String> {
        *self.0.lock().unwrap() += 1;
        Ok("It depends on the weather.".into())
    }
}

#[test]
fn a_seat_that_wouldnt_commit_is_kept_with_why() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let root = scratch("seats");
    let mut d = Daemon::new(cfg, &p, Some(Arc::new(Undecided(Mutex::new(0)))), Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));
    let _ = d.ask_the_room("should we move the team offsite to Lisbon?");
    let log = atlas::trace::log_path(&root);
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut t = atlas::store::now() + 50;
    while Instant::now() < deadline && atlas::trace::examples(&log).is_empty() {
        let _ = d.tick(t);
        t += 1;
        std::thread::sleep(Duration::from_millis(10));
    }
    let kept = atlas::trace::examples(&log);
    assert!(!kept.is_empty());
    assert!(kept.iter().all(|e| e.asked_by == "council" && !e.good && e.why.as_deref() == Some("wouldn't commit")), "{kept:?}");
    assert!(kept[0].reply.contains("It depends"));
}

// ---------------------------------------------------------------- the envelope is yours alone

#[test]
fn a_guest_profile_cannot_ask_about_the_envelope() {
    use atlas::profiles::Role;
    assert!(Role::Owner.may("after_me"));
    assert!(!Role::Guest.may("after_me"), "a guest can ask where the envelope is");
    assert_eq!(atlas::profiles::ONLY_YOU_MAY_ASK, &["after_me"]);
    // Handing Atlas over refuses it too.
    assert!(atlas::handover::refuses("after_me"));
}

#[test]
fn a_voice_that_isnt_yours_or_might_not_be_is_told_no() {
    let cfg: &'static Config = Box::leak(Box::new(Config::load(Path::new("config")).unwrap()));
    let p = plat();
    let mut d = Daemon::new(cfg, &p, None, Store::new(scratch("envelope-voice")), Proactive::new(ProactiveConfig::default()));
    d.last_verdict = atlas::voiceid::Verdict::NotYou(0.2);
    let said = d.execute_timed(&Intent::AfterMe, "where's my envelope");
    assert_eq!(said, "That's only for the person this Atlas belongs to.");
    d.last_verdict = atlas::voiceid::Verdict::Unsure(0.6);
    let said = d.execute_timed(&Intent::AfterMe, "where's my envelope");
    assert!(said.contains("couldn't be sure that was your voice"), "{said}");
    // You, or typed with no voice to judge: answered.
    d.last_verdict = atlas::voiceid::Verdict::You(0.9);
    // Nothing is arranged in a fresh store, so the answer says that.
    let said = d.execute_timed(&Intent::AfterMe, "where's my envelope");
    assert!(said.contains("there's no envelope") && !said.contains("only for"), "{said}");
    d.last_verdict = atlas::voiceid::Verdict::NotEnrolled;
    let typed = d.execute_timed(&Intent::AfterMe, "where's my envelope");
    assert!(typed.contains("there's no envelope") && !typed.contains("only for"), "{typed}");
}

// ---------------------------------------------------------------- found on the laptop

/// On Eric's laptop, 25 Sep 2026, typing into Windows 11 Notepad in one
/// burst came out as "Atlas ................": the first few characters,
/// then the last one over and over. Typing is now paced, and a reply is read
/// back before Enter; one that didn't land as written is never sent.
#[test]
fn typing_that_comes_out_wrong_is_never_sent() {
    use atlas::platform::mock::Action;
    let p = plat();
    p.focus_on("SomeChat.exe", "");
    *p.front.borrow_mut() = Some(atlas::platform::WindowId(7));
    p.set_window_text(7, "Sam: Friday?");
    *p.garbles.borrow_mut() = true;
    let r = atlas::delegate::type_into_window(&p, atlas::platform::WindowId(7), "Friday works.", true);
    assert!(r.unwrap_err().contains("didn't send"));
    assert!(!p.actions().iter().any(|a| matches!(a, Action::Press(k) if k == "enter")), "a garbled reply was sent");
    // Landing as written, it goes.
    *p.garbles.borrow_mut() = false;
    p.set_window_text(7, "Sam: Friday?");
    assert!(atlas::delegate::type_into_window(&p, atlas::platform::WindowId(7), "Friday works.", true).is_ok());
    assert!(p.actions().iter().any(|a| matches!(a, Action::Press(k) if k == "enter")));
}
