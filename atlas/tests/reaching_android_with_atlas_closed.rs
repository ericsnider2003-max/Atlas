//! Item 15 for Android (decision 3, Eric's yes, 1 Oct 2026: "it must work on
//! Android too", no Google needed): the laptop seals a message for one phone
//! (Web Push, RFC 8291), signs the request (VAPID, RFC 8292) and posts it to
//! the phone's UnifiedPush address. What can be checked off the phone is
//! checked here: the sealing matches the RFC's own worked example byte for
//! byte, a phone can open what was sealed for it and nothing else can, the
//! address only arrives from your own devices, and the lock screen gets the
//! title only.

use atlas::webpush::{self, b64url_decode, Devices, Outcome};
use p256::ecdsa::signature::Verifier;
use p256::elliptic_curve::sec1::ToEncodedPoint;

fn unb64(s: &str) -> Vec<u8> {
    b64url_decode(s).expect("base64url")
}

// RFC 8291 §5 and Appendix A.
const AS_PRIVATE: &str = "yfWPiYE-n46HLnH0KqZOF1fJJU3MYrct3AELtAQ-oRw";
const UA_PUBLIC: &str = "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4";
const UA_PRIVATE: &str = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
const AUTH: &str = "BTBZMqHH6r4Tts7J_aSIgg";
const SALT: &str = "DGv6ra1nlYgDCS1FRnbzlw";
const MESSAGE: &str = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27ml\
                       mlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPT\
                       pK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";

#[test]
fn the_rfcs_worked_example_comes_out_byte_for_byte() {
    let as_secret = p256::SecretKey::from_slice(&unb64(AS_PRIVATE)).unwrap();
    let mut salt = [0u8; 16];
    salt.copy_from_slice(&unb64(SALT));
    let body = webpush::seal_with(b"When I grow up, I want to be a watermelon", &unb64(UA_PUBLIC), &unb64(AUTH), &as_secret, &salt).unwrap();
    assert_eq!(body, unb64(MESSAGE), "not RFC 8291's message: a phone would fail to open it");
    // 86 bytes of header, 42 of message with its delimiter, 16 of tag. The
    // RFC's example says "Content-Length: 145"; its own bytes are 144.
    assert_eq!(body.len(), 144);
}

/// The phone's side of RFC 8291, written here from the RFC (not from
/// `webpush`): what the Android connector does when a message arrives.
fn open_as_the_phone(body: &[u8], ua_secret: &p256::SecretKey, auth: &[u8]) -> Option<Vec<u8>> {
    use aes_gcm::aead::{Aead, KeyInit};
    use hmac::{Hmac, Mac};
    let h = |k: &[u8], parts: &[&[u8]]| -> Vec<u8> {
        let mut m = <Hmac<sha2::Sha256> as Mac>::new_from_slice(k).unwrap();
        for p in parts {
            m.update(p);
        }
        m.finalize().into_bytes().to_vec()
    };
    let salt = &body[..16];
    let idlen = body[20] as usize;
    let as_public = &body[21..21 + idlen];
    let sealed = &body[21 + idlen..];
    let ua_public = ua_secret.public_key().to_encoded_point(false);
    let shared = p256::ecdh::diffie_hellman(ua_secret.to_nonzero_scalar(), p256::PublicKey::from_sec1_bytes(as_public).ok()?.as_affine());
    let prk_key = h(auth, &[shared.raw_secret_bytes().as_slice()]);
    let ikm = h(&prk_key, &[b"WebPush: info\0", ua_public.as_bytes(), as_public, &[1]]);
    let prk = h(salt, &[&ikm]);
    let cek = h(&prk, &[b"Content-Encoding: aes128gcm\0", &[1]]);
    let nonce = h(&prk, &[b"Content-Encoding: nonce\0", &[1]]);
    let mut plain = aes_gcm::Aes128Gcm::new_from_slice(&cek[..16]).ok()?.decrypt(aes_gcm::Nonce::from_slice(&nonce[..12]), sealed).ok()?;
    assert_eq!(plain.pop(), Some(2), "the last record's delimiter");
    Some(plain)
}

#[test]
fn the_phone_opens_what_was_sealed_for_it_and_nobody_else_can() {
    let ua_secret = p256::SecretKey::from_slice(&unb64(UA_PRIVATE)).unwrap();
    let auth = unb64(AUTH);
    let words = br#"{"title":"Your 3pm call","body":"Ask me when you're ready.","urgent":false}"#;
    let a = webpush::seal(words, &unb64(UA_PUBLIC), &auth).unwrap();
    let b = webpush::seal(words, &unb64(UA_PUBLIC), &auth).unwrap();
    assert_ne!(a, b, "a fresh key and salt every time");
    assert_eq!(open_as_the_phone(&a, &ua_secret, &auth).as_deref(), Some(&words[..]));
    // Another phone, or the right phone with the wrong secret: nothing.
    let other = p256::SecretKey::from_slice(&[9u8; 32]).unwrap();
    assert_eq!(open_as_the_phone(&a, &other, &auth), None);
    assert_eq!(open_as_the_phone(&a, &ua_secret, &[0u8; 16]), None);
    // And the distributor in the middle sees no words.
    assert!(!String::from_utf8_lossy(&a).contains("3pm"));
}

#[test]
fn the_request_is_signed_with_the_laptops_own_key_kept_from_first_use() {
    let dir = std::env::temp_dir().join(format!("atlas-vapid-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let key = webpush::vapid_key(&dir).unwrap();
    assert_eq!(webpush::vapid_key(&dir).unwrap().to_bytes(), key.to_bytes(), "the same key the next time");
    let public = webpush::vapid_public(&key);
    assert_eq!(public.len(), 87, "what an app registers with: 87 base64url characters");

    let endpoint = "https://ntfy.sh/upAbc123?up=1";
    assert_eq!(webpush::audience(endpoint).as_deref(), Some("https://ntfy.sh"));
    let header = webpush::vapid_header(&key, endpoint, 1_790_000_000).unwrap();
    let (t, k) = header.strip_prefix("vapid t=").unwrap().split_once(", k=").unwrap();
    assert_eq!(k, public);
    let parts: Vec<&str> = t.split('.').collect();
    let claims: serde_json::Value = serde_json::from_slice(&unb64(parts[1])).unwrap();
    assert_eq!(claims["aud"], "https://ntfy.sh");
    assert_eq!(claims["exp"], 1_790_000_000u64 + 12 * 3600);
    let sig = p256::ecdsa::Signature::from_slice(&unb64(parts[2])).unwrap();
    p256::ecdsa::VerifyingKey::from(key.public_key())
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig)
        .expect("the push service could check it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_phones_address_is_only_taken_from_your_own_devices() {
    let dir = std::env::temp_dir().join(format!("atlas-webpush-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let endpoint = "https://ntfy.sh/upAbc123?up=1";
    let (id, field, to) = webpush::change_to_carry("erics-pixel", endpoint, UA_PUBLIC, AUTH);
    assert_eq!(field, "webpush");
    // From somebody else's bundle: never.
    assert_eq!(webpush::take_synced(&dir, &id, &to, false, 1), None);
    assert!(Devices::load(&dir).devices.is_empty());
    // From your own: kept, said once.
    assert!(webpush::take_synced(&dir, &id, &to, true, 2).unwrap().contains("erics-pixel"));
    assert_eq!(webpush::take_synced(&dir, &id, &to, true, 3), None);
    let d = Devices::load(&dir);
    assert_eq!(d.devices.len(), 1);
    assert_eq!(d.devices[0].endpoint, endpoint);
    assert!(webpush::can_reach(&dir));
    // A new address for the same phone replaces the old.
    let (id2, _, to2) = webpush::change_to_carry("erics-pixel", "https://ntfy.sh/upNew", UA_PUBLIC, AUTH);
    assert!(webpush::take_synced(&dir, &id2, &to2, true, 4).is_some());
    assert_eq!(Devices::load(&dir).devices.len(), 1);
    // Nonsense isn't an address.
    for (e, k, a) in [
        ("ftp://x/y", UA_PUBLIC, AUTH),
        ("https://ntfy.sh/a b", UA_PUBLIC, AUTH),
        (endpoint, "AAAA", AUTH),
        (endpoint, UA_PUBLIC, "AAAA"),
    ] {
        assert!(!webpush::looks_like_an_address(e, k, a), "{e} {k} {a}");
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_lock_screen_gets_the_title_and_never_something_private() {
    let mut note = atlas::notify::Note::new("Your 3pm call", "with Sam about the account 4111 1111 1111 1111", atlas::notify::Urgency::Urgent, 1);
    let mut cfg = atlas::phone::PhoneConfig::default();
    let p: serde_json::Value = serde_json::from_str(&webpush::push_payload(&note, &cfg)).unwrap();
    assert_eq!(p["title"], "Your 3pm call");
    assert_eq!(p["body"], "Ask me when you're ready.");
    assert_eq!(p["urgent"], true);
    cfg.include_detail = true;
    let p = webpush::push_payload(&note, &cfg);
    assert!(p.contains("with Sam") && !p.contains("4111 1111"), "scrubbed: {p}");
    note.private = true;
    assert!(!webpush::push_payload(&note, &cfg).contains("with Sam"));
}

#[test]
fn the_signature_and_the_sealed_body_never_sit_on_a_command_line() {
    let args = webpush::push_args(std::path::Path::new("/tmp/h.txt"), std::path::Path::new("/tmp/b.bin"), "https://ntfy.sh/up1");
    assert!(args.contains(&"@/tmp/h.txt".to_string()));
    assert!(args.contains(&"@/tmp/b.bin".to_string()));
    assert!(!args.iter().any(|a| a.contains("vapid")));
    assert_eq!(args.last().map(String::as_str), Some("https://ntfy.sh/up1"));
}

#[test]
fn the_distributors_answers_mean_what_they_say() {
    assert_eq!(webpush::read_outcome("\n201"), Outcome::Delivered);
    assert_eq!(webpush::read_outcome("{}\n200"), Outcome::Delivered);
    assert_eq!(webpush::read_outcome("gone\n410"), Outcome::Gone);
    assert_eq!(webpush::read_outcome("\n404"), Outcome::Gone);
    assert!(matches!(webpush::read_outcome("\n000"), Outcome::Failed(w) if w.contains("couldn't reach")));
    assert!(matches!(webpush::read_outcome("slow down\n429"), Outcome::Failed(w) if w.contains("429")));
}

#[test]
fn the_android_app_registers_and_hands_its_address_over() {
    let receiver = std::fs::read_to_string("mobile/android/app/src/main/java/app/atlas/PushReceiver.kt").unwrap();
    let manifest = std::fs::read_to_string("mobile/android/app/src/main/AndroidManifest.xml").unwrap();
    let main = std::fs::read_to_string("mobile/android/app/src/main/java/app/atlas/MainActivity.kt").unwrap();
    let gradle = std::fs::read_to_string("mobile/android/app/build.gradle.kts").unwrap();
    assert!(receiver.contains("/hub/web-push-endpoint") && receiver.contains("pubKeySet"));
    assert!(manifest.contains("org.unifiedpush.android.connector.PUSH_EVENT"));
    assert!(main.contains("PushReceiver.register(this)"));
    assert!(gradle.contains("org.unifiedpush.android:connector:"));
    // And the hub takes it from the app.
    assert!(matches!(
        atlas::server::route(&atlas::server::parse_request("POST /hub/web-push-endpoint HTTP/1.1", "{}").unwrap()),
        Some(atlas::server::Action::WebPushEndpoint(_))
    ));
}
