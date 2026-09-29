//! A phone mirror nothing writes must not report on a phone.
//!
//! `atlas mirror` loads the `phone_mirror` record. **Nothing in the tree ever
//! saves it, and `Phone::capture` has no production caller** — there is no
//! phone-side client to capture into it and no sync to write it.
//!
//! So `mirrored_at` was always `None`, and `Phone::state` answered *"Nothing
//! from the laptop yet."* — identical on a phone that has never synced and on
//! one that synced an hour ago. That sentence reads as a fact about the
//! phone and is a fact about an empty file.
//!
//! Everything else in `companion` is built and right: `capture` works with
//! the laptop off, `waiting` lists what has not landed, `merge` decides what
//! happens when the laptop comes back, and `state` attaches the right warning
//! to a stale snapshot. The missing half is the other end.
//!
//! Found by `tests/one_name_one_record.rs`, which noticed the record has a
//! reader and no writer. This file holds the caveat and the wiring together,
//! so whoever builds the phone side is told to delete the caveat rather than
//! leaving it to outlive its truth.

use atlas::companion::{unbuilt, CompanionConfig, Phone, Piece};

/// Every production reference that would fill the mirror.
fn production_writers() -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from("src")];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in std::fs::read_dir(&p).expect("readable").flatten() {
                stack.push(e.path());
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let live = match text.find("#[cfg(test)]") {
            Some(at) => &text[..at],
            None => &text[..],
        };
        for (i, line) in live.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            if line.contains("save(\"phone_mirror\"") || line.contains("phone.capture(") {
                out.push(format!("{}:{}", p.display(), i + 1));
            }
        }
    }
    out
}

#[test]
fn the_caveat_exists_exactly_while_nothing_fills_it() {
    let writers = production_writers();
    match unbuilt() {
        Some(_) => assert!(
            writers.is_empty(),
            "`companion::unbuilt()` still says nothing fills the mirror, and these \
             places now do:\n  {}\n\nDelete `unbuilt`'s body (return `None`).",
            writers.join("\n  ")
        ),
        None => assert!(
            !writers.is_empty(),
            "`companion::unbuilt()` says the mirror is being written and nothing \
             writes it, so `atlas mirror` reports on a file that will never exist"
        ),
    }
}

#[test]
fn an_unwritten_mirror_is_not_reported_as_an_empty_one() {
    let said = Phone::default().state(1_700_000_000, &CompanionConfig::default());
    if unbuilt().is_some() {
        assert!(
            !said.starts_with("Nothing from the laptop"),
            "an unwritten mirror is reported as a phone with nothing on it: {said:?}"
        );
        assert!(
            said.contains("can't tell") || said.contains("nothing writes"),
            "the report does not say the answer is unknown: {said:?}"
        );
    }
}

#[test]
fn a_mirror_that_has_been_filled_still_reports_normally() {
    // So the caveat cannot be read as "this module does not work". Given a
    // `mirrored_at`, every other arm is untouched.
    let mut p = Phone::default();
    p.mirrored_at = Some(1_700_000_000);
    let fresh = p.state(1_700_000_000, &CompanionConfig::default());
    assert!(fresh.contains("Up to date"), "a same-day mirror: {fresh:?}");

    let cfg = CompanionConfig::default();
    let stale = p.state(1_700_000_000 + 9 * 86_400, &cfg);
    assert!(stale.contains("snapshot"), "a nine-day-old mirror: {stale:?}");

    // Past `hold_days` (200 by default), the warning is the point.
    let ancient = p.state(1_700_000_000 + (cfg.hold_days as u64 + 5) * 86_400, &cfg);
    assert!(
        ancient.contains("some of it will be wrong"),
        "a mirror past hold_days ({}) does not warn: {ancient:?}",
        cfg.hold_days
    );
}

#[test]
fn capturing_still_works_for_whoever_builds_the_phone_side() {
    // The half that exists, checked so the caveat is about the wiring and
    // not about the mechanism.
    let mut p = Phone::default();
    let id = p.capture("ring the vet", Piece::Notes, 1_700_000_000);
    assert!(id > 0);
    assert_eq!(p.waiting().len(), 1, "a capture did not end up waiting");
}
