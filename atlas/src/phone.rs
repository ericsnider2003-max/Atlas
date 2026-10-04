//! Reaching you when the laptop isn't with you.
//!
//! The last gap in the delivery chain. Atlas speaks when you are there, draws
//! its own window when you are not, and holds anything it cannot get to you —
//! but if you have walked out of the house, holding means you hear nothing
//! until you come back. For "your disk is full" that is fine. For "the trade
//! you were watching just moved" it is not.
//!
//! ## How it reaches the phone
//!
//! A single HTTP POST to a push endpoint you control, which the phone is
//! already subscribed to. No app to build, no account, nothing running on the
//! phone that Atlas has to maintain.
//!
//! **ntfy's publish format.** The message goes to the server's root as JSON
//! with the topic inside it (`{"topic": "atlas", "title": …}` — docs.ntfy.sh,
//! "Publish as JSON"). The first version posted that JSON to `/atlas`, which
//! ntfy treats as a plain-text message: the phone would have shown the raw
//! JSON. A token, when set, goes in `Authorization: Bearer`, where ntfy reads
//! it; before round 5 the field was read by nothing.
//!
//! **Plain HTTP or TLS.** `http://box:80` or `box:80` — a server on your own
//! network over Tailscale, which encrypts the hop itself — goes as plain
//! HTTP. `https://…` goes over TLS with the system's certificate checks,
//! which is how a public push service works. Either way this is the online,
//! secondary path; nothing depends on it.
//!
//! ## What goes in the message
//!
//! Titles, never contents. A phone notification lands on a lock screen that
//! anybody can see, and Atlas has no way to know who is looking — the same
//! problem the desk window has, with less control over it. So the phone gets
//! the knock and the detail waits until you open Atlas.

use crate::error::Result;
use serde::Deserialize;
use std::time::Duration;

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct PhoneConfig {
    pub enabled: bool,
    /// `host:port` of the push server. A Tailscale name is the intended use:
    /// `pushbox:80`, not a public address.
    pub host: String,
    /// The path the phone is subscribed to. For an ntfy-style server this is
    /// the topic, e.g. `/atlas`.
    pub path: String,
    /// Sent as a bearer token when the server wants one.
    pub token: Option<String>,
    /// How long to wait before giving up. Short on purpose: this runs inside
    /// the daemon tick, and a push server that has gone away must not stall
    /// everything else Atlas was doing.
    pub timeout_secs: u64,
    /// Send the body as well as the title.
    ///
    /// Off by default, and that default is the point. A phone notification
    /// lands on a lock screen anyone can read.
    pub include_detail: bool,
    /// Put what Atlas is doing and what's next on a lock-screen widget, not
    /// just the time and the count (`glance`). Off by default for the same
    /// reason as `include_detail`: a lock screen is read by whoever holds the
    /// phone.
    pub widget_titles_on_lock_screen: bool,
    /// Apple's push service, for an iPhone with Atlas closed (item 15,
    /// `apns`). Works beside the push server above, or instead of it.
    pub apns: crate::apns::ApnsConfig,
}

impl Default for PhoneConfig {
    fn default() -> Self {
        PhoneConfig {
            enabled: false,
            host: String::new(),
            path: "/atlas".into(),
            token: None,
            timeout_secs: 5,
            include_detail: false,
            widget_titles_on_lock_screen: false,
            apns: crate::apns::ApnsConfig::default(),
        }
    }
}

/// Why the phone cannot be reached, in words that say what to do about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotSet {
    /// Switched off in settings.
    Disabled,
    /// No address at all.
    NoHost,
    /// An `https://` address, which this client cannot speak.
    NeedsTls(String),
}

impl NotSet {
    pub fn plain(&self) -> String {
        match self {
            NotSet::Disabled => "phone alerts are switched off in your settings".into(),
            NotSet::NoHost => {
                "no push address is set, so there's nowhere to send a phone alert".into()
            }
            NotSet::NeedsTls(h) => format!(
                "{h} has no server name after https:// — give the push server's address, e.g. \
                 https://ntfy.example.com, or box:80 for one on your own network"
            ),
        }
    }
}

/// Is the phone actually reachable, as configured?
///
/// Checked up front rather than at the moment something urgent needs sending.
/// The `https` case in particular would otherwise fail as a confusing
/// connection error at the worst possible time.
pub fn configured(cfg: &PhoneConfig) -> std::result::Result<(), NotSet> {
    if !cfg.enabled {
        return Err(NotSet::Disabled);
    }
    let h = cfg.host.trim();
    if h.is_empty() {
        // No push server, but an iPhone Apple can reach or an Android phone
        // with a UnifiedPush address: that's a way.
        if apple_can_reach(cfg) || android_can_reach() {
            return Ok(());
        }
        return Err(NotSet::NoHost);
    }
    // An https address is fine now (TLS, round 5). One with nothing after the
    // scheme is still refused up front rather than at the moment it's needed.
    if let Some(rest) = h.strip_prefix("https://") {
        if rest.trim_matches('/').is_empty() {
            return Err(NotSet::NeedsTls(h.to_string()));
        }
    }
    Ok(())
}

/// Strip a scheme, so `http://box:80` and `box:80` both work.
fn host_only(host: &str) -> &str {
    host.trim().trim_start_matches("https://").trim_start_matches("http://").trim_end_matches('/')
}

/// The topic, from `path`: `/atlas` → `atlas`.
fn topic(cfg: &PhoneConfig) -> String {
    cfg.path.trim().trim_matches('/').to_string()
}

/// What actually gets sent.
///
/// Built here rather than inline so it can be tested without a server, which
/// matters: the redaction rule is the part most likely to be got wrong, and
/// the part with the worst consequence.
pub fn body_for(note: &crate::notify::Note, cfg: &PhoneConfig) -> String {
    // Even with detail on, what leaves the machine is scrubbed the same way a
    // prompt to an online model is (`redact`): keys, card and account
    // numbers, addresses become placeholders on the lock screen.
    let detail = if note.private || !cfg.include_detail {
        "Ask me when you're ready.".to_string()
    } else {
        crate::redact::Scrubber::default().scrub(&note.body)
    };
    // Hand-built rather than via serde: a few fields and the escaping is the
    // only part that matters.
    format!(
        "{{\"topic\":\"{}\",\"title\":\"{}\",\"message\":\"{}\",\"priority\":{},\"tags\":[\"atlas\"]}}",
        escape(&topic(cfg)),
        escape(&note.title),
        escape(&detail),
        if note.urgency == crate::notify::Urgency::Urgent { 5 } else { 3 }
    )
}

fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => {}
            // Control characters would produce invalid JSON and a rejected
            // push, which would read as "the phone is unreachable".
            c if (c as u32) < 0x20 => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Send one notification to the phone.
///
/// Returns the server's own refusal rather than a generic failure. A 404 from
/// a push server means the topic is wrong; a connection refused means it is
/// not running. Those send you to different places.
pub fn send(note: &crate::notify::Note, cfg: &PhoneConfig) -> Result<()> {
    if let Err(e) = configured(cfg) {
        return Err(crate::error::AtlasError::Platform(e.plain()));
    }
    // One way to reach your phone, two senders (decision 3): the push server
    // (Android, and anything subscribed to it) and Apple's push service for
    // an iPhone. Delivered if either got it.
    // An Android phone with Atlas closed goes through its UnifiedPush
    // address (`webpush`), sealed for that phone alone.
    let by_apple = if apple_can_reach(cfg) { Some(crate::apns::send(note, cfg)) } else { None };
    let by_android = if android_can_reach() { Some(crate::webpush::send(note, cfg)) } else { None };
    let direct = [by_apple, by_android];
    if direct.iter().any(|r| matches!(r, Some(Ok(())))) && cfg.host.trim().is_empty() {
        return Ok(());
    }
    if cfg.host.trim().is_empty() {
        return Err(crate::error::AtlasError::Platform(
            direct.into_iter().flatten().find_map(|r| r.err()).unwrap_or_else(|| NotSet::NoHost.plain()),
        ));
    }
    match send_to_server(note, cfg) {
        Ok(()) => Ok(()),
        Err(_) if direct.iter().any(|r| matches!(r, Some(Ok(())))) => Ok(()),
        Err(e) => Err(e),
    }
}

/// Has an Android phone given Atlas its UnifiedPush address?
fn android_can_reach() -> bool {
    !crate::webpush::Devices::load(&crate::roots::state_dir()).devices.is_empty()
}

/// Is an iPhone reachable through Apple's push service: the key set up, and
/// at least one phone has given its address?
fn apple_can_reach(cfg: &PhoneConfig) -> bool {
    cfg.apns.ready(&crate::roots::install_root()).is_ok()
        && !crate::apns::Devices::load(&crate::roots::state_dir()).devices.is_empty()
}

/// The push server (ntfy's JSON publish).
fn send_to_server(note: &crate::notify::Note, cfg: &PhoneConfig) -> Result<()> {
    let body = body_for(note, cfg);
    let timeout = Duration::from_secs(cfg.timeout_secs.max(1));
    let token = cfg.token.as_deref().filter(|t| !t.trim().is_empty());
    // To the root: ntfy's JSON publish carries the topic in the body.
    let r = if cfg.host.trim().starts_with("https://") {
        crate::http::https_post_json(host_only(&cfg.host), "/", &body, token, timeout)?
    } else {
        match token {
            Some(t) => crate::http::post_json_with_token(host_only(&cfg.host), "/", &body, t, timeout)?,
            None => crate::http::post_json(host_only(&cfg.host), "/", &body, timeout)?,
        }
    };
    if !r.ok() {
        return Err(crate::error::AtlasError::Platform(format!(
            "the push server answered {} — {}",
            r.status,
            if r.status == 404 {
                "that topic doesn't exist on it"
            } else if r.status == 401 || r.status == 403 {
                "it wants a token, or the one set is wrong"
            } else {
                "it refused the message"
            }
        )));
    }
    Ok(())
}

/// `send`, with a link the notification opens when tapped (ntfy's
/// `click`): the texts Atlas writes open Messages with them filled in. Only
/// with detail on -- the link carries the words.
pub(crate) fn send_with_click(note: &crate::notify::Note, cfg: &PhoneConfig, click: &str) -> Result<()> {
    if note.private || !cfg.include_detail {
        return send(note, cfg);
    }
    if let Err(e) = configured(cfg) {
        return Err(crate::error::AtlasError::Platform(e.plain()));
    }
    let body = body_for(note, cfg);
    let body = format!("{},\"click\":\"{}\"}}", body.trim_end_matches('}'), escape(click));
    let timeout = Duration::from_secs(cfg.timeout_secs.max(1));
    let token = cfg.token.as_deref().filter(|t| !t.trim().is_empty());
    let r = if cfg.host.trim().starts_with("https://") {
        crate::http::https_post_json(host_only(&cfg.host), "/", &body, token, timeout)?
    } else {
        match token {
            Some(t) => crate::http::post_json_with_token(host_only(&cfg.host), "/", &body, t, timeout)?,
            None => crate::http::post_json(host_only(&cfg.host), "/", &body, timeout)?,
        }
    };
    if !r.ok() {
        return Err(crate::error::AtlasError::Platform(format!("the push server answered {}", r.status)));
    }
    Ok(())
}

/// Said plainly when there is no way to reach the phone.
pub const NO_PHONE: &str =
    "I can't reach your phone — no push address is set up. Anything urgent while you're out is \
     held until you're back at the machine.";
