//! What Atlas can honestly say about a private network it cannot use.
//!
//! `mesh.rs` is on `CAPABILITY_UNWIRED` and belongs there. `choose` picks
//! between `SameNetwork`, `Mesh`, `Cable` and `Cloud`, and only `Cloud` is
//! built — syncing goes through a folder both machines can see. Nothing in
//! this tree reaches another device directly.
//!
//! That is the transport. The **advice** is a different thing, and all of it
//! was written, correct, and reached by nothing: `Mesh::honest`,
//! `what_it_adds`, `works_without`, `setup_steps`, `YOU_APPROVE_THE_DEVICE`
//! and `WHAT_ID_DO` were test-only. `mesh.kind` was the setting underneath —
//! a string nothing parsed, so `kind: tailscale` and `kind: banana` were the
//! same setting, which is to say none.
//!
//! So the module splits the way `messaging` did an hour earlier, on the same
//! question: **does an honest answer need the missing thing?** `kind` does
//! not and is settable and read. `prefer_direct` does — its only reader is
//! `choose` — so it is `#[serde(skip)]` and named in
//! `PROMISES_ABOUT_WHAT_IS_NOT_BUILT` with the decision it records.

use atlas::mesh::{
    self, what_a_private_network_would_give_you as advice, Mesh, MeshConfig, Path,
};

fn cfg(kind: &str, enabled: bool) -> MeshConfig {
    MeshConfig { enabled, kind: kind.into(), ..Default::default() }
}

// ================= the setting that was a string nobody parsed ==========

#[test]
fn the_kind_you_wrote_is_the_one_you_get() {
    assert_eq!(Mesh::from_setting("tailscale"), Some(Mesh::Tailscale));
    assert_eq!(Mesh::from_setting("  Tailscale "), Some(Mesh::Tailscale));
    assert_eq!(Mesh::from_setting("head-scale"), Some(Mesh::Headscale));
    assert_eq!(Mesh::from_setting("wg"), Some(Mesh::Wireguard));
    assert_eq!(Mesh::from_setting("none"), Some(Mesh::None));
    assert_eq!(Mesh::from_setting(""), Some(Mesh::None));

    // "I didn't understand what you wrote" and "you have no private network"
    // are different answers, and only one of them is actionable. Falling
    // through to `None` would have made a typo indistinguishable from a
    // decision.
    assert_eq!(Mesh::from_setting("banana"), None);
    assert_eq!(Mesh::from_setting("tailscle"), None);
}

#[test]
fn a_kind_it_does_not_know_is_said_rather_than_treated_as_none() {
    let said = advice(&cfg("banana", false)).join(" ");
    assert!(said.contains("banana"), "the typo vanished: {said}");
    assert!(said.contains("I know Tailscale"), "{said}");
    // And it does not then recommend anything, because the thing to fix is
    // the line you wrote.
    assert!(!said.contains("Set up Tailscale. It's free for you"), "{said}");
}

// ================= it never pretends the transport exists ===============

#[test]
fn every_answer_opens_by_saying_this_is_not_wired() {
    // A page of enthusiastic setup steps with that fact left off is how
    // somebody spends ten minutes installing Tailscale and then finds
    // nothing uses it.
    for k in ["none", "tailscale", "headscale", "wireguard", "banana"] {
        for on in [true, false] {
            let lines = advice(&cfg(k, on));
            assert_eq!(lines[0], mesh::NOT_BUILT_HERE, "{k}/{on} buried it");
        }
    }
}

#[test]
fn the_route_it_would_take_is_still_not_a_route_it_can_take() {
    // `choose` is the transport and it stays unwired. Kept as an assertion
    // so that "mesh has a command now" never turns into "mesh is wired".
    let c = cfg("tailscale", true);
    assert_eq!(mesh::choose(false, true, true, false, &c).0, Path::Mesh);

    // `mesh` came off CAPABILITY_UNWIRED later the same day, honestly: once
    // `nearby::look` existed, `choose` got a real observation instead of the
    // hardcoded literal every caller used to pass, so its code genuinely is
    // reached. What did not change is that nothing *sends* over it.
    //
    // This assertion used to read the unwired list and had to move, which is
    // the useful part: a test pinned to *where* a fact is recorded breaks
    // when the fact moves, even though the fact is unchanged. It reads the
    // reason now, which is the thing that actually matters.
    let honesty = std::fs::read_to_string("tests/capability_honesty.rs").expect("capability_honesty");
    let named = honesty
        .split("PLANNED_FOR_A_REASON_OF_ITS_OWN: &[(&str, &str)] = &[")
        .nth(1)
        .and_then(|r| r.split("];").next())
        .expect("the list of Planned entries with reasons of their own");
    assert!(
        named.contains("\"mesh\""),
        "nothing records why mesh is still Planned -- if a direct transport was built, \
         say so by changing the catalogue entry instead"
    );
    let src = crate::common::source_of("main");
    assert!(!src.contains("mesh::choose("), "the command started picking routes");
}

// ================= what it says when you have chosen one ================

#[test]
fn choosing_one_gets_you_what_it_costs_and_what_you_do_yourself() {
    let said = advice(&cfg("tailscale", true)).join(" ");
    assert!(said.contains("You've chosen Tailscale"), "{said}");
    assert!(said.contains("BotFather") == false, "wrong module's advice: {said}");
    assert!(said.contains("I can:"), "it didn't say what Atlas does: {said}");
    assert!(said.contains("You do: approve this machine"), "{said}");
    assert!(said.contains("a thing you did rather than a thing that happened"), "{said}");

    // The one line that is a decision rather than a description.
    assert!(said.contains("a company you don't run is in the path"), "{said}");
    // And it is not a restatement of what `honest` already said.
    assert_eq!(said.matches("can't read what passes between them").count(), 1, "{said}");
}

#[test]
fn the_one_that_needs_a_server_says_so_before_you_start() {
    let said = advice(&cfg("headscale", true)).join(" ");
    assert!(said.contains("needs a machine that's always on"), "{said}");
    assert!(said.contains("most expensive part of Atlas"), "{said}");
    // Headscale is not a third party in the path, and claiming it is would be
    // the same laziness in the other direction.
    assert!(!said.contains("a company you don't run is in the path"), "{said}");
}

#[test]
fn choosing_one_and_switching_it_on_are_two_decisions() {
    // Saying nothing about the gap between them is how a setting you did set
    // looks like one that does not work.
    let off = advice(&cfg("tailscale", false)).join(" ");
    assert!(off.contains("`mesh.enabled` is off"), "{off}");
    let on = advice(&cfg("tailscale", true)).join(" ");
    assert!(!on.contains("`mesh.enabled` is off"), "{on}");
}

#[test]
fn having_chosen_nothing_gets_the_recommendation() {
    let said = advice(&cfg("none", false)).join(" ");
    assert!(said.contains("You haven't chosen one"), "{said}");
    assert!(said.contains("Set up Tailscale"), "{said}");
}

#[test]
fn what_still_works_is_said_last_and_never_left_out() {
    // This module's own header: a private network is an addition, not a
    // replacement. A list of what you are missing without one, with no list
    // of what still works, reads as a list of what is broken.
    for k in ["none", "tailscale", "headscale", "wireguard", "banana"] {
        let lines = advice(&cfg(k, true));
        let last = lines.last().expect("lines");
        assert!(last.starts_with("What works without it:"), "{k}: {last}");
        assert!(last.contains("syncing through the cloud folder"), "{k}: {last}");
    }
}

// ================= reachable from the program ===========================

#[test]
fn the_command_is_what_reaches_it_rather_than_this_test() {
    let raw = crate::common::source_of("main");
    assert!(raw.contains("fn run_mesh("), "there is no way to ask");
    assert!(
        raw.contains("atlas::mesh::what_a_private_network_would_give_you(&mcfg)"),
        "nothing reads the mesh settings"
    );
    assert!(raw.contains("Some(\"mesh\")"), "the command is not reachable from the line");

    let yaml = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(yaml.contains("kind: none                  # tailscale"), "nowhere to set the kind");
    // `prefer_direct` went the other way: its only reader is the transport
    // that does not exist.
    assert!(!yaml.contains("prefer_direct:"), "prefer_direct is settable again");
    assert!(MeshConfig::default().prefer_direct, "the recorded decision was lost");
}
