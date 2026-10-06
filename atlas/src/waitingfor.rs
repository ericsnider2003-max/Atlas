//! Who owes you a reply, and what you said you'd do.
//!
//! Two lists, read from your recent mail (`mailbook`):
//!
//! - **Owed to you.** You asked something ("can you send…", "could you…",
//!   "let me know…", a question to them) and nobody on the other side has
//!   written back in that conversation since. Said once it's been
//!   `owed_after_days` working days.
//! - **You promised.** You wrote "I'll send it Friday", "I will get back to
//!   you", "let me check and…" -- a commitment, with a date when you gave
//!   one (read by `when`, against the day you wrote it). Said on the day
//!   it's due, and after.
//!
//! **Sources:** Microsoft Research's work on commitment detection in email
//! (Lampert et al.; the "Email overload" project) established the two
//! things this is built around: requests and commitments are carried by a
//! small set of phrasings, and a detector trained on one organisation's mail
//! does badly on another's. So these are rules, not a model, and **you teach
//! it**: "that's not a promise" (or "done") on an item is recorded, and a
//! phrasing you've dismissed more than you've kept stops counting
//! (`Taught::trusts`). Research on why people abandon trackers (Epstein et
//! al., UbiComp 2015: forgetting and upkeep) is why it fills itself in from
//! mail you already sent rather than asking you to log anything.

use crate::mailbook::{Letter, MailBook};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WaitingConfig {
    pub enabled: bool,
    /// Working days before an unanswered question of yours is said.
    pub owed_after_days: u64,
    /// How far back to look.
    pub look_back_days: u64,
}

impl Default for WaitingConfig {
    fn default() -> Self {
        WaitingConfig { enabled: true, owed_after_days: 3, look_back_days: 30 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Side {
    /// They owe you a reply.
    Owed,
    /// You said you'd do something.
    Promised,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Waiting {
    pub side: Side,
    /// The letter it came from.
    pub letter: String,
    pub thread: String,
    pub with: String,
    pub subject: String,
    /// The sentence that carried it, shortened.
    pub said: String,
    /// The phrasing that matched, for learning.
    pub cue: String,
    pub since: u64,
    /// When it's due: for a promise, the date you gave (or none); for a
    /// request, when the waiting becomes worth saying.
    pub due: Option<u64>,
}

/// What you've told it about items, and what that teaches.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Taught {
    /// Letter ids you've closed ("done", "not a promise").
    pub closed: Vec<String>,
    /// Per cue: (kept, dismissed as wrong).
    pub cues: BTreeMap<String, (u32, u32)>,
}

impl Taught {
    /// A cue stops counting once you've called it wrong more often than
    /// right, three times at least.
    pub fn trusts(&self, cue: &str) -> bool {
        match self.cues.get(cue) {
            Some((kept, wrong)) => !(*wrong >= 3 && wrong > kept),
            None => true,
        }
    }

    /// You closed it because it's done: the cue was right.
    pub fn done(&mut self, w: &Waiting) {
        self.closed.push(w.letter.clone());
        self.cues.entry(w.cue.clone()).or_default().0 += 1;
    }

    /// You closed it because it was never a request or a promise.
    pub fn wrong(&mut self, w: &Waiting) {
        self.closed.push(w.letter.clone());
        self.cues.entry(w.cue.clone()).or_default().1 += 1;
    }
}

/// Phrasings that ask something of the other side.
const ASKS: &[&str] = &[
    "can you", "could you", "would you", "will you", "please send", "please let me know", "let me know",
    "please confirm", "can i get", "could i get", "are you able to", "please share", "please review",
    "any update", "do you have", "when can you", "waiting to hear", "get back to me",
];

/// Phrasings that commit you to something.
const PROMISES: &[&str] = &[
    "i'll", "i will", "i'm going to", "i am going to", "will send", "will get back", "let me check",
    "i'll get back", "i can send", "i'll follow up", "will follow up", "i'll have it", "you'll have it",
];

/// Where a sentence says it isn't a commitment after all.
const HEDGES: &[&str] = &["if you", "i'll be", "i will be", "i'll need", "i'll let you know if", "i'd", "i'll leave"];

fn sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars() {
        cur.push(c);
        if matches!(c, '.' | '?' | '!' | '\n') {
            let t = cur.trim().to_string();
            if !t.is_empty() {
                out.push(t);
            }
            cur.clear();
        }
    }
    let t = cur.trim().to_string();
    if !t.is_empty() {
        out.push(t);
    }
    out
}

/// Everything after a reply's quoted history starts is theirs, not yours:
/// "On Mon, 3 Mar 2026, Sam wrote:", Outlook's "-----Original Message-----"
/// or its "From: … Sent: …" block, a phone's "Sent from my …".
fn own_words(excerpt: &str) -> &str {
    let low = excerpt.to_ascii_lowercase();
    let mut end = excerpt.len();
    if let Some(w) = low.find(" wrote:") {
        // Back to the "On …" that opens the attribution, if it's close by.
        let start = low[..w].rfind("on ").filter(|o| w - o < 120).unwrap_or(w);
        end = end.min(start);
    }
    for marker in ["-----original message", "________________", "sent from my"] {
        if let Some(i) = low.find(marker) {
            end = end.min(i);
        }
    }
    if let Some(f) = low.find("from: ") {
        if low[f..].contains("sent: ") {
            end = end.min(f);
        }
    }
    while !excerpt.is_char_boundary(end) {
        end -= 1;
    }
    &excerpt[..end]
}

/// The first request and the first promise in a letter you wrote.
pub fn read_letter(l: &Letter, taught: &Taught) -> (Option<(String, String)>, Option<(String, String)>) {
    if !l.mine {
        return (None, None);
    }
    let mut ask = None;
    let mut promise = None;
    for s in sentences(own_words(&l.excerpt)) {
        let low = s.to_lowercase().replace('\u{2019}', "'");
        if ask.is_none() {
            if let Some(cue) = ASKS.iter().find(|c| low.contains(**c)).filter(|c| taught.trusts(c)) {
                ask = Some((s.clone(), cue.to_string()));
            } else if low.ends_with('?') && low.split_whitespace().count() >= 4 && taught.trusts("?") {
                ask = Some((s.clone(), "?".to_string()));
            }
        }
        if promise.is_none() && !HEDGES.iter().any(|h| low.contains(h)) {
            if let Some(cue) = PROMISES.iter().find(|c| low.contains(**c)).filter(|c| taught.trusts(c)) {
                promise = Some((s.clone(), cue.to_string()));
            }
        }
    }
    (ask, promise)
}

fn short(s: &str) -> String {
    let t: String = s.chars().take(90).collect();
    if s.chars().count() > 90 {
        format!("{t}…")
    } else {
        t
    }
}

/// Working days after `from` (UTC seconds), skipping Saturdays and Sundays.
fn working_days_after(from: u64, days: u64) -> u64 {
    let mut t = from;
    let mut left = days;
    while left > 0 {
        t += 86_400;
        let wd = ((t / 86_400) as i64 + 3).rem_euclid(7);
        if wd < 5 {
            left -= 1;
        }
    }
    t
}

/// Everything still open, from the book: questions of yours nobody has
/// answered in the conversation since, and promises you haven't closed.
/// `offset` is your time zone's offset in seconds at `now`, so "Friday"
/// in a promise is your Friday.
pub fn open(book: &MailBook, taught: &Taught, cfg: &WaitingConfig, now: u64, offset: i64) -> Vec<Waiting> {
    let from = now.saturating_sub(cfg.look_back_days * 86_400);
    let mut out = Vec::new();
    for l in book.letters.iter().filter(|l| l.mine && l.at >= from && !taught.closed.contains(&l.id)) {
        let (ask, promise) = read_letter(l, taught);
        let thread = l.thread();
        let later: Vec<&Letter> = book.conversation(&thread).into_iter().filter(|x| x.at > l.at).collect();
        let with = l.others().into_iter().next().unwrap_or_default();
        if let Some((said, cue)) = ask {
            // Answered once anyone other than you has written in the
            // conversation since.
            if !later.iter().any(|x| !x.mine) {
                out.push(Waiting {
                    side: Side::Owed,
                    letter: l.id.clone(),
                    thread: thread.clone(),
                    with: with.clone(),
                    subject: l.subject.clone(),
                    said: short(&said),
                    cue,
                    since: l.at,
                    due: Some(working_days_after(l.at, cfg.owed_after_days)),
                });
            }
        }
        if let Some((said, cue)) = promise {
            let local_sent = (l.at as i64 + offset).max(0) as u64;
            let due = crate::when::parse(&said, local_sent)
                .filter(|p| p.day_said && p.start > local_sent)
                .map(|p| (p.start as i64 - offset).max(0) as u64);
            out.push(Waiting {
                side: Side::Promised,
                letter: l.id.clone(),
                thread,
                with,
                subject: l.subject.clone(),
                said: short(&said),
                cue,
                since: l.at,
                due,
            });
        }
    }
    out.sort_by_key(|w| (w.due.unwrap_or(u64::MAX), w.since));
    out
}

/// The ones worth saying now: owed replies past their wait, promises due
/// today or overdue (and undated ones over a week old).
pub fn due_now(items: &[Waiting], now: u64) -> Vec<&Waiting> {
    items
        .iter()
        .filter(|w| match (w.side, w.due) {
            (_, Some(d)) => d <= now + 12 * 3600,
            (Side::Promised, None) => now.saturating_sub(w.since) >= 7 * 86_400,
            (Side::Owed, None) => false,
        })
        .collect()
}

fn days_ago(since: u64, now: u64) -> String {
    match now.saturating_sub(since) / 86_400 {
        0 => "today".into(),
        1 => "yesterday".into(),
        n => format!("{n} days ago"),
    }
}

/// The list, numbered so "done 2" and "not a promise 3" can refer to it.
pub fn say(items: &[Waiting], now: u64) -> String {
    if items.is_empty() {
        return "Nothing outstanding in your recent mail: nobody owes you a reply that I can see, and no promise of yours is open.".into();
    }
    let mut out = Vec::new();
    let owed: Vec<(usize, &Waiting)> = items.iter().enumerate().filter(|(_, w)| w.side == Side::Owed).collect();
    let promised: Vec<(usize, &Waiting)> = items.iter().enumerate().filter(|(_, w)| w.side == Side::Promised).collect();
    if !owed.is_empty() {
        out.push("Waiting on:".to_string());
        for (i, w) in owed {
            out.push(format!("  {}. {} -- \"{}\" ({}, asked {})", i + 1, w.with, w.subject, w.said, days_ago(w.since, now)));
        }
    }
    if !promised.is_empty() {
        out.push("You said you would:".to_string());
        for (i, w) in promised {
            let due = match w.due {
                Some(d) if d < now => " -- overdue".to_string(),
                Some(d) => format!(" -- due in {} day{}", (d - now).div_ceil(86_400), if (d - now).div_ceil(86_400) == 1 { "" } else { "s" }),
                None => String::new(),
            };
            out.push(format!("  {}. {}: \"{}\" (to {}, {}){due}", i + 1, w.subject, w.said, w.with, days_ago(w.since, now)));
        }
    }
    out.push("Say \"done 2\" when one is settled, or \"not a promise 3\" if I misread it -- I learn from that.".into());
    out.join("\n")
}
