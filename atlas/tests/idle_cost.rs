//! What Atlas costs while nothing is happening.
//!
//! The yardstick is Microsoft's idle-energy assessment, the only external one
//! with numbers in it: no periodic disk write more often than every ten
//! minutes, at most 200 flushes per ten-minute trace, and nothing waking the
//! CPU more often than every 100ms. Atlas failed three of the four for
//! nothing. One of the four test files the 10 Sep package named and the tree
//! never got; rebuilt 26 Sep 2026 with the pass itself.

use atlas::awareness::{Awareness, SCAN_BACKOFF_MAX, SCAN_BACKOFF_PRESENT};
use atlas::brain::{with_keep_alive, LlmConfig, COLD_FOR, WARM_FOR};
use atlas::config::Config;
use atlas::daemon::{Daemon, PERSIST_SWEEP_SECS};
use atlas::index::{Index, IndexConfig};
use atlas::platform::mock::MockPlatform;
use atlas::platform::Monitor;
use atlas::proactive::{Proactive, ProactiveConfig};
use atlas::store::Store;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-idle-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn plat() -> MockPlatform {
    MockPlatform::new(vec![Monitor { id: 1, x: 0, y: 0, width: 1920, height: 1040, primary: true }])
}

fn source(path: &str) -> String {
    crate::common::read_source_path(path).unwrap_or_else(|| panic!("{path}"))
}

/// The code of a function, from its signature to the next `fn` at the same
/// depth — enough for a source guard, comments stripped.
fn body_of(src: &str, sig: &str) -> String {
    let at = src.find(sig).unwrap_or_else(|| panic!("{sig} is gone"));
    let rest = &src[at + sig.len()..];
    // The earliest of the next method at any visibility and the end of the
    // `impl` (29 Sep 2026). This took the next `pub fn`, else the next `fn`;
    // after daemon.rs was split a `pub(super) fn` was neither, and a method
    // at the end of one child ran on into the next file.
    let end = ["\n    pub fn ", "\n    pub(super) fn ", "\n    fn ", "\n}\n"]
        .iter()
        .filter_map(|m| rest.find(m))
        .min()
        .unwrap_or(rest.len());
    rest[..end].lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n")
}

// ---------------------------------------------------------------------------
// 1. Writing everything it knows, every two seconds
// ---------------------------------------------------------------------------

#[test]
fn an_unchanged_value_is_not_written_again() {
    let store = Store::new(tmp("same"));
    store.save("notes", &vec!["a", "b"]).unwrap();
    let file = std::fs::read_dir(store.root()).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("notes")).unwrap().path();
    let before = std::fs::metadata(&file).unwrap().modified().unwrap();
    std::thread::sleep(Duration::from_millis(30));
    store.save("notes", &vec!["a", "b"]).unwrap();
    assert_eq!(std::fs::metadata(&file).unwrap().modified().unwrap(), before, "identical bytes were rewritten");
    store.save("notes", &vec!["a", "c"]).unwrap();
    assert_ne!(std::fs::metadata(&file).unwrap().modified().unwrap(), before, "a changed value was not written");
}

#[test]
fn the_skip_compares_against_the_disk_so_a_damaged_file_is_repaired() {
    let store = Store::new(tmp("repair"));
    store.save("notes", &vec!["kept"]).unwrap();
    let file = std::fs::read_dir(store.root()).unwrap().flatten().find(|e| e.file_name().to_string_lossy().starts_with("notes")).unwrap().path();
    std::fs::write(&file, b"{ damaged").unwrap();
    store.save("notes", &vec!["kept"]).unwrap();
    let back: Vec<String> = store.load("notes");
    assert_eq!(back, vec!["kept".to_string()]);
}

fn state_files(root: &Path) -> usize {
    std::fs::read_dir(root).map(|d| d.flatten().filter(|e| e.path().extension().is_some_and(|x| x == "json")).count()).unwrap_or(0)
}

fn clear_state(root: &Path) {
    for e in std::fs::read_dir(root).unwrap().flatten() {
        if e.path().extension().is_some_and(|x| x == "json") {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

#[test]
fn a_quiet_tick_saves_on_the_minute_and_a_tick_that_spoke_saves_at_once() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let root = tmp("persist");
    let mut d = Daemon::new(&c, &p, None, Store::new(root.clone()), Proactive::new(ProactiveConfig::default()));

    d.persist_after(1_000, false);
    assert!(state_files(&root) > 0, "the first save of a run always writes");

    clear_state(&root);
    d.persist_after(1_010, false);
    assert_eq!(state_files(&root), 0, "a quiet tick ten seconds later serialised everything anyway");

    d.persist_after(1_012, true);
    assert!(state_files(&root) > 0, "a tick that said something must save at once");

    clear_state(&root);
    d.persist_after(1_012 + PERSIST_SWEEP_SECS, false);
    assert!(state_files(&root) > 0, "the minute's sweep did not save");
}

#[test]
fn the_tick_saves_through_persist_after_and_nowhere_else() {
    let d = source("src/daemon.rs");
    let tick = body_of(&d, "pub fn tick(&mut self, t: u64) -> Vec<String> {");
    assert!(tick.contains("self.persist_after(t, !out.is_empty())"), "the tick no longer saves through persist_after");
    assert!(!tick.contains("self.persist();"), "persist() is back on the tick: every state file, every two seconds");
}

// ---------------------------------------------------------------------------
// 2. Walking every indexed folder every minute, all night
// ---------------------------------------------------------------------------

fn index_cfg(dir: &Path) -> IndexConfig {
    serde_json::from_value(serde_json::json!({ "roots": [dir.to_string_lossy()] })).unwrap()
}

#[test]
fn eight_quiet_hours_cost_about_sixteen_walks_not_four_hundred_and_eighty() {
    let dir = tmp("walks");
    let cfg = index_cfg(&dir);
    let p = plat();
    let mut index = Index::default();
    let mut a = Awareness::default();
    // Well past the last window change: nobody at the machine.
    let start = atlas::store::now() + 3_600;
    let mut t = start;
    while t < start + 8 * 3_600 {
        a.observe(&p, &mut index, Some(&cfg), false, t);
        t += 2;
    }
    let walks = a.scans();
    assert!((8..=24).contains(&walks), "{walks} walks in eight quiet hours");
    assert_eq!(a.how_long_to_wait(t), SCAN_BACKOFF_MAX, "backs off to half an hour");
}

#[test]
fn saying_anything_puts_the_scan_straight_back_to_a_minute() {
    let dir = tmp("heard");
    let cfg = index_cfg(&dir);
    let p = plat();
    let mut index = Index::default();
    let mut a = Awareness::default();
    let start = atlas::store::now() + 3_600;
    for i in 0..2_000 {
        a.observe(&p, &mut index, Some(&cfg), false, start + i * 2);
    }
    assert!(a.how_long_to_wait(start + 4_000) > a.scan_every);
    a.heard_you(start + 4_000);
    assert_eq!(a.how_long_to_wait(start + 4_000), a.scan_every);
}

#[test]
fn a_change_found_puts_the_scan_back_to_a_minute() {
    let dir = tmp("change");
    let cfg = index_cfg(&dir);
    let p = plat();
    let mut index = Index::default();
    let mut a = Awareness::default();
    let start = atlas::store::now() + 3_600;
    let mut t = start;
    while t < start + 3 * 3_600 {
        a.observe(&p, &mut index, Some(&cfg), false, t);
        t += 2;
    }
    assert!(a.how_long_to_wait(t) > a.scan_every);
    std::fs::write(dir.join("new.txt"), "hello").unwrap();
    // Run until the next scan finds it.
    let before = a.scans();
    while a.scans() == before {
        a.observe(&p, &mut index, Some(&cfg), false, t);
        t += 2;
    }
    assert_eq!(a.how_long_to_wait(t), a.scan_every);
}

#[test]
fn while_somebody_is_at_the_machine_the_longest_gap_is_five_minutes() {
    let dir = tmp("present");
    let cfg = index_cfg(&dir);
    let p = plat();
    let mut index = Index::default();
    let mut a = Awareness::default();
    let mut t = atlas::store::now() + 3_600;
    for _ in 0..8_000 {
        a.observe(&p, &mut index, Some(&cfg), false, t);
        t += 2;
    }
    assert_eq!(a.how_long_to_wait(t), SCAN_BACKOFF_MAX);
    p.focus_on("code.exe", "main.rs");
    a.observe(&p, &mut index, Some(&cfg), false, t);
    assert_eq!(a.how_long_to_wait(t), SCAN_BACKOFF_PRESENT);
}

// ---------------------------------------------------------------------------
// 3. Copying the whole tools config to read one field
// ---------------------------------------------------------------------------

#[test]
fn the_tools_config_is_resolved_once_and_shared() {
    let c = Config::load(Path::new("config")).unwrap();
    let p = plat();
    let d = Daemon::new(&c, &p, None, Store::new(tmp("shared")), Proactive::new(ProactiveConfig::default()));
    assert!(std::sync::Arc::ptr_eq(&d.tools_cfg(), &d.tools_cfg()), "tools_cfg is copying again");
    assert!(d.tools_cfg().work_dir.len() > 0, "work_dir is still resolved against this install");
    let src: String = source("src/daemon.rs").lines().filter(|l| !l.trim_start().starts_with("//")).collect::<Vec<_>>().join("\n");
    assert!(!src.contains("self.cfg.tools.clone().unwrap_or_default()"), "the whole config is being cloned per call again");
}

// ---------------------------------------------------------------------------
// 4. Reloading the model on nearly every turn
// ---------------------------------------------------------------------------

fn llm(url: &str, request: &str) -> LlmConfig {
    serde_json::from_value(serde_json::json!({
        "command": "curl",
        "args": ["-s", "-X", "POST", url, "-d", "@-"],
        "request": request,
        "response_path": "response"
    }))
    .unwrap()
}

const OLLAMA: &str = "http://localhost:11434/api/generate";
const BODY: &str = r#"{"model":"llama3.1:8b","prompt":"hi","stream":false}"#;

#[test]
fn a_machine_with_room_holds_the_model_for_half_an_hour() {
    let out = with_keep_alive(BODY, &llm(OLLAMA, BODY), Some(true));
    let v: serde_json::Value = serde_json::from_str(&out).expect("still JSON");
    assert_eq!(v["keep_alive"], WARM_FOR);
    assert_eq!(v["model"], "llama3.1:8b");
}

#[test]
fn a_machine_without_room_lets_it_go_after_a_minute() {
    let out = with_keep_alive(BODY, &llm(OLLAMA, BODY), Some(false));
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["keep_alive"], COLD_FOR);
}

#[test]
fn a_hosted_api_or_your_own_keep_alive_is_left_alone() {
    let hosted = llm("https://api.example.com/v1/chat/completions", BODY);
    assert_eq!(with_keep_alive(BODY, &hosted, Some(true)), BODY, "a hosted API rejects fields it doesn't know");
    let yours = r#"{"model":"m","keep_alive":-1,"prompt":"hi"}"#;
    assert_eq!(with_keep_alive(yours, &llm(OLLAMA, yours), Some(true)), yours, "a config that sets it wins");
    assert_eq!(with_keep_alive(BODY, &llm(OLLAMA, BODY), None), BODY, "not measured, not touched");
}

#[test]
fn the_shipped_request_still_parses_with_the_keep_alive_in_it() {
    // The shipped settings use Atlas's own model now (26 Sep 2026), so the
    // Ollama request this is about ships commented out, as the example to
    // uncomment. It's that example that has to still take the keep-alive.
    let c = Config::load(Path::new("config")).unwrap();
    assert!(c.tools.as_ref().unwrap().llm.is_none(), "the shipped settings name a model connection again");
    let yaml = std::fs::read_to_string("config/tools.yaml").unwrap();
    let block: String = yaml
        .lines()
        .skip_while(|l| !l.starts_with("# llm:"))
        .take_while(|l| l.starts_with('#') && !l.trim_start_matches('#').trim().is_empty())
        .filter(|l| !l.trim_start_matches('#').trim_start().starts_with('#'))
        .map(|l| l.strip_prefix("# ").unwrap_or(l.trim_start_matches('#')))
        .collect::<Vec<_>>()
        .join("\n");
    let mut parsed: std::collections::HashMap<String, LlmConfig> =
        serde_yaml::from_str(&block).expect("the commented Ollama example in tools.yaml is valid settings");
    let lc = parsed.remove("llm").expect("the Ollama example is there to uncomment");
    let body = lc.request.replace("{llm_model}", "m").replace("{system}", "s").replace("{user}", "u");
    let out = with_keep_alive(&body, &lc, Some(true));
    let v: serde_json::Value = serde_json::from_str(&out).expect("valid JSON");
    assert_eq!(v["keep_alive"], WARM_FOR);
}

#[test]
fn the_measured_plan_is_what_sets_it() {
    let d = source("src/daemon.rs");
    assert!(d.contains("let keep_resident = crate::fit::plan_for(&here).keep_model_warm;"));
    assert!(d.contains("crate::brain::set_keep_warm(keep_resident)"));
    // And the same plan keeps the model server from being let go when idle
    // (Phase 0.2): `lifecycle::model_stays_when_it_fits`.
    assert!(d.contains("model_stays_when_it_fits("));
    let b = source("src/brain.rs");
    assert!(b.matches("with_keep_alive(&expand(").count() >= 2, "a model request is going out without the keep-alive");
}

// ---------------------------------------------------------------------------
// 5. 144,000 wake-ups for an hour-long render
// ---------------------------------------------------------------------------

#[test]
fn polling_a_running_tool_starts_at_a_millisecond_and_settles_at_two_hundred() {
    let gaps: Vec<u64> = (0..12).map(|i| atlas::tools::poll_gap(i).as_millis() as u64).collect();
    assert_eq!(gaps, vec![1, 2, 4, 8, 16, 32, 64, 128, 200, 200, 200, 200]);
    assert_eq!(atlas::tools::poll_gap(u32::MAX).as_millis(), 200);
}

#[test]
fn an_hour_long_render_costs_under_twenty_thousand_wakeups() {
    let (mut elapsed, mut polls) = (0u64, 0u32);
    while elapsed < 3_600_000 {
        elapsed += atlas::tools::poll_gap(polls).as_millis() as u64;
        polls += 1;
    }
    assert!(polls < 20_000, "{polls} wake-ups; it was 144,000");
}

#[cfg(unix)]
#[test]
fn a_tool_that_finishes_at_once_is_noticed_at_once() {
    let tool: atlas::tools::ExternalTool = serde_json::from_value(serde_json::json!({ "command": "true" })).unwrap();
    let t = std::time::Instant::now();
    tool.run(&Default::default(), None).unwrap();
    crate::common::assert_prompt(t.elapsed(), Duration::from_millis(250), "took too long");
}

// ---------------------------------------------------------------------------
// The battery, which nothing read
// ---------------------------------------------------------------------------

#[test]
fn windows_power_status_is_read_without_guessing() {
    use atlas::health::power_from_status;
    assert_eq!(power_from_status(0, 1, 64), (true, Some(64)), "on battery");
    assert_eq!(power_from_status(1, 8, 90), (false, Some(90)), "charging");
    assert_eq!(power_from_status(1, 128, 255), (false, None), "a desktop with no battery");
    assert_eq!(power_from_status(255, 255, 255), (false, None), "unknown is never 'on battery'");
}

#[test]
fn linux_power_supplies_are_read_without_guessing() {
    use atlas::health::power_from_sysfs;
    let s = |k: &str, st: &str, on: &str, cap: &str| (k.to_string(), st.to_string(), on.to_string(), cap.to_string());
    assert_eq!(power_from_sysfs(vec![s("Battery", "Discharging", "", "41"), s("Mains", "", "0", "")].into_iter()), (true, Some(41)));
    assert_eq!(power_from_sysfs(vec![s("Battery", "Charging", "", "41"), s("Mains", "", "1", "")].into_iter()), (false, Some(41)));
    assert_eq!(power_from_sysfs(std::iter::empty()), (false, None), "a desktop");
}

#[cfg(windows)]
#[test]
fn windows_the_battery_is_read_on_this_machine() {
    // Checked by hand against `Get-CimInstance Win32_Battery` on le3o.
    let r = atlas::health::read_machine();
    println!("LIVE [power] on battery: {}, charge: {:?}", r.on_battery, r.battery_percent);
    if let Some(p) = r.battery_percent {
        assert!(p <= 100);
    }
    assert!(r.ram_total_gb > 0.0, "the memory reading still works beside it");
}
