//! Coming back.
//!
//! You step away and things happen: a job finishes, something fails, a post
//! misses its window. When you sit back down, the wrong move is to say all of
//! it, and the other wrong move is to say none of it and let you find out.
//!
//! What you get depends on how long you were gone. Five minutes is not an
//! absence and deserves silence. Four hours is, and deserves the short
//! version. Overnight deserves to know what needs deciding before you start.

use serde::{Deserialize, Serialize};

/// How Atlas addresses you. Yours to choose, including "don't".
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Address {
    /// Nothing at all — just get on with it.
    None,
    /// "Eric".
    Name(String),
    /// "sir", "boss", whatever you set.
    Title(String),
}

impl Default for Address {
    fn default() -> Self {
        // Nothing, until you say otherwise. A system that calls you "sir"
        // uninvited is doing a bit.
        Address::None
    }
}

impl Address {
    pub fn load(store: &crate::store::Store) -> Address {
        store.load("address")
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save("address", self)
    }

    /// The greeting, or nothing.
    pub fn greet(&self, hour: u32) -> String {
        let time = match hour {
            5..=11 => "Morning",
            12..=17 => "Afternoon",
            18..=22 => "Evening",
            _ => "Hello",
        };
        match self {
            Address::None => String::new(),
            Address::Name(n) => format!("{time}, {n}. "),
            Address::Title(t) => format!("{time}, {t}. "),
        }
    }
}

/// How long you were gone, and what that earns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gone {
    /// Not really away. Say nothing.
    Moment,
    /// A break. Only what's urgent.
    ShortWhile,
    /// Half a day. The short version.
    HalfDay,
    /// Overnight or longer. What needs deciding, first.
    Overnight,
}

pub fn how_long(secs: u64) -> Gone {
    match secs {
        0..=900 => Gone::Moment,
        901..=14_400 => Gone::ShortWhile,
        14_401..=39_600 => Gone::HalfDay,
        _ => Gone::Overnight,
    }
}

/// Something that happened while you were out.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Happened {
    pub what: String,
    /// Needs a decision from you.
    pub needs_you: bool,
    /// Went wrong.
    pub failed: bool,
    pub at: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ReturnConfig {
    pub enabled: bool,
    pub address: String,
    /// "name" or "title".
    pub address_as: String,
    /// Offer the brief rather than reciting it.
    pub offer_first: bool,
    /// Most things named out loud before it summarises the rest.
    pub name_at_most: usize,
}

impl Default for ReturnConfig {
    fn default() -> Self {
        ReturnConfig {
            enabled: true,
            address: String::new(),
            address_as: "name".into(),
            // Ask before reciting. You may have come back to do something
            // specific and a briefing is in the way.
            offer_first: true,
            name_at_most: 2,
        }
    }
}

impl ReturnConfig {
    pub fn how_to_address(&self) -> Address {
        if self.address.trim().is_empty() {
            return Address::None;
        }
        if self.address_as == "title" {
            Address::Title(self.address.clone())
        } else {
            Address::Name(self.address.clone())
        }
    }
}

/// What Atlas says when you sit back down.
#[derive(Debug, Clone, PartialEq)]
pub enum Welcome {
    /// Nothing worth saying.
    Nothing,
    /// One line, said straight away — something is wrong or waiting.
    Straight(String),
    /// An offer. "I've got updates when you're ready."
    Offer(String),
}

pub fn welcome(
    gone_secs: u64,
    happened: &[Happened],
    hour: u32,
    cfg: &ReturnConfig,
) -> Welcome {
    if !cfg.enabled {
        return Welcome::Nothing;
    }
    let gone = how_long(gone_secs);
    if gone == Gone::Moment {
        // Five minutes is not an absence.
        return Welcome::Nothing;
    }

    let urgent: Vec<&Happened> = happened.iter().filter(|h| h.needs_you || h.failed).collect();
    let rest = happened.len() - urgent.len();
    let greeting = cfg.how_to_address().greet(hour);

    // Something is wrong or waiting: say it, don't offer it. What's named
    // first is what matters most (`next_up`): a decision waiting on you
    // before something that went wrong, and newer before older.
    if !urgent.is_empty() {
        let picked = crate::next_up::top(
            &urgent,
            |_| true,
            |h| (if h.needs_you { 2.0 } else { 0.0 }) + (if h.failed { 1.0 } else { 0.0 }) + h.at as f64 / 1e12,
            cfg.name_at_most,
        );
        let named: Vec<&str> = picked.chosen.iter().map(|h| h.what.as_str()).collect();
        let more = picked.passed_over;
        let mut s = format!("{greeting}{}", named.join(", and "));
        if more > 0 {
            s.push_str(&format!(", and {more} more"));
        }
        s.push('.');
        if rest > 0 {
            s.push_str(&format!(" {rest} other thing{} when you want it.",
                if rest == 1 { "" } else { "s" }));
        }
        return Welcome::Straight(s);
    }

    if happened.is_empty() {
        // A quiet absence is worth one line after a long one, and silence
        // after a short one.
        return match gone {
            Gone::Overnight => Welcome::Straight(format!("{greeting}Nothing needed you.")),
            _ => Welcome::Nothing,
        };
    }

    // Nothing urgent. Offer rather than recite — you may have come back to do
    // something specific, and a briefing is in the way.
    let subjects = subjects_of(happened);
    let s = if cfg.offer_first {
        match subjects.len() {
            1 => format!("{greeting}I've got an update on {} when you're ready.", subjects[0]),
            _ => format!(
                "{greeting}Updates on {} when you want them.",
                list(&subjects, cfg.name_at_most)
            ),
        }
    } else {
        format!("{greeting}{}", happened.iter().map(|h| h.what.clone()).collect::<Vec<_>>().join(". "))
    };
    Welcome::Offer(s)
}

/// The subjects, so the offer names things rather than counting them.
fn subjects_of(happened: &[Happened]) -> Vec<String> {
    let mut s: Vec<String> = happened.iter().map(|h| subject_of(&h.what)).collect();
    s.dedup();
    s
}

/// What a thing is about, short enough to name: the words before the first
/// break in the sentence, at most four, not ending on "your" or "the".
/// Cutting at three words regardless read "Atlas — your machine" as "an
/// update on Atlas — your when you're ready" (found in round 9).
fn subject_of(what: &str) -> String {
    let mut parts = what.split(['—', '–', ':', ';', ',', '.']).map(str::trim).filter(|p| !p.is_empty());
    let mut head = parts.next().unwrap_or("").to_string();
    // A note titled "Atlas — …" is about what follows the dash, not Atlas.
    if head.eq_ignore_ascii_case("atlas") {
        if let Some(next) = parts.next() {
            head = next.to_string();
        }
    }
    let mut words: Vec<&str> = head.split_whitespace().take(4).collect();
    while words.len() > 1 && matches!(words.last().map(|w| w.to_lowercase()).as_deref(), Some("your" | "the" | "a" | "an" | "to" | "for" | "of" | "on" | "and" | "in" | "at")) {
        words.pop();
    }
    words.join(" ")
}

fn list(items: &[String], at_most: usize) -> String {
    let named: Vec<&str> = items.iter().take(at_most).map(|s| s.as_str()).collect();
    let more = items.len().saturating_sub(named.len());
    if more == 0 {
        named.join(" and ")
    } else {
        format!("{}, and {more} more", named.join(", "))
    }
}

/// "Call me Eric." / "call me sir." / "stop calling me that."
///
/// Changing it should cost one sentence, not a trip to settings — it's the
/// sort of thing you notice is wrong exactly once, in the moment.
pub fn address_change(said: &str) -> Option<Address> {
    let t = said.trim().to_lowercase();

    for stop in ["stop calling me", "don't call me", "dont call me"] {
        if t.contains(stop) {
            return Some(Address::None);
        }
    }
    if t.contains("just talk") || t.contains("no name") || t.contains("drop the") {
        return Some(Address::None);
    }

    for lead in ["call me ", "address me as ", "you can call me ", "refer to me as "] {
        if let Some(i) = t.find(lead) {
            let rest = t[i + lead.len()..]
                .trim()
                .trim_end_matches(['.', '!', ','])
                .trim();
            if rest.is_empty() {
                return None;
            }
            // "call me back later" is not a name change. Anything followed by
            // a word that only makes sense as a verb phrase isn't one either.
            const NOT_A_NAME: &[&str] = &[
                "back", "later", "when", "if", "tomorrow", "now", "then",
                "about", "on", "at", "in", "after", "before", "once",
            ];
            let first = rest.split_whitespace().next().unwrap_or(rest);
            if NOT_A_NAME.contains(&first) {
                return None;
            }
            // Titles are the handful of words people mean as titles; anything
            // else is a name, including nicknames.
            let titles = ["sir", "ma'am", "maam", "madam", "boss", "chief", "captain", "doctor"];
            let word = rest.split_whitespace().next().unwrap_or(rest);
            return Some(if titles.contains(&word) {
                Address::Title(word.to_string())
            } else {
                // Keep the capital: it's a name.
                let mut c = word.chars();
                let cased = match c.next() {
                    Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
                    None => word.to_string(),
                };
                Address::Name(cased)
            });
        }
    }
    None
}

/// What Atlas says once you've changed it. Short, and uses it immediately so
/// you can hear whether you like it.
pub fn confirm_address(a: &Address) -> String {
    match a {
        Address::None => "Right — no name.".into(),
        Address::Name(n) => format!("Got it, {n}."),
        Address::Title(t) => format!("Very good, {t}."),
    }
}

/// You said yes to the offer.
pub fn full_brief(happened: &[Happened]) -> String {
    if happened.is_empty() {
        return "Nothing happened.".into();
    }
    let mut out = String::new();
    let (needs, rest): (Vec<&Happened>, Vec<&Happened>) =
        happened.iter().partition(|h| h.needs_you || h.failed);

    for h in needs {
        out.push_str(&format!("· {}\n", h.what));
    }
    if !rest.is_empty() {
        if !out.is_empty() {
            out.push('\n');
        }
        for h in rest {
            out.push_str(&format!("  {}\n", h.what));
        }
    }
    out
}

#[cfg(test)]
mod subject_tests {
    use super::subject_of;

    #[test]
    fn a_subject_is_named_whole_or_not_at_all() {
        assert_eq!(subject_of("Atlas — your machine"), "your machine");
        assert_eq!(subject_of("Backup finished: 3 folders"), "Backup finished");
        assert_eq!(subject_of("Posted the weekly update to the blog"), "Posted the weekly update");
        assert_eq!(subject_of("Scheduled the call for"), "Scheduled the call");
    }
}
