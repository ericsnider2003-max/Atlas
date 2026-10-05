//! The phone's own language model (OPEN_GAPS P.7, D6).
//!
//! On the laptop Atlas runs `llama-server` as a separate program. A phone
//! can't: iOS lets an app start no other program at all, and Android offers
//! none to start. So on the phone the model runs *inside* Atlas -- llama.cpp
//! linked in as a library (`llama-cpp-2`), Metal on iPhone and iPad, the CPU
//! on Android -- behind the same `brain::Llm` every other model is behind
//! (`PhoneLlm`). The prompt is wrapped from the file's own chat template, as
//! on the laptop (`models::Template`). A connection you've set yourself
//! (`tools.llm`, say your laptop's model over Tailscale) stays as the
//! fallback, answered in-process (`tools::curl_in_process`).
//!
//! Which model: the laptop's 4B is too big for a phone's memory. Two pinned
//! files, chosen by how much memory the phone has (`choose`):
//! - Qwen3 1.7B (Q8_0, 1.8 GB) on phones with 8 GB or more;
//! - Qwen3 0.6B (Q8_0, 0.64 GB) on the rest.
//!
//! Both are Qwen's own GGUF files, pinned by SHA-256 (the Hugging Face LFS
//! object id, checked by downloading both on 27 Sep 2026). The phone fetches
//! its model itself (`fetch`), only when asked -- 0.6 to 1.8 GB is not
//! something to start on a phone's data plan unasked.
//!
//! The engine is compiled only with the `phone-llm` feature, which the phone
//! builds turn on; everything else here (the choice, the download, the pins)
//! is ordinary code and tested on any machine.

use std::path::{Path, PathBuf};

/// One model file the phone can have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhoneModel {
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
    /// The least memory a phone should have to run it well.
    pub min_ram: u64,
}

const GB: u64 = 1024 * 1024 * 1024;

/// The phone's models, largest first.
pub const MODELS: [PhoneModel; 2] = [
    PhoneModel {
        name: "Qwen3 1.7B",
        file: "Qwen3-1.7B-Q8_0.gguf",
        url: "https://huggingface.co/Qwen/Qwen3-1.7B-GGUF/resolve/main/Qwen3-1.7B-Q8_0.gguf",
        sha256: "061b54daade076b5d3362dac252678d17da8c68f07560be70818cace6590cb1a",
        bytes: 1_834_426_016,
        // 8 GB phones report a little under 8.
        min_ram: 7 * GB,
    },
    PhoneModel {
        name: "Qwen3 0.6B",
        file: "Qwen3-0.6B-Q8_0.gguf",
        url: "https://huggingface.co/Qwen/Qwen3-0.6B-GGUF/resolve/main/Qwen3-0.6B-Q8_0.gguf",
        sha256: "9465e63a22add5354d9bb4b99e90117043c7124007664907259bd16d043bb031",
        bytes: 639_446_688,
        min_ram: 0,
    },
];

/// The model that suits a phone with this much memory.
fn choose(ram_bytes: Option<u64>) -> &'static PhoneModel {
    let ram = ram_bytes.unwrap_or(0);
    MODELS.iter().find(|m| ram >= m.min_ram).unwrap_or(&MODELS[MODELS.len() - 1])
}

/// How much memory this device has, if it says.
fn device_ram() -> Option<u64> {
    #[cfg(any(target_os = "ios", target_os = "macos"))]
    {
        let mut v: u64 = 0;
        let mut len = std::mem::size_of::<u64>();
        let name = b"hw.memsize\0";
        // SAFETY: a read-only sysctl into a u64 of the size given.
        let r = unsafe { libc::sysctlbyname(name.as_ptr().cast(), (&mut v as *mut u64).cast(), &mut len, std::ptr::null_mut(), 0) };
        return (r == 0 && v > 0).then_some(v);
    }
    #[cfg(all(unix, not(any(target_os = "ios", target_os = "macos"))))]
    {
        // SAFETY: sysconf only reads.
        let (pages, size) = unsafe { (libc::sysconf(libc::_SC_PHYS_PAGES), libc::sysconf(libc::_SC_PAGESIZE)) };
        return (pages > 0 && size > 0).then(|| pages as u64 * size as u64);
    }
    #[allow(unreachable_code)]
    None
}

/// The model file this phone has, if any: the one that suits it, else the
/// other if that's what's there.
pub fn present(models_dir: &Path) -> Option<(PathBuf, &'static PhoneModel)> {
    let first = choose(device_ram());
    std::iter::once(first)
        .chain(MODELS.iter().filter(|m| *m != first))
        .map(|m| (models_dir.join(m.file), m))
        .find(|(p, m)| std::fs::metadata(p).map(|md| md.len() == m.bytes).unwrap_or(false))
}

/// Fetch the model into `models_dir`, resuming a partial download, and keep
/// it only if its SHA-256 is the pinned one. `get(url, from_byte)` does the
/// fetching (`https_range`, or a stand-in in a test) and returns the bytes
/// from that offset in pieces through `write`.
fn fetch(
    models_dir: &Path,
    m: &PhoneModel,
    get: &mut dyn FnMut(&str, u64, &mut dyn FnMut(&[u8]) -> std::io::Result<()>) -> Result<(), String>,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<PathBuf, String> {
    use std::io::Write;
    std::fs::create_dir_all(models_dir).map_err(|e| format!("couldn't make the models folder: {e}"))?;
    let done = models_dir.join(m.file);
    if std::fs::metadata(&done).map(|md| md.len() == m.bytes).unwrap_or(false) {
        return Ok(done);
    }
    let part = models_dir.join(format!("{}.part", m.file));
    let have = std::fs::metadata(&part).map(|md| md.len()).unwrap_or(0).min(m.bytes);
    let mut f = std::fs::OpenOptions::new().create(true).append(true).open(&part).map_err(|e| e.to_string())?;
    f.set_len(have).map_err(|e| e.to_string())?;
    let mut so_far = have;
    progress(so_far, m.bytes);
    if have < m.bytes {
        get(m.url, have, &mut |chunk: &[u8]| {
            if so_far + chunk.len() as u64 > m.bytes {
                return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "more than the model's size arrived"));
            }
            f.write_all(chunk)?;
            so_far += chunk.len() as u64;
            progress(so_far, m.bytes);
            Ok(())
        })?;
    }
    drop(f);
    if so_far != m.bytes {
        return Err(format!("{} stopped partway ({} of {} MB); asking again carries on from there", m.name, so_far / 1_000_000, m.bytes / 1_000_000));
    }
    let got = crate::digest::sha256_file_hex(&part).map_err(|e| e.to_string())?;
    if !got.eq_ignore_ascii_case(m.sha256) {
        let _ = std::fs::remove_file(&part);
        return Err(format!("the {} file that arrived isn't the one it should be, so I threw it away", m.name));
    }
    std::fs::rename(&part, &done).map_err(|e| e.to_string())?;
    Ok(done)
}

/// An HTTPS GET from `from_byte` on, following redirects (Hugging Face sends
/// a file to its storage host), handing the body to `write` as it arrives.
/// In-process -- a phone has no `curl` to run.
pub fn https_range(url: &str, from_byte: u64, write: &mut dyn FnMut(&[u8]) -> std::io::Result<()>) -> Result<(), String> {
    use std::io::{BufRead, BufReader, Read, Write};
    let mut url = url.to_string();
    for _ in 0..6 {
        let rest = url.strip_prefix("https://").ok_or("only https addresses")?;
        let (host, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
        let path = if path.is_empty() { "/" } else { path };
        let tcp = std::net::TcpStream::connect((host, 443)).map_err(|e| format!("couldn't reach {host}: {e}"))?;
        tcp.set_read_timeout(Some(std::time::Duration::from_secs(60))).ok();
        let tls = native_tls::TlsConnector::new().map_err(|e| e.to_string())?;
        let mut s = tls.connect(host, tcp).map_err(|e| format!("secure connection to {host}: {e}"))?;
        let range = if from_byte > 0 { format!("Range: bytes={from_byte}-\r\n") } else { String::new() };
        write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nUser-Agent: Atlas\r\n{range}Connection: close\r\n\r\n").map_err(|e| e.to_string())?;
        let mut r = BufReader::new(s);
        let mut status = String::new();
        r.read_line(&mut status).map_err(|e| e.to_string())?;
        let code: u16 = status.split_whitespace().nth(1).and_then(|c| c.parse().ok()).unwrap_or(0);
        let (mut location, mut chunked) = (None, false);
        loop {
            let mut line = String::new();
            if r.read_line(&mut line).map_err(|e| e.to_string())? == 0 || line == "\r\n" {
                break;
            }
            let lower = line.to_ascii_lowercase();
            if let Some(v) = lower.strip_prefix("location:") {
                location = Some(line[line.len() - v.len()..].trim().to_string());
            }
            if lower.starts_with("transfer-encoding:") && lower.contains("chunked") {
                chunked = true;
            }
        }
        match code {
            301 | 302 | 303 | 307 | 308 => {
                let to = location.ok_or("a redirect with nowhere to go")?;
                url = if to.starts_with("https://") { to } else { format!("https://{host}{to}") };
                continue;
            }
            // From the start (200) only when asked from the start; a server
            // that ignored the range would repeat what's already here.
            200 if from_byte == 0 => {}
            206 => {}
            416 => return Ok(()), // nothing left to send
            _ => return Err(format!("{host} answered {code}")),
        }
        if chunked {
            return Err(format!("{host} sent the file in a form this doesn't read (chunked)"));
        }
        let mut buf = vec![0u8; 256 * 1024];
        loop {
            let n = r.read(&mut buf).map_err(|e| format!("the download stopped: {e}"))?;
            if n == 0 {
                return Ok(());
            }
            write(&buf[..n]).map_err(|e| e.to_string())?;
        }
    }
    Err("too many redirects".into())
}

// ---- The engine: llama.cpp, in-process --------------------------------------

/// What one request to the model asks for -- the same fields Atlas sends
/// `llama-server` (`models::completion_body`).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Asked {
    pub prompt: String,
    #[serde(default = "default_n_predict")]
    pub n_predict: i32,
    #[serde(default)]
    pub stop: Vec<String>,
    #[serde(default)]
    pub temperature: Option<f32>,
}

fn default_n_predict() -> i32 {
    512
}

/// Cut `text` at the first stop string, if one has appeared. Returns whether
/// it was cut.
pub fn cut_at_stop(text: &mut String, stops: &[String]) -> bool {
    let at = stops.iter().filter(|s| !s.is_empty()).filter_map(|s| text.find(s.as_str())).min();
    match at {
        Some(i) => {
            text.truncate(i);
            true
        }
        None => false,
    }
}

/// The phone's models are Qwen3, which thinks out loud first by default
/// (`<think>…</think>`): hundreds of tokens before the answer, seconds each on
/// a phone, and words Atlas would then say. Its own template's "no thinking"
/// switch is an empty thought already written at the start of the answer;
/// this is that, for a prompt the model named `model_name` will get.
pub fn no_thinking(prompt: String, model_name: &str) -> String {
    if model_name.to_ascii_lowercase().starts_with("qwen3") && prompt.ends_with("<|im_start|>assistant\n") {
        prompt + "<think>\n\n</think>\n\n"
    } else {
        prompt
    }
}

/// Any thinking a model did anyway, taken out of what it says.
pub fn without_thinking(text: &str) -> String {
    let mut out = text.to_string();
    while let Some(a) = out.find("<think>") {
        match out[a..].find("</think>") {
            Some(b) => out.replace_range(a..a + b + "</think>".len(), ""),
            None => out.truncate(a),
        }
    }
    out.trim().to_string()
}

/// Should the prompt get a beginning-of-text token added? Not when it already
/// starts with the model's own control tokens (the chat templates Atlas
/// renders do: `<|im_start|>`, `<|begin_of_text|>`, `<s>`).
pub fn add_bos_for(prompt: &str) -> bool {
    let p = prompt.trim_start();
    !(p.starts_with("<|") || p.starts_with("<s>") || p.starts_with("<bos>"))
}

/// How the phone's model download is going, for "how's the model download".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Download {
    pub name: String,
    pub have: u64,
    pub of: u64,
    /// Finished, or why it stopped.
    pub finished: Option<Result<(), String>>,
}

static DOWNLOAD: std::sync::Mutex<Option<Download>> = std::sync::Mutex::new(None);

/// The download as it stands, if one has been started.
pub fn download_state() -> Option<Download> {
    DOWNLOAD.lock().or_else(crate::crash::unpoison).ok().and_then(|d| d.clone())
}

/// Whether the phone should start fetching its own model now, by itself
/// (Eric, 27 Sep 2026: nobody should have to know to say "get your own
/// model"). Only on wifi or another network that isn't metered, only when
/// there's no model yet and nothing is downloading, and after a stopped
/// download no sooner than ten minutes after the last try.
pub fn fetch_by_itself(unmetered: bool, have_one: bool, state: Option<&Download>, now: u64, last_try: u64) -> bool {
    if !unmetered || have_one {
        return false;
    }
    match state.map(|d| &d.finished) {
        None => true,
        Some(None) | Some(Some(Ok(()))) => false,
        Some(Some(Err(_))) => now.saturating_sub(last_try) >= 600,
    }
}

/// "How's the model download", in a sentence.
pub fn download_said(d: Option<&Download>, attached: Option<&str>) -> String {
    match (d, attached) {
        (_, Some(name)) => format!("This phone has its own model, {name}, and I'm using it."),
        (None, None) => "This phone hasn't got its own model yet. Say \"get your own model\" and I'll fetch one (0.6 to 1.8 GB, on Wi-Fi only), then answer without the internet.".into(),
        (Some(d), None) => match &d.finished {
            None => format!(
                "{}: {} of {} MB so far ({}%).",
                d.name,
                d.have / 1_000_000,
                d.of / 1_000_000,
                if d.of == 0 { 0 } else { d.have * 100 / d.of }
            ),
            Some(Ok(())) => format!("{} is here and loading.", d.name),
            Some(Err(why)) => format!("The {} download stopped: {why}. It carries on by itself on wifi, or say \"get your own model\".", d.name),
        },
    }
}

/// Start fetching this phone's model in the background, into `models_dir`,
/// then `then(path)` with the file (to load it). Returns what to say.
pub fn start_download(models_dir: PathBuf, then: impl FnOnce(PathBuf) + Send + 'static) -> String {
    if let Some(d) = download_state() {
        if d.finished.is_none() {
            return download_said(Some(&d), None);
        }
    }
    let m = *choose(device_ram());
    if let Ok(mut d) = DOWNLOAD.lock().or_else(crate::crash::unpoison) {
        *d = Some(Download { name: m.name.into(), have: 0, of: m.bytes, finished: None });
    }
    std::thread::spawn(move || {
        let mut get = |url: &str, from: u64, w: &mut dyn FnMut(&[u8]) -> std::io::Result<()>| https_range(url, from, w);
        let mut progress = |have: u64, of: u64| {
            if let Ok(mut d) = DOWNLOAD.lock().or_else(crate::crash::unpoison) {
                if let Some(d) = d.as_mut() {
                    d.have = have;
                    d.of = of;
                }
            }
        };
        let got = fetch(&models_dir, &m, &mut get, &mut progress);
        if let Ok(mut d) = DOWNLOAD.lock().or_else(crate::crash::unpoison) {
            if let Some(d) = d.as_mut() {
                d.finished = Some(got.as_ref().map(|_| ()).map_err(|e| e.clone()));
            }
        }
        if let Ok(path) = got {
            then(path);
        }
    });
    format!(
        "Getting {} for this phone ({} MB). It stays on the phone and nothing you say leaves it. \
         Say \"how's the model download\" any time.",
        m.name,
        m.bytes / 1_000_000
    )
}

#[cfg(feature = "phone-llm")]
pub use engine::{attach, attached, PhoneLlm};

#[cfg(feature = "phone-llm")]
mod engine {
    use super::*;
    use llama_cpp_2::context::params::LlamaContextParams;
    use llama_cpp_2::llama_backend::LlamaBackend;
    use llama_cpp_2::llama_batch::LlamaBatch;
    use llama_cpp_2::model::params::LlamaModelParams;
    use llama_cpp_2::model::{AddBos, LlamaModel};
    use llama_cpp_2::sampling::LlamaSampler;
    use std::num::NonZeroU32;
    use std::sync::{Arc, Mutex, OnceLock};

    /// llama.cpp's backend is set up once per process.
    fn backend() -> Result<&'static LlamaBackend, String> {
        static B: OnceLock<Result<LlamaBackend, String>> = OnceLock::new();
        B.get_or_init(|| LlamaBackend::init().map_err(|e| e.to_string())).as_ref().map_err(|e| e.clone())
    }

    /// What the engine's thread is asked, and where the answer goes.
    type Job = (Asked, std::sync::mpsc::Sender<Result<(String, usize, f64, bool), String>>);

    /// A loaded model, with its own thread holding the model's working
    /// memory between questions.
    ///
    /// Kept between questions on purpose: Atlas's instructions to the model
    /// come first in every prompt and are the same each time, and on a phone
    /// reading them is most of the wait (measured 27 Sep 2026, 0.6B on two
    /// laptop cores: about 15 s of a 16 s answer). Whatever a new prompt
    /// shares with the last one from the start is kept, as `llama-server`
    /// does, and only the rest is read.
    struct Engine {
        jobs: Mutex<std::sync::mpsc::Sender<Job>>,
    }

    impl Engine {
        fn load(path: &Path) -> Result<Engine, String> {
            let b = backend()?;
            // Everything on the GPU where there is one (Metal on iPhone and
            // iPad); ignored where there isn't.
            let params = LlamaModelParams::default().with_n_gpu_layers(999);
            let model = LlamaModel::load_from_file(b, path, &params).map_err(|e| format!("couldn't load {}: {e}", path.display()))?;
            // The phone holds one model for as long as Atlas runs.
            let model: &'static LlamaModel = Box::leak(Box::new(model));
            // A phone's big cores: four at most, so the small ones and the
            // rest of the phone aren't starved.
            let threads = std::thread::available_parallelism().map(|n| n.get() as i32).unwrap_or(4).clamp(1, 4);
            // 2,048 tokens each: Atlas's prompts are about 1,100 and an answer
            // at most 512. Two of them (below), so both kinds of question
            // Atlas asks in one turn keep what they've read.
            let context = model.n_ctx_train().clamp(512, 2048);
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
            std::thread::Builder::new()
                .name("phone-model".into())
                .spawn(move || {
                    // Each turn asks the model two different things (what
                    // you meant, then the answer), so one kept prompt would
                    // be thrown away twice a turn (measured: 358 of 1,060
                    // tokens reused). Two slots, each keeping its own; a
                    // question goes to the one that shares most with it.
                    let mut slots = Vec::new();
                    for _ in 0..SLOTS {
                        let cp = LlamaContextParams::default()
                            .with_n_ctx(NonZeroU32::new(context))
                            .with_n_batch(512)
                            .with_n_threads(threads)
                            .with_n_threads_batch(threads);
                        match model.new_context(b, cp) {
                            Ok(c) => slots.push((c, Vec::<llama_cpp_2::token::LlamaToken>::new(), 0u64)),
                            Err(e) => {
                                let _ = ready_tx.send(Err(e.to_string()));
                                return;
                            }
                        }
                    }
                    let _ = ready_tx.send(Ok(()));
                    let mut turn = 0u64;
                    while let Ok((asked, answer)) = rx.recv() {
                        turn += 1;
                        let bos = if add_bos_for(&asked.prompt) { AddBos::Always } else { AddBos::Never };
                        let tokens = match model.str_to_token(&asked.prompt, bos) {
                            Ok(t) => t,
                            Err(e) => {
                                let _ = answer.send(Err(e.to_string()));
                                continue;
                            }
                        };
                        // The slot that keeps most of what it has read and
                        // shares most with this prompt. One that would lose
                        // most of what it holds is left for the prompt it
                        // belongs to; then the one used longest ago is taken.
                        // (Choosing on sharing alone sent both kinds of
                        // question to one slot: they share Atlas's first 358
                        // tokens, and each threw the other's away.)
                        let keeps = |i: usize| {
                            let n = shared(&slots[i].1, &tokens);
                            (n * 2 >= slots[i].1.len() && n > 0).then_some(n)
                        };
                        let best = (0..slots.len())
                            .filter_map(|i| keeps(i).map(|n| (n, i)))
                            .max()
                            .map(|(_, i)| i)
                            .or_else(|| (0..slots.len()).min_by_key(|&i| slots[i].2))
                            .unwrap_or(0);
                        let (ctx, held, used) = &mut slots[best];
                        *used = turn;
                        let got = complete_in(model, ctx, context, held, tokens, &asked);
                        let _ = answer.send(got);
                    }
                })
                .map_err(|e| e.to_string())?;
            ready_rx.recv().map_err(|_| "the model's thread stopped".to_string())??;
            Ok(Engine { jobs: Mutex::new(tx) })
        }

        /// Answer one request: (the text, tokens made, tokens a second,
        /// stopped by a stop word).
        fn complete(&self, a: &Asked) -> Result<(String, usize, f64, bool), String> {
            let (tx, rx) = std::sync::mpsc::channel();
            self.jobs.lock().map_err(|_| "the model is unavailable")?.send((a.clone(), tx)).map_err(|_| "the model's thread stopped")?;
            rx.recv().map_err(|_| "the model's thread stopped".to_string())?
        }
    }

    /// How many prompts the model keeps read at once (`Engine::load`).
    const SLOTS: usize = 2;

    /// How many tokens two prompts share from the start.
    fn shared(a: &[llama_cpp_2::token::LlamaToken], b: &[llama_cpp_2::token::LlamaToken]) -> usize {
        a.iter().zip(b).take_while(|(x, y)| x == y).count()
    }

    fn complete_in(
        model: &'static LlamaModel,
        ctx: &mut llama_cpp_2::context::LlamaContext<'static>,
        context: u32,
        held: &mut Vec<llama_cpp_2::token::LlamaToken>,
        mut tokens: Vec<llama_cpp_2::token::LlamaToken>,
        a: &Asked,
    ) -> Result<(String, usize, f64, bool), String> {
        // Keep the end of an over-long prompt: that's where the question is.
        let room = context as usize - (a.n_predict.max(16) as usize).min(context as usize / 2);
        if tokens.len() > room {
            tokens.drain(..tokens.len() - room);
        }
        if tokens.is_empty() {
            return Err("an empty prompt".into());
        }
        // What this prompt shares with what's already read is kept; at least
        // the last token is read again, for the model's next-word guess.
        let keep = shared(held, &tokens).min(tokens.len() - 1);
        if ctx.clear_kv_cache_seq(Some(0), Some(keep as u32), None).map_err(|e| e.to_string())? {
            held.truncate(keep);
        } else {
            ctx.clear_kv_cache();
            held.clear();
        }
        let from = held.len();
        if std::env::var_os("ATLAS_PHONE_DEBUG").is_some() {
            crate::errln!("phone-model: {} of {} prompt tokens already read", from, tokens.len());
        }
        let mut batch = LlamaBatch::new(512, 1);
        for (start, piece) in tokens[from..].chunks(512).enumerate().map(|(i, c)| (from + i * 512, c)) {
            batch.clear();
            for (j, t) in piece.iter().enumerate() {
                let pos = start + j;
                batch.add(*t, pos as i32, &[0], pos == tokens.len() - 1).map_err(|e| e.to_string())?;
            }
            ctx.decode(&mut batch).map_err(|e| {
                held.clear();
                e.to_string()
            })?;
        }
        *held = tokens.clone();
        let mut sampler = LlamaSampler::chain_simple([
            LlamaSampler::top_k(40),
            LlamaSampler::top_p(0.95, 1),
            LlamaSampler::temp(a.temperature.unwrap_or(0.7)),
            LlamaSampler::dist(0x5eed),
        ]);
        let mut decoder = encoding_rs::UTF_8.new_decoder();
        let mut out = String::new();
        let mut pos = tokens.len() as i32;
        let started = std::time::Instant::now();
        let mut made = 0usize;
        let mut stopped = false;
        let limit = if a.n_predict <= 0 { 512 } else { a.n_predict as usize };
        while made < limit && (pos as u32) < context {
            let tok = sampler.sample(ctx, batch.n_tokens() - 1);
            sampler.accept(tok);
            if model.is_eog_token(tok) {
                break;
            }
            out.push_str(&model.token_to_piece(tok, &mut decoder, false, None).unwrap_or_default());
            made += 1;
            if cut_at_stop(&mut out, &a.stop) {
                stopped = true;
                break;
            }
            batch.clear();
            batch.add(tok, pos, &[0], true).map_err(|e| e.to_string())?;
            pos += 1;
            ctx.decode(&mut batch).map_err(|e| {
                held.clear();
                e.to_string()
            })?;
            held.push(tok);
        }
        let secs = started.elapsed().as_secs_f64().max(1e-6);
        Ok((out, made, made as f64 / secs, stopped))
    }

    /// The model this phone is using: the engine, how to wrap a prompt for
    /// it, and its name. `Loading` while it's being read into memory.
    enum Held {
        Loading,
        Ready(Arc<Engine>, crate::models::Template, String),
        Failed(String),
    }

    static HELD: Mutex<Option<Held>> = Mutex::new(None);

    /// Load `path` as this phone's model (in the calling thread). The chat
    /// template comes from the file itself, as on the laptop.
    pub fn attach(path: &Path) -> Result<(), String> {
        if let Ok(mut h) = HELD.lock().or_else(crate::crash::unpoison) {
            *h = Some(Held::Loading);
        }
        let template = path
            .parent()
            .map(crate::models::Registry::scan)
            .and_then(|r| r.models.iter().find(|m| m.path == path).map(|m| crate::models::Template::detect(m.chat_template.as_deref(), &m.id)))
            .unwrap_or(crate::models::Template::ChatMl);
        let name = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        let got = Engine::load(path);
        let result = got.as_ref().map(|_| ()).map_err(|e| e.clone());
        if let Ok(mut h) = HELD.lock().or_else(crate::crash::unpoison) {
            *h = Some(match got {
                Ok(e) => Held::Ready(Arc::new(e), template, name),
                Err(why) => Held::Failed(why),
            });
        }
        result
    }

    /// The name of the model in use, once it's loaded.
    pub fn attached() -> Option<String> {
        match HELD.lock().or_else(crate::crash::unpoison).ok()?.as_ref()? {
            Held::Ready(_, _, n) => Some(n.clone()),
            _ => None,
        }
    }

    /// Atlas's language model on the phone: the one inside the app.
    pub struct PhoneLlm;

    impl crate::brain::Llm for PhoneLlm {
        fn complete(&self, system: &str, user: &str) -> crate::error::Result<String> {
            use crate::error::AtlasError;
            // A model being read into memory is worth a short wait.
            let started = std::time::Instant::now();
            let (engine, template, name) = loop {
                let got = match HELD.lock().map_err(|_| AtlasError::Platform("the model is unavailable".into()))?.as_ref() {
                    Some(Held::Ready(e, t, n)) => Some(Ok((e.clone(), *t, n.clone()))),
                    Some(Held::Failed(why)) => Some(Err(why.clone())),
                    Some(Held::Loading) => None,
                    None => Some(Err("there's no language model on this phone yet -- say \"get your own model\" (best on wifi)".into())),
                };
                match got {
                    Some(Ok(x)) => break x,
                    Some(Err(why)) => return Err(AtlasError::Platform(why)),
                    None if started.elapsed().as_secs() < crate::models::LOADING_SECS => std::thread::sleep(std::time::Duration::from_millis(200)),
                    None => return Err(AtlasError::Platform("the phone's model is still loading".into())),
                }
            };
            let asked = Asked {
                prompt: no_thinking(template.render(system, user), &name),
                n_predict: 512,
                stop: template.stop_tokens().iter().map(|s| s.to_string()).collect(),
                temperature: None,
            };
            engine.complete(&asked).map(|(text, ..)| without_thinking(&text)).map_err(AtlasError::Platform)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_phone_gets_the_model_its_memory_can_hold() {
        assert_eq!(choose(Some(8 * GB)).name, "Qwen3 1.7B");
        assert_eq!(choose(Some(7_600_000_000)).name, "Qwen3 1.7B", "an 8 GB phone reports a little under 8");
        assert_eq!(choose(Some(6 * GB)).name, "Qwen3 0.6B");
        assert_eq!(choose(None).name, "Qwen3 0.6B", "unknown memory: the small one");
        for m in MODELS {
            assert!(m.url.starts_with("https://huggingface.co/Qwen/") && m.url.ends_with(m.file), "{}", m.url);
            assert_eq!(m.sha256.len(), 64);
        }
    }

    #[test]
    fn stop_words_cut_and_control_tokens_decide_the_first_token() {
        let mut t = "Sure.<|im_end|>\nmore".to_string();
        assert!(cut_at_stop(&mut t, &["<|im_end|>".into(), "".into()]));
        assert_eq!(t, "Sure.");
        let mut t = "nothing to cut".to_string();
        assert!(!cut_at_stop(&mut t, &["<|im_end|>".into()]));
        assert!(!add_bos_for("<|im_start|>system\nhi"));
        assert!(!add_bos_for("<|begin_of_text|>"));
        assert!(add_bos_for("Once upon a time"));
        // Qwen3 answers straight away rather than thinking out loud first.
        let p = crate::models::Template::ChatMl.render("sys", "hi");
        assert!(no_thinking(p.clone(), "Qwen3-0.6B-Q8_0").ends_with("<|im_start|>assistant\n<think>\n\n</think>\n\n"));
        assert_eq!(no_thinking(p.clone(), "stories15M"), p);
        assert_eq!(without_thinking("<think>\nhmm, the user\n</think>\n\nSure."), "Sure.");
        assert_eq!(without_thinking("Sure. <think>unfinished"), "Sure.");
        let asked: Asked = serde_json::from_str(&crate::models::completion_body("P", crate::models::Template::ChatMl, 64, &Default::default())).unwrap();
        assert_eq!((asked.prompt.as_str(), asked.n_predict), ("P", 64));
        assert!(asked.stop.iter().any(|s| s == "<|im_end|>"), "{:?}", asked.stop);
    }

    #[test]
    fn a_download_resumes_and_is_kept_only_if_it_is_the_pinned_file() {
        let dir = std::env::temp_dir().join(format!("atlas-phonemodel-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let bytes: Vec<u8> = (0..100_000u32).map(|i| (i % 251) as u8).collect();
        let sha: &'static str = Box::leak(crate::digest::sha256_hex(&bytes).into_boxed_str());
        let m = PhoneModel { name: "test", file: "t.gguf", url: "https://example/t.gguf", sha256: sha, bytes: bytes.len() as u64, min_ram: 0 };
        // Stops after 30 000 bytes the first time.
        let mut asked_from = Vec::new();
        let mut first = |_: &str, from: u64, w: &mut dyn FnMut(&[u8]) -> std::io::Result<()>| {
            asked_from.push(from);
            w(&bytes[from as usize..30_000]).map_err(|e| e.to_string())
        };
        let r = fetch(&dir, &m, &mut first, &mut |_, _| {});
        assert!(r.unwrap_err().contains("stopped partway"));
        let mut rest = |_: &str, from: u64, w: &mut dyn FnMut(&[u8]) -> std::io::Result<()>| {
            asked_from.push(from);
            w(&bytes[from as usize..]).map_err(|e| e.to_string())
        };
        let got = fetch(&dir, &m, &mut rest, &mut |_, _| {}).unwrap();
        assert_eq!(asked_from, vec![0, 30_000], "it didn't carry on from where it stopped");
        assert_eq!(std::fs::read(&got).unwrap(), bytes);
        // A file that isn't the pinned one is thrown away.
        let m2 = PhoneModel { file: "u.gguf", ..m };
        let mut wrong = |_: &str, _: u64, w: &mut dyn FnMut(&[u8]) -> std::io::Result<()>| w(&vec![0u8; bytes.len()]).map_err(|e| e.to_string());
        assert!(fetch(&dir, &m2, &mut wrong, &mut |_, _| {}).unwrap_err().contains("isn't the one"));
        assert!(!dir.join("u.gguf").exists() && !dir.join("u.gguf.part").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The real download, from Hugging Face through its redirect to storage,
    /// stopped partway and carried on. Needs the internet:
    /// `cargo test --lib phonemodel -- --ignored`.
    #[test]
    #[ignore = "needs the internet"]
    fn a_real_download_from_hugging_face_resumes_and_checks() {
        let dir = std::env::temp_dir().join(format!("atlas-phonemodel-real-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let m = PhoneModel {
            name: "a tiny test model",
            file: "stories15M-q4_0.gguf",
            url: "https://huggingface.co/ggml-org/models/resolve/main/tinyllamas/stories15M-q4_0.gguf",
            sha256: "66967fbece6dbe97886593fdbb73589584927e29119ec31f08090732d1861739",
            bytes: 19_077_344,
            min_ram: 0,
        };
        // Stop after about 5 MB, as a phone losing its signal would.
        let mut first = |url: &str, from: u64, w: &mut dyn FnMut(&[u8]) -> std::io::Result<()>| {
            let mut got = 0u64;
            let _ = https_range(url, from, &mut |c: &[u8]| {
                if got > 5_000_000 {
                    return Err(std::io::Error::other("signal lost"));
                }
                got += c.len() as u64;
                w(c)
            });
            Ok(())
        };
        assert!(fetch(&dir, &m, &mut first, &mut |_, _| {}).is_err());
        let partial = std::fs::metadata(dir.join("stories15M-q4_0.gguf.part")).unwrap().len();
        assert!(partial > 4_000_000 && partial < m.bytes, "{partial}");
        let mut rest = |url: &str, from: u64, w: &mut dyn FnMut(&[u8]) -> std::io::Result<()>| https_range(url, from, w);
        let path = fetch(&dir, &m, &mut rest, &mut |_, _| {}).unwrap();
        assert_eq!(std::fs::metadata(&path).unwrap().len(), m.bytes);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_phone_fetches_its_own_model_by_itself_only_on_wifi_and_only_once() {
        let going = Download { name: "m".into(), have: 1, of: 9, finished: None };
        let stopped = Download { finished: Some(Err("the wifi went".into())), ..going.clone() };
        let done = Download { finished: Some(Ok(())), ..going.clone() };
        assert!(fetch_by_itself(true, false, None, 1_000, 0), "on wifi with no model, it starts by itself");
        assert!(!fetch_by_itself(false, false, None, 1_000, 0), "never on mobile data");
        assert!(!fetch_by_itself(true, true, None, 1_000, 0), "not when it has one");
        assert!(!fetch_by_itself(true, false, Some(&going), 1_000, 0), "not a second download");
        assert!(!fetch_by_itself(true, false, Some(&done), 1_000, 0));
        assert!(!fetch_by_itself(true, false, Some(&stopped), 1_000, 700), "a stopped one isn't retried every second");
        assert!(fetch_by_itself(true, false, Some(&stopped), 1_400, 700), "but is, ten minutes on");
    }

}
