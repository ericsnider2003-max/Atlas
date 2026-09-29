use atlas::awareness::Signals;
use atlas::index::{Index, IndexConfig};
use atlas::input::{HoldToTalk, KeyEvent};
use atlas::lanes::{lane_for, Lane, LaneConfig, Queue, TaskState};
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::{Button, Monitor, Platform};
use atlas::research::{extract_urls, first_sentences, strip_html, urlencode};
use std::fs;
use std::path::PathBuf;

fn signals(idle: u64, talking: bool) -> Signals {
    Signals { idle_secs: idle, in_conversation: talking, ..Default::default() }
}
fn cfg() -> LaneConfig {
    LaneConfig::default()
}

// ============ hold-to-talk on a key you also type with ============

#[test]
fn a_normal_tab_press_passes_straight_through() {
    let mut h = HoldToTalk::new("tab", 300);
    h.down(1000);
    assert_eq!(h.up(1080), KeyEvent::PassThrough, "80ms is a tab, not a hold");
    assert!(!h.is_talking());
}

#[test]
fn holding_tab_starts_listening_before_you_let_go() {
    // You should hear it start, not discover afterwards that it did.
    let mut h = HoldToTalk::new("tab", 300);
    assert_eq!(h.down(1000), KeyEvent::Waiting);
    assert_eq!(h.poll(1200), KeyEvent::Waiting);
    assert_eq!(h.poll(1300), KeyEvent::StartTalking);
    assert!(h.is_talking());
    assert_eq!(h.up(2500), KeyEvent::StopTalking);
}

#[test]
fn rapid_tabbing_never_triggers_push_to_talk() {
    let mut h = HoldToTalk::new("tab", 300);
    for i in 0..20 {
        let t = 1000 + i * 90;
        h.down(t);
        assert_eq!(h.up(t + 40), KeyEvent::PassThrough);
    }
}

#[test]
fn the_hold_only_fires_once_per_press() {
    let mut h = HoldToTalk::new("tab", 300);
    h.down(1000);
    assert_eq!(h.poll(1400), KeyEvent::StartTalking);
    assert_eq!(h.poll(1500), KeyEvent::Waiting, "already talking");
    assert_eq!(h.poll(1600), KeyEvent::Waiting);
}

// ============ working while Atlas works ============

#[test]
fn background_work_starts_immediately_even_while_you_type() {
    let mut q = Queue::default();
    q.push("research IETF QUIC v1 framing", Lane::Background);
    let ready = q.ready(&signals(0, false), &cfg(), 100);
    assert_eq!(ready.len(), 1, "research must not wait for you to stop working");
}

#[test]
fn screen_work_waits_for_a_gap_instead_of_stealing_focus() {
    let mut q = Queue::default();
    let id = q.push("open chrome", Lane::Foreground);
    assert!(q.ready(&signals(2, false), &cfg(), 100).is_empty(), "you are mid-keystroke");
    assert_eq!(q.tasks[0].state, TaskState::WaitingForGap);

    let ready = q.ready(&signals(60, false), &cfg(), 200);
    assert_eq!(ready, vec![id], "gap appeared, take the screen");
}

#[test]
fn atlas_does_not_grab_the_screen_mid_conversation() {
    let mut q = Queue::default();
    q.push("open chrome", Lane::Foreground);
    assert!(q.ready(&signals(999, true), &cfg(), 100).is_empty());
}

#[test]
fn one_screen_task_at_a_time_many_background_ones() {
    let mut q = Queue::default();
    q.push("open chrome", Lane::Foreground);
    q.push("focus discord", Lane::Foreground);
    q.push("research a", Lane::Background);
    q.push("research b", Lane::Background);
    q.push("research c", Lane::Background);

    let ready = q.ready(&signals(999, false), &cfg(), 100);
    let fg = ready.iter().filter(|id| q.tasks.iter().any(|t| t.id == **id && t.lane == Lane::Foreground)).count();
    let bg = ready.len() - fg;
    assert_eq!(fg, 1, "only one thing may own the screen");
    assert_eq!(bg, 2, "background slots default to 2");
}

#[test]
fn a_running_screen_task_keeps_the_screen_until_it_finishes() {
    let mut q = Queue::default();
    let a = q.push("open chrome", Lane::Foreground);
    q.ready(&signals(999, false), &cfg(), 100);
    q.push("focus discord", Lane::Foreground);
    assert!(q.ready(&signals(999, false), &cfg(), 110).is_empty());
    q.finish(a, "ok", true);
    assert_eq!(q.ready(&signals(999, false), &cfg(), 120).len(), 1);
}

#[test]
fn screen_work_that_never_gets_a_gap_gives_up_rather_than_lurking() {
    let mut q = Queue::default();
    q.push_at("open chrome", Lane::Foreground, 100);
    q.ready(&signals(0, false), &cfg(), 100);
    q.ready(&signals(0, false), &cfg(), 100_000);
    assert_eq!(q.tasks[0].state, TaskState::Failed);
    assert!(q.tasks[0].result.as_ref().unwrap().contains("free moment"));
}

#[test]
fn commands_are_routed_to_the_right_lane_automatically() {
    assert_eq!(lane_for("research quantum error correction"), Lane::Background);
    assert_eq!(lane_for("search my documents for the invoice"), Lane::Background);
    assert_eq!(lane_for("open chrome"), Lane::Foreground);
    assert_eq!(lane_for("boot workspace"), Lane::Foreground);
    assert_eq!(lane_for("scroll down"), Lane::Foreground);
    assert_eq!(lane_for("view my display"), Lane::Foreground);
}

#[test]
fn the_queue_survives_a_restart() {
    let d = std::env::temp_dir().join("atlas-queue-test");
    let _ = fs::remove_dir_all(&d);
    let store = atlas::store::Store::new(&d);
    let mut q = Queue::default();
    q.push("research something", Lane::Background);
    q.save(&store).unwrap();
    assert_eq!(Queue::load(&store).pending(), 1);
}

// ============ driving the workspace ============

#[test]
fn atlas_can_click_scroll_and_type() {
    let p = MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }]);
    p.click(400, 300, Button::Left).unwrap();
    p.scroll(0, -3).unwrap();
    p.type_text("hello there").unwrap();
    p.press("ctrl+f").unwrap();
    assert_eq!(
        p.actions(),
        vec![
            Action::Click(400, 300, Button::Left),
            Action::Scroll(0, -3),
            Action::Type("hello there".into()),
            Action::Press("ctrl+f".into()),
        ]
    );
}

// ============ research ============

#[test]
fn links_are_pulled_out_of_a_results_page() {
    let html = r#"<a href="https://example.com/article-one">One</a>
                  <a href="https://research.org/paper/12345">Two</a>
                  <a href="/relative/path">skip</a>"#;
    let urls = extract_urls(html, 10);
    assert!(urls.contains(&"https://example.com/article-one".to_string()));
    assert!(urls.contains(&"https://research.org/paper/12345".to_string()));
    assert_eq!(urls.len(), 2, "relative links are not sources: {urls:?}");
}

#[test]
fn assets_and_the_search_engine_itself_are_not_treated_as_sources() {
    let html = r#"https://duckduckgo.com/?q=x https://cdn.site.com/a.js
                  https://site.com/style.css https://good.example.com/real-article-here"#;
    assert_eq!(extract_urls(html, 10), vec!["https://good.example.com/real-article-here"]);
}

#[test]
fn duplicate_links_are_only_fetched_once() {
    let html = "https://example.com/aaaaaaaa https://example.com/aaaaaaaa https://example.com/bbbbbbbb";
    assert_eq!(extract_urls(html, 10).len(), 2);
}

#[test]
fn script_and_style_bodies_never_reach_the_model() {
    let html = "<html><body><style>body{color:red}</style>\
<script>var x = 'DO NOT SUMMARIZE ME';</script>\
<h1>Real Heading</h1><p>Actual content here.</p></body></html>";
    let text = strip_html(html);
    assert!(!text.contains("DO NOT SUMMARIZE"), "got: {text}");
    assert!(!text.contains("color:red"));
    assert!(text.contains("Real Heading"));
    assert!(text.contains("Actual content here."));
}

#[test]
fn html_entities_are_decoded_and_whitespace_collapsed() {
    assert_eq!(strip_html("<p>a &amp; b</p>\n\n   <p>c&nbsp;d</p>"), "a & b c d");
}

#[test]
fn the_spoken_summary_is_the_first_couple_of_sentences() {
    let body = "Rust is a systems language. It has no garbage collector. A third sentence follows.";
    let s = first_sentences(body, 2);
    assert_eq!(s, "Rust is a systems language. It has no garbage collector.");
}

#[test]
fn queries_are_url_encoded() {
    assert_eq!(urlencode("IETF QUIC v1 & HTTP3"), "IETF+QUIC+v1+%26+HTTP3");
}

// ============ searching inside documents ============

#[test]
fn atlas_searches_inside_files_not_just_filenames() {
    let d: PathBuf = std::env::temp_dir().join("atlas-content-search");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("notes.md"), "meeting about the Hetzner VPS migration plan").unwrap();
    fs::write(d.join("other.md"), "unrelated content").unwrap();

    let cfg: IndexConfig = serde_yaml::from_str(&format!(
        "roots: [\"{}\"]\nmax_depth: 3\nmax_enrich_mb: 20\n", d.display().to_string().replace('\\', "/")
    )).unwrap();
    let idx = Index::scan(&cfg);
    let hits = idx.search_content("hetzner vps", &cfg, 5);
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].name, "notes.md");
    assert!(hits[0].excerpt.contains("Hetzner VPS"), "excerpt shows context: {}", hits[0].excerpt);
}

#[test]
fn content_search_skips_binaries_and_oversized_files() {
    let d: PathBuf = std::env::temp_dir().join("atlas-content-skip");
    let _ = fs::remove_dir_all(&d);
    fs::create_dir_all(&d).unwrap();
    fs::write(d.join("big.md"), "needle ".repeat(10)).unwrap();
    fs::write(d.join("photo.png"), "needle").unwrap();

    let cfg: IndexConfig = serde_yaml::from_str(&format!(
        "roots: [\"{}\"]\nmax_depth: 3\nmax_enrich_mb: 0\n", d.display().to_string().replace('\\', "/")
    )).unwrap();
    let idx = Index::scan(&cfg);
    assert!(idx.search_content("needle", &cfg, 5).is_empty(), "size cap of 0 blocks everything");
}

#[test]
fn the_shipped_config_wires_research_and_push_to_talk() {
    let y = std::fs::read_to_string("config/tools.yaml").unwrap();
    let t: atlas::voice::ToolsConfig = serde_yaml::from_str(&y).unwrap();
    assert_eq!(t.push_to_talk.key, "tab");
    assert!(t.push_to_talk.hold_ms >= 250, "shorter and normal tabbing misfires");
    // Ships *off*, and that is the point of the rest of this test. Research
    // is the one thing in this file that leaves the machine, so it waits to
    // be asked for — but everything it needs must already be in place, or
    // turning it on in the hub would hand you a switch that does nothing.
    // This used to assert `enabled` was true, back when `settings.rs`'s
    // registry and `ResearchConfig::default()` disagreed about the default
    // and the shipped file followed the registry.
    // On since 27 Sep 2026 (Eric: when Atlas doesn't know, it looks it up),
    // and ready: the switch in Settings turns it off.
    assert!(t.research.enabled, "web lookup ships on");
    let fetch = t.research.fetch.expect("a fetch tool, configured and ready for when it is");
    assert!(
        fetch.args.iter().any(|a| a.contains("headless")),
        "research must not drive the browser you are using"
    );
    assert!(t.lanes.gap_secs > 0);
}
