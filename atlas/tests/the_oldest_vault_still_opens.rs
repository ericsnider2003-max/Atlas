//! The vault from before the real cipher, and the migration that destroyed it.
//!
//! ## What happened
//!
//! `Vault::open_the_old_way` decides which of three situations it is in:
//!
//! 1. There is a check value → verify against it.
//! 2. No check value, but a secret with `real == true` → verify against that
//!    secret, and write the check while the key is in hand.
//! 3. Neither → *"no passphrase has ever been set on this vault. This unlock
//!    chooses one... Nothing is sealed yet, so there is nothing to
//!    convert."*
//!
//! Case 3's premise is false for the oldest vaults there are. Before AEAD
//! sealing existed, secrets were written with the repeating-key XOR in
//! `seal()` and recorded as `real: false`; the check value post-dates them
//! too. So such a vault has no check value and no `real` secret, satisfies
//! case 3's condition exactly, and is **full**.
//!
//! What case 3 does is `self.key = Some(random_bytes(32))`. And `Vault::get`
//! reads a legacy secret as `seal(&s.sealed, &key)` — the XOR, keyed with the
//! data key. Those bytes were XORed with the key derived from the real
//! passphrase. XOR them with a fresh random key and the result is noise;
//! `String::from_utf8` fails; every secret answers "that didn't decrypt". The
//! new envelope is then written over the vault, so the key that could have
//! read them is gone from the only place it existed.
//!
//! The person typed their own correct passphrase and lost every credential
//! they had stored. There is no recovery: the derived key was never written
//! down, which is the one thing the vault is most careful about.
//!
//! ## Why it was invisible
//!
//! Every test of the migration built its "old vault" with
//! `pre_envelope_for_test`, which seals with AEAD and sets `real: true` — so
//! every one of them went down case 2 and case 3 was never entered with
//! anything in it. The fixture and the code agreed, and only the oldest real
//! vault on disk disagreed. `Vault::legacy_cipher_for_test` exists so that
//! the fixture is genuinely XOR under a genuinely derived key, because
//! otherwise this file would test the same case 2 all over again.

use atlas::vault::{Kind, Vault, VaultConfig};

const PASSPHRASE: &str = "the one thing you actually know";

/// The real config. The key derivation is deliberately slow, which is why
/// this file has few tests rather than many.
fn cfg() -> VaultConfig {
    VaultConfig {
        enabled: true,
        lock_after_mins: 15,
        lock_on_screen_lock: true,
        open_on_this_login: false,
    }
}

fn legacy() -> Vault {
    let v = Vault::legacy_cipher_for_test(
        PASSPHRASE,
        &[
            ("bank", Kind::Login, "correct horse battery staple"),
            ("note", Kind::Note, "the spare key is under the mat"),
        ],
        50,
        &cfg(),
    )
    .expect("a legacy vault");
    // The fixture has to actually be in the shape that triggered it, or this
    // file is testing the well-trodden path with a new name.
    assert!(v.check.is_empty(), "the fixture has a check value, so it is not the old format");
    assert!(v.wraps.is_empty(), "the fixture has an envelope, so it is not the old format");
    assert!(
        v.secrets.iter().all(|s| !s.real),
        "the fixture's secrets are AEAD-sealed, so this goes down the branch that \
         already worked"
    );
    v
}

#[test]
fn the_oldest_vault_opens_and_still_has_everything_in_it() {
    let mut v = legacy();

    v.open(PASSPHRASE, 100, &cfg()).expect("the oldest vault stopped opening");

    assert_eq!(
        v.get("bank", 100).expect("bank"),
        "correct horse battery staple",
        "the secret came back wrong, which is what a replaced data key does to a \
         legacy secret"
    );
    assert_eq!(v.get("note", 100).expect("note"), "the spare key is under the mat");
}

#[test]
fn opening_it_does_not_rewrite_a_single_secret() {
    // The difference between an upgrade and a re-encryption that can go wrong
    // half way. The key the secrets are already sealed under is adopted; the
    // bytes do not move.
    let mut v = legacy();
    let before: Vec<Vec<u8>> = v.secrets.iter().map(|s| s.sealed.clone()).collect();

    v.open(PASSPHRASE, 100, &cfg()).expect("opens");
    let after: Vec<Vec<u8>> = v.secrets.iter().map(|s| s.sealed.clone()).collect();

    assert_eq!(before, after, "the secrets were rewritten while opening the vault");
}

#[test]
fn it_reopens_and_keeps_reopening() {
    let mut v = legacy();
    v.open(PASSPHRASE, 100, &cfg()).expect("first open");
    v.lock();
    v.open(PASSPHRASE, 200, &cfg()).expect("it did not reopen");
    assert_eq!(v.get("bank", 200).expect("bank"), "correct horse battery staple");
    v.lock();
    v.open(PASSPHRASE, 300, &cfg()).expect("it did not reopen a third time");
    assert_eq!(v.get("note", 300).expect("note"), "the spare key is under the mat");
}

#[test]
fn opening_it_writes_nothing_at_all() {
    // **The fix to the fix.** The first version of this branch adopted the
    // derived key and then wrote a check value and an envelope from it. That
    // is what would have made a false accept permanent: with a
    // `How::Passphrase` wrap present, the next open goes through
    // `open_wrapped` and tries only the wrap built from whatever was typed --
    // so a wrong passphrase that passed the weak plausibility check would
    // lock the correct one out for ever. The same loss this branch exists to
    // prevent, arriving by a longer road.
    //
    // A plausible-UTF-8 check is weak evidence. Weak evidence is enough to
    // justify reading the data and not enough to justify writing a new way
    // in, so nothing is written and the vault stays in its old format until
    // a deliberate act upgrades it.
    let mut v = legacy();
    let check_before = v.check.clone();
    let wraps_before = v.wraps.len();

    v.open(PASSPHRASE, 100, &cfg()).expect("opens");

    assert_eq!(v.check, check_before, "a check value was written from a weak check");
    assert_eq!(
        v.wraps.len(),
        wraps_before,
        "an envelope was built from a passphrase that was only plausibly right.          That is what makes a false accept irreversible."
    );
}

#[test]
fn a_weak_check_is_not_reported_as_proof_of_who_typed() {
    // `proved_it()` is `key.is_some() && verified`, and is documented as "the
    // only form of 'the vault is open' that says anything about who typed".
    // `handover::take_back` uses it as identity proof. A check that amounts
    // to "those bytes are valid UTF-8" is not identity proof, and the first
    // version of this branch set `verified = true` under a comment that said
    // it stayed false.
    let mut v = legacy();
    v.open(PASSPHRASE, 100, &cfg()).expect("opens");
    assert!(v.state() == atlas::vault::State::Open, "the vault did not open");
    assert!(
        !v.proved_it(),
        "a legacy vault opened on a plausibility check reports that the owner          proved who they are -- which is what `handover::take_back` asks before          handing control back"
    );
}

#[test]
fn every_legacy_secret_has_to_decode_not_just_one() {
    // `any` was the first shape and it is too weak: a wrong 32-byte key
    // produces valid UTF-8 from a short value roughly one time in sixteen for
    // four characters, and nothing rate-limits attempts. With `all`, a false
    // accept is the product of those probabilities across every secret.
    //
    // Checked by construction: a vault with one short secret and one long one
    // must refuse a wrong passphrase that the short one alone might have
    // waved through.
    let mut v = Vault::legacy_cipher_for_test(
        PASSPHRASE,
        &[
            ("pin", Kind::Note, "1234"),
            ("bank", Kind::Login, "correct horse battery staple and then some more"),
        ],
        50,
        &cfg(),
    )
    .expect("a legacy vault");

    // A spread of wrong passphrases. Any one of them might make the 4-byte
    // secret decode; none should make both.
    for attempt in ["wrong", "also wrong", "nope not it", "the one thing you actually knew"] {
        assert!(
            v.open(attempt, 100, &cfg()).is_err(),
            "{attempt:?} opened a legacy vault it should not have"
        );
    }
    v.open(PASSPHRASE, 100, &cfg()).expect("the right passphrase still opens it");
    assert_eq!(v.get("pin", 100).expect("pin"), "1234");
}

#[test]
fn a_legacy_vault_holding_only_empty_values_is_not_a_permanent_lockout() {
    // The mirror image of the defect, and just as much a loss. With `all`
    // and a bytes check, a vault whose legacy secrets are all empty carries
    // no evidence either way -- and refusing for ever on evidence that does
    // not exist would lock the owner out of a vault with nothing in it to
    // protect.
    let mut v = Vault::legacy_cipher_for_test(PASSPHRASE, &[("blank", Kind::Note, "")], 50, &cfg())
        .expect("a legacy vault");
    v.open(PASSPHRASE, 100, &cfg())
        .expect("a vault with nothing readable in it refused to open at all");
    assert!(!v.proved_it(), "an empty legacy vault still claims proof");
}

#[test]
fn the_wrong_passphrase_is_refused_rather_than_adopted() {
    // The other half, and the one that makes the fix safe rather than merely
    // non-destructive. XOR has no authentication tag, so "did it decrypt" is
    // not a question the cipher can answer -- what is checked is that a
    // secret comes back as non-empty valid UTF-8, which a wrong key across a
    // secret of any length very rarely produces.
    //
    // It matters that this REFUSES rather than opening: an unlock that
    // adopted a wrong derived key would write an envelope around it and make
    // the wrong passphrase the way in to a key that decrypts nothing.
    let mut v = legacy();
    let err = v
        .open("not the passphrase at all", 100, &cfg())
        .expect_err("a wrong passphrase opened the oldest vault");
    assert!(err.contains("passphrase"), "the refusal does not name the passphrase: {err}");
    assert!(v.wraps.is_empty(), "a refused unlock still wrote an envelope");
    assert!(v.check.is_empty(), "a refused unlock still wrote a check value");
}

#[test]
fn a_full_legacy_vault_is_not_reported_as_having_no_passphrase() {
    // `has_a_passphrase` had the same false premise, and it is the question
    // everything else asks before treating an unlock as proof of anything. It
    // looked for a wrap, a check value, or a `real` secret -- none of which a
    // legacy vault has -- and so answered "no passphrase has ever been set"
    // about a vault whose secrets are sealed under one.
    let v = legacy();
    assert!(
        v.has_a_passphrase(),
        "a vault full of secrets sealed under a derived key reports that nobody has \
         ever set a passphrase, so the next unlock is treated as choosing one"
    );
}

#[test]
fn a_genuinely_empty_vault_still_treats_the_first_unlock_as_choosing() {
    // So the fix cannot be satisfied by refusing everything. An empty vault
    // has nothing sealed, and its first unlock really is a passphrase being
    // chosen -- which must stay distinguishable from one being checked.
    let mut fresh = Vault::default();
    assert!(!fresh.has_a_passphrase());
    fresh.open(PASSPHRASE, 100, &cfg()).expect("a new vault opens");
    assert!(
        !fresh.proved_it(),
        "choosing a passphrase was reported as proving one, which is the thing \
         `proved_it` exists to keep apart"
    );
}
