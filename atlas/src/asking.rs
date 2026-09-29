//! Getting the question ready before searching for it.
//!
//! `recall` takes whatever it is handed and matches it against the index. What
//! it gets handed is a transcript of something you said out loud, and spoken
//! questions are a bad shape for search in three specific ways.
//!
//! **They carry words that match everything.** "Can you find me the thing
//! about the budget" is eight words of scaffolding and one word of content.
//! Every one of those scaffolding words appears in half the index, so they
//! dilute the score of the word that mattered.
//!
//! **They point at things instead of naming them.** "That file I was looking
//! at yesterday" contains no term the index has ever seen. Searching it
//! finds nothing, and nothing reads as a definite answer.
//!
//! **They arrive as one question that is really two.** "What did I decide
//! about the trip and who was I going with" retrieves the average of two
//! topics and the best match for neither.
//!
//! None of that needs a model. It is the cheapest possible improvement to
//! retrieval quality and it happens before anything expensive runs — which
//! matters, because a search that goes off badly wastes the expensive part
//! too.
//!
//! What this deliberately does **not** do is guess at what you meant. It
//! strips, splits and flags. When a question cannot be searched as it stands,
//! it says which part is the problem rather than inventing a referent — an
//! invented one retrieves confidently and wrongly, which is worse than
//! retrieving nothing.

use serde::{Deserialize, Serialize};

/// Words that carry no signal in a search index.
///
/// Not a general stop-word list. These are the words spoken questions are
/// padded with — the ones that appear in most documents and in almost every
/// request, so they drag the score toward the middle wherever they appear.
const SCAFFOLDING: &[&str] = &[
    "can", "you", "please", "find", "me", "get", "show", "tell", "give",
    "what", "whats", "was", "were", "is", "are", "the", "a", "an", "my",
    "that", "this", "about", "for", "of", "on", "in", "to", "do", "did",
    "have", "has", "i", "we", "it", "there", "any", "some", "thing", "things",
    "again", "just", "like", "know", "remember", "look", "up", "search",
];

/// Words that point at something without naming it.
///
/// A question built from these has nothing for an index to match, and the
/// referent lives in the conversation rather than in the notes.
const POINTS_AT_SOMETHING: &[&str] = &[
    "that one", "this one", "the one", "it", "them", "those", "these",
    "the thing", "that thing", "yesterday", "earlier", "last time",
    "the other day", "before", "back then", "just now", "the last one",
];

/// Single words that point rather than name.
///
/// Kept apart from the phrases above because they survive the scaffolding
/// filter on their own — "show me that one again" leaves the term `one`, which
/// looks like a real search term and is not. The rule that matters is not
/// "did anything survive" but "is what survived worth searching for".
const POINTER_WORDS: &[&str] = &[
    "one", "ones", "thing", "things", "stuff", "bit", "part", "file", "note",
    "yesterday", "earlier", "before", "then", "other", "last", "previous",
    // Bare plurals point just as hard as "that one" and survive the
    // scaffolding filter on their own.
    "those", "these", "them", "they", "ours", "mine", "yours",
];

/// Words that join two questions into one.
const JOINS: &[&str] = &[" and also ", " and then ", " and who ", " and what ", " and when ", " and where "];

/// A question, ready to search — or a reason it isn't.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Prepared {
    /// The words worth searching on.
    pub terms: Vec<String>,
    /// What was said, kept so an answer can quote it back.
    pub said: String,
    /// Separate questions found inside one utterance.
    ///
    /// Searched separately rather than together: retrieving the average of two
    /// topics gives the best match for neither.
    pub also: Vec<String>,
}

/// Why a question can't be searched as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Unsearchable {
    /// Nothing left once the scaffolding came off.
    NothingToSearchFor,
    /// It refers to something the index has never seen.
    PointsAtSomething(String),
}

impl Unsearchable {
    /// What to ask, in the words you would use.
    ///
    /// Asks rather than guesses. A guessed referent retrieves confidently and
    /// wrongly, and you have no way to tell that from a real answer.
    pub fn ask(&self) -> String {
        match self {
            Unsearchable::NothingToSearchFor => {
                "I didn't catch anything in that I could look for — what's it about?".into()
            }
            Unsearchable::PointsAtSomething(w) => {
                format!("When you say \"{w}\" — which one do you mean?")
            }
        }
    }
}

fn words(s: &str) -> Vec<String> {
    s.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '-' && c != '\'')
        .filter(|w| !w.is_empty())
        .map(|w| w.to_string())
        .collect()
}

/// Split one utterance into the separate questions inside it.
fn split_questions(said: &str) -> Vec<String> {
    let lower = said.to_lowercase();
    for j in JOINS {
        if let Some(at) = lower.find(j) {
            let (a, b) = said.split_at(at);
            let b = &b[j.len()..];
            // Only a real split. "fish and chips" is one thing, and cutting it
            // leaves two halves that match nothing.
            if words(a).len() >= 2 && words(b).len() >= 2 {
                return vec![a.trim().to_string(), b.trim().to_string()];
            }
        }
    }
    vec![said.to_string()]
}

/// Get a spoken question ready to search.
pub fn prepare(said: &str) -> Result<Prepared, Unsearchable> {
    let parts = split_questions(said);
    let head = parts.first().cloned().unwrap_or_default();

    // Something that points rather than names, and has nothing else in it, is
    // a question about the conversation rather than about the notes.
    let lower = head.to_lowercase();
    let terms: Vec<String> = words(&head)
        .into_iter()
        .filter(|w| !SCAFFOLDING.contains(&w.as_str()))
        .filter(|w| w.len() > 2)
        .collect();

    // Nothing worth searching for is not the same as nothing at all: a
    // question made only of pointers leaves terms behind that look real and
    // match nothing.
    let all_pointers = !terms.is_empty()
        && terms.iter().all(|t| POINTER_WORDS.contains(&t.as_str()));

    if terms.is_empty() || all_pointers {
        if let Some(p) = POINTS_AT_SOMETHING.iter().find(|p| lower.contains(**p)) {
            return Err(Unsearchable::PointsAtSomething((*p).to_string()));
        }
        if let Some(t) = terms.first() {
            return Err(Unsearchable::PointsAtSomething(t.clone()));
        }
        return Err(Unsearchable::NothingToSearchFor);
    }

    Ok(Prepared {
        terms,
        said: said.to_string(),
        also: parts.into_iter().skip(1).collect(),
    })
}

impl Prepared {
    /// The query to hand to the index.
    pub fn query(&self) -> String {
        self.terms.join(" ")
    }

    /// Was anything dropped?
    ///
    /// Reported so a search that found nothing can say what it actually looked
    /// for. "I couldn't find anything about the budget" is a useful answer;
    /// "I couldn't find anything" is not.
    pub fn searched_for(&self) -> String {
        match self.terms.as_slice() {
            [] => "nothing".into(),
            [one] => one.clone(),
            many => many.join(", "),
        }
    }

    /// Is this one question or several?
    pub fn is_more_than_one_question(&self) -> bool {
        !self.also.is_empty()
    }
}
