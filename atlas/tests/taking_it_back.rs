//! Which sites Atlas can sign into, and taking that away.
//!
//! `signin.rs` says of itself, in its third design constraint:
//!
//! > **Access is per-site and revocable from one page**, without touching
//! > anything else and without your having to remember what you granted.
//!
//! On 19 Sep 2026 none of that worked, and the failures compounded:
//!
//! 1. Nothing anywhere called `Access::grant`, so there was never a grant.
//! 2. Nothing loaded or saved an `Access` — the daemon built a
//!    `default()` at startup — so one could not have survived a restart.
//! 3. The access page was handed an empty slice of sites, so the "signed in
//!    right now" section never rendered. That page's own docstring warns that
//!    an access page saying nothing reads as *nothing to worry about*, and
//!    then did exactly that for the half it was warning about.
//! 4. The revoke buttons it renders post to `/hub/access/revoke`, and no such
//!    route existed. A revoke button that does nothing is worse than no
//!    button, because you press it and believe it.
//! 5. `Access::may_fill` — the lookalike-domain check the whole module exists
//!    for — had no caller at all.
//!
//! Nine `signin::` functions were on the dead list. They were not nine
//! separate omissions; they were one capability with no way in and no way
//! out.

use atlas::signin::{self, Access, Allowed, Refused, SignInConfig};
use atlas::store::Store;
use std::path::PathBuf;

const DAY: u64 = 86_400;

/// A store of its own, wiped first, so a run never reads what the last one
/// left.
fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-access-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).expect("a temp dir");
    p
}

fn on() -> SignInConfig {
    SignInConfig { enabled: true, ..Default::default() }
}

fn granted() -> Access {
    let mut a = Access::default();
    a.grant("mybank.com", "eric", "the bank", Allowed::SignIn, "bank login", 1000);
    a.grant("github.com", "esnider", "GitHub", Allowed::SignInAndUse, "gh", 1000);
    a
}

// ================= a grant survives the machine going off =================

#[test]
fn what_you_granted_is_still_there_after_a_restart() {
    // The daemon built an `Access::default()` at startup and nothing ever
    // loaded or saved one. Even if something had granted, the grant would
    // have lasted until the next restart and then been silently gone — and
    // "I don't have access to that" is indistinguishable from never having
    // granted it.
    let store = Store::new(tmp("restart"));

    let mine = granted();
    mine.save(&store).expect("it saves");

    let back = Access::load(&store);
    assert_eq!(back.grants.len(), 2);
    assert!(back.find("mybank.com").is_some());
    assert_eq!(back.find_account("github.com", "esnider").map(|g| g.allowed), Some(Allowed::SignInAndUse));

    // And a store that has never held one comes back empty rather than
    // failing, which is what a first run looks like.
    assert!(Access::load(&Store::new(tmp("first-run"))).grants.is_empty());
}

#[test]
fn a_grant_points_at_the_vault_and_never_holds_the_password() {
    let a = granted();
    let g = a.find("mybank.com").expect("granted");
    assert_eq!(g.vault_entry, "bank login");
    // Nothing anywhere in what is stored is worth anything on its own.
    let whole = serde_json::to_string(&a).expect("it serialises");
    for secret in ["password", "passphrase", "hunter2"] {
        assert!(!whole.contains(secret), "{whole}");
    }
}

// ================= one site, taken away, without touching the rest =========

#[test]
fn taking_one_away_leaves_the_others_exactly_as_they_were() {
    let mut a = granted();
    assert!(a.revoke("mybank.com"));
    assert!(a.find("mybank.com").is_none());
    assert!(a.find("github.com").is_some(), "revoking one site took another with it");

    // Revoking something that was never granted says so rather than
    // pretending.
    assert!(!a.revoke("mybank.com"), "it revoked the same thing twice");
    assert!(!a.revoke("never-heard-of-it.com"));

    // A subdomain is the same registered domain, so taking away
    // `login.github.com` takes away GitHub. That is the point of matching on
    // the registered domain rather than on the URL.
    assert!(a.revoke("login.github.com"));
    assert!(a.grants.is_empty());
}

#[test]
fn taking_all_of_it_away_says_how_much_went() {
    let mut a = granted();
    assert_eq!(a.revoke_all(), 2);
    assert!(a.grants.is_empty());
    assert_eq!(a.revoke_all(), 0, "it counted an empty list as work done");
}

// ================= the page that says it can be taken away =================

#[test]
fn the_access_page_lists_the_doors_that_are_open_and_a_way_to_close_each() {
    let rows = signin::hub_rows(&granted(), 2000);
    assert_eq!(rows.len(), 2);

    let page = atlas::hub::access_page_full(&[], &[], &rows);
    assert!(page.contains("Signed in right now"), "the sites section is missing");
    assert!(page.contains("the bank"), "{page}");
    assert!(page.contains("mybank.com"), "the revoke form has no site to act on");
    assert!(page.contains("/hub/access/revoke"), "there is no way to take it back");
    assert!(page.contains("Take all of it away"));

    // And an empty page does not claim there is nothing to worry about by
    // rendering a section with nothing in it.
    let empty = atlas::hub::access_page_full(&[], &[], &[]);
    assert!(!empty.contains("Signed in right now"));
    assert!(empty.contains("What Atlas never holds"), "the honest half is still there");
}

#[test]
fn the_revoke_buttons_post_to_routes_that_exist() {
    // They have been rendered since the page was written, posting to routes
    // that did not exist. The page and the server disagreed, and the page was
    // the convincing one.
    use atlas::server::{route, Action, Request};
    let post = |path: &str, body: &str| Request {
        method: "POST".into(),
        path: path.into(),
        query: String::new(),
        token: Some("x".repeat(24)),
        token_from_url: false,
        body: body.into(),
    };
    assert!(matches!(
        route(&post("/hub/access/revoke", "domain=mybank.com")),
        Some(Action::RevokeAccess(d)) if d == "mybank.com"
    ));
    assert!(matches!(
        route(&post("/hub/access/revoke-all", "")),
        Some(Action::RevokeAllAccess)
    ));
    // A revoke with no site named is not a revoke of everything: back to the
    // Access page saying nothing was taken (27 Sep 2026; it was `None`, which
    // showed "that isn't a page in Atlas").
    assert!(matches!(
        route(&post("/hub/access/revoke", "")),
        Some(Action::HubBack(atlas::hub::Page::Access, said)) if said.contains("nothing was taken away")
    ));
}

#[test]
fn the_hub_is_what_fills_the_page_rather_than_this_test() {
    let live = crate::common::source_of("hublive");
    assert!(
        live.contains("crate::signin::hub_rows(&self.access, now)"),
        "the access page is still handed an empty list of sites"
    );
    assert!(
        live.contains("Action::RevokeAccess(domain)"),
        "nothing acts on the revoke button"
    );
    assert!(
        live.contains("self.access.save(&self.store)"),
        "a revoke that is only in memory is a revoke that did not happen"
    );

    let daemon = crate::common::source_of("daemon");
    assert!(
        daemon.contains("crate::signin::Access::load(&store_for_load)"),
        "grants still start empty on every run"
    );
    assert!(
        daemon.contains("self.access.note_use("),
        "nothing records that a credential was used"
    );

    let main = crate::common::source_of("main");
    assert!(main.contains("fn run_access("), "there is still no way to grant anything");
    assert!(main.contains("access.grant(&site, &account, &name, allowed,"));
    assert!(
        main.contains("Action::RevokeAccess(_) | Action::RevokeAllAccess"),
        "the settings-only hub silently shows a different page instead"
    );
}

// ================= what the list is for =================

#[test]
fn a_credential_that_has_stopped_working_is_the_thing_worth_saying() {
    // Age alone is not a problem. A password that was changed somewhere else
    // is, and "sign-in failed" sends you to check the site when the answer is
    // usually simpler.
    let mut a = granted();
    a.note_use("mybank.com", "eric", "mybank.com/login", true, true, 1000);
    a.note_use("mybank.com", "eric", "mybank.com/login", false, true, 2000);
    a.note_use("mybank.com", "eric", "mybank.com/login", false, true, 3000);

    let g = a.find("mybank.com").expect("granted");
    assert!(g.looks_superseded, "two failures after it worked is the tell");
    let said = signin::probably_changed(g);
    assert!(said.contains("the bank"), "{said}");
    assert!(said.contains("password was changed somewhere else"), "{said}");

    let rows = signin::hub_rows(&a, 4000);
    let bank = rows.iter().find(|(l, _, _, _)| l.contains("the bank")).expect("listed");
    assert!(bank.3, "the page doesn't flag it");
    assert!(bank.1.contains("looks like it changed"), "{}", bank.1);

    // Pointing it at the new entry clears it, rather than needing the grant
    // to be made again from scratch.
    assert!(a.superseded_by("mybank.com", "eric", "new bank login"));
    let g = a.find("mybank.com").expect("still granted");
    assert!(!g.looks_superseded);
    assert_eq!(g.vault_entry, "new bank login");
    assert_eq!(g.failures_in_a_row, 0);

    // And naming a site that was never granted is said rather than silently
    // doing nothing.
    assert!(!a.superseded_by("nowhere.com", "eric", "x"));
}

#[test]
fn what_it_says_out_loud_counts_logins_and_sites_separately() {
    let mut a = granted();
    // Two accounts on one site is two logins across one site, and saying
    // "two sites" would be wrong in the way that matters: signing into the
    // wrong one of two accounts is hard to see and hard to undo.
    a.grant("github.com", "work", "GitHub (work)", Allowed::SignIn, "gh-work", 1000);
    let said = signin::spoken(&a, 2000);
    assert!(said.contains("3 logins across 2 sites"), "{said}");

    assert_eq!(
        signin::spoken(&Access::default(), 0),
        "I can't sign into anything. Say \"give me access to\" and a site."
    );

    // A failed sign-in this week is worth a sentence.
    a.note_use("mybank.com", "eric", "p", false, true, 10 * DAY);
    let said = signin::spoken(&a, 10 * DAY + 3600);
    assert!(said.contains("failed this week"), "{said}");
}

#[test]
fn a_grant_that_exists_is_what_makes_the_refusal_mean_something() {
    // `may_start` could only ever answer `NotGranted`, because there was
    // never a grant. With one, the answer is about this site rather than
    // about the empty list.
    let a = granted();
    let cfg = on();
    assert!(a.may_start("mybank.com", true, true, &cfg).is_ok());
    assert!(matches!(
        a.may_start("mybank.com.evil.co", true, true, &cfg),
        Err(Refused::NotGranted(_))
    ));
    assert_eq!(a.may_start("mybank.com", false, true, &cfg), Err(Refused::Locked));

    // And once taken away, the same site refuses again.
    let mut gone = granted();
    gone.revoke("mybank.com");
    assert!(matches!(
        gone.may_start("mybank.com", true, true, &cfg),
        Err(Refused::NotGranted(_))
    ));
}
