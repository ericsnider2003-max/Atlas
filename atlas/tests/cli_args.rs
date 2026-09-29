//! The flag bug, pinned.
//!
//! `main` strips every `--flag` out of the word list before dispatching. Three
//! commands added on 14 September read their own flags, and for the first run
//! of each they were handed a sentence with the flags — and the values
//! attached to them — already gone.
//!
//! Watched failing: with `tail_after` replaced by the filtered word list,
//! `a_duration_given_on_the_line_survives` fails with `None`, which is exactly
//! how the real bug presented — `atlas remote done 1 "it rendered" --secs 200`
//! reporting that the job had finished too fast to be worth mentioning.

use atlas::cli::{flag_value, plain_words, tail_after};

fn argv(words: &[&str]) -> Vec<String> {
    words.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_tail_after_a_subcommand_still_has_its_flags() {
    let a = argv(&["remote", "done", "1", "it rendered", "--secs", "200"]);
    assert_eq!(
        tail_after(&a, "remote"),
        &argv(&["done", "1", "it rendered", "--secs", "200"])[..]
    );
}

#[test]
fn a_word_that_is_not_there_gives_an_empty_tail_rather_than_the_whole_line() {
    let a = argv(&["carry", "list"]);
    assert!(tail_after(&a, "mobile").is_empty());
}

#[test]
fn a_duration_given_on_the_line_survives() {
    let a = argv(&["done", "1", "it rendered", "--secs", "200"]);
    assert_eq!(flag_value(&a, "--secs"), Some("200"));
}

#[test]
fn a_flag_with_nothing_after_it_has_no_value_rather_than_an_empty_one() {
    // The difference matters: an empty string parses to zero seconds, and
    // zero seconds is a claim about how long the work took.
    let a = argv(&["done", "1", "--secs"]);
    assert_eq!(flag_value(&a, "--secs"), None);

    let b = argv(&["done", "1", "--secs", "--quietly"]);
    assert_eq!(flag_value(&b, "--secs"), None);
}

#[test]
fn the_value_a_flag_takes_is_not_left_inside_the_instruction() {
    // `--to sarah` used to leave "sarah" in the middle of the request text.
    let a = argv(&["ask", "render", "the", "draft", "--to", "sarah", "--urgent"]);
    assert_eq!(plain_words(&a[1..], &["--to"]), "render the draft");
}

#[test]
fn a_flag_that_takes_no_value_does_not_eat_the_next_word() {
    let a = argv(&["ask", "--urgent", "render", "the", "draft"]);
    assert_eq!(plain_words(&a[1..], &["--to", "--secs"]), "render the draft");
}

#[test]
fn an_instruction_with_no_flags_at_all_comes_through_whole() {
    let a = argv(&["ask", "render", "the", "draft"]);
    assert_eq!(plain_words(&a[1..], &["--to"]), "render the draft");
}
