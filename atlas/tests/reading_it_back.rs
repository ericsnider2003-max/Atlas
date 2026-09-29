//! Saying it back before a security change, now that something asks.
//!
//! `confirmed.rs` was a complete, tested module nothing in the running
//! program could reach. It was written for Atlas making the change itself —
//! signing in and clicking the switch — and nothing here drives a settings
//! page, so there was no caller to write.
//!
//! But the read-back is the half that does the work, and it is worth exactly
//! as much when *you* are the one about to click: "turn it off on Instagram"
//! and "turn it off on Instagram and Facebook" sound alike, and Instagram's
//! settings page changes both. `atlas walkthrough turn-off` is the one place
//! in this tree where a security change actually gets started, and until
//! 19 Sep 2026 it started with no read-back and no yes.

use atlas::confirmed::{
    answer, before_a_run, how_to_undo, needs_reading_back, read_back, saying_it_back, Asked,
    Change, ConfirmConfig, Run, Step,
};

fn on() -> ConfirmConfig {
    ConfirmConfig { enabled: true, ..Default::default() }
}

fn asked(site: &str, change: Change) -> Asked {
    Asked { site: site.into(), account: String::new(), change }
}

// ================= which ones get said back =================

#[test]
fn reading_back_everything_is_a_switch_and_weakening_changes_ignore_it() {
    let weaker = asked("Instagram", Change::TurnOffTwoFactor);
    let stronger = asked("GitHub", Change::GenerateRecoveryCodes);

    // Shipped on: both get read back. "Turn it on" misheard as "turn it off"
    // is the failure this catches, and it goes both ways.
    let cfg = on();
    assert!(cfg.read_back_everything);
    assert!(needs_reading_back(&weaker, &cfg));
    assert!(needs_reading_back(&stronger, &cfg));

    // Off: only the ones that leave an account less protected than it is now.
    let mut quieter = on();
    quieter.read_back_everything = false;
    assert!(needs_reading_back(&weaker, &quieter), "a weakening change went through silently");
    assert!(!needs_reading_back(&stronger, &quieter));

    // And the feature being off means nothing is read back at all.
    let off = ConfirmConfig::default();
    assert!(!off.enabled);
    assert!(!needs_reading_back(&weaker, &off));
}

#[test]
fn the_read_back_names_the_site_the_change_and_what_it_costs_you() {
    let said = saying_it_back(&asked("Instagram", Change::TurnOffTwoFactor));
    assert!(said.contains("Instagram"), "{said}");
    assert!(said.to_lowercase().contains("two"), "{said}");
    assert!(said.ends_with("Yes or no?"), "{said}");
}

#[test]
fn the_vault_check_belongs_to_atlas_doing_it_and_not_to_you_doing_it() {
    // `read_back` refuses with the vault locked, because it was written for
    // Atlas signing in and making the change. A walkthrough is you making it.
    // Passing `true` for a vault nobody opened would have been a lie in the
    // shape of a precondition, so the read-back itself is separate.
    let a = asked("Instagram", Change::TurnOffTwoFactor);
    assert!(matches!(read_back(&a, true, false, &on()), Step::Cannot(_)));
    assert!(!saying_it_back(&a).is_empty(), "the read-back needs no vault");

    // And being away still stops the version that acts.
    assert!(matches!(read_back(&a, false, true, &on()), Step::Cannot(_)));
}

// ================= one at a time, and only what you agreed to =================

fn three() -> Vec<Asked> {
    vec![
        asked("Instagram", Change::TurnOffTwoFactor),
        asked("TikTok", Change::TurnOffTwoFactor),
        asked("GitHub", Change::TurnOffTwoFactor),
    ]
}

#[test]
fn a_run_cannot_express_a_batch_so_a_single_yes_cannot_cover_three() {
    // This used to be a `#[serde(skip)]` bool pinned true that nothing read.
    // The type is what holds it: there is no way in to answer for several.
    let mut r = Run::new(three());
    assert!(before_a_run(&r.items).contains("one at a time"));
    r.record(true);
    assert!(!r.finished());
    r.record(true);
    assert!(!r.finished());
    r.record(true);
    assert!(r.finished());
}

#[test]
fn saying_no_to_one_takes_that_one_out_and_leaves_the_others() {
    let mut r = Run::new(three());
    let first = r.current().cloned().unwrap();
    assert!(matches!(answer("no", &first), Step::Dropped));
    r.skip();
    let second = r.current().cloned().unwrap();
    assert_eq!(second.site, "TikTok");
    assert!(matches!(answer("yes", &second), Step::Go { .. }));
    r.record(true);
    r.skip();
    assert!(r.finished());

    // Only what you said yes to is recorded as done.
    assert_eq!(r.done.len(), 1);
    assert!(r.done[0].0.starts_with("TikTok"));
    assert!(r.summary().contains("1 done"));
    assert!(r.summary().contains("2 skipped"), "{}", r.summary());
}

#[test]
fn saying_yes_comes_with_how_to_put_it_back() {
    // The question you'll have later and won't remember the answer to.
    let off = how_to_undo(&Change::TurnOffTwoFactor);
    assert!(off.contains("same page"), "{off}");
    // Turning it back on needs something in hand, and turning it off does
    // not — so the two are not the same sentence with the words swapped.
    assert!(off.contains("you'll need"), "{off}");
    let on = how_to_undo(&Change::TurnOnTwoFactor);
    assert_ne!(off, on, "both directions give the same advice");
    assert!(!on.contains("you'll need"), "{on}");

    // A switch between methods names both of them, in the direction you
    // would be going back.
    let switched = how_to_undo(&Change::SwitchMethod {
        from: "a text code".into(),
        to: "an authenticator app".into(),
    });
    assert!(switched.contains("from an authenticator app to a text code"), "{switched}");

    // And the one that cannot be undone says so rather than offering a page.
    let codes = how_to_undo(&Change::GenerateRecoveryCodes);
    assert!(codes.contains("old set is gone"), "{codes}");
    assert!(!codes.contains("same page to"), "{codes}");
}

// ================= it is actually reachable =================

#[test]
fn the_walkthrough_is_what_asks_rather_than_these_tests() {
    // The point of the change. Everything above passed for weeks while
    // nothing in the running program ever built an `Asked`.
    let main = crate::common::source_of("main");
    assert!(
        main.contains("confirmed::needs_reading_back(a, &ccfg)"),
        "the walkthrough still starts a security change with no read-back"
    );
    assert!(
        main.contains("confirmed::saying_it_back("),
        "nothing says the change back to you"
    );
    assert!(main.contains("Some(said @ (\"yes\" | \"no\"))"), "there is no way to answer");
    assert!(
        main.contains("confirmed::record(&asked, worked, said,"),
        "nothing keeps a trail of what was changed and when"
    );
    // Only the ones you agreed to are walked. A walk that still visited the
    // pages you declined would make the question decorative.
    assert!(
        main.contains("w.stops.retain(|stop| confirmed_sites.iter().any(|s| s == &stop.site))"),
        "the walk ignores what you said no to"
    );

    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("read_back_everything:"));
    // Still off in the shipped file: this is a feature you turn on knowing
    // what it is.
    assert!(!ConfirmConfig::default().enabled);

    // And it is no longer listed as a module nothing can reach.
    let wiring = std::fs::read_to_string("tests/wiring.rs").expect("wiring.rs");
    let baseline = wiring
        .split("UNWIRED_BASELINE")
        .nth(1)
        .and_then(|s| s.split_once("];"))
        .map(|(a, _)| a)
        .unwrap_or("");
    assert!(!baseline.contains("\"confirmed\","), "still listed as unreachable");
}
