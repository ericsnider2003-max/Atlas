//! 12 Oct 2026, Eric's laptop (integrated Intel graphics, Vulkan llama.cpp):
//! the model server's log said `vk::Queue::submit: ErrorDeviceLost` while
//! reading a prompt of 1,000-2,000 tokens. The server did not exit, `/health`
//! kept saying ok, and every later request failed -- so Atlas trusted a
//! running process and turns waited on it ("replies of 12-40 s, jobs never
//! finish").
//!
//! What is held here:
//! - the reset is recognised in the server's own words;
//! - a reset is remembered, and while it is, a start uses settings that do
//!   not use the graphics at all -- shown against a stand-in server program
//!   that dies with the device-lost text unless it is started that way;
//! - `atlas doctor` says so, with how to try again;
//! - a turn asked while the server can't answer is refused at once, so the
//!   rules-only path answers, rather than waiting out a read timeout.

use crate::common::{self, contains_code};

use atlas::gpulost::{self, Trouble};
use atlas::models::{self, Model, ModelsConfig};
use std::path::PathBuf;

const DAY: u64 = 86_400;
/// 2026-10-12 12:00 UTC.
const NOW: u64 = 1_791_806_400;

fn scratch(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-gpu-lost-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn source(path: &str) -> String {
    common::read_source_path(path).unwrap_or_else(|| panic!("{path}: not found"))
}

fn model() -> Model {
    Model {
        path: PathBuf::from("Qwen3VL-4B-Instruct-Q4_K_M.gguf"),
        id: "Qwen3VL-4B-Instruct-Q4_K_M".into(),
        architecture: "qwen3vl".into(),
        quant: "Q4_K_M".into(),
        parameters: 4_000_000_000,
        weight_bytes: 2_500_000_000,
        max_context: 32_768,
        chat_template: None,
    }
}

/// What Eric's log said, lines taken from model-server.log.previous.
const THE_LOG: &str = "\
0.44.303.399 I slot launch_slot_: id  0 | task 0 | processing task, is_child = 0
0.56.002.581 E ggml_vulkan: device lost on Vulkan0
0.56.002.599 E ggml_vulkan: getFenceStatus at D:\\a\\llama.cpp\\ggml-vulkan.cpp:2604
0.56.008.541 E srv  update_slots: decode() failed: vk::Device::getFenceStatus: ErrorDeviceLost
0.56.009.046 W srv          stop: cancel task, id_task = 0
";

#[test]
fn a_reset_is_recognised_in_the_servers_own_words() {
    assert!(gpulost::device_lost_in(THE_LOG));
    assert!(gpulost::device_lost_in(
        "decode() failed: vk::Queue::submit: ErrorDeviceLost"
    ));
    assert!(!gpulost::device_lost_in(
        "load_tensors: loading model\nlistening on http://127.0.0.1:8080\n"
    ));
    assert!(!gpulost::device_lost_in(
        "ggml_vulkan: Device memory allocation failed: out of memory"
    ));
    let line = gpulost::lost_line_in(THE_LOG).unwrap();
    assert!(line.contains("ErrorDeviceLost"), "{line}");
}

#[test]
fn a_reset_starts_safe_mode_which_ends_and_is_not_renewed_by_itself() {
    let cfg = ModelsConfig::default();
    assert!(!gpulost::apply(cfg.clone(), &Trouble::default(), NOW).gpu_safe);

    let mut t = Trouble::default();
    gpulost::record_reset(&mut t, "m", "device lost", NOW);
    assert_eq!((t.resets, t.first_at, t.safe_since), (1, NOW, NOW));
    let safe = gpulost::apply(cfg.clone(), &t, NOW + DAY);
    assert!(safe.gpu_safe);
    assert_eq!(safe.gpu_layers, "off");
    assert_eq!(safe.context, gpulost::SAFE_CONTEXT);
    // A smaller context you set is kept; "the model's own" (0) is capped.
    let small = ModelsConfig {
        context: 2048,
        ..cfg.clone()
    };
    assert_eq!(gpulost::apply(small, &t, NOW).context, 2048);
    let own = ModelsConfig { context: 0, ..cfg };
    assert_eq!(gpulost::apply(own, &t, NOW).context, gpulost::SAFE_CONTEXT);

    // Another reset while safe mode stands does not push its end out.
    gpulost::record_reset(&mut t, "m", "device lost", NOW + 3 * DAY);
    assert_eq!((t.resets, t.safe_since), (2, NOW));
    // Past the end the graphics are tried again.
    let later = NOW + gpulost::SAFE_FOR_SECS + 1;
    assert!(!gpulost::safe_now(&t, later));
    assert!(!gpulost::apply(ModelsConfig::default(), &t, later).gpu_safe);
}

#[test]
fn safe_mode_puts_nothing_on_the_graphics() {
    let m = model();
    let normal = ModelsConfig::default();
    let args = models::server_args(&m, &normal, models::ALL_LAYERS);
    assert!(!args.iter().any(|a| a == "--device"), "{args:?}");

    let mut t = Trouble::default();
    gpulost::record_reset(&mut t, "m", "device lost", NOW);
    let safe = gpulost::apply(normal, &t, NOW);
    let machine = atlas::fit::measure();
    assert_eq!(models::layers_here(&m, &safe, &machine), 0);
    let args = models::server_args(&m, &safe, 0);
    let at = args.iter().position(|a| a == "--device").expect("--device");
    assert_eq!(args[at + 1], "none");
    assert!(args.iter().any(|a| a == "--no-mmproj-offload"));
    let c = args.iter().position(|a| a == "-c").unwrap();
    assert_eq!(args[c + 1], gpulost::SAFE_CONTEXT.to_string());
    let n = args.iter().position(|a| a == "-ngl").unwrap();
    assert_eq!(args[n + 1], "0");
}

/// A stand-in for llama-server: dies with the device-lost text unless it was
/// started with `--device none`, as the real one does on the laptop's
/// graphics.
fn fake_server(dir: &std::path::Path) -> PathBuf {
    #[cfg(windows)]
    {
        let p = dir.join("fake-llama-server.bat");
        std::fs::write(
            &p,
            "@echo off\r\n\
             echo %* | findstr /C:\"--device none\" >nul && (echo listening on http://127.0.0.1:8080 1>&2 & exit /b 0)\r\n\
             echo ggml_vulkan: device lost on Vulkan0 1>&2\r\n\
             echo srv update_slots: decode() failed: vk::Queue::submit: ErrorDeviceLost 1>&2\r\n\
             exit /b 1\r\n",
        )
        .unwrap();
        p
    }
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = dir.join("fake-llama-server.sh");
        std::fs::write(
            &p,
            "#!/bin/sh\ncase \"$*\" in *\"--device none\"*) echo 'listening on http://127.0.0.1:8080' >&2; exit 0;; esac\n\
             echo 'ggml_vulkan: device lost on Vulkan0' >&2\n\
             echo 'srv update_slots: decode() failed: vk::Queue::submit: ErrorDeviceLost' >&2\n\
             exit 1\n",
        )
        .unwrap();
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        p
    }
}

fn run_it(cfg: &ModelsConfig, layers: u32, log: &str) -> (bool, String) {
    let mut child =
        models::launch_logging(&model(), cfg, layers, &atlas::tools::Vars::default(), log)
            .expect("the stand-in starts");
    let status = child.wait().unwrap();
    let text = std::fs::read_to_string(atlas::roots::data_dir().join("logs").join(log))
        .unwrap_or_default();
    (status.success(), text)
}

#[test]
fn the_server_that_lost_its_graphics_is_started_again_once_without_them() {
    let dir = scratch("fake");
    let cfg = ModelsConfig {
        server: Some(atlas::tools::ExternalTool {
            command: fake_server(&dir).display().to_string(),
            args: Vec::new(),
            stdin_text: false,
            result_file: None,
            timeout_secs: 30,
        }),
        ..ModelsConfig::default()
    };

    // First start, the usual way: it dies with the device-lost text.
    let (ok, log) = run_it(&cfg, models::ALL_LAYERS, "fake-server-1.log");
    assert!(!ok);
    assert!(gpulost::device_lost_in(&log), "{log}");

    // That is recorded...
    let mut t = Trouble::default();
    gpulost::record_reset(
        &mut t,
        "Qwen3VL-4B-Instruct-Q4_K_M",
        &gpulost::lost_line_in(&log).unwrap(),
        NOW,
    );

    // ...and the one retry uses the safe settings and stays up.
    let safe = gpulost::apply(cfg.clone(), &t, NOW);
    let layers = models::layers_here(&model(), &safe, &atlas::fit::measure());
    let (ok, log) = run_it(&safe, layers, "fake-server-2.log");
    assert!(ok, "{log}");
    assert!(!gpulost::device_lost_in(&log), "{log}");
    assert!(log.contains("listening"), "{log}");

    // The record is what the next Atlas reads, so it starts safe at once.
    gpulost::save(&t).unwrap();
    let again = gpulost::apply(cfg, &gpulost::load(), NOW + DAY);
    assert!(again.gpu_safe);
    gpulost::save(&Trouble::default()).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_doctor_says_the_graphics_kept_resetting_and_how_to_retry() {
    assert!(atlas::doctor::gpu_finding(&Trouble::default(), NOW).is_none());

    let mut t = Trouble::default();
    gpulost::record_reset(&mut t, "m", "device lost", NOW);
    gpulost::record_reset(&mut t, "m", "device lost", NOW + 60);
    let f = atlas::doctor::gpu_finding(&t, NOW + DAY).expect("a finding");
    assert_eq!(f.label, "model server");
    assert!(!f.ok, "safe mode is slower, so it is not a clean line");
    assert!(f.detail.contains("GPU kept resetting"), "{}", f.detail);
    assert!(f.detail.contains("2 times"), "{}", f.detail);
    assert!(
        f.detail.contains("safe mode since 2026-10-12"),
        "{}",
        f.detail
    );
    assert!(f.detail.contains("processor"), "{}", f.detail);
    assert!(f.detail.contains("--retry-gpu"), "{}", f.detail);

    // Once safe mode has ended it is history, not a problem.
    let f = atlas::doctor::gpu_finding(&t, NOW + gpulost::SAFE_FOR_SECS + DAY).unwrap();
    assert!(f.ok, "{}", f.detail);

    // `atlas doctor` reads the record, and `--retry-gpu` clears safe mode.
    assert!(contains_code(
        &source("src/doctor.rs"),
        "gpu_finding(&crate::gpulost::load(), crate::store::now())"
    ));
    assert!(contains_code(
        &source("src/main.rs"),
        "gpulost::retry_graphics()"
    ));
    gpulost::save(&t).unwrap();
    gpulost::retry_graphics().unwrap();
    let after = gpulost::load();
    assert!(!gpulost::safe_now(&after, NOW + DAY));
    assert_eq!(after.resets, 2, "the history stays");
    gpulost::save(&Trouble::default()).unwrap();
}

struct Counting(std::sync::atomic::AtomicUsize, Result<String, String>);

impl atlas::brain::Llm for Counting {
    fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.1.clone().map_err(atlas::error::AtlasError::Platform)
    }
}

#[test]
fn a_question_asked_while_the_server_cannot_answer_is_refused_at_once() {
    use atlas::brain::Llm;
    use std::sync::atomic::Ordering;
    use std::sync::Arc;

    // The server answers with the device-lost text the first time...
    let inner = Arc::new(Counting(
        0.into(),
        Err("decode() failed: vk::Queue::submit: ErrorDeviceLost".into()),
    ));
    let wrapped = models::WaitsForServer {
        inner: inner.clone(),
        cfg: ModelsConfig::default(),
        vars: atlas::tools::Vars::default(),
    };
    gpulost::clear();
    assert!(wrapped.complete("s", "u").is_err());
    assert_eq!(inner.0.load(Ordering::SeqCst), 1);
    assert!(gpulost::flagged(), "the failure raised the flag");

    // ...so the next calls never reach it, and never wait.
    let began = std::time::Instant::now();
    for _ in 0..3 {
        let e = wrapped.complete("s", "u").unwrap_err().to_string();
        assert!(e.contains("graphics reset"), "{e}");
    }
    assert!(began.elapsed() < std::time::Duration::from_secs(1));
    assert_eq!(inner.0.load(Ordering::SeqCst), 1);

    // The flag lapses by itself: a process with no daemon to restart the
    // server must not refuse for ever.
    gpulost::flag_at(NOW);
    assert!(gpulost::flagged_at(NOW + gpulost::FLAG_SECS - 1));
    assert!(!gpulost::flagged_at(NOW + gpulost::FLAG_SECS));

    // And a server that answers takes the flag down.
    gpulost::clear();
    assert!(!gpulost::flagged());
}

#[test]
fn the_daemon_notes_the_reset_before_it_restarts_and_starts_safely() {
    let model = source("src/daemon/model.rs");
    // Looked for before a suspect server is probed: /health says ok on a
    // server whose graphics are gone, so probing alone never finds it.
    let watch = model
        .find("if self.watch_for_lost_graphics(")
        .expect("watched");
    let suspect = model.find("if self.model_suspect && !loading {").unwrap();
    assert!(watch < suspect);
    // A server that went down with its graphics is not just a young death.
    let died = model.find("pub(super) fn model_server_died(").unwrap();
    assert!(model[died..died + 400].contains("model_server_log_says_device_lost()"));
    // The record is written before the server is stopped.
    let lost = model.find("fn graphics_were_lost(").unwrap();
    let body = &model[lost..];
    let saved = body.find("gpulost::save(&record)").expect("saved");
    let stopped = body
        .find("self.helpers.finished(\"model-server\")")
        .unwrap();
    assert!(saved < stopped);
    // The next start reads the record and applies it before choosing layers.
    assert!(contains_code(
        &model,
        "let cfg = crate::gpulost::apply(cfg, &crate::gpulost::load(), crate::store::now());"
    ));
    let applied = model.find("crate::gpulost::apply(cfg,").unwrap();
    let layers = model
        .find("crate::models::layers_here(&model, &cfg, &machine)")
        .unwrap();
    assert!(applied < layers);
    // The one retry is immediate; one already safe waits its doubling pause.
    assert!(contains_code(&model, "self.model_recovering = !was_safe;"));
}

#[test]
fn every_model_call_goes_through_the_guard() {
    let m = source("src/models.rs");
    for f in ["complete", "complete_long", "chat"] {
        let at = m
            .find(&format!("fn {f}("))
            .and_then(|_| m.find("impl crate::brain::Llm for WaitsForServer"))
            .unwrap();
        assert!(m[at..].contains("self.ready()?;"));
    }
    assert!(contains_code(&m, "if crate::gpulost::flagged() {"));
}

#[test]
fn a_health_ok_beside_the_device_lost_text_is_not_a_usable_server() {
    // 21 Oct 2026: /health says ok on a server whose graphics are gone.
    assert!(!models::usable_given_log(true, Some(THE_LOG)));
    assert!(!models::usable_given_log(false, Some(THE_LOG)));
    // A healthy server with a clean log, or no log yet, is usable.
    let clean = "load_tensors: loading model\nlistening on http://127.0.0.1:8080\n";
    assert!(models::usable_given_log(true, Some(clean)));
    assert!(models::usable_given_log(true, None));
    assert!(!models::usable_given_log(false, Some(clean)));
    // The finding is what the daemon's safe-mode path goes by: the same
    // text, read as a log, is a reset.
    assert!(gpulost::device_lost_in(THE_LOG));
}

#[test]
fn the_log_is_consulted_before_a_request_and_with_the_health_check() {
    let m = source("src/models.rs");
    // The health check does not accept an ok that the log contradicts.
    assert!(contains_code(
        &m,
        "Ok(body) => health_says_ok(&body) && !log_shows_lost_graphics(),"
    ));
    // The guard before a call reads the log before it looks at the flag.
    let ready = m.find("fn ready(&self) -> Result<()> {").unwrap();
    let body = &m[ready..];
    let scan = body.find("log_shows_lost_graphics()").expect("scans");
    let flag = body.find("if crate::gpulost::flagged() {").unwrap();
    assert!(scan < flag);
    // Finding it raises the flag, which the daemon's watch turns into the
    // stop-and-restart-in-safe-mode path.
    let finder = m.find("pub fn log_shows_lost_graphics()").unwrap();
    assert!(m[finder..finder + 1200].contains("crate::gpulost::flag();"));
    let model = source("src/daemon/model.rs");
    let watch = model.find("fn watch_for_lost_graphics(").unwrap();
    assert!(model[watch..].contains("crate::gpulost::flagged()"));
}
