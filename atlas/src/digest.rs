//! SHA-256, written out in full, and the one date format Atlas prints.
//!
//! In-house rather than a dependency: the hash is small, fixed by the
//! standard, and checked here against the published test vectors, including
//! the awkward ones — a message that lands exactly on the padding boundary, a
//! leap day, a century that is not a leap year. Everything in Atlas that
//! fingerprints something (the tamper-evident log, releases, downloaded
//! pieces, plugins) uses this one copy.

// ---------------------------------------------------------------------------
// SHA-256
// ---------------------------------------------------------------------------

const K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// One 64-byte block into the running state — the whole of SHA-256 apart
/// from the padding, shared by the in-memory and the streamed forms so the
/// two can't drift.
fn compress(h: &mut [u32; 8], block: &[u8]) {
    let mut w = [0u32; 64];
    for (i, word) in block.as_chunks::<4>().0.iter().enumerate() {
        w[i] = u32::from_be_bytes([word[0], word[1], word[2], word[3]]);
    }
    for i in 16..64 {
        let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
        let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
        w[i] = w[i - 16]
            .wrapping_add(s0)
            .wrapping_add(w[i - 7])
            .wrapping_add(s1);
    }

    let (mut a, mut b, mut c, mut d) = (h[0], h[1], h[2], h[3]);
    let (mut e, mut f, mut g, mut hh) = (h[4], h[5], h[6], h[7]);

    for i in 0..64 {
        let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let ch = (e & f) ^ ((!e) & g);
        let t1 = hh
            .wrapping_add(s1)
            .wrapping_add(ch)
            .wrapping_add(K[i])
            .wrapping_add(w[i]);
        let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let maj = (a & b) ^ (a & c) ^ (b & c);
        let t2 = s0.wrapping_add(maj);

        hh = g;
        g = f;
        f = e;
        e = d.wrapping_add(t1);
        d = c;
        c = b;
        b = a;
        a = t1.wrapping_add(t2);
    }

    h[0] = h[0].wrapping_add(a);
    h[1] = h[1].wrapping_add(b);
    h[2] = h[2].wrapping_add(c);
    h[3] = h[3].wrapping_add(d);
    h[4] = h[4].wrapping_add(e);
    h[5] = h[5].wrapping_add(f);
    h[6] = h[6].wrapping_add(g);
    h[7] = h[7].wrapping_add(hh);
}

/// The raw thirty-two bytes.
///
/// Not public outside the crate. The hex form is what everything else
/// prints and compares; the raw bytes are for `agefile` (HMAC and HKDF,
/// 23 Sep), which needs them inside the crate and nowhere else.
pub(crate) fn sha256(data: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // The message, then a single one bit, then zeros, then the length in bits
    // as eight big-endian bytes. Written out rather than padded in place so
    // the boundary cases — a message of exactly fifty-five bytes, or exactly
    // sixty-four — are visible rather than implied.
    let mut message = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bits.to_be_bytes());

    for block in message.as_chunks::<64>().0 {
        compress(&mut h, block);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// The usual sixty-four lowercase hex characters.
pub fn sha256_hex(data: &[u8]) -> String {
    hex(&sha256(data))
}

/// The SHA-256 of a file, read a megabyte at a time.
///
/// For the downloads `atlas get` checks: a picture model is 2.5 GB, and
/// reading it whole to hash it (then copying it to pad it) asked a 16 GB
/// laptop for 5 GB at once.
pub fn sha256_file_hex(path: &std::path::Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let mut buf = vec![0u8; 1 << 20];
    let mut carry: Vec<u8> = Vec::with_capacity(64);
    let mut total: u64 = 0;
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        total += n as u64;
        let mut chunk = &buf[..n];
        if !carry.is_empty() {
            let need = 64 - carry.len();
            let take = need.min(chunk.len());
            carry.extend_from_slice(&chunk[..take]);
            chunk = &chunk[take..];
            if carry.len() == 64 {
                compress(&mut h, &carry);
                carry.clear();
            }
        }
        let whole = chunk.len() / 64 * 64;
        for block in chunk[..whole].as_chunks::<64>().0 {
            compress(&mut h, block);
        }
        carry.extend_from_slice(&chunk[whole..]);
    }
    let bits = total.wrapping_mul(8);
    carry.push(0x80);
    while carry.len() % 64 != 56 {
        carry.push(0);
    }
    carry.extend_from_slice(&bits.to_be_bytes());
    for block in carry.as_chunks::<64>().0 {
        compress(&mut h, block);
    }
    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    Ok(hex(&out))
}

fn hex(bytes: &[u8; 32]) -> String {
    let mut out = String::with_capacity(64);
    for &byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).unwrap_or('0'));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).unwrap_or('0'));
    }
    out
}

// ---------------------------------------------------------------------------
// Time, written one way everywhere
// ---------------------------------------------------------------------------

/// Seconds since the epoch as `YYYY-MM-DDTHH:MM:SS+00:00`.
///
/// Always UTC and always with the offset spelled out. A time written with no
/// zone reads as two different instants on two machines in two time zones,
/// and anything compared or fingerprinted from that text would disagree for
/// no visible reason.
pub fn iso_utc(seconds: u64) -> String {
    let days = (seconds / 86_400) as i64;
    let rest = seconds % 86_400;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}+00:00",
        rest / 3600,
        (rest % 3600) / 60,
        rest % 60
    )
}

/// Days since 1970 to a calendar date.
///
/// The era arithmetic rather than a loop over years: a loop is easy to read
/// and gets the century rules wrong under test at exactly the dates nobody
/// tries — 2000 is a leap year and 2100 is not, and both are in the fixture.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}
