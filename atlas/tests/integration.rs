use atlas::config::{Config, FracRect, RoleMatch};
use atlas::error::AtlasError;
use atlas::intent::{Intent, Parser};
use atlas::layout::{monitor_for_role, resolve_roles, to_pixels};
use atlas::platform::mock::{Action, MockPlatform};
use atlas::platform::Monitor;
use atlas::policy::{self, AllowAll, DenyAll};
use atlas::workspace;
use std::path::Path;

fn cfg() -> Config {
    Config::load(Path::new("config")).expect("shipped config must load")
}

fn mon(id: u32, x: i32, primary: bool) -> Monitor {
    Monitor { id, x, y: 0, width: 1920, height: 1040, primary }
}

// ---------- config ----------

#[test]
fn shipped_config_loads_and_cross_validates() {
    let c = cfg();
    assert!(c.apps.apps.contains_key("claude"));
    assert_eq!(c.apps.startup_order.len(), c.apps.shutdown_order.len());
}

#[test]
fn app_pointing_at_missing_layout_is_rejected() {
    let mut c = cfg();
    c.apps.apps.get_mut("chrome").unwrap().layout = "nope".into();
    let err = c.apps.validate(&c.layouts).unwrap_err();
    assert!(matches!(err, AtlasError::Config(_)), "got {err:?}");
}

// ---------- intent parsing ----------

#[test]
fn longest_phrase_wins_over_bare_verb() {
    let p = Parser::new(&cfg().commands);
    // "open" is also a phrase for open_app; it must not win here.
    assert_eq!(p.parse("open workspace"), Intent::WorkspaceOn);
    assert_eq!(p.parse("open chrome"), Intent::OpenApp("chrome".into()));
}

#[test]
fn punctuation_case_and_spacing_are_normalized() {
    let p = Parser::new(&cfg().commands);
    assert_eq!(p.parse("  Boot  Workspace!  "), Intent::WorkspaceOn);
    assert_eq!(p.parse("BOOT WORKSPACE"), Intent::WorkspaceOn);
}

#[test]
fn verb_with_no_target_is_not_a_match() {
    let p = Parser::new(&cfg().commands);
    assert_eq!(p.parse("open"), Intent::Unknown("open".into()));
}

#[test]
fn argument_is_captured_verbatim() {
    let p = Parser::new(&cfg().commands);
    assert_eq!(
        p.parse("research IETF QUIC v1 framing"),
        Intent::Research("ietf quic v1 framing".into())
    );
}

// ---------- monitor role resolution ----------

#[test]
fn three_monitors_map_to_distinct_roles() {
    let c = cfg();
    let mons = vec![mon(1, 0, true), mon(2, -1920, false), mon(3, 1920, false)];
    let roles = resolve_roles(&c.layouts, &mons);
    assert_eq!(roles["main"].id, 3, "main is rightmost");
    assert_eq!(roles["side"].id, 2, "side is leftmost of what's left");
    assert_eq!(roles["laptop"].id, 1, "laptop is the primary");
}

#[test]
fn unplugging_a_monitor_falls_back_instead_of_failing() {
    let c = cfg();
    let mons = vec![mon(1, 0, true)]; // laptop only, on the road
    let roles = resolve_roles(&c.layouts, &mons);
    let main = monitor_for_role(&c.layouts, &roles, "main").unwrap();
    let side = monitor_for_role(&c.layouts, &roles, "side").unwrap();
    assert_eq!(main.id, 1);
    assert_eq!(side.id, 1);
}

#[test]
fn roles_do_not_depend_on_windows_monitor_numbering() {
    let c = cfg();
    // Same physical arrangement, ids shuffled.
    let a = resolve_roles(&c.layouts, &vec![mon(1, 0, true), mon(2, 1920, false)]);
    let b = resolve_roles(&c.layouts, &vec![mon(7, 0, true), mon(4, 1920, false)]);
    assert_eq!(a["main"].x, b["main"].x);
    assert_eq!(a["laptop"].x, b["laptop"].x);
}

// ---------- layout math ----------

#[test]
fn fractional_rect_maps_onto_a_negative_origin_monitor() {
    let m = mon(2, -1920, false);
    let r = to_pixels(&m, FracRect { x: 0.5, y: 0.0, w: 0.5, h: 1.0 });
    assert_eq!((r.x, r.y, r.width, r.height), (-960, 0, 960, 1040));
}

// ---------- workspace orchestration ----------

#[test]
fn workspace_on_launches_and_places_every_app() {
    let c = cfg();
    let p = MockPlatform::new(vec![mon(1, 0, true), mon(2, -1920, false), mon(3, 1920, false)]);
    let r = workspace::workspace_on(&c, &p).unwrap();
    assert!(r.ok(), "failures: {:?}", r.failed);
    assert_eq!(r.placed.len(), c.apps.startup_order.len());

    let launches: Vec<_> = p.actions().iter()
        .filter_map(|a| match a { Action::Launch(n) => Some(n.clone()), _ => None })
        .collect();
    assert_eq!(launches, vec!["claude.exe", "discord.exe", "chrome.exe", "notepad.exe"]);
}

#[test]
fn a_slow_app_is_waited_for_not_skipped() {
    let c = cfg();
    let p = MockPlatform::new(vec![mon(1, 0, true), mon(3, 1920, false)])
        .with_slow_app("discord.exe", 12);
    let r = workspace::workspace_on(&c, &p).unwrap();
    assert!(r.ok(), "failures: {:?}", r.failed);

    let sleeps = p.actions().iter().filter(|a| matches!(a, Action::Sleep(_))).count();
    assert_eq!(sleeps, 12, "should poll exactly as long as needed");
    assert!(p.actions().iter().any(|a| matches!(a, Action::Place(n, _) if n == "discord.exe")));
}

#[test]
fn one_app_never_appearing_does_not_abort_the_rest() {
    let c = cfg();
    let p = MockPlatform::new(vec![mon(1, 0, true)])
        .with_slow_app("chrome.exe", 9999);
    let r = workspace::workspace_on(&c, &p).unwrap();

    assert!(!r.ok());
    assert_eq!(r.failed.len(), 1);
    assert_eq!(r.failed[0].0, "chrome");
    assert!(r.placed.contains(&"claude".to_string()));
    assert!(r.placed.contains(&"discord".to_string()), "apps after the failure still ran");
}

// ---------- running on the laptop alone ----------

#[test]
fn undocked_atlas_uses_standalone_layouts_not_shrunken_docked_ones() {
    let c = cfg();
    let p = MockPlatform::new(vec![mon(1, 0, true)]);
    workspace::workspace_on(&c, &p).unwrap();
    // Chrome is left_half when docked; on one screen it should be full.
    let full = to_pixels(&mon(1, 0, true), c.layouts.layout("full").unwrap());
    let placed_chrome = p.actions().iter().find_map(|a| match a {
        Action::Place(n, r) if n == "chrome.exe" => Some(*r),
        _ => None,
    });
    assert_eq!(placed_chrome, Some(full), "chrome should take the whole panel");
}

#[test]
fn docked_only_apps_are_skipped_on_a_single_screen() {
    let c = cfg();
    let p = MockPlatform::new(vec![mon(1, 0, true)]);
    let r = workspace::workspace_on(&c, &p).unwrap();
    assert!(!r.placed.contains(&"notepad".to_string()), "no room for the side pad");
    assert!(r.ok(), "skipping is not failing: {:?}", r.failed);
}

#[test]
fn the_same_config_still_tiles_properly_when_docked() {
    let c = cfg();
    let p = MockPlatform::new(vec![mon(1, 0, true), mon(2, -1920, false), mon(3, 1920, false)]);
    let r = workspace::workspace_on(&c, &p).unwrap();
    assert!(r.placed.contains(&"notepad".to_string()));
    let side = mon(2, -1920, false);
    let half = to_pixels(&side, c.layouts.layout("left_half").unwrap());
    let chrome = p.actions().iter().find_map(|a| match a {
        Action::Place(n, r) if n == "chrome.exe" => Some(*r),
        _ => None,
    });
    assert_eq!(chrome, Some(half), "docked, chrome goes back to half the side screen");
}

#[test]
fn already_running_app_is_placed_without_relaunching() {
    let c = cfg();
    let p = MockPlatform::new(vec![mon(1, 0, true)]);
    workspace::workspace_on(&c, &p).unwrap();
    let first = p.actions().iter().filter(|a| matches!(a, Action::Launch(_))).count();

    workspace::workspace_on(&c, &p).unwrap();
    let second = p.actions().iter().filter(|a| matches!(a, Action::Launch(_))).count();
    assert_eq!(first, second, "second boot must not spawn duplicates");
}

// ---------- policy ----------

#[test]
fn destructive_intents_are_blocked_without_approval() {
    assert!(policy::gate(&Intent::WorkspaceOff, &DenyAll).is_err());
    assert!(policy::gate(&Intent::CloseApp("chrome".into()), &DenyAll).is_err());
    assert!(policy::gate(&Intent::WorkspaceOff, &AllowAll).is_ok());
}

#[test]
fn unrecognized_input_defaults_to_blocked_not_allowed() {
    assert!(policy::gate(&Intent::Unknown("wipe everything".into()), &DenyAll).is_err());
}

#[test]
fn benign_intents_pass_without_a_prompt() {
    assert!(policy::gate(&Intent::WorkspaceOn, &DenyAll).is_ok());
    assert!(policy::gate(&Intent::Research("x".into()), &DenyAll).is_ok());
}

#[test]
fn role_match_enum_is_reachable() {
    assert_ne!(RoleMatch::Primary, RoleMatch::Leftmost);
}

#[test]
fn a_store_app_is_configured_by_id_not_by_path() {
    // Windows blocks running a packaged app from its install folder, so a
    // path is not just wrong here — it can never work.
    let c = cfg();
    let claude = c.apps.get("claude").unwrap();
    assert!(claude.store, "Claude is a Store app on this machine");
    assert!(claude.launch.contains('!'), "an app id, not a path: {}", claude.launch);
    assert!(!claude.launch.contains(":\\"), "must not be a file path");
}

#[test]
fn ordinary_apps_are_still_configured_by_path() {
    let c = cfg();
    assert!(!c.apps.get("chrome").unwrap().store);
    assert!(c.apps.get("chrome").unwrap().launch.contains("chrome.exe"));
}
