use atlas::craft::{self, Lang};
use std::path::PathBuf;

fn tmp(tag: &str) -> PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-craft-e2e-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

// ========================= lang_of_dir =========================

#[test]
fn a_cargo_toml_means_rust_even_with_no_rust_files_yet() {
    let dir = tmp("rust-empty");
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
    assert_eq!(craft::lang_of_dir(&dir), Some(Lang::Rust));
}

#[test]
fn any_recognised_python_marker_means_python() {
    for marker in ["pyproject.toml", "setup.py", "setup.cfg", "requirements.txt"] {
        let dir = tmp(&format!("py-{marker}"));
        std::fs::write(dir.join(marker), "").unwrap();
        assert_eq!(craft::lang_of_dir(&dir), Some(Lang::Python), "marker: {marker}");
    }
}

#[test]
fn an_empty_directory_is_told_apart_from_a_recognised_one() {
    let dir = tmp("nothing");
    assert_eq!(craft::lang_of_dir(&dir), None);
}

#[test]
fn a_single_stray_python_file_with_no_project_marker_is_not_enough() {
    // The whole point of going by the directory rather than one file's
    // extension: a lone .py script sitting in an otherwise-Rust repo should
    // not flip the ladder.
    let dir = tmp("stray");
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
    std::fs::write(dir.join("scratch.py"), "print(1)").unwrap();
    assert_eq!(craft::lang_of_dir(&dir), Some(Lang::Rust));
}

#[test]
fn cargo_toml_wins_when_both_markers_are_somehow_present() {
    let dir = tmp("both");
    std::fs::write(dir.join("Cargo.toml"), "[package]\nname=\"x\"").unwrap();
    std::fs::write(dir.join("setup.py"), "").unwrap();
    assert_eq!(craft::lang_of_dir(&dir), Some(Lang::Rust));
}

// ========================= the real ladder, on a real toy project =========================
//
// Not a mock of cargo -- an actual crate on disk, actually compiled. This is
// the same discipline as tonight's SignalListener tests: a fake would only
// prove my model of the tool, not the tool.

fn toy_crate_that_compiles(dir: &std::path::Path) {
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"toy\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/main.rs"), "fn main() { println!(\"hi\"); }\n").unwrap();
}

fn toy_crate_that_does_not_compile(dir: &std::path::Path) {
    std::fs::write(
        dir.join("Cargo.toml"),
        "[package]\nname = \"toy\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    // Missing semicolon -- a real, unambiguous syntax error.
    std::fs::write(dir.join("src/main.rs"), "fn main() { let x = 1 }\n").unwrap();
}

#[test]
fn a_real_crate_that_compiles_is_recognised_as_rust() {
    let dir = tmp("compiles");
    toy_crate_that_compiles(&dir);
    assert_eq!(craft::lang_of_dir(&dir), Some(Lang::Rust));
}

#[test]
fn cargo_check_on_a_real_broken_crate_actually_fails() {
    // Proves the ladder's ordering matters for a real reason: this is the
    // gate that should catch a broken crate before anything slower runs.
    let dir = tmp("broken");
    toy_crate_that_does_not_compile(&dir);
    let out = std::process::Command::new("cargo")
        .args(["check", "--all-targets"])
        .current_dir(&dir)
        .output()
        .expect("cargo must be on PATH for this test to mean anything");
    assert!(!out.status.success(), "a genuinely broken crate should fail cargo check");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("error"), "cargo's own words should say so: {stderr}");
}

#[test]
fn cargo_check_on_a_real_working_crate_actually_passes() {
    let dir = tmp("working");
    toy_crate_that_compiles(&dir);
    let out = std::process::Command::new("cargo")
        .args(["check", "--all-targets"])
        .current_dir(&dir)
        .output()
        .expect("cargo must be on PATH for this test to mean anything");
    assert!(out.status.success(), "a working crate should pass cargo check");
}

// ========================= read_ladder ordering, against real output shapes =========================

#[test]
fn a_real_compiler_error_is_read_as_a_blocking_fix_not_a_note() {
    let ran = vec![craft::Ran {
        command: "cargo check --all-targets".into(),
        tells: craft::Tells::Sound,
        passed: false,
        output: "error[E0308]: mismatched types".into(),
    }];
    match craft::read_ladder(Lang::Rust, &ran) {
        craft::Next::Fix { gate, .. } => assert_eq!(gate.command, "cargo check --all-targets"),
        other => panic!("a real compile error should block: {other:?}"),
    }
}

#[test]
fn still_worth_running_does_not_offer_tests_after_a_real_compile_failure() {
    let ran = vec![craft::Ran {
        command: "cargo check --all-targets".into(),
        tells: craft::Tells::Sound,
        passed: false,
        output: "error[E0308]: mismatched types".into(),
    }];
    let todo = craft::still_worth_running(Lang::Rust, &ran);
    assert!(
        !todo.iter().any(|g| g.tells == craft::Tells::Behaviour),
        "running tests against code that does not compile produces noise, not signal"
    );
}
