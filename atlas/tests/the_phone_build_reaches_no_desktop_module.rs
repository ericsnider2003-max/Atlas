//! The phone builds (`--no-default-features`) leave out the desktop-only
//! modules. A call to one of them from code every build compiles breaks the
//! iPhone and Android builds, and nothing on the desktop notices.
//!
//! 28 Sep 2026: exactly that failed both phone workflows —
//! `daemon::about_now` asked `setupwin::setup_pieces` for the list of setup
//! pieces, and `setupwin` exists only with `desktop-ui`. The list moved to
//! `getpieces`, and this test stops the next one.

use std::path::Path;

/// Modules `lib.rs` declares behind `feature = "desktop-ui"`.
fn desktop_only() -> Vec<String> {
    let lib = std::fs::read_to_string("src/lib.rs").unwrap();
    let lines: Vec<&str> = lib.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if let Some(rest) = l.trim().strip_prefix("pub mod ") {
            // The attributes directly above this `pub mod`, and nothing further.
            let mut j = i;
            let mut gated = false;
            while j > 0 && lines[j - 1].trim_start().starts_with("#[") {
                j -= 1;
                gated |= lines[j].contains("feature = \"desktop-ui\"");
            }
            if gated {
                out.push(rest.trim_end_matches(';').to_string());
            }
        }
    }
    out
}

/// Whether line `i` sits inside an item gated on `desktop-ui`: the attribute
/// on the nearest enclosing item (the last line above it that starts at
/// column zero), or on any statement of the lines just above it.
fn behind_desktop_ui(lines: &[&str], i: usize) -> bool {
    if lines[i.saturating_sub(3)..i].iter().any(|l| l.contains("feature = \"desktop-ui\"")) {
        return true;
    }
    let mut j = i;
    while j > 0 {
        let l = lines[j];
        // An item starts at column zero with a word (`fn`, `pub`, `impl` …);
        // `) {` and `}` closing a signature or block do not.
        let top = l.chars().next().is_some_and(|c| c.is_ascii_alphabetic());
        if top {
            break;
        }
        j -= 1;
    }
    let mut k = j;
    while k > 0 && (lines[k - 1].starts_with("#[") || lines[k - 1].starts_with("///")) {
        k -= 1;
        if lines[k].contains("feature = \"desktop-ui\"") {
            return true;
        }
    }
    false
}

#[test]
fn nothing_every_build_compiles_calls_a_desktop_only_module() {
    let gated = desktop_only();
    assert!(gated.contains(&"setupwin".to_string()), "lib.rs no longer gates setupwin: {gated:?}");
    let mut bad = Vec::new();
    for entry in std::fs::read_dir("src").unwrap().flatten() {
        let p = entry.path();
        let name = p.file_stem().and_then(|s| s.to_str()).unwrap_or("").to_string();
        // The gated modules themselves, and the desktop binary, may.
        if p.extension().and_then(|e| e.to_str()) != Some("rs") || gated.contains(&name) || name == "main" {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, l) in lines.iter().enumerate() {
            for m in &gated {
                if l.contains(&format!("crate::{m}::")) && !l.trim_start().starts_with("//") {
                    // Allowed when the use itself sits behind the same feature.
                    if !behind_desktop_ui(&lines, i) {
                        bad.push(format!("{}:{}: {}", Path::new(&p).display(), i + 1, l.trim()));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "these break the phone builds (no desktop-ui):\n  {}", bad.join("\n  "));
}
