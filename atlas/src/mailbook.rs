//! A small, local record of your recent mail -- sent and received -- so the
//! things that need "who said what, and when" have something to ask.
//!
//! Mail used to be fetched, sorted and dropped: nothing remembered that you
//! asked Sam for the contract on Monday, so nothing could notice on Thursday
//! that Sam never answered. The waiting-for tracker (`waitingfor`), meeting
//! prep (`meetprep`) and your people (`people`) all read this.
//!
//! **Kept small and plain on purpose.** Headers, the thread links (JWZ, see
//! `mailthread`), and the first `EXCERPT` characters of the body with any
//! secret-looking string scrubbed out (`redact`). Not attachments, not the
//! whole body. `keep_days` of it, and never more than `MAX_LETTERS`.
//! It stays on this machine: it isn't synced to the phone and isn't sent to
//! a model whole (the waiting-for rules read it locally).

use serde::{Deserialize, Serialize};

/// Characters of body kept.
pub const EXCERPT: usize = 600;
/// The most letters kept, oldest dropped first.
pub const MAX_LETTERS: usize = 3000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Letter {
    /// `Message-ID`, without the angle brackets.
    pub id: String,
    pub in_reply_to: Option<String>,
    pub refs: Vec<String>,
    pub from_name: String,
    pub from: String,
    /// Addresses it went to (To and Cc), lower-cased.
    pub to: Vec<String>,
    pub subject: String,
    /// UTC seconds, from the `Date:` header (the fetch time when it's missing
    /// or unreadable, and `dated` says so).
    pub at: u64,
    #[serde(default = "yes")]
    pub dated: bool,
    /// You sent it.
    pub mine: bool,
    pub excerpt: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MailBook {
    pub letters: Vec<Letter>,
    /// Per account address: when its Sent folder was last read, so the
    /// next check asks only for what's newer.
    #[serde(default)]
    pub checked: std::collections::BTreeMap<String, u64>,
}

/// Addresses out of a To/Cc header: "Sam <sam@x.com>, jo@y.com".
pub fn mail_addresses(header: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in header.split(',') {
        let (_, addr) = crate::unsub::split_from(part);
        let addr = addr.trim().trim_matches(|c| c == '"' || c == '\'').to_string();
        if addr.contains('@') && !out.contains(&addr) {
            out.push(addr);
        }
    }
    out
}

fn message_ids(header: &str) -> Vec<String> {
    header
        .split(|c: char| c.is_whitespace() || c == ',')
        .map(|t| t.trim().trim_matches(|c| c == '<' || c == '>').to_string())
        .filter(|t| t.contains('@'))
        .collect()
}

/// The body's first words, with anything secret-looking replaced.
pub fn excerpt(body: &str) -> String {
    let flat: String = body.split_whitespace().collect::<Vec<_>>().join(" ");
    let cut: String = flat.chars().take(EXCERPT).collect();
    if crate::redact::secrets_in(&cut).is_empty() {
        cut
    } else {
        let mut s = crate::redact::Scrubber::default();
        s.scrub(&cut)
    }
}

impl Letter {
    /// From a fetched message. `mine` when it came from your Sent folder (or
    /// is from one of your addresses).
    pub fn from_imap(m: &crate::imap::Message, mine: bool, fetched_at: u64) -> Letter {
        let (from_name, from) = crate::unsub::split_from(&m.from);
        let parsed = crate::triage::parse_rfc2822(&m.date);
        let mut to = mail_addresses(&m.to);
        for a in mail_addresses(&m.cc) {
            if !to.contains(&a) {
                to.push(a);
            }
        }
        Letter {
            id: m.message_id.trim().trim_matches(|c| c == '<' || c == '>').to_string(),
            in_reply_to: message_ids(&m.in_reply_to).into_iter().next(),
            refs: message_ids(&m.references),
            from_name,
            from,
            to,
            subject: m.subject.trim().to_string(),
            at: parsed.unwrap_or(fetched_at),
            dated: parsed.is_some(),
            mine,
            excerpt: excerpt(&m.body),
        }
    }

    /// The conversation it belongs to: the first message it refers back to,
    /// or its own id when it starts one.
    pub fn thread(&self) -> String {
        self.refs.first().cloned().or_else(|| self.in_reply_to.clone()).unwrap_or_else(|| self.id.clone())
    }

    /// Everyone else on it: the sender if it's theirs, the recipients if
    /// it's yours.
    pub fn others(&self) -> Vec<String> {
        if self.mine {
            self.to.clone()
        } else {
            vec![self.from.clone()]
        }
    }
}

impl MailBook {
    /// Where it's kept.
    pub const FILE: &'static str = "mailbook";

    /// Add letters, skipping any already held (by id), then forget what's
    /// past `keep_days`. Returns how many were new.
    pub fn add(&mut self, letters: Vec<Letter>, keep_days: u64, now: u64) -> usize {
        let mut new = 0;
        let mut held: std::collections::HashSet<String> = self.letters.iter().map(|x| x.id.clone()).collect();
        for l in letters {
            if l.id.is_empty() || !held.insert(l.id.clone()) {
                continue;
            }
            self.letters.push(l);
            new += 1;
        }
        self.letters.sort_by_key(|l| l.at);
        let cut = now.saturating_sub(keep_days * 86_400);
        self.letters.retain(|l| l.at >= cut);
        let over = self.letters.len().saturating_sub(MAX_LETTERS);
        if over > 0 {
            self.letters.drain(..over);
        }
        new
    }

    /// Every letter in a conversation, oldest first. A letter belongs if it
    /// shares the thread root, or names the root's id in its references.
    pub fn conversation(&self, thread: &str) -> Vec<&Letter> {
        self.letters.iter().filter(|l| l.thread() == thread || l.id == thread || l.refs.iter().any(|r| r == thread)).collect()
    }

    /// When you last heard from, or wrote to, this address.
    pub fn last_contact(&self, address: &str) -> Option<(u64, bool)> {
        let a = address.to_lowercase();
        self.letters
            .iter()
            .rev()
            .find(|l| (l.mine && l.to.contains(&a)) || (!l.mine && l.from == a))
            .map(|l| (l.at, l.mine))
    }

    /// Recent letters with anyone whose address or name holds `who`.
    pub fn with(&self, who: &str, n: usize) -> Vec<&Letter> {
        let w = who.to_lowercase();
        if w.trim().len() < 2 {
            return Vec::new();
        }
        self.letters
            .iter()
            .rev()
            .filter(|l| {
                l.from.contains(&w) || l.from_name.to_lowercase().contains(&w) || l.to.iter().any(|t| t.contains(&w))
            })
            .take(n)
            .collect()
    }
}
