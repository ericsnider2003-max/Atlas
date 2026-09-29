//! **Web research, read well** (28 Sep 2026).
//!
//! Two things, both from projects with the reach to trust:
//!
//! * `mozilla/readability` (Firefox Reader View) was already how `readable`
//!   picks the article. What it lacked: hidden parts of a page were read as if
//!   shown (Readability's `_isProbablyVisible`), and the article came out as
//!   flat lines -- a heading, a list and a code block all the same shape.
//!   It now also comes out as Markdown, which is what research writes up from.
//! * `searxng/searxng`: your own metasearch, asked for JSON
//!   (`research.searxng_url`); the old search tool when it's unset, or finds
//!   nothing.
//!
//! Fixtures: the three saved pages in `tests/fixtures/crawl`, and a stand-in
//! SearXNG and web server on this machine.

use atlas::brain::Llm;
use atlas::research::{page_markdown, page_text, searxng_url, urls_from_searxng, Research, ResearchConfig};
use atlas::tools::ExternalTool;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!("tests/fixtures/crawl/{name}.html")).unwrap()
}

// ================= Markdown out of real pages =================

#[test]
fn a_real_page_keeps_its_headings_and_code_as_markdown() {
    let md = page_markdown(&fixture("python-classes"));
    assert!(md.lines().any(|l| l.starts_with('#') && l.contains("Classes")), "no heading:\n{}", &md[..md.len().min(800)]);
    // The code example is fenced, whole lines intact.
    let fence = md.find("```").expect("no code fence");
    let code = &md[fence..];
    assert!(code.contains("valedictorian = max((student.gpa, student.name) for student in graduates)"), "{}", &code[..code.len().min(600)]);
    // The furniture stays out, as it does from `page_text`.
    for junk in ["Previous topic", "Report a bug", "Show source", "Python Software Foundation"] {
        assert!(!md.contains(junk), "kept {junk:?}");
    }
    let go = page_markdown(&fixture("go-loopvar"));
    assert!(go.lines().any(|l| l.starts_with("#") && l.contains("The Problem")), "{}", &go[..go.len().min(1200)]);
    assert!(go.contains("```"), "the Go examples are code");
    assert!(!go.contains("Why Go") && !go.contains("Copyright"));
}

#[test]
fn lists_quotes_and_tables_keep_their_shape() {
    let html = r#"<html><head><title>How to repot a fern</title></head><body>
      <nav class=menu><a href=/>Home</a></nav>
      <article class="post-body">
        <h2>What you need</h2>
        <p>Repotting a fern is easy, quick, and cheap, and it keeps the plant healthy for another year or two at least.</p>
        <ul><li>A pot one size up</li><li>Fresh peat-free compost, a <a href=/x>good brand</a></li></ul>
        <h2>Steps</h2>
        <ol><li>Water the day before.</li><li>Ease it out, roots and all.</li></ol>
        <blockquote><p>Never bury the crown, or it will rot, sooner or later, whatever you do.</p></blockquote>
        <table><tr><th>Pot</th><th>Compost</th></tr><tr><td>12 cm</td><td>1 litre</td></tr></table>
        <p>That is all there is to it, and your fern will thank you, as ferns do, in their own way.</p>
      </article>
      <footer><p>All rights reserved, forever, and ever, amen.</p></footer></body></html>"#;
    let a = atlas::readable::extract(html);
    let md = &a.markdown;
    assert!(md.contains("## What you need"), "{md}");
    assert!(md.contains("- A pot one size up"), "{md}");
    assert!(md.contains("- Fresh peat-free compost, a good brand"), "link words kept, address dropped: {md}");
    assert!(md.contains("1. Water the day before.\n2. Ease it out, roots and all."), "{md}");
    assert!(md.contains("> Never bury the crown"), "{md}");
    assert!(md.contains("Pot | Compost\n12 cm | 1 litre"), "{md}");
    assert!(!md.contains("rights reserved") && !md.contains("Home"), "{md}");
    // The plain text is unchanged in kind: no Markdown marks in it.
    assert!(!a.text.contains("## ") && !a.text.contains("- A pot"), "{}", a.text);
}

#[test]
fn what_the_page_hides_is_not_read() {
    let html = r#"<html><body><div class="content">
      <p>The visible article paragraph is long enough, with commas, clauses, and detail, to be the article.</p>
      <p hidden>A hidden paragraph that says something nobody reading the page would ever see at all.</p>
      <div style="display: none"><p>A collapsed panel of text that is in the markup but not on the screen.</p></div>
      <p aria-hidden="true">Text for decoration only, hidden from screen readers and from Atlas too.</p>
      <p class="note hidden-xs">A shown paragraph whose class merely contains the word hidden, which counts.</p>
      <p>The second visible paragraph carries on, again with commas, clauses, and enough words to count.</p>
    </div></body></html>"#;
    let t = page_text(html);
    assert!(t.contains("visible article paragraph") && t.contains("second visible paragraph"), "{t}");
    assert!(t.contains("merely contains the word hidden"), "a class is not the hidden attribute: {t}");
    for gone in ["hidden paragraph that says", "collapsed panel", "decoration only"] {
        assert!(!t.contains(gone), "read what the page hides: {gone:?}\n{t}");
    }
}

// ================= SearXNG =================

const SEARX_JSON: &str = r#"{"query": "ferns", "number_of_results": 4, "results": [
  {"url": "https://www.rhs.org.uk/ferns", "title": "Ferns | RHS", "content": "Growing ferns", "engine": "duckduckgo"},
  {"url": "https://cdn.example.com/app.js", "title": "noise", "engine": "x"},
  {"url": "https://en.wikipedia.org/wiki/Fern", "title": "Fern", "engine": "wikipedia"},
  {"url": "https://www.rhs.org.uk/ferns", "title": "Ferns again", "engine": "bing"},
  {"url": "ftp://old.example.com/fern.txt", "title": "old"},
  {"url": "https://gardening.example.org/repot", "title": "Repotting"}
], "answers": [], "suggestions": []}"#;

#[test]
fn searxng_results_are_read_in_order_without_noise_or_repeats() {
    assert_eq!(searxng_url("http://127.0.0.1:8888/", "boston ferns"), "http://127.0.0.1:8888/search?q=boston+ferns&format=json");
    let urls = urls_from_searxng(SEARX_JSON, 5).unwrap();
    assert_eq!(urls, ["https://www.rhs.org.uk/ferns", "https://en.wikipedia.org/wiki/Fern", "https://gardening.example.org/repot"]);
    assert_eq!(urls_from_searxng(SEARX_JSON, 2).unwrap().len(), 2);
    // An instance with JSON turned off answers with a page, not results.
    assert!(urls_from_searxng("<html>Too many requests</html>", 5).is_none());
    assert_eq!(urls_from_searxng(r#"{"results": []}"#, 5), Some(vec![]));
}

/// A stand-in SearXNG and web: `/search` answers JSON pointing at two
/// articles on the same server, `/html` answers a results page for the old
/// search tool, and `/a1`, `/a2` are articles. Every request's path is kept.
fn stand_in() -> (String, Arc<Mutex<Vec<String>>>) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let (b, s) = (base.clone(), seen.clone());
    std::thread::spawn(move || {
        for conn in l.incoming() {
            let Ok(mut c) = conn else { continue };
            let mut r = BufReader::new(c.try_clone().unwrap());
            let mut first = String::new();
            let _ = r.read_line(&mut first);
            loop {
                let mut h = String::new();
                if r.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
            }
            let path = first.split_whitespace().nth(1).unwrap_or("/").to_string();
            s.lock().unwrap().push(path.clone());
            let article = |h: &str, body: &str| {
                format!(
                    "<html><head><title>{h}</title></head><body><nav class=menu><a href=/>Home</a></nav>\
                     <div class=article-body><h2>{h}</h2><p>{body}, which is long enough, with commas, to be the article.</p>\
                     <p>A second paragraph about {h}, with more words, clauses, and detail, so it reads as content.</p>\
                     <ul><li>First point about {h}</li><li>Second point about {h}</li></ul></div></body></html>"
                )
            };
            let body = if path.starts_with("/search") {
                format!(r#"{{"results": [{{"url": "{b}/a1"}}, {{"url": "{b}/a2"}}]}}"#)
            } else if path.starts_with("/html") {
                format!(r#"<html><body><a href="{b}/a2">Result</a></body></html>"#)
            } else if path == "/a1" {
                article("Fern light", "Ferns like bright, indirect light")
            } else {
                article("Fern water", "Ferns like their soil damp but never soggy")
            };
            let _ = write!(c, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
        }
    });
    (base, seen)
}

struct Writer(Mutex<Vec<String>>);
impl Llm for Writer {
    fn complete(&self, _system: &str, user: &str) -> atlas::error::Result<String> {
        self.0.lock().unwrap().push(user.to_string());
        Ok("Ferns want bright, indirect light and damp soil. Both sources agree.".into())
    }
}

fn curl(url: &str) -> ExternalTool {
    ExternalTool { command: "curl".into(), args: vec!["-s".into(), url.into()], ..Default::default() }
}

#[test]
fn research_searches_with_searxng_and_reads_the_pages_as_markdown() {
    let (base, seen) = stand_in();
    let cfg = ResearchConfig {
        searxng_url: base.clone(),
        // The old search tool is there, and not used when SearXNG answers.
        search: Some(curl(&format!("{base}/html?q={{query}}"))),
        fetch: Some(curl("{url}")),
        ..Default::default()
    };
    let r = Research { cfg, vars: Default::default(), browser: None };
    let llm = Writer(Mutex::new(Vec::new()));
    let note = r.run("how to look after ferns", &llm).unwrap();
    let got: Vec<&str> = note.sources.iter().map(|s| s.url.as_str()).collect();
    assert_eq!(got, [format!("{base}/a1"), format!("{base}/a2")]);
    let paths = seen.lock().unwrap().clone();
    assert!(paths[0].starts_with("/search?q=how+to+look+after+ferns&format=json"), "{paths:?}");
    assert!(!paths.iter().any(|p| p.starts_with("/html")), "the old search tool wasn't needed: {paths:?}");
    let asked = llm.0.lock().unwrap().join("\n");
    assert!(asked.contains("> ## Fern light") && asked.contains("> - First point about Fern water"), "Markdown, and quoted: {asked}");
    assert!(!asked.contains("Home"), "{asked}");
}

#[test]
fn a_searxng_that_isnt_there_falls_back_to_the_search_tool() {
    let (base, seen) = stand_in();
    // A port nothing listens on.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let dead_url = format!("http://{}", dead.local_addr().unwrap());
    drop(dead);
    let cfg = ResearchConfig {
        searxng_url: dead_url,
        search: Some(curl(&format!("{base}/html?q={{query}}"))),
        fetch: Some(curl("{url}")),
        ..Default::default()
    };
    let r = Research { cfg, vars: Default::default(), browser: None };
    let note = r.run("ferns", &Writer(Mutex::new(Vec::new()))).unwrap();
    assert_eq!(note.sources.len(), 1);
    assert!(note.sources[0].url.ends_with("/a2"));
    assert!(seen.lock().unwrap().iter().any(|p| p.starts_with("/html")));
}

#[test]
fn the_shipped_setting_leaves_searxng_off() {
    let c = atlas::config::Config::load(std::path::Path::new("config")).unwrap();
    assert_eq!(c.tools.unwrap().research.searxng_url, "", "off unless you run one");
}
