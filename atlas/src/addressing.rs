//! Was that meant for Atlas?
//!
//! "Stop when the user speaks" is too blunt. If Atlas is halfway through a
//! task and you take a phone call, or someone walks in, it should not abandon
//! the work because it heard a voice. Equally, if you genuinely say "stop",
//! it must stop.
//!
//! So speech is assessed, not just detected: is this directed at Atlas, and
//! how sure are we? Low confidence on a consequential decision goes back
//! through the confidence system and becomes a question.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Directed {
    /// Clearly aimed at Atlas.
    AtAtlas,
    /// Clearly someone else — a call, a person in the room, the TV.
    Overheard,
    /// Could be either.
    Unclear,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Command,
    Question,
    Statement,
    /// Too short or incomplete to be either.
    Fragment,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Assessment {
    pub directed: Directed,
    pub confidence: f32,
    pub kind: Kind,
    pub why: String,
}

/// What to do about a running task when speech arrives.
#[derive(Debug, Clone, PartialEq)]
pub enum Response {
    /// Confidently for Atlas — act on it, stand down from what you were doing.
    Act,
    /// Confidently not for Atlas — carry on working, say nothing.
    Ignore,
    /// Not sure, and abandoning the work would cost something. Ask.
    Ask(String),
}

/// Context that shifts the reading.
#[derive(Debug, Clone, Default)]
pub struct Situation {
    /// The wake word fired just before this.
    pub after_wake_word: bool,
    /// Atlas asked something and is waiting for the answer.
    pub awaiting_answer: bool,
    /// Atlas is mid-task; stopping wrongly costs work.
    pub working: bool,
    /// Names of people other than you that Atlas knows about.
    pub other_names: Vec<String>,
    /// Atlas spoke to you within the last `STILL_TALKING_SECS`: a follow-up
    /// is the conversation carrying on, so talking about "him" or "them" is
    /// not a sign it was meant for someone else.
    pub just_spoke: bool,
    /// The speaker check said this clearly wasn't your voice (`voiceid`'s
    /// `NotYou`, not its unsure band). Only consulted on the open floor:
    /// your name or the wake word still settles it, so a check that gets
    /// you wrong costs one "Atlas, ..." rather than locking you out.
    pub other_voice: bool,
}

/// How long after Atlas speaks a follow-up is still the same conversation.
pub const STILL_TALKING_SECS: u64 = 30;

pub fn assess(said: &str, s: &Situation) -> Assessment {
    let t = normalize(said);
    let words: Vec<&str> = t.split_whitespace().collect();
    let kind = classify(&t, &words);

    // The wake word settles it outright.
    if s.after_wake_word || t.starts_with("atlas") || t.contains(" atlas") {
        return Assessment {
            directed: Directed::AtAtlas,
            confidence: 0.98,
            kind,
            why: "you addressed it by name".into(),
        };
    }

    // Not your voice, and not said to Atlas by name: on the open floor this
    // is the television, the radio, or someone else in the room. It must
    // not answer Atlas's question for you either -- a stranger's "yes" is
    // not your approval (30 Sep 2026: the reply window answered anyone).
    if s.other_voice {
        return Assessment {
            directed: Directed::Overheard,
            confidence: 0.1,
            kind,
            why: "a voice that isn't yours, not said to Atlas by name".into(),
        };
    }

    // A pending question makes the next thing you say an answer.
    if s.awaiting_answer && !words.is_empty() {
        return Assessment {
            directed: Directed::AtAtlas,
            confidence: 0.9,
            kind,
            why: "it had just asked you something".into(),
        };
    }

    let mut score: f32 = 0.5;
    let mut reasons: Vec<&str> = Vec::new();

    if starts_with_any(&words, IMPERATIVES) {
        score += 0.25;
        reasons.push("starts with an instruction");
    }
    if starts_with_any(&words, SECOND_PERSON) {
        score += 0.2;
        reasons.push("addressed to you");
    }
    if kind == Kind::Command {
        score += 0.1;
    }

    // Signals it was somebody else -- except in the half minute after Atlas
    // spoke, when "what did he say then?" is a follow-up about what it just
    // told you. Those penalties dropped the follow-up as overheard and the
    // conversation ended in silence (27 Sep 2026).
    if s.just_spoke {
        score += 0.2;
        reasons.push("it had just spoken to you");
    } else {
        if contains_any(&t, THIRD_PARTY) {
            score -= 0.3;
            reasons.push("talking about someone else");
        }
        if contains_any(&t, PHONE_MARKERS) {
            score -= 0.35;
            reasons.push("sounds like another conversation");
        }
        for name in &s.other_names {
            if t.contains(&name.to_lowercase()) {
                score -= 0.3;
                reasons.push("names another person");
                break;
            }
        }
    }
    if kind == Kind::Fragment {
        score -= 0.2;
        reasons.push("only a fragment");
    }
    if words.len() > 25 {
        score -= 0.15;
        reasons.push("longer than a command");
    }

    let confidence = score.clamp(0.02, 0.98);
    let directed = if confidence >= 0.7 {
        Directed::AtAtlas
    } else if confidence <= 0.35 {
        Directed::Overheard
    } else {
        Directed::Unclear
    };

    Assessment {
        directed,
        confidence,
        kind,
        why: if reasons.is_empty() { "nothing either way".into() } else { reasons.join(", ") },
    }
}

/// Decide what a running task should do about it.
///
/// The asymmetry that matters: interrupting when you were not talking to Atlas
/// throws away work, and ignoring you when you were is merely annoying. So
/// while working, an unclear utterance is a question rather than a guess.
pub fn respond(a: &Assessment, s: &Situation) -> Response {
    match a.directed {
        Directed::AtAtlas => Response::Act,
        // Atlas asked you something and another voice answered: said once,
        // with how to answer if it was you after all, and nothing done.
        Directed::Overheard if s.other_voice && s.awaiting_answer => {
            Response::Ask("I didn't recognise that voice. If it was you, start with my name and answer again.".into())
        }
        Directed::Overheard if s.other_voice => Response::Ignore,
        // Mid-conversation, a follow-up is never dropped without a word: at
        // worst Atlas asks.
        Directed::Overheard if s.just_spoke => Response::Ask("Sorry -- was that for me?".into()),
        Directed::Overheard => Response::Ignore,
        Directed::Unclear => {
            if !s.working {
                // Nothing to lose; treat it as meant for Atlas.
                Response::Act
            } else {
                Response::Ask("Were you talking to me?".into())
            }
        }
    }
}

fn classify(_t: &str, words: &[&str]) -> Kind {
    if words.len() < 2 {
        return Kind::Fragment;
    }
    if starts_with_any(words, QUESTIONS) {
        return Kind::Question;
    }
    if starts_with_any(words, IMPERATIVES) {
        return Kind::Command;
    }
    Kind::Statement
}

const IMPERATIVES: &[&str] = &[
    "open", "close", "focus", "move", "boot", "start", "stop", "pause", "resume",
    "research", "find", "search", "read", "write", "draft", "send", "schedule",
    "cancel", "show", "tell", "give", "make", "put", "type", "paste", "look",
    "switch", "shut", "turn", "post", "reply", "undo", "back", "run", "check",
];

const SECOND_PERSON: &[&str] = &["can", "could", "would", "will", "please", "you"];

const QUESTIONS: &[&str] =
    &["what", "when", "where", "who", "why", "how", "is", "are", "do", "does", "did", "can"];

/// Talking *about* someone rather than to Atlas.
const THIRD_PARTY: &[&str] =
    &[" he ", " she ", " they ", " him ", " her ", " them ", " his ", " their "];

/// The shape of half a phone call.
const PHONE_MARKERS: &[&str] = &[
    "yeah no", "no yeah", "hold on let me", "i told him", "i told her", "she said",
    "he said", "they said", "i was like", "you know what i mean", "anyway so",
];

fn normalize(s: &str) -> String {
    let cleaned: String = s
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .collect();
    format!(" {} ", cleaned.split_whitespace().collect::<Vec<_>>().join(" "))
        .trim_start()
        .to_string()
}

fn starts_with_any(words: &[&str], set: &[&str]) -> bool {
    words.first().map(|w| set.contains(w)).unwrap_or(false)
}

fn contains_any(t: &str, set: &[&str]) -> bool {
    set.iter().any(|k| t.contains(k))
}
