//! Text messages: "text Sam saying I'm running late".
//!
//! Eric's ruling (30 Sep 2026): messaging goes "where the main conversations
//! take place" -- text messages. What a phone allows sets the shape:
//!
//! - **iPhone.** No app can read your Messages or send a text by itself
//!   (Apple's `MFMessageComposeViewController` always leaves Send to you).
//!   So Atlas writes the text and opens Messages with the person and the
//!   words filled in; you tap Send.
//! - **The laptop.** It can't send texts at all, so the same text goes to
//!   your phone as a notification that opens Messages, and sits on the hub's
//!   Talk page with an "Open in Messages" button.
//!
//! Nothing here claims a text was sent: Atlas only knows it was written.

use serde::{Deserialize, Serialize};

/// A text written and waiting for you to send it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waiting {
    pub to_name: String,
    pub number: String,
    pub body: String,
    pub at: u64,
}

/// The record they're kept in.
pub const FILE: &str = "texts_waiting";

/// How long a written text is offered for.
pub const KEPT_SECS: u64 = 6 * 3600;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Texts {
    pub waiting: Vec<Waiting>,
}

impl Texts {
    pub fn load(store: &crate::store::Store) -> Texts {
        store.load(FILE)
    }
    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }
    /// Add one, dropping any older than `KEPT_SECS` and keeping the last few.
    pub fn add(&mut self, w: Waiting) {
        let now = w.at;
        self.waiting.retain(|x| now.saturating_sub(x.at) < KEPT_SECS);
        self.waiting.push(w);
        let extra = self.waiting.len().saturating_sub(5);
        self.waiting.drain(..extra);
    }
    /// The ones still worth offering at `now`, newest first.
    pub fn current(&self, now: u64) -> Vec<&Waiting> {
        self.waiting.iter().rev().filter(|w| now.saturating_sub(w.at) < KEPT_SECS).collect()
    }
}

/// "text Sam saying ...", "send a text to Sam saying ...", "send Sam a text
/// saying ...", "text Sam: ...". The person and the words, or `None`.
pub fn text_asked(said: &str) -> Option<(String, String)> {
    let s = said.trim().trim_end_matches(['.', '!']);
    let low = s.to_ascii_lowercase();
    let leads = [
        "send a text message to ", "send a text to ", "send text to ", "shoot a text to ", "text message ", "can you text ",
        "please text ", "send ", "shoot ", "text ",
    ];
    let lead = leads.iter().find(|p| low.starts_with(**p))?;
    let rest_low = &low[lead.len()..];
    let (at, mark) = [" saying ", " that ", ": ", ", ", " to say "]
        .iter()
        .filter_map(|m| rest_low.find(m).map(|i| (i, *m)))
        .min_by_key(|(i, _)| *i)?;
    let mut who = s[lead.len()..lead.len() + at].trim();
    // "send Sam a text saying ...", "shoot Sam a text ..."
    if matches!(*lead, "send " | "shoot ") {
        who = who.strip_suffix(" a text").or_else(|| who.strip_suffix(" a text message"))?.trim();
    }
    let message = s[lead.len() + at + mark.len()..].trim();
    if who.is_empty() || who.split_whitespace().count() > 4 || message.split_whitespace().count() < 2 {
        return None;
    }
    if ["him", "her", "them", "it", "me", "everyone"].contains(&who.to_lowercase().as_str()) {
        return None;
    }
    Some((who.to_string(), message.to_string()))
}

/// The link that opens Messages with `number` and `body` filled in. `?&body=`
/// is the form both iOS and Android read.
pub fn sms_link(number: &str, body: &str) -> String {
    format!("sms:{number}?&body={}", percent(body))
}

fn percent(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// The texts waiting, as a card for the top of the Talk page.
pub fn card(waiting: &[&Waiting]) -> String {
    if waiting.is_empty() {
        return String::new();
    }
    let mut out = String::from("<section class=card aria-label='Texts to send'><h2>Texts to send</h2><ul class=plain>");
    for w in waiting {
        out.push_str(&format!(
            "<li><p><strong>{}</strong> ({}): {}</p><p><a class='button primary' href=\"{}\">Open in Messages</a></p></li>",
            crate::hub::esc(&w.to_name),
            crate::hub::esc(&w.number),
            crate::hub::esc(&w.body),
            crate::hub::esc(&sms_link(&w.number, &w.body)),
        ));
    }
    out.push_str("</ul><p class=note>Your phone sends it when you tap Send. Atlas can't send texts by itself.</p></section>");
    out
}
