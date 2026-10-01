//! Hearing the name by its sound: sherpa-onnx keyword spotting (1 Oct 2026,
//! research report item 25).
//!
//! Until now the wake word was found by writing out everything said near
//! the microphone with Parakeet and looking for "Atlas" in the words
//! (`VoiceWork::name_in`). This is the spotter sherpa-onnx ships for the
//! job: a 3.3-million-parameter streaming zipformer trained on GigaSpeech
//! (`sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01`, Apache-2.0),
//! told to listen for "ATLAS" and "HEY ATLAS". It runs in Atlas's own
//! process through sherpa-onnx's C library, the same library and version
//! Kokoro and Parakeet already use (`kokoro::SHERPA_VERSION`).
//!
//! **Measured before it was wired (1 Oct 2026)**, on 63 requests that start
//! with the name ("Atlas, what time is it?", "Hey Atlas, open Chrome.",
//! "Atlas.") and 135 near-misses ("At last we're finally home", "I use
//! Atlassian at work", "Alice, can you pass the salt?", "Hey Alexa"...), in
//! nine synthetic voices (piper: four speakers of LibriTTS-R and three
//! single-speaker voices, US and British), plus 50 LibriSpeech readings:
//!
//! | | heard the name | false wakes (no "atlas" said) |
//! |---|---|---|
//! | Parakeet, name in the words (as before) | 58/63 | 0/167 |
//! | the spotter alone | 58/63 | 3/167 (all "Atlassian") |
//! | both must agree | 56/63 | 0/167 |
//! | Parakeet, or the spotter plus a sound-alike first word | **60/63** | **0/167** |
//!
//! So the spotter is not more accurate than Parakeet on its own, and the
//! report's idea of *replacing* speech-to-text with it would have cost
//! nothing in accuracy only by luck. What it is good for:
//!
//! - **Recovering misheard names.** Parakeet wrote "Alice, what time is
//!   it?" and "He alice." for two of the requests the spotter heard: when
//!   the spotter heard the name and the first word sounds like it, that
//!   word is taken as the name (`soundalike_at_start`). That is the last
//!   row, and it is on whenever the spotter is installed.
//! - **Not writing out speech that isn't for Atlas** (`wake.listen_first`,
//!   off by default): the spotter, tiny and fast, decides first, and
//!   Parakeet only hears speech that had the name in it. Saves processor
//!   and keeps the room's talk out of transcripts, at the measured cost of
//!   two requests in 63. Worth turning on once it's measured on your own
//!   voice and microphone.
//!
//! "Atlas" spoken mid-sentence ("Charles Atlas was a bodybuilder") is heard
//! by both, as it should be: whether that was *for* Atlas is `addressing`'s
//! question, not hearing's.
//!
//! The spotter only knows the name as the model's own word pieces (`▁AT LA
//! S`), so it listens only when the wake phrase is "Atlas"; a different
//! phrase works as before.

use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use crate::getpieces::{Lands, Piece};

/// Where the model lands, install-relative.
pub const MODEL_DIR: &str = "models/kws";

const ENCODER: &str = "encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx";
const DECODER: &str = "decoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx";
const JOINER: &str = "joiner-epoch-12-avg-2-chunk-16-left-64.int8.onnx";

/// The name, as the model's word pieces (its own tokenizer's output for
/// "ATLAS" and "HEY ATLAS").
pub const KEYWORDS: &str = "▁AT LA S @atlas\n▁HE Y ▁AT LA S @atlas\n";

/// The measured best (see the table above): a boost of 1.0 to the name's
/// pieces, and a hit when they average 0.15 or more.
const SCORE: f32 = 1.0;
const THRESHOLD: f32 = 0.15;

/// The model, pinned like every other download. Hash and size from
/// downloading it on 1 Oct 2026.
pub fn model_piece() -> Piece {
    Piece {
        name: "the wake-word spotter",
        for_what: "hearing its name by the sound, not only in the words",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/kws-models/sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01.tar.bz2",
        sha256: "f170013b4716e41b62b9bfd809687c207cef798ef9bc6534d524e17af9b6561a",
        bytes: 17_626_723,
        lands: Lands::Zip {
            inside: "sherpa-onnx-kws-zipformer-gigaspeech-3.3M-2024-01-01",
            dir: MODEL_DIR,
            key: "models/kws/encoder-epoch-12-avg-2-chunk-16-left-64.int8.onnx",
        },
    }
}

/// Is the phrase one the spotter knows?
pub fn knows(phrase: &str) -> bool {
    let p: String = phrase.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    p == "atlas" || p == "heyatlas"
}

/// The words Parakeet has written for "Atlas" when it misheard it (1 Oct
/// 2026: "Alice", "Ellis", "Axis", "He alice"), and near spellings. Taken
/// as the name only when the spotter heard the name too.
const SOUNDALIKES: &[&str] = &["alice", "ellis", "axis", "atlis", "atless", "atlus", "atlass", "attlas", "adlas", "atla", "outlas", "addis"];

/// When the first word (after "hey", "ok", "okay") sounds like the name:
/// what was said after it.
pub fn soundalike_at_start(words: &str) -> Option<String> {
    let all: Vec<&str> = words.split_whitespace().collect();
    let plain = |w: &str| w.chars().filter(|c| c.is_alphanumeric()).collect::<String>().to_lowercase();
    let mut i = 0;
    while i < all.len().min(2) && matches!(plain(all[i]).as_str(), "hey" | "hi" | "ok" | "okay" | "he" | "a") {
        i += 1;
    }
    let w = plain(all.get(i)?);
    if !SOUNDALIKES.contains(&w.as_str()) {
        return None;
    }
    let rest = all[i + 1..].join(" ");
    Some(rest.trim_start_matches(|c: char| !c.is_alphanumeric()).trim().to_string())
}

// ---------------------------------------------------------------- where it is

/// The sherpa-onnx library folders Atlas may already have: Parakeet's
/// (`tools/sherpa`, the hearing download) and Kokoro's (`tools/kokoro`).
/// Either has the C library; the first with both files wins.
fn runtime_dirs(root: &Path) -> Vec<PathBuf> {
    vec![root.join("tools/sherpa/lib"), root.join("tools/sherpa/bin"), root.join(crate::kokoro::RUNTIME_DIR)]
}

/// The library folder and the model folder, once both are here.
pub(crate) fn ready(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let (ort, capi) = crate::kokoro::runtime_files()?;
    let runtime = runtime_dirs(root).into_iter().find(|d| d.join(ort).is_file() && d.join(capi).is_file())?;
    let model = root.join(MODEL_DIR);
    [ENCODER, DECODER, JOINER, "tokens.txt"].iter().all(|f| model.join(f).is_file()).then_some((runtime, model))
}

// ---------------------------------------------------------------- the library

// Copied from sherpa-onnx-sys 1.13.8 `src/online_asr.rs` and `src/kws.rs`
// (Apache-2.0). Only right for that version; `Spotter::load` checks the
// version before using them.
#[repr(C)]
#[derive(Clone, Copy)]
struct TransducerConfig {
    encoder: *const c_char,
    decoder: *const c_char,
    joiner: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ParaformerConfig {
    encoder: *const c_char,
    decoder: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct OneModel {
    model: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct OnlineModelConfig {
    transducer: TransducerConfig,
    paraformer: ParaformerConfig,
    zipformer2_ctc: OneModel,
    tokens: *const c_char,
    num_threads: i32,
    provider: *const c_char,
    debug: i32,
    model_type: *const c_char,
    modeling_unit: *const c_char,
    bpe_vocab: *const c_char,
    tokens_buf: *const u8,
    tokens_buf_size: i32,
    nemo_ctc: OneModel,
    t_one_ctc: OneModel,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct FeatureConfig {
    sample_rate: i32,
    feature_dim: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SpotterConfig {
    feat_config: FeatureConfig,
    model_config: OnlineModelConfig,
    max_active_paths: i32,
    num_trailing_blanks: i32,
    keywords_score: f32,
    keywords_threshold: f32,
    keywords_file: *const c_char,
    keywords_buf: *const c_char,
    keywords_buf_size: i32,
}

#[repr(C)]
struct KeywordResult {
    keyword: *const c_char,
    tokens: *const c_char,
    tokens_arr: *const *const c_char,
    count: i32,
    timestamps: *mut f32,
    start_time: f32,
    json: *const c_char,
}

type VersionFn = unsafe extern "C" fn() -> *const c_char;
type CreateFn = unsafe extern "C" fn(*const SpotterConfig) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type StreamFn = unsafe extern "C" fn(*const c_void) -> *const c_void;
type DestroyStreamFn = unsafe extern "C" fn(*const c_void);
type AcceptFn = unsafe extern "C" fn(*const c_void, i32, *const f32, i32);
type FinishedFn = unsafe extern "C" fn(*const c_void);
type ReadyFn = unsafe extern "C" fn(*const c_void, *const c_void) -> i32;
type DecodeFn = unsafe extern "C" fn(*const c_void, *const c_void);
type ResultFn = unsafe extern "C" fn(*const c_void, *const c_void) -> *const KeywordResult;
type FreeResultFn = unsafe extern "C" fn(*const KeywordResult);

/// The spotter, loaded and ready.
struct Spotter {
    kws: *const c_void,
    destroy: DestroyFn,
    stream: StreamFn,
    destroy_stream: DestroyStreamFn,
    accept: AcceptFn,
    finished: FinishedFn,
    is_ready: ReadyFn,
    decode: DecodeFn,
    result: ResultFn,
    free_result: FreeResultFn,
    // Dropped after `kws` is destroyed, sherpa-onnx before ONNX Runtime.
    _capi: libloading::Library,
    _ort: libloading::Library,
}

// Only ever used behind a Mutex (`spotter`), one call at a time.
unsafe impl Send for Spotter {}

impl Drop for Spotter {
    fn drop(&mut self) {
        if !self.kws.is_null() {
            unsafe { (self.destroy)(self.kws) };
        }
    }
}

impl Spotter {
    /// Open the library in `runtime` and the model in `model`.
    fn load(runtime: &Path, model: &Path) -> Result<Spotter, String> {
        let (ort_name, capi_name) = crate::kokoro::runtime_files().ok_or("the wake-word spotter isn't available on this kind of computer")?;
        let ort = unsafe { libloading::Library::new(runtime.join(ort_name)) }.map_err(|e| format!("the wake-word spotter's engine wouldn't open ({ort_name}: {e})"))?;
        let capi = unsafe { libloading::Library::new(runtime.join(capi_name)) }.map_err(|e| format!("the wake-word spotter's engine wouldn't open ({capi_name}: {e})"))?;
        fn sym<T: Copy>(lib: &libloading::Library, name: &[u8]) -> Result<T, String> {
            unsafe { lib.get::<T>(name) }.map(|s| *s).map_err(|e| format!("the wake-word spotter's engine is missing a part ({e})"))
        }
        let version: VersionFn = sym(&capi, b"SherpaOnnxGetVersionStr\0")?;
        let v = unsafe {
            let p = version();
            if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
        };
        if v != crate::kokoro::SHERPA_VERSION {
            return Err(format!("the wake-word spotter's engine is version {v}, and Atlas speaks to {} only", crate::kokoro::SHERPA_VERSION));
        }
        let create: CreateFn = sym(&capi, b"SherpaOnnxCreateKeywordSpotter\0")?;
        let destroy: DestroyFn = sym(&capi, b"SherpaOnnxDestroyKeywordSpotter\0")?;
        let stream: StreamFn = sym(&capi, b"SherpaOnnxCreateKeywordStream\0")?;
        let destroy_stream: DestroyStreamFn = sym(&capi, b"SherpaOnnxDestroyOnlineStream\0")?;
        let accept: AcceptFn = sym(&capi, b"SherpaOnnxOnlineStreamAcceptWaveform\0")?;
        let finished: FinishedFn = sym(&capi, b"SherpaOnnxOnlineStreamInputFinished\0")?;
        let is_ready: ReadyFn = sym(&capi, b"SherpaOnnxIsKeywordStreamReady\0")?;
        let decode: DecodeFn = sym(&capi, b"SherpaOnnxDecodeKeywordStream\0")?;
        let result: ResultFn = sym(&capi, b"SherpaOnnxGetKeywordResult\0")?;
        let free_result: FreeResultFn = sym(&capi, b"SherpaOnnxDestroyKeywordResult\0")?;

        let c = |p: PathBuf| CString::new(p.to_string_lossy().into_owned()).map_err(|_| "a path with a zero byte in it".to_string());
        let enc = c(model.join(ENCODER))?;
        let dec = c(model.join(DECODER))?;
        let joi = c(model.join(JOINER))?;
        let tokens = c(model.join("tokens.txt"))?;
        let cpu = CString::new("cpu").unwrap_or_default();
        let words = CString::new(KEYWORDS).unwrap_or_default();

        // All zero is sherpa-onnx's "use the default" for every field not set.
        let mut cfg: SpotterConfig = unsafe { std::mem::zeroed() };
        cfg.feat_config = FeatureConfig { sample_rate: 16000, feature_dim: 80 };
        cfg.model_config.transducer = TransducerConfig { encoder: enc.as_ptr(), decoder: dec.as_ptr(), joiner: joi.as_ptr() };
        cfg.model_config.tokens = tokens.as_ptr();
        cfg.model_config.num_threads = 1;
        cfg.model_config.provider = cpu.as_ptr();
        cfg.max_active_paths = 4;
        cfg.keywords_score = SCORE;
        cfg.keywords_threshold = THRESHOLD;
        cfg.keywords_buf = words.as_ptr();
        cfg.keywords_buf_size = KEYWORDS.len() as i32;
        let kws = unsafe { create(&cfg) };
        if kws.is_null() {
            return Err("the wake-word spotter's files are there but wouldn't load — `atlas get wakeword` fetches them again".into());
        }
        Ok(Spotter { kws, destroy, stream, destroy_stream, accept, finished, is_ready, decode, result, free_result, _capi: capi, _ort: ort })
    }

    /// Is the name anywhere in these 16 kHz samples?
    fn heard(&self, samples: &[f32]) -> bool {
        unsafe {
            let s = (self.stream)(self.kws);
            if s.is_null() {
                return false;
            }
            // A moment of quiet either side: the model looks a little ahead,
            // and a clip cut right at the name's end needs the room to finish
            // it (measured: without the tail, bare "Atlas." was missed).
            let quiet = vec![0f32; 16000 * 3 / 10];
            let tail = vec![0f32; 16000 * 6 / 10];
            for part in [&quiet[..], samples, &tail[..]] {
                (self.accept)(s, 16000, part.as_ptr(), part.len() as i32);
            }
            (self.finished)(s);
            let mut found = false;
            while !found && (self.is_ready)(self.kws, s) != 0 {
                (self.decode)(self.kws, s);
                let r = (self.result)(self.kws, s);
                if !r.is_null() {
                    found = !(*r).keyword.is_null() && *(*r).keyword != 0;
                    (self.free_result)(r);
                }
            }
            (self.destroy_stream)(s);
            found
        }
    }
}

// ---------------------------------------------------------------- one per process

type Loaded = Result<Arc<Mutex<Spotter>>, String>;
static SPOTTER: Mutex<Option<(PathBuf, Loaded)>> = Mutex::new(None);

/// The spotter for the install at `root`, loaded the first time and kept
/// (a failure too, so a missing model is found missing once). `None` when
/// it isn't downloaded.
fn spotter(root: &Path) -> Option<Loaded> {
    let mut slot = SPOTTER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, loaded)) = slot.as_ref() {
        if at == root {
            return Some(loaded.clone());
        }
    }
    let (runtime, model) = ready(root)?;
    let loaded = Spotter::load(&runtime, &model).map(|s| Arc::new(Mutex::new(s)));
    if let Err(e) = &loaded {
        crate::errln!("[atlas] wake-word spotter: {e}");
    }
    *slot = Some((root.to_path_buf(), loaded.clone()));
    Some(loaded)
}

/// Did the spotter hear the name in these 16 kHz samples? `None` when it
/// isn't here or wouldn't load: hearing goes on as before.
pub fn heard_name(root: &Path, samples: &[i16]) -> Option<bool> {
    let s = spotter(root)?.ok()?;
    let f: Vec<f32> = samples.iter().map(|&v| v as f32 / 32768.0).collect();
    let g = s.lock().ok()?;
    Some(g.heard(&f))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_name_it_knows() {
        assert!(knows("Atlas") && knows("hey atlas") && knows("ATLAS!"));
        assert!(!knows("computer") && !knows("jarvis"));
    }

    #[test]
    fn a_misheard_name_at_the_start() {
        assert_eq!(soundalike_at_start("Alice, what time is it?").as_deref(), Some("what time is it?"));
        assert_eq!(soundalike_at_start("He alice.").as_deref(), Some(""));
        assert_eq!(soundalike_at_start("Hey Ellis open Chrome").as_deref(), Some("open Chrome"));
        assert_eq!(soundalike_at_start("I told Alice yesterday"), None);
        assert_eq!(soundalike_at_start("I use Atlassian at work"), None);
        assert_eq!(soundalike_at_start(""), None);
    }

    #[test]
    fn nothing_downloaded_is_nothing_heard() {
        let dir = std::env::temp_dir().join(format!("atlas-kws-none-{}", std::process::id()));
        assert!(ready(&dir).is_none());
        assert_eq!(heard_name(&dir, &[0i16; 1600]), None);
    }
}
