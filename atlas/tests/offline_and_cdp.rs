use atlas::awareness::Signals;
use atlas::cdp::{click_js, exists_js, fill_js, js_str, ws_url_from_targets};
use atlas::connectivity::{deferral_message, need_of, Connectivity, ConnectivityConfig, Need, Reach};
use atlas::intent::Intent;
use atlas::lanes::{Lane, LaneConfig, Queue, TaskState};
use atlas::ws::{encode_frame, handshake_request, unmask};

// ================= offline is the normal case, not the failure case =================

#[test]
fn everything_that_makes_atlas_useful_works_offline() {
    // If this list ever shrinks, Atlas has become internet-dependent.
    let local = [
        Intent::WorkspaceOn,
        Intent::WorkspaceOff,
        Intent::OpenApp("chrome".into()),
        Intent::CloseApp("chrome".into()),
        Intent::FocusApp("chrome".into()),
        Intent::ViewDisplay,
        Intent::CaptureWebcam,
        Intent::Ask("which one?".into()),
        Intent::Unknown("mumble".into()),
    ];
    for i in &local {
        assert_eq!(need_of(i), Need::Local, "{i:?} must not need a network");
    }
}

#[test]
fn only_web_research_genuinely_requires_a_connection() {
    assert_eq!(need_of(&Intent::Research("x".into())), Need::Internet);
    // Conversation degrades rather than failing — a local model still answers.
    assert_eq!(need_of(&Intent::Say("x".into())), Need::PrefersInternet);
}

#[test]
fn local_and_degradable_work_is_allowed_even_when_known_offline() {
    let mut c = Connectivity::new(ConnectivityConfig { assume_offline: true, ..Default::default() });
    assert_eq!(c.status(100), Reach::Offline);
    assert!(c.allows(Need::Local), "local work must never consult the network");
    assert!(c.allows(Need::PrefersInternet), "degrade, don't block");
    assert!(!c.allows(Need::Internet));
}

#[test]
fn an_air_gapped_machine_never_probes_at_all() {
    let mut c = Connectivity::new(ConnectivityConfig { assume_offline: true, ..Default::default() });
    for t in 0..5 {
        assert_eq!(c.status(t * 1000), Reach::Offline);
    }
}

#[test]
fn reachability_is_cached_so_it_is_cheap_to_check_every_tick() {
    let mut c = Connectivity::new(ConnectivityConfig {
        probe: "127.0.0.1:1".into(), // nothing listening
        timeout_ms: 50,
        cache_secs: 30,
        assume_offline: false,
    });
    assert_eq!(c.status(1000), Reach::Offline);
    let t0 = std::time::Instant::now();
    for t in 0..200 {
        c.status(1000 + t % 20);
    }
    crate::common::assert_prompt(t0.elapsed(), std::time::Duration::from_millis(200), "cached checks must not re-probe");
}

#[test]
fn a_failure_can_force_a_fresh_check() {
    let mut c = Connectivity::new(ConnectivityConfig {
        probe: "127.0.0.1:1".into(), timeout_ms: 50, cache_secs: 9999, assume_offline: false,
    });
    c.status(100);
    assert_eq!(c.cached(), Reach::Offline);
    c.invalidate();
    assert_eq!(c.cached(), Reach::Unknown, "next call re-probes");
}

fn signals() -> Signals {
    Signals { idle_secs: 999, ..Default::default() }
}

#[test]
fn offline_research_waits_for_a_connection_instead_of_failing() {
    let mut q = Queue::default();
    let id = q.push_online("research IETF QUIC v1", Lane::Background);
    assert!(q.ready_with(&signals(), &LaneConfig::default(), 100, Reach::Offline).is_empty());
    assert_eq!(q.waiting_for_network().len(), 1);
    assert_eq!(q.tasks[0].state, TaskState::Queued, "queued, not failed");

    assert_eq!(q.ready_with(&signals(), &LaneConfig::default(), 200, Reach::Online), vec![id]);
}

#[test]
fn local_work_still_runs_while_something_else_waits_for_the_network() {
    let mut q = Queue::default();
    q.push_online("research something", Lane::Background);
    let local = q.push("open chrome", Lane::Foreground);
    let ready = q.ready_with(&signals(), &LaneConfig::default(), 100, Reach::Offline);
    assert_eq!(ready, vec![local], "being offline must not stall the workspace");
}

#[test]
fn a_job_waiting_for_the_network_is_never_expired_for_waiting() {
    // Losing a connection is not the job's fault.
    let mut q = Queue::default();
    q.push_online("research something", Lane::Foreground);
    q.ready_with(&signals(), &LaneConfig::default(), 100, Reach::Offline);
    q.ready_with(&signals(), &LaneConfig::default(), 10_000_000, Reach::Offline);
    assert_ne!(q.tasks[0].state, TaskState::Failed);
    assert_eq!(q.waiting_for_network().len(), 1);
}

#[test]
fn atlas_says_why_and_what_happens_next_rather_than_just_failing() {
    let m = deferral_message(&Intent::Research("the quic v1 spec".into()));
    assert!(m.contains("the quic v1 spec"));
    assert!(m.to_lowercase().contains("back online"), "must say what happens next: {m}");
}

// ================= websocket framing =================

#[test]
fn a_client_frame_is_masked_and_round_trips() {
    let mask = [0xAA, 0xBB, 0xCC, 0xDD];
    let f = encode_frame(0x1, b"hello", mask);
    assert_eq!(f[0], 0x81, "FIN + text opcode");
    assert_eq!(f[1] & 0x80, 0x80, "client frames must be masked");
    assert_eq!(f[1] & 0x7F, 5);
    let mut payload = f[6..].to_vec();
    unmask(&mut payload, mask);
    assert_eq!(payload, b"hello");
}

#[test]
fn medium_payloads_use_the_two_byte_length() {
    let f = encode_frame(0x1, &vec![b'x'; 300], [0; 4]);
    assert_eq!(f[1] & 0x7F, 126);
    assert_eq!(u16::from_be_bytes([f[2], f[3]]), 300);
}

#[test]
fn large_payloads_use_the_eight_byte_length() {
    let f = encode_frame(0x1, &vec![b'x'; 70_000], [0; 4]);
    assert_eq!(f[1] & 0x7F, 127);
    assert_eq!(u64::from_be_bytes(f[2..10].try_into().unwrap()), 70_000);
}

#[test]
fn an_empty_frame_is_valid() {
    let f = encode_frame(0x8, &[], [1, 2, 3, 4]);
    assert_eq!(f.len(), 6);
}

#[test]
fn the_handshake_asks_for_the_right_upgrade() {
    let r = handshake_request("127.0.0.1:9222", "/devtools/page/ABC", "dGhlIHNhbXBsZQ==");
    assert!(r.starts_with("GET /devtools/page/ABC HTTP/1.1\r\n"));
    assert!(r.contains("Upgrade: websocket\r\n"));
    assert!(r.contains("Sec-WebSocket-Version: 13\r\n"));
    assert!(r.ends_with("\r\n\r\n"));
}

// ================= driving the page =================

#[test]
fn selectors_are_escaped_so_a_quote_cannot_break_out() {
    let js = click_js("input[name='q']");
    assert!(!js.contains("name='q'"), "the inner quotes must be escaped: {js}");
    assert!(js.contains("\\'q\\'"));
}

#[test]
fn typed_text_cannot_inject_script_into_the_page() {
    let js = fill_js("#search", "</script><script>alert(1)</script>");
    assert!(!js.contains("<script>"), "angle brackets must be escaped: {js}");
    assert!(js.contains("\\x3C"));
}

#[test]
fn backslashes_and_newlines_survive_escaping() {
    assert_eq!(js_str("a\\b"), "a\\\\b");
    assert_eq!(js_str("line1\nline2"), "line1\\nline2");
}

#[test]
fn a_click_reports_a_missing_element_rather_than_silently_doing_nothing() {
    // The generated JS returns false when nothing matches, which the caller
    // turns into a named error.
    assert!(click_js("#gone").contains("return false"));
}

#[test]
fn filling_a_field_fires_the_events_frameworks_listen_for() {
    let js = fill_js("#email", "me@example.com");
    assert!(js.contains("new Event('input'"), "React ignores a bare value set");
    assert!(js.contains("new Event('change'"));
    assert!(js.contains("bubbles:true"));
}

#[test]
fn clicking_scrolls_the_element_into_view_first() {
    assert!(click_js("#submit").contains("scrollIntoView"));
}

#[test]
fn the_page_target_is_picked_out_of_chromes_target_list() {
    let body = r#"[
      {"type":"background_page","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/BG"},
      {"type":"page","webSocketDebuggerUrl":"ws://127.0.0.1:9222/devtools/page/REAL"}
    ]"#;
    assert_eq!(
        ws_url_from_targets(body).as_deref(),
        Some("ws://127.0.0.1:9222/devtools/page/REAL"),
        "background pages are not the tab we want"
    );
}

#[test]
fn no_page_target_is_a_clean_none_not_a_panic() {
    assert!(ws_url_from_targets("[]").is_none());
    assert!(ws_url_from_targets("not json").is_none());
}

#[test]
fn exists_check_is_a_plain_boolean() {
    assert_eq!(exists_js("#x"), "!!document.querySelector('#x')");
}

#[test]
fn the_shipped_config_keeps_the_offline_promise() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();

    // The reasoning model is Atlas's own llama-server on this machine, so
    // conversation works with the cable pulled out. (It was a local Ollama
    // until 26 Sep 2026.)
    assert!(t.llm.is_none(), "no hand-written connection overriding the local model");
    assert_eq!(
        atlas::models::self_built_endpoint(&t.models),
        atlas::brain::Endpoint::ThisMachine,
        "the default model must be local"
    );
    // Speech in and out are local binaries, not web services.
    for tool in [&t.record, &t.stt, &t.tts, &t.play] {
        assert!(
            !tool.args.iter().any(|a| a.starts_with("http")),
            "voice must not depend on a web endpoint: {:?}", tool.args
        );
    }
    assert!(t.connectivity.timeout_ms > 0 && t.connectivity.cache_secs > 0);
}

#[test]
fn a_pinned_state_survives_cache_expiry() {
    // On a captive-portal network the TCP probe succeeds while nothing works.
    // Pinning has to outlast the cache, or the probe overrides you.
    let mut c = Connectivity::new(ConnectivityConfig {
        probe: "127.0.0.1:1".into(), timeout_ms: 20, cache_secs: 1, assume_offline: false,
    });
    c.set(Reach::Online, 0);
    assert_eq!(c.status(0), Reach::Online);
    assert_eq!(c.status(1_000_000), Reach::Online, "the probe must not override a pin");
    c.unpin();
    assert_eq!(c.status(1_000_001), Reach::Offline, "and unpinning gives the probe back");
}
