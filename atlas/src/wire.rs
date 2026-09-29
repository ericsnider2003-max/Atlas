//! Atlas's own encryption between two people's Atlases.
//!
//! Your own devices can reach each other over your own private network, which
//! encrypts for them. Between two *people* nothing like that sits in the way:
//! your Atlas knocks on your friend's over the open internet, so everything it
//! says has to be sealed by Atlas itself. This is that seal.
//!
//! **Who can open it.** Every request is sealed *to* the key the sender pinned
//! for the receiver when they became friends, and *by* the sender's own key. A
//! fresh one-time key is made for every envelope, so no two envelopes share a
//! key and an old one is never readable later from anything sent before it.
//! Only the receiver can open it, and opening it proves who sent it -- nobody
//! else could have made it -- so a friend passing it on (`mailbox`) carries a
//! blob it can neither read nor forge.
//!
//! **What's inside** is the same request the door always handled -- which door
//! (`/chat`, `/group`, ...), the pairing's token, and the body -- so every
//! door keeps exactly its own rules. The answer comes back sealed with a key
//! only the sender can derive.
//!
//! **Replays.** An envelope carries the time it was made and is refused
//! outside a ten-minute window, and each one-time key is accepted once.
//!
//! The pieces: X25519 for agreeing a secret (`peerkey`), HKDF-SHA256 to turn
//! it into keys, ChaCha20-Poly1305 to seal. Nothing here is new cryptography;
//! it is the shape of Noise's "K" pattern, one message and its answer.

use crate::peerkey::{exchange_point, Identity};
use chacha20poly1305::aead::{Aead, KeyInit, OsRng, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The door a sealed envelope is posted to.
pub const PATH: &str = "/sealed";
/// How far an envelope's time may be from the receiver's clock.
pub const WINDOW_SECS: u64 = 600;
/// The largest envelope a door reads: a release piece, sealed and encoded.
pub const MAX_ENVELOPE: usize = 2 * 1024 * 1024;
const VERSION: u8 = 1;
const LABEL: &[u8] = b"atlas-sealed-v1";

/// What travels. Nothing in it but keys and ciphertext.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub v: u8,
    /// The sender's Atlas key.
    pub from: String,
    /// The receiver's Atlas key -- who it's sealed to.
    pub to: String,
    /// The one-time key, hex.
    pub eph: String,
    /// The sealed request, base64.
    pub ct: String,
}

/// What's inside: the request the door handles.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Inner {
    pub path: String,
    /// The pairing's token. Empty for a first knock (`/friend`), which has none.
    pub token: String,
    pub body: String,
    /// When it was sealed, by the sender's clock.
    pub at: u64,
}

/// The key the answer is sealed with: only the two ends can make it.
#[derive(Clone)]
pub struct ReplyKey([u8; 32]);

/// The answer, inside.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub status: u16,
    pub body: String,
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex32(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn hmac(key: &[u8], data: &[&[u8]]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.iter().map(|b| b ^ 0x36).collect::<Vec<u8>>());
    for d in data {
        inner.update(d);
    }
    let mut outer = Sha256::new();
    outer.update(k.iter().map(|b| b ^ 0x5c).collect::<Vec<u8>>());
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// HKDF-SHA256 (RFC 5869), one block out.
fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8]) -> [u8; 32] {
    let prk = hmac(salt, &[ikm]);
    hmac(&prk, &[info, &[1u8]])
}

/// The two keys for one envelope: (request, reply).
fn keys(dh_eph: &[u8; 32], dh_static: &[u8; 32], eph: &[u8; 32], from: &str, to: &str) -> ([u8; 32], [u8; 32]) {
    let mut ikm = dh_eph.to_vec();
    ikm.extend_from_slice(dh_static);
    let mut salt = LABEL.to_vec();
    salt.extend_from_slice(eph);
    salt.extend_from_slice(from.as_bytes());
    salt.extend_from_slice(to.as_bytes());
    (hkdf(&salt, &ikm, b"request"), hkdf(&salt, &ikm, b"reply"))
}

fn aad(from: &str, to: &str) -> Vec<u8> {
    let mut a = LABEL.to_vec();
    a.extend_from_slice(from.as_bytes());
    a.extend_from_slice(to.as_bytes());
    a
}

fn lock(key: &[u8; 32], aad: &[u8], plain: &[u8]) -> Option<Vec<u8>> {
    // A key is used for exactly one message, so the all-zero nonce is safe.
    ChaCha20Poly1305::new(Key::from_slice(key)).encrypt(Nonce::from_slice(&[0u8; 12]), Payload { msg: plain, aad }).ok()
}

fn unlock(key: &[u8; 32], aad: &[u8], sealed: &[u8]) -> Option<Vec<u8>> {
    ChaCha20Poly1305::new(Key::from_slice(key)).decrypt(Nonce::from_slice(&[0u8; 12]), Payload { msg: sealed, aad }).ok()
}

/// Seal a request from `me` to the Atlas whose key is `to`. Returns the
/// envelope and the key its answer will come back under.
pub fn seal(me: &Identity, to: &str, inner: &Inner) -> Option<(Envelope, ReplyKey)> {
    use chacha20poly1305::aead::rand_core::RngCore;
    let to_point = exchange_point(to)?;
    let mut e = [0u8; 32];
    OsRng.fill_bytes(&mut e);
    let eph = curve25519_dalek::MontgomeryPoint::mul_base_clamped(e).to_bytes();
    let dh_eph = curve25519_dalek::MontgomeryPoint(to_point).mul_clamped(e).to_bytes();
    if dh_eph == [0u8; 32] {
        return None;
    }
    let dh_static = me.agree_point(to_point)?;
    let from = me.public();
    let (req, reply) = keys(&dh_eph, &dh_static, &eph, &from, to);
    let plain = serde_json::to_vec(inner).ok()?;
    let ct = lock(&req, &aad(&from, to), &plain)?;
    Some((Envelope { v: VERSION, from, to: to.to_string(), eph: hex(&eph), ct: crate::b64::encode(&ct) }, ReplyKey(reply)))
}

/// Why an envelope wasn't opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    NotForMe,
    Unreadable,
    TooOld,
    Replayed,
}

/// One-time keys already accepted, within the window.
#[derive(Debug, Default)]
pub struct Seen {
    at: std::collections::VecDeque<(String, u64)>,
}

impl Seen {
    fn check_and_keep(&mut self, eph: &str, now: u64) -> bool {
        while self.at.front().is_some_and(|(_, t)| now.saturating_sub(*t) > 2 * WINDOW_SECS) {
            self.at.pop_front();
        }
        if self.at.iter().any(|(e, _)| e == eph) {
            return false;
        }
        self.at.push_back((eph.to_string(), now));
        while self.at.len() > 50_000 {
            self.at.pop_front();
        }
        true
    }
}

/// Open an envelope sealed to `me`. Returns who sealed it (proven: only the
/// holder of that key could have), what's inside, and the key to answer with.
pub fn open(me: &Identity, env: &Envelope, now: u64, seen: &mut Seen) -> Result<(String, Inner, ReplyKey), Refused> {
    if env.v != VERSION || env.to != me.public() {
        return Err(Refused::NotForMe);
    }
    let eph = unhex32(&env.eph).ok_or(Refused::Unreadable)?;
    let from_point = exchange_point(&env.from).ok_or(Refused::Unreadable)?;
    let dh_eph = me.agree_point(eph).ok_or(Refused::Unreadable)?;
    let dh_static = me.agree_point(from_point).ok_or(Refused::Unreadable)?;
    let (req, reply) = keys(&dh_eph, &dh_static, &eph, &env.from, &env.to);
    let ct = crate::b64::decode(&env.ct).map_err(|_| Refused::Unreadable)?;
    let plain = unlock(&req, &aad(&env.from, &env.to), &ct).ok_or(Refused::Unreadable)?;
    let inner: Inner = serde_json::from_slice(&plain).map_err(|_| Refused::Unreadable)?;
    if inner.at.abs_diff(now) > WINDOW_SECS {
        return Err(Refused::TooOld);
    }
    if !seen.check_and_keep(&env.eph, now) {
        return Err(Refused::Replayed);
    }
    Ok((env.from.clone(), inner, ReplyKey(reply)))
}

/// Seal the answer to an opened envelope.
pub fn seal_reply(k: &ReplyKey, status: u16, body: &str) -> String {
    let plain = serde_json::to_vec(&Reply { status, body: body.to_string() }).unwrap_or_default();
    lock(&k.0, LABEL, &plain).map(|c| crate::b64::encode(&c)).unwrap_or_default()
}

/// Open the answer to an envelope you sealed.
pub fn open_reply(k: &ReplyKey, text: &str) -> Option<Reply> {
    let ct = crate::b64::decode(text.trim()).ok()?;
    serde_json::from_slice(&unlock(&k.0, LABEL, &ct)?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inner(path: &str, at: u64) -> Inner {
        Inner { path: path.into(), token: "tok".into(), body: "{\"x\":1}".into(), at }
    }

    #[test]
    fn only_the_receiver_opens_it_and_knows_who_sent_it() {
        let (eric, sam, maya) =
            (Identity::from_seed_for_test([1; 32]), Identity::from_seed_for_test([2; 32]), Identity::from_seed_for_test([3; 32]));
        let (env, rk) = seal(&eric, &sam.public(), &inner("/chat", 1_000)).unwrap();
        let mut seen = Seen::default();
        let (from, got, k) = open(&sam, &env, 1_000, &mut seen).unwrap();
        assert_eq!(from, eric.public());
        assert_eq!(got, inner("/chat", 1_000));
        // The answer: only the sender reads it.
        let r = seal_reply(&k, 200, "ok");
        assert_eq!(open_reply(&rk, &r), Some(Reply { status: 200, body: "ok".into() }));
        // Nobody else opens it, even with the envelope retargeted.
        assert_eq!(open(&maya, &env, 1_000, &mut Seen::default()).err(), Some(Refused::NotForMe));
        let mut stolen = env.clone();
        stolen.to = maya.public();
        assert!(open(&maya, &stolen, 1_000, &mut Seen::default()).is_err());
    }

    #[test]
    fn nobody_can_seal_as_someone_else() {
        let (eric, sam, maya) =
            (Identity::from_seed_for_test([1; 32]), Identity::from_seed_for_test([2; 32]), Identity::from_seed_for_test([3; 32]));
        // Maya seals, then claims it came from Eric.
        let (mut env, _) = seal(&maya, &sam.public(), &inner("/chat", 1_000)).unwrap();
        env.from = eric.public();
        assert_eq!(open(&sam, &env, 1_000, &mut Seen::default()).err(), Some(Refused::Unreadable));
    }

    #[test]
    fn a_changed_or_replayed_or_stale_envelope_is_refused() {
        let (eric, sam) = (Identity::from_seed_for_test([1; 32]), Identity::from_seed_for_test([2; 32]));
        let (env, _) = seal(&eric, &sam.public(), &inner("/chat", 1_000)).unwrap();
        let mut seen = Seen::default();
        assert!(open(&sam, &env, 1_000, &mut seen).is_ok());
        assert_eq!(open(&sam, &env, 1_001, &mut seen).err(), Some(Refused::Replayed));
        let mut bent = env.clone();
        let mut ct = crate::b64::decode(&bent.ct).unwrap();
        ct[3] ^= 1;
        bent.ct = crate::b64::encode(&ct);
        assert_eq!(open(&sam, &bent, 1_000, &mut Seen::default()).err(), Some(Refused::Unreadable));
        let (late, _) = seal(&eric, &sam.public(), &inner("/chat", 1_000)).unwrap();
        assert_eq!(open(&sam, &late, 1_000 + WINDOW_SECS + 1, &mut Seen::default()).err(), Some(Refused::TooOld));
    }

    #[test]
    fn hkdf_matches_rfc_5869_case_1() {
        // RFC 5869 A.1, first 32 bytes of OKM.
        let ikm = [0x0bu8; 22];
        let salt: Vec<u8> = (0x00..=0x0c).collect();
        let info: Vec<u8> = (0xf0..=0xf9).collect();
        let okm = hkdf(&salt, &ikm, &info);
        assert_eq!(hex(&okm), "3cb25f25faacd57a90434f64d0362f2a2d2d0a90cf1a5a4c5db02d56ecc4c5bf");
    }
}
