//! What you ask Atlas to be able to do (5 Oct 2026).
//!
//! Eric: "I need the ability to tell Atlas what I want when I want to add
//! new capabilities to Atlas." Before this, "I want you to be able to ..."
//! reached whatever intent a word in it matched ("... send texts" became a
//! text), or the model, which agreed and kept nothing.
//!
//! Now it is heard first and kept: your words as you said them, when, what
//! Atlas already does that looks like it, and where the request has got to.
//! The list is on the Improvements page, read back when you ask, and it is
//! the queue the work on Atlas starts from -- a coding session, or Atlas's
//! own self-work, reads `data/state/capability_requests.json`.
//!
//! What this does not pretend: hearing a request is not building it. Atlas
//! says plainly which of the two has happened.

use serde::{Deserialize, Serialize};

/// Where the requests are kept (`Store` name).
pub const FILE: &str = "capability_requests";

/// Where one request has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Heard and kept; nothing built yet.
    Asked,
    /// Work on it has started (a branch, a session).
    Started,
    /// It does it now.
    Done,
    /// You took it back.
    Dropped,
}

impl Status {
    pub fn plain(&self) -> &'static str {
        match self {
            Status::Asked => "asked for, not started",
            Status::Started => "being built",
            Status::Done => "done",
            Status::Dropped => "dropped",
        }
    }
}

/// One thing you asked Atlas to be able to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Request {
    pub n: u32,
    /// Your words, the whole sentence.
    pub said: String,
    /// What's being asked for, with the lead ("I want you to be able to")
    /// taken off.
    pub what: String,
    pub at: u64,
    pub status: Status,
    /// Capabilities Atlas already has that share words with it, by id.
    #[serde(default)]
    pub looks_like: Vec<String>,
    /// Anything you added after ("for request 2: it should also ...").
    #[serde(default)]
    pub more: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Requests {
    #[serde(default)]
    pub list: Vec<Request>,
}

impl Requests {
    /// Keep a new one; its number.
    pub fn add(&mut self, said: &str, what: &str, looks_like: Vec<String>, at: u64) -> u32 {
        let n = self.list.iter().map(|r| r.n).max().unwrap_or(0) + 1;
        self.list.push(Request { n, said: said.trim().to_string(), what: what.trim().to_string(), at, status: Status::Asked, looks_like, more: Vec::new() });
        n
    }

    /// The open ones (not done, not dropped), oldest first.
    pub fn open(&self) -> Vec<&Request> {
        self.list.iter().filter(|r| matches!(r.status, Status::Asked | Status::Started)).collect()
    }

    /// The same thing asked before and still open: said again, not kept twice.
    pub fn already(&self, what: &str) -> Option<&Request> {
        let want = meaning_words(what);
        self.open().into_iter().find(|r| {
            let had = meaning_words(&r.what);
            !want.is_empty() && want.iter().filter(|w| had.contains(*w)).count() * 4 >= want.len().max(had.len()) * 3
        })
    }

    pub fn set(&mut self, n: u32, status: Status) -> Option<&Request> {
        let r = self.list.iter_mut().find(|r| r.n == n)?;
        r.status = status;
        Some(r)
    }
}

/// The leads that make a sentence a request for a new ability, with what
/// follows them being the ability. A request about one of your own projects
/// ("add a feature to my app") is not one: that's `improve`.
const LEADS: &[&str] = &[
    "i want you to be able to ",
    "i'd like you to be able to ",
    "i would like you to be able to ",
    "i need you to be able to ",
    "i want atlas to be able to ",
    "i need atlas to be able to ",
    "you should be able to ",
    "i want you to learn how to ",
    "i want you to learn to ",
    "can you learn how to ",
    "can you learn to ",
    "learn how to ",
    "teach yourself how to ",
    "teach yourself to ",
    "give yourself the ability to ",
    "add the ability to ",
    "add a capability to ",
    "add a capability for ",
    "add a capability that ",
    "add a capability: ",
    "add a capability ",
    "new capability: ",
    "new capability ",
    "add a feature to yourself that ",
    "add a feature to yourself: ",
    "add a feature to atlas that ",
    "add a feature to atlas: ",
    "add to atlas: ",
    "add to atlas the ability to ",
    "add to yourself the ability to ",
];

/// Words that open a sentence and change nothing ("ok", "Atlas,").
const FILLER: &[&str] = &["ok ", "okay ", "so ", "alright ", "atlas ", "hey atlas ", "now ", "also ", "and ", "please "];

/// What's asked for, when this is a request for a new ability.
pub fn ability_asked_for(said: &str) -> Option<String> {
    let mut t = said.trim().to_lowercase().replace(['\u{2019}', '\u{2018}'], "'");
    t = t.replace(',', " ");
    t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    t.push(' ');
    loop {
        let before = t.len();
        for f in FILLER {
            if let Some(rest) = t.strip_prefix(f) {
                t = rest.to_string();
            }
        }
        if t.len() == before {
            break;
        }
    }
    for lead in LEADS {
        if let Some(rest) = t.strip_prefix(lead) {
            let what = rest.trim().trim_end_matches(['.', '!', '?']).trim().to_string();
            // "Can you learn to?" with nothing after it isn't a request.
            if what.split_whitespace().count() >= 2 {
                return Some(what);
            }
        }
    }
    None
}

/// "What have I asked you to be able to do?"
pub fn list_asked(said: &str) -> bool {
    let t = said.to_lowercase();
    [
        "my capability requests",
        "capability requests",
        "what have i asked you to be able to",
        "what have i asked you to learn",
        "what have i asked you to add",
        "what capabilities have i asked for",
        "what have i asked for",
        "list my requests",
        "show my requests",
        "my feature requests",
    ]
    .iter()
    .any(|k| t.contains(k))
}

/// "Drop request 2", "forget request 2", "request 2 is done".
pub fn changed(said: &str) -> Option<(u32, Status)> {
    let t = said.to_lowercase();
    let n = |after: &str| -> Option<u32> {
        let i = t.find(after)? + after.len();
        t[i..].split_whitespace().next()?.trim_matches(|c: char| !c.is_ascii_digit()).parse().ok()
    };
    for lead in ["drop request ", "forget request ", "cancel request ", "remove request "] {
        if let Some(k) = n(lead) {
            return Some((k, Status::Dropped));
        }
    }
    if t.contains(" is done") || t.contains(" is finished") {
        if let Some(k) = n("request ") {
            return Some((k, Status::Done));
        }
    }
    None
}

const STOP: &[&str] = &[
    "about", "after", "again", "also", "and", "being", "could", "every", "from", "have", "into", "just", "like", "make", "more", "need", "only",
    "should", "some", "than", "that", "them", "then", "there", "these", "they", "thing", "things", "this", "what", "when", "where", "which",
    "while", "with", "would", "your", "able", "want", "atlas", "yourself",
];

/// The words that carry meaning: four letters or more, not filler.
fn meaning_words(s: &str) -> Vec<String> {
    let mut v: Vec<String> = s
        .to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() >= 4 && !STOP.contains(w))
        .map(|w| w.trim_end_matches('s').to_string())
        .collect();
    v.sort();
    v.dedup();
    v
}

/// What Atlas already does that shares at least two meaningful words with
/// the request, best first, at most three. Words, not meaning: it says "this
/// looks like" and lets you decide.
pub fn looks_like(what: &str, catalogue: &[crate::capability::Capability]) -> Vec<(String, String)> {
    let want = meaning_words(what);
    let mut scored: Vec<(usize, &crate::capability::Capability)> = catalogue
        .iter()
        .map(|c| {
            let has = meaning_words(c.what);
            (want.iter().filter(|w| has.contains(*w)).count(), c)
        })
        .filter(|(n, _)| *n >= 2)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored.into_iter().take(3).map(|(_, c)| (c.id.to_string(), c.what.to_string())).collect()
}

/// The reply to a new request.
pub fn heard_reply(n: u32, what: &str, similar: &[(String, String)], again: Option<u32>) -> String {
    if let Some(k) = again {
        return format!("You asked for that already -- it's request {k}, still open. I've kept your new wording with it.");
    }
    let mut out = format!("Kept as request {n}: \"{what}\". ");
    if let Some((id, does)) = similar.first() {
        out.push_str(&format!(
            "Close to something I already do ({id}: {}). If that's what you meant, say \"drop request {n}\" and tell me what it gets wrong instead. ",
            does.split(" -- ").next().unwrap_or(does)
        ));
    }
    out.push_str("It's on the Improvements page under \"Asked for\". Nothing's built yet -- that's the next step, and I'll say when it starts.");
    out
}

/// The list, read back.
pub fn read_back(r: &Requests) -> String {
    let open = r.open();
    if open.is_empty() {
        return "You haven't asked me for anything new that's still open. Say \"I want you to be able to ...\" and I'll keep it.".into();
    }
    let lines: Vec<String> = open.iter().map(|q| format!("{}. {} ({})", q.n, q.what, q.status.plain())).collect();
    format!("{} open:\n{}", open.len(), lines.join("\n"))
}

/// The Improvements page's "Asked for" section.
pub fn section(r: &Requests) -> String {
    let esc = crate::hub::esc;
    let mut rows = String::new();
    for q in r.list.iter().rev().filter(|q| q.status != Status::Dropped).take(30) {
        let more = if q.more.is_empty() { String::new() } else { format!("<br><small>{}</small>", esc(&q.more.join(" / "))) };
        rows.push_str(&format!("<li><b>{}.</b> {} -- <i>{}</i>{more}</li>", q.n, esc(&q.what), q.status.plain()));
    }
    if rows.is_empty() {
        rows.push_str("<li>Nothing yet. Say \"I want you to be able to ...\" and it's kept here.</li>");
    }
    format!("<section><h2>Asked for</h2><p>What you've asked me to be able to do, in your words.</p><ul>{rows}</ul></section>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_is_heard_with_the_lead_taken_off() {
        assert_eq!(ability_asked_for("I want you to be able to send texts from my phone").as_deref(), Some("send texts from my phone"));
        assert_eq!(ability_asked_for("Ok, Atlas, I'd like you to be able to edit my videos.").as_deref(), Some("edit my videos"));
        assert_eq!(ability_asked_for("Add a capability that tracks my sleep").as_deref(), Some("tracks my sleep"));
        assert_eq!(ability_asked_for("can you learn how to read my calendar?").as_deref(), Some("read my calendar"));
        // Not requests for a new ability.
        assert_eq!(ability_asked_for("send a text to mom"), None);
        assert_eq!(ability_asked_for("I want to go to bed"), None);
        assert_eq!(ability_asked_for("add a feature to my app that exports csv"), None);
        assert_eq!(ability_asked_for("can you learn to?"), None);
    }

    #[test]
    fn the_list_numbers_keeps_and_closes_them() {
        let mut r = Requests::default();
        let a = r.add("I want you to be able to edit videos", "edit videos", vec![], 1);
        let b = r.add("I want you to be able to read pdfs", "read pdfs", vec![], 2);
        assert_eq!((a, b), (1, 2));
        assert!(r.already("edit videos").is_some());
        assert!(r.already("book flights").is_none());
        assert_eq!(changed("drop request 2"), Some((2, Status::Dropped)));
        assert_eq!(changed("request 1 is done"), Some((1, Status::Done)));
        r.set(2, Status::Dropped);
        assert_eq!(r.open().len(), 1);
        assert!(read_back(&r).contains("1. edit videos"));
        assert!(section(&r).contains("edit videos") && !section(&r).contains("read pdfs"));
        assert!(list_asked("what have I asked you to be able to do?"));
    }

    #[test]
    fn what_atlas_already_does_is_named_by_shared_words() {
        let all = crate::capability::all();
        let hits = looks_like("check the weather forecast for tomorrow in another town", &all);
        assert!(hits.iter().any(|(id, _)| id == "weather"), "{hits:?}");
        assert!(looks_like("juggle flaming torches", &all).is_empty());
    }
}
