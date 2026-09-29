//! A level nothing emits is a level that does not exist.
//!
//! `log::warn` sat in `ORPHANS` with that sentence as its reason, and the
//! reason was right. `Log` offered `info` and `warn`; everything in the tree
//! called `info`. So the log had one level wearing two names, and the WARN
//! branch was a promise to the reader that some lines mattered more.
//!
//! What made it worth fixing rather than deleting is what was *in* the log at
//! INFO. "Refused while handed over" — Atlas working exactly as designed —
//! sat at the same level as "couldn't save a handoff", which is somebody's
//! file not being where they were told it is. **A log where a routine refusal
//! and a lost file look identical is a log you cannot skim**, which means it
//! is a log nobody reads, which is the same as not having one.
//!
//! So the rule this file holds: a line Atlas writes because something it
//! tried to do **failed** is a warning; a line about something it decided
//! **not** to do is information.

use atlas::log::Log;

fn tmp(tag: &str) -> std::path::PathBuf {
    let p = std::env::temp_dir().join(format!("atlas-log-levels-{tag}"));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    p
}

#[test]
fn the_two_levels_are_distinguishable_in_the_file() {
    // The narrow claim first: a reader grepping for WARN finds the warnings
    // and not the rest. If both levels rendered the same, everything below is
    // decoration.
    let dir = tmp("levels");
    let log = Log::new(&dir, 1024 * 1024);
    log.info("refused while handed over: mail");
    log.warn("couldn't save a handoff from Sam: disk full");
    let text = std::fs::read_to_string(dir.join("atlas.log")).expect("the log file");

    let warns: Vec<&str> = text.lines().filter(|l| l.contains(" WARN ")).collect();
    let infos: Vec<&str> = text.lines().filter(|l| l.contains(" INFO ")).collect();
    assert_eq!(warns.len(), 1, "grepping for WARN does not isolate the warning:\n{text}");
    assert_eq!(infos.len(), 1, "{text}");
    assert!(warns[0].contains("handoff"), "{}", warns[0]);
    assert!(infos[0].contains("refused"), "{}", infos[0]);
}

#[test]
fn failures_are_warnings_and_refusals_are_not() {
    // Read out of `daemon.rs` itself, because the rule is only worth having
    // if it holds at the call sites. This is the same shape as the other
    // text-reading guards in this suite: the property lives in the source,
    // so the source is what gets checked.
    let src = crate::common::source_of("daemon");

    let mut wrong: Vec<String> = Vec::new();
    for line in src.lines() {
        let t = line.trim();
        let Some(rest) = t.strip_prefix("self.log.info(") else { continue };
        let lower = rest.to_lowercase();
        // "couldn't", "could not", "failed" — Atlas tried and did not manage.
        // Anything matching that is a warning, whatever else is on the line.
        if lower.contains("couldn't")
            || lower.contains("could not")
            || lower.contains("failed")
            || lower.contains("unable to")
        {
            wrong.push(t.to_string());
        }
    }
    assert!(
        wrong.is_empty(),
        "these report a failure at INFO, which puts them level with \
         \"refused while handed over\":\n  {}\n\nUse `self.log.warn` — a log \
         where a lost file and a routine refusal look the same is one nobody \
         skims.",
        wrong.join("\n  ")
    );
}

#[test]
fn warn_is_actually_emitted_somewhere() {
    // The other direction, and the one the ORPHANS entry was about. Without
    // this, deleting every `warn` call would leave the test above passing
    // happily — there would be no failures logged at INFO, because there
    // would be no failures logged at all.
    let src = crate::common::source_of("daemon");
    let n = src.matches("self.log.warn(").count();
    assert!(
        n >= 3,
        "only {n} warning(s) in the daemon. `log::warn` spent weeks in ORPHANS \
         for exactly this reason: a level nothing emits is a level that does \
         not exist."
    );
}
