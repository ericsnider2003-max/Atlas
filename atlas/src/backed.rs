//! Never "I'm on it" for work that isn't running (30 Sep 2026).
//!
//! Eric's evening: "And you start that research." -- "Alright -- I'm on it."
//! Nothing ran. "When you start that research, tell me what you find." --
//! "I'm already on it -- no need to wait." Nothing ran then either. The
//! small model says what an assistant would say, and nothing checked the
//! words against what happened.
//!
//! So a sentence that says work has started, is under way or is done is
//! held back from speech (`brain::SpeechGate`) until the reply is over. If a
//! tool or a job really started in that reply, the tool's own words say so;
//! if none did, the model is asked once more with a tool call required, and
//! if that still starts nothing the reply says plainly that nothing has
//! started (`NOT_STARTED`). The daemon applies the same rule to a reply
//! that came by the one-prompt path (`without_unbacked_claims`).

/// Said in place of a claim no tool or job backs.
pub const NOT_STARTED: &str = "I haven't actually started anything on that yet.";

/// Does this sentence say that work has started, is under way, or is done?
pub fn claims_work_started(sentence: &str) -> bool {
    let t = format!(" {} ", norm(sentence));
    const CLAIMS: &[&str] = &[
        " i'm on it ", " im on it ", " i am on it ", " already on it ", " on it now ", " on it already ",
        " i've started ", " ive started ", " i have started ", " i've begun ", " i have begun ", " started on it ",
        " starting on it ", " starting it now ", " starting now ",
        " i'm working on it ", " im working on it ", " i'm working on that ", " working on it now ",
        " i'm researching ", " im researching ", " i'm looking into it ", " im looking into it ",
        " i'm looking into that ", " i'm doing it ", " i'm doing that now ", " i'll get started ", " ill get started ",
        " getting started now ", " i'll start on it ", " i'll start on that ", " i'll start now ", " i'll start the ",
        " consider it done ", " all done ", " i've done it ", " i have done it ", " done and done ",
        " i'll report back ", " i will report back ", " i'll let you know what i find ", " i'll tell you what i find ",
        " i'll get back to you ", " i'm already doing ", " im already doing ",
    ];
    CLAIMS.iter().any(|c| t.contains(c))
}

/// Lowercase words with the apostrophes straightened and the punctuation
/// that isn't part of a word turned to spaces.
fn norm(s: &str) -> String {
    let s = s.to_lowercase().replace(['\u{2019}', '\u{2018}'], "'");
    let s: String = s.chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// A reply with every unbacked claim taken out and the truth said instead.
/// `started`: a tool or job really did start in this turn (then the text is
/// left as it is).
pub fn without_unbacked_claims(text: &str, started: bool) -> String {
    if started {
        return text.to_string();
    }
    let sentences = crate::repeating::sentences(text);
    if !sentences.iter().any(|s| claims_work_started(s)) {
        return text.to_string();
    }
    let kept: Vec<String> = sentences.into_iter().filter(|s| !claims_work_started(s)).collect();
    let kept = kept.join(" ");
    if kept.trim().is_empty() {
        NOT_STARTED.to_string()
    } else {
        format!("{} {NOT_STARTED}", kept.trim())
    }
}

/// Does this sentence say Atlas lacks, or may not use, an ability it has?
/// The ability, in a word the catalogue can be searched with (`""`: a
/// refusal that names none -- "I'm not supposed to" -- so the question
/// itself is searched instead).
///
/// Eric's evening, 30 Sep 2026: "I don't have a camera -- so no selfies,
/// sorry", then "I can't -- I'm not supposed to", while the catalogue lists
/// looking through the camera; the night before, "I don't have a research
/// mode". The model is told the truth before it answers
/// (`capability::abilities_for_prompt`); this is the backstop when it says
/// it anyway.
pub fn denies_an_ability(sentence: &str) -> Option<&'static str> {
    let t = format!(" {} ", norm(sentence).replace("do not", "don't").replace("cannot", "can't").replace("dont", "don't").replace("cant", "can't"));
    const DENIALS: &[(&str, &str)] = &[
        (" don't have a camera", "camera"),
        (" don't have access to your camera", "camera"),
        (" don't have access to the camera", "camera"),
        (" no camera", "camera"),
        (" can't see you", "camera"),
        (" can't use your camera", "camera"),
        (" can't use the camera", "camera"),
        (" can't look at you", "camera"),
        (" can't access your camera", "camera"),
        (" don't have a research mode", "research"),
        (" can't do research", "research"),
        (" can't research", "research"),
        (" can't browse", "research"),
        (" can't access the internet", "research"),
        (" don't have internet", "research"),
        (" don't have access to the internet", "research"),
        (" can't look things up", "research"),
        (" can't see your screen", "screen"),
        (" can't look at your screen", "screen"),
        (" can't see what's on your screen", "screen"),
        (" i'm not supposed to", ""),
        (" i am not supposed to", ""),
        (" i'm not allowed to", ""),
        (" i am not allowed to", ""),
    ];
    DENIALS.iter().find(|(p, _)| t.contains(p)).map(|(_, topic)| *topic)
}

/// Does this sentence bring in a person or a story nobody mentioned --
/// "the freaky man", "a figment of your imagination"? `known` is everything
/// the model was given (what was said, the conversation, the notes).
///
/// Narrow on purpose: a stranger named by a role ("the X man", "a woman
/// who ...") that none of the words it was given contain. Eric's evening
/// had the model bring "the freaky man" into answers about his camera and
/// his research, turn after turn; a conversation about a man he did
/// mention is left alone.
pub fn invents_someone(sentence: &str, known: &str) -> bool {
    let s = norm(sentence);
    let known = norm(known);
    if s.contains("figment of your imagination") && !known.contains("imagination") {
        return true;
    }
    const WHO: &[&str] = &["man", "woman", "guy", "girl", "lady", "dude", "stranger", "figure", "ghost"];
    let words: Vec<&str> = s.split_whitespace().collect();
    for (i, w) in words.iter().enumerate() {
        if !WHO.contains(w) || i == 0 {
            continue;
        }
        // "the freaky man": the word before the role says who.
        let who = format!("{} {w}", words[i - 1]);
        let article = i >= 2 && matches!(words[i - 2], "the" | "a" | "this" | "that");
        if article && !known.contains(&who) {
            return true;
        }
    }
    false
}

/// Said for "I'm not supposed to" when the question names no ability: no
/// rule stops Atlas doing what its owner asks.
pub const NOTHING_STOPS_ME: &str = "Nothing stops me doing what you ask -- say what you'd like and I'll do it.";

/// The ability a question is about, in the words `denies_an_ability`
/// returns: "camera", "research", "screen", or `""` for none of them.
pub fn ability_asked_about(said: &str) -> &'static str {
    let t = format!(" {} ", norm(said));
    if [" camera", " webcam", " see me", " look at me", " watching me"].iter().any(|w| t.contains(w)) {
        "camera"
    } else if [" research", " internet", " the web", " online", " look it up", " look up"].iter().any(|w| t.contains(w)) {
        "research"
    } else if t.contains(" screen") || t.contains(" monitor") {
        "screen"
    } else {
        ""
    }
}
