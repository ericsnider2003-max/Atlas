//! Forgetting the passphrase, and what it costs.
//!
//! # What was wrong
//!
//! The vault derived its key **from the passphrase**. That is the obvious
//! design, and it has a consequence that only shows up on the worst day:
//! forget the sentence and every secret is gone, permanently, with no second
//! chance of any kind. Worse — because it is not even your own data at stake
//! — a handover could never be taken back, so forgetting a passphrase meant
//! being locked out of your own assistant by your own assistant, forever.
//!
//! Atlas said so plainly ("Atlas cannot recover it and neither can anyone
//! else"), which is honest and is not the same as being finished. Every
//! serious system that holds a key you can forget ships a second way in:
//! BitLocker prints a recovery key, FileVault prints a recovery key, a
//! password manager gives you an emergency kit. None of them can recover the
//! passphrase itself — nor can this, and nor should it, because a system that
//! could reconstruct your passphrase from what is on the disk would hand it
//! to anyone who took the disk.
//!
//! # What these tests defend
//!
//! The secrets are sealed under a random **data key**, and that key is sealed
//! separately under the passphrase and under a recovery key. Either opens it.
//! So the properties worth pinning down are:
//!
//! * the recovery key really does open a vault whose passphrase is gone, and
//!   really does end a handover — the lockout is survivable;
//! * it is never written down anywhere by Atlas itself;
//! * making a new one stops the old one working;
//! * a vault written in the old format still opens, keeps every secret, and
//!   comes out the other side with an envelope round it.

use atlas::handover::{Handover, Stance};
use atlas::vault::{How, Kind, Vault, VaultConfig};

const PASSPHRASE: &str = "the one thing you actually know";
const FORGOTTEN: &str = "whatever it was that I typed in September";

fn cfg() -> VaultConfig {
    VaultConfig::default()
}

/// A vault with a passphrase and a recovery key, shut again.
fn set_up() -> (Vault, String) {
    let mut v = Vault::default();
    v.open(PASSPHRASE, 100, &cfg()).expect("first open sets the passphrase");
    let code = v.issue_recovery_key(100, &cfg()).expect("no recovery key was made");
    v.put("bank", Kind::Login, "correct horse battery staple", 100).unwrap();
    v.lock();
    (v, code)
}

// --- the day it matters ----------------------------------------------------

#[test]
fn a_forgotten_passphrase_is_no_longer_the_end_of_it() {
    let (mut v, code) = set_up();

    // The passphrase is gone. That part is real and stays real.
    let refused = v.open(FORGOTTEN, 200, &cfg()).unwrap_err();
    assert!(refused.contains("isn't the passphrase"), "{refused}");
    assert_eq!(v.state(), atlas::vault::State::Sealed);

    // The piece of paper opens it, and everything is still in there.
    v.open_with_recovery_key(&code, 200, &cfg()).expect("the recovery key was refused");
    assert_eq!(v.get("bank", 200).unwrap(), "correct horse battery staple");
    assert!(v.proved_it(), "coming in on the recovery key proved nothing");
    assert_eq!(v.opened_with(), Some(How::RecoveryKey));
}

#[test]
fn the_recovery_key_takes_a_handover_back() {
    // The worse half of the old failure, and the reason this is not only
    // about the secrets. `take_back` rests on `proved_it`, so what matters is
    // that a recovery key produces proof -- and it is the right kind of
    // proof for this situation, because the person holding your laptop is in
    // the room and the card in your desk drawer is not.
    let (mut v, code) = set_up();
    let mut h = Handover::default();
    h.hand_over("Sam has it", 100);

    // Forgotten passphrase, handed-over laptop: the old dead end.
    assert!(v.open(FORGOTTEN, 200, &cfg()).is_err());
    assert!(h.take_back(&v, 200).is_err(), "a shut vault took the handover back");
    assert_eq!(h.stance, Stance::HandedOver);

    v.open_with_recovery_key(&code, 200, &cfg()).unwrap();
    let said = h.take_back(&v, 200).expect("the recovery key did not take it back");
    assert!(said.contains("Yours again"), "{said}");
    assert_eq!(h.stance, Stance::Yours);
}

#[test]
fn setting_a_new_passphrase_is_how_you_leave_a_recovery() {
    // Coming in on the paper and walking away without setting a passphrase
    // leaves a vault whose only key is that one piece of paper -- the same
    // failure, one step further along.
    let (mut v, code) = set_up();
    v.open_with_recovery_key(&code, 200, &cfg()).unwrap();
    v.set_passphrase_from_recovery("a sentence I will actually remember", 200, &cfg())
        .expect("could not set a passphrase after recovering");
    v.lock();

    v.open("a sentence I will actually remember", 300, &cfg()).expect("the new passphrase failed");
    assert!(v.proved_it());
    assert_eq!(v.get("bank", 300).unwrap(), "correct horse battery staple");

    // And the old one is dead.
    v.lock();
    assert!(v.open(PASSPHRASE, 300, &cfg()).is_err(), "the replaced passphrase still opens it");
}

#[test]
fn a_passphrase_you_have_cannot_be_replaced_through_the_recovery_door() {
    // `set_passphrase_from_recovery` asks no old passphrase, so the thing
    // standing between it and being a way to change anyone's passphrase is
    // the requirement that *this unlock* came in on a recovery key. Merely
    // being open is not enough -- on a fresh vault, being open is a state
    // anybody can reach by typing twelve characters.
    let (mut v, _code) = set_up();
    v.open(PASSPHRASE, 200, &cfg()).unwrap();
    let why = v.set_passphrase_from_recovery("something else entirely", 200, &cfg()).unwrap_err();
    assert!(why.contains("recovery key"), "{why}");

    let mut fresh = Vault::default();
    fresh.open("twelve characters at least", 100, &cfg()).unwrap();
    assert!(
        fresh.set_passphrase_from_recovery("another twelve characters", 100, &cfg()).is_err(),
        "an unverified first unlock could set a passphrase through the recovery door"
    );
}

// --- the key itself --------------------------------------------------------

#[test]
fn the_recovery_key_is_not_written_down_by_atlas() {
    // The whole value of it depends on this. A recovery key stored next to
    // the thing it unlocks is a key taped to the lid, which is the failure
    // this module's own doc opens by naming.
    let (v, code) = set_up();
    let yaml = serde_yaml::to_string(&v).unwrap();
    let bare: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    assert!(!yaml.contains(&code), "the recovery key was serialised verbatim");
    assert!(!yaml.contains(&bare), "the recovery key was serialised without its dashes");
    for group in code.split('-') {
        assert!(
            !yaml.contains(group),
            "a piece of the recovery key ({group}) is in the file on disk"
        );
    }
}

#[test]
fn making_a_new_one_stops_the_old_one_working() {
    // The behaviour you want on the day you think somebody saw it. If making
    // a new key left the old one working, people would make a new one and
    // believe they had done something.
    let (mut v, old) = set_up();
    v.open(PASSPHRASE, 200, &cfg()).unwrap();
    let new = v.issue_recovery_key(200, &cfg()).unwrap();
    assert_ne!(old, new);
    v.lock();

    assert!(v.open_with_recovery_key(&old, 300, &cfg()).is_err(), "the retired key still opens it");
    v.open_with_recovery_key(&new, 300, &cfg()).expect("the new key does not open it");
    assert_eq!(v.get("bank", 300).unwrap(), "correct horse battery staple");
}

#[test]
fn it_is_read_the_way_somebody_copies_it_off_paper() {
    // Case, spacing and the four letters the alphabet exists to avoid. This
    // is not politeness: the entire situation is a person reading their own
    // handwriting months later, and a key refused because they typed a
    // lowercase o is a key that did not work.
    let (mut v, code) = set_up();
    let bare: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();

    for spelling in [
        code.to_lowercase(),
        bare.clone(),
        bare.chars().flat_map(|c| [c, ' ']).collect::<String>(),
        code.replace('-', " "),
        // Read back as the letters they look like. None of these can occur in
        // a real key, so accepting them costs nothing.
        bare.replace('1', "l").replace('0', "O"),
    ] {
        v.lock();
        v.open_with_recovery_key(&spelling, 300, &cfg())
            .unwrap_or_else(|e| panic!("refused {spelling:?}: {e}"));
    }

    // And it is not so forgiving that it opens for something else.
    v.lock();
    assert!(v.open_with_recovery_key("not a recovery key at all", 300, &cfg()).is_err());
    v.lock();
    assert!(v.open_with_recovery_key("", 300, &cfg()).is_err());
}

#[test]
fn a_key_is_long_enough_to_be_worth_writing_down() {
    // 24 characters from a 32-letter alphabet is 120 bits. The number matters
    // less than the shape: this is the one secret in the system that is not
    // chosen by a person, so it has no business being guessable.
    let (_v, code) = set_up();
    let bare: String = code.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    assert_eq!(bare.len(), 24, "{code}");
    assert!(code.contains('-'), "it is printed as one unbroken run: {code}");
    for c in bare.chars() {
        assert!(c.is_ascii_uppercase() || c.is_ascii_digit(), "unexpected character in {code}");
        assert!(!"ILOU".contains(c), "{c} is one of the four that get misread: {code}");
    }

    // Two in a row are not the same, which is the cheapest possible check
    // that this is random at all.
    let mut v = Vault::default();
    v.open(PASSPHRASE, 100, &cfg()).unwrap();
    let a = v.issue_recovery_key(100, &cfg()).unwrap();
    let b = v.issue_recovery_key(100, &cfg()).unwrap();
    assert_ne!(a, b);
}

#[test]
fn a_vault_with_no_recovery_key_says_so_rather_than_refusing_vaguely() {
    let mut v = Vault::default();
    v.open(PASSPHRASE, 100, &cfg()).unwrap();
    v.lock();
    assert!(!v.has_a_recovery_key());
    let why = v.open_with_recovery_key("ABCD-EFGH-JKMN-PQRS-TVWX-YZ23", 200, &cfg()).unwrap_err();
    assert!(why.contains("no recovery key"), "{why}");
    assert!(why.contains("atlas vault recovery"), "it does not say how to get one: {why}");
}

// --- the vault that already exists -----------------------------------------

#[test]
fn a_vault_from_before_all_this_opens_and_keeps_everything() {
    // The part of this change that could destroy data. A vault on disk today
    // has its secrets sealed under a key derived straight from the
    // passphrase, and no wraps at all.
    let mut old = Vault::pre_envelope_for_test(
        PASSPHRASE,
        &[("bank", Kind::Login, "correct horse battery staple"), ("note", Kind::Note, "x")],
        50,
        &cfg(),
    )
    .unwrap();
    assert!(old.wraps.is_empty(), "the fixture is not in the old format");

    old.open(PASSPHRASE, 100, &cfg()).expect("an existing vault stopped opening");
    assert!(old.proved_it());
    assert_eq!(old.get("bank", 100).unwrap(), "correct horse battery staple");
    assert_eq!(old.get("note", 100).unwrap(), "x");

    // It came out with an envelope round it -- and, critically, without the
    // secrets having been rewritten. Re-sealing them would be the moment a
    // half-finished conversion could lose the lot.
    assert!(!old.wraps.is_empty(), "it opened and was not converted, so this never happens");
    let sealed_now: Vec<Vec<u8>> = old.secrets.iter().map(|s| s.sealed.clone()).collect();
    old.lock();
    old.open(PASSPHRASE, 200, &cfg()).expect("it did not reopen after converting");
    let sealed_after: Vec<Vec<u8>> = old.secrets.iter().map(|s| s.sealed.clone()).collect();
    assert_eq!(sealed_now, sealed_after, "the secrets were rewritten by the conversion");

    // And now it can have a recovery key like any other.
    let code = old.issue_recovery_key(200, &cfg()).unwrap();
    old.lock();
    old.open_with_recovery_key(&code, 300, &cfg()).unwrap();
    assert_eq!(old.get("bank", 300).unwrap(), "correct horse battery staple");
}

#[test]
fn the_wrong_passphrase_is_still_refused_on_a_converted_vault() {
    let mut old = Vault::pre_envelope_for_test(
        PASSPHRASE,
        &[("bank", Kind::Login, "correct horse battery staple")],
        50,
        &cfg(),
    )
    .unwrap();
    assert!(old.open(FORGOTTEN, 100, &cfg()).is_err(), "the wrong passphrase converted it");
    assert!(old.wraps.is_empty(), "a failed open left a wrap behind");
    assert_eq!(old.state(), atlas::vault::State::Sealed);
}

#[test]
fn changing_the_passphrase_leaves_the_secrets_and_the_recovery_key_alone() {
    // Two properties in one, because they come from the same fact: the data
    // key does not change, so neither the secrets nor the other wraps care.
    //
    // The second one is a deliberate choice rather than an accident. Changing
    // your passphrase should not silently retire the card in your desk --
    // you would not find out until the day you needed it.
    let (mut v, code) = set_up();
    v.change_passphrase(PASSPHRASE, "an entirely different sentence", 200, &cfg())
        .expect("the passphrase would not change");
    v.lock();

    v.open("an entirely different sentence", 300, &cfg()).unwrap();
    assert_eq!(v.get("bank", 300).unwrap(), "correct horse battery staple");
    v.lock();
    v.open_with_recovery_key(&code, 300, &cfg())
        .expect("changing the passphrase quietly retired the recovery key");
    assert_eq!(v.get("bank", 300).unwrap(), "correct horse battery staple");
    v.lock();
    assert!(v.open(PASSPHRASE, 300, &cfg()).is_err(), "the old passphrase still works");
}

#[test]
fn a_used_recovery_key_can_be_thrown_away() {
    let (mut v, code) = set_up();
    v.open(PASSPHRASE, 200, &cfg()).unwrap();
    assert!(v.forget_recovery_key());
    assert!(!v.forget_recovery_key(), "it reported throwing away a second one");
    v.lock();
    assert!(v.open_with_recovery_key(&code, 300, &cfg()).is_err());
    // The passphrase still works -- forgetting the spare is not locking
    // yourself out.
    v.open(PASSPHRASE, 300, &cfg()).unwrap();
    assert_eq!(v.get("bank", 300).unwrap(), "correct horse battery staple");
}

#[test]
fn every_wrap_gets_its_own_random_salt() {
    // Caught by planting a fixed salt and watching every test above stay
    // green. Nothing in what a vault *does* depends on the salt being random
    // -- it opens, it recovers, it keeps the secrets -- so the only way this
    // is defended is by asserting it directly.
    //
    // Two things go wrong with a shared or fixed salt, and neither shows up
    // in ordinary use. A salt that is the same on every machine makes one
    // precomputed table worth building against everybody, which is the whole
    // reason the vault salt exists. And two wraps sharing one salt means the
    // passphrase and the recovery key derive the same key-encrypting key
    // whenever they happen to match -- which leaks, to anyone with the file,
    // the fact that they do.
    let (a, code_a) = set_up();
    assert_eq!(a.wraps.len(), 2, "expected a passphrase wrap and a recovery wrap");

    let salts: Vec<&Vec<u8>> = a.wraps.iter().map(|w| &w.salt).collect();
    assert_ne!(salts[0], salts[1], "two wraps in one vault share a salt");
    for s in &salts {
        assert_eq!(s.len(), 16, "a salt is not the expected length");
        assert!(s.iter().any(|b| *b != s[0]), "that salt is a constant: {s:?}");
    }

    // And a second vault, same passphrase, different salts -- so the derived
    // key is different on every machine even when the sentence is not.
    let (b, code_b) = set_up();
    assert_ne!(code_a, code_b);
    for wa in &a.wraps {
        for wb in &b.wraps {
            assert_ne!(
                wa.salt, wb.salt,
                "two installs derived from the same salt, so one table attacks both"
            );
        }
    }
}

#[test]
fn a_way_in_can_be_named_out_loud() {
    // Said back to the person after a recovery, where "the vault is open" on
    // its own would not tell them the thing that matters: they got in without
    // the passphrase, so the next step is to set one.
    assert_eq!(How::Passphrase.plain(), "the passphrase");
    assert_eq!(How::RecoveryKey.plain(), "a recovery key");
    for how in [How::Passphrase, How::RecoveryKey] {
        assert!(!how.plain().contains('_'), "an identifier would reach the screen");
        assert!(how.plain().chars().next().unwrap().is_lowercase(), "it is written to sit mid-sentence");
    }
}

#[test]
fn what_was_typed_is_tidied_the_way_paper_is_misread() {
    // The transcription rule on its own, rather than only through an unlock.
    // It is the piece most likely to be quietly changed later -- it looks
    // like string cleanup and it is actually the difference between a
    // recovery key working and not working at the one moment it is needed.
    use atlas::vault::tidy_recovery_key as tidy;

    assert_eq!(tidy("abcd-2345"), "ABCD2345", "case and dashes");
    assert_eq!(tidy("  ab cd  23 45 "), "ABCD2345", "however they grouped it");
    assert_eq!(tidy("AB.CD/23:45"), "ABCD2345", "whatever punctuation they used");

    // The four the alphabet leaves out, read back as what they look like.
    // None can occur in a real key, so this only ever rescues a misreading.
    assert_eq!(tidy("IL0O1"), "11001");

    // And it does not invent anything: an unrelated string stays unrelated.
    assert_eq!(tidy("hello there"), "HE110THERE");
    assert_eq!(tidy(""), "");
    assert_eq!(tidy("-----"), "");
}
