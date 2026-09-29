//! Atlas does not fall over on a word with an accent in it.
//!
//! ## What happened
//!
//! Two places in `prose.rs` assumed one character is one byte. Rust does not
//! let you take a byte range that ends inside a character — it panics — so
//! both were crashes, not wrong answers:
//!
//! * the sentence-capital fix recorded `len: 1` for a first character that
//!   might be two, three or four bytes wide;
//! * the doubled-word fix computed "one past the previous character" as
//!   `rfind(..) + 1`.
//!
//! **And these are not turn-level failures.** `crash::caught` wraps `tick`;
//! the conversation path is not wrapped, so the panic unwound out of the run
//! loop and ended the process. Saying "review this post école starts today"
//! to Atlas closed Atlas.
//!
//! ## Why a whole file for it
//!
//! The defect is a class, not two lines. Anywhere Atlas does byte arithmetic
//! on something a person said, the same mistake produces the same crash, and
//! the input that finds it is ordinary: a French café, a German name, an
//! emoji in a message. These drive the real public entry points rather than
//! the two fixed lines, so a new instance anywhere in `check`/`apply_certain`
//! fails here too.

use atlas::prose::{apply_certain, check, ProseConfig};

/// Text a person could plausibly say, none of it ASCII-only.
const AWKWARD: &[&str] = &[
    "école starts today",
    "über alles is the line",
    "ñandú runs fast",
    "ça va bien",
    "привет there",
    "日本語 is hard",
    "the café café is open",
    "größe größe again",
    "naïve naïve twice",
    "🙂 starts the sentence",
    "é",
    "é é",
    "  é é  ",
    "ÉCOLE shouting",
    "a é b é c",
];

#[test]
fn checking_awkward_text_does_not_panic() {
    let cfg = ProseConfig::default();
    for t in AWKWARD {
        let fixes = check(t, &cfg);
        // Every fix must describe a range that actually exists in the text,
        // on both ends. This is the invariant the crash was a violation of,
        // asserted directly so a bad `len` is caught here rather than in
        // whichever caller applies it first.
        for f in &fixes {
            assert!(
                t.is_char_boundary(f.at),
                "{t:?}: fix starts at {} which is mid-character ({f:?})",
                f.at
            );
            assert!(
                f.at + f.len <= t.len() && t.is_char_boundary(f.at + f.len),
                "{t:?}: fix ends at {} which is mid-character or past the end ({f:?})",
                f.at + f.len
            );
        }
    }
}

#[test]
fn applying_fixes_to_awkward_text_does_not_panic() {
    let cfg = ProseConfig::default();
    for t in AWKWARD {
        let fixes = check(t, &cfg);
        let (out, _) = apply_certain(t, &fixes);
        // And the result is still text. A byte-sliced String that survived by
        // luck would show up here.
        assert!(out.chars().count() > 0 || t.trim().is_empty(), "{t:?} became {out:?}");
    }
}

#[test]
fn the_sentence_capital_is_the_right_character_and_not_the_first_byte() {
    // The behaviour, not just the absence of a panic: it must capitalise the
    // accented letter itself rather than mangling it.
    let cfg = ProseConfig::default();
    let (out, n) = {
        let t = "école starts today";
        let f = check(t, &cfg);
        apply_certain(t, &f)
    };
    assert!(n >= 1, "nothing was corrected at all: {out:?}");
    assert!(
        out.starts_with('É'),
        "the first letter was not capitalised as one character: {out:?}"
    );
    assert!(out.contains("cole starts today"), "the rest of the word was damaged: {out:?}");
}

#[test]
fn a_doubled_accented_word_loses_exactly_one_copy() {
    let cfg = ProseConfig::default();
    let t = "the café café is open";
    let fixes = check(t, &cfg);
    let (out, _) = apply_certain(t, &fixes);
    assert_eq!(
        out.matches("café").count(),
        1,
        "the repeat was not removed cleanly: {out:?}"
    );
    assert!(out.contains("is open"), "the tail was damaged: {out:?}");
}

#[test]
fn ascii_behaviour_is_unchanged() {
    // The fix must not have bought safety by changing what Atlas does to the
    // ordinary case, which is the overwhelming majority of what it sees.
    let cfg = ProseConfig::default();
    // "the the cat sat" -> "The cat sat": the repeat goes AND the sentence
    // capital is applied, so the survivor is "The". Compared case-insensitively
    // for that reason -- an earlier version of this assertion counted "the"
    // and failed on the capital, which is the test being wrong about the code
    // rather than the other way round.
    let t = "the the cat sat";
    let fixes = check(t, &cfg);
    let (out, n) = apply_certain(t, &fixes);
    assert_eq!(
        out.to_lowercase().matches("the").count(),
        1,
        "the doubled word was not reduced to one: {out:?}"
    );
    assert_eq!(out, "The cat sat", "{out:?}");
    assert!(n >= 1);

    let t2 = "hello there";
    let f2 = check(t2, &cfg);
    let (out2, _) = apply_certain(t2, &f2);
    assert!(out2.starts_with('H'), "{out2:?}");
}
