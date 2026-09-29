//! Accounts, with somewhere to keep them.
//!
//! `accounts.rs` could work out that an email account protected only by a
//! password is the worst thing on your machine, and it had no way to be told
//! about one. `audit` was complete, tested, and only ever called on an empty
//! slice — so the hub's Accounts page listed nothing, on a machine with plenty
//! of accounts, and read as if there were nothing to worry about.

use atlas::accounts::{Book, Change, SecondFactor, Stakes};
use atlas::hub;
use atlas::store::Store;
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-accounts-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ---------------------------------------------------------------------------
// Keeping them
// ---------------------------------------------------------------------------

#[test]
fn tracking_a_site_works_out_what_it_is_worth_without_asking() {
    let mut b = Book::default();
    assert!(b.note("gmail"));
    let a = b.get("gmail").expect("tracked");
    assert_eq!(
        a.stakes,
        Stakes::Keystone,
        "every other account resets through email, so it owns all the others"
    );
    assert!(
        a.settings_url.is_some(),
        "a known site should come with the page you would actually change it on"
    );
}

#[test]
fn an_unknown_site_gets_no_invented_settings_link() {
    let mut b = Book::default();
    b.note("some local forum");
    assert_eq!(
        b.get("some local forum").unwrap().settings_url,
        None,
        "a guessed URL that 404s is worse than no link, because it looks like \
         Atlas checked"
    );
}

#[test]
fn tracking_the_same_site_twice_does_not_wipe_what_you_told_me() {
    let mut b = Book::default();
    b.note("github");
    b.set_second_factor("github", SecondFactor::Key);
    assert!(!b.note("GitHub"), "already tracked, however you capitalise it");
    assert_eq!(
        b.get("github").unwrap().second_factor,
        SecondFactor::Key,
        "re-adding must not quietly undo the security picture the audit is \
         built from"
    );
}

#[test]
fn the_book_survives_a_restart() {
    let store = Store::new(tmp("roundtrip"));
    let mut b = Book::load(&store);
    b.note("gmail");
    b.set_second_factor("gmail", SecondFactor::App);
    b.set_reused("gmail", true);
    b.save(&store).unwrap();

    let back = Book::load(&store);
    let a = back.get("gmail").expect("still there");
    assert_eq!(a.second_factor, SecondFactor::App);
    assert!(a.reused_password);
}

#[test]
fn what_matters_most_is_listed_first() {
    let mut b = Book::default();
    b.note("some local forum");
    b.note("gmail");
    assert_eq!(
        b.accounts[0].site, "gmail",
        "a list you have to scan for the important one is a list read wrong"
    );
}

// ---------------------------------------------------------------------------
// The thing that made the page worth having
// ---------------------------------------------------------------------------

#[test]
fn a_keystone_account_with_only_a_password_is_the_first_thing_said() {
    let mut b = Book::default();
    b.note("gmail");
    b.note("some local forum");
    let advice = b.advice();
    assert!(!advice.is_empty(), "the audit found nothing to say");
    assert_eq!(
        advice[0].site, "gmail",
        "worst first — otherwise the answer is buried in the inventory"
    );
}

#[test]
fn an_account_i_know_nothing_about_is_not_reported_as_safe() {
    // The same shape as an unread instrument reading as a healthy machine:
    // `audit` finds nothing wrong because it has been told nothing.
    let mut b = Book::default();
    b.note("some local forum");
    assert_eq!(
        b.undescribed().len(),
        1,
        "an account described to nobody must not pass as a clean bill of health"
    );

    b.set_second_factor("some local forum", SecondFactor::App);
    assert!(b.undescribed().is_empty(), "now it has been described");
}

#[test]
fn nothing_in_the_book_is_worth_stealing() {
    // `reused_password` is a boolean about a password, which is the whole
    // design: the book records how protected you are, the vault holds the
    // things themselves. So this checks the written *values*, not the field
    // names — a secret would have to appear as one.
    let mut b = Book::default();
    b.note("gmail");
    b.set_second_factor("gmail", SecondFactor::App);
    b.set_reused("gmail", true);
    let written = serde_yaml::to_string(&b).unwrap();

    let allowed = [
        "true", "false", "gmail", "keystone", "app", "accounts", "site",
        "second_factor", "stakes", "reused_password", "has_recovery_codes",
        "settings_url", "https://myaccount.google.com/security", "-",
    ];
    for line in written.lines() {
        for part in line.split(':') {
            let v = part.trim().trim_start_matches("- ").trim();
            if v.is_empty() || v.starts_with("//") || v == "https" {
                continue;
            }
            assert!(
                allowed.contains(&v) || v.starts_with("//myaccount"),
                "an unexpected value reached the accounts file: {v:?} — this \
                 file records how safe you are and must never hold the things \
                 themselves"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Reading a request
// ---------------------------------------------------------------------------

#[test]
fn a_change_is_read_from_its_fields() {
    assert_eq!(
        Change::parse(Some("note"), Some("gmail"), None),
        Some(Change::Note("gmail".into()))
    );
    assert_eq!(
        Change::parse(Some("factor"), Some("gmail"), Some("passkey")),
        Some(Change::Factor("gmail".into(), SecondFactor::Passkey))
    );
    assert_eq!(
        Change::parse(Some("unique"), Some("gmail"), None),
        Some(Change::Reused("gmail".into(), false))
    );
}

#[test]
fn a_misspelled_factor_records_nothing_rather_than_the_weakest_thing() {
    assert_eq!(
        Change::parse(Some("factor"), Some("gmail"), Some("athenticator")),
        None,
        "falling back to a default here would silently record 'password only' \
         against your email — wrong in the dangerous direction"
    );
    assert_eq!(Change::parse(Some("sideways"), Some("gmail"), None), None);
    assert_eq!(Change::parse(Some("note"), Some("   "), None), None);
    assert_eq!(Change::parse(None, Some("gmail"), None), None);
}

#[test]
fn applying_a_change_reports_whether_anything_changed() {
    let mut b = Book::default();
    assert!(b.apply(&Change::Note("gmail".into())));
    assert!(!b.apply(&Change::Note("gmail".into())), "already tracked");
    assert!(b.apply(&Change::Factor("gmail".into(), SecondFactor::App)));
    assert!(
        !b.apply(&Change::Factor("gmail".into(), SecondFactor::App)),
        "no change means no write"
    );
    assert!(b.apply(&Change::Forget("gmail".into())));
    assert!(!b.apply(&Change::Forget("gmail".into())));
}

// ---------------------------------------------------------------------------
// The page
// ---------------------------------------------------------------------------

#[test]
fn an_empty_page_invites_you_to_start_rather_than_looking_broken() {
    let html = hub::accounts_page(&[], &[], &[], &[], false, &[]);
    assert!(html.contains("not tracking any accounts yet"));
    assert!(html.contains("value=note"), "and offers the way to fix that");
}

#[test]
fn the_page_leads_with_what_to_fix_not_with_an_inventory() {
    let mut b = Book::default();
    b.note("gmail");
    let advice = b.advice();
    let html = hub::accounts_page(&b.accounts, &advice, &[], &[], false, &[]);
    let fix = html.find("two-factor").expect("the advice is on the page");
    let list = html.find("<h2>Accounts</h2>").expect("the inventory too");
    assert!(fix < list, "an inventory is a list; the advice is the answer");
}

#[test]
fn the_page_says_atlas_opens_the_settings_rather_than_changing_them() {
    let mut b = Book::default();
    b.note("gmail");
    let html = hub::accounts_page(&b.accounts, &b.advice(), &[], &[], false, &[]);
    assert!(html.contains("Open the settings page"));
    assert!(
        !html.contains("Turn on two-factor for me"),
        "a button that quietly does less than it looks like it does is worse \
         than no button"
    );
}

#[test]
fn a_locked_vault_says_so_rather_than_looking_empty() {
    let html = hub::accounts_page(&[], &[], &[], &[], false, &[]);
    assert!(html.contains("vault is locked"));
    let open = hub::accounts_page(&[], &[], &[], &[], true, &[]);
    assert!(open.contains("vault is unlocked"));
}

#[test]
fn what_is_stored_is_named_in_words_not_in_types() {
    let stored = vec![(
        "gmail".to_string(),
        "the seed behind an authenticator code".to_string(),
    )];
    let html = hub::accounts_page(&[], &[], &[], &stored, true, &[]);
    assert!(html.contains("the seed behind an authenticator code"));
    assert!(!html.contains("TotpSeed"), "that is a variable name");
}

#[test]
fn a_site_name_from_outside_cannot_smuggle_markup_onto_the_page() {
    let mut b = Book::default();
    b.note("<script>alert(1)</script>");
    let html = hub::accounts_page(&b.accounts, &b.advice(), &[], &[], false, &[]);
    assert!(!html.contains("<script>alert"), "escaped");
}
