//! Working out what you want back.
//!
//! Someone tells you a problem. There are three useful things you can do, and
//! doing the wrong one is most of what makes people bad at this:
//!
//! * **Solve it** — right when they asked, wrong when they didn't.
//! * **Hear it** — right when they're working something out, insulting when
//!   they wanted an answer and got sympathy.
//! * **Both, in order** — hear it first, then offer.
//!
//! Atlas reads which from how you said it, and **asks when it can't tell**
//! rather than guessing. Asking costs one sentence; guessing wrong costs the
//! conversation.
//!
//! Two things this is not. It doesn't infer how you feel — it reads what
//! response you're asking for, which is a different and much more legible
//! thing. And listening is not agreeing: Atlas will still say when it thinks
//! you're wrong, because a system that only reflects you back is worse than
//! useless when you need to be told something.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Wanted {
    /// Ideas, options, a plan.
    Solutions,
    /// To be understood, not fixed.
    Hearing,
    /// Understood first, then options.
    Both,
    /// Can't tell — ask.
    Unclear,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Reading {
    pub wanted: Wanted,
    /// 0 to 1.
    pub confidence: f32,
    /// What in the words said so, so you can correct it.
    pub because: String,
}

/// Said outright. Nothing beats being told.
const SAYS_LISTEN: &[&str] = &[
    "just listen", "don't fix", "dont fix", "not looking for advice",
    "don't need solutions", "dont need solutions", "just need to vent",
    "just venting", "let me get this out", "hear me out", "no advice",
    "not asking you to fix", "just needed to say",
];

const SAYS_SOLVE: &[&str] = &[
    "what should i", "what do i do", "any ideas", "how do i", "help me",
    "what would you", "give me options", "fix this", "sort this out",
    "talk me through", "what's the play", "whats the play", "how would you",
];

/// Marks of working something out rather than asking something.
const THINKING_ALOUD: &[&str] = &[
    "i can't believe", "i cant believe", "i don't even know", "i dont even know",
    "it's just", "its just", "the worst part", "and then", "on top of that",
    "i'm so", "im so", "i've had it", "ive had it", "again", "every time",
];

/// A question that's really a question.
///
/// A question mark counts, but so does how it opens: typed and spoken
/// questions mostly come without one ("how are you", "what's the capital of
/// France"), and reading those as "can't tell" made Atlas answer ordinary
/// questions with "do you want me to think about it with you, or just
/// listen?" — and then take the real answer as the reply to that (26 Sep 2026).
fn asks_something(s: &str) -> bool {
    let t = s.trim().to_lowercase();
    if ["right?", "you know?", "isn't it?", "no?"].iter().any(|f| t.ends_with(f)) {
        return false;
    }
    t.ends_with('?') || opens_as_a_request(&t)
}

/// Is this a question or a request, going by how it's put?
pub fn is_a_question(said: &str) -> bool {
    said.split_inclusive(['.', '!', '?']).any(asks_something)
}

/// Opens the way a question or a request does.
fn opens_as_a_request(t: &str) -> bool {
    const OPENERS: &[&str] = &[
        "what", "what's", "whats", "who", "who's", "whos", "why", "how", "when", "where", "which", "is", "are",
        "can", "could", "would", "will", "do", "does", "did", "should", "tell", "give", "show", "explain",
        "recommend", "suggest", "summarise", "summarize", "describe", "name", "list", "write", "find",
    ];
    let first = t.split_whitespace().next().unwrap_or("").trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
    OPENERS.contains(&first)
}

pub fn read(said: &str) -> Reading {
    let t = said.to_lowercase();

    // Being told outright beats every other signal.
    if let Some(m) = SAYS_LISTEN.iter().find(|m| t.contains(**m)) {
        return Reading {
            wanted: Wanted::Hearing,
            confidence: 0.97,
            because: format!("you said \"{m}\""),
        };
    }
    if let Some(m) = SAYS_SOLVE.iter().find(|m| t.contains(**m)) {
        return Reading {
            wanted: Wanted::Solutions,
            confidence: 0.93,
            because: format!("you asked \"{m}\""),
        };
    }

    let words = t.split_whitespace().count();
    let questions = said.split_inclusive(['.', '!', '?']).filter(|s| asks_something(s)).count();
    let venting = THINKING_ALOUD.iter().filter(|m| t.contains(**m)).count();

    // A direct question wants an answer.
    if questions > 0 && words < 40 {
        return Reading {
            wanted: Wanted::Solutions,
            confidence: 0.8,
            because: "you asked a question".into(),
        };
    }

    // A long stretch with no question in it is usually someone working
    // something out.
    if words > 45 && questions == 0 {
        return Reading {
            wanted: if venting >= 2 { Wanted::Hearing } else { Wanted::Both },
            confidence: if venting >= 2 { 0.75 } else { 0.55 },
            because: if venting >= 2 {
                "it reads like you're getting it out rather than asking".into()
            } else {
                "you told me a lot and didn't ask anything".into()
            },
        };
    }

    if venting >= 2 {
        return Reading {
            wanted: Wanted::Hearing,
            confidence: 0.7,
            because: "the way it's phrased".into(),
        };
    }

    // Short, no question, no strong markers: genuinely ambiguous.
    Reading {
        wanted: Wanted::Unclear,
        confidence: 0.3,
        because: "I can't tell from how you said it".into(),
    }
}

/// The question Atlas asks when it can't tell.
///
/// Short, offers both, and doesn't make a thing of it. A long careful question
/// about whether you'd like emotional support is itself a kind of imposition.
pub fn ask_which() -> &'static str {
    "Do you want me to think about it with you, or just listen?"
}

/// Your answer to that.
pub fn answer_to_ask(said: &str) -> Option<Wanted> {
    // By whole words (2 Oct 2026): "open notepad" holds "no" and was taken
    // as "just listen", so the request after the question was lost -- and
    // "know", "note", "nothing to do with it" the same way.
    let t = format!(" {} ", crate::intent::normalize(said));
    let has = |w: &&str| t.contains(&format!(" {w} "));
    if ["listen", "just listen", "nothing", "no", "neither", "vent", "nope"].iter().any(has) {
        return Some(Wanted::Hearing);
    }
    if ["think", "thinking", "think it through", "ideas", "solve", "options", "yes", "help", "fix"].iter().any(has) {
        return Some(Wanted::Solutions);
    }
    if ["both", "either", "up to you", "whatever"].iter().any(has) {
        return Some(Wanted::Both);
    }
    None
}

// ---------- what Atlas actually says ----------

/// Reflecting back what was said.
///
/// The thing that makes someone feel heard is **accurate understanding**, not
/// sympathy noises. So this restates the substance and gets it right or gets
/// corrected — it never says "that sounds hard", which is what a system says
/// when it has understood nothing.
pub fn heard(substance: &str) -> String {
    format!("So — {substance}.")
}

/// Having listened, offering rather than launching in.
///
/// The offer comes after, and it is an offer. Following "I hear you" with
/// three bullet points is not listening with extra steps, it's ignoring what
/// was asked for.
pub fn then_offer() -> &'static str {
    "Want me to think about any of it, or leave it there?"
}

/// What Atlas must not do while listening, kept as a rule rather than a hope.
///
/// Listening is not agreeing. If Atlas thinks you've got something wrong it
/// still says so — a system that only reflects you back is worse than useless
/// on the day you need telling.
pub const STILL_HONEST: &str =
    "Listening doesn't mean agreeing. If I think you've got something wrong I'll say so — \
     just not while you're still saying it.";

/// Phrases that mean Atlas has understood nothing and is filling space.
pub const EMPTY_SYMPATHY: &[&str] = &[
    "that sounds really hard",
    "i'm so sorry to hear that",
    "that must be frustrating",
    "i can only imagine",
    "sending you strength",
    "you've got this",
];

/// Is a reply just sympathy noises?
pub fn is_empty_sympathy(reply: &str) -> bool {
    let t = reply.to_lowercase();
    EMPTY_SYMPATHY.iter().any(|p| t.contains(p))
}

// ---------- learning which you usually want ----------

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Preferences {
    /// Topic to what you wanted, and how many times.
    pub by_topic: Vec<(String, Wanted, u32)>,
}

impl Preferences {
    pub fn note(&mut self, topic: &str, wanted: Wanted) {
        match self.by_topic.iter_mut().find(|(t, w, _)| t == topic && *w == wanted) {
            Some((_, _, n)) => *n += 1,
            None => self.by_topic.push((topic.into(), wanted, 1)),
        }
    }

    /// What you usually want when this comes up.
    ///
    /// Only used to skip the question, never to override what you actually
    /// said this time.
    pub fn usual(&self, topic: &str) -> Option<Wanted> {
        let mut best: Option<(Wanted, u32)> = None;
        let mut total = 0;
        for (t, w, n) in &self.by_topic {
            if t == topic {
                total += n;
                if best.map(|(_, b)| n > &b).unwrap_or(true) {
                    best = Some((*w, *n));
                }
            }
        }
        // Two of the same isn't a pattern; three is.
        best.filter(|(_, n)| *n >= 3 && *n * 2 > total).map(|(w, _)| w)
    }
}

/// The `wanted:` block of tools.yaml (Settings → How it talks back →
/// Clarifying questions).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq)]
#[serde(default)]
pub struct WantedConfig {
    /// Ask "ideas, or just to be heard?" when it cannot tell. Off: it never
    /// asks, and an unclear message goes on to the normal answer.
    pub ask_when_unclear: bool,
}

impl Default for WantedConfig {
    fn default() -> Self {
        WantedConfig { ask_when_unclear: true }
    }
}

/// Put it together: read the words, fall back to what you usually want, ask if
/// neither settles it.
pub fn decide(said: &str, topic: &str, prefs: &Preferences) -> (Wanted, Option<&'static str>) {
    let r = read(said);
    // What you said this time always wins over what you usually want.
    if r.confidence >= 0.7 {
        return (r.wanted, None);
    }
    if let Some(usual) = prefs.usual(topic) {
        return (usual, None);
    }
    if r.wanted == Wanted::Unclear {
        return (Wanted::Unclear, Some(ask_which()));
    }
    (r.wanted, None)
}
