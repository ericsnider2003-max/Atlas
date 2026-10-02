//! Clearing out a personal inbox.
//!
//! Different problem from triage. Triage sorts what matters; this gets rid of
//! what doesn't, which is most of a personal inbox.
//!
//! ## The trap worth knowing about
//!
//! Clicking "unsubscribe" in a spam email is how you confirm your address is
//! real and read. It makes things worse, reliably. So Atlas distinguishes
//! sharply:
//!
//! * **A legitimate sender** — a shop, a newsletter you signed up to — has a
//!   `List-Unsubscribe` header, which is the machine-readable, one-click,
//!   standardised way out. Use it.
//! * **Spam** has no such header, or has one pointing somewhere odd. Never
//!   touch it. Block the sender and move on.
//!
//! Getting that distinction wrong in the wrong direction actively harms you,
//! which is why it's the thing this module is built around.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sender {
    pub address: String,
    /// What it calls itself.
    pub name: String,
    /// Messages in the period looked at.
    pub count: u32,
    /// How many you opened.
    pub opened: u32,
    /// How many you replied to.
    pub replied: u32,
    /// The standard header, when present.
    pub list_unsubscribe: Option<String>,
    /// Days since the most recent one.
    pub last_seen_days: u32,
}

impl Sender {
    /// How much you actually engage with this.
    fn engagement(&self) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        (self.opened as f32 + self.replied as f32 * 3.0) / self.count as f32
    }

    /// Does it offer the proper way out?
    pub fn has_safe_exit(&self) -> bool {
        match &self.list_unsubscribe {
            None => false,
            Some(u) => {
                let l = u.to_lowercase();
                // One-click unsubscribe is either a mailto: or an https URL.
                // Anything else is not the standard and isn't trusted.
                (l.starts_with("<mailto:") || l.starts_with("<https://") || l.starts_with("mailto:")
                    || l.starts_with("https://"))
                    && !l.contains("http://")
            }
        }
    }
}

/// Built from a window of real fetched mail, grouped by sender address.
/// `count`, `opened`, `replied` and `last_seen_days` all come straight
/// from the server's own flags and dates — Atlas doesn't keep a separate
/// history of its own. One fetch over a wide-enough window is enough for
/// a real verdict, because the server already remembers what you did
/// with each message.
pub fn senders_from(messages: &[crate::imap::Message], now: u64) -> Vec<Sender> {
    let mut by_address: std::collections::BTreeMap<String, Sender> = std::collections::BTreeMap::new();
    for m in messages {
        let (name, address) = split_from(&m.from);
        if address.is_empty() {
            continue;
        }
        let entry = by_address.entry(address.clone()).or_insert_with(|| Sender {
            address: address.clone(),
            name: name.clone(),
            count: 0,
            opened: 0,
            replied: 0,
            list_unsubscribe: None,
            last_seen_days: u32::MAX,
        });
        entry.count += 1;
        if m.seen {
            entry.opened += 1;
        }
        if m.answered {
            entry.replied += 1;
        }
        if entry.list_unsubscribe.is_none() && !m.list_unsubscribe.trim().is_empty() {
            entry.list_unsubscribe = Some(m.list_unsubscribe.clone());
        }
        if entry.name.is_empty() && !name.is_empty() {
            entry.name = name;
        }
        if let Some(at) = crate::triage::parse_rfc2822(&m.date) {
            let days = (now.saturating_sub(at) / 86400) as u32;
            entry.last_seen_days = entry.last_seen_days.min(days);
        }
    }
    by_address
        .into_values()
        .map(|mut s| {
            // No message from this sender had a date that parsed. Assume
            // recent rather than invent an old one — `last_seen_days`
            // feeding into "block this, you haven't looked in months" is
            // exactly the claim not to make up.
            if s.last_seen_days == u32::MAX {
                s.last_seen_days = 0;
            }
            s
        })
        .collect()
}

/// Splits a `From` header into (display name, bare address). `"Name
/// <addr@example.com>"` and a bare `"addr@example.com"` both parse; the
/// name comes back empty when there isn't one, which the caller treats
/// as "fall back to the address" rather than something to guess at.
pub(crate) fn split_from(from: &str) -> (String, String) {
    let from = from.trim();
    if let Some(open) = from.find('<') {
        if let Some(close_rel) = from[open..].find('>') {
            let address = from[open + 1..open + close_rel].trim().to_lowercase();
            let name = from[..open].trim().trim_matches('"').to_string();
            return (name, address);
        }
    }
    (String::new(), from.to_lowercase())
}

/// What to do about a sender.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Verdict {
    /// Legitimate and you don't read it. One-click out.
    Unsubscribe { how: String, why: String },
    /// No safe way out. Block rather than reply.
    ///
    /// Never unsubscribe from these — clicking confirms your address is live.
    BlockOnly { why: String },
    /// You do read it.
    Keep { why: String },
    /// Ask — it's borderline.
    Ask { why: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct UnsubConfig {
    pub enabled: bool,
    /// Below this engagement, it's a candidate.
    pub dead_below: f32,
    /// At least this many messages before judging.
    pub min_messages: u32,
    /// Never unsubscribe from these, whatever the numbers say.
    pub never: Vec<String>,
    /// Do it without asking each time.
    pub bulk_without_asking: bool,
}

impl Default for UnsubConfig {
    fn default() -> Self {
        UnsubConfig {
            enabled: false,
            dead_below: 0.1,
            min_messages: 4,
            never: vec![
                // The ones where missing a message costs you something real.
                "bank".into(), "hmrc".into(), "irs".into(), "gov".into(),
                "insurance".into(), "landlord".into(), "school".into(),
                "doctor".into(), "pharmacy".into(), "broker".into(),
                "invoice".into(), "receipt".into(), "security".into(),
            ],
            bulk_without_asking: false,
        }
    }
}

pub fn judge(s: &Sender, cfg: &UnsubConfig) -> Verdict {
    let a = format!("{} {}", s.address, s.name).to_lowercase();

    // Some senders you don't unsubscribe from however dead the engagement,
    // because the cost of missing one is not symmetrical.
    if let Some(word) = cfg.never.iter().find(|n| a.contains(n.as_str())) {
        return Verdict::Keep { why: format!("anything with \"{word}\" in it stays") };
    }

    if s.count < cfg.min_messages {
        return Verdict::Keep { why: "not enough of them to judge".into() };
    }

    let engagement = s.engagement();

    if engagement >= cfg.dead_below {
        return Verdict::Keep {
            why: format!("you open about {:.0}% of them", engagement.min(1.0) * 100.0),
        };
    }

    // Dead. Now the question that matters: is there a safe way out?
    if s.has_safe_exit() {
        Verdict::Unsubscribe {
            how: s.list_unsubscribe.clone().unwrap_or_default(),
            why: format!(
                "{} messages, you've opened {} — and it has a proper unsubscribe",
                s.count, s.opened
            ),
        }
    } else {
        // Never click a link in the body of one of these. Clicking confirms
        // your address is real and read, which makes it worse.
        Verdict::BlockOnly {
            why: format!(
                "{} messages, none opened, and no proper unsubscribe — clicking a link in \
                 something like that just confirms your address is real",
                s.count
            ),
        }
    }
}

/// The whole inbox's worth.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct Cleanup {
    pub unsubscribe: Vec<(String, String)>,
    pub block: Vec<String>,
    pub keep: usize,
    pub ask_about: Vec<String>,
    /// Messages a year of this would stop.
    pub saves_per_year: u32,
}

pub fn plan(senders: &[Sender], cfg: &UnsubConfig) -> Cleanup {
    let mut c = Cleanup {
        unsubscribe: Vec::new(),
        block: Vec::new(),
        keep: 0,
        ask_about: Vec::new(),
        saves_per_year: 0,
    };
    for s in senders {
        match judge(s, cfg) {
            Verdict::Unsubscribe { how, .. } => {
                c.unsubscribe.push((s.name.clone(), how));
                c.saves_per_year += yearly_rate(s);
            }
            Verdict::BlockOnly { .. } => {
                c.block.push(s.name.clone());
                c.saves_per_year += yearly_rate(s);
            }
            Verdict::Keep { .. } => c.keep += 1,
            Verdict::Ask { .. } => c.ask_about.push(s.name.clone()),
        }
    }
    c
}

/// Roughly how many a year, from how many arrived and how recently.
fn yearly_rate(s: &Sender) -> u32 {
    if s.last_seen_days == 0 {
        return s.count;
    }
    // Assume the count covers roughly the window since the oldest.
    let per_day = s.count as f32 / (s.last_seen_days.max(1) as f32 + 30.0);
    (per_day * 365.0).round() as u32
}

/// What Atlas says about a cleanup, before doing any of it.
pub fn spoken(c: &Cleanup) -> String {
    if c.unsubscribe.is_empty() && c.block.is_empty() {
        return "Nothing worth clearing out — you read most of what you get.".into();
    }
    let mut s = String::new();
    if !c.unsubscribe.is_empty() {
        s.push_str(&format!(
            "{} I can unsubscribe from properly",
            c.unsubscribe.len()
        ));
    }
    if !c.block.is_empty() {
        if !s.is_empty() {
            s.push_str(", and ");
        }
        s.push_str(&format!(
            "{} I'd block rather than unsubscribe — clicking those would confirm your address",
            c.block.len()
        ));
    }
    s.push('.');
    if c.saves_per_year > 20 {
        s.push_str(&format!(" That's about {} fewer a year.", c.saves_per_year));
    }
    s
}

/// What one-click unsubscribe actually sends.
///
/// RFC 8058: a POST with this exact body to the https URL, or an empty mail to
/// the mailto address. Not a browser visit — a browser visit is what loads
/// their tracking.
pub fn one_click(header: &str) -> Option<(String, &'static str)> {
    // The header is a list -- `<https://a>, <mailto:b>` -- and each entry is
    // taken whole (1 Oct 2026 security pass: only the outer brackets were
    // stripped, so a two-entry header became one URL `https://a>, <mailto:b`).
    // The web link is preferred; an entry with a space, a quote or a bracket
    // inside it is not a link at all.
    let entries: Vec<&str> = if header.contains('<') {
        header.split('<').skip(1).filter_map(|e| e.split_once('>').map(|(inside, _)| inside.trim())).collect()
    } else {
        vec![header.trim()]
    };
    let entries: Vec<&str> = entries
        .into_iter()
        .filter(|e| !e.chars().any(|c| c.is_whitespace() || c.is_control() || "<>\"'".contains(c)))
        .collect();
    if let Some(web) = entries.iter().find(|e| e.starts_with("https://")) {
        return Some((web.to_string(), "List-Unsubscribe=One-Click"));
    }
    entries.iter().find(|e| e.starts_with("mailto:") && e.len() > 7).map(|m| (m.to_string(), ""))
}

#[cfg(test)]
mod senders_from_tests {
    use super::*;

    fn msg(from: &str, date: &str, seen: bool, answered: bool, list_unsub: &str) -> crate::imap::Message {
        crate::imap::Message {
            from: from.into(),
            date: date.into(),
            seen,
            answered,
            list_unsubscribe: list_unsub.into(),
            ..Default::default()
        }
    }

    #[test]
    fn split_from_reads_a_display_name_and_address() {
        assert_eq!(
            split_from("Newsletter <news@shop.example>"),
            ("Newsletter".to_string(), "news@shop.example".to_string())
        );
    }

    #[test]
    fn split_from_handles_a_bare_address_with_no_display_name() {
        assert_eq!(split_from("news@shop.example"), (String::new(), "news@shop.example".to_string()));
    }

    #[test]
    fn split_from_strips_surrounding_quotes_from_the_display_name() {
        let (name, _) = split_from("\"Big Shop\" <news@shop.example>");
        assert_eq!(name, "Big Shop");
    }

    #[test]
    fn split_from_lowercases_the_address_for_consistent_grouping() {
        let (_, addr) = split_from("News <News@Shop.Example>");
        assert_eq!(addr, "news@shop.example");
    }

    #[test]
    fn messages_from_the_same_address_are_grouped_into_one_sender() {
        let now = 1_700_000_000;
        let messages = vec![
            msg("Shop <news@shop.example>", "Mon, 1 Jan 2026 09:00:00 +0000", true, false, "<mailto:unsub@shop.example>"),
            msg("Shop <news@shop.example>", "Tue, 2 Jan 2026 09:00:00 +0000", false, false, ""),
        ];
        let senders = senders_from(&messages, now);
        assert_eq!(senders.len(), 1);
        assert_eq!(senders[0].count, 2);
        assert_eq!(senders[0].opened, 1, "only one of the two was seen");
    }

    #[test]
    fn a_reply_is_counted_as_replied_not_just_opened() {
        let now = 1_700_000_000;
        let messages = vec![msg("Boss <boss@work.example>", "Mon, 1 Jan 2026 09:00:00 +0000", true, true, "")];
        let senders = senders_from(&messages, now);
        assert_eq!(senders[0].opened, 1);
        assert_eq!(senders[0].replied, 1);
    }

    #[test]
    fn the_list_unsubscribe_header_is_carried_onto_the_sender() {
        let now = 1_700_000_000;
        let messages = vec![msg(
            "Shop <news@shop.example>",
            "Mon, 1 Jan 2026 09:00:00 +0000",
            false,
            false,
            "<https://shop.example/unsub>",
        )];
        let senders = senders_from(&messages, now);
        assert_eq!(senders[0].list_unsubscribe.as_deref(), Some("<https://shop.example/unsub>"));
    }

    #[test]
    fn two_different_addresses_produce_two_separate_senders() {
        let now = 1_700_000_000;
        let messages = vec![
            msg("Shop A <a@shop.example>", "Mon, 1 Jan 2026 09:00:00 +0000", true, false, ""),
            msg("Shop B <b@shop.example>", "Mon, 1 Jan 2026 09:00:00 +0000", true, false, ""),
        ];
        let senders = senders_from(&messages, now);
        assert_eq!(senders.len(), 2);
    }

    #[test]
    fn last_seen_days_reflects_the_most_recent_message_from_that_sender() {
        // "now" is exactly 5 days after the second message's date.
        let jan_6_2026 = 1_767_657_600; // date -u -d "2026-01-06" +%s
        let now = jan_6_2026 + 5 * 86400;
        let messages = vec![
            msg("Shop <news@shop.example>", "Mon, 1 Jan 2026 00:00:00 +0000", true, false, ""),
            msg("Shop <news@shop.example>", "Tue, 6 Jan 2026 00:00:00 +0000", true, false, ""),
        ];
        let senders = senders_from(&messages, now);
        assert_eq!(senders[0].last_seen_days, 5, "should reflect the newer message, not the older one");
    }

    #[test]
    fn a_message_with_no_from_address_at_all_is_skipped_rather_than_creating_a_blank_sender() {
        let now = 1_700_000_000;
        let messages = vec![msg("", "Mon, 1 Jan 2026 09:00:00 +0000", true, false, "")];
        let senders = senders_from(&messages, now);
        assert!(senders.is_empty());
    }
}

/// Where the last look's plans wait for your "go ahead", per account.
pub const PENDING: &str = "unsub_pending";

/// "Unsubscribe from those": the go-ahead after the report.
pub fn go_ahead(said: &str) -> bool {
    let t: String = said.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' }).collect();
    let t = t.split_whitespace().collect::<Vec<_>>().join(" ");
    [
        "unsubscribe from those", "unsubscribe from them", "unsubscribe from all of them", "unsubscribe me from those",
        "unsubscribe me from them", "go ahead and unsubscribe", "yes unsubscribe", "do the unsubscribes", "unsubscribe from all those",
    ]
    .iter()
    .any(|p| t.starts_with(p) || t == *p)
}
