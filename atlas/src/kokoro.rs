//! Kokoro, the better voice, spoken inside Atlas.
//!
//! Until 28 Sep 2026 "Kokoro" in this tree meant a wrapper script you were
//! told to write yourself (`tools/kokoro/speak.cmd`), which nothing
//! installed, so nobody had it. This is Kokoro spoken by Atlas itself,
//! through sherpa-onnx (k2-fsa, Apache-2.0) — its C library, loaded when the
//! first sentence is spoken, not linked into atlas.exe.
//!
//! ## Why loaded at run time rather than linked
//!
//! The official Rust crate (`sherpa-onnx` 1.13.8) links sherpa-onnx and ONNX
//! Runtime statically, from prebuilt archives its build script downloads.
//! Measured here on 28 Sep 2026:
//!
//! - **Linux**: builds and runs (a 40 MB test program).
//! - **Windows, cross-built with MinGW** (`x86_64-pc-windows-gnu`, how every
//!   atlas.exe so far was made): fails. The only Windows archives are built
//!   with Microsoft's compiler (`win-x64-static-MT-Release-lib`, `.lib`
//!   files, 123 MB). MinGW's linker can't find them under their names at
//!   all, and renamed, it can't read the Microsoft linker directives inside
//!   (`/FAILIFMISMATCH`, `/DEFAULTLIB:libcpmt`, Microsoft's own C++ runtime)
//!   and ran out of memory (ld killed, signal 9) before finishing.
//! - **Windows on GitHub Actions** (`windows.yml`) builds with Microsoft's
//!   own toolchain (`x86_64-pc-windows-msvc`), where the static archives
//!   would link — but it would put ~20 MB of ONNX Runtime into every
//!   atlas.exe, download 123 MB on every uncached build, and make the MinGW
//!   build impossible.
//!
//! The C library's *shared* build has none of those problems: its functions
//! are plain C, so an exe from either compiler can call them. The Windows one
//! (`win-x64-shared-MT-Release-lib`, 8 MB) carries its own C runtime and needs
//! nothing installed (its imports are KERNEL32, ADVAPI32, and ONNX Runtime
//! beside it). So Atlas downloads it with the voice, into `tools/kokoro/`,
//! and opens it with `libloading` the first time it speaks in Kokoro.
//!
//! The structures handed across are copied from `sherpa-onnx-sys` 1.13.8
//! (Apache-2.0) and are only right for that version, so the library's own
//! version is checked before anything is passed to it: a different one is
//! refused in words, never called with the wrong layout.
//!
//! ## Licences
//!
//! Kokoro's weights and sherpa-onnx are Apache-2.0; ONNX Runtime is MIT.
//! **The sherpa-onnx library also contains espeak-ng (GPL-3.0)**, which it
//! uses to turn words it has no pronunciation for into sounds. That is the
//! same licence `piper1-gpl` was avoided for. Atlas doesn't link it — it is a
//! separate download, opened at run time — which keeps it out of atlas.exe
//! itself, but it is GPL code running in Atlas's process, and that is said
//! here rather than discovered later. (piper, the voice Atlas already ships,
//! is in the same position: its Windows zip carries `espeak-ng.dll`.)
//!
//! ## Falling back
//!
//! No runtime, no model, a load that fails: Atlas speaks in piper as before,
//! says why once (`note_once`), and doesn't try again on every sentence
//! (`engine` keeps the failure until `forget` — after a download finishes).

use std::collections::VecDeque;
use std::ffi::{c_char, c_void, CStr, CString};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use crate::getpieces::{Lands, Piece};

/// The sherpa-onnx release the structures below were copied from, and the
/// only one Atlas will call.
pub const SHERPA_VERSION: &str = "1.13.8";

/// Where the library lands, install-relative.
pub const RUNTIME_DIR: &str = "tools/kokoro";
/// Where the model lands, install-relative.
pub const MODEL_DIR: &str = "models/kokoro";

/// The voice used when the one in your settings isn't a Kokoro voice (a
/// piper voice left over from before the switch). The warmest of the
/// American voices, and first on the shortlist in `tts::SHORTLIST`.
pub const DEFAULT_VOICE: &str = "af_heart";

/// Kokoro v1.0's voices in speaker-id order, read from the model's own
/// `speaker2id` metadata (kokoro-int8-multi-lang-v1_0, 28 Sep 2026).
pub const VOICES: &[&str] = &[
    "af_alloy", "af_aoede", "af_bella", "af_heart", "af_jessica", "af_kore", "af_nicole", "af_nova",
    "af_river", "af_sarah", "af_sky", "am_adam", "am_echo", "am_eric", "am_fenrir", "am_liam",
    "am_michael", "am_onyx", "am_puck", "am_santa", "bf_alice", "bf_emma", "bf_isabella", "bf_lily",
    "bm_daniel", "bm_fable", "bm_george", "bm_lewis", "ef_dora", "em_alex", "ff_siwis", "hf_alpha",
    "hf_beta", "hm_omega", "hm_psi", "if_sara", "im_nicola", "jf_alpha", "jf_gongitsune", "jf_nezumi",
    "jf_tebukuro", "jm_kumo", "pf_dora", "pm_alex", "pm_santa", "zf_xiaobei", "zf_xiaoni", "zf_xiaoxiao",
    "zf_xiaoyi", "zm_yunjian", "zm_yunxi", "zm_yunxia", "zm_yunyang", "em_santa",
];

/// The English voices (American and British), which are the ones offered:
/// the others speak other languages.
pub fn english_voices() -> Vec<&'static str> {
    VOICES.iter().copied().filter(|v| v.starts_with('a') || v.starts_with('b')).collect()
}

/// A voice's speaker id in the model, if it is one of Kokoro's.
pub fn speaker_id(voice: &str) -> Option<i32> {
    let v = voice.trim().to_lowercase();
    VOICES.iter().position(|x| *x == v).map(|i| i as i32)
}

/// The voice to speak in, and its id: yours when it's a Kokoro voice,
/// `DEFAULT_VOICE` when it isn't.
pub fn voice_or_default(voice: &str) -> (&'static str, i32) {
    match speaker_id(voice) {
        Some(i) => (VOICES[i as usize], i),
        None => (DEFAULT_VOICE, speaker_id(DEFAULT_VOICE).unwrap_or(3)),
    }
}

// ---------------------------------------------------------------- the files

/// The library files for this computer: ONNX Runtime, then sherpa-onnx's C
/// library. `None` where there is no prebuilt library Atlas fetches.
pub fn runtime_files() -> Option<(&'static str, &'static str)> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some(("onnxruntime.dll", "sherpa-onnx-c-api.dll"))
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(("libonnxruntime.so", "libsherpa-onnx-c-api.so"))
    } else {
        None
    }
}

/// The runtime piece for this computer, pinned like every other download.
pub fn runtime_piece() -> Option<Piece> {
    if cfg!(all(windows, target_arch = "x86_64")) {
        Some(Piece {
            name: "the better voice's engine",
            for_what: "speaking in the Kokoro voice",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib.tar.bz2",
            sha256: "b8eedf41bd6d3779218887b48367bb7a3ece5aaa7667f01f69ee823a12b0a9e7",
            bytes: 8_032_957,
            lands: Lands::Zip {
                inside: "sherpa-onnx-v1.13.8-win-x64-shared-MT-Release-lib/lib",
                dir: RUNTIME_DIR,
                key: "tools/kokoro/sherpa-onnx-c-api.dll",
            },
        })
    } else if cfg!(all(target_os = "linux", target_arch = "x86_64")) {
        Some(Piece {
            name: "the better voice's engine",
            for_what: "speaking in the Kokoro voice",
            url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/v1.13.8/sherpa-onnx-v1.13.8-linux-x64-shared-lib.tar.bz2",
            sha256: "3892d184be41027e18165e67f549cd4e4cdd8dcd73ac5579e97afd55e14e30b6",
            bytes: 9_816_899,
            lands: Lands::Zip {
                inside: "sherpa-onnx-v1.13.8-linux-x64-shared-lib/lib",
                dir: RUNTIME_DIR,
                key: "tools/kokoro/libsherpa-onnx-c-api.so",
            },
        })
    } else {
        None
    }
}

/// Kokoro v1.0, 8-bit (132 MB to download, 183 MB unpacked): all 54 voices,
/// including every one on `tts::SHORTLIST`. The English-only v0.19 is
/// smaller (103 MB) but has 11 voices and none of the shortlist.
pub fn model_piece() -> Piece {
    Piece {
        name: "the better voice",
        for_what: "speaking in the Kokoro voice",
        url: "https://github.com/k2-fsa/sherpa-onnx/releases/download/tts-models/kokoro-int8-multi-lang-v1_0.tar.bz2",
        sha256: "4c3052abaa60943a341f193888cf6abd68787dae6ab8ae5c925a706caa247e4e",
        bytes: 132_303_094,
        lands: Lands::Zip { inside: "kokoro-int8-multi-lang-v1_0", dir: MODEL_DIR, key: "models/kokoro/model.int8.onnx" },
    }
}

/// Everything Kokoro needs, in the order it's fetched. Empty on a computer
/// with no prebuilt library.
pub fn pieces() -> Vec<Piece> {
    match runtime_piece() {
        Some(r) => vec![r, model_piece()],
        None => Vec::new(),
    }
}

/// Megabytes to download for all of it.
pub fn download_mb() -> u64 {
    pieces().iter().map(|p| p.megabytes()).sum()
}

/// What's missing, if anything.
#[derive(Debug, Clone, PartialEq)]
pub enum Missing {
    /// No prebuilt library for this kind of computer.
    NotHere,
    /// The library hasn't been downloaded.
    Runtime,
    /// The voice itself hasn't been downloaded.
    Model,
}

impl Missing {
    pub fn plain(&self) -> String {
        match self {
            Missing::NotHere => "the Kokoro voice isn't available on this kind of computer yet".into(),
            Missing::Runtime | Missing::Model => format!(
                "the Kokoro voice isn't downloaded yet ({} MB, from Sound & voice)",
                download_mb()
            ),
        }
    }
}

/// The folders, once both are there.
#[derive(Debug, Clone, PartialEq)]
pub struct Ready {
    pub runtime: PathBuf,
    pub model: PathBuf,
}

/// Is Kokoro here, under the install at `root`?
pub fn check(root: &Path) -> Result<Ready, Missing> {
    let (ort, capi) = runtime_files().ok_or(Missing::NotHere)?;
    let runtime = root.join(RUNTIME_DIR);
    if !runtime.join(ort).is_file() || !runtime.join(capi).is_file() {
        return Err(Missing::Runtime);
    }
    let model = root.join(MODEL_DIR);
    let needed = ["model.int8.onnx", "voices.bin", "tokens.txt", "lexicon-us-en.txt"];
    if needed.iter().any(|f| !model.join(f).is_file()) || !model.join("espeak-ng-data").is_dir() {
        return Err(Missing::Model);
    }
    Ok(Ready { runtime, model })
}

// ---------------------------------------------------------------- the library

// Copied from sherpa-onnx-sys 1.13.8 `src/tts.rs` (Apache-2.0). Only right
// for that version; `Kokoro::load` checks the version before using them.
#[repr(C)]
#[derive(Clone, Copy)]
struct VitsConfig {
    model: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: f32,
    noise_scale_w: f32,
    length_scale: f32,
    dict_dir: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct MatchaConfig {
    acoustic_model: *const c_char,
    vocoder: *const c_char,
    lexicon: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    noise_scale: f32,
    length_scale: f32,
    dict_dir: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KokoroConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: f32,
    dict_dir: *const c_char,
    lexicon: *const c_char,
    lang: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct KittenConfig {
    model: *const c_char,
    voices: *const c_char,
    tokens: *const c_char,
    data_dir: *const c_char,
    length_scale: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ZipvoiceConfig {
    tokens: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    vocoder: *const c_char,
    data_dir: *const c_char,
    lexicon: *const c_char,
    feat_scale: f32,
    t_shift: f32,
    target_rms: f32,
    guidance_scale: f32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PocketConfig {
    lm_flow: *const c_char,
    lm_main: *const c_char,
    encoder: *const c_char,
    decoder: *const c_char,
    text_conditioner: *const c_char,
    vocab_json: *const c_char,
    token_scores_json: *const c_char,
    voice_embedding_cache_capacity: i32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct SupertonicConfig {
    duration_predictor: *const c_char,
    text_encoder: *const c_char,
    vector_estimator: *const c_char,
    vocoder: *const c_char,
    tts_json: *const c_char,
    unicode_indexer: *const c_char,
    voice_style: *const c_char,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct ModelConfig {
    vits: VitsConfig,
    num_threads: i32,
    debug: i32,
    provider: *const c_char,
    matcha: MatchaConfig,
    kokoro: KokoroConfig,
    kitten: KittenConfig,
    zipvoice: ZipvoiceConfig,
    pocket: PocketConfig,
    supertonic: SupertonicConfig,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TtsConfig {
    model: ModelConfig,
    rule_fsts: *const c_char,
    max_num_sentences: i32,
    rule_fars: *const c_char,
    silence_scale: f32,
}

#[repr(C)]
struct GeneratedAudio {
    samples: *const f32,
    n: i32,
    sample_rate: i32,
}

type VersionFn = unsafe extern "C" fn() -> *const c_char;
type CreateFn = unsafe extern "C" fn(*const TtsConfig) -> *const c_void;
type DestroyFn = unsafe extern "C" fn(*const c_void);
type RateFn = unsafe extern "C" fn(*const c_void) -> i32;
type GenerateFn = unsafe extern "C" fn(*const c_void, *const c_char, i32, f32) -> *const GeneratedAudio;
type FreeAudioFn = unsafe extern "C" fn(*const GeneratedAudio);

/// Kokoro, loaded and ready to speak.
pub struct Kokoro {
    tts: *const c_void,
    generate: GenerateFn,
    free_audio: FreeAudioFn,
    destroy: DestroyFn,
    rate: u32,
    // Dropped after `tts` is destroyed (in `Drop`), sherpa-onnx before ONNX
    // Runtime, which it uses.
    _capi: libloading::Library,
    _ort: libloading::Library,
}

// The engine is only ever used behind a Mutex (`engine`), one call at a time.
unsafe impl Send for Kokoro {}

impl Drop for Kokoro {
    fn drop(&mut self) {
        if !self.tts.is_null() {
            unsafe { (self.destroy)(self.tts) };
        }
    }
}

impl Kokoro {
    /// Open the library in `runtime` and load the model in `model`, using
    /// `threads` processor threads for each sentence.
    pub fn load(runtime: &Path, model: &Path, threads: i32) -> Result<Kokoro, String> {
        let (ort_name, capi_name) = runtime_files().ok_or_else(|| Missing::NotHere.plain())?;
        // ONNX Runtime first, by its full path. On Windows 11 there is an
        // older `onnxruntime.dll` in System32 (Windows' own), and a DLL's
        // imports are looked for beside atlas.exe and in System32 — not beside
        // the DLL. Already loaded by name, ours is the one it gets.
        let ort = unsafe { libloading::Library::new(runtime.join(ort_name)) }
            .map_err(|e| format!("the Kokoro voice's engine wouldn't open ({ort_name}: {e})"))?;
        let capi = unsafe { libloading::Library::new(runtime.join(capi_name)) }
            .map_err(|e| format!("the Kokoro voice's engine wouldn't open ({capi_name}: {e})"))?;

        let version = unsafe {
            let f = *capi
                .get::<VersionFn>(b"SherpaOnnxGetVersionStr\0")
                .map_err(|e| format!("the Kokoro voice's engine is missing a part ({e})"))?;
            let p = f();
            if p.is_null() { String::new() } else { CStr::from_ptr(p).to_string_lossy().into_owned() }
        };
        if version != SHERPA_VERSION {
            return Err(format!(
                "the Kokoro voice's engine is version {version}, and Atlas speaks to {SHERPA_VERSION} only — \
                 download it again from Sound & voice"
            ));
        }
        fn sym<T: Copy>(lib: &libloading::Library, name: &[u8]) -> Result<T, String> {
            unsafe { lib.get::<T>(name) }
                .map(|s| *s)
                .map_err(|e| format!("the Kokoro voice's engine is missing a part ({e})"))
        }
        let create: CreateFn = sym(&capi, b"SherpaOnnxCreateOfflineTts\0")?;
        let destroy: DestroyFn = sym(&capi, b"SherpaOnnxDestroyOfflineTts\0")?;
        let rate: RateFn = sym(&capi, b"SherpaOnnxOfflineTtsSampleRate\0")?;
        let generate: GenerateFn = sym(&capi, b"SherpaOnnxOfflineTtsGenerate\0")?;
        let free_audio: FreeAudioFn = sym(&capi, b"SherpaOnnxDestroyOfflineTtsGeneratedAudio\0")?;

        let c = |p: PathBuf| CString::new(p.to_string_lossy().into_owned()).map_err(|_| "a path with a zero byte in it".to_string());
        let m = c(model.join("model.int8.onnx"))?;
        let voices = c(model.join("voices.bin"))?;
        let tokens = c(model.join("tokens.txt"))?;
        let data = c(model.join("espeak-ng-data"))?;
        let dict = c(model.join("dict"))?;
        let lexicon = CString::new(format!(
            "{},{}",
            model.join("lexicon-us-en.txt").to_string_lossy(),
            model.join("lexicon-zh.txt").to_string_lossy()
        ))
        .map_err(|_| "a path with a zero byte in it".to_string())?;
        let cpu = CString::new("cpu").unwrap_or_default();

        // All zero is sherpa-onnx's own "use the default" for every field not
        // set here (its C side reads a null string as "" and a zero number as
        // the default), which is also how its Rust wrapper starts one.
        let mut cfg: TtsConfig = unsafe { std::mem::zeroed() };
        cfg.model.kokoro.model = m.as_ptr();
        cfg.model.kokoro.voices = voices.as_ptr();
        cfg.model.kokoro.tokens = tokens.as_ptr();
        cfg.model.kokoro.data_dir = data.as_ptr();
        cfg.model.kokoro.dict_dir = dict.as_ptr();
        cfg.model.kokoro.lexicon = lexicon.as_ptr();
        cfg.model.kokoro.length_scale = 1.0;
        cfg.model.num_threads = threads.max(1);
        cfg.model.provider = cpu.as_ptr();
        cfg.max_num_sentences = 1;
        cfg.silence_scale = 0.2;

        let tts = unsafe { create(&cfg) };
        if tts.is_null() {
            return Err("the Kokoro voice's files are there but wouldn't load — download it again from Sound & voice".into());
        }
        let r = unsafe { rate(tts) };
        Ok(Kokoro { tts, generate, free_audio, destroy, rate: r.max(1) as u32, _capi: capi, _ort: ort })
    }

    /// Samples per second of what it says (24,000 for Kokoro).
    pub fn sample_rate(&self) -> u32 {
        self.rate
    }

    /// Say `text` in voice `sid` at `speed` (1 is normal, larger is faster):
    /// the samples, -1..1.
    pub fn synth(&self, text: &str, sid: i32, speed: f32) -> Result<Vec<f32>, String> {
        let t = CString::new(text.replace('\0', " ")).map_err(|_| "unspeakable text".to_string())?;
        let speed = if speed.is_finite() && speed > 0.0 { speed } else { 1.0 };
        let audio = unsafe { (self.generate)(self.tts, t.as_ptr(), sid, speed) };
        if audio.is_null() {
            return Err("the Kokoro voice produced nothing".into());
        }
        let out = unsafe {
            let a = &*audio;
            let v = if a.samples.is_null() || a.n <= 0 {
                Vec::new()
            } else {
                std::slice::from_raw_parts(a.samples, a.n as usize).to_vec()
            };
            (self.free_audio)(audio);
            v
        };
        Ok(out)
    }

    /// The same, as a 16-bit WAV file's bytes — what the player and the
    /// moving mark (`speaking`) both read.
    pub fn synth_wav(&self, text: &str, sid: i32, speed: f32) -> Result<Vec<u8>, String> {
        let s = self.synth(text, sid, speed)?;
        Ok(to_wav(&s, self.rate))
    }
}

/// Float samples to a 16-bit mono WAV.
pub fn to_wav(samples: &[f32], rate: u32) -> Vec<u8> {
    let pcm: Vec<i16> = samples.iter().map(|s| (s.clamp(-1.0, 1.0) * 32767.0).round() as i16).collect();
    crate::audio::wav_bytes(&pcm, rate)
}

/// How many processor threads each sentence gets: half the machine's, at
/// least two (or one on a single-core machine) and at most four, so the
/// model server keeps the rest. Measured 28 Sep 2026 on two shared 2.1 GHz
/// Xeon cores: two threads made speech at 1.25-1.31x its own length, one
/// thread at 1.67x.
pub fn thread_count() -> i32 {
    let n = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(2);
    (n / 2).clamp(2, 4).min(n) as i32
}

// ---------------------------------------------------------------- one per process

type Loaded = Result<Arc<Mutex<Kokoro>>, String>;
static ENGINE: Mutex<Option<(PathBuf, Loaded)>> = Mutex::new(None);

/// Kokoro for the install at `root`, loaded the first time and kept. A
/// failure is kept too, so a missing voice is found missing once, not on
/// every sentence; `forget` clears it.
pub fn engine(root: &Path) -> Loaded {
    let mut slot = ENGINE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((at, loaded)) = slot.as_ref() {
        if at == root {
            return loaded.clone();
        }
    }
    let loaded = match check(root) {
        Err(m) => Err(m.plain()),
        Ok(r) => Kokoro::load(&r.runtime, &r.model, thread_count()).map(|k| Arc::new(Mutex::new(k))),
    };
    *slot = Some((root.to_path_buf(), loaded.clone()));
    loaded
}

/// Try again next time: after a download finishes, or files were put back.
pub fn forget() {
    *ENGINE.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    *NOTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
}

static NOTED: Mutex<Option<String>> = Mutex::new(None);

/// The one thing to say about falling back to the usual voice — the first
/// time, and again only if the reason changes. `None` otherwise.
pub fn note_once(why: &str) -> Option<String> {
    let mut n = NOTED.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if n.as_deref() == Some(why) {
        return None;
    }
    *n = Some(why.to_string());
    Some(format!("Speaking in the usual voice: {why}."))
}

/// Why Atlas last fell back, if it has, for the Sound page.
pub fn last_note() -> Option<String> {
    NOTED.lock().or_else(crate::crash::unpoison).ok().and_then(|n| n.clone())
}

// ---------------------------------------------------------------- one sentence ahead

/// Makes a sentence's audio.
pub type Synth = Arc<dyn Fn(&str) -> Result<Vec<u8>, String> + Send + Sync>;

struct Job {
    id: u64,
    text: String,
    started: bool,
    out: Option<Result<Vec<u8>, String>>,
}

#[derive(Default)]
struct State {
    jobs: VecDeque<Job>,
    next_id: u64,
    worker: bool,
    synth: Option<Synth>,
}

/// The next sentence, made while this one plays.
///
/// A reply is handed over whole (`prepare`) before its first sentence is
/// said; a thread of its own then makes the audio for the sentence at the
/// front of the queue and no further. When a sentence is taken to be played
/// (`take`), the next one comes to the front and is made while it plays —
/// exactly one ahead, so a reply you interrupt wastes at most one sentence
/// of work, and the model server isn't starved for the rest of a long reply.
#[derive(Clone, Default)]
pub struct Ahead {
    inner: Arc<(Mutex<State>, Condvar)>,
}

/// How long the thread waits for a sentence to be taken before it stops
/// (it starts again with the next reply). A reply that was cut off never
/// takes the rest.
const IDLE: Duration = Duration::from_secs(30);

impl Ahead {
    /// Queue `sentences`, in order, replacing whatever was left of an
    /// earlier reply.
    pub fn prepare(&self, sentences: Vec<String>, synth: Synth) {
        let (m, cv) = &*self.inner;
        let mut st = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        st.jobs.clear();
        for text in sentences.into_iter().filter(|s| !s.trim().is_empty()) {
            st.next_id += 1;
            let id = st.next_id;
            st.jobs.push_back(Job { id, text, started: false, out: None });
        }
        st.synth = Some(synth);
        cv.notify_all();
        self.keep_working(&mut st);
    }

    /// Queue `sentences` after what's already queued: more of the same reply,
    /// written by the model while the start of it was being said.
    pub fn extend(&self, sentences: Vec<String>, synth: Synth) {
        let (m, cv) = &*self.inner;
        let mut st = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        for text in sentences.into_iter().filter(|s| !s.trim().is_empty()) {
            st.next_id += 1;
            let id = st.next_id;
            st.jobs.push_back(Job { id, text, started: false, out: None });
        }
        st.synth = Some(synth);
        cv.notify_all();
        self.keep_working(&mut st);
    }

    /// A thread making audio whenever there is a sentence not yet started
    /// (it stops when idle, and a later `take` may need it again).
    fn keep_working(&self, st: &mut State) {
        if !st.worker && st.jobs.iter().any(|j| !j.started) {
            st.worker = true;
            let inner = self.inner.clone();
            std::thread::spawn(move || work(inner));
        }
    }

    /// Is `text` queued?
    pub fn has(&self, text: &str) -> bool {
        let st = self.inner.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        st.jobs.iter().any(|j| j.text == text)
    }

    /// The audio for `text`, waiting while it's being made. `None` when it
    /// isn't queued (or the queue was replaced while waiting): make it
    /// yourself. Sentences queued before it are dropped — they were skipped.
    pub fn take(&self, text: &str) -> Option<Result<Vec<u8>, String>> {
        let (m, cv) = &*self.inner;
        let mut st = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let pos = st.jobs.iter().position(|j| j.text == text)?;
        st.jobs.drain(..pos);
        let id = st.jobs.front().map(|j| j.id)?;
        cv.notify_all();
        loop {
            self.keep_working(&mut st);
            match st.jobs.front() {
                Some(j) if j.id == id && j.out.is_some() => {
                    let out = st.jobs.pop_front().and_then(|j| j.out);
                    // The next one is at the front now: go.
                    self.keep_working(&mut st);
                    cv.notify_all();
                    return out;
                }
                Some(j) if j.id == id => {}
                _ => return None,
            }
            let (next, _) = cv.wait_timeout(st, Duration::from_millis(500)).unwrap_or_else(std::sync::PoisonError::into_inner);
            st = next;
        }
    }
}

fn work(inner: Arc<(Mutex<State>, Condvar)>) {
    let (m, cv) = &*inner;
    loop {
        let (id, text, synth) = {
            let mut st = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            let mut waited = Duration::ZERO;
            loop {
                if !st.jobs.iter().any(|j| !j.started) {
                    st.worker = false;
                    return;
                }
                // Only the front: one sentence ahead of what is playing.
                if let Some(j) = st.jobs.front() {
                    if !j.started {
                        break;
                    }
                }
                if waited >= IDLE {
                    st.worker = false;
                    return;
                }
                let (next, _) = cv.wait_timeout(st, Duration::from_millis(500)).unwrap_or_else(std::sync::PoisonError::into_inner);
                st = next;
                waited += Duration::from_millis(500);
            }
            let Some(synth) = st.synth.clone() else {
                st.worker = false;
                return;
            };
            let j = st.jobs.front_mut().expect("checked above");
            j.started = true;
            (j.id, j.text.clone(), synth)
        };
        let out = synth(&text);
        let mut st = m.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(j) = st.jobs.iter_mut().find(|j| j.id == id) {
            j.out = Some(out);
        }
        cv.notify_all();
    }
}

// ---------------------------------------------------------------- getting it

/// The name its download goes by on the Sound page (`voicepick::Downloads`).
pub const DOWNLOAD_ID: &str = "kokoro";

/// Fetch the library and the model into the install at `root`, reporting to
/// `downloads`, through the same pinned, checked, resumable fetch as every
/// other piece (`getpieces::fetch`: a download slower than 10 kB/s for a
/// minute is retried, five times, and carries on from what arrived). Runs on
/// its own thread: 140 MB would hold up everything else.
pub fn fetch_all(root: &Path, downloads: &crate::voicepick::Downloads, tools: &crate::getpieces::Tools) {
    let pieces = pieces();
    if pieces.is_empty() {
        downloads.finish(DOWNLOAD_ID, Err(Missing::NotHere.plain()));
        return;
    }
    let of: u64 = pieces.iter().map(|p| p.bytes).sum();
    let mut before = 0;
    for p in &pieces {
        let report = |got: u64, _: u64| downloads.progress(DOWNLOAD_ID, before + got, of);
        if let Err(why) = crate::getpieces::fetch(p, root, tools, &report) {
            downloads.finish(DOWNLOAD_ID, Err(why));
            return;
        }
        before += p.bytes;
    }
    // Found missing before; found here from the next sentence on.
    forget();
    downloads.finish(DOWNLOAD_ID, Ok(()));
}

/// "af_heart" as a name: "Heart".
pub fn display_name(voice: &str) -> String {
    let n = voice.split('_').nth(1).unwrap_or(voice);
    let mut c = n.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// "American" / "British", from the voice's first letter.
pub fn accent(voice: &str) -> &'static str {
    match voice.chars().next() {
        Some('a') => "American",
        Some('b') => "British",
        _ => "",
    }
}

// ---------------------------------------------------------------- stock lines

/// Lines Atlas says often and always the same way, made once when Kokoro is
/// loaded so they play at once (Phase 0.8, 1 Oct 2026: Kokoro took about
/// 1.3x as long to make a sentence as to play it, so even "Yes?" waited on
/// synthesis). Each is split as `speak` splits, and kept per sentence.
pub const STOCK_LINES: &[&str] = &[
    "Yes?",
    "Looking now.",
    "Anytime.",
    "Sorry about that.",
    "One moment.",
    "Done.",
    "Got it.",
    crate::daemon::STILL_LOADING_WORDS,
];

/// The made lines, and what they were made with (voice, speed, volume): a
/// change of voice makes them stale, and a stale one is never played.
static STOCK: Mutex<Option<(String, std::collections::HashMap<String, Vec<u8>>)>> = Mutex::new(None);

fn stock_key(sentence: &str) -> String {
    sentence.trim().to_lowercase()
}

/// Make `STOCK_LINES` with `synth`, on a thread of its own. `made_for`
/// names the settings, so a later change is seen.
pub fn prepare_stock(synth: Synth, made_for: String) {
    std::thread::Builder::new()
        .name("atlas-stock-lines".into())
        .spawn(move || {
            let mut made = std::collections::HashMap::new();
            for line in STOCK_LINES {
                for s in crate::speech::split(line) {
                    if let Ok(wav) = synth(&s) {
                        made.insert(stock_key(&s), wav);
                    }
                }
            }
            if let Ok(mut g) = STOCK.lock().or_else(crate::crash::unpoison) {
                *g = Some((made_for, made));
            }
        })
        .ok();
}

/// A stock sentence already made with these settings, if it is one.
pub fn stock(sentence: &str, made_for: &str) -> Option<Vec<u8>> {
    let g = STOCK.lock().or_else(crate::crash::unpoison).ok()?;
    let (with, made) = g.as_ref()?;
    (with == made_for).then(|| made.get(&stock_key(sentence)).cloned()).flatten()
}

/// What the stock lines are made with: voice, speed and volume.
pub fn made_for(voice: &str, speed: f32, volume: u8) -> String {
    format!("{voice}|{speed:.3}|{volume}")
}
