//! Text you type again and again -- an address, a signature, a standard
//! reply -- kept once and typed for you: "type my address", or `;addr`
//! followed by the expand key.
//!
//! **Sources:** Espanso (GPL-3.0; read for its ideas only) for triggers and
//! variables. Espanso expands by reading every keystroke through a system
//! hook, which is a keylogger by construction; **this doesn't.** Atlas never
//! watches your typing. Expansion happens only when asked:
//!
//! - by voice or the command line ("type my signature"), or
//! - with the expand hotkey (a chord the OS delivers only when pressed,
//!   `RegisterHotKey`): Atlas selects the word just before the cursor, reads
//!   it through the clipboard, puts your clipboard back, and types the
//!   snippet over the word if -- and only if -- it's one of your triggers.
//!
//! **Soundproofing.** It won't type into an app on your no-input list or the
//! dictation never-list (`dictate::may_type_into`). An app running as
//! administrator refuses typed input from Atlas (Windows' UIPI) and that is
//! said, not swallowed. A snippet that holds a secret-looking string isn't
//! saved at all -- that's what the vault is for.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Snippets {
    /// Trigger (lower-case, e.g. ";addr" or "address") to its text.
    pub by_trigger: BTreeMap<String, String>,
}

/// Why a snippet couldn't be kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    Empty,
    /// It holds something secret-looking; names the kinds.
    Secret(Vec<&'static str>),
    TooLong,
}

/// The most a snippet may hold.
pub const MAX_CHARS: usize = 4000;

fn norm(trigger: &str) -> String {
    trigger.trim().trim_start_matches("my ").trim().to_lowercase()
}

impl Snippets {
    pub fn save(&mut self, trigger: &str, text: &str) -> Result<(), Refused> {
        let t = norm(trigger);
        if t.is_empty() || text.trim().is_empty() {
            return Err(Refused::Empty);
        }
        if text.chars().count() > MAX_CHARS {
            return Err(Refused::TooLong);
        }
        let secrets = crate::redact::secrets_in(text);
        if !secrets.is_empty() {
            return Err(Refused::Secret(secrets));
        }
        self.by_trigger.insert(t, text.to_string());
        Ok(())
    }

    pub fn remove(&mut self, trigger: &str) -> bool {
        self.by_trigger.remove(&norm(trigger)).is_some()
    }

    /// The snippet for a trigger or a name: exact first, then with or
    /// without a leading ";", then a name inside a trigger ("address" finds
    /// ";addr" only if unambiguous).
    pub fn get(&self, asked: &str) -> Option<(&str, &str)> {
        let a = norm(asked);
        let bare = a.trim_start_matches(';').to_string();
        if let Some((k, v)) = self.by_trigger.get_key_value(&a) {
            return Some((k, v));
        }
        for (k, v) in &self.by_trigger {
            if k.trim_start_matches(';') == bare {
                return Some((k, v));
            }
        }
        let hits: Vec<(&String, &String)> = self
            .by_trigger
            .iter()
            .filter(|(k, _)| {
                let kb = k.trim_start_matches(';');
                bare.len() >= 3 && (bare.starts_with(kb) || kb.starts_with(&bare))
            })
            .collect();
        match hits.as_slice() {
            [(k, v)] => Some((k.as_str(), v.as_str())),
            _ => None,
        }
    }

    /// The trigger only if the word is exactly one of yours -- what the
    /// hotkey expansion uses, so an ordinary word is never replaced.
    pub fn exact(&self, word: &str) -> Option<&str> {
        self.by_trigger.get(&word.trim().to_lowercase()).map(|s| s.as_str())
    }
}

/// Fill the variables: `{date}` (2026-09-25), `{time}` (14:05),
/// `{weekday}` (Friday), `{year}`. `local` is local seconds. Unknown braces
/// are left as written.
pub fn fill(text: &str, local: u64) -> String {
    let c = crate::civil::Civil::from_local(local as i64);
    let wd = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"][c.weekday() as usize];
    text.replace("{date}", &format!("{:04}-{:02}-{:02}", c.year, c.month, c.day))
        .replace("{time}", &format!("{:02}:{:02}", c.hour, c.minute))
        .replace("{weekday}", wd)
        .replace("{year}", &c.year.to_string())
}

/// "save snippet ;sig as Best, Eric" / "remember my address is 1 Main St":
/// (trigger, text).
pub fn read_save(said: &str) -> Option<(String, String)> {
    // ASCII lowering keeps byte positions, so the text is cut from what you
    // said, not from a copy with different lengths.
    let low = said.to_ascii_lowercase();
    for (lead, sep) in [("save snippet ", " as "), ("add snippet ", " as "), ("snippet ", " as "), ("save my ", " as ")] {
        if let Some(rest) = low.strip_prefix(lead) {
            let at = rest.find(sep)?;
            let trigger = rest[..at].trim().to_string();
            let start = lead.len() + at + sep.len();
            let text = said.get(start..)?.trim().to_string();
            return Some((trigger, text));
        }
    }
    None
}
