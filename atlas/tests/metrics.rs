//! Metrics must describe this repo, not a remembered one.

use atlas::metrics;
use std::path::Path;

fn m() -> metrics::Metrics {
    metrics::gather(Path::new("."), vec!["look".into(), "why".into()])
}

#[test]
fn it_counts_this_repo_not_a_hardcoded_one() {
    let m = m();
    assert!(m.modules > 100, "expected the real module count, got {}", m.modules);
    assert!(m.src_lines > 10_000);
    assert!(m.test_fns > 1_000);
}

#[test]
fn dependency_count_ignores_feature_lists() {
    // Cargo.toml lists feature strings inside the windows dependency table.
    // Counting those as crates is how you end up claiming ten dependencies
    // in a document that boasts about having four.
    let m = m();
    assert!(
        m.dependencies <= EVERY_DEPENDENCY.len(),
        "feature lines are being counted as dependencies: {}",
        m.dependencies
    );
}

/// Every crate Atlas depends on, and why it earns its place.
///
/// A number on its own only makes growth *visible*; a list makes each addition
/// argue for itself against the ones already here. Atlas is meant to be as
/// self-contained as it can be, so anything added has to be doing work Atlas
/// genuinely cannot do itself.
const EVERY_DEPENDENCY: &[(&str, &str)] = &[
    ("aes-gcm", "sealing a push for one Android phone (Web Push, RFC 8291: AES-128-GCM), item 15, 2 Oct 2026"),
    ("hmac", "the HKDF steps of that sealing (HMAC-SHA256), item 15, 2 Oct 2026"),
    ("libloading", "opening sherpa-onnx's library at run time for the Kokoro voice, so the exe needs no C++ link and stays small (28 Sep 2026)"),
    ("serde", "turning state into files and back"),
    ("serde_yaml", "the config format, which is meant to be hand-editable"),
    ("serde_json", "what the API and the phone speak"),
    ("thiserror", "one error type across every module"),
    ("eframe", "Atlas's own window; drawing one from scratch is an OS project"),
    ("argon2", "turning a passphrase into a key, correctly"),
    ("chacha20poly1305", "actually encrypting the vault"),
    ("miniz_oxide", "inflating what PDFs and zips store deflated, so \"read this PDF\" and unzipping need no outside program (already in the tree under the image decoders)"),
    ("tract-onnx", "running models inside atlas.exe rather than shelling out to Python"),
    ("p256", "signing the token Apple's push service requires (ES256, P-256) with Eric's own key, so an iPhone hears from Atlas with the app closed -- a curve Atlas has nowhere else, and not one to hand-write"),
    ("ort", "putting the search and voice models on the laptop's NPU (item 20): ONNX Runtime opened at run time, the copy Kokoro already brings, with Intel's OpenVINO plugin -- the NPU has no other open route"),
    ("windows", "talking to the operating system at all"),
    ("cpal", "Windows builds only: recording a call -- your microphone, and what the laptop plays (WASAPI loopback) once the others have said yes (Eric, 24 Sep 2026). Talking to the sound devices is Windows' own COM plumbing; this is the maintained wrapper rather than several hundred lines of unsafe FFI"),
    ("wry", "Windows builds only: the hub shown inside Atlas's own window through the web view that ships with Windows (WebView2) -- Eric's 23 Sep ruling; a web engine is not something Atlas should write, and this borrows the one Windows already has rather than bundling one"),
    ("webview2-com-sys", "not a new dependency but wry's own, patched: vendored so atlas.exe doesn't import WebView2Loader.dll and refuse to start without it beside it -- see vendor/webview2-com-sys/ATLAS_VENDORED.md"),
    ("raw-window-handle", "Windows builds only: hands Atlas's window to the web view so the hub sits inside it rather than in a window of its own"),
    ("qrcode", "the code a phone scans to open Atlas -- QR's error-correction maths is not something Atlas should re-derive, and the crate is pure Rust with its image features off"),
    ("native-tls", "the mail client's transport (IMAP/SMTP) -- the system's own OpenSSL for one socket's worth of TLS, rather than a vendored Rust crypto stack"),
    ("ed25519-dalek", "signing releases so a device can prove an update is authored by you before installing it -- the symmetric household key proves origin, not authorship, and an update channel over a mesh needs authorship or a forged update is code execution on every device"),
    ("curve25519-dalek", "agreeing a key with a friend's Atlas (X25519) so Atlas can seal what it sends across the open internet itself -- the same curve the signing keys live on, so the key pinned for a friend is the key sealed to; already compiled in as a dependency of ed25519-dalek, declared because wire.rs calls it directly"),
    ("llama-cpp-2", "the phone's own language model, compiled into the phone app (feature `phone-llm`, off on the desktop): llama.cpp is the engine the laptop already runs as llama-server, and a phone can't start another program, so it goes in as a library rather than being rewritten (OPEN_GAPS P.7)"),
    ("encoding_rs", "turning the phone model's tokens back into text a piece at a time without splitting a character (llama-cpp-2's own API takes its decoder; `phone-llm` only)"),
    ("libc", "phones and other Unix systems only: asking how much memory the phone has (sysconf / sysctl), to choose the model it can hold"),
    ("sha2", "fingerprinting each release file inside the signed manifest so one signature binds every platform's build -- already compiled in as a dependency of ed25519-dalek, declared because release.rs calls it directly"),
];

/// Each dependency is named and justified.
///
/// The check that matters more than the count: a crate can be added, the
/// ceiling raised by one, and nobody ever asks what it was for. Here it has to
/// be written down next to the others.
#[test]
fn every_dependency_says_what_it_is_for() {
    let toml = std::fs::read_to_string("Cargo.toml").expect("Cargo.toml");
    // Section-aware: a `[features]` table lists feature *names*, not crates, so
    // its keys (`default`, `onnx`, `desktop-ui`, …) must not be read as
    // dependencies. Track the current `[section]` header and skip that table
    // whole, the same way the allowlist below skips package/profile knobs.
    let mut section = "";
    let mut listed: Vec<&str> = Vec::new();
    for raw in toml.lines() {
        let l = raw.trim();
        if l.starts_with('#') {
            continue;
        }
        if l.starts_with('[') && l.ends_with(']') {
            section = l;
            continue;
        }
        // Nor are the lint table's keys (lint names, audit Q4) or the
        // package's own settings.
        if section == "[features]" || section.starts_with("[lints") || section == "[package]" {
            continue;
        }
        let Some((name, _)) = l.split_once(" = ") else {
            continue;
        };
        if matches!(
            name,
            "name" | "version" | "edition" | "opt-level" | "strip" | "lto"
                | "codegen-units" | "debug" | "panic" | "incremental"
                // Test-target wiring, not dependencies: the suite is
                // declared by hand (autotests off) as [[test]] entries,
                // each a name/path pair pointing at a tests/*.rs file.
                | "autotests" | "path"
                // A test target that needs a feature (the phone's engine).
                | "required-features"
        ) {
            continue;
        }
        listed.push(name);
    }

    for crate_name in &listed {
        assert!(
            EVERY_DEPENDENCY.iter().any(|(n, _)| n == crate_name),
            "{crate_name} was added without saying what it's for — put it in \
             EVERY_DEPENDENCY with a reason, so the next one has to argue \
             against a written list rather than against a number"
        );
    }
    for (name, why) in EVERY_DEPENDENCY {
        assert!(
            why.split_whitespace().count() >= 4,
            "{name}'s reason is too thin to be a reason: {why:?}"
        );
    }
}

#[test]
fn platform_directory_counts_as_one_module() {
    // Must agree with what lib.rs calls a module.
    let m = m();
    let declared = std::fs::read_to_string("src/lib.rs")
        .unwrap()
        .matches("pub mod ")
        .count();
    assert!(
        m.modules.abs_diff(declared) <= 1,
        "metrics says {} modules, lib.rs declares {declared}",
        m.modules
    );
}

#[test]
fn the_unwired_list_is_reported_not_hidden() {
    let m = m();
    let out = m.render();
    assert!(out.contains("look"));
    assert!(out.contains("not yet wired in"));
}

#[test]
fn a_fully_wired_repo_says_so() {
    let clean = metrics::gather(Path::new("."), Vec::new());
    assert!(clean.modules > 0, "gathered nothing, so \"all wired\" would be vacuous");
    assert!(clean.render().contains("Every module is reachable"));
}

/// The list `atlas metrics` writes must be module names, not chopped prose.
///
/// # What shipped
///
/// `main.rs::unwired_from_wiring_test` read `UNWIRED_BASELINE` out of
/// `tests/wiring.rs` with `split(',')`. Every entry in that baseline is
/// followed by a comment saying why it is there, and comments contain commas,
/// so the split chopped the prose into fragments and every non-empty fragment
/// became a module name. `docs/METRICS.md` -- the file whose own header says
/// *do not restate these figures anywhere else, link here instead* -- shipped
/// with a "written but not reachable" section listing `tested (28`, `19`,
/// `13`, and two paragraphs of a comment about `afterme`. It said 18 modules
/// were unreachable. One is.
///
/// The parser was fixed to read lines. This is the test that would have caught
/// it, and it checks the property rather than the implementation: whatever
/// ends up in that list has to be something the tree actually contains.
#[test]
fn what_metrics_calls_unreachable_is_a_module_name_and_not_a_piece_of_a_comment() {
    let baseline: Vec<String> = std::fs::read_to_string("tests/wiring.rs")
        .expect("tests/wiring.rs")
        .lines()
        .skip_while(|l| !l.contains("UNWIRED_BASELINE"))
        .skip(1)
        .take_while(|l| !l.contains("];"))
        .filter_map(|l| {
            let t = l.trim();
            if t.starts_with("//") {
                return None;
            }
            let n = t.strip_prefix('"')?.split('"').next()?;
            (!n.is_empty()).then(|| n.to_string())
        })
        .collect();

    // Empty since 24 Sep 2026: consent, the last module nothing reached, was
    // wired with call notes. So the reader is checked against the file
    // itself instead: every quoted line inside the list is a name it read.
    let quoted_lines = std::fs::read_to_string("tests/wiring.rs")
        .expect("tests/wiring.rs")
        .lines()
        .skip_while(|l| !l.contains("UNWIRED_BASELINE"))
        .skip(1)
        .take_while(|l| !l.contains("];"))
        .filter(|l| l.trim().starts_with('"'))
        .count();
    assert_eq!(
        baseline.len(),
        quoted_lines,
        "the reader and the list disagree about how many modules are unwired -- it is \
         splitting the justifying comments again"
    );

    for name in &baseline {
        // A module name is one word. Anything with a space, a bracket or a
        // slash in it is a fragment of the comment that explains the entry.
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '_' || c.is_ascii_digit()),
            "{name:?} came out of UNWIRED_BASELINE and is not a module name. The \
             reader is splitting the justifying comments instead of skipping them."
        );
        assert!(
            Path::new(&format!("src/{name}.rs")).exists()
                || Path::new(&format!("src/{name}")).is_dir(),
            "UNWIRED_BASELINE names {name:?} and there is no such module. Either \
             the module was renamed and the baseline was not, or the reader is \
             chopping prose again."
        );
    }
}
