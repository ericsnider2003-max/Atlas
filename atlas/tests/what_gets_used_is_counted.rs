//! **What gets used is counted (30 Sep 2026).**
//!
//! "Nothing records which capabilities a turn used" was on the open list, so
//! the Improvements page's "never used" had nothing to go on. `used` counts
//! each request against the ability that does it; these hold the table to
//! the requests that really exist, and the Ideas page's free wins to having
//! a button when they aren't on.

#[test]
fn every_request_kind_in_the_table_is_a_real_one() {
    let session = std::fs::read_to_string("src/session.rs").unwrap();
    let start = session.find("pub fn kind_of").unwrap();
    let body = &session[start..];
    for (kind, ability) in atlas::used::FOR_KIND {
        assert!(body.contains(&format!("=> \"{kind}\"")), "{kind} isn't a request kind");
        assert_eq!(atlas::used::ability_for(kind), Some(*ability));
    }
}

#[test]
fn a_fresh_count_calls_nothing_unused() {
    let mut u = atlas::used::Used::default();
    u.record("agenda", 1_000);
    assert!(u.unasked(1_000).is_empty());
    let later = 1_000 + atlas::used::JUDGED_AFTER_DAYS * 86_400 + 1;
    let unused = u.unasked(later);
    assert!(!unused.contains(&"calendar".to_string()));
    // Only abilities there's a way to ask for are ever named.
    for a in &unused {
        assert!(atlas::used::FOR_KIND.iter().any(|(_, x)| x == a), "{a}");
    }
}

#[test]
fn a_free_win_that_is_not_on_has_its_button() {
    let wins = vec![
        atlas::hub::FreeWin { what: "Meaning vectors".into(), worth: "fast".into(), here: "Not here yet.".into(), get: Some(("Get the meaning model (91 MB)".into(), "get-understanding".into())) },
        atlas::hub::FreeWin { what: "Learned routes".into(), worth: "large".into(), here: "On here.".into(), get: None },
    ];
    let html = atlas::hub::recommendations_page(&[], None, &wins);
    assert!(html.contains("value='get-understanding'"), "{html}");
    // One button: the win that's on has none.
    assert_eq!(html.matches("<button name=what").count(), 1, "{html}");
    assert!(html.contains("name=from value=ideas"));
    assert!(html.contains("On here."));
    // Shown even when there are no ideas to act on.
    assert!(html.contains("Nothing I'd change"));
}
