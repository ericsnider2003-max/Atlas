//! Okapi BM25 over an inverted index, and Reciprocal Rank Fusion to merge it
//! with meaning search.
//!
//! **Sources:** `quickwit-oss/tantivy` (MIT) for the parameters it ships
//! (k1 = 1.2, b = 0.75, and the never-negative IDF
//! `ln(1 + (N − n + 0.5)/(n + 0.5))`); `dorianbrown/rank_bm25` (Apache-2.0)
//! as a second reference — it uses k1 = 1.5 and floors negative IDF with an
//! epsilon, which the tantivy IDF makes unnecessary. RRF from Cormack, Clarke
//! & Büttcher, SIGIR 2009: `score = Σ 1/(k + rank)`, k = 60. Clean-room.
//!
//! **Why Atlas wants it.** `recall::word_score` already has two of BM25's three
//! ideas — a saturating term count (`hits/(hits+1.6)`) and a rarity weight —
//! but not the third: **length normalisation**. Without it a long note that
//! mentions a word once beats a short note that is *about* that word, and
//! Idea #6 (grounded answers from your own files) is exactly where notes get
//! long. It also scans every piece per query; this is an inverted index, so a
//! query touches only the documents that hold its words.
//!
//! RRF is the second half. `recall` merges words and meaning as
//! `words·(1−w) + meaning·w·3.0` — two scores on different scales added with a
//! hand-set weight. RRF merges *ranks*, so it needs no scale and no weight,
//! and it is what the published hybrid-search systems converged on.

use crate::stemmer::stems_of;
use std::collections::HashMap;

pub const K1: f64 = 1.2;
pub const B: f64 = 0.75;
/// Cormack et al.: "near-optimal, but not critical" — 10 to 100 all did well.
pub const RRF_K: f64 = 60.0;

/// One term's BM25 contribution before IDF, scaled into 0..1 by dividing out
/// `K1 + 1` — the shape `recall::word_score` uses, so a note's length now
/// counts against a word it mentions once in passing.
pub fn term_weight(tf: f64, doc_len: f64, avg_len: f64) -> f64 {
    if tf <= 0.0 {
        return 0.0;
    }
    let norm = 1.0 - B + B * doc_len / avg_len.max(1.0);
    tf / (tf + K1 * norm)
}

/// Words too common to rank on. Short on purpose: BM25's IDF already
/// down-weights common words, this only saves index space.
const STOP: [&str; 25] = [
    "a", "an", "and", "are", "as", "at", "be", "by", "for", "from", "has", "in", "is", "it", "of", "on", "or", "that",
    "the", "this", "to", "was", "were", "will", "with",
];

#[derive(Debug, Default, Clone)]
pub struct Index {
    ids: Vec<u64>,
    lens: Vec<u32>,
    total_len: u64,
    live: u64,
    postings: HashMap<String, Vec<(u32, u32)>>,
}

fn terms(text: &str) -> Vec<String> {
    stems_of(text).into_iter().filter(|w| !STOP.contains(&w.as_str())).collect()
}

impl Index {
    /// Add a document. The title counts twice — a cheap BM25F: a word in the
    /// title is stronger evidence of what a chunk is about.
    pub fn add(&mut self, id: u64, title: &str, body: &str) {
        let mut toks = terms(title);
        toks.extend(terms(title));
        toks.extend(terms(body));
        let doc = self.ids.len() as u32;
        let mut tf: HashMap<String, u32> = HashMap::new();
        for t in &toks {
            *tf.entry(t.clone()).or_default() += 1;
        }
        for (t, n) in tf {
            self.postings.entry(t).or_default().push((doc, n));
        }
        self.ids.push(id);
        self.lens.push(toks.len() as u32);
        self.total_len += toks.len() as u64;
        self.live += 1;
    }

    fn idf(&self, n: usize) -> f64 {
        let big_n = self.live as f64;
        (1.0 + (big_n - n as f64 + 0.5) / (n as f64 + 0.5)).ln()
    }

    /// Top `k` documents for `query`, best first, as `(id, score)`.
    pub fn search(&self, query: &str, k: usize) -> Vec<(u64, f64)> {
        if self.live == 0 {
            return vec![];
        }
        let avg = self.total_len as f64 / self.live as f64;
        let mut q = terms(query);
        q.sort();
        q.dedup();
        let mut acc: HashMap<u32, f64> = HashMap::new();
        for t in &q {
            let Some(list) = self.postings.get(t) else { continue };
            let idf = self.idf(list.len());
            for (d, tf) in list {
                let tf = *tf as f64;
                let len = self.lens[*d as usize] as f64;
                let s = idf * tf * (K1 + 1.0) / (tf + K1 * (1.0 - B + B * len / avg.max(1.0)));
                *acc.entry(*d).or_default() += s;
            }
        }
        let mut out: Vec<(u64, f64)> = acc.into_iter().map(|(d, s)| (self.ids[d as usize], s)).collect();
        out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        out.truncate(k);
        out
    }
}

/// Reciprocal Rank Fusion. Each input is a ranked list of ids, best first.
/// An id missing from a list simply gets nothing from it. Ties break by id so
/// the result is deterministic.
pub fn rrf(lists: &[Vec<u64>], k: f64) -> Vec<(u64, f64)> {
    let mut acc: HashMap<u64, f64> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *acc.entry(*id).or_default() += 1.0 / (k + rank as f64 + 1.0);
        }
    }
    let mut out: Vec<(u64, f64)> = acc.into_iter().collect();
    out.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idx() -> Index {
        let mut i = Index::default();
        i.add(1, "VPS disk", "The homelab server disk filled up overnight.");
        i.add(
            2,
            "Long journal",
            "Monday I went for a walk, cooked, read a book about ships, fixed the fence, \
             called mum, looked at the garden, and at some point the disk was mentioned once \
             by someone at dinner, then we talked about holidays and weather and cars for hours.",
        );
        i.add(3, "Trading notes", "Closed three trades early; trading was slow.");
        i
    }

    #[test]
    fn short_note_about_it_beats_long_note_mentioning_it() {
        let r = idx().search("disk", 3);
        assert_eq!(r[0].0, 1);
        assert_eq!(r[1].0, 2);
    }

    #[test]
    fn stemming_finds_other_forms() {
        let r = idx().search("what did I trade", 3);
        assert_eq!(r.first().map(|x| x.0), Some(3));
    }

    #[test]
    fn idf_never_negative_even_for_a_word_in_every_doc() {
        let mut i = Index::default();
        for id in 0..5 {
            i.add(id, "", "atlas atlas");
        }
        assert!(i.search("atlas", 5).iter().all(|(_, s)| *s > 0.0));
    }

    #[test]
    fn term_weight_saturates_and_penalises_length() {
        assert_eq!(term_weight(0.0, 10.0, 10.0), 0.0);
        let short = term_weight(1.0, 5.0, 20.0);
        let long = term_weight(1.0, 80.0, 20.0);
        assert!(short > long);
        assert!(term_weight(50.0, 20.0, 20.0) < 1.0);
    }

    #[test]
    fn rrf_rewards_agreement_between_lists() {
        let words = vec![10, 20, 30];
        let meaning = vec![30, 40, 10];
        let fused = rrf(&[words, meaning], RRF_K);
        // 10 is 1st and 3rd; 30 is 3rd and 1st: tie, broken by id.
        assert_eq!(fused[0].0, 10);
        assert_eq!(fused[1].0, 30);
        assert!(fused[0].1 > fused[2].1);
        assert_eq!(fused.len(), 4);
    }
}
