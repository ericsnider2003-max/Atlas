//! Reading the room.
//!
//! Atlas shouldn't answer "what's the plan for the quarter" and "what did you
//! think of that film" in the same voice. Nor should it drop a formal
//! transparency notice into a call with two friends.
//!
//! So before it decides *what* to say, it works out what kind of moment this
//! is. Everything downstream — length, tone, whether a joke is welcome —
//! keys off that.

use serde::{Deserialize, Serialize};

/// What sort of exchange this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Register {
    /// A task. Get on with it.
    Working,
    /// Talking, not tasking. Longer answers are welcome; so is an opinion.
    Chatting,
    /// A question about Atlas itself.
    AboutAtlas,
    /// Frustrated, tired, or something went wrong. Not a moment to be clever.
    Rough,
}

/// How many sentences a spoken conversational answer stops at, unless you
/// asked for something long (`asks_for_length`).
pub const CHAT_SENTENCES: usize = 4;

/// Did they ask for something that runs long -- a story, a poem, a list,
/// detail, steps?
pub fn asks_for_length(said: &str) -> bool {
    let t = format!(" {} ", said.to_lowercase());
    [
        " story", " poem", " song", " list", " detail", " explain", " steps", " step by step", " walk me through",
        " tell me about", " tell me more", " in depth", " everything", " ideas", " options", " examples", " compare",
        " summar", " write ", " draft ",
    ]
    .iter()
    .any(|w| t.contains(w))
}

impl Register {
    /// Sentences before it should stop.
    pub fn length(&self) -> usize {
        match self {
            Register::Working => 2,
            Register::AboutAtlas => 4,
            // A conversation cut to two sentences is not a conversation.
            Register::Chatting => 8,
            Register::Rough => 3,
        }
    }

    /// The most the model may write for a reply in this register, in
    /// tokens. A hard stop, set well past the sentence count the model is
    /// asked for, so an answer ends where it chose to rather than being cut
    /// mid-thought (27 Sep 2026: a poem got two sentences).
    pub fn max_tokens(&self) -> u32 {
        match self {
            Register::Working => 160,
            Register::Chatting => 450,
            Register::AboutAtlas => 250,
            Register::Rough => 200,
        }
    }

    /// Is humour welcome here?
    pub fn humour_welcome(&self) -> bool {
        matches!(self, Register::Chatting)
    }

    /// Should Atlas volunteer an opinion rather than waiting to be asked?
    pub fn opinions_welcome(&self) -> bool {
        matches!(self, Register::Chatting | Register::AboutAtlas)
    }
}

/// What Atlas knows about the moment, beyond the words.
#[derive(Debug, Clone, Default)]
pub struct Moment {
    /// The last few things said, oldest first.
    pub recent: Vec<String>,
    /// Something just failed.
    pub after_a_failure: bool,
    /// A task is running.
    pub busy: bool,
}

/// Work out the register from what was said and what's going on.
pub fn read(said: &str, m: &Moment) -> Register {
    let t = normalise(said);

    // Frustration first. Nothing else matters if the last thing that happened
    // went wrong.
    if m.after_a_failure || sounds_fed_up(&t) {
        return Register::Rough;
    }

    // A question about Atlas — judged by what it's asking about, not by the
    // presence of the word "you". "Can you open Chrome" is a task.
    if about_atlas(&t) {
        return Register::AboutAtlas;
    }

    // Something to write or explain is a conversation even when it opens
    // with a task verb: "write me a poem" wants the room a poem needs, not
    // two sentences (27 Sep 2026).
    if wants_prose(&t) {
        return Register::Chatting;
    }

    if is_a_task(&t) {
        return Register::Working;
    }

    // Short fragments during work are almost always task-related.
    if m.busy && t.split_whitespace().count() <= 4 {
        return Register::Working;
    }

    Register::Chatting
}

fn normalise(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace() || *c == '\'')
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

const TASK_VERBS: &[&str] = &[
    "open", "close", "focus", "move", "boot", "start", "stop", "run", "research",
    "find", "search", "read", "write", "draft", "send", "schedule", "post",
    "cancel", "show", "make", "put", "type", "paste", "switch", "shut", "turn",
    "back up", "undo", "record", "edit", "index",
];

fn is_a_task(t: &str) -> bool {
    let first = t.split_whitespace().next().unwrap_or("");
    // "find me a good book on options", "show me how margin works": asking to
    // be told or shown something is a question, not a task (30 Sep 2026:
    // those got two sentences and the shortest reply window).
    let asks_to_be_shown = ["show me how", "show me what", "show me why", "find me a good", "find me some", "tell me"]
        .iter()
        .any(|p| t.starts_with(p))
        // "find me a book" is asking for a suggestion; "find me the lease"
        // or "find me a file" is a search of your things.
        || ((t.starts_with("find me a ") || t.starts_with("find me an "))
            && !["file", "document", "doc", "pdf", "folder", "email", "mail", "note", "photo", "picture", "spreadsheet"]
                .iter()
                .any(|w| t.split_whitespace().any(|x| x.trim_end_matches('s') == *w)));
    if TASK_VERBS.contains(&first) && !asks_to_be_shown {
        return true;
    }
    // "can you open chrome" is a task wearing a question's clothes.
    for lead in ["can you ", "could you ", "would you ", "please "] {
        if let Some(rest) = t.strip_prefix(lead) {
            let v = rest.split_whitespace().next().unwrap_or("");
            if TASK_VERBS.contains(&v) {
                return true;
            }
        }
    }
    false
}

/// Is this a question about Atlas, rather than a question containing "you"?
///
/// The distinction that matters: "what do you do" is about Atlas. "what do you
/// think of this draft" is a request for an opinion on something else.
fn about_atlas(t: &str) -> bool {
    const SUBJECTS: &[&str] = &[
        "what do you do", "what can you do", "what are you", "who are you",
        "how do you work", "what's this thing", "whats this thing",
        "are you recording", "are you listening", "what are you doing",
        "why did you", "how did you", "can you explain why", "what did you just",
        "are you an ai", "what model", "how are you built", "what happens to what i say",
        "what happens to my data", "what happens to my recordings",
        "where does that go", "do you store", "is that private",
    ];
    SUBJECTS.iter().any(|s| t.contains(s))
}

/// Asked to write, tell or explain something: the answer is prose.
fn wants_prose(t: &str) -> bool {
    const ASKS: &[&str] = &[
        "poem", "story", "joke", "song", "lyrics", "essay", "haiku", "limerick", "letter to",
        "speech", "explain", "describe", "tell me about", "tell me a", "what do you think",
        "ideas for", "brainstorm", "summarise", "summarize", "compare",
    ];
    // Not "write a script" or "draft a post": those are Atlas's own tasks.
    const TASKS: &[&str] = &["post", "email", "script", "function", "program", "code", "reply"];
    ASKS.iter().any(|a| t.contains(a)) && !TASKS.iter().any(|w| t.split_whitespace().any(|x| x == *w))
}

fn sounds_fed_up(t: &str) -> bool {
    // "again" alone was here, so "say that again" and "tell me again" read as
    // frustration. It counts next to something having gone wrong.
    const SIGNS: &[&str] = &[
        "that didn't work", "that didnt work", "still broken", "not working again", "wrong again", "failed again", "why isn't",
        "why isnt", "this is broken", "forget it", "useless", "for god's sake",
        "for gods sake", "not again", "you keep",
    ];
    // "Seriously" and "come on" said on their own or with a complaint are
    // fed up; "seriously, what's the best broker?" is emphasis (30 Sep 2026:
    // that got a three-sentence "no jokes" answer).
    let alone = ["come on", "seriously", "oh come on", "seriously atlas", "come on atlas"].iter().any(|s| t.trim() == *s);
    let with_complaint = (t.starts_with("come on") || t.starts_with("seriously"))
        && ["wrong", "broken", "didn't", "didnt", "not what", "again", "stop"].iter().any(|w| t.contains(w));
    alone || with_complaint || SIGNS.iter().any(|s| t.contains(s))
}

// ---------- calls ----------

/// How formal a call is, which decides what Atlas says when it joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Formality {
    /// Colleagues, clients, anyone you'd write an email to.
    Business,
    /// Friends and family.
    Casual,
    /// Not enough to go on.
    Unsure,
}

/// What Atlas can see about a call before it starts.
#[derive(Debug, Clone, Default)]
pub struct CallContext {
    pub title: String,
    /// Email addresses or names of the other participants.
    pub participants: Vec<String>,
    /// Domains that are yours — anyone else is external.
    pub my_domains: Vec<String>,
    /// Minutes past midnight, local.
    pub minutes_of_day: u32,
    pub weekday: u32,
    /// It came from a calendar entry rather than an ad-hoc link.
    pub from_calendar: bool,
}

/// Judge the formality.
///
/// Deliberately biased toward Business when unsure: sounding a little formal
/// with a friend is mildly awkward, and sounding casual with a client is
/// worse.
pub fn formality(c: &CallContext) -> Formality {
    let title = c.title.to_lowercase();
    let mut business = 0i32;
    let mut casual = 0i32;

    const BUSINESS_WORDS: &[&str] = &[
        "meeting", "standup", "stand-up", "sync", "review", "interview", "client",
        "kickoff", "kick-off", "onboarding", "1:1", "one on one", "quarterly",
        "sprint", "retro", "demo", "proposal", "contract", "board", "vendor",
    ];
    const CASUAL_WORDS: &[&str] =
        &["catch up", "catchup", "chat", "hangout", "hang out", "game", "coffee", "beers", "family"];

    business += BUSINESS_WORDS.iter().filter(|w| title.contains(**w)).count() as i32 * 2;
    casual += CASUAL_WORDS.iter().filter(|w| title.contains(**w)).count() as i32 * 2;

    // Someone outside your own domains is a strong business signal.
    let external = c
        .participants
        .iter()
        .filter(|p| p.contains('@'))
        .filter(|p| !c.my_domains.iter().any(|d| p.to_lowercase().ends_with(&d.to_lowercase())))
        .count();
    if external > 0 {
        business += 2;
    }
    // Several people is more likely a meeting than a chat.
    if c.participants.len() >= 4 {
        business += 1;
    } else if c.participants.len() == 1 {
        casual += 1;
    }

    if c.from_calendar {
        business += 1;
    }
    // Evenings and weekends lean casual.
    let evening = c.minutes_of_day >= 19 * 60 || c.minutes_of_day < 8 * 60;
    let weekend = c.weekday >= 5;
    if evening {
        casual += 1;
    }
    if weekend {
        casual += 1;
    }

    match business - casual {
        d if d >= 2 => Formality::Business,
        d if d <= -2 => Formality::Casual,
        _ => Formality::Unsure,
    }
}

/// The announcement that fits.
pub fn announcement_for(f: Formality) -> &'static str {
    match f {
        Formality::Business => {
            "For transparency: this call is being transcribed by an assistant so I can write up \
             notes afterwards. Please let me know if you'd prefer I turn it off."
        }
        Formality::Casual => {
            "FYI I'm having an assistant take notes so I don't have to type while we talk. \
             Happy to switch it off."
        }
        // Unsure leans formal: awkward beats inappropriate.
        Formality::Unsure => {
            "Heads up — I've got an assistant taking notes on this call. Say if you'd rather \
             I didn't."
        }
    }
}
