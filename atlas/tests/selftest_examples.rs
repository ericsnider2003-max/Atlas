//! The self-test asks each command something that fits it (1 Oct 2026: it
//! tacked "the quarterly budget" onto every one and filed the nonsense
//! replies as broken commands on the Improvements page).

use std::path::Path;

#[test]
fn every_command_that_needs_words_has_an_example_that_fits() {
    let c = atlas::config::Config::load(Path::new("config")).unwrap();
    let book = atlas::intent::ToolBook::new(&c.commands);
    let missing: Vec<String> = book
        .entries()
        .iter()
        .filter(|e| e.takes_arg && !e.arg_optional && !e.describe.starts_with("Internal:"))
        .filter(|e| atlas::selftest::example_for(&e.name).is_none())
        .map(|e| e.name.clone())
        .collect();
    assert!(missing.is_empty(), "add an example to selftest::EXAMPLES for: {missing:?}");
    let s = atlas::selftest::sentences(&book);
    assert!(!s.iter().any(|(_, said, _)| said.contains("quarterly budget")), "no filler left");
    assert!(s.iter().any(|(_, said, _)| said == "mode focus" || said.ends_with(" focus")));
}

