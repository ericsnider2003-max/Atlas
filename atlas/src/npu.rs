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

#[path = "npu/worker.rs"]
mod worker;
static WORKER_ROOT: OnceLock<PathBuf> = OnceLock::new();
static IN_WORKER: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Private helper entry. No ordinary Atlas state or services are started here.
pub fn worker_entry() -> i32 {
    if std::env::args_os().count() != 2 { return 2; }
    IN_WORKER.store(true, std::sync::atomic::Ordering::Relaxed);
    #[cfg(windows)]
    unsafe { windows::Win32::System::Diagnostics::Debug::SetErrorMode(windows::Win32::System::Diagnostics::Debug::SEM_NOGPFAULTERRORBOX | windows::Win32::System::Diagnostics::Debug::SEM_FAILCRITICALERRORS); }
    worker::serve()
}

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
fn cache_dir() -> PathBuf {
    WORKER_ROOT.get().map(|root| root.join("data")).unwrap_or_else(crate::roots::data_dir).join("cache").join("npu")
}

/// Intel's plugin package, pinned. Windows on x86-64 only: it's the only
/// build Intel publishes.
pub fn npu_piece() -> Option<Piece> {
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
        npu_piece().into_iter().collect()
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
    blocked: Option<String>,
}

// The guard and native provider loader are separate so the crash boundary
// can be exercised without loading a DLL into the test process.
fn guarded_provider<T>(guard: impl FnOnce() -> Option<String>, load: impl FnOnce() -> Option<T>) -> (Option<T>, Option<String>) {
    let blocked = guard();
    if blocked.is_some() {
        (None, blocked)
    } else {
        (load(), None)
    }
}

static NATIVE_ATTEMPT: Mutex<()> = Mutex::new(());

// Access violations cannot unwind a Rust guard: the marker therefore remains
// after a native crash, but an ordinary returned error removes it.
struct NativeAttempt {
    marker: PathBuf,
    _serial: std::sync::MutexGuard<'static, ()>,
}

impl NativeAttempt {
    fn begin(marker: PathBuf, what: &str) -> Result<Self, String> {
        use std::io::Write;
        let serial = NATIVE_ATTEMPT.lock().unwrap_or_else(|e| e.into_inner());
        let parent = marker.parent().ok_or("no parent for the native recovery marker")?;
        std::fs::create_dir_all(parent).map_err(|e| format!("couldn't prepare native recovery: {e}"))?;
        let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(&marker)
            .map_err(|e| format!("won't use the NPU without a recovery marker: {e}"))?;
        if let Err(e) = file.write_all(what.as_bytes()).and_then(|_| file.sync_all()) {
            drop(file);
            crate::heard!(std::fs::remove_file(&marker));
            return Err(format!("won't use the NPU without a durable recovery marker: {e}"));
        }
        Ok(Self { marker, _serial: serial })
    }
}

// Owned helpers have a parent-held per-attempt receipt. Legacy in-process
// markers remain readable for upgrade recovery but are never written by them.
fn child_attempt(what: &str) -> Result<Option<NativeAttempt>, String> {
    if IN_WORKER.load(std::sync::atomic::Ordering::Relaxed) { Ok(None) }
    else { NativeAttempt::begin(cache_dir().join(COMPILING), what).map(Some) }
}

impl Drop for NativeAttempt {
    fn drop(&mut self) {
        crate::heard!(std::fs::remove_file(&self.marker));
    }
}

static ENGINE: OnceLock<Result<Engine, String>> = OnceLock::new();

/// The runtime library: the one the Kokoro voice brought.
fn runtime_path(root: &Path) -> Option<PathBuf> {
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
            if !IN_WORKER.load(std::sync::atomic::Ordering::Relaxed) { return Ok(Engine { env, npu: false, _plugin: None, blocked: None }); }
            let plugin_path = root.join(PLUGIN);
            let (plugin, crashed) = guarded_provider(|| crashed_atlas(&verdicts(), &engine_version()), || if IN_WORKER.load(std::sync::atomic::Ordering::Relaxed) && plugin_path.is_file() {
                let _attempt = match child_attempt("loading the NPU provider") {
                    Ok(attempt) => attempt,
                    Err(why) => {
                        crate::outln!("{why}; using the processor");
                        return None;
                    }
                };
                match env.register_ep_library("openvino", &plugin_path) {
                    Ok(p) => {
                        let available = npu_devices(&env).next().is_some();
                        Some((p, available))
                    }
                    Err(e) => {
                        crate::outln!("the NPU engine wouldn't load: {e}");
                        None
                    }
                }
            } else {
                None
            });
            // An engine that has crashed Atlas is never used again
            // (`crashed_atlas`): Eric's laptop, 2 Oct to 6 Oct 2026, seven
            // crashes, every one an access violation inside
            // openvino_intel_npu_compiler.dll -- in Atlas's own process, so
            // the whole of Atlas went with it.
            if let Some(why) = &crashed {
                crate::outln!("the NPU stays off: {why}. Search and voice ID run on the processor.");
            }
            let npu = crashed.is_none() && plugin.as_ref().is_some_and(|(_, available)| *available);
            let plugin = plugin.map(|(p, _)| p);
            Ok(Engine { env, npu, _plugin: plugin, blocked: crashed })
        })
        .as_ref()
        .map_err(|e| e.clone())
}

fn npu_devices(env: &ort::environment::Environment) -> impl Iterator<Item = ort::device::Device<'_>> + '_ {
    env.devices().filter(|d| {
        d.ep().is_ok_and(|ep| ep == PROVIDER) && d.hardware_device().ty() == ort::memory::DeviceType::NPU
    })
}

/// Eligible for a bounded isolated trial; files alone do not prove NPU readiness.
pub fn npu_ready(root: &Path) -> bool {
    if IN_WORKER.load(std::sync::atomic::Ordering::Relaxed) { engine(root).is_ok_and(|e| e.npu) }
    else { root.join(PLUGIN).is_file() && runtime_path(root).is_some() && crashed_atlas(&verdicts(), &engine_version()).is_none() && worker::allowed(root) }
}

/// Is ONNX Runtime itself usable from here, NPU or not? It comes with the
/// Kokoro voice. The hands use it on the processor too (2 Oct 2026): it ran
/// the two hand models seven to ten times faster than `tract` when measured.
pub fn runtime_ready(root: &Path) -> bool {
    engine(root).is_ok()
}

/// Why the NPU isn't used, in words, or `None` when it is.
fn why_not_on_npu(root: &Path) -> Option<String> {
    if !has_intel_npu() && cfg!(windows) {
        return Some("this computer has no Intel NPU".into());
    }
    if !IN_WORKER.load(std::sync::atomic::Ordering::Relaxed) {
        if let Some(reason) = crashed_atlas(&verdicts(), &engine_version()) { return Some(reason); }
        if let Some(reason) = worker::blocked(root) { return Some(reason); }
        return Some(if npu_ready(root) { "eligible for an isolated helper trial; availability is not yet verified".into() } else { "no eligible isolated NPU engine is available".into() });
    }
    match engine(root) {
        Err(e) => Some(e.clone()),
        Ok(e) if e.blocked.is_some() => e.blocked.clone(),
        Ok(e) if !e.npu && !root.join(PLUGIN).is_file() => Some("the NPU engine isn't downloaded yet".into()),
        Ok(e) if !e.npu => Some("the NPU engine is here, but no NPU answered".into()),
        Ok(_) => None,
    }
}

/// One input: its name, shape and values.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub enum In {
    F32(String, Vec<i64>, Vec<f32>),
    I64(String, Vec<i64>, Vec<i64>),
}

/// A model opened in ONNX Runtime, on the NPU when it can be.
pub struct Session {
    s: Option<Mutex<ort::session::Session>>,
    remote: Option<Mutex<worker::Client>>,
    fallback: Option<(PathBuf, PathBuf, Vec<(String, Vec<i64>)>, bool)>,
    cpu_fallback: Mutex<Option<Box<Session>>>,
    on: std::sync::atomic::AtomicU8,
}

/// A copy of `model` with these input sizes written in, kept in the NPU
/// cache (made once). `None` if it can't be made.
fn fixed_copy(model: &Path, shapes: &[(String, Vec<i64>)]) -> Option<PathBuf> {
    let stem = model.file_stem()?.to_string_lossy().to_string();
    let sig: String = shapes.iter().map(|(_, d)| d.iter().map(|x| x.to_string()).collect::<Vec<_>>().join("x")).collect::<Vec<_>>().join("-");
    let out = cache_dir().join(format!("{stem}-{sig}.onnx"));
    if out.is_file() {
        return Some(out);
    }
    let bytes = std::fs::read(model).ok()?;
    let fixed = crate::onnxfix::with_fixed_inputs(&bytes, shapes)?;
    std::fs::create_dir_all(cache_dir()).ok()?;
    let tmp = out.with_extension("part");
    std::fs::write(&tmp, fixed).ok()?;
    crate::store::rename_patiently(&tmp, &out).ok()?;
    Some(out)
}

// ------------------------------------------------------------------ verdicts

/// Where each model and size ran best when last measured, kept so a model
/// that lost to the processor isn't compiled for the NPU again on every
/// start (each try costs a few seconds). Keyed by the NPU engine's version
/// too, so a new engine is tried afresh.
fn verdict_key(model: &Path, shapes: &[(String, Vec<i64>)]) -> String {
    let version = npu_piece().map(|p| p.sha256.get(..12).unwrap_or("").to_string()).unwrap_or_default();
    format!("{}|{}|{version}", model.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(), reshape_value(shapes))
}

/// The NPU engine's version, as the verdicts key it.
fn engine_version() -> String {
    npu_piece().map(|p| p.sha256.get(..12).unwrap_or("").to_string()).unwrap_or_default()
}

/// The verdict that turns the NPU off for this engine: it took Atlas down.
fn crash_key(version: &str) -> String {
    format!("crashed|{version}")
}

/// Why the NPU must stay off, when it has crashed Atlas with this engine:
/// a compile that never finished (`COMPILING` left behind by a process that
/// died), or a crash Windows recorded in the NPU compiler. `None` when
/// neither. Checked before the engine is used, every start.
pub fn crashed_atlas(v: &std::collections::BTreeMap<String, String>, version: &str) -> Option<String> {
    crashed_atlas_in(v, version, &cache_dir(), windows_saw_the_compiler_crash)
}

fn crashed_atlas_in(v: &std::collections::BTreeMap<String, String>, version: &str, cache: &Path, compiler_crash: impl FnOnce() -> Option<String>) -> Option<String> {
    if let Some(when) = v.get(&crash_key(version)) {
        return Some(format!("its compiler crashed Atlas ({when})"));
    }
    let marker = cache.join(COMPILING);
    let mut why = None;
    if let Ok(text) = std::fs::read_to_string(&marker) {
        why = Some(format!("Atlas stopped while compiling {} for it", text.trim()));
    } else if let Some(when) = compiler_crash() {
        why = Some(format!("Windows recorded Atlas crashing in its compiler on {when}"));
    }
    let why = why?;
    let mut v = v.clone();
    v.insert(crash_key(version), why.clone());
    crate::heard!(std::fs::create_dir_all(cache));
    match crate::store::write_json(&cache.join("verdicts.json"), &v) {
        Ok(()) => { crate::heard!(std::fs::remove_file(&marker)); }
        Err(e) => crate::outln!("couldn't keep the NPU quarantine: {e}; leaving its recovery marker in place"),
    }
    Some(why)
}

/// Left in the cache while a model compiles for the NPU, removed when it
/// has: a process that dies in the compiler leaves it behind.
const COMPILING: &str = "compiling.now";

/// When Windows last recorded atlas.exe crashing inside the NPU compiler
/// (Application log, event 1000), from `wevtutil`. Windows only.
fn windows_saw_the_compiler_crash() -> Option<String> {
    if !cfg!(windows) {
        return None;
    }
    let out = crate::tools::command("wevtutil")
        .args(["qe", "Application", "/q:*[System[(EventID=1000)]]", "/c:50", "/rd:true", "/f:text"])
        .output()
        .ok()?;
    crash_in_compiler(&String::from_utf8_lossy(&out.stdout))
}

/// The date of the newest event in `wevtutil`'s text that is atlas.exe
/// faulting in the OpenVINO NPU compiler.
pub fn crash_in_compiler(events: &str) -> Option<String> {
    events.split("Event[").find_map(|e| {
        let ours = e.contains("Faulting application name: atlas.exe") && e.to_lowercase().contains("openvino_intel_npu_compiler");
        if !ours {
            return None;
        }
        let date = e.lines().find_map(|l| l.trim().strip_prefix("Date:")).map(|d| d.trim().to_string()).unwrap_or_else(|| "a recent day".into());
        Some(date)
    })
}

fn verdicts() -> std::collections::BTreeMap<String, String> {
    std::fs::read(cache_dir().join("verdicts.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Did this model and size lose to the processor before, with this engine?
pub fn lost_before(model: &Path, shapes: &[(String, Vec<i64>)]) -> bool {
    verdicts().get(&verdict_key(model, shapes)).is_some_and(|v| v == "processor")
}

/// Remember which won.
pub fn remember(model: &Path, shapes: &[(String, Vec<i64>)], npu_won: bool) {
    let mut v = verdicts();
    v.insert(verdict_key(model, shapes), if npu_won { "npu" } else { "processor" }.into());
    crate::heard!(std::fs::create_dir_all(cache_dir()));
    crate::kept!(crate::store::write_json(&cache_dir().join("verdicts.json"), &v));
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
    /// Current actual backend, including a switch after isolated failure.
    pub fn on(&self) -> Where { if self.on.load(std::sync::atomic::Ordering::Acquire) == 1 { Where::Npu } else { Where::Cpu } }
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
        Session::open_with(root, model, shapes, want, false)
    }

    /// `open`, and with `quiet` the processor side kept to one thread that
    /// sleeps rather than spins between runs. For a model run many times a
    /// second (the hands, 2 Oct 2026): ONNX Runtime's default is a thread
    /// per core spinning while it waits, which is the fan noise this was
    /// moved to the NPU to stop.
    #[allow(clippy::result_large_err, reason = "the ONNX runtime's own error type; built once at start-up")]
    pub fn open_with(root: &Path, model: &Path, shapes: &[(String, Vec<i64>)], want: Where, quiet: bool) -> Result<Session, String> {
        if want == Where::Npu && !IN_WORKER.load(std::sync::atomic::Ordering::Relaxed) && npu_ready(root) {
            match worker::Client::open(root, model, shapes, quiet) {
                Ok(remote) => return Ok(Session { s: None, remote: Some(Mutex::new(remote)),
                    fallback: Some((root.into(), model.into(), shapes.to_vec(), quiet)),
                    cpu_fallback: Mutex::new(None), on: std::sync::atomic::AtomicU8::new(1) }),
                Err(why) => crate::outln!("NPU helper unavailable: {why}; using the processor"),
            }
        }
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
        if quiet {
            b = b
                .with_intra_threads(1)
                .and_then(|b| b.with_intra_op_spinning(false))
                .and_then(|b| b.with_inter_op_spinning(false))
                .map_err(|e| e.to_string())?;
        }
        let mut on = Where::Cpu;
        let mut native_attempt = None;
        if want == Where::Npu && e.npu {
            native_attempt = child_attempt(&verdict_key(model, shapes))?;
            let cache = cache_dir();
            crate::heard!(std::fs::create_dir_all(&cache));
            let opts = vec![
                (format!("{PROVIDER}.cache_dir"), cache.display().to_string()),
                (format!("{PROVIDER}.reshape_input"), reshape_value(shapes)),
            ];
            b = b.with_devices(npu_devices(&e.env), Some(&opts)).map_err(|e| format!("the NPU wouldn't take the model: {e}"))?;
            on = Where::Npu;
        }
        // On the NPU, a copy of the model with the sizes written into the
        // file itself (`onnxfix`): measured on the laptop, the NPU compiler
        // ignored both the provider's reshape and the runtime's overrides.
        let file = if on == Where::Npu { fixed_copy(model, shapes).unwrap_or_else(|| model.to_path_buf()) } else { model.to_path_buf() };
        // The NPU compile is where Atlas crashed (`crashed_atlas`): a marker
        // while it runs, so a process that dies in it turns the NPU off for
        // the next start instead of crashing again.
        let s = b.commit_from_file(&file).map_err(|e| format!("couldn't open {} on {}: {e}", model.display(), on.plain()));
        drop(native_attempt);
        let s = s?;
        Ok(Session { s: Some(Mutex::new(s)), remote: None, fallback: None, cpu_fallback: Mutex::new(None), on: std::sync::atomic::AtomicU8::new(if on == Where::Npu { 1 } else { 0 }) })
    }

    /// Run, timed: the values and how long the model took.
    pub fn run_timed(&self, inputs: Vec<In>) -> Result<(Vec<Vec<f32>>, std::time::Duration), String> {
        let t = std::time::Instant::now();
        let out = self.run(inputs)?;
        Ok((out, t.elapsed()))
    }

    /// Run, returning each output's values as f32, in the model's order.
    pub fn run(&self, inputs: Vec<In>) -> Result<Vec<Vec<f32>>, String> {
        worker::validate_inputs(&inputs)?;
        if let Some(remote) = &self.remote {
            let mut remote = remote.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if self.on() == Where::Npu {
                match remote.run(inputs.clone()) { Ok(output) => return Ok(output), Err(why) => {
                    self.on.store(0, std::sync::atomic::Ordering::Release); crate::outln!("NPU helper stopped: {why}; using the processor");
                } }
            }
            drop(remote);
            let (root, model, shapes, quiet) = self.fallback.as_ref().ok_or("processor fallback is missing")?;
            let mut fallback = self.cpu_fallback.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if fallback.is_none() { *fallback = Some(Box::new(Session::open_with(root, model, shapes, Where::Cpu, *quiet)?)); }
            return fallback.as_ref().ok_or("processor fallback is unavailable")?.run(inputs);
        }
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
        let mut s = self.s.as_ref().ok_or("model session is unavailable")?.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let out = s.run(values).map_err(|e| format!("the model failed on {}: {e}", self.on().plain()))?;
        let mut all = Vec::new();
        for (_, v) in out.iter() {
            let (_, data) = v.try_extract_tensor::<f32>().map_err(|e| e.to_string())?;
            if data.len() > worker::ELEMENTS || all.iter().map(Vec::len).sum::<usize>().saturating_add(data.len()) > worker::ELEMENTS || data.iter().any(|value| !value.is_finite()) { return Err("model output exceeds the isolated inference budget or contains nonfinite values".into()); }
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
    match why_not_on_npu(root) {
        None => out.push("NPU engine: ready".into()),
        Some(why) => out.push(format!("NPU engine: not in use -- {why}")),
    }
    // Search. The meaning model runs through ONNX Runtime, which the phone
    // builds leave out (`--no-default-features`, 2 Oct 2026: run 12 of the
    // iPhone build stopped here).
    #[cfg(not(feature = "onnx"))]
    out.push("Search: this build has no meaning model (no ONNX Runtime)".into());
    #[cfg(feature = "onnx")]
    match crate::meaningnative::Native::load(root) {
        None => out.push("Search: the meaning model isn't downloaded, so there's nothing to measure".into()),
        Some(mut enc) => {
            let (mut cpu_ms, mut npu_ms, mut worst) = (0u128, 0u128, 1.0f32);
            // Every text once first: each length compiles on its first use
            // (and fills the cache) and is checked against the processor.
            let t = std::time::Instant::now();
            for text in CHECK_TEXTS {
                // unheard-ok: returns `Option<Vec<f32>>`, not a Result
                let _ = enc.embed_on_processor(text);
                // unheard-ok: returns `Option<Vec<f32>>`, not a Result
                let _ = enc.embed(text);
            }
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
            let lengths: Vec<String> = enc
                .npu_lengths()
                .iter()
                .map(|(len, on)| format!("{len} words: {}", if *on { "NPU" } else { "processor" }))
                .collect();
            out.push(format!(
                "Search: {} ms a sentence on the processor, {} ms the way Atlas now runs it (setting up: {first} ms); \
                 answers agree to {:.4} at worst. {}",
                cpu_ms / n,
                npu_ms / n,
                worst,
                if lengths.is_empty() { "Nothing tried on the NPU.".to_string() } else { lengths.join(", ") }
            ));
        }
    }
    // Telling voices apart, on three seconds of a test tone.
    let models = root.join("models");
    if crate::speakernet::installed(&models) {
        let samples: Vec<f32> = (0..48_000).map(|i| ((i as f32) * 0.05).sin() * 0.3 + ((i as f32) * 0.013).sin() * 0.2).collect();
        crate::heard!(crate::speakernet::embed_on_processor(&samples, &models));
        let t = std::time::Instant::now();
        let a = crate::speakernet::embed_on_processor(&samples, &models);
        let cpu = t.elapsed().as_millis();
        crate::heard!(crate::speakernet::embed(&samples, &models));
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

#[cfg(test)]
mod a_crash_turns_it_off {
    use super::*;

    #[test]
    #[ignore = "owned child of abrupt_native_exit_is_quarantined_on_the_next_start"]
    fn native_exit_child() {
        let marker = PathBuf::from(std::env::var_os("ATLAS_NPU_EXIT_PROOF_MARKER").expect("owned marker"));
        assert!(marker.starts_with(std::env::temp_dir()));
        assert_eq!(marker.file_name().unwrap(), COMPILING);
        let _attempt = NativeAttempt::begin(marker, "owned native stage").unwrap();
        // A process exit skips Drop, just as a native access violation does.
        std::process::exit(37);
    }

    #[test]
    fn abrupt_native_exit_is_quarantined_on_the_next_start() {
        let cache = std::env::temp_dir().join(format!("atlas-npu-exit-proof-{}", std::process::id()));
        std::fs::create_dir_all(&cache).unwrap();
        let marker = cache.join(COMPILING);
        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["npu::a_crash_turns_it_off::native_exit_child", "--exact", "--ignored", "--test-threads=1"])
            .env("ATLAS_NPU_EXIT_PROOF_MARKER", &marker)
            .stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn().unwrap();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        let status = loop {
            if let Some(status) = child.try_wait().unwrap() { break status; }
            if std::time::Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("owned native exit proof did not finish");
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        };
        assert_eq!(status.code(), Some(37));
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "owned native stage");
        let touched = std::cell::Cell::new(false);
        let (plugin, why) = guarded_provider(|| crashed_atlas_in(&Default::default(), "exit-proof", &cache, || None), || {
            touched.set(true);
            Some(())
        });
        assert!(plugin.is_none() && !touched.get());
        assert!(why.unwrap().contains("owned native stage"));
        assert!(!marker.exists());
        let saved = serde_json::from_slice(&std::fs::read(cache.join("verdicts.json")).unwrap()).unwrap();
        assert!(crashed_atlas_in(&saved, "exit-proof", &cache, || None).is_some());
        std::fs::remove_file(cache.join("verdicts.json")).unwrap();
        std::fs::remove_dir(cache).unwrap();
    }

    #[test]
    fn a_quarantined_provider_is_never_loaded_into_atlas() {
        let touched = std::cell::Cell::new(false);
        let (plugin, why) = guarded_provider(|| Some("known compiler crash".into()), || {
            touched.set(true);
            Some(())
        });
        assert!(!touched.get(), "the crash guard must run before touching the native provider");
        assert!(plugin.is_none());
        assert_eq!(why.as_deref(), Some("known compiler crash"));
    }

    #[test]
    fn a_provider_without_a_crash_is_allowed() {
        let (plugin, why) = guarded_provider(|| None, || Some(7));
        assert_eq!(plugin, Some(7));
        assert!(why.is_none());
    }

    #[test]
    fn native_work_requires_a_durable_marker_and_cleans_up_returned_errors() {
        let root = std::env::temp_dir().join(format!("atlas-npu-attempt-{}", std::process::id()));
        std::fs::create_dir_all(&root).unwrap();
        let marker = root.join("marker");
        std::fs::write(&marker, "an unfinished earlier attempt").unwrap();
        assert!(NativeAttempt::begin(marker.clone(), "new work").is_err());
        assert_eq!(std::fs::read_to_string(&marker).unwrap(), "an unfinished earlier attempt");
        std::fs::remove_file(&marker).unwrap();
        {
            let _attempt = NativeAttempt::begin(marker.clone(), "loading provider").unwrap();
            assert_eq!(std::fs::read_to_string(&marker).unwrap(), "loading provider");
        }
        assert!(!marker.exists());
        std::fs::remove_dir(&root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    #[ignore = "run alone with ATLAS_ORT_TEST_ROOT for the laptop's actual runtime"]
    fn actual_runtime_keeps_a_quarantined_native_provider_unloaded() {
        use windows::core::w;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        let root = PathBuf::from(std::env::var_os("ATLAS_ORT_TEST_ROOT").expect("explicit runtime root"));
        assert!(root.join(PLUGIN).is_file(), "this proof requires the actual provider to be present");
        let mut v = verdicts();
        v.insert(crash_key(&engine_version()), "quarantined for the safe-start proof".into());
        crate::store::write_json(&cache_dir().join("verdicts.json"), &v).unwrap();
        assert!(runtime_ready(&root), "the processor runtime must remain available");
        assert!(!npu_ready(&root));
        assert!(why_not_on_npu(&root).unwrap().contains("quarantined"));
        let model = root.join(crate::meaningnative::MODEL);
        let vocabulary = std::fs::read_to_string(root.join(crate::meaningnative::VOCAB)).expect("installed meaning vocabulary");
        let token = |word: &str| vocabulary.lines().position(|line| line == word).expect("meaning token") as i64;
        let names = Session::input_names(&root, &model).expect("actual processor model inputs");
        let shapes = names.iter().map(|name| (name.clone(), vec![1, 16])).collect::<Vec<_>>();
        let session = Session::open_with(&root, &model, &shapes, Where::Npu, true).expect("quarantined NPU must fall back to the processor");
        assert_eq!(session.on(), Where::Cpu);
        let inputs = || names.iter().map(|name| {
            let mut values = vec![0; 16];
            if name.contains("input_ids") { values[..3].copy_from_slice(&[token("[CLS]"), token("hello"), token("[SEP]")]); }
            else if name.contains("attention_mask") { values[..3].fill(1); }
            else { assert!(name.contains("token_type"), "unexpected installed model input: {name}"); }
            In::I64(name.clone(), vec![1, 16], values)
        }).collect();
        let first = session.run(inputs()).expect("actual fallback inference");
        let again = session.run(inputs()).expect("repeat actual fallback inference");
        assert!(!first.is_empty() && first.iter().all(|values| !values.is_empty() && values.iter().all(|value| value.is_finite())));
        assert!(first.iter().flatten().any(|value| value.abs() > 0.001));
        assert_eq!(first, again, "fallback inference must remain deterministic on the same input");
        assert!(unsafe { GetModuleHandleW(w!("onnxruntime_providers_openvino_plugin.dll")) }.is_err(), "a quarantined DLL must never enter Atlas's process");
        assert!(unsafe { GetModuleHandleW(w!("openvino_intel_npu_compiler.dll")) }.is_err());
    }

    #[test]
    fn windows_record_of_the_compiler_crash_is_read() {
        let log = "Event[0]:\r\n  Log Name: Application\r\n  Date: 2026-10-06T07:34:49.000\r\n  Event ID: 1000\r\n  Description: \r\nFaulting application name: atlas.exe, version: 0.1.0.0\r\nFaulting module name: openvino_intel_npu_compiler.dll, version: 2026.3.0.1\r\nException code: 0xc0000005\r\n\r\nEvent[1]:\r\n  Date: 2026-10-05T01:00:00.000\r\nFaulting application name: chrome.exe\r\n";
        assert_eq!(crash_in_compiler(log).as_deref(), Some("2026-10-06T07:34:49.000"));
        assert_eq!(crash_in_compiler("Event[0]:\r\nFaulting application name: atlas.exe\r\nFaulting module name: ntdll.dll\r\n"), None);
        assert_eq!(crash_in_compiler(""), None);
    }

    #[test]
    fn a_compile_that_never_finished_turns_the_npu_off_and_it_stays_off() {
        // `cache_dir` is under the test process's own data folder.
        let _ = std::fs::create_dir_all(cache_dir());
        std::fs::write(cache_dir().join(COMPILING), "encoder.onnx|ids[1,16]|abc").unwrap();
        let why = crashed_atlas(&Default::default(), "testversion").expect("off");
        assert!(why.contains("encoder.onnx"), "{why}");
        assert!(!cache_dir().join(COMPILING).exists(), "the marker is spent");
        // Kept: the next start reads the verdict, with no marker left.
        assert!(crashed_atlas(&verdicts(), "testversion").is_some());
        // A new engine version is tried afresh.
        let mut v = verdicts();
        v.remove(&crash_key("testversion"));
        if !cfg!(windows) {
            assert!(crashed_atlas(&v, "newversion").is_none());
        }
    }
}
