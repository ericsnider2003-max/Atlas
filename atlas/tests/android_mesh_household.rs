use atlas::android::{
    abilities as android_abilities, serious_permissions, wake_word_battery_percent_per_hour,
    AndroidConfig, THE_TRADE, VERSUS_IOS,
};
use atlas::household::{
    accept_bundle, meets, new_pairing, saw_another, share_with_friend, HouseholdConfig, Meeting,
    SEPARATE_BY_DEFAULT, THEIRS_IS_THEIRS,
};
use atlas::ios::Can;
use atlas::mesh::{choose, what_it_adds, works_without, Mesh, MeshConfig, Path, WHAT_ID_DO};

// ================= Android does what iOS won't =================

#[test]
fn the_wake_word_works_with_the_app_closed() {
    // The single thing iOS refuses.
    let wake = android_abilities()
        .into_iter()
        .find(|a| a.what.contains("wake on a word"))
        .unwrap();
    assert_eq!(wake.can, Can::Yes);
    assert!(wake.detail.contains("works exactly like the laptop"));
}

#[test]
fn atlas_can_be_the_assistant_on_the_home_button() {
    let a = android_abilities()
        .into_iter()
        .find(|a| a.what.contains("assistant on the home button"))
        .unwrap();
    assert_eq!(a.can, Can::Yes);
    assert!(a.detail.contains("replace Google Assistant outright"));
}

#[test]
fn it_can_read_the_screen_and_act_in_apps_which_ios_has_no_equivalent_of() {
    let read = android_abilities().into_iter().find(|a| a.what.contains("read what's on screen")).unwrap();
    assert!(read.needs_deliberate_permission);
    let act = android_abilities().into_iter().find(|a| a.what.contains("tap and type")).unwrap();
    assert!(act.detail.contains("iOS has no equivalent of at all"));
}

#[test]
fn the_powerful_permissions_are_flagged_as_powerful() {
    let perms = serious_permissions();
    let acc = perms.iter().find(|(n, _, _)| n.contains("accessibility")).unwrap();
    assert!(acc.2.contains("most powerful permission on the phone"));
    assert!(acc.2.contains("leave it off and everything else still works"));
}

#[test]
fn the_battery_cost_of_listening_is_stated_rather_than_discovered() {
    assert!(wake_word_battery_percent_per_hour() > 0.0);
    let perms = serious_permissions();
    assert!(perms.iter().any(|(_, _, why)| why.contains("costs battery and Atlas should say how much")));
}

#[test]
fn everything_powerful_is_off_until_chosen() {
    let c = AndroidConfig::default();
    assert!(!c.always_listening);
    assert!(!c.can_act_in_apps);
    assert!(!c.read_notifications);
    assert!(!c.replace_assistant);
}

#[test]
fn credentials_stay_off_android_too_and_that_is_not_configurable() {
    let parsed: AndroidConfig =
        serde_yaml::from_str("enabled: true\nholds_credentials: true\n").unwrap();
    assert!(!parsed.holds_credentials);
}

#[test]
fn a_friend_with_an_android_gets_the_better_version_and_is_told_so() {
    assert!(VERSUS_IOS.contains("iOS allows none of those"));
    assert!(VERSUS_IOS.contains("closer-to-the-laptop version"));
    assert!(THE_TRADE.contains("worth leaving off"));
}

// ================= reaching the laptop on cell =================

#[test]
fn tailscale_is_free_and_needs_no_server() {
    assert!(Mesh::Tailscale.free());
    assert!(!Mesh::Tailscale.needs_a_server());
    assert!(Mesh::Tailscale.honest().contains("free for one person"));
}

#[test]
fn headscale_is_honestly_described_as_needing_a_machine_you_do_not_have() {
    assert!(Mesh::Headscale.needs_a_server());
    assert!(Mesh::Headscale.honest().contains("you don't"));
    assert!(Mesh::Headscale.honest().contains("most expensive part of Atlas"));
}

#[test]
fn whether_anyone_else_is_in_the_path_is_stated_precisely() {
    // Neither alarmed nor silent.
    assert!(Mesh::Tailscale.third_party_in_the_path());
    assert!(!Mesh::Headscale.third_party_in_the_path());
    assert!(Mesh::Tailscale.honest().contains("can't read what passes between them"));
}

#[test]
fn what_it_adds_and_what_still_works_without_it_are_both_listed() {
    assert!(what_it_adds().iter().any(|s| s.contains("directly")));
    assert!(works_without().iter().any(|s| s.contains("cloud folder")));
    assert!(works_without().iter().any(|s| s.contains("everything on the phone itself")));
}

#[test]
fn the_same_network_still_wins_when_it_is_there() {
    let cfg = MeshConfig { enabled: true, ..Default::default() };
    assert_eq!(choose(true, true, true, true, &cfg).0, Path::SameNetwork);
}

#[test]
fn on_cell_the_private_network_is_used_before_the_cloud() {
    let cfg = MeshConfig { enabled: true, ..Default::default() };
    let (p, why) = choose(false, true, true, false, &cfg);
    assert_eq!(p, Path::Mesh);
    assert!(why.contains("even on cell"));
}

#[test]
fn without_a_mesh_it_falls_back_rather_than_looking_broken() {
    let cfg = MeshConfig::default();
    assert_eq!(choose(false, false, true, false, &cfg).0, Path::Cloud);
    assert!(cfg.fall_back);
}

#[test]
fn with_nothing_reachable_it_holds_rather_than_failing() {
    let (p, why) = choose(false, false, false, false, &MeshConfig::default());
    assert_eq!(p, Path::Nothing);
    assert!(why.contains("hold it until something is"));
}

#[test]
fn the_recommendation_is_tailscale_with_the_reason() {
    assert!(WHAT_ID_DO.contains("free for you"));
    assert!(WHAT_ID_DO.contains("cloud folder from your main path into your fallback"));
}

// ================= your friends are not linked to you =================

#[test]
fn another_households_atlas_gets_nothing_and_no_prompt() {
    // A friend on your sofa shouldn't get a popup.
    let m = meets("mine-abc", "theirs-xyz", false);
    assert_eq!(m, Meeting::NotMine);
    assert_eq!(saw_another(&m), "", "silence — even saying you saw it leaks something");
}

#[test]
fn your_own_unpaired_device_is_offered_pairing() {
    let m = meets("mine-abc", "mine-abc", false);
    assert!(matches!(m, Meeting::NeedsPairing { .. }));
    assert!(saw_another(&m).contains("Want to?"));
}

#[test]
fn pairing_needs_both_devices_in_hand_and_expires_quickly() {
    // A code left on a screen should be worthless by the time anyone finds it.
    let p = new_pairing("4417", 0);
    assert!(p.still_good(60));
    assert!(!p.still_good(400));
    assert!(p.valid_secs <= 300);
}

#[test]
fn a_bundle_from_someone_else_is_dropped_without_being_read() {
    // Someone AirDropping you their export by mistake should be a non-event.
    let e = accept_bundle("mine-abc", "theirs-xyz").unwrap_err();
    assert!(e.contains("I've not opened it"));
    assert!(e.contains("nothing for you to do"));
    assert!(accept_bundle("mine-abc", "mine-abc").is_ok());
}

#[test]
fn joining_a_household_automatically_is_not_configurable() {
    // Two `#[serde(skip)]` bools pinned false, which this asserted were
    // false. Deleted 19 Sep 2026 -- they restated invariants the code
    // enforces, and a bool nobody reads is not a boundary.
    //
    // Joining: being near another Atlas never means belonging to it, and
    // `meets` yields `NeedsPairing` rather than `Mine` for an unpaired
    // device, so there is no path from proximity to membership.
    use atlas::household::{meets, Meeting};
    assert!(matches!(
        meets("mine-abc", "mine-abc", false),
        Meeting::NeedsPairing { .. }
    ));
    assert!(matches!(meets("mine-abc", "mine-abc", true), Meeting::Mine));

    // Sharing across households: refused where bundles are taken in, not by
    // a flag beside it.
    assert!(matches!(meets("mine-abc", "theirs-xyz", true), Meeting::NotMine));
    assert!(accept_bundle("mine-abc", "theirs-xyz").is_err());

    // And an old config naming any of the removed keys still loads.
    // `discoverable` joined `auto_join` and `share_across_households` on
    // 19 Sep 2026: nothing here announces itself on a local network -- `mesh`
    // is unwired and says so, and `server.reachable_from` is the opposite
    // approach, where you give the address yourself -- so a switch for it
    // read as a behaviour you had turned off.
    let parsed: HouseholdConfig = serde_yaml::from_str(
        "discoverable: true\nauto_join: true\nshare_across_households: true\ndevice_name: \"the laptop\"\n",
    )
    .expect("an unknown key must not stop the section parsing");
    assert_eq!(parsed.device_name, "the laptop", "the field that does work was taken down with them");
}

#[test]
fn sending_a_friend_something_carries_the_thing_and_nothing_else() {
    let h = share_with_friend("the fee comparison", "Eric");
    assert!(!h.carries_history);
    assert_eq!(h.from, "Eric");
}

#[test]
fn the_separation_rule_is_written_down_for_anyone_reading_the_source() {
    assert!(SEPARATE_BY_DEFAULT.contains("Being on the same wifi doesn't do it"));
    assert!(SEPARATE_BY_DEFAULT.contains("no setting that changes that"));
    assert!(THEIRS_IS_THEIRS.contains("makes its own household on first run"));
}

#[test]
fn atlas_can_set_tailscale_up_including_signing_you_in() {
    // Saying it couldn't was inconsistent — signing into a site is what the
    // sign-in capability is for, and Tailscale is a site.
    use atlas::mesh::setup_steps;
    let steps = setup_steps(Mesh::Tailscale);
    let signin = steps.iter().find(|(s, _)| s.contains("sign-in page")).unwrap();
    assert!(signin.1, "opening the sign-in page is Atlas's job");
    let fill = steps.iter().find(|(s, _)| s.contains("fill your login")).unwrap();
    assert!(fill.1, "and so is filling it from the vault");
}

#[test]
fn approving_a_new_device_onto_the_network_stays_yours() {
    // The one step that decides what can reach your laptop.
    use atlas::mesh::{setup_steps, YOU_APPROVE_THE_DEVICE};
    let steps = setup_steps(Mesh::Tailscale);
    let approve = steps.iter().find(|(s, _)| s.contains("approve this machine")).unwrap();
    assert!(!approve.1);
    assert!(YOU_APPROVE_THE_DEVICE.contains("a thing you did rather than a thing that happened"));
}

#[test]
fn headscale_setup_is_honest_that_there_is_nothing_to_set_up() {
    use atlas::mesh::setup_steps;
    assert!(setup_steps(Mesh::Headscale)[0].0.contains("needs a server you don't have"));
}
