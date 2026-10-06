//! Every reader of outside input survives garbage (5 Oct 2026, audit Q20).
//!
//! A calendar invite, a feed, a contact card, a model file, a web page, a PDF,
//! an HTTP reply, a pairing code: each arrives from somewhere Atlas doesn't
//! control, and a panic while reading one stops the part of Atlas that was
//! reading it. Real fuzzing (`cargo fuzz`) needs the nightly compiler, which
//! the pinned toolchain (rust-toolchain.toml) rules out, so this is the
//! stable form of it: a seeded generator throws thousands of mutated and
//! random inputs at each reader and the only thing asserted is that none of
//! them panics. An error back is fine; a panic is the bug.
//!
//! Seeded, so a failure names the input that caused it and reproduces.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// xorshift64*: small, fast, and the same sequence everywhere.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// A variation on `seed`: bytes flipped, dropped, repeated, spliced with
/// pieces of `alphabet`, or cut short -- the shapes damaged input comes in.
fn mutate(r: &mut Rng, seed: &[u8], alphabet: &[&[u8]]) -> Vec<u8> {
    let mut v = seed.to_vec();
    for _ in 0..1 + r.below(6) {
        match r.below(6) {
            0 if !v.is_empty() => {
                let i = r.below(v.len());
                v[i] ^= 1 << r.below(8);
            }
            1 if !v.is_empty() => {
                let i = r.below(v.len());
                let n = r.below(16).min(v.len() - i);
                v.drain(i..i + n);
            }
            2 if !v.is_empty() => {
                let i = r.below(v.len());
                let n = r.below(32).min(v.len() - i);
                let piece = v[i..i + n].to_vec();
                for _ in 0..r.below(50) {
                    v.splice(i..i, piece.iter().copied());
                }
            }
            3 => {
                let i = r.below(v.len() + 1);
                let token = alphabet[r.below(alphabet.len())];
                v.splice(i..i, token.iter().copied());
            }
            4 => v.truncate(r.below(v.len() + 1)),
            _ => {
                let i = r.below(v.len() + 1);
                let n = r.below(8);
                let noise: Vec<u8> = (0..n).map(|_| r.next() as u8).collect();
                v.splice(i..i, noise);
            }
        }
    }
    v
}

/// Run `read` on `rounds` inputs made from `seeds`; fail naming the first
/// input that panicked.
fn survives(name: &str, rounds: usize, seeds: &[&[u8]], alphabet: &[&[u8]], read: impl Fn(&[u8])) {
    // `ATLAS_FUZZ_ROUNDS=50000` for a deep run; CI uses the default.
    let rounds = std::env::var("ATLAS_FUZZ_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(rounds);
    let mut r = Rng(0x9E37_79B9_7F4A_7C15 ^ name.len() as u64);
    // The seeds themselves, empty input, and pure noise first.
    let mut inputs: Vec<Vec<u8>> = seeds.iter().map(|s| s.to_vec()).collect();
    inputs.push(Vec::new());
    for _ in 0..rounds {
        let input = if r.below(10) == 0 {
            (0..r.below(512)).map(|_| r.next() as u8).collect()
        } else {
            let seed = seeds[r.below(seeds.len())];
            mutate(&mut r, seed, alphabet)
        };
        inputs.push(input);
    }
    let mut failed = None;
    for input in &inputs {
        if let Err(p) = catch_unwind(AssertUnwindSafe(|| read(input))) {
            let why = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            failed = Some((input.clone(), why));
            break;
        }
    }
    if let Some((input, why)) = failed {
        panic!("{name} panicked ({why}) on {} bytes: {:?}", input.len(), String::from_utf8_lossy(&input));
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

const ROUNDS: usize = 3000;

#[test]
fn a_calendar_repeat_rule() {
    survives(
        "recur::Rule::parse",
        ROUNDS,
        &[b"FREQ=WEEKLY;INTERVAL=2;BYDAY=TU,TH;COUNT=10", b"FREQ=MONTHLY;BYMONTHDAY=-1;UNTIL=20271231T235959Z"],
        &[b";", b"=", b"FREQ=", b"INTERVAL=", b"BYDAY=", b"COUNT=", b"UNTIL=", b"-", b"99999999999999999999", b"0", b",", b"MO"],
        |b| {
            let _ = atlas::recur::Rule::parse(&text(b));
        },
    );
    survives(
        "recur::parse_ical_time",
        ROUNDS,
        &[b"20261005T093000Z", b"20261005", b"TZID=America/New_York:20261101T013000"],
        &[b"T", b"Z", b":", b"9999", b"00", b"-"],
        |b| {
            let _ = atlas::recur::parse_ical_time(&text(b));
        },
    );
}

#[test]
fn a_feed() {
    survives(
        "feeds::parse",
        ROUNDS,
        &[
            b"<?xml version='1.0'?><rss><channel><title>t</title><item><title>a</title><link>http://x/1</link><pubDate>Mon, 05 Oct 2026 09:00:00 GMT</pubDate></item></channel></rss>",
            b"<feed xmlns='http://www.w3.org/2005/Atom'><entry><title>a</title><link href='http://x/2' rel='alternate'/><updated>2026-10-05T09:00:00Z</updated></entry></feed>",
            b"<rdf:RDF><item rdf:about='http://x/3'><title>b</title></item></rdf:RDF>",
        ],
        &[b"<", b">", b"</item>", b"<item>", b"&amp;", b"&#", b"<![CDATA[", b"]]>", b"'", b"\""],
        |b| {
            let _ = atlas::feeds::parse(&text(b));
        },
    );
    survives(
        "feeds::parse_date",
        ROUNDS,
        &[b"Mon, 05 Oct 2026 09:00:00 GMT", b"2026-10-05T09:00:00+02:00"],
        &[b":", b"+", b"-", b"99", b"GMT", b" "],
        |b| {
            let _ = atlas::feeds::parse_date(&text(b));
        },
    );
}

#[test]
fn a_contact_card_or_invite() {
    survives(
        "vformat::parse",
        ROUNDS,
        &[
            b"BEGIN:VCARD\r\nVERSION:3.0\r\nFN:Ada Lovelace\r\nEMAIL;TYPE=work:ada@example.com\r\nEND:VCARD\r\n",
            b"BEGIN:VCALENDAR\r\nBEGIN:VEVENT\r\nDTSTART:20261005T090000Z\r\nSUMMARY:Stand-up\r\n with a folded line\r\nEND:VEVENT\r\nEND:VCALENDAR\r\n",
        ],
        &[b"\r\n", b"\r\n ", b"BEGIN:", b"END:", b":", b";", b"=", b"\\n", b"\t"],
        |b| {
            let _ = atlas::vformat::parse(&text(b));
        },
    );
}

#[test]
fn a_3d_model() {
    survives(
        "meshio::read_obj",
        ROUNDS,
        &[b"v 0 0 0\nv 1 0 0\nv 0 1 0\nvn 0 0 1\nf 1//1 2//1 3//1\nf 1 2 3 4\n"],
        &[b"v ", b"vn ", b"f ", b"/", b"//", b"-1", b"999999", b"nan", b"\n", b"usemtl x\n"],
        |b| {
            let _ = atlas::meshio::read_obj(&text(b), None);
        },
    );
    let mut stl = vec![0u8; 84 + 50];
    stl[80] = 1;
    survives("meshio::read_stl", ROUNDS, &[&stl, b"solid x\nfacet normal 0 0 1\nouter loop\nvertex 0 0 0\nendloop\nendfacet\nendsolid"], &[b"\xff\xff\xff\xff", b"facet", b"vertex"], |b| {
        let _ = atlas::meshio::read_stl(b);
    });
    survives(
        "meshio::read_gltf",
        ROUNDS,
        &[b"{\"asset\":{\"version\":\"2.0\"},\"meshes\":[{\"primitives\":[{\"attributes\":{\"POSITION\":0}}]}],\"accessors\":[{\"bufferView\":0,\"count\":3,\"type\":\"VEC3\",\"componentType\":5126}],\"bufferViews\":[{\"buffer\":0,\"byteLength\":36}],\"buffers\":[{\"byteLength\":36}]}", b"glTF\x02\x00\x00\x00"],
        &[b"{", b"}", b"[", b"]", b"\"count\":4294967295", b"\"byteOffset\":-1", b"null"],
        |b| {
            let _ = atlas::meshio::read_gltf(b, None);
        },
    );
}

#[test]
fn a_reply_off_the_network() {
    survives(
        "http::parse_response",
        ROUNDS,
        &[
            b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello",
            b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        ],
        &[b"\r\n", b"Content-Length: 99999999999999999999\r\n", b"ffffffffffffffff\r\n", b"Transfer-Encoding: chunked\r\n", b":"],
        |b| {
            let _ = atlas::http::parse_response(b);
        },
    );
    survives(
        "imap::list_entry",
        ROUNDS,
        &[b"* LIST (\\HasNoChildren) \"/\" \"INBOX\"", b"* LIST (\\Noselect) \".\" Archive"],
        &[b"(", b")", b"\"", b"\\", b"{12}"],
        |b| {
            let _ = atlas::imap::list_entry(&text(b));
        },
    );
}

#[test]
fn a_page_or_a_document() {
    survives(
        "readable::extract",
        ROUNDS,
        &[b"<html><head><title>t</title></head><body><article><h1>A</h1><p>One <b>two</b></p><pre>x</pre></article></body></html>"],
        &[b"<", b">", b"</p>", b"<div>", b"</div>", b"<pre>", b"&nbsp;", b"<!--", b"-->", b"<script>"],
        |b| {
            let _ = atlas::readable::extract(&text(b));
        },
    );
    survives(
        "pdftext::read",
        1000,
        &[b"%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n2 0 obj << /Type /Pages /Kids [] /Count 0 >> endobj\ntrailer << /Root 1 0 R >>\n%%EOF"],
        &[b"obj", b"endobj", b"stream\n", b"endstream", b"/Length 99999999", b"/FlateDecode", b"<<", b">>", b"0 R"],
        |b| {
            let _ = atlas::pdftext::read(b);
        },
    );
}

#[test]
fn an_address_or_a_code() {
    survives(
        "mailbook::mail_addresses",
        ROUNDS,
        &[b"\"Lovelace, Ada\" <ada@example.com>, bob@example.org (Bob)"],
        &[b"<", b">", b",", b"\"", b"@", b"(", b")", b"=?utf-8?q?", b"?="],
        |b| {
            let _ = atlas::mailbook::mail_addresses(&text(b));
        },
    );
    survives("mailbook::excerpt", ROUNDS, &[b"Hi,\n\n> quoted\nOn Mon someone wrote:\n-- \nsig"], &[b"\n", b">", b"-- \n", b"\r"], |b| {
        let _ = atlas::mailbook::excerpt(&text(b));
    });
    survives(
        "household::decode_pairing",
        ROUNDS,
        &[b"ATLAS1-abcdefghjkmnpqrstuvwxyz23456789", b"K7Q2M9X4TP"],
        &[b"-", b"=", b"+", b"/", b"ATLAS1-", b"\xc3\xa9"],
        |b| {
            let _ = atlas::household::decode_pairing(&text(b));
        },
    );
}

#[test]
fn the_harness_does_catch_a_panic() {
    let caught = catch_unwind(|| {
        survives("a reader that panics on 'x'", 200, &[b"abc"], &[b"x"], |b| {
            assert!(!b.contains(&b'x'), "x");
        })
    });
    assert!(caught.is_err(), "a panicking reader must fail the test");
}
