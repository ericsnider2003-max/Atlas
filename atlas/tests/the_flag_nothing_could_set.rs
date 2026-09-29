//! The recovery-codes flag, and the wrong answer it was producing.
//!
//! `Book::set_recovery_codes` existed, was public, and had **no caller
//! anywhere in the program**. So `has_recovery_codes` was false for every
//! account that ever existed.
//!
//! That is not an unused function. `goingaway::would_lock_you_out` clears an
//! account off the lockout list exactly when that flag is true, so with
//! nothing able to set it, **Atlas told you every second-factor account would
//! strand you abroad** — confidently, every time you asked. A false security
//! warning looks exactly like a true one; the only way to know is to already
//! know the answer.
//!
//! The tests below assert properties, not sentences:
//!
//! 1. recording the codes actually changes what the travel answer says;
//! 2. setting and saving cannot come apart, because they are one call;
//! 3. a write that failed is never reported as success, and never leaves the
//!    book in memory disagreeing with the book on disk.

use atlas::accounts::{Account, Book, Recorded, SecondFactor, Stakes};
use atlas::store::Store;

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-codes-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn book_with(site: &str) -> Book {
    let mut b = Book::default();
    b.accounts.push(Account {
        site: site.into(),
        second_factor: SecondFactor::Sms,
        stakes: Stakes::High,
        reused_password: false,
        has_recovery_codes: false,
        settings_url: None,
    });
    b
}

#[test]
fn recording_the_codes_changes_the_travel_answer() {
    // The defect, stated as a property: before this verb existed the answer
    // could not move, because nothing could move the input.
    let store = Store::new(tmp("travel"));
    let mut book = book_with("Chase");

    let before = atlas::goingaway::spoken(&book.accounts);
    assert!(before.contains("Chase"), "an SMS account with no codes should be named: {before}");

    assert_eq!(book.record_codes(&store, "Chase", true), Recorded::Changed);

    let after = atlas::goingaway::spoken(&book.accounts);
    assert!(
        !after.contains("Chase"),
        "recording the recovery codes did not change the travel answer, which is the \
         defect this file exists for: {after}"
    );
}

#[test]
fn recording_the_codes_survives_a_restart() {
    // Setting without saving is the arrangement that could be got wrong, so
    // it is the one that must not exist. Deleting the `save` inside
    // `record_codes` leaves every in-memory test green -- this is the one it
    // does not.
    let dir = tmp("restart");
    let store = Store::new(dir.clone());
    let mut book = book_with("Chase");
    assert_eq!(book.record_codes(&store, "Chase", true), Recorded::Changed);

    let reloaded = Book::load(&Store::new(dir));
    assert!(
        reloaded.accounts.iter().any(|a| a.site == "Chase" && a.has_recovery_codes),
        "the flag was accepted and not written down, so it is gone on restart"
    );
}

#[test]
fn a_site_not_in_the_book_is_not_reported_as_recorded() {
    let store = Store::new(tmp("typo"));
    let mut book = book_with("Chase");
    assert_eq!(
        book.record_codes(&store, "Chsae", true),
        Recorded::NoSuchSite,
        "a typo'd site must not read as success -- that is how you believe an account \
         is safe when nothing was recorded"
    );
}

#[test]
fn saying_it_twice_is_distinguished_from_doing_nothing() {
    let store = Store::new(tmp("twice"));
    let mut book = book_with("Chase");
    assert_eq!(book.record_codes(&store, "Chase", true), Recorded::Changed);
    assert_eq!(
        book.record_codes(&store, "Chase", true),
        Recorded::AlreadySo,
        "`nothing happened` and `it was already true` look identical from outside, and \
         only one of them is fine to say nothing about"
    );
}

#[test]
fn a_write_that_failed_is_not_reported_as_recorded() {
    // The store's root is a regular file, so `create_dir_all` fails for every
    // user including root -- the test runner here is root, and a `chmod`-based
    // version of this test passes while asserting on a write that quietly
    // succeeded.
    let dir = tmp("unwritable");
    let blocked = dir.join("state");
    std::fs::write(&blocked, b"not a directory").unwrap();
    let store = Store::new(blocked.clone());
    assert!(
        store.save("probe", &1u8).is_err(),
        "the store was supposed to be unwritable, and is not: this test would pass on \
         any behaviour at all"
    );

    let mut book = book_with("Chase");
    match book.record_codes(&store, "Chase", true) {
        Recorded::CouldNotWrite(_) => {}
        other => panic!("a failed write was reported as {other:?}"),
    }
    assert!(
        !book.accounts[0].has_recovery_codes,
        "the book in memory kept a flag that was never written, so the next question \
         gets answered from a state that does not exist"
    );
}

#[test]
fn set_recovery_codes_reports_whether_it_changed_anything() {
    // `record_codes` distinguishes `Changed` from `AlreadySo` purely by this
    // return value, so the bool is a contract and not a convenience. Tested
    // directly because a helper whose caller is well tested is still a helper
    // nothing has pinned down.
    let mut book = book_with("Chase");
    assert!(book.set_recovery_codes("Chase", true), "first set should report a change");
    assert!(!book.set_recovery_codes("Chase", true), "setting it again is not a change");
    assert!(book.set_recovery_codes("Chase", false), "clearing it is a change");
    assert!(
        !book.set_recovery_codes("Chsae", true),
        "an unknown site must report no change rather than silently adding one"
    );
}
