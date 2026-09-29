//! The chunk decoder was handed a string that had already been mangled, and
//! the read that produced it had no limit.
//!
//! ## The ordering
//!
//! `dechunk`'s first comment says:
//!
//! > Bytes, not characters. A chunk size on the wire counts bytes … Slicing a
//! > `&str` at that offset panics … Reassembling first and decoding once at
//! > the end is also the only way to get the right answer.
//!
//! All true, and all of it was defeated one level up. `with_header` did
//! `parse_response(&String::from_utf8_lossy(&raw))` — so every byte that is
//! not valid UTF-8 became U+FFFD, **three bytes where one arrived**, before
//! `dechunk` ever saw it. From the first such byte onward every chunk-size
//! header pointed at the wrong offset, and the decoder read sizes out of the
//! middle of the data.
//!
//! Atlas speaks HTTP to Chrome's debug port (which returns screenshot
//! payloads), to another Atlas over a pairing (which carries handed-over
//! files), and to a phone. Non-UTF-8 bytes are not the exotic case there.
//!
//! ## The limit
//!
//! `s.read_to_end(&mut raw)` had none. The read timeout does not bound the
//! total — a server sending a little every second resets it on every read —
//! so the buffer grows until the process dies.

use atlas::http::{dechunk_for_test, parse_response, MAX_RESPONSE};

/// A chunked body whose first chunk holds a byte that is not valid UTF-8.
///
/// 0x80 is a continuation byte with nothing in front of it: `from_utf8_lossy`
/// turns it into U+FFFD, which is `EF BF BD` — one byte becomes three.
fn a_body_with_a_bad_byte() -> Vec<u8> {
    let mut raw = Vec::new();
    raw.extend_from_slice(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
    // First chunk: five bytes, one of them invalid.
    raw.extend_from_slice(b"5\r\n");
    raw.extend_from_slice(&[b'a', b'b', 0x80, b'c', b'd']);
    raw.extend_from_slice(b"\r\n");
    // Second chunk: plain text, and the one that proves the offsets held.
    // 13 bytes, hex d — "the real part" counted on the wire, not in characters.
    raw.extend_from_slice(b"d\r\nthe real part\r\n");
    raw.extend_from_slice(b"0\r\n\r\n");
    raw
}

#[test]
fn a_chunked_reply_with_an_undecodable_byte_still_yields_the_rest() {
    let r = parse_response(&a_body_with_a_bad_byte()).expect("parse");
    assert_eq!(r.status, 200);
    assert!(
        r.body.ends_with("the real part"),
        "the chunk after the bad byte was lost, which is what happens when the lossy \
         conversion runs before the offsets are used: {:?}",
        r.body
    );
    assert!(r.body.starts_with("ab"), "the good bytes of the first chunk are gone: {:?}", r.body);
    assert!(
        r.body.contains('\u{FFFD}'),
        "the undecodable byte vanished rather than being marked: {:?}",
        r.body
    );
}

#[test]
fn converting_first_is_what_broke_it() {
    // The control that gives the test above its meaning. Doing it the old way
    // — decode the whole response, then hand the text to the decoder — loses
    // the second chunk, because the first chunk is now seven bytes long where
    // its header says five.
    let raw = a_body_with_a_bad_byte();
    // The old shape, reproduced by hand: decode the whole response, then
    // parse the text. There is no longer a `&str` entry point to do it by
    // accident — that is the point — so the test does it deliberately.
    let the_old_way =
        parse_response(String::from_utf8_lossy(&raw).as_bytes()).expect("parse");
    let now = parse_response(&raw).expect("parse");
    assert_ne!(
        the_old_way.body, now.body,
        "decoding before dechunking makes no difference to this body, so it is not the \
         body the bug needs and this file proves nothing"
    );
    assert!(
        !the_old_way.body.ends_with("the real part"),
        "the old ordering was supposed to lose the tail here"
    );
}

#[test]
fn an_ordinary_reply_is_unchanged() {
    // So none of this is satisfied by mangling the common case.
    let raw = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\r\n{\"ok\":true}";
    let r = parse_response(raw).expect("parse");
    assert_eq!(r.status, 200);
    assert_eq!(r.body, "{\"ok\":true}");

    let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n4\r\nabcd\r\n3\r\nefg\r\n0\r\n\r\n";
    assert_eq!(parse_response(chunked).expect("parse").body, "abcdefg");
}

#[test]
fn a_character_split_across_two_chunks_is_reassembled() {
    // The case `dechunk`'s doc is about, checked from the socket end rather
    // than from a string: a server splits wherever its buffer ran out, which
    // is routinely in the middle of a character.
    let mut raw = Vec::new();
    raw.extend_from_slice(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n");
    // "café" is 5 bytes; the é (C3 A9) is split between the two chunks.
    raw.extend_from_slice(b"4\r\n");
    raw.extend_from_slice(&[b'c', b'a', b'f', 0xC3]);
    raw.extend_from_slice(b"\r\n1\r\n");
    raw.extend_from_slice(&[0xA9]);
    raw.extend_from_slice(b"\r\n0\r\n\r\n");

    assert_eq!(
        parse_response(&raw).expect("parse").body,
        "café",
        "a character split across a chunk boundary did not come back whole"
    );
}

#[test]
fn there_is_exactly_one_way_in() {
    // The `&str` sibling is gone rather than kept. It is what the bug was
    // made of: `with_header` called it as
    // `parse_response(&String::from_utf8_lossy(&raw))`, so keeping it "for
    // callers that already have text" would leave that mistake one call
    // away — and its only remaining callers were tests, which `http.rs`'s
    // own comment says is not a reason to keep a function.
    let r = parse_response(b"HTTP/1.1 404 Not Found\r\n\r\nnope").expect("parse");
    assert_eq!(r.status, 404);
    assert_eq!(r.body, "nope");
    assert_eq!(dechunk_for_test("4\r\nabcd\r\n0\r\n\r\n"), "abcd");
}

#[test]
fn a_malformed_reply_is_refused_rather_than_guessed_at() {
    assert!(parse_response(b"not http at all").is_err());
    assert!(parse_response(b"HTTP/1.1\r\n\r\nbody").is_err(), "no status code");
}

#[test]
fn there_is_a_ceiling_on_what_one_reply_can_hold() {
    // A number rather than nothing. The read timeout does not bound the
    // total: a server sending a little every second resets it on every read,
    // and `read_to_end` grows the buffer until the process dies.
    //
    // Generous rather than tight, because Chrome's debug port returns
    // screenshot payloads on this path — 64 MiB is the figure `imap` and
    // `smtp` settled on for the same reason.
    assert_eq!(MAX_RESPONSE, 64 * 1024 * 1024);
    assert!(
        MAX_RESPONSE >= 16 * 1024 * 1024,
        "a screenshot from Chrome's debug port would not fit under {MAX_RESPONSE}"
    );
}

// ===================== the crash note ===================================

#[test]
fn a_caught_panic_says_whether_there_is_actually_a_note() {
    // `crash::caught` said *"I've written down what happened and carried
    // on."* — a claim about a file — whether or not the hook had run. The
    // flag that tells those apart was set by the hook, stored `false` by
    // `caught`, and never read.
    //
    // Without `crash::watch` having been called there is no hook, so no note
    // is written, and the sentence has to say so.
    let err = atlas::crash::caught("testing the note", || panic!("deliberate"))
        .expect_err("a panic came back as success");
    assert!(err.contains("testing the note"), "it did not say what it was doing: {err}");
    assert!(
        err.contains("no crash note") || err.contains("written down what happened"),
        "it said neither that there is a note nor that there isn't: {err}"
    );
}

#[test]
fn work_that_does_not_panic_is_returned_untouched() {
    // The control. `caught` is a wrapper, not a change of behaviour.
    assert_eq!(atlas::crash::caught("adding up", || 2 + 2), Ok(4));
}
