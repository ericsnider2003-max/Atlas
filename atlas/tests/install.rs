use atlas::install::{
    after, before, download_mb, pieces, what_to_fetch, required, state_of, InstallConfig, State,
    Step, COSTS_NOTHING,
};

#[test]
fn whisper_comes_first_because_it_unblocks_the_most() {
    // Nothing else unblocks more than one capability.
    assert_eq!(pieces()[0].name, "whisper");
    assert!(pieces()[0].without_it.contains("unblocks the most"));
}

#[test]
fn every_piece_says_what_stops_working_without_it() {
    for p in pieces() {
        assert!(!p.without_it.is_empty(), "{} doesn't say why it matters", p.name);
    }
}

#[test]
fn the_required_set_is_small_and_the_big_model_is_optional() {
    // 4.4GB should not stand between you and a working install.
    assert!(required().len() <= 5);
    let llm = pieces().into_iter().find(|p| p.name == "a thinking model").unwrap();
    assert!(llm.optional);
    // Renamed from `total_mb` on 19 Sep 2026: `retention` has a `total_mb`
    // method, and the deadness scans read bare names -- a call to this one
    // made that one look as though something reached it.
    assert!(download_mb(false) < 500, "the required set is {}MB", download_mb(false));
}

#[test]
fn a_half_downloaded_file_is_worse_than_a_missing_one_and_is_caught() {
    // Everything downstream fails confusingly instead of clearly.
    let model = pieces().into_iter().find(|p| p.name == "the listening model").unwrap();
    assert_eq!(state_of(&model, None), State::Missing);
    assert_eq!(state_of(&model, Some(148_000_000)), State::Present);
    assert_eq!(state_of(&model, Some(2_000_000)), State::HalfDownloaded);
}

#[test]
fn running_it_again_only_fetches_what_is_missing() {
    let found = vec![
        ("whisper", Some(30_000_000u64)),
        ("the listening model", Some(148_000_000)),
    ];
    let steps = what_to_fetch(&pieces(), &found);
    assert!(matches!(steps[0], Step::Skip { name: "whisper" }));
    assert!(steps.iter().any(|s| matches!(s, Step::Fetch { .. })));
}

#[test]
fn a_stopped_download_is_replaced_rather_than_skipped() {
    let found = vec![("whisper", Some(500_000u64))];
    let steps = what_to_fetch(&pieces(), &found);
    match &steps[0] {
        Step::Replace { why, .. } => assert!(why.contains("stopped partway")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn it_says_how_much_to_download_rather_than_inventing_a_time() {
    let said = before(&what_to_fetch(&pieces(), &[]));
    assert!(said.contains("MB to fetch"));
    assert!(said.contains("picks up where it stopped"));
}

#[test]
fn nothing_left_to_do_says_so() {
    let all: Vec<(&'static str, Option<u64>)> = pieces()
        .iter()
        .map(|p| (p.name, Some(p.mb as u64 * 1_000_000)))
        .collect();
    assert_eq!(before(&what_to_fetch(&pieces(), &all)), "Everything's already here.");
}

#[test]
fn the_report_names_each_failure_rather_than_saying_install_complete() {
    // "Install complete" with the model missing is how you find out an hour
    // later.
    let said = after(&[("whisper", true), ("the listening model", false)]);
    assert!(said.contains("the listening model"));
    assert!(said.contains("Without"));
    assert!(said.contains("Run it again"));
}

#[test]
fn a_failure_that_only_lost_optional_things_says_so() {
    let said = after(&[("tesseract", false)]);
    assert!(said.contains("All optional"));
}

#[test]
fn everything_working_gets_one_line() {
    assert!(after(&[("whisper", true)]).contains("All there"));
}

#[test]
fn one_flaky_download_does_not_cost_you_the_other_five() {
    assert!(InstallConfig::default().carry_on_after_failure);
}

#[test]
fn nothing_needs_a_key_an_account_or_a_card() {
    assert!(COSTS_NOTHING.contains("No account, no key, no card"));
    assert!(COSTS_NOTHING.contains("something is wrong and you should stop"));
}






// ================= one file, not fourteen =================

fn launcher() -> String {
    std::fs::read_to_string("ATLAS.bat").expect("ATLAS.bat must exist at the top level")
}

#[test]
fn there_is_exactly_one_thing_to_double_click() {
    // A folder of fourteen batch files with no obvious first one is worse
    // than no launcher at all — you end up opening them at random, which is
    // exactly what happened.
    let bats: Vec<String> = std::fs::read_dir(".")
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|n| n.to_lowercase().ends_with(".bat"))
        .collect();
    assert_eq!(bats, vec!["ATLAS.bat".to_string()], "found: {bats:?}");
}

#[test]
fn a_first_run_installs_rather_than_showing_a_menu() {
    // Showing options to someone who hasn't installed anything is how they
    // end up guessing.
    let b = launcher();
    assert!(b.contains("Atlas isn't set up yet"));
    let first = b.find("isn't set up yet").unwrap();
    let menu = b.find(":menu").unwrap();
    assert!(first < menu, "the menu comes before the setup prompt");
}

#[test]
fn the_option_you_want_is_marked_and_is_the_default() {
    let b = launcher();
    assert!(b.contains("this is the one you want"));
    assert!(b.contains("just press Enter for 1"));
    assert!(b.contains(r#"if "%PICK%"=="" goto start"#), "Enter does nothing");
}

#[test]
fn it_can_be_run_again_safely() {
    let b = launcher();
    assert!(b.contains("[have]"), "it doesn't skip what's already there");
    assert!(b.contains("picks up where it stopped"));
}

#[test]
fn the_launcher_never_needs_admin_rights() {
    let b = launcher();
    for needs_admin in ["runas", "net session", "Start-Process -Verb"] {
        assert!(!b.contains(needs_admin), "asks for admin via {needs_admin}");
    }
}

#[test]
fn no_url_in_the_launcher_carries_a_key() {
    let b = launcher().to_lowercase();
    for leak in ["api_key", "apikey", "token=", "?key=", "authorization"] {
        assert!(!b.contains(leak), "found {leak}");
    }
}

#[test]
fn every_menu_entry_reaches_something_that_exists() {
    // A menu offering a thing that isn't there is the fourteen-files problem
    // in a smaller box.
    let b = launcher();
    let main = crate::common::source_of("main");
    for (label, subcommand) in [
        ("Check what's working", "doctor"),
        ("Settings", "settings"),
        ("Who I can sign you in as", "access"),
        ("Set up phone sync", "sync-setup"),
    ] {
        // `"%EXE%" doctor`, not `atlas.exe doctor`: the launcher now calls
        // Atlas by an absolute path built from `%~dp0`, because a bare
        // `atlas.exe` after a four-level walk up the working directory meant
        // the menu could reach a different install than the one the file
        // sits in.
        assert!(
            b.contains(&format!("\"%EXE%\" {subcommand}")),
            "{label} calls nothing — or calls it without a full path"
        );
        assert!(
            main.contains(&format!("Some(\"{subcommand}\")")),
            "{label} calls `atlas {subcommand}`, which main.rs doesn't handle"
        );
    }

    // The two doors the program is built around. Menu item 1 said "this is
    // the one you want" and ran the bare typed prompt; `--daemon` (wake
    // word, scheduled work, proactive offers) and `--voice` were unreachable
    // from the launcher entirely, so the always-on assistant could only be
    // started by someone who already knew the flag.
    for flag in ["--daemon", "--voice"] {
        assert!(b.contains(flag), "the launcher cannot start Atlas with {flag}");
        assert!(
            main.contains(&format!("flag(\"{flag}\")")),
            "the launcher offers {flag} and main.rs does not handle it"
        );
    }
}
