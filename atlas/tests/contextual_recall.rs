//! Contextual recall — the conversation breaks ties, silently.
//!
//! Eric's two rulings, 22 Sep 2026: context is built from topic/word overlap
//! with the live thread (embeddings can strengthen it later through the same
//! seam), and it is a SILENT bias — it re-ranks what a question finds, it
//! never volunteers. These tests pin both halves through the daemon, plus the
//! property that makes "silent" safe: a zero-scoring piece stays zero
//! whatever the conversation was about, so context cannot make Atlas bring
//! up a note in answer to a question that note does not match.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::facts::{Book, Fact, Kind};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::recall::{context_terms_from, Library, Piece, RecallConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-ctx-{tag}"));
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

fn daemon_at<'a>(c: &'a Config, p: &'a MockPlatform, root: PathBuf) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
}

fn write_note_in(root: &Path, name: &str, body: &str) {
    let dir = root.join("data/notes");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(name), body).unwrap();
}

/// Two notes answer "apples" about equally, which without context is a
/// disagreement Atlas refuses to settle ("my notes answer that two ways").
/// One turn of conversation about the cider press settles it — the cider
/// note wins cleanly, because it connects to what was just being discussed.
#[test]
fn the_conversation_settles_a_question_the_notes_answer_two_ways() {
    let root = tmp("settles");
    write_note_in(
        &root,
        "test-ctx-harvest.md",
        "# Harvest log\n\nThe apples came off the east rows this week.\n",
    );
    write_note_in(
        &root,
        "test-ctx-cider.md",
        "# Cider batch\n\nThe apples went through the press on Sunday.\n",
    );
    let (c, p) = (cfg(), plat());

    // Without context: near-equal scores, and the honest answer is to say so.
    let root2 = tmp("settles-control");
    write_note_in(&root2, "test-ctx-harvest.md", "# Harvest log\n\nThe apples came off the east rows this week.\n");
    write_note_in(&root2, "test-ctx-cider.md", "# Cider batch\n\nThe apples went through the press on Sunday.\n");
    let mut control = daemon_at(&c, &p, root2);
    let cold = control.turn("what do I know about apples", 100);
    assert!(
        cold.contains("two ways") || cold.contains("Cider") || cold.contains("Harvest"),
        "the control should either report the tie or pick one: {cold}"
    );

    // With one turn of cider context first, the cider note must win outright.
    let mut d = daemon_at(&c, &p, root);
    let _ = d.turn("what do I know about the cider press", 100);
    let warm = d.turn("what do I know about apples", 200);
    assert!(
        warm.contains("Cider") && !warm.contains("two ways"),
        "one turn about the cider press should settle which apples note is meant: {warm}"
    );
}

/// The silent rule: context re-ranks, it never introduces. However much the
/// conversation dwelt on bicycles, a question about something else entirely
/// must not surface the bicycle note — a zero base times any nudge is zero.
#[test]
fn talking_about_a_topic_does_not_make_it_the_answer_to_everything() {
    let root = tmp("nevervolunteers");
    write_note_in(
        &root,
        "test-ctx-bicycle.md",
        "# Bicycle maintenance\n\nChain tension, spoke torque, tyre pressure.\n",
    );
    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let _ = d.turn("is there anything about my bicycle chain", 100);
    let _ = d.turn("what about bicycle spokes", 150);
    let reply = d.turn("what is the capital of peru", 200);
    assert!(
        !reply.contains("Bicycle"),
        "two turns about bicycles made an unrelated question surface the bicycle note: {reply}"
    );
}

/// The fact-book half, on the Book's own API: two facts share the question's
/// word, context picks the one connected to the conversation, and a fact the
/// question never matched stays off the list entirely.
#[test]
fn a_fact_tie_breaks_toward_the_conversation_and_nothing_new_appears() {
    let mut b = Book::default();
    let now = 1_000_000;
    b.learn(Fact::new("garage code", "the garage keypad code", "garage keypad opens with 4417", Kind::Reference, now), now);
    b.learn(Fact::new("garage rent", "the garage rent", "garage rent is due monthly to the keypad landlord", Kind::Reference, now), now);
    b.learn(Fact::new("dentist", "the dentist", "the dentist is on maple street", Kind::Reference, now), now);

    let ctx = context_terms_from(&["I can't remember the keypad number".into()], "what do I know about the garage");
    assert!(ctx.contains(&"keypad".to_string()), "context should carry 'keypad': {ctx:?}");

    let hits = b.recall_in_context("garage", &ctx, now);
    assert!(!hits.is_empty());
    // Both garage facts mention "keypad" in body or summary — the CODE fact
    // carries it in the summary too, so the keypad conversation pulls the
    // code fact ahead of the rent fact.
    assert_eq!(hits[0].name, "garage-code", "the keypad conversation should pick the code fact");
    assert!(
        hits.iter().all(|f| f.name != "dentist"),
        "a fact the question never matched must not appear because of context"
    );
}

/// The library-level property, pinned directly: identical base scores, the
/// context word flips the order — and the nudge is bounded, so a strong
/// query match cannot be overturned by chatter.
#[test]
fn the_nudge_reorders_equals_and_cannot_overturn_a_better_match() {
    let mut lib = Library::default();
    let mk = |id: u64, title: &str, text: &str| Piece {
        id,
        source: format!("{title}.md"),
        title: title.into(),
        text: text.into(),
        at: 1_000_000,
        embedding: None,
    };
    lib.add(mk(1, "alpha", "the shipment arrives tuesday by lorry"));
    lib.add(mk(2, "beta", "the shipment arrives tuesday by ferry"));
    let cfg = RecallConfig::default();
    let now = 1_000_100;

    let ctx = vec!["ferry".to_string()];
    let hits = lib.search_in_context("shipment tuesday", None, &ctx, None, &cfg, now);
    assert_eq!(hits.first().map(|h| h.title.as_str()), Some("beta"), "the ferry context should pick beta");

    // A decisively better match stays on top whatever the context says: the
    // multiplier is capped well below the gap two extra matching words make.
    lib.add(mk(3, "gamma", "shipment shipment tuesday tuesday arrives on the dot"));
    let hits = lib.search_in_context("shipment tuesday", None, &ctx, None, &cfg, now);
    assert_eq!(
        hits.first().map(|h| h.title.as_str()),
        Some("gamma"),
        "context is a tiebreak, not a veto over a clearly better match"
    );
}

/// The #6 upgrade: context connects by MEANING, not only by shared words.
/// Two pieces the query scores equally, a context vector that points at one
/// of them, and no word overlap at all — the meaning signal alone settles it.
/// Hand-made unit vectors stand in for the encoder (the daemon-level test with
/// the real encoder lives in meaning_search.rs).
#[test]
fn a_context_vector_breaks_a_tie_by_meaning_with_no_shared_words() {
    let mut lib = Library::default();
    let mk = |id: u64, title: &str, text: &str, emb: Vec<f32>| Piece {
        id,
        source: format!("{title}.md"),
        title: title.into(),
        text: text.into(),
        at: 1_000_000,
        embedding: Some(emb),
    };
    // Both answer "shipment tuesday" identically on words; their vectors point
    // in different directions. 16 dims, the floor parse_embedding accepts.
    let mut a = vec![0.0f32; 16];
    a[0] = 1.0;
    let mut b = vec![0.0f32; 16];
    b[1] = 1.0;
    lib.add(mk(1, "alpha", "the shipment arrives tuesday", a));
    lib.add(mk(2, "beta", "the shipment arrives tuesday", b));
    let cfg = RecallConfig { semantic: true, ..RecallConfig::default() };
    let now = 1_000_100;

    // A context vector aligned with beta's, sharing NO words with either piece.
    let mut ctx_vec = vec![0.0f32; 16];
    ctx_vec[1] = 1.0;
    let hits = lib.search_in_context("shipment tuesday", None, &[], Some(&ctx_vec), &cfg, now);
    assert_eq!(
        hits.first().map(|h| h.title.as_str()),
        Some("beta"),
        "the context vector points at beta by meaning, with no shared words to do it"
    );

    // And with meaning search off, the context vector must change nothing:
    // the same search with and without it returns identical scores, proving
    // the lift above came from the semantic path and not by accident. (The
    // two pieces are not themselves perfectly tied — their titles differ and
    // freshness reads the title — so the invariant is "the vector had no
    // effect", checked piece-by-piece, not "the two are equal".)
    let off = RecallConfig { semantic: false, ..RecallConfig::default() };
    let with_vec = lib.search_in_context("shipment tuesday", None, &[], Some(&ctx_vec), &off, now);
    let without = lib.search_in_context("shipment tuesday", None, &[], None, &off, now);
    assert_eq!(with_vec.len(), without.len());
    for (a, b) in with_vec.iter().zip(without.iter()) {
        assert_eq!(a.title, b.title, "order changed with semantic off");
        assert!(
            (a.score - b.score).abs() < 1e-9,
            "the context vector moved a score with semantic off: {} vs {}",
            a.score,
            b.score
        );
    }
}
