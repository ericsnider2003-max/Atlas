//! Small models on the laptop's NPU (item 20, Eric's yes, 1 Oct 2026).
//!
//! The HP OmniBook's Core Ultra 7 256V has an Intel AI Boost NPU that Atlas
//! never used: search (the meaning encoder) and telling voices apart (CAM++)
//! ran on the processor, competing with the voice and the model server, and
//! the graphics chip is kept for the thinking model. This moves those two to
//! the NPU, where the machine has one, and leaves everything exactly as it
//! was where it doesn't.
//!
//! How, all permissive and all fetched by Atlas itself (Eric's
//! self-provisioning ruling):
//!
//! - **ONNX Runtime 1.28.2** -- already here: it's the `onnxruntime.dll`
//!   that comes with the Kokoro voice (`kokoro::RUNTIME_DIR`). One runtime
//!   in the process, opened at run time (`ort`, `load-dynamic`), so
//!   nothing is linked into atlas.exe.
//! - **Intel's OpenVINO plugin for it** (`Intel.ML.OnnxRuntime.EP.OpenVINO`
//!   1.7.0, MIT, 118 MB, hash-pinned): the execution provider and
//!   OpenVINO 2026.3 with its NPU compiler, in `tools/npu`. Registered as a
//!   plugin (ONNX Runtime 1.23+), so the stock runtime needs no special
//!   build. Fetched only on a computer that has an Intel NPU.
//!
//! What the NPU wants is fixed input sizes ("only models with static shapes
//! are supported on NPU", OpenVINO's NPU guide). Both models here already
//! run that way: the meaning encoder pads texts to 16/32/64/128 tokens, and
//! CAM++ only works on 200-frame windows. Each size is given to the provider
//! (`reshape_input`) and compiled once into `data/cache/npu`, so later starts
//! skip the compile.
//!
//! Anything that goes wrong -- no NPU, no plugin, the driver refuses a model
//! -- is said once in the log and that model stays on the processor
//! (`tract`), which is what ran before. Nothing here can make Atlas worse
//! than it was.
//!
//! **Not moved yet, said plainly:** speech-to-text (Parakeet) and the wake
//! word run inside sherpa-onnx, whose NPU route (`provider=openvino`, 1.13.8)
//! needs an ONNX Runtime built with OpenVINO inside rather than this plugin.
//! That's the next step, measured first (`atlas npu-check`).

use crate::getpieces::{Lands, Piece};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// Where the plugin and OpenVINO land, install-relative.
pub const DIR: &str = "tools/npu";

/// The plugin library ONNX Runtime registers.
pub const PLUGIN: &str = "tools/npu/onnxruntime_providers_openvino_plugin.dll";

/// The name the plugin's provider goes by.
pub const PROVIDER: &str = "OpenVINOExecutionProvider";

/// Compiled models, kept between starts.
pub fn cache_dir() -> PathBuf {
    crate::roots::data_dir().join("cache").join("npu")
}

/// Intel's plugin package, pinned. Windows on x86-64 only: it's the only
/// build Intel publishes.
pub fn piece() -> Option<Piece> {
    if !cfg!(all(windows, target_arch = "x86_64")) {
        return None;
    }
    Some(Piece {
        name: "the NPU engine",
        for_what: "searching and telling voices apart on the NPU, leaving the processor free",
        url: "https://api.nuget.org/v3-flatcontainer/intel.ml.onnxruntime.ep.openvino/1.7.0/intel.ml.onnxruntime.ep.openvino.1.7.0.nupkg",
        sha256: "483c6ac2c268f8fc3dcbbd951fdb41ebf053456c6718d60941f50fb392dd794d",
        bytes: 117_914_126,
        lands: Lands::Zip { inside: "runtimes/win-x64/native", dir: DIR, key: PLUGIN },
    })
}

/// What Atlas fetches for the NPU: the plugin, on a computer that has an
/// Intel NPU. Nothing anywhere else.
pub fn pieces() -> Vec<Piece> {
    if has_intel_npu() {
        piece().into_iter().collect()
    } else {
        Vec::new()
    }
}

/// Whether this computer has an Intel NPU with its driver running, from the
/// "compute accelerator" device class -- read once.
pub fn has_intel_npu() -> bool {
    static FOUND: OnceLock<bool> = OnceLock::new();
    *FOUND.get_or_init(|| accelerators().iter().any(|name| is_intel_npu(name)))
}

/// Is this device name an Intel NPU? "Intel(R) AI Boost" on Meteor, Lunar
/// and Arrow Lake.
pub fn is_intel_npu(device: &str) -> bool {
    let d = device.to_lowercase();
    d.contains("intel") && (d.contains("ai boost") || d.contains("npu"))
}

/// The names of the compute accelerators Windows has a driver for
/// (`ComputeAccelerator` class, {f01a9d53-...}).
#[cfg(windows)]
fn accelerators() -> Vec<String> {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ};
    let mut out = Vec::new();
    for i in 0..8 {
        let key = HSTRING::from(format!(
            r"SYSTEM\CurrentControlSet\Control\Class\{{f01a9d53-3ff6-48d2-9f97-c8a7004be10c}}\{i:04}"
        ));
        let value = HSTRING::from("DriverDesc");
        let mut buf = [0u16; 256];
        let mut len = (buf.len() * 2) as u32;
        let ok = unsafe {
            RegGetValueW(
                HKEY_LOCAL_MACHINE,
                PCWSTR(key.as_ptr()),
                PCWSTR(value.as_ptr()),
                RRF_RT_REG_SZ,
                None,
                Some(buf.as_mut_ptr().cast()),
                Some(&mut len),
            )
        };
        if ok.is_ok() {
            let n = (len as usize / 2).saturating_sub(1).min(buf.len());
            out.push(String::from_utf16_lossy(&buf[..n]));
        }
    }
    out
}

#[cfg(not(windows))]
fn accelerators() -> Vec<String> {
    Vec::new()
}

// ------------------------------------------------------------------ engine

/// Where a model runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Where {
    Npu,
    /// ONNX Runtime on the processor: used to check the NPU's answers, and
    /// where there is no NPU but the runtime is here.
    Cpu,
}

impl Where {
    pub fn plain(self) -> &'static str {
        match self {
            Where::Npu => "the NPU",
            Where::Cpu => "the processor",
        }
    }
}

struct Engine {
    env: std::sync::Arc<ort::environment::Environment>,
    npu: bool,
    /// Kept registered for the life of the process.
    _plugin: Option<ort::ep::ExecutionProviderLibrary>,
}

static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();

/// The runtime library: the one the Kokoro voice brought.
pub fn runtime_path(root: &Path) -> Option<PathBuf> {
    let (ort, _) = crate::kokoro::runtime_files()?;
    let p = root.join(crate::kokoro::RUNTIME_DIR).join(ort);
    p.is_file().then_some(p)
}

fn engine(root: &Path) -> Result<&'static Engine, String> {
    ENGINE
        .get_or_init(|| {
            let lib = runtime_path(root).ok_or("ONNX Runtime isn't here yet (it comes with the Kokoro voice)")?;
            ort::init_from(&lib).map_err(|e| format!("ONNX Runtime wouldn't open: {e}"))?.with_name("atlas").commit();
            let env = ort::environment::Environment::current().map_err(|e| format!("ONNX Runtime wouldn't start: {e}"))?;
            let plugin_path = root.join(PLUGIN);
            let plugin = if plugin_path.is_file() {
                match env.register_ep_library("openvino", &plugin_path) {
                    Ok(p) => Some(p),
                    Err(e) => {
                        crate::outln!("the NPU engine wouldn't load: {e}");
                        None
                    }
                }
            } else {
                None
            };
            let npu = plugin.is_some() && npu_devices(&env).next().is_some();
            Ok(Engine { env, npu, _plugin: plugin })
        })
        .as_ref()
        .map_err(|e| e.clone())
}

fn npu_devices(env: &ort::environment::Environment) -> impl Iterator<Item = ort::device::Device<'_>> + '_ {
    env.devices().filter(|d| {
        d.ep().is_ok_and(|ep| ep == PROVIDER) && d.hardware_device().ty() == ort::memory::DeviceType::NPU
    })
}

/// Is the NPU usable from here (runtime, plugin, device all present)?
pub fn npu_ready(root: &Path) -> bool {
    engine(root).is_ok_and(|e| e.npu)
}

/// Why the NPU isn't used, in words, or `None` when it is.
pub fn why_not(root: &Path) -> Option<String> {
    if !has_intel_npu() && cfg!(windows) {
        return Some("this computer has no Intel NPU".into());
    }
    match engine(root) {
        Err(e) => Some(e.clone()),
        Ok(e) if !e.npu && !root.join(PLUGIN).is_file() => Some("the NPU engine isn't downloaded yet".into()),
        Ok(e) if !e.npu => Some("the NPU engine is here, but no NPU answered".into()),
        Ok(_) => None,
    }
}

/// One input: its name, shape and values.
pub enum In {
    F32(String, Vec<i64>, Vec<f32>),
    I64(String, Vec<i64>, Vec<i64>),
}

/// A model opened in ONNX Runtime, on the NPU when it can be.
pub struct Session {
    s: Mutex<ort::session::Session>,
    pub on: Where,
}

/// Each free dimension's name in `model`'s inputs, with the size `shapes`
/// gives it there: what `with_dimension_override` fixes.
fn free_dimensions(model: &Path, shapes: &[(String, Vec<i64>)]) -> Result<Vec<(String, i64)>, String> {
    let s = ort::session::Session::builder()
        .and_then(|mut b| b.commit_from_file(model))
        .map_err(|e| format!("couldn't open {}: {e}", model.display()))?;
    let mut out: Vec<(String, i64)> = Vec::new();
    for input in s.inputs() {
        let Some((_, want)) = shapes.iter().find(|(n, _)| n == input.name()) else { continue };
        if let ort::value::ValueType::Tensor { dimension_symbols, .. } = input.dtype() {
            out.extend(free_dimensions_of(dimension_symbols, want));
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

/// The named dimensions of one input, paired with the sizes wanted.
pub fn free_dimensions_of(symbols: &[String], want: &[i64]) -> Vec<(String, i64)> {
    symbols.iter().zip(want).filter(|(s, _)| !s.is_empty()).map(|(s, w)| (s.clone(), *w)).collect()
}

/// Should a model stay on the NPU, from its first run beside the processor's?
/// Only when it gives the same answer (cosine 0.99 or better) and is faster
/// -- the NPU is there to make Atlas quicker, never slower.
pub fn worth_keeping(npu: std::time::Duration, cpu: std::time::Duration, agree: f32) -> bool {
    agree >= 0.99 && npu < cpu
}

/// The `reshape_input` value for fixed shapes: `name[1,16],other[1,16]`.
pub fn reshape_value(shapes: &[(String, Vec<i64>)]) -> String {
    shapes
        .iter()
        .map(|(n, s)| format!("{n}[{}]", s.iter().map(|d| d.to_string()).collect::<Vec<_>>().join(",")))
        .collect::<Vec<_>>()
        .join(",")
}

impl Session {
    /// The model's input names, from a quick open on the processor.
    pub fn input_names(root: &Path, model: &Path) -> Result<Vec<String>, String> {
        engine(root)?;
        let s = ort::session::Session::builder()
            .and_then(|mut b| b.commit_from_file(model))
            .map_err(|e| format!("couldn't open {}: {e}", model.display()))?;
        Ok(s.inputs().iter().map(|i| i.name().to_string()).collect())
    }

    /// Open `model` with these fixed input shapes: on the NPU when `want`
    /// is `Where::Npu` and the NPU is ready, else on the processor.
    pub fn open(root: &Path, model: &Path, shapes: &[(String, Vec<i64>)], want: Where) -> Result<Session, String> {
        let e = engine(root)?;
        let mut b = ort::session::Session::builder().map_err(|e| e.to_string())?;
        // The sizes fixed in the graph itself, by the names the model gives
        // its free dimensions ("batch_size", "sequence_length"): measured on
        // the laptop (2 Oct 2026), the provider's own `reshape_input` alone
        // left them free, the NPU compiler refused the graph ("upper bounds
        // are not specified"), and every sentence fell back -- 326 ms against
        // tract's 20. Fixed here, before any provider sees the graph.
        for (sym, size) in free_dimensions(model, shapes)? {
            b = b.with_dimension_override(&sym, size).map_err(|e| e.to_string())?;
        }
        let mut on = Where::Cpu;
        if want == Where::Npu && e.npu {
            let cache = cache_dir();
            let _ = std::fs::create_dir_all(&cache);
            let opts = vec![
                (format!("{PROVIDER}.cache_dir"), cache.display().to_string()),
                (format!("{PROVIDER}.reshape_input"), reshape_value(shapes)),
            ];
            b = b.with_devices(npu_devices(&e.env), Some(&opts)).map_err(|e| format!("the NPU wouldn't take the model: {e}"))?;
            on = Where::Npu;
        }
        let s = b.commit_from_file(model).map_err(|e| format!("couldn't open {} on {}: {e}", model.display(), on.plain()))?;
        Ok(Session { s: Mutex::new(s), on })
    }

    /// Run, timed: the values and how long the model took.
    pub fn run_timed(&self, inputs: Vec<In>) -> Result<(Vec<Vec<f32>>, std::time::Duration), String> {
        let t = std::time::Instant::now();
        let out = self.run(inputs)?;
        Ok((out, t.elapsed()))
    }

    /// Run, returning each output's values as f32, in the model's order.
    pub fn run(&self, inputs: Vec<In>) -> Result<Vec<Vec<f32>>, String> {
        let mut values: Vec<(std::borrow::Cow<'static, str>, ort::session::SessionInputValue<'static>)> = Vec::new();
        for i in inputs {
            match i {
                In::F32(n, shape, v) => {
                    let t = ort::value::Tensor::from_array((shape, v)).map_err(|e| e.to_string())?;
                    values.push((n.into(), t.into()));
                }
                In::I64(n, shape, v) => {
                    let t = ort::value::Tensor::from_array((shape, v)).map_err(|e| e.to_string())?;
                    values.push((n.into(), t.into()));
                }
            }
        }
        let mut s = self.s.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let out = s.run(values).map_err(|e| format!("the model failed on {}: {e}", self.on.plain()))?;
        let mut all = Vec::new();
        for (_, v) in out.iter() {
            let (_, data) = v.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            all.push(data.to_vec());
        }
        Ok(all)
    }
}

// ------------------------------------------------------------------ check

/// Cosine similarity of two vectors (both already unit length, so a dot
/// product), for checking the NPU gives the processor's answer.
pub fn agreement(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// The sentences `check` times: everyday requests of the length search sees.
pub const CHECK_TEXTS: &[&str] = &[
    "where did I put the lease for the apartment",
    "what did I say about the market brief last week",
    "find the notes from the call with the developer",
    "remind me what the plan was for the phone app",
    "which videos did I post in September and how did they do",
    "the invoice from the printer people, the one in the downloads folder",
    "what's on my calendar tomorrow afternoon",
    "open the document about the hub design",
];

/// `atlas npu-check`: is the NPU there, does each model run on it, does it
/// give the processor's answer, and how long each takes -- measured here,
/// on this computer, never assumed.
pub fn check(root: &Path) -> String {
    let mut out = Vec::new();
    out.push(format!(
        "Intel NPU: {}",
        if has_intel_npu() { "yes" } else if cfg!(windows) { "no" } else { "not looked for (not Windows)" }
    ));
    match why_not(root) {
        None => out.push("NPU engine: ready".into()),
        Some(why) => out.push(format!("NPU engine: not in use -- {why}")),
    }
    // Search.
    match crate::meaningnative::Native::load(root) {
        None => out.push("Search: the meaning model isn't downloaded, so there's nothing to measure".into()),
        Some(mut enc) => {
            let (mut cpu_ms, mut npu_ms, mut worst) = (0u128, 0u128, 1.0f32);
            // Once each first: the first run compiles (and fills the cache).
            let _ = enc.embed_on_processor(CHECK_TEXTS[0]);
            let t = std::time::Instant::now();
            let _ = enc.embed(CHECK_TEXTS[0]);
            let first = t.elapsed().as_millis();
            for text in CHECK_TEXTS {
                let t = std::time::Instant::now();
                let a = enc.embed_on_processor(text);
                cpu_ms += t.elapsed().as_millis();
                let t = std::time::Instant::now();
                let b = enc.embed(text);
                npu_ms += t.elapsed().as_millis();
                if let (Some(a), Some(b)) = (a, b) {
                    worst = worst.min(agreement(&a, &b));
                }
            }
            let n = CHECK_TEXTS.len() as u128;
            out.push(format!(
                "Search: {} ms a sentence on the processor, {} ms the way Atlas now runs it (first, compiling: {first} ms); \
                 answers agree to {:.4} at worst",
                cpu_ms / n,
                npu_ms / n,
                worst
            ));
        }
    }
    // Telling voices apart, on three seconds of a test tone.
    let models = root.join("models");
    if crate::speakernet::installed(&models) {
        let samples: Vec<f32> = (0..48_000).map(|i| ((i as f32) * 0.05).sin() * 0.3 + ((i as f32) * 0.013).sin() * 0.2).collect();
        let _ = crate::speakernet::embed_on_processor(&samples, &models);
        let t = std::time::Instant::now();
        let a = crate::speakernet::embed_on_processor(&samples, &models);
        let cpu = t.elapsed().as_millis();
        let _ = crate::speakernet::embed(&samples, &models);
        let t = std::time::Instant::now();
        let b = crate::speakernet::embed(&samples, &models);
        let now = t.elapsed().as_millis();
        let agree = match (a, b) {
            (Ok(a), Ok(b)) => format!("{:.4}", agreement(&a, &b)),
            (Err(e), _) | (_, Err(e)) => format!("couldn't compare: {e}"),
        };
        out.push(format!("Voice ID: {cpu} ms on the processor, {now} ms the way Atlas now runs it; answers agree to {agree}"));
    } else {
        out.push("Voice ID: the voice model isn't downloaded, so there's nothing to measure".into());
    }
    out.join("\n")
}
