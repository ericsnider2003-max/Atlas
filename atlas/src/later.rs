//! The list for later.
//!
//! Eric, 25 Sep 2026 (F8): "I can instruct Atlas to add it to a list for
//! later as well so it's not forgotten." Anything Atlas just said (a
//! recommendation, an offer, an answer) can be put on it, read back on
//! request, and it's mentioned in the brief once a week so it isn't
//! forgotten. Nothing on it is acted on; it's a list, not a queue.

use serde::{Deserialize, Serialize};

pub const RECORD: &str = "later_list";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub what: String,
    pub added: u64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Later {
    pub items: Vec<Item>,
    /// When the brief last mentioned it.
    #[serde(default)]
    pub mentioned: u64,
}

/// The gist of something Atlas said: its first sentence, kept short.
pub fn gist(said: &str) -> String {
    let first = said
        .split_inclusive(['.', '?', '!', '\n'])
        .next()
        .unwrap_or(said)
        .trim()
        .trim_end_matches(['.', '\n']);
    let short: String = first.chars().take(160).collect();
    short.trim().to_string()
}

impl Later {
    pub fn add(&mut self, what: &str, t: u64) -> bool {
        let what = what.trim();
        if what.is_empty() || self.items.iter().any(|i| i.what.eq_ignore_ascii_case(what)) {
            return false;
        }
        self.items.push(Item { what: what.to_string(), added: t });
        true
    }

    /// Take off the one matching these words best; `None` if nothing matches.
    pub fn take_off(&mut self, words: &str) -> Option<Item> {
        let said: Vec<String> = words.to_lowercase().split_whitespace().filter(|w| w.len() > 2).map(str::to_string).collect();
        let (i, hits) = self
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| (i, said.iter().filter(|w| it.what.to_lowercase().contains(w.as_str())).count()))
            .max_by_key(|(_, h)| *h)?;
        (hits > 0).then(|| self.items.remove(i))
    }

    pub fn read_back(&self) -> String {
        if self.items.is_empty() {
            return "Your later list is empty.".into();
        }
        let each: Vec<String> = self.items.iter().enumerate().map(|(n, i)| format!("{}. {}", n + 1, i.what)).collect();
        format!("On your later list: {}", each.join("; "))
    }

    /// The weekly line for the brief, when there's anything on it.
    pub fn weekly_line(&mut self, t: u64) -> Option<String> {
        if self.items.is_empty() || t.saturating_sub(self.mentioned) < 7 * 86_400 {
            return None;
        }
        self.mentioned = t;
        let n = self.items.len();
        Some(format!(
            "{n} thing{} on your later list, the oldest \"{}\" — say \"what's on my later list\" to hear them",
            if n == 1 { "" } else { "s" },
            self.items[0].what
        ))
    }
}
