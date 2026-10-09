//! Research's headless-browser fallback for the search step.
//!
//! When curl gets a search engine's script-only or challenge page, the
//! results page has no links in it and research used to stop at "no sources
//! found". It now opens the same page in the headless browser and reads the
//! links the page's scripts rendered (`Browser::links`, over `Cdp::links`).
//!
//! What can be exercised without a live Chrome is exercised here: which page
//! the browser would open, how the links it reads become sources, and that a
//! browser that cannot be reached falls through to the original error.

use atlas::browser::{Browser, BrowserConfig};
use atlas::research::{extract_urls, search_page_url, urls_from_links, Research, ResearchConfig};
use atlas::tools::{ExternalTool, Vars};

/// Built the way `config/tools.yaml`'s `research.search` is.
fn ddg_search() -> ExternalTool {
    ExternalTool {
        command: "curl".into(),
        args: vec![
            "-s".into(),
            "-A".into(),
            "Mozilla/5.0".into(),
            "https://html.duckduckgo.com/html/?q={query}".into(),
        ],
        ..Default::default()
    }
}

fn vars_with_query(q: &str) -> Vars {
    let mut v = Vars::new();
    v.insert("query".into(), q.into());
    v
}

fn links(ls: &[&str]) -> Vec<String> {
    ls.iter().map(|s| s.to_string()).collect()
}

// ============ which page the browser opens ============

#[test]
fn the_search_page_is_the_search_tools_own_url_with_the_query_filled_in() {
    let url = search_page_url(&ddg_search(), &vars_with_query("ventura+tides"));
    assert_eq!(url.as_deref(), Some("https://html.duckduckgo.com/html/?q=ventura+tides"));
}

#[test]
fn a_search_tool_with_no_web_address_has_no_page_to_open() {
    let script = ExternalTool {
        command: "sh".into(),
        args: vec!["-c".into(), "echo {query}".into()],
        ..Default::default()
    };
    assert_eq!(search_page_url(&script, &vars_with_query("x")), None);
}

#[test]
fn a_plain_http_address_counts_too() {
    let t = ExternalTool {
        command: "curl".into(),
        args: vec!["http://search.local/?q={query}".into()],
        ..Default::default()
    };
    assert_eq!(
        search_page_url(&t, &vars_with_query("a")).as_deref(),
        Some("http://search.local/?q=a")
    );
}

// ============ links the browser read, turned into sources ============

#[test]
fn links_are_filtered_exactly_like_a_results_page() {
    // The same noise `extract_urls` drops in tests/lanes_research.rs: the
    // search engine itself, CDN scripts, stylesheets.
    let ls = links(&[
        "https://duckduckgo.com/?q=x",
        "https://cdn.site.com/a.js",
        "https://site.com/style.css",
        "https://good.example.com/real-article-here",
    ]);
    assert_eq!(urls_from_links(&ls, 10), vec!["https://good.example.com/real-article-here"]);
    assert_eq!(urls_from_links(&ls, 10), extract_urls(&ls.join(" "), 10));
}

#[test]
fn duplicate_links_become_one_source() {
    let ls = links(&[
        "https://example.com/aaaaaaaa",
        "https://example.com/aaaaaaaa",
        "https://example.com/bbbbbbbb",
    ]);
    assert_eq!(
        urls_from_links(&ls, 10),
        vec!["https://example.com/aaaaaaaa", "https://example.com/bbbbbbbb"]
    );
}

#[test]
fn no_more_than_max_sources_are_taken() {
    let ls = links(&[
        "https://one.example.com/article",
        "https://two.example.com/article",
        "https://three.example.com/article",
    ]);
    assert_eq!(
        urls_from_links(&ls, 2),
        vec!["https://one.example.com/article", "https://two.example.com/article"]
    );
}

#[test]
fn a_search_engines_click_through_redirect_is_unwrapped_to_its_target() {
    // What `a.href` reads on DuckDuckGo's html results: every result wrapped
    // in a duckduckgo.com redirect, which would otherwise be dropped as noise.
    let ls = links(&[
        "https://duckduckgo.com/l/?uddg=https%3A%2F%2Ftides.example.org%2Fventura&rut=abc",
        "https://duckduckgo.com/about",
    ]);
    assert_eq!(urls_from_links(&ls, 10), vec!["https://tides.example.org/ventura"]);
}

#[test]
fn no_links_means_no_sources() {
    assert!(urls_from_links(&[], 4).is_empty());
}

// ============ Browser::links ============

#[test]
fn browser_links_is_the_page_link_reader() {
    // `Browser::links` needs a live Chrome to return anything; this pins its
    // shape (what research calls) without one.
    let reader: fn(&mut Browser) -> atlas::error::Result<Vec<String>> = Browser::links;
    let _ = reader;
}

// ============ the fallback in Research::run ============

struct NeverCalled;
impl atlas::brain::Llm for NeverCalled {
    fn supports_bounded_chat(&self) -> bool { true }
    fn chat_until(&self, _: &atlas::brain::ChatRequest, _: &mut dyn FnMut(&str) -> bool, keep_going: &dyn Fn() -> bool) -> atlas::error::Result<atlas::brain::ChatReply> {
        if !keep_going() { return Err(atlas::error::AtlasError::Platform("research fixture stopped".into())); }
        panic!("no sources were found, so nothing should reach the bounded model");
    }
    fn complete(&self, _s: &str, _u: &str) -> atlas::error::Result<String> {
        panic!("no sources were found, so nothing should reach the model");
    }
}

/// A search that returns a page with no links in it -- what a challenge page
/// looks like -- but that does have a web address a browser could open.
fn research_with(browser: Option<BrowserConfig>) -> Research {
    let synthetic = |text: &str, url: bool| ExternalTool {
        command: if cfg!(windows) { "cmd" } else { "sh" }.into(),
        args: if cfg!(windows) {
            let mut args = vec!["/d".into(), "/c".into(), format!("echo {text}")];
            if url { args.push("https://html.duckduckgo.com/html/?q={query}".into()); } args
        } else {
            let mut args = vec!["-c".into(), format!("echo '{text}'")];
            if url { args.push("https://html.duckduckgo.com/html/?q={query}".into()); } args
        }, ..Default::default()
    };
    let cfg = ResearchConfig {
        enabled: true,
        search: Some(synthetic("please enable javascript", true)),
        fetch: Some(synthetic("unused", false)),
        ..ResearchConfig::default()
    };
    Research { cfg, vars: Vars::new(), browser }
}

/// A browser that cannot be reached: nothing listens on port 1 and there is
/// no launch command, so `Browser::start` fails at once.
fn unreachable_browser() -> BrowserConfig {
    BrowserConfig { port: 1, timeout_ms: 200, startup_ms: 200, launch: None, ..BrowserConfig::default() }
}

#[test]
fn without_a_browser_the_original_error_is_unchanged() {
    let err = research_with(None).run("tides", &NeverCalled).unwrap_err().to_string();
    assert!(err.contains("no sources found for 'tides'"), "got: {err}");
    assert!(!err.contains("headless browser"), "got: {err}");
}

#[test]
fn a_browser_that_cannot_start_falls_through_to_no_sources_and_says_it_tried() {
    let err = research_with(Some(unreachable_browser()))
        .run("tides", &NeverCalled)
        .unwrap_err()
        .to_string();
    assert!(err.contains("no sources found for 'tides'"), "the original message is kept: {err}");
    assert!(err.contains("(the headless browser found nothing either)"), "got: {err}");
}
