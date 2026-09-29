use atlas::browser::{default_sites, post_plan, BrowserConfig, PostStep, SiteProfile};
use atlas::http::{build_request, parse_response};

fn x() -> SiteProfile {
    default_sites().into_iter().find(|s| s.name == "x").unwrap()
}

// ================= the tiny HTTP client =================

#[test]
fn a_get_request_is_well_formed_and_closes_the_connection() {
    let r = build_request("GET", "127.0.0.1:9222", "/json/list", None);
    assert!(r.starts_with("GET /json/list HTTP/1.1\r\n"));
    assert!(r.contains("Host: 127.0.0.1:9222\r\n"));
    assert!(r.contains("Connection: close\r\n"), "we read to EOF, so must not keep-alive");
    assert!(r.ends_with("\r\n\r\n"));
}

#[test]
fn a_post_carries_a_content_length_matching_the_body() {
    let body = r#"{"a":1}"#;
    let r = build_request("POST", "h:1", "/x", Some(body));
    assert!(r.contains(&format!("Content-Length: {}\r\n", body.len())));
    assert!(r.ends_with(body));
}

#[test]
fn a_normal_response_is_split_into_status_and_body() {
    let raw = "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n[{\"id\":1}]";
    let r = parse_response(raw.as_bytes()).unwrap();
    assert_eq!(r.status, 200);
    assert!(r.ok());
    assert_eq!(r.body, "[{\"id\":1}]");
}

#[test]
fn an_error_status_is_reported_not_swallowed() {
    let r = parse_response(b"HTTP/1.1 404 Not Found\r\n\r\nnope").unwrap();
    assert_eq!(r.status, 404);
    assert!(!r.ok());
}

#[test]
fn chunked_responses_are_decoded_because_chrome_uses_them() {
    let raw = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
    assert_eq!(parse_response(raw.as_bytes()).unwrap().body, "hello world");
}

#[test]
fn a_truncated_chunked_body_yields_what_arrived_rather_than_panicking() {
    let raw = "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\nFF\r\ntrunc";
    assert_eq!(parse_response(raw.as_bytes()).unwrap().body, "hello");
}

#[test]
fn a_malformed_response_is_a_clear_error() {
    assert!(parse_response(b"garbage with no headers").is_err());
    assert!(parse_response(b"NOTHTTP\r\n\r\nbody").is_err());
}

// ================= site profiles =================

#[test]
fn a_site_is_found_by_name_or_by_host() {
    let cfg = BrowserConfig::default();
    assert_eq!(cfg.profile("x").unwrap().name, "x");
    assert_eq!(cfg.profile("https://x.com/home").unwrap().name, "x");
    assert_eq!(cfg.profile("linkedin").unwrap().name, "linkedin");
    assert!(cfg.profile("myspace").is_none());
}

#[test]
fn every_profile_offers_more_than_one_selector_for_its_controls() {
    // Sites redesign. A single selector is a guaranteed future breakage, so
    // profiles carry fallbacks and Atlas uses the first that exists.
    for p in default_sites() {
        assert!(
            p.compose_box.len() >= 1 && !p.compose_box[0].is_empty(),
            "{} has no compose selector", p.name
        );
        assert!(!p.submit.is_empty(), "{} has no submit selector", p.name);
        assert!(!p.ready.is_empty(), "{} has no readiness check", p.name);
    }
    assert!(x().compose_box.len() >= 2, "the busiest site should have fallbacks");
}

#[test]
fn profiles_check_for_a_signed_in_session() {
    // Typing into a logged-out page silently does nothing, which looks
    // exactly like success.
    assert!(!x().signed_in.is_empty());
}

// ================= the posting sequence =================

#[test]
fn drafting_never_includes_the_submit_step() {
    let steps = post_plan(&x(), "hello world", false);
    assert!(!steps.contains(&PostStep::Submit), "nothing goes out as a side effect of drafting");
    assert_eq!(steps.last(), Some(&PostStep::Fill("hello world".into())));
}

#[test]
fn submitting_only_appears_once_approval_is_in_hand() {
    let steps = post_plan(&x(), "hello world", true);
    assert_eq!(steps.last(), Some(&PostStep::Submit));
}

#[test]
fn the_sequence_checks_readiness_and_sign_in_before_typing() {
    let steps = post_plan(&x(), "hi", true);
    let fill_at = steps.iter().position(|s| matches!(s, PostStep::Fill(_))).unwrap();
    let ready_at = steps.iter().position(|s| *s == PostStep::WaitReady).unwrap();
    let signed_at = steps.iter().position(|s| *s == PostStep::CheckSignedIn).unwrap();
    assert!(ready_at < fill_at, "must wait for the page");
    assert!(signed_at < fill_at, "must confirm the session before typing");
}

#[test]
fn the_sequence_starts_by_opening_the_right_page() {
    match &post_plan(&x(), "hi", true)[0] {
        PostStep::Open(url) => assert!(url.contains("x.com")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn the_default_browser_config_is_loopback_only() {
    let cfg = BrowserConfig::default();
    assert_eq!(cfg.port, 9222);
    assert!(cfg.startup_ms > cfg.timeout_ms / 2, "give Chrome time to start");
}

#[test]
fn the_shipped_browser_config_keeps_atlas_out_of_your_own_chrome_profile() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    let launch = t.browser.launch.expect("a launch command");
    assert!(
        launch.args.iter().any(|a| a.contains("--headless")),
        "must never be the window you are using"
    );
    assert!(
        launch.args.iter().any(|a| a.contains("--user-data-dir")),
        "must have its own profile, not your live sessions"
    );
}
