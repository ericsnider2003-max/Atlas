//! More than one person on one machine.
//!
//! This module had behaviour and no test that called it, which the
//! retrospective audit found. The risk here isn't technical — it's one
//! person's assistant quietly knowing another person's business — so it wants
//! testing more than most things, not less.

use atlas::profiles::{slug, Profiles, Role};

#[test]
fn a_guest_cannot_do_the_things_that_act_as_you() {
    // The whole point of the role. A guest with an assistant is fine; a guest
    // able to post as you is not.
    //
    // This test used to read:
    //
    //     for forbidden in ["publish", "send_email", "promote_changes", ...]
    //
    // and it passed, and nothing was protected. `Role::may` is only ever
    // called with `session::kind_of(intent)`, whose set of names is closed
    // and contains none of those four. So the guard matched nothing, the test
    // proved it matched nothing, and the next test below asserted that a
    // guest *may* `draft_post` -- which is the actual name of posting as you.
    //
    // A test can do worse than fail to catch a bug: it can certify it. This
    // one did, for as long as the role has existed.
    for forbidden in ["draft_post", "review_post", "mail", "work_on_yourself", "back_up", "undo"] {
        assert!(!Role::Guest.may(forbidden), "a guest could {forbidden}");
        assert!(Role::Owner.may(forbidden), "the owner is blocked from {forbidden}");
    }
}

#[test]
fn every_guest_restriction_is_a_name_that_can_actually_arrive() {
    // The assertion that would have caught the above on day one. A
    // restriction naming something `kind_of` never produces is not a weak
    // protection, it is no protection wearing the word.
    let session = std::fs::read_to_string("src/session.rs").expect("src/session.rs");
    for action in atlas::profiles::NEVER_AS_A_GUEST {
        assert!(
            session.contains(&format!("\"{action}\"")),
            "a guest is 'blocked' from {action:?}, which `session::kind_of` never \
             produces -- so nothing is blocked"
        );
    }
}

#[test]
fn a_guest_cannot_reach_your_accounts_or_hand_atlas_on() {
    // Not in the original list at all, and arguably worse than posting: the
    // vault, signing in as you, and pairing your Atlas to someone else's.
    for forbidden in ["unlock", "sign_in", "create_account", "pair", "accept_pairing", "forget_peer"] {
        assert!(!Role::Guest.may(forbidden), "a guest could {forbidden}");
    }
}

#[test]
fn a_guest_can_still_use_it_for_themselves() {
    // The other half, and the reason the list is named rather than "deny
    // everything": a guest account nobody can use is not a guest account.
    for allowed in ["research", "outstanding", "capture", "say", "open_app", "whats_there"] {
        assert!(Role::Guest.may(allowed), "a guest couldn't {allowed}");
    }
}

#[test]
fn each_profile_gets_its_own_state_directory_rather_than_a_flag() {
    // A setting can be got round. A separate directory cannot.
    let root = std::path::Path::new("/tmp/atlas-test");
    let mine = Profiles::state_dir(root, "eric");
    let theirs = Profiles::state_dir(root, "sam");
    assert_ne!(mine, theirs);
    assert!(mine.to_string_lossy().contains("profiles"));
}

#[test]
fn a_name_becomes_something_safe_to_put_in_a_path() {
    assert_eq!(slug("Eric"), "eric");
    assert_eq!(slug("Sam R."), "sam-r");
    // The one that matters: a name can't climb out of the profiles folder.
    assert!(!slug("../../etc").contains(".."));
    assert!(!slug("a/b").contains('/'));
}

#[test]
fn a_profile_with_no_usable_name_is_refused() {
    let mut p = Profiles::default();
    assert!(p.add("", Role::Owner).is_err());
    assert!(p.add("///", Role::Owner).is_err(), "nothing left after slugging");
}

#[test]
fn two_profiles_cannot_share_a_name() {
    let mut p = Profiles::default();
    p.add("Eric", Role::Owner).unwrap();
    let e = p.add("eric", Role::Guest).unwrap_err();
    assert!(format!("{e}").contains("already a profile"));
}

#[test]
fn the_first_profile_made_becomes_the_active_one() {
    let mut p = Profiles::default();
    assert!(p.current().is_none());
    p.add("Eric", Role::Owner).unwrap();
    assert_eq!(p.current().unwrap().name, "Eric");
}

#[test]
fn adding_a_second_profile_does_not_switch_to_it() {
    // Handing your laptop to someone shouldn't move you into their world.
    let mut p = Profiles::default();
    p.add("Eric", Role::Owner).unwrap();
    p.add("Sam", Role::Guest).unwrap();
    assert_eq!(p.current().unwrap().name, "Eric");
}

#[test]
fn a_profile_can_be_found_by_its_id() {
    let mut p = Profiles::default();
    let made = p.add("Sam R.", Role::Guest).unwrap();
    assert_eq!(p.get(&made.id).unwrap().role, Role::Guest);
    assert!(p.get("nobody").is_none());
}

#[test]
fn one_person_on_one_machine_never_gets_a_per_profile_directory() {
    // The rule that keeps the usual install out of the profile machinery
    // entirely, asserted directly rather than through `roots::store()`.
    //
    // It matters in both directions. If a single-profile install returned a
    // per-person directory, everything Atlas already remembered would appear
    // to vanish the day a profile got a name -- the state is still on disk,
    // one level up, and nothing would say so. And if a two-profile install
    // returned `None`, the guest would be reading and writing the owner's
    // memory, which is the entire point of having profiles.
    let root = std::env::temp_dir().join("atlas-profiles-active-dir");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();

    let mut p = Profiles::default();
    assert_eq!(p.active_state_dir(&root), None, "an empty registry named a directory");

    p.add("Eric", Role::Owner).unwrap();
    assert_eq!(
        p.active_state_dir(&root),
        None,
        "one profile is still just you -- the install's own state is where it lives"
    );

    let guest = p.add("Sam", Role::Guest).unwrap();
    p.switch(&guest.id, 100).unwrap();
    let dir = p.active_state_dir(&root).expect("two profiles and no directory for either");
    assert!(dir.starts_with(&root), "the profile directory escaped the install: {dir:?}");
    assert!(
        dir.to_string_lossy().contains(&guest.id),
        "the active profile's directory is not the active profile's: {dir:?}"
    );
}
