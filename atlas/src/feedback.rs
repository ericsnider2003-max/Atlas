//! Feedback: your friends tell you something's wrong with Atlas, when *they*
//! decide it is, and you tell them what you did about it.
//!
//! Eric, 26 Sep: "I don't want my friends' Atlas to tell me. I want a way for
//! my friends to be able to submit feedback to me to tell me that there is a
//! bug when the friend makes that determination, then I can get a report.
//! Kind of like a feedback loop."
//!
//! So nothing here happens by itself.
//!
//! 1. **The friend decides.** `atlas feedback send` asks what's wrong in their
//!    own words.
//!    - If an update failed on their Atlas, it offers to attach what was
//!      written down about it (`update_apply::FailureReport`). The attachment
//!      holds the version, the step it failed at, its error lines and where it
//!      crashed, with their name and home folder already taken out.
//!    - It shows them exactly that, and attaches it only on their yes.
//!    - It sends only on a second yes.
//! 2. **It travels over the pairing** to whoever sends them Atlas updates: the
//!    owner of their release channel. It goes through its own door,
//!    `/feedback`, which is token-checked, size-capped and rate-limited. The
//!    only thing that door does is file it.
//! 3. **You get a report.** Each piece of feedback lands in `atlas feedback`
//!    with its own number, who sent it, from which version and device, their
//!    words, and the attachment. An attached failure also goes into
//!    `atlas update failures`, so the fix brief has it.
//! 4. **You answer, and they hear it.** `atlas feedback reply <n> ...`
//!    marks it seen, being fixed, fixed in a version, or not something you'll
//!    change, with a note if you like. The answer travels back through
//!    `/feedback-reply`.
//!    - Their Atlas tells them in one sentence.
//!    - It takes an answer only from the person the feedback was sent to,
//!      and only about feedback it really sent.
//!
//! What never happens: a friend's Atlas reporting on them, halting your
//! releases on a friend's say-so, or sending anything the friend didn't see.

use crate::store::Store;
use crate::update_apply::FailureReport;
use serde::{Deserialize, Serialize};

const SENT: &str = "feedback_sent";
const OUTBOX: &str = "feedback_outbox";
const INBOX: &str = "feedback_inbox";
const REPLIES_OUT: &str = "feedback_replies_out";

/// The largest piece of feedback (or answer) accepted over a pairing.
pub const MAX_FEEDBACK_BYTES: usize = 24 * 1024;
/// The longest the written part may be.
pub const MAX_WORDS_CHARS: usize = 4_000;

/// Where a piece of feedback stands, as the person who received it says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case", tag = "state", content = "detail")]
pub enum FeedbackStatus {
    /// Sent (on the sender's side) or arrived (on yours), not answered yet.
    #[default]
    New,
    Seen,
    Fixing,
    /// Fixed in this version.
    Fixed(String),
    /// Not something that will change.
    WontChange,
}

impl FeedbackStatus {
    pub fn plain(&self) -> String {
        match self {
            FeedbackStatus::New => "not answered yet".into(),
            FeedbackStatus::Seen => "seen".into(),
            FeedbackStatus::Fixing => "being fixed".into(),
            FeedbackStatus::Fixed(v) => format!("fixed in {v}"),
            FeedbackStatus::WontChange => "won't be changed".into(),
        }
    }

    /// Read from what you type: `seen`, `fixing`, `fixed 1.4.2`, `wont`.
    pub fn from_words(word: &str, version: Option<&str>) -> Option<FeedbackStatus> {
        Some(match word {
            "seen" => FeedbackStatus::Seen,
            "fixing" => FeedbackStatus::Fixing,
            "fixed" => FeedbackStatus::Fixed(version?.to_string()),
            "wont" | "won't" | "wontfix" => FeedbackStatus::WontChange,
            _ => return None,
        })
    }
}

/// One piece of feedback.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feedback {
    /// Random, made by the sender; how an answer finds its way back.
    pub id: String,
    /// In their own words.
    pub words: String,
    /// The Atlas version it was sent from.
    pub version: String,
    pub platform: String,
    /// An update failure the sender chose to attach.
    #[serde(default)]
    pub attached: Option<FailureReport>,
    pub at: u64,
    /// Who it's from, set by the receiver from the pairing, never trusted from the body.
    #[serde(default)]
    pub from: String,
    /// Who it went to, by pairing name (sender's side).
    #[serde(default)]
    pub to: String,
    #[serde(default)]
    pub status: FeedbackStatus,
    /// Answers, oldest first: (when, words).
    #[serde(default)]
    pub replies: Vec<(u64, String)>,
}

/// An answer to a piece of feedback, as it travels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Answer {
    pub id: String,
    pub status: FeedbackStatus,
    #[serde(default)]
    pub note: String,
    pub at: u64,
}

fn new_id() -> String {
    crate::release::seed_hex(&crate::release::new_seed())[..16].to_string()
}

/// Write a piece of feedback. `attach` only if the person said yes to it.
/// Who sends this Atlas its updates: (their key, your name for them if
/// you're paired, whether that's you). From your release channel's signed
/// list -- the same answer for `atlas feedback send`, the voice, and the
/// window's button.
pub fn release_sender(store: &Store, peer_dir: &std::path::Path) -> Option<(String, Option<String>, bool)> {
    let groups = crate::groups::Groups::load(store);
    let channel = groups.held.values().map(|h| &h.state).find(|g| g.release_channel)?;
    let me = crate::peerkey::Identity::load_or_create(peer_dir).ok().map(|i| i.public()).unwrap_or_default();
    let mine = !me.is_empty() && (channel.owner == me || channel.is_delegate(&me));
    let name = crate::kin::Pairings::load(peer_dir).name_of_key(&channel.owner);
    Some((channel.owner.clone(), name, mine))
}

/// Where a piece of feedback went, once it's been decided on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sending {
    /// Into your own list: this is your own Atlas.
    Filed,
    /// Queued for them; the running Atlas delivers it.
    Queued(String),
}

impl Sending {
    pub fn plain(&self) -> String {
        match self {
            Sending::Filed => "This is your own Atlas, so it's in your own feedback list.".into(),
            Sending::Queued(to) => format!("Sending it to {to}. It goes when Atlas can reach them, and you'll hear when they answer."),
        }
    }
}

/// Send (or, on your own Atlas, file) a piece of feedback the person has
/// already seen and said yes to.
pub fn send_decided(store: &Store, peer_dir: &std::path::Path, f: Feedback) -> Result<Sending, String> {
    let (_, name, mine) = release_sender(store, peer_dir)
        .ok_or("This Atlas isn't in anyone's release channel, so there's no one to send feedback to.")?;
    if mine {
        // unheard-ok: returns `Option<String>`, not a Result
        let _ = heard_feedback(store, "you", &serde_json::to_string(&f).unwrap_or_default());
        return Ok(Sending::Filed);
    }
    let to = name.ok_or("I can't reach whoever sends you updates -- you aren't paired with them -- so nothing was sent.")?;
    queue_feedback(store, f, &to);
    Ok(Sending::Queued(to))
}

/// Everything about feedback, in a few sentences for saying out loud: what
/// came in (numbered, as `atlas feedback` numbers it) and what you sent.
pub fn spoken_list(store: &Store) -> String {
    let got = feedback_inbox(store);
    let sent = feedback_sent(store);
    if got.is_empty() && sent.is_empty() {
        return "No feedback yet, either way. Say \"report a bug\" and what's wrong to tell whoever sends you Atlas.".into();
    }
    let mut out = Vec::new();
    if !got.is_empty() {
        let new = got.iter().filter(|f| f.status == FeedbackStatus::New).count();
        out.push(format!(
            "{} piece{} of feedback from your friends{}.",
            got.len(),
            if got.len() == 1 { "" } else { "s" },
            if new > 0 { format!(", {new} not answered") } else { String::new() }
        ));
        for (i, f) in got.iter().enumerate().rev().take(3) {
            out.push(format!(
                "Number {}, from {} on Atlas {}: \"{}\"{} -- {}.",
                i + 1,
                f.from,
                f.version,
                f.words,
                if f.attached.is_some() { ", with an update failure attached" } else { "" },
                f.status.plain()
            ));
        }
        if new > 0 {
            out.push("Say \"answer feedback\" with its number and seen, fixing, fixed and the version, or wont.".into());
        }
    }
    for f in sent.iter().rev().take(2) {
        let mut line = format!("You told {}: \"{}\" -- {}.", f.to, f.words, f.status.plain());
        if let Some((_, note)) = f.replies.last() {
            line.push_str(&format!(" They said: {note}"));
        }
        out.push(line);
    }
    out.join(" ")
}

/// "2 fixing", "1 fixed 1.3.0 thanks", "3 wont not a bug": the same words
/// `atlas feedback reply` takes, read from a sentence.
pub fn read_reply(words: &str) -> Option<(usize, FeedbackStatus, String)> {
    let w: Vec<&str> = words.split_whitespace().filter(|w| !["number", "as", "to", "is"].contains(&w.to_lowercase().as_str())).collect();
    let n = w.first()?.trim_start_matches('#').parse::<usize>().ok()?;
    let word = w.get(1)?.to_lowercase().replace('\'', "");
    let version = w.get(2).copied();
    let status = FeedbackStatus::from_words(&word, version)?;
    let skip = if matches!(status, FeedbackStatus::Fixed(_)) { 3 } else { 2 };
    Some((n, status, w.iter().skip(skip).cloned().collect::<Vec<_>>().join(" ")))
}

pub fn compose_feedback(words: &str, attach: Option<FailureReport>, now: u64) -> Result<Feedback, String> {
    let words = words.trim();
    if words.is_empty() {
        return Err("There's nothing written to send.".into());
    }
    if words.chars().count() > MAX_WORDS_CHARS {
        return Err(format!("That's longer than {MAX_WORDS_CHARS} characters; say it in fewer words."));
    }
    Ok(Feedback {
        id: new_id(),
        words: words.to_string(),
        version: crate::upgrade::version().to_string(),
        platform: crate::release::this_platform().unwrap_or("unknown").to_string(),
        attached: attach,
        at: now,
        ..Default::default()
    })
}

/// Exactly what would be sent, for the person to read before saying yes.
pub fn feedback_preview(f: &Feedback) -> String {
    let mut s = format!("From Atlas {} on {}:\n  \"{}\"\n", f.version, f.platform, f.words);
    if let Some(a) = &f.attached {
        s.push_str(&format!("Attached: Atlas {} failed at its {} here.\n", a.version, a.stage));
        for r in a.reasons.iter().filter(|r| !r.starts_with("(passed)")) {
            s.push_str(&format!("  - {r}\n"));
        }
        if !a.crash.is_empty() {
            s.push_str(&format!("  crash: {}\n", a.crash.replace('\n', " | ")));
        }
    }
    s
}

/// Queue feedback for the person with this pairing name. The running Atlas
/// sends it when it can reach them, and keeps it until they took it.
pub fn queue_feedback(store: &Store, mut f: Feedback, to: &str) {
    f.to = to.to_string();
    let mut out: Vec<Feedback> = store.load(OUTBOX);
    out.push(f.clone());
    let _ = store.save(OUTBOX, &out);
    let mut sent: Vec<Feedback> = store.load(SENT);
    sent.push(f);
    let _ = store.save(SENT, &sent);
}

/// Feedback still to be sent: (to, wire body, id).
pub fn feedback_outbox(store: &Store) -> Vec<(String, String, String)> {
    store
        .load::<Vec<Feedback>>(OUTBOX)
        .into_iter()
        .map(|f| (f.to.clone(), serde_json::to_string(&Feedback { to: String::new(), ..f.clone() }).unwrap_or_default(), f.id))
        .collect()
}

/// It reached them.
pub fn feedback_delivered(store: &Store, id: &str) {
    let mut out: Vec<Feedback> = store.load(OUTBOX);
    out.retain(|f| f.id != id);
    let _ = store.save(OUTBOX, &out);
}

/// What you've sent, with where each stands.
pub fn feedback_sent(store: &Store) -> Vec<Feedback> {
    store.load(SENT)
}

/// On your side: feedback arrived from `from` (a pairing's name). Filed and
/// said in one sentence; `None` for anything that isn't feedback, and for the
/// same piece arriving twice.
pub fn heard_feedback(store: &Store, from: &str, body: &str) -> Option<String> {
    if body.len() > MAX_FEEDBACK_BYTES {
        return None;
    }
    let mut f: Feedback = serde_json::from_str(body).ok()?;
    if f.id.is_empty() || f.id.len() > 64 || f.words.trim().is_empty() || f.words.chars().count() > MAX_WORDS_CHARS {
        return None;
    }
    // A phone to add to the next iPhone build rides this channel too
    // (`phoneadd`); it goes to the waiting list, not the inbox.
    if f.words.starts_with(crate::phoneadd::WIRE_PREFIX) {
        return crate::phoneadd::heard(store, from, &f.words, f.at);
    }
    f.from = from.to_string();
    f.to.clear();
    f.status = FeedbackStatus::New;
    f.replies.clear();
    let mut inbox: Vec<Feedback> = store.load(INBOX);
    if inbox.iter().any(|x| x.id == f.id && x.from == f.from) {
        return None;
    }
    // An attached failure also goes where the fix briefs are made from.
    if let Some(a) = f.attached.as_mut() {
        a.from = from.to_string();
        a.because = crate::update_apply::failed_because(&a.reasons, &a.crash);
        crate::update_apply::file_friend_report(store, a.clone());
    }
    let n = inbox.len() + 1;
    let said = format!(
        "{from} sent feedback about Atlas {}: \"{}\"{}. It's number {n} on your Feedback page.",
        f.version,
        short(&f.words),
        if f.attached.is_some() { ", with the details of an update that failed" } else { "" }
    );
    inbox.push(f);
    let _ = store.save(INBOX, &inbox);
    Some(said)
}

fn short(s: &str) -> String {
    let one: String = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() > 90 {
        format!("{}...", one.chars().take(90).collect::<String>())
    } else {
        one
    }
}

/// Everything your friends have sent you, numbered from 1.
pub fn feedback_inbox(store: &Store) -> Vec<Feedback> {
    store.load(INBOX)
}

/// Answer feedback number `n` (from `inbox`): its new status and a note.
/// Queued back to whoever sent it. Returns their name.
pub fn answer_feedback(store: &Store, n: usize, status: FeedbackStatus, note: &str, now: u64) -> Result<String, String> {
    let mut inbox: Vec<Feedback> = store.load(INBOX);
    let f = n.checked_sub(1).and_then(|i| inbox.get_mut(i)).ok_or_else(|| format!("There's no feedback number {n}."))?;
    f.status = status.clone();
    if !note.trim().is_empty() {
        f.replies.push((now, note.trim().to_string()));
    }
    let a = Answer { id: f.id.clone(), status, note: note.trim().to_string(), at: now };
    let to = f.from.clone();
    let _ = store.save(INBOX, &inbox);
    let mut out: Vec<(String, Answer)> = store.load(REPLIES_OUT);
    out.push((to.clone(), a));
    let _ = store.save(REPLIES_OUT, &out);
    Ok(to)
}

/// Answers still to be sent: (to, wire body, id).
pub fn answers_out(store: &Store) -> Vec<(String, String, String)> {
    store
        .load::<Vec<(String, Answer)>>(REPLIES_OUT)
        .into_iter()
        .map(|(to, a)| (to, serde_json::to_string(&a).unwrap_or_default(), a.id))
        .collect()
}

/// An answer reached them.
pub fn answer_delivered(store: &Store, id: &str) {
    let mut out: Vec<(String, Answer)> = store.load(REPLIES_OUT);
    if let Some(i) = out.iter().position(|(_, a)| a.id == id) {
        out.remove(i);
    }
    let _ = store.save(REPLIES_OUT, &out);
}

/// On the sender's side: an answer arrived from `from`. Taken only about
/// feedback this Atlas really sent, and only from the person it went to.
pub fn heard_answer(store: &Store, from: &str, body: &str) -> Option<String> {
    if body.len() > MAX_FEEDBACK_BYTES {
        return None;
    }
    let a: Answer = serde_json::from_str(body).ok()?;
    let mut sent: Vec<Feedback> = store.load(SENT);
    let f = sent.iter_mut().find(|f| f.id == a.id && f.to.eq_ignore_ascii_case(from))?;
    if f.replies.iter().any(|(at, _)| *at == a.at) && f.status == a.status {
        return None; // the same answer twice
    }
    f.status = a.status.clone();
    if !a.note.is_empty() {
        f.replies.push((a.at, a.note.clone()));
    }
    let said = format!(
        "{from} answered your feedback (\"{}\"): {}{}",
        short(&f.words),
        a.status.plain(),
        if a.note.is_empty() { ".".to_string() } else { format!(" -- \"{}\"", a.note) }
    );
    let _ = store.save(SENT, &sent);
    Some(said)
}
