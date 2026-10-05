//! How you talk: the words you use for things, learned from being corrected
//! (2 Oct 2026, "Atlas doesn't really understand me").
//!
//! Every time Atlas got a sentence wrong it got it wrong for good. You said
//! "play some tunes", it didn't know, you said "open Spotify", that worked --
//! and the next "play some tunes" went to the model to be guessed at all over
//! again. A person learns your words after being told once. This is that
//! list: the wording that missed, and the action that turned out to be right.
//!
//! **How a wording gets in.** Three ways, each read from what actually
//! happened rather than from anyone's say-so:
//!
//! - **You corrected it** -- "no, I meant open Spotify", "I said check my
//!   calendar". What Atlas then did with the corrected words is the action.
//!   Sure from the start.
//! - **You answered "what should I have done instead?"** with something Atlas
//!   can do. Sure, the same way.
//! - **You said it differently, straight after a miss**, and the second way
//!   worked. Kept, but not *sure*: a rephrase might be you giving up and
//!   asking for something else. Until it has been used once without you
//!   undoing it, a consequential action it leads to is still asked about.
//!
//! **How it is used.** Before the model guesses at a sentence the phrases
//! didn't recognise, it is looked up here: exactly, then with the polite
//! words and punctuation taken off, then by the words that carry the meaning
//! (so "play me some tunes" finds "play some tunes"), then -- when the
//! meaning model is running -- by meaning. A close enough match is routed
//! straight to the action, with no model call; a looser one goes to the model
//! as a hint ("when they said X before, they meant Y").
//!
//! **What is never learned.** Anything that isn't an action (a chat reply, a
//! question back, "undo"), and the vault's passphrase in any form. A
//! correction while Atlas is handed over to somebody else teaches nothing:
//! their words are not yours.
//!
//! **Undoing it.** "What have you learned about how I talk" reads the list;
//! "forget that phrase" drops the last one used or learned; "forget the
//! phrase play some tunes" drops that one; the Improvements page lists them
//! with a Forget button each. Being undone or corrected straight after a
//! learned route drops it too -- a wrong lesson does not get a second go.
//!
//! Kept in the store, so each Atlas -- yours, a friend's -- learns its own
//! person and nobody else's.

use crate::intent::{Intent, Parser};
use serde::{Deserialize, Serialize};

/// Where it is kept, in the store.
pub const FILE: &str = "phrasebook";
/// The most wordings kept. The least used, oldest go first past this.
pub const MOST_KEPT: usize = 300;
/// A match this sure is routed without the model.
pub const ROUTE_AT: f32 = 0.86;
/// A match this sure goes to the model as a hint, below routing.
pub const HINT_AT: f32 = 0.5;
/// Meaning vectors this close count as the same request. Set high on
/// purpose: two requests that *mean* nearly the same can still want different
/// things ("open my notes" / "open my mail").
pub const SAME_MEANING: f32 = 0.92;

/// How a wording was learned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Taught {
    /// "No, I meant ..."
    Corrected,
    /// The answer to "what should I have done instead?".
    Answered,
    /// Said again differently straight after a miss, and the second worked.
    Rephrased,
}

impl Taught {
    pub fn plain(self) -> &'static str {
        match self {
            Taught::Corrected => "you corrected me",
            Taught::Answered => "you told me what I should have done",
            Taught::Rephrased => "you said it another way and that worked",
        }
    }
}

/// One wording and what it means.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Phrase {
    /// What you said that Atlas missed, as you said it.
    pub wording: String,
    /// The words that then worked.
    pub meant: String,
    /// The command it turned out to be (`open_app`), for `intent::from_tool`.
    /// Empty when the command is only reachable by reading `meant` again.
    pub tool: String,
    /// Its argument ("spotify").
    pub arg: String,
    /// What the action is called, for reading the list back.
    pub action: String,
    pub taught: Taught,
    /// False for a rephrase not yet used without being undone: a
    /// consequential action it leads to is asked about first.
    pub sure: bool,
    pub learned_at: u64,
    #[serde(default)]
    pub used: u32,
    #[serde(default)]
    pub last_used: u64,
    /// The wording's meaning vector, when the meaning model was running.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub meaning: Vec<f32>,
}

/// A wording found for a sentence, and how sure the match is.
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    pub phrase: Phrase,
    pub score: f32,
    /// "the same words", "the same words, give or take", ...
    pub how: &'static str,
}

/// Your phrasebook.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Phrasebook {
    pub phrases: Vec<Phrase>,
}

impl Phrasebook {
    pub fn load(store: &crate::store::Store) -> Phrasebook {
        store.load(FILE)
    }

    pub fn save(&self, store: &crate::store::Store) -> crate::error::Result<()> {
        store.save(FILE, self)
    }

    /// Keep a wording. The same wording learned again replaces what it meant
    /// (you corrected the correction); a sure lesson is never made unsure by
    /// a later rephrase of the same thing. True when something changed.
    pub fn keep_phrase(&mut self, p: Phrase) -> bool {
        let key = phrase_key(&p.wording);
        if key.is_empty() {
            return false;
        }
        if let Some(old) = self.phrases.iter_mut().find(|o| phrase_key(&o.wording) == key) {
            if old.tool == p.tool && old.arg == p.arg && old.meant == p.meant {
                let was_sure = old.sure;
                old.sure = old.sure || p.sure;
                if old.meaning.is_empty() {
                    old.meaning = p.meaning;
                }
                return old.sure != was_sure;
            }
            let sure = p.sure || (old.sure && old.tool == p.tool);
            *old = Phrase { sure, ..p };
            return true;
        }
        self.phrases.push(p);
        if self.phrases.len() > MOST_KEPT {
            // The least used go first, and of those the oldest.
            let worst = self
                .phrases
                .iter()
                .enumerate()
                .min_by_key(|(_, x)| (x.used, x.last_used.max(x.learned_at)))
                .map(|(i, _)| i);
            if let Some(i) = worst {
                self.phrases.remove(i);
            }
        }
        true
    }

    /// The closest wording to `said`, if any is close enough to be worth a
    /// hint. `meaning` is the sentence's vector when the meaning model gave
    /// one in time.
    pub fn closest(&self, said: &str, meaning: Option<&[f32]>) -> Option<Found> {
        let key = phrase_key(said);
        if key.is_empty() {
            return None;
        }
        let raw = said.trim().to_lowercase();
        let mut best: Option<Found> = None;
        for p in &self.phrases {
            let (score, how) = if p.wording.trim().to_lowercase() == raw {
                (1.0, "the same words")
            } else if phrase_key(&p.wording) == key {
                (0.97, "the same words, give or take")
            } else {
                let words = phrase_overlap(&p.wording, said) * 0.95;
                let meant = match meaning {
                    Some(v) if !p.meaning.is_empty() && p.meaning.len() == v.len() => {
                        let c = crate::router::cosine(v, &p.meaning);
                        // Scaled so SAME_MEANING lands on ROUTE_AT: a cosine
                        // under it can still be a hint, never a route.
                        if c >= SAME_MEANING { ROUTE_AT + (c - SAME_MEANING) } else { c * (ROUTE_AT / SAME_MEANING) - 0.1 }
                    }
                    _ => 0.0,
                };
                if meant > words {
                    (meant, "what it means")
                } else {
                    (words, "the words that matter")
                }
            };
            if score >= HINT_AT && best.as_ref().is_none_or(|b| score > b.score) {
                best = Some(Found { phrase: p.clone(), score, how });
            }
        }
        best
    }

    /// Drop the wording that reads as `wording`. What was dropped.
    pub fn forget_wording(&mut self, wording: &str) -> Option<Phrase> {
        let key = phrase_key(wording);
        let i = self.phrases.iter().position(|p| phrase_key(&p.wording) == key)?;
        Some(self.phrases.remove(i))
    }

    /// The wording used or learned most recently: what "forget that phrase"
    /// means.
    pub fn latest_wording(&self) -> Option<String> {
        self.phrases.iter().max_by_key(|p| p.last_used.max(p.learned_at)).map(|p| p.wording.clone())
    }

    /// A wording was used to route a sentence.
    pub fn used_now(&mut self, wording: &str, t: u64) {
        let key = phrase_key(wording);
        if let Some(p) = self.phrases.iter_mut().find(|p| phrase_key(&p.wording) == key) {
            p.used = p.used.saturating_add(1);
            p.last_used = t;
        }
    }

    /// A route that wasn't undone or corrected: a rephrase becomes sure.
    pub fn stood_up(&mut self, wording: &str) {
        let key = phrase_key(wording);
        if let Some(p) = self.phrases.iter_mut().find(|p| phrase_key(&p.wording) == key) {
            p.sure = true;
        }
    }

    /// The list, read back: what each wording means and how it was learned.
    pub fn listing(&self, most: usize) -> String {
        if self.phrases.is_empty() {
            return "Nothing yet. When I get something wrong, tell me what you meant -- \"no, I meant open my calendar\" \
                    -- and I'll remember those words for next time."
                .into();
        }
        let mut v: Vec<&Phrase> = self.phrases.iter().collect();
        v.sort_by_key(|p| std::cmp::Reverse(p.last_used.max(p.learned_at)));
        let lines: Vec<String> = v
            .iter()
            .take(most)
            .map(|p| {
                format!(
                    "\"{}\" means \"{}\"{} ({}{})",
                    p.wording.trim(),
                    p.meant.trim(),
                    if p.sure { "" } else { ", still checking" },
                    p.taught.plain(),
                    match p.used {
                        0 => String::new(),
                        1 => ", used once".into(),
                        n => format!(", used {n} times"),
                    }
                )
            })
            .collect();
        let more = v.len().saturating_sub(most);
        format!(
            "I've learned {} way{} you put things: {}.{} Say \"forget the phrase\" and the words to drop one.",
            v.len(),
            if v.len() == 1 { "" } else { "s" },
            lines.join("; "),
            if more > 0 { format!(" And {more} more on the Improvements page.") } else { String::new() }
        )
    }
}

/// A wording reduced to what decides it: lower case, no punctuation, no
/// polite words or Atlas's name at either end. "Atlas, can you play some
/// tunes please?" and "play some tunes" have the same key.
pub fn phrase_key(said: &str) -> String {
    let n = crate::intent::normalize(said);
    let mut t = crate::intent::without_fillers(&n);
    for lead in ["can you ", "could you ", "would you ", "will you ", "i want you to ", "i need you to ", "go ahead and "] {
        if let Some(r) = t.strip_prefix(lead) {
            t = crate::intent::without_fillers(r);
        }
    }
    t
}

/// Words that don't decide what a request is, for the overlap. Not
/// `router`'s conversational list: that drops "make", "keep" and "go", which
/// do decide a command.
const NOT_DECIDING: &[&str] = &[
    "a", "an", "the", "my", "me", "i", "you", "your", "it", "this", "that", "some", "any", "please", "for", "to",
    "of", "on", "in", "up", "just", "can", "could", "would", "will", "atlas", "now", "hey", "ok", "okay", "yeah",
    "so", "and", "thanks", "real", "quick", "quickly", "bit", "little", "from", "with", "by", "at", "about",
];

/// How much two wordings share of the words that decide them, 0 to 1
/// (Jaccard over stemmed words). Two words or more each, or it is only an
/// exact match: "open it" and "open them" are not the same request.
fn phrase_overlap(a: &str, b: &str) -> f32 {
    let bag = |s: &str| -> std::collections::BTreeSet<String> {
        crate::intent::normalize(s)
            .split_whitespace()
            .filter(|w| !NOT_DECIDING.contains(w))
            // A closure, not `.map(stem)`: the guards find a call by `name(`.
            .map(crate::stemmer::stem)
            .collect()
    };
    let (x, y) = (bag(a), bag(b));
    if x.len() < 2 || y.len() < 2 {
        return 0.0;
    }
    let both = x.intersection(&y).count() as f32;
    let either = x.union(&y).count() as f32;
    both / either
}

/// Whether an action may be learned as what a wording means. Only real
/// actions: not conversation, not a question back, not an undo or a
/// complaint, and never the vault.
pub fn learnable_action(intent: &Intent) -> bool {
    !matches!(
        intent,
        Intent::Unknown(_)
            | Intent::Say(_)
            | Intent::Ask(_)
            | Intent::Undo
            | Intent::GotItWrong(_)
            | Intent::ApplyLesson
            | Intent::Why(_)
            | Intent::History(_)
            | Intent::Unlock(_)
            | Intent::Pause
            | Intent::Resume
    ) && !crate::intent::NEVER_FOR_THE_MODEL.contains(&crate::session::kind_of(intent))
}

/// The command and argument an intent is, checked by building it back: the
/// pair is kept only when `intent::from_tool` makes exactly this intent again
/// from it, so a route can never land somewhere the lesson didn't. `meant` is
/// the sentence that produced it, which commands reading the whole sentence
/// are given.
pub fn intent_as_tool(intent: &Intent, meant: &str) -> Option<(String, String)> {
    let name = crate::session::kind_of(intent).to_string();
    // The argument from the intent's own shape: `OpenApp("spotify")`, or a
    // bare `Pause`. Anything richer isn't a single argument and is left to
    // reading `meant` again.
    let shown = format!("{intent:?}");
    let arg = match shown.find('(') {
        None => String::new(),
        Some(open) => {
            let inner = shown[open + 1..].strip_suffix(')')?;
            if !inner.starts_with('"') {
                return None;
            }
            serde_json::from_str::<String>(inner).ok()?
        }
    };
    let rebuilt = crate::intent::from_tool(&name, &serde_json::Value::String(arg.clone()), meant)?;
    (rebuilt == *intent).then_some((name, arg))
}

/// What a learned wording routes to now, without the model: the command
/// built from its name and argument, or failing that the words that worked,
/// read by the phrases again. `None` when neither gives the action back --
/// then it can only be a hint.
pub fn phrase_route(p: &Phrase, parser: &Parser) -> Option<Intent> {
    if !p.tool.is_empty() {
        if let Some(i) = crate::intent::from_tool(&p.tool, &serde_json::Value::String(p.arg.clone()), &p.meant) {
            return learnable_action(&i).then_some(i);
        }
    }
    match parser.parse(&p.meant) {
        Intent::Unknown(_) => None,
        i => learnable_action(&i).then_some(i),
    }
}

/// What was asked about the phrasebook itself.
#[derive(Debug, Clone, PartialEq)]
pub enum Asked {
    /// "What have you learned about how I talk?"
    List,
    /// "Forget that phrase."
    ForgetLatest,
    /// "Forget the phrase play some tunes."
    Forget(String),
    /// "Forget everything you've learned about how I talk."
    ForgetAll,
}

/// Is this a question or an instruction about the phrasebook? Narrow shapes
/// only.
pub fn asked_about_phrasebook(said: &str) -> Option<Asked> {
    let t = crate::intent::normalize(said);
    let t = crate::intent::without_fillers(&t);
    const LIST: &[&str] = &[
        "what have you learned about how i talk",
        "what have you learnt about how i talk",
        "what have you learned about the way i talk",
        "what have you learned about how i say things",
        "what phrases have you learned",
        "what phrases have you learnt",
        "what words have you learned",
        "show me my phrasebook",
        "show my phrasebook",
        "read me my phrasebook",
        "whats in my phrasebook",
        "what do you know about how i talk",
    ];
    if LIST.contains(&t.as_str()) {
        return Some(Asked::List);
    }
    const ALL: &[&str] = &[
        "forget everything you learned about how i talk",
        "forget everything youve learned about how i talk",
        "forget all the phrases",
        "forget all my phrases",
        "clear my phrasebook",
        "empty my phrasebook",
    ];
    if ALL.contains(&t.as_str()) {
        return Some(Asked::ForgetAll);
    }
    if matches!(t.as_str(), "forget that phrase" | "forget that wording" | "unlearn that" | "dont learn that" | "forget what that meant") {
        return Some(Asked::ForgetLatest);
    }
    for lead in ["forget the phrase ", "forget the wording ", "unlearn the phrase ", "forget what i mean by ", "forget what i meant by "] {
        if let Some(rest) = t.strip_prefix(lead) {
            let rest = rest.trim();
            if !rest.is_empty() {
                // From the sentence as said, so the reply quotes your words.
                return Some(Asked::Forget(rest.to_string()));
            }
        }
    }
    None
}

/// "No, I meant open Spotify", "I said check my calendar", "I was asking you
/// to start the timer": the words you meant, or `None` when this isn't a
/// correction of that shape. "Actually" and "correction" are left to the
/// fact book (`learn_stated`): "actually my car is a Toyota" is about your
/// car, not about a sentence of yours.
pub fn meant_instead(said: &str) -> Option<String> {
    let t = said.trim();
    let low = t.to_lowercase();
    // "Atlas," and a "no" in front are the same correction.
    let mut at = 0;
    for lead in ["atlas,", "atlas", "no no,", "no no", "no,", "no", "nope,", "nope", "sorry,", "sorry"] {
        if low[at..].starts_with(lead) && low[at + lead.len()..].starts_with([' ', ',']) {
            at += lead.len();
            at += low[at..].len() - low[at..].trim_start_matches([' ', ',']).len();
        }
    }
    let rest_low = &low[at..];
    for lead in [
        "i meant ",
        "i mean ",
        "what i meant was ",
        "what i meant is ",
        "i said ",
        "what i said was ",
        "i was asking you to ",
        "i asked you to ",
        "i wanted you to ",
        "i want you to ",
    ] {
        if let Some(r) = rest_low.strip_prefix(lead) {
            // Your own casing when lower-casing kept every byte where it was;
            // otherwise the lower-cased words, which mean the same.
            let words = if low.len() == t.len() { &t[t.len() - r.len()..] } else { r };
            let words = words.trim().trim_matches(['"', '\'']).trim_end_matches(['.', '!']).trim();
            // At least something to do: "I meant it" is not a correction.
            if words.split_whitespace().count() >= 2 || (words.len() >= 4 && !matches!(words.to_lowercase().as_str(), "that" | "this" | "it")) {
                return Some(words.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_polite_words_do_not_change_the_key() {
        assert_eq!(phrase_key("Atlas, can you play some tunes please?"), phrase_key("play some tunes"));
    }

    #[test]
    fn the_words_that_matter_find_a_near_wording() {
        assert!(phrase_overlap("play me some tunes", "play some tunes") >= 0.99);
        assert!(phrase_overlap("play some tunes by drake", "play some tunes") < ROUTE_AT);
        assert_eq!(phrase_overlap("open it", "open them"), 0.0);
    }

    #[test]
    fn a_correction_gives_the_words_meant() {
        assert_eq!(meant_instead("No, I meant open Spotify").as_deref(), Some("open Spotify"));
        assert_eq!(meant_instead("i said check my calendar.").as_deref(), Some("check my calendar"));
        assert_eq!(meant_instead("no I meant it"), None);
        assert_eq!(meant_instead("what does it mean"), None);
    }
}
