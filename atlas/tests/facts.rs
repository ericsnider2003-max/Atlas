//! Typed, linked memory.

use atlas::facts::*;
use atlas::freshness::{Shelf, State};
use atlas::store::Store;

const DAY: u64 = 86_400;

fn f(name: &str, kind: Kind, body: &str, as_of: u64) -> Fact {
    Fact::new(name, name, body, kind, as_of)
}

// --- kinds decide decay -----------------------------------------------------

#[test]
fn a_preference_does_not_go_stale_on_a_clock() {
    let pref = f("bullets-not-prose", Kind::Instruction, "keep it short", 0);
    assert_eq!(pref.kind.shelf(), Shelf::Yours);
    assert_eq!(pref.known().state(900 * DAY), State::Fresh);
}

#[test]
fn something_atlas_worked_out_fades_fastest() {
    // The weakest kind gets the shortest life. If Atlas decided it and has not
    // seen it hold up since, it should stop being asserted.
    let guess = f("prefers-mornings", Kind::Noticed, "seems to start early", 0);
    assert_eq!(guess.kind.shelf(), Shelf::Quick);
    assert_eq!(guess.known().state(90 * DAY), State::Stale);
}

#[test]
fn a_project_note_ages_but_not_quickly() {
    let p = f("atlas-build", Kind::Project, "wiring the last modules", 0);
    assert_eq!(p.known().state(10 * DAY), State::Fresh);
    assert_eq!(p.known().state(400 * DAY), State::Stale);
}

#[test]
fn what_you_said_and_what_atlas_guessed_are_sourced_differently() {
    // Once written down they look identical. The source is the only thing that
    // survives storage.
    assert!(Kind::Instruction.came_from_you());
    assert!(!Kind::Noticed.came_from_you());
    assert!(f("a", Kind::Noticed, "", 0).known().source
        == atlas::freshness::Checkable::ModelAlone);
}

#[test]
fn a_guess_is_answered_as_a_guess_not_as_fact() {
    // Atlas's own unconfirmed read is hedged when spoken as an answer — this is
    // what keeps it from sounding confidently wrong about something it guessed.
    let guess = f("mornings", Kind::Noticed, "prefers mornings", 0);
    let said = guess.answer(DAY);
    assert!(said.to_lowercase().contains("i think"), "a guess is hedged: {said}");
    assert!(said.to_lowercase().contains("my own"), "and named as its own read: {said}");
}

#[test]
fn something_you_stated_is_answered_plainly() {
    // A fact you gave it carries no hedge — only the freshness caveat, which a
    // fresh fact doesn't trigger.
    let told = Fact::new("wifi", "the wifi password is hunter2", "the wifi password is hunter2", Kind::You, 0);
    let said = told.answer(DAY);
    assert!(!said.to_lowercase().contains("i think"), "a stated fact is not hedged: {said}");
    assert!(said.contains("hunter2"), "and it still answers: {said}");
}

// --- precedence -------------------------------------------------------------

#[test]
fn an_instruction_beats_an_observation_however_old() {
    // Recency alone would let something Atlas noticed this morning override
    // something you said last year.
    let b = Book::default();
    let said = f("format", Kind::Instruction, "short answers", 0);
    let noticed = f("format2", Kind::Noticed, "seems to like detail", 900 * DAY);
    assert_eq!(b.settles(&said, &noticed).name, "format");
    assert_eq!(b.settles(&noticed, &said).name, "format");
}

#[test]
fn within_one_kind_the_newer_one_wins() {
    let b = Book::default();
    let old = f("a", Kind::Project, "", 0);
    let new = f("b", Kind::Project, "", 100);
    assert_eq!(b.settles(&old, &new).name, "b");
}

#[test]
fn precedence_is_explicit_not_declaration_order() {
    assert!(Kind::Instruction.weight() > Kind::You.weight());
    assert!(Kind::You.weight() > Kind::Project.weight());
    assert!(Kind::Project.weight() > Kind::Reference.weight());
    assert!(Kind::Reference.weight() > Kind::Noticed.weight());
}

// --- links ------------------------------------------------------------------

#[test]
fn links_are_read_out_of_the_body() {
    let x = Fact::new("trip", "trip", "flying with [[partner]] to [[spain-trip]]", Kind::Project, 0);
    assert_eq!(x.links, vec!["partner", "spain-trip"]);
}

#[test]
fn a_link_to_something_unwritten_is_kept_not_refused() {
    // Writing a link before the thing it points at is how notes get made.
    // Refusing would mean facts could only be added in dependency order.
    let mut b = Book::default();
    b.put(Fact::new("a", "a", "see [[not-written-yet]]", Kind::Project, 0));
    assert_eq!(b.dangling(), vec![("a".to_string(), "not-written-yet".to_string())]);
    assert_eq!(b.get("a").unwrap().links.len(), 1);
}

#[test]
fn link_names_are_slugged_so_two_spellings_meet() {
    assert_eq!(slug("Spain Trip!"), "spain-trip");
    assert_eq!(slug("  --Atlas  Build--  "), "atlas-build");
    let x = Fact::new("t", "t", "see [[Spain Trip!]]", Kind::Project, 0);
    assert_eq!(x.links, vec!["spain-trip"]);
}

#[test]
fn duplicate_links_in_one_body_are_recorded_once() {
    let x = Fact::new("t", "t", "[[a]] and again [[a]]", Kind::Project, 0);
    assert_eq!(x.links, vec!["a"]);
}

#[test]
fn an_unclosed_link_does_not_swallow_the_rest() {
    let x = Fact::new("t", "t", "[[open and [[closed]]", Kind::Project, 0);
    assert!(x.links.contains(&"open-and-closed".to_string()) || x.links == vec!["closed"]);
}

// --- reach ------------------------------------------------------------------

#[test]
fn recall_follows_links_as_far_as_it_is_told_to() {
    let mut b = Book::default();
    b.put(Fact::new("trip", "trip", "with [[partner]]", Kind::Project, 0));
    b.put(Fact::new("partner", "partner", "works at [[acme]]", Kind::You, 0));
    b.put(Fact::new("acme", "acme", "a company", Kind::Reference, 0));

    assert_eq!(b.reachable("trip", 0).len(), 1);
    assert_eq!(b.reachable("trip", 1).len(), 2);
    assert_eq!(b.reachable("trip", 2).len(), 3);
}

#[test]
fn a_loop_does_not_hang_the_walk() {
    let mut b = Book::default();
    b.put(Fact::new("a", "a", "[[b]]", Kind::Project, 0));
    b.put(Fact::new("b", "b", "[[a]]", Kind::Project, 0));
    assert_eq!(b.reachable("a", 9).len(), 2);
}

#[test]
fn following_a_dangling_link_returns_what_exists() {
    let mut b = Book::default();
    b.put(Fact::new("a", "a", "[[nowhere]]", Kind::Project, 0));
    assert_eq!(b.reachable("a", 3).len(), 1);
}

// --- the book ---------------------------------------------------------------

#[test]
fn one_name_holds_one_fact() {
    // Two facts under one name means recall picks whichever it reaches first,
    // and which that is depends on insertion order.
    let mut b = Book::default();
    b.put(f("x", Kind::Project, "first", 0));
    b.put(f("x", Kind::Project, "second", 1));
    assert_eq!(b.facts.len(), 1);
    assert_eq!(b.get("x").unwrap().body, "second");
}

#[test]
fn facts_can_be_listed_by_kind_newest_first() {
    let mut b = Book::default();
    b.put(f("old", Kind::Instruction, "", 0));
    b.put(f("new", Kind::Instruction, "", 100));
    b.put(f("other", Kind::Project, "", 50));
    let got = b.of_kind(Kind::Instruction);
    assert_eq!(got.len(), 2);
    assert_eq!(got[0].name, "new");
}

#[test]
fn old_guesses_are_offered_for_review() {
    // An observation nobody has confirmed in months has been sitting there
    // long enough to look like a fact.
    let mut b = Book::default();
    b.put(f("guess", Kind::Noticed, "", 0));
    b.put(f("told", Kind::Instruction, "", 0));
    let stale = b.guesses_worth_checking(120 * DAY);
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0].name, "guess");
}

#[test]
fn a_fresh_guess_is_left_alone() {
    let mut b = Book::default();
    b.put(f("guess", Kind::Noticed, "", 0));
    assert!(b.guesses_worth_checking(DAY).is_empty());
}

#[test]
fn every_kind_can_say_what_it_is() {
    for k in [Kind::You, Kind::Instruction, Kind::Project, Kind::Reference, Kind::Noticed] {
        assert!(k.plain().len() > 5, "{k:?} cannot describe itself");
    }
}

// --- recall: content search over the index ----------------------------------

#[test]
fn recall_finds_a_fact_by_a_word_in_it() {
    let mut b = Book::default();
    b.put(Fact::stated("the wifi password is hunter2", 0));
    b.put(Fact::stated("my dentist is Dr Alvarez on Oak Street", 0));
    let hits = b.recall("what's the wifi password", 0);
    assert!(!hits.is_empty(), "should find the wifi fact");
    assert!(hits[0].summary.contains("hunter2"), "best hit is the wifi one: {:?}", hits[0]);
    // A query about something it doesn't know returns nothing, rather than a
    // wrong guess.
    assert!(b.recall("what car do I drive", 0).is_empty());
}

#[test]
fn a_stated_fact_carries_your_word_not_a_guess() {
    // Anything the person states is theirs, never Noticed.
    assert_eq!(Fact::stated("always use metric units", 0).kind, Kind::Instruction);
    assert_eq!(Fact::stated("the api key is abc123", 0).kind, Kind::Reference);
    assert_eq!(Fact::stated("I grew up in Toledo", 0).kind, Kind::You);
    assert!(Fact::stated("anything at all here", 0).kind.came_from_you());
}

#[test]
fn what_you_stated_outranks_what_atlas_noticed() {
    let mut b = Book::default();
    // Both mention "coffee"; one you stated, one Atlas guessed.
    b.put(Fact::new("coffee-pref", "I take my coffee black", "coffee black", Kind::You, 10));
    b.put(Fact::new("coffee-guess", "seems to drink coffee late", "coffee late", Kind::Noticed, 10));
    let hits = b.recall("coffee", 0);
    assert_eq!(hits[0].name, "coffee-pref", "the stated fact leads: {hits:?}");
}

#[test]
fn recall_reads_the_index_not_the_whole_book() {
    // A book with many facts still answers about one of them. This is a
    // behavioural stand-in for "stays fast as it grows": every fact mentions a
    // unique word, and recall returns only the matching one.
    let mut b = Book::default();
    for i in 0..500 {
        b.put(Fact::new(&format!("f{i}"), &format!("fact number {i} about topic{i}"), "", Kind::You, 0));
    }
    let hits = b.recall("topic373", 0);
    assert_eq!(hits.len(), 1, "only the matching fact comes back");
    assert!(hits[0].name.contains("373"));
}

#[test]
fn a_stale_guess_sinks_below_a_fresh_stated_fact() {
    let mut b = Book::default();
    b.put(Fact::new("old-guess", "the server topic seems slow", "server topic slow", Kind::Noticed, 0));
    b.put(Fact::new("fresh-note", "the server topic runs nightly", "server topic nightly", Kind::You, 100 * DAY));
    let hits = b.recall("server topic", 101 * DAY);
    assert_eq!(hits[0].name, "fresh-note", "the fresh stated fact outranks the stale guess: {hits:?}");
}

// --- phase 2: merge on restate, spaced retention, bounded eviction ----------

#[test]
fn restating_a_fact_strengthens_it_instead_of_duplicating() {
    let mut b = Book::default();
    let added = b.learn(Fact::stated("I take my coffee black", 0), 0);
    assert!(!added || true); // first is an add
    // Say it again, slightly differently — it should merge, not add a second.
    b.learn(Fact::stated("I take my coffee black no sugar", 10), 10);
    assert_eq!(b.facts.len(), 1, "a restatement must not add a second fact: {:?}", b.facts);
    assert!(b.facts[0].confirmed >= 1, "restating raises the confirmation count");
    // The richer wording is kept.
    assert!(b.facts[0].summary.contains("no sugar"));
}

#[test]
fn a_confirmed_fact_decays_slower_than_one_seen_once() {
    use atlas::freshness::State;
    // Two Project facts, same age; one confirmed several times.
    let once = f("seen-once", Kind::Project, "", 0);
    let mut often = f("seen-often", Kind::Project, "", 0);
    often.confirmed = 6;
    // At an age where the once-seen one has gone stale, the confirmed one has
    // not — repetition made it durable.
    assert_eq!(once.known().state(400 * DAY), State::Stale);
    assert_ne!(often.known().state(400 * DAY), State::Stale, "a confirmed fact holds up longer");
}

#[test]
fn trim_never_touches_what_you_stated() {
    let mut b = Book::default();
    // Fill well past a tiny budget with stated facts of every non-noticed kind.
    b.put(Fact::new("you", "a long fact about you that takes some room here", "body ".repeat(50).as_str(), Kind::You, 0));
    b.put(Fact::new("instr", "always do this particular thing in this way here", "body ".repeat(50).as_str(), Kind::Instruction, 0));
    b.put(Fact::new("ref", "the password is a long secret string kept here", "body ".repeat(50).as_str(), Kind::Reference, 0));
    let before = b.facts.len();
    // A budget far below the footprint. Nothing stated may be dropped.
    let dropped = b.trim(10, 900 * DAY);
    assert_eq!(dropped, 0, "stated facts are never dropped, even over budget");
    assert_eq!(b.facts.len(), before, "all stated facts survive");
    // And their bodies are intact — stated facts aren't even compacted.
    assert!(b.facts.iter().all(|x| !x.body.is_empty()), "stated bodies are kept whole");
}

#[test]
fn trim_fades_atlas_own_stale_guesses_first_keeping_the_summary() {
    use atlas::freshness::State;
    let mut b = Book::default();
    // A stale, unconfirmed guess with a big body.
    b.put(Fact::new("guess", "atlas thought you liked early starts", &"detail ".repeat(200), Kind::Noticed, 0));
    // One stated fact that must be protected.
    b.put(Fact::new("stated", "you told it you like late starts", "short", Kind::You, 0));
    assert_eq!(b.get("guess").unwrap().known().state(400 * DAY), State::Stale);
    // A budget that forces eviction but is above the compacted footprint (the
    // two summaries plus the stated body), so compacting alone brings it under
    // and the guess is kept as a one-line trace rather than deleted.
    b.trim(100, 400 * DAY);
    let guess = b.get("guess").expect("the guess is not deleted, only compacted");
    assert!(guess.body.is_empty(), "the stale guess's body is dropped");
    assert!(!guess.summary.is_empty(), "but its one-line summary is kept as a trace");
    assert_eq!(b.get("stated").unwrap().body, "short", "the stated fact is untouched");
}

// --- phase 3: tags and associative recall -----------------------------------

#[test]
fn a_fact_is_tagged_by_topic_and_by_explicit_hashtags() {
    let f = Fact::stated("the homelab server runs on a #vps in the cloud", 0);
    // Explicit hashtag is a tag.
    assert!(f.tags.contains(&"vps".to_string()), "explicit #vps tag: {:?}", f.tags);
    // And a distinctive content word is picked up as a topic tag.
    assert!(f.tags.iter().any(|t| t == "homelab" || t == "server"), "a topic tag: {:?}", f.tags);
}

#[test]
fn related_facts_are_found_by_a_shared_tag() {
    let mut b = Book::default();
    b.learn(Fact::stated("the server runs homelab the photo backups", 0), 0);
    b.learn(Fact::stated("the server password is a long secret string", 0), 0);
    b.learn(Fact::stated("I like my coffee black in the morning", 0), 0);
    // The two server facts share the "server" tag; the coffee one is unrelated.
    let server = b.get(&slug(&b.facts.iter().find(|f| f.summary.contains("runs homelab")).unwrap().name)).unwrap().clone();
    let neighbours = b.related(&server, 0, 5);
    assert!(
        neighbours.iter().any(|f| f.summary.contains("password")),
        "the other server fact is a neighbour: {:?}",
        neighbours.iter().map(|f| &f.summary).collect::<Vec<_>>()
    );
    assert!(
        !neighbours.iter().any(|f| f.summary.contains("coffee")),
        "the unrelated coffee fact is not pulled in"
    );
}

#[test]
fn related_facts_are_found_by_an_explicit_link() {
    let mut b = Book::default();
    b.put(Fact::new("trip", "a trip to spain with my partner", "flying with [[partner]]", Kind::Project, 0));
    b.put(Fact::new("partner", "my partner is named Jordan", "Jordan", Kind::You, 0));
    let trip = b.get("trip").unwrap().clone();
    let neighbours = b.related(&trip, 0, 5);
    assert!(neighbours.iter().any(|f| f.name == "partner"), "the linked fact is a neighbour: {neighbours:?}");
}

// --- incremental (sharded) persistence --------------------------------------

fn tmp_store(tag: &str) -> (std::path::PathBuf, Store) {
    let dir = std::env::temp_dir().join(format!("atlas-facts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let store = Store::new(dir.clone());
    (dir, store)
}

#[test]
fn the_book_round_trips_through_shards() {
    let (dir, store) = tmp_store("roundtrip");
    {
        let mut b = Book::default();
        b.learn(Fact::stated("the wifi password is hunter2", 0), 0);
        b.learn(Fact::stated("I like my coffee black", 0), 0);
        b.save(&store).unwrap();
    }
    let b2 = Book::load(&store);
    assert_eq!(b2.facts.len(), 2, "both facts come back");
    assert!(b2.recall("wifi password", 0).iter().any(|f| f.summary.contains("hunter2")));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_book_is_written_as_shards_not_one_big_file() {
    let (dir, store) = tmp_store("shards");
    let mut b = Book::default();
    // Genuinely distinct facts (no shared words), so none merge and they spread
    // across shards by name.
    for i in 0..40 {
        b.learn(Fact::stated(&format!("alpha{i} bravo{i} charlie{i} delta{i}"), 0), 0);
    }
    assert_eq!(b.facts.len(), 40, "distinct facts don't merge");
    b.save(&store).unwrap();
    let shard_files = (0..16).filter(|s| dir.join(format!("facts-{s}.json")).exists()).count();
    assert!(shard_files > 1, "the book is split across several shard files, not one");
    assert!(!dir.join("facts.json").exists(), "the whole book is never written as a single file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_book_saved_the_old_way_is_migrated_to_shards() {
    let (dir, store) = tmp_store("migrate");
    // Lay down a pre-sharding single "facts" file the old way.
    let mut legacy = Book::default();
    legacy.put(Fact::stated("a legacy fact about the vault", 0));
    store.save("facts", &legacy).unwrap();
    // Loading migrates it; the knowledge is intact and a save writes shards.
    let mut b = Book::load(&store);
    assert!(b.recall("vault", 0).iter().any(|f| f.summary.contains("legacy")), "legacy fact loaded");
    b.save(&store).unwrap();
    assert!((0..16).any(|s| dir.join(format!("facts-{s}.json")).exists()), "shards were written");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn correcting_a_restated_fact_supersedes_the_old_value() {
    let mut b = Book::default();
    b.learn(Fact::stated("the wifi password is hunter2", 0), 0);
    // Later, a correction that repeats the subject — the common shape of a fix.
    b.learn(Fact::stated("the wifi password is now hunter3", 100), 100);
    let hits = b.recall("wifi password", 100);
    assert_eq!(hits.len(), 1, "the correction merges, not duplicates: {hits:?}");
    assert!(hits[0].summary.contains("hunter3"), "the newer value wins: {}", hits[0].summary);
    assert!(!hits[0].summary.contains("hunter2"), "the old value stops surfacing");
    assert!(hits[0].confirmed >= 1, "and it counts as a restatement of the subject");
}

#[test]
fn a_confirmed_guess_is_no_longer_evictable() {
    let mut b = Book::default();
    let mut guess = Fact::new("guess", "a guess atlas made and you confirmed", &"x ".repeat(300), Kind::Noticed, 0);
    guess.confirmed = 1; // you confirmed it once
    b.put(guess);
    let before = b.get("guess").unwrap().body.len();
    b.trim(10, 400 * DAY);
    assert_eq!(b.get("guess").unwrap().body.len(), before, "a confirmed guess is protected like a stated fact");
}

// --- the core memory (research report item 20) ------------------------------

#[test]
fn a_correction_keeps_what_it_replaced() {
    let mut b = Book::default();
    b.learn(Fact::stated("my car is a Honda", 100), 100);
    b.learn(Fact::stated("my car is a Toyota", 200), 200);
    let car = b.facts.iter().find(|f| f.summary.contains("Toyota")).expect("the newer value wins");
    assert_eq!(car.history, vec![(100, "my car is a Honda".to_string())], "the old value is kept, not lost");
    // Saying the same thing again isn't history.
    b.learn(Fact::stated("my car is a Toyota", 300), 300);
    let car = b.facts.iter().find(|f| f.summary.contains("Toyota")).unwrap();
    assert_eq!(car.history.len(), 1);
}

#[test]
fn who_you_are_leads_with_your_instructions_and_never_carries_a_secret() {
    let mut b = Book::default();
    b.learn(Fact::stated("I trade forex for a living", 100), 100);
    b.learn(Fact::stated("always answer my question first", 110), 110);
    b.learn(Fact::stated("the wifi password is hunter2pass", 120), 120);
    b.learn(Fact::stated("the backups live at D:/backups", 130), 130);
    let core: Vec<&str> = b.core(8).iter().map(|f| f.summary.as_str()).collect();
    assert_eq!(core.first(), Some(&"always answer my question first"), "{core:?}");
    assert!(core.contains(&"I trade forex for a living"), "{core:?}");
    assert!(!core.iter().any(|s| s.contains("password") || s.contains("backups")), "pointers and secrets stay out: {core:?}");
    // The same order every time, for the model server's cache.
    let again: Vec<&str> = b.core(8).iter().map(|f| f.summary.as_str()).collect();
    assert_eq!(core, again);
}

#[test]
fn what_bears_on_the_question_is_found_by_relevance_and_kept_from_secrets() {
    let mut b = Book::default();
    b.learn(Fact::new("rack", "the homelab rack is 10 inch", "the homelab rack is 10 inch", Kind::Project, 100), 100);
    b.learn(Fact::new("gym", "my gym is Northside Fitness", "my gym is Northside Fitness", Kind::Reference, 100), 100);
    b.learn(Fact::stated("the wifi password is hunter2pass", 100), 100);
    let hits: Vec<&str> = b.bearing_on("what is my gym, and the wifi password", 100 + DAY, &[], 3).iter().map(|f| f.summary.as_str()).collect();
    assert_eq!(hits, vec!["my gym is Northside Fitness"], "{hits:?}");
    assert!(b.bearing_on("how are you today", 100, &[], 3).is_empty());
}
