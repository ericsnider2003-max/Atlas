//! The getting-things-done sweep (30 Sep 2026): notes kept and found again,
//! drafts sent, research write-ups reachable, documents written, messages
//! for you actually said, and settings never wiped by a file that won't read.

use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-gtd-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn cfg() -> atlas::config::Config {
    atlas::config::Config::load(Path::new("config")).unwrap()
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}
const NOW: u64 = 1_790_776_800;

#[test]
fn a_note_is_kept_and_found_again() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("notes")), Proactive::new(ProactiveConfig::default()));
    let kept = d.turn("note that the broker fee is 25 dollars a month", NOW);
    assert!(!kept.to_lowercase().contains("couldn't"), "{kept}");
    let found = d.turn("where's that note about the broker fee", NOW + 60);
    assert!(found.contains("25 dollars"), "{found}");
    assert!(d.turn("find my note about the gym", NOW + 70).starts_with("I can't find a note like that"));
}

#[test]
fn a_capture_that_reads_as_a_whole_item_is_kept_too() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("made")), Proactive::new(ProactiveConfig::default()));
    let before = d.notebook.notes.len();
    let said = d.turn("note this: record a video called Tuesday tips, due Friday", NOW);
    assert!(d.notebook.notes.len() > before, "announced ({said}) and kept");
}

#[test]
fn drafts_are_listed_and_sending_says_what_it_needs() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("drafts")), Proactive::new(ProactiveConfig::default()));
    assert_eq!(d.turn("what drafts are waiting", NOW), "No drafts waiting.");
    assert_eq!(d.turn("send the reply to Jane", NOW), "I don't have a draft waiting for jane.");
}

#[test]
fn duckduckgo_results_are_read_through_its_redirect() {
    let html = r#"<a class="result__a" href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fexample.org%2Fsolar%2Dpanels&amp;rut=abc">Solar</a>
<a href="//duckduckgo.com/l/?uddg=https%3A%2F%2Fen.wikipedia.org%2Fwiki%2FSolar_panel&amp;rut=def">Wiki</a>"#;
    let urls = atlas::research::extract_urls(html, 5);
    assert_eq!(urls, vec!["https://example.org/solar-panels".to_string(), "https://en.wikipedia.org/wiki/Solar_panel".to_string()]);
}

#[test]
fn the_full_research_write_up_can_be_heard() {
    let (mut c, p) = (cfg(), plat());
    let notes = tmp("research-notes");
    c.tools.as_mut().unwrap().research.notes_dir = notes.display().to_string();
    std::fs::write(notes.join("1790000000-solar.md"), "# solar\n\nPanels lose about half a percent a year.\n\n## Sources\n- https://example.org\n").unwrap();
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("research")), Proactive::new(ProactiveConfig::default()));
    let heard = d.turn("read me the full brief", NOW);
    assert_eq!(heard, "Panels lose about half a percent a year.", "the write-up, without its title or source list");
}

#[test]
fn a_letter_asked_for_is_written_not_built() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("writing")), Proactive::new(ProactiveConfig::default()));
    // No model here: it says what it needs, rather than building a program.
    let said = d.turn("write me a letter to my landlord about the heating", NOW);
    assert_eq!(said, "I need a language model to write that, and I haven't got one yet.");
    assert!(matches!(d.parser.parse("write me a letter to my landlord"), atlas::intent::Intent::Unknown(_)), "not a program to build");
}

#[test]
fn a_note_routed_to_speaking_is_said() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(tmp("speak")), Proactive::new(ProactiveConfig::default()));
    d.to_say_aloud.push("Your research on solar is ready.".into());
    let out = d.tick(NOW);
    assert!(out.iter().any(|l| l.contains("research on solar")), "{out:?}");
    assert!(d.to_say_aloud.is_empty());
}

#[test]
fn a_settings_file_that_wont_read_is_not_overwritten() {
    let dir = tmp("prefs");
    for f in std::fs::read_dir("config").unwrap().flatten() {
        if f.path().is_file() {
            std::fs::copy(f.path(), dir.join(f.file_name())).unwrap();
        }
    }
    let prefs = atlas::preferences::Preferences::file(&dir);
    std::fs::write(&prefs, "wake.enabled: [unclosed\n").unwrap();
    let said = atlas::settingswin::keep_setting(&dir, "wake.enabled", "off");
    let err = said.expect_err("refused");
    assert!(err.starts_with("I haven't changed anything"), "{err}");
    assert_eq!(std::fs::read_to_string(&prefs).unwrap(), "wake.enabled: [unclosed\n", "left as it was");
}
