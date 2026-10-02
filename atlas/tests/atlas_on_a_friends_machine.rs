//! Atlas on somebody else's machine.
//!
//! 27 Sep 2026: Atlas started going to Eric's friends, to run on their own
//! computers. Three things that only ever worked because the machine was
//! Eric's laptop, set up by hand:
//!
//! - the language model's server was never started by Atlas — on the laptop
//!   it had been started by hand, so nothing noticed. When Atlas did try (only
//!   when asked "which model"), the helpers' 600 MB budget refused it, and the
//!   shipped setting named a `llama-server` a fresh Windows doesn't have on
//!   its PATH;
//! - everyone was called Eric, and a guest at Eric's own laptop was too;
//! - `atlas doctor` said nothing about the language model at all.
//!
//! The install (`ATLAS_HOME`) is process-wide, so these run in their own test
//! target, one at a time.

mod common; // `common::source_of`: a module's source wherever its files live

use atlas::brain::Llm;
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, Once};

const NOW: u64 = 1_700_000_000;

fn home() -> PathBuf {
    std::env::temp_dir().join(format!("atlas-friend-{}", std::process::id()))
}

/// One install, one test at a time, handed back before each.
fn alone() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    static ONCE: Once = Once::new();
    let g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    ONCE.call_once(|| {
        let p = home();
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(p.join("data").join("state")).unwrap();
        std::env::set_var("ATLAS_HOME", &p);
    });
    atlas::handover::Handover::default().save(&atlas::roots::install_state()).unwrap();
    g
}

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-friend-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn shipped() -> Config {
    Config::load(Path::new("config")).unwrap()
}

/// A model that remembers every system prompt it was given.
#[derive(Default)]
struct Heard(Mutex<Vec<String>>);

impl Llm for Heard {
    fn complete(&self, system: &str, _user: &str) -> atlas::error::Result<String> {
        self.0.lock().unwrap().push(system.to_string());
        Ok("Here's one: I told my computer a joke. It didn't laugh; it just processed it.".into())
    }
}

// ---------------------------------------------------------------- names

#[test]
fn the_shipped_settings_call_nobody_by_name() {
    let c = shipped();
    assert_eq!(
        c.tools.as_ref().unwrap().persona.address,
        "",
        "the shipped persona calls whoever installs it by a name; a friend's Atlas called them Eric"
    );
}

#[test]
fn a_guest_is_not_called_by_the_owners_name() {
    let _g = alone();
    // Eric's own settings, where his name is written in.
    let mut c = shipped();
    c.tools.as_mut().unwrap().persona.address = "Eric".into();
    let p = plat();
    let heard = Arc::new(Heard::default());
    let mut d = Daemon::new(&c, &p, Some(heard.clone() as Arc<dyn Llm>), Store::new(tmp("guest")), Proactive::new(ProactiveConfig::default()));

    let _ = d.turn("tell me a joke", NOW);
    let owner: Vec<String> = heard.0.lock().unwrap().drain(..).collect();
    assert!(!owner.is_empty(), "the question never reached the model");
    assert!(owner.iter().any(|s| s.contains("Eric")), "the owner isn't called by his name: {owner:?}");

    let _ = d.turn("you're talking to someone else now", NOW + 5);
    assert!(atlas::handover::Handover::load(&atlas::roots::install_state()).stance.handed_over());
    let _ = d.turn("tell me a joke", NOW + 10);
    let guest: Vec<String> = heard.0.lock().unwrap().drain(..).collect();
    assert!(!guest.is_empty(), "the guest's question never reached the model");
    assert!(
        guest.iter().all(|s| !s.contains("Eric")),
        "the guest was addressed by the owner's name: {:?}",
        guest.iter().find(|s| s.contains("Eric"))
    );
}

#[test]
fn stop_calling_me_that_reaches_the_model_too() {
    let _g = alone();
    let mut c = shipped();
    c.tools.as_mut().unwrap().persona.address = "Eric".into();
    let p = plat();
    let heard = Arc::new(Heard::default());
    let mut d = Daemon::new(&c, &p, Some(heard.clone() as Arc<dyn Llm>), Store::new(tmp("stop")), Proactive::new(ProactiveConfig::default()));

    let _ = d.turn("stop calling me that", NOW);
    heard.0.lock().unwrap().clear();
    let _ = d.turn("tell me a joke", NOW + 5);
    let said: Vec<String> = heard.0.lock().unwrap().drain(..).collect();
    assert!(!said.is_empty());
    assert!(
        said.iter().all(|s| !s.contains("as Eric")),
        "\"stop calling me that\" was undone by the settings' name in every reply"
    );
}

#[test]
fn the_first_run_says_how_to_be_called_something() {
    let src = std::fs::read_to_string("src/firstrun.rs").unwrap();
    assert!(src.contains("Say \\\"call me\\\" and your name"), "first run no longer says how to set a name");
}

#[test]
fn nothing_atlas_says_or_tells_the_model_is_eric_s() {
    // What the model is told when it writes as you, and what a friend's
    // Atlas says when updates stop: neither is Eric's any more.
    let job = atlas::delegate::Delegation::new("Messages", "tell Sam I'm running late", atlas::delegate::Reach::Draft, 3);
    let told = job.system_prompt();
    assert!(told.contains("tell Sam I'm running late"), "the request isn't in it: {told}");
    assert_eq!(told.matches("Eric").count(), 0, "a friend's Atlas writes as Eric: {told}");
    let said = atlas::release::Freshness::Overdue { days: 3 }.plain().unwrap();
    assert!(said.contains("3 days"), "{said}");
    assert_eq!(said.matches("Eric").count(), 0, "a friend's Atlas sends them to Eric: {said}");
}

// ---------------------------------------------------------------- the model server

#[test]
fn the_model_server_is_found_where_atlas_get_puts_it() {
    let _g = alone();
    let dir = home().join("tools").join("llama");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join(if cfg!(windows) { "llama-server.exe" } else { "llama-server" });
    let mut cfg = shipped().tools.unwrap().models;

    let _ = std::fs::remove_file(&exe);
    assert_eq!(atlas::models::server_tool(&cfg).unwrap().command, "llama-server", "none downloaded: the setting as written");

    std::fs::write(&exe, b"").unwrap();
    assert_eq!(
        PathBuf::from(atlas::models::server_tool(&cfg).unwrap().command),
        exe,
        "the one `atlas get pictures` fetched isn't used"
    );

    cfg.server.as_mut().unwrap().command = "/opt/mine/llama-server".into();
    assert_eq!(atlas::models::server_tool(&cfg).unwrap().command, "/opt/mine/llama-server", "a path you wrote is yours");
    let _ = std::fs::remove_file(&exe);
}

#[test]
fn the_model_server_stays_warm_through_a_conversation() {
    let shipped_keep = shipped().tools.unwrap().lifecycle.keep_warm_secs.get("model-server").copied();
    let default_keep = atlas::lifecycle::LifecycleConfig::default().keep_warm_secs.get("model-server").copied();
    for (what, v) in [("shipped", shipped_keep), ("default", default_keep)] {
        assert!(
            v.unwrap_or(0) >= 600,
            "{what}: a model takes seconds to minutes to load; 90 s idle meant reloading it between questions"
        );
    }
}

#[test]
fn every_door_with_a_model_starts_its_server() {
    let src = crate::common::source_of("main");
    // The typing prompt, `--daemon`, and `atlas voice`.
    assert_eq!(src.matches(".starting_the_model_server()").count(), 3);
}

/// Real bytes for a small model, so the registry picks it.
fn tiny_model() -> Vec<u8> {
    fn u32b(v: u32) -> Vec<u8> { v.to_le_bytes().to_vec() }
    fn u64b(v: u64) -> Vec<u8> { v.to_le_bytes().to_vec() }
    fn strb(s: &str) -> Vec<u8> {
        let mut o = u64b(s.len() as u64);
        o.extend_from_slice(s.as_bytes());
        o
    }
    let kv: Vec<(&str, Vec<u8>)> = vec![
        ("general.architecture", [u32b(8), strb("llama")].concat()),
        ("general.name", [u32b(8), strb("Tiny")].concat()),
        ("llama.context_length", [u32b(4), u32b(2048)].concat()),
        ("llama.block_count", [u32b(4), u32b(4)].concat()),
        ("llama.embedding_length", [u32b(4), u32b(256)].concat()),
        ("llama.attention.head_count", [u32b(4), u32b(4)].concat()),
        ("llama.attention.head_count_kv", [u32b(4), u32b(4)].concat()),
        ("tokenizer.chat_template", [u32b(8), strb("<|im_start|>system")].concat()),
    ];
    let tensors: Vec<Vec<u8>> = vec![[strb("token_embd.weight"), u32b(2), u64b(256), u64b(1000), u32b(12), u64b(0)].concat()];
    let mut o = b"GGUF".to_vec();
    o.extend(u32b(3));
    o.extend(u64b(tensors.len() as u64));
    o.extend(u64b(kv.len() as u64));
    for (k, v) in kv {
        o.extend(strb(k));
        o.extend(v);
    }
    for t in tensors {
        o.extend(t);
    }
    o
}

#[cfg(unix)]
#[test]
fn atlas_starts_its_own_model_server_and_keeps_one() {
    use std::os::unix::fs::PermissionsExt;
    let _g = alone();
    let h = home();
    std::fs::create_dir_all(h.join("models")).unwrap();
    std::fs::write(h.join("models").join("tiny.gguf"), tiny_model()).unwrap();
    // A stand-in for llama-server: says it was started, with what, and
    // stays up like the real one.
    let dir = h.join("tools").join("llama");
    std::fs::create_dir_all(&dir).unwrap();
    let exe = dir.join("llama-server");
    let marker = h.join("launched.txt");
    std::fs::write(&exe, format!("#!/bin/sh\necho \"$@\" >> '{}'\nexec sleep 30\n", marker.display())).unwrap();
    std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
    let _ = std::fs::remove_file(&marker);

    let mut c = shipped();
    {
        let t = c.tools.as_mut().unwrap();
        // Nothing listens here, so it isn't "already running".
        t.models.port = 18_979;
        assert!(t.llm.is_none(), "the shipped settings must use Atlas's own model");
    }
    let p = plat();
    let heard: Arc<dyn Llm> = Arc::new(Heard::default());

    // A test's Atlas, or any door that didn't ask, never starts one.
    {
        let mut quiet = Daemon::new(&c, &p, Some(heard.clone()), Store::new(tmp("quiet")), Proactive::new(ProactiveConfig::default()));
        let _ = quiet.turn("tell me a joke", NOW);
        std::thread::sleep(std::time::Duration::from_millis(500));
        assert!(!marker.exists(), "an Atlas that wasn't asked to started a model server");
    }

    let mut d = Daemon::new(&c, &p, Some(heard), Store::new(tmp("starts")), Proactive::new(ProactiveConfig::default()))
        .starting_the_model_server();
    let _ = d.turn("tell me a joke", NOW);
    let waited = std::time::Instant::now();
    while !marker.exists() && waited.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let args = std::fs::read_to_string(&marker).expect(
        "the model server was never started — refused by the helpers' budget, or not found in tools/llama",
    );
    assert!(args.contains("tiny.gguf"), "started without the model: {args}");
    assert!(args.contains("--port 18979"), "started on the wrong port: {args}");
    assert!(args.contains("--host 127.0.0.1"), "a model server must stay on this machine: {args}");
    assert!(d.helpers.is_running("model-server"), "started, but nothing owns it: it would outlive Atlas");

    // The next turn uses the one that's running.
    let _ = d.turn("and another", NOW + 20);
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 1, "a second server was started");

    // Let go after the quiet it's allowed, and started again when needed.
    // The quiet allowed is half an hour where the model doesn't fit and a
    // week where it does (Phase 0.2, `lifecycle::model_stays_when_it_fits`),
    // and which one depends on the machine the test runs on -- so past both.
    let later = NOW + 20 + atlas::lifecycle::MODEL_RESIDENT_SECS + 1;
    let stopped = d.helpers.reap(later);
    assert!(!stopped.is_empty() && !d.helpers.is_running("model-server"));
    let _ = d.turn("one more", later + 100);
    let waited = std::time::Instant::now();
    while std::fs::read_to_string(&marker).unwrap().lines().count() < 2 && waited.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert_eq!(std::fs::read_to_string(&marker).unwrap().lines().count(), 2, "not started again after being let go");
    drop(d);
    let _ = std::fs::remove_dir_all(h.join("models"));
}

// ---------------------------------------------------------------- doctor

#[test]
fn doctor_says_when_there_is_no_language_model() {
    let _g = alone();
    let _ = std::fs::remove_dir_all(home().join("models"));
    let c = shipped();
    let p = plat();
    let found = atlas::doctor::run(&c, c.tools.as_ref(), &p);
    let talking = found.iter().find(|f| f.label == "talking").expect("doctor says nothing about the language model");
    assert!(!talking.ok);
    assert!(talking.detail.contains("setup fetches it") && !talking.detail.contains("`atlas"), "doesn't say how to get one, or says it with a command: {}", talking.detail);
}

/// Eric, 27 Sep 2026: setup finished and Atlas could answer nothing, because
/// the language model was a separate command. Setup fetches it now, with
/// everything else.
#[test]
fn setup_fetches_the_language_model_itself() {
    let pieces = atlas::setupwin::setup_pieces();
    for p in atlas::getpieces::pictures() {
        assert!(pieces.iter().any(|q| q.url == p.url), "setup doesn't fetch {}", p.name);
    }
    assert!(pieces.iter().any(|p| p.name == "the language model" && p.for_what.contains("answering you")));
}
