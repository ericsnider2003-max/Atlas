//! Three settings that parsed, were listed as dead, and stayed that way.
//!
//! All the same shape, and it is the `server.enabled` shape from earlier
//! today: the code builds `XConfig::default()` at the call site while your
//! section of `tools.yaml` sits unread. The file says one thing, the program
//! does another, and nothing in between says so.
//!
//! `config::PARSED_AND_NEVER_READ` had named all three for some time —
//! `atlas doctor` has been reading them out to whoever asked. The list was
//! right; nothing acted on it.
//!
//! **`retention` is the one that mattered.** It is the pass that deletes your
//! files to stay inside a disk budget, and it was ignoring the budget.

use atlas::config::Config;
use std::path::Path;

fn tools() -> atlas::voice::ToolsConfig {
    Config::load(Path::new("config")).unwrap().tools.expect("tools.yaml loads")
}

#[test]
fn the_disk_budget_is_read_from_your_config_not_rebuilt() {
    // The call site is inside a crew errand, so this checks the source: the
    // closure must be handed a config from `tools_cfg()` rather than making
    // one. A behavioural test would need a 500MB data directory to tell the
    // two apart.
    let d = crate::common::source_of("daemon");
    let at = d.find("crate::retention::survey").expect("the housekeeping pass is gone");
    let window = &d[at.saturating_sub(1200)..(at + 400).min(d.len())];
    // Comments stripped first. The note explaining this fix names the very
    // pattern it forbids, and the first version of this test failed on its
    // own explanation.
    let around: String = window
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !around.contains("RetentionConfig::default()"),
        "the retention pass is building its own budget again, so your \
         `retention:` section does nothing"
    );
    assert!(
        around.contains("self.tools_cfg().retention"),
        "the retention pass is not reading your config"
    );
}

#[test]
fn the_trading_thresholds_are_read_from_your_config() {
    // 28 Sep 2026: the ladder half of this went with `ladder.rs`, which left
    // personal Atlas (tests/personal_atlas_is_its_own.rs). `together` stays.
    let m = crate::common::source_of("main");
    assert!(
        m.contains("fn trade_cfgs()"),
        "the helper that reads your together settings is gone"
    );
    assert!(
        !m.contains("atlas::together::TogetherConfig::default()"),
        "`atlas trade` is building the say-above/alarm-above thresholds again"
    );
}

#[test]
fn the_shipped_file_still_carries_both_sections() {
    // Wiring a setting is only half of it. If the section vanished from the
    // shipped file, nobody would know the behaviour was theirs to change.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    // 28 Sep 2026: `ladder:` left with `ladder.rs`; the other two remain.
    for section in ["retention:", "together:"] {
        assert!(raw.contains(section), "{section} is no longer in the shipped config");
    }
}

#[test]
fn a_changed_setting_is_what_the_program_sees() {
    // The parse half, end to end: the value in the file is the value on the
    // struct. If this ever diverges, everything above is checking plumbing
    // that carries the wrong water.
    let t = tools();
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");

    let budget_in_file: u64 = raw
        .lines()
        .find_map(|l| l.trim().strip_prefix("total_budget_mb:"))
        .and_then(|v| v.trim().split_whitespace().next())
        .and_then(|v| v.parse().ok())
        .expect("total_budget_mb is in the shipped file");

    assert_eq!(
        t.retention.total_budget_mb as u64, budget_in_file,
        "the budget the program holds is not the budget in the file"
    );
}

#[test]
fn nothing_claims_these_are_dead_any_more() {
    // `atlas doctor` reads this list out loud. Leaving them on it after wiring
    // would tell the user a working setting does nothing, which is the same
    // lie pointing the other way.
    for (key, _) in atlas::config::PARSED_AND_NEVER_READ {
        assert!(
            !["retention", "ladder", "together", "sync"].contains(key),
            "{key} is wired and still listed as doing nothing"
        );
    }
}

// ---------- the hub's off switch ----------

#[test]
fn the_hub_can_be_turned_off_from_config() {
    // `hub.enabled` shipped `true` and was read by nothing, so there was no
    // way to turn the dashboard off — `config::PARSED_AND_NEVER_READ` recorded
    // exactly that sentence against it. Same shape as `server.enabled`.
    //
    // A source check: the pages are served inside a request closure in the
    // binary, and standing up an HTTP server to prove one branch would test
    // the socket rather than the switch.
    let m = crate::common::source_of("main");
    assert!(
        (m.contains("Action::Hub(_) if !tools.hub.enabled") || m.contains("Action::Hub(_) | Action::HubQ(..) if !tools.hub.enabled")),
        "the hub serves its pages without consulting hub.enabled again"
    );

    // And the shipped file still offers the switch.
    let raw = std::fs::read_to_string("config/tools.yaml").expect("tools.yaml");
    assert!(raw.contains("hub:"), "the hub section is no longer in the shipped config");
}

#[test]
fn the_hub_ships_on() {
    // On, as it already was. Wiring a switch must not quietly take the
    // dashboard away from anyone who never asked for it to change.
    assert!(tools().hub.enabled, "the hub now ships off, which nobody asked for");
}
