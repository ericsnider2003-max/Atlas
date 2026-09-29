use atlas::opsec::{check, OpsecConfig, Risk};

fn cfg() -> OpsecConfig {
    OpsecConfig { enabled: true, ..OpsecConfig::default() }
}

#[test]
fn switched_off_finds_nothing_regardless_of_what_the_text_says() {
    let mut c = cfg();
    c.enabled = false;
    let found = check("i'm leaving on deployment from the base tomorrow", &[], &c, "2026-09-12");
    assert!(found.is_empty());
}

#[test]
fn an_unambiguous_movement_term_fires_on_its_own() {
    // "deployment" isn't a word used for much else.
    let found = check("thinking about the deployment next month", &[], &cfg(), "2026-09-12");
    assert!(found.iter().any(|s| s.risk == Risk::Movement), "{found:?}");
}

#[test]
fn an_unambiguous_place_fires_on_its_own() {
    let found = check("stopped by the naval station today", &[], &cfg(), "2026-09-12");
    assert!(found.iter().any(|s| s.risk == Risk::Location), "{found:?}");
}

#[test]
fn ordinary_personal_writing_that_shares_a_word_is_left_alone() {
    // "my last day", "base" and "post" are all completely ordinary things to
    // say about a civilian job change and a blog -- none of them should read
    // as an affiliation leak on their own.
    let found = check(
        "my last day at the old job is Friday, then I'm writing a post about \
         the whole experience from first base to now",
        &[],
        &cfg(),
        "2026-09-12",
    );
    assert!(found.is_empty(), "ordinary personal writing should not trip opsec: {found:?}");
}

#[test]
fn an_ambiguous_term_fires_once_something_unambiguous_corroborates_it() {
    // "back in" alone (ambiguous) says nothing. Paired with "wheels up"
    // (unambiguous), the sentence as a whole is actually about a movement.
    let found = check("wheels up tonight, back in on the 15th", &[], &cfg(), "2026-09-12");
    let moved: Vec<&str> = found
        .iter()
        .filter(|s| s.risk == Risk::Movement)
        .map(|s| s.detail.as_str())
        .collect();
    assert!(moved.iter().any(|d| d.contains("wheels up")), "{moved:?}");
    assert!(moved.iter().any(|d| d.contains("back in")), "{moved:?}");
}

#[test]
fn in_uniform_alone_is_enough_to_corroborate_an_ambiguous_term() {
    let mut c = cfg();
    c.in_uniform = true;
    let found = check("my last day here is soon", &[], &c, "2026-09-12");
    assert!(found.iter().any(|s| s.risk == Risk::Movement), "{found:?}");
}

#[test]
fn your_own_stated_words_are_never_treated_as_a_leak() {
    let mut c = cfg();
    c.not_a_leak = vec!["deployment".into()];
    let found = check("wrote a blog post about the deployment process for my app", &[], &c, "2026-09-12");
    assert!(
        !found.iter().any(|s| s.risk == Risk::Movement),
        "a word you've named as your own should never fire: {found:?}"
    );
}

#[test]
fn nothing_applies_past_the_stated_end_date() {
    let mut c = cfg();
    c.applies_until = "2026-06-01".into();
    let found = check("shipping out on deployment from the naval station", &[], &c, "2026-09-12");
    assert!(found.is_empty(), "the affiliation has ended by this date: {found:?}");
}

#[test]
fn it_still_applies_on_and_before_the_end_date() {
    let mut c = cfg();
    c.applies_until = "2026-09-12".into();
    let found = check("shipping out on deployment", &[], &c, "2026-09-12");
    assert!(!found.is_empty(), "the end date is inclusive: {found:?}");
}

#[test]
fn with_no_end_date_it_applies_indefinitely() {
    let found = check("shipping out on deployment", &[], &cfg(), "2099-01-01");
    assert!(!found.is_empty());
}
