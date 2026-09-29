//! Talking a problem through in a window.
//!
//! Atlas has already tried everything on its ladder. Now it takes the write-up
//! into a conversation that's already open and works through it — sending,
//! waiting, reading, and following up until something testable comes back.
//!
//! Two rules make this behave like a person rather than a script:
//!
//! 1. **Wait for the whole answer.** A reply that's still arriving looks like
//!    a short reply. Acting on half of one is how you end up applying the
//!    first paragraph of a two-paragraph fix.
//! 2. **A turn is spent on a solution, not on a sentence.** A question back, a
//!    request for more detail, an explanation with no code — none of those cost
//!    an attempt. Atlas reads them, works out what's being asked, answers, and
//!    carries on. The budget is only touched when there's something to test.

use crate::handoff::{extract_blocks, Block};
use serde::{Deserialize, Serialize};

/// What came back.
#[derive(Debug, Clone, PartialEq)]
pub enum Reply {
    /// Still arriving. Do nothing yet.
    StillComing,
    /// Something testable. This is the only kind that costs an attempt.
    Solution(Vec<Block>),
    /// A question. Answer it and carry on.
    Question(String),
    /// An explanation, or a next step to take, but nothing to apply yet.
    Guidance(String),
    /// It's asking for something Atlas hasn't given it.
    WantsMore(String),
}

impl Reply {
    /// Does acting on this use up one of the attempts?
    pub fn costs_an_attempt(&self) -> bool {
        matches!(self, Reply::Solution(_))
    }
    pub fn is_ready(&self) -> bool {
        !matches!(self, Reply::StillComing)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ConsultConfig {
    /// The text has to stop changing for this long before it counts as
    /// finished. A streaming reply looks like a short reply otherwise.
    pub settle_ms: u64,
    /// Give up waiting after this. A reply that never lands is a stuck
    /// window, not a slow one.
    pub reply_timeout_secs: u64,
    /// Exchanges — messages either way — before Atlas stops, regardless of
    /// how many were solutions. Stops a loop that's going nowhere politely.
    pub max_exchanges: u32,
    /// The first thing Atlas says. Sets the context and asks for what it
    /// actually needs.
    pub opening: String,
}

impl Default for ConsultConfig {
    fn default() -> Self {
        ConsultConfig {
            settle_ms: 2500,
            reply_timeout_secs: 180,
            max_exchanges: 20,
            opening: "Pause all other tasks. I ran into an issue while running diagnostics — \
                      the details are in the write-up below. I've already tried a number of \
                      approaches; they're listed with what each one showed. What I need is the \
                      change itself so I can apply it and run the tests."
                .into(),
        }
    }
}

/// Is the reply finished, or still arriving?
///
/// Judged on the text having stopped changing rather than on any end marker,
/// because there isn't a reliable one.
pub fn settled(previous: &str, current: &str, unchanged_for_ms: u64, cfg: &ConsultConfig) -> bool {
    if current.trim().is_empty() {
        return false;
    }
    if previous != current {
        return false;
    }
    unchanged_for_ms >= cfg.settle_ms
}

/// Work out what kind of reply this is.
///
/// Ordering matters: code beats everything, because a reply that explains
/// *and* gives the change is a solution, not guidance.
pub fn classify(text: &str) -> Reply {
    let blocks = extract_blocks(text);
    if !blocks.is_empty() {
        return Reply::Solution(blocks);
    }
    let t = text.trim();
    if t.is_empty() {
        return Reply::StillComing;
    }
    let lower = t.to_lowercase();

    // Asking for something Atlas can actually supply.
    const ASKS_FOR: &[&str] = &[
        // Requests to *see* something Atlas can paste. A plain question
        // ("which version are you on?") is a Question — it gets answered, and
        // neither kind costs an attempt.
        "can you show", "can you paste", "could you share", "send me",
        "let me see", "what's in", "whats in", "paste the", "show me the",
    ];
    if ASKS_FOR.iter().any(|a| lower.contains(a)) {
        return Reply::WantsMore(first_question(t).unwrap_or_else(|| t.to_string()));
    }
    if let Some(q) = first_question(t) {
        return Reply::Question(q);
    }
    Reply::Guidance(t.to_string())
}

fn first_question(t: &str) -> Option<String> {
    t.split_inclusive(['.', '!', '?'])
        .map(str::trim)
        .find(|s| s.ends_with('?'))
        .map(|s| s.to_string())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Exchange {
    pub sent: String,
    pub received: String,
    /// Did this one produce something testable?
    pub was_a_solution: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Consultation {
    pub problem: String,
    pub exchanges: Vec<Exchange>,
    /// Attempts spent — solutions tested, not messages sent.
    pub attempts_used: u32,
    pub finished: Option<String>,
}

/// What Atlas should do next in the conversation.
#[derive(Debug, Clone, PartialEq)]
pub enum Move {
    /// Open with this, and attach the write-up.
    Open { message: String, attach: String },
    /// Keep waiting.
    Wait,
    /// Say this back.
    Reply(String),
    /// Apply and test these. Costs an attempt.
    Test(Vec<Block>),
    /// Stop.
    Stop(String),
}

impl Consultation {
    pub fn new(problem: &str) -> Consultation {
        Consultation { problem: problem.to_string(), ..Default::default() }
    }

    /// The opening message, with the write-up attached.
    pub fn open(&self, brief: &str, cfg: &ConsultConfig) -> Move {
        Move::Open { message: cfg.opening.clone(), attach: brief.to_string() }
    }

    pub fn record(&mut self, sent: &str, received: &str, was_a_solution: bool) {
        if was_a_solution {
            self.attempts_used += 1;
        }
        self.exchanges.push(Exchange {
            sent: sent.to_string(),
            received: received.to_string(),
            was_a_solution,
        });
    }

    /// Decide what to do with what came back.
    ///
    /// `attempts_left` is the budget from the strategy ladder. Only a solution
    /// draws on it.
    pub fn next(&self, reply: &Reply, attempts_left: u32, cfg: &ConsultConfig) -> Move {
        if let Some(why) = &self.finished {
            return Move::Stop(why.clone());
        }
        // A conversation going nowhere politely is still going nowhere.
        if self.exchanges.len() as u32 >= cfg.max_exchanges {
            return Move::Stop(format!("{} exchanges without getting there", self.exchanges.len()));
        }

        match reply {
            Reply::StillComing => Move::Wait,

            Reply::Solution(blocks) => {
                if attempts_left == 0 {
                    return Move::Stop("out of attempts to test with".into());
                }
                Move::Test(blocks.clone())
            }

            // None of these cost an attempt. Atlas reads, works out what's
            // wanted, and answers.
            Reply::WantsMore(what) => Move::Reply(format!(
                "Here's what you asked for. {what} — I'll gather that and paste it. \
                 If there's anything else you need to see, say so and I'll get it."
            )),
            Reply::Question(q) => Move::Reply(format!(
                "Answering that: {q} — I'll check and come back. \
                 If you'd rather I just try something, tell me what to change and I'll test it."
            )),
            Reply::Guidance(_) => Move::Reply(
                "That makes sense. What should I change, specifically? \
                 Give me the replacement and I'll apply it and run the tests."
                    .into(),
            ),
        }
    }

    /// How the last attempt went, fed back into the conversation.
    ///
    /// Reporting the result is what makes it a conversation rather than a
    /// series of guesses.
    pub fn report_result(&self, passed: bool, output: &str) -> String {
        if passed {
            format!("That worked — all tests pass. {}", first_line(output))
        } else {
            format!(
                "Applied it, still failing. Here's what came back:\n\n```\n{}\n```\n\nWhat next?",
                crate::handoff::trim_middle(output, 1200)
            )
        }
    }

    /// Messages sent that produced nothing testable. Worth knowing — a
    /// conversation that's all discussion isn't working.
    pub fn unproductive(&self) -> usize {
        self.exchanges.iter().filter(|e| !e.was_a_solution).count()
    }

    pub fn summary(&self) -> String {
        if self.exchanges.is_empty() {
            return "Haven't asked yet.".into();
        }
        format!(
            "{} exchanges, {} of them produced something to test.",
            self.exchanges.len(),
            self.attempts_used
        )
    }
}

fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").trim().to_string()
}
