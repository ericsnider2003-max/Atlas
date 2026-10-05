//! Atlas on a phone, standing alone.
//!
//! Eric's ruling: Atlas is a standalone on phones as well — not a window onto
//! the laptop. So the phone runs the same core (`--no-default-features`: no
//! desktop GUI stack, `platform::mobile`), and the phone app's screens are the
//! hub this core serves to the app's own WebView over loopback. The design's
//! phone artboards are those same pages at phone width (`hub::STYLE`'s phone
//! rules: the tab bar, one column, safe areas).
//!
//! This module is the one door the native shells (`mobile/ios`,
//! `mobile/android`) call through, as a C ABI they link against:
//!
//! - `atlas_mobile_start(home, port)` (`start_once`) starts Atlas in its own thread with its
//!   data under `home` (the app's private folder), serving the hub on
//!   127.0.0.1 only. It never blocks for long: 0 if the hub is answering,
//!   2 if it's still starting (poll `atlas_mobile_url`), 1 if it was
//!   already running. Only one Atlas is ever started: a second call while
//!   the first is starting says "starting" rather than starting another.
//! - `atlas_mobile_url(buf, len)` writes the hub's address, token included,
//!   for the WebView to open; -1 until it's answering.
//! - `atlas_mobile_state()` says where it is: 0 not started, 1 starting,
//!   2 running, -1 it couldn't start.
//! - `atlas_mobile_stop()` asks it to stop; the thread finishes its turn.
//!
//! Nothing here opens a port anyone else can reach: loopback, and the same
//! token the laptop's hub uses.

use crate::config::Config;
use crate::daemon::Daemon;
use crate::proactive::Proactive;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};


/// Where the phone's Atlas is. One of these at a time, so one Atlas.
///
/// Until 28 Sep 2026 the start waited up to 20 s for the hub -- on the phone
/// app's main thread -- and on a slow start gave up with the serving thread
/// still running but no address kept, so the next start began a second
/// Atlas beside it. Now the serving thread writes its own address when it's
/// answering, and a start while one is on its way says so.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Phase {
    Stopped,
    Starting,
    Running(String),
    Failed(String),
}

static PHASE: Mutex<Phase> = Mutex::new(Phase::Stopped);
/// Which start the serving thread belongs to, so an old thread finishing
/// after a restart doesn't overwrite the new one's state.
static GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn phase_now() -> Phase {
    PHASE.lock().or_else(crate::crash::unpoison).map(|p| p.clone()).unwrap_or(Phase::Stopped)
}

fn set_phase(generation: u64, p: Phase) {
    if GENERATION.load(Ordering::SeqCst) != generation {
        return;
    }
    if let Ok(mut now) = PHASE.lock().or_else(crate::crash::unpoison) {
        *now = p;
    }
}

/// Where the phone's Atlas is now.
pub fn phase() -> Phase {
    phase_now()
}

/// How many times an Atlas has been started in this process: a test, and
/// the log, can see that a second start didn't start a second one.
pub fn starts() -> u64 {
    GENERATION.load(Ordering::SeqCst)
}

/// What `start` found or did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Started {
    /// It's answering, at this address.
    Running(String),
    /// It was already answering before this call.
    AlreadyRunning(String),
    /// On its way: ask again (`phase`) for the address.
    Starting,
    /// It couldn't start, and why.
    Failed(String),
}

/// Start Atlas here, once. Waits at most `wait` for the hub to answer; the
/// app's main thread passes a short wait and polls. A call while an earlier
/// start is still on its way never starts a second Atlas.
pub fn start_once(home: std::path::PathBuf, port: u16, wait: std::time::Duration) -> Started {
    let generation = {
        let Ok(mut p) = PHASE.lock().or_else(crate::crash::unpoison) else { return Started::Failed("Atlas's own state couldn't be read".into()) };
        match &*p {
            Phase::Running(url) if !stopping() => return Started::AlreadyRunning(url.clone()),
            Phase::Starting => return Started::Starting,
            _ => {}
        }
        *p = Phase::Starting;
        GENERATION.fetch_add(1, Ordering::SeqCst) + 1
    };
    // A fresh flag for this Atlas: one being stopped keeps its own.
    let stop = Arc::new(AtomicBool::new(false));
    if let Some(slot) = STOP_NOW.lock().or_else(crate::crash::unpoison).ok().as_mut() {
        **slot = Some(stop.clone());
    }
    std::thread::spawn(move || {
        let r = serve(&home, port, stop, |url| set_phase(generation, Phase::Running(url)));
        set_phase(generation, match r {
            Ok(()) => Phase::Stopped,
            Err(why) => {
                crate::errln!("atlas: {why}");
                Phase::Failed(why)
            }
        });
    });
    let until = std::time::Instant::now() + wait;
    loop {
        match phase_now() {
            Phase::Running(url) => return Started::Running(url),
            Phase::Failed(why) => return Started::Failed(why),
            Phase::Stopped => return Started::Failed("it stopped as it started".into()),
            Phase::Starting if std::time::Instant::now() >= until => return Started::Starting,
            Phase::Starting => std::thread::sleep(std::time::Duration::from_millis(20)),
        }
    }
}

/// The stop flag of the Atlas started last.
static STOP_NOW: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);

/// Ask the phone's Atlas to stop. It finishes the turn it's on.
pub fn stop_now() {
    if let Ok(s) = STOP_NOW.lock().or_else(crate::crash::unpoison) {
        if let Some(f) = s.as_ref() {
            f.store(true, Ordering::SeqCst);
        }
    }
}

/// Has the Atlas started last been asked to stop (and not finished yet)?
fn stopping() -> bool {
    STOP_NOW.lock().or_else(crate::crash::unpoison).ok().and_then(|s| s.as_ref().map(|f| f.load(Ordering::SeqCst))).unwrap_or(false)
}
/// Whether the phone is on wifi (or another network that isn't metered), as
/// the app last said through `atlas_mobile_network`.
static UNMETERED: AtomicBool = AtomicBool::new(false);


/// Run Atlas here until `stop` is set, serving the hub on loopback at `port`
/// (0: any free port). `ready` gets the hub's address once it's answering.
/// Config comes from `home/config` (the app ships a copy of the default
/// config there on first run).
pub fn serve(home: &std::path::Path, port: u16, stop: Arc<AtomicBool>, ready: impl FnOnce(String)) -> Result<(), String> {
    std::env::set_var("ATLAS_HOME", home);
    crate::phonemode::switch_on();
    let cfg_dir = crate::roots::config_dir();
    let mut cfg = Config::load(&cfg_dir).map_err(|e| format!("couldn't read the config in {}: {e}", cfg_dir.display()))?;
    if let Some(t) = cfg.tools.as_mut() {
        crate::phonemode::as_a_phone(t);
    }
    let cfg: &'static Config = Box::leak(Box::new(cfg));
    let tools = cfg.tools.clone().unwrap_or_default();
    let plat: &'static crate::platform::mobile::MobilePlatform = Box::leak(Box::new(crate::platform::mobile::MobilePlatform));
    let store = crate::roots::store();
    // Your yes to the free online models, from before (`phonemode`).
    crate::phonemode::set_online_ok(store.load::<bool>(crate::phonemode::ONLINE_ASKED));
    let mut d = Daemon::new(cfg, plat, phone_llm(&tools), store, Proactive::new(tools.proactive.clone()));
    let token = crate::server::token_for(&d.store).map_err(|e| e.to_string())?;
    let mut scfg = tools.server.clone();
    scfg.enabled = true;
    scfg.port = port;
    // Loopback only, whatever the laptop's config says about reaching it.
    scfg.reachable_from = String::new();
    let server = crate::server::Server::bind(&scfg, &token)
        .and_then(|s| s.threaded())
        .map_err(|e| e.to_string())?;
    ready(crate::server::hub_url(server.port(), &token, "/hub"));
    #[cfg(feature = "phone-llm")]
    let models_dir = crate::models::Registry::dir_for(&tools.models);
    #[cfg(feature = "phone-llm")]
    let mut last_try = 0u64;
    let mut last_tick = 0u64;
    while !stop.load(Ordering::Relaxed) {
        // Every request waiting, answered as soon as it arrives; the 30ms
        // is the longest this waits before looking at the clock again.
        let _ = server.wait_and_answer(30, &mut |action| {
            crate::crash::caught("answering the hub", || crate::hublive::reply(&mut d, action)).unwrap_or_else(|why| {
                crate::server::Reply { status: 500, body: serde_json::json!({ "error": why }).to_string(), ..Default::default() }
            })
        });
        let now = crate::store::now();
        if now != last_tick {
            // Once a second: reminders, held messages going out, the brief.
            // What it says is kept for the app to show (30 Sep 2026: it was
            // dropped here, so reminders never appeared on the phone).
            let said = d.tick(now);
            d.keep_said_for_apps(said);
            last_tick = now;
            // The phone's own model, fetched by itself on wifi.
            // Only once you've asked for it: it's 0.6-1.8 GB (`phonemode`).
            #[cfg(feature = "phone-llm")]
            if crate::phonemodel::fetch_by_itself(
                UNMETERED.load(Ordering::Relaxed),
                crate::phonemodel::present(&models_dir).is_some(),
                crate::phonemodel::download_state().as_ref(),
                now,
                last_try,
            ) && d.store.load::<bool>(crate::phonemode::MODEL_ASKED_FOR)
            {
                last_try = now;
                let _ = crate::phonemodel::start_download(models_dir.clone(), |path| {
                    let _ = crate::phonemodel::attach(&path);
                });
            }
        }
    }
    Ok(())
}

/// The phone's language model (OPEN_GAPS P.7). Until 27 Sep 2026 the phone
/// core was started with none at all -- the connection was built only in the
/// desktop program's `main` -- so nothing it said came from a model, not even
/// the laptop's over Tailscale. Now: the model inside the app when the phone
/// build has one (`phonemodel::PhoneLlm`, loaded from the models folder if
/// it's there), with the connection you've set yourself (`tools.llm`, say your
/// laptop's model) behind it; or just that connection.
fn phone_llm(tools: &crate::voice::ToolsConfig) -> Option<std::sync::Arc<dyn crate::brain::Llm>> {
    let configured = crate::models::connection(tools);
    #[cfg(feature = "phone-llm")]
    {
        let dir = crate::models::Registry::dir_for(&tools.models);
        if let Some((path, _)) = crate::phonemodel::present(&dir) {
            std::thread::spawn(move || {
                if let Err(why) = crate::phonemodel::attach(&path) {
                    crate::errln!("atlas: the phone's model didn't load: {why}");
                }
            });
        }
        let own: std::sync::Arc<dyn crate::brain::Llm> = std::sync::Arc::new(crate::phonemodel::PhoneLlm);
        let atlas: std::sync::Arc<dyn crate::brain::Llm> = std::sync::Arc::new(crate::brain::FallbackLlm::new(own, configured));
        // Apple's model first on an iPhone that has it, Atlas's own for each
        // request it can't do (decision 2, `applebrain`). Elsewhere nothing
        // registers Apple's, and every request goes straight to Atlas's.
        return Some(std::sync::Arc::new(crate::applebrain::AppleFirst::new(atlas)));
    }
    #[allow(unreachable_code)]
    configured.map(|c| std::sync::Arc::new(crate::applebrain::AppleFirst::new(c)) as std::sync::Arc<dyn crate::brain::Llm>)
}

/// Start Atlas on the phone. `home`: a NUL-terminated UTF-8 path to the app's
/// private folder. Never blocks for more than a moment, so it's safe from the
/// app's main thread. Returns 0 once the hub is answering, 1 if it was
/// already running, 2 if it's still starting (poll `atlas_mobile_url` or
/// `atlas_mobile_state`), and a negative number (with the reason in the log)
/// if it couldn't start.
///
/// # Safety
/// `home` must be a valid NUL-terminated string for the length of the call.
#[no_mangle]
pub unsafe extern "C" fn atlas_mobile_start(home: *const std::os::raw::c_char, port: u16) -> i32 {
    if home.is_null() {
        return -1;
    }
    let Ok(home) = std::ffi::CStr::from_ptr(home).to_str().map(|s| std::path::PathBuf::from(s)) else { return -2 };
    let before = crate::mobile::starts();
    match crate::mobile::start_once(home, port, std::time::Duration::from_millis(START_WAIT_MS)) {
        Started::Running(_) => 0,
        Started::AlreadyRunning(_) => 1,
        Started::Starting => {
            if crate::mobile::starts() == before {
                crate::errln!("atlas: already starting (start {before}); not starting another");
            }
            2
        }
        Started::Failed(_) => -3,
    }
}

/// How long `atlas_mobile_start` waits before saying "starting": short
/// enough never to freeze the app's screen.
pub const START_WAIT_MS: u64 = 300;

/// Where the phone's Atlas is: 0 not started (or stopped), 1 starting,
/// 2 running, -1 it couldn't start.
#[no_mangle]
pub extern "C" fn atlas_mobile_state() -> i32 {
    match crate::mobile::phase() {
        Phase::Stopped => 0,
        Phase::Starting => 1,
        Phase::Running(_) => 2,
        Phase::Failed(_) => -1,
    }
}

/// Write the hub's address into `buf` (NUL-terminated). Returns the length
/// written, or -1 if Atlas isn't answering yet or `buf` is too small.
///
/// # Safety
/// `buf` must point to `len` writable bytes.
#[no_mangle]
pub unsafe extern "C" fn atlas_mobile_url(buf: *mut std::os::raw::c_char, len: usize) -> i32 {
    let url = match crate::mobile::phase() {
        Phase::Running(u) => u,
        _ => String::new(),
    };
    if url.is_empty() || buf.is_null() || url.len() + 1 > len {
        return -1;
    }
    std::ptr::copy_nonoverlapping(url.as_ptr(), buf as *mut u8, url.len());
    *buf.add(url.len()) = 0;
    url.len() as i32
}

/// Ask Atlas on the phone to stop. It finishes the turn it's on.
/// The app says whether the phone is on wifi (or another network that isn't
/// metered): 1 yes, 0 no. Called at start and whenever it changes. The
/// phone's own model is fetched by itself only while this is 1.
#[no_mangle]
pub extern "C" fn atlas_mobile_network(unmetered: i32) {
    UNMETERED.store(unmetered != 0, Ordering::Relaxed);
}

#[no_mangle]
pub extern "C" fn atlas_mobile_stop() {
    crate::mobile::stop_now();
}

// Tested in `tests/the_phone_stands_alone.rs`, in its own process: `serve`
// points ATLAS_HOME at the app's folder, which is right for the phone app
// (Atlas is the only thing in it) and wrong for a test binary shared with
// hundreds of other tests.
