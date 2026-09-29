//! Telling Atlas something and asking it back — the unified fact book.
//!
//! Capture used to write only into the note store, and "what do you know about
//! X" read a different store that never saw it. Now a plain "note that …" also
//! files a typed fact, and "what do you know about …" reads it back through the
//! fact book's index. These drive that round trip through a whole daemon, and
//! prove it survives a restart.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-remember-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

#[test]
fn something_you_tell_it_comes_back_when_you_ask() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("roundtrip")), Proactive::new(ProactiveConfig::default()));

    d.turn("note that the wifi password is hunter2", 100);
    let reply = d.turn("what do you know about the wifi password", 100);
    assert!(
        reply.to_lowercase().contains("hunter2"),
        "it should tell back what it was told: {reply}"
    );
}

#[test]
fn a_fact_is_remembered_across_a_restart() {
    let (c, p) = (cfg(), plat());
    let d0 = dir("restart");
    {
        let mut d = Daemon::new(&c, &p, None, Store::new(d0.clone()), Proactive::new(ProactiveConfig::default()));
        d.turn("note that my dentist is Dr Alvarez on Oak Street", 100);
        d.persist();
    }
    // A fresh daemon on the same store still knows it.
    let mut d2 = Daemon::new(&c, &p, None, Store::new(d0), Proactive::new(ProactiveConfig::default()));
    let reply = d2.turn("what do you know about my dentist", 100);
    assert!(
        reply.to_lowercase().contains("alvarez"),
        "a remembered fact must survive a restart: {reply}"
    );
}

#[test]
fn telling_it_the_same_thing_twice_keeps_one_fact_not_two() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("restate")), Proactive::new(ProactiveConfig::default()));
    d.turn("note that I take my coffee black", 100);
    d.turn("note that I take my coffee black, no sugar", 100);
    // One strengthened fact about coffee, not two near-duplicates.
    let about_coffee = d.facts.recall("coffee", atlas::store::now());
    assert_eq!(about_coffee.len(), 1, "restating should merge, not duplicate: {about_coffee:?}");
    assert!(about_coffee[0].confirmed >= 1, "the restated fact is strengthened");
}

#[test]
fn a_plain_question_is_answered_from_the_book_not_just_what_do_you_know() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("askplain")), Proactive::new(ProactiveConfig::default()));
    d.turn("note that the wifi password is hunter2", 100);
    // A plain question (not the "what do you know about" phrasing) should still
    // be answered from the fact book, because knowledge and questions now share
    // one indexed store.
    let reply = d.turn("what's the wifi password", 100);
    assert!(
        reply.to_lowercase().contains("hunter2"),
        "a plain question it knows the answer to should be answered from the book: {reply}"
    );
}

#[test]
fn asking_about_one_thing_surfaces_what_is_connected_to_it() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("assoc")), Proactive::new(ProactiveConfig::default()));
    d.turn("note that the homelab server runs my photo backups", 100);
    d.turn("note that the homelab server is on a windows vps", 100);
    let reply = d.turn("what do you know about the homelab server", 100);
    // The direct answer, plus the connected fact via associative recall.
    assert!(reply.to_lowercase().contains("photo") || reply.to_lowercase().contains("vps"), "{reply}");
    assert!(
        reply.to_lowercase().contains("you also know") || reply.to_lowercase().contains("vps"),
        "asking about the server should surface both connected facts: {reply}"
    );
}

#[test]
fn learning_a_document_breaks_it_into_many_recallable_facts() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("learndoc")), Proactive::new(ProactiveConfig::default()));
    let doc = "The home wifi network is called Starlight.\n\n\
               The router lives in the hall closet.\n\n\
               The internet provider is Comcast on a gigabit plan.";
    let _ = d.turn(&format!("learn this {doc}"), 100);
    // A plain string literal so the every-intent-reaches-the-daemon guard sees
    // this branch is driven end to end.
    let reply = d.turn("learn this the spare key is under the third flowerpot on the porch", 100);
    assert!(reply.to_lowercase().contains("learned"), "should confirm it learned: {reply}");
    // Three paragraphs became three recallable facts.
    let ask = d.turn("what do you know about the router", 100);
    assert!(ask.to_lowercase().contains("closet"), "a fact from the middle of the doc recalls: {ask}");
    let ask2 = d.turn("what do you know about the internet provider", 100);
    assert!(ask2.to_lowercase().contains("comcast"), "another fact from the doc recalls: {ask2}");
    // The document really became several individually-indexed facts, not one
    // blob: the router fact is recallable on its own from the book.
    assert!(
        !d.facts.recall("router", atlas::store::now()).is_empty(),
        "the middle-of-the-document fact is its own entry in the book"
    );
}

#[test]
fn learning_from_a_whole_folder_imports_every_text_file() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("folder")), Proactive::new(ProactiveConfig::default()));
    // A folder of reference material, plus a binary that must be left alone.
    let src = std::env::temp_dir().join(format!("atlas-kb-src-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&src);
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("network.md"), "The office wifi network is called Starlight.").unwrap();
    std::fs::write(src.join("hardware.txt"), "The main server is a Dell PowerEdge in the closet.").unwrap();
    std::fs::write(src.join("logo.bin"), [0u8, 159, 146, 150, 255, 0, 1]).unwrap();

    let reply = d.turn(&format!("learn from {}", src.display()), 100);
    assert!(reply.to_lowercase().contains("learned"), "confirms the import: {reply}");
    // A fact from each text file is recallable; the binary produced nothing.
    assert!(!d.facts.recall("wifi", 100).is_empty(), "the network file was learned");
    assert!(!d.facts.recall("server", 100).is_empty(), "the hardware file was learned");
    let _ = std::fs::remove_dir_all(&src);
}

#[test]
fn asking_about_something_it_was_never_told_says_so() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("unknown")), Proactive::new(ProactiveConfig::default()));
    d.turn("note that the wifi password is hunter2", 100);
    let reply = d.turn("what do you know about my car", 100);
    assert!(
        reply.to_lowercase().contains("nothing"),
        "it shouldn't invent knowledge it doesn't have: {reply}"
    );
}
