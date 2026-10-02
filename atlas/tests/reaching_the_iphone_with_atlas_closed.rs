//! Item 15 and decision 3 (Eric's yes, 1 Oct 2026): the laptop reaches the
//! iPhone with Atlas closed, through Apple's push service and Eric's own
//! key. What can be checked off the phone is checked here: the token Apple
//! asks for is signed correctly, the phone's address only arrives from your
//! own devices, the lock screen gets the title only, and the signed token
//! never sits on a command line.

use atlas::apns::{self, ApnsConfig, Devices, Outcome};
use p256::ecdsa::signature::Verifier;
use p256::pkcs8::EncodePrivateKey;

fn a_key() -> (String, p256::ecdsa::VerifyingKey) {
    let k = p256::ecdsa::SigningKey::from_slice(&[7u8; 32]).unwrap();
    let pem = k.to_pkcs8_pem(p256::pkcs8::LineEnding::LF).unwrap().to_string();
    (pem, *k.verifying_key())
}

fn unb64url(s: &str) -> Vec<u8> {
    let mut t = s.replace('-', "+").replace('_', "/");
    while t.len() % 4 != 0 {
        t.push('=');
    }
    atlas::b64::decode(&t).unwrap()
}

#[test]
fn the_token_is_es256_with_the_key_id_and_team_and_it_verifies() {
    let (pem, public) = a_key();
    let jwt = apns::signed_token(&pem, "G3D89YSJD4", "Z6NSM9AXB7", 1_790_000_000).unwrap();
    let parts: Vec<&str> = jwt.split('.').collect();
    assert_eq!(parts.len(), 3);
    let header: serde_json::Value = serde_json::from_slice(&unb64url(parts[0])).unwrap();
    let claims: serde_json::Value = serde_json::from_slice(&unb64url(parts[1])).unwrap();
    assert_eq!(header["alg"], "ES256");
    assert_eq!(header["kid"], "G3D89YSJD4");
    assert_eq!(claims["iss"], "Z6NSM9AXB7");
    assert_eq!(claims["iat"], 1_790_000_000u64);
    // The signature is the raw 64-byte r||s Apple expects, over header.claims.
    let sig_bytes = unb64url(parts[2]);
    assert_eq!(sig_bytes.len(), 64);
    let sig = p256::ecdsa::Signature::from_slice(&sig_bytes).unwrap();
    public.verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &sig).expect("Apple could check it");
    // No padding anywhere: base64url without '='.
    assert!(!jwt.contains('='));
}

#[test]
fn a_file_that_isnt_a_key_is_said_plainly() {
    let e = apns::signed_token("not a key", "G3D89YSJD4", "Z6NSM9AXB7", 1).unwrap_err();
    assert!(e.contains("isn't a key Apple gives"), "{e}");
    // While a real key signs.
    assert_eq!(apns::signed_token(&a_key().0, "G3D89YSJD4", "Z6NSM9AXB7", 1).map(|t| t.split('.').count()), Ok(3));
}

#[test]
fn the_phones_address_is_only_taken_from_your_own_devices() {
    let dir = std::env::temp_dir().join(format!("atlas-apns-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let token = "a".repeat(64);
    let (id, field, to) = apns::change_to_carry("erics-iphone", &token, "production");
    assert_eq!(field, "apns");
    // Unsealed (not your household key): ignored.
    assert!(apns::take_synced(&dir, &id, &to, false, 100).is_none());
    assert!(Devices::load(&dir).devices.is_empty());
    // Sealed: kept, and said.
    let said = apns::take_synced(&dir, &id, &to, true, 100).expect("kept");
    assert!(said.contains("erics-iphone"), "{said}");
    // The same again: nothing new to say.
    assert!(apns::take_synced(&dir, &id, &to, true, 200).is_none());
    // A new address replaces the old (a restore or a reinstall).
    let newer = "b".repeat(64);
    let (_, _, to2) = apns::change_to_carry("erics-iphone", &newer, "production");
    apns::take_synced(&dir, &id, &to2, true, 300).unwrap();
    let d = Devices::load(&dir);
    assert_eq!(d.devices.len(), 1);
    assert_eq!(d.devices[0].token, newer);
    // Something that isn't a push address is refused.
    assert!(apns::take_synced(&dir, &id, "hello|production", true, 400).is_none());
    assert!(apns::take_synced(&dir, &id, &format!("{newer}|elsewhere"), true, 400).is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_lock_screen_gets_the_title_and_never_the_detail_unless_allowed() {
    let note = atlas::notify::Note::new("Your render finished", "card 4111 1111 1111 1111 on the invoice", atlas::notify::Urgency::Routine, 1);
    let cfg = atlas::phone::PhoneConfig::default();
    let p: serde_json::Value = serde_json::from_str(&apns::payload(&note, &cfg)).unwrap();
    assert_eq!(p["aps"]["alert"]["title"], "Your render finished");
    assert_eq!(p["aps"]["alert"]["body"], "Ask me when you're ready.");
    assert!(!apns::payload(&note, &cfg).contains("4111"));
}

#[test]
fn the_signed_token_goes_in_a_header_file_not_on_the_command_line() {
    let d = apns::Device { name: "p".into(), token: "c".repeat(64), env: "production".into(), since: 0 };
    let args = apns::curl_args(std::path::Path::new("/tmp/h.txt"), &d);
    assert!(args.contains(&"--http2".to_string()));
    assert!(args.contains(&"@/tmp/h.txt".to_string()));
    assert!(!args.iter().any(|a| a.contains("bearer")));
    assert!(args.last().unwrap().starts_with("https://api.push.apple.com/3/device/"));
    let sandbox = apns::Device { env: "sandbox".into(), ..d };
    assert!(apns::curl_args(std::path::Path::new("h"), &sandbox).last().unwrap().starts_with(apns::SANDBOX));
}

#[test]
fn apples_answers_are_read_and_a_dead_address_is_dropped() {
    assert_eq!(apns::read_outcome("\n200"), Outcome::Delivered);
    assert_eq!(apns::read_outcome("{\"reason\":\"Unregistered\"}\n410"), Outcome::Gone);
    assert_eq!(apns::read_outcome("{\"reason\":\"BadDeviceToken\"}\n400"), Outcome::Gone);
    assert!(matches!(apns::read_outcome("{\"reason\":\"ExpiredProviderToken\"}\n403"), Outcome::Failed(w) if w.contains("403")));
    let mut d = Devices::default();
    d.set("a", &"1".repeat(64), "production", 1);
    d.set("b", &"2".repeat(64), "production", 1);
    d.forget_token(&"1".repeat(64));
    assert_eq!(d.devices.len(), 1);
}

#[test]
fn the_key_must_be_set_up_before_anything_is_signed() {
    let root = std::env::temp_dir();
    let mut c = ApnsConfig::default();
    assert!(c.ready(&root).unwrap_err().contains("key_id"));
    c.key_id = "G3D89YSJD4".into();
    assert!(c.ready(&root).unwrap_err().contains("Team ID"));
    c.team_id = "Z6NSM9AXB7".into();
    c.key_file = "no-such-key.p8".into();
    assert!(c.ready(&root).unwrap_err().contains("isn't at"));
    assert_eq!(c.topic, "com.ericsnider.atlas");
}

#[test]
fn curl_with_http2_is_pinned_for_windows() {
    if let Some(p) = apns::curl_piece() {
        assert_eq!(p.key_path(), "tools/curl/curl.exe");
        assert_eq!(p.sha256.len(), 64);
    } else {
        assert!(!cfg!(all(windows, target_arch = "x86_64")));
    }
}

#[test]
fn the_iphone_app_asks_for_its_address_and_hands_it_over() {
    let app = std::fs::read_to_string("mobile/ios/Atlas/AtlasApp.swift").unwrap();
    let core = std::fs::read_to_string("mobile/ios/Atlas/AtlasCore.swift").unwrap();
    let ent = std::fs::read_to_string("mobile/ios/Atlas/Atlas.entitlements").unwrap();
    assert!(app.contains("registerForRemoteNotifications()"));
    assert!(app.contains("didRegisterForRemoteNotificationsWithDeviceToken"));
    assert!(core.contains("/hub/push-token"));
    assert!(ent.contains("<key>aps-environment</key>"));
}
