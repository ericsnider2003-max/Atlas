//! Proof that research answers in two parts now: an immediate, honest
//! acknowledgment, and the real answer once the crew errand actually
//! finishes -- never the full answer synchronously, and never silence.
//!
//! Uses local shell commands standing in for the search and fetch tools, so
//! this is deterministic and does not depend on reaching the real internet.

use atlas::brain::{Llm, LlmConfig};
use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::error::Result;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use atlas::vault::{Kind, VaultConfig};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn freellmapi_test_guard() -> std::sync::MutexGuard<'static, ()> {
    static GATE: std::sync::OnceLock<std::sync::Mutex<()>> = std::sync::OnceLock::new();
    GATE.get_or_init(|| std::sync::Mutex::new(()))
        .lock()
        .unwrap()
}

fn tmp(tag: &str) -> PathBuf {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock is before the Unix epoch")
        .as_nanos();
    let p = std::env::temp_dir().join(format!(
        "atlas-research-crew-{tag}-{}-{nonce}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

/// Answers every completion with a fixed summary, regardless of what it was
/// asked -- these tests are about the plumbing around the model call, not
/// the model call itself.
struct StubLlm;
impl Llm for StubLlm {
    fn execution_descriptor(&self) -> atlas::execution::ExecutionDescriptor {
        atlas::execution::ExecutionDescriptor::new(
            "fixture-local",
            "stub-research",
            "test-runtime",
            "test-device",
            "offline",
            "resident",
        )
        .expect("fixture descriptor is valid")
    }

    fn supports_bounded_chat(&self) -> bool {
        true
    }
    fn chat_until(
        &self,
        _: &atlas::brain::ChatRequest,
        on_text: &mut dyn FnMut(&str) -> bool,
        keep_going: &dyn Fn() -> bool,
    ) -> Result<atlas::brain::ChatReply> {
        let text = "Tide times at Ventura peak just after noon. Wear sandals.";
        if !keep_going() || !on_text(text) || !keep_going() {
            return Err(atlas::error::AtlasError::Platform(
                "research fixture stopped".into(),
            ));
        }
        Ok(atlas::brain::ChatReply {
            text: text.into(),
            ..Default::default()
        })
    }
    fn complete(&self, _system: &str, _user: &str) -> Result<String> {
        Ok("Tide times at Ventura peak just after noon. Wear sandals.".into())
    }
}

/// A config with research switched on and pointed at local shell commands
/// instead of a real search engine, so a test run never depends on the
/// network. `search` prints one URL; `fetch` prints enough text to clear
/// the 200-character floor `Research::run` filters short pages with.
fn cfg_researching_locally() -> Config {
    let mut c = Config::load(Path::new("config")).unwrap();
    let tools = c
        .tools
        .as_mut()
        .expect("the shipped config has a tools section");
    tools.research.enabled = true;
    // `example.test` is never looked up for real (it can't be), and the stub
    // fetch below goes nowhere; the public-address check has its own tests.
    tools.research.pages_on_this_machine = true;
    tools.research.search = Some(crate::common::printing("http://example.test/tides"));
    tools.research.fetch = Some(crate::common::printing(
        &"Ventura tide information. ".repeat(20),
    ));
    tools.llm = Some(LlmConfig {
        tool: Default::default(),
        request: r#"{"model":"stub","prompt":"{user}"}"#.into(),
        response_path: "response".into(),
        vision_request: None,
    });
    c
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    let mut d = Daemon::new(
        c,
        p,
        Some(std::sync::Arc::new(StubLlm)),
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    );
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    d
}

fn daemon_without_local_background<'a>(
    c: &'a Config,
    p: &'a MockPlatform,
    tag: &str,
) -> Daemon<'a> {
    let mut d = Daemon::new(
        c,
        p,
        None,
        Store::new(tmp(tag)),
        Proactive::new(ProactiveConfig::default()),
    );
    d.connectivity.set(atlas::connectivity::Reach::Online, 0);
    d
}

#[test]
fn asking_to_research_something_gets_an_honest_immediate_acknowledgment() {
    let c = cfg_researching_locally();
    let p = plat();
    let mut d = daemon(&c, &p, "ack");

    let reply = d.turn("research tide times at Ventura", 100);

    assert!(
        reply.contains("Looking into") || reply.contains("looking into"),
        "the immediate reply should be an acknowledgment, not an answer: {reply}"
    );
    // The honest half of this: it must not claim to already know the
    // answer it hasn't looked up yet. "tide" trivially appears because the
    // ack echoes the topic back -- the real signal is the model's actual
    // answer text, which must not have leaked in before the lookup ran.
    assert!(
        !reply.to_lowercase().contains("sandals"),
        "the immediate reply must not contain the answer before the lookup has run: {reply}"
    );
    assert_eq!(
        d.crew.active(),
        1,
        "the actual lookup should be running in the crew"
    );
}

#[test]
fn the_real_answer_arrives_later_once_the_crew_actually_finishes() {
    let c = cfg_researching_locally();
    let p = plat();
    let mut d = daemon(&c, &p, "later");

    let ack = d.turn("research tide times at Ventura", 100);
    assert!(
        !ack.to_lowercase().contains("sandals"),
        "got the answer too early: {ack}"
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut saw_the_answer = false;
    let mut t: u64 = 101;
    while Instant::now() < deadline {
        let said = d.tick(t);
        if said.iter().any(|l| l.to_lowercase().contains("sandals")) {
            saw_the_answer = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
    assert!(saw_the_answer, "the real research result never arrived");
    let settle_deadline = Instant::now() + Duration::from_secs(5);
    while d.crew.active() != 0 && Instant::now() < settle_deadline {
        d.tick(t);
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
    assert_eq!(d.crew.active(), 0, "research indexing did not settle");
}

#[test]
fn completed_background_research_receipt_records_the_adapter_that_finished_it() {
    let c = cfg_researching_locally();
    let p = plat();
    let mut d = daemon(&c, &p, "execution-provenance");

    d.turn("research tide times at Ventura", 100);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut t: u64 = 101;
    let mut records = Vec::new();
    while Instant::now() < deadline {
        d.tick(t);
        records = atlas::job::load_checked(&d.store).expect("job journal remains readable");
        if records.iter().any(|record| {
            record.stage == "research"
                && record.state == atlas::job::State::Succeeded
                && record.receipt.is_some()
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
    let research_records: Vec<_> = records
        .iter()
        .filter(|record| record.stage == "research")
        .collect();
    assert_eq!(
        research_records.len(),
        1,
        "exactly one research job is recorded"
    );

    let record = research_records[0];
    assert_eq!(record.state, atlas::job::State::Succeeded);
    let execution = record
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.execution.as_ref())
        .expect("completed research receipt records execution identity");
    assert_eq!(record.receipt.as_ref().unwrap().job_id, record.job_id);
    assert_eq!(execution.provider, "fixture-local");
    assert_eq!(execution.model, "stub-research");
    assert_eq!(execution.network, "offline");
}

#[test]
fn delegated_research_failure_uses_local_fallback_and_records_local_identity() {
    let _test_guard = freellmapi_test_guard();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
    listener
        .set_nonblocking(true)
        .expect("make fake gateway bounded");
    let port = listener.local_addr().expect("fake gateway address").port();
    let gateway = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = [0u8; 1024];
                    let _ = stream.read(&mut request);
                    stream
                        .write_all(
                            b"HTTP/1.1 503 Service Unavailable\r\nContent-Length: 3\r\nConnection: close\r\n\r\nbad",
                        )
                        .expect("write fake gateway failure");
                    return true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fake gateway accept failed: {error}"),
            }
        }
        false
    });

    let mut c = cfg_researching_locally();
    let tools = c.tools.as_mut().expect("the shipped config has tools");
    tools.freellmapi.enabled = true;
    tools.freellmapi.verify = false;
    tools.freellmapi.endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
    tools.freellmapi.model = "fixture-remote-model".into();
    tools.freellmapi.token_vault = "freellmapi fixture key".into();

    let p = plat();
    let mut d = daemon(&c, &p, "remote-fallback");
    d.vault
        .open(
            "a genuinely long passphrase, not a word",
            0,
            &VaultConfig::default(),
        )
        .expect("open fixture vault");
    d.vault
        .put("freellmapi fixture key", Kind::Login, "fixture-token", 0)
        .expect("store fixture gateway token");

    let ack = d.turn("research tide times at Ventura", 100);
    assert!(
        ack.contains("worker online"),
        "FreeLLMAPI worker was not admitted: {ack}"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut t: u64 = 101;
    let mut records = Vec::new();
    while Instant::now() < deadline {
        d.tick(t);
        records = atlas::job::load_checked(&d.store).expect("job journal remains readable");
        if records.iter().any(|record| {
            record.stage == "research"
                && record.state == atlas::job::State::Succeeded
                && record.receipt.is_some()
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
    assert_eq!(
        records
            .iter()
            .filter(|record| record.stage == "research")
            .count(),
        1,
        "exactly one research job is recorded"
    );
    let record = records
        .iter()
        .find(|record| record.stage == "research")
        .expect("research record exists");
    assert_eq!(record.state, atlas::job::State::Succeeded);
    let execution = record
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.execution.as_ref())
        .expect("fallback receipt records execution identity");
    assert_eq!(execution.provider, "fixture-local");
    assert!(gateway.join().expect("fake gateway thread completed"));
}

#[test]
fn delegated_research_success_records_freellmapi_request_and_identity() {
    let _test_guard = freellmapi_test_guard();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
    listener
        .set_nonblocking(true)
        .expect("make fake gateway bounded");
    let port = listener.local_addr().expect("fake gateway address").port();
    let (request_tx, request_rx) = std::sync::mpsc::channel();
    let gateway = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 4096];
                    let _ = stream
                        .read(&mut chunk)
                        .map(|read| request.extend_from_slice(&chunk[..read]));
                    request_tx
                        .send(request)
                        .expect("send captured gateway request");
                    let body = br#"{"choices":[{"message":{"content":"Tide times at Ventura peak just after noon. Wear sandals."}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Routed-Via: fixture-provider\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        String::from_utf8_lossy(body)
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write fake gateway success");
                    return true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fake gateway accept failed: {error}"),
            }
        }
        false
    });

    let mut c = cfg_researching_locally();
    let tools = c.tools.as_mut().expect("the shipped config has tools");
    tools.freellmapi.enabled = true;
    tools.freellmapi.verify = false;
    tools.freellmapi.endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
    tools.freellmapi.model = "fixture-remote-model".into();
    tools.freellmapi.token_vault = "freellmapi fixture key".into();

    let p = plat();
    let mut d = daemon(&c, &p, "remote-success");
    d.vault
        .open(
            "a genuinely long passphrase, not a word",
            0,
            &VaultConfig::default(),
        )
        .expect("open fixture vault");
    d.vault
        .put("freellmapi fixture key", Kind::Login, "fixture-token", 0)
        .expect("store fixture gateway token");

    let ack = d.turn("research tide times at Ventura", 100);
    assert!(
        ack.contains("worker online"),
        "FreeLLMAPI worker was not admitted: {ack}"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut t: u64 = 101;
    let mut records = Vec::new();
    while Instant::now() < deadline {
        d.tick(t);
        records = atlas::job::load_checked(&d.store).expect("job journal remains readable");
        if records.iter().any(|record| {
            record.stage == "research"
                && record.state == atlas::job::State::Succeeded
                && record.receipt.is_some()
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }

    let request = request_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("FreeLLMAPI received a research request");
    let request = String::from_utf8_lossy(&request);
    assert!(
        request.starts_with("POST /v1/chat/completions "),
        "{request}"
    );
    assert!(
        request.contains("Authorization: Bearer fixture-token"),
        "{request}"
    );
    assert!(
        request.contains(r#""model":"fixture-remote-model""#),
        "{request}"
    );

    assert_eq!(
        records
            .iter()
            .filter(|record| record.stage == "research")
            .count(),
        1,
        "exactly one research job is recorded"
    );
    let record = records
        .iter()
        .find(|record| record.stage == "research")
        .expect("research record exists");
    assert_eq!(record.state, atlas::job::State::Succeeded);
    let execution = record
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.execution.as_ref())
        .expect("successful receipt records execution identity");
    assert_eq!(execution.provider, "freellmapi-gateway");
    assert_eq!(execution.model, "fixture-remote-model");
    assert_eq!(execution.network, "remote-via-loopback-gateway");
    let evidence: Vec<atlas::evidence::Bundle> = d.store.load("evidence");
    let research_evidence = evidence
        .iter()
        .find(|bundle| bundle.receipt_job_id.as_deref() == Some(record.job_id.as_str()))
        .expect("research evidence is linked to the completed job");
    assert_eq!(research_evidence.provider, "freellmapi-gateway");
    assert_eq!(research_evidence.execution.as_ref(), Some(execution));
    assert_eq!(
        research_evidence
            .execution_metadata
            .as_ref()
            .and_then(|metadata| metadata.routed_via.as_deref()),
        Some("fixture-provider")
    );
    assert!(gateway.join().expect("fake gateway thread completed"));
}

#[test]
fn freellmapi_can_write_research_without_a_local_background_model() {
    let _test_guard = freellmapi_test_guard();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind fake gateway");
    listener
        .set_nonblocking(true)
        .expect("make fake gateway bounded");
    let port = listener.local_addr().expect("fake gateway address").port();
    let gateway = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    let mut request = [0u8; 4096];
                    let _ = stream.read(&mut request);
                    let body = br#"{"choices":[{"message":{"content":"FreeLLMAPI research found Ventura tide times near noon, based on the fetched source."}}]}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nX-Routed-Via: fixture-provider\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        String::from_utf8_lossy(body)
                    );
                    stream
                        .write_all(response.as_bytes())
                        .expect("write fake gateway success");
                    return true;
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) => panic!("fake gateway accept failed: {error}"),
            }
        }
        false
    });

    let mut c = cfg_researching_locally();
    let tools = c
        .tools
        .as_mut()
        .expect("the shipped config has a tools section");
    tools.llm = None;
    tools.freellmapi.enabled = true;
    tools.freellmapi.verify = false;
    tools.freellmapi.endpoint = format!("http://127.0.0.1:{port}/v1/chat/completions");
    tools.freellmapi.model = "fixture-remote-model".into();
    tools.freellmapi.token_vault = "freellmapi fixture key".into();

    let p = plat();
    let mut d = daemon_without_local_background(&c, &p, "remote-only");
    d.vault
        .open(
            "a genuinely long passphrase, not a word",
            0,
            &VaultConfig::default(),
        )
        .expect("open fixture vault");
    d.vault
        .put("freellmapi fixture key", Kind::Login, "fixture-token", 0)
        .expect("store fixture gateway token");

    let ack = d.turn("research tide times at Ventura", 100);
    assert!(
        ack.contains("worker online"),
        "FreeLLMAPI-only worker was not admitted: {ack}"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut t: u64 = 101;
    let mut records = Vec::new();
    while Instant::now() < deadline {
        d.tick(t);
        records = atlas::job::load_checked(&d.store).expect("job journal remains readable");
        if records.iter().any(|record| {
            record.stage == "research"
                && record.state == atlas::job::State::Succeeded
                && record.receipt.is_some()
        }) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }

    let record = records
        .iter()
        .find(|record| record.stage == "research")
        .expect("research record exists");
    assert_eq!(record.state, atlas::job::State::Succeeded);
    let execution = record
        .receipt
        .as_ref()
        .and_then(|receipt| receipt.execution.as_ref())
        .expect("successful receipt records execution identity");
    assert_eq!(execution.provider, "freellmapi-gateway");
    assert_eq!(execution.model, "fixture-remote-model");
    assert_eq!(execution.network, "remote-via-loopback-gateway");
    assert!(gateway.join().expect("fake gateway thread completed"));
}

#[test]
fn a_slow_or_fast_research_answer_is_always_spoken_unlike_a_silent_chore() {
    // Unlike backup and housekeeping, research must never go quiet just
    // because it happened to finish fast -- it's an answer to something
    // asked for, not a chore nobody's waiting on.
    let c = cfg_researching_locally();
    let p = plat();
    let mut d = daemon(&c, &p, "always-speaks");
    d.turn("research tide times at Ventura", 100);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut said_lines: Vec<String> = Vec::new();
    let mut t: u64 = 101;
    while Instant::now() < deadline && said_lines.is_empty() {
        said_lines = d.tick(t);
        if said_lines.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
            t += 1;
        }
    }
    assert!(
        !said_lines.is_empty(),
        "research's result must be spoken even if it finished quickly"
    );
}

#[test]
fn the_report_names_the_answer_and_source_count_but_never_the_save_mechanics() {
    // Eric's call: he doesn't need to be told where a note landed, or
    // whether the save worked -- it saves automatically either way, and
    // asking to see or save something is a separate, deliberate request.
    let c = cfg_researching_locally();
    let p = plat();
    let mut d = daemon(&c, &p, "no-save-narration");
    d.turn("research tide times at Ventura", 100);

    let deadline = Instant::now() + Duration::from_secs(5);
    let mut report = String::new();
    let mut t: u64 = 101;
    while Instant::now() < deadline && report.is_empty() {
        for line in d.tick(t) {
            if line.to_lowercase().contains("sandals") {
                report = line;
            }
        }
        if report.is_empty() {
            std::thread::sleep(Duration::from_millis(10));
            t += 1;
        }
    }
    assert!(!report.is_empty(), "never got the research report");
    assert!(
        !report.contains("Saved"),
        "the save location must not be spoken: {report}"
    );
    assert!(
        !report.contains("couldn't save"),
        "a save failure must not be spoken: {report}"
    );
    assert!(
        report.contains("Read 1 source"),
        "the source count should still be there: {report}"
    );
}

#[test]
fn offline_research_speaks_the_shared_deferral_message_not_a_hand_written_copy() {
    // The offline branch used to hand-write its own "no connection, on the
    // outstanding list" sentence. It now routes through
    // `connectivity::deferral_message` -- the one place that answers "the
    // network was needed and isn't here" -- so there is a single copy of that
    // promise and the named function has its production caller. The signature
    // of the shared message is that it says what happens next ("back online"),
    // which the old hand-written string never did.
    let c = cfg_researching_locally();
    let p = plat();
    let mut d = daemon(&c, &p, "offline-deferral");
    d.connectivity.set(atlas::connectivity::Reach::Offline, 0);

    let reply = d.turn("research tide times at Ventura", 100);

    assert!(
        reply.to_lowercase().contains("back online"),
        "offline research must say what happens next, as deferral_message does: {reply}"
    );
    assert!(
        reply.to_lowercase().contains("tide"),
        "the deferral must name the topic it is holding: {reply}"
    );
    // No search errand may start while offline -- the request is deferred, not run.
    assert_eq!(
        d.crew.active(),
        0,
        "an offline research request must not start a lookup"
    );
}

/// "research it again" means the last topic, not the word "it" (30 Sep 2026
/// sweep: it searched for "it", or answered "I can't tell what you mean").
#[test]
fn researching_it_again_means_the_last_topic() {
    let c = cfg_researching_locally();
    let p = plat();
    let mut d = daemon(&c, &p, "it-again");
    let first = d.turn("research tide times at Ventura", 100);
    assert!(first.to_lowercase().contains("looking into"), "{first}");
    let deadline = Instant::now() + Duration::from_secs(5);
    let mut t: u64 = 101;
    while Instant::now() < deadline && d.crew.active() > 0 {
        d.tick(t);
        std::thread::sleep(Duration::from_millis(10));
        t += 1;
    }
    // Asked the same way, it's answered from what was found -- the cache
    // matches what was asked, not the answer's wording.
    let same = d.turn("research tide times at Ventura", t + 5);
    assert!(same.contains("From what I found before"), "{same}");
    // "again" is a fresh look at the last topic, not a search for "it".
    let again = d.turn("research it again", t + 10);
    let low = again.to_lowercase();
    assert!(low.contains("tide times at ventura"), "{again}");
    assert!(
        low.contains("looking into"),
        "a fresh run, not the cache: {again}"
    );
    assert!(!low.contains("can't tell"), "{again}");
}
