//! Microsoft's OAuth2 device code flow — the one real way into Outlook
//! and Microsoft 365 mail, now that password-based IMAP and SMTP are
//! gone entirely.
//!
//! One thing this module cannot do for you: get a client ID. That means
//! registering an app in Azure AD yourself — Microsoft's own portal,
//! your own Microsoft account, a few clicks. No code here can do that
//! part; it's the one manual step everything else is built to need only
//! once. See `SETUP` below for exactly what to do.
//!
//! Everything past that is real: request a device code, show you the
//! short code and the URL, poll until you've approved it somewhere else
//! (your phone, another tab — never a browser Atlas drives itself, which
//! is the whole point of this flow existing), then hold the refresh
//! token in the vault and mint a fresh access token before every
//! connection rather than caching one that might have gone stale.
//!
//! Uses `curl` for the HTTP, the same as the unsubscribe one-click POST —
//! this is exactly curl's strong protocol, unlike the IMAP support that
//! ruled it out for the mail client itself.

use serde::Deserialize;

/// What you need to have done once, in Microsoft's own portal, before any
/// of this works. Written down here rather than only in a README, so
/// whatever surfaces this error can hand the person the actual steps.
pub const SETUP: &str = "\
1. Go to https://portal.azure.com, sign in, and open 'App registrations'.
2. 'New registration' — any name, 'Accounts in any organizational \
directory and personal Microsoft accounts' as the supported account \
type, no redirect URI.
3. After it's created, open 'Authentication', and under 'Advanced \
settings' turn on 'Allow public client flows'.
4. Open 'API permissions', add: IMAP.AccessAsUser.All, SMTP.Send, and \
offline_access (all under 'Delegated permissions', Office 365 Exchange \
Online / Microsoft Graph).
5. Copy the 'Application (client) ID' from the Overview page — that's \
the client_id this needs.";

const SCOPE: &str = "https://outlook.office.com/IMAP.AccessAsUser.All \
                      https://outlook.office.com/SMTP.Send offline_access";
const DEVICE_CODE_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/devicecode";
const TOKEN_URL: &str = "https://login.microsoftonline.com/common/oauth2/v2.0/token";
const GRANT_TYPE_DEVICE: &str = "urn:ietf:params:oauth:grant-type:device_code";

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct DeviceCode {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub expires_in: u64,
    /// Seconds to wait between poll attempts — the server's own choice,
    /// not a fixed guess. Polling faster than this gets you
    /// `slow_down`, per the protocol's own rule.
    pub interval: u64,
    /// The sentence Microsoft already wrote for exactly this moment —
    /// "go to <url> and enter <code>" — used as-is rather than
    /// reassembled from the other fields, since it's the one they
    /// actually tested reads correctly out loud.
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Tokens {
    pub access_token: String,
    /// Microsoft rotates it: each refresh may hand back a new one, and the
    /// old one stops working once the new one has been used (Q7).
    #[serde(default)]
    pub refresh_token: String,
    pub expires_in: u64,
}

/// What a still-pending poll looks like, versus a real failure. RFC
/// 8628's own vocabulary: `authorization_pending` means keep waiting,
/// `slow_down` means keep waiting *and* poll less often, everything
/// else means stop.
#[derive(Debug, Clone, PartialEq)]
pub enum PollOutcome {
    Ready(Tokens),
    Pending,
    SlowDown,
    Denied,
    Expired,
    Other(String),
}

/// Step one: ask for a device code. Returns what to show the person —
/// the code and the URL — and what to poll with next.
pub fn request_device_code(client_id: &str) -> Result<DeviceCode, String> {
    let body = post_form(DEVICE_CODE_URL, &[("client_id", client_id), ("scope", SCOPE)])?;
    serde_json::from_str(&body).map_err(|e| format!("couldn't read Microsoft's own response: {e}"))
}

/// One poll of the token endpoint. Called in a loop by whatever's
/// driving the flow, sleeping `interval` seconds — or longer, after a
/// `SlowDown` — between calls; this function itself does not sleep, so
/// it stays testable without a real clock.
pub fn poll_once(client_id: &str, device_code: &str) -> PollOutcome {
    let result = post_form(
        TOKEN_URL,
        &[
            ("grant_type", GRANT_TYPE_DEVICE),
            ("client_id", client_id),
            ("device_code", device_code),
        ],
    );
    let body = match result {
        Ok(b) => b,
        Err(e) => return PollOutcome::Other(e),
    };
    parse_poll_response(&body)
}

/// Trades a stored refresh token for a fresh access token. Mail asks
/// through `access`, which keeps the token while it has five minutes left:
/// the old rule here, a fresh one before every connection, made four
/// requests a check and let two threads refresh one sign-in together.
pub fn refresh(client_id: &str, refresh_token: &str) -> Result<Tokens, String> {
    let using = current(refresh_token);
    let body = post_form(
        TOKEN_URL,
        &[
            ("grant_type", "refresh_token"),
            ("client_id", client_id),
            ("refresh_token", &using),
            ("scope", SCOPE),
        ],
    )?;
    let tokens: Tokens = serde_json::from_str(&body).map_err(|_| match serde_json::from_str::<serde_json::Value>(&body) {
        Ok(v) if v.get("error").and_then(|e| e.as_str()) == Some("invalid_grant") => {
            "Microsoft no longer accepts Atlas's sign-in for this account (revoked or expired) -- sign in again".to_string()
        }
        Ok(v) => format!("Microsoft said {}", v.get("error").and_then(|e| e.as_str()).unwrap_or("something unexpected")),
        Err(e) => format!("couldn't read Microsoft's own response: {e}"),
    })?;
    rotated(refresh_token, &tokens.refresh_token);
    Ok(tokens)
}

/// An access token for mail, from the stored refresh token: the one kept
/// while it has more than five minutes left, else a fresh one -- fetched by
/// one thread at a time, so the four places mail connects never refresh the
/// same sign-in together (`connect::access_once`, N5).
pub fn access(client_id: &str, refresh_token: &str) -> Result<String, String> {
    let key = format!("ms-mail {client_id} {refresh_token}");
    crate::connect::access_once(&key, crate::store::now(), || refresh(client_id, refresh_token).map(|t| (t.access_token, t.expires_in)))
}

// ------------------------------------------------------------ rotation (Q7)
//
// Microsoft hands back a new refresh token with most refreshes and retires
// the old one after a while; Atlas kept using the one from the day it signed
// in, so every Outlook sign-in quietly expired. The new one is kept here at
// once (so this process keeps working) and queued for the vault, which the
// tick writes (`connecting::keep_rotated`) -- the mail check runs on a crew
// thread with no vault in reach.

static LATEST: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());
static TO_KEEP: std::sync::Mutex<Vec<(String, String)>> = std::sync::Mutex::new(Vec::new());

/// The newest refresh token known for one first given as `original`.
pub fn current(original: &str) -> String {
    let latest = LATEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    latest.iter().rev().find(|(o, _)| o == original).map(|(_, n)| n.clone()).unwrap_or_else(|| original.to_string())
}

/// Note a refresh that came back with a different refresh token.
pub fn rotated(original: &str, new: &str) {
    if new.is_empty() || new == original || new == current(original) {
        return;
    }
    let mut latest = LATEST.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    latest.retain(|(o, _)| o != original);
    latest.push((original.to_string(), new.to_string()));
    drop(latest);
    TO_KEEP.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push((original.to_string(), new.to_string()));
}

/// Rotations waiting to be written to the vault: (stored, newer).
pub fn take_rotations() -> Vec<(String, String)> {
    std::mem::take(&mut *TO_KEEP.lock().unwrap_or_else(std::sync::PoisonError::into_inner))
}

/// Put back what couldn't be kept yet (the vault was locked).
pub fn keep_later(left: Vec<(String, String)>) {
    TO_KEEP.lock().unwrap_or_else(std::sync::PoisonError::into_inner).extend(left);
}

fn parse_poll_response(body: &str) -> PollOutcome {
    if let Ok(tokens) = serde_json::from_str::<Tokens>(body) {
        return PollOutcome::Ready(tokens);
    }
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    match serde_json::from_str::<ErrorBody>(body) {
        Ok(e) => match e.error.as_str() {
            "authorization_pending" => PollOutcome::Pending,
            "slow_down" => PollOutcome::SlowDown,
            "authorization_declined" => PollOutcome::Denied,
            "expired_token" | "bad_verification_code" => PollOutcome::Expired,
            other => PollOutcome::Other(other.to_string()),
        },
        Err(_) => PollOutcome::Other(format!("unrecognised response: {body}")),
    }
}

/// The `AUTHENTICATE XOAUTH2` / `AUTH XOAUTH2` payload both IMAP and
/// SMTP want: `user=<address>\x01auth=Bearer <token>\x01\x01`, base64'd.
/// The two `\x01`s are not typos — the format wants a trailing empty
/// field, and an implementation that leaves one off gets a cryptic
/// failure from the server instead of a clear "malformed" one.
pub fn xoauth2_string(user: &str, access_token: &str) -> String {
    let raw = format!("user={user}\x01auth=Bearer {access_token}\x01\x01");
    crate::b64::encode(raw.as_bytes())
}

/// A form POST over Atlas's own TLS client. It ran `curl` with the refresh
/// token on the command line, where any process on the machine can read it,
/// and found `curl` by searching PATH (Q7, Q9).
fn post_form(url: &str, fields: &[(&str, &str)]) -> Result<String, String> {
    let rest = url.strip_prefix("https://").ok_or("Microsoft's address must be https")?;
    let (host, path) = rest.split_once('/').map(|(h, p)| (h, format!("/{p}"))).unwrap_or((rest, "/".into()));
    let form = fields
        .iter()
        .map(|(k, v)| format!("{}={}", crate::research::urlencode(k), crate::research::urlencode(v)))
        .collect::<Vec<_>>()
        .join("&");
    let (resp, _) = crate::http::https_call(
        "POST",
        host,
        &path,
        &[("User-Agent", "PersonalAtlas/1.0")],
        Some(("application/x-www-form-urlencoded", &form)),
        std::time::Duration::from_secs(20),
    )
    .map_err(|e| format!("couldn't reach Microsoft: {e}"))?;
    Ok(resp.body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_code_response_parses_from_real_shaped_json() {
        let json = r#"{
            "device_code": "GMMhmHCXhWEz...",
            "user_code": "FWTY2QGCN",
            "verification_uri": "https://microsoft.com/devicelogin",
            "expires_in": 900,
            "interval": 5,
            "message": "To sign in, use a web browser to open the page https://microsoft.com/devicelogin and enter the code FWTY2QGCN to authenticate."
        }"#;
        let dc: DeviceCode = serde_json::from_str(json).unwrap();
        assert_eq!(dc.user_code, "FWTY2QGCN");
        assert_eq!(dc.interval, 5);
    }

    #[test]
    fn a_ready_token_response_is_recognised() {
        let json = r#"{"token_type":"Bearer","expires_in":3599,"access_token":"eyJ...","refresh_token":"AwAB..."}"#;
        match parse_poll_response(json) {
            PollOutcome::Ready(t) => {
                assert_eq!(t.access_token, "eyJ...");
                assert_eq!(t.refresh_token, "AwAB...");
            }
            other => panic!("expected Ready, got {other:?}"),
        }
    }

    #[test]
    fn authorization_pending_is_recognised_as_keep_waiting() {
        let json = r#"{"error":"authorization_pending","error_description":"..."}"#;
        assert_eq!(parse_poll_response(json), PollOutcome::Pending);
    }

    #[test]
    fn slow_down_is_distinguished_from_plain_pending() {
        // Distinguished on purpose -- a poller that treats slow_down the
        // same as authorization_pending keeps polling at the same rate,
        // which is the exact thing the server just asked it not to do.
        let json = r#"{"error":"slow_down"}"#;
        assert_eq!(parse_poll_response(json), PollOutcome::SlowDown);
    }

    #[test]
    fn authorization_declined_is_recognised_as_a_real_no() {
        let json = r#"{"error":"authorization_declined"}"#;
        assert_eq!(parse_poll_response(json), PollOutcome::Denied);
    }

    #[test]
    fn expired_token_and_bad_verification_code_both_mean_start_over() {
        assert_eq!(parse_poll_response(r#"{"error":"expired_token"}"#), PollOutcome::Expired);
        assert_eq!(parse_poll_response(r#"{"error":"bad_verification_code"}"#), PollOutcome::Expired);
    }

    #[test]
    fn an_unrecognised_error_is_carried_through_rather_than_swallowed() {
        match parse_poll_response(r#"{"error":"invalid_client"}"#) {
            PollOutcome::Other(msg) => assert_eq!(msg, "invalid_client"),
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn garbage_that_is_not_json_at_all_does_not_panic() {
        match parse_poll_response("this is not json") {
            PollOutcome::Other(_) => {}
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn xoauth2_string_has_both_trailing_control_characters() {
        let s = xoauth2_string("me@outlook.com", "abc123");
        let decoded = String::from_utf8(crate::b64::decode(&s).unwrap()).unwrap();
        assert_eq!(decoded, "user=me@outlook.com\x01auth=Bearer abc123\x01\x01");
    }

    #[test]
    fn xoauth2_string_is_valid_base64() {
        let s = xoauth2_string("me@outlook.com", "abc123");
        assert!(crate::b64::decode(&s).is_ok());
    }
}
