//! You can reach Atlas without reading a console window — and not through a
//! browser.
//!
//! ## The ruling this file is shaped by
//!
//! **Eric, 17 Sep 2026: "the hub should not be a browser. You say that if
//! Atlas has a dependency on the internet."** The same position
//! `tests/capability_wiring.rs` already records against `look`: an HTML panel
//! needs an external browser or a bundled web engine, and this system is
//! in-house and self-contained, so the panels are painted natively instead.
//!
//! So the local web server is **for the phone** — a phone cannot run the
//! native window, and loopback plus a VPN is the honest way to reach a desktop
//! from one. On this machine the surface is Atlas's own window.
//!
//! **Eric, 23 Sep 2026**, asked what the hub was for if the laptop couldn't
//! reach it: the hub is where everything is tracked, accessed and changed.
//! His ruling: it shows **inside Atlas's window**, drawn by the web view that
//! ships with Windows — still no browser, and still no internet: the region
//! only loads Atlas's own hub on this machine. See `src/hubwin.rs` and
//! `tests/hub_in_the_window.rs`. What this file still forbids is Atlas
//! launching someone else's program to show you your own pages.
//!
//! ## What was wrong
//!
//! Reaching the server meant starting the daemon, finding the console window
//! it prints into, and copying a 28-character token out of the address. Every
//! time, because the token was different every time.
//!
//! Two separate defects, found 17 Sep 2026 while answering "why is getting to
//! the hub still this hard?":
//!
//! 1. **The token was minted fresh on every start.** `server.rs`'s own rule 2
//!    says "generated on first run, stored with the rest of Atlas's state",
//!    and nothing stored it. So the URL changed every launch and could not be
//!    bookmarked, pinned, or saved on a phone — which is what this module
//!    exists for: `carry on from my phone` broke on every desktop reboot.
//!
//! 2. **`atlas settings` printed addresses with no token on them.** Every
//!    request carries a token with no exemptions — `token_matches` returns
//!    false for `None` — so the server answered `Denied` to both printed
//!    addresses, and the token it had just generated was printed nowhere at
//!    all. That is menu item 3 in `ATLAS.bat`, labelled *"works even when
//!    Atlas won't"*: the recovery path for when the voice or the daemon is
//!    broken answered Denied.
//!
//! The second one is the more interesting failure. It is not a subtle bug —
//! the feature was completely non-functional — and it survived because
//! nothing ever asserted that a printed address is one the server would
//! accept. A URL in a `println!` is not reachable by any test that does not
//! go looking for it deliberately.

use std::collections::HashSet;
use std::fs;

fn main_rs() -> String {
    crate::common::source_of("main")
}

fn server_rs() -> String {
    crate::common::source_of("server")
}

/// Every line that prints something starting `http://127.0.0.1`.
fn printed_addresses(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            !t.starts_with("//") && !t.starts_with("///") && t.contains("http://127.0.0.1")
        })
        .map(|l| l.trim().to_string())
        .collect()
}

#[test]
fn no_printed_hub_address_is_missing_its_token() {
    // The assertion that would have caught defect 2 on the day it was
    // written. A hub address without `?t=` is refused by the server, so
    // printing one tells a person to go somewhere that will not let them in.
    //
    // `hub_url` is the sanctioned way to build one and it always appends the
    // token, so a literal `http://127.0.0.1...` in a format string is the
    // shape to object to.
    let offenders: Vec<String> = printed_addresses(&main_rs())
        .into_iter()
        .filter(|l| !l.contains("?t="))
        .collect();
    assert!(
        offenders.is_empty(),
        "these print a hub address with no token, which the server answers Denied to. \
         Use `server::hub_url(port, &token, path)`:\n  {}",
        offenders.join("\n  ")
    );
}

#[test]
fn the_token_is_stored_rather_than_minted_every_start() {
    // Defect 1. `new_token` is still the right thing on FIRST run -- the
    // point is that nothing reaches for it when a token already exists.
    let src = server_rs();
    assert!(
        src.contains("pub fn token_for("),
        "server::token_for is gone -- the hub token is back to being ephemeral \
         and the dashboard URL changes on every restart"
    );
    assert!(
        src.contains("store.save(FILE, &HeldToken"),
        "token_for no longer writes the token down, so it is regenerated every \
         start and no bookmark survives a reboot"
    );

    // And every `Server::bind` is handed a stored token, not a fresh one.
    // This is the half that actually regressed: `token_for` can exist and be
    // bypassed, which is the state this tree was in.
    //
    // Scoped to functions that bind the hub, NOT to `new_token` everywhere.
    // `new_token` is still exactly right for the three other things that use
    // it -- a kin invite's peer token and two household pairing codes -- and
    // those *must* be fresh per invite. The first version of this test said
    // "no `new_token` in main.rs" and flagged all three, which would have
    // pushed a one-time code into permanent storage to silence it. A rule
    // that is wrong about the neighbours does not get to be strict.
    let main = main_rs();
    let mut offenders = Vec::new();
    for (name, body) in functions_of(&main) {
        // 28 Sep 2026: the daemon's hub is opened by `server::open_hub`
        // (which binds, and keeps trying when the port is taken), so that
        // counts as binding the hub too -- otherwise `run_daemon` would have
        // slipped out of this guard. (Its `new_token` is the start's id for
        // `/hub/ping`, not the hub token.)
        if !body.contains("Server::bind(") && !body.contains("open_hub(") {
            continue;
        }
        if !body.contains("token_for") {
            offenders.push(name);
        }
    }
    assert!(
        !offenders.is_empty() || main.contains("Server::bind("),
        "no function in main.rs binds the hub any more, so this guard is measuring nothing"
    );
    assert!(
        offenders.is_empty(),
        "these bind the hub with a token that did not come from `server::token_for`, \
         so the dashboard address changes every launch: {offenders:?}"
    );
}

/// `main.rs` split into `(name, body)` at each top-level `fn`.
fn functions_of(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut name = String::from("<before the first fn>");
    let mut body = String::new();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("fn ") {
            out.push((name, std::mem::take(&mut body)));
            name = rest.split('(').next().unwrap_or(rest).trim().to_string();
        }
        body.push_str(line);
        body.push('\n');
    }
    out.push((name, body));
    out
}

#[test]
fn the_stored_token_is_the_same_one_next_time() {
    // The behaviour, not the shape of the code. Two reads of the same store
    // must give the same token, and it must survive the store being dropped
    // and reopened -- that is what "bookmarkable" means.
    let dir = std::env::temp_dir().join(format!("atlas-hub-token-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("temp dir");

    let first = {
        let store = atlas::store::Store::new(&dir);
        atlas::server::token_for(&store).expect("a first token")
    };
    let second = {
        let store = atlas::store::Store::new(&dir);
        atlas::server::token_for(&store).expect("a second token")
    };

    assert_eq!(first, second, "the token changed between runs, so a bookmark would break");
    assert!(first.len() >= 24, "the token got shorter: {first}");
    // Still random, not a constant compiled in: a different install gets a
    // different token.
    let elsewhere = std::env::temp_dir().join(format!("atlas-hub-other-{}", std::process::id()));
    let _ = fs::remove_dir_all(&elsewhere);
    fs::create_dir_all(&elsewhere).expect("temp dir");
    let other = {
        let store = atlas::store::Store::new(&elsewhere);
        atlas::server::token_for(&store).expect("a token")
    };
    assert_ne!(
        first, other,
        "two installs share one token -- it is a constant, not randomness"
    );

    let _ = fs::remove_dir_all(&dir);
    let _ = fs::remove_dir_all(&elsewhere);
}

#[test]
fn a_built_address_is_one_the_server_would_accept() {
    // Ties the two halves together: the thing `hub_url` produces has to carry
    // a token the request parser will find and `token_matches` will accept.
    // Asserted against `token_matches` itself rather than against the string
    // `?t=`, so a change to how the token is carried fails here rather than
    // silently printing something refused.
    let token = "abcdef-ghjkmn-pqrstu-vwxyz2";
    let url = atlas::server::hub_url(8787, token, "/hub");
    assert!(url.starts_with("http://127.0.0.1:8787/hub?t="), "unexpected shape: {url}");

    let query = url.split_once("?t=").expect("a token in the address").1;
    assert!(
        atlas::server::token_matches(token, Some(query)),
        "the address carries something the server would not accept: {query}"
    );
    assert!(
        !atlas::server::token_matches(token, None),
        "a request with no token is accepted, so the address does not need one \
         and this whole file is about nothing"
    );
}

#[test]
fn there_is_one_command_that_says_where_the_hub_is() {
    // The answer to "how do I get to it" should be a command, not a paragraph
    // of instructions about console windows.
    let src = main_rs();
    assert!(
        src.contains(r#"== Some("hub")"#),
        "`atlas hub` is gone -- the only way to learn the address is to read \
         the daemon's console output again"
    );
    assert!(
        src.contains("fn run_hub_address("),
        "the hub command dispatches to nothing"
    );
    // It must print the address even when it cannot open a browser, because
    // the address is the useful part and the browser is a convenience.
    // To the closing brace at the function's own depth (29 Sep 2026): cutting at the next plain `fn` at that depth ran on past `pub(super) fn`s once main.rs and daemon.rs were split.
    let body = src
        .split_once("fn run_hub_address(")
        .expect("run_hub_address")
        .1
        .split("\n}\n")
        .next()
        .unwrap_or_default()
        .to_string();
    assert!(
        body.contains("hub_url("),
        "the hub command does not build the address through `hub_url`, so it can \
         drift from the one the daemon prints"
    );
    // It must point at the native window for this machine, not at a browser,
    // and must not pretend the native settings panel is finished when it is
    // a placeholder that lists nothing.
    assert!(
        body.contains("show me settings"),
        "the hub command does not tell you how to reach settings on this machine \
         through Atlas's own window"
    );
    // The native settings are real now (the Atlas window's Settings page,
    // `settingswin`), so the command must not still call them a placeholder
    // and send you to the browser page instead.
    assert!(
        !body.contains("placeholder"),
        "the hub command still calls the native settings a placeholder"
    );
    assert!(
        src.contains("Some(\"home\")") && src.contains("\"settings\""),
        "the Settings page it points at isn't reachable from the program"
    );
}

#[test]
fn nothing_opens_a_browser() {
    // **Eric's ruling, 17 Sep 2026: "the hub should not be a browser. You say
    // that if Atlas has a dependency on the internet."**
    //
    // An earlier version of this session added `open_in_browser` and had
    // `atlas hub open` launch Chrome. That walked straight back into the
    // design position the panels were rebuilt to escape:
    // `tests/capability_wiring.rs` records it against `look` -- rendering the
    // panels as HTML "needs an external browser or a bundled web engine, and
    // the ruling for this system is in-house and self-contained".
    //
    // The local web server stays, because it is how a PHONE reaches this
    // desktop and a phone cannot run the native window. What must not come
    // back is Atlas launching someone else's program to show you your own
    // settings.
    let src = main_rs();
    let launching: Vec<&str> = src
        .lines()
        .filter(|l| {
            let t = l.trim_start();
            if t.starts_with("//") || t.starts_with("///") {
                return false;
            }
            t.contains("xdg-open")
                || t.contains("\"start\", \"\"")
                || (t.contains("Command::new(\"open\")"))
        })
        .collect();
    assert!(
        launching.is_empty(),
        "something launches an external browser. The hub is not a browser -- the \
         native window is the surface on this machine:\n  {}",
        launching.join("\n  ")
    );

    // And the launcher does not claim otherwise.
    let bat = fs::read_to_string("ATLAS.bat").expect("ATLAS.bat");
    assert!(
        !bat.contains("opening in your browser"),
        "ATLAS.bat still says a browser is opening"
    );
}

#[test]
fn the_hub_pages_a_person_is_sent_to_actually_exist() {
    // A printed address for a route the server does not serve is the same
    // defect as one missing its token, arriving from the other side. Checked
    // against `hub::route`'s own list rather than a copy of it.
    let hub = crate::common::source_of("hub");
    let routed: HashSet<&str> = hub
        .lines()
        .filter_map(|l| l.split_once("\"/hub").and_then(|(_, r)| r.split('"').next()))
        .map(|r| r.trim())
        .collect();
    assert!(
        !routed.is_empty(),
        "no /hub routes were found in hub.rs, so this guard would pass for anything"
    );

    for line in printed_addresses(&main_rs()) {
        // The path is whatever `hub_url` was handed; pull it back out.
        let Some(after) = line.split_once("/hub") else { continue };
        let path: String = after.1.chars().take_while(|c| *c != '?' && *c != '"').collect();
        if path.is_empty() {
            continue; // "/hub" itself, always served
        }
        assert!(
            routed.contains(path.as_str()),
            "main.rs sends a person to /hub{path}, which hub::route does not serve. \
             Routes it does serve: {routed:?}"
        );
    }
}
