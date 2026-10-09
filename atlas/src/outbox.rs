//! Client and brand replies Atlas has drafted for review.
//!
//! The drafting itself needs no approval — Eric's own rule. What this
//! module holds is the gap between "drafted" and "sent": without
//! client replies await exact fresh approval. Brand outreach retains its
//! separate master switch and per-recipient approval boundary. Submission
//! receipts live here too.

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailReceipt { ProgramAccepted, OwnerConfirmedSent, OwnerConfirmedAbsent, #[serde(other)] Unknown }

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
    #[serde(default)]
    submissions: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    delivery_receipts: std::collections::BTreeMap<String, MailReceipt>,
}

impl Outbox {
    pub fn load(store: &Store) -> Outbox {
        Self::load_checked(store).unwrap_or_default()
    }
    pub fn load_checked(store: &Store) -> Result<Outbox> { Ok(store.load_checked("outbox")?.unwrap_or_default()) }

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
        if self.submissions.contains_key(&reply.id) { return; }
        if self.get(&reply.id).is_some_and(|r| r.status != Status::Waiting) { return; }
        self.replies.retain(|r| r.id != reply.id);
        self.replies.push(reply);
    }

    pub fn keep_draft(store: &Store, reply: PendingReply) -> Result<()> {
        let _guard = store.transaction()?;
        let mut outbox = Self::load_checked(store)?;
        if let Some(current) = outbox.get(&reply.id) {
            if outbox.submissions.contains_key(&reply.id) || current.status != Status::Waiting {
                if serde_json::to_value(current).ok() == serde_json::to_value(&reply).ok() { return Ok(()); }
                return Err(std::io::Error::new(std::io::ErrorKind::AlreadyExists, "That draft identity already has a submitted or discarded outcome; the new draft was not kept. Create a fresh draft before approving it.").into());
            }
        }
        outbox.add(reply);
        outbox.save(store)
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
        self.submissions.remove(id);
        self.delivery_receipts.insert(id.to_string(), MailReceipt::ProgramAccepted);
        if let Some(r) = self.replies.iter_mut().find(|r| r.id == id) {
            r.status = Status::Sent;
        }
    }

    pub fn submission_status(&self, id: &str) -> Option<&str> { self.submissions.get(id).map(String::as_str) }
    pub fn delivery_receipt(&self, id: &str) -> Option<MailReceipt> { self.delivery_receipts.get(id).copied() }
    pub fn unconfirmed(&self) -> Vec<&PendingReply> { self.replies.iter().filter(|r| self.submissions.contains_key(&r.id)).collect() }
    pub fn unconfirmed_for(&self, who: &str) -> Option<&PendingReply> { self.unconfirmed().into_iter().filter(|r| r.to_name.eq_ignore_ascii_case(who) || r.to_address.eq_ignore_ascii_case(who)).max_by_key(|r| r.created_at) }
    pub fn reconcile_owner_check(&mut self, who: &str, sent: bool) -> bool {
        let matching: Vec<String> = self.unconfirmed().into_iter().filter(|r| r.to_name.eq_ignore_ascii_case(who) || r.to_address.eq_ignore_ascii_case(who)).map(|r| r.id.clone()).collect();
        if matching.len() != 1 { return false; }
        let id = &matching[0];
        let Some(reply) = self.replies.iter_mut().find(|r| &r.id == id) else { return false; };
        reply.status = if sent { Status::Sent } else { Status::Waiting };
        self.submissions.remove(id);
        self.delivery_receipts.insert(id.clone(), if sent { MailReceipt::OwnerConfirmedSent } else { MailReceipt::OwnerConfirmedAbsent });
        true
    }

    /// Persist an old-build-safe fence before any transport can accept mail.
    pub fn begin_submission(&mut self, expected: &PendingReply) -> bool {
        if self.submissions.contains_key(&expected.id) { return false; }
        let Some(current) = self.replies.iter_mut().find(|r| r.id == expected.id) else { return false; };
        if current.status != Status::Waiting || serde_json::to_value(&*current).ok() != serde_json::to_value(expected).ok() { return false; }
        current.status = Status::Discarded;
        self.submissions.insert(expected.id.clone(), "Submission pending; outcome unconfirmed. Check the recipient/service before repeating.".into());
        true
    }

    fn uncertain_submission(&mut self, id: &str, why: &str) {
        if let Some(status) = self.submissions.get_mut(id) { *status = format!("Submission unconfirmed: {why}. Do not repeat without checking."); }
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
            .filter(|r| r.kind == Kind::ColdOutreach && (r.status == Status::Sent || self.submissions.contains_key(&r.id)) && r.created_at >= since)
            .count()
    }
}


pub fn send_fenced_capped(store: &Store, pending: &PendingReply, cap: Option<(u64, usize)>, send: impl FnOnce() -> std::result::Result<(), String>) -> std::result::Result<(), String> {
    let guard = store.transaction().map_err(|e| format!("Nothing submitted: could not reserve the draft state ({e})"))?;
    let mut outbox = Outbox::load_checked(store).map_err(|e| format!("Nothing submitted: outbox state couldn't be read ({e})"))?;
    if let Some((since, limit)) = cap { if pending.kind == Kind::ColdOutreach && outbox.cold_outreach_sent_since(since) >= limit { return Err("Nothing submitted: today's outreach cap, including unconfirmed submissions, is reached.".into()); } }
    if outbox.get(&pending.id).is_none() { outbox.add(pending.clone()); }
    if !outbox.begin_submission(pending) { return Err("This exact draft is no longer waiting, or its earlier submission is unconfirmed.".into()); }
    outbox.save(store).map_err(|e| format!("Nothing submitted: couldn't save its submission fence ({e})"))?;
    drop(guard);
    match send() {
        Ok(()) => {
            let _guard = store.transaction().map_err(|e| format!("The service accepted the reply, but state is busy ({e}); its persisted fence prevents repetition."))?;
            let mut outbox = Outbox::load_checked(store).map_err(|e| format!("The service accepted the reply, but its state couldn't be read ({e}); do not repeat it."))?;
            if outbox.submission_status(&pending.id).is_none() { return Err("The service accepted the reply, but its persisted submission record could not be read; do not repeat it.".into()); }
            outbox.mark_sent(&pending.id);
            outbox.save(store).map_err(|e| format!("The service accepted the reply, but its sent receipt couldn't be saved ({e}); the durable submission fence prevents another send."))
        }
        Err(e) => {
            let _guard = store.transaction().map_err(|_| format!("Submission remains unconfirmed ({e}); its persisted fence prevents repetition."))?;
            let mut outbox = Outbox::load_checked(store).map_err(|_| format!("Submission remains unconfirmed ({e}); state couldn't be read. Do not repeat it."))?;
            if outbox.submission_status(&pending.id).is_none() { return Err(format!("Submission remains unconfirmed ({e}); its persisted record could not be read. Do not repeat it.")); }
            outbox.uncertain_submission(&pending.id, &e);
            let _ = outbox.save(store);
            Err(format!("The reply's outcome isn't confirmed ({e}); check the service before another attempt."))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn submission_fence_blocks_retry_after_restart_and_older_load() {
        let root = std::env::temp_dir().join(format!("atlas-mail-fence-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = Store::new(root.clone());
        let draft = reply("fenced", Kind::Client, 100);
        let calls = std::cell::Cell::new(0);
        assert!(send_fenced_capped(&store, &draft, None, || { calls.set(calls.get()+1); Err("connection lost after DATA".into()) }).is_err());
        assert_eq!(calls.get(), 1);
        let reloaded = Outbox::load(&store);
        assert!(reloaded.waiting().is_empty());
        assert!(reloaded.submission_status("fenced").unwrap().contains("unconfirmed"));
        assert!(send_fenced_capped(&store, &draft, None, || { calls.set(calls.get()+1); Ok(()) }).is_err());
        assert_eq!(calls.get(), 1);
        let mut legacy = serde_json::to_value(reloaded).unwrap();
        legacy.as_object_mut().unwrap().remove("submissions");
        let legacy: Outbox = serde_json::from_value(legacy).unwrap();
        assert!(legacy.waiting().is_empty());
        let _ = std::fs::remove_file(root.join("outbox.json"));
    }

    #[test]
    fn failed_fence_save_makes_zero_provider_calls() {
        let path = std::env::temp_dir().join(format!("atlas-mail-blocked-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::write(&path, b"block directory creation").unwrap();
        let store = Store::new(path.clone());
        assert!(send_fenced_capped(&store, &reply("zero", Kind::Client, 100), None, || panic!("provider must not be called if durable fence failed")).is_err());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn a_new_draft_cannot_claim_a_sent_identity_was_saved() {
        let root = std::env::temp_dir().join(format!("atlas-mail-collision-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = Store::new(root);
        let draft = reply("same-second", Kind::Client, 100);
        send_fenced_capped(&store, &draft, None, || Ok(())).unwrap();
        let mut collision = draft.clone(); collision.body = "different request in the same second".into();
        assert!(Outbox::keep_draft(&store, collision).is_err());
        let current = Outbox::load_checked(&store).unwrap();
        assert_eq!(current.get("same-second").unwrap().body, draft.body);
        assert_eq!(current.get("same-second").unwrap().status, Status::Sent);
        assert!(current.waiting().is_empty());
    }

    #[test]
    fn stale_inbox_draft_cannot_overwrite_submission_and_owner_check_requires_one_exact_recipient() {
        let root = std::env::temp_dir().join(format!("atlas-mail-stale-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = Store::new(root);
        let draft = reply("stale", Kind::ColdOutreach, 100);
        assert!(send_fenced_capped(&store, &draft, None, || Err("lost receipt".into())).is_err());
        let mut stale = draft.clone(); stale.body = "new body from a delayed model".into();
        assert!(Outbox::keep_draft(&store, stale).is_err());
        let mut current = Outbox::load(&store);
        assert_eq!(current.get("stale").unwrap().body, draft.body);
        assert!(current.waiting().is_empty());
        assert_eq!(current.cold_outreach_sent_since(0), 1);
        let next = reply("capped", Kind::ColdOutreach, 101);
        assert!(send_fenced_capped(&store, &next, Some((0, 1)), || panic!("uncertain submissions must consume the cap before another provider call")).is_err());
        assert!(current.reconcile_owner_check(&draft.to_address, false));
        assert_eq!(current.waiting().len(), 1);
        assert_eq!(current.waiting()[0].body, draft.body);
        assert_eq!(current.delivery_receipts["stale"], MailReceipt::OwnerConfirmedAbsent);
        let mut second = reply("second", Kind::Client, 101); second.to_address = draft.to_address.clone();
        current.add(second.clone());
        assert!(current.begin_submission(&draft)); assert!(current.begin_submission(&second));
        assert!(!current.reconcile_owner_check(&draft.to_address, true));
    }

    #[test]
    fn accepted_mail_with_unsaved_receipt_stays_fenced() {
        let root = std::env::temp_dir().join(format!("atlas-mail-receipt-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = Store::new(root.clone());
        let draft = reply("receipt", Kind::Client, 100);
        let backup = root.with_extension("fenced-copy");
        let result = send_fenced_capped(&store, &draft, None, || { std::fs::rename(&root, &backup).map_err(|e| e.to_string())?; std::fs::write(&root, b"state unavailable").map_err(|e| e.to_string())?; Ok(()) });
        assert!(result.is_err());
        std::fs::remove_file(&root).unwrap();
        std::fs::rename(&backup, &root).unwrap();
        assert!(Outbox::load(&store).waiting().is_empty());
        assert!(send_fenced_capped(&store, &draft, None, || panic!("accepted mail must not be repeated after a lost receipt")).is_err());
    }

    #[test]
    fn unreadable_corrupt_or_unknown_format_outbox_never_defaults_into_another_send() {
        let root = std::env::temp_dir().join(format!("atlas-mail-checked-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        let store = Store::new(root.clone());
        let draft = reply("checked", Kind::Client, 100);
        Outbox::keep_draft(&store, draft.clone()).unwrap();
        for bytes in [b"broken JSON".as_slice(), b"{\"schema\":99,\"data\":{\"replies\":[]}}".as_slice()] {
            std::fs::write(root.join("outbox.json"), bytes).unwrap();
            assert!(send_fenced_capped(&store, &draft, None, || panic!("unreadable state must never become implicit approval")).is_err());
            assert_eq!(std::fs::read(root.join("outbox.json")).unwrap(), bytes);
        }
        std::fs::remove_file(root.join("outbox.json")).unwrap();
        std::fs::create_dir(root.join("outbox.json")).unwrap();
        assert!(send_fenced_capped(&store, &draft, None, || panic!("non-file state must not reach provider")).is_err());
        assert!(root.join("outbox.json").is_dir());
    }

    #[test]
    fn checked_state_read_preserves_unknown_fields_in_legacy_and_current_envelopes() {
        let root = std::env::temp_dir().join(format!("atlas-mail-unknown-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let store = Store::new(root.clone());
        for input in [serde_json::json!({"replies":[],"future_field":{"keep":42}}), serde_json::json!({"schema":crate::store::SCHEMA,"data":{"replies":[],"future_field":{"keep":42}}})] {
            std::fs::write(root.join("outbox.json"), input.to_string()).unwrap();
            let checked = Outbox::load_checked(&store).unwrap();
            checked.save(&store).unwrap();
            let saved: serde_json::Value = serde_json::from_slice(&std::fs::read(root.join("outbox.json")).unwrap()).unwrap();
            assert_eq!(saved["data"]["future_field"]["keep"], 42);
        }
    }

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
