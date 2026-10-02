//! A trading day's process, kept: a short check before the session and a
//! short journal after it -- about **you and your process**, never about a
//! trade.
//!
//! **Sources:** the pre-market checklist and post-session journal prompts
//! common to trading-journal practice (FX Replay's "5 journal prompts",
//! TradesViz on psychology tracking) were read for the shape: a handful of
//! yes/no questions on readiness and rule-following, a 1–5 state rating,
//! one free line. Kept to that. The day's scheduled releases come from
//! `marketdays`, as a schedule.
//!
//! **What it will never do.** No question asks about a position, a level or
//! a direction, and no summary reads one. The summary counts what you
//! answered -- days checked in, rules kept, the state you gave -- and ends,
//! with the line that says so:
//! `MEASUREMENTS. NO VERDICT. YOU DECIDE. · NOT FINANCIAL ADVICE`.

use serde::{Deserialize, Serialize};

pub const THE_LINE: &str = "MEASUREMENTS. NO VERDICT. YOU DECIDE. · NOT FINANCIAL ADVICE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum When {
    /// Before the session.
    Before,
    /// After it.
    After,
}

/// What a question takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Answer {
    YesNo,
    /// 1 to 5.
    Scale,
    Line,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Question {
    pub ask: String,
    pub takes: Answer,
    /// A "yes" is the good answer (for "any trade outside your plan?" it's
    /// "no"), so the summary can count kept rules the right way round.
    #[serde(default = "yes")]
    pub yes_is_good: bool,
}

fn yes() -> bool {
    true
}

fn q(ask: &str, takes: Answer, yes_is_good: bool) -> Question {
    Question { ask: ask.into(), takes, yes_is_good }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct TradeDayConfig {
    pub enabled: bool,
    /// Ask on its own, before the open and after the close, on trading
    /// days -- once each, at a moment Atlas may speak.
    pub prompt: bool,
    /// Minutes before the 9:30 New York open that the check-in is asked.
    pub before_minutes: u64,
    /// Minutes after the close that the journal is asked.
    pub after_minutes: u64,
    pub before: Vec<Question>,
    pub after: Vec<Question>,
}

impl Default for TradeDayConfig {
    fn default() -> Self {
        TradeDayConfig {
            enabled: true,
            prompt: true,
            before_minutes: 30,
            after_minutes: 5,
            before: vec![
                q("Slept well?", Answer::YesNo, true),
                q("Platform and connection healthy?", Answer::YesNo, true),
                q("Today's scheduled releases looked at?", Answer::YesNo, true),
                q("Rules read through?", Answer::YesNo, true),
                q("How sharp are you, 1 to 5?", Answer::Scale, true),
            ],
            after: vec![
                q("Kept to your rules?", Answer::YesNo, true),
                q("Anything done outside the plan?", Answer::YesNo, false),
                q("How are you now, 1 to 5?", Answer::Scale, true),
                q("One line: what to keep doing?", Answer::Line, true),
            ],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Given {
    Yes,
    No,
    Scale(u8),
    Line(String),
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    /// Local day number (days since 1970).
    pub day: i64,
    pub when: When,
    pub at: u64,
    pub answers: Vec<(String, Given)>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Journal {
    pub entries: Vec<Entry>,
}

/// The questions, said, numbered, with today's market schedule first when
/// there is one.
pub fn ask(questions: &[Question], when: When, schedule: &[String]) -> String {
    let mut out = Vec::new();
    out.push(match when {
        When::Before => "Before the session:".to_string(),
        When::After => "After the session:".to_string(),
    });
    for line in schedule {
        out.push(format!("  (schedule) {line}"));
    }
    for (i, qq) in questions.iter().enumerate() {
        let how = match qq.takes {
            Answer::YesNo => "yes/no",
            Answer::Scale => "1-5",
            Answer::Line => "a line",
        };
        out.push(format!("  {}. {} ({how})", i + 1, qq.ask));
    }
    out.push("Answer in one go -- \"yes yes no yes 4\" -- or \"skip\" for any.".into());
    out.join("\n")
}

/// Read the answers to `questions` from one reply. Yes/no words, digits for
/// a scale, and whatever's left over for a line question. `None` when the
/// reply doesn't hold an answer for every yes/no and scale question -- so a
/// stray sentence isn't recorded as a check-in.
pub fn read(questions: &[Question], reply: &str) -> Option<Vec<Given>> {
    let low = reply.to_lowercase();
    let mut tokens: Vec<String> = low
        .split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()).to_string())
        .filter(|t| !t.is_empty())
        .collect();
    // "1 yes 2 no": numbering is dropped when it counts the questions.
    let numbered: Vec<String> = (1..=questions.len()).map(|n| n.to_string()).collect();
    if tokens.len() >= 2 * questions.len() && tokens.iter().step_by(2).zip(&numbered).all(|(a, b)| a == b) {
        tokens = tokens.into_iter().skip(1).step_by(2).collect();
    }
    let mut out = Vec::new();
    let mut i = 0;
    for qq in questions {
        match qq.takes {
            Answer::Line => {
                let rest: Vec<&str> = reply.split_whitespace().skip(i).collect();
                let line = rest.join(" ");
                out.push(if line.is_empty() || line.eq_ignore_ascii_case("skip") { Given::Skipped } else { Given::Line(line) });
                i = tokens.len();
            }
            _ => {
                let t = tokens.get(i)?.as_str();
                let g = match (qq.takes, t) {
                    (_, "skip" | "pass" | "-") => Given::Skipped,
                    (Answer::YesNo, "yes" | "y" | "yeah" | "yep" | "done") => Given::Yes,
                    (Answer::YesNo, "no" | "n" | "nope" | "not") => Given::No,
                    (Answer::Scale, d) => match d.parse::<u8>() {
                        Ok(n) if (1..=5).contains(&n) => Given::Scale(n),
                        _ => return None,
                    },
                    _ => return None,
                };
                out.push(g);
                i += 1;
            }
        }
    }
    Some(out)
}

impl Journal {
    pub fn record(&mut self, day: i64, when: When, at: u64, questions: &[Question], given: Vec<Given>) {
        // One of each per day: a second check-in replaces the first.
        self.entries.retain(|e| !(e.day == day && e.when == when));
        self.entries.push(Entry { day, when, at, answers: questions.iter().map(|q| q.ask.clone()).zip(given).collect() });
        self.entries.sort_by_key(|e| (e.day, e.at));
    }

    pub fn has(&self, day: i64, when: When) -> bool {
        self.entries.iter().any(|e| e.day == day && e.when == when)
    }

    /// Counts over the last `days` days, ending with THE_LINE.
    pub fn summary(&self, cfg: &TradeDayConfig, today: i64, days: i64) -> String {
        let from = today - days + 1;
        let recent: Vec<&Entry> = self.entries.iter().filter(|e| e.day >= from && e.day <= today).collect();
        let before = recent.iter().filter(|e| e.when == When::Before).count();
        let after = recent.iter().filter(|e| e.when == When::After).count();
        let mut out = vec![format!("Last {days} days: checked in before {before} session{}, journalled after {after}.", if before == 1 { "" } else { "s" })];
        for (qs, when) in [(&cfg.before, When::Before), (&cfg.after, When::After)] {
            for qq in qs.iter() {
                let answers: Vec<&Given> = recent
                    .iter()
                    .filter(|e| e.when == when)
                    .filter_map(|e| e.answers.iter().find(|(a, _)| *a == qq.ask).map(|(_, g)| g))
                    .collect();
                match qq.takes {
                    Answer::YesNo => {
                        let yes = answers.iter().filter(|g| matches!(g, Given::Yes)).count();
                        let no = answers.iter().filter(|g| matches!(g, Given::No)).count();
                        if yes + no > 0 {
                            let good = if qq.yes_is_good { yes } else { no };
                            out.push(format!("  {} -- {good} of {} the way you meant it to go.", qq.ask, yes + no));
                        }
                    }
                    Answer::Scale => {
                        let v: Vec<u8> = answers.iter().filter_map(|g| if let Given::Scale(n) = g { Some(*n) } else { None }).collect();
                        if !v.is_empty() {
                            let mean = v.iter().map(|n| *n as f64).sum::<f64>() / v.len() as f64;
                            out.push(format!("  {} -- averaged {mean:.1} over {} answer{}.", qq.ask, v.len(), if v.len() == 1 { "" } else { "s" }));
                        }
                    }
                    Answer::Line => {}
                }
            }
        }
        out.push(THE_LINE.into());
        out.join("\n")
    }
}

/// Words a line here must never use: this is about process, not markets.
pub const NEVER_SAYS: &[&str] = &["buy", "sell", "long", "short", "entry", "target", "stop loss", "bullish", "bearish"];

/// The scheduled releases inside a day, said for the pre-market check
/// (why-stale idea 7, the general market desk, 1 Oct 2026): the ones that
/// move markets (impact 2 and 3) from `market::events`' checked tables, in
/// time order, at your clock. `None` when there are none, or the tables
/// refuse the month.
pub fn releases_in(events: &[crate::market::events::Event], from_ms: i64, to_ms: i64, offset_secs: i64) -> Option<String> {
    let mut today: Vec<&crate::market::events::Event> =
        events.iter().filter(|e| e.at >= from_ms && e.at < to_ms && e.impact >= 2).collect();
    if today.is_empty() {
        return None;
    }
    today.sort_by_key(|e| e.at);
    let said: Vec<String> = today
        .iter()
        .take(6)
        .map(|e| {
            let local = (e.at / 1000 + offset_secs).rem_euclid(86_400);
            let (h, m) = (local / 3600, (local % 3600) / 60);
            let (h12, ap) = match h {
                0 => (12, "am"),
                1..=11 => (h, "am"),
                12 => (12, "pm"),
                _ => (h - 12, "pm"),
            };
            format!("{} ({}) at {h12}:{m:02} {ap}{}", e.name, e.currency, if e.impact >= 3 { ", a big one" } else { "" })
        })
        .collect();
    Some(format!("Scheduled today: {}.", said.join("; ")))
}

/// "Why did Nvidia move today?", "why is the S&P down", "why did gold drop":
/// the thing asked about, for a dated research question. `None` when it isn't
/// that question.
pub fn why_it_moved(said: &str) -> Option<String> {
    let t = said.trim().trim_end_matches(['?', '.', '!']).to_ascii_lowercase();
    let t = t.trim_start_matches("atlas, ").trim_start_matches("atlas ");
    let rest = ["why did ", "why is ", "why's ", "why are "].iter().find_map(|p| t.strip_prefix(p))?;
    const MOVES: &[&str] = &[
        " move today", " moving today", " move", " moving", " up today", " down today", " up", " down", " drop today",
        " drop", " dropping", " jump today", " jump", " fall today", " fall", " falling", " rally", " rallying",
        " spike", " tank", " tanking", " crash", " sell off", " surge",
    ];
    let what = MOVES.iter().find_map(|m| rest.strip_suffix(m))?.trim().trim_start_matches("the ").trim();
    let what = what.strip_suffix(" stock").unwrap_or(what).trim();
    (!what.is_empty() && what.split_whitespace().count() <= 4).then(|| what.to_string())
}
