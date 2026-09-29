//! Reading your Telegram, which is the one chat service that lets you.
//!
//! # Why this one
//!
//! `messaging.rs` has had a truthful table of six platforms since the day it
//! was written, and `Platform::what_it_permits` says of each what you can
//! actually do. Two of the six can never work for a personal account —
//! WhatsApp's interface is for businesses and costs per message, Signal is
//! deliberately closed — and the tools claiming otherwise drive the desktop
//! app while pretending to be you, which gets accounts banned. Atlas does not
//! do that.
//!
//! Telegram and GroupMe have real interfaces. This is Telegram's, because it
//! is the one you are most likely to already use and the setup is two minutes
//! with BotFather.
//!
//! # This is an online, secondary capability, and it says so
//!
//! Everything primary in Atlas works with the network unplugged. This cannot:
//! the messages are on somebody else's server. So it is off by default, it is
//! marked `offline: false` in the catalogue, and nothing else depends on it —
//! a machine with no network loses this and keeps everything.
//!
//! # What a bot can and cannot see, said plainly
//!
//! A Telegram bot is not you. It sees:
//!
//! * messages sent directly to it;
//! * messages in a group it has been added to — and by default **only those
//!   that name it**, unless you turn privacy mode off in BotFather.
//!
//! It does **not** see your existing one-to-one conversations with other
//! people. Nothing can, short of logging in as you, which is the thing this
//! module exists not to do. So this is useful for a channel you point at
//! Atlas, and it is honest about not being your whole inbox.
//!
//! # The token
//!
//! It goes in the vault, never in `tools.yaml`. A token in a config file is a
//! token in your backups, your sync folder and any screenshot of your
//! settings — and this one can read and send as the bot.

use serde::Deserialize;
use std::time::Duration;

/// Where the token lives in the vault.
pub const TOKEN: &str = "telegram_bot_token";

/// Where the messages read so far are kept.
pub const KEPT: &str = "telegram_messages";

/// Where "how far I got" is kept.
///
/// Its own record rather than derived from `KEPT` on each read: the kept
/// messages are trimmed and the offset must not move backwards when they are,
/// or Telegram hands back everything again.
pub const READ_UP_TO: &str = "telegram_read_up_to";

/// The host, named once.
pub const HOST: &str = "api.telegram.org";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct TelegramConfig {
    /// Off until you have made a bot and put its token in the vault.
    pub enabled: bool,
    /// How long to wait on the server.
    pub timeout_secs: u64,
    /// The most messages to take in one go.
    ///
    /// Bounded because a bot left alone for a week comes back to a queue, and
    /// a reply that reads out four hundred messages is worse than one that
    /// reads out twenty and says there are more.
    pub most_at_once: u32,
}

impl Default for TelegramConfig {
    fn default() -> Self {
        TelegramConfig { enabled: false, timeout_secs: 10, most_at_once: 20 }
    }
}

/// What to tell somebody who has not set this up.
pub const HOW_TO_SET_UP: &str = "\
Open Telegram and message @BotFather. Send /newbot, pick a name, and it gives you a token that \
looks like 123456789:AA.... Then run `atlas telegram token` and paste it — it goes in the vault, \
not into a settings file. Message your new bot once so it has something to read.

What a bot can see is worth knowing before you start: messages sent straight to it, and messages \
in a group it has been added to that name it. It cannot see your existing conversations with \
other people, and nothing can without logging in as you, which I won't do.";

/// One message, as Atlas holds it.
///
/// Deliberately the same shape `messaging::Message` already uses, so the
/// sorting that was written and never reachable — `sort`, `folder_for`,
/// `note_on`, `spoken` — works on these without a translation layer.
pub fn into_messages(raw: &str) -> Result<Vec<crate::messaging::Message>, String> {
    let v: serde_json::Value =
        serde_json::from_str(raw).map_err(|e| format!("that wasn't JSON: {e}"))?;

    // Telegram reports its own failures in a 200 with `ok: false`, so the
    // HTTP status is not the answer. A wrong token arrives here, not as a
    // transport error.
    if v.get("ok").and_then(|o| o.as_bool()) != Some(true) {
        let why = v
            .get("description")
            .and_then(|d| d.as_str())
            .unwrap_or("no reason given");
        return Err(format!("Telegram refused: {why}"));
    }

    let items = match v.get("result").and_then(|r| r.as_array()) {
        Some(a) => a,
        // `ok: true` with no result array is a shape change, not an empty
        // inbox, and reporting it as "no messages" would be the same defect
        // as `spoken(&[])` reporting a count of something nothing had read.
        None => return Err("Telegram said ok and sent no result list".into()),
    };

    let mut out = Vec::new();
    for item in items {
        let Some(m) = item.get("message").or_else(|| item.get("channel_post")) else {
            // An update that is not a message — someone edited one, a button
            // was pressed. Skipped rather than failed: an unknown update type
            // must not stop the ones after it being read.
            continue;
        };
        let text = m
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string();
        if text.trim().is_empty() {
            continue;
        }
        let from_obj = m.get("from");
        let from = from_obj
            .and_then(|f| f.get("first_name"))
            .and_then(|n| n.as_str())
            .or_else(|| from_obj.and_then(|f| f.get("username")).and_then(|n| n.as_str()))
            .unwrap_or("someone")
            .to_string();
        let chat = m.get("chat");
        let group = chat
            .and_then(|c| c.get("title"))
            .and_then(|t| t.as_str())
            .map(|s| s.to_string());
        // `mentions_you` is whether Atlas was named, which for a bot in a
        // group is the only kind of message it is shown by default. Left for
        // `messaging::sort` to decide from the text rather than guessed here:
        // one rule, in the module that owns it.
        out.push(crate::messaging::Message {
            id: item
                .get("update_id")
                .and_then(|u| u.as_u64())
                .map(|u| u.to_string())
                .unwrap_or_default(),
            platform: crate::messaging::Platform::Telegram,
            from,
            group,
            text,
            at: m.get("date").and_then(|d| d.as_u64()).unwrap_or(0),
            mentions_you: false,
        });
    }
    Ok(out)
}

/// The highest update id in a batch, plus one.
///
/// Telegram hands out the same messages again until you say how far you got,
/// so without this every read returns everything since the bot was made.
/// `+1` because the offset means "start here", not "I had this one".
pub fn read_up_to(messages: &[crate::messaging::Message]) -> Option<u64> {
    messages.iter().filter_map(|m| m.id.parse::<u64>().ok()).max().map(|n| n + 1)
}

/// The path to ask for, given where you got to last time.
///
/// Named `updates_path` rather than `path_for`: `install` has a `path_for`
/// too, and the deadness scans read bare names -- fifth collision in two
/// days, so this one was renamed on the guard's first complaint rather than
/// argued with.
pub fn updates_path(token: &str, since: Option<u64>, most: u32) -> String {
    let mut p = format!("/bot{token}/getUpdates?limit={most}");
    if let Some(n) = since {
        p.push_str(&format!("&offset={n}"));
    }
    p
}

/// Ask Telegram what has arrived.
///
/// Takes the token rather than reaching into the vault, so the one thing that
/// must not be logged is passed explicitly and this function can be tested
/// without one.
pub fn fetch(
    token: &str,
    since: Option<u64>,
    cfg: &TelegramConfig,
) -> Result<Vec<crate::messaging::Message>, String> {
    if token.trim().is_empty() {
        return Err("no token — run `atlas telegram token`".into());
    }
    let path = updates_path(token.trim(), since, cfg.most_at_once);
    let r = crate::http::https_get(HOST, &path, Duration::from_secs(cfg.timeout_secs))
        .map_err(|e| {
            // The token is in the path, so an error carrying the request
            // would put it in the log. Said without it, deliberately.
            let said = e.to_string();
            if said.contains(token.trim()) {
                "couldn't reach Telegram".to_string()
            } else {
                format!("couldn't reach Telegram: {said}")
            }
        })?;
    into_messages(&r.body)
}

/// A token that is the right shape.
///
/// Not a check that it works — only Telegram can say that. This catches the
/// paste that took the wrong half of the line, which is the mistake people
/// actually make, before it becomes "couldn't reach Telegram".
pub fn looks_like_a_token(s: &str) -> bool {
    let s = s.trim();
    let Some((digits, rest)) = s.split_once(':') else { return false };
    !digits.is_empty()
        && digits.chars().all(|c| c.is_ascii_digit())
        && rest.len() >= 30
        && rest.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}
