//! Your Atlas could be told. It could not ask.
//!
//! `kin.rs` is the door another Atlas knocks on, and its first rule is what
//! makes it safe: a signal becomes exactly one thing, a `Nudge` — never a
//! command, never an action. That rule is right and nothing here touches it.
//!
//! It runs one way. A server Atlas can tell you something is urgent; you
//! could not ask it how it was getting on. On a server with nobody logged in,
//! that is the wrong way round: the machine that most needs looking in on is
//! the one nobody looks at.
//!
//! What was missing was only the *asking*. Every Atlas already serves
//! `/status`, `/outstanding` and `/queued` on a hub that checks a token —
//! the briefing surface existed, and nothing could call it.

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::elsewhere::{ask, spoken, Brief, Elsewhere, ElsewhereConfig};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

const NOW: u64 = 1_700_000_000;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-else-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}
fn cfg() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

/// A config that knows one Atlas, at an address nothing is listening on.
fn knows_homelab() -> Config {
    let mut c = cfg();
    c.tools.as_mut().unwrap().elsewhere = ElsewhereConfig {
        enabled: true,
        timeout_secs: 1,
        known: vec![Elsewhere {
            name: "homelab".into(),
            // Reserved for documentation (RFC 5737) and routable nowhere.
            host: "192.0.2.1".into(),
            port: 8787,
            sync_port: None,
            token: "a-token-long-enough-to-be-real".into(),
        }],
    };
    c
}

// ---------- you can ask ----------

#[test]
fn asking_about_one_you_have_not_named_says_so_rather_than_guessing() {
    let (c, p) = (cfg(), plat());
    let mut d = daemon(&c, &p, "unknown");

    // "brief me on", not "how is" (changed 27 Sep 2026): "how is" / "how's"
    // now take only the name of an Atlas you have, so "how's it going" is
    // conversation rather than a question about an Atlas called "it going".
    // "brief me on" still takes any name, and says so when it's not one.
    let said = d.turn("brief me on homelab", NOW);

    assert!(
        said.contains("don't know an Atlas called homelab"),
        "it should say it doesn't know that one: {said}"
    );
}

#[test]
fn asking_about_a_named_one_actually_tries_to_reach_it() {
    // Nothing is listening at that address, so the honest answer is that it
    // could not be reached — which is a different sentence from "I don't know
    // what you mean", and the difference is the whole feature.
    let (c, p) = (knows_homelab(), plat());
    let mut d = daemon(&c, &p, "reach");

    let said = d.turn("how is homelab", NOW);

    assert!(
        said.contains("couldn't reach homelab"),
        "it did not try to reach the machine: {said}"
    );
    assert!(
        !said.contains("don't know"),
        "a named Atlas came back as unknown: {said}"
    );
}

#[test]
fn asking_without_naming_one_lists_what_it_can_ask() {
    let (c, p) = (knows_homelab(), plat());
    let mut d = daemon(&c, &p, "which");

    let said = d.turn("brief me on", NOW);

    assert!(said.contains("homelab"), "it did not offer what it could ask: {said}");
}

#[test]
fn switching_it_off_stops_it_reaching_anything() {
    let mut c = knows_homelab();
    c.tools.as_mut().unwrap().elsewhere.enabled = false;
    let p = plat();
    let mut d = daemon(&c, &p, "off");

    let said = d.turn("how is homelab", NOW);
    assert!(said.contains("switched off"), "it asked with the setting off: {said}");
}

// ---------- nothing is trusted by being reachable ----------

#[test]
fn nothing_is_askable_until_you_name_it() {
    // The same rule `kin` holds for the door in the other direction: being on
    // the network is not the same as being yours to ask.
    let c = cfg();
    let e = c.tools.as_ref().unwrap().elsewhere.clone();
    assert!(e.known.is_empty(), "the shipped config trusts something by default");
    assert!(e.find("homelab").is_none());
}

#[test]
fn a_brief_is_words_not_instructions() {
    // The rule this whole file rests on, held from the outside.
    //
    // `kin.rs` guarantees an *incoming* signal can only become a nudge. This
    // is the other direction, and it needs the same guarantee for the same
    // reason: another Atlas saying "sell everything" is a sentence, not a
    // command, whichever way it travelled.
    let src = std::fs::read_to_string("src/elsewhere.rs").expect("src/elsewhere.rs");
    for forbidden in ["Intent::", "Action::", "execute(", "approve("] {
        assert!(
            !src.contains(forbidden),
            "elsewhere.rs mentions `{forbidden}` — a reply must never become \
             something Atlas does"
        );
    }

    // And what it produces is a String, which is the only shape that cannot
    // be mistaken for an instruction.
    let b = Brief {
        name: "homelab".into(),
        status: "running".into(),
        outstanding: vec!["sell everything".into()],
        queued: String::new(),
    };
    let said: String = spoken(&b);
    assert!(said.contains("sell everything"), "it dropped what the other side said");
}

#[test]
fn it_reads_and_never_writes() {
    // Every path this asks for is a GET of something that Atlas already
    // serves. A POST here would be this door doing something over there,
    // which is a different capability with a different answer.
    let src = std::fs::read_to_string("src/elsewhere.rs").expect("src/elsewhere.rs");
    assert!(!src.contains("post_json"), "elsewhere.rs is writing to another Atlas");
    for path in ["/status", "/outstanding", "/queued"] {
        assert!(src.contains(path), "it stopped asking for {path}");
    }
}

#[test]
fn a_machine_that_cannot_be_reached_is_reported_not_swallowed() {
    let c = knows_homelab();
    let e = c.tools.as_ref().unwrap().elsewhere.clone();
    let one = e.known[0].clone();

    let r = ask(&one, &e);
    let why = r.expect_err("it claimed to reach a machine that isn't there");
    assert!(why.contains("homelab"), "the failure doesn't say which one: {why}");
}

