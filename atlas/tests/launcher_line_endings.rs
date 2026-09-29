//! A Windows batch file with Unix line endings can make `goto` and `call
//! :label` miss their labels — cmd.exe reads labels in 512-byte blocks and
//! gets them wrong without the carriage returns. ATLAS.bat jumps to `:menu`
//! and calls `:setup`, so it has to be CRLF (found 23 Sep 2026: it was LF).

#[test]
fn every_batch_file_is_crlf() {
    for p in ["ATLAS.bat", "setup/reference/RUN-TESTS.bat"] {
        let b = std::fs::read(p).unwrap();
        let lf = b.iter().filter(|&&c| c == b'\n').count();
        let crlf = b.windows(2).filter(|w| w == b"\r\n").count();
        assert!(lf > 0 && lf == crlf, "{p} has {lf} line ends, {crlf} of them CRLF");
    }
}

#[test]
fn a_fresh_copy_builds_its_own_exe_instead_of_stopping() {
    // The launcher used to stop at "Could not find atlas.exe" before its own
    // Build option could run, because cargo puts the exe in target\release.
    let b = std::fs::read_to_string("ATLAS.bat").unwrap();
    let not_found = b.find("Could not find atlas.exe").expect("the message is gone");
    let build = b.find("cargo build --release").expect("no build step");
    let copy = b.find(r#"copy /y "target\release\atlas.exe" "atlas.exe""#).expect("no copy beside the launcher");
    assert!(build < not_found && copy < not_found, "the build must be tried before giving up");
}
