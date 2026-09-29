//! Getting a spoken question ready before searching for it.

use atlas::asking::*;

// --- stripping the scaffolding ----------------------------------------------

#[test]
fn the_padding_around_a_spoken_question_is_dropped() {
    // "Can you find me the thing about the budget" is eight words of
    // scaffolding and one of content. Every padding word appears in half the
    // index, so they drag the score of the word that mattered toward the
    // middle.
    let p = prepare("can you find me the thing about the budget").unwrap();
    assert_eq!(p.terms, vec!["budget"]);
    assert_eq!(p.query(), "budget");
}

#[test]
fn real_words_survive() {
    let p = prepare("what did I decide about the Spain trip").unwrap();
    assert!(p.terms.contains(&"spain".to_string()));
    assert!(p.terms.contains(&"trip".to_string()));
    assert!(p.terms.contains(&"decide".to_string()));
}

#[test]
fn what_was_said_is_kept_for_quoting_back() {
    let p = prepare("find the budget notes").unwrap();
    assert_eq!(p.said, "find the budget notes");
}

// --- questions that point rather than name ----------------------------------

#[test]
fn a_question_that_points_at_something_is_asked_about() {
    // "That file I was looking at yesterday" contains no term the index has
    // ever seen. Searching it finds nothing, and nothing reads as an answer.
    // Asserted as an equality rather than a substring: the pointer it reports
    // must be the one that was said, not merely something containing those
    // words.
    assert_eq!(
        prepare("show me that one again"),
        Err(Unsearchable::PointsAtSomething("that one".into()))
    );
    // And a request with the same shape but a real noun in it is searchable,
    // which is the half that stops this becoming an obstacle.
    assert!(prepare("show me that budget again").is_ok());
}

#[test]
fn the_question_names_the_pointer_it_actually_found() {
    // Computed rather than compared against fixed wording: what matters is
    // that the question quotes back the words you used, so you know which part
    // of what you said was the problem.
    // Note the limit this establishes: a request only counts as unsearchable
    // when *everything* left is a pointer. "open the thing from yesterday"
    // keeps "open", which is a real word, so it is searched rather than
    // queried back. That is the right side to err on — asking about a request
    // that had something in it is more annoying than searching one that
    // didn't.
    for (said, pointer) in [
        ("show me that one again", "that one"),
        ("what was that thing", "that thing"),
        ("what about those", "those"),
    ] {
        let Err(e) = prepare(said) else {
            panic!("{said:?} was searched as-is");
        };
        let ask = e.ask();
        assert!(
            ask.contains(pointer),
            "{said:?} asked about something else: {ask}"
        );
        assert!(ask.ends_with('?'), "not a question: {ask}");
    }
}

#[test]
fn an_empty_question_says_so_rather_than_searching_for_nothing() {
    let e = prepare("can you just have a look").unwrap_err();
    assert_eq!(e, Unsearchable::NothingToSearchFor);
    assert!(e.ask().contains("what's it about"));
}

#[test]
fn a_pointer_with_real_words_beside_it_is_still_searchable() {
    // "that budget file" points, but "budget" is enough to search on. Refusing
    // here would make the check into an obstacle.
    let p = prepare("show me that budget file again").unwrap();
    assert!(p.terms.contains(&"budget".to_string()));
}

// --- two questions in one ---------------------------------------------------

#[test]
fn two_questions_are_separated_rather_than_averaged() {
    // Retrieving the average of two topics gives the best match for neither.
    let p = prepare("what did I decide about the trip and who was I going with").unwrap();
    assert!(p.is_more_than_one_question());
    assert_eq!(p.also.len(), 1);
    assert!(p.also[0].to_lowercase().contains("going with"));
}

#[test]
fn an_and_inside_one_idea_is_not_a_split() {
    // "fish and chips" is one thing, and cutting it leaves two halves that
    // match nothing.
    let p = prepare("the fish and chips receipt").unwrap();
    assert!(!p.is_more_than_one_question(), "it split one idea in half");
}

#[test]
fn a_single_question_reports_itself_as_one() {
    assert!(!prepare("the budget notes").unwrap().is_more_than_one_question());
}

// --- saying what it looked for ----------------------------------------------

#[test]
fn a_failed_search_can_say_what_it_actually_looked_for() {
    // "I couldn't find anything about the budget" is a useful answer.
    // "I couldn't find anything" is not.
    let p = prepare("can you find me the budget notes").unwrap();
    let said = p.searched_for();
    assert!(said.contains("budget"));
    assert!(!said.contains("can") && !said.contains("you"));
}

#[test]
fn several_terms_are_listed_rather_than_run_together() {
    let p = prepare("the Spain trip budget").unwrap();
    assert!(p.searched_for().contains(", "), "got: {}", p.searched_for());
}

// --- it must not become an obstacle -----------------------------------------

#[test]
fn an_ordinary_typed_query_passes_through_intact() {
    // Someone typing "quarterly revenue forecast" already knows what they
    // want. Nothing should be taken away.
    let p = prepare("quarterly revenue forecast").unwrap();
    assert_eq!(p.terms.len(), 3);
}

#[test]
fn short_words_that_matter_are_not_lost_to_a_length_rule() {
    // Three letters is the floor, so real short terms survive.
    let p = prepare("the tax return").unwrap();
    assert!(p.terms.contains(&"tax".to_string()));
}

#[test]
fn nothing_is_invented_that_was_not_said() {
    // The one thing it must never do: add a term you didn't say, because a
    // search on an invented term retrieves confidently and wrongly.
    let said = "budget notes";
    let p = prepare(said).unwrap();
    for t in &p.terms {
        assert!(said.to_lowercase().contains(t.as_str()), "{t} was invented");
    }
}
