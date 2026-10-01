//! Choosing the few tools a sentence needs (30 Sep 2026).
//!
//! Eric's laptop (Qwen3-VL-4B, llama.cpp on the Arc 140V): with a clean
//! ~400-token system prompt and four tool schemas, the same model answered
//! eight of his real requests correctly in 1-3 s each. Inside Atlas it read
//! 2,500-3,000-token prompts -- thirteen "core" tools on every turn and up to
//! eighteen in all, each a full schema -- took 17-40 s and routed badly.
//! Measured here by replaying his evening through the prompt builder
//! (`tests/the_right_tools_for_the_sentence.rs`): 4,218 characters of tool
//! schemas on an average turn, more than the conversation itself.
//!
//! So a turn now offers one tool that is always the same -- the
//! capabilities tool, which can list or search everything Atlas does, so the
//! model can find the rest -- and at most `SHORTLIST` others, picked here for
//! the sentence, with their descriptions cut to one line. A sentence that
//! reads like none of them (small talk) is offered none.
//!
//! How they are picked: BM25 (`bm25.rs`) over each command's phrases, its
//! description, the everyday words people use for it (`EVERYDAY`, from
//! Eric's own sentences) and what the capability catalogue says the same
//! thing does (`capability.rs`), with the conversational words that match
//! everything taken out of the question first.
//!
//! Sources: RAG-MCP (Gan et al., 2025, arXiv:2505.03275) -- retrieving the
//! relevant tool descriptions instead of listing all of them cut prompt
//! tokens by over half and tripled tool-selection accuracy (43% vs 14%);
//! ToolLLM's API retriever (Qin et al., 2023, arXiv:2307.16789) for the same
//! shape; Qwen's function-calling guide for keeping tool descriptions short.
//! Clean-room, no code taken.

use crate::intent::{Exposure, ToolBook, ToolEntry};
use serde_json::{json, Value};

/// The tool offered on every turn, first, in the same bytes: what Atlas can
/// do, searched. The model's way to the tools it wasn't shown.
pub const META_TOOL: &str = "capabilities";

/// The most tools picked for one sentence.
pub const SHORTLIST: usize = 4;

/// The most tools any conversation turn offers: the meta tool, the
/// shortlist, and a couple from other programs (`mcp`).
pub const CEILING: usize = 8;

/// A score below which a tool is not offered: BM25 over ~130 short
/// documents gives one rare, content word about 3-5; a word shared by a
/// dozen commands about 1.5. Set from Eric's sentences in the test, where
/// every request he made cleared it and his small talk did not.
pub const FLOOR: f64 = 2.4;

/// A tool scoring under this share of the best one isn't offered: it
/// matched on a word the sentence shares with half the list.
pub const RELATIVE: f64 = 0.45;

/// The sentence as its words, lowercased, without the wake word or "can
/// you" / "please" in front: what a phrase is matched against.
fn leading_words(said: &str) -> String {
    let t: String = said.to_lowercase().replace('\u{2019}', "'").chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    let mut w: Vec<&str> = t.split_whitespace().collect();
    loop {
        let before = w.len();
        for lead in [&["hey", "atlas"][..], &["atlas"], &["ok"], &["okay"], &["please"], &["can", "you"], &["could", "you"], &["would", "you"], &["will", "you"], &["i", "want", "you", "to"], &["i", "need", "you", "to"]] {
            if w.len() > lead.len() && w[..lead.len()] == *lead {
                w.drain(..lead.len());
            }
        }
        if w.len() == before {
            break;
        }
    }
    w.join(" ")
}

/// Does `lead` start with the phrase, as whole words? One-word phrases
/// count only when they are a verb-like command word, not "open" in "open
/// question" -- so the phrase must be followed by something.
fn starts_with_phrase(lead: &str, phrase: &str) -> bool {
    let p: String = phrase.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    let p = p.split_whitespace().collect::<Vec<_>>().join(" ");
    !p.is_empty() && p.len() >= 5 && (lead == p || lead.starts_with(&format!("{p} ")))
}

/// Words that say nothing about which tool: they are in nearly every
/// sentence said to an assistant, and several commands' phrases carry them
/// ("keep at it", "show me", "tell them"), so leaving them in offered tools
/// to small talk.
const CONVERSATIONAL: &[&str] = &[
    "i", "me", "my", "mine", "you", "your", "we", "us", "our", "it", "its", "this", "that", "these", "those", "a",
    "an", "the", "and", "or", "but", "so", "to", "of", "in", "on", "at", "for", "with", "about", "from", "up", "out",
    "is", "are", "was", "were", "be", "been", "am", "do", "does", "did", "done", "can", "could", "would", "will",
    "should", "shall", "may", "might", "must", "have", "has", "had", "get", "got", "go", "going", "gone", "want",
    "wanted", "need", "needs", "like", "just", "really", "please", "now", "then", "there", "here", "what", "whats",
    "what's", "how", "why", "when", "who", "which", "where", "some", "any", "all", "more", "less", "very", "too",
    "also", "not", "no", "yes", "yeah", "ok", "okay", "hey", "hi", "atlas", "guess", "think", "know", "said",
    "say", "tell", "talk", "talking", "keep", "make", "let", "lets", "let's", "thing", "things", "something",
    "anything", "everything", "again", "still", "right", "good", "great", "thanks", "thank", "i'm", "im", "you're",
    "youre", "don't", "dont", "can't", "cant", "it's", "that's", "thats", "come", "on", "if", "as", "be", "being",
    "one", "way", "ways", "yourself", "myself", "mean", "means", "meant", "actually", "because", "hear", "hearing", "how's", "hows", "who's", "whos", "where's", "there's", "here's", "i've", "i'll", "you've", "you'll", "least", "asked", "care", "cares", "time", "day", "back", "into", "them", "they", "he", "she", "him", "her", "his",
];

/// The words Eric actually used for a thing, which its phrases and
/// description don't carry. Each came from something he said to Atlas
/// (`tests/the_right_tools_for_the_sentence.rs` holds the sentences).
const EVERYDAY: &[(&str, &str)] = &[
    ("tidy_desktop", "organize organise organizing organising clean clean-up clutter messy mess desktop sort files folders"),
    ("view_display", "screen screens monitor monitors display showing look see read window chart what's on"),
    ("whats_there", "camera room who's there look around what do you see"),
    ("capture_webcam", "camera webcam see me look at me face use camera turn on camera"),
    ("whats_this", "camera holding showing hold up"),
    ("self_check", "diagnosis diagnose diagnostic yourself self report status health check-up checkup missing broken setup"),
    ("finish_setup", "setup set-up set up install configure left unfinished"),
    ("research", "internet web online research researching find out investigate study report document look up learn"),
    ("use_mic", "microphone mic webcam headset headphones airpods listen hear"),
    ("wit", "smart-ass smartass smart ass sarcasm sarcastic jokes joking funny calm tone chill"),
    ("capture", "note notes remember write down jot"),
    ("schedule", "calendar event meeting appointment book add"),
    ("agenda", "calendar today tomorrow week busy free plans schedule"),
    ("find_file", "file files document documents pdf photo find where"),
    ("mail", "email emails inbox mail"),
    ("delegate", "reply respond draft answer message email"),
    ("message", "text message send friend"),
    ("machine_health", "computer laptop slow memory ram disk cpu fan hot"),
    ("recommend", "improve improving better faster quality upgrade"),
    ("work_on_yourself", "fix yourself improve yourself own code"),
    ("clock", "time date"),
    ("queued", "working on doing jobs errands running background progress"),
    ("outstanding", "unfinished couldn't failed list"),
];

/// Capability entries that are the same thing as a command under another
/// name, so what the catalogue says about them helps route to the command.
const SAME_THING: &[(&str, &str)] = &[
    ("vision", "whats_there"),
    ("presence", "whats_there"),
    ("vision", "whats_this"),
    ("ocr", "view_display"),
    ("calendar", "agenda"),
    ("calendar", "schedule"),
    ("findfile", "find_file"),
    ("doctor", "self_check"),
    ("checkup", "self_check"),
    ("wit", "wit"),
];

/// The sentence as words that could say which tool: lowercased, without
/// the conversational ones.
pub fn request_words(said: &str) -> Vec<String> {
    compounds(&said.to_lowercase().replace('\u{2019}', "'"))
        .split(|c: char| !c.is_alphanumeric() && c != '\'' && c != '-')
        .map(|w| w.trim_matches(|c: char| c == '\'' || c == '-'))
        .filter(|w| !w.is_empty() && !CONVERSATIONAL.contains(w))
        .map(str::to_string)
        .collect()
}

/// Two-word names made one, so their halves don't match on their own:
/// "are you smart?" is not about the smart-ass setting.
fn compounds(text: &str) -> String {
    let mut t = text.to_string();
    for (two, one) in [("smart ass", "smartass"), ("smart-ass", "smartass"), ("smart apps", "smartass"), ("set up", "setup"), ("set-up", "setup"), ("e-mail", "email"), ("web cam", "webcam")] {
        t = t.replace(two, one);
    }
    t
}

/// Every command the model may choose, indexed to be picked from.
pub struct Router {
    entries: Vec<ToolEntry>,
    index: crate::bm25::Index,
}

impl Router {
    pub fn new(book: &ToolBook) -> Router {
        let catalogue = crate::capability::all();
        let mut entries: Vec<ToolEntry> = Vec::new();
        let mut index = crate::bm25::Index::default();
        for e in book.entries() {
            if e.exposure == Exposure::Never || e.name == META_TOOL {
                continue;
            }
            let body = e.describe.clone();
            // The everyday words count with the phrases (twice, as a title):
            // they are what people actually say for it.
            let mut title = e.phrases.join(" ");
            for (name, words) in EVERYDAY {
                if *name == e.name {
                    title.push(' ');
                    title.push_str(words);
                }
            }
            let mut body = body;
            for c in &catalogue {
                let same = c.id == e.name || SAME_THING.iter().any(|(cap, cmd)| *cap == c.id && *cmd == e.name);
                if same {
                    body.push(' ');
                    body.push_str(c.what);
                }
            }
            index.add(entries.len() as u64, &compounds(&title.to_lowercase()), &compounds(&body.to_lowercase()));
            entries.push(e.clone());
        }
        Router { entries, index }
    }

    /// The tools this sentence reads like, best first, with their scores:
    /// at most `k`, none under `FLOOR` or under `RELATIVE` of the best. A
    /// command whose phrase the sentence starts with ("research ways to
    /// ...") comes first whatever the scores say.
    pub fn shortlist(&self, said: &str, k: usize) -> Vec<(&ToolEntry, f64)> {
        let q = request_words(said).join(" ");
        if q.trim().is_empty() {
            return Vec::new();
        }
        let mut out: Vec<(&ToolEntry, f64)> = Vec::new();
        let lead = leading_words(said);
        if let Some(e) = self.entries.iter().find(|e| e.phrases.iter().any(|p| starts_with_phrase(&lead, p))) {
            out.push((e, f64::MAX));
        }
        let hits = self.index.search(&q, k + 1);
        let best = hits.first().map(|h| h.1).unwrap_or(0.0);
        for (id, s) in hits {
            if s < FLOOR || s < best * RELATIVE {
                continue;
            }
            if let Some(e) = self.entries.get(id as usize) {
                if !out.iter().any(|(o, _)| o.name == e.name) {
                    out.push((e, s));
                }
            }
        }
        out.truncate(k);
        out
    }

    /// The names only.
    pub fn names_for(&self, said: &str, k: usize) -> Vec<String> {
        self.shortlist(said, k).into_iter().map(|(e, _)| e.name.clone()).collect()
    }

    /// `shortlist` for this sentence; when it reads like no tool and there
    /// is something they have been asking for lately ("that's it", "do it
    /// now"), the tools for that instead.
    pub fn for_turn(&self, said: &str, goal: Option<&str>, k: usize) -> Vec<&ToolEntry> {
        let mut picked: Vec<&ToolEntry> = self.shortlist(said, k).into_iter().map(|(e, _)| e).collect();
        // Only for a sentence with next to nothing of its own ("that's it",
        // "do it now"): "tell me about octopuses" is its own subject.
        // Never for small talk: "hey, how's it going" has no words of its own
        // either, and was offered the tools for research asked an hour before
        // (30 Sep 2026, a real model then called one of them).
        if picked.is_empty() && request_words(said).len() <= 1 && !small_talk(said) {
            if let Some(g) = goal.filter(|g| !g.trim().is_empty()) {
                picked = self.shortlist(g, k.min(3)).into_iter().map(|(e, _)| e).collect();
            }
        }
        picked
    }
}

/// One line of a description: its first sentence, at most `most` characters.
pub fn one_line(text: &str, most: usize) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let first = match t.find(". ") {
        Some(i) => t[..=i].to_string(),
        None => t,
    };
    if first.chars().count() <= most {
        first
    } else {
        let cut: String = first.chars().take(most.saturating_sub(1)).collect();
        let cut = match cut.rfind(' ') {
            Some(i) if i > most / 2 => cut[..i].to_string(),
            _ => cut,
        };
        format!("{}…", cut.trim_end_matches([',', ';', ':']))
    }
}

/// A command as a tool, in as few words as still say what it does. `note`
/// is added to the argument's description (the known apps, for the
/// open/close/switch tools).
pub fn compact_spec(e: &ToolEntry, note: Option<&str>) -> Value {
    let (what, arg) = match e.describe.split_once(" arg: ") {
        Some((w, a)) => (w.trim().to_string(), a.trim().to_string()),
        None => (e.describe.trim().to_string(), String::new()),
    };
    let what = one_line(&what, 110);
    let parameters = if e.takes_arg {
        let mut doc = if arg.is_empty() { "What it's about, in the user's words.".to_string() } else { one_line(&arg, 70) };
        if let Some(n) = note.filter(|n| !n.trim().is_empty()) {
            doc = format!("{} {}", doc.trim_end_matches('.'), one_line(n, 120)).trim().to_string();
        }
        let mut p = json!({"type": "object", "properties": {"arg": {"type": "string", "description": doc}}});
        if !e.arg_optional {
            p["required"] = json!(["arg"]);
        }
        p
    } else {
        json!({"type": "object", "properties": {}})
    };
    json!({"type": "function", "function": {"name": e.name, "description": what, "parameters": parameters}})
}

/// The meta tool, the same bytes every turn.
pub fn meta_spec() -> Value {
    json!({"type": "function", "function": {
        "name": META_TOOL,
        "description": "Search everything Atlas can do, with whether each works or needs setting up. Use it when asked what you can do, or when no other tool fits.",
        "parameters": {"type": "object", "properties": {"arg": {"type": "string", "description": "An ability, e.g. camera; empty for all."}}}
    }})
}

/// Is the day what's being talked about -- the calendar, what's on, what's
/// due -- so the calendar and today's reminders are worth putting in front
/// of the model? (They went in front of every turn until 30 Sep 2026.)
pub fn about_the_day(said: &str) -> bool {
    let words = request_words(said);
    const DAY: &[&str] = &[
        "today", "tonight", "tomorrow", "tomorrow's", "week", "weekend", "calendar", "schedule", "agenda", "busy",
        "free", "meeting", "meetings", "appointment", "appointments", "plans", "reminder", "reminders", "due",
        "morning", "afternoon", "evening", "monday", "tuesday", "wednesday", "thursday", "friday", "saturday",
        "sunday",
    ];
    words.iter().any(|w| DAY.contains(&w.as_str()))
}

/// `text` in at most `most` characters, on one line, cut at a word with "…".
pub fn clip_words(text: &str, most: usize) -> String {
    let t = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= most {
        return t;
    }
    let cut: String = t.chars().take(most.saturating_sub(1)).collect();
    let cut = match cut.rfind(' ') {
        Some(i) if i > most / 2 => cut[..i].to_string(),
        _ => cut,
    };
    format!("{}…", cut.trim_end_matches([',', ';', ':', '.']))
}

/// A greeting, a thanks, a goodbye or "how's it going": said to be sociable,
/// not about anything asked earlier.
pub fn small_talk(said: &str) -> bool {
    let t: String = said.to_lowercase().replace('\u{2019}', "'").chars().map(|c| if c.is_alphanumeric() || c == '\'' { c } else { ' ' }).collect();
    let t = format!(" {} ", t.split_whitespace().collect::<Vec<_>>().join(" "));
    [
        " hi ", " hey ", " hello ", " morning ", " evening ", " thanks ", " thank you ", " cheers ", " bye ", " goodbye ",
        " good night ", " goodnight ", " how's it going ", " hows it going ", " how are you ", " how's your day ",
        " what's up ", " whats up ", " see you ", " have a good ", " you there ", " can you hear me ",
    ]
    .iter()
    .any(|w| t.contains(w))
}
