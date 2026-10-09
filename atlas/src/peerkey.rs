//! This Atlas's own identity: a signing key that says "this came from me"
//! in a way anyone can check and nobody else can forge.
//!
//! Pairing gives each pair of Atlases a shared token, and a token proves who
//! is knocking *on your own door*. It proves nothing about a message that
//! reached you through somebody else. Group chats need exactly that: a group's
//! membership is decided by whoever created it, and every member has to be
//! able to trust a membership list they heard from another member, not only
//! one the creator handed them directly. So each Atlas gets an ed25519 key of
//! its own (the same algorithm the release signature uses), introduces its
//! public half to each paired Atlas over the authenticated pairing channel
//! (`/hello`), and signs what it alone may decide.
//!
//! The private half lives with the pairings, in this install's state folder --
//! the same protection class as the pairing tokens, which already let anyone
//! holding them speak as you to your peers. It never leaves this machine.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::Path;

const FILE: &str = "identity.json";

#[derive(Serialize, Deserialize)]
struct Stored {
    /// The private seed, hex. Never sent anywhere.
    seed: String,
}

/// This Atlas's signing identity.
pub struct Identity {
    key: SigningKey,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn unhex<const N: usize>(s: &str) -> Option<[u8; N]> {
    let s = s.trim();
    if s.len() != N * 2 || !s.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let mut out = [0u8; N];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

impl Identity {
    /// This install's identity, made the first time it is asked for.
    ///
    /// A file that exists and does not read is an error, never a reason to
    /// make a new one: a new identity is a different Atlas as far as every
    /// paired device is concerned, and silently becoming one would lock you
    /// out of every group you own.
    pub fn load_or_create(dir: &Path) -> Result<Identity, String> {
        let _state = crate::store::state_transaction(dir).map_err(|e| e.to_string())?;
        let path = dir.join(FILE);
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                let s: Stored = serde_json::from_str(&text)
                    .map_err(|e| format!("{} doesn't read ({e}); I won't replace it", path.display()))?;
                let seed = unhex::<32>(&s.seed)
                    .ok_or_else(|| format!("{} is damaged; I won't replace it", path.display()))?;
                Ok(Identity { key: SigningKey::from_bytes(&seed) })
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                use chacha20poly1305::aead::rand_core::RngCore;
                let mut seed = [0u8; 32];
                chacha20poly1305::aead::OsRng.fill_bytes(&mut seed);
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                serde_json::to_vec(&Stored { seed: hex(&seed) })
                    .map_err(std::io::Error::other)
                    .and_then(|body| crate::store::write_whole(&path, &body))
                    .map_err(|e| format!("couldn't save this Atlas's identity: {e}"))?;
                Ok(Identity { key: SigningKey::from_bytes(&seed) })
            }
            Err(e) => Err(format!("couldn't read {}: {e}", path.display())),
        }
    }

    /// From a known seed. For tests, and nothing else.
    pub fn from_seed_for_test(seed: [u8; 32]) -> Identity {
        Identity { key: SigningKey::from_bytes(&seed) }
    }

    /// The public half, hex -- what other Atlases pin and check against.
    pub fn public(&self) -> String {
        hex(self.key.verifying_key().as_bytes())
    }

    /// A secret only this Atlas and the owner of `their_point` can compute
    /// (X25519): this key's scalar times their Curve25519 point. `None` for a
    /// point that yields nothing (a low-order point an attacker might send).
    ///
    /// The same key signs and agrees. That pairing is studied and holds
    /// (Thormarker, "On using the same key pair for Ed25519 and an X25519
    /// based KEM", 2021) -- and it's what lets the key a friend pinned be the
    /// key they encrypt to, with nothing more to exchange.
    pub fn agree_point(&self, their_point: [u8; 32]) -> Option<[u8; 32]> {
        let out = curve25519_dalek::MontgomeryPoint(their_point).mul_clamped(self.key.to_scalar_bytes()).to_bytes();
        (out != [0u8; 32]).then_some(out)
    }

    /// A separate secret for a separate job (`label`), made from this key and
    /// nothing else: the same Atlas always gets the same one, and it gives
    /// nothing away about the key itself.
    pub fn derive(&self, label: &str) -> [u8; 32] {
        let mut h = Sha256::new();
        h.update(b"atlas-derive-v1\0");
        h.update(label.as_bytes());
        h.update(b"\0");
        h.update(self.key.to_bytes());
        h.finalize().into()
    }

    /// Sign `message` under `domain`, so a signature made for one purpose can
    /// never be replayed as one made for another.
    pub fn sign(&self, domain: &[u8], message: &[u8]) -> String {
        let mut m = domain.to_vec();
        m.extend_from_slice(message);
        hex(&self.key.sign(&m).to_bytes())
    }
}

/// Keeps a delegation from being replayed as any other kind of signature.
pub const DELEGATION_DOMAIN: &[u8] = b"atlas-device-delegation-v1\n";

/// "This other device of mine may act for me": a device key, signed by the
/// key it acts for. How your phone manages a group your laptop made -- the
/// group's owner is still one key, and every member can check the phone was
/// vouched for by it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Delegation {
    pub device: String,
    pub signature: String,
}

impl Identity {
    /// Vouch for another device of yours.
    pub fn delegate(&self, device: &str) -> Delegation {
        Delegation { device: device.to_string(), signature: self.sign(DELEGATION_DOMAIN, device.as_bytes()) }
    }
}

/// An Atlas public key (hex) in the form X25519 agrees with.
pub fn exchange_point(public_hex: &str) -> Option<[u8; 32]> {
    Some(VerifyingKey::from_bytes(&unhex::<32>(public_hex)?).ok()?.to_montgomery().to_bytes())
}

/// Did `owner` vouch for this device?
pub fn verify_delegation(owner: &str, d: &Delegation) -> bool {
    is_public_key(&d.device) && verify(owner, DELEGATION_DOMAIN, d.device.as_bytes(), &d.signature)
}

/// A short, stable fingerprint of a public key: the first 16 bytes of its
/// SHA-256, hex. What a group id carries to bind it to its creator.
pub fn fingerprint(public_hex: &str) -> Option<String> {
    let bytes = unhex::<32>(public_hex)?;
    Some(hex(&Sha256::digest(bytes)[..16]))
}

/// Is `public_hex` a real public key?
pub fn is_public_key(public_hex: &str) -> bool {
    unhex::<32>(public_hex).is_some_and(|b| VerifyingKey::from_bytes(&b).is_ok())
}

/// Did the holder of `public_hex` sign `message` under `domain`? Strict
/// verification; anything malformed is simply "no".
pub fn verify(public_hex: &str, domain: &[u8], message: &[u8], signature_hex: &str) -> bool {
    let (Some(k), Some(s)) = (unhex::<32>(public_hex), unhex::<64>(signature_hex)) else { return false };
    let Ok(key) = VerifyingKey::from_bytes(&k) else { return false };
    let mut m = domain.to_vec();
    m.extend_from_slice(message);
    key.verify_strict(&m, &Signature::from_bytes(&s)).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signature_verifies_only_for_its_key_its_domain_and_its_words() {
        let me = Identity::from_seed_for_test([7; 32]);
        let other = Identity::from_seed_for_test([8; 32]);
        let sig = me.sign(b"d1\n", b"hello");
        assert!(verify(&me.public(), b"d1\n", b"hello", &sig));
        assert!(!verify(&other.public(), b"d1\n", b"hello", &sig));
        assert!(!verify(&me.public(), b"d2\n", b"hello", &sig), "replayed across purposes");
        assert!(!verify(&me.public(), b"d1\n", b"hellO", &sig));
        assert!(!verify("zz", b"d1\n", b"hello", &sig));
        assert!(!verify(&me.public(), b"d1\n", b"hello", "00"));
    }

    #[test]
    fn an_identity_is_made_once_and_never_silently_replaced() {
        let dir = std::env::temp_dir().join(format!("atlas-peerkey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let a = Identity::load_or_create(&dir).unwrap();
        let b = Identity::load_or_create(&dir).unwrap();
        assert_eq!(a.public(), b.public());
        assert!(is_public_key(&a.public()));
        assert_eq!(fingerprint(&a.public()).unwrap().len(), 32);
        std::fs::write(dir.join(FILE), "garbage").unwrap();
        assert!(Identity::load_or_create(&dir).is_err(), "a damaged identity must not be replaced by a new one");
    }
}
