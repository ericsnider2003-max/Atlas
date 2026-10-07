//! Connecting your accounts, worked out from what you type (2 Oct 2026,
//! Eric: "I want an easier way to connect things like social media
//! accounts, emails, calendars ... the connection process doesn't seem like
//! it was actually thought out").
//!
//! Before this, a Gmail or Yahoo mailbox couldn't be connected from Atlas at
//! all -- it took a hand-edited `mail.accounts` entry and a vault entry no
//! command could make -- and no calendar connected on the laptop. Now:
//!
//! - **Type your email address.** Atlas works out the provider and says the
//!   one thing it needs: for most, an app password (with the provider's own
//!   page for making one, linked); for Outlook and Hotmail, Microsoft's
//!   sign-in, because Microsoft switched passwords off in 2024.
//! - **A provider Atlas doesn't know** is looked up the way Thunderbird does
//!   it (Mozilla's ISPDB, `autoconfig.thunderbird.net`), so you still only
//!   type the address.
//! - **A calendar by its private link** -- Google's "secret address in iCal
//!   format", Outlook's published ICS, iCloud's public link -- read again
//!   every 15 minutes.
//!
//! The password is tried against the real server before anything is kept:
//! "connected" is said only of an account that let Atlas in.

/// What connecting this address takes.
#[derive(Debug, Clone, PartialEq)]
pub enum Way {
    /// A password made for Atlas on the provider's own page.
    AppPassword(Mailbox),
    /// Microsoft's sign-in: Outlook.com, Hotmail, Live, MSN.
    MicrosoftSignIn,
    /// A provider Atlas has no entry for: looked up next (`ispdb_url`).
    LookItUp { domain: String },
    /// A provider that can't be read over IMAP at all, and why.
    NotPossible { provider: &'static str, why: &'static str },
}

/// Where a mailbox is read, and how you make the password for it.
#[derive(Debug, Clone, PartialEq)]
pub struct Mailbox {
    pub provider: String,
    pub imap_host: String,
    pub imap_port: u16,
    /// The steps, said plainly.
    pub steps: String,
    /// The provider's page for making an app password, when there is one.
    pub link: Option<String>,
}

fn mailbox(provider: &str, imap: &str, steps: &str, link: Option<&str>) -> Way {
    Way::AppPassword(Mailbox {
        provider: provider.into(),
        imap_host: imap.into(),
        imap_port: 993,
        steps: steps.into(),
        link: link.map(str::to_string),
    })
}

/// The address, tidied, if it is one: `None` for anything that isn't
/// `someone@somewhere.tld`.
pub fn address(typed: &str) -> Option<String> {
    let a = typed.trim().trim_matches(['<', '>']).to_ascii_lowercase();
    let (user, domain) = a.split_once('@')?;
    let ok = !user.is_empty()
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !a.contains(char::is_whitespace)
        && a.matches('@').count() == 1;
    ok.then_some(a)
}

/// What connecting `address` takes. `None` when it isn't an address.
pub fn way_for(typed: &str) -> Option<Way> {
    let a = address(typed)?;
    let domain = a.rsplit('@').next().unwrap_or("").to_string();
    Some(match domain.as_str() {
        "gmail.com" | "googlemail.com" => mailbox(
            "Gmail",
            "imap.gmail.com",
            "Google needs 2-Step Verification on your account first. Then open App passwords, name it \"Atlas\", \
             and copy the 16-letter password it shows you into the box below.",
            Some("https://myaccount.google.com/apppasswords"),
        ),
        "outlook.com" | "hotmail.com" | "live.com" | "msn.com" | "outlook.co.uk" | "hotmail.co.uk" => Way::MicrosoftSignIn,
        "yahoo.com" | "ymail.com" | "rocketmail.com" | "yahoo.co.uk" | "yahoo.ca" => mailbox(
            "Yahoo Mail",
            "imap.mail.yahoo.com",
            "Open Yahoo's app passwords page, make one called \"Atlas\", and copy it into the box below. \
             Your normal Yahoo password won't work here.",
            Some("https://login.yahoo.com/myaccount/security/app-password"),
        ),
        "aol.com" => mailbox(
            "AOL Mail",
            "imap.aol.com",
            "Open AOL's app passwords page, make one called \"Atlas\", and copy it into the box below.",
            Some("https://login.aol.com/myaccount/security/app-password"),
        ),
        "icloud.com" | "me.com" | "mac.com" => mailbox(
            "iCloud Mail",
            "imap.mail.me.com",
            "Sign in at account.apple.com, open Sign-In and Security, then App-Specific Passwords. Make one \
             called \"Atlas\" and copy it into the box below.",
            Some("https://account.apple.com/account/manage"),
        ),
        "fastmail.com" | "fastmail.fm" => mailbox(
            "Fastmail",
            "imap.fastmail.com",
            "In Fastmail, open Settings, then Privacy & Security, then Manage app passwords. Make one with mail \
             access and copy it into the box below.",
            Some("https://app.fastmail.com/settings/security/apps"),
        ),
        "zoho.com" | "zohomail.com" => mailbox(
            "Zoho Mail",
            "imap.zoho.com",
            "Turn on IMAP in Zoho Mail's settings, then make an app-specific password under Security and copy \
             it into the box below.",
            Some("https://accounts.zoho.com/home#security/app_password"),
        ),
        "gmx.com" | "gmx.net" | "gmx.de" => mailbox(
            "GMX",
            "imap.gmx.com",
            "Turn on POP3/IMAP in GMX's settings (under Email, then POP3/IMAP), then put your GMX password in \
             the box below.",
            None,
        ),
        "proton.me" | "protonmail.com" | "pm.me" => Way::NotPossible {
            provider: "Proton Mail",
            why: "Proton encrypts your mail so only its own apps (or Proton Mail Bridge, on a paid plan) can read \
                  it -- Atlas can't reach it directly.",
        },
        _ => Way::LookItUp { domain },
    })
}

/// Mozilla's database of mail providers, the way Thunderbird finds one.
pub fn ispdb_url(domain: &str) -> String {
    format!("https://autoconfig.thunderbird.net/v1.1/{domain}")
}

/// The IMAP server in an autoconfig answer (Mozilla's ISPDB format): the
/// first `<incomingServer type="imap">` over SSL, with its port. A server
/// only offered without encryption is passed over.
pub fn imap_from_autoconfig(xml: &str) -> Option<(String, u16)> {
    let mut rest = xml;
    while let Some(at) = rest.find("<incomingServer") {
        let block_end = rest[at..].find("</incomingServer>").map(|e| at + e).unwrap_or(rest.len());
        let block = &rest[at..block_end];
        rest = &rest[block_end.min(rest.len())..];
        if rest.starts_with("</incomingServer>") {
            rest = &rest["</incomingServer>".len()..];
        }
        let head = block.split('>').next().unwrap_or("");
        if !head.contains("type=\"imap\"") && !head.contains("type='imap'") {
            continue;
        }
        let tag = |name: &str| -> Option<String> {
            let open = format!("<{name}>");
            let i = block.find(&open)? + open.len();
            let j = block[i..].find("</")? + i;
            Some(block[i..j].trim().to_string())
        };
        let host = tag("hostname")?;
        let port: u16 = tag("port").and_then(|p| p.parse().ok()).unwrap_or(993);
        let secure = tag("socketType").map(|s| s.eq_ignore_ascii_case("SSL")).unwrap_or(false);
        if secure && !host.is_empty() && !host.contains('%') {
            return Some((host, port));
        }
    }
    None
}

/// The way for a provider found in the ISPDB.
pub fn found_mailbox(domain: &str, host: &str, port: u16) -> Way {
    Way::AppPassword(Mailbox {
        provider: domain.to_string(),
        imap_host: host.to_string(),
        imap_port: port,
        steps: format!(
            "Your provider's mail server is {host}. If it offers app passwords (most do, under security \
             settings), make one called \"Atlas\"; otherwise use your mail password. Put it in the box below."
        ),
        link: None,
    })
}

/// The vault entry an account's password is kept under.
pub fn vault_name(address: &str) -> String {
    format!("mail {address}")
}

// ------------------------------------------------------------ calendars

/// A calendar read from its link, again and again.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CalendarLink {
    /// What you call it.
    pub name: String,
    /// The https address it's read from.
    pub url: String,
    /// When it was last read, and what came of it.
    pub last_read: u64,
    pub last_ok: bool,
    pub last_said: String,
    /// Its sign-in was refused as revoked or expired (`revoked`): not read
    /// again until you sign in again, which replaces the link (N5).
    pub needs_signin: bool,
}

/// The calendar links kept, by the store.
pub const CALENDAR_LINKS: &str = "calendar_links";

/// How often a linked calendar is read again.
pub const READ_EVERY_SECS: u64 = 15 * 60;

/// The link, made readable: `webcal://` is https by another name, and only
/// https is accepted (a calendar's private link is a password in all but
/// name; it doesn't go over plain http).
pub fn calendar_link(typed: &str) -> Result<String, String> {
    let t = typed.trim();
    let t = match t.strip_prefix("webcal://") {
        Some(rest) => format!("https://{rest}"),
        None => t.to_string(),
    };
    let rest = t
        .strip_prefix("https://")
        .ok_or_else(|| "That isn't a calendar link -- it should start with https:// or webcal://.".to_string())?;
    let host = rest.split(['/', '?']).next().unwrap_or("");
    if host.is_empty() || !host.contains('.') || rest.contains(char::is_whitespace) {
        return Err("That link doesn't name a website, so there's nothing to read.".into());
    }
    Ok(t)
}

/// What to call a calendar from its link, when you didn't say.
pub fn calendar_name(url: &str) -> String {
    let host = url.trim_start_matches("https://").split(['/', '?']).next().unwrap_or("");
    if host.contains("google.com") {
        "Google Calendar".into()
    } else if host.contains("outlook") || host.contains("office365") || host.contains("live.com") {
        "Outlook calendar".into()
    } else if host.contains("icloud.com") {
        "iCloud calendar".into()
    } else {
        host.trim_start_matches("www.").to_string()
    }
}

/// Is a linked calendar due to be read again?
///
/// Not one whose sign-in was refused: asking again every 15 minutes with a
/// token the provider has already turned down is the "hammering a dead
/// token for hours" the connections design warns about (N5).
pub fn read_due(link: &CalendarLink, now: u64) -> bool {
    !link.needs_signin && now.saturating_sub(link.last_read) >= READ_EVERY_SECS
}

/// Take in how one read of a linked calendar went. Returns what to say aloud
/// when its sign-in has just stopped working (once, on the change, never on
/// every read after).
pub fn took_read(link: &mut CalendarLink, failed: Option<&str>) -> Option<String> {
    match failed {
        None => {
            link.last_ok = true;
            link.needs_signin = false;
            None
        }
        Some(e) => {
            link.last_ok = false;
            link.last_said = e.to_string();
            if !revoked(e) || link.needs_signin {
                return None;
            }
            link.needs_signin = true;
            Some(format!(
                "{} stopped letting me in ({e}). I've stopped asking it; press Sign in again beside it on the Accounts page.",
                link.name
            ))
        }
    }
}

// ------------------------------------------------------------ lifecycle (N5)

/// The words every refused sign-in is said with -- Google's and Microsoft's
/// `invalid_grant`, whichever reader met it -- so one test, `revoked`, tells
/// a dead sign-in from a passing network failure.
pub const REVOKED: &str = "no longer accepts Atlas's sign-in";

/// Was this failure the provider refusing the sign-in itself (revoked, or
/// lapsed), rather than something that a later try could get past?
pub fn revoked(said: &str) -> bool {
    said.contains(REVOKED)
}

/// Is this account's sign-in known to be refused? Then it isn't tried again
/// until you sign in again (which notes it working, `note_health`).
pub fn sign_in_refused(store: &crate::store::Store, who: &str) -> bool {
    health_of(store, who).is_some_and(|h| !h.ok && revoked(&h.said))
}

/// How long before its stated expiry an access token is fetched again:
/// a token used at the edge of its life fails half way through a check.
pub const RENEW_BEFORE_SECS: u64 = 5 * 60;

type Held = std::sync::Arc<std::sync::Mutex<Option<(String, u64)>>>;
static ACCESS: std::sync::Mutex<Vec<(u64, Held)>> = std::sync::Mutex::new(Vec::new());

fn key_of(key: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}

/// An access token for one sign-in, fetched by `fetch` -- which answers the
/// token and how many seconds it lasts -- only when there isn't one with
/// more than `RENEW_BEFORE_SECS` left, and by one thread at a time: a mail
/// check and a calendar read wanting the same sign-in at once make one
/// request, not two (the "thundering herd" the design names). A failure is
/// never kept; the next ask tries again.
///
/// `key` names the sign-in and what the token is for (Microsoft's mail and
/// calendar tokens differ); it is hashed, so no refresh token sits in here.
pub fn access_once(key: &str, now: u64, fetch: impl FnOnce() -> Result<(String, u64), String>) -> Result<String, String> {
    let k = key_of(key);
    let held: Held = {
        let mut all = ACCESS.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        match all.iter().find(|(h, _)| *h == k) {
            Some((_, held)) => held.clone(),
            None => {
                let held = Held::default();
                all.push((k, held.clone()));
                held
            }
        }
    };
    let mut slot = held.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((token, until)) = slot.as_ref() {
        if now + RENEW_BEFORE_SECS < *until {
            return Ok(token.clone());
        }
    }
    *slot = None;
    let (token, lasts) = fetch()?;
    *slot = Some((token.clone(), now + lasts));
    Ok(token)
}

/// Forget a sign-in's access token (it was disconnected, or refused).
pub fn forget_access(key: &str) {
    let k = key_of(key);
    ACCESS.lock().unwrap_or_else(std::sync::PoisonError::into_inner).retain(|(h, _)| *h != k);
}

/// The link's host and path, for an https request.
pub fn host_and_path(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("https://")?;
    match rest.find('/') {
        Some(i) => Some((rest[..i].to_string(), rest[i..].to_string())),
        None => Some((rest.to_string(), "/".to_string())),
    }
}

// ------------------------------------------------------------ health

/// How each connected account last went: working, or the error it gave.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Health {
    pub at: u64,
    pub ok: bool,
    pub said: String,
}

/// The store record of every account's health, by address or link.
pub const HEALTH: &str = "connected_health";

/// Note how an account just went (`None`: it worked).
pub fn note_health(store: &crate::store::Store, who: &str, failed: Option<&str>) {
    let mut all: std::collections::BTreeMap<String, Health> = store.load(HEALTH);
    all.insert(
        who.to_ascii_lowercase(),
        Health { at: crate::store::now(), ok: failed.is_none(), said: failed.unwrap_or("").chars().take(200).collect() },
    );
    let _ = store.save(HEALTH, &all);
}

/// How an account last went, if it has been tried.
pub fn health_of(store: &crate::store::Store, who: &str) -> Option<Health> {
    let all: std::collections::BTreeMap<String, Health> = store.load(HEALTH);
    all.get(&who.to_ascii_lowercase()).cloned()
}
