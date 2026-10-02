//! Client and brand replies Atlas has drafted, waiting either for you to
//! look at them or for standing approval to let them go on their own.
//!
//! The drafting itself needs no approval — Eric's own rule. What this
//! module holds is the gap between "drafted" and "sent": without
//! `may_email_clients` or `may_email_brands` on, a reply sits here until
//! you ask to see it; with it on, the same reply is sent and reported,
//! and this is where the record of that lives too.

use crate::error::Result;
use crate::store::Store;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A reply to someone already on the client list.
    Client,
    /// Cold outreach to a brand — the riskier one, and the one the daily
    /// cap actually applies to.
    ColdOutreach,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Drafted, not yet sent, waiting on you or on standing approval.
    Waiting,
    Sent,
    /// You looked at it and said no.
    Discarded,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingReply {
    pub id: String,
    /// Which mailbox this would send from — the same account the
    /// original message arrived at.
    pub account: String,
    pub to_address: String,
    pub to_name: String,
    pub subject: String,
    pub body: String,
    pub kind: Kind,
    /// What `draft::critique` found, so a held draft carries its own
    /// second opinion rather than needing it recomputed when you ask to
    /// see it.
    pub critique: Vec<crate::draft::Note>,
    pub created_at: u64,
    pub status: Status,
    /// The message it answers, so it lands in the same conversation.
    #[serde(default)]
    pub thread: Thread,
}

/// Where a reply sits in a conversation: the `Message-ID` it answers and
/// every one before it. Until 1 Oct 2026 replies carried neither, so they
/// arrived as new conversations (research report, Stage 1 item 8).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Thread {
    pub in_reply_to: String,
    pub references: Vec<String>,
}

impl Thread {
    /// Answering the message `id`, which itself followed `refs`. Angle
    /// brackets are taken off; they're written back when it's sent.
    pub fn replying_to(id: &str, refs: &[String]) -> Thread {
        let clean = |s: &str| s.trim().trim_start_matches('<').trim_end_matches('>').trim().to_string();
        let id = clean(id);
        if id.is_empty() {
            return Thread::default();
        }
        let mut references: Vec<String> = refs.iter().map(|r| clean(r)).filter(|r| !r.is_empty() && *r != id).collect();
        references.push(id.clone());
        Thread { in_reply_to: id, references }
    }
}

impl PendingReply {
    pub fn spoken_notice(&self) -> String {
        format!("There's a reply to {} ready to look at.", self.to_name)
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Outbox {
    replies: Vec<PendingReply>,
}

impl Outbox {
    pub fn load(store: &Store) -> Outbox {
        store.load("outbox")
    }

    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("outbox", self)
    }

    /// A short, stable id from what the reply actually is — the same
    /// account, recipient and moment can only produce one draft, so
    /// asking Atlas to draft the same reply twice updates the one
    /// already waiting rather than piling up duplicates.
    pub fn make_id(account: &str, to_address: &str, created_at: u64) -> String {
        format!("{account}:{to_address}:{created_at}")
    }

    pub fn add(&mut self, reply: PendingReply) {
        self.replies.retain(|r| r.id != reply.id);
        self.replies.push(reply);
    }

    pub fn get(&self, id: &str) -> Option<&PendingReply> {
        self.replies.iter().find(|r| r.id == id)
    }

    /// Every reply still waiting on you or on approval — not sent, not
    /// discarded. What "pull up my drafts" and the auto-send sweep both
    /// actually want.
    /// Replies that went to this address.
    pub fn sent_to(&self, address: &str) -> Vec<&PendingReply> {
        self.replies
            .iter()
            .filter(|r| r.status == Status::Sent && r.to_address.eq_ignore_ascii_case(address))
            .collect()
    }

    pub fn waiting(&self) -> Vec<&PendingReply> {
        self.replies.iter().filter(|r| r.status == Status::Waiting).collect()
    }

    /// The most recently created reply still waiting for someone
    /// identified by name or address — "pull up the reply to Jane"
    /// doesn't require you to remember the exact id.
    pub fn waiting_for(&self, who: &str) -> Option<&PendingReply> {
        let who = who.trim().to_lowercase();
        self.replies
            .iter()
            .filter(|r| r.status == Status::Waiting)
            .filter(|r| r.to_address.to_lowercase() == who || r.to_name.to_lowercase() == who)
            .max_by_key(|r| r.created_at)
    }

    pub fn mark_sent(&mut self, id: &str) {
        if let Some(r) = self.replies.iter_mut().find(|r| r.id == id) {
            r.status = Status::Sent;
        }
    }

    pub fn mark_discarded(&mut self, id: &str) {
        if let Some(r) = self.replies.iter_mut().find(|r| r.id == id) {
            r.status = Status::Discarded;
        }
    }

    /// How many `Kind::ColdOutreach` replies were sent today — the number
    /// the daily cap is actually checked against. Client replies never
    /// count here; the cap was never meant to throttle them.
    pub fn cold_outreach_sent_since(&self, since: u64) -> usize {
        self.replies
            .iter()
            .filter(|r| r.kind == Kind::ColdOutreach && r.status == Status::Sent && r.created_at >= since)
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(id: &str, kind: Kind, created_at: u64) -> PendingReply {
        PendingReply {
            id: id.to_string(),
            account: "personal".into(),
            to_address: "jane@client.example".into(),
            to_name: "Jane".into(),
            subject: "Re: project update".into(),
            body: "Sounds good, let's proceed.".into(),
            kind,
            critique: Vec::new(),
            created_at,
            status: Status::Waiting,
            thread: Thread::default(),
        }
    }

    #[test]
    fn a_fresh_outbox_has_nothing_waiting() {
        let ob = Outbox::default();
        assert!(ob.waiting().is_empty());
    }

    #[test]
    fn an_added_reply_shows_up_in_waiting() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        assert_eq!(ob.waiting().len(), 1);
    }

    #[test]
    fn adding_the_same_id_twice_replaces_rather_than_duplicates() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        ob.add(reply("a", Kind::Client, 0));
        assert_eq!(ob.waiting().len(), 1);
    }

    #[test]
    fn marking_a_reply_sent_takes_it_out_of_waiting() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        ob.mark_sent("a");
        assert!(ob.waiting().is_empty());
        assert_eq!(ob.get("a").unwrap().status, Status::Sent);
    }

    #[test]
    fn marking_a_reply_discarded_also_takes_it_out_of_waiting() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        ob.mark_discarded("a");
        assert!(ob.waiting().is_empty());
    }

    #[test]
    fn waiting_for_finds_by_name_or_address_case_insensitively() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        assert!(ob.waiting_for("jane").is_some());
        assert!(ob.waiting_for("JANE").is_some());
        assert!(ob.waiting_for("jane@client.example").is_some());
        assert!(ob.waiting_for("nobody").is_none());
    }

    #[test]
    fn waiting_for_prefers_the_most_recent_when_several_match() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        ob.add(PendingReply { id: "b".into(), created_at: 100, ..reply("b", Kind::Client, 100) });
        let found = ob.waiting_for("jane").unwrap();
        assert_eq!(found.id, "b");
    }

    #[test]
    fn a_sent_reply_is_not_returned_by_waiting_for() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        ob.mark_sent("a");
        assert!(ob.waiting_for("jane").is_none());
    }

    #[test]
    fn cold_outreach_sent_count_ignores_client_replies() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::Client, 0));
        ob.mark_sent("a");
        ob.add(reply("b", Kind::ColdOutreach, 0));
        ob.mark_sent("b");
        assert_eq!(ob.cold_outreach_sent_since(0), 1, "the client reply must not count toward the cap");
    }

    #[test]
    fn cold_outreach_count_ignores_anything_not_yet_sent() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::ColdOutreach, 0));
        // Never marked sent.
        assert_eq!(ob.cold_outreach_sent_since(0), 0);
    }

    #[test]
    fn cold_outreach_count_only_looks_back_to_the_given_time() {
        let mut ob = Outbox::default();
        ob.add(reply("a", Kind::ColdOutreach, 100));
        ob.mark_sent("a");
        assert_eq!(ob.cold_outreach_sent_since(200), 0, "sent before the window must not count");
        assert_eq!(ob.cold_outreach_sent_since(50), 1);
    }

    #[test]
    fn make_id_is_stable_for_the_same_account_recipient_and_moment() {
        let a = Outbox::make_id("personal", "jane@client.example", 100);
        let b = Outbox::make_id("personal", "jane@client.example", 100);
        assert_eq!(a, b);
    }

    #[test]
    fn saved_and_reloaded_outbox_survives_a_restart() {
        let dir = std::env::temp_dir().join(format!("atlas-outbox-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::new(&dir);

        let mut ob = Outbox::load(&store);
        ob.add(reply("a", Kind::Client, 0));
        ob.save(&store).unwrap();

        let reloaded = Outbox::load(&store);
        assert_eq!(reloaded.waiting().len(), 1);
    }
}

/// "Email Sam saying I'll be late" / "send an email to jo@x.com that the
/// deck is ready": who, and what to say, in your words and capitals.
///
/// 30 Sep 2026: there was no way to start an email by asking. Atlas drafted
/// replies to mail that came in, and "email Sam saying ..." went to the
/// model, which said it had sent something it hadn't.
pub fn email_asked(said: &str) -> Option<(String, String)> {
    let s = said.trim().trim_end_matches(['.', '!']);
    let low = s.to_ascii_lowercase();
    let lead = [
        "send an email to ", "send email to ", "send a email to ", "send an e-mail to ", "write an email to ",
        "shoot an email to ", "drop an email to ", "shoot ", "email ", "e-mail ", "can you email ", "please email ",
    ]
    .iter()
    .find(|p| low.starts_with(**p))?;
    let rest_low = &low[lead.len()..];
    let (at, mark) = [" saying ", " telling them ", " that ", ": ", ", ", " to say "]
        .iter()
        .filter_map(|m| rest_low.find(m).map(|i| (i, *m)))
        .min_by_key(|(i, _)| *i)?;
    let who = s[lead.len()..lead.len() + at].trim();
    // "shoot Sam an email saying ..."
    let who = who.strip_suffix(" an email").or_else(|| who.strip_suffix(" a message")).unwrap_or(who).trim();
    if *lead == "shoot " && !low[lead.len()..lead.len() + at].ends_with(" an email") {
        return None;
    }
    let message = s[lead.len() + at + mark.len()..].trim();
    if who.is_empty() || who.split_whitespace().count() > 4 || message.split_whitespace().count() < 2 {
        return None;
    }
    // "email him" with nobody named isn't something to guess at.
    if ["him", "her", "them", "it", "me", "everyone"].contains(&who.to_ascii_lowercase().as_str()) {
        return None;
    }
    Some((who.to_string(), message.to_string()))
}

/// What was said, as a letter's body: the first letter a capital, a lone
/// "i" an "I", ending in a full stop.
pub fn body_from_spoken(message: &str) -> String {
    let mut words: Vec<String> = message
        .split_whitespace()
        .map(|w| match w {
            "i" => "I".to_string(),
            w if w.starts_with("i'") => format!("I{}", &w[1..]),
            w => w.to_string(),
        })
        .collect();
    if let Some(first) = words.first_mut() {
        let mut c = first.chars();
        if let Some(f) = c.next() {
            *first = f.to_uppercase().collect::<String>() + c.as_str();
        }
    }
    let mut body = words.join(" ");
    if !body.ends_with(['.', '!', '?']) {
        body.push('.');
    }
    body
}

/// A subject line from the body: its first few words.
pub fn subject_from_body(body: &str) -> String {
    let words: Vec<&str> = body.split_whitespace().collect();
    let mut s = words.iter().take(7).cloned().collect::<Vec<_>>().join(" ");
    s = s.trim_end_matches(['.', ',', '!', '?', ';', ':']).to_string();
    if words.len() > 7 {
        s.push_str("...");
    }
    s
}
