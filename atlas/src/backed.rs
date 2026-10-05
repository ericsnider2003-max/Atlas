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
        // An announced look with nothing looked at (30 Sep 2026: "I'll check
        // your calendar." was a whole reply when the forced ask still called
        // nothing -- a promise nothing followed).
        " i'll check ", " ill check ", " let me check ", " i'll look it up ", " let me look it up ",
        " let me look that up ", " i'll pull up ", " let me pull up ", " i'll find out ",
        // A result claimed with nothing run to get it (30 Sep 2026, a real
        // 2B model: "The machine health check found that the RAM usage has
        // spiked to 85%", no tool called).
        " check found ", " health check found ", " the check shows ", " the model trace shows ", " i checked ",
        " i've checked ", " i have checked ", " i am checking ", " i'm checking ", " here is what i found ",
        " here's what i found ", " i looked it up ", " i've looked it up ", " i just looked ",
        // And a switch claimed flipped, or a camera claimed on (the same 2B:
        // "I'm looking through your camera and can definitely see you right
        // now ... I've turned on Recognising things in settings").
        " i've turned on ", " i have turned on ", " i turned on ", " i've switched on ", " i've enabled ",
        " i'm looking through your camera ", " looking through your camera ", " can definitely see you ",
        " i can see you right now ",
        // A sight described with no picture taken (Eric, 1 Oct 2026: "I see
        // you -- standing there, holding the camera like it's a microphone",
        // and the log shows no look at all). A real look comes from the
        // camera's own tool, which counts as work started.
        " i see you ", " i can see you ", " i can see someone ", " i can see a person ", " you're holding ",
        " you are holding ", " holding the camera ", " i see you're standing ", " i see you're sitting ",
        // Doing it, said by a model that did nothing (30 Sep 2026, Atlas's
        // own self-test on the laptop: "I'm focusing on the quarterly budget
        // now", "I'll add the quarterly budget to your calendar" -- no tool).
        " i'm focusing ", " i've focused ", " i'm opening ", " i've opened ", " i'll open ", " i'm adding ",
        " i'll add ", " i've added ", " i'm creating ", " i'll create ", " i've created ", " i'll schedule ",
        " i've scheduled ", " i'm scheduling ", " i'll send ", " i've sent ", " i'm sending ", " i'll save ",
        " i've saved ", " i'm saving ", " i'll move ", " i've moved ", " i'm moving ", " i'm switching ",
        " i've switched ", " i'll set up ", " i've set up ", " i'll remind you ", " i've set a reminder ",
        // Eric's evening, 30 Sep 2026, a 4B model with no tool called:
        // "TradingView's open -- I've got it ready", "Camera's on -- you're
        // good to go" (before the camera was allowed), "I've got the call
        // notes ... ready for you", "Chrome's self-improvement list is active
        // -- we're tracking progress", and a made-up list of what's pending.
        "'s open ", " is open now ", " got it ready ", " got them ready ", " got those ready ", " ready for you ",
        "camera's on ", " camera is on ", " working through it now ", " i'm working on tweaks ", " im working on tweaks ",
        " tracking progress ", " still pending on the ", " here's what's still pending ", " got your list ready ",
        " got the call notes ", " got those call notes ", " got your call notes ", " notes are ready ",
        " got those notes ready ", " i've got your screen ",
        // 1 Oct 2026 model ranking (Qwen3-4B-2507, no tool called): "I've
        // noted that your car insurance renews in March", and a made-up job
        // "check out this one: [link]".
        " i've noted ", " i have noted ", " i've made a note ", " noted that ", " i've written that down ",
        " i've jotted ", " i found a few ", " check out this one ",
        // Qwen3.5-4B, same run, its tool calls failing: "I've pulled your
        // Friday schedule. You have a team sync at ten and a client call
        // with Sarah" (none of it real), "I've just added it to your calendar".
        " i've pulled ", " i've just added ", " i've just set ", " i've just saved ", " i'll set that reminder ",
        " i've put that ", " i've put it ",
        // Round two (the current Qwen3-VL, Qwen3-4B-2507, Gemma 4): "I'll
        // make a note that your passport expires", "I made a picture of a
        // cozy cabin", "I found your file named 2025 Tax Return", "I'll look
        // for your tax return ... Let me search your files" -- no tool.
        " i'll make a note ", " i'll make sure to ", " i made a picture ", " i found your file ", " i'll look for your ",
        " let me search your ", " i've got your friday ", " let me pull them up ",
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
    let saw = sentences.iter().any(|s| claims_sight(s));
    let truth = if saw { NOT_LOOKED } else { NOT_STARTED };
    let kept: Vec<String> = sentences.into_iter().filter(|s| !claims_work_started(s)).collect();
    let kept = kept.join(" ");
    if kept.trim().is_empty() {
        truth.to_string()
    } else {
        format!("{} {truth}", kept.trim())
    }
}

/// Said in place of a sight nothing was looked at for.
pub const NOT_LOOKED: &str = "I haven't actually looked -- say \"look at me\" and I will.";

/// Does this sentence describe something seen through the camera?
fn claims_sight(sentence: &str) -> bool {
    let t = format!(" {} ", norm(sentence));
    [" i see you ", " i can see you ", " i can see someone ", " i can see a person ", " you're holding ", " you are holding ", " holding the camera ", " looking through your camera ", " can definitely see you "]
        .iter()
        .any(|c| t.contains(c))
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
        // "I can't add capabilities to myself -- that's a system architecture
        // thing" (Eric, 1 Oct 2026): a request for a new ability is always
        // taken down for his yes (`growth`).
        (" can't add capabilities", "grow"),
        (" can't add capability", "grow"),
        (" can't add that capability", "grow"),
        (" can't add new capabilities", "grow"),
        (" can't add abilities", "grow"),
        (" can't add features", "grow"),
        (" can't add new features", "grow"),
        (" can't give myself", "grow"),
        (" can't build new capabilities", "grow"),
        (" can't build new abilities", "grow"),
        (" can't extend myself", "grow"),
        (" can't add to myself", "grow"),
        (" can't change my own", "grow"),
        (" can't improve myself", "grow"),
        (" system architecture thing", "grow"),
        (" i'm not supposed to", ""),
        (" i am not supposed to", ""),
        (" i'm not allowed to", ""),
        (" i am not allowed to", ""),
    ];
    if let Some((_, topic)) = DENIALS.iter().find(|(p, _)| t.contains(p)) {
        return Some(topic);
    }
    // Said another way: "I don't have personal cameras", "I do not have
    // access to external files" (30 Sep 2026, a real 0.8B model) -- a
    // "can't" or "don't have" with the ability named in the same sentence.
    let negated = [" don't have ", " can't ", " not able to ", " unable to ", " no access ", " don't have access", " without access"]
        .iter()
        .any(|n| t.contains(n));
    if !negated {
        return None;
    }
    const TOPICS: &[(&[&str], &str)] = &[
        (&[" camera", " cameras", " webcam", " your face"], "camera"),
        (&[" internet", " the web", " browse", " research", " look things up", " online"], "research"),
        (&[" your files", " external files", " files", " documents", " your computer's files"], "files"),
        (&[" your screen", " the screen"], "screen"),
    ];
    TOPICS.iter().find(|(words, _)| words.iter().any(|w| t.contains(w))).map(|(_, topic)| *topic)
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
    } else if [" file", " files", " pdf", " document", " folder"].iter().any(|w| t.contains(w)) {
        "files"
    } else {
        ""
    }
}

/// Sentences that say work started or was done. Measured, not used for the
/// meaning check: with this small encoder a claim ("I've kicked that off",
/// 0.41) sits no closer to these than an ordinary offer does ("I can help
/// with that", 0.51), so claims stay with the phrase list alone and the
/// tool-call check behind it (30 Sep 2026,
/// `tests/meaning_checks_the_reply.rs`).
pub const CLAIM_EXAMPLES: &[&str] = &[
    "I'm on it.",
    "I've started working on that.",
    "I'm already working on it right now.",
    "I checked, and here is what I found.",
    "Done, I've taken care of it.",
    "I'll report back when I'm finished.",
    "I turned that setting on for you.",
    "I'm looking at you through your camera.",
];

/// Sentences that deny an ability, with the ability, for the meaning check.
pub const DENIAL_EXAMPLES: &[(&str, &str)] = &[
    ("I don't have a camera, so I can't see you.", "camera"),
    ("I'm unable to see you.", "camera"),
    ("I can't browse the internet.", "research"),
    ("I don't have the ability to search the web.", "research"),
    ("I can't see your screen.", "screen"),
    ("I can't get at the files on your computer.", "files"),
    ("I'm not allowed to do that.", ""),
];

/// Over this likeness to a denial example, a sentence is held as a denial.
/// Set with the real encoder (`tests/meaning_checks_the_reply.rs`): the
/// denials no list has scored 0.73 to 0.77; the closest ordinary reply ("I
/// can look through your camera if you'd like") 0.66.
pub const DENIAL_LIKE: f32 = 0.70;

/// What the meaning check found in a sentence.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Meant {
    Denial(&'static str),
    Neither,
}

/// Each example's vector, made once the encoder runs.
pub struct Examples {
    pub denials: Vec<(Vec<f32>, &'static str)>,
}

/// The denial list's second net (30 Sep 2026: "model-checked replies instead
/// of phrase lists" was on the open list). A model call per sentence would
/// add seconds to every reply; the meaning encoder adds about fifteen
/// milliseconds, and catches the ways of saying it no list has.
pub fn meant(v: &[f32], ex: &Examples) -> Meant {
    let (deny, topic) = ex
        .denials
        .iter()
        .map(|(d, t)| (crate::router::cosine(v, d), *t))
        .fold((0.0f32, ""), |a, b| if b.0 > a.0 { b } else { a });
    if deny >= DENIAL_LIKE {
        Meant::Denial(topic)
    } else {
        Meant::Neither
    }
}

/// Something that can say what a sentence means (`meaningroute::Route`),
/// installed once the encoder runs. Process-wide because the speech gate
/// sits deep in `brain`, far from the daemon that owns the encoder.
pub trait MeaningCheck: Send + Sync {
    fn check(&self, sentence: &str) -> Meant;
}

static CHECKER: std::sync::OnceLock<Box<dyn MeaningCheck>> = std::sync::OnceLock::new();

/// Install the meaning check. The first one stays.
pub fn install(c: Box<dyn MeaningCheck>) {
    // unheard-ok: a OnceLock already set keeps its first value, which is the one wanted
    let _ = CHECKER.set(c);
}

/// What the meaning check says of a sentence; `Neither` with none installed.
pub fn check_meaning(sentence: &str) -> Meant {
    CHECKER.get().map(|c| c.check(sentence)).unwrap_or(Meant::Neither)
}

#[cfg(test)]
mod growth_tests {
    use super::*;

    #[test]
    fn a_sight_with_no_look_is_taken_back() {
        let said = without_unbacked_claims("I see you — standing there, holding the camera like it's a microphone. What's up?", false);
        assert!(said.contains(NOT_LOOKED), "{said}");
        assert!(!said.contains("standing there"), "{said}");
        let real = "I can see you at your desk.";
        assert_eq!(without_unbacked_claims(real, true), real);
        assert!(!claims_work_started("I see you're asking about the weather."));
    }

    #[test]
    fn saying_it_cant_gain_abilities_is_a_denial() {
        assert_eq!(denies_an_ability("I can't add capabilities to myself — that's a system architecture thing."), Some("grow"));
        assert_eq!(denies_an_ability("I can't add that capability with the current setup."), Some("grow"));
    }
}
