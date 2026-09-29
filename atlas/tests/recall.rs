use atlas::recall::{needs_a_model, spoken, words_of, Library, Piece, RecallConfig};

fn cfg() -> RecallConfig {
    RecallConfig::default()
}

fn piece(id: u64, title: &str, text: &str, at: u64) -> Piece {
    Piece { id, source: format!("notes/{title}.md"), title: title.into(), text: text.into(), at, embedding: None }
}

fn library() -> Library {
    let mut l = Library::default();
    l.add(piece(1, "VPS decision",
        "Decided to keep the photo server on the Windows VPS rather than moving to Linux. \
         The scanner vendor's kiosk library is Windows-only, which settles it.", 1_000_000));
    l.add(piece(2, "Atlas phase one",
        "Dictation, then endpointing. The fixed eight second window is the biggest avoidable \
         cost on this laptop.", 1_600_000));
    l.add(piece(3, "Kitchen notes",
        "The oven runs about twenty degrees hot. Everything needs less time than the recipe says.", 900_000));
    l.add(piece(4, "Kiln call",
        "They confirmed the kiln certification takes six weeks. Nothing can be fired before that.", 1_500_000));
    l
}

// ================= it works with nothing installed =================

#[test]
fn searching_by_words_needs_no_model_at_all() {
    // Search that only works once you've downloaded something is search you
    // won't have on day one.
    assert!(!needs_a_model(&cfg()));
    let hits = library().search("kiln", None, &cfg(), 2_000_000);
    assert!(!hits.is_empty());
}

#[test]
fn a_rare_word_finds_the_thing() {
    let hits = library().search("kiln certification", None, &cfg(), 2_000_000);
    assert_eq!(hits[0].title, "Kiln call");
}

#[test]
fn a_word_in_everything_does_not_drown_the_search() {
    // A word in every note tells you nothing; a word in three tells you a
    // great deal. Without that, common words win.
    let mut l = Library::default();
    for i in 0..20 {
        l.add(piece(i, &format!("note {i}"), "the project notes about the project", i * 1000));
    }
    l.add(piece(99, "special", "the project notes about kintsugi", 21_000));
    let hits = l.search("project kintsugi", None, &cfg(), 30_000);
    assert_eq!(hits[0].title, "special", "the rare word decides");
}

#[test]
fn a_match_in_the_title_counts_for_more_than_one_buried_in_the_body() {
    let mut l = Library::default();
    l.add(piece(1, "VPS decision", "some unrelated body text here entirely", 1000));
    l.add(piece(2, "random note", "we talked about the vps once in passing", 1000));
    let hits = l.search("vps", None, &cfg(), 2000);
    assert_eq!(hits[0].title, "VPS decision");
}

#[test]
fn the_tenth_mention_of_a_word_says_little_more_than_the_third() {
    let mut l = Library::default();
    l.add(piece(1, "spam", &"kintsugi ".repeat(40), 1000));
    l.add(piece(2, "real", "kintsugi is the art of repairing with gold, and it matters here", 1000));
    let hits = l.search("kintsugi repairing gold", None, &cfg(), 2000);
    assert_eq!(hits[0].title, "real", "saturation stops repetition winning");
}

// ================= you can tell which one it is =================

#[test]
fn it_quotes_the_line_it_matched_so_you_need_not_open_it() {
    let hits = library().search("kiln certification", None, &cfg(), 2_000_000);
    assert!(hits[0].quote.contains("six weeks"), "got: {}", hits[0].quote);
}

#[test]
fn it_says_why_each_one_came_back() {
    let hits = library().search("windows vps", None, &cfg(), 2_000_000);
    assert!(hits[0].why.starts_with("mentions"), "got: {}", hits[0].why);
}

#[test]
fn a_very_long_line_is_cut_rather_than_read_out_whole() {
    let mut l = Library::default();
    l.add(piece(1, "long", &format!("kintsugi {}", "words ".repeat(200)), 1000));
    let hits = l.search("kintsugi", None, &cfg(), 2000);
    assert!(hits[0].quote.chars().count() <= 161);
    assert!(hits[0].quote.ends_with('…'));
}

#[test]
fn what_atlas_says_leads_with_the_one_it_thinks_you_mean() {
    // A list of five filenames is not an answer.
    let said = spoken(&library().search("kiln", None, &cfg(), 2_000_000));
    assert!(said.contains("—"), "the title and the line: {said}");
    assert!(said.contains('"'), "quoted");
}

#[test]
fn finding_nothing_says_so_plainly() {
    assert_eq!(spoken(&[]), "I can't find anything about that.");
    assert!(library().search("submarines", None, &cfg(), 2_000_000).is_empty());
}

// ================= recency nudges, it doesn't decide =================

#[test]
fn something_older_that_matches_exactly_still_wins() {
    let mut l = Library::default();
    l.add(piece(1, "old exact", "kintsugi kintsugi repair", 0));
    l.add(piece(2, "new vague", "some other topic entirely", 9_000_000));
    let hits = l.search("kintsugi", None, &cfg(), 10_000_000);
    assert_eq!(hits[0].title, "old exact");
}

#[test]
fn between_two_equal_matches_the_recent_one_leads() {
    let mut l = Library::default();
    l.add(piece(1, "older", "kintsugi notes", 0));
    l.add(piece(2, "newer", "kintsugi notes", 9_900_000));
    let hits = l.search("kintsugi", None, &cfg(), 10_000_000);
    assert_eq!(hits[0].title, "newer");
}

// ================= meaning, when a model is there =================

fn with_meaning() -> RecallConfig {
    RecallConfig { semantic: true, ..cfg() }
}

#[test]
fn meaning_finds_the_thing_when_you_remember_what_it_was_about() {
    // No shared words at all between the question and the note.
    let mut l = Library::default();
    let mut p = piece(1, "VPS decision", "kept the server where it was", 1000);
    p.embedding = Some(vec![0.9, 0.1, 0.2]);
    l.add(p);
    let mut other = piece(2, "Kitchen notes", "the oven runs hot", 1000);
    other.embedding = Some(vec![0.1, 0.9, 0.1]);
    l.add(other);

    let question = vec![0.88, 0.12, 0.25];
    let hits = l.search("where does the photo thing live", Some(&question), &with_meaning(), 2000);
    assert_eq!(hits[0].title, "VPS decision");
    assert_eq!(hits[0].why, "about the same thing");
}

#[test]
fn words_and_meaning_are_merged_rather_than_chosen_between() {
    let mut l = Library::default();
    let mut a = piece(1, "words only", "kintsugi appears here", 1000);
    a.embedding = Some(vec![0.0, 1.0, 0.0]);
    l.add(a);
    let mut b = piece(2, "both", "kintsugi appears here too", 1000);
    b.embedding = Some(vec![1.0, 0.0, 0.0]);
    l.add(b);

    let q = vec![1.0, 0.0, 0.0];
    let hits = l.search("kintsugi", Some(&q), &with_meaning(), 2000);
    assert_eq!(hits[0].title, "both", "matching both ways beats matching one");
}

#[test]
fn meaning_is_ignored_entirely_when_it_is_switched_off() {
    let mut l = Library::default();
    let mut p = piece(1, "unrelated", "nothing in common", 1000);
    p.embedding = Some(vec![1.0, 0.0]);
    l.add(p);
    let hits = l.search("kintsugi", Some(&[1.0, 0.0]), &cfg(), 2000);
    assert!(hits.is_empty(), "words only, so no match");
}

#[test]
fn pieces_still_needing_a_meaning_vector_can_be_listed() {
    let mut l = library();
    assert_eq!(l.unembedded().len(), 4);
    l.set_embedding(1, vec![0.1, 0.2]);
    assert_eq!(l.unembedded().len(), 3);
}

// ================= the basics =================

#[test]
fn filler_words_are_not_searched_for() {
    let w = words_of("what did we decide about the vps");
    assert!(w.contains(&"decide".to_string()));
    assert!(w.contains(&"vps".to_string()));
    assert!(!w.contains(&"the".to_string()));
    assert!(!w.contains(&"what".to_string()));
}

#[test]
fn a_bad_match_is_not_offered_at_all() {
    // Worse than finding nothing.
    let hits = library().search("oven", None, &cfg(), 2_000_000);
    assert!(hits.iter().all(|h| h.score > cfg().floor));
    assert_eq!(hits.len(), 1, "only the kitchen note, not everything else");
}

#[test]
fn there_is_a_ceiling_on_how_many_come_back() {
    let mut l = Library::default();
    for i in 0..50 {
        l.add(piece(i, &format!("note {i}"), "kintsugi", i * 1000));
    }
    assert_eq!(l.search("kintsugi", None, &cfg(), 60_000).len(), 5);
}

#[test]
fn a_small_library_still_finds_things_it_cannot_tell_apart() {
    // With four notes, a word in all four is "common" and would score zero.
    // It should still come back — Atlas just can't rank them by that word.
    let mut l = Library::default();
    for i in 0..4 {
        l.add(piece(i, &format!("note {i}"), "everything mentions kintsugi", i * 1000));
    }
    assert_eq!(l.search("kintsugi", None, &cfg(), 9000).len(), 4);
}
