//! No test may reach outside its own scratch folder (12 Oct 2026).
//!
//! A test that pressed every hub button once opened dozens of real browser
//! tabs (sign-in buttons) and started real multi-gigabyte downloads. The
//! rule since: a test never launches a browser, an app or a downloader. Any
//! control that could is checked by its wiring, or pressed only against a
//! recording stub (`MockPlatform`).
//!
//! This reads every `tests/*.rs` for the ways a test could do it by hand.
//! The allow-list is for files that must start a process and that start
//! only harmless ones; each entry says why.

use std::path::Path;

/// (file, why it may). Keep this short.
const ALLOWED: &[(&str, &str)] = &[
    (
        "nothing_is_left_in_the_process_table.rs",
        "starts `cmd /C exit`, which does nothing, to prove it is reaped",
    ),
    (
        "no_test_touches_the_machine.rs",
        "this file names the things it looks for",
    ),
];

const LAUNCHERS: &[&str] = &[
    "curl",
    "wget",
    "cmd",
    "explorer",
    "xdg-open",
    "open",
    "start",
    "chrome",
    "msedge",
    "firefox",
    "brave",
    "powershell",
    "pwsh",
    "rundll32",
    "iexplore",
    "safari",
];

#[test]
fn no_test_launches_a_browser_a_downloader_or_an_app() {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(Path::new("tests")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path.file_name().unwrap().to_string_lossy().to_string();
        if ALLOWED.iter().any(|(f, _)| *f == name) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        for l in LAUNCHERS {
            for ctor in ["Command::new", "process::Command::new"] {
                let needle = format!("{ctor}(\"{l}\"");
                if crate::common::contains_code(&text, &needle) {
                    found.push(format!("{name}: {needle})"));
                }
            }
        }
        for api in [
            "ShellExecuteW(",
            "ShellExecuteA(",
            "webbrowser::open(",
            "open::that(",
        ] {
            // A call, not a quoted name a source-scanning test looks for.
            if text.lines().any(|l| {
                l.find(api)
                    .is_some_and(|i| !l[..i].ends_with('"') && !l.trim_start().starts_with("//"))
            }) {
                found.push(format!("{name}: {api}"));
            }
        }
    }
    assert!(
        found.is_empty(),
        "tests that could touch the real machine:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_allow_list_is_tiny_and_every_entry_exists() {
    assert!(ALLOWED.len() <= 3, "the allow-list grew; justify it");
    for (f, why) in ALLOWED {
        assert!(Path::new("tests").join(f).exists(), "{f} is gone");
        assert!(!why.is_empty());
    }
}
