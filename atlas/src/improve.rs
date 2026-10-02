//! Getting better without new hardware.
//!
//! The question worth answering: given the machine you already have, what
//! makes Atlas better in six months than it is today?
//!
//! Not a bigger model. The honest answers are all about **spending what you
//! have more cleverly** and **accumulating things that don't cost memory** —
//! and most of them run while you sleep.

use serde::{Deserialize, Serialize};

/// A way Atlas gets better on the same hardware.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Gain {
    /// Route reliability moves from a guess to a measurement.
    LearnedRoutes,
    /// New failure modes get added to the procedures.
    LearnedSnags,
    /// Your words — names, jargon, the way you say things — improve
    /// transcription.
    YourVocabulary,
    /// Embeddings computed once, overnight, rather than per search.
    Precomputed,
    /// The model stays loaded rather than being read from disk each turn.
    WarmModel,
    /// Answers to things you ask repeatedly are kept.
    Remembered,
    /// A small model handles what a small model can, so the big one is free
    /// for what needs it.
    RightSizedModel,
    /// What the big model decided is kept and reused locally.
    Distilled,
    /// Work done while you're not waiting.
    MovedOffThePath,
    /// Things installed that you never use get dropped.
    TrimmedDown,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mechanism {
    pub gain: Gain,
    /// What it does.
    pub what: String,
    /// How much better, honestly.
    pub worth: &'static str,
    /// Does it need you to do anything?
    pub automatic: bool,
    /// Memory cost. Most of these are free, which is the point.
    pub costs_mb: u64,
}

pub fn mechanisms() -> Vec<Mechanism> {
    vec![
        Mechanism {
            gain: Gain::LearnedRoutes,
            what: "Every route Atlas tries is scored on whether it worked, here, on this site or \
                   in this app. After a dozen attempts it knows better than the numbers I shipped."
                .into(),
            worth: "large — it stops trying things that don't work in your world",
            automatic: true,
            costs_mb: 0,
        },
        Mechanism {
            gain: Gain::LearnedSnags,
            what: "When something fails in a way Atlas hasn't seen, the symptom, cause and fix \
                   join the procedure. The same surprise happens once."
                .into(),
            worth: "large, and it compounds",
            automatic: true,
            costs_mb: 0,
        },
        Mechanism {
            gain: Gain::YourVocabulary,
            what: "Names, jargon and the phrasings you use get collected and given to the speech \
                   model as hints, so it stops mishearing the words you say most."
                .into(),
            worth: "large for anyone with domain words — 'QUIC', product names, people's names",
            automatic: true,
            costs_mb: 1,
        },
        Mechanism {
            gain: Gain::Precomputed,
            what: "Meaning vectors for everything you've written are computed overnight rather \
                   than when you search."
                .into(),
            worth: "turns a slow search into an instant one",
            automatic: true,
            costs_mb: 60,
        },
        Mechanism {
            gain: Gain::WarmModel,
            what: "The model stays in memory between turns instead of being read from disk each \
                   time."
                .into(),
            worth: "the single biggest latency win available — seconds per turn",
            automatic: true,
            costs_mb: 2000,
        },
        Mechanism {
            gain: Gain::Remembered,
            what: "Answers to questions you ask repeatedly are kept, with what they depended on, \
                   so they can be invalidated rather than going stale."
                .into(),
            worth: "moderate, and it grows with use",
            automatic: true,
            costs_mb: 20,
        },
        Mechanism {
            gain: Gain::RightSizedModel,
            what: "Classifying, routing and summarising go to the small model; only writing and \
                   reasoning go to the large one."
                .into(),
            worth: "large — most requests never need the expensive path",
            automatic: true,
            costs_mb: 700,
        },
        Mechanism {
            gain: Gain::Distilled,
            what: "When a hosted model solves something, what it decided is kept as an example. \
                   Similar problems later are answered locally from the pattern."
                .into(),
            worth: "moderate, and it reduces what you'd ever pay for",
            automatic: false,
            costs_mb: 30,
        },
        Mechanism {
            gain: Gain::MovedOffThePath,
            what: "Indexing, embedding, statement reading and self-work happen while you aren't \
                   waiting on them."
                .into(),
            worth: "large in how it feels, nothing in what it can do",
            automatic: true,
            costs_mb: 0,
        },
        Mechanism {
            gain: Gain::TrimmedDown,
            what: "Voices you rejected, languages you don't speak, indexes of folders you \
                   deleted — dropped, and the space goes to something you use."
                .into(),
            worth: "moderate, and it's how a small machine stays capable",
            automatic: false,
            costs_mb: 0,
        },
    ]
}

/// What's already happening versus what needs a decision from you.
pub fn automatic() -> Vec<Mechanism> {
    mechanisms().into_iter().filter(|m| m.automatic).collect()
}

/// Total memory the automatic gains cost, so it can be weighed against the
/// model size.
pub fn automatic_cost_mb() -> u64 {
    automatic().iter().map(|m| m.costs_mb).sum()
}

/// How many of your words go to the speech model. Kept short: a long list
/// makes everything sound like something on it.
pub const HINTS_GIVEN: usize = 24;

/// The speech model's hint arguments (whisper's `--prompt`): `primer` words
/// first (the assistant's own name, 29 Sep 2026, so there is a prompt before
/// any of your words are learned), then your words, without the ones already
/// there -- or nothing when there are none.
pub fn hint_args(v: &Vocabulary, primer: &[&str]) -> (String, String) {
    let mut words: Vec<String> = primer.iter().map(|w| w.to_string()).collect();
    for w in v.hints(HINTS_GIVEN) {
        if !words.iter().any(|x| x.eq_ignore_ascii_case(&w)) {
            words.push(w);
        }
    }
    words.truncate(HINTS_GIVEN);
    if words.is_empty() {
        (String::new(), String::new())
    } else {
        ("--prompt".into(), format!("{}.", words.join(", ")))
    }
}

/// Words Atlas has learned you say, which improve transcription.
///
/// This is the cheapest large gain there is: a list of your own vocabulary,
/// weighing a kilobyte, that stops the speech model guessing at the words you
/// use most.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Vocabulary {
    /// Word to how often you've used it.
    pub words: Vec<(String, u32)>,
}

impl Vocabulary {
    /// Learn from something you said or wrote.
    pub fn learn(&mut self, text: &str) {
        // A capital at the start of a sentence is grammar, not a name, so an
        // ordinary word opening a sentence is skipped ("What", "Please") and
        // a name there still counts ("Homelab is down").
        const STARTERS: &[&str] = &[
            "what", "when", "where", "which", "who", "whom", "whose", "why", "how", "please", "can", "could",
            "would", "will", "should", "shall", "does", "did", "have", "has", "had", "tell", "show", "open",
            "close", "remind", "send", "make", "find", "check", "read", "play", "find", "okay", "yeah", "thanks",
            "thank", "hello", "hey", "also", "then", "just", "maybe", "there", "this", "that", "these", "those",
            "they", "their", "what's", "whats", "it's", "i'm", "let's", "don't", "ask", "call", "take", "give",
            "keep", "move", "start", "stop", "draft", "write", "schedule", "cancel", "turn", "look",
        ];
        let mut sentence_start = true;
        for raw in text.split_whitespace() {
            let starts = sentence_start;
            sentence_start = raw.ends_with(['.', '?', '!']);
            let bare = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase();
            if starts && STARTERS.contains(&bare.as_str()) {
                continue;
            }
            let w = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            let w = w.trim_end_matches("'s");
            // Ordinary words are already known. What helps is the unusual
            // ones: names, jargon, project names.
            if w.len() < 4 || w.chars().all(|c| c.is_lowercase()) {
                continue;
            }
            match self.words.iter_mut().find(|(s, _)| s == w) {
                Some((_, n)) => *n += 1,
                None => self.words.push((w.to_string(), 1)),
            }
        }
        if self.words.len() > 500 {
            self.words.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
            self.words.truncate(400);
        }
    }

    /// Words the speech model got wrong and you put right (`misses`, 2 Oct
    /// 2026): given straight to the hint list, lower case or not -- "spotify"
    /// said as "spot if I" is exactly the word a hint is for, and `learn`
    /// skips it for having no capital. Counted at the hint list's own bar, so
    /// it is handed over from the next sentence.
    pub fn heard_wrong_as(&mut self, meant: &str) -> bool {
        let mut changed = false;
        for w in meant.split_whitespace() {
            let w = w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'');
            if w.len() < 3 {
                continue;
            }
            match self.words.iter_mut().find(|(s, _)| s.eq_ignore_ascii_case(w)) {
                Some((_, n)) if *n >= 4 => {}
                Some((_, n)) => {
                    *n = 4;
                    changed = true;
                }
                None => {
                    self.words.push((w.to_string(), 4));
                    changed = true;
                }
            }
        }
        changed
    }

    /// The hint list to hand the speech model.
    ///
    /// Kept short: a long list makes transcription worse, not better, because
    /// everything starts sounding like something on it.
    pub fn hints(&self, n: usize) -> Vec<String> {
        let mut v = self.words.clone();
        v.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
        v.into_iter().filter(|(_, c)| *c >= 2).take(n).map(|(w, _)| w).collect()
    }
}

/// What Atlas says when asked how it's getting on.
pub fn progress(route_attempts: u32, snags_learned: usize, vocab: usize) -> String {
    if route_attempts == 0 && snags_learned == 0 && vocab == 0 {
        return "Nothing learned yet — ask me to do things and I'll get better at them.".into();
    }
    let mut parts = Vec::new();
    if route_attempts > 0 {
        parts.push(format!("{route_attempts} attempts scored"));
    }
    if snags_learned > 0 {
        parts.push(format!("{snags_learned} new failure{} understood",
            if snags_learned == 1 { "" } else { "s" }));
    }
    if vocab > 0 {
        parts.push(format!("{vocab} of your words learned"));
    }
    format!("{}. None of it cost you anything.", parts.join(", "))
}
