use atlas::look::TOKENS;
use atlas::panel::{narration, place, place_anyway, window_args, Decision, Panel, PanelConfig};
use atlas::platform::Monitor;

fn two_screens() -> Vec<Monitor> {
    vec![
        Monitor { id: 1, x: 0, y: 0, width: 2560, height: 1392, primary: true },
        Monitor { id: 2, x: 2560, y: 0, width: 2560, height: 1392, primary: false },
    ]
}
fn one_screen() -> Vec<Monitor> {
    vec![Monitor { id: 1, x: 0, y: 0, width: 2560, height: 1392, primary: true }]
}
fn cfg() -> PanelConfig {
    PanelConfig::default()
}

// ================= what gets said =================

/// Salvaged from the improvements chat's `panels.rs` in the second 17 Sep
/// merge, and it is the one test in that file worth carrying across: the rest
/// of its additions exercise `look`'s HTML renderer, which this side deleted
/// when the panels were rebuilt natively, so they no longer compile.
///
/// This one is about `panel::narration`, which is not HTML and is real. It
/// earns its place because `narration` had **no direct test on this side at
/// all** — and it is exactly the function the first 17 Sep merge re-orphaned
/// by taking a `daemon.rs` that hand-rolled its logic instead of calling it.
/// A recovered wiring with no test is a wiring that gets lost a third time.
#[test]
fn everything_is_spoken_as_well_as_shown_except_the_ambient_mark() {
    let c = cfg();
    assert!(narration(Panel::Tasks, "three things waiting", &c).is_some());
    assert!(narration(Panel::Mind, "step two of three", &c).is_some());
    assert!(narration(Panel::Presence, "", &c).is_none(), "the mark says nothing");
}

// ================= where things appear =================

#[test]
fn the_waking_panel_takes_the_middle_of_the_screen_youre_looking_at() {
    // The one moment worth interrupting for — and it leaves by itself.
    match place(Panel::Waking, &two_screens(), &cfg()) {
        Decision::Show(p) => {
            assert_eq!(p.monitor, 1, "your main screen");
            assert!(p.rect.x > 700 && p.rect.x < 1100, "centred: {}", p.rect.x);
            assert!(p.on_top && p.chromeless);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn reading_panels_open_full_height_down_the_right_of_the_screen_youre_on() {
    // Its own window, top to bottom. Nothing to scroll a small box for, and
    // on the screen you're actually looking at.
    for panel in [Panel::Tasks, Panel::Mind, Panel::Controls] {
        match place(panel, &two_screens(), &cfg()) {
            Decision::Show(p) => {
                assert_eq!(p.monitor, 1, "{panel:?} goes on your main screen");
                assert_eq!(p.rect.width, 400, "a slim column");
                assert_eq!(p.rect.height, 1392, "top of the screen to the bottom");
                assert_eq!(p.rect.x, 2160, "against the right edge");
                assert_eq!(p.rect.y, 0);
                assert!(p.on_top, "you asked for it, so it stays up");
            }
            o => panic!("{panel:?}: {o:?}"),
        }
    }
}

#[test]
fn you_can_send_them_to_the_other_screen_if_you_would_rather() {
    let second = PanelConfig { prefer_second_screen: true, ..cfg() };
    match place(Panel::Tasks, &two_screens(), &second) {
        Decision::Show(p) => assert_eq!(p.monitor, 2),
        o => panic!("{o:?}"),
    }
    let p = atlas::panel::place_on_second(Panel::Tasks, &two_screens(), &cfg()).unwrap();
    assert_eq!(p.monitor, 2);
}

#[test]
fn on_a_big_monitor_the_column_costs_nothing_so_it_just_opens() {
    // 400px of 2560 is a sixth of the width.
    match place(Panel::Tasks, &one_screen(), &cfg()) {
        Decision::Show(p) => assert_eq!(p.monitor, 1),
        o => panic!("{o:?}"),
    }
}

#[test]
fn on_a_laptop_screen_it_asks_because_a_fifth_of_the_width_is_in_your_way() {
    // The thing that matters is how much of the screen it costs, not how
    // many screens there are.
    let laptop = vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1080, primary: true }];
    match place(Panel::Tasks, &laptop, &cfg()) {
        Decision::AskFirst(q) => assert!(q.contains("shall I just read it"), "got: {q}"),
        o => panic!("{o:?}"),
    }
    let small = vec![Monitor { id: 1, x: 0, y: 0, width: 1366, height: 768, primary: true }];
    assert!(matches!(place(Panel::Mind, &small, &cfg()), Decision::AskFirst(_)));
}

#[test]
fn plugging_into_monitors_stops_it_asking_without_you_changing_anything() {
    let laptop = vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1080, primary: true }];
    assert!(matches!(place(Panel::Tasks, &laptop, &cfg()), Decision::AskFirst(_)));
    assert!(matches!(place(Panel::Tasks, &two_screens(), &cfg()), Decision::Show(_)));
}

#[test]
fn saying_yes_on_a_laptop_still_puts_it_down_the_side() {
    let laptop = vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1080, primary: true }];
    let p = place_anyway(Panel::Tasks, &laptop, &cfg()).unwrap();
    assert_eq!(p.rect.x, 1520);
    assert_eq!(p.rect.height, 1080, "still full height");
}

#[test]
fn it_only_asks_when_you_wanted_a_second_screen_and_there_isnt_one() {
    let second = PanelConfig { prefer_second_screen: true, ..cfg() };
    match place(Panel::Tasks, &one_screen(), &second) {
        Decision::AskFirst(q) => assert!(q.contains("or shall I just read it"), "got: {q}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn saying_no_means_it_speaks_instead_of_showing() {
    let never_ask =
        PanelConfig { prefer_second_screen: true, ask_on_single_display: false, ..cfg() };
    match place(Panel::Tasks, &one_screen(), &never_ask) {
        Decision::SpeakOnly(why) => assert!(why.contains("nowhere to put it")),
        o => panic!("{o:?}"),
    }
}

#[test]
fn saying_yes_puts_it_down_the_side_taking_as_little_as_possible() {
    let p = place_anyway(Panel::Tasks, &one_screen(), &cfg()).unwrap();
    assert_eq!(p.rect.width, 400);
    assert_eq!(p.rect.x, 2160, "against the right edge");
    assert!(p.on_top, "you asked for it, so it stays visible");
}

#[test]
fn the_ambient_mark_never_touches_the_screen_youre_working_on() {
    match place(Panel::Presence, &two_screens(), &cfg()) {
        Decision::Show(p) => {
            assert_eq!(p.monitor, 2);
            assert_eq!(p.rect.width, 120, "small");
            assert!(p.rect.x > 4900, "bottom right, in peripheral vision");
            assert!(p.rect.y > 1200);
        }
        o => panic!("{o:?}"),
    }
}

#[test]
fn with_one_screen_there_is_nowhere_for_an_ambient_mark_to_go() {
    // Better to have none than to put a glowing dot over your work.
    assert!(matches!(place(Panel::Presence, &one_screen(), &cfg()), Decision::SpeakOnly(_)));
}

#[test]
fn a_panel_window_has_no_browser_furniture() {
    let p = place_anyway(Panel::Tasks, &one_screen(), &cfg()).unwrap();
    let args = window_args("http://127.0.0.1:8787/panel/tasks", &p);
    assert!(args.iter().any(|a| a.starts_with("--app=")), "no tabs, no address bar");
    assert!(args.iter().any(|a| a.contains("--window-position=2160,0")));
    assert!(args.iter().any(|a| a.contains("--window-size=400,1392")));
    assert!(args.iter().any(|a| a.contains("panel-profile")), "never inherits your tabs");
    assert!(args.iter().any(|a| a == "--disable-background-networking"), "a panel doesn't call Google on its own");
}

// ================= how it looks =================

#[test]
fn the_palette_avoids_both_obvious_routes() {
    // Iron Man cyan is the cliché the moment you say Jarvis, and near-black
    // with one acid accent is what every dark interface does. The panels
    // wear the locked hub design since 26 Sep: Warm Paper's warm-orange on
    // white, and Ember Dark's slate — neither cyan nor black.
    assert!(TOKENS.contains("--signal:#D9730D"), "warm orange, not cyan");
    assert!(!TOKENS.contains("#00FFFF") && !TOKENS.contains("#00E5FF"));
    assert!(atlas::look::TOKENS_DARK.contains("--ink:#0C0F14"), "slate, not black");
    assert!(!TOKENS.contains("#000000") && !atlas::look::TOKENS_DARK.contains("#000000"));
}

// ================= summoning them by voice =================

use atlas::config::Config;
use atlas::daemon::Daemon;
use atlas::platform::mock::MockPlatform;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-panel-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}
fn conf() -> Config {
    Config::load(Path::new("config")).unwrap()
}
fn mock(monitors: Vec<Monitor>) -> MockPlatform {
    MockPlatform::new(monitors)
}
fn daemon<'a>(c: &'a Config, p: &'a MockPlatform, tag: &str) -> Daemon<'a> {
    Daemon::new(c, p, None, Store::new(tmp(tag)), Proactive::new(ProactiveConfig::default()))
}

#[test]
fn saying_youre_ready_brings_up_the_waking_panel_and_the_brief() {
    let (c, p) = (conf(), mock(two_screens()));
    let mut d = daemon(&c, &p, "ready");
    let said = d.turn("i'm ready", 100);
    assert_eq!(d.wants_panel, Some(Panel::Waking));
    assert!(!said.is_empty(), "and it says the brief out loud: {said}");
}

#[test]
fn asking_for_the_outstanding_list_puts_it_up_and_reads_it() {
    let (c, p) = (conf(), mock(two_screens()));
    let mut d = daemon(&c, &p, "tasks");
    let said = d.turn("pull up my outstanding tasks", 100);
    assert_eq!(d.wants_panel, Some(Panel::Tasks));
    assert!(!said.is_empty(), "spoken as well as shown");
}

#[test]
fn asking_what_its_thinking_puts_up_the_live_panel() {
    let (c, p) = (conf(), mock(two_screens()));
    let mut d = daemon(&c, &p, "mind");
    d.turn("show me what you're thinking", 100);
    assert_eq!(d.wants_panel, Some(Panel::Mind));
}

#[test]
fn asking_for_settings_saves_you_going_to_find_them() {
    let (c, p) = (conf(), mock(two_screens()));
    let mut d = daemon(&c, &p, "controls");
    d.turn("pull up your settings", 100);
    assert_eq!(d.wants_panel, Some(Panel::Controls));
}

#[test]
fn one_screen_still_gets_the_panel() {
    let (c, p) = (conf(), mock(one_screen()));
    let mut d = daemon(&c, &p, "single");
    // A single big monitor: the column costs a sixth of the width, so it
    // just opens.
    d.turn("pull up my outstanding tasks", 100);
    assert_eq!(d.wants_panel, Some(Panel::Tasks));
}

#[test]
fn dismissing_takes_it_away_without_a_word() {
    let (c, p) = (conf(), mock(two_screens()));
    let mut d = daemon(&c, &p, "dismiss");
    d.turn("pull up my outstanding tasks", 100);
    assert!(d.wants_panel.is_some());
    assert_eq!(d.turn("that's enough", 110), "");
    assert!(d.wants_panel.is_none());
}

#[test]
fn asking_for_something_that_isnt_a_panel_says_so() {
    let (c, p) = (conf(), mock(two_screens()));
    let mut d = daemon(&c, &p, "nope");
    let said = d.turn("pull up the weather", 100);
    assert!(said.contains("can't put") && said.contains("open an app by name"), "{said}");
}

#[test]
fn saying_youre_ready_only_means_resume_if_something_was_paused() {
    // Otherwise it's you arriving at the desk, which is a different thing.
    let (c, p) = (conf(), mock(two_screens()));
    let mut d = daemon(&c, &p, "ready-vs-resume");
    d.turn("hold on", 100);
    let resumed = d.turn("i'm ready", 110);
    assert!(resumed.contains("Go ahead") || resumed.contains("Where were we")
        || !resumed.is_empty(), "got: {resumed}");
    assert!(d.wants_panel.is_none(), "resuming is not the waking moment");
}

