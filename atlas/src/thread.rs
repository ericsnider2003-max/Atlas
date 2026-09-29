//! One conversation, forever.
//!
//! This is the largest single difference between what Atlas was and what you
//! pictured. Jarvis never starts a session — Tony walks in mid-thought and it
//! continues. No greeting, no re-explaining, no "how can I help you today".
//!
//! So there are no sessions here. There is one thread that spans days. It
//! grows, gets folded into a summary as it grows, and picks up where it left
//! off. The only thing that resets is what fits in the model's context, and
//! that is a compression problem, not a conversation boundary.

use crate::error::Result;
use crate::store::{now, Store};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Exchange {
    pub at: u64,
    pub said: String,
    pub reply: String,
    /// A topic label, so a gap of days can be described rather than replayed.
    #[serde(default)]
    pub about: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ThreadConfig {
    /// Exchanges kept word for word. Older ones live in the summary.
    pub verbatim: usize,
    /// Fold into the summary once there are this many beyond verbatim.
    pub fold_after: usize,
    /// Rough character budget for the context handed to the model.
    pub context_chars: usize,
    /// A gap longer than this is worth acknowledging.
    pub gap_secs: u64,
}

impl Default for ThreadConfig {
    fn default() -> Self {
        ThreadConfig { verbatim: 12, fold_after: 24, context_chars: 4000, gap_secs: 3600 }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Thread {
    /// Everything older than `verbatim`, compressed. Written by the model.
    pub summary: String,
    pub recent: Vec<Exchange>,
    /// Exchanges folded away so far. Only for reporting.
    #[serde(default)]
    pub folded: usize,
    #[serde(default)]
    pub last_active: u64,
    /// What you were last working on, so a return is a continuation.
    #[serde(default)]
    pub current_topic: Option<String>,
}

impl Thread {
    pub fn load(store: &Store) -> Thread {
        store.load("thread")
    }
    pub fn save(&self, store: &Store) -> Result<()> {
        store.save("thread", self)
    }

    pub fn append(&mut self, said: &str, reply: &str, about: Option<String>, t: u64) {
        if let Some(a) = &about {
            self.current_topic = Some(a.clone());
        }
        self.recent.push(Exchange {
            at: t,
            said: said.to_string(),
            reply: reply.to_string(),
            about,
        });
        self.last_active = t;
    }

    pub fn len(&self) -> usize {
        self.recent.len() + self.folded
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Was an exchange for exactly `said` written since the thread was
    /// `before` long? Asked of the exchanges themselves, not the length: a
    /// different turn that appended meanwhile must not count (28 Sep 2026).
    /// Exchanges folded away since then can't be checked and are looked for
    /// among the ones still kept.
    pub fn said_since(&self, before: usize, said: &str) -> bool {
        let start = before.saturating_sub(self.folded).min(self.recent.len());
        self.recent[start..].iter().any(|e| e.said == said)
    }

    /// Time to compress the older end?
    pub fn needs_folding(&self, cfg: &ThreadConfig) -> bool {
        self.recent.len() > cfg.fold_after
    }

    /// The exchanges that should be summarised away.
    pub fn foldable(&self, cfg: &ThreadConfig) -> &[Exchange] {
        if self.recent.len() <= cfg.verbatim {
            return &[];
        }
        &self.recent[..self.recent.len() - cfg.verbatim]
    }

    /// Replace the old exchanges with a summary of them.
    ///
    /// The thread is never truncated — it is compressed. Dropping old turns
    /// outright is what makes an assistant feel amnesiac.
    pub fn fold(&mut self, new_summary: &str, cfg: &ThreadConfig) {
        let n = self.foldable(cfg).len();
        if n == 0 {
            return;
        }
        self.recent.drain(0..n);
        self.folded += n;
        self.summary = new_summary.trim().to_string();
    }

    /// Fold the first `n` exchanges into `new_summary` — the number that was
    /// handed to the model, since more may have been said while it worked.
    pub fn fold_first(&mut self, n: usize, new_summary: &str) {
        let n = n.min(self.recent.len());
        if n == 0 {
            return;
        }
        self.recent.drain(0..n);
        self.folded += n;
        self.summary = new_summary.trim().to_string();
    }

    /// What to hand the model for folding.
    pub fn fold_input(&self, cfg: &ThreadConfig) -> String {
        let mut s = String::new();
        if !self.summary.is_empty() {
            s.push_str("So far:\n");
            s.push_str(&self.summary);
            s.push_str("\n\nSince then:\n");
        }
        for e in self.foldable(cfg) {
            s.push_str(&format!("you: {}\natlas: {}\n", e.said, e.reply));
        }
        s
    }

    /// Context for an ordinary turn: the summary, then recent exchanges
    /// verbatim, newest last, inside a character budget.
    pub fn context(&self, cfg: &ThreadConfig) -> String {
        let mut parts: Vec<String> = Vec::new();
        let mut used = 0usize;

        for e in self.recent.iter().rev() {
            let line = format!("you: {}\natlas: {}", e.said, e.reply);
            if used + line.len() > cfg.context_chars {
                break;
            }
            used += line.len();
            parts.push(line);
        }
        parts.reverse();

        let mut out = String::new();
        if !self.summary.is_empty() {
            out.push_str("Earlier: ");
            out.push_str(&self.summary);
            out.push_str("\n\n");
        }
        out.push_str(&parts.join("\n"));
        out
    }

    /// The conversation as turns, for a model that takes messages: the
    /// folded summary as one "Earlier:" system line, then the last
    /// `exchanges` exchanges (up to half as many again, see below) as real
    /// user and assistant messages, newest last, inside a budget of roughly
    /// `budget_tokens` tokens (four characters to a token). The newest
    /// exchange is always kept whole.
    ///
    /// `context` pasted all of this into the user's message as "you: … /
    /// atlas: …", so the model read one long user turn and often answered
    /// its own last line (27 Sep 2026).
    ///
    /// **Where the window starts moves in steps, not one exchange a turn**
    /// (28 Sep 2026). It used to be exactly the last six: once there were
    /// six, every turn dropped the oldest, so the conversation the model
    /// read started somewhere new every time -- and llama.cpp, which can
    /// only reuse a prompt's unchanged beginning (the shipped Qwen3-VL can't
    /// shift a cached middle), read all of it again on every turn. Now the
    /// start sits on a boundary counted from the first exchange ever
    /// (`folded` included, so it doesn't move when old ones are folded
    /// away) and jumps `exchanges / 2` at a time: the model reads between
    /// `exchanges` and `exchanges + exchanges / 2 - 1` earlier exchanges --
    /// never fewer than before when they fit the budget -- and the start
    /// stays put for several turns in a row. Over the budget, the start
    /// jumps forward by the same steps until it fits.
    pub fn messages(&self, exchanges: usize, budget_tokens: usize) -> Vec<crate::brain::Msg> {
        use crate::brain::Msg;
        let budget = budget_tokens * 4;
        let step = (exchanges / 2).max(1);
        // Every exchange by its place in the whole conversation.
        let total = self.folded + self.recent.len();
        let size_from = |from: usize| -> usize {
            self.recent
                .iter()
                .enumerate()
                .filter(|(i, _)| self.folded + i >= from)
                .map(|(_, e)| e)
                .filter(|e| !e.said.trim().is_empty() && !e.reply.trim().is_empty())
                .map(|e| e.said.len() + e.reply.len())
                .sum()
        };
        // The newest exchange with words on both sides: kept whatever the
        // budget says.
        let newest = self
            .recent
            .iter()
            .rposition(|e| !e.said.trim().is_empty() && !e.reply.trim().is_empty())
            .map(|i| self.folded + i)
            .unwrap_or(total);
        let mut from = if total > exchanges { ((total - exchanges) / step) * step } else { 0 };
        from = from.max(self.folded).min(newest);
        while from < newest && size_from(from) > budget {
            from = (from + step).min(newest);
        }
        let mut out = Vec::new();
        if !self.summary.trim().is_empty() {
            out.push(Msg::system(format!("Earlier: {}", self.summary.trim())));
        }
        for (i, e) in self.recent.iter().enumerate() {
            // An exchange with nothing said on either side is no turn at all.
            if self.folded + i < from || e.said.trim().is_empty() || e.reply.trim().is_empty() {
                continue;
            }
            out.push(Msg::user(e.said.clone()));
            out.push(Msg::assistant(e.reply.clone()));
        }
        out
    }

    /// How long since you last said anything.
    fn gap(&self, t: u64) -> u64 {
        t.saturating_sub(self.last_active)
    }

    /// What Atlas says on picking the thread back up.
    ///
    /// Never a greeting. Either it continues silently, or it names what you
    /// were doing — which is the thing that makes it feel like the same
    /// conversation rather than a new one.
    pub fn resume_line(&self, cfg: &ThreadConfig, t: u64) -> Option<String> {
        if self.is_empty() || self.gap(t) < cfg.gap_secs {
            return None;
        }
        let topic = self.current_topic.as_ref()?;
        Some(match self.gap(t) {
            g if g < 6 * 3600 => format!("We were on {topic}."),
            g if g < 48 * 3600 => format!("Last time we were on {topic}."),
            _ => format!("It's been a while. We were on {topic}."),
        })
    }

    /// The last thing discussed, for pronouns and follow-ups.
    pub fn last(&self) -> Option<&Exchange> {
        self.recent.last()
    }

    /// Was this asked before? Repeating an answer verbatim is a small thing
    /// that makes an assistant feel like it isn't listening.
    pub fn asked_before(&self, said: &str) -> Option<&Exchange> {
        let n = normalize(said);
        if n.len() < 8 {
            return None;
        }
        self.recent.iter().rev().skip(1).find(|e| normalize(&e.said) == n)
    }
}

fn normalize(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub const FOLD_PROMPT: &str = "\
Compress this conversation into a short running summary. Keep decisions made,
things still open, names, numbers, dates, file paths, and anything asked to be
remembered. Drop pleasantries and
anything already resolved. Write it as notes, not prose. Under 200 words.";

pub fn now_secs() -> u64 {
    now()
}

/// Lines from the exchanges that must survive any summary: anything asked to
/// be remembered, decided, promised, dated or counted. Checked against what
/// the model wrote, and added back if it left them out.
pub fn must_keep(exchanges: &[Exchange]) -> Vec<String> {
    const MARKS: &[&str] = &[
        "remember", "don't forget", "dont forget", "decided", "we'll go with", "let's go with", "i'll ", "promise",
        "deadline", "due ", "by friday", "by monday", "tomorrow", "next week", "password hint", "always ", "never ",
    ];
    let mut out = Vec::new();
    for e in exchanges {
        let l = e.said.to_lowercase();
        let has_number = e.said.chars().any(|c| c.is_ascii_digit());
        if MARKS.iter().any(|m| l.contains(m)) || has_number {
            let line = e.said.trim().to_string();
            if !line.is_empty() && !out.contains(&line) {
                out.push(line);
            }
        }
    }
    out
}

/// The model's summary with anything important it dropped added back.
pub fn with_the_important_kept(summary: &str, exchanges: &[Exchange], earlier: &str) -> String {
    let lower = summary.to_lowercase();
    let missing: Vec<String> = must_keep(exchanges)
        .into_iter()
        .filter(|line| {
            // Kept if its distinctive words are in the summary.
            let words: Vec<String> = line
                .to_lowercase()
                .split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.len() > 3 || w.chars().any(|c| c.is_ascii_digit()))
                .map(str::to_string)
                .collect();
            let hits = words.iter().filter(|w| lower.contains(w.as_str())).count();
            words.is_empty() || (hits as f32) < (words.len() as f32) * 0.6
        })
        .collect();
    let mut out = summary.trim().to_string();
    if out.is_empty() {
        out = earlier.trim().to_string();
    }
    if !missing.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("Said along the way: {}", missing.join(" / ")));
    }
    out
}

/// A summary without a model: what it was about, and the important lines.
/// Better than "12 earlier exchanges", which kept nothing.
pub fn plain_fold(earlier: &str, exchanges: &[Exchange]) -> String {
    let mut topics: Vec<String> = Vec::new();
    for e in exchanges {
        if let Some(a) = &e.about {
            if !topics.contains(a) {
                topics.push(a.clone());
            }
        }
    }
    let mut out = earlier.trim().to_string();
    if !topics.is_empty() {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(&format!("Talked about {}.", topics.join(", ")));
    }
    with_the_important_kept(&out, exchanges, earlier)
}
