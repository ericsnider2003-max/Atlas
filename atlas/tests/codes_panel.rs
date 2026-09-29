use atlas::codes::{
    before_you_go, gaps, logins_available, used_one, where_to_get, CodesConfig, Set, THE_LIMIT,
    WHY_THIS_WORKS,
};

fn cfg() -> CodesConfig {
    CodesConfig { enabled: true, ..Default::default() }
}

fn set(site: &str, issued: u32, used: u32, in_hand: bool) -> Set {
    Set { site: site.into(), issued, used, at: 0, kept_where: None, in_hand }
}

// ================= codes are built for exactly this =================

#[test]
fn atlas_knows_what_each_service_calls_them_and_where_they_hide() {
    // They all call it something different, which is most of why people never
    // find them.
    let (url, name, how) = where_to_get("Gmail").unwrap();
    assert!(url.contains("two-step-verification"));
    assert_eq!(name, "Backup codes");
    assert!(how.contains("regenerate whenever you like"));

    assert_eq!(where_to_get("GitHub").unwrap().1, "Recovery codes");
    assert_eq!(where_to_get("Apple").unwrap().1, "Recovery Key");
}

#[test]
fn the_services_that_only_give_one_code_are_flagged_as_such() {
    // Ten is a deployment. One is a single login.
    assert!(where_to_get("x.com").unwrap().2.contains("single code rather than ten"));
    assert!(where_to_get("Microsoft").unwrap().2.contains("one long code, not a list"));
}

#[test]
fn apples_permanent_lockout_warning_is_passed_on() {
    assert!(where_to_get("Apple").unwrap().2.contains("lock you out permanently"));
}

#[test]
fn an_account_with_no_codes_at_all_is_the_top_of_the_list() {
    let g = gaps(&[], &["Gmail".into(), "GitHub".into()], &cfg());
    assert_eq!(g.len(), 2);
    assert!(g[0].what.contains("this is the one that locks you out"));
    assert!(g[0].url.is_some(), "and it can open the page");
}

#[test]
fn running_low_is_caught_with_enough_warning_to_do_something() {
    let sets = vec![set("Gmail", 10, 8, true)];
    let s = &sets[0];
    // Takes the config since 19 Sep 2026. It hardcoded three and had no
    // caller, while `gaps` compared against `cfg.warn_at` -- two answers to
    // the same question, and the one a person could change was not the one
    // the named predicate gave. `gaps` calls this now, so there is one rule.
    assert!(s.running_low(&cfg()));
    assert_eq!(s.left(), 2);
    let g = gaps(&sets, &["Gmail".into()], &cfg());
    assert!(g[0].what.contains("2 left"));
}

#[test]
fn generated_but_not_printed_is_treated_as_not_done() {
    // Codes on a screen you won't have with you are not codes.
    let sets = vec![set("Gmail", 10, 0, false)];
    let g = gaps(&sets, &["Gmail".into()], &cfg());
    assert!(g.iter().any(|x| x.what.contains("haven't said they're printed")));
    assert_eq!(logins_available(&sets), 0, "not counted until they're on you");
}

#[test]
fn spending_one_is_tracked_so_you_find_out_before_the_last() {
    let mut sets = vec![set("Gmail", 10, 0, true)];
    assert_eq!(used_one(&mut sets, "gmail"), Some(9));
    assert_eq!(logins_available(&sets), 9);
}

#[test]
fn a_spent_set_says_generate_a_new_one() {
    let sets = vec![set("Gmail", 10, 10, true)];
    let g = gaps(&sets, &["Gmail".into()], &cfg());
    assert!(g[0].what.contains("all used"));
}

#[test]
fn fully_prepared_says_how_many_logins_you_actually_have() {
    // The number that matters.
    let sets = vec![set("Gmail", 10, 0, true), set("GitHub", 16, 2, true)];
    let said = before_you_go(&sets, &["Gmail".into(), "GitHub".into()], &cfg());
    assert!(said.contains("24 logins between them"));
    assert!(said.contains("You're covered"));
}

#[test]
fn atlas_offers_to_open_each_page_and_you_print_them() {
    let said = before_you_go(&[], &["Gmail".into()], &cfg());
    assert!(said.contains("I'll open each page"));
    assert!(said.contains("tell me they're on you"));
}

#[test]
fn atlas_never_stores_the_codes_themselves() {
    // This asserted a `#[serde(skip)]` field pinned false -- that a constant
    // was the constant it was declared as, which is true of every constant.
    // The field is gone (19 Sep 2026); the guarantee is the shape of `Set`,
    // which has nowhere to put a code: a site, two counts, a date, where the
    // paper is, and whether you have it on you.
    let src = std::fs::read_to_string("src/codes.rs").expect("src/codes.rs");
    let body = src.split("pub struct Set").nth(1).expect("Set is gone");
    let body = &body[..body.find("\n}").expect("unterminated Set")];
    let fields: Vec<String> = body
        .lines()
        .map(str::trim)
        .filter_map(|l| l.strip_prefix("pub "))
        .map(|f| f.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect())
        .collect();
    assert_eq!(
        fields,
        vec!["site", "issued", "used", "at", "kept_where", "in_hand"],
        "`Set` gained a field. Recovery codes are the one thing here that must never be in \
         Atlas's own files -- if the new field can hold one, that is the promise broken."
    );

    // And an old config naming the removed key still loads.
    let parsed: CodesConfig =
        serde_yaml::from_str("enabled: true\nstores_the_codes: true\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the section parsing");
}

#[test]
fn the_limit_of_this_approach_is_stated_rather_than_glossed() {
    assert!(THE_LIMIT.contains("ten sign-ins"));
    assert!(THE_LIMIT.contains("isn't enough on its own"));
    assert!(THE_LIMIT.contains("down to three"));
}

#[test]
fn why_this_fits_the_situation_is_specific_not_general() {
    assert!(WHY_THIS_WORKS.contains("locked-down government computer"));
    assert!(WHY_THIS_WORKS.contains("no phone and no signal"));
    assert!(WHY_THIS_WORKS.contains("you can only disable it from inside the account"));
}
