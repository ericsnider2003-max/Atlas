//! What you copied, kept for a while so it can be found and pasted again.
//!
//! **Off unless you turn it on** (`clipboard_history.enabled`). `clipboard.rs`
//! was written on the rule that Atlas never watches the clipboard, because a
//! clipboard monitor sees every password you copy. Round 11 builds the
//! history you asked for, and keeps that rule's reason rather than its
//! letter:
//!
//! - **Nothing is read on a timer.** The tick asks the OS one number -- the
//!   clipboard's sequence number -- and only when it has moved is the copy
//!   read, once (`Platform::clipboard_change`, `clipboard_copy`).
//! - **A password manager's "don't keep this" is obeyed before any text is
//!   read.** Windows' `ExcludeClipboardContentFromMonitorProcessing`,
//!   `CanIncludeInClipboardHistory = 0` and `Clipboard Viewer Ignore` (the
//!   formats KeePass, 1Password and Bitwarden set) make the copy `Private`,
//!   and its text never enters this process.
//! - **Anything that looks secret is not kept either**, whoever copied it:
//!   keys, tokens, card and account numbers (`redact::secrets_in`).
//! - **Nothing leaves the machine.** The history isn't synced, isn't sent to
//!   a model, and isn't in backups' `upgrade::YOURS` list of things to carry.
//! - **It forgets.** Entries older than `keep_hours` go, and there are never
//!   more than `max_items`; one item is capped at `max_chars`.
//!
//! **Sources:** Ditto (GPL-3.0; read for its ideas only -- search, paste
//! again, the app a copy came from); Microsoft's clipboard documentation for
//! `GetClipboardSequenceNumber` and the do-not-keep formats; the CrossPaste
//! and Bitwarden issues that established which formats password managers
//! actually set.

use serde::{Deserialize, Serialize};

/// Settings, under `clipboard_history` in tools.yaml.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct HistoryConfig {
    /// Off by default: turning it on is your decision, not Atlas's.
    pub enabled: bool,
    pub keep_hours: u64,
    pub max_items: usize,
    pub max_chars: usize,
    /// Apps whose copies are never kept, by process name (a password
    /// manager, a banking app) -- belt and braces over the formats above.
    pub never_from: Vec<String>,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        HistoryConfig {
            enabled: false,
            keep_hours: 24,
            max_items: 200,
            max_chars: 20_000,
            never_from: vec!["KeePass".into(), "KeePassXC".into(), "1Password".into(), "Bitwarden".into(), "LastPass".into()],
        }
    }
}

/// One thing you copied.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub text: String,
    pub at: u64,
    /// The app in front when it was copied.
    pub from: String,
}

/// Why a copy wasn't kept, so the history can say so without saying what.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Skipped {
    /// The copying app said not to keep it.
    Private,
    /// It looked like a secret.
    Secret,
    /// From an app on the never list.
    NeverFrom,
    NotText,
    Empty,
    /// The same as the last one.
    Repeat,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct History {
    pub clips: Vec<Clip>,
    /// The last sequence number seen; not saved, so a restart reads once.
    #[serde(skip)]
    pub last_seq: Option<u32>,
    #[serde(skip)]
    pub skipped: u32,
}

impl History {
    /// Called with the clipboard's sequence number each tick. True when it
    /// has moved since last time -- the only case in which the copy is read.
    pub fn changed(&mut self, seq: Option<u32>) -> bool {
        match seq {
            None => false,
            Some(n) => {
                let moved = self.last_seq.map(|l| l != n).unwrap_or(true);
                self.last_seq = Some(n);
                moved
            }
        }
    }

    /// Keep one copy, if it may be kept. `from` is the app in front.
    pub fn keep(&mut self, cfg: &HistoryConfig, copy: &crate::platform::ClipCopy, from: &str, now: u64) -> Result<(), Skipped> {
        let text = match copy {
            crate::platform::ClipCopy::Private => return self.skip(Skipped::Private),
            crate::platform::ClipCopy::NotText => return self.skip(Skipped::NotText),
            crate::platform::ClipCopy::Text(t) => t,
        };
        let low_from = from.to_lowercase();
        if cfg.never_from.iter().any(|n| !n.is_empty() && low_from.contains(&n.to_lowercase())) {
            return self.skip(Skipped::NeverFrom);
        }
        if text.trim().is_empty() {
            return self.skip(Skipped::Empty);
        }
        if !crate::redact::secrets_in(text).is_empty() {
            return self.skip(Skipped::Secret);
        }
        let text: String = text.chars().take(cfg.max_chars).collect();
        if self.clips.last().map(|c| c.text == text).unwrap_or(false) {
            return self.skip(Skipped::Repeat);
        }
        // The same text copied again moves to the front rather than
        // appearing twice.
        self.clips.retain(|c| c.text != text);
        self.clips.push(Clip { text, at: now, from: from.to_string() });
        self.forget(cfg, now);
        Ok(())
    }

    fn skip(&mut self, why: Skipped) -> Result<(), Skipped> {
        self.skipped += 1;
        Err(why)
    }

    /// Drop what's past `keep_hours`, and the oldest past `max_items`.
    pub fn forget(&mut self, cfg: &HistoryConfig, now: u64) {
        let cut = now.saturating_sub(cfg.keep_hours * 3600);
        self.clips.retain(|c| c.at >= cut);
        let over = self.clips.len().saturating_sub(cfg.max_items);
        if over > 0 {
            self.clips.drain(..over);
        }
    }

    /// Newest first: the one you want is usually the one you just copied.
    pub fn recent(&self, n: usize) -> Vec<&Clip> {
        self.clips.iter().rev().take(n).collect()
    }

    /// The copies that match `query`, best first: every word must appear
    /// (tolerating a typo in longer words), newer wins a tie. A number
    /// ("the 3rd", "2") picks by position, newest first.
    pub fn find(&self, query: &str) -> Vec<&Clip> {
        let q = query.trim().to_lowercase();
        if let Some(n) = position_in(&q) {
            return self.recent(n).into_iter().skip(n.saturating_sub(1)).take(1).collect();
        }
        let words: Vec<&str> = q.split_whitespace().filter(|w| !STOP.contains(w)).collect();
        if words.is_empty() {
            return self.recent(10);
        }
        let mut hits: Vec<(usize, &Clip)> = self
            .clips
            .iter()
            .enumerate()
            .filter(|(_, c)| {
                let hay = c.text.to_lowercase();
                let tokens: Vec<&str> = hay.split(|ch: char| !ch.is_alphanumeric()).filter(|t| !t.is_empty()).collect();
                words.iter().all(|w| {
                    hay.contains(w)
                        || tokens.iter().any(|t| crate::typos::osa(t, w, crate::typos::allowance(w.chars().count())).is_some())
                })
            })
            .collect();
        hits.sort_by_key(|b| std::cmp::Reverse(b.0));
        hits.into_iter().map(|(_, c)| c).collect()
    }

    /// What the history says about itself, never quoting a clip.
    pub fn describe(&self, cfg: &HistoryConfig) -> String {
        if !cfg.enabled {
            return "Clipboard history is off. It's yours to turn on (clipboard_history.enabled): \
                    it keeps what you copy for a day, never a password manager's copies or anything \
                    that looks like a key or a card number, and never leaves this machine."
                .into();
        }
        let n = self.clips.len();
        let mut out = format!("{n} thing{} copied in the last {} hours.", if n == 1 { "" } else { "s" }, cfg.keep_hours);
        if self.skipped > 0 {
            out.push_str(&format!(" {} copies weren't kept (private, secret-looking, or from an app on the never list).", self.skipped));
        }
        out
    }
}

const STOP: &[&str] = &["the", "one", "about", "with", "that", "i", "copied", "thing", "a", "an", "from", "paste", "copy", "back"];

/// "the 3rd", "2", "second", "last": a position, newest first.
fn position_in(q: &str) -> Option<usize> {
    const WORDS: &[(&str, usize)] = &[("last", 1), ("latest", 1), ("first", 1), ("second", 2), ("third", 3), ("fourth", 4), ("fifth", 5)];
    let words: Vec<&str> = q.split_whitespace().filter(|w| !STOP.contains(w)).collect();
    if words.len() != 1 {
        return None;
    }
    let w = words[0];
    if let Some((_, n)) = WORDS.iter().find(|(x, _)| *x == w) {
        return Some(*n);
    }
    let digits: String = w.chars().take_while(|c| c.is_ascii_digit()).collect();
    let rest = &w[digits.len()..];
    (!digits.is_empty() && matches!(rest, "" | "st" | "nd" | "rd" | "th")).then(|| digits.parse().ok()).flatten().filter(|n| *n >= 1)
}

/// One clip as a line, shortened: "“the quarterly numbers …” (from EXCEL, 3 min ago)".
pub fn line(c: &Clip, now: u64) -> String {
    let flat: String = c.text.split_whitespace().collect::<Vec<_>>().join(" ");
    let short: String = flat.chars().take(60).collect();
    let more = if flat.chars().count() > 60 { " …" } else { "" };
    let ago = now.saturating_sub(c.at) / 60;
    let when = if ago == 0 { "just now".to_string() } else if ago < 60 { format!("{ago} min ago") } else { format!("{} h ago", ago / 60) };
    let from = c.from.trim_end_matches(".exe").trim_end_matches(".EXE");
    if from.is_empty() {
        format!("\u{201c}{short}{more}\u{201d} ({when})")
    } else {
        format!("\u{201c}{short}{more}\u{201d} (from {from}, {when})")
    }
}
