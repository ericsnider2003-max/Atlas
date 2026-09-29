//! `server.enabled: false` listened anyway.
//!
//! The local API is what your dashboard is served from, and what a phone
//! talks to. `config/tools.yaml` shipped it as `enabled: false`, which read
//! as "off unless you ask" — the convention for everything else in that file.
//!
//! It was never off. `Server::bind` did not read `enabled` at all, and the
//! daemon's start-up path took a copy of your config and set `enabled = true`
//! on it before calling `bind`. So the listener came up on every start, and
//! the one setting a person would use to stop it did nothing.
//!
//! Loopback-only with a token is a narrow door and this was never a wide
//! hole. It is the *switch* that was the defect: you turned it off, it stayed
//! on, and nothing told you.

use atlas::config::Config;
use atlas::server::{Server, ServerConfig};
use std::path::Path;

fn token() -> String {
    "a-token-long-enough-to-be-accepted".into()
}

#[test]
fn switching_the_local_api_off_actually_stops_it_listening() {
    let off = ServerConfig { enabled: false, ..ServerConfig::default() };
    let r = Server::bind(&off, &token());
    assert!(r.is_err(), "the listener came up with the setting off");
    let why = format!("{}", r.err().unwrap());
    assert!(
        why.contains("server.enabled"),
        "the refusal doesn't name the setting that caused it: {why}"
    );
}

#[test]
fn the_shipped_config_says_what_actually_happens() {
    // It shipped `false` while the code forced `true`. Either half could have
    // been the one to change; the honest direction is the one that keeps
    // today's behaviour and makes the switch real, because nobody's dashboard
    // should disappear on an upgrade.
    let c = Config::load(Path::new("config")).unwrap();
    assert!(
        c.tools.as_ref().unwrap().server.enabled,
        "the config now claims off — which would silently take away the dashboard"
    );
}

#[test]
fn the_daemon_no_longer_overrides_your_setting() {
    // A source check, because the defect was one assignment in the start-up
    // path rather than anything the API does.
    let main = crate::common::source_of("main");
    assert!(
        !main.contains("scfg.enabled = true;"),
        "the start-up path is overriding server.enabled again"
    );
}

#[test]
fn asking_for_the_settings_page_by_name_still_opens_it() {
    // The one place that should ignore the switch: you have typed `atlas
    // settings`, and the command that opens a page is not the same thing as
    // the dashboard a daemon leaves listening.
    let main = crate::common::source_of("main");
    assert!(
        main.contains("ServerConfig { enabled: true, ..ServerConfig::default() }"),
        "the settings page no longer enables itself explicitly, so `atlas \
         settings` breaks for anyone who turned the API off"
    );
}
