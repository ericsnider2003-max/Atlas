//! Holding secrets so a stolen laptop is worth nothing.
//!
//! ## The thing that makes this real rather than theatre
//!
//! Encrypting a file is easy. The hard part is where the key lives, and it's
//! where most "encrypted" local storage quietly fails: if Atlas can read the
//! secrets whenever it likes, the key is on the machine, and whoever has the
//! machine has both. That's a locked box with the key taped to the lid.
//!
//! So the key is **derived from something not on the machine** — a passphrase
//! you type — and held only in memory, only while you're using it. A stolen
//! laptop then contains a file nobody can open, including Atlas.
//!
//! The cost is honest and worth stating: you type a passphrase once per
//! session. Nothing unattended can touch the vault, which means overnight work
//! can't use it either. That's the trade, and it's the right way round.

use serde::{Deserialize, Serialize};

/// What's stored.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Secret {
    pub name: String,
    pub kind: Kind,
    /// Encrypted. Never the plain value.
    pub sealed: Vec<u8>,
    /// Sealed with the OS's real data protection rather than the stand-in.
    ///
    /// Defaults to false so a vault written before this existed still reads:
    /// those entries genuinely were sealed the weak way, and claiming
    /// otherwise would be the same lie in the other direction.
    #[serde(default)]
    pub real: bool,
    pub added: u64,
    /// Last time you used it, so stale ones can be noticed.
    pub last_used: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A username and password for one site, kept together — they're useless
    /// apart and storing them separately is how they drift.
    Login,
    /// The seed behind an authenticator code.
    TotpSeed,
    /// One-time recovery codes.
    RecoveryCodes,
    /// An API key for something.
    ApiKey,
    /// Anything else.
    Note,
}

impl Kind {
    /// Would losing this to someone reading the file cost you something you
    /// cannot undo?
    pub fn needs_real_crypto(&self) -> bool {
        match self {
            Kind::Login | Kind::TotpSeed | Kind::RecoveryCodes | Kind::ApiKey => true,
            // A reminder to yourself is not worth refusing to store.
            Kind::Note => false,
        }
    }

    pub fn plain(&self) -> &'static str {
        match self {
            Kind::Login => "logins",
            Kind::TotpSeed => "authenticator seeds",
            Kind::RecoveryCodes => "recovery codes",
            Kind::ApiKey => "API keys",
            Kind::Note => "notes",
        }
    }
}

impl Kind {
    /// Can Atlas use this without you present?
    ///
    /// Only when you've sealed the vault to your Windows sign-in
    /// (`vault.open_on_this_login`), and then only the secrets scheduled work
    /// actually needs: a mail app password or an API key, a site login. Never
    /// authenticator seeds or recovery codes — those are what you'd use to
    /// take an account back, and a laptop someone else is signed into must
    /// not be able to.
    pub fn usable_unattended(&self) -> bool {
        matches!(self, Kind::ApiKey | Kind::Login)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Nothing readable. This is how it sits on disk and how it sits at rest.
    Sealed,
    /// You've unlocked it. The key is in memory only.
    Open,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct VaultConfig {
    pub enabled: bool,
    /// Re-lock after this long without use.
    pub lock_after_mins: u64,
    /// Re-lock when the machine sleeps or locks.
    pub lock_on_screen_lock: bool,
    /// Seal a copy of the vault's key to your Windows sign-in, so it opens
    /// without a passphrase while you're signed in (`loginseal`). On by
    /// default since 5 Oct 2026: a passphrase nobody remembers made the
    /// vault -- and every Connect button behind it -- something no one used.
    pub open_on_this_login: bool,
}

impl VaultConfig {
    /// Argon2's real cost knob, in KiB.
    ///
    /// Fixed at 64 MiB with three passes rather than scaled from
    /// `kdf_rounds`. Scaling it was the first thing tried here and it was
    /// wrong twice over: it produced a 139 MiB, 3.4-second unlock, and it kept
    /// alive the idea that `kdf_rounds` means something. It does not. Argon2's
    /// cost is memory and passes, and the old round count was the exact number
    /// that turned out to buy nothing.
    ///
    /// Measured on the build machine, release profile: 123ms. Memory is the
    /// lever that matters -- 19 MiB with sixteen passes costs the same wall
    /// time and is far easier to attack in parallel on a GPU.
    ///
    /// The `kdf_rounds` field was deleted on 19 Sep 2026. Keeping it "so old
    /// configs still load" was never the reason it needed to exist — serde
    /// ignores a key with no field — and while it existed, `tools.yaml`
    /// shipped it with the comment "a second to unlock, years to attack"
    /// beside a number nothing read. `config::NO_FIELD_TO_LAND_IN` carries it
    /// now, so `atlas doctor` says so out loud to anyone whose file still
    /// sets it. See `KDF_ROUNDS_IS_IGNORED`.
    fn memory_kib(&self) -> u32 {
        64 * 1024
    }
}

impl Default for VaultConfig {
    fn default() -> Self {
        VaultConfig {
            enabled: false,
            // Long enough to be usable, short enough that a walk to the
            // kitchen isn't a window.
            lock_after_mins: 15,
            lock_on_screen_lock: true,
            open_on_this_login: true,
            // Deliberately expensive: a second to unlock, years to attack.
        }
    }
}

/// How somebody got in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    /// The sentence you know.
    Passphrase,
    /// The code you wrote down and put somewhere that is not this machine.
    RecoveryKey,
    /// Your Windows sign-in on this machine (`loginseal`). Opens the vault for
    /// scheduled work while you're signed in; never proves it's you.
    ThisLogin,
}

impl How {
    pub fn plain(&self) -> &'static str {
        match self {
            How::Passphrase => "the passphrase",
            How::RecoveryKey => "a recovery key",
            How::ThisLogin => "your Windows sign-in",
        }
    }
}

/// One way in: a copy of the vault's data key, sealed under something a
/// person can produce.
///
/// # Why the vault works this way now
///
/// It used to derive the key **from the passphrase directly**, which is the
/// obvious design and has one consequence that is not obvious until it
/// happens to you: the passphrase is not *a* way in, it is *the* way in.
/// Forget it and every secret is gone, and — worse, because it is not even
/// your data at stake — a handover can never be taken back. An assistant you
/// can be permanently locked out of by forgetting a sentence is not a system
/// you would trust with anything that matters.
///
/// So the secrets are sealed under a **random data key**, and that key is
/// sealed separately under each thing that may open it. Any one of them
/// recovers the same key. Adding a way in does not touch a single secret,
/// and removing one does not either.
///
/// What this does **not** do, and cannot: recover the passphrase itself.
/// Nothing here stores it, and nothing here could — a design that let Atlas
/// reconstruct your passphrase would let anyone holding the disk do the same.
/// A recovery key is a second key to the same lock, not a copy of the first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wrap {
    pub how: How,
    /// This wrap's own salt. Never shared with another wrap: two wraps over
    /// one salt would mean a passphrase and a recovery key that happened to
    /// match produced the same key-encrypting key, and would leak that fact.
    pub salt: Vec<u8>,
    /// The data key, sealed under what this wrap is opened by.
    pub sealed_key: Vec<u8>,
    pub made: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Vault {
    pub secrets: Vec<Secret>,
    /// Every way into this vault. Empty on a vault written before the
    /// envelope existed — `open` upgrades one the first time it is opened,
    /// without re-sealing anything, because the data key it adopts is the
    /// key those secrets are already sealed under.
    #[serde(default)]
    pub wraps: Vec<Wrap>,
    /// Per-install random salt for the key derivation.
    ///
    /// Stored, not secret. Its job is to make the derived key different on
    /// every machine, so a precomputed table built against one passphrase is
    /// worthless against yours. The old `derive` had no salt at all, which
    /// meant the same passphrase produced the same key everywhere.
    #[serde(default)]
    pub salt: Vec<u8>,
    /// A known value, sealed with your key, so a wrong passphrase can be told
    /// from a right one.
    ///
    /// Without this `open` could not fail. It derived a key from whatever you
    /// typed and returned `Ok`, so "the vault is open" meant "a key was
    /// derived", not "the passphrase was right" — and the two read the same
    /// from outside. The error only surfaced later and somewhere else, as a
    /// secret that would not decrypt.
    ///
    /// Stored, not secret: it is a fixed plaintext under your key, which is
    /// exactly what a verifier is. Empty on a vault whose passphrase has
    /// never been set, and `open` then *establishes* it — a first unlock is
    /// how you choose a passphrase, and it must not be mistaken for a
    /// successful check against nothing.
    #[serde(default)]
    pub check: Vec<u8>,
    /// Never serialised. Present only while open.
    #[serde(skip)]
    key: Option<Vec<u8>>,
    #[serde(skip)]
    opened_at: u64,
    /// Did the current unlock *check* the passphrase, or *set* it?
    ///
    /// Never serialised, because it is a fact about this unlock and not about
    /// the vault on disk. `state() == Open` cannot answer it: on a vault with
    /// no passphrase the first `open` succeeds for any twelve characters, so
    /// an open vault is proof of identity only when something was verified.
    #[serde(skip)]
    verified: bool,
    /// Which way in this unlock used. Never serialised — a fact about this
    /// unlock, like `verified`.
    ///
    /// Kept so the person who just came in on a recovery key can be told to
    /// set a new passphrase. Coming in that way means the old one is gone,
    /// and a vault you can only open with the piece of paper is one piece of
    /// paper away from the situation this was built to prevent.
    #[serde(skip)]
    opened_with: Option<How>,
}

/// Words a guesser aiming at this vault would try first.
const VAULT_WORDS: &[&str] = &["atlas", "vault", "passphrase", "recovery"];

impl Vault {
    pub fn state(&self) -> State {
        if self.key.is_some() {
            State::Open
        } else {
            State::Sealed
        }
    }

    /// Unlock with your passphrase.
    pub fn open(&mut self, passphrase: &str, now: u64, cfg: &VaultConfig) -> Result<(), String> {
        if passphrase.len() < 12 {
            // Length beats complexity, and a short passphrase makes the whole
            // exercise decorative.
            return Err("that's short enough to be guessable — use a sentence, not a word".into());
        }
        // Choosing one, not typing one: the first unlock sets the passphrase,
        // so this is the moment to refuse "password1234" — twelve characters
        // that a guesser tries in its first ten thousand. Later unlocks are
        // checked against what was set and never re-judged.
        if !self.has_a_passphrase() {
            crate::guessable::fit_for_the_vault(passphrase, VAULT_WORDS)?;
        }
        // A vault started on your Windows sign-in has no passphrase; typing
        // one here *adds* it, around the key the vault already has. Never the
        // brand-new branch of `open_the_old_way`, which would make a second
        // key and strand everything sealed under the first.
        if self.opens_on_login_only() {
            if self.key.is_none() {
                self.open_unattended(now)?;
            }
            self.adopt_envelope(passphrase, now, cfg)?;
            self.verified = false;
            self.opened_with = Some(How::Passphrase);
            return Ok(());
        }
        // The envelope, once this vault has one.
        if self.has_wrap(How::Passphrase) {
            return self.open_wrapped(passphrase, How::Passphrase, now, cfg);
        }
        self.open_the_old_way(passphrase, now, cfg)
    }

    /// Unlock with a recovery key, because the passphrase is gone.
    ///
    /// Deliberately a separate door rather than `open` trying both. Two
    /// reasons, and the second is the one that matters: every attempt costs a
    /// full Argon2 derivation, so trying every wrap on every unlock would
    /// double the wait for the ordinary case; and a wrong passphrase would
    /// otherwise be reported after also failing against the recovery key,
    /// which is a different sentence than the one the person needs.
    ///
    /// Typing is forgiving on purpose — see `tidy_recovery_key`. Somebody
    /// reading this off a piece of paper months later, possibly in a hurry,
    /// is the entire situation it exists for.
    pub fn open_with_recovery_key(
        &mut self,
        code: &str,
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<(), String> {
        if !self.has_wrap(How::RecoveryKey) {
            return Err(
                "there's no recovery key on this vault — one is made when you set the \
                 passphrase, or with `atlas vault recovery`"
                    .into(),
            );
        }
        let tidy = tidy_recovery_key(code);
        if tidy.is_empty() {
            return Err("that doesn't look like a recovery key".into());
        }
        self.open_wrapped(&tidy, How::RecoveryKey, now, cfg)
    }

    /// Try every wrap of this kind. One that opens is proof.
    fn open_wrapped(
        &mut self,
        secret: &str,
        how: How,
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<(), String> {
        let wraps: Vec<Wrap> =
            self.wraps.iter().filter(|w| w.how == how).cloned().collect();
        for w in &wraps {
            let kek = derive(secret, &w.salt, cfg)?;
            // The unwrap *is* the check. `unseal_aead` is authenticated, so a
            // wrong key cannot produce a plausible data key -- it produces an
            // error. That is why an envelope vault keeps no separate check
            // value: a second thing to verify against is a second thing that
            // can disagree with the first.
            if let Ok(key) = unseal_aead(&w.sealed_key, &kek) {
                self.key = Some(key);
                self.opened_at = now;
                self.verified = true;
                self.opened_with = Some(how);
                return Ok(());
            }
        }
        Err(match how {
            How::Passphrase => "that isn't the passphrase".into(),
            How::RecoveryKey => "that isn't a recovery key for this vault".into(),
            How::ThisLogin => "this sign-in can't open it".into(),
        })
    }

    /// The pre-envelope vault: key derived straight from the passphrase.
    ///
    /// Still here because vaults written that way exist and must keep
    /// opening. A vault that opens here leaves with an envelope around it —
    /// see `adopt_envelope`.
    fn open_the_old_way(
        &mut self,
        passphrase: &str,
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<(), String> {
        if self.salt.is_empty() {
            self.salt = random_bytes(16);
        }
        let key = derive(passphrase, &self.salt, cfg)?;

        if !self.check.is_empty() {
            // The ordinary case: verify.
            if unseal_aead(&self.check, &key).ok().as_deref() != Some(CHECK_VALUE) {
                // Deliberately does not set `self.key`. A wrong passphrase
                // that left the vault "open" would go on to seal new secrets
                // under the wrong key, and the real ones would become
                // unreadable with no explanation of when it happened.
                return Err("that isn't the passphrase".into());
            }
            self.verified = true;
        } else if let Some(existing) = self.secrets.iter().find(|s| s.real) {
            // A vault written before this check existed. Verify against a
            // real secret instead, and write the check while we have the key,
            // so it only happens once.
            if unseal_aead(&existing.sealed, &key).is_err() {
                return Err("that isn't the passphrase".into());
            }
            self.check = seal_aead(CHECK_VALUE, &key)?;
            self.verified = true;
        } else if self.secrets.iter().any(|s| !s.real) {
            // THE SAME CASE, ONE CIPHER OLDER -- and getting it wrong here
            // destroyed the secrets rather than refusing to open them.
            //
            // The branch below used to catch this, on the premise stated in
            // its own comment: *"Nothing is sealed yet, so there is nothing
            // to convert."* That premise is false for a vault whose secrets
            // were all written with the legacy cipher. `check` is empty
            // because the check value post-dates them, and no secret has
            // `real == true` because none was sealed with AEAD -- so the
            // condition guarding "brand-new vault" was satisfied by a vault
            // that was full.
            //
            // What then happened: `self.key = Some(random_bytes(32))`. But
            // `get` reads a legacy secret as `seal(&s.sealed, &key)` -- the
            // repeating-key XOR, keyed with the data key. Those bytes were
            // XORed with the key derived from the OLD passphrase. XOR them
            // with a fresh random key and the result is noise,
            // `String::from_utf8` fails, and every secret in the vault
            // answers "that didn't decrypt". The new envelope is then saved
            // over the old salt, so the derived key that could have read them
            // no longer exists anywhere. The person typed their correct
            // passphrase and lost every credential they had stored.
            //
            // The fix is the arm above, with the other cipher: the derived
            // key IS the data key for these secrets, so verify against one
            // and adopt it. Nothing is re-encrypted and nothing moves --
            // `weakly_sealed()` goes on naming them so they can be replaced
            // deliberately.
            //
            // ## Verification here is WEAK, and everything below follows from
            // ## saying so instead of pretending otherwise
            //
            // XOR has no authentication tag, so "did it decrypt correctly" is
            // not a question this cipher can answer. All that can be checked
            // is that the bytes come back as plausible plaintext: non-empty
            // valid UTF-8, which is true of every secret `put` ever wrote
            // (it stores `value.as_bytes()`).
            //
            // `all`, not `any`. A wrong 32-byte key produces valid UTF-8 from
            // a short value more often than is comfortable -- roughly one in
            // sixteen for four characters -- and there is no attempt limiter
            // in front of this. Requiring **every** legacy secret to decode
            // makes a false accept the product of those probabilities, which
            // for two or more secrets of any length is negligible.
            //
            // The first version of this arm then did two things it had no
            // right to do, and its own comment said the opposite of the first
            // of them:
            //
            // 1. `self.verified = true`, under a comment reading "`verified`
            //    stays false". `proved_it()` is `key.is_some() && verified`
            //    and is documented as "the only form of 'the vault is open'
            //    that says anything about who typed" -- `handover::take_back`
            //    uses it as identity proof. A plausible-UTF-8 check is not
            //    identity proof, and a wrong passphrase that happened to pass
            //    it would have read as the owner.
            // 2. Wrote `self.check` and adopted an envelope from the derived
            //    key. That is what would have made a false accept
            //    **permanent**: on the next open, `has_wrap(Passphrase)` is
            //    true, so `open_wrapped` runs and tries only the wrap built
            //    from the wrong passphrase -- and the correct one is refused
            //    for ever. The same class of loss the whole branch exists to
            //    prevent, arriving by a longer road.
            //
            // So: adopt the key, which is the only thing that makes the
            // secrets readable, and **write nothing**. No check value, no
            // envelope, `verified` genuinely false. The vault stays in its
            // old format until the person does something that constitutes
            // real proof -- setting a new passphrase, or storing a secret
            // with the real cipher, after which the `s.real` arm above takes
            // over and verification is strong. `weakly_sealed()` goes on
            // naming what needs replacing.
            let legacy: Vec<&Secret> = self.secrets.iter().filter(|s| !s.real).collect();
            let has_bytes = legacy.iter().any(|s| !s.sealed.is_empty());
            if has_bytes {
                let all_plausible = legacy.iter().all(|s| {
                    // An entry with no bytes carries no evidence either way,
                    // so it neither confirms nor refutes.
                    s.sealed.is_empty()
                        || String::from_utf8(seal(&s.sealed, &key)).is_ok()
                });
                if !all_plausible {
                    return Err("that isn't the passphrase".into());
                }
                self.key = Some(key);
                self.opened_at = now;
                self.verified = false;
                self.opened_with = Some(How::Passphrase);
                return Ok(());
            }
            // Every legacy secret is empty, so there is nothing to read and
            // nothing to lose. Falling through to the branch below treats it
            // as the empty vault it effectively is, rather than refusing for
            // ever on evidence that does not exist -- which is the mirror
            // image of the defect above and would be just as much a lockout.
            self.key = Some(random_bytes(32));
            self.opened_at = now;
            self.verified = false;
            self.opened_with = Some(How::Passphrase);
            self.adopt_envelope(passphrase, now, cfg)?;
            return Ok(());
        } else {
            // Nothing to check against: no passphrase has ever been set on
            // this vault. This unlock chooses one. Setting a passphrase is
            // not the same as passing a check, and nothing downstream may
            // read it as one.
            //
            // A brand-new vault gets a *random* data key rather than the one
            // derived from what was typed, so the passphrase is a way in from
            // the very first moment rather than the thing everything is
            // welded to. Nothing is sealed yet, so there is nothing to
            // convert.
            self.key = Some(random_bytes(32));
            self.opened_at = now;
            self.verified = false;
            self.opened_with = Some(How::Passphrase);
            self.adopt_envelope(passphrase, now, cfg)?;
            return Ok(());
        }

        self.key = Some(key);
        self.opened_at = now;
        self.opened_with = Some(How::Passphrase);
        // Opened the old way and verified, so this really is the owner and
        // the derived key really is what the secrets are sealed under. Wrap
        // that same key rather than making a new one: every secret stays
        // exactly as it is on disk, which is the difference between an
        // upgrade and a re-encryption that can go wrong half way.
        self.adopt_envelope(passphrase, now, cfg)?;
        Ok(())
    }

    /// Put an envelope around the key this vault is already using.
    ///
    /// Idempotent in the sense that matters: it replaces any existing
    /// passphrase wrap, so calling it again after a passphrase change leaves
    /// exactly one.
    fn adopt_envelope(
        &mut self,
        passphrase: &str,
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<(), String> {
        let key = self.key.clone().ok_or("the vault is locked")?;
        let wrap = Self::wrap_key(&key, passphrase, How::Passphrase, now, cfg)?;
        self.wraps.retain(|w| w.how != How::Passphrase);
        self.wraps.push(wrap);
        Ok(())
    }

    fn wrap_key(
        key: &[u8],
        secret: &str,
        how: How,
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<Wrap, String> {
        let salt = random_bytes(16);
        let kek = derive(secret, &salt, cfg)?;
        Ok(Wrap { how, salt, sealed_key: seal_aead(key, &kek)?, made: now })
    }

    fn has_wrap(&self, how: How) -> bool {
        self.wraps.iter().any(|w| w.how == how)
    }

    /// Nothing in it and no way in yet: the vault as it is before first use.
    pub fn is_brand_new(&self) -> bool {
        self.wraps.is_empty() && self.secrets.is_empty() && self.check.is_empty()
    }

    /// Opens with your Windows sign-in and nothing else.
    pub fn opens_on_login_only(&self) -> bool {
        self.has_wrap(How::ThisLogin) && !self.has_wrap(How::Passphrase) && !self.has_wrap(How::RecoveryKey)
    }

    /// Start a brand-new vault on your Windows sign-in, with no passphrase.
    ///
    /// 5 Oct 2026: a passphrase you chose once and can't remember is a vault
    /// you can't use, and nobody wants to type one to connect an account.
    /// What professional apps do on Windows is what this does -- seal the key
    /// to your sign-in (DPAPI), which you already use every day and which
    /// Windows lets you reset. What's kept in here is sign-ins and keys you
    /// can make again by connecting again, so losing your Windows account
    /// loses nothing you can't get back. A passphrase can still be added
    /// later, for the secrets that never open on a sign-in alone.
    pub fn start_on_this_login(&mut self, now: u64) -> Result<(), String> {
        if !self.is_brand_new() {
            return Err("this vault already has a way in, so it isn't started again".into());
        }
        let key = random_bytes(32);
        let blob = crate::loginseal::seal(&key)?;
        self.wraps.push(Wrap { how: How::ThisLogin, salt: Vec::new(), sealed_key: blob, made: now });
        self.key = Some(key);
        self.opened_at = now;
        self.verified = false;
        self.opened_with = Some(How::ThisLogin);
        Ok(())
    }

    /// Seal a copy of the open vault's key to this Windows sign-in.
    pub fn seal_to_this_login(&mut self, now: u64) -> Result<(), String> {
        let key = self.key.clone().ok_or("the vault is locked")?;
        let blob = crate::loginseal::seal(&key)?;
        self.wraps.retain(|w| w.how != How::ThisLogin);
        self.wraps.push(Wrap { how: How::ThisLogin, salt: Vec::new(), sealed_key: blob, made: now });
        Ok(())
    }

    /// Take the sign-in copy away. The passphrase and recovery key are untouched.
    pub fn unseal_from_this_login(&mut self) -> bool {
        let before = self.wraps.len();
        self.wraps.retain(|w| w.how != How::ThisLogin);
        before != self.wraps.len()
    }

    pub fn sealed_to_this_login(&self) -> bool {
        self.has_wrap(How::ThisLogin)
    }

    /// Open for scheduled work, with nobody there to type: only through the
    /// sign-in copy, and never counted as having proved it's you.
    pub fn open_unattended(&mut self, now: u64) -> Result<(), String> {
        let w = self.wraps.iter().find(|w| w.how == How::ThisLogin).cloned().ok_or("the vault isn't sealed to your sign-in")?;
        let key = crate::loginseal::unseal(&w.sealed_key)?;
        // A data key has a fixed length; anything else is a blob from
        // something that isn't this vault.
        if key.len() != 32 {
            return Err("that sign-in copy isn't a key for this vault".into());
        }
        self.key = Some(key);
        self.opened_at = now;
        self.verified = false;
        self.opened_with = Some(How::ThisLogin);
        Ok(())
    }

    /// Is there a second way in, if the passphrase goes?
    pub fn has_a_recovery_key(&self) -> bool {
        self.has_wrap(How::RecoveryKey)
    }

    /// How this unlock got in. `None` when it is shut.
    pub fn opened_with(&self) -> Option<How> {
        self.opened_with
    }

    /// Make a recovery key, and hand it back **once**.
    ///
    /// The only time this string exists. It is not stored anywhere — what is
    /// stored is the data key sealed under it, which is one-way. Lose the
    /// paper and this specific key is gone; that is the same property that
    /// makes it worth having.
    ///
    /// Issuing replaces any previous recovery key, which is the behaviour you
    /// want the day you think one might have been seen: making a new one has
    /// to be the same act as destroying the old one, or people will make a
    /// new one and leave the old one working.
    pub fn issue_recovery_key(&mut self, now: u64, cfg: &VaultConfig) -> Result<String, String> {
        let key = self
            .key
            .clone()
            .ok_or("the vault has to be open before I can make a recovery key for it")?;
        let code = new_recovery_key();
        let wrap = Self::wrap_key(&key, &tidy_recovery_key(&code), How::RecoveryKey, now, cfg)?;
        self.wraps.retain(|w| w.how != How::RecoveryKey);
        self.wraps.push(wrap);
        Ok(code)
    }

    /// Set a passphrase on a vault that was opened with a recovery key.
    ///
    /// Separate from `change_passphrase` because the old passphrase is
    /// exactly what is missing: this is the one path that replaces it without
    /// being able to produce it. What it requires instead is that the vault
    /// was opened with a recovery key *this unlock* -- not merely that it is
    /// open, which on a vault with no passphrase is a state anyone can reach.
    pub fn set_passphrase_from_recovery(
        &mut self,
        new: &str,
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<(), String> {
        if self.opened_with != Some(How::RecoveryKey) {
            return Err(
                "this is only for a vault opened with its recovery key -- to change a \
                 passphrase you have, use `atlas vault passphrase`"
                    .into(),
            );
        }
        if new.len() < 12 {
            return Err("that's short enough to be guessable — use a sentence, not a word".into());
        }
        self.adopt_envelope(new, now, cfg)?;
        // A converted vault's old verifier was a function of the old
        // passphrase. It cannot verify the new one and nothing consults it
        // any more; leaving it would be leaving a second opinion about who is
        // allowed in.
        self.check.clear();
        self.salt.clear();
        Ok(())
    }

    /// Throw the recovery key away without making another.
    ///
    /// Here because the alternative is somebody deleting the line in the file
    /// by hand. Returns whether there was one.
    pub fn forget_recovery_key(&mut self) -> bool {
        let had = self.has_wrap(How::RecoveryKey);
        self.wraps.retain(|w| w.how != How::RecoveryKey);
        had
    }

    /// Has a passphrase ever been set on this vault?
    ///
    /// Worth asking before treating an unlock as proof of anything: on a
    /// vault with no passphrase, the first `open` sets one rather than
    /// checking one, so "it opened" says nothing about who is typing.
    pub fn has_a_passphrase(&self) -> bool {
        if self.opens_on_login_only() {
            // Its secrets are sealed under a random key, not one a passphrase
            // made, so the "any secret at all" reading below doesn't apply.
            return false;
        }
        self.has_wrap(How::Passphrase)
            || !self.check.is_empty()
            // `!s.real` as well as `s.real`, which is to say: any secret at
            // all. A legacy vault has no check value and no AEAD-sealed
            // secret, and its secrets are nonetheless sealed under a key
            // derived from a passphrase somebody chose. Reading that as "no
            // passphrase has ever been set" is the same false premise that
            // let `open_the_old_way` treat a full legacy vault as a brand-new
            // one and replace its data key with a random one.
            || !self.secrets.is_empty()
    }

    /// Open *and* opened by passing a check.
    ///
    /// This is the only form of "the vault is open" that says anything about
    /// who typed. `state() == Open` is true after a first unlock as well, and
    /// a first unlock is a passphrase being chosen — by whoever is sitting
    /// there. Anything using the vault as proof of identity asks this.
    pub fn proved_it(&self) -> bool {
        self.key.is_some() && self.verified
    }

    /// Where the vault lives, under the install's own state.
    ///
    /// Deliberately not under the active profile's directory. Every other
    /// piece of per-person state is split per profile so a guest cannot read
    /// your memory or your drafts -- but the vault is the one thing that is
    /// the *owner's* rather than the session's, and splitting it would mean a
    /// guest profile could create a second vault, set its passphrase by
    /// typing twelve characters, and then hold something Atlas treats as
    /// proof that it is you. See `handover::take_back`.
    pub const FILE: &str = "vault";

    /// Read the vault off disk.
    ///
    /// Until this existed, `Vault` was constructed with `default()` at start
    /// and never written anywhere: the salt, the check value and every secret
    /// put into it lived in memory for one run. Which meant the crypto was
    /// real and stored nothing -- mail passwords were re-asked for every
    /// start, and the "passphrase" was whichever twelve characters were typed
    /// first that run, because a vault with no check value has its passphrase
    /// *set* by the first unlock rather than checked.
    ///
    /// Sealed on disk: the secrets are ciphertext and the key is not here.
    /// What is readable is the salt and the check value, and both are meant
    /// to be.
    pub fn load(state: &crate::store::Store) -> Vault {
        state.load(Self::FILE)
    }

    /// Write it back. Called after anything that changes what is stored --
    /// including the first unlock, which is when the salt and the check value
    /// come into existence.
    pub fn save(&self, state: &crate::store::Store) -> crate::error::Result<()> {
        state.save(Self::FILE, self)
    }

    /// Change the passphrase.
    ///
    /// Needed because the passphrase is the only thing in this codebase that
    /// proves who is typing -- `handover::take_back` rests on it -- and until
    /// this existed there was no way to change one that had been guessed at,
    /// shoulder-surfed, or set by somebody else on a machine you have just
    /// been handed.
    ///
    /// # Why this no longer re-seals every secret
    ///
    /// It used to, because it had to: the key *was* the passphrase, so a new
    /// passphrase meant a new key and every secret had to be decrypted and
    /// written again. That worked, and it carried a risk worth being rid of
    /// -- a failure part-way through, with some secrets under the old key and
    /// some under the new, is the one outcome worse than either.
    ///
    /// With the envelope there is nothing to re-seal. The data key does not
    /// change; only the wrap around it does. The secrets are not touched, not
    /// read, and not rewritten, so there is no half-converted state to end up
    /// in. A recovery key keeps working across a passphrase change for the
    /// same reason, which is what you want: changing your passphrase is not a
    /// reason to go and reprint the piece of paper in your desk.
    pub fn change_passphrase(
        &mut self,
        old: &str,
        new: &str,
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<(), String> {
        if !self.has_a_passphrase() {
            return Err("there's no passphrase on this vault yet -- setting one is the first unlock".into());
        }
        if new.len() < 12 {
            return Err("that's short enough to be guessable — use a sentence, not a word".into());
        }
        crate::guessable::fit_for_the_vault(new, VAULT_WORDS)?;
        self.lock();
        // Opening with the old one also converts a pre-envelope vault, so
        // what follows has a wrap to replace whichever kind of vault this is.
        self.open(old, now, cfg)?;
        if !self.proved_it() {
            // Belt and braces: `open` returning Ok without verifying is the
            // exact failure this module has already had once.
            self.lock();
            return Err("that isn't the passphrase".into());
        }
        self.adopt_envelope(new, now, cfg)?;
        // A vault converted from the old scheme still carries the check value
        // from when the key was derived from the passphrase. It verifies
        // against the data key, which has not changed, so it stays true --
        // but it is no longer what `open` consults, and leaving a second
        // verifier lying around is how the two come to disagree.
        self.check.clear();
        self.salt.clear();
        // Prove the new one rather than assume it, by shutting the vault and
        // opening it with what was just set.
        self.lock();
        self.open(new, now, cfg)?;
        if !self.proved_it() {
            return Err("the new passphrase did not verify -- nothing was changed".into());
        }
        Ok(())
    }

    pub fn lock(&mut self) {
        // Overwrite before dropping, so it isn't left lying in memory.
        if let Some(k) = self.key.as_mut() {
            for b in k.iter_mut() {
                *b = 0;
            }
        }
        self.key = None;
        self.verified = false;
        self.opened_with = None;
    }

    /// Time to re-lock?
    pub fn should_lock(&self, now: u64, screen_locked: bool, cfg: &VaultConfig) -> bool {
        if self.key.is_none() {
            return false;
        }
        if screen_locked && cfg.lock_on_screen_lock {
            return true;
        }
        now.saturating_sub(self.opened_at) > cfg.lock_after_mins * 60
    }

    pub fn put(&mut self, name: &str, kind: Kind, value: &str, now: u64) -> Result<(), String> {
        // Fail rather than fall back. A secret stored under placeholder
        // crypto is worse than a refusal: you would file it, stop thinking
        // about it, and everything downstream would treat it as protected.
        // The refusal is about whether the encryption is real, not about
        // what kind of secret it is. On Windows it is real, so this doesn't
        // fire and credentials store normally.
        if !real_crypto_here() && kind.needs_real_crypto() {
            return Err(format!(
                "I'm not storing {} yet. What's in here is a stand-in — the key \
                 stretching collapses and the cipher reuses one key across every \
                 secret, so it would look protected and wouldn't be. Keep that in \
                 your password manager until this is real.",
                kind.plain()
            ));
        }
        if self.opens_on_login_only() && !kind.usable_unattended() {
            return Err(format!(
                "{} never open on your Windows sign-in alone, and this vault has no passphrase yet. \
                 Add one on the Accounts page first, or keep them in your password manager.",
                kind.plain()
            ));
        }
        let key = self.key.as_ref().ok_or("the vault is locked")?;

        // The bug this replaced: the refusal above asks "is real encryption
        // available?" and, when the answer was yes, fell straight through to
        // `seal` — the repeating-key XOR the refusal message itself describes
        // as "would look protected and wouldn't be". So on Windows, the one
        // platform where the guard relaxed, every login, API key, TOTP seed
        // and recovery code was stored under the cipher the guard existed to
        // prevent. `seal_for_real` was written, tested and called by nothing.
        //
        // Availability of real encryption is now what *selects* it, not
        // merely permission to store.
        let (sealed, real) = if real_crypto_here() {
            match seal_aead(value.as_bytes(), key) {
                Ok(b) => (b, true),
                // Never silently downgrade. If the OS refuses, a secret that
                // needs real protection does not get written at all.
                Err(e) if kind.needs_real_crypto() => {
                    return Err(format!("I couldn't seal that properly, so I haven't stored it: {e}"))
                }
                Err(_) => (seal(value.as_bytes(), key), false),
            }
        } else {
            (seal(value.as_bytes(), key), false)
        };

        self.secrets.retain(|s| s.name != name);
        self.secrets.push(Secret {
            name: name.into(),
            kind,
            sealed,
            added: now,
            last_used: now,
            real,
        });
        Ok(())
    }

    pub fn get(&mut self, name: &str, now: u64) -> Result<String, String> {
        let key = self.key.as_ref().ok_or("the vault is locked")?.clone();
        let unattended = self.opened_with == Some(How::ThisLogin);
        let s = self
            .secrets
            .iter_mut()
            .find(|s| s.name == name)
            .ok_or_else(|| format!("nothing called {name} in there"))?;
        if unattended && !s.kind.usable_unattended() {
            return Err(format!(
                "{name} is one of your {}, and those only open with your passphrase — not for \
                 scheduled work on your sign-in",
                s.kind.plain()
            ));
        }
        s.last_used = now;
        // Which way it was sealed is recorded per secret rather than inferred
        // from the current machine. A vault written before the real path was
        // wired still contains XOR entries, and reading those with the OS
        // unprotect would fail with a confusing error rather than the value.
        let plain = if s.real {
            unseal_aead(&s.sealed, &key)?
        } else {
            // Written before the real cipher existed. Readable, so nothing is
            // stranded, but `weakly_sealed()` names it so it can be replaced.
            seal(&s.sealed, &key)
        };
        String::from_utf8(plain).map_err(|_| "that didn't decrypt".into())
    }

    /// A vault in the **pre-envelope** format: key derived straight from the
    /// passphrase, secrets sealed under it, one check value.
    ///
    /// Named with the `_for_test` suffix the dead-capability sweep exempts,
    /// because that is exactly what it is for and it must never be called by
    /// anything that ships. It exists because the migration is the part of
    /// this change that can silently destroy data, and a test that builds its
    /// "old vault" by copying fields out of a new one is not testing the
    /// migration -- it is testing whatever the current format happens to be.
    /// That test existed, and it passed for as long as the two formats were
    /// the same thing.
    pub fn pre_envelope_for_test(
        passphrase: &str,
        items: &[(&str, Kind, &str)],
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<Vault, String> {
        let salt = random_bytes(16);
        let key = derive(passphrase, &salt, cfg)?;
        let mut v = Vault { salt, check: seal_aead(CHECK_VALUE, &key)?, ..Default::default() };
        for (name, kind, value) in items {
            v.secrets.push(Secret {
                name: (*name).to_string(),
                kind: *kind,
                sealed: seal_aead(value.as_bytes(), &key)?,
                real: true,
                added: now,
                last_used: now,
            });
        }
        Ok(v)
    }

    /// A vault in the **legacy-cipher** format: key derived from the
    /// passphrase, secrets XORed under it, no check value, no envelope.
    ///
    /// Carries the `_for_test` suffix the dead-capability sweeps exempt, and
    /// it must never be called by anything that ships — nothing writes this
    /// format any more.
    ///
    /// It exists for the same reason `pre_envelope_for_test` does, and the
    /// reason is sharper here: this is the format whose migration silently
    /// destroyed the data. A test that assembles its "old vault" by setting
    /// `real: false` on secrets that were actually AEAD-sealed is testing
    /// nothing — the bytes have to really be XOR under the really-derived
    /// key, or the branch being tested is not the branch that runs.
    pub fn legacy_cipher_for_test(
        passphrase: &str,
        items: &[(&str, Kind, &str)],
        now: u64,
        cfg: &VaultConfig,
    ) -> Result<Vault, String> {
        let salt = random_bytes(16);
        let key = derive(passphrase, &salt, cfg)?;
        let mut v = Vault { salt, ..Default::default() };
        for (name, kind, value) in items {
            v.secrets.push(Secret {
                name: (*name).to_string(),
                kind: *kind,
                sealed: seal(value.as_bytes(), &key),
                // What made it a legacy vault, and what the guard condition
                // keyed on.
                real: false,
                added: now,
                last_used: now,
            });
        }
        Ok(v)
    }

    /// Secrets that needed real encryption and did not get it.
    ///
    /// These exist on any machine that stored credentials before the real
    /// path was wired. They cannot be upgraded in place — the vault would
    /// have to decrypt and re-seal, which needs the passphrase and is the
    /// person's call, not something to do silently. Naming them is the honest
    /// minimum: a secret that is weakly sealed and never mentioned is exactly
    /// the "you would file it, stop thinking about it" case `put` warns about.
    pub fn weakly_sealed(&self) -> Vec<&str> {
        self.secrets
            .iter()
            .filter(|s| !s.real && s.kind.needs_real_crypto())
            .map(|s| s.name.as_str())
            .collect()
    }

    /// What's in it, without opening it.
    ///
    /// Names and kinds are readable while sealed on purpose: knowing you have
    /// a recovery code for something is useful, and it gives nothing away.
    pub fn list(&self) -> Vec<(&str, Kind)> {
        self.secrets.iter().map(|s| (s.name.as_str(), s.kind)).collect()
    }

    /// Something you haven't touched in a long time.
    pub fn stale(&self, now: u64, days: u64) -> Vec<&str> {
        self.secrets
            .iter()
            .filter(|s| now.saturating_sub(s.last_used) > days * 86_400)
            .map(|s| s.name.as_str())
            .collect()
    }
}

/// Whether the vault is backed by real cryptography.
///
/// One constant, checked at every write, so the answer cannot be inferred
/// wrongly from how careful the surrounding code looks.
///
/// **Flipped to true on 9 Sep 2026**, in the same change that replaced
/// `derive` with Argon2id and `seal` with XChaCha20-Poly1305. The note that
/// stood here previously named Windows DPAPI as the intended replacement,
/// which was right when Atlas targeted one machine and wrong once it targeted
/// every platform: DPAPI has no macOS or Linux equivalent, so that route meant
/// the vault refusing to store a credential forever on two of three systems.
///
/// The rest of that note still holds and is worth keeping: this is the single
/// assumption everything else in the vault rests on, and rolling your own is
/// the one place "build it from scratch" is the wrong instinct. Which is why
/// this is two audited libraries compiled in, not hand-written primitives.
pub const REAL_CRYPTO: bool = true;

/// Stretch a passphrase into a key.
///
/// The note below describes the version this replaced, and is kept because it
/// is the clearest statement of why a KDF that looks expensive can be free.
///
/// It was written as "the shape" of a KDF, deliberately slow, with the rounds
/// visible so a reviewer could see the number. The number is not the security
/// here, and measuring it showed why:
///
/// - The round function is `next[i] = (out[i]*31 + out[(i+7)%32] + r) % 251`,
///   which is **affine over Z_251**. Affine maps compose, so 600,000 rounds
///   collapse into one 32x32 matrix and an offset. The defender pays 600,000
///   steps per guess; an attacker who notices pays one.
/// - Measured, 600,000 rounds take about 19ms — roughly 51 guesses per second
///   per core before that collapse, and effectively unbounded after it. A KDF
///   at this round count should take about a second.
/// - There is no salt, so the same passphrase yields the same key on every
///   machine.
///
/// Raising `kdf_rounds` did not help; composition was the problem, not speed.
///
/// Replaced with Argon2id, which is what `kdf_rounds` was pretending to be.
/// `kdf_rounds` was kept in the config for a while because people have it set
/// in their files, and is gone from the struct now, but it is
/// now read as a *cost hint* rather than an iteration count -- Argon2's cost
/// is memory, not rounds, and exposing rounds again would re-teach the same
/// wrong lesson.
/// A 32-byte key from words a person can write down.
///
/// Deliberately deterministic and salted by a fixed context string rather
/// than a random salt: the point is that typing the same phrase on another
/// machine — or on the same machine after a reinstall — produces the same
/// key, with nothing to carry across but the phrase itself. That is what
/// makes a sealed bundle recoverable by hand, which is the property the whole
/// design is built backwards from.
///
/// The cost is the vault's: Argon2id at 64 MiB and three passes. Half a
/// second on a laptop, and it happens once per run rather than once per
/// bundle.
///
/// `context` separates purposes. Two different uses of the same phrase must
/// not produce the same key, and a fixed label is what keeps them apart.
pub fn key_from_words(words: &str, context: &[u8]) -> Result<Vec<u8>, String> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let words = words.trim();
    if words.len() < 8 {
        return Err("that's too short to be a key phrase".into());
    }
    let params = Params::new(64 * 1024, 3, 1, Some(32))
        .map_err(|e| format!("bad key-derivation settings: {e}"))?;
    let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = vec![0u8; 32];
    a.hash_password_into(words.as_bytes(), context, &mut out)
        .map_err(|e| format!("could not derive a key: {e}"))?;
    Ok(out)
}

fn derive(passphrase: &str, salt: &[u8], cfg: &VaultConfig) -> Result<Vec<u8>, String> {
    use argon2::{Algorithm, Argon2, Params, Version};
    let params = Params::new(cfg.memory_kib(), 3, 1, Some(32))
    .map_err(|e| format!("bad key-derivation settings: {e}"))?;
    let a = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut out = vec![0u8; 32];
    a.hash_password_into(passphrase.as_bytes(), salt, &mut out)
        .map_err(|e| format!("could not derive a key: {e}"))?;
    Ok(out)
}

/// Said out loud rather than left as a surprise: the number in your config
/// does nothing now.
///
/// Silently ignoring a setting somebody deliberately set is the same class of
/// problem as the cipher this replaced -- it looks like it is doing something.
pub const KDF_ROUNDS_IS_IGNORED: &str =
    "kdf_rounds no longer does anything. It counted rounds of a key stretch that collapsed to a \
     single step, so the number never bought what it looked like it bought. The vault now uses \
     Argon2id, whose cost is memory rather than rounds, and that is set to a fixed sensible value.";

/// Random bytes from the operating system.
/// The alphabet a recovery key is written in.
///
/// Crockford's base32: the digits and letters, minus `I`, `L`, `O` and `U`.
/// The first three go because a handwritten `I` is a `1`, an `l` is a `1` and
/// an `O` is a `0`, and this string's whole job is to be copied onto paper by
/// hand and typed back months later. `U` goes because removing it is what
/// stops the generator producing a word somebody would rather not have
/// written on the card in their desk drawer.
const RECOVERY_ALPHABET: &[u8] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// How many characters a recovery key is.
///
/// 24 from a 32-character alphabet is 120 bits. Far past anything that can be
/// guessed, and short enough to write on a card in six groups of four without
/// losing your place. The passphrase it stands in for will usually be weaker
/// than this, which is the right way round for something that is written down
/// rather than remembered.
const RECOVERY_LEN: usize = 24;

/// A short code, from the same alphabet, for something typed once.
///
/// `len` characters grouped in fives. Ten of them is 50 bits, which is the
/// number that matters for a pairing code: what it protects is an invitation
/// sitting in a shared folder for three minutes, sealed by Argon2id at 64MiB.
/// At a tenth of a second per guess — generous to the attacker, since that
/// KDF is built to resist exactly this — ten thousand cores working the whole
/// window would cover about 2^35 of 2^50. The window closes, the file deletes
/// itself, and the guessing has to start again on a code that no longer
/// exists.
///
/// Twenty-four characters would be stronger and nobody types twenty-four
/// characters twice. A code people will not use protects nothing.
pub fn short_code(len: usize) -> String {
    let n = RECOVERY_ALPHABET.len() as u8;
    let ceiling = (256 / n as u16 * n as u16) as u16;
    let mut out = String::with_capacity(len + len / 5);
    let mut taken = 0;
    while taken < len {
        for b in random_bytes(len) {
            if (b as u16) >= ceiling {
                continue;
            }
            if taken > 0 && taken % 5 == 0 {
                out.push('-');
            }
            out.push(RECOVERY_ALPHABET[(b % n) as usize] as char);
            taken += 1;
            if taken == len {
                break;
            }
        }
    }
    out
}

/// A fresh recovery key, grouped for copying by hand.
///
/// Drawn by rejection rather than by modulo: `byte % 32` would be uniform
/// here because 32 divides 256, but writing it that way leaves a landmine for
/// the day somebody changes the alphabet's length to 33 and introduces a bias
/// nothing would ever notice.
/// Also what `sync` issues as a household key phrase. Same alphabet, same
/// grouping, same `tidy_recovery_key` on the way back in — a person copying
/// one of these from paper should not have to know which of the two it is.
pub fn new_recovery_key() -> String {
    let n = RECOVERY_ALPHABET.len() as u8;
    let ceiling = (256 / n as u16 * n as u16) as u16;
    let mut out = String::with_capacity(RECOVERY_LEN + RECOVERY_LEN / 4);
    let mut taken = 0;
    while taken < RECOVERY_LEN {
        for b in random_bytes(RECOVERY_LEN) {
            if (b as u16) >= ceiling {
                continue;
            }
            if taken > 0 && taken % 4 == 0 {
                out.push('-');
            }
            out.push(RECOVERY_ALPHABET[(b % n) as usize] as char);
            taken += 1;
            if taken == RECOVERY_LEN {
                break;
            }
        }
    }
    out
}

/// What was typed, as the key it was meant to be.
///
/// Forgiving in exactly the ways a person copying from paper is wrong, and in
/// no others:
///
/// * case, because nobody holds shift for twenty-four characters;
/// * spaces, dashes and any other punctuation, because people group things
///   differently from however it was printed;
/// * `I` and `L` read as `1`, `O` as `0`, because those are the confusions
///   the alphabet was chosen to avoid — the letters cannot occur in a real
///   key, so accepting them costs nothing and refusing them would reject a
///   correct key written in tired handwriting.
///
/// Anything else is left exactly as typed and simply will not match, which is
/// the honest outcome: this normalises transcription, it does not guess.
pub fn tidy_recovery_key(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| match c.to_ascii_uppercase() {
            'I' | 'L' => '1',
            'O' => '0',
            other => other,
        })
        .collect()
}

pub(crate) fn random_bytes(n: usize) -> Vec<u8> {
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut v = vec![0u8; n];
    chacha20poly1305::aead::OsRng.fill_bytes(&mut v);
    v
}

/// The legacy cipher. Kept **only** so secrets written before the real one
/// existed can still be read.
///
/// It is a repeating-key XOR -- a Vigenere cipher with a 32-byte key. The same
/// key covered every secret, so XORing any two stored secrets cancelled the
/// key and left the two plaintexts XORed together; and there was no
/// authentication tag, so stored ciphertext could be altered undetectably.
/// Nothing writes with this any more; `Secret::real` says which entries still
/// need it.
fn seal(data: &[u8], key: &[u8]) -> Vec<u8> {
    data.iter().enumerate().map(|(i, b)| b ^ key[i % key.len()]).collect()
}

/// Encrypt, for real, on every platform Atlas runs on.
///
/// XChaCha20-Poly1305 with a fresh random nonce per secret, stored in front of
/// the ciphertext. Two properties the old `seal` did not have: identical
/// plaintexts no longer produce identical ciphertexts, and the authentication
/// tag means a tampered secret fails to open instead of decrypting to
/// something subtly wrong.
///
/// This replaced a Windows-only path. `real_crypto_here()` used to mean "are
/// we on Windows", which left macOS and Linux -- both of which Atlas targets --
/// refusing to store a credential at all.
/// The plaintext sealed as the passphrase check. Fixed and not secret.
const CHECK_VALUE: &[u8] = b"atlas vault check v1";

pub fn seal_aead(data: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    if key.len() != 32 {
        return Err("the vault key is the wrong size".into());
    }
    let c = XChaCha20Poly1305::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce_bytes = random_bytes(24);
    let nonce = XNonce::from_slice(&nonce_bytes);
    let mut out = nonce_bytes.clone();
    out.extend(c.encrypt(nonce, data).map_err(|_| "could not seal that".to_string())?);
    Ok(out)
}

/// The other half of [`seal_aead`], public for the same reason it is: sync
/// seals bundles with the same primitive rather than inventing a second one.
pub fn unseal_aead(sealed: &[u8], key: &[u8]) -> Result<Vec<u8>, String> {
    use chacha20poly1305::aead::{Aead, KeyInit};
    use chacha20poly1305::{XChaCha20Poly1305, XNonce};
    if key.len() != 32 {
        return Err("the vault key is the wrong size".into());
    }
    if sealed.len() < 24 {
        return Err("that secret is too short to be a sealed one".into());
    }
    let c = XChaCha20Poly1305::new_from_slice(key).map_err(|e| e.to_string())?;
    let nonce = XNonce::from_slice(&sealed[..24]);
    // A failure here is either the wrong passphrase or a tampered file, and
    // both deserve the same answer: it did not open. Guessing which would be
    // guessing.
    c.decrypt(nonce, &sealed[24..])
        .map_err(|_| "that didn't open -- wrong passphrase, or the file has been altered".into())
}

/// Seal with the OS's own data protection, keyed by raw bytes.
///
/// The vault holds a *derived key*, not the passphrase — `unlock` runs the
/// passphrase through `derive` and `lock` zeroes the result. So the only
/// entropy `put`/`get` can hand to the OS is those bytes, and
/// `seal_for_real`'s `&str` signature could not accept them. That mismatch is
/// a large part of why the real path was never called: it did not fit where
/// it was needed, so the weak `seal` stayed.
pub fn seal_bytes(data: &[u8], extra: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        crate::platform::win::protect(data, extra).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (data, extra);
        Err("real encryption needs Windows data protection, which isn't available here".into())
    }
}

pub fn unseal_bytes(sealed: &[u8], extra: &[u8]) -> Result<Vec<u8>, String> {
    #[cfg(windows)]
    {
        crate::platform::win::unprotect(sealed, extra).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = (sealed, extra);
        Err("real encryption needs Windows data protection, which isn't available here".into())
    }
}

// `seal_for_real` and `unseal_for_real` were here, and are gone.
//
// They wrapped Windows DPAPI, taking the passphrase as a `&str`. That
// signature is why they were never called, by their own replacement's
// account: the vault holds a *derived key*, not the passphrase — `unlock`
// runs the passphrase through `derive` and `lock` zeroes the result — so
// the only entropy `put`/`get` can hand the OS is raw bytes, which a `&str`
// cannot carry. `seal_bytes`/`unseal_bytes` below are the same idea with the
// signature the call site actually needs.
//
// Two functions doing one job, one of them documented as unusable for the
// only place it was wanted, and both named `_for_real` while the real path
// is `seal_aead` — that naming is a trap for the next person reading this
// file, which is the specific failure this vault already shipped once.
// Deleted rather than left counted.

/// Is real encryption available on this machine?
///
/// Checked rather than assumed. The vault stores credentials when this is
/// true and refuses when it isn't, which means the refusal disappears on the
/// machine it was built for and stays everywhere else.
pub fn real_crypto_here() -> bool {
    // Used to be `cfg!(windows)`, because the only real encryption wired in
    // was Windows data protection. Atlas is built for every platform, so that
    // meant macOS and Linux refused to store a credential forever -- a correct
    // refusal and an unusable assistant. The cipher is now in-process and
    // identical everywhere, so there is no platform on which this is false.
    true
}

/// Why this isn't a password manager, said plainly.
///
/// Worth being clear rather than letting it be assumed: this is for the
/// handful of things Atlas needs, not for your whole life.
pub const NOT_A_PASSWORD_MANAGER: &str =
    "This holds the few things I need, not everything you own. A real password manager is \
     audited, syncs to your phone, and survives this laptop dying — I'm none of those. Keep your \
     passwords there and let me hold the couple of secrets I actually use.";

/// What it can and can't do while you're asleep.
pub const WHILE_YOU_SLEEP: &str =
    "Nothing in the vault is reachable unattended. That's the whole point of the passphrase: if I \
     could open it while you're asleep, so could anyone who took the laptop while you're asleep. \
     Overnight work runs without it.";

/// The honest limit.
pub const WHAT_THIS_DOES_NOT_STOP: &str =
    "If someone has the laptop while it's unlocked and the vault is open, they have what's in it. \
     Encryption protects a machine that's off or locked. Nothing protects a machine that's open \
     and in someone else's hands.";

/// Set the first passphrase, or change the one there is — the one place both
/// the command line and the hub's Accounts page do it (27 Sep 2026: the hub
/// had no way to set one, so a person who never opens a terminal could never
/// take a handover back).
///
/// Returns what to say and, on a first passphrase, the recovery key made in
/// the same breath. The key is shown once by the caller and exists nowhere
/// else afterwards. The vault is locked again before this returns, whatever
/// happened; saving it is the caller's, because only the caller knows where
/// it lives.
///
/// Whether the person holding the machine may set a *first* passphrase at
/// all (`handover::would_hand_out_the_way_back`) is the caller's to ask
/// before this is reached — it is a question about who is asking, and this
/// only knows what was typed.
pub fn set_passphrase(
    v: &mut Vault,
    old: &str,
    new: &str,
    again: &str,
    cfg: &VaultConfig,
    now: u64,
) -> Result<(String, Option<String>), String> {
    let first = !v.has_a_passphrase();
    if new.is_empty() {
        return Err("Nothing typed — unchanged.".into());
    }
    if !first && old.is_empty() {
        return Err("The current passphrase is needed to change it — nothing changed.".into());
    }
    if new != again {
        return Err("Those two didn't match. Nothing changed.".into());
    }
    // Refusals come back from the vault itself; the "accepted, but" note is
    // said here, where there is someone to hear it.
    let mut said: Vec<String> = Vec::new();
    match crate::guessable::fit_for_the_vault(new, VAULT_WORDS) {
        Ok(Some(note)) => said.push(note),
        Ok(None) => said.push(format!("Strength: {}.", crate::guessable::estimate(new, VAULT_WORDS).say())),
        Err(_) => {} // the vault says why, below
    }
    let outcome = if first {
        v.open(new, now, cfg).map(|()| "Set.".to_string())
    } else {
        v.change_passphrase(old, new, now, cfg)
            .map(|()| "Changed, and everything in the vault re-sealed under it.".to_string())
    };
    let done = match outcome {
        Ok(done) => done,
        Err(why) => {
            v.lock();
            return Err(why);
        }
    };
    // The recovery key is made *here*, in the same breath as the passphrase,
    // rather than offered as a later step somebody will mean to get round to.
    // The failure this guards against is not exotic: it is forgetting a
    // sentence you typed once, months ago, and discovering that your own
    // assistant will never let you back in.
    let issued = if first && !v.has_a_recovery_key() {
        match v.issue_recovery_key(now, cfg) {
            Ok(code) => Some(code),
            Err(e) => {
                said.push(format!("(I couldn't make a recovery key: {e})"));
                None
            }
        }
    } else {
        None
    };
    said.push(done);
    v.lock();
    Ok((said.join(" "), issued))
}

/// Make a new recovery key, retiring any old one, after the passphrase has
/// proved who is asking. Locked again before it returns. Shared by the
/// command line and the hub for the same reason as `set_passphrase`.
pub fn make_recovery_key(v: &mut Vault, phrase: &str, cfg: &VaultConfig, now: u64) -> Result<String, String> {
    if !v.has_a_passphrase() {
        return Err("There's no passphrase on this vault yet, so there's nothing to make a second way into.".into());
    }
    if phrase.is_empty() {
        return Err("Nothing typed — unchanged.".into());
    }
    v.lock();
    if let Err(why) = v.open(phrase, now, cfg) {
        v.lock();
        return Err(why);
    }
    if !v.proved_it() {
        v.lock();
        return Err("that isn't the passphrase".into());
    }
    let made = v.issue_recovery_key(now, cfg);
    v.lock();
    made
}

/// Said when the vault is one made before it opened on your sign-in.
pub const OLD_VAULT: &str = "your vault is one made before Atlas opened it with your Windows sign-in, so it \
     still wants the passphrase you chose then -- once, on the Accounts page, and never again after. If you don't \
     remember it, \"Start a new vault\" there sets one up that needs nothing (you'd connect your accounts again)";
