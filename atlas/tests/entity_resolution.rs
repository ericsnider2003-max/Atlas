//! Entity resolution — the lightweight ontology layer.
//!
//! One thing can be called several names: "the homelab server", "the
//! windows vps", "the backup box". Without resolution, facts about it scatter
//! across those names and recall misses. An alias declaration folds the names
//! onto one canonical entity, so everything Atlas knows about the thing sits in
//! one place and answers to any of its names.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::facts::{alias_decl, Book, Fact};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn dir(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-entity-{tag}-{}", std::process::id()));
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

// --- the declaration is read -----------------------------------------------

#[test]
fn an_alias_declaration_names_which_is_which() {
    let (alias, canonical) =
        alias_decl("the windows vps is also known as the homelab server").expect("an alias");
    // The name led with is canonical; the one after the marker is the alias.
    assert_eq!(canonical, "windows vps");
    assert_eq!(alias, "homelab server");
}

#[test]
fn a_plain_fact_is_not_mistaken_for_an_alias() {
    assert!(alias_decl("the homelab server is on a windows vps").is_none());
}

// --- resolution at the book level ------------------------------------------

#[test]
fn facts_under_two_names_collapse_onto_one_entity() {
    let mut b = Book::default();
    b.note_alias("windows vps", "homelab server", 100);
    // A fact stated about the alias is filed under the canonical entity.
    b.learn(Fact::stated("the windows vps runs my photo backups", 100), 100);
    b.learn(Fact::stated("the homelab server is backed up nightly", 100), 100);
    // Asking by the canonical name finds both facts.
    let hits = b.recall("homelab server", 100);
    assert!(hits.len() >= 2, "both facts sit under one entity: {hits:?}");
}

#[test]
fn an_alias_declared_after_the_fact_pulls_it_over() {
    let mut b = Book::default();
    b.learn(Fact::stated("the windows vps is in the closet", 100), 100);
    // The fact was filed under "windows vps"; declaring the alias re-points it.
    b.note_alias("windows vps", "homelab server", 200);
    let hit = b.slot_answer("what do you know about the homelab server", 200);
    assert!(hit.is_some(), "the pre-existing fact now answers to the canonical name");
}

#[test]
fn asking_by_any_name_finds_the_fact() {
    let mut b = Book::default();
    b.note_alias("windows vps", "homelab server", 100);
    b.learn(Fact::stated("the homelab server is a headless box", 100), 100);
    // The fact is filed under "homelab server"; a question using the alias
    // still finds it.
    let hit = b.slot_answer("what is the windows vps", 100);
    assert!(hit.is_some(), "a question by the alias resolves to the canonical entity");
}

// --- end to end through the daemon -----------------------------------------

#[test]
fn telling_atlas_two_names_are_one_thing_joins_what_it_knows() {
    let (c, p) = (cfg(), plat());
    let mut d = Daemon::new(&c, &p, None, Store::new(dir("join")), Proactive::new(ProactiveConfig::default()));
    d.turn("note that the homelab server runs my photo backups", 100);
    let ack = d.turn("the windows vps is also known as the homelab server", 200);
    assert!(ack.to_lowercase().contains("same thing"), "the alias is acknowledged: {ack}");
    // Now asking about the windows vps surfaces what was filed under the server.
    let reply = d.turn("what do you know about the windows vps", 200);
    assert!(reply.to_lowercase().contains("photo"), "knowledge answers to either name: {reply}");
    // The book actually resolves both names to the same recall result, not by
    // coincidence of wording.
    let by_vps = d.facts.recall("windows vps", 200).len();
    let by_server = d.facts.recall("homelab server", 200).len();
    assert_eq!(by_vps, by_server, "either name reaches the same facts: {by_vps} vs {by_server}");
    assert!(by_vps >= 1, "and there is at least the photo fact under the entity");
}
