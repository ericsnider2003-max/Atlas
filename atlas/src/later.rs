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
    /// Versions survive deletion and restart, preventing delayed adds from
    /// resurrecting an item. Old saved lists have no versions yet.
    #[serde(default)]
    pub versions: std::collections::BTreeMap<String, crate::sync::Version>,
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
    pub fn note_version(&mut self, event: &crate::sync::Event) -> bool {
        let crate::sync::What::Changed { id, field, .. } = &event.what else { return false };
        let Some(what) = id.strip_prefix("later:").filter(|_| field == "later") else { return false };
        let key = what.to_lowercase();
        let version = crate::sync::Version::of(event);
        if self.versions.get(&key).is_some_and(|old| *old >= version) {
            return false;
        }
        self.versions.insert(key, version);
        true
    }

    /// Returns true when the saved list/version changed, including a delete
    /// of an absent item: that tombstone is needed for future delayed events.
    pub fn apply_synced(&mut self, event: &crate::sync::Event) -> bool {
        let crate::sync::What::Changed { id, field, to } = &event.what else { return false };
        let Some(what) = id.strip_prefix("later:").filter(|_| field == "later") else { return false };
        let added = if to.is_empty() { None } else {
            let Ok(added) = to.parse::<u64>() else { return false };
            Some(added)
        };
        if !self.note_version(event) {
            return false;
        }
        self.items.retain(|i| i.what.to_lowercase() != what.to_lowercase());
        if let Some(added) = added {
            self.items.push(Item { what: what.into(), added });
        }
        self.items.sort_by(|a, b| a.added.cmp(&b.added).then_with(|| a.what.to_lowercase().cmp(&b.what.to_lowercase())));
        true
    }
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

#[cfg(test)]
mod sync_recovery {
    use super::*;
    fn event(device: &str, at: u64, to: &str) -> crate::sync::Event {
        crate::sync::Event {
            device: device.into(), seq: 1, at, hlc: crate::hlc::Stamp::ZERO,
            what: crate::sync::What::Changed { id: "later:write the script".into(), field: "later".into(), to: to.into() },
        }
    }

    #[test]
    fn distinct_items_have_the_same_order_after_opposite_delivery() {
        let first = event("phone", 10, "10");
        let mut second = event("laptop", 20, "20");
        second.what = crate::sync::What::Changed { id: "later:record the video".into(), field: "later".into(), to: "20".into() };
        let mut a = Later::default();
        a.apply_synced(&first);
        a.apply_synced(&second);
        let mut b = Later::default();
        b.apply_synced(&second);
        b.apply_synced(&first);
        assert_eq!(a, b, "the visible list must converge, including order");
    }

    #[test]
    fn delayed_add_cannot_resurrect_a_deleted_item_after_restart() {
        let add = event("phone", 10, "10");
        let delete = event("laptop", 20, "");
        let mut a = Later::default();
        a.apply_synced(&add);
        a.apply_synced(&delete);
        let saved = serde_json::to_vec(&a).unwrap();
        let mut restarted: Later = serde_json::from_slice(&saved).unwrap();
        assert!(!restarted.apply_synced(&add), "an older add must not override the saved deletion");
        assert!(restarted.items.is_empty());
        let mut b = Later::default();
        b.apply_synced(&delete);
        b.apply_synced(&add);
        assert_eq!(a, b, "opposite delivery orders must converge");
    }

    #[test]
    fn concurrent_changes_use_device_tie_break_and_future_adds_work() {
        let add = event("a-phone", 20, "20");
        let delete = event("z-laptop", 20, "");
        let mut a = Later::default();
        a.apply_synced(&add);
        a.apply_synced(&delete);
        let mut b = Later::default();
        b.apply_synced(&delete);
        b.apply_synced(&add);
        assert_eq!(a, b);
        assert!(b.items.is_empty());
        assert!(b.apply_synced(&event("a-phone", 21, "21")));
        assert_eq!(b.items[0].added, 21);
    }
}
