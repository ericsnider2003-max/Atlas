//! What the vault's encryption actually does.
//!
//! `derive` carried a comment saying the round count *was* the security, and
//! `kdf_rounds` defaults to 600,000, which reads like a considered choice.
//! Measuring it showed the rounds cost an attacker nothing and the cipher
//! reuses one key across every secret.
//!
//! Following HOW_TO_FIND_PROBLEMS §5: everything else in the vault — the
//! passphrase length floor, key held in memory only, sealed at rest, locked
//! while you sleep, honest constants about what it can't protect — is careful
//! and correct, and all of it rests on these two functions. Careful work built
//! on one weak assumption is not careful work.
//!
//! These tests exist so the refusal cannot be quietly removed, and so that
//! flipping `REAL_CRYPTO` without replacing the primitives fails the build.

use atlas::vault::{self, Kind, Vault, VaultConfig};

const PASSPHRASE: &str = "a reasonably long sentence for this";

fn opened() -> Vault {
    let mut v = Vault::default();
    v.open(PASSPHRASE, 0, &VaultConfig::default())
        .expect("passphrase is long enough");
    v
}

// --- the refusal ------------------------------------------------------------

#[test]
fn real_crypto_is_declared_not_assumed() {
    // The same constant, now asserting the other way. It was `false` with a
    // note saying to flip it in the same change that replaced `derive` and
    // `seal`. That change has happened, so this holds the line from the other
    // side: nothing may quietly revert the vault to the stand-in.
    assert!(vault::REAL_CRYPTO, "the vault has been reverted to placeholder crypto");
    assert!(vault::real_crypto_here(), "a platform is claiming it cannot encrypt");
}

#[test]
fn a_credential_stores_on_every_platform_rather_than_being_refused() {
    // Replaces `it_refuses_to_store_anything_it_cannot_actually_protect`.
    // The refusal was correct while the cipher was fake. Keeping it now would
    // mean an assistant that cannot hold a single login on any machine.
    let mut v = opened();
    for kind in [Kind::Login, Kind::TotpSeed, Kind::RecoveryCodes, Kind::ApiKey] {
        v.put("something", kind, "hunter2", 0).unwrap_or_else(|e| {
            panic!("{kind:?} was refused even though the vault can protect it: {e}")
        });
        assert_eq!(v.get("something", 0).unwrap(), "hunter2");
    }
    assert!(v.weakly_sealed().is_empty(), "something went in under the stand-in");
}

#[test]
fn a_setting_that_no_longer_does_anything_says_so_out_loud() {
    // Replaces `the_refusal_says_what_to_do_instead`. `kdf_rounds` is still in
    // everyone's tools.yaml and is now ignored. Silently ignoring a number
    // somebody deliberately set is the same shape of problem as the cipher
    // this replaced: it looks like it is doing something.
    assert!(vault::KDF_ROUNDS_IS_IGNORED.contains("kdf_rounds"));
    assert!(
        vault::KDF_ROUNDS_IS_IGNORED.contains("Argon2id"),
        "it says the old setting is dead without saying what replaced it"
    );
}

#[test]
fn a_note_to_yourself_is_still_worth_storing() {
    // Failing closed on everything would make the vault useless rather than
    // honest. The line is whether losing it costs something you can't undo.
    let mut v = opened();
    assert!(v.put("where the spare key is", Kind::Note, "under the pot", 0).is_ok());
    assert!(!Kind::Note.needs_real_crypto());
}

#[test]
fn every_credential_kind_is_classified() {
    for kind in [Kind::Login, Kind::TotpSeed, Kind::RecoveryCodes, Kind::ApiKey] {
        assert!(kind.needs_real_crypto(), "{kind:?} is unclassified");
        assert!(!kind.plain().is_empty());
    }
}

#[test]
fn a_short_passphrase_is_still_refused() {
    let mut v = Vault::default();
    assert!(v.open("short", 0, &VaultConfig::default()).is_err());
}

// ================= what makes storing a password reasonable =================

#[test]
fn the_refusal_is_about_the_encryption_not_about_the_kind_of_secret() {
    // Refusing logins forever would have made the vault pointless. The
    // problem was never that a password is special — it was that the cipher
    // was a stand-in. Fix the cipher and the refusal should disappear.
    let src = std::fs::read_to_string("src/vault.rs").unwrap();
    assert!(
        src.contains("if !real_crypto_here() && kind.needs_real_crypto()"),
        "the refusal is still hardcoded rather than checking whether encryption is real"
    );
}

#[test]
fn real_sealing_uses_the_operating_systems_own_protection() {
    // The same mechanism the Windows credential manager uses, rather than
    // anything hand-rolled. Hand-rolling it is what went wrong the first time.
    let win = std::fs::read_to_string("src/platform/win.rs").unwrap();
    assert!(win.contains("CryptProtectData"));
    assert!(win.contains("CryptUnprotectData"));
    assert!(win.contains("CRYPTPROTECT_UI_FORBIDDEN"), "the plaintext must not reach the swap file");
}

#[test]
fn the_passphrase_still_matters_under_real_encryption() {
    // Without mixing it in, anything running as your Windows user could
    // unprotect the file without knowing it.
    let win = std::fs::read_to_string("src/platform/win.rs").unwrap();
    assert!(win.contains("p_optional_entropy"));
    let vault = std::fs::read_to_string("src/vault.rs").unwrap();
    assert!(vault.contains("passphrase.as_bytes()"), "the passphrase isn't mixed in");
}

#[test]
fn a_wrong_passphrase_and_a_stolen_file_fail_identically() {
    // Saying which would tell an attacker something.
    let win = std::fs::read_to_string("src/platform/win.rs").unwrap();
    let msg = win.split("couldn't open it").nth(1).unwrap();
    let first: String = msg.chars().take(140).collect();
    assert!(first.contains("wrong passphrase"));
    assert!(first.contains("another machine"));
}

#[test]
fn there_is_no_weaker_fallback_when_sealing_fails() {
    // A stub that silently does something weaker is how the original problem
    // happened. This used to check the `cfg(not(windows))` block, back when
    // "real encryption" meant "Windows". The cipher is the same everywhere
    // now, so the fallback that matters is what `put` does when sealing
    // errors — behaviour, checked below, rather than the shape of the source.
    let v = Vault::default();
    // Locked: no key, so sealing cannot happen.
    let mut v2 = v.clone();
    assert!(v2.put("bank", Kind::Login, "hunter2", 0).is_err());
    assert!(v2.secrets.is_empty(), "it stored something it could not seal");
}

#[test]
fn what_encryption_cannot_do_is_still_stated() {
    // Real encryption doesn't change this, and the old comment was right —
    // so the two statements must stay different from each other rather than
    // one quietly replacing the other.
    use atlas::vault::{NOT_A_PASSWORD_MANAGER, WHAT_THIS_DOES_NOT_STOP};
    assert!(WHAT_THIS_DOES_NOT_STOP.contains("open and in someone else's hands"));
    assert_ne!(NOT_A_PASSWORD_MANAGER, WHAT_THIS_DOES_NOT_STOP);
    assert!(WHAT_THIS_DOES_NOT_STOP.len() > 80);
}

// ---------------------------------------------------------------------------
// Real encryption has to be the thing that actually runs.
//
// The refusal above asked "is real encryption available on this machine?" and,
// when the answer was yes, stored the secret with `seal` anyway — the
// repeating-key XOR its own refusal message calls out by name. `seal_for_real`
// existed, was documented, was tested, and was called by nothing.
//
// The effect was backwards: on Linux, where no credential was at stake, the
// vault refused. On Windows — the machine this is built for — the guard
// relaxed and every login, API key, TOTP seed and recovery code went in under
// the cipher the guard existed to prevent.
// ---------------------------------------------------------------------------

#[test]
fn availability_of_real_encryption_selects_it_rather_than_merely_permitting_storage() {
    // The whole bug in one assertion. On a machine with real crypto, a stored
    // credential must come back marked as really sealed — not merely stored.
    if !vault::real_crypto_here() {
        return; // asserted from the other side below
    }
    let mut v = opened();
    v.put("bank", Kind::Login, "hunter2", 0).unwrap();
    assert!(
        v.weakly_sealed().is_empty(),
        "a credential was stored under the stand-in cipher on a machine that has real encryption"
    );
    assert_eq!(v.get("bank", 0).unwrap(), "hunter2", "real sealing must still round-trip");
}

#[test]
fn a_secret_that_cannot_be_sealed_is_not_stored_at_all() {
    // Replaces `without_real_encryption_a_credential_is_still_refused`, which
    // became unreachable the moment `real_crypto_here()` stopped being
    // platform-dependent — it early-returned on every platform and asserted
    // nothing, which is a passing test that tests nothing.
    //
    // The refusal it was guarding still exists and still matters: it now hangs
    // off the cipher failing rather than off the platform. A vault whose key
    // is the wrong size must refuse a credential, never downgrade it.
    let mut v = Vault::default();
    v.secrets.clear();
    let e = v.put("bank", Kind::Login, "hunter2", 0).unwrap_err();
    assert!(e.contains("locked"), "a locked vault stored a credential: {e}");
    assert!(v.list().is_empty(), "a refused secret was stored anyway");
}

#[test]
fn a_note_does_not_need_real_encryption_and_still_round_trips() {
    // The one kind that is allowed the stand-in, so the refusal doesn't make
    // the vault useless on a machine without OS data protection.
    let mut v = opened();
    v.put("reminder", Kind::Note, "back door key is under the mat", 0).unwrap();
    assert_eq!(v.get("reminder", 0).unwrap(), "back door key is under the mat");
}

#[test]
fn how_a_secret_was_sealed_is_recorded_on_the_secret_not_guessed_from_the_machine() {
    // A vault file written before the real path existed contains XOR entries.
    // Reading those with the OS unprotect fails with a confusing error rather
    // than the value, so the method has to travel with the secret.
    let mut v = opened();
    v.put("reminder", Kind::Note, "value", 0).unwrap();
    let s = v.secrets.iter().find(|s| s.name == "reminder").unwrap();
    assert_eq!(
        s.real,
        vault::real_crypto_here(),
        "the marker must reflect how it was actually sealed"
    );
}

#[test]
fn an_old_weakly_sealed_credential_is_named_rather_than_left_to_look_protected() {
    // Hand-built to stand in for a vault written before the fix: a Login that
    // was sealed the weak way. It cannot be silently upgraded — that needs the
    // passphrase and is the person's call — so the minimum is to say so.
    let mut v = opened();
    v.secrets.push(atlas::vault::Secret {
        name: "old-bank".into(),
        kind: Kind::Login,
        sealed: vec![1, 2, 3],
        added: 0,
        last_used: 0,
        real: false,
    });
    assert_eq!(v.weakly_sealed(), vec!["old-bank"]);
}

#[test]
fn a_note_sealed_the_weak_way_is_not_reported_as_a_problem() {
    // Notes are allowed the stand-in on purpose. Listing them would bury the
    // credentials that actually matter.
    let mut v = opened();
    v.secrets.push(atlas::vault::Secret {
        name: "shopping".into(),
        kind: Kind::Note,
        sealed: vec![1, 2, 3],
        added: 0,
        last_used: 0,
        real: false,
    });
    assert!(v.weakly_sealed().is_empty());
}

#[test]
fn nothing_writes_with_the_stand_in_cipher_any_more() {
    // Replaces a test that measured the XOR's weakness by writing two secrets
    // with it. Nothing writes that way now, so the measurement became
    // unreachable. What is worth holding instead: every new secret is marked
    // as really sealed, so `weakly_sealed()` can only ever name entries that
    // predate the fix.
    let mut v = opened();
    v.put("a", Kind::Login, "value", 0).unwrap();
    v.put("b", Kind::Note, "value", 0).unwrap();
    assert!(v.secrets.iter().all(|s| s.real), "something was written with the stand-in cipher");
}

// ---------------------------------------------------------------------------
// Real encryption, on every platform.
//
// The previous state: `real_crypto_here()` was `cfg!(windows)`, the only real
// path was Windows data protection, and `put` fell through to a repeating-key
// XOR anyway. Atlas targets every platform, so that combination meant secrets
// were fake-sealed on Windows and refused entirely on macOS and Linux.
//
// Now: Argon2id for the key, XChaCha20-Poly1305 for the secret, identical
// everywhere, no OS integration to keep working.
// ---------------------------------------------------------------------------

#[test]
fn a_credential_stores_and_returns_on_this_platform_whatever_it_is() {
    let mut v = opened();
    v.put("bank", Kind::Login, "correct horse battery staple", 0).unwrap();
    assert_eq!(v.get("bank", 0).unwrap(), "correct horse battery staple");
    assert!(v.weakly_sealed().is_empty(), "stored under the stand-in cipher");
}

#[test]
fn identical_secrets_do_not_produce_identical_ciphertext() {
    // The property the old cipher lacked. Two secrets with the same value
    // under one key produced byte-identical output, so anyone reading the file
    // learned they matched -- and XORing any two cancelled the key entirely.
    let mut v = opened();
    v.put("a", Kind::Login, "AAAAAAAAAAAAAAAA", 0).unwrap();
    v.put("b", Kind::Login, "AAAAAAAAAAAAAAAA", 0).unwrap();
    let a = &v.secrets.iter().find(|s| s.name == "a").unwrap().sealed;
    let b = &v.secrets.iter().find(|s| s.name == "b").unwrap().sealed;
    assert_ne!(a, b, "same plaintext gave same ciphertext -- the key is reused");
}

#[test]
fn a_tampered_secret_fails_to_open_rather_than_decrypting_to_rubbish() {
    // The other thing the old cipher lacked: no authentication tag, so a
    // flipped bit in a stored password was undetectable and came back as a
    // slightly wrong password.
    let mut v = opened();
    v.put("bank", Kind::Login, "correct horse battery staple", 0).unwrap();
    let s = v.secrets.iter_mut().find(|s| s.name == "bank").unwrap();
    let last = s.sealed.len() - 1;
    s.sealed[last] ^= 0x01;
    let e = v.get("bank", 0).unwrap_err();
    assert!(e.contains("altered") || e.contains("open"), "got: {e}");
}

#[test]
fn the_wrong_passphrase_does_not_open_the_vault() {
    let mut v = opened();
    v.put("bank", Kind::Login, "correct horse battery staple", 0).unwrap();
    let salt = v.salt.clone();
    let secrets = v.secrets.clone();

    let mut other = Vault::default();
    other.salt = salt;
    other.secrets = secrets;

    // This test has always been named for the property and asserted
    // something weaker: it `unwrap()`ed the open — expecting it to *succeed*
    // — and only checked that reading a secret then failed. `open` derived a
    // key from whatever you typed and returned `Ok`, so "the vault is open"
    // meant "a key was derived" and the wrong passphrase was indistinguishable
    // from the right one until something failed to decrypt somewhere else.
    //
    // Worse than a confusing error: an "open" vault under the wrong key would
    // seal *new* secrets with it, and the real ones became unreadable with no
    // sign of when it happened.
    //
    // The vault carries a sealed check value now, so the name is true.
    let e = other
        .open("a completely different sentence", 0, &VaultConfig::default())
        .expect_err("the wrong passphrase opened the vault");
    assert!(e.contains("passphrase"), "the refusal does not say what was wrong: {e}");
    assert_eq!(other.state(), atlas::vault::State::Sealed, "it was left open anyway");
    assert!(other.get("bank", 0).is_err(), "the wrong passphrase opened a secret");
}

#[test]
fn an_older_vault_with_no_check_value_still_opens_and_gains_one() {
    // A vault written before the check existed has secrets and no verifier.
    // It must keep working, and it must not stay unverifiable forever.
    let mut v = opened();
    v.put("bank", Kind::Login, "correct horse battery staple", 0).unwrap();

    // Built in the old format on purpose, rather than by copying fields out
    // of a new-format vault. The two used to be the same thing, so copying
    // worked; now that they are not, copying would produce a vault whose
    // secrets are sealed under a key the old path could never derive, and
    // this test would be asserting that a nonsense vault fails to open.
    let mut old = Vault::pre_envelope_for_test(
        PASSPHRASE,
        &[("bank", Kind::Login, "correct horse battery staple")],
        0,
        &VaultConfig::default(),
    )
    .unwrap();
    old.check.clear();
    assert!(old.wraps.is_empty(), "the fixture is not in the old format");

    old.open(PASSPHRASE, 0, &VaultConfig::default()).expect("the right passphrase was refused");
    assert_eq!(old.get("bank", 0).unwrap(), "correct horse battery staple");
    assert!(!old.check.is_empty(), "it opened and did not write a verifier, so the next open is unverified too");

    // And the wrong one is refused against a real secret, before the check
    // value exists.
    let mut old2 = Vault::pre_envelope_for_test(
        PASSPHRASE,
        &[("bank", Kind::Login, "correct horse battery staple")],
        0,
        &VaultConfig::default(),
    )
    .unwrap();
    old2.check.clear();
    assert!(old2.open("a completely different sentence", 0, &VaultConfig::default()).is_err());
}

#[test]
fn a_vault_with_no_passphrase_says_so_rather_than_accepting_anything() {
    // The honest edge: on a fresh vault the first unlock *chooses* the
    // passphrase rather than checking one, so an open proves nothing about
    // who typed it. Anything treating an unlock as proof has to ask first.
    let fresh = Vault::default();
    assert!(!fresh.has_a_passphrase());

    let mut v = Vault::default();
    v.open(PASSPHRASE, 0, &VaultConfig::default()).unwrap();
    assert!(v.has_a_passphrase(), "after the first unlock there is something to check against");
}

#[test]
fn the_same_passphrase_gives_different_keys_on_different_installs() {
    // The old `derive` had no salt, so one passphrase produced one key
    // everywhere -- precompute once, open every Atlas that used it.
    let mut a = Vault::default();
    a.open(PASSPHRASE, 0, &VaultConfig::default()).unwrap();
    let mut b = Vault::default();
    b.open(PASSPHRASE, 0, &VaultConfig::default()).unwrap();
    assert_ne!(a.salt, b.salt, "two installs generated the same salt");

    a.put("x", Kind::Login, "value", 0).unwrap();
    // b cannot read a's secret, because the salt differs.
    b.secrets = a.secrets.clone();
    assert!(b.get("x", 0).is_err(), "a secret opened under a different install's key");
}

#[test]
fn the_salt_is_stored_so_the_vault_reopens_after_a_restart() {
    // The salt is not secret, and losing it would make every stored secret
    // unrecoverable. It has to survive serialisation.
    let mut v = opened();
    v.put("bank", Kind::Login, "correct horse battery staple", 0).unwrap();
    let yaml = serde_yaml::to_string(&v).unwrap();
    let mut back: Vault = serde_yaml::from_str(&yaml).unwrap();
    assert!(!back.salt.is_empty(), "the salt did not survive being written out");
    back.open(PASSPHRASE, 0, &VaultConfig::default()).unwrap();
    assert_eq!(back.get("bank", 0).unwrap(), "correct horse battery staple");
}

#[test]
fn the_key_is_never_written_out() {
    let mut v = opened();
    v.put("bank", Kind::Login, "correct horse battery staple", 0).unwrap();
    let yaml = serde_yaml::to_string(&v).unwrap();
    assert!(!yaml.contains("correct horse"), "the plaintext was serialised");
    // Matched as a whole field name rather than as a substring. `contains`
    // was the check here, and `sealed_key:` -- the wrapped copy of the data
    // key, which is *meant* to be written out -- contains "key:", so this
    // guard failed on a change that was not the failure it guards against.
    // The tree's standing caution about substring guards, caught once more.
    let fields: Vec<&str> = yaml
        .lines()
        .filter_map(|l| l.trim().split(':').next())
        .collect();
    assert!(!fields.contains(&"key"), "the derived key was serialised:\n{yaml}");
}

#[test]
fn deriving_a_key_is_slow_enough_to_be_worth_something() {
    // The old KDF advertised 600,000 rounds and collapsed to a single affine
    // map, so an attacker paid one step per guess. This asserts the real cost
    // is actually being paid -- generously, so a fast machine doesn't fail it.
    let start = std::time::Instant::now();
    let mut v = Vault::default();
    v.open(PASSPHRASE, 0, &VaultConfig::default()).unwrap();
    let took = start.elapsed();
    assert!(took.as_millis() >= 15, "key derivation took {took:?} -- too cheap to slow anyone down");
}

#[test]
fn the_passphrase_can_be_changed_and_everything_survives_it() {
    // Until this existed there was no way to change a passphrase at all --
    // and the passphrase is the only thing in this codebase that proves who
    // is typing, so "somebody watched me type it" had no answer.
    let cfg = VaultConfig::default();
    let mut v = Vault::default();
    v.open("the first passphrase here", 100, &cfg).unwrap();
    v.put("mail", Kind::Login, "hunter2", 100).unwrap();
    v.put("a note", Kind::Note, "the spare key is under the mat", 100).unwrap();
    let salt_before = v.salt.clone();

    v.change_passphrase("the first passphrase here", "a different passphrase now", 200, &cfg)
        .expect("the change should have gone through");

    // Everything is still there, and readable.
    assert_eq!(v.get("mail", 200).unwrap(), "hunter2");
    assert_eq!(v.get("a note", 200).unwrap(), "the spare key is under the mat");

    // And it is genuinely under the new key, not merely reported as such.
    assert_ne!(v.salt, salt_before, "the salt was reused, so the key is a function of an old value");
    v.lock();
    assert!(
        v.open("the first passphrase here", 300, &cfg).is_err(),
        "the old passphrase still opens it"
    );
    v.open("a different passphrase now", 300, &cfg).expect("the new passphrase does not open it");
    assert!(v.proved_it(), "the new passphrase opened it without proving anything");
    assert_eq!(v.get("mail", 300).unwrap(), "hunter2");
}

#[test]
fn a_failed_change_leaves_the_vault_exactly_as_it_was() {
    // The worst available outcome is a vault that is neither the old
    // passphrase nor the new one. A wrong current passphrase must change
    // nothing at all.
    let cfg = VaultConfig::default();
    let mut v = Vault::default();
    v.open("the first passphrase here", 100, &cfg).unwrap();
    v.put("mail", Kind::Login, "hunter2", 100).unwrap();
    let salt_before = v.salt.clone();
    let check_before = v.check.clone();

    let why = v
        .change_passphrase("not the right one at all", "a different passphrase now", 200, &cfg)
        .unwrap_err();
    assert!(why.contains("isn't the passphrase"), "{why}");
    assert_eq!(v.salt, salt_before, "a refused change replaced the salt");
    assert_eq!(v.check, check_before, "a refused change replaced the check value");

    v.lock();
    v.open("the first passphrase here", 300, &cfg).expect("the original passphrase stopped working");
    assert_eq!(v.get("mail", 300).unwrap(), "hunter2");
}

#[test]
fn a_change_that_fails_half_way_puts_everything_back() {
    // The wrong-current-passphrase case above never reaches the destructive
    // half -- it returns before anything is taken apart, so it cannot tell
    // you whether the rollback works. This one does reach it: the old
    // passphrase is right, the secrets are read out, the salt is replaced,
    // and *then* the new passphrase is refused for being too short.
    //
    // Without the restore, that leaves a vault whose salt and check value
    // belong to a key nobody ever chose: the old passphrase no longer opens
    // it, the new one was never accepted, and every secret in it is gone.
    let cfg = VaultConfig::default();
    let mut v = Vault::default();
    v.open("the first passphrase here", 100, &cfg).unwrap();
    v.put("mail", Kind::Login, "hunter2", 100).unwrap();
    let salt_before = v.salt.clone();
    let check_before = v.check.clone();

    let why = v.change_passphrase("the first passphrase here", "short", 200, &cfg).unwrap_err();
    assert!(why.contains("guessable"), "{why}");

    assert_eq!(v.salt, salt_before, "the salt was left as the one nobody chose");
    assert_eq!(v.check, check_before, "the check value was left mid-change");
    v.lock();
    v.open("the first passphrase here", 300, &cfg)
        .expect("the vault was left openable by neither passphrase");
    assert_eq!(v.get("mail", 300).unwrap(), "hunter2", "the secrets did not come back");
}

#[test]
fn a_vault_with_no_passphrase_is_told_to_set_one_rather_than_change_one() {
    let cfg = VaultConfig::default();
    let mut v = Vault::default();
    let why = v.change_passphrase("anything at all here", "something else here", 100, &cfg).unwrap_err();
    assert!(why.contains("no passphrase"), "{why}");
}
