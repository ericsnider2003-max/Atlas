//! The one sentence telling a person which half of a post Atlas checks named
//! the half it does not check.
//!
//! `opsec::NOT_ITS_BUSINESS` read:
//!
//! > I check what's in the frame, not what you're saying.
//!
//! `check` takes a `visible: &[(Risk, f32, String)]` — the frame half — and
//! **nothing in `src/` produces a `Risk`.** The only call site,
//! `daemon.rs`, passes `&[]`. So the loop under the comment *"Whatever was
//! actually seen in the frame"* has never run, and the half that does run is
//! the transcript scan: the words.
//!
//! The module's own doc opens with *"visible in the frame**, not** about what
//! you're allowed to think"*, and the reality is the reverse. That matters
//! more than a wrong comment usually does, because the sentence is a promise
//! about what Atlas reads.
//!
//! It also had no caller. A promise nobody reads is a promise nothing checks,
//! which is how it stayed wrong.
//!
//! `vision::Scene` exists and detects objects and faces, so the frame half is
//! buildable. What it needs is a ruling on which detector labels amount to
//! `Risk::Insignia` or `Risk::Documents`, not a wiring job — so until then
//! `frame_unchecked()` says so, out loud, alongside anything found.

use atlas::opsec::{check, frame_unchecked, spoken, OpsecConfig, NOT_ITS_BUSINESS};

fn cfg() -> OpsecConfig {
    OpsecConfig { enabled: true, applies_until: String::new(), ..Default::default() }
}

const TODAY: &str = "2026-09-18";

#[test]
fn the_caveat_exists_exactly_while_nothing_feeds_it_a_frame() {
    // Both directions, so the note is deleted by the change that makes it
    // false rather than surviving it — the shape `budget::untracked` uses.
    let producers = things_that_build_a_risk();
    match frame_unchecked() {
        Some(_) => assert!(
            producers.is_empty(),
            "`opsec::frame_unchecked()` still says nothing looks at the picture, and \
             these places now build a `Risk`:\n  {}\n\nDelete its body (return `None`) — \
             the caveat is the thing that is now false.",
            producers.join("\n  ")
        ),
        None => assert!(
            !producers.is_empty(),
            "`frame_unchecked()` says the frame is being checked and nothing in src/ \
             produces a `Risk`, so `check`'s `visible` list is still always empty and \
             the loop that reads it still never runs."
        ),
    }
}

/// Everywhere outside `opsec.rs` that constructs a `Risk`.
fn things_that_build_a_risk() -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![std::path::PathBuf::from("src")];
    while let Some(p) = stack.pop() {
        if p.is_dir() {
            for e in std::fs::read_dir(&p).expect("src is readable").flatten() {
                stack.push(e.path());
            }
            continue;
        }
        if p.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        if p.ends_with("opsec.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap_or_default();
        let live = match text.find("#[cfg(test)]") {
            Some(at) => &text[..at],
            None => &text[..],
        };
        for (i, line) in live.lines().enumerate() {
            let t = line.trim_start();
            if t.starts_with("//") || t.starts_with("///") {
                continue;
            }
            if line.contains("Risk::") || line.contains("opsec::Risk") {
                out.push(format!("{}:{}", p.display(), i + 1));
            }
        }
    }
    out
}

#[test]
fn the_sentence_no_longer_claims_the_half_it_does_not_do() {
    // The specific inversion.
    assert!(
        !NOT_ITS_BUSINESS.contains("not what you're saying"),
        "it still disclaims the only half it actually does: {NOT_ITS_BUSINESS}"
    );
    assert!(
        NOT_ITS_BUSINESS.contains("what you said") || NOT_ITS_BUSINESS.contains("you said"),
        "it does not say that it reads what you said: {NOT_ITS_BUSINESS}"
    );
    // And it still keeps the line that matters: opinions are not its business.
    assert!(
        NOT_ITS_BUSINESS.contains("opinions"),
        "the promise not to read your posts for opinions was dropped: {NOT_ITS_BUSINESS}"
    );
}

#[test]
fn a_finding_says_what_was_not_looked_at() {
    // A warning about a word, on its own, reads as "I looked and this is what
    // I found". The picture is the half the module is named for.
    let found = check("wheels up on the fifteenth", &[], &cfg(), TODAY);
    assert!(!found.is_empty(), "the transcript scan found nothing, so this proves nothing");

    let said = spoken(&found, &cfg(), TODAY);
    assert!(
        said.contains("haven't looked at the picture"),
        "a transcript-only warning is delivered as though the frame had been checked: \
         {said}"
    );
}

#[test]
fn saying_nothing_stays_saying_nothing() {
    // The caveat rides along with a finding. It does not become a message of
    // its own on every clean post, which is how a true note turns into noise
    // and gets switched off.
    let found = check("the coffee here is bad", &[], &cfg(), TODAY);
    assert_eq!(
        spoken(&found, &cfg(), TODAY),
        "",
        "it now says something about every post, including the ones with nothing in them"
    );
}

#[test]
fn nothing_is_said_at_all_once_the_date_has_passed() {
    // The control. `still_applies` gates the whole module, and the caveat
    // must not become a way past that — a person out from under the rules
    // should hear nothing, including notes about what was not checked.
    let expired = OpsecConfig { enabled: true, applies_until: "2020-01-01".into(), ..Default::default() };
    let found = check("wheels up on the fifteenth", &[], &expired, TODAY);
    assert!(found.is_empty());
    assert_eq!(spoken(&found, &expired, TODAY), "");
}

#[test]
fn the_frame_half_still_works_when_something_hands_it_one() {
    // So the fix is about the claim rather than about giving up on the
    // feature: the loop reads what it is given, and it is what a producer
    // would feed.
    use atlas::opsec::Risk;
    let seen = vec![
        (Risk::Insignia, 12.0f32, "a unit patch on the left shoulder".to_string()),
        (Risk::Location, 40.0f32, "a gate sign".to_string()),
    ];
    let found = check("nothing incriminating here", &seen, &cfg(), TODAY);
    assert_eq!(found.len(), 2, "the frame findings were dropped: {found:?}");
    // Heaviest first, and the insignia outweighs the location.
    assert_eq!(found[0].risk, Risk::Insignia);
    assert_eq!(found[0].at, Some(12.0));
}
