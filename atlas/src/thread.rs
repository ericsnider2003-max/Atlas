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
        // Kept as it was said (merged 30 Sep 2026). The other chat took out
        // here any sentence copied from the last four replies; this chat
        // stops those sentences before they are said (`repeating::
        // SentenceFilter`, through `brain::SpeechGate` -- near copies and stock
        // closers too, and the reply kept is what was said), and shows the
        // model each past reply only as its first sentence or two, near
        // copies left out (`messages`). Doing it here as well rewrote replies
        // that were right to repeat ("Moved 3 files to Documents.").
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

    /// What to hand the model for folding: the notes so far, and what the
    /// user said since -- only the user (29 Sep 2026). With Atlas's replies
    /// in it, the model copied one into the summary ("User is frustrated,
    /// hey, if you want me to stop, I'll stop ... What's your next move?"),
    /// and the summary goes in front of every turn after, so the loop was
    /// fed back in forever.
    pub fn fold_input(&self, cfg: &ThreadConfig) -> String {
        let mut s = String::new();
        let replies: Vec<&str> = self.recent.iter().map(|e| e.reply.as_str()).collect();
        let summary = clean_summary(&self.summary, &replies);
        if !summary.trim().is_empty() {
            s.push_str("Notes so far:\n");
            s.push_str(summary.trim());
            s.push_str("\n\n");
        }
        s.push_str("What the user said since, in order:\n");
        for e in self.foldable(cfg) {
            if !e.said.trim().is_empty() {
                s.push_str(&format!("- {}\n", e.said.trim()));
            }
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
                .map(|e| e.said.len() + crate::repeating::for_history(&e.reply, HISTORY_SENTENCES).len())
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
        // The summary as it can be trusted: a summary written before 29 Sep
        // 2026 may hold Atlas's own reply boilerplate (`clean_summary`).
        let replies: Vec<&str> = self.recent.iter().map(|e| e.reply.as_str()).collect();
        let summary = clean_summary(&self.summary, &replies);
        if !summary.trim().is_empty() {
            out.push(Msg::system(format!("Earlier: {}", summary.trim())));
        }
        // Atlas's own past replies go back only as their first sentence or
        // two, without the stock closers, and a reply that says again what
        // an earlier one in the window said goes back not at all (29 Sep
        // 2026): shown its own loop twenty times, a small model copies it a
        // twenty-first. What was said is kept whole in the thread; this is
        // only what the model is shown.
        let mut shown: Vec<String> = Vec::new();
        for (i, e) in self.recent.iter().enumerate() {
            // An exchange with nothing said on either side is no turn at all.
            if self.folded + i < from || e.said.trim().is_empty() || e.reply.trim().is_empty() {
                continue;
            }
            // What whisper writes for a quiet room was never said (29 Sep 2026).
            if crate::voice::not_really_said(&e.said) {
                continue;
            }
            // Merged 30 Sep 2026: both chats stopped the model being shown its
            // own reply again and again. The other chat's way dropped an
            // earlier reply when the same one came later (exact match); this
            // one (below) keeps the first and leaves out later near copies
            // (`repeating::near_copy`), which also catches a copy with a
            // different first few words, and leaves the start of the prompt
            // as it was, so the model server can reuse what it already read.
            out.push(Msg::user(e.said.clone()));
            let short = crate::repeating::for_history(&e.reply, HISTORY_SENTENCES);
            if short.trim().is_empty() || shown.iter().any(|s| crate::repeating::near_copy(&short, s) || crate::repeating::near_copy(&e.reply, s)) {
                continue;
            }
            shown.push(short.clone());
            out.push(Msg::assistant(short));
        }
        out
    }

    /// Atlas's last `n` replies in full, oldest first: what a new reply is
    /// checked against for saying the same again (`brain::Turn::recent_replies`).
    pub fn recent_replies(&self, n: usize) -> Vec<String> {
        let start = self.recent.len().saturating_sub(n);
        self.recent[start..].iter().map(|e| e.reply.clone()).filter(|r| !r.trim().is_empty()).collect()
    }

    /// What the user is trying to get done, from what they said lately: the
    /// newest request among the last few exchanges ("I want you to organize
    /// my desktop"), in their words, or `None` when they haven't asked for
    /// anything lately. Kept in front of the model every turn, so a "that's
    /// it" or "this means to organize my desktop" is read as about the
    /// request, not as small talk (Eric's evening, 29 Sep 2026).
    ///
    /// Only among the requests `open` says are still open: the daemon passes
    /// "not one of my own commands", which were done when asked.
    pub fn current_goal_where(&self, open: impl Fn(&str) -> bool) -> Option<String> {
        self.recent.iter().rev().take(GOAL_LOOKBACK).find(|e| is_a_request(&e.said) && open(&e.said)).map(|e| {
            let s = e.said.split_whitespace().collect::<Vec<_>>().join(" ");
            if s.chars().count() > 160 { format!("{}…", s.chars().take(160).collect::<String>()) } else { s }
        })
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

    /// The last time exactly this was said, before it is written down as
    /// this turn (`asked_before` is asked after).
    pub fn said_earlier(&self, said: &str) -> Option<&Exchange> {
        let n = normalize(said);
        if n.len() < 8 {
            return None;
        }
        self.recent.iter().rev().find(|e| normalize(&e.said) == n)
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

/// How much of each past reply the model is shown (`Thread::messages`).
pub const HISTORY_SENTENCES: usize = 2;

/// How many exchanges back a request still counts as what the user is
/// trying to get done (`Thread::current_goal`).
pub const GOAL_LOOKBACK: usize = 6;

/// Does this read as asking for something to be done, rather than talk?
pub fn is_a_request(said: &str) -> bool {
    let t = crate::repeating::words(said).join(" ");
    if t.is_empty() {
        return false;
    }
    const ASKS: &[&str] = &[
        "i want you to", "i need you to", "i asked you to", "can you", "could you", "would you", "will you",
        "please", "go and", "try to", "you need to", "i want to", "help me", "i'd like you to", "id like you to",
    ];
    const DOING: &[&str] = &[
        "organize", "organise", "tidy", "clean", "sort", "look", "check", "open", "close", "use", "set", "find",
        "make", "do", "put", "generate", "run", "listen", "switch", "show", "tell", "fix", "install", "start",
        "stop", "send", "write", "draft", "remind", "move", "play", "read", "search", "research", "diagnose",
    ];
    let first = t.split(' ').next().unwrap_or("");
    ASKS.iter().any(|a| format!(" {t} ").contains(&format!(" {a} "))) || DOING.contains(&first)
}

/// Lines of a summary that are not notes about the user: Atlas's own reply
/// boilerplate copied in, editorialising about how the conversation went or
/// how the user feels, and any line sharing a long run of words with one of
/// `replies` (29 Sep 2026: Eric's summary ended "User is frustrated, hey, if
/// you want me to stop, I'll stop ... Either way, I'm here."). Taken out a
/// line (or, in a one-paragraph summary, a sentence) at a time.
pub fn clean_summary(summary: &str, replies: &[&str]) -> String {
    let pieces: Vec<String> = if summary.lines().filter(|l| !l.trim().is_empty()).count() > 1 {
        summary.lines().map(str::to_string).collect()
    } else {
        crate::repeating::sentences(summary)
    };
    let kept: Vec<String> = pieces
        .into_iter()
        .filter(|l| !l.trim().is_empty())
        .filter(|l| !is_summary_noise(l, replies))
        .map(|l| l.trim_end().to_string())
        .collect();
    let joiner = if summary.lines().filter(|l| !l.trim().is_empty()).count() > 1 { "\n" } else { " " };
    kept.join(joiner)
}

/// One line of a summary that isn't a note about the user (`clean_summary`).
fn is_summary_noise(line: &str, replies: &[&str]) -> bool {
    let l = crate::repeating::words(line).join(" ");
    if l.is_empty() {
        return true;
    }
    if crate::repeating::carries_boilerplate(line) {
        return true;
    }
    const EDITORIAL: &[&str] = &[
        "user is frustrated", "user seems", "user was frustrated", "the user is frustrated", "user is annoyed",
        "user is upset", "atlas responded", "atlas replied", "atlas failed", "atlas kept", "atlas was",
        "atlas did not", "atlas didnt", "atlas could not", "atlas couldnt", "the assistant", "assistant responded",
        "non actionable", "no task execution", "no names dates", "recorded beyond",
    ];
    if EDITORIAL.iter().any(|m| l.contains(m)) {
        return true;
    }
    replies.iter().any(|r| crate::repeating::longest_shared_run(line, r) >= 6)
}

/// The model's new summary, if it can be trusted, else one made without it
/// (29 Sep 2026). A summary goes in front of every turn after it, so one
/// holding Atlas's replies or commentary is worse than a plain one: its
/// noise is taken out (`clean_summary`), and when that leaves nothing, or
/// took out more than it left, the plain summary (`plain_fold`) is used.
/// Either way what the user said that must survive is added back
/// (`with_the_important_kept`).
pub fn accepted_summary(model_summary: &str, exchanges: &[Exchange], earlier: &str) -> String {
    let replies: Vec<&str> = exchanges.iter().map(|e| e.reply.as_str()).collect();
    let earlier = clean_summary(earlier, &replies);
    // Counted the way `clean_summary` takes them out: lines, or the
    // sentences of a one-paragraph summary.
    let units = |s: &str| {
        let lines = s.lines().filter(|l| !l.trim().is_empty()).count();
        if lines > 1 { lines } else { crate::repeating::sentences(s).len() }
    };
    let cleaned = clean_summary(model_summary, &replies);
    let before = units(model_summary);
    let after = if cleaned.trim().is_empty() { 0 } else { units(&cleaned) };
    if after == 0 || after * 2 < before {
        return plain_fold(&earlier, exchanges);
    }
    with_the_important_kept(&cleaned, exchanges, &earlier)
}

pub const FOLD_PROMPT: &str = "\
Update the running notes of a conversation with what the user said since. Keep only what the USER said: \
facts about them, decisions, requests still open, names, numbers, dates, file paths, and anything they asked to \
be remembered. Never quote or describe the assistant's replies, never describe the user's mood, no commentary. \
One note per line, each starting with \"- \". Under 120 words.";

pub fn now_secs() -> u64 {
    now()
}

/// Lines from the exchanges that must survive any summary: anything asked to
/// be remembered, decided, promised, dated or counted. Checked against what
/// the model wrote, and added back if it left them out.
///
/// And, from 29 Sep 2026, the user's requests (`is_a_request`) -- "I want
/// you to organize my desktop" -- the last `REQUESTS_KEPT` of them: open
/// requests are what the next turn most needs to know, and the summary
/// made without a model kept none of them.
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
    let requests: Vec<String> = exchanges.iter().filter(|e| is_a_request(&e.said)).map(|e| e.said.trim().to_string()).collect();
    for r in requests.iter().skip(requests.len().saturating_sub(REQUESTS_KEPT)) {
        if !r.is_empty() && !out.contains(r) {
            out.push(r.clone());
        }
    }
    out
}

/// How many of the user's requests a summary keeps word for word (`must_keep`).
pub const REQUESTS_KEPT: usize = 6;

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
