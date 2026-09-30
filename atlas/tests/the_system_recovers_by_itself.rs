//! 29 Sep 2026, the sweep after Eric's laptop: the places where Atlas
//! stopped working and either said nothing or said the wrong thing, and
//! stayed that way until someone ended it in Task Manager.
//!
//! - A background Atlas ended in Task Manager left a lock that read
//!   "running" for minutes, so the next start refused, silently.
//! - A failed model call reached him as an empty reply with a caveat.
//! - A model server that died loading was started again on every pass.
//! - "No model" was explained as "it isn't loaded yet" whatever the cause.
//! - A second question while one was thinking froze the loop.
//! - The Talk page's polling piled up until the hub refused every page.

fn source(path: &str) -> String {
    // Through `read_source_path` (29 Sep 2026): daemon.rs and main.rs were
    // split into src/daemon/*.rs and src/main/*.rs, read here as one.
    crate::common::read_source_path(path).unwrap_or_else(|| panic!("{path}: not found"))
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-recovers-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---------------------------------------------------------------- the lock

#[test]
fn the_lock_names_its_holder_and_still_reads_the_old_form() {
    // What `take` writes: the moment, then this process.
    let dir = scratch("lock-line");
    let lock = atlas::onlyone::OnlyOne::at(&dir);
    lock.take(1_000).unwrap();
    let s = std::fs::read_to_string(lock.path()).unwrap();
    assert_eq!(atlas::onlyone::moment_in(&s), Some(1_000));
    assert_eq!(atlas::onlyone::holder_in(&s), Some(std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    // Written before 29 Sep 2026: a moment alone.
    assert_eq!(atlas::onlyone::moment_in("1000\n"), Some(1_000));
    assert_eq!(atlas::onlyone::holder_in("1000\n"), None);
}

#[cfg(target_os = "linux")]
#[test]
fn a_lock_whose_holder_has_ended_is_free_at_once() {
    let dir = scratch("dead-holder");
    let lock = atlas::onlyone::OnlyOne::at(&dir);
    let mut child = std::process::Command::new("true").spawn().unwrap();
    let gone = child.id();
    child.wait().unwrap();
    let now = 5_000;
    // Beat a second ago: before, this read "running" for GONE_AFTER_SECS.
    std::fs::write(lock.path(), format!("{} {gone}", now - 1)).unwrap();
    assert_eq!(lock.look(now), atlas::onlyone::Found::Free);
    assert!(lock.take(now).is_ok(), "a start after Task Manager is refused");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_lock_whose_holder_is_alive_still_reads_running() {
    let dir = scratch("live-holder");
    let lock = atlas::onlyone::OnlyOne::at(&dir);
    let now = 5_000;
    // Held by a process that is certainly there: this one.
    std::fs::write(lock.path(), format!("{} {}", now - 1, std::process::id())).unwrap();
    assert!(matches!(lock.look(now), atlas::onlyone::Found::Running { .. }));
    // And one with no holder named keeps the old reading.
    std::fs::write(lock.path(), format!("{}", now - 1)).unwrap();
    assert!(matches!(lock.look(now), atlas::onlyone::Found::Running { .. }));
    let _ = std::fs::remove_dir_all(&dir);
}

// ------------------------------------------------ a start that fails says so

#[test]
fn a_background_start_that_failed_says_why() {
    let root = scratch("start-problem");
    let plain = atlas::firstlaunch::start_failed_words(&root, Some(1));
    assert!(plain.contains("stopped straight away") && plain.contains("code 1"), "{plain}");
    atlas::firstlaunch::note_start_problem(&root, "Atlas is already running -- it checked in 3 seconds ago.");
    let told = atlas::firstlaunch::start_failed_words(&root, Some(1));
    assert!(told.contains("already running"), "what the background Atlas wrote is not shown: {told}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn opening_atlas_watches_the_background_start() {
    let main = source("src/main.rs");
    let at = main.find("if opening.start_background {").expect("opening starts the background Atlas");
    let block = &main[at..at + 600];
    let watched = block.find("start_background_watched(").expect("started unwatched again");
    let shown = block.find("show_problem(").expect("a failed start isn't shown");
    assert!(watched < shown);
}

// ------------------------------------------------------------ setup and updates

#[test]
fn setup_with_a_problem_is_tried_again_next_time() {
    let setup = source("src/setupwin.rs");
    let problems = setup.find("p.problems()").expect("setup no longer counts its problems");
    let only_then = setup[problems..].find("if problems == 0").map(|i| i + problems).expect("setup is marked done whatever happened");
    let mark = setup[only_then..].find("mark_set_up(").map(|i| i + only_then).expect("never marked done");
    assert!(problems < only_then && only_then < mark);
}

#[test]
fn the_overlay_and_typing_box_never_swap_in_an_update() {
    let main = source("src/main.rs");
    let helper = main.find("let helper = matches!(").expect("helpers aren't told apart");
    let gate = main.find("if !helper && std::env::var_os(\"ATLAS_UPDATE_PROBE\")").expect("helpers still swap and count trial starts");
    let swap = main.find("upgrade::swap_checked(").unwrap();
    let trial = main.find("upgrade::trial_on_start(").unwrap();
    assert!(helper < gate && gate < swap && swap < trial);
}

// ------------------------------------------------------------- the model

#[test]
fn a_failed_model_call_is_answered_with_why() {
    let w = atlas::daemon::model_failed_words("Model unreachable: connection refused");
    assert!(w.contains("connection refused") && w.contains("next message"), "{w}");
    assert!(!w.contains("Model unreachable"), "{w}");
    let bare = atlas::daemon::model_failed_words("");
    assert!(bare.contains("language model"), "{bare}");
    // And in the turn: the reason replaces the empty reply before the caveat
    // would be added, and the caveat is skipped for it.
    let daemon = source("src/daemon.rs");
    let failed = daemon.find("let failed_silent = decision.model == brain::Reached::No").expect("the empty reply is back");
    let mark = daemon[failed..].find("integrations::mark(").map(|i| i + failed).unwrap();
    assert!(daemon[failed..mark].contains("model_failed_words("));
}

#[test]
fn a_model_server_that_dies_young_waits_longer_each_time() {
    use atlas::daemon::model_server_pause as pause;
    assert_eq!(pause(0).as_secs(), 60);
    assert_eq!(pause(1).as_secs(), 120);
    assert_eq!(pause(2).as_secs(), 240);
    assert_eq!(pause(9).as_secs(), 30 * 60, "the pause has no ceiling");
    let mut last = 0;
    for d in 0..12 {
        let p = pause(d).as_secs();
        assert!(p >= last, "a later death waits less");
        last = p;
    }
}

#[test]
fn why_the_model_stopped_is_its_own_last_error() {
    let log = "load_tensors: loading model\nggml_vulkan: Device memory allocation failed: out of memory\nmain: exiting\n";
    let w = atlas::models::last_words_in(log).unwrap();
    assert!(w.contains("out of memory"), "{w}");
    assert_eq!(atlas::models::last_words_in("all fine\nlistening\n").as_deref(), Some("listening"));
    assert_eq!(atlas::models::last_words_in("\n  \n"), None);
}

#[test]
fn no_model_is_explained_by_what_was_found() {
    let empty = scratch("no-models");
    let cfg = atlas::models::ModelsConfig { dir: empty.display().to_string(), ..Default::default() };
    let why = atlas::models::why_no_model(&cfg);
    assert!(why.contains("no language model in my models folder"), "{why}");
    let missing = empty.join("not-there");
    let cfg = atlas::models::ModelsConfig { dir: missing.display().to_string(), ..Default::default() };
    let why = atlas::models::why_no_model(&cfg);
    assert!(why.contains("doesn't exist"), "{why}");
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn a_second_question_while_one_is_thinking_takes_the_other_slot() {
    // Both turns named the model's conversation slot, so the second waited
    // on the loop for the first to finish. It now takes the other slot, and
    // is still answered at once (`the_second_scan_holds` measures that).
    let daemon = source("src/daemon.rs");
    let aside = daemon.find("if self.pending_turn.is_some() {\n                        turn.aside = true;").expect("a second turn waits behind the first again");
    let asked = daemon[aside..].find(".converse_noting(").map(|i| i + aside).expect("converse_noting");
    assert!(aside < asked);
    let brain = source("src/brain.rs");
    assert_eq!(brain.matches("aside: turn.aside").count(), 2, "the conversation call no longer carries the choice");
}

// ------------------------------------------------------------- the Talk page

#[test]
fn the_talk_page_asks_one_question_at_a_time() {
    let s = atlas::hubpages::TALK_WAIT_SCRIPT;
    let guard = s.find("||out)return;out=true;").expect("fetches pile up again");
    let fetch = s.find("fetch(").unwrap();
    assert!(guard < fetch);
    assert!(s.contains("r.ok?r.json():null"), "a busy reply is read as done");
    let live = atlas::hub::LIVE_SCRIPT;
    assert!(live.find("||out)return;").unwrap() < live.find("out=true;fetch('/hub/changed.json").unwrap());
}

// ------------------------------------------------------ the firewall rule

#[test]
fn a_rule_windows_shows_with_a_variable_in_it_is_still_ours() {
    std::env::set_var("ATLAS_TEST_RULE_HOME", r"C:\Users\erics\AppData\Local");
    let exe = std::path::Path::new(r"C:\Users\erics\AppData\Local\Atlas\atlas.exe");
    let shown = "Rule Name:                            Atlas - your own devices\r\n\
                 Program:                              %ATLAS_TEST_RULE_HOME%\\Atlas\\atlas.exe\r\n";
    assert!(atlas::doorrule::describes_rule_for(shown, exe), "setup asks for Windows' permission every time again");
    let elsewhere = "Program:                              C:\\Other\\Atlas\\atlas.exe\r\n";
    assert!(!atlas::doorrule::describes_rule_for(elsewhere, exe));
    assert_eq!(atlas::doorrule::expand_vars("%NOT_A_VAR_ATLAS%\\x"), "%NOT_A_VAR_ATLAS%\\x");
}

// ------------------------------------------------ what it says it can't do

#[test]
fn atlas_doesnt_say_it_cant_see_the_screen_when_the_picture_reader_is_there() {
    let plan_says = vec![
        "I can't look at your screen and understand it — I can read text off it.".to_string(),
        "No language model fits, so I'll follow rules rather than reason. Most of what I do doesn't need one.".to_string(),
        "One thing at a time here.".to_string(),
    ];
    let said = atlas::daemon::what_this_machine_cant_do(plan_says.clone(), Some(Ok(())), true);
    assert_eq!(said, vec!["One thing at a time here.".to_string()], "the start-up notice contradicts what's installed");
    let missing = atlas::daemon::what_this_machine_cant_do(plan_says, Some(Err("I don't have its picture encoder yet".into())), false);
    assert!(missing[0].contains("picture encoder"), "{missing:?}");
    assert!(missing.iter().any(|l| l.starts_with("No language model fits")));
}

// -------------------------------------------------- is the internet there

#[test]
fn the_internet_check_asks_more_than_one_door() {
    // A network that blocks TCP to 1.1.1.1:53 read as offline all day.
    assert_eq!(atlas::connectivity::ConnectivityConfig::default().probe, atlas::connectivity::SHIPPED_PROBE);
    assert!(atlas::connectivity::ALSO_TRIED.iter().all(|a| a.ends_with(":443")));
    assert_eq!(atlas::connectivity::probe_targets("1.1.1.1:53, 8.8.8.8:443"), vec!["1.1.1.1:53", "8.8.8.8:443"]);
    // An address you chose is asked alone: nothing listening is offline.
    let mut c = atlas::connectivity::Connectivity::new(atlas::connectivity::ConnectivityConfig {
        probe: "127.0.0.1:1".into(),
        timeout_ms: 50,
        cache_secs: 9999,
        assume_offline: false,
    });
    assert_eq!(c.status(1), atlas::connectivity::Reach::Offline);
}
