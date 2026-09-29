//! Proving a change *inside* a real project.
//!
//! The distinction these defend is the one that matters for a system someone's
//! livelihood runs on: "the snippet compiled on its own" (isolated) is not
//! "the project still builds and its own tests pass with this in it"
//! (integrated). `prove_in_project` is the integrated check, and it must (a)
//! work in a throwaway copy, never the project itself, (b) run the project's
//! own command and read its real result, and (c) tell the truth about whether
//! the change actually replaced code the tests exercise.

use atlas::selfwork::{prove_in_project, Edit};

/// A tiny project whose "test command" is a stub script emitting a chosen
/// result line — so the plumbing (copy, apply, run in the copy, parse) is
/// exercised deterministically, fast, without a cold compile. The real
/// toolchain is the authority in production; here the authority is the parser,
/// which is tested against known output.
fn tiny_project(dir: &std::path::Path, result_line: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "// original\npub fn f() -> i32 { 1 }\n").unwrap();
    // Build output that must NOT be copied — if it were, the copy would be huge
    // and slow. A marker file proves it was skipped.
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("target/marker"), "should not be copied").unwrap();
    let script = format!("#!/bin/sh\necho \"{result_line}\"\n");
    let sh = dir.join("run.sh");
    std::fs::write(&sh, script).unwrap();
    std::fs::set_permissions(&sh, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn scratch(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("atlas-iv-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn a_change_that_replaces_a_real_file_and_keeps_the_suite_green_is_verified() {
    let work = scratch("pass");
    let root = work.join("proj");
    tiny_project(&root, "test result: ok. 3 passed; 0 failed; 0 ignored");

    let edits = vec![Edit {
        path: "src/lib.rs".into(),
        content: "// changed\npub fn f() -> i32 { 2 }\n".into(),
        reason: String::new(),
    }];
    let proof = prove_in_project(&root, "./run.sh", &edits, &work.join("base")).unwrap();

    assert!(proof.built_and_passed, "the stub reported all passing: {}", proof.output);
    assert!(proof.replaced_existing, "src/lib.rs already existed, so this is a real replacement");
    assert_eq!(proof.tests_run, 3);
    assert!(proof.plain("Homelab").contains("Verified inside Homelab"), "{}", proof.plain("Homelab"));

    // The project itself was never touched — the change is only in the copy.
    let still = std::fs::read_to_string(root.join("src/lib.rs")).unwrap();
    assert!(still.contains("// original"), "the real project must be untouched");
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_failing_suite_is_not_a_pass() {
    let work = scratch("fail");
    let root = work.join("proj");
    tiny_project(&root, "test result: FAILED. 2 passed; 1 failed; 0 ignored");

    let edits = vec![Edit { path: "src/lib.rs".into(), content: "broken".into(), reason: String::new() }];
    let proof = prove_in_project(&root, "./run.sh", &edits, &work.join("base")).unwrap();

    assert!(!proof.built_and_passed, "a failed test is not a pass");
    assert!(proof.plain("Homelab").contains("doesn't hold up"), "{}", proof.plain("Homelab"));
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_brand_new_file_is_not_claimed_as_exercised() {
    // The honesty guard: a new, unreferenced file can leave the suite green
    // while proving nothing about the new code. built_and_passed can be true,
    // but replaced_existing is false and the wording must say so.
    let work = scratch("newfile");
    let root = work.join("proj");
    tiny_project(&root, "test result: ok. 1 passed; 0 failed");

    let edits = vec![Edit {
        path: "src/brand_new.rs".into(),
        content: "pub fn g() {}\n".into(),
        reason: String::new(),
    }];
    let proof = prove_in_project(&root, "./run.sh", &edits, &work.join("base")).unwrap();

    assert!(!proof.replaced_existing, "src/brand_new.rs did not exist before");
    let said = proof.plain("Homelab");
    assert!(!said.contains("Verified inside"), "must not claim verification of unexercised code: {said}");
    assert!(said.contains("doesn't exercise it") || said.contains("Read it"), "{said}");
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn build_output_is_not_copied_into_the_work_area() {
    // A stub that fails if target/ came across, proving the skip. The script
    // checks for the marker and reports failure if it's present.
    let work = scratch("skip");
    let root = work.join("proj");
    tiny_project(&root, "unused");
    // Overwrite run.sh to report based on whether target/ was copied.
    use std::os::unix::fs::PermissionsExt;
    let script = "#!/bin/sh\nif [ -e target/marker ]; then echo \"test result: FAILED. 0 passed; 1 failed\"; else echo \"test result: ok. 1 passed; 0 failed\"; fi\n";
    std::fs::write(root.join("run.sh"), script).unwrap();
    std::fs::set_permissions(root.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();

    let edits = vec![Edit { path: "src/lib.rs".into(), content: "// x\n".into(), reason: String::new() }];
    let proof = prove_in_project(&root, "./run.sh", &edits, &work.join("base")).unwrap();
    assert!(proof.built_and_passed, "target/ should have been skipped, so the run sees no marker");
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn a_missing_project_or_empty_command_is_refused() {
    let work = scratch("bad");
    assert!(prove_in_project(&work.join("nope"), "./run.sh", &[], &work.join("b")).is_err());
    let root = work.join("proj");
    tiny_project(&root, "test result: ok. 1 passed");
    assert!(prove_in_project(&root, "   ", &[], &work.join("b")).is_err());
    let _ = std::fs::remove_dir_all(&work);
}
