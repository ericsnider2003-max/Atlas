//! `roster.rs` -- the gate `kin.rs` deliberately doesn't answer on its own:
//! being paired proves who you are, not what you may see.

use atlas::kin::{Pairings, Peer};
use atlas::roster::{Roster, RosterError};
use atlas::store::Store;

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-roster-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn paired_with(name: &str) -> Pairings {
    let mut p = Pairings::default();
    p.peers.push(Peer::new(name, "some-token"));
    p
}

#[test]
fn an_unlisted_name_sees_nothing_by_default() {
    let roster = Roster::default();
    let pairings = paired_with("Sarah");
    assert!(!roster.may_see("Acme", "Sarah", &pairings), "default deny -- nobody starts with access");
}

#[test]
fn a_stranger_kin_has_never_heard_of_cannot_be_added() {
    let mut roster = Roster::default();
    let pairings = Pairings::default(); // nobody paired at all
    let err = roster.add("Acme", "Sarah", &pairings).unwrap_err();
    assert_eq!(err, RosterError::NotAPeer, "a roster entry for someone unpaired would enforce nothing");
}

#[test]
fn adding_a_real_peer_grants_access_to_exactly_that_business() {
    let mut roster = Roster::default();
    let pairings = paired_with("Sarah");
    roster.add("Acme", "Sarah", &pairings).unwrap();

    assert!(roster.may_see("Acme", "Sarah", &pairings));
    // The whole point: being paired, and being on Acme's roster, says
    // nothing about a *different* business.
    assert!(!roster.may_see("Widgets Inc", "Sarah", &pairings));
}

#[test]
fn adding_the_same_person_twice_is_refused_not_duplicated() {
    let mut roster = Roster::default();
    let pairings = paired_with("Sarah");
    roster.add("Acme", "Sarah", &pairings).unwrap();
    let err = roster.add("Acme", "Sarah", &pairings).unwrap_err();
    assert_eq!(err, RosterError::AlreadyAMember);
}

#[test]
fn adding_is_case_insensitive_against_the_same_pairing() {
    let mut roster = Roster::default();
    let pairings = paired_with("Sarah");
    roster.add("Acme", "sarah", &pairings).unwrap();
    assert!(roster.may_see("Acme", "SARAH", &pairings));
}

#[test]
fn removing_someone_revokes_exactly_that_business_and_nothing_else() {
    let mut roster = Roster::default();
    let pairings = paired_with("Sarah");
    roster.add("Acme", "Sarah", &pairings).unwrap();
    roster.add("Widgets Inc", "Sarah", &pairings).unwrap();

    assert!(roster.remove("Acme", "Sarah"));
    assert!(!roster.may_see("Acme", "Sarah", &pairings));
    assert!(roster.may_see("Widgets Inc", "Sarah", &pairings), "the other business is untouched");
}

/// The core security property Eric asked for: forgetting the underlying
/// pairing revokes business access in the same motion, with no separate
/// cleanup step -- a roster entry left behind on disk enforces nothing
/// once the pairing it depends on is gone.
#[test]
fn forgetting_the_pairing_revokes_every_business_it_granted_access_to() {
    let mut roster = Roster::default();
    let mut pairings = paired_with("Sarah");
    roster.add("Acme", "Sarah", &pairings).unwrap();
    assert!(roster.may_see("Acme", "Sarah", &pairings));

    pairings.forget("Sarah");

    assert!(!roster.may_see("Acme", "Sarah", &pairings), "no pairing, no access, regardless of the list on disk");
    assert!(
        roster.standing("Sarah", &pairings).is_empty(),
        "standing() must agree with may_see -- the pairing is gone"
    );
}

#[test]
fn standing_lists_every_business_a_peer_currently_has_access_to() {
    let mut roster = Roster::default();
    let pairings = paired_with("Sarah");
    roster.add("Acme", "Sarah", &pairings).unwrap();
    roster.add("Widgets Inc", "Sarah", &pairings).unwrap();

    let mut businesses = roster.standing("Sarah", &pairings);
    businesses.sort();
    assert_eq!(businesses, vec!["Acme".to_string(), "Widgets Inc".to_string()]);
}

#[test]
fn two_different_peers_on_the_same_business_do_not_see_each_other_s_other_businesses() {
    let mut roster = Roster::default();
    let mut pairings = Pairings::default();
    pairings.peers.push(Peer::new("Sarah", "tok-a"));
    pairings.peers.push(Peer::new("Tom", "tok-b"));

    roster.add("Acme", "Sarah", &pairings).unwrap();
    roster.add("Acme", "Tom", &pairings).unwrap();
    roster.add("Widgets Inc", "Tom", &pairings).unwrap();

    assert!(roster.may_see("Acme", "Sarah", &pairings));
    assert!(roster.may_see("Acme", "Tom", &pairings));
    assert!(!roster.may_see("Widgets Inc", "Sarah", &pairings));
    assert!(roster.may_see("Widgets Inc", "Tom", &pairings));
}

#[test]
fn a_roster_survives_save_and_load() {
    let dir = tmp("roundtrip");
    let store = Store::new(&dir);
    let pairings = paired_with("Sarah");
    let mut roster = Roster::default();
    roster.add("Acme", "Sarah", &pairings).unwrap();
    roster.save(&store).unwrap();

    let loaded = Roster::load(&store);
    assert!(loaded.may_see("Acme", "Sarah", &pairings));
}

#[test]
fn the_roster_lists_the_businesses_it_knows() {
    // `calendar::space_for_request` matches a scheduling request against this
    // list, so the roster has to be able to say which businesses exist.
    let mut roster = Roster::default();
    let pairings = paired_with("Sam");
    roster.add("Northwind", "Sam", &pairings).unwrap();
    roster.add("Riverstone", "Sam", &pairings).unwrap();

    let mut names = roster.businesses();
    names.sort();
    assert_eq!(names, vec!["Northwind".to_string(), "Riverstone".to_string()]);

    // Empty by default: no businesses, so scheduling stays personal.
    assert!(Roster::default().businesses().is_empty());
}
