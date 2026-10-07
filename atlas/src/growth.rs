//! "Can you work that so you have this capability?" -- asking Atlas for a
//! new ability (Eric, 1 Oct 2026).
//!
//! **Why this exists.** Asked to give itself continuous camera watching, the
//! model answered from its idea of itself: "I can't add capabilities to
//! myself -- that's a system architecture thing, not something I can learn
//! my way around." Then "do some research on things that would allow you to
//! advance your own capabilities, then present them to me for approval" was
//! answered "I can't tell what you mean". Both were wrong. Atlas can't
//! *quietly* give itself a new ability -- Eric's standing rule is that bug
//! fixes Atlas may make itself, and new features wait for his yes -- but it
//! can always write the request down, say what it would take, and put it
//! where he approves things.
//!
//! So a request for a new ability is never refused and never lost:
//!
//! 1. **Written down** in `wanted_abilities`, in the words it was asked in,
//!    with when, and its state (`Asked`, then `Approved` or `Declined`).
//! 2. **Said back honestly:** what was asked, that it waits for his yes, and
//!    how to give it.
//! 3. **On his yes** ("build that ability", "yes, add it") it's marked
//!    approved -- the list the build sessions and self-work read from. Atlas
//!    never marks one approved by itself.

use serde::{Deserialize, Serialize};

/// Where a request is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Asked,
    Approved,
    Declined,
}

/// One ability asked for.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wanted {
    pub what: String,
    pub asked_at: u64,
    pub state: State,
}

/// Every ability asked for, oldest first.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct WantedAbilities {
    pub items: Vec<Wanted>,
}

/// The store key.
pub const STORE: &str = "wanted_abilities";

impl WantedAbilities {
    /// Add a request; the same request asked again is kept once (its time
    /// updated), not twice.
    pub fn ask(&mut self, what: &str, t: u64) -> &Wanted {
        let what = what.trim().trim_end_matches(['.', '?', '!']).to_string();
        if let Some(i) = self.items.iter().position(|w| w.what.eq_ignore_ascii_case(&what) && w.state == State::Asked) {
            self.items[i].asked_at = t;
            return &self.items[i];
        }
        let i = self.items.len();
        self.items.push(Wanted { what, asked_at: t, state: State::Asked });
        &self.items[i]
    }

    /// The most recent request still waiting on a yes.
    pub fn waiting(&self) -> Option<&Wanted> {
        self.items.iter().rev().find(|w| w.state == State::Asked)
    }

    /// Mark the latest waiting request; `None` when nothing is waiting.
    pub fn decide_latest(&mut self, state: State) -> Option<String> {
        let i = self.items.iter().rposition(|w| w.state == State::Asked)?;
        self.items[i].state = state;
        Some(self.items[i].what.clone())
    }

    /// Said for "what abilities have I asked for?".
    pub fn spoken(&self) -> String {
        if self.items.is_empty() {
            return "You haven't asked me for any new abilities yet.".into();
        }
        let line = |w: &Wanted| {
            let s = match w.state {
                State::Asked => "waiting on your yes",
                State::Approved => "approved",
                State::Declined => "declined",
            };
            format!("{} ({s})", w.what)
        };
        let all: Vec<String> = self.items.iter().rev().take(5).map(line).collect();
        format!("Abilities you've asked for: {}.", all.join("; "))
    }
}

fn plain(s: &str) -> String {
    let s = s.to_lowercase().replace('\u{2019}', "'");
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Is this a request for Atlas to gain a new ability? The ability, in the
/// words used (`""` when it points back at what was just discussed --
/// "work that so you have this capability").
pub fn asks_for_an_ability(said: &str) -> Option<String> {
    let t = plain(said);
    if t.is_empty() || t.starts_with("don't") || t.starts_with("do not") || t.starts_with("what ") || t.starts_with("which ") {
        return None;
    }
    // "Give yourself the ability to …", "add the capability to …".
    const LEADS: &[&str] = &[
        // The way it's most often said (5 Oct 2026: "I need the ability to
        // tell Atlas what I want" -- and "I want you to be able to ..."
        // reached nothing here).
        "i want you to be able to ",
        "i'd like you to be able to ",
        "i would like you to be able to ",
        "i need you to be able to ",
        "i want atlas to be able to ",
        "i need atlas to be able to ",
        "you should be able to ",
        "can you learn to ",
        "can you learn how to ",
        "add a capability that ",
        "add a capability for ",
        "add a new capability that ",
        "add a new capability to ",
        "add a new ability to ",
        "new capability ",
        "give yourself the ability to ",
        "give yourself the capability to ",
        "give yourself the ability ",
        "give yourself a way to ",
        "add the ability to ",
        "add the capability to ",
        "add an ability to ",
        "add a capability to ",
        "add a feature to ",
        "add the feature to ",
        "add a feature that ",
        "build yourself the ability to ",
        "build yourself a way to ",
        "learn how to ",
        "teach yourself to ",
        "teach yourself how to ",
    ];
    for lead in LEADS {
        if let Some(i) = t.find(lead) {
            let boundary = i == 0 || t.as_bytes()[i - 1] == b' ';
            if boundary {
                let rest = t[i + lead.len()..].trim();
                // "learn how to" is everyday English; only about Atlas itself.
                if lead.starts_with("learn") && !t.contains("yourself") && !t.contains("your own") {
                    continue;
                }
                // "Can you learn to ..." with nothing much after it.
                if lead.starts_with("can you") && rest_words(&t[i + lead.len()..]) < 2 {
                    continue;
                }
                // "Add a feature to my app" is work on your code, not a new
                // ability for Atlas (2 Oct 2026): your app, project or a
                // folder named means the project, and `improve` takes it.
                let yours = [" my app", " my project", " my code", " my script", " my program", " my site", " my website", " my repo", " my tool", " the app", " project"];
                if lead.contains("feature") && (yours.iter().any(|y| format!(" {t}").contains(y)) || crate::build_it::folder_named(said).is_some()) {
                    continue;
                }
                if !rest.is_empty() {
                    return Some(rest.to_string());
                }
            }
        }
    }
    // Pointing back: "work that so you have this capability", "make it so
    // you can do that", "add your own capabilities", "advance your own
    // capabilities".
    const BACK: &[&str] = &[
        "so you have this capability",
        "so that you have this capability",
        "so you have that capability",
        "so you have the capability",
        "so you have this ability",
        "so you can do that",
        "so you can do this",
        "add your own capabilities",
        "add your own abilities",
        "add your own features",
        "advance your own capabilities",
        "expand your own capabilities",
        "give yourself that ability",
        "give yourself this ability",
        "give yourself that capability",
        "give yourself this capability",
        "build that capability",
        "build this capability",
        "add that capability",
        "add this capability",
    ];
    if BACK.iter().any(|b| t.contains(b)) {
        return Some(String::new());
    }
    None
}

fn rest_words(s: &str) -> usize {
    s.split_whitespace().count()
}

const STOP: &[&str] = &[
    "about", "after", "again", "also", "being", "could", "every", "from", "have", "into", "just", "like", "make", "more", "need", "only",
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
/// a request, best first, at most three (5 Oct 2026). Words, not meaning:
/// it says "this looks like" and leaves the deciding to you.
pub fn already_close(what: &str, catalogue: &[crate::capability::Capability]) -> Vec<(String, String)> {
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

/// The Improvements page's "Asked for" section: every ability asked for,
/// newest first, and where it stands.
pub fn section(w: &WantedAbilities) -> String {
    let esc = crate::hub::esc;
    let mut rows = String::new();
    for x in w.items.iter().rev().take(30) {
        let s = match x.state {
            State::Asked => "waiting on your yes",
            State::Approved => "approved -- on the build list",
            State::Declined => "declined",
        };
        rows.push_str(&format!("<li>{} -- <i>{s}</i></li>", esc(&x.what)));
    }
    if rows.is_empty() {
        rows.push_str("<li>Nothing yet. Say \"I want you to be able to read my texts out loud\" -- or whatever you'd like -- and it's kept here.</li>");
    }
    format!("<section><h2>Asked for</h2><p>New abilities you've asked me for, in your words. Say \"approve that ability\" for the newest.</p><ul>{rows}</ul></section>")
}

/// "Yes, build that ability", "approve that ability", "no, not that ability".
pub fn answer(said: &str) -> Option<State> {
    let t = plain(said);
    let about = ["that ability", "this ability", "that capability", "this capability", "the ability", "the new ability"]
        .iter()
        .any(|a| t.contains(a));
    if !about {
        return None;
    }
    if ["don't", "do not", "decline", "no ", "not ", "never", "skip", "drop"].iter().any(|n| t.contains(n) || t == n.trim()) {
        return Some(State::Declined);
    }
    if ["build", "approve", "yes", "go ahead", "add it", "do it", "make it"].iter().any(|y| t.contains(y)) {
        return Some(State::Approved);
    }
    None
}

/// "Set that ability up": the bookkeeping for the newest approved one, in
/// Atlas's source (`scaffold`).
pub fn asks_to_set_up(said: &str) -> bool {
    let t = plain(said);
    ["set that ability up", "set up that ability", "set that up in your source", "scaffold that ability", "start building that ability", "set up the new ability"]
        .iter()
        .any(|p| t.contains(p))
}

/// The newest approved ability.
pub fn latest_approved(w: &WantedAbilities) -> Option<&Wanted> {
    w.items.iter().rev().find(|x| x.state == State::Approved)
}

/// Is this "what abilities have I asked for?"
pub fn asks_for_the_list(said: &str) -> bool {
    let t = plain(said);
    ["abilities have i asked", "abilities i asked", "abilities i've asked", "ability requests", "capabilities have i asked", "capabilities i asked", "new abilities waiting"]
        .iter()
        .any(|p| t.contains(p))
}

/// What Atlas says when it writes a request down.
fn noted(what: &str) -> String {
    format!(
        "I can't switch on a new ability by myself -- new abilities wait for your yes -- but I've written it \
         down as a request: \"{what}\". Say \"approve that ability\" and it goes on the build list; \
         \"what abilities have I asked for\" lists them."
    )
}

/// `noted`, with what Atlas already does that looks like it (5 Oct 2026).
pub fn noted_beside(what: &str, close: &[(String, String)]) -> String {
    match close.first() {
        None => noted(what),
        Some((id, does)) => format!(
            "{} One thing: I already {} ({id}). If that's what you meant, say \"no, not that ability\" and tell me what it gets wrong instead.",
            noted(what),
            does.split(" -- ").next().unwrap_or(does)
        ),
    }
}

/// The truth when the model says it can't gain abilities (`backed`).
pub const CAN_GROW: &str = "Actually, I can take a request for a new ability: say \"give yourself the ability to read my texts out loud\" (or whatever you'd like) and I'll write it down for your yes, then it goes on the build list. Bug fixes I make myself; new abilities wait for you.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erics_sentences_ask_for_an_ability() {
        assert_eq!(asks_for_an_ability("Ok so can you work that so you have this capability."), Some(String::new()));
        assert_eq!(
            asks_for_an_ability("Oh great youre learning but you should also be learning how you can add your own capabilities."),
            Some(String::new())
        );
        assert_eq!(
            asks_for_an_ability("Give yourself the ability to watch me for five minutes"),
            Some("watch me for five minutes".into())
        );
        assert_eq!(asks_for_an_ability("add the capability to read my texts aloud").as_deref(), Some("read my texts aloud"));
    }

    #[test]
    fn the_way_it_is_most_often_said_is_heard() {
        assert_eq!(asks_for_an_ability("I want you to be able to send texts from my phone").as_deref(), Some("send texts from my phone"));
        assert_eq!(asks_for_an_ability("Ok Atlas, I'd like you to be able to edit my videos.").as_deref(), Some("edit my videos"));
        assert_eq!(asks_for_an_ability("add a capability that tracks my sleep").as_deref(), Some("tracks my sleep"));
        assert_eq!(asks_for_an_ability("can you learn how to read my calendar?").as_deref(), Some("read my calendar"));
        assert_eq!(asks_for_an_ability("can you learn to?"), None);
        assert_eq!(asks_for_an_ability("I want to go to bed"), None);
    }

    #[test]
    fn what_atlas_already_does_is_named_and_the_list_is_on_the_page() {
        let all = crate::capability::all();
        let close = already_close("check the weather forecast for tomorrow in another town", &all);
        assert!(close.iter().any(|(id, _)| id == "weather"), "{close:?}");
        assert!(already_close("juggle flaming torches", &all).is_empty());
        assert!(noted_beside("say the weather", &close).contains("(weather)"));
        let mut w = WantedAbilities::default();
        w.ask("juggle flaming torches", 1);
        assert!(section(&w).contains("juggle flaming torches") && section(&w).contains("waiting on your yes"));
    }

    #[test]
    fn ordinary_sentences_are_not() {
        for s in ["learn how to cook rice", "what can you do", "add milk to my list", "don't add that capability", "open chrome"] {
            assert_eq!(asks_for_an_ability(s), None, "{s}");
        }
    }

    #[test]
    fn a_request_is_kept_once_and_waits_for_a_yes() {
        let mut w = WantedAbilities::default();
        w.ask("watch me for five minutes", 1);
        w.ask("Watch me for five minutes.", 2);
        assert_eq!(w.items.len(), 1);
        assert_eq!(w.items[0].asked_at, 2);
        assert_eq!(w.waiting().map(|x| x.what.as_str()), Some("watch me for five minutes"));
        assert_eq!(answer("yes, build that ability"), Some(State::Approved));
        assert_eq!(answer("no, not that ability"), Some(State::Declined));
        assert_eq!(answer("build the house"), None);
        assert_eq!(w.decide_latest(State::Approved).as_deref(), Some("watch me for five minutes"));
        assert!(w.waiting().is_none());
        assert!(w.spoken().contains("approved"));
    }
}
