//! One-click sign-in with Google and Microsoft, for calendars and Outlook
//! mail.
//!
//! The person clicks a button, signs in on Google's or Microsoft's own page
//! in their own browser, and approves read access. Atlas never sees the
//! password. What comes back is a refresh token, kept sealed in the vault,
//! and traded for a short-lived access token each time something is read.
//!
//! Atlas is registered once, by its maker, with each provider. The two
//! client IDs below are that registration. They are public by design: every
//! copy of Atlas carries them, and they identify the app, not anyone's
//! account.
//!
//! - **Flow:** OAuth 2.0 authorization code with PKCE (RFC 7636), redirected
//!   to a port on this machine (RFC 8252's loopback redirect). The PKCE
//!   verifier never leaves this machine, so a code caught on the way back is
//!   useless to anyone else.
//! - **Google** still asks a "Desktop app" client for its client secret at
//!   the token endpoint, while saying itself that such a secret can't be kept
//!   secret. It isn't written in the source: it is compiled in from
//!   `ATLAS_GOOGLE_CLIENT_SECRET`, or read from that variable at run time.
//!   A copy built without it says so instead of half-working.
//! - **Microsoft** needs no secret at all for a public client.
//! - **What's asked for:** read-only calendars from both; Outlook mail over
//!   IMAP (read) and SMTP (send replies you approve) from Microsoft. Gmail
//!   isn't here: full Gmail access is a "restricted" scope that Google only
//!   grants after a paid security audit, so Gmail keeps its app password.
//!
//! Calendars come back as JSON and are turned into iCalendar text, so they
//! go through the same import as a calendar connected by its private link.

use crate::social::apis::Net;

/// Atlas's registration with Google (a "Desktop app" client).
pub const GOOGLE_CLIENT_ID: &str = "372645916141-182rrs5j6r9f7o9uhuumm2qivdukvlnb.apps.googleusercontent.com";

/// Atlas's registration with Microsoft (a public client, any Microsoft
/// account: personal and work or school).
pub const MICROSOFT_CLIENT_ID: &str = "0c54be08-ddff-4d25-ab6f-9791070e3b2a";

/// Google's client secret for the Desktop client: compiled in, or from the
/// environment at run time. `None` in a copy built without it.
pub fn google_secret() -> Option<String> {
    let built = option_env!("ATLAS_GOOGLE_CLIENT_SECRET").map(str::to_string);
    built
        .or_else(|| std::env::var("ATLAS_GOOGLE_CLIENT_SECRET").ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Which sign-in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Google,
    Microsoft,
}

impl Provider {
    pub fn from_key(k: &str) -> Option<Provider> {
        match k {
            "google" => Some(Provider::Google),
            "microsoft" => Some(Provider::Microsoft),
            _ => None,
        }
    }
    pub fn key(self) -> &'static str {
        match self {
            Provider::Google => "google",
            Provider::Microsoft => "microsoft",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Provider::Google => "Google",
            Provider::Microsoft => "Microsoft",
        }
    }
    /// What this sign-in connects, said on its button.
    pub fn connects(self) -> &'static str {
        match self {
            Provider::Google => "your Google Calendar",
            Provider::Microsoft => "your Outlook mail and calendar",
        }
    }
    /// The redirect back to this machine. Google's Desktop clients take any
    /// port on 127.0.0.1; Microsoft's registration is `http://localhost`,
    /// where any port is allowed.
    pub fn redirect(self, port: u16) -> String {
        match self {
            Provider::Google => format!("http://127.0.0.1:{port}"),
            Provider::Microsoft => format!("http://localhost:{port}"),
        }
    }
}

const GOOGLE_SCOPE: &str = "openid email https://www.googleapis.com/auth/calendar.readonly";
/// Consent for everything at once (mail and calendar are two Microsoft
/// resources, so the token endpoint is asked for one resource at a time).
const MS_CONSENT: &str = "openid email offline_access https://outlook.office.com/IMAP.AccessAsUser.All \
https://outlook.office.com/SMTP.Send https://graph.microsoft.com/Calendars.Read";
/// The mail resource's scopes: what the code is redeemed for, and what
/// `msoauth::refresh` asks for before each mail check.
const MS_MAIL: &str = "openid email offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send";
const MS_CALENDAR: &str = "offline_access https://graph.microsoft.com/Calendars.Read";

fn enc(s: &str) -> String {
    crate::research::urlencode(s)
}

/// The page to send the browser to.
pub fn consent_url(p: Provider, redirect: &str, state: &str, challenge: &str) -> String {
    match p {
        Provider::Google => format!(
            "https://accounts.google.com/o/oauth2/v2/auth?client_id={}&redirect_uri={}&response_type=code&scope={}\
             &state={}&code_challenge={}&code_challenge_method=S256&access_type=offline&prompt=consent",
            enc(GOOGLE_CLIENT_ID),
            enc(redirect),
            enc(GOOGLE_SCOPE),
            enc(state),
            enc(challenge)
        ),
        Provider::Microsoft => format!(
            "https://login.microsoftonline.com/common/oauth2/v2.0/authorize?client_id={}&redirect_uri={}\
             &response_type=code&response_mode=query&scope={}&state={}&code_challenge={}&code_challenge_method=S256&prompt=select_account",
            enc(MICROSOFT_CLIENT_ID),
            enc(redirect),
            enc(MS_CONSENT),
            enc(state),
            enc(challenge)
        ),
    }
}

/// PKCE's S256 challenge for a verifier.
pub fn challenge(verifier: &str) -> String {
    use sha2::Digest;
    let d = sha2::Sha256::digest(verifier.as_bytes());
    crate::b64::encode(&d).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

/// The code from the request the provider's redirect makes to this
/// machine, when its state is the one sent.
pub fn code_from(p: Provider, request_line: &str, state: &str) -> Result<String, String> {
    let target = request_line.split_whitespace().nth(1).unwrap_or("");
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or("");
    let (mut code, mut got_state, mut error, mut detail) = (None, None, None, None);
    for kv in query.split('&') {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        let v = crate::hub::urldecode(&v.replace('+', " "));
        match k {
            "code" => code = Some(v),
            "state" => got_state = Some(v),
            "error" => error = Some(v),
            "error_description" => detail = Some(v),
            _ => {}
        }
    }
    if let Some(e) = error {
        return Err(match e.as_str() {
            "access_denied" => "you didn't allow it, so nothing was connected".into(),
            _ => format!("{} said no ({e}{})", p.name(), detail.map(|d| format!(": {d}")).unwrap_or_default()),
        });
    }
    if got_state.as_deref() != Some(state) {
        return Err("that answer wasn't for this sign-in (its state didn't match), so I ignored it".into());
    }
    code.filter(|c| !c.is_empty()).ok_or_else(|| format!("{} came back without a code", p.name()))
}

/// What a finished sign-in gives: whose account, and the refresh token.
#[derive(Debug, Clone, PartialEq)]
pub struct SignedIn {
    pub provider: Provider,
    pub email: String,
    pub refresh_token: String,
}

/// The form that redeems the code.
pub fn exchange_form(p: Provider, code: &str, redirect: &str, verifier: &str, secret: Option<&str>) -> String {
    match p {
        Provider::Google => format!(
            "grant_type=authorization_code&code={}&client_id={}&client_secret={}&redirect_uri={}&code_verifier={}",
            enc(code),
            enc(GOOGLE_CLIENT_ID),
            enc(secret.unwrap_or("")),
            enc(redirect),
            enc(verifier)
        ),
        Provider::Microsoft => format!(
            "grant_type=authorization_code&code={}&client_id={}&redirect_uri={}&code_verifier={}&scope={}",
            enc(code),
            enc(MICROSOFT_CLIENT_ID),
            enc(redirect),
            enc(verifier),
            enc(MS_MAIL)
        ),
    }
}

fn token_endpoint(p: Provider) -> (&'static str, &'static str) {
    match p {
        Provider::Google => ("oauth2.googleapis.com", "/token"),
        Provider::Microsoft => ("login.microsoftonline.com", "/common/oauth2/v2.0/token"),
    }
}

fn json(body: &str, who: &str) -> Result<serde_json::Value, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|_| format!("{who} answered with something that isn't JSON"))?;
    if let Some(e) = v.get("error") {
        let code = e.as_str().map(str::to_string).or_else(|| e.get("code").and_then(|c| c.as_str()).map(str::to_string)).unwrap_or_default();
        let said = v
            .get("error_description")
            .and_then(|d| d.as_str())
            .or_else(|| e.get("message").and_then(|m| m.as_str()))
            .unwrap_or("")
            .lines()
            .next()
            .unwrap_or("")
            .to_string();
        return Err(match code.as_str() {
            "invalid_grant" => format!("{who} no longer accepts Atlas's sign-in (it was revoked or expired) -- sign in again"),
            _ => format!("{who} said {code}{}", if said.is_empty() { String::new() } else { format!(": {said}") }),
        });
    }
    Ok(v)
}

/// The email inside an ID token (a JWT): its middle part, base64url JSON.
/// Read only for the address to label the account with; the token came
/// straight from the provider over TLS, which is what vouches for it.
fn email_in_id_token(id_token: &str) -> Option<String> {
    let mid = id_token.split('.').nth(1)?;
    let mut b = mid.replace('-', "+").replace('_', "/");
    while b.len() % 4 != 0 {
        b.push('=');
    }
    let raw = crate::b64::decode(&b).ok()?;
    let v: serde_json::Value = serde_json::from_slice(&raw).ok()?;
    ["email", "preferred_username", "upn"]
        .iter()
        .find_map(|k| v.get(*k).and_then(|e| e.as_str()).filter(|e| e.contains('@')).map(|e| e.trim().to_lowercase()))
}

/// Read the token endpoint's answer to a code.
pub fn signed_in_from(p: Provider, body: &str) -> Result<SignedIn, String> {
    let v = json(body, p.name())?;
    let refresh = v.get("refresh_token").and_then(|t| t.as_str()).unwrap_or("").to_string();
    if refresh.is_empty() {
        return Err(format!(
            "{} gave no lasting sign-in, so nothing was connected. Remove Atlas from your account's connected apps and try again.",
            p.name()
        ));
    }
    let email = v
        .get("id_token")
        .and_then(|t| t.as_str())
        .and_then(email_in_id_token)
        .ok_or_else(|| format!("{} didn't say which account this is, so nothing was connected", p.name()))?;
    Ok(SignedIn { provider: p, email, refresh_token: refresh })
}

/// Swap the code for the sign-in.
pub fn exchange(net: &dyn Net, p: Provider, code: &str, redirect: &str, verifier: &str) -> Result<SignedIn, String> {
    let secret = google_secret();
    if p == Provider::Google && secret.is_none() {
        return Err(NO_GOOGLE_SECRET.into());
    }
    let (host, path) = token_endpoint(p);
    let r = net.post_form(host, path, &exchange_form(p, code, redirect, verifier, secret.as_deref()))?;
    signed_in_from(p, &r.body)
}

pub const NO_GOOGLE_SECRET: &str = "This copy of Atlas was built without Google's sign-in key, so Google can't be connected \
from it. Connect the calendar by its private link instead.";

/// A fresh access token for reading calendars.
///
/// Kept while it has more than five minutes left, and fetched by one thread
/// at a time (`connect::access_once`, N5).
fn calendar_access(net: &dyn Net, p: Provider, refresh_token: &str) -> Result<String, String> {
    let key = format!("calendar {} {refresh_token}", p.key());
    crate::connect::access_once(&key, crate::store::now(), || fetch_calendar_access(net, p, refresh_token))
}

fn fetch_calendar_access(net: &dyn Net, p: Provider, refresh_token: &str) -> Result<(String, u64), String> {
    let form = match p {
        Provider::Google => format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}&client_secret={}",
            enc(refresh_token),
            enc(GOOGLE_CLIENT_ID),
            enc(&google_secret().ok_or(NO_GOOGLE_SECRET)?)
        ),
        Provider::Microsoft => format!(
            "grant_type=refresh_token&refresh_token={}&client_id={}&scope={}",
            enc(&crate::msoauth::current(refresh_token)),
            enc(MICROSOFT_CLIENT_ID),
            enc(MS_CALENDAR)
        ),
    };
    let (host, path) = token_endpoint(p);
    let v = json(&net.post_form(host, path, &form)?.body, p.name())?;
    // Microsoft rotates refresh tokens; the new one is kept (Q7).
    if let Some(newer) = v.get("refresh_token").and_then(|t| t.as_str()) {
        crate::msoauth::rotated(refresh_token, newer);
    }
    let token = v.get("access_token").and_then(|t| t.as_str()).map(str::to_string).ok_or_else(|| format!("{} gave no access token", p.name()))?;
    Ok((token, v.get("expires_in").and_then(|e| e.as_u64()).unwrap_or(3600)))
}

/// Take back a Google sign-in at Google itself, not only out of the vault:
/// Google's revocation endpoint, which ends the refresh token and the access
/// granted with it. Disconnecting should mean disconnected (N5).
///
/// Google ends the whole grant -- every scope this app was given on that
/// Google account -- so a caller only does this when nothing else Atlas
/// keeps rides the same grant (`connecting`, the YouTube sign-in).
pub fn revoke_google(net: &dyn Net, refresh_token: &str) -> Result<(), String> {
    let r = net.post_form("oauth2.googleapis.com", "/revoke", &format!("token={}", enc(refresh_token)))?;
    // 400 invalid_token: already gone, which is what was wanted.
    let already_gone = r.status == 400 && serde_json::from_str::<serde_json::Value>(&r.body).ok().is_some_and(|body| body.get("error").and_then(|error| error.as_str()) == Some("invalid_token"));
    if (200..300).contains(&r.status) || already_gone {
        crate::connect::forget_access(&format!("calendar {} {refresh_token}", Provider::Google.key()));
        return Ok(());
    }
    Err(format!("Google answered {}", r.status))
}

/// Where you take back Microsoft's permission yourself: Microsoft offers an
/// app like Atlas (a public client) no way to end its own grant.
pub const MICROSOFT_PERMISSIONS: &str = "https://account.live.com/consent/Manage";

// ------------------------------------------------------------ calendars

/// How far either side of now is read: a week back, ten weeks on.
pub const BACK_SECS: u64 = 7 * 86_400;
pub const AHEAD_SECS: u64 = 70 * 86_400;

/// `2026-10-05T16:00:00Z` for an instant.
pub fn utc_stamp(t: u64) -> String {
    let days = (t / 86_400) as i64;
    let s = t % 86_400;
    let (y, m, d) = crate::civil::civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", s / 3600, s / 60 % 60, s % 60)
}

/// An RFC 3339 time (`2026-10-05T09:00:00-07:00`, `...Z`, fractions
/// allowed) as seconds since the epoch. With no zone at all it's taken as
/// UTC, which is what Microsoft sends when asked for UTC.
pub fn epoch_of(s: &str) -> Option<i64> {
    let s = s.trim();
    let (date, rest) = s.split_once('T')?;
    let mut dp = date.split('-');
    let (y, m, d): (i64, u32, u32) = (dp.next()?.parse().ok()?, dp.next()?.parse().ok()?, dp.next()?.parse().ok()?);
    let (clock, offset) = match rest.find(['Z', 'z', '+']).or_else(|| rest.rfind('-')) {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    let mut cp = clock.split(':');
    let h: i64 = cp.next()?.parse().ok()?;
    let mi: i64 = cp.next()?.parse().ok()?;
    let sec: i64 = cp.next().map(|x| x.split('.').next().unwrap_or("0")).unwrap_or("0").parse().ok()?;
    let off = match offset {
        "" | "Z" | "z" => 0,
        o => {
            let sign = if o.starts_with('-') { -1 } else { 1 };
            let mut op = o[1..].split(':');
            let oh: i64 = op.next()?.parse().ok()?;
            let om: i64 = op.next().unwrap_or("0").parse().ok()?;
            sign * (oh * 3600 + om * 60)
        }
    };
    Some(crate::civil::days_from_civil(y, m, d) * 86_400 + h * 3600 + mi * 60 + sec - off)
}

fn ics_utc(t: i64) -> String {
    utc_stamp(t.max(0) as u64).replace(['-', ':'], "")
}

fn ics_text(s: &str) -> String {
    s.replace('\\', "\\\\").replace(';', "\\;").replace(',', "\\,").replace(['\r', '\n'], " ")
}

/// One event, as the iCalendar text the calendar import reads.
struct Ev {
    uid: String,
    summary: String,
    location: String,
    /// `Ok(utc seconds)` for a timed event, `Err("YYYYMMDD")` for a day.
    start: Result<i64, String>,
    end: Result<i64, String>,
}

fn vcalendar(events: &[Ev]) -> String {
    let mut out = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//Atlas//oauthlink//EN\r\n");
    for e in events {
        out.push_str("BEGIN:VEVENT\r\n");
        out.push_str(&format!("UID:{}\r\n", ics_text(&e.uid)));
        out.push_str(&format!("SUMMARY:{}\r\n", ics_text(if e.summary.is_empty() { "(no title)" } else { &e.summary })));
        if !e.location.is_empty() {
            out.push_str(&format!("LOCATION:{}\r\n", ics_text(&e.location)));
        }
        for (k, v) in [("DTSTART", &e.start), ("DTEND", &e.end)] {
            match v {
                Ok(t) => out.push_str(&format!("{k}:{}\r\n", ics_utc(*t))),
                Err(day) => out.push_str(&format!("{k};VALUE=DATE:{day}\r\n")),
            }
        }
        out.push_str("END:VEVENT\r\n");
    }
    out.push_str("END:VCALENDAR\r\n");
    out
}

fn day_of(s: &str) -> Option<String> {
    let d = s.get(..10)?;
    (d.len() == 10 && d.as_bytes()[4] == b'-').then(|| d.replace('-', ""))
}

/// Google Calendar's `events.list` answer as iCalendar. Cancelled events
/// are left out.
pub fn ics_from_google(body: &str) -> Result<String, String> {
    let v = json(body, "Google Calendar")?;
    let items = v.get("items").and_then(|i| i.as_array()).cloned().unwrap_or_default();
    let when = |x: Option<&serde_json::Value>| -> Option<Result<i64, String>> {
        let x = x?;
        if let Some(t) = x.get("dateTime").and_then(|t| t.as_str()) {
            return epoch_of(t).map(Ok);
        }
        x.get("date").and_then(|d| d.as_str()).and_then(day_of).map(Err)
    };
    let mut evs = Vec::new();
    for it in &items {
        if it.get("status").and_then(|s| s.as_str()) == Some("cancelled") {
            continue;
        }
        let (Some(start), Some(end)) = (when(it.get("start")), when(it.get("end"))) else { return Err("Google calendar event has invalid times; cached events kept".into()) };
        evs.push(Ev {
            uid: format!("google-{}", it.get("iCalUID").or(it.get("id")).and_then(|u| u.as_str()).unwrap_or("")),
            summary: it.get("summary").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            location: it.get("location").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            start,
            end,
        });
    }
    Ok(vcalendar(&evs))
}

/// Microsoft Graph's `calendarView` answer (asked for in UTC) as
/// iCalendar. Cancelled events are left out.
pub fn ics_from_graph(body: &str) -> Result<String, String> {
    let v = json(body, "Outlook Calendar")?;
    let items = v.get("value").and_then(|i| i.as_array()).cloned().unwrap_or_default();
    let mut evs = Vec::new();
    for it in &items {
        if it.get("isCancelled").and_then(|c| c.as_bool()) == Some(true) {
            continue;
        }
        let all_day = it.get("isAllDay").and_then(|c| c.as_bool()) == Some(true);
        let at = |k: &str| it.get(k).and_then(|x| x.get("dateTime")).and_then(|t| t.as_str()).map(str::to_string);
        let (Some(s), Some(e)) = (at("start"), at("end")) else { return Err("Outlook calendar event has missing times; cached events kept".into()) };
        let (start, end) = if all_day {
            match (day_of(&s), day_of(&e)) {
                (Some(a), Some(b)) => (Err(a), Err(b)),
                _ => return Err("Outlook calendar event has invalid dates; cached events kept".into()),
            }
        } else {
            match (epoch_of(&s), epoch_of(&e)) {
                (Some(a), Some(b)) => (Ok(a), Ok(b)),
                _ => return Err("Outlook calendar event has invalid times; cached events kept".into()),
            }
        };
        evs.push(Ev {
            uid: format!("outlook-{}", it.get("iCalUId").or(it.get("id")).and_then(|u| u.as_str()).unwrap_or("")),
            summary: it.get("subject").and_then(|s| s.as_str()).unwrap_or("").to_string(),
            location: it.get("location").and_then(|l| l.get("displayName")).and_then(|s| s.as_str()).unwrap_or("").to_string(),
            start,
            end,
        });
    }
    Ok(vcalendar(&evs))
}

/// Read the calendar from now - a week to now + ten weeks, as iCalendar.

/// Compatibility wrapper for callers using the default calendar selection.
pub fn calendar_ics(net: &dyn Net, p: Provider, refresh_token: &str, now: u64) -> Result<String, String> {
    calendar_ics_selected(net, p, refresh_token, now, &[])
}
/// Read explicitly selected calendars; empty selection retains the default
/// calendar. Every page must succeed before a snapshot can replace the cache.
pub fn calendar_ics_selected(net: &dyn Net, p: Provider, refresh_token: &str, now: u64, selected: &[String]) -> Result<String, String> {
    let token = calendar_access(net, p, refresh_token)?;
    calendar_snapshot_with_access(net, p, &token, now, selected)
}

/// Snapshot seam: supplied access token, no account setup or environment lookup.
pub fn calendar_snapshot_with_access(net: &dyn Net, p: Provider, token: &str, now: u64, selected: &[String]) -> Result<String, String> {
    let auth = format!("Bearer {token}");
    let (from, to) = (utc_stamp(now.saturating_sub(BACK_SECS)), utc_stamp(now + AHEAD_SECS));
    let defaults = vec![if p == Provider::Google { "primary".to_string() } else { String::new() }];
    let calendars = if selected.is_empty() { &defaults } else { selected };
    let mut all = Vec::new();
    for calendar in calendars {
        let (host, field, base) = match p {
            Provider::Google => ("www.googleapis.com", "items", format!("/calendar/v3/calendars/{}/events?singleEvents=true&orderBy=startTime&maxResults=500&timeMin={}&timeMax={}", enc(calendar), enc(&from), enc(&to))),
            Provider::Microsoft => ("graph.microsoft.com", "value", format!("/v1.0/me/{}calendarView?startDateTime={}&endDateTime={}&$top=500&$select=id,iCalUId,subject,start,end,location,isAllDay,isCancelled", if calendar.is_empty() { String::new() } else { format!("calendars/{}/", enc(calendar)) }, enc(&from), enc(&to))),
        };
        let mut path = base.clone();
        let mut seen = std::collections::BTreeSet::new();
        loop {
            if seen.len() >= 1000 || !seen.insert(path.clone()) { return Err("calendar pagination did not finish; cached events kept".into()); }
            let reply = net.get(host, &path, &[("Authorization", &auth), ("Prefer", "outlook.timezone=\"UTC\"")])?;
            if !(200..300).contains(&reply.status) { return Err(format!("calendar answered {}; cached events kept", reply.status)); }
            let page = json(&reply.body, "Calendar")?;
            let items = page.get(field).and_then(|v| v.as_array()).ok_or("calendar reply has no event list; cached events kept")?;
            for item in items {
                let mut item = item.clone();
                // Expanded recurring occurrences must not overwrite one another.
                let identity = item.get("id").or(item.get("iCalUID")).or(item.get("iCalUId")).and_then(|v| v.as_str()).unwrap_or("");
                if identity.is_empty() { return Err("calendar event has no identity; cached events kept".into()); }
                let identity = format!("{}:{identity}", crate::digest::sha256_hex(calendar.as_bytes()));
                item[if p == Provider::Google { "iCalUID" } else { "iCalUId" }] = serde_json::Value::String(identity);
                all.push(item);
            }
            let next = match p {
                Provider::Google => page.get("nextPageToken").and_then(|v| v.as_str()).map(|t| format!("{base}&pageToken={}", enc(t))),
                Provider::Microsoft => match page.get("@odata.nextLink").and_then(|v| v.as_str()) {
                    Some(link) => Some(link.strip_prefix("https://graph.microsoft.com").filter(|s| s.starts_with("/v1.0/")).ok_or("calendar continuation points outside Microsoft; cached events kept")?.to_string()),
                    None => None,
                },
            };
            match next { Some(next) => path = next, None => break }
        }
    }
    let combined = serde_json::json!({ (if p == Provider::Google { "items" } else { "value" }): all }).to_string();
    if p == Provider::Google { ics_from_google(&combined) } else { ics_from_graph(&combined) }
}

pub fn snapshot_window(now: u64) -> (u64, u64) { (now.saturating_sub(BACK_SECS), now.saturating_add(AHEAD_SECS)) }

/// Calendar names for the user's picker. Continuation URLs never carry the
/// access token outside the provider's host.
pub fn available_calendars(net: &dyn Net, provider: Provider, refresh_token: &str) -> Result<Vec<(String, String)>, String> {
    let auth = format!("Bearer {}", calendar_access(net, provider, refresh_token)?);
    let (host, field, base) = if provider == Provider::Google { ("www.googleapis.com", "items", "/calendar/v3/users/me/calendarList?maxResults=250") } else { ("graph.microsoft.com", "value", "/v1.0/me/calendars?$top=100&$select=id,name") };
    let mut path = base.to_string(); let mut seen = std::collections::BTreeSet::new(); let mut out = Vec::new();
    loop {
        if seen.len() >= 1000 || !seen.insert(path.clone()) { return Err("calendar list did not finish".into()); }
        let reply = net.get(host, &path, &[("Authorization", &auth)])?;
        if !(200..300).contains(&reply.status) { return Err(format!("calendar list answered {}", reply.status)); }
        let page = json(&reply.body, "Calendar list")?;
        for entry in page.get(field).and_then(|v| v.as_array()).ok_or("calendar list missing")? {
            let id = entry.get("id").and_then(|v| v.as_str()).ok_or("calendar identity missing")?;
            let name = entry.get(if provider == Provider::Google { "summary" } else { "name" }).and_then(|v| v.as_str()).unwrap_or("Untitled calendar");
            out.push((id.into(), name.into()));
        }
        let next = if provider == Provider::Google { page.get("nextPageToken").and_then(|v| v.as_str()).map(|token| format!("{base}&pageToken={}", enc(token))) } else {
            match page.get("@odata.nextLink").and_then(|v| v.as_str()) { Some(link) => Some(link.strip_prefix("https://graph.microsoft.com").filter(|s| s.starts_with("/v1.0/")).ok_or("calendar list continuation left Microsoft")?.into()), None => None }
        };
        match next { Some(next) => path = next, None => break }
    }
    Ok(out)
}

/// How an OAuth calendar is named in the calendar list's `url` field, so
/// it rides the same list, health light and Disconnect as a link.
pub fn calendar_key(p: Provider, email: &str) -> String {
    format!("oauth:{}:{}", p.key(), email)
}

/// The other way: provider and email from a calendar key.
pub fn parse_calendar_key(key: &str) -> Option<(Provider, String)> {
    let rest = key.strip_prefix("oauth:")?;
    let (p, email) = rest.split_once(':')?;
    Some((Provider::from_key(p)?, email.to_string()))
}

/// The vault entry holding a sign-in's refresh token.
pub fn vault_name(p: Provider, email: &str) -> String {
    format!("signin {} {}", p.key(), email)
}

/// The Outlook account a Microsoft sign-in adds to mail. Microsoft 365
/// addresses on custom domains aren't recognised from the address, so the
/// host is said outright.
pub fn outlook_account(email: &str) -> crate::mail::Account {
    crate::mail::Account {
        name: email.split('@').next().unwrap_or(email).to_string(),
        address: email.to_string(),
        imap_host: "outlook.office365.com".into(),
        password_from_vault: vault_name(Provider::Microsoft, email),
        oauth: true,
        client_id: MICROSOFT_CLIENT_ID.into(),
        ..Default::default()
    }
}

\r\n