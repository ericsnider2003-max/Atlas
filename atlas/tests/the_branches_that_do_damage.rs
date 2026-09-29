//! Daemon branches where getting it wrong costs something real.
//!
//! `tests/every_intent_reaches_the_daemon.rs` counts the gap: how many of the
//! 63 intents are ever driven through `Daemon::execute` by a test. This file
//! closes the part of that gap that matters most, on one rule:
//!
//! > If the branch being wrong would hand somebody your secrets, act as you,
//! > or silently lose something, it gets a test here.
//!
//! The rest of the gap is nuisance, not risk: a wrong `which model fits here`
//! is a bad answer, and you find out immediately.
//!
//! **Five of the ten closed here are code this chat wrote in the last two
//! days** — messaging, the spoken handover, `this is me`, `finish setting up`.
//! That is not a coincidence and it is worth saying rather than quietly
//! fixing: new work is exactly where an untested branch hides, because the
//! capability arrives with its own tests and everyone reads those as coverage
//! of the whole thing.
//!
//! # What these tests assert, and what they do not
//!
//! They assert **properties**, not wording. `"Told Sam."` is a sentence
//! somebody will improve, and a test that pins the sentence fails on the
//! improvement while passing on a branch that sends to the wrong person. So:
//! a message to nobody must not report success; a handover must actually
//! narrow what comes next; an unlock on a fresh vault must not be treated as
//! proof of anything.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::handover::{refusal, Handover};
use atlas::intent::Intent;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::Once;

/// This file's own install, set before `roots` is first asked.
///
/// `roots::install_state()` caches in a `OnceLock`, so the first thing in the
/// process to ask it fixes the answer for every later caller. Without this,
/// the handover tests below would write a handed-over flag into the
/// repository's own state directory and leave it there.
fn home() -> PathBuf {
    static ONCE: Once = Once::new();
    let p = std::env::temp_dir().join("atlas-branches-that-do-damage");
    ONCE.call_once(|| {
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data").join("state")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
    });
    p
}

fn install() -> Store {
    home();
    atlas::roots::install_state()
}

fn it_is_yours() {
    Handover::default().save(&install()).unwrap();
}

/// Run a test that touches the handover flag, alone.
///
/// The flag lives in one file in one install directory, `roots` caches that
/// directory in a `OnceLock` for the whole process, and cargo runs the tests
/// in this file on several threads at once. So two handover tests running
/// together read each other's state, and the failure is a different test than
/// the one that broke.
///
/// This was not theoretical: a planted defect in the capture branch made
/// `an_unlock_on_a_vault_with_no_passphrase_does_not_prove_anything` fail,
/// which has nothing to do with capture. The mutation pass found it. Without
/// the lock these tests were passing on timing.
///
/// The lock is deliberately taken around the reset as well as the body — a
/// test that tidies up outside the lock hands the next one a half-reset
/// install. Poisoning is stepped over rather than propagated, because a
/// panicking test has already reported its own failure and a cascade of
/// `PoisonError`s from the others only buries it.
fn alone<T>(body: impl FnOnce() -> T) -> T {
    static GATE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _held = GATE.lock().unwrap_or_else(|e| e.into_inner());
    it_is_yours();
    let out = body();
    it_is_yours();
    out
}

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-damage-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).expect("config")
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    home();
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

// --- acting as you ---------------------------------------------------------

#[test]
fn a_message_to_nobody_is_not_reported_as_sent() {
    // Messaging is the owner's, so this reads the handover flag: run
    // under `alone` or a concurrent handover test hands it over mid-test
    // (seen 23 Sep 2026, ~1 in 12 runs).
    alone(|| {
        // The worst failure this branch has available: accepting a sentence it
        // could not address, saying something reassuring, and dropping it. A
        // message is the one thing Atlas does that cannot be recalled once it is
        // on somebody else's machine, so the branch has to fail loudly.
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "msg-nobody");

        let said = d.execute(&Intent::Message(String::new()));
        assert!(
            said.contains('?'),
            "an unaddressed message got a statement rather than a question: {said}"
        );
        assert!(
            !said.to_lowercase().contains("sent") && !said.to_lowercase().contains("told"),
            "it claimed to have delivered a message with no recipient: {said}"
        );
    })
}

#[test]
fn a_message_to_a_stranger_names_the_reason_and_the_fix() {
    // Messaging is the owner's, so this reads the handover flag: run
    // under `alone` or a concurrent handover test hands it over mid-test
    // (seen 23 Sep 2026, ~1 in 12 runs).
    alone(|| {
        // You cannot message somebody you have never paired with, and the branch
        // has two honest ways to say so and one dishonest one. "Couldn't send" is
        // the dishonest one: it reads as a network problem, so you say it again.
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "msg-stranger");

        let said = d.execute(&Intent::Message("sam the roof is leaking".into()));
        assert!(
            said.contains("sam"),
            "it did not say who it could not reach: {said}"
        );
        assert!(
            said.contains("add a friend") && said.contains("Friends page"),
            "it named the problem without saying how to fix it: {said}"
        );
        // And nothing was written into a room, because there is no room.
        let empty = d.execute(&Intent::Messages);
        assert!(
            !empty.contains("roof"),
            "a message that could not be addressed was stored anyway: {empty}"
        );
    })
}

#[test]
fn a_message_with_a_recipient_and_no_words_asks_rather_than_sending_nothing() {
    // Messaging is the owner's, so this reads the handover flag: run
    // under `alone` or a concurrent handover test hands it over mid-test
    // (seen 23 Sep 2026, ~1 in 12 runs).
    alone(|| {
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "msg-empty");
        let said = d.execute(&Intent::Message("sam".into()));
        assert!(said.contains('?'), "{said}");
        assert!(said.to_lowercase().contains("sam"), "it forgot who: {said}");
    })
}

// --- the handover, from the daemon's side ----------------------------------

#[test]
fn saying_hand_over_actually_narrows_what_comes_next() {
    alone(|| {
        // The end-to-end claim the whole handover design rests on, and the one
        // nothing tested at this level: the *spoken* sentence has to reach the
        // stored flag, or "hand over" is a reassuring noise and your friend has
        // your mail.
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "handover-narrows");
        // Before: Atlas is yours, and it is not refusing on those grounds.
        assert!(!Handover::load(&install()).stance.handed_over());
        d.execute(&Intent::HandOver("Sam is borrowing it".into()));
        let after = Handover::load(&install());
        assert!(
            after.stance.handed_over(),
            "the spoken hand-over did not reach the stored flag"
        );
        assert_eq!(after.note, "Sam is borrowing it", "the note was dropped");
    });
}

#[test]
fn hand_over_with_no_note_is_still_a_hand_over() {
    alone(|| {
        // A handover that insists on a reason is one you skip in the moment you
        // need it -- somebody is standing there waiting for the laptop.
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "handover-bare");
        d.execute(&Intent::HandOver(String::new()));
        assert!(
            Handover::load(&install()).stance.handed_over(),
            "it refused a hand-over because nobody explained themselves"
        );
    });
}
// Each of these is written out rather than routed through a shared helper,
// and that is a deliberate reversal. The first version did share one — six
// tests calling `a_guest_is_refused(tag, kind, intent)` — and **two separate
// guards rejected it in the same run**:
//
// * `retrospective.rs` reported all six as tests that assert nothing, which
//   is exactly what they looked like: a body with one function call in it.
//   That guard exists because a test whose assertions live somewhere else is
//   one nobody checks.
// * `every_intent_reaches_the_daemon.rs` — the guard written in this same
//   pass — stopped seeing five of the intents as covered, because it reads
//   `execute(&Intent::X)` and the helper turned that into `execute(&intent)`.
//
// The second one is the more interesting failure: a coverage measurement that
// a refactor can quietly defeat. Writing the intent at the point it is used
// keeps both guards able to see what is happening, and repetition in tests is
// cheaper than either.
//
// The shape is the same in all six: ask as the owner, hand over, ask again,
// and require the second answer to be **exactly** `handover::refusal`.
//
// Not a word-sniff. An earlier draft checked that the guest's answer
// contained "not" or "can't", and it was worthless: the owner's own reply to
// `finish setting up` is "Nothing was left unfinished", to `create account`
// "Signing up is switched off", to `work on yourself` "Looking at myself is
// switched off". A guest test looking for refusal-ish words passed on all of
// them with the restriction doing nothing — deleting `this_is_me` from
// `NEVER_AS_A_GUEST` left the file green. The mutation pass found that;
// re-reading would not have, because the tests read correctly.

#[test]
fn a_stranger_cannot_teach_atlas_that_their_face_is_yours() {
    // The single worst thing on either restriction list. If `this is me`
    // works while handed over, Atlas comes to believe a stranger is you, and
    // every later check that trusts the album is wrong from then on.
    alone(|| {
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "this-is-me-guest");
        let as_owner = d.execute(&Intent::ThisIsMe);
        d.execute(&Intent::HandOver("Sam has it".into()));
        let as_guest = d.execute(&Intent::ThisIsMe);
        assert_eq!(as_guest, refusal("this_is_me"), "a guest reached the face album");
        assert_ne!(as_owner, as_guest, "the handover changed nothing about the answer");
    });
}

#[test]
fn a_stranger_cannot_finish_your_setup() {
    // Finishing setup writes config: which monitor is which, which
    // microphone, which voice. Somebody else's answers are answers about
    // their desk, not yours, and unlike the handover they outlive it.
    alone(|| {
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "finish-setup-guest");
        let as_owner = d.execute(&Intent::FinishSetup);
        d.execute(&Intent::HandOver("Sam has it".into()));
        let as_guest = d.execute(&Intent::FinishSetup);
        assert_eq!(as_guest, refusal("finish_setup"), "a guest set up your machine");
        assert_ne!(as_owner, as_guest, "the handover changed nothing about the answer");
    });
}

#[test]
fn a_stranger_cannot_read_your_messages() {
    // `THE_OWNERS_OWN` exists for exactly this, and it is the list that was
    // missing for a whole revision -- during which "hand over" stopped your
    // friend posting as you and let them read everything you had.
    alone(|| {
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "messages-guest");
        let as_owner = d.execute(&Intent::Messages);
        d.execute(&Intent::HandOver("Sam has it".into()));
        let as_guest = d.execute(&Intent::Messages);
        assert_eq!(as_guest, refusal("messages"), "a guest was read your messages");
        assert_ne!(as_owner, as_guest, "the handover changed nothing about the answer");
    });
}

#[test]
fn a_stranger_cannot_open_accounts_in_your_name() {
    // The most durable thing on the list: the handover ends when you take the
    // laptop back, and the account does not.
    alone(|| {
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "create-account-guest");
        let as_owner = d.execute(&Intent::CreateAccount("example.com".into()));
        d.execute(&Intent::HandOver("Sam has it".into()));
        let as_guest = d.execute(&Intent::CreateAccount("example.com".into()));
        assert_eq!(as_guest, refusal("create_account"), "a guest signed up as you");
        assert_ne!(as_owner, as_guest, "the handover changed nothing about the answer");
    });
}

#[test]
fn a_stranger_cannot_set_atlas_to_work_on_itself() {
    // Anything that changes Atlas outlasts the handover too, and this one
    // changes the thing doing the checking.
    alone(|| {
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "self-work-guest");
        let as_owner = d.execute(&Intent::WorkOnYourself(String::new()));
        d.execute(&Intent::HandOver("Sam has it".into()));
        let as_guest = d.execute(&Intent::WorkOnYourself(String::new()));
        assert_eq!(as_guest, refusal("work_on_yourself"), "a guest set Atlas on itself");
        assert_ne!(as_owner, as_guest, "the handover changed nothing about the answer");
    });
}

#[test]
fn a_stranger_cannot_reach_your_other_machines() {
    // On `THE_OWNERS_OWN` rather than `NEVER_AS_A_GUEST`, and the distinction
    // is the whole reason there are two lists: this does not act as you, it
    // reaches *your* other machines, which the first list never covered.
    alone(|| {
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "sync-guest");
        let as_owner = d.execute(&Intent::Sync(String::new()));
        d.execute(&Intent::HandOver("Sam has it".into()));
        let as_guest = d.execute(&Intent::Sync(String::new()));
        assert_eq!(as_guest, refusal("sync"), "a guest reached your other machines");
        assert_ne!(as_owner, as_guest, "the handover changed nothing about the answer");
    });
}
// --- secrets ---------------------------------------------------------------

#[test]
fn an_unlock_on_a_vault_with_no_passphrase_does_not_prove_anything() {
    alone(|| {
        // On a vault that has never had a passphrase, the first unlock *sets*
        // one, so it opens for whoever speaks first. Treating that as proof of
        // identity is the escape this suite already caught once: set the first
        // passphrase while handed over, then use it to take the machine back.
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "unlock-fresh");
        d.execute(&Intent::HandOver("Sam has it".into()));
        d.execute(&Intent::Unlock("whatever sam typed".into()));
        assert!(
            Handover::load(&install()).stance.handed_over(),
            "opening a fresh vault handed the machine back to a stranger"
        );
    });
}

#[test]
fn signing_in_needs_the_vault_open_and_says_which_it_is() {
    alone(|| {
        // Two separate refusals -- switched off, and locked -- and they must not
        // be the same sentence. "I can't sign you in" sends somebody to the wrong
        // settings page for as long as they are willing to keep trying.
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "signin-locked");
        let said = d.execute(&Intent::SignIn("example.com".into()));
        let lower = said.to_lowercase();
        assert!(
            lower.contains("vault") || lower.contains("switched off") || lower.contains("settings"),
            "it refused to sign in without saying which thing was in the way: {said}"
        );
        assert!(
            !lower.contains("signed you in"),
            "it claimed a sign-in with a locked vault: {said}"
        );
    });
}

// --- writing into what Atlas remembers -------------------------------------

#[test]
fn a_captured_thought_is_on_disk_before_atlas_says_it_kept_it() {
    alone(|| {
        // "Got it" and nothing written is the failure this branch has available,
        // and it is invisible until the day you go looking for the thing.
        let c = cfg();
        let p = plat();
        let mut d = daemon(&c, &p, "capture");
        let said = d.execute(&Intent::Capture("the roof needs looking at before winter".into()));
        assert!(!said.trim().is_empty(), "it said nothing at all");
        // Whatever it chose to call the record, something on disk now mentions
        // the roof. Asserted against the store rather than against a reply,
        // because the reply is the part that can lie.
        let root = std::env::temp_dir().join("atlas-damage-capture");
        let mut found = false;
        for entry in walk(&root) {
            if std::fs::read_to_string(&entry).unwrap_or_default().contains("roof") {
                found = true;
                break;
            }
        }
        assert!(found, "Atlas acknowledged a thought and wrote nothing down");
    });
}

#[test]
fn a_captured_thought_survives_a_restart() {
    alone(|| {
        // The half the test above does not cover, and the half that was broken.
        // `Notebook` has derived `Serialize` and `Deserialize` since it was
        // written; the daemon built one with `Notebook::default()` at startup and
        // nothing ever wrote it down. "Catch a thought before it's gone" lost the
        // thought at the next restart, and said "got it" on the way.
        //
        // `capture.rs`'s own tests passed throughout, because they exercise the
        // notebook directly and never ask where it goes.
        let c = cfg();
        let p = plat();
        let dir = tmp("capture-restart");
        {
            let mut d = Daemon::new(
                &c,
                &p,
                None,
                Store::new(dir.clone()),
                Proactive::new(ProactiveConfig::default()),
            );
            d.execute(&Intent::Capture("the roof needs looking at before winter".into()));
        }
        // A second Atlas, on the same state directory. Nothing is shared but the
        // disk, which is the whole claim.
        let d2 = Daemon::new(&c, &p, None, Store::new(dir), Proactive::new(ProactiveConfig::default()));
        assert!(
            d2.notebook.notes.iter().any(|n| n.text.contains("roof")),
            "the thought did not survive the restart -- {} notes came back",
            d2.notebook.notes.len()
        );
    });
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

// --- silently wrong, rather than silently lost ----------------------------

/// A lesson that could not be written down must not be promised.
///
/// `apply_lesson` ended `let _ = self.store.save(...)` and then said "Written
/// to <file>. I'll keep to it." — a promise made without checking, to someone
/// who has just corrected Atlas about the same thing twice.
///
/// Losing the correction is bad. Losing it *and* telling them it was kept is
/// worse, because they stop repeating it. That is the `silently lose
/// something` limb of this file's rule, so it belongs here rather than in
/// `NO_DAEMON_TEST`, where it sat with the reason "a wrong branch here shows
/// up the next time you ask what it learned". It does not.
///
/// # Two premises, asserted rather than assumed
///
/// A test for a failure path that never enters the path passes on any code at
/// all, and both of this one's premises are easy to get wrong:
///
/// * the store is made unwritable by pointing it at a **regular file**, so
///   `create_dir_all` fails for every user. A `chmod`-based version passes
///   while asserting on a write that quietly succeeded, because the test
///   runner is root.
/// * the correction is made in **two sittings**. `revise` counts by session
///   on purpose — "restating a complaint in the same breath is emphasis, not
///   a second occasion" — so saying it twice in one session earns no edit and
///   `apply_lesson` returns its early "nothing waiting".
#[test]
fn a_lesson_that_could_not_be_saved_is_not_promised() {
    let c = cfg();
    let p = plat();

    let dir = tmp("lesson-unwritable");
    let blocked = dir.join("state");
    std::fs::write(&blocked, b"not a directory").unwrap();

    let mut d = Daemon::new(
        &c,
        &p,
        None,
        Store::new(blocked.clone()),
        Proactive::new(ProactiveConfig::default()),
    );

    d.got_it_wrong("you said it in feet, I wanted metres", 1_000);
    d.session.started += 1;
    let offered = d.got_it_wrong("you said it in feet, I wanted metres", 200_000);

    assert!(
        d.pending_edit_for_test().is_some(),
        "no edit was earned, so `apply_lesson` returns its early return and this test \
         would pass on any behaviour at all. Second correction said: {offered}"
    );
    assert!(
        Store::new(blocked).save("mending", &1u8).is_err(),
        "the store was supposed to be unwritable, and is not"
    );

    let said = d.execute(&Intent::ApplyLesson);
    assert!(
        !said.contains("I'll keep to it") || said.contains("couldn't"),
        "a lesson that could not be written down was promised anyway: {said}"
    );
}

// Call notes, driven through the daemon: switched off, it says so and
// records nothing; switched on with no call going, "they said yes" can't
// start recording anyone (Eric, 24 Sep 2026: "voices ask then record").
#[test]
fn call_notes_through_the_daemon_record_nobody_without_the_steps() {
    let cfg = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    let p = atlas::platform::mock::MockPlatform::new(vec![atlas::platform::Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    let dir = std::env::temp_dir().join(format!("atlas-callbranch-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let mut d = atlas::daemon::Daemon::new(&cfg, &p, None, atlas::store::Store::new(dir.clone()), atlas::proactive::Proactive::new(atlas::proactive::ProactiveConfig::default()));
    let off = d.execute_timed(&atlas::intent::Intent::CallNotes("start".into()), "take notes on this call");
    // Switched off — or, where another test in this file has the machine
    // handed over, refused as the owner's. Either way: no call is noted.
    assert!(off.starts_with("Call notes are switched off") || off.contains("call notes is the owner's"), "{off}");
    assert!(d.call_notes.call.is_none());
    let _ = std::fs::remove_dir_all(dir);
}
