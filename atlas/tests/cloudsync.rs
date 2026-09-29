use atlas::cloudsync::{
    compare_providers, free_tier_is_enough, laptop_steps, phone_steps, result, setting_up,
    setup_guidance, space_needed_mb, where_to_look, CloudConfig, Provider, Setup, Trouble,
    WHAT_YOU_DO, WHY_ONEDRIVE,
};

// ================= what it costs =================

#[test]
fn the_bundles_are_text_so_this_costs_nothing() {
    // Worth knowing before worrying about space.
    let needed = space_needed_mb(200, 365, false);
    let (enough, why) = free_tier_is_enough(Provider::OneDrive, needed);
    assert!(enough);
    assert!(why.contains("costs you nothing, and won't"));
    // The sentence used to end at "won't" and the assertion above was happy
    // with that, because nothing ever printed it. Pinned whole now.
    assert!(
        why.ends_with("grow into something that does"),
        "the reassurance stops mid-sentence: {why:?}"
    );
}

#[test]
fn the_onedrive_aside_is_only_said_about_onedrive() {
    // It used to be said whatever provider you picked, so setting up Dropbox
    // told you about OneDrive. Invisible for as long as the only caller of
    // `laptop_steps` in tests passed `already_installed: true`, which skips
    // the step this text lives on.
    let dropbox = laptop_steps(Provider::Dropbox, false);
    let install = dropbox
        .iter()
        .find(|s| s.what.contains("install"))
        .expect("a not-yet-installed provider has an install step");
    let why = install.why_you.clone().unwrap_or_default();
    assert!(!why.contains("OneDrive"), "Dropbox setup mentions OneDrive: {why:?}");
    assert!(why.contains("don't sign into anything"));

    let onedrive = laptop_steps(Provider::OneDrive, false);
    let install = onedrive
        .iter()
        .find(|s| s.what.contains("install"))
        .expect("install step");
    assert!(install.why_you.clone().unwrap_or_default().contains("already there"));
}

#[test]
fn carrying_files_through_it_too_is_still_inside_the_free_tier() {
    let needed = space_needed_mb(200, 365, true);
    assert!(free_tier_is_enough(Provider::OneDrive, needed).0);
}

#[test]
fn every_provider_states_its_free_allowance() {
    assert_eq!(Provider::OneDrive.free_gb(), 5);
    assert_eq!(Provider::GoogleDrive.free_gb(), 15);
    assert_eq!(Provider::Dropbox.free_gb(), 2);
}

#[test]
fn onedrive_is_recommended_for_a_windows_laptop_with_the_reason() {
    assert!(WHY_ONEDRIVE.contains("already in Windows"));
    assert!(WHY_ONEDRIVE.contains("already signed in"));
    assert!(WHY_ONEDRIVE.contains("switching later costs nothing"));
    // It used to finish "because I encrypt before anything is written either
    // way", which was false: `carry_to_your_other_devices` writes the bundle
    // with `serde_json::to_string_pretty` and nothing encrypts it. A page
    // that claims a protection the code does not provide is worse than one
    // that claims nothing -- it decides what the reader does with the folder.
    assert!(
        !WHY_ONEDRIVE.to_lowercase().contains("encrypt"),
        "this page is claiming encryption again while bundles are written in the clear"
    );
}

#[test]
fn how_each_behaves_on_windows_is_stated_honestly() {
    assert!(Provider::OneDrive.on_windows().contains("built in"));
    assert!(Provider::ICloud.on_windows().contains("weakest of the four"));
    assert!(Provider::Dropbox.on_windows().contains("quickest to actually push"));
}

// ================= what Atlas does and what you do =================

#[test]
fn everything_on_the_laptop_side_is_atlas_including_signing_in() {
    // This used to assert that signing in was yours, which contradicted the
    // sign-in capability that already existed. A cloud provider is a site.
    for s in laptop_steps(Provider::OneDrive, true) {
        assert!(s.automatic, "{} should be automatic", s.what);
    }
}

#[test]
fn with_onedrive_already_there_atlas_does_the_whole_thing() {
    let steps = laptop_steps(Provider::OneDrive, true);
    assert!(steps.iter().all(|s| s.automatic));
    assert!(steps.iter().any(|s| s.what.contains("find the OneDrive folder")));
}

#[test]
fn it_tests_that_the_folder_really_syncs_rather_than_assuming() {
    let steps = laptop_steps(Provider::OneDrive, true);
    assert!(steps.iter().any(|s| s.what.contains("watch it sync")));
}

#[test]
fn it_marks_the_folder_always_keep_which_is_the_failure_people_actually_hit() {
    // The file is "there" and useless offline, which is exactly when you need
    // it.
    let steps = laptop_steps(Provider::OneDrive, true);
    assert!(steps.iter().any(|s| s.what.contains("always-keep rather than online-only")));
    assert!(Trouble::OnlineOnly.looks_like().contains("placeholder"));
    assert!(Trouble::OnlineOnly.atlas_can_fix());
}

#[test]
fn the_phone_side_is_honest_that_ios_limits_what_atlas_can_do() {
    let steps = phone_steps(Provider::OneDrive);
    let files = steps.iter().find(|s| s.what.contains("Files")).unwrap();
    assert!(!files.automatic);
    assert!(files.why_you.as_ref().unwrap().contains("iOS only lets the app itself do that"));
}

#[test]
fn atlas_finds_the_folder_by_what_the_client_set_not_by_guessing_a_name() {
    // The environment variable is right even when the folder has been moved.
    let looks = where_to_look(Provider::OneDrive);
    assert_eq!(looks[0], "%OneDrive%");
    assert!(looks.contains(&"%OneDriveCommercial%"), "work accounts too");
}

#[test]
fn a_missing_provider_says_atlas_will_pick_it_up_by_itself_later() {
    let said = setting_up(Provider::OneDrive, None);
    assert!(said.contains("I'll pick it up by myself"));
    assert!(said.contains("nothing for you to point me at"));
}

#[test]
fn finding_it_says_where_and_gets_on_with_it() {
    let said = setting_up(Provider::OneDrive, Some("C:/Users/erics/OneDrive"));
    assert!(said.contains("C:/Users/erics/OneDrive"));
    assert!(said.contains("Making a folder and testing it"));
    // Contrast: not found must read as a different message, not a variant of
    // the same sentence with a blank where the path goes.
    let not_found = setting_up(Provider::OneDrive, None);
    assert_ne!(not_found, said);
    assert!(!not_found.contains("Making a folder and testing it"));
}

#[test]
fn a_working_test_explains_what_the_path_is_for() {
    let said = result(true, 4, Provider::OneDrive);
    assert!(said.contains("came back in 4 seconds"));
    assert!(said.contains("when they're not on the same wifi"));
}

#[test]
fn a_failed_test_says_the_thing_to_check() {
    assert!(result(false, 0, Provider::OneDrive).contains("tray icon"));
}

// ================= when it goes wrong =================

#[test]
fn atlas_fixes_what_it_can_and_hands_back_what_it_cannot() {
    assert!(Trouble::OnlineOnly.atlas_can_fix());
    assert!(Trouble::Moved.atlas_can_fix());
    assert!(!Trouble::SignedOut.atlas_can_fix());
    assert!(Trouble::SignedOut.fix().contains("that one's yours"));
}

#[test]
fn each_problem_says_what_it_looks_like_not_just_what_it_is() {
    for t in [
        Trouble::NotActuallySyncing,
        Trouble::OnlineOnly,
        Trouble::OutOfSpace,
        Trouble::SignedOut,
        Trouble::Moved,
    ] {
        assert!(!t.looks_like().is_empty(), "{t:?}");
        assert!(!t.fix().is_empty(), "{t:?}");
    }
}

#[test]
fn it_keeps_checking_rather_than_assuming_it_still_works() {
    assert!(CloudConfig::default().check_every_hours <= 24);
}

#[test]
fn an_old_config_naming_the_removed_switch_still_loads() {
    // `encrypt_before_writing` was a `#[serde(skip)]` field pinned true that
    // nothing read, while bundles were written in the clear -- a guarantee
    // stated in a struct and kept nowhere. The switch that does the job is
    // `sync.encrypt_bundles`. Anyone who wrote the old key into their file
    // must still be able to start Atlas.
    let parsed: CloudConfig =
        serde_yaml::from_str("enabled: true\nencrypt_before_writing: false\n").unwrap();
    assert!(parsed.enabled, "an unknown key must not stop the rest of the section parsing");
}

#[test]
fn the_split_of_responsibility_is_stated_plainly() {
    assert!(WHAT_YOU_DO.contains("sign you in from the vault"));
    assert!(WHAT_YOU_DO.contains("the first time you set the vault passphrase"));
    assert!(WHAT_YOU_DO.contains("works when you're offline"));
}

// ============ the provider-compare / setup surface ============
//
// These three prove the readouts actually call the per-provider and
// per-trouble readings, rather than just existing: `on_windows`, `on_ios`,
// `windows_hint` (Provider) and `looks_like`, `atlas_can_fix` (Trouble).

#[test]
fn comparing_providers_speaks_windows_the_phone_and_the_windows_path() {
    let s = compare_providers();
    // on_windows(), for two providers so a single shared string can't pass it
    assert!(s.contains("built in"), "OneDrive's Windows note is missing:\n{s}");
    assert!(s.contains("weakest of the four"), "iCloud's Windows note is missing:\n{s}");
    // on_ios()
    assert!(s.contains("shows up in Files"), "OneDrive's phone note is missing:\n{s}");
    // windows_hint()
    assert!(s.contains("%OneDrive%"), "the Windows path hint is missing:\n{s}");
    // free_gb(), so the comparison is quantitative too
    assert!(s.contains("15GB free"), "Google Drive's free tier is missing:\n{s}");
}

#[test]
fn setting_up_on_the_phone_uses_the_ios_note_and_the_trouble_readout() {
    let s = setup_guidance(Provider::ICloud, Setup::Phone);
    assert!(s.contains("already there, nothing to install"), "iCloud on_ios missing:\n{s}");
    // looks_like() for a trouble
    assert!(s.contains("placeholder"), "the online-only symptom is missing:\n{s}");
    // atlas_can_fix() — both branches, so the flag is actually read
    assert!(s.contains("I fix this"), "a trouble Atlas can fix is not marked:\n{s}");
    assert!(s.contains("needs you"), "a trouble needing the user is not marked:\n{s}");
    // Behavioural, not just wording: the guidance is actually provider- and
    // device-specific, not one canned block — iCloud on the phone reads
    // differently from OneDrive on the phone.
    assert_ne!(s, setup_guidance(Provider::OneDrive, Setup::Phone), "guidance must vary by provider");
}

#[test]
fn setting_up_on_the_laptop_uses_the_windows_note_and_the_path_hint() {
    let s = setup_guidance(Provider::OneDrive, Setup::Laptop);
    assert!(s.contains("built in"), "OneDrive on_windows missing:\n{s}");
    assert!(s.contains("%OneDrive%"), "OneDrive windows_hint missing:\n{s}");
    // Behavioural: laptop guidance differs from the phone guidance for the
    // same provider — it reads the device, not a fixed script.
    assert_ne!(s, setup_guidance(Provider::OneDrive, Setup::Phone), "guidance must vary by device");
}

#[test]
fn the_per_provider_notes_are_read_directly() {
    // Name on_ios and windows_hint at the call site, not only through the
    // rendered guidance, so each predicate is exercised on its own.
    use atlas::cloudsync::Provider;
    assert!(!Provider::OneDrive.on_ios().is_empty(), "OneDrive should say something about iOS");
    assert!(!Provider::ICloud.on_ios().is_empty(), "iCloud should say something about iOS");
    assert!(!Provider::OneDrive.windows_hint().is_empty(), "the Windows path hint");
}
