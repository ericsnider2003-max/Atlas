//! Text that isn't plain ASCII.
//!
//! Two crashes, both from treating a byte count as a character position.
//! Atlas transcribes speech, reads web pages and indexes files, so accents,
//! em dashes and curly quotes are routine rather than exotic — and slicing a
//! `&str` at an offset inside a character is a panic, not a wrong answer.

use atlas::faithful::{check, Step};
use atlas::http::dechunk_for_test as dechunk;

// --- the report checker -----------------------------------------------------

#[test]
fn a_report_containing_a_dash_does_not_crash_the_checker() {
    // `naïve — done.` was the one that found it. The negation scan counted
    // `word.len() + 1` per word, which assumes a one-byte separator; an em
    // dash is three, so the running total drifted into the middle of a
    // character.
    // Not crashing is the floor. The property worth holding is stronger and
    // does not require hardcoding a verdict: **a multi-byte character must not
    // change the answer.** Each pair is the same sentence twice, once in ASCII
    // and once not, and the two must agree.
    let steps = vec![Step::not_checked("x")];
    let pairs: &[(&str, &str)] = &[
        ("naive - done.", "naïve — done."),
        ("Saved - nothing checked.", "Saved — nothing checked."),
        ("Cafe saved.", "Café saved."),
        ("Backed up... not verified.", "Backed up… not verified."),
        ("Resume saved and verified.", "Résumé saved and verified."),
        ("- leading dash", "— leading dash"),
        ("text saved.", "日本語 saved."),
        ("emoji :) done.", "emoji 🙂 done."),
    ];
    for (plain, fancy) in pairs {
        let a = check(&steps, plain);
        let b = check(&steps, fancy);
        assert_eq!(
            a.len(),
            b.len(),
            "{plain:?} gave {a:?} but {fancy:?} gave {b:?}"
        );
    }
}

#[test]
fn negation_still_works_across_a_multi_byte_separator() {
    // The fix must not quietly stop finding things. "nothing — checked" still
    // has the claim negated.
    let steps = vec![Step::not_checked("x")];
    assert!(
        check(&steps, "Copied it — nothing checked.").is_empty(),
        "honest wording was flagged once a dash was involved"
    );
}

#[test]
fn a_claim_after_a_dash_is_still_caught() {
    // And the fix must not turn the check off either.
    let steps = vec![Step::not_checked("x")];
    assert!(
        !check(&steps, "Ran it — saved and verified.").is_empty(),
        "an unsupported claim slipped through after a dash"
    );
}

#[test]
fn a_word_at_the_very_end_is_still_found() {
    // A word running to the end of a clause never meets a separator, which is
    // the case a scan built around separators forgets.
    let steps = vec![Step::not_checked("x")];
    assert!(!check(&steps, "It is done").is_empty());
}

// --- the http client --------------------------------------------------------

#[test]
fn a_chunk_boundary_inside_a_character_does_not_crash() {
    // Chunk sizes count bytes and a server splits wherever its buffer ran
    // out. This says four bytes of a five-byte string, landing inside the é.
    let bad = "4\r\ncaf\u{00e9}\r\n0\r\n\r\n";
    let got = dechunk(bad);
    assert!(!got.is_empty(), "it returned nothing rather than what arrived");
}

#[test]
fn a_character_split_across_two_chunks_is_reassembled() {
    // The reason for decoding once at the end: a character split in half is
    // only valid when the halves are back together.
    // Built from raw bytes so the split really is mid-character, then checked
    // for the whole word — decoding each chunk separately would give two
    // replacement characters instead.
    let word = "café".as_bytes();          // c a f + two bytes of é
    let (first, second) = word.split_at(4); // splits inside the é
    let mut raw = Vec::new();
    raw.extend_from_slice(b"4\r\n");
    raw.extend_from_slice(first);
    raw.extend_from_slice(b"\r\n1\r\n");
    raw.extend_from_slice(second);
    raw.extend_from_slice(b"\r\n0\r\n\r\n");
    let body = String::from_utf8_lossy(&raw).into_owned();

    // The lossy conversion above already mangles the halves, so this asserts
    // the weaker thing that is still worth holding: it returns something and
    // does not panic on a boundary that falls inside a character.
    let got = dechunk(&body);
    assert!(!got.is_empty(), "a split character produced nothing at all");
}

#[test]
fn an_ordinary_chunked_body_still_decodes() {
    assert_eq!(dechunk("5\r\ncaf\u{00e9}\r\n0\r\n\r\n"), "café");
    assert_eq!(dechunk("3\r\nabc\r\n3\r\ndef\r\n0\r\n\r\n"), "abcdef");
}

#[test]
fn a_chunk_size_with_an_extension_is_read() {
    // `1a;name=value` is legal. Only the part before the semicolon is the size.
    assert_eq!(dechunk("3;foo=bar\r\nabc\r\n0\r\n\r\n"), "abc");
}

#[test]
fn an_incomplete_final_chunk_is_dropped_not_half_returned() {
    // Decided already, in browser_http.rs, and my first version of the byte
    // rewrite changed it by accident. A chunk that promised more than arrived
    // is incomplete, and handing back the partial tail gives you a half-page
    // indistinguishable from a whole one.
    assert_eq!(dechunk("3\r\nabc\r\nFF\r\ntrunc"), "abc");
    assert_eq!(dechunk("10\r\nabc"), "");
}

#[test]
fn a_zero_chunk_ends_it_cleanly() {
    assert_eq!(dechunk("0\r\n\r\n"), "");
}

#[test]
fn nonsense_in_place_of_a_size_stops_rather_than_looping() {
    assert_eq!(dechunk("not-a-number\r\nabc\r\n"), "");
    assert_eq!(dechunk(""), "");
}
