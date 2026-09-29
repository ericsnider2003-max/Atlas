//! Online delegation ships off, offline-first, and set up correctly.
//!
//! Cloudflare delegation is the one thing here that reaches a third-party
//! service, so — like research — it waits to be asked for. These check that
//! the shipped config is off but complete (so turning it on is the only
//! step), that readiness names the missing piece rather than failing on first
//! use, and that a disabled or offline provider never delegates.

use atlas::config::Config;
use atlas::online::{readiness, Readiness};
use std::path::Path;

fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}

#[test]
fn cloudflare_ships_off_but_ready_to_be_switched_on() {
    let c = cfg();
    let cf = c.tools.as_ref().expect("tools.yaml loads").cloudflare.clone();
    assert!(!cf.enabled, "the online booster must wait to be asked for");
    // Complete enough that enabling + adding an account and token is the only
    // work — the switch must not be a switch that does nothing.
    assert!(cf.inference.is_some(), "no inference endpoint, so the switch would do nothing");
    assert!(!cf.model.trim().is_empty(), "a model must be pre-filled so there's a working default");
    assert!(!cf.token_vault.trim().is_empty(), "the vault entry name must be pre-filled");
    assert!(cf.verify, "verification should default on — the point is to check what comes back");
}

#[test]
fn readiness_is_blocked_until_it_is_set_up() {
    let c = cfg();
    let cf = c.tools.as_ref().unwrap().cloudflare.clone();
    // Shipped: disabled → blocked, and the reason says so.
    match readiness(&cf, false) {
        Readiness::Blocked(why) => assert!(why.contains("switched off")),
        Readiness::Ready => panic!("shipped config must not be ready"),
    }
}

#[test]
fn enabled_without_a_token_names_the_vault_entry() {
    let c = cfg();
    let mut cf = c.tools.as_ref().unwrap().cloudflare.clone();
    cf.enabled = true;
    cf.account_id = "acct".into();
    // Everything set except the token in the vault.
    let r = readiness(&cf, false);
    assert!(!r.ready_now(), "no token means not ready");
    assert!(
        r.why().contains(&cf.token_vault) || r.why().contains("token"),
        "the reason should name the missing token: {}",
        r.why()
    );
}

#[test]
fn enabled_and_set_up_is_ready() {
    let c = cfg();
    let mut cf = c.tools.as_ref().unwrap().cloudflare.clone();
    cf.enabled = true;
    cf.account_id = "acct".into();
    assert!(readiness(&cf, true).ready_now(), "with everything set and a token present, ready");
}
