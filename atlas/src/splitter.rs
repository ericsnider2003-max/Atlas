//! Free-form speech, read without a model (CONV1 phase 1, 12 Oct 2026).
//!
//! Eric's standard is that he can say any coherent sentence, in any order,
//! offline, and Atlas acts. The phrase table (`intent::Parser`) reads one
//! command per sentence, worded close to a phrase it knows. Measured on the
//! free-form corpus (`tests/free_form_corpus.rs`) before this module, with no
//! model, it understood well under half of ordinary spoken requests: filler
//! ("hey could you maybe open up chrome for me"), a self-correction ("open
//! notepad, no wait, open chrome") and every compound request ("open chrome
//! and then put it on the left monitor and remind me at 5") came out as
//! nothing, or as one wrong thing.
//!
//! This sits in front of the phrase table and does three things, all of them
//! deterministic and all of them conservative:
//!
//! 1. **Filler off.** Politeness and hedging that change nothing about what
//!    is asked ("hey", "could you", "maybe", "for me", "open *up*") are taken
//!    off the ends of each part, and the plain request is read instead.
//! 2. **Self-corrections.** "no wait", "I mean", "actually", "scratch that":
//!    the last instruction wins. "open notepad no wait chrome" borrows the
//!    verb ("open chrome"), but only for a command whose argument is a name.
//! 3. **Compounds.** A request is cut where "and then", "then", "after that",
//!    "also", "and", a comma or a full stop could start a new instruction --
//!    and a cut is KEPT only when the part on each side independently reads
//!    as something Atlas does. "remind me to call mom and dad at 5" stays one
//!    reminder because "dad at 5" reads as nothing. Free-text commands (a
//!    message, a note, a reminder's text) are never cut at a bare "and" or a
//!    comma, only at an explicit "then" / "after that" / "also".
//!
//! Nothing here decides whether anything may run. Every part goes through
//! `run_command` on its own, with every approval and outside-text gate it
//! would have had if said alone (`daemon::turn::spoken_steps`). When a part
//! does not read, nothing is split and the sentence goes on exactly as it did
//! before: to the model when there is one, or to "I didn't catch that".

use crate::intent::{Intent, Parser};

/// What one part of a sentence reads as, without a model: a phrase-table
/// command, or one of the helpers the turn asks when the table has nothing
/// (`Daemon::asked_of_a_helper`) whose wording can be recognised without the
/// daemon's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reading {
    Command(Intent),
    /// "remind me at 5 to stretch" (`remind_help`).
    Reminder,
    /// "put it on the left monitor" (`move_window_help`).
    MoveWindow,
    /// "text Sam saying I'm late" (`text_help`).
    Text,
    /// "what's the weather tomorrow" (`weather_help`).
    Weather,
}

impl Reading {
    /// In words, for a spoken list of what was and was not done.
    pub fn plain(&self) -> String {
        match self {
            Reading::Command(i) => i.plain(),
            Reading::Reminder => "the reminder".into(),
            Reading::MoveWindow => "moving the window".into(),
            Reading::Text => "the text".into(),
            Reading::Weather => "the weather".into(),
        }
    }
}

/// One instruction out of a sentence: the words that are run (the plain
/// request, fillers off) and what they read as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpokenStep {
    pub words: String,
    pub reading: Reading,
}

/// What a single part reads as, in the order the turn itself tries: the
/// phrase table first, then the helpers.
pub fn reading_of(words: &str, parser: &Parser) -> Option<Reading> {
    let words = words.trim();
    if words.is_empty() {
        return None;
    }
    let i = parser.parse(words);
    if !matches!(i, Intent::Unknown(_)) {
        return Some(Reading::Command(i));
    }
    if reminder_restated(words).is_some() {
        return Some(Reading::Reminder);
    }
    if crate::workspace::move_to_screen_asked(words).is_some() {
        return Some(Reading::MoveWindow);
    }
    if crate::texting::text_asked(words).is_some() {
        return Some(Reading::Text);
    }
    if crate::weather::about_the_weather(words).is_some() {
        return Some(Reading::Weather);
    }
    None
}

/// The other ways of asking for a reminder, said as "remind me ...". One
/// list, read by `remind_help` and by `reading_of`, so the splitter and the
/// reminder can never disagree about what a reminder is.
pub fn reminder_restated(said: &str) -> Option<String> {
    const OTHER_WAYS: &[&str] = &[
        "set a reminder for ",
        "set a reminder ",
        "set reminder for ",
        "set reminder ",
        "add a reminder for ",
        "add a reminder ",
        "give me a nudge ",
        "nudge me ",
        "ping me ",
        "give me a shout ",
        "give me a reminder ",
        "send me a reminder ",
    ];
    let low = said.trim().to_lowercase();
    if low.starts_with("remind me") || low.starts_with("remind ") {
        return Some(said.trim().to_string());
    }
    OTHER_WAYS.iter().find(|p| low.starts_with(**p)).map(|p| {
        format!(
            "remind me {}",
            said.trim().get(p.len()..).unwrap_or("").trim()
        )
    })
}

/// The instructions in what was said, in order, when reading it as plain
/// requests (fillers off, corrections applied, compounds cut) understands
/// more than the phrase table does on the whole sentence.
///
/// `None` means "nothing to add": the sentence is passed on unchanged --
/// either the table already reads it and there is nothing to cut, or some
/// part of it reads as nothing (and then the whole thing is the model's to
/// work out, as before).
pub fn spoken_steps(said: &str, parser: &Parser) -> Option<Vec<SpokenStep>> {
    let whole = reading_of(said, parser);
    // A passphrase is never cut, cleaned or re-read.
    if matches!(whole, Some(Reading::Command(Intent::Unlock(_)))) {
        return None;
    }
    // What the table makes of the whole sentence counts only if it is sane:
    // "open chrome, open notepad" read as one app called "chrome open
    // notepad" is not a reading worth keeping the sentence whole for.
    let whole = whole.filter(|w| names_known(w, parser));
    let tokens = tokens_of(said);
    if tokens.is_empty() {
        return None;
    }
    let pieces = pieces_of(&tokens);
    let mut drafts: Vec<Draft> = Vec::new();
    let mut corrected = false;
    for piece in pieces {
        let text = span_text(&tokens, piece.start, piece.end);
        let Some(last) = drafts.last_mut() else {
            drafts.push(Draft::new(piece.start, piece.end, &text, parser));
            continue;
        };
        match piece.join {
            Join::Correction { weak } => {
                // The last instruction wins: what comes after the marker,
                // read on its own, or with the verb borrowed from what it
                // corrects ("open notepad -- no wait -- chrome").
                let plain = plain_request(&text);
                let own = reading_of(&text, parser)
                    .map(|r| (text.clone(), r))
                    .or_else(|| reading_of(&plain, parser).map(|r| (plain.clone(), r)))
                    .filter(|(_, r)| names_known(r, parser));
                let replaced = match own {
                    Some(found) if !weak || last.reading.is_some() => Some(found),
                    // "remind me at 5, no wait, at 6 to call mom": put in
                    // place of the words it repeats. Only after a marker
                    // nobody says by accident; a bare "no" could be part of
                    // what is being said ("remind me to say no to Bob").
                    _ if !weak => corrected_in_place(last, &plain, parser)
                        .or_else(|| borrowed_verb(last, &plain, parser)),
                    _ => borrowed_verb(last, &plain, parser),
                };
                match replaced {
                    Some((words, reading)) => {
                        last.start = piece.start;
                        last.end = piece.end;
                        last.words = words;
                        last.reading = Some(reading);
                        corrected = true;
                    }
                    None => last.extend(piece.end, &tokens, parser),
                }
            }
            Join::Between { strong, firm } => {
                let next = Draft::new(piece.start, piece.end, &text, parser);
                let keep_cut = match (&last.reading, &next.reading) {
                    // A sentence the table already reads whole is cut only
                    // where words or a full stop join two instructions, never
                    // at a comma or a question mark alone: "can you see me?
                    // what am I holding up?" is one look, "schedule the
                    // dentist at 9, remind me 15 minutes before" one event.
                    _ if !firm && whole.is_some() => false,
                    // The same thing asked twice ("Are you using my camera?
                    // Can you see me?") is one request, said two ways.
                    (Some(before), Some(after)) if before == after => false,
                    (Some(before), Some(after)) => {
                        strong
                            || (!takes_free_text(before)
                                && stands_alone_at_a_bare_and(before)
                                && stands_alone_at_a_bare_and(after))
                    }
                    _ => false,
                };
                if keep_cut {
                    drafts.push(next);
                    continue;
                }
                // "open chrome and notepad", "open chrome, then notepad": the
                // verb carries over to a bare name.
                if next.reading.is_none() {
                    if let Some((words, reading)) =
                        borrowed_verb(last, &plain_request(&text), parser)
                    {
                        let mut carried = Draft::new(piece.start, piece.end, &text, parser);
                        carried.words = words;
                        carried.reading = Some(reading);
                        drafts.push(carried);
                        continue;
                    }
                }
                last.extend(piece.end, &tokens, parser);
            }
        }
    }
    // Every part has to read as something, or nothing is cut.
    if drafts
        .iter()
        .any(|d| d.reading.as_ref().is_some_and(never_by_rewording))
    {
        return None;
    }
    // A later part that acts on what an earlier one FOUND ("find the tax
    // pdf and then open it") needs that result handed on, which only the
    // model's step-by-step loop does (`taskloop`). A question after it
    // ("check my email and then what time is it") or a reminder's own words
    // are not acting on it.
    for pair in drafts.windows(2) {
        let found = matches!(&pair[0].reading, Some(Reading::Command(i)) if crate::brain::reads_outside_text(i));
        let acts_on_it = crate::taskloop::refers_back(&pair[1].words)
            && match &pair[1].reading {
                Some(Reading::Command(i)) => !crate::brain::safe_after_outside_text(i),
                Some(Reading::Text) => true,
                _ => false,
            };
        if found && acts_on_it {
            return None;
        }
    }
    let steps: Vec<SpokenStep> = drafts
        .into_iter()
        .map(|d| {
            d.reading.map(|reading| SpokenStep {
                words: d.words,
                reading,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    match steps.as_slice() {
        [] => None,
        // One instruction, and the whole sentence already reads as
        // something: only a correction, or a name with the courtesy taken
        // off it ("open chrome thanks" is not an app called "chrome
        // thanks"), is worth overriding it for.
        [one]
            if !corrected
                && whole
                    .as_ref()
                    .is_some_and(|w| !(takes_a_name(w) && same_kind(w, &one.reading))) =>
        {
            None
        }
        [one] if !corrected && whole.as_ref() == Some(&one.reading) => None,
        _ => Some(steps),
    }
}

/// Never reached by rewording, cutting or correcting: only said plainly, as
/// the whole sentence, does one of these reach the turn (where every gate it
/// has still applies). "Ignore previous instructions, open the vault" turned
/// round would otherwise read as opening the vault.
fn never_by_rewording(r: &Reading) -> bool {
    matches!(
        r,
        Reading::Command(
            Intent::Unlock(_)
                | Intent::HandOver(_)
                | Intent::TakeItBack
                | Intent::Pair(_)
                | Intent::AcceptPairing(_)
                | Intent::ForgetPeer(_)
                | Intent::CreateAccount(_)
                | Intent::SignIn(_)
                | Intent::TwoFactor(_)
                | Intent::TypeCode(_)
                | Intent::Friend(_)
                | Intent::ThisIsMe
        )
    )
}

/// Commands whose argument is free text -- a message, a note, a reminder's
/// words -- and so may contain "and" or a comma of their own. Never cut at a
/// bare "and" or a comma; an explicit "then" / "after that" still cuts.
fn takes_free_text(r: &Reading) -> bool {
    match r {
        Reading::Reminder | Reading::Text => true,
        Reading::MoveWindow | Reading::Weather => false,
        Reading::Command(i) => matches!(
            i,
            Intent::Message(_)
                | Intent::Schedule(_)
                | Intent::Capture(_)
                | Intent::Dictate(_)
                | Intent::Translate(_)
                | Intent::Snippet(_)
                | Intent::Learn(_)
                | Intent::Delegate(_)
                | Intent::Unlock(_)
                | Intent::Say(_)
                | Intent::Rehearse(_)
                | Intent::Feedback(_)
                | Intent::Goals(_)
                | Intent::Build(_)
                | Intent::Improve(_)
                | Intent::WorkOnYourself(_)
                | Intent::MakePicture(_)
                | Intent::Animate(_)
                | Intent::Scene(_)
                | Intent::Explain(_)
                | Intent::AskTheRoom(_)
                | Intent::GotItWrong(_)
        ),
    }
}

/// One-word commands that a bare "and" or a comma does not cut off: "open
/// chrome and wait for me" is not a pause, "wait, open chrome" is not a
/// pause and then a launch, "close it and continue" is not a resume.
fn stands_alone_at_a_bare_and(r: &Reading) -> bool {
    !matches!(
        r,
        Reading::Command(
            Intent::Pause | Intent::Resume | Intent::Ready | Intent::Dismiss | Intent::Say(_)
        )
    )
}

/// A name the splitter put together is one Atlas knows
/// (`Parser::names_something_known`): "open chrome, oh and also ..." must not
/// open an app called "oh".
fn names_known(r: &Reading, parser: &Parser) -> bool {
    match r {
        Reading::Command(i) => parser.names_something_known(i),
        _ => true,
    }
}

/// A command whose argument is a name, so a bare name after "and" or a
/// correction can borrow its verb.
fn takes_a_name(r: &Reading) -> bool {
    matches!(
        r,
        Reading::Command(
            Intent::OpenApp(_) | Intent::CloseApp(_) | Intent::FocusApp(_) | Intent::SetMode(_)
        )
    )
}

/// "open notepad" + "chrome" -> "open chrome": the leading words of the
/// earlier instruction with the new name, kept only when that reads as the
/// same kind of command and the name is short and not itself a request
/// ("open chrome and play some music" is not an app called "play some
/// music").
fn borrowed_verb(before: &Draft, name: &str, parser: &Parser) -> Option<(String, Reading)> {
    let kind = before.reading.as_ref().filter(|r| takes_a_name(r))?;
    let name = name.trim();
    let n = name.split_whitespace().count();
    if n == 0 || n > 2 || crate::taskloop::starts_with_verb(name) {
        return None;
    }
    let lead: Vec<&str> = before.words.split_whitespace().collect();
    for k in 1..lead.len() {
        let words = format!("{} {name}", lead[..k].join(" "));
        if let Some(r) = reading_of(&words, parser) {
            if same_command(&r, kind) && names_known(&r, parser) {
                return Some((words, r));
            }
            return None;
        }
    }
    None
}

/// "remind me at 5" corrected by "at 6 to call mom": the correction starts
/// with a word the instruction already has, so it replaces the instruction
/// from that word on ("remind me at 6 to call mom"). Kept only when the
/// result reads as the same kind of thing.
fn corrected_in_place(before: &Draft, after: &str, parser: &Parser) -> Option<(String, Reading)> {
    let kind = before.reading.as_ref()?;
    let first = after.split_whitespace().next()?.to_lowercase();
    let lead: Vec<&str> = before.words.split_whitespace().collect();
    let j = (1..lead.len()).rev().find(|&j| {
        lead[j]
            .to_lowercase()
            .trim_matches(|c: char| !c.is_alphanumeric())
            == first
    })?;
    let words = format!("{} {after}", lead[..j].join(" "));
    let r = reading_of(&words, parser)?;
    (same_kind(&r, kind) && names_known(&r, parser)).then_some((words, r))
}

/// The same command, or the same helper.
fn same_kind(a: &Reading, b: &Reading) -> bool {
    match (a, b) {
        (Reading::Command(_), Reading::Command(_)) => same_command(a, b),
        _ => std::mem::discriminant(a) == std::mem::discriminant(b),
    }
}

fn same_command(a: &Reading, b: &Reading) -> bool {
    match (a, b) {
        (Reading::Command(x), Reading::Command(y)) => {
            std::mem::discriminant(x) == std::mem::discriminant(y)
        }
        _ => false,
    }
}

/// A part being built: where it is in the sentence, the words to run and
/// what they read as.
#[derive(Debug, Clone)]
struct Draft {
    start: usize,
    end: usize,
    words: String,
    reading: Option<Reading>,
}

impl Draft {
    fn new(start: usize, end: usize, text: &str, parser: &Parser) -> Draft {
        let (words, reading) = read_plainly(text, parser);
        let reading = reading.filter(|r| names_known(r, parser));
        Draft {
            start,
            end,
            words,
            reading,
        }
    }

    /// The cut was wrong: this part runs on to `end`, and is read again.
    fn extend(&mut self, end: usize, tokens: &[Token], parser: &Parser) {
        self.end = end;
        let text = span_text(tokens, self.start, self.end);
        let (words, reading) = read_plainly(&text, parser);
        let reading = reading.filter(|r| names_known(r, parser));
        self.words = words;
        self.reading = reading;
    }
}

/// The words as said if they read; otherwise the plain request (fillers
/// off), if that reads; otherwise the request put back in the usual order
/// (`reordered`). The words as said, unread, when none of those does.
fn read_plainly(text: &str, parser: &Parser) -> (String, Option<Reading>) {
    let plain = plain_request(text);
    if let Some(r) = reading_of(text, parser) {
        // "open chrome thanks": a name with a courtesy on the end.
        if takes_a_name(&r) && plain != text.trim() {
            if let Some(p) = reading_of(&plain, parser).filter(|p| same_kind(p, &r)) {
                return (plain, Some(p));
            }
        }
        return (text.trim().to_string(), Some(r));
    }
    if let Some(r) = reading_of(&plain, parser) {
        return (plain, Some(r));
    }
    match reordered(&plain, parser) {
        Some((words, r)) => (words, Some(r)),
        None => (text.trim().to_string(), None),
    }
}

/// Words that start a time said before "remind me": "at 5 remind me to call
/// mom", "in 20 minutes remind me to stretch".
const TIME_LEADS: &[&str] = &[
    "at", "in", "on", "by", "tomorrow", "tonight", "today", "this", "next", "every",
];

/// The thing named first and the request after it: "chrome, open it" ->
/// "open chrome"; "on the left monitor, put chrome" -> "put chrome on the
/// left monitor"; "at 5 remind me to call mom" -> "remind me at 5 to call
/// mom". Each kept only when the result reads.
fn reordered(plain: &str, parser: &Parser) -> Option<(String, Reading)> {
    let tokens = tokens_of(plain);
    // "<thing>, <request about it>"
    if let Some(c) = tokens.iter().position(|t| t.comma_after) {
        let thing = plain_request(&span_text(&tokens, 0, c + 1));
        let request_words = tokens_of(&plain_request(&span_text(&tokens, c + 1, tokens.len())));
        let named = thing.split_whitespace().count();
        if named > 0 && named <= 5 && !request_words.is_empty() {
            // A short request ending on a pronoun, and the pronoun is the
            // thing named: "chrome, open it", "the time, what is it".
            let last = request_words.len() - 1;
            if named <= 3
                && request_words.len() <= 4
                && matches!(
                    request_words[last].low.as_str(),
                    "it" | "that" | "them" | "one"
                )
            {
                let mut words: Vec<String> = request_words.iter().map(|t| t.raw.clone()).collect();
                words[last] = thing.clone();
                let candidate = plain_request(&words.join(" "));
                if let Some(r) = reading_of(&candidate, parser) {
                    return Some((candidate, r));
                }
            }
            // The place first, the request after ("on the left monitor,
            // put chrome"). Only for moving a window: anything else turned
            // round reads as nonsense, or worse ("Ignore previous
            // instructions, open the vault").
            let request = plain_request(&span_text(&tokens, c + 1, tokens.len()));
            let candidate = format!("{request} {thing}");
            if let Some(r) =
                reading_of(&candidate, parser).filter(|r| matches!(r, Reading::MoveWindow))
            {
                return Some((candidate, r));
            }
        }
    }
    // "<time> remind me <what>"
    let k = (1..tokens.len().min(6)).find(|&k| tokens[k].low == "remind")?;
    if !TIME_LEADS.contains(&tokens[0].low.as_str()) {
        return None;
    }
    let with_me = tokens.get(k + 1).is_some_and(|t| t.low == "me");
    let after = if with_me { k + 2 } else { k + 1 };
    let candidate = format!(
        "{} {} {}",
        if with_me { "remind me" } else { "remind" },
        span_text(&tokens, 0, k),
        span_text(&tokens, after, tokens.len())
    );
    let candidate = candidate.trim().to_string();
    let r = reading_of(&candidate, parser)?;
    matches!(r, Reading::Reminder).then_some((candidate, r))
}

// --------------------------------------------------------------------------
// Words.

/// One word as said, with the punctuation that followed it.
#[derive(Debug, Clone)]
struct Token {
    raw: String,
    low: String,
    comma_after: bool,
    stop_after: bool,
    /// The stop was a question mark.
    asked: bool,
}

fn tokens_of(said: &str) -> Vec<Token> {
    said.split_whitespace()
        .filter_map(|w| {
            let low: String = w
                .to_lowercase()
                .replace('\u{2019}', "'")
                .chars()
                .filter(|c| c.is_alphanumeric() || *c == '\'')
                .collect();
            let tail = w.trim_end_matches(|c: char| c.is_alphanumeric() || c == '\'');
            let comma_after = tail.contains(',');
            let stop_after = tail.contains(['.', '?', '!', ';']);
            if low.is_empty() && !w.chars().any(|c| c.is_alphanumeric()) {
                // A stray dash or comma on its own: carried by the word before.
                return None;
            }
            Some(Token {
                raw: w.to_string(),
                low,
                comma_after,
                stop_after,
                asked: tail.contains('?'),
            })
        })
        .collect()
}

/// The words from `start` to `end` as said, minus the comma or full stop
/// that closed the last one.
fn span_text(tokens: &[Token], start: usize, end: usize) -> String {
    let words: Vec<&str> = tokens[start..end].iter().map(|t| t.raw.as_str()).collect();
    let s = words.join(" ");
    s.trim_end_matches([',', ';', '.', '!', '?', ':', ' '])
        .trim()
        .to_string()
}

/// How a piece is joined to the one before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Join {
    /// "and", a comma (weak) or "then", "after that", "also", a full stop
    /// (strong). `firm` when words or a full stop joined it, not only a
    /// comma or a question mark.
    Between { strong: bool, firm: bool },
    /// "no wait", "I mean", "actually": this replaces the one before. A weak
    /// marker ("no", "wait" alone) only counts when both sides read.
    Correction { weak: bool },
}

#[derive(Debug, Clone, Copy)]
struct Piece {
    start: usize,
    end: usize,
    join: Join,
}

/// Word sequences that join two instructions, longest first.
const STRONG_JOINS: &[&[&str]] = &[
    &["and", "after", "that"],
    &["and", "then"],
    &["and", "also"],
    &["and", "afterwards"],
    &["after", "that"],
    &["then"],
    &["also"],
    &["afterwards"],
];
const WEAK_JOINS: &[&[&str]] = &[&["and"], &["plus"]];

/// Word sequences that take back what came before, longest first.
const CORRECTIONS: &[&[&str]] = &[
    &["no", "wait"],
    &["wait", "no"],
    &["no", "no"],
    &["no", "actually"],
    &["actually", "no"],
    &["no", "i", "mean"],
    &["no", "i", "meant"],
    &["sorry", "i", "mean"],
    &["sorry", "i", "meant"],
    &["i", "mean"],
    &["i", "meant"],
    &["scratch", "that"],
    &["or", "rather"],
    &["make", "that"],
    &["change", "that", "to"],
    &["actually"],
    &["sorry"],
];
const WEAK_CORRECTIONS: &[&[&str]] = &[&["no"], &["wait"]];

fn matches_at(tokens: &[Token], i: usize, seq: &[&str]) -> bool {
    i + seq.len() <= tokens.len() && seq.iter().enumerate().all(|(k, w)| tokens[i + k].low == *w)
}

/// The sentence cut at every place a new instruction could start. Whether a
/// cut is kept is decided by `spoken_steps`, which can read the parts.
fn pieces_of(tokens: &[Token]) -> Vec<Piece> {
    let mut pieces: Vec<Piece> = Vec::new();
    let mut start = 0usize;
    let mut join = Join::Between {
        strong: true,
        firm: true,
    };
    let mut i = 0usize;
    while i < tokens.len() {
        // Punctuation after the previous word.
        if i > 0 && i > start {
            let prev = &tokens[i - 1];
            if prev.stop_after || prev.comma_after {
                cut_at(
                    &mut pieces,
                    &mut start,
                    &mut join,
                    i,
                    0,
                    Join::Between {
                        strong: prev.stop_after,
                        firm: prev.stop_after && !prev.asked,
                    },
                );
            }
        }
        // A correction takes back something said before it; with nothing
        // before it ("and no translate"), "no" is a "no", not a correction.
        let said_before = i > start || !pieces.is_empty();
        if let Some(seq) = CORRECTIONS
            .iter()
            .find(|s| said_before && matches_at(tokens, i, s))
        {
            cut_at(
                &mut pieces,
                &mut start,
                &mut join,
                i,
                seq.len(),
                Join::Correction { weak: false },
            );
            i += seq.len();
            continue;
        }
        if let Some(seq) = STRONG_JOINS.iter().find(|s| matches_at(tokens, i, s)) {
            cut_at(
                &mut pieces,
                &mut start,
                &mut join,
                i,
                seq.len(),
                Join::Between {
                    strong: true,
                    firm: true,
                },
            );
            i += seq.len();
            continue;
        }
        if said_before {
            if let Some(seq) = WEAK_CORRECTIONS.iter().find(|s| matches_at(tokens, i, s)) {
                cut_at(
                    &mut pieces,
                    &mut start,
                    &mut join,
                    i,
                    seq.len(),
                    Join::Correction { weak: true },
                );
                i += seq.len();
                continue;
            }
        }
        if let Some(seq) = WEAK_JOINS.iter().find(|s| matches_at(tokens, i, s)) {
            cut_at(
                &mut pieces,
                &mut start,
                &mut join,
                i,
                seq.len(),
                Join::Between {
                    strong: false,
                    firm: true,
                },
            );
            i += seq.len();
            continue;
        }
        i += 1;
    }
    if tokens.len() > start {
        pieces.push(Piece {
            start,
            end: tokens.len(),
            join,
        });
    }
    pieces
}

/// A cut found at `at`: the piece before it is closed (if it has words) and
/// the next one starts after the `skip` joining words.
fn cut_at(
    pieces: &mut Vec<Piece>,
    start: &mut usize,
    join: &mut Join,
    at: usize,
    skip: usize,
    how: Join,
) {
    if at > *start {
        pieces.push(Piece {
            start: *start,
            end: at,
            join: *join,
        });
        *join = how;
    } else if !pieces.is_empty() {
        // Two joins in a row ("and, then"): the stronger reading wins.
        // (Joining words at the very start, "and also, open chrome", are
        // filler, not a join.)
        *join = strongest(*join, how);
    }
    *start = at + skip;
}

fn strongest(a: Join, b: Join) -> Join {
    match (a, b) {
        (Join::Correction { weak: false }, _) | (_, Join::Correction { weak: false }) => {
            Join::Correction { weak: false }
        }
        (Join::Correction { weak: true }, _) | (_, Join::Correction { weak: true }) => {
            Join::Correction { weak: true }
        }
        (
            Join::Between {
                strong: x,
                firm: fx,
            },
            Join::Between {
                strong: y,
                firm: fy,
            },
        ) => Join::Between {
            strong: x || y,
            firm: fx || fy,
        },
    }
}

// --------------------------------------------------------------------------
// Fillers.

/// Words before a request that change nothing about it, longest first.
const LEAD_FILLERS: &[&[&str]] = &[
    &["i", "was", "wondering", "if", "you", "could"],
    &["when", "you", "get", "a", "chance"],
    &["would", "you", "be", "so", "kind", "as", "to"],
    &["if", "it's", "not", "too", "much", "trouble"],
    &["if", "its", "not", "too", "much", "trouble"],
    &["it", "would", "be", "great", "if", "you", "could"],
    &["it'd", "be", "great", "if", "you", "could"],
    &["itd", "be", "great", "if", "you", "could"],
    &["sorry", "to", "bother", "you", "but"],
    &["do", "me", "a", "favour", "and"],
    &["do", "me", "a", "favor", "and"],
    &["i'd", "love", "it", "if", "you", "could"],
    &["would", "you", "be", "able", "to"],
    &["do", "you", "think", "you", "could"],
    &["could", "you", "please"],
    &["can", "you", "please"],
    &["would", "you", "please"],
    &["would", "you", "mind"],
    &["do", "you", "mind"],
    &["i", "would", "like", "you", "to"],
    &["i'd", "like", "you", "to"],
    &["id", "like", "you", "to"],
    &["i", "want", "you", "to"],
    &["i", "need", "you", "to"],
    &["can", "you", "go", "ahead", "and"],
    &["go", "ahead", "and"],
    &["why", "don't", "you"],
    &["why", "dont", "you"],
    &["don't", "forget", "to"],
    &["dont", "forget", "to"],
    &["make", "sure", "to"],
    &["if", "you", "could"],
    &["if", "you", "can"],
    &["hey", "atlas"],
    &["ok", "atlas"],
    &["okay", "atlas"],
    &["could", "you"],
    &["can", "you"],
    &["would", "you"],
    &["will", "you"],
    &["you", "can"],
    &["i", "think"],
    &["i", "guess"],
    &["you", "know"],
    &["you", "should"],
    &["tell", "me"],
    &["sorry"],
    &["actually"],
    &["hang", "on"],
    &["hold", "on"],
    &["atlas"],
    &["hey"],
    &["hi"],
    &["yo"],
    &["ok"],
    &["okay"],
    &["so"],
    &["um"],
    &["umm"],
    &["uh"],
    &["uhh"],
    &["er"],
    &["erm"],
    &["hmm"],
    &["well"],
    &["alright"],
    &["right"],
    &["now"],
    &["and"],
    &["then"],
    &["also"],
    &["please"],
    &["kindly"],
    &["just"],
    &["maybe"],
    &["perhaps"],
    &["possibly"],
    &["wait"],
    &["quickly"],
];

/// Words after a request that change nothing about it, longest first.
const TAIL_FILLERS: &[&[&str]] = &[
    &["when", "you", "get", "a", "chance"],
    &["that", "would", "be", "great"],
    &["that'd", "be", "great"],
    &["thatd", "be", "great"],
    &["if", "you", "don't", "mind"],
    &["if", "you", "dont", "mind"],
    &["thank", "you", "very", "much"],
    &["when", "you", "can"],
    &["if", "you", "can"],
    &["if", "you", "could"],
    &["thank", "you"],
    &["for", "me"],
    &["real", "quick"],
    &["right", "now"],
    &["would", "you"],
    &["will", "you"],
    &["could", "you"],
    &["can", "you"],
    &["please"],
    &["thanks"],
    &["now"],
    &["atlas"],
    &["ok"],
    &["okay"],
    &["quickly"],
];

/// "would you mind opening chrome": the verb as a command.
const GERUNDS: &[(&str, &str)] = &[
    ("opening", "open"),
    ("closing", "close"),
    ("launching", "launch"),
    ("playing", "play"),
    ("putting", "put"),
    ("moving", "move"),
    ("setting", "set"),
    ("reminding", "remind"),
    ("sending", "send"),
    ("texting", "text"),
    ("checking", "check"),
    ("taking", "take"),
    ("making", "make"),
    ("starting", "start"),
    ("switching", "switch"),
    ("looking", "look"),
    ("researching", "research"),
    ("messaging", "message"),
];

/// The request with the courtesies and hedges off both ends: "hey could you
/// maybe open up chrome for me" -> "open chrome". The words keep the case
/// they were said in.
pub fn plain_request(text: &str) -> String {
    let mut words: Vec<Token> = tokens_of(text);
    let mut minded = false;
    loop {
        let before = words.len();
        if let Some(seq) = LEAD_FILLERS.iter().find(|s| matches_at(&words, 0, s)) {
            if words.len() > seq.len() {
                minded |= seq.contains(&"mind");
                words.drain(..seq.len());
            }
        }
        if let Some(seq) = TAIL_FILLERS
            .iter()
            .find(|s| words.len() > s.len() && matches_at(&words, words.len() - s.len(), s))
        {
            let keep = words.len() - seq.len();
            words.truncate(keep);
        }
        if words.len() == before {
            break;
        }
    }
    // "would you mind opening chrome" -- but not "Opening Chrome.", which
    // is a report, not a request.
    if let Some(first) = words.first_mut().filter(|_| minded) {
        if let Some((_, verb)) = GERUNDS.iter().find(|(g, _)| first.low == *g) {
            first.raw = verb.to_string();
            first.low = verb.to_string();
        }
    }
    // "open up chrome": the "up" of "open up" (not "look up", "back up").
    if words.len() > 2 && words[0].low == "open" && words[1].low == "up" {
        words.remove(1);
    }
    let raw: Vec<&str> = words.iter().map(|t| t.raw.as_str()).collect();
    raw.join(" ")
        .trim_end_matches([',', ';', '.', '!', '?', ':', ' '])
        .trim_start_matches([',', ' '])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parser() -> Parser {
        let cfg = crate::config::Config::load(std::path::Path::new("config"))
            .unwrap_or_else(|e| panic!("config: {e}"));
        Parser::new(&cfg.commands)
    }

    fn steps(said: &str) -> Vec<String> {
        spoken_steps(said, &known_parser())
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.words)
            .collect()
    }

    #[test]
    fn splitter_takes_the_filler_off_a_single_request() {
        assert_eq!(
            plain_request("hey could you maybe open up chrome for me"),
            "open chrome"
        );
        assert_eq!(
            plain_request("um, would you mind opening notepad please"),
            "open notepad"
        );
        assert_eq!(plain_request("open chrome"), "open chrome");
        // Never down to nothing.
        assert_eq!(plain_request("please"), "please");
    }

    #[test]
    fn splitter_leaves_a_sentence_the_table_already_reads() {
        assert!(spoken_steps("open chrome", &parser()).is_none());
        assert!(spoken_steps("what time is it", &parser()).is_none());
    }

    #[test]
    fn splitter_cuts_a_compound_only_where_both_sides_read() {
        assert_eq!(
            steps("open chrome and then open notepad"),
            ["open chrome", "open notepad"]
        );
        assert_eq!(
            steps("open chrome, open notepad"),
            ["open chrome", "open notepad"]
        );
        // "dad at 5" is not an instruction: one reminder.
        assert!(spoken_steps("remind me to call mom and dad at 5", &parser()).is_none());
    }

    #[test]
    fn splitter_never_cuts_free_text_at_a_bare_and() {
        // A note's own "and" is the note's.
        assert!(
            spoken_steps("note that I need milk and open the fridge", &parser())
                .map(|s| s.len() == 1)
                .unwrap_or(true)
        );
    }

    #[test]
    fn splitter_last_instruction_wins_on_a_correction() {
        assert_eq!(steps("open notepad no wait open chrome"), ["open chrome"]);
        assert_eq!(steps("open notepad, no wait, chrome"), ["open chrome"]);
        assert_eq!(steps("open notepad, actually open chrome"), ["open chrome"]);
    }

    #[test]
    fn splitter_carries_the_verb_to_a_bare_name() {
        assert_eq!(
            steps("open chrome and notepad"),
            ["open chrome", "open notepad"]
        );
    }

    #[test]
    fn splitter_never_touches_a_passphrase() {
        assert!(spoken_steps("unlock correct horse, then battery and staple", &parser()).is_none());
    }

    #[test]
    fn splitter_a_bare_and_does_not_start_a_pause() {
        let s = spoken_steps("open chrome and wait", &parser());
        assert!(s.as_ref().map(|v| v.len() < 2).unwrap_or(true), "{s:?}");
    }

    fn known_parser() -> Parser {
        let cfg = crate::config::Config::load(std::path::Path::new("config"))
            .unwrap_or_else(|e| panic!("config: {e}"));
        let mut p = Parser::new(&cfg.commands);
        p.know_names(crate::intent::KnownNames {
            apps: cfg.apps.apps.keys().cloned().collect(),
            modes: Vec::new(),
            peers: Vec::new(),
        });
        p
    }

    #[test]
    fn splitter_never_makes_up_an_app_from_a_cut() {
        let p = known_parser();
        // "oh" is not an app, and neither is "play some music".
        for said in [
            "open notepad, oh and also open chrome",
            "open chrome and play some music",
            "hey so could you please, like, open notepad",
        ] {
            let got = spoken_steps(said, &p).unwrap_or_default();
            assert!(
                got.iter()
                    .all(|s| p.names_something_known(match &s.reading {
                        Reading::Command(i) => i,
                        _ => &Intent::Clock,
                    })),
                "{said}: {got:?}"
            );
            assert!(
                !got.iter().any(|s| s.words.to_lowercase().contains("oh")
                    || s.words.to_lowercase().contains("music")
                    || s.words.to_lowercase().contains("like")),
                "{said}: {got:?}"
            );
        }
    }

    #[test]
    fn splitter_a_bare_no_inside_a_reminder_is_not_a_correction() {
        let p = parser();
        // "say no to Bob" is what to be reminded of.
        assert!(spoken_steps("remind me to say no to bob at 5", &p).is_none());
    }

    #[test]
    fn splitter_a_correction_replaces_the_words_it_repeats() {
        assert_eq!(
            steps("remind me at 5, no wait, at 6 to call mom"),
            ["remind me at 6 to call mom"]
        );
    }

    #[test]
    fn splitter_puts_a_fronted_thing_back_in_order() {
        assert_eq!(steps("chrome, open it"), ["open chrome"]);
        assert_eq!(
            steps("at 5 remind me to call mom"),
            ["remind me at 5 to call mom"]
        );
        assert_eq!(
            steps("on the left monitor, put chrome"),
            ["put chrome on the left monitor"]
        );
    }

    #[test]
    fn splitter_the_same_thing_asked_twice_is_one_request() {
        // Eric, 29 Sep 2026: one camera question, said two ways.
        assert!(spoken_steps("Are you using my camera? Can you see me?", &parser()).is_none());
    }

    #[test]
    fn splitter_takes_a_courtesy_off_a_name() {
        assert_eq!(steps("can you open chrome thanks"), ["open chrome"]);
    }

    #[test]
    fn splitter_free_text_still_cuts_at_an_explicit_then() {
        assert_eq!(
            steps("set a reminder for 6 to eat and then open notepad"),
            ["set a reminder for 6 to eat", "open notepad"]
        );
    }

    #[test]
    fn splitter_reminder_wordings_are_one_list() {
        assert_eq!(
            reminder_restated("set a reminder for 5 to stretch").as_deref(),
            Some("remind me 5 to stretch")
        );
        assert!(reminder_restated("open chrome").is_none());
    }
}
