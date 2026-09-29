//! "Stop" when several things are running: which one did you mean?
//!
//! Eric's ruling (23 Sep 2026): stopping a single errand **pauses it and
//! keeps what it has done** — and when several errands are running, Atlas has
//! to work out which one "stop" was about, say which one it paused and which
//! ones carry on, and ask when it genuinely can't tell.
//!
//! The order it decides in, each step only if the one before settled nothing:
//!
//! 1. **Named.** "stop the research", "pause the backup", "hold the Postgres
//!    one" — a word of the errand's kind or of what it was asked about.
//! 2. **Numbered.** "the second one", "the last one" — oldest first, the
//!    order a list of them is read out in.
//! 3. **Only one.** Nothing to choose between.
//! 4. **The conversation.** What you were just talking about names exactly
//!    one of them.
//! 5. **Just started.** One of them began in the last minute and a half and
//!    none of the others did — "stop" right after asking is about that one.
//! 6. **Ask.** Name them, numbered, and wait for the answer.
//!
//! Steps 4 and 5 are guesses from context, so they are only ever used for a
//! pause (which loses nothing and is undone by "no, the other one"), and the
//! answer always says what it picked, why, and what is still going. Calling an
//! errand off for good is never decided from context: that needs a name, a
//! number, "all", or an answer to the question.


/// What was asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Hold where it is, lose nothing. What "stop" means for one errand.
    Pause,
    /// Carry on from where it held.
    Resume,
    /// End it for good. Only ever on an explicit choice.
    Cancel,
}

/// One errand, as far as choosing between them goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub id: u64,
    /// The errand's kind, as the crew knows it ("research", "backup").
    pub label: String,
    /// What it was asked about, if anything.
    pub topic: Option<String>,
    pub started: u64,
    pub paused: bool,
    /// Does it have safe points to hold at? One that doesn't can't pause —
    /// it can only finish or be called off.
    pub can_hold: bool,
}

/// Why this one was picked — said back, so a wrong guess is easy to correct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Why {
    Named,
    Numbered,
    OnlyOne,
    Conversation,
    JustStarted,
    All,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    These(Vec<u64>, Why),
    /// Couldn't tell. These are the ones to ask about.
    Ask(Vec<u64>),
    /// Not about any errand at all — leave the sentence to everything else.
    Nothing,
}

/// How long "stop" right after starting something is taken to mean that thing.
pub const JUST_STARTED_SECS: u64 = 90;

/// The verb and whatever followed it, or `None` when this isn't an errand
/// control at all.
///
/// Bare "pause" and "hold on" are not here: those pause *Atlas* (and hold
/// every errand with it) and are heard before this. Bare "stop" is here —
/// it is the single-errand control.
pub fn verb_and_target(said: &str) -> Option<(Verb, String)> {
    let t = normalise(said);
    if t.is_empty() {
        return None;
    }
    // Longest first so "hold off on" beats "hold".
    const PAUSE: &[&str] = &["hold off on", "put on hold", "stop", "pause", "hold", "freeze"];
    const RESUME: &[&str] = &[
        "carry on with", "keep going with", "continue with", "pick back up", "pick up",
        "unpause", "resume", "continue", "carry on", "keep going",
    ];
    const CANCEL: &[&str] = &["call off", "cancel", "abandon", "scrap", "kill", "drop"];
    for (verb, list) in [(Verb::Resume, RESUME), (Verb::Cancel, CANCEL), (Verb::Pause, PAUSE)] {
        for v in list {
            if t == *v {
                return Some((verb, String::new()));
            }
            if let Some(rest) = t.strip_prefix(&format!("{v} ")) {
                let rest = rest.trim_end_matches(" on hold").trim().to_string();
                return Some((verb, rest));
            }
        }
    }
    // "put the research on hold"
    if let Some(rest) = t.strip_prefix("put ").and_then(|r| r.strip_suffix(" on hold")) {
        return Some((Verb::Pause, rest.trim().to_string()));
    }
    None
}

/// "no, the backup" / "not that one, the backup" / "wrong one" — a correction
/// of the last pick. Returns what it should have been (possibly empty).
pub fn correction(said: &str) -> Option<String> {
    let t = normalise(said);
    for lead in ["not that one", "wrong one", "no not that", "not that", "no"] {
        if t == lead {
            return Some(String::new());
        }
        if let Some(rest) = t.strip_prefix(&format!("{lead} ")) {
            let rest = rest.trim_start_matches("i meant ").trim_start_matches("i mean ");
            return Some(rest.trim().to_string());
        }
    }
    None
}

/// Decide which errands a sentence is about. `recent` is what was said
/// before this, newest first.
pub fn pick(
    verb: Verb,
    target: &str,
    candidates: &[Candidate],
    recent: &[String],
    now: u64,
) -> Pick {
    let pool: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| match verb {
            Verb::Pause => !c.paused,
            Verb::Resume => c.paused,
            Verb::Cancel => true,
        })
        .collect();
    if pool.is_empty() {
        return Pick::Nothing;
    }
    let ids_of = |v: &[&Candidate]| v.iter().map(|c| c.id).collect::<Vec<u64>>();
    let target = normalise(target);
    let content: Vec<String> =
        words(&target).into_iter().filter(|w| !FILLER.contains(&w.as_str())).collect();

    if !content.is_empty() {
        if content.iter().all(|w| ALL.contains(&w.as_str())) {
            return Pick::These(ids_of(&pool), Why::All);
        }
        if let Some(i) = ordinal(&content) {
            let mut by_age = pool.clone();
            by_age.sort_by_key(|c| (c.started, c.id));
            let at = if i == usize::MAX { by_age.len() - 1 } else { i };
            return match by_age.get(at) {
                Some(c) => Pick::These(vec![c.id], Why::Numbered),
                None => Pick::Ask(ids_of(&pool)),
            };
        }
        let scored: Vec<(usize, &Candidate)> =
            pool.iter().map(|c| (score(c, &content), *c)).filter(|(s, _)| *s > 0).collect();
        let Some(best) = scored.iter().map(|(s, _)| *s).max() else {
            // Words were given and none of them are about an errand: this
            // sentence is about something else.
            return Pick::Nothing;
        };
        let top: Vec<&Candidate> =
            scored.iter().filter(|(s, _)| *s == best).map(|(_, c)| *c).collect();
        return if top.len() == 1 {
            Pick::These(vec![top[0].id], Why::Named)
        } else {
            Pick::Ask(ids_of(&top))
        };
    }

    // Nothing named. From here on it is context.
    if pool.len() == 1 {
        return Pick::These(vec![pool[0].id], Why::OnlyOne);
    }
    if verb == Verb::Cancel {
        // Never end something for good on a guess.
        return Pick::Ask(ids_of(&pool));
    }
    for said in recent.iter().take(3) {
        let said_words = words(&normalise(said));
        let hits: Vec<&Candidate> =
            pool.iter().filter(|c| score(c, &said_words) > 0).copied().collect();
        if hits.len() == 1 {
            return Pick::These(vec![hits[0].id], Why::Conversation);
        }
        if hits.len() > 1 {
            break; // the conversation names several: no better than nothing
        }
    }
    if verb == Verb::Pause {
        let fresh: Vec<&Candidate> = pool
            .iter()
            .filter(|c| now.saturating_sub(c.started) <= JUST_STARTED_SECS)
            .copied()
            .collect();
        if fresh.len() == 1 {
            return Pick::These(vec![fresh[0].id], Why::JustStarted);
        }
    }
    Pick::Ask(ids_of(&pool))
}

/// "the research on Postgres pricing".
pub fn describe(c: &Candidate) -> String {
    let kind = match c.label.as_str() {
        "research" => "the research",
        "council" => "the council",
        "build" => "the build",
        "improve" => "the project change",
        "mail" => "the mail check",
        "unsubscribe" => "the unsubscribing",
        "outreach" => "the outreach draft",
        "outlook-connect" => "the Outlook sign-in",
        "backup" => "the backup",
        "housekeeping" => "the housekeeping",
        "search-check" => "the search check",
        "hub-send" => "the send from the hub",
        "addon-share" => "the add-on share",
        "friend-knock" => "the knock on a friend's Atlas",
        "phone-code" => "the phone's code",
        "pictures" => {
            return match &c.topic {
                Some(t) if !t.trim().is_empty() => format!("the look at your {}", t.trim()),
                _ => "the look at the picture".into(),
            }
        }
        "call-notes" => {
            return match &c.topic {
                Some(t) if !t.trim().is_empty() => format!("the write-up of the {} call", t.trim()),
                _ => "the call write-up".into(),
            }
        }
        "conversation" => {
            return match &c.topic {
                Some(t) if !t.trim().is_empty() => format!("the conversation in {}", t.trim()),
                _ => "the conversation".into(),
            }
        }
        other => return format!("the {other}"),
    };
    match &c.topic {
        Some(t) if !t.trim().is_empty() => format!("{kind} on {}", t.trim()),
        _ => kind.to_string(),
    }
}

/// The question, when it can't tell: numbered, oldest first.
pub fn question(verb: Verb, among: &[&Candidate]) -> String {
    let mut by_age: Vec<&&Candidate> = among.iter().collect();
    by_age.sort_by_key(|c| (c.started, c.id));
    let listed: Vec<String> =
        by_age.iter().enumerate().map(|(i, c)| format!("{}) {}", i + 1, describe(c))).collect();
    let doing = match verb {
        Verb::Pause => "pause",
        Verb::Resume => "pick back up",
        Verb::Cancel => "call off for good",
    };
    let state = if verb == Verb::Resume { "paused" } else { "going" };
    format!(
        "{} things are {state}: {}. Which should I {doing} — a number, a name, or 'all'?",
        spell(by_age.len()),
        listed.join(", ")
    )
}

/// What was done, why that one, and what carries on.
pub fn answered(verb: Verb, picked: &[&Candidate], why: Why, others: &[&Candidate]) -> String {
    let names = |v: &[&Candidate]| join_and(&v.iter().map(|c| describe(c)).collect::<Vec<_>>());
    let (hold, cant): (Vec<&Candidate>, Vec<&Candidate>) =
        picked.iter().partition(|c| verb != Verb::Pause || c.can_hold);
    let mut s = String::new();
    if !hold.is_empty() {
        s.push_str(&match verb {
            Verb::Pause => format!(
                "Paused {} — it holds at its next safe point with nothing lost",
                names(&hold)
            ),
            Verb::Resume => format!("Picking {} back up from where it held", names(&hold)),
            Verb::Cancel => format!("Called off {} — what it had done is dropped", names(&hold)),
        });
        match why {
            Why::Conversation => s.push_str(" (it's what you were just talking about)"),
            Why::JustStarted => s.push_str(" (it's the one I'd just started)"),
            _ => {}
        }
        s.push('.');
    }
    if !cant.is_empty() {
        if !s.is_empty() {
            s.push(' ');
        }
        s.push_str(&format!(
            "{} has no safe point to hold at, so it will finish on its own — say 'cancel' \
             if you want it called off instead.",
            capitalise(&names(&cant))
        ));
    }
    let going: Vec<&Candidate> = others.iter().copied().filter(|c| !c.paused).collect();
    if verb != Verb::Resume && !going.is_empty() {
        s.push_str(&format!(" Still going: {}.", names(&going)));
        if matches!(why, Why::Conversation | Why::JustStarted) {
            s.push_str(" If I picked the wrong one, say 'no, the …' and I'll swap them.");
        }
    }
    s
}

// ---------------------------------------------------------------------------

/// Words that name an errand's kind.
fn aliases(label: &str) -> &'static [&'static str] {
    match label {
        "research" => &["research", "researching", "looking", "lookup", "search"],
        "council" => &["council", "room", "seats", "opinions"],
        "build" => &["build", "building", "page", "site", "website", "code", "app"],
        "improve" => &["improve", "improving", "change", "project"],
        "mail" => &["mail", "email", "emails", "inbox"],
        "unsubscribe" => &["unsubscribe", "unsubscribing", "newsletters"],
        "outreach" => &["outreach", "pitch"],
        "outlook-connect" => &["outlook", "signin", "sign"],
        "backup" => &["backup", "backing"],
        "housekeeping" => &["housekeeping", "tidy", "tidying", "cleanup", "clean"],
        "search-check" => &["measuring", "measure", "check", "scoring"],
        // A window Atlas is working for you (`daemon::WorkingForYou`); its
        // topic is the app, so "stop the Slack one" names it.
        "pictures" => &["looking", "look", "picture", "pictures", "screen", "webcam"],
        "call-notes" => &["call", "notes", "writeup", "write", "transcript"],
        "conversation" => &["conversation", "chat", "reply", "replying", "replies", "draft", "drafting", "window", "messages"],
        _ => &[],
    }
}

fn score(c: &Candidate, said: &[String]) -> usize {
    let kind = aliases(&c.label);
    let own = c.label.to_lowercase();
    let topic: Vec<String> = c
        .topic
        .as_deref()
        .map(|t| words(&normalise(t)))
        .unwrap_or_default()
        .into_iter()
        .filter(|w| w.len() >= 4 && !FILLER.contains(&w.as_str()))
        .collect();
    said.iter()
        .filter(|w| !FILLER.contains(&w.as_str()))
        .map(|w| {
            let mut n = 0;
            if kind.contains(&w.as_str()) || *w == own {
                n += 2;
            }
            if topic.iter().any(|t| t == w) {
                n += 1;
            }
            n
        })
        .sum()
}

/// Index into oldest-first, `usize::MAX` for "last".
fn ordinal(content: &[String]) -> Option<usize> {
    if content.len() > 2 {
        return None;
    }
    let w = content.iter().find(|w| *w != "number")?;
    Some(match w.as_str() {
        "first" | "1" | "1st" | "oldest" => 0,
        "second" | "2" | "2nd" => 1,
        "third" | "3" | "3rd" => 2,
        "fourth" | "4" | "4th" => 3,
        "last" | "latest" | "newest" => usize::MAX,
        _ => return None,
    })
}

const FILLER: &[&str] = &[
    "the", "that", "this", "it", "a", "an", "my", "on", "about", "for", "with", "of", "one",
    "ones", "errand", "errands", "task", "tasks", "job", "jobs", "thing", "things", "please",
    "atlas", "now", "just", "you", "your", "those", "them", "these", "to", "up", "and", "is",
    "are", "doing", "running", "i", "meant", "mean", "going",
];

const ALL: &[&str] = &["all", "everything", "both", "every", "others", "rest", "other"];

fn words(normalised: &str) -> Vec<String> {
    normalised.split(|c: char| c.is_whitespace() || c == '-').filter(|w| !w.is_empty()).map(str::to_string).collect()
}

fn normalise(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c.is_whitespace() || c == '-' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn spell(n: usize) -> String {
    match n {
        2 => "Two".into(),
        3 => "Three".into(),
        4 => "Four".into(),
        5 => "Five".into(),
        n => n.to_string(),
    }
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

fn join_and(parts: &[String]) -> String {
    match parts {
        [] => String::new(),
        [one] => one.clone(),
        [a, b] => format!("{a} and {b}"),
        _ => format!("{}, and {}", parts[..parts.len() - 1].join(", "), parts[parts.len() - 1]),
    }
}
