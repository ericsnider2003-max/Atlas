//! Recall, actually reachable.
//!
//! `recall.rs` was complete — rarity-weighted ranking, a recency nudge that
//! never decides, a relative floor that stops a weak hit riding along beside a
//! strong one, and a `clarity()` check that notices when two notes answer a
//! question two different ways. Nothing ever put a single `Piece` into a
//! `Library`, so none of it could run.
//!
//! Wiring `research` made that worse before it made it better: Atlas started
//! writing notes to disk that it had no way of ever finding again.
//!
//! These tests go through `Daemon::turn` rather than calling `search`
//! directly, because unit tests on `search` are exactly what already existed
//! while the feature was dead.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-rc-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor {
        id: 1,
        x: 0,
        y: 0,
        width: 1920,
        height: 1040,
        primary: true,
    }])
}

/// Notes live under the store's own root now (`Daemon::notes_dir` resolves
/// `research.notes_dir` against it, the same fix `backup.dir` got) — so a
/// note has to be written under the exact same directory the daemon in
/// this test will use as its store, not a bare path relative to the
/// working directory the way both sides used to agree on by accident.
///
/// `tmp(tag)` wipes and recreates its directory every time it's called, so
/// the root is computed once per test (via `tmp(tag)`) and passed in here
/// rather than re-derived — calling `tmp(tag)` a second time inside
/// `daemon()` would delete the note before the daemon ever read it.
fn write_note_in(root: &Path, name: &str, body: &str) -> PathBuf {
    let dir = root.join("data/notes");
    std::fs::create_dir_all(&dir).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, body).unwrap();
    p
}

fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

/// For tests that need notes on disk before the daemon is constructed —
/// `root` must be the exact same directory as `daemon_at`'s `Store` uses.
fn daemon_at<'a>(c: &'a Config, p: &'a MockPlatform, root: PathBuf) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(root), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn a_note_written_earlier_can_be_found_later() {
    // The whole point. Before this, a research note was write-only.
    let root = tmp("found");
    let n = write_note_in(
        &root,
        "test-recall-tideline.md",
        "# Tideline harbour depths\n\nThe harbour silts up to four metres at low water in winter.\n",
    );
    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("what do I know about harbour silting", 100);
    let _ = std::fs::remove_file(&n);
    assert!(
        reply.contains("Tideline harbour depths") || reply.contains("silts"),
        "the note was on disk and could not be found: {reply}"
    );
}

#[test]
fn the_library_is_loaded_at_startup_not_only_after_a_research_run() {
    // Loading it only after research would mean notes from an earlier session
    // stayed invisible until another one happened — wired, and still broken.
    let root = tmp("startup");
    let n = write_note_in(&root, "test-recall-startup.md", "# Startup note\n\nSomething memorable here.\n");
    let (c, p) = (cfg(), plat());
    let d = daemon_at(&c, &p, root);
    let loaded = !d.library.is_empty();
    let _ = std::fs::remove_file(&n);
    assert!(loaded, "the library was empty on a fresh daemon with notes on disk");
}

#[test]
fn a_question_the_notes_do_not_answer_falls_through_rather_than_guessing() {
    // Returning the best of a bad set is worse than saying nothing: it reads
    // as an answer.
    let root = tmp("fallthrough");
    let n = write_note_in(&root, "test-recall-unrelated.md", "# Bicycle maintenance\n\nChain tension.\n");
    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("what is the capital of Peru", 100);
    let _ = std::fs::remove_file(&n);
    assert!(
        !reply.contains("Bicycle maintenance"),
        "an unrelated note was offered as the answer: {reply}"
    );
}

#[test]
fn two_notes_that_disagree_are_reported_as_a_disagreement() {
    // `clarity()` knows something the ranked list does not show. Handing over
    // the top hit here would pick a side in a disagreement without saying
    // there was one — and a contradiction you are not told about is worse
    // than a gap, because a gap sends you to look.
    let root = tmp("disagree");
    let a = write_note_in(
        &root,
        "test-recall-holiday-a.md",
        "# Holiday allowance policy\n\nThe holiday allowance is twenty five days per year.\n",
    );
    let b = write_note_in(
        &root,
        "test-recall-holiday-b.md",
        "# Holiday allowance policy\n\nThe holiday allowance is twenty eight days per year.\n",
    );
    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("what is the holiday allowance", 100);
    let _ = std::fs::remove_file(&a);
    let _ = std::fs::remove_file(&b);
    assert!(
        reply.contains("two ways") || reply.contains("pick"),
        "two equally good contradictory notes were resolved silently: {reply}"
    );
}

#[test]
fn an_empty_shelf_is_not_an_error() {
    // A fresh install has no notes. That must read as nothing to find, never
    // as something broken.
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "empty");
    d.library = atlas::recall::Library::default();
    let reply = d.turn("what do I know about anything at all", 100);
    assert!(!reply.contains("error"), "an empty library reported an error: {reply}");
    assert!(!reply.is_empty(), "an empty library produced no reply at all");
}

#[test]
fn searching_works_without_any_model_installed() {
    // Word search runs with nothing downloaded. Search that only works once
    // you have installed an embedding model is search you do not have on day
    // one — and day one is when you have the least idea where anything is.
    let c = cfg();
    let r = c.tools.as_ref().unwrap().recall.clone();
    assert!(!atlas::recall::needs_a_model(&r), "recall now requires a model to work at all");

    let root = tmp("nomodel");
    let n = write_note_in(&root, "test-recall-nomodel.md", "# Kettle descaling\n\nVinegar and patience.\n");
    let (c2, p) = (cfg(), plat());
    // `llm: None` — no model of any kind.
    let mut d = daemon_at(&c2, &p, root);
    let reply = d.turn("what do I know about descaling", 100);
    let _ = std::fs::remove_file(&n);
    assert!(
        reply.contains("Kettle descaling") || reply.contains("Vinegar"),
        "search failed with no model installed: {reply}"
    );
}

#[test]
fn a_note_is_findable_by_its_title_as_well_as_its_body() {
    let root = tmp("title");
    let n = write_note_in(&root, "test-recall-title.md", "# Aquifer recharge rates\n\nUnrelated body text.\n");
    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("tell me about aquifer recharge", 100);
    let _ = std::fs::remove_file(&n);
    assert!(reply.contains("Aquifer"), "the title was not searchable: {reply}");
}

/// A recalled fact is aged against the note it rests on.
///
/// `certainty::aged` was built, tested and reached by nothing: the recall
/// ranking already weighed freshness to pick a winner, but the winner was then
/// spoken as though it had been checked today. A version number pulled from a
/// note written months ago reads as current fact and is not. Wiring `aged`
/// into `from_notes` attaches the note's age when the shelf it sits on has run
/// out -- and, the other direction that matters just as much, leaves a note
/// still inside its shelf untouched so a fresh answer is not padded with a
/// qualifier nobody needs.
#[test]
fn a_fact_recalled_from_a_stale_note_is_said_with_its_age() {
    use std::time::{Duration, UNIX_EPOCH};

    // "version" puts this on the Quick shelf (a 14-day half-life): the kind of
    // claim that genuinely goes out of date, unlike a settled fact.
    let root = tmp("aged-stale");
    let n = write_note_in(
        &root,
        "test-recall-firmware.md",
        "# Router firmware version\n\nThe firmware version on the router is 7.2.\n",
    );
    // Backdate the note far past its shelf: five half-lives old, so it is
    // unambiguously Stale rather than merely Ageing.
    let as_of = 1_000_000u64;
    let half_life = 60 * 60 * 24 * 14u64;
    let now = as_of + 5 * half_life;
    let f = std::fs::File::options().write(true).open(&n).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(as_of)).unwrap();
    drop(f);

    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("what firmware version is the router on", now);
    let _ = std::fs::remove_file(&n);

    assert!(
        reply.contains("rests on") && reply.contains("changes over weeks"),
        "a fact from a note five half-lives past its shelf was said with no age \
         attached: {reply}"
    );
}

/// The other half of the same wiring: a note still inside its shelf is not
/// qualified. Without this, `aged` could be "wired" by a caller that always
/// appends something, which is the opposite of what it is for.
#[test]
fn a_fact_recalled_from_a_fresh_note_carries_no_caveat() {
    use std::time::{Duration, UNIX_EPOCH};

    let root = tmp("aged-fresh");
    let n = write_note_in(
        &root,
        "test-recall-firmware-fresh.md",
        "# Router firmware version\n\nThe firmware version on the router is 7.2.\n",
    );
    // One day old, well within a 14-day half-life: Fresh.
    let as_of = 1_000_000u64;
    let now = as_of + 60 * 60 * 24;
    let f = std::fs::File::options().write(true).open(&n).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(as_of)).unwrap();
    drop(f);

    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("what firmware version is the router on", now);
    let _ = std::fs::remove_file(&n);

    assert!(
        !reply.contains("rests on"),
        "a note one day old was qualified as though it had gone stale: {reply}"
    );
}

/// A stale note whose source Atlas can re-read for itself is not just aged --
/// it comes with an offer to go and check it.
///
/// `certainty::aged` says how old a note is. `freshness::should_recheck` says
/// the separate thing: whether acting on that age is in Atlas's power -- worth
/// rechecking by shelf, stale by state, AND from a source it can read alone (a
/// file on this machine, a page, a command). Wiring it into `from_notes` means
/// the answer now offers to go back to the file the fact came from, rather than
/// leaving you to notice the age caveat and do nothing with it.
#[test]
fn a_stale_note_from_a_file_offers_to_go_and_recheck_it() {
    use std::time::{Duration, UNIX_EPOCH};

    let root = tmp("recheck-offer");
    let n = write_note_in(
        &root,
        "test-recall-firmware-recheck.md",
        "# Router firmware version\n\nThe firmware version on the router is 7.2.\n",
    );
    // Five half-lives past a 14-day (Quick) shelf: unambiguously Stale, and the
    // source is a file the daemon can re-read on its own.
    let as_of = 1_000_000u64;
    let half_life = 60 * 60 * 24 * 14u64;
    let now = as_of + 5 * half_life;
    let f = std::fs::File::options().write(true).open(&n).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(as_of)).unwrap();
    drop(f);

    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("what firmware version is the router on", now);
    let _ = std::fs::remove_file(&n);

    assert!(
        reply.contains("go back and check it"),
        "a stale note from a re-readable file offered no way to check it: {reply}"
    );
}

/// The other half: a fresh note carries no offer to recheck. Without this, the
/// wire could be faked by a caller that always appends the offer, which would
/// be the opposite of what `should_recheck` is for -- it must stay silent while
/// the note is still inside its shelf.
#[test]
fn a_fresh_note_makes_no_offer_to_recheck() {
    use std::time::{Duration, UNIX_EPOCH};

    let root = tmp("recheck-fresh");
    let n = write_note_in(
        &root,
        "test-recall-firmware-fresh-recheck.md",
        "# Router firmware version\n\nThe firmware version on the router is 7.2.\n",
    );
    let as_of = 1_000_000u64;
    let now = as_of + 60 * 60 * 24; // one day: Fresh.
    let f = std::fs::File::options().write(true).open(&n).unwrap();
    f.set_modified(UNIX_EPOCH + Duration::from_secs(as_of)).unwrap();
    drop(f);

    let (c, p) = (cfg(), plat());
    let mut d = daemon_at(&c, &p, root);
    let reply = d.turn("what firmware version is the router on", now);
    let _ = std::fs::remove_file(&n);

    assert!(
        !reply.contains("go back and check it"),
        "a fresh note offered a recheck it did not need: {reply}"
    );
}
