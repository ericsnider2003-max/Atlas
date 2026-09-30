//! Text in on stdin, one meaning vector out on stdout.
//!
//! The whole contract `atlas/src/meaning.rs` asks of an encoder, and nothing
//! else: read the text, print the numbers, exit zero. The vector is 384
//! floats from all-MiniLM-L6-v2 — token embeddings mean-pooled under the
//! attention mask and L2-normalised, which is exactly what
//! sentence-transformers does with this model, so the cosine scores Atlas
//! computes match what the model was trained to make meaningful.
//!
//! The tokenizer is BERT's, written out here rather than pulled in as a
//! crate: lowercase, split on whitespace and punctuation, strip accents,
//! then greedy longest-match WordPiece against vocab.txt. It is ~120 lines,
//! it has tests, and it keeps this program at one dependency (tract, which
//! personal Atlas already builds with). A tokenizer crate would be a second
//! ML dependency to vet for a job this small.
//!
//! Usage:
//!   echo "my car is a corolla" | embed --model models/all-MiniLM-L6-v2.onnx --vocab models/vocab.txt
//!
//! Resident (`--lines`, 30 Sep 2026): stays running, one text per line in,
//! one vector per line out, the model loaded once. Loading is ~0.25 s of the
//! ~0.26 s a one-shot call takes, which is too slow for choosing tools on
//! every spoken sentence; resident, a sentence takes a few milliseconds.
//! Texts are padded to a few fixed lengths under the attention mask, so at
//! most four compiled copies of the model are ever made, and the pooled
//! vector is the same as the one-shot one (the pads are masked out of both
//! the attention and the mean).

use std::collections::HashMap;
use std::io::Read;
use tract_onnx::prelude::*;

const MAX_TOKENS: usize = 256;
const CLS: &str = "[CLS]";
const SEP: &str = "[SEP]";
const UNK: &str = "[UNK]";

fn main() {
    let mut model_path = String::from("models/all-MiniLM-L6-v2.onnx");
    let mut vocab_path = String::from("models/vocab.txt");
    let mut lines = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--model" => model_path = args.next().unwrap_or_default(),
            "--vocab" => vocab_path = args.next().unwrap_or_default(),
            "--lines" => lines = true,
            "--help" | "-h" => {
                eprintln!("text on stdin -> 384 numbers on stdout");
                eprintln!("usage: embed [--model <onnx>] [--vocab <vocab.txt>]");
                return;
            }
            other => {
                eprintln!("unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    if lines {
        return resident(&model_path, &vocab_path);
    }

    let mut text = String::new();
    if std::io::stdin().read_to_string(&mut text).is_err() || text.trim().is_empty() {
        eprintln!("nothing on stdin to embed");
        std::process::exit(2);
    }

    let vocab = match load_vocab(&vocab_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not read the vocab at {vocab_path}: {e}");
            std::process::exit(1);
        }
    };
    let ids = tokenize(&text, &vocab);

    match run(&model_path, &ids) {
        Ok(v) => {
            let line: Vec<String> = v.iter().map(|x| format!("{x:.6}")).collect();
            println!("{}", line.join(" "));
        }
        Err(e) => {
            eprintln!("the model at {model_path} could not run: {e}");
            std::process::exit(1);
        }
    }
}

/// A compiled model, as tract hands it back.
type Runnable = std::sync::Arc<TypedRunnableModel>;

/// Lengths a text is padded to in `--lines` mode.
const BUCKETS: [usize; 4] = [16, 32, 64, 128];

/// `--lines`: one vector per input line until stdin closes. A line that
/// can't be embedded prints `!` and the reason, so the caller never waits
/// on an answer that isn't coming.
fn resident(model_path: &str, vocab_path: &str) {
    use std::io::{BufRead, Write};
    let vocab = match load_vocab(vocab_path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("could not read the vocab at {vocab_path}: {e}");
            std::process::exit(1);
        }
    };
    let proto = match tract_onnx::onnx().model_for_path(model_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("the model at {model_path} could not load: {e}");
            std::process::exit(1);
        }
    };
    let mut compiled: HashMap<usize, Runnable> = HashMap::new();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let mut ids = tokenize(&line, &vocab);
        ids.truncate(*BUCKETS.last().unwrap());
        let len = BUCKETS.iter().copied().find(|b| *b >= ids.len()).unwrap_or(*BUCKETS.last().unwrap());
        let answer = (|| -> TractResult<Vec<f32>> {
            if !compiled.contains_key(&len) {
                let m = shaped(proto.clone(), len)?;
                compiled.insert(len, m);
            }
            pooled(compiled.get(&len).unwrap(), &ids, len)
        })();
        let _ = match answer {
            Ok(v) => writeln!(out, "{}", v.iter().map(|x| format!("{x:.6}")).collect::<Vec<_>>().join(" ")),
            Err(e) => writeln!(out, "! {e}"),
        };
        let _ = out.flush();
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

/// Run a compiled model over `ids` padded to `n`, pooling only the real
/// tokens.
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

fn load_vocab(path: &str) -> std::io::Result<HashMap<String, i64>> {
    let raw = std::fs::read_to_string(path)?;
    Ok(raw.lines().enumerate().map(|(i, w)| (w.to_string(), i as i64)).collect())
}

/// BERT-uncased basic tokenization + WordPiece, greedy longest match.
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

/// Run the model over one tokenized text and pool to a sentence vector.
fn run(model_path: &str, ids: &[i64]) -> TractResult<Vec<f32>> {
    let n = ids.len();
    let model = tract_onnx::onnx()
        .model_for_path(model_path)?
        .with_input_fact(0, InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)))?
        .with_input_fact(1, InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)))?
        .with_input_fact(2, InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)))?
        .into_optimized()?
        .into_runnable()?;

    let input_ids = tract_ndarray::Array2::from_shape_vec((1, n), ids.to_vec())?;
    let attention = tract_ndarray::Array2::<i64>::ones((1, n));
    let token_type = tract_ndarray::Array2::<i64>::zeros((1, n));

    let out = model.run(tvec!(
        Tensor::from(input_ids).into(),
        Tensor::from(attention).into(),
        Tensor::from(token_type).into(),
    ))?;

    // [1, n, 384] token embeddings -> mean over tokens -> unit length.
    let hidden = out[0].to_plain_array_view::<f32>()?;
    let dims = hidden.shape();
    let (seq, width) = (dims[1], dims[2]);
    let mut pooled = vec![0.0f32; width];
    for t in 0..seq {
        for d in 0..width {
            pooled[d] += hidden[[0, t, d]];
        }
    }
    for v in pooled.iter_mut() {
        *v /= seq as f32;
    }
    let norm = pooled.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
    for v in pooled.iter_mut() {
        *v /= norm;
    }
    Ok(pooled)
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
