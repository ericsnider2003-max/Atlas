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
use std::io::Read;
use std::path::Path;
use tract_onnx::prelude::*;

const MAX_TOKENS: usize = 256;
const MAX_VOCAB_BYTES: u64 = 16 * 1024 * 1024;
const CLS: &str = "[CLS]";
const SEP: &str = "[SEP]";
const UNK: &str = "[UNK]";

/// Where the two files land, install-relative.
pub const MODEL: &str = "models/understanding/all-MiniLM-L6-v2.onnx";
pub const VOCAB: &str = "models/understanding/vocab.txt";
/// The same model quantised to 8-bit integers: a quarter of the size, the
/// one the phone app carries (`bundled`, 24 Oct 2026). Used when it's there
/// and the full-size one isn't.
pub const MODEL_INT8: &str = "models/understanding/all-MiniLM-L6-v2-int8.onnx";

/// The model file to load: the full-size one if it's installed, else the
/// 8-bit one, else none.
pub(crate) fn model_path(root: &Path) -> Option<std::path::PathBuf> {
    [MODEL, MODEL_INT8]
        .iter()
        .map(|m| root.join(m))
        .find(|p| p.exists())
}

/// Lengths a text is padded to.
const BUCKETS: [usize; 4] = [16, 32, 64, 128];
/// The largest bucket, read at compile time: an empty `BUCKETS` fails the build
/// rather than panicking at run time (audit Q2).
const MOST_TOKENS: usize = BUCKETS[BUCKETS.len() - 1];

type Runnable = std::sync::Arc<TypedRunnableModel>;

/// The encoder, loaded.
pub struct Native {
    vocab: HashMap<String, i64>,
    proto: InferenceModel,
    compiled: HashMap<usize, Runnable>,
    /// On the NPU (item 20), one fixed length per bucket: `None` once a
    /// length was tried there and refused, so it stays on `tract`.
    npu: HashMap<usize, Option<(crate::npu::Session, Vec<String>)>>,
    /// Lengths whose first NPU run was checked against the processor's.
    npu_judged: Vec<usize>,
    root: std::path::PathBuf,
}

impl Native {
    /// Load from an install: `None` when the files aren't there.
    pub fn load(root: &Path) -> Option<Native> {
        let (m, v) = (model_path(root)?, root.join(VOCAB));
        if !v.exists() {
            return None;
        }
        let file = std::fs::File::open(&v).ok()?;
        let metadata = file.metadata().ok()?;
        if !metadata.is_file() || metadata.len() > MAX_VOCAB_BYTES {
            return None;
        }
        let mut raw = String::new();
        file.take(MAX_VOCAB_BYTES + 1)
            .read_to_string(&mut raw)
            .ok()?;
        if raw.len() as u64 > MAX_VOCAB_BYTES {
            return None;
        }
        let vocab = raw
            .lines()
            .enumerate()
            .map(|(i, w)| (w.to_string(), i as i64))
            .collect();
        let proto = tract_onnx::onnx().model_for_path(&m).ok()?;
        // A model that reads but can't be compiled would answer every text
        // with nothing (24 Oct 2026: the 8-bit export failed tract's analysis
        // at its first Unsqueeze). Prove the smallest length compiles, or
        // say there's no meaning layer rather than a broken one.
        let first = shaped(proto.clone(), BUCKETS[0]);
        if let Err(e) = &first {
            crate::errln!(
                "atlas: the meaning model at {} can't run here ({e}); meaning search is off",
                m.display()
            );
            return None;
        }
        let mut compiled = HashMap::new();
        if let Ok(run) = first {
            compiled.insert(BUCKETS[0], run);
        }
        Some(Native {
            vocab,
            proto,
            compiled,
            npu: HashMap::new(),
            npu_judged: Vec::new(),
            root: root.to_path_buf(),
        })
    }

    /// Are the files there?
    pub fn installed(root: &Path) -> bool {
        model_path(root).is_some() && root.join(VOCAB).exists()
    }

    pub fn embed(&mut self, text: &str) -> Option<Vec<f32>> {
        self.embed_where(text, true)
    }

    /// Which padded lengths run on the NPU now (true) or were sent back to
    /// the processor (false), for `atlas npu-check`.
    pub fn npu_lengths(&self) -> Vec<(usize, bool)> {
        let mut v: Vec<(usize, bool)> = self
            .npu
            .iter()
            .map(|(n, s)| {
                (
                    *n,
                    s.as_ref()
                        .is_some_and(|(session, _)| session.on() == crate::npu::Where::Npu),
                )
            })
            .collect();
        v.sort();
        v
    }

    /// The same, on the processor only (`tract`): what `atlas npu-check`
    /// compares the NPU against.
    pub fn embed_on_processor(&mut self, text: &str) -> Option<Vec<f32>> {
        self.embed_where(text, false)
    }

    fn embed_where(&mut self, text: &str, npu: bool) -> Option<Vec<f32>> {
        let mut ids = tokenize(text, &self.vocab);
        let most = MOST_TOKENS;
        if ids.len() > most {
            ids.truncate(most - 1);
            ids.push(*self.vocab.get(SEP).unwrap_or(&102));
        }
        let n = BUCKETS
            .iter()
            .copied()
            .find(|b| *b >= ids.len())
            .unwrap_or(most);
        // The NPU first, where there is one; the same vector either way.
        if npu {
            if let Some(v) = self.on_npu(&ids, n) {
                return Some(v);
            }
        }
        if !self.compiled.contains_key(&n) {
            let m = shaped(self.proto.clone(), n).ok()?;
            self.compiled.insert(n, m);
        }
        pooled(self.compiled.get(&n)?, &ids, n).ok()
    }
}

impl Native {
    /// The vector from the NPU, or `None` to use `tract` (no NPU, or this
    /// length was refused there -- said once).
    fn on_npu(&mut self, ids: &[i64], n: usize) -> Option<Vec<f32>> {
        if !crate::npu::npu_ready(&self.root) {
            return None;
        }
        let root = self.root.clone();
        let session = self.npu.entry(n).or_insert_with(|| {
            let model = root.join(MODEL);
            let names = crate::npu::Session::input_names(&root, &model).ok()?;
            let shapes: Vec<(String, Vec<i64>)> = names
                .iter()
                .map(|nm| (nm.clone(), vec![1, n as i64]))
                .collect();
            // Lost to the processor last time, with this engine: not tried again.
            if crate::npu::lost_before(&model, &shapes) {
                return None;
            }
            match crate::npu::Session::open(&root, &model, &shapes, crate::npu::Where::Npu) {
                Ok(s) => Some((s, names)),
                Err(why) => {
                    crate::outln!("search stays on the processor for {n}-word texts: {why}");
                    None
                }
            }
        });
        let (s, names) = session.as_ref()?;
        let names_c = names.clone();
        // The export's own names, in its own order: ids, mask, segment.
        let name = |want: &str, i: usize| {
            names
                .iter()
                .find(|x| x.contains(want))
                .or(names.get(i))
                .cloned()
                .unwrap_or_default()
        };
        let real = ids.len().min(n);
        let mut padded = ids[..real].to_vec();
        padded.resize(n, 0);
        let mut mask = vec![1i64; real];
        mask.resize(n, 0);
        let mut inputs = vec![
            crate::npu::In::I64(name("input_ids", 0), vec![1, n as i64], padded),
            crate::npu::In::I64(name("attention_mask", 1), vec![1, n as i64], mask),
        ];
        if names.len() > 2 {
            inputs.push(crate::npu::In::I64(
                name("token_type", 2),
                vec![1, n as i64],
                vec![0; n],
            ));
        }
        let (out, npu_took) = s.run_timed(inputs).ok()?;
        if s.on() != crate::npu::Where::Npu {
            return None;
        }
        let v = pool_flat(out.first()?, real, n);
        // The first sentence at this length runs on both, once: the NPU stays
        // only if it gives the processor's answer and is quicker.
        if !self.npu_judged.contains(&n) {
            self.npu_judged.push(n);
            let t = std::time::Instant::now();
            let cpu = self.embed_where_tract(ids, n);
            let cpu_took = t.elapsed();
            let agree = cpu
                .as_ref()
                .map(|c| crate::npu::agreement(c, &v))
                .unwrap_or(0.0);
            let keep = crate::npu::worth_keeping(npu_took, cpu_took, agree);
            let model = self.root.join(MODEL);
            let shapes: Vec<(String, Vec<i64>)> = names_c
                .iter()
                .map(|x| (x.clone(), vec![1, n as i64]))
                .collect();
            crate::npu::remember(&model, &shapes, keep);
            if !keep {
                crate::outln!(
                    "search stays on the processor for {n}-word texts: the NPU took {} ms against {} ms (answers agree to {agree:.3})",
                    npu_took.as_millis(),
                    cpu_took.as_millis()
                );
                self.npu.insert(n, None);
                return cpu;
            }
        }
        Some(v)
    }

    /// `tract`'s answer for already-tokenized ids at length `n`.
    fn embed_where_tract(&mut self, ids: &[i64], n: usize) -> Option<Vec<f32>> {
        if !self.compiled.contains_key(&n) {
            let m = shaped(self.proto.clone(), n).ok()?;
            self.compiled.insert(n, m);
        }
        pooled(self.compiled.get(&n)?, ids, n).ok()
    }
}

/// Mean over the real tokens of a `[1, n, width]` hidden state, unit length:
/// the same pooling `pooled` does, for a flat output.
pub fn pool_flat(hidden: &[f32], real: usize, n: usize) -> Vec<f32> {
    let width = hidden.len() / n.max(1);
    let mut v = vec![0.0f32; width];
    for t in 0..real.min(n) {
        for d in 0..width {
            v[d] += hidden[t * width + d];
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.into_iter().map(|x| x / norm).collect()
}

fn shaped(model: InferenceModel, n: usize) -> TractResult<Runnable> {
    model
        .with_input_fact(
            0,
            InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)),
        )?
        .with_input_fact(
            1,
            InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)),
        )?
        .with_input_fact(
            2,
            InferenceFact::dt_shape(i64::datum_type(), tvec!(1, n as i64)),
        )?
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
        words
            .iter()
            .enumerate()
            .map(|(i, w)| (w.to_string(), i as i64))
            .collect()
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

/// The phone's 8-bit meaning layer against the laptop's full-size one, on
/// real files (24 Oct 2026). Run by hand: `ATLAS_INT8_SPIKE=<dir>` where
/// `<dir>/full` and `<dir>/int8` are each an install root holding one model
/// and the word list. Prints how close each pair of vectors is and whether
/// the 8-bit layer picks the same nearest meaning.
#[cfg(test)]
mod int8_against_full {
    use super::*;

    fn cos(a: &[f32], b: &[f32]) -> f32 {
        a.iter().zip(b).map(|(x, y)| x * y).sum()
    }

    #[test]
    #[ignore = "needs both model files; run with ATLAS_INT8_SPIKE set"]
    fn the_8_bit_layer_means_what_the_full_one_means() {
        let dir =
            std::path::PathBuf::from(std::env::var("ATLAS_INT8_SPIKE").expect("ATLAS_INT8_SPIKE"));
        let mut full = Native::load(&dir.join("full")).expect("full model");
        let mut int8 = Native::load(&dir.join("int8")).expect("int8 model");
        let meanings = [
            "set a timer",
            "what time is it",
            "remind me to call mum at six",
            "back up my documents folder",
            "how much memory is free",
            "open the downloads folder",
            "send this to my laptop",
            "what's on my calendar today",
            "turn the volume down",
            "find the photo of the beach",
            "read me the last message",
            "pause everything",
        ];
        let asked = [
            ("start a ten minute timer", 0),
            ("tell me the time", 1),
            ("ping mum at 6pm", 2),
            ("copy my docs somewhere safe", 3),
            ("am I running out of ram", 4),
            ("show me what I downloaded", 5),
            ("do this on the computer instead", 6),
            ("anything on today", 7),
            ("quieter please", 8),
            ("where is that beach picture", 9),
            ("what did they just send me", 10),
            ("stop what you're doing", 11),
            ("wake me at seven tomorrow", 2),
            ("how full is my disk", 4),
        ];
        let mut sims = Vec::new();
        let (mut same, mut full_right, mut int8_right) = (0, 0, 0);
        let fm: Vec<Vec<f32>> = meanings.iter().map(|m| full.embed(m).unwrap()).collect();
        let im: Vec<Vec<f32>> = meanings.iter().map(|m| int8.embed(m).unwrap()).collect();
        for (i, m) in meanings.iter().enumerate() {
            let _ = m;
            sims.push(cos(&fm[i], &im[i]));
        }
        for (q, want) in asked {
            let (f, n) = (full.embed(q).unwrap(), int8.embed(q).unwrap());
            sims.push(cos(&f, &n));
            let best = |v: &Vec<f32>, set: &Vec<Vec<f32>>| {
                (0..set.len())
                    .max_by(|a, b| cos(v, &set[*a]).total_cmp(&cos(v, &set[*b])))
                    .unwrap()
            };
            let (bf, bi) = (best(&f, &fm), best(&n, &im));
            same += (bf == bi) as usize;
            full_right += (bf == want) as usize;
            int8_right += (bi == want) as usize;
        }
        let min = sims.iter().cloned().fold(f32::MAX, f32::min);
        let mean = sims.iter().sum::<f32>() / sims.len() as f32;
        let t = std::time::Instant::now();
        for _ in 0..20 {
            int8.embed("remind me to call mum at six").unwrap();
        }
        let int8_ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        let t = std::time::Instant::now();
        for _ in 0..20 {
            full.embed("remind me to call mum at six").unwrap();
        }
        let full_ms = t.elapsed().as_secs_f64() * 1000.0 / 20.0;
        println!(
            "SPIKE cosine full-vs-int8 mean {mean:.4} min {min:.4} over {} texts; same nearest {same}/{}; right full {full_right}/{n} int8 {int8_right}/{n}; ms/embed full {full_ms:.1} int8 {int8_ms:.1}",
            sims.len(),
            asked.len(),
            n = asked.len()
        );
        assert!(min > 0.95, "the 8-bit layer drifts: min cosine {min}");
        assert!(
            int8_right >= full_right.saturating_sub(1),
            "the 8-bit layer picks worse"
        );
    }
}

#[cfg(test)]
mod int8_why {
    #[test]
    #[ignore = "diagnostic; needs ATLAS_INT8_SPIKE"]
    fn why_the_8_bit_model_does_not_run() {
        let dir = std::path::PathBuf::from(std::env::var("ATLAS_INT8_SPIKE").unwrap());
        let m = dir.join("int8").join(super::MODEL_INT8);
        let proto =
            tract_onnx::prelude::Framework::model_for_path(&tract_onnx::onnx(), &m).unwrap();
        for (label, f) in [
            ("int8", m.clone()),
            ("full", dir.join("full").join(super::MODEL)),
        ] {
            let g =
                tract_onnx::prelude::Framework::model_for_path(&tract_onnx::onnx(), &f).unwrap();
            let names: Vec<String> = g
                .input_outlets()
                .unwrap()
                .iter()
                .map(|o| g.node(o.node).name.clone())
                .collect();
            println!("INPUTS {label}: {names:?}");
        }
        match super::shaped(proto, 16) {
            Ok(_) => println!("INT8 shaped ok"),
            Err(e) => println!("INT8 ERROR {e:?}"),
        }
    }
}
