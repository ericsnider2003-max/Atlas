//! Reaching the iPhone with Atlas closed (item 15, decision 3, Eric's yes,
//! 1 Oct 2026): Apple's push service, with Eric's own key.
//!
//! The iPhone app asks iOS for a push address (a "device token") and hands
//! it to Atlas on the phone, which carries it to the laptop the way
//! everything else travels between your devices: a sync event, taken only
//! from a sealed bundle (your household key), never from anyone else
//! (`take_synced`). The laptop keeps the addresses in `data/state/apns.json`.
//!
//! To send, the laptop signs a short token with the key from Eric's Apple
//! Developer account (ES256, the only kind Apple takes; the key file never
//! leaves the laptop and is never in the repository) and posts the knock
//! over HTTP/2 to `api.push.apple.com`. Windows' own `curl.exe` has no
//! HTTP/2, so Atlas fetches curl's official Windows build itself
//! (`curl_piece`, pinned).
//!
//! What goes: the title only, as with every push (`phone::body_for`): a
//! lock screen is read by whoever holds the phone.

use crate::getpieces::{Lands, Piece};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// The sync id prefix for a device's push address.
pub const SYNC_PREFIX: &str = "pushtoken:";

/// Where the laptop keeps the addresses, in its state folder.
pub const FILE: &str = "apns.json";

/// Apple's push service.
pub const PRODUCTION: &str = "https://api.push.apple.com";
pub const SANDBOX: &str = "https://api.sandbox.push.apple.com";

/// A signed token is good for an hour; Apple asks for a new one no more
/// often than every 20 minutes. Renewed at 50.
pub const TOKEN_LIFE_SECS: u64 = 50 * 60;

#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct ApnsConfig {
    /// The .p8 key file from the Apple Developer account (Keys → Apple Push
    /// Notifications service), install-relative or absolute.
    pub key_file: String,
    /// Its Key ID (the ten characters in the file's name).
    pub key_id: String,
    /// The Apple Developer Team ID (Membership details).
    pub team_id: String,
    /// The app's bundle id.
    pub topic: String,
}

impl Default for ApnsConfig {
    fn default() -> Self {
        ApnsConfig { key_file: String::new(), key_id: String::new(), team_id: String::new(), topic: "com.ericsnider.atlas".into() }
    }
}

impl ApnsConfig {
    /// Everything needed to sign, or what's missing, in words.
    pub fn ready(&self, root: &Path) -> Result<PathBuf, String> {
        if self.key_id.trim().len() != 10 {
            return Err("the push key's ID isn't set (phone.apns.key_id)".into());
        }
        if self.team_id.trim().len() != 10 {
            return Err("the Apple Team ID isn't set (phone.apns.team_id)".into());
        }
        let p = PathBuf::from(&self.key_file);
        let p = if p.is_absolute() { p } else { root.join(p) };
        if !p.is_file() {
            return Err(format!("the push key file isn't at {}", p.display()));
        }
        Ok(p)
    }
}

/// One iPhone's push address.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Device {
    pub name: String,
    pub token: String,
    /// "production" (ad hoc and TestFlight builds) or "sandbox" (debug).
    pub env: String,
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

    /// One address per device: a new one replaces the old (iOS changes it
    /// after a restore or reinstall).
    pub fn set(&mut self, name: &str, token: &str, env: &str, now: u64) {
        self.devices.retain(|d| d.name != name);
        self.devices.push(Device { name: name.into(), token: token.into(), env: env.into(), since: now });
    }

    /// Apple said this address is gone (410): forget it.
    pub fn forget_token(&mut self, token: &str) {
        self.devices.retain(|d| d.token != token);
    }
}

/// The sync event a phone sends with its address: `(id, field, to)` for
/// `sync::What::Changed`.
pub fn change_to_carry(device: &str, token: &str, env: &str) -> (String, String, String) {
    (format!("{SYNC_PREFIX}{device}"), "apns".into(), format!("{token}|{env}"))
}

/// Is this a push address an iPhone could have given? 64 or more hex digits.
pub fn looks_like_a_token(t: &str) -> bool {
    t.len() >= 64 && t.len() <= 200 && t.chars().all(|c| c.is_ascii_hexdigit())
}

/// A phone's address arriving by sync, kept only from a sealed bundle (your
/// own devices). Returns a line to log, or `None` when it wasn't taken.
pub fn take_synced(state_dir: &Path, id: &str, to: &str, sealed: bool, now: u64) -> Option<String> {
    let device = id.strip_prefix(SYNC_PREFIX)?;
    if !sealed {
        return None;
    }
    let (token, env) = to.split_once('|').unwrap_or((to, "production"));
    if !looks_like_a_token(token) || !matches!(env, "production" | "sandbox") {
        return None;
    }
    let mut d = Devices::load(state_dir);
    if d.devices.iter().any(|x| x.name == device && x.token == token) {
        return None;
    }
    d.set(device, token, env, now);
    d.save(state_dir).ok()?;
    Some(format!("{device} can now be reached with Atlas closed"))
}

// ------------------------------------------------------------------ signing

fn b64url(bytes: &[u8]) -> String {
    crate::b64::encode(bytes).trim_end_matches('=').replace('+', "-").replace('/', "_")
}

/// The signed token Apple asks for: ES256 over `{"alg","kid"}.{"iss","iat"}`.
pub fn provider_token(key_pem: &str, key_id: &str, team_id: &str, now: u64) -> Result<String, String> {
    use p256::ecdsa::signature::Signer;
    use p256::pkcs8::DecodePrivateKey;
    let key = p256::ecdsa::SigningKey::from_pkcs8_pem(key_pem).map_err(|e| format!("the push key isn't a key Apple gives: {e}"))?;
    let header = b64url(format!("{{\"alg\":\"ES256\",\"kid\":\"{}\"}}", key_id.trim()).as_bytes());
    let claims = b64url(format!("{{\"iss\":\"{}\",\"iat\":{now}}}", team_id.trim()).as_bytes());
    let signing_input = format!("{header}.{claims}");
    let sig: p256::ecdsa::Signature = key.sign(signing_input.as_bytes());
    Ok(format!("{signing_input}.{}", b64url(&sig.to_bytes())))
}

/// What goes to the phone: the title, and the line `phone::body_for` would
/// give (no detail unless you allowed it, and never for something private).
pub fn push_payload(note: &crate::notify::Note, cfg: &crate::phone::PhoneConfig) -> String {
    let detail = if note.private || !cfg.include_detail {
        "Ask me when you're ready.".to_string()
    } else {
        crate::redact::Scrubber::default().scrub(&note.body)
    };
    serde_json::json!({
        "aps": {
            "alert": { "title": note.title, "body": detail },
            "sound": "default",
            "interruption-level": if note.urgency == crate::notify::Urgency::Urgent { "time-sensitive" } else { "active" },
        }
    })
    .to_string()
}

// ------------------------------------------------------------------ sending

/// curl's official Windows build, with HTTP/2 (Windows' own has none).
pub fn curl_piece() -> Option<Piece> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return None;
    }
    Some(Piece {
        name: "the push sender",
        for_what: "reaching your iPhone when Atlas there is closed",
        url: "https://curl.se/windows/dl-8.22.0_2/curl-8.22.0_2-win64-mingw.zip",
        sha256: "7c8c6b953b4eb2953d2bdc08cca1d5f09a964e9f86c361693559400c9a6d6db0",
        bytes: 8_690_576,
        lands: Lands::Zip { inside: "curl-8.22.0_2-win64-mingw/bin", dir: "tools/curl", key: "tools/curl/curl.exe" },
    })
}

/// The curl to use: the fetched one on Windows, the system's elsewhere.
pub(crate) fn pinned_curl(root: &Path) -> PathBuf {
    let ours = root.join("tools").join("curl").join(if cfg!(windows) { "curl.exe" } else { "curl" });
    if ours.is_file() {
        ours
    } else {
        PathBuf::from("curl")
    }
}

/// The arguments for one push. The token goes in a header file rather than
/// on the command line, where other programs could read it.
pub fn push_args(headers_file: &Path, device: &Device) -> Vec<String> {
    let host = if device.env == "sandbox" { SANDBOX } else { PRODUCTION };
    vec![
        "--http2".into(),
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
        "@-".into(),
        format!("{host}/3/device/{}", device.token),
    ]
}

/// What Apple's answer means for this address.
#[derive(Debug, Clone, PartialEq)]
pub enum Outcome {
    Delivered,
    /// 410, or 400 BadDeviceToken: the app was removed or the token changed.
    Gone,
    Failed(String),
}

/// Read curl's output: Apple's body, then the status on the last line.
pub fn read_outcome(out: &str) -> Outcome {
    let (body, status) = out.rsplit_once('\n').unwrap_or(("", out));
    match status.trim() {
        "200" => Outcome::Delivered,
        "410" => Outcome::Gone,
        "400" if body.contains("BadDeviceToken") => Outcome::Gone,
        s => Outcome::Failed(format!("Apple answered {s}: {}", body.trim())),
    }
}

/// Send to every iPhone the laptop knows. Ok when at least one got it.
pub fn send(note: &crate::notify::Note, cfg: &crate::phone::PhoneConfig) -> Result<(), String> {
    let root = crate::roots::install_root();
    let state = crate::roots::state_dir();
    let key_path = cfg.apns.ready(&root)?;
    let mut devices = Devices::load(&state);
    if devices.devices.is_empty() {
        return Err("no iPhone has given Atlas its push address yet".into());
    }
    let pem = std::fs::read_to_string(&key_path).map_err(|e| format!("couldn't read the push key: {e}"))?;
    let jwt = provider_token(&pem, &cfg.apns.key_id, &cfg.apns.team_id, crate::store::now())?;
    let body = push_payload(note, cfg);
    let headers = crate::roots::data_dir().join("tmp").join(format!("apns-{}.txt", std::process::id()));
    let _ = std::fs::create_dir_all(headers.parent().unwrap_or(&state));
    let priority = if note.urgency == crate::notify::Urgency::Urgent { "10" } else { "5" };
    std::fs::write(
        &headers,
        format!(
            "authorization: bearer {jwt}\napns-topic: {}\napns-push-type: alert\napns-priority: {priority}\ncontent-type: application/json\n",
            cfg.apns.topic.trim()
        ),
    )
    .map_err(|e| format!("couldn't prepare the push: {e}"))?;
    let mut delivered = 0;
    let mut why = String::new();
    let mut gone = Vec::new();
    for d in &devices.devices {
        match run_curl(&root, &headers, d, &body) {
            Outcome::Delivered => delivered += 1,
            Outcome::Gone => gone.push(d.token.clone()),
            Outcome::Failed(w) => why = w,
        }
    }
    let _ = std::fs::remove_file(&headers);
    if !gone.is_empty() {
        for t in &gone {
            devices.forget_token(t);
        }
        let _ = devices.save(&state);
    }
    if delivered > 0 {
        Ok(())
    } else if why.is_empty() {
        Err("the iPhone's push address is no longer valid; opening Atlas on it gives a new one".into())
    } else {
        Err(why)
    }
}

fn run_curl(root: &Path, headers: &Path, d: &Device, body: &str) -> Outcome {
    use std::io::Write;
    let mut child = match crate::tools::command(pinned_curl(root))
        .args(push_args(headers, d))
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return Outcome::Failed(format!("couldn't start curl: {e}")),
    };
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(body.as_bytes());
    }
    match child.wait_with_output() {
        Ok(o) => read_outcome(&String::from_utf8_lossy(&o.stdout)),
        Err(e) => Outcome::Failed(format!("curl didn't finish: {e}")),
    }
}
