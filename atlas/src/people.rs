//! The people you deal with, and what you'd want to remember about them:
//! a small personal CRM, kept on this machine.
//!
//! "Remember Sam's daughter is called Leo." "Keep in touch with Priya every
//! month." "Who haven't I talked to in a while?" "What do I know about Sam?"
//!
//! **Sources:** Monica (AGPL; read for its ideas only) for what a personal
//! CRM holds -- notes, how you met, a keep-in-touch cadence, birthdays -- and
//! for the lesson that the entry form is where these tools die. So nothing
//! here asks you to fill anything in: a person comes into being the first
//! time you name them, and "last talked" comes from the mail cache
//! (`mailbook`) rather than from you logging calls.
//!
//! **Soundproofing.**
//! - A note that holds a secret-looking string (a key, a card number) is
//!   refused, not kept -- the vault is for that.
//! - Health words (`nudge::NEVER_NUDGES_ABOUT`) can be kept as a note if you
//!   say them, but never come back unasked: "who's due" and the brief read
//!   only names and dates.
//! - Bounded: 2,000 people, 100 notes each, 500 characters a note.
//! - A name that could be two people ("Sam" when you know Sam Lee and Sam
//!   Ortiz) is asked about, never guessed.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const MAX_PEOPLE: usize = 2000;
pub const MAX_NOTES: usize = 100;
pub const MAX_NOTE_CHARS: usize = 500;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Contact {
    /// As you first said it: "Sam Lee".
    pub name: String,
    #[serde(default)]
    pub emails: Vec<String>,
    /// (when, what), oldest first.
    #[serde(default)]
    pub notes: Vec<(u64, String)>,
    /// Keep in touch every this many days.
    #[serde(default)]
    pub every_days: Option<u32>,
    /// Last contact you told me about ("I called Sam"), UTC seconds. Mail
    /// is read from the cache at the time of asking and not copied here.
    #[serde(default)]
    pub last_said: Option<u64>,
    /// (month, day).
    #[serde(default)]
    pub birthday: Option<(u32, u32)>,
}

impl Contact {
    /// The latest contact from what you said and the mail cache.
    pub fn last_contact(&self, book: &crate::mailbook::MailBook) -> Option<u64> {
        let mut mail = self.emails.iter().filter_map(|e| book.last_contact(e).map(|x| x.0)).max();
        // No address known: a full name ("Sam Lee", not "Sam") matched on
        // the sender's display name is specific enough to count.
        if self.emails.is_empty() && self.name.split_whitespace().count() >= 2 {
            mail = book.with(&self.name, 1).first().map(|l| l.at);
        }
        mail.into_iter().chain(self.last_said).max()
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct People {
    /// Lower-case full name to the person.
    pub by_key: BTreeMap<String, Contact>,
}

/// What a name found.
#[derive(Debug, Clone, PartialEq)]
pub enum Found<'a> {
    One(&'a str),
    /// More than one person answers to it; the full names.
    Several(Vec<String>),
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    Empty,
    Secret(Vec<&'static str>),
    Full,
    /// "Sam" could be either of these.
    Which(Vec<String>),
}

fn key(name: &str) -> String {
    name.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

impl People {
    /// Who a name means: the full name exactly, an email, or a first name
    /// only when it's unique.
    pub fn find(&self, name: &str) -> Found<'_> {
        let k = key(name);
        if k.is_empty() {
            return Found::None;
        }
        if let Some((kk, _)) = self.by_key.get_key_value(&k) {
            return Found::One(kk);
        }
        if k.contains('@') {
            if let Some((kk, _)) = self.by_key.iter().find(|(_, c)| c.emails.iter().any(|e| e.eq_ignore_ascii_case(&k))) {
                return Found::One(kk);
            }
            return Found::None;
        }
        let first: Vec<&String> = self.by_key.keys().filter(|kk| kk.split(' ').next() == Some(k.as_str())).collect();
        match first.as_slice() {
            [one] => Found::One(one),
            [] => Found::None,
            many => Found::Several(many.iter().map(|kk| self.by_key[*kk].name.clone()).collect()),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Contact> {
        match self.find(name) {
            Found::One(k) => self.by_key.get(k),
            _ => None,
        }
    }

    /// The person for this name, made on first mention -- but never a new
    /// "Sam" when "Sam" already means one of two others.
    fn entry(&mut self, name: &str) -> Result<&mut Contact, Refused> {
        let k = match self.find(name) {
            Found::One(k) => k.to_string(),
            Found::Several(names) => return Err(Refused::Which(names)),
            Found::None => {
                let k = key(name);
                if k.is_empty() {
                    return Err(Refused::Empty);
                }
                if self.by_key.len() >= MAX_PEOPLE {
                    return Err(Refused::Full);
                }
                let tidy = name.split_whitespace().collect::<Vec<_>>().join(" ");
                let is_email = tidy.contains('@');
                self.by_key.insert(
                    k.clone(),
                    Contact { name: tidy.clone(), emails: if is_email { vec![tidy.to_lowercase()] } else { vec![] }, ..Default::default() },
                );
                k
            }
        };
        Ok(self.by_key.get_mut(&k).expect("just found or made"))
    }

    pub fn note(&mut self, name: &str, text: &str, now: u64) -> Result<(), Refused> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Refused::Empty);
        }
        let secrets = crate::redact::secrets_in(text);
        if !secrets.is_empty() {
            return Err(Refused::Secret(secrets));
        }
        let c = self.entry(name)?;
        let t: String = text.chars().take(MAX_NOTE_CHARS).collect();
        if !c.notes.iter().any(|(_, n)| n.eq_ignore_ascii_case(&t)) {
            c.notes.push((now, t));
            if c.notes.len() > MAX_NOTES {
                c.notes.remove(0);
            }
        }
        Ok(())
    }

    pub fn every(&mut self, name: &str, days: Option<u32>) -> Result<(), Refused> {
        self.entry(name)?.every_days = days.map(|d| d.clamp(1, 3650));
        Ok(())
    }

    pub fn talked(&mut self, name: &str, now: u64) -> Result<(), Refused> {
        let c = self.entry(name)?;
        c.last_said = Some(c.last_said.map_or(now, |l| l.max(now)));
        Ok(())
    }

    pub fn email(&mut self, name: &str, address: &str) -> Result<(), Refused> {
        let a = address.trim().to_lowercase();
        if !a.contains('@') {
            return Err(Refused::Empty);
        }
        let c = self.entry(name)?;
        if !c.emails.contains(&a) {
            c.emails.push(a);
        }
        Ok(())
    }

    pub fn birthday(&mut self, name: &str, month: u32, day: u32) -> Result<(), Refused> {
        if !(1..=12).contains(&month) || day == 0 || day > crate::civil::days_in_month(2024, month) {
            return Err(Refused::Empty);
        }
        self.entry(name)?.birthday = Some((month, day));
        Ok(())
    }

    /// Forget one person entirely.
    pub fn forget(&mut self, name: &str) -> bool {
        match self.find(name) {
            Found::One(k) => {
                let k = k.to_string();
                self.by_key.remove(&k).is_some()
            }
            _ => false,
        }
    }

    /// People past their keep-in-touch cadence, most overdue first:
    /// (name, days since, cadence). Someone never contacted counts from
    /// when the cadence was set -- which isn't kept -- so they're listed as
    /// "no contact on record" with `None`.
    pub fn due(&self, book: &crate::mailbook::MailBook, now: u64) -> Vec<(String, Option<u64>, u32)> {
        let mut out: Vec<(String, Option<u64>, u32)> = self
            .by_key
            .values()
            .filter_map(|c| {
                let every = c.every_days?;
                match c.last_contact(book) {
                    Some(t) => {
                        let days = now.saturating_sub(t) / 86_400;
                        (days >= every as u64).then(|| (c.name.clone(), Some(days), every))
                    }
                    None => Some((c.name.clone(), None, every)),
                }
            })
            .collect();
        // Most overdue relative to their cadence first; never-contacted last.
        out.sort_by(|a, b| {
            let r = |x: &(String, Option<u64>, u32)| x.1.map(|d| d as f64 / x.2 as f64).unwrap_or(-1.0);
            r(b).partial_cmp(&r(a)).unwrap_or(std::cmp::Ordering::Equal)
        });
        out
    }

    /// Birthdays in the next `days` days from `local_now` (local seconds):
    /// (name, days away).
    pub fn birthdays(&self, local_now: u64, days: u32) -> Vec<(String, u32)> {
        let today = crate::civil::Civil::from_local(local_now as i64);
        let t0 = today.days();
        let mut out = Vec::new();
        for c in self.by_key.values() {
            let Some((m, d)) = c.birthday else { continue };
            for y in [today.year, today.year + 1] {
                // 29 Feb falls on 28 Feb in a common year.
                let dd = d.min(crate::civil::days_in_month(y, m));
                let away = crate::civil::days_from_civil(y, m, dd) - t0;
                if (0..=days as i64).contains(&away) {
                    out.push((c.name.clone(), away as u32));
                    break;
                }
            }
        }
        out.sort_by_key(|x| x.1);
        out
    }

    /// "What do I know about Sam?" -- the notes, newest first, and when you
    /// last spoke.
    pub fn what_i_know(&self, name: &str, book: &crate::mailbook::MailBook, now: u64) -> String {
        let c = match self.find(name) {
            Found::One(k) => &self.by_key[k],
            Found::Several(names) => return format!("Which one -- {}?", names.join(" or ")),
            Found::None => return format!("I've nothing on {name} yet. \"Remember {name} …\" starts it."),
        };
        let mut out = vec![c.name.clone()];
        match c.last_contact(book) {
            Some(t) => out.push(format!("Last in touch {}.", ago(now, t))),
            None => out.push("No contact on record.".into()),
        }
        if let Some(e) = c.every_days {
            out.push(format!("You want to be in touch every {e} days."));
        }
        if let Some((m, d)) = c.birthday {
            out.push(format!("Birthday {} {d}.", MONTHS[(m - 1) as usize]));
        }
        for (_, n) in c.notes.iter().rev().take(10) {
            out.push(format!("- {n}"));
        }
        if c.notes.len() > 10 {
            out.push(format!("(and {} older notes)", c.notes.len() - 10));
        }
        out.join("\n")
    }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn ago(now: u64, t: u64) -> String {
    match now.saturating_sub(t) / 86_400 {
        0 => "today".into(),
        1 => "yesterday".into(),
        d => format!("{d} days ago"),
    }
}

/// What a sentence asked of the people list.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    /// "remember Sam's daughter is Leo", "note about Sam: …".
    Note { who: String, text: String },
    /// "keep in touch with Sam every 3 weeks"; `None` stops it.
    Every { who: String, days: Option<u32> },
    /// "I talked to Sam", "I called Sam".
    Talked { who: String },
    /// "Sam's birthday is March 4".
    Birthday { who: String, month: u32, day: u32 },
    /// "what do I know about Sam", "tell me about Sam".
    About { who: String },
    /// "who should I catch up with", "who haven't I talked to".
    Due,
    /// "Sam's email is sam@x.com".
    Email { who: String, address: String },
}

fn cadence(words: &str) -> Option<u32> {
    let w: Vec<&str> = words.split_whitespace().collect();
    let unit = |u: &str| -> Option<u32> {
        match u.trim_end_matches('s') {
            "day" => Some(1),
            "week" => Some(7),
            "fortnight" => Some(14),
            "month" => Some(30),
            "quarter" => Some(91),
            "year" => Some(365),
            _ => None,
        }
    };
    match w.as_slice() {
        ["every", u] => unit(u),
        ["every", "other", u] => unit(u).map(|d| d * 2),
        ["every", n, u] => {
            let n: u32 = match *n {
                "two" => 2,
                "three" => 3,
                "four" => 4,
                "six" => 6,
                x => x.parse().ok()?,
            };
            unit(u).map(|d| d * n)
        }
        ["weekly"] => Some(7),
        ["monthly"] => Some(30),
        ["yearly"] | ["annually"] => Some(365),
        _ => None,
    }
}

fn month_no(w: &str) -> Option<u32> {
    let w = w.to_ascii_lowercase();
    MONTHS.iter().position(|m| w.len() >= 3 && m.to_ascii_lowercase().starts_with(&w)).map(|i| i as u32 + 1)
}

/// Read one sentence. ASCII lower-casing keeps byte offsets, so names and
/// notes are cut from what you said, with your capitals.
pub fn read(said: &str) -> Option<Asked> {
    let s = said.trim().trim_end_matches(['.', '?', '!']);
    let low = s.to_ascii_lowercase();
    for lead in ["who should i catch up with", "who haven't i talked to", "who have i not talked to", "who's due a catch up", "who am i out of touch with"] {
        if low.starts_with(lead) {
            return Some(Asked::Due);
        }
    }
    for lead in ["what do i know about ", "tell me about ", "who is "] {
        if let Some(rest) = low.strip_prefix(lead) {
            if lead == "tell me about " && rest.split_whitespace().count() > 3 {
                return None;
            }
            return Some(Asked::About { who: s[lead.len()..].trim().to_string() });
        }
    }
    if let Some(rest) = low.strip_prefix("keep in touch with ") {
        let start = "keep in touch with ".len();
        if let Some(at) = rest.find(" every ").or_else(|| rest.find(" weekly")).or_else(|| rest.find(" monthly")) {
            let who = s[start..start + at].trim().to_string();
            return cadence(&rest[at..]).map(|d| Asked::Every { who, days: Some(d) });
        }
        return None;
    }
    if let Some(rest) = low.strip_prefix("stop keeping in touch with ") {
        return Some(Asked::Every { who: s[s.len() - rest.len()..].trim().to_string(), days: None });
    }
    for lead in ["i talked to ", "i spoke to ", "i spoke with ", "i called ", "i met ", "i saw ", "caught up with ", "i caught up with "] {
        if let Some(rest) = low.strip_prefix(lead) {
            let who = s[lead.len()..lead.len() + rest.len()].trim();
            let who = who.split(" today").next().unwrap_or(who).split(" yesterday").next().unwrap_or(who).trim();
            if !who.is_empty() && who.split_whitespace().count() <= 3 {
                return Some(Asked::Talked { who: who.to_string() });
            }
        }
    }
    // "Sam's email is sam@x.com" -- what ties a name to the mail cache.
    for key in ["'s email is ", "'s email address is "] {
        if let Some(at) = low.find(key) {
            let who = s[..at].trim().to_string();
            let address = s[at + key.len()..].trim().trim_end_matches('.').to_string();
            if !who.is_empty() && address.contains('@') && !address.contains(' ') {
                return Some(Asked::Email { who, address });
            }
        }
    }
    // "Sam's birthday is March 4" / "Sam's birthday is 4 March".
    if let Some(at) = low.find("'s birthday is ") {
        let who = s[..at].trim().to_string();
        let rest: Vec<&str> = low[at + "'s birthday is ".len()..].split_whitespace().collect();
        let (m, d) = match rest.as_slice() {
            [a, b, ..] => match (month_no(a), b.trim_end_matches(|c: char| c.is_alphabetic()).parse::<u32>().ok()) {
                (Some(m), Some(d)) => (m, d),
                _ => (month_no(b)?, a.trim_end_matches(|c: char| c.is_alphabetic()).parse().ok()?),
            },
            _ => return None,
        };
        return Some(Asked::Birthday { who, month: m, day: d });
    }
    // "note about Sam: …" / "note on Sam: …"
    // 30 Sep 2026: spoken, there's no colon -- "note about Sam that he's
    // moving in June" -- so "that" (or a comma) does the same job.
    for lead in ["note about ", "note on ", "remember about ", "make a note about ", "add a note about "] {
        if let Some(rest) = low.strip_prefix(lead) {
            let (at, skip) = match rest.find(':') {
                Some(c) => (c, 1),
                None => {
                    let m = [" that ", ", "].iter().filter_map(|m| rest.find(m).map(|i| (i, m.len()))).min_by_key(|(i, _)| *i)?;
                    m
                }
            };
            let who = s[lead.len()..lead.len() + at].trim().to_string();
            let text = s[lead.len() + at + skip..].trim().to_string();
            let lw = who.to_ascii_lowercase();
            if who.split_whitespace().count() > 3 || ["the ", "a ", "an ", "this ", "that "].iter().any(|p| lw.starts_with(p)) || lw == "it" {
                return None;
            }
            return (!who.is_empty() && !text.is_empty()).then_some(Asked::Note { who, text });
        }
    }
    // "remember Sam's daughter is called Leo" -- the possessive names them.
    if let Some(rest) = low.strip_prefix("remember ") {
        let at = rest.find("'s ")?;
        let name = &s["remember ".len().."remember ".len() + at];
        // "remember that Sam's" -> "Sam".
        let name = name.strip_prefix("that ").unwrap_or(name).trim();
        if name.is_empty() || name.split_whitespace().count() > 3 || name.eq_ignore_ascii_case("it") {
            return None;
        }
        let text = s["remember ".len()..].trim();
        let text = text.strip_prefix("that ").unwrap_or(text).to_string();
        return Some(Asked::Note { who: name.to_string(), text });
    }
    None
}

/// "Who's due": said.
pub fn due_said(due: &[(String, Option<u64>, u32)], birthdays: &[(String, u32)]) -> String {
    let mut out = Vec::new();
    for (name, days, every) in due {
        out.push(match days {
            Some(d) => format!("{name}: {d} days (you wanted every {every})."),
            None => format!("{name}: no contact on record (you wanted every {every})."),
        });
    }
    for (name, away) in birthdays {
        out.push(match away {
            0 => format!("{name}'s birthday is today."),
            1 => format!("{name}'s birthday is tomorrow."),
            d => format!("{name}'s birthday is in {d} days."),
        });
    }
    if out.is_empty() {
        "No one's overdue.".into()
    } else {
        out.join("\n")
    }
}
