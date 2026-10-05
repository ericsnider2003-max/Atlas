//! Reaching an Android phone with Atlas closed (item 15, decision 3: "it
//! must work on Android too", with no Google dependency by default).
//!
//! The Android app registers with a UnifiedPush distributor -- the ntfy app
//! is the usual one, Sunup and NextPush are others -- and gets back a push
//! address (an `https://` URL) and two keys of its own. Atlas on the phone
//! carries those to the laptop the way the iPhone's address travels
//! (`apns`): a sync event, taken only from a sealed bundle (`take_synced`).
//! The laptop keeps them in `data/state/webpush.json`.
//!
//! To send, the laptop seals the message for that phone alone -- Web Push
//! encryption, RFC 8291: an ECDH key agreement with the phone's key, HKDF,
//! AES-128-GCM -- and POSTs it to the address with a VAPID signature
//! (RFC 8292) from a key the laptop makes for itself on first use. The
//! distributor (ntfy.sh, or your own ntfy) carries bytes it can't read.
//! UnifiedPush has required exactly this since late 2024.
//!
//! What goes: the title, and the detail only when `phone.include_detail`
//! allows it, as with every push (`phone::body_for`).

use hmac::{Hmac, Mac};
use p256::elliptic_curve::sec1::ToEncodedPoint;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::path::Path;

/// The sync id prefix for an Android phone's push address.
pub const SYNC_PREFIX: &str = "webpush:";

/// Where the laptop keeps the addresses, in its state folder.
pub const FILE: &str = "webpush.json";

/// The laptop's own VAPID key, made once, in its state folder.
pub const VAPID_FILE: &str = "vapid.pem";

/// How long a distributor may hold a message for a phone that's off: a day.
pub const TTL_SECS: u64 = 24 * 60 * 60;

/// Record size written in the header (RFC 8188); one record carries it all.
const RECORD_SIZE: u32 = 4096;

// ------------------------------------------------------------------ addresses

/// One Android phone's push address and keys.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Device {
    pub name: String,
    /// The distributor's URL for this phone.
    pub endpoint: String,
    /// The phone's P-256 public key, base64url, uncompressed (65 bytes).
    pub p256dh: String,
    /// The phone's authentication secret, base64url (16 bytes).
    pub auth: String,
    pub since: u64,
}

/// The addresses the laptop knows.
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct Devices {
    pub devices: Vec<Device>,
}

impl Devices {
    pub fn load(state_dir: &Path) -> Devices {
        std::fs::read(state_dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
    }

    pub fn save(&self, state_dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(state_dir)?;
        crate::store::write_whole(&state_dir.join(FILE), &serde_json::to_vec_pretty(self).map_err(std::io::Error::other)?)
    }

    /// One address per phone: a new one replaces the old (the distributor
    /// hands out a new one after a reinstall or a change of distributor).
    pub fn set(&mut self, d: Device) {
        self.devices.retain(|x| x.name != d.name);
        self.devices.push(d);
    }

    /// The distributor said this address is gone (404/410): forget it.
    pub fn forget(&mut self, endpoint: &str) {
        self.devices.retain(|d| d.endpoint != endpoint);
    }
}

/// Is this an address, with keys, that a phone could have given? The
/// address is an `https://` URL (or `http://` to an ntfy on your own
/// network), the key 65 bytes and the secret 16.
pub fn looks_like_an_address(endpoint: &str, p256dh: &str, auth: &str) -> bool {
    let e = endpoint.trim();
    let url_ok = (e.starts_with("https://") || e.starts_with("http://"))
        && e.len() < 1024
        && !e.chars().any(|c| c.is_whitespace() || c == '|' || c == '"');
    url_ok
        && b64url_decode(p256dh).is_some_and(|k| k.len() == 65 && k[0] == 4 && p256::PublicKey::from_sec1_bytes(&k).is_ok())
        && b64url_decode(auth).is_some_and(|a| a.len() == 16)
}

/// The sync event a phone sends with its address: `(id, field, to)` for
/// `sync::What::Changed`.
pub fn change_to_carry(device: &str, endpoint: &str, p256dh: &str, auth: &str) -> (String, String, String) {
    (format!("{SYNC_PREFIX}{device}"), "webpush".into(), format!("{}|{}|{}", endpoint.trim(), p256dh.trim(), auth.trim()))
}

/// A phone's address arriving by sync, kept only from a sealed bundle (your
/// own devices). Returns a line to log, or `None` when it wasn't taken.
pub fn take_synced(state_dir: &Path, id: &str, to: &str, sealed: bool, now: u64) -> Option<String> {
    let device = id.strip_prefix(SYNC_PREFIX)?;
    if !sealed {
        return None;
    }
    let mut parts = to.splitn(3, '|');
    let (endpoint, p256dh, auth) = (parts.next()?, parts.next()?, parts.next()?);
    if !looks_like_an_address(endpoint, p256dh, auth) {
        return None;
    }
    let mut d = Devices::load(state_dir);
    if d.devices.iter().any(|x| x.name == device && x.endpoint == endpoint && x.p256dh == p256dh) {
        return None;
    }
    d.set(Device { name: device.into(), endpoint: endpoint.into(), p256dh: p256dh.into(), auth: auth.into(), since: now });
    d.save(state_dir).ok()?;
    Some(format!("{device} can now be reached with Atlas closed"))
}

// ------------------------------------------------------------------ encryption

fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    #[allow(clippy::expect_used, reason = "HMAC accepts a key of any length; new_from_slice cannot fail for it")]
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC takes any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

/// Seal `plaintext` for the phone whose key is `ua_public` (65 bytes) and
/// secret `auth` (16), as RFC 8291 lays out, with this sender key and salt.
/// Returns the whole body: the RFC 8188 header, then the one record.
pub fn seal_with(
    plaintext: &[u8],
    ua_public: &[u8],
    auth: &[u8],
    as_secret: &p256::SecretKey,
    salt: &[u8; 16],
) -> Result<Vec<u8>, String> {
    use aes_gcm::aead::{Aead, KeyInit};
    let ua = p256::PublicKey::from_sec1_bytes(ua_public).map_err(|_| "the phone's key isn't a P-256 key".to_string())?;
    let as_public = as_secret.public_key().to_encoded_point(false);
    let as_public = as_public.as_bytes();
    let ecdh = p256::ecdh::diffie_hellman(as_secret.to_nonzero_scalar(), ua.as_affine());
    // Combining the shared and authentication secrets (§3.3).
    let prk_key = hmac(auth, &[ecdh.raw_secret_bytes().as_slice()]);
    let ikm = hmac(&prk_key, &[b"WebPush: info\0", ua_public, as_public, &[1]]);
    // The content key and nonce (RFC 8188 §2.2, 2.3).
    let prk = hmac(salt, &[&ikm]);
    let cek = hmac(&prk, &[b"Content-Encoding: aes128gcm\0", &[1]]);
    let nonce = hmac(&prk, &[b"Content-Encoding: nonce\0", &[1]]);
    let mut record = plaintext.to_vec();
    record.push(2); // the last record's padding delimiter
    let cipher = aes_gcm::Aes128Gcm::new_from_slice(&cek[..16]).map_err(|e| e.to_string())?;
    let sealed = cipher
        .encrypt(aes_gcm::Nonce::from_slice(&nonce[..12]), record.as_slice())
        .map_err(|_| "couldn't seal the message".to_string())?;
    let mut out = Vec::with_capacity(86 + sealed.len());
    out.extend_from_slice(salt);
    out.extend_from_slice(&RECORD_SIZE.to_be_bytes());
    out.push(as_public.len() as u8);
    out.extend_from_slice(as_public);
    out.extend_from_slice(&sealed);
    Ok(out)
}

/// `seal_with` with a fresh sender key and salt, as every message needs.
pub fn seal_for_phone(plaintext: &[u8], ua_public: &[u8], auth: &[u8]) -> Result<Vec<u8>, String> {
    let as_secret = fresh_secret();
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&crate::vault::random_bytes(16));
    seal_with(plaintext, ua_public, auth, &as_secret, &salt)
}

fn fresh_secret() -> p256::SecretKey {
    loop {
        if let Ok(k) = p256::SecretKey::from_slice(&crate::vault::random_bytes(32)) {
            return k;
        }
    }
}

// ------------------------------------------------------------------ VAPID

/// The laptop's VAPID key, made the first time it's needed and kept.
pub fn vapid_key(state_dir: &Path) -> Result<p256::SecretKey, String> {
    use p256::pkcs8::{DecodePrivateKey, EncodePrivateKey};
    let path = state_dir.join(VAPID_FILE);
    if let Ok(pem) = std::fs::read_to_string(&path) {
        return p256::SecretKey::from_pkcs8_pem(&pem).map_err(|e| format!("the push signing key at {} is damaged: {e}", path.display()));
    }
    let key = fresh_secret();
    let pem = key.to_pkcs8_pem(Default::default()).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(state_dir).map_err(|e| e.to_string())?;
    std::fs::write(&path, pem.as_bytes()).map_err(|e| format!("couldn't keep the push signing key: {e}"))?;
    Ok(key)
}

/// The public half, base64url uncompressed: what `k=` carries, and what an
/// app may register with (87 characters).
pub fn vapid_public(key: &p256::SecretKey) -> String {
    b64url(key.public_key().to_encoded_point(false).as_bytes())
}

/// The push service's origin (`https://ntfy.sh`), the audience VAPID signs.
pub fn audience(endpoint: &str) -> Option<String> {
    let (scheme, rest) = endpoint.trim().split_once("://")?;
    let host = rest.split('/').next().filter(|h| !h.is_empty())?;
    Some(format!("{scheme}://{host}"))
}

/// `Authorization: vapid t=<jwt>, k=<key>` for `endpoint`, good for 12 hours.
pub fn vapid_header(key: &p256::SecretKey, endpoint: &str, now: u64) -> Result<String, String> {
    use p256::ecdsa::signature::Signer;
    let aud = audience(endpoint).ok_or_else(|| "the push address has no server in it".to_string())?;
    let header = b64url(br#"{"typ":"JWT","alg":"ES256"}"#);
    let claims = b64url(
        serde_json::json!({ "aud": aud, "exp": now + 12 * 60 * 60, "sub": "mailto:atlas@localhost" }).to_string().as_bytes(),
    );
    let input = format!("{header}.{claims}");
    let sig: p256::ecdsa::Signature = p256::ecdsa::SigningKey::from(key).sign(input.as_bytes());
    Ok(format!("vapid t={input}.{}, k={}", b64url(&sig.to_bytes()), vapid_public(key)))
}

// ------------------------------------------------------------------ sending

/// What goes to the phone, before sealing: the title, and the line
/// `phone::body_for` would give (no detail unless allowed, never for
/// something private). The Android app reads these three fields.
pub fn android_payload(note: &crate::notify::Note, cfg: &crate::phone::PhoneConfig) -> String {
    let detail = if note.private || !cfg.include_detail {
        "Ask me when you're ready.".to_string()
    } else {
        crate::redact::Scrubber::default().scrub(&note.body)
    };
    serde_json::json!({ "title": note.title, "body": detail, "urgent": note.urgency == crate::notify::Urgency::Urgent }).to_string()
}

/// The curl arguments for one push; the headers come from a file so the
/// signature isn't on a command line other programs can read.
pub fn webpush_args(headers_file: &Path, body_file: &Path, endpoint: &str) -> Vec<String> {
    vec![
        "--silent".into(),
        "--max-time".into(),
        "10".into(),
        "--output".into(),
        "-".into(),
        "--write-out".into(),
        "\n%{http_code}".into(),
        "--header".into(),
        format!("@{}", headers_file.display()),
        "--data-binary".into(),
        format!("@{}", body_file.display()),
        endpoint.trim().into(),
    ]
}

/// What the distributor's answer means for this address.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Delivered,
    /// 404 or 410: the app unregistered or the distributor dropped it.
    Gone,
    Failed(String),
}

/// Read curl's output: the body, then the status on the last line.
pub fn read_distributor_reply(out: &str) -> Outcome {
    let (body, status) = out.rsplit_once('\n').unwrap_or(("", out));
    match status.trim() {
        "200" | "201" | "202" => Outcome::Delivered,
        "404" | "410" => Outcome::Gone,
        "" | "000" => Outcome::Failed("couldn't reach the push service".into()),
        s => Outcome::Failed(format!("the push service answered {s}: {}", body.trim().chars().take(200).collect::<String>())),
    }
}

/// Send to every Android phone the laptop knows. Ok when at least one got it.
pub fn send(note: &crate::notify::Note, cfg: &crate::phone::PhoneConfig) -> Result<(), String> {
    let state = crate::roots::state_dir();
    let mut devices = Devices::load(&state);
    if devices.devices.is_empty() {
        return Err("no Android phone has given Atlas its push address yet".into());
    }
    let key = vapid_key(&state)?;
    let payload = android_payload(note, cfg);
    let tmp = crate::roots::data_dir().join("tmp");
    crate::heard!(std::fs::create_dir_all(&tmp));
    let now = crate::store::now();
    let (mut delivered, mut why, mut gone) = (0, String::new(), Vec::new());
    for (i, d) in devices.devices.iter().enumerate() {
        let (Some(ua), Some(auth)) = (b64url_decode(&d.p256dh), b64url_decode(&d.auth)) else {
            gone.push(d.endpoint.clone());
            continue;
        };
        let outcome = (|| -> Result<Outcome, String> {
            let body = seal_for_phone(payload.as_bytes(), &ua, &auth)?;
            let headers = tmp.join(format!("webpush-{}-{i}.txt", std::process::id()));
            let body_file = tmp.join(format!("webpush-{}-{i}.bin", std::process::id()));
            std::fs::write(
                &headers,
                format!(
                    "authorization: {}\ncontent-encoding: aes128gcm\ncontent-type: application/octet-stream\nttl: {TTL_SECS}\nurgency: {}\n",
                    vapid_header(&key, &d.endpoint, now)?,
                    if note.urgency == crate::notify::Urgency::Urgent { "high" } else { "normal" }
                ),
            )
            .map_err(|e| format!("couldn't prepare the push: {e}"))?;
            std::fs::write(&body_file, &body).map_err(|e| format!("couldn't prepare the push: {e}"))?;
            let out = crate::tools::command(crate::apns::pinned_curl(&crate::roots::install_root()))
                .args(webpush_args(&headers, &body_file, &d.endpoint))
                .stderr(std::process::Stdio::null())
                .output();
            crate::heard!(std::fs::remove_file(&headers));
            crate::heard!(std::fs::remove_file(&body_file));
            let out = out.map_err(|e| format!("couldn't start curl: {e}"))?;
            Ok(read_distributor_reply(&String::from_utf8_lossy(&out.stdout)))
        })();
        match outcome {
            Ok(Outcome::Delivered) => delivered += 1,
            Ok(Outcome::Gone) => gone.push(d.endpoint.clone()),
            Ok(Outcome::Failed(w)) | Err(w) => why = w,
        }
    }
    if !gone.is_empty() {
        for e in &gone {
            devices.forget(e);
        }
        crate::kept!(devices.save(&state));
    }
    if delivered > 0 {
        Ok(())
    } else if why.is_empty() {
        Err("the Android phone's push address is no longer valid; opening Atlas on it gives a new one".into())
    } else {
        Err(why)
    }
}

// ------------------------------------------------------------------ base64url

fn b64url(bytes: &[u8]) -> String {
    crate::b64::encode(bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// base64url, padded or not, whitespace ignored.
pub fn b64url_decode(s: &str) -> Option<Vec<u8>> {
    let mut t: String = s.chars().filter(|c| !c.is_whitespace()).map(|c| match c {
        '-' => '+',
        '_' => '/',
        c => c,
    }).collect();
    let t2 = t.trim_end_matches('=').to_string();
    t = t2;
    while t.len() % 4 != 0 {
        t.push('=');
    }
    crate::b64::decode(&t).ok()
}
