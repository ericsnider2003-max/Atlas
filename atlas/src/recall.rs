//! Finding something you wrote, without remembering where you put it.
//!
//! Phase 2. This is the payoff for everything Atlas has been accumulating —
//! notes, drafts, research, transcripts, the conversation thread. Months of it
//! is worth nothing if the only way in is remembering a filename.
//!
//! Two ways of searching, and they answer different questions:
//!
//! * **Words.** Fast, exact, needs no model, and finds the thing when you
//!   remember a word from it. Ranked properly rather than by count — a word
//!   that appears in every note tells you nothing, and one that appears in
//!   three notes tells you a great deal.
//! * **Meaning.** Finds the thing when you remember what it was *about*.
//!   Needs a small embedding model — 50–100MB, which is nothing beside a
//!   language model, and on this laptop it runs on the NPU.
//!
//! Both together beat either alone, so results are merged rather than chosen
//! between.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Piece {
    pub id: u64,
    /// Where it came from: a path, a note name, "conversation".
    pub source: String,
    /// A line naming it, for reading out.
    pub title: String,
    pub text: String,
    pub at: u64,
    /// The meaning vector, when one has been made.
    #[serde(default)]
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct RecallConfig {
    /// Use meaning as well as words. Needs an embedding model.
    pub semantic: bool,
    /// Results to bring back.
    pub results: usize,
    /// Below this, don't offer it at all — a bad match is worse than none.
    ///
    /// Low on purpose. A single mention of a common word scores around 0.07,
    /// and that is still a real match worth offering; the ranking decides the
    /// order, this only keeps out noise.
    pub floor: f32,
    /// How much weight meaning gets against words, 0 to 1.
    pub meaning_weight: f32,
    /// Prefer recent things, slightly.
    pub recency_days: f32,
    /// How much the live conversation may nudge the order, 0 to 1.
    ///
    /// Contextual recall, the silent half: when two pieces answer a question
    /// about equally, the one that connects to what you were just talking
    /// about wins. It is a multiplier on pieces the QUERY already matched —
    /// context re-ranks, it never introduces. A note surfacing because you
    /// mentioned its topic an hour ago, in answer to a question it does not
    /// match, would be recall volunteering, and volunteering was decided
    /// against: context changes nothing you would notice except answers
    /// getting more apt.
    pub context_weight: f32,
    /// Drop anything scoring below this fraction of the best hit.
    ///
    /// The absolute `floor` above catches noise. This catches something else:
    /// a weak-but-not-noise hit sitting beside a strong one.
    ///
    /// Ask "how many days holiday do I get" and the search might return the
    /// policy at 0.92, a note about holidays being separate at 0.87, and an
    /// old draft saying a different number at 0.51. Every one clears an
    /// absolute floor. Handing all three to whatever reads them supplies two
    /// contradictory answers and no way to tell which is the real one — and a
    /// contradiction is worse than a gap, because a gap makes you go and look.
    ///
    /// Relative rather than absolute because it depends on the question. When
    /// the best hit is strong, weak neighbours dilute. When everything is
    /// mediocre, they are all you have, and the fraction keeps them.
    pub relative_floor: f32,
}

impl Default for RecallConfig {
    fn default() -> Self {
        RecallConfig {
            semantic: false,
            results: 5,
            floor: 0.04,
            relative_floor: 0.5,
            meaning_weight: 0.55,
            context_weight: 0.25,
            recency_days: 120.0,
        }
    }
}

/// Everything Atlas can search.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    pub pieces: Vec<Piece>,
    /// Word to how many pieces contain it. What makes the ranking work.
    document_counts: BTreeMap<String, usize>,
    /// Words across every piece, for the average length BM25 normalises by.
    #[serde(default)]
    total_words: usize,
}

impl Library {
    pub fn add(&mut self, p: Piece) {
        self.total_words += stems(&format!("{} {}", p.title, p.text).to_lowercase()).len();
        for w in unique_words(&format!("{} {}", p.title, p.text)) {
            *self.document_counts.entry(w).or_insert(0) += 1;
        }
        self.pieces.push(p);
    }

    pub fn len(&self) -> usize {
        self.pieces.len()
    }
    pub fn is_empty(&self) -> bool {
        self.pieces.is_empty()
    }

    /// How much a word narrows things down.
    ///
    /// A word in every piece tells you little; a word in three tells you a
    /// great deal. Without this, common words drown the search.
    ///
    /// The floor matters more than it looks. With four notes, a word in all
    /// four is "common" and scores zero — so searching a small library for
    /// the only word you remember finds nothing at all. It should still find
    /// them; it just can't tell them apart.
    fn rarity(&self, word: &str) -> f32 {
        let n = self.pieces.len().max(1) as f32;
        let seen = *self.document_counts.get(word).unwrap_or(&0) as f32;
        ((n + 1.0) / (seen + 1.0)).ln().max(0.18)
    }

    fn word_score(&self, p: &Piece, query: &[String]) -> f32 {
        let hay = format!("{} {}", p.title, p.text).to_lowercase();
        let words: Vec<String> = stems(&hay);
        if words.is_empty() {
            return 0.0;
        }
        let mut score = 0.0;
        let avg = self.total_words as f64 / self.pieces.len().max(1) as f64;
        for q in query {
            let qs = crate::stemmer::stem(q);
            let hits = words.iter().filter(|w| **w == qs).count() as f32;
            if hits == 0.0 {
                continue;
            }
            // Saturating: the tenth mention of a word says little more than
            // the third. And BM25's third idea, which this lacked until the
            // 23 Sep ports: a word said once in a long note is weaker evidence
            // than the same word in a short one, so the note's length against
            // the library's average counts (`bm25::term_weight`, k1 1.2,
            // b 0.75 — tantivy's defaults).
            let saturated = crate::bm25::term_weight(hits as f64, words.len() as f64, avg) as f32;
            let mut w = saturated * self.rarity(&qs);
            // A word in the title is worth more than one buried in the body.
            if p.title.to_lowercase().contains(q) {
                w *= 1.7;
            }
            score += w;
        }
        score
    }

    /// Search by words, by meaning, or both.
    pub fn search(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        cfg: &RecallConfig,
        now: u64,
    ) -> Vec<Hit> {
        self.search_in_context(query, query_embedding, &[], None, cfg, now)
    }

    /// `search`, with the live conversation allowed to break ties.
    ///
    /// `context` is words from the last few turns (see `context_terms_from`),
    /// and `context_embedding` is those same turns as one meaning vector when
    /// an encoder is installed. Either one scales a piece's score by up to
    /// `1 + context_weight` — and ONLY a piece the query itself matched,
    /// because a zero base times anything is still zero. That is the whole
    /// design: context re-ranks what was found, it never finds. The two
    /// signals are taken at their max, not summed: words and meaning are two
    /// ways of asking the same question ("does this connect to what we were
    /// just discussing"), so the stronger answer stands rather than doubling
    /// the weight when both happen to fire.
    pub fn search_in_context(
        &self,
        query: &str,
        query_embedding: Option<&[f32]>,
        context: &[String],
        context_embedding: Option<&[f32]>,
        cfg: &RecallConfig,
        now: u64,
    ) -> Vec<Hit> {
        let terms = words_of(&query.to_lowercase());
        if terms.is_empty() && query_embedding.is_none() {
            return Vec::new();
        }

        let mut hits: Vec<Hit> = self
            .pieces
            .iter()
            .map(|p| {
                let words = self.word_score(p, &terms);
                let meaning = match (cfg.semantic, query_embedding, &p.embedding) {
                    (true, Some(q), Some(e)) => crate::voiceid::cosine(q, e).max(0.0),
                    _ => 0.0,
                };
                // Both together beat either alone, so they are merged rather
                // than chosen between.
                let combined = if meaning > 0.0 {
                    words * (1.0 - cfg.meaning_weight) + meaning * cfg.meaning_weight * 3.0
                } else {
                    words
                };
                let age_days = (now.saturating_sub(p.at) as f32) / 86_400.0;
                // Recency nudges, it doesn't decide. Something from a year
                // ago that matches exactly should still win.
                let recency = 1.0 + 0.25 * (-age_days / cfg.recency_days.max(1.0)).exp();
                // Age alone is the wrong measure. A note on how a protocol
                // works does not get worse with time; a note on a version
                // number does. What kind of claim it is decides how much its
                // age should count against it.
                let known = crate::freshness::Known::new(
                    &p.title,
                    crate::freshness::shelf_for(&format!("{} {}", p.title, p.text)),
                    crate::freshness::Checkable::File(p.source.clone()),
                    p.at,
                );
                let freshness = recency * crate::freshness::ranking_multiplier(&known, now);
                // The conversation's nudge. Multiplicative, capped at
                // `1 + context_weight` — enough to settle a tie between two
                // near-equal answers, nowhere near enough to outrank a piece
                // that actually matched the question better.
                let context_nudge = if combined <= 0.0 {
                    1.0
                } else {
                    // Two ways the conversation can connect to this piece.
                    //
                    // Words: saturating rather than linear, because the job is
                    // binary — either the last few turns genuinely overlap this
                    // piece or they don't. A real overlap (one shared topical
                    // word scores ~0.3) earns most of the weight; a trace earns
                    // almost none.
                    let word_sig = if context.is_empty() {
                        0.0
                    } else {
                        let s = self.word_score(p, context);
                        s / (s + 0.25)
                    };
                    // Meaning: the same turns as one vector against this piece's
                    // vector, so the conversation can connect by what it was
                    // about even when it shares no words — the #6 upgrade, and
                    // the reason a note about "the cider press" lifts a note
                    // about "orchard yield" without either saying the other's
                    // words. Zero whenever meaning search is off or either
                    // vector is missing, so this silently degrades to the word
                    // signal, which itself degrades to no nudge.
                    let mean_sig = match (cfg.semantic, context_embedding, &p.embedding) {
                        (true, Some(cv), Some(e)) => crate::voiceid::cosine(cv, e).max(0.0),
                        _ => 0.0,
                    };
                    let sig = word_sig.max(mean_sig);
                    // The full weight must clear `clarity`'s 0.9 band — a nudge
                    // that cannot settle the tie it exists to settle would be
                    // wiring shaped like a feature — but the cap keeps it a
                    // tiebreak, never a veto over a clearly better match.
                    1.0 + cfg.context_weight.clamp(0.0, 1.0) * sig
                };
                Hit {
                    id: p.id,
                    title: p.title.clone(),
                    source: p.source.clone(),
                    score: combined * freshness * context_nudge,
                    why: why_matched(p, &terms, meaning),
                    quote: quote_from(&p.text, &terms),
                }
            })
            .filter(|h| h.score > cfg.floor)
            .collect();

        hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        if let Some(best) = hits.first().map(|h| h.score) {
            let cut = best * cfg.relative_floor.clamp(0.0, 1.0);
            hits.retain(|h| h.score >= cut);
        }
        hits.truncate(cfg.results);
        hits
    }

    /// How clear-cut the top answer is.
    ///
    /// Two hits within a whisker of each other is not a confident answer with
    /// a runner-up, it is a question the notes answer two ways. Saying so is
    /// more use than picking, and it is the one thing the ranking knows that
    /// the ranked list does not show.
    pub fn clarity(hits: &[Hit]) -> Clarity {
        match hits {
            [] => Clarity::NothingFound,
            [_] => Clarity::OneClearAnswer,
            [a, b, ..] => {
                if b.score >= a.score * 0.9 {
                    Clarity::TwoEquallyGood {
                        first: a.title.clone(),
                        second: b.title.clone(),
                    }
                } else {
                    Clarity::OneClearAnswer
                }
            }
        }
    }

    pub fn get(&self, id: u64) -> Option<&Piece> {
        self.pieces.iter().find(|p| p.id == id)
    }

    /// Pieces still needing a meaning vector.
    pub fn unembedded(&self) -> Vec<u64> {
        self.pieces.iter().filter(|p| p.embedding.is_none()).map(|p| p.id).collect()
    }

    pub fn set_embedding(&mut self, id: u64, e: Vec<f32>) {
        if let Some(p) = self.pieces.iter_mut().find(|p| p.id == id) {
            p.embedding = Some(e);
        }
    }
}

/// Whether the search settled the question.
#[derive(Debug, Clone, PartialEq)]
pub enum Clarity {
    NothingFound,
    /// One hit stands out.
    OneClearAnswer,
    /// Two hits are close enough that the notes disagree.
    TwoEquallyGood { first: String, second: String },
}

impl Clarity {
    /// What to say before the results, if anything.
    pub fn caveat(&self) -> Option<String> {
        match self {
            Clarity::TwoEquallyGood { first, second } => Some(format!(
                "Two things match about equally well — \"{first}\" and \"{second}\" — \
                 so you may want to check which one is current."
            )),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    pub id: u64,
    pub title: String,
    pub source: String,
    pub score: f32,
    /// Why this one, in a few words.
    pub why: String,
    /// The sentence it matched on, so you can tell without opening it.
    pub quote: String,
}

fn why_matched(p: &Piece, terms: &[String], meaning: f32) -> String {
    let hay = format!("{} {}", p.title, p.text).to_lowercase();
    let found: Vec<&String> = terms.iter().filter(|t| hay.contains(t.as_str())).collect();
    match (found.is_empty(), meaning > 0.3) {
        (false, _) => {
            let names: Vec<&str> = found.iter().take(3).map(|s| s.as_str()).collect();
            format!("mentions {}", names.join(", "))
        }
        (true, true) => "about the same thing".into(),
        _ => "loosely related".into(),
    }
}

/// The sentence a match came from, so you can tell whether it's the right one
/// without opening the file.
fn quote_from(text: &str, terms: &[String]) -> String {
    let sentences: Vec<&str> = text.split_inclusive(['.', '!', '?', '\n']).collect();
    let best = sentences
        .iter()
        .max_by_key(|s| {
            let l = s.to_lowercase();
            terms.iter().filter(|t| l.contains(t.as_str())).count()
        })
        .copied()
        .unwrap_or("");
    let trimmed = best.trim();
    if trimmed.chars().count() > 160 {
        format!("{}…", trimmed.chars().take(160).collect::<String>())
    } else {
        trimmed.to_string()
    }
}

const NOISE: &[&str] = &[
    "the", "a", "an", "and", "or", "but", "of", "to", "in", "on", "at", "for",
    "with", "is", "was", "были", "it", "that", "this", "i", "you", "we", "my",
    "what", "where", "when", "did", "do", "does", "about", "from", "by", "as",
];

pub fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .map(|w| w.trim().to_lowercase())
        .filter(|w| w.len() > 2 && !NOISE.contains(&w.as_str()))
        .collect()
}

/// What matching compares: `words_of`, stemmed — "trades", "traded" and
/// "trading" are one word to a person, and since 23 Sep to recall too
/// (Porter2, `stemmer`). Only the comparison is stemmed. Query words,
/// context words and quotes stay the words people wrote, and a stem is never
/// shown to anyone.
fn stems(text: &str) -> Vec<String> {
    words_of(text).iter().map(|w| crate::stemmer::stem(w)).collect()
}

fn unique_words(text: &str) -> Vec<String> {
    let mut w = stems(&text.to_lowercase());
    w.sort();
    w.dedup();
    w
}

/// What Atlas says about what it found.
///
/// Leads with the one it thinks you mean and quotes the line, because a list
/// of five filenames is not an answer.
pub fn spoken(hits: &[Hit]) -> String {
    match hits.split_first() {
        None => "I can't find anything about that.".into(),
        Some((first, rest)) => {
            let mut s = format!("{} — \"{}\"", first.title, first.quote.trim());
            if !rest.is_empty() {
                s.push_str(&format!(" And {} other{}.", rest.len(), if rest.len() == 1 { "" } else { "s" }));
            }
            s
        }
    }
}

/// Whether the search needs a model at all.
///
/// Words work with nothing installed, which matters: search that only works
/// once you have downloaded something is search you won't have on day one.
pub fn needs_a_model(cfg: &RecallConfig) -> bool {
    cfg.semantic
}

/// The conversation, as words a search can use.
///
/// Recent turns in, one deduplicated word list out — minus the current
/// question's own words, because the question already scores itself and
/// counting its words twice would be the query wearing a context costume.
/// Capped, so one long-winded turn cannot flood the nudge.
pub fn context_terms_from(recent: &[String], question: &str) -> Vec<String> {
    let asked: Vec<String> = words_of(&question.to_lowercase());
    let mut out: Vec<String> = Vec::new();
    for turn in recent {
        for w in words_of(&turn.to_lowercase()) {
            if !asked.contains(&w) && !out.contains(&w) {
                out.push(w);
            }
        }
    }
    out.truncate(24);
    out
}

// ---------------------------------------------------------------------------
// How good is search? Measured, not guessed.
//
// A fixed set of questions with known answers, asked of the library by words
// alone and by words and meaning together, scored the standard way (recall
// at 5, mean reciprocal rank, nDCG at 5). Run again whenever the meaning
// model changes, so "the new model is better" is a number, not an
// impression. The idea is the retrieval-metrics check in the MIT-licensed
// llm-application-dev plugin of wshobson/agents (THIRD_PARTY_NOTICES.md).
// ---------------------------------------------------------------------------

/// A question the library should answer with one particular note.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct KnownQuestion {
    pub ask: String,
    /// The title of the note that answers it.
    pub answer: String,
    /// Written by you ("search-questions.txt"), or made from the note.
    pub yours: bool,
}

/// Questions made from the notes themselves: for each note, its most
/// informative sentence with the note's own title words taken out and every
/// third word dropped — roughly how a half-remembered detail comes back to
/// you. Deterministic, so the same notes give the same questions and two
/// runs compare. At most `max`.
pub fn questions_from(lib: &Library, max: usize) -> Vec<KnownQuestion> {
    let mut pieces: Vec<&Piece> = lib.pieces.iter().collect();
    pieces.sort_by(|a, b| a.title.cmp(&b.title));
    let mut out = Vec::new();
    for p in pieces {
        if out.len() >= max {
            break;
        }
        let title_words: Vec<String> = words_of(&p.title.to_lowercase());
        let body: String = p
            .text
            .lines()
            .take_while(|l| !l.starts_with("## Sources") && !l.starts_with("## Figures"))
            .filter(|l| !l.starts_with('#') && !l.trim().starts_with("- http"))
            .collect::<Vec<_>>()
            .join(" ");
        let best = body
            .split_inclusive(['.', '!', '?'])
            .max_by_key(|s| words_of(&s.to_lowercase()).into_iter().filter(|w| !title_words.contains(w)).count())
            .unwrap_or("");
        let kept: Vec<String> = words_of(&best.to_lowercase())
            .into_iter()
            .filter(|w| !title_words.contains(w))
            .enumerate()
            .filter(|(i, _)| i % 3 != 2)
            .map(|(_, w)| w)
            .collect();
        if kept.len() >= 4 {
            out.push(KnownQuestion { ask: kept.join(" "), answer: p.title.clone(), yours: false });
        }
    }
    out
}

/// Your own questions, one per line: `what you'd ask => the note's title`.
pub fn questions_written(text: &str) -> Vec<KnownQuestion> {
    text.lines()
        .filter_map(|l| {
            let (ask, answer) = l.split_once("=>")?;
            let (ask, answer) = (ask.trim(), answer.trim());
            (!ask.is_empty() && !answer.is_empty() && !ask.starts_with('#'))
                .then(|| KnownQuestion { ask: ask.into(), answer: answer.into(), yours: true })
        })
        .collect()
}

/// One way of searching, scored.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    /// Questions whose answer was in the top five.
    pub recall_at_5: f32,
    /// On average, 1 / the position the answer came in (0 when missed).
    pub mrr: f32,
    /// Discounted gain at five: finding it first counts most.
    pub ndcg_at_5: f32,
    pub asked: usize,
}

/// Ask each question and score where its answer came. `embed` turns a
/// question into a meaning vector; `None` from it (or no `embed`) means
/// words alone.
pub fn measure(
    lib: &Library,
    questions: &[KnownQuestion],
    embed: Option<&dyn Fn(&str) -> Option<Vec<f32>>>,
    cfg: &RecallConfig,
    now: u64,
) -> Scores {
    let mut cfg = cfg.clone();
    cfg.results = 5;
    cfg.relative_floor = 0.0;
    cfg.semantic = embed.is_some();
    let (mut found, mut rr, mut gain) = (0usize, 0f32, 0f32);
    for q in questions {
        let v = embed.and_then(|e| e(&q.ask));
        let hits = lib.search(&q.ask, v.as_deref(), &cfg, now);
        if let Some(rank) = hits.iter().position(|h| h.title.eq_ignore_ascii_case(&q.answer)) {
            found += 1;
            rr += 1.0 / (rank as f32 + 1.0);
            gain += 1.0 / ((rank as f32 + 2.0).log2());
        }
    }
    let n = questions.len().max(1) as f32;
    Scores { recall_at_5: found as f32 / n, mrr: rr / n, ndcg_at_5: gain / n, asked: questions.len() }
}

/// A measurement, kept so the next one has something to compare with.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SearchCheck {
    pub at: u64,
    /// Which meaning model (`meaning::fingerprint`); empty for words only.
    pub model: String,
    pub words: Scores,
    pub meaning: Option<Scores>,
}

impl SearchCheck {
    /// Said plainly, against the last one when there is one.
    pub fn said(&self, before: Option<&SearchCheck>) -> String {
        let pct = |x: f32| (x * 100.0).round() as i32;
        let mut s = format!(
            "Search check, {} questions: by words, the right note is in the top five {}% of the time (first-place score {}).",
            self.words.asked,
            pct(self.words.recall_at_5),
            pct(self.words.mrr)
        );
        if let Some(m) = &self.meaning {
            let diff = pct(m.recall_at_5) - pct(self.words.recall_at_5);
            s.push_str(&format!(
                " With meaning as well: {}% ({}), {}.",
                pct(m.recall_at_5),
                pct(m.mrr),
                match diff {
                    d if d > 0 => format!("meaning finds {d} points more"),
                    d if d < 0 => format!("meaning makes it {} points worse — worth a look", -d),
                    _ => "no better than words alone".into(),
                }
            ));
        }
        if let (Some(b), Some(m)) = (before, &self.meaning) {
            if let Some(bm) = &b.meaning {
                let change = pct(m.ndcg_at_5) - pct(bm.ndcg_at_5);
                if b.model != self.model && change != 0 {
                    s.push_str(&format!(
                        " Against the previous meaning model: {} {} points.",
                        if change > 0 { "up" } else { "down" },
                        change.abs()
                    ));
                }
            }
        }
        s
    }
}

/// The notes folder as a library, words only (no meaning vectors yet): each
/// `.md` file one piece, titled by its first heading or else its file name.
/// Most pieces taken from the reading shelf, so a shelf of books can't make
/// every search slow.
pub const READING_PIECES_MOST: usize = 6000;

/// Add what Atlas has read -- documents and books, kept whole in the
/// reading folder (`read_document_off`, `learn_knowledge`) -- to `lib`, a
/// piece per chunk, each citing its file and lines (1 Oct 2026, from Open
/// Notebook in the research report: until now a PDF Atlas had read could
/// never be found again).
pub fn add_readings(lib: &mut Library, dir: &std::path::Path) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    let mut paths: Vec<std::path::PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|x| x.to_str()), Some("txt") | Some("md")))
        .collect();
    paths.sort();
    let mut id = lib.pieces.iter().map(|p| p.id).max().unwrap_or(0);
    let mut added = 0usize;
    for path in paths {
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let name = path.file_stem().map(|f| f.to_string_lossy().into_owned()).unwrap_or_default();
        let at = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let Ok(chunks) = crate::chunker::chunk(&text, crate::chunker::ChunkConfig { max: 1200, overlap: 150 }) else { continue };
        let shown = path.to_string_lossy().into_owned();
        for c in chunks {
            if added >= READING_PIECES_MOST {
                return;
            }
            id += 1;
            added += 1;
            lib.add(Piece {
                id,
                source: c.cite(&shown),
                title: format!("{name}, lines {}-{}", c.start_line, c.end_line),
                text: c.text,
                at,
                embedding: None,
            });
        }
    }
}

pub fn library_from_dir(dir: &std::path::Path) -> Library {
    let mut lib = Library::default();
    let Ok(entries) = std::fs::read_dir(dir) else { return lib };
    let mut paths: Vec<std::path::PathBuf> = entries.flatten().map(|e| e.path()).collect();
    paths.sort();
    let mut id = 0u64;
    for path in paths {
        if path.extension().and_then(|x| x.to_str()) != Some("md") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let title = text
            .lines()
            .find(|l| l.starts_with("# "))
            .map(|l| l.trim_start_matches("# ").trim().to_string())
            .unwrap_or_else(|| path.file_stem().map(|f| f.to_string_lossy().into()).unwrap_or_default());
        let at = std::fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        id += 1;
        lib.add(Piece { id, source: path.to_string_lossy().into(), title, text, at, embedding: None });
    }
    lib
}
