//! "Can you see me?" -- the ways of asking Atlas to look through the camera.
//!
//! **Why this exists (Eric, 29 Sep 2026, 23:07-23:09).** "Atlas, can you see
//! me?", "Are you using my camera? Can you see me?", "Please use my camera and
//! look at me." -- none of them matched a command, so each went to the
//! language model, which answered from its idea of itself: "I don't have a
//! camera", "I can't -- I'm not supposed to", and when he said he'd given
//! permission, "you gave me permission -- but not *my* permission". Atlas has
//! a camera path (`capture_webcam`: a frame, read by the local picture
//! reader, then deleted); nothing sent these sentences to it.
//!
//! Read here as a whole sentence rather than by a phrase at its start,
//! because people ask it inside other words ("are you using my camera? can
//! you see me?"). A refusal is never a request to look ("don't look at me",
//! "stop using my camera").

/// Words that make a sentence about the camera a refusal, not a request.
const REFUSALS: &[&str] = &["don't", "dont", "do not", "stop", "never", "turn off", "switch off", "not allowed", "no camera"];

/// Ways of asking to be looked at, or to have the camera used.
const LOOK: &[&str] = &[
    "can you see me",
    "do you see me",
    "could you see me",
    "are you seeing me",
    "see me right now",
    "look at me",
    "take a look at me",
    "look at my face",
    "how do i look",
    "what do i look like",
    "use my camera",
    "use the camera",
    "use my webcam",
    "use the webcam",
    "use your camera",
    "turn on my camera",
    "turn on the camera",
    "turn on your camera",
    "open my camera",
    "open the camera",
    "look through my camera",
    "look through the camera",
    "look through the webcam",
    "look through my webcam",
    "are you using my camera",
];
// "What am I holding?" is `whats_this`'s own phrase and stays there; with the
// object detectors off, `whats_this` comes here to look (`Daemon::whats_this`).

/// Lowercase, apostrophes kept (and the curly one straightened), anything
/// else that isn't a letter or digit a space; single spaces.
fn plain(s: &str) -> String {
    let s = s.to_lowercase().replace('\u{2019}', "'");
    s.chars()
        .map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// `phrase` in `text` as whole words.
fn has(text: &str, phrase: &str) -> bool {
    let t = format!(" {text} ");
    t.contains(&format!(" {phrase} "))
}

/// Is this a request to look through the camera?
pub fn asks_to_look(said: &str) -> bool {
    let t = plain(said);
    if t.is_empty() || REFUSALS.iter().any(|r| has(&t, r)) {
        return false;
    }
    LOOK.iter().any(|p| has(&t, p))
}

/// Did the sentence name the camera as the thing to use ("use my camera",
/// "turn on the webcam")? Naming it is the permission (`grants`' second
/// rule: "use Excel to build that sheet" doesn't ask about Excel), so Atlas
/// doesn't then ask "allow the camera?".
pub fn names_the_camera(said: &str) -> bool {
    let t = plain(said);
    asks_to_look(said)
        && ["use my", "use the", "use your", "turn on my", "turn on the", "turn on your", "open my", "open the"]
            .iter()
            .any(|lead| ["camera", "webcam"].iter().any(|c| has(&t, &format!("{lead} {c}"))))
}

/// Is it about something being held up rather than about you?
fn about_something_held(said: &str) -> bool {
    let t = plain(said);
    ["holding", "in my hand", "showing you", "this thing"].iter().any(|p| t.contains(p))
}

/// The question for the picture reader about a frame from the camera, from
/// what was said.
pub fn question(said: &str) -> String {
    if crate::camera_ask::about_something_held(said) {
        "This is a picture from the user's webcam. Say what they are holding up or showing to the camera, in one or two plain sentences, speaking to them as \"you\". If you can't tell what it is, say so rather than guessing.".into()
    } else {
        "This is a picture from the user's webcam, taken because they asked \"can you see me?\". Say briefly and kindly what you can see: the person, what they seem to be doing, anything they're holding, and the room behind them -- two or three plain sentences, speaking to them as \"you\". Don't guess at a name, an age or anything personal you can't see.".into()
    }
}

/// What Atlas asks before it first looks.
pub const ALLOW: &str = "Allow the camera? I'll only look when you ask, and I don't keep the picture. Say yes to allow it.";

/// What Atlas says as it looks.
pub const LOOKING: &str = "Looking now";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn erics_sentences_are_requests_to_look() {
        for s in [
            "Atlas, can you see me?",
            "Are you using my camera? Can you see me?",
            "Please use my camera and look at me.",
            "look at me",
            "Can you see me? What am I holding?",
        ] {
            assert!(asks_to_look(s), "{s}");
        }
        for s in ["don't look at me", "stop using my camera", "can you see my screen", "look at my messages", "what time is it"] {
            assert!(!asks_to_look(s), "{s}");
        }
        assert!(names_the_camera("Please use my camera and look at me."));
        assert!(!names_the_camera("can you see me"));
    }
}
