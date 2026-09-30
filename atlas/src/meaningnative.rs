//! The meaning encoder inside Atlas (30 Sep 2026).
//!
//! The same model and maths as the `embed` program (all-MiniLM-L6-v2,
//! mean-pooled under the attention mask, unit length), run in-process with
//! the `tract` Atlas already builds with. So the laptop needs two downloaded
//! files (`atlas get understanding`: the model and its word list, both
//! Hugging Face's own, hash-pinned) and no separate program. Texts are padded
//! to a few fixed lengths so at most four compiled copies are ever made;
//! pads are masked out of the attention and the mean, so the vector is the
//! one the one-shot program gives (checked against it, 30 Sep 2026).
//!
//! The tokenizer is copied from `embed/src/main.rs`, tests and all.

use std::collections::HashMap;
use std::path::Path;
use tract_onnx::prelude::*;

const MAX_TOKENS: usize = 256;
const CLS: &str = "[CLS]";
const SEP: &str = "[SEP]";
const UNK: &str = "[UNK]";

/// Where the two files land, install-relative.
pub const MODEL: &str = "models/understanding/all-MiniLM-L6-v2.onnx";
pub const VOCAB: &str = "models/understanding/vocab.txt";

/// Lengths a text is padded to.
const BUCKETS: [usize; 4] = [16, 32, 64, 128];

type Runnable = std::sync::Arc<TypedRunnableModel>;

/// The encoder, loaded.
pub struct Native {
    vocab: HashMap<String, i64>,
    proto: InferenceModel,
    compiled: HashMap<usize, Runnable>,
}

impl Native {
    /// Load from an install: `None` when the files aren't there.
    pub fn load(root: &Path) -> Option<Native> {
        let (m, v) = (root.join(MODEL), root.join(VOCAB));
        if !m.exists() || !v.exists() {
            return None;
        }
        let raw = std::fs::read_to_string(&v).ok()?;
        let vocab = raw.lines().enumerate().map(|(i, w)| (w.to_string(), i as i64)).collect();
        let proto = tract_onnx::onnx().model_for_path(&m).ok()?;
        Some(Native { vocab, proto, compiled: HashMap::new() })
    }

    /// Are the files there?
    pub fn installed(root: &Path) -> bool {
        root.join(MODEL).exists() && root.join(VOCAB).exists()
    }

    pub fn embed(&mut self, text: &str) -> Option<Vec<f32>> {
        let mut ids = tokenize(text, &self.vocab);
        let most = *BUCKETS.last().unwrap();
        if ids.len() > most {
            ids.truncate(most - 1);
            ids.push(*self.vocab.get(SEP).unwrap_or(&102));
        }
        let n = BUCKETS.iter().copied().find(|b| *b >= ids.len()).unwrap_or(most);
        if !self.compiled.contains_key(&n) {
            let m = shaped(self.proto.clone(), n).ok()?;
            self.compiled.insert(n, m);
        }
        pooled(self.compiled.get(&n)?, &ids, n).ok()
    }
}

fn shaped(model: InferenceModel, n: usize) -> TractResult<Runnable> {
    model
        .with_input_fact(0, InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)))?
        .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)))?
        .with_input_fact(2, InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)))?
        .into_optimized()?
        .into_runnable()
}

fn pooled(model: &Runnable, ids: &[i64], n: usize) -> TractResult<Vec<f32>> {
    let real = ids.len().min(n);
    let mut padded = ids[..real].to_vec();
    padded.resize(n, 0);
    let mut mask = vec![1i64; real];
    mask.resize(n, 0);
    let out = model.run(tvec!(
        Tensor::from(tract_ndarray::Array2::from_shape_vec((1, n), padded)?).into(),
        Tensor::from(tract_ndarray::Array2::from_shape_vec((1, n), mask)?).into(),
        Tensor::from(tract_ndarray::Array2::<i64>::zeros((1, n))).into(),
    ))?;
    let hidden = out[0].to_plain_array_view::<f32>()?;
    let width = hidden.shape()[2];
    let mut v = vec![0.0f32; width];
    for t in 0..real {
        for d in 0..width {
            v[d] += hidden[[0, t, d]];
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    Ok(v.into_iter().map(|x| x / norm).collect())
}

fn tokenize(text: &str, vocab: &HashMap<String, i64>) -> Vec<i64> {
    let unk = *vocab.get(UNK).unwrap_or(&100);
    let mut ids = vec![*vocab.get(CLS).unwrap_or(&101)];

    for word in basic_tokens(text) {
        if ids.len() >= MAX_TOKENS - 1 {
            break;
        }
        for piece in wordpiece(&word, vocab, unk) {
            if ids.len() >= MAX_TOKENS - 1 {
                break;
            }
            ids.push(piece);
        }
    }
    ids.push(*vocab.get(SEP).unwrap_or(&102));
    ids
}

/// Lowercase, strip accents, split on whitespace, and make every punctuation
/// character its own token — BERT's "basic tokenizer" for the uncased models.
fn basic_tokens(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in text.chars().flat_map(unaccented) {
        let c = c.to_lowercase().next().unwrap_or(c);
        if c.is_whitespace() {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
        } else if c.is_ascii_punctuation() || is_cjk_punct(c) {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            out.push(c.to_string());
        } else if !c.is_control() {
            cur.push(c);
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// The accents that actually occur in English notes — café, naïve, résumé.
/// Full Unicode NFD is a crate; this table is the working subset, and an
/// unlisted accented character simply stays itself and WordPiece copes.
fn unaccented(c: char) -> std::vec::IntoIter<char> {
    let s = match c {
        'à' | 'á' | 'â' | 'ã' | 'ä' | 'å' => "a",
        'è' | 'é' | 'ê' | 'ë' => "e",
        'ì' | 'í' | 'î' | 'ï' => "i",
        'ò' | 'ó' | 'ô' | 'õ' | 'ö' => "o",
        'ù' | 'ú' | 'û' | 'ü' => "u",
        'ç' => "c",
        'ñ' => "n",
        'ý' | 'ÿ' => "y",
        _ => return vec![c].into_iter(),
    };
    s.chars().collect::<Vec<_>>().into_iter()
}

fn is_cjk_punct(c: char) -> bool {
    matches!(c, '、' | '。' | '「' | '」' | '『' | '』' | '，' | '．')
}

/// Greedy longest-match against the vocab; continuations carry `##`.
/// A word no prefix of which is in the vocab becomes [UNK], whole — the
/// standard behaviour, so ids match what the model saw in training.
fn wordpiece(word: &str, vocab: &HashMap<String, i64>, unk: i64) -> Vec<i64> {
    let chars: Vec<char> = word.chars().collect();
    if chars.len() > 100 {
        return vec![unk];
    }
    let mut out = Vec::new();
    let mut start = 0;
    while start < chars.len() {
        let mut end = chars.len();
        let mut found = None;
        while start < end {
            let mut piece: String = chars[start..end].iter().collect();
            if start > 0 {
                piece = format!("##{piece}");
            }
            if let Some(&id) = vocab.get(&piece) {
                found = Some(id);
                break;
            }
            end -= 1;
        }
        match found {
            Some(id) => {
                out.push(id);
                start = end;
            }
            None => return vec![unk],
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab_of(words: &[&str]) -> HashMap<String, i64> {
        words.iter().enumerate().map(|(i, w)| (w.to_string(), i as i64)).collect()
    }

    #[test]
    fn wordpiece_splits_with_continuations() {
        let v = vocab_of(&["[UNK]", "un", "##aff", "##able"]);
        assert_eq!(wordpiece("unaffable", &v, 0), vec![1, 2, 3]);
    }

    #[test]
    fn an_unknown_word_is_unk_whole_not_partly_matched() {
        let v = vocab_of(&["[UNK]", "un"]);
        // "un" matches but "matchable" has no continuation piece — the whole
        // word must collapse to [UNK], not leak a half-tokenization.
        assert_eq!(wordpiece("unmatchable", &v, 0), vec![0]);
    }

    #[test]
    fn basic_tokens_lowercase_split_punctuation_and_accents() {
        assert_eq!(
            basic_tokens("Café's open, isn't it?"),
            vec!["cafe", "'", "s", "open", ",", "isn", "'", "t", "it", "?"]
        );
    }

    #[test]
    fn cls_and_sep_frame_the_ids() {
        let v = vocab_of(&["[UNK]", "hello", "[CLS]", "[SEP]"]);
        assert_eq!(tokenize("hello hello", &v), vec![2, 1, 1, 3]);
    }
}
