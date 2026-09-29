//! SHA-256 and the date format, against answers Atlas didn't compute.
//!
//! Every expected value here came from independent tooling — Python's
//! `hashlib` and `datetime` — and was pasted in. That is the only
//! thing that makes this file worth anything: a hash tested against itself
//! proves it is consistent, which is exactly what a wrong hash also is.
//!
//! The awkward inputs are deliberate. A message that lands exactly on the
//! padding boundary, a leap day, and a century that is not a leap year are
//! where a hand-written implementation goes wrong, and they are the three
//! cases nobody writes a test for.

use atlas::digest::{iso_utc, sha256_hex};

#[test]
fn the_known_answers_are_the_known_answers() {
    for (input, expected) in [
        ("", "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"),
        ("abc", "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"),
        ("hello world", "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"),
        (
            "The quick brown fox jumps over the lazy dog",
            "d7a8fbb307d7809469ca9abcb0082e4f8d5651e46d3cdb762d02d0bf37c9e592",
        ),
    ] {
        assert_eq!(sha256_hex(input.as_bytes()), expected, "on {input:?}");
    }
}

#[test]
fn the_padding_boundary_is_handled_in_both_directions() {
    // Fifty-five bytes is the last length that fits in one block with its
    // length field; fifty-six needs a second block for the length alone. Get
    // this wrong and almost every short message still hashes correctly, which
    // is what makes it worth a test of its own.
    for (n, expected) in [
        (55usize, "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318"),
        (56, "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a"),
        (63, "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34"),
        (64, "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb"),
        (65, "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0"),
        (119, "31eba51c313a5c08226adf18d4a359cfdfd8d2e816b13f4af952f7ea6584dcfb"),
        (120, "2f3d335432c70b580af0e8e1b3674a7c020d683aa5f73aaaedfdc55af904c21c"),
        (1000, "41edece42d63e8d9bf515a9ba6932e1c20cbc9f5a5d134645adb5db1b9737ea3"),
    ] {
        let input = "a".repeat(n);
        assert_eq!(sha256_hex(input.as_bytes()), expected, "on {n} bytes");
    }
}

#[test]
fn a_hash_is_sixty_four_lowercase_hex_characters() {
    let h = sha256_hex(b"anything");
    assert_eq!(h.len(), 64);
    assert!(h.chars().all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)), "{h}");
}

// ---------------------------------------------------------------------------
// Time, written the one way both sides read it
// ---------------------------------------------------------------------------

#[test]
fn a_time_is_written_the_way_python_writes_it() {
    for (seconds, expected) in [
        (0u64, "1970-01-01T00:00:00+00:00"),
        (1, "1970-01-01T00:00:01+00:00"),
        (86_399, "1970-01-01T23:59:59+00:00"),
        (86_400, "1970-01-02T00:00:00+00:00"),
        // A leap day in a century that IS a leap year.
        (951_782_400, "2000-02-29T00:00:00+00:00"),
        (1_767_600_000, "2026-01-05T08:00:00+00:00"),
        (1_767_686_399, "2026-01-06T07:59:59+00:00"),
        // A century that is NOT a leap year — the case a loop over years gets
        // wrong, and the reason the era arithmetic is used instead.
        (4_102_444_800, "2100-01-01T00:00:00+00:00"),
        (4_107_542_400, "2100-03-01T00:00:00+00:00"),
        (2_147_483_648, "2038-01-19T03:14:08+00:00"),
    ] {
        assert_eq!(iso_utc(seconds), expected, "at {seconds}");
    }
}

// ---------------------------------------------------------------------------
// The id neither side issues
// ---------------------------------------------------------------------------
