//! Sending a file only one person can open: the `age` format, in house.
//!
//! **Sources:** the age v1 specification (C2SP, `age-encryption.org/v1`) —
//! header of recipient stanzas, an HMAC over it, and a STREAM of 64 KiB
//! ChaCha20-Poly1305 chunks; RFC 7748 (X25519: the Montgomery ladder over
//! GF(2^255−19), with the RFC's test vectors); RFC 5869 (HKDF) and RFC 2104
//! (HMAC) over the tree's own SHA-256 (`digest`); BIP 173 (bech32) for the
//! `age1…` and `AGE-SECRET-KEY-1…` strings. `FiloSottile/age` (BSD-3) and
//! `str4d/rage` (MIT/Apache-2.0) read as references. ChaCha20-Poly1305 is the
//! `chacha20poly1305` crate the vault already depends on. Clean-room.
//!
//! **Why Atlas wants it.** Sending a contract or a statement to a business
//! partner meant either email in the clear or a third-party service. An
//! `age1…` key is a line anyone can paste into an email; a file sealed to it
//! opens with their `age` (or their Atlas) and nothing else, offline, and
//! the real `age` tool reads what this writes (checked in the tests against
//! `age` 1.1.1).

use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};

// ---------------------------------------------------------------- X25519

type Fe = [u64; 5];
const MASK: u64 = (1 << 51) - 1;

fn fe_from(b: &[u8; 32]) -> Fe {
    let load = |i: usize| -> u64 {
        let mut x = 0u64;
        for k in 0..8 {
            if i + k < 32 {
                x |= (b[i + k] as u64) << (8 * k);
            }
        }
        x
    };
    [
        load(0) & MASK,
        (load(6) >> 3) & MASK,
        (load(12) >> 6) & MASK,
        (load(19) >> 1) & MASK,
        (load(24) >> 12) & MASK,
    ]
}

fn fe_carry(mut a: Fe) -> Fe {
    for _ in 0..2 {
        for i in 0..4 {
            a[i + 1] += a[i] >> 51;
            a[i] &= MASK;
        }
        a[0] += 19 * (a[4] >> 51);
        a[4] &= MASK;
    }
    a
}

fn fe_bytes(a: Fe) -> [u8; 32] {
    let mut a = fe_carry(a);
    // Fully reduce: subtract p if a >= p.
    let mut q = (a[0] + 19) >> 51;
    q = (a[1] + q) >> 51;
    q = (a[2] + q) >> 51;
    q = (a[3] + q) >> 51;
    q = (a[4] + q) >> 51;
    a[0] += 19 * q;
    for i in 0..4 {
        a[i + 1] += a[i] >> 51;
        a[i] &= MASK;
    }
    a[4] &= MASK;
    let mut out = [0u8; 32];
    let mut acc: u128 = 0;
    let mut bits = 0;
    let mut o = 0;
    for limb in a {
        acc |= (limb as u128) << bits;
        bits += 51;
        while bits >= 8 && o < 32 {
            out[o] = acc as u8;
            acc >>= 8;
            bits -= 8;
            o += 1;
        }
    }
    if o < 32 {
        out[o] = acc as u8;
    }
    out
}

fn fe_add(a: Fe, b: Fe) -> Fe {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3], a[4] + b[4]]
}

fn fe_sub(a: Fe, b: Fe) -> Fe {
    // Add 2p first so nothing underflows.
    let two_p: Fe = [0xFFFFFFFFFFFDA, 0xFFFFFFFFFFFFE, 0xFFFFFFFFFFFFE, 0xFFFFFFFFFFFFE, 0xFFFFFFFFFFFFE];
    fe_carry([a[0] + two_p[0] - b[0], a[1] + two_p[1] - b[1], a[2] + two_p[2] - b[2], a[3] + two_p[3] - b[3], a[4] + two_p[4] - b[4]])
}

fn fe_mul(a: Fe, b: Fe) -> Fe {
    let m = |x: u64, y: u64| x as u128 * y as u128;
    let b19 = [b[0], b[1] * 19, b[2] * 19, b[3] * 19, b[4] * 19];
    let r0 = m(a[0], b[0]) + m(a[1], b19[4]) + m(a[2], b19[3]) + m(a[3], b19[2]) + m(a[4], b19[1]);
    let r1 = m(a[0], b[1]) + m(a[1], b[0]) + m(a[2], b19[4]) + m(a[3], b19[3]) + m(a[4], b19[2]);
    let r2 = m(a[0], b[2]) + m(a[1], b[1]) + m(a[2], b[0]) + m(a[3], b19[4]) + m(a[4], b19[3]);
    let r3 = m(a[0], b[3]) + m(a[1], b[2]) + m(a[2], b[1]) + m(a[3], b[0]) + m(a[4], b19[4]);
    let r4 = m(a[0], b[4]) + m(a[1], b[3]) + m(a[2], b[2]) + m(a[3], b[1]) + m(a[4], b[0]);
    let mut r = [r0, r1, r2, r3, r4];
    for i in 0..4 {
        r[i + 1] += r[i] >> 51;
        r[i] &= MASK as u128;
    }
    let c = r[4] >> 51;
    r[4] &= MASK as u128;
    r[0] += c * 19;
    fe_carry([r[0] as u64, r[1] as u64, r[2] as u64, r[3] as u64, r[4] as u64])
}

fn fe_sq(a: Fe) -> Fe {
    fe_mul(a, a)
}

fn fe_mul_small(a: Fe, k: u64) -> Fe {
    fe_mul(a, [k, 0, 0, 0, 0])
}

fn fe_invert(a: Fe) -> Fe {
    // a^(p-2), p-2 = 2^255 - 21: square-and-multiply over the bits.
    let mut result: Fe = [1, 0, 0, 0, 0];
    let e: [u8; 32] = {
        let mut e = [0xFFu8; 32];
        e[0] = 0xEB;
        e[31] = 0x7F;
        e
    };
    for i in (0..255).rev() {
        result = fe_sq(result);
        if (e[i / 8] >> (i % 8)) & 1 == 1 {
            result = fe_mul(result, a);
        }
    }
    result
}

fn cswap(swap: u64, a: &mut Fe, b: &mut Fe) {
    let mask = 0u64.wrapping_sub(swap);
    for i in 0..5 {
        let t = mask & (a[i] ^ b[i]);
        a[i] ^= t;
        b[i] ^= t;
    }
}

/// RFC 7748 X25519(k, u).
fn x25519(k: &[u8; 32], u: &[u8; 32]) -> [u8; 32] {
    let mut k = *k;
    k[0] &= 248;
    k[31] &= 127;
    k[31] |= 64;
    let mut u = *u;
    u[31] &= 127;
    let x1 = fe_from(&u);
    let (mut x2, mut z2, mut x3, mut z3): (Fe, Fe, Fe, Fe) = ([1, 0, 0, 0, 0], [0; 5], x1, [1, 0, 0, 0, 0]);
    let mut swap = 0u64;
    for t in (0..255).rev() {
        let kt = ((k[t / 8] >> (t % 8)) & 1) as u64;
        swap ^= kt;
        cswap(swap, &mut x2, &mut x3);
        cswap(swap, &mut z2, &mut z3);
        swap = kt;
        let a = fe_carry(fe_add(x2, z2));
        let aa = fe_sq(a);
        let b = fe_sub(x2, z2);
        let bb = fe_sq(b);
        let e = fe_sub(aa, bb);
        let c = fe_carry(fe_add(x3, z3));
        let d = fe_sub(x3, z3);
        let da = fe_mul(d, a);
        let cb = fe_mul(c, b);
        x3 = fe_sq(fe_carry(fe_add(da, cb)));
        z3 = fe_mul(x1, fe_sq(fe_sub(da, cb)));
        x2 = fe_mul(aa, bb);
        z2 = fe_mul(e, fe_carry(fe_add(aa, fe_mul_small(e, 121665))));
    }
    cswap(swap, &mut x2, &mut x3);
    cswap(swap, &mut z2, &mut z3);
    fe_bytes(fe_mul(x2, fe_invert(z2)))
}

const BASEPOINT: [u8; 32] = {
    let mut b = [0u8; 32];
    b[0] = 9;
    b
};

// ---------------------------------------------------------------- HMAC / HKDF

fn hmac(key: &[u8], msg: &[u8]) -> [u8; 32] {
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&crate::digest::sha256(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner: Vec<u8> = k.iter().map(|b| b ^ 0x36).collect();
    inner.extend_from_slice(msg);
    let ih = crate::digest::sha256(&inner);
    let mut outer: Vec<u8> = k.iter().map(|b| b ^ 0x5c).collect();
    outer.extend_from_slice(&ih);
    crate::digest::sha256(&outer)
}

fn hkdf(salt: &[u8], ikm: &[u8], info: &[u8], len: usize) -> Vec<u8> {
    let prk = hmac(if salt.is_empty() { &[0u8; 32] } else { salt }, ikm);
    let mut out = Vec::new();
    let mut t: Vec<u8> = Vec::new();
    let mut i = 1u8;
    while out.len() < len {
        let mut m = t.clone();
        m.extend_from_slice(info);
        m.push(i);
        t = hmac(&prk, &m).to_vec();
        out.extend_from_slice(&t);
        i += 1;
    }
    out.truncate(len);
    out
}

// ---------------------------------------------------------------- bech32 / base64

const CHARSET: &[u8] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";

fn polymod(values: &[u8]) -> u32 {
    let gen = [0x3b6a57b2u32, 0x26508e6d, 0x1ea119fa, 0x3d4233dd, 0x2a1462b3];
    let mut chk = 1u32;
    for v in values {
        let b = chk >> 25;
        chk = ((chk & 0x1ffffff) << 5) ^ *v as u32;
        for (i, g) in gen.iter().enumerate() {
            if (b >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

fn hrp_expand(hrp: &str) -> Vec<u8> {
    let mut v: Vec<u8> = hrp.bytes().map(|c| c >> 5).collect();
    v.push(0);
    v.extend(hrp.bytes().map(|c| c & 31));
    v
}

fn to5(data: &[u8]) -> Vec<u8> {
    let (mut acc, mut bits, mut out) = (0u32, 0u32, Vec::new());
    for b in data {
        acc = (acc << 8) | *b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(((acc >> bits) & 31) as u8);
        }
    }
    if bits > 0 {
        out.push(((acc << (5 - bits)) & 31) as u8);
    }
    out
}

fn from5(data: &[u8]) -> Option<Vec<u8>> {
    let (mut acc, mut bits, mut out) = (0u32, 0u32, Vec::new());
    for v in data {
        acc = (acc << 5) | *v as u32;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    (bits < 5 && (acc & ((1 << bits) - 1)) == 0).then_some(out)
}

fn bech32_encode(hrp: &str, data: &[u8]) -> String {
    let d5 = to5(data);
    let mut v = hrp_expand(hrp);
    v.extend(&d5);
    v.extend([0u8; 6]);
    let pm = polymod(&v) ^ 1;
    let mut s = format!("{hrp}1");
    for x in d5.iter().chain((0..6).map(|i| ((pm >> (5 * (5 - i))) & 31) as u8).collect::<Vec<_>>().iter()) {
        s.push(CHARSET[*x as usize] as char);
    }
    s
}

fn bech32_decode(s: &str) -> Option<(String, Vec<u8>)> {
    let lower = s.to_lowercase();
    let pos = lower.rfind('1')?;
    let (hrp, rest) = (&lower[..pos], &lower[pos + 1..]);
    if rest.len() < 6 {
        return None;
    }
    let vals: Vec<u8> = rest.bytes().map(|c| CHARSET.iter().position(|x| *x == c).map(|p| p as u8)).collect::<Option<_>>()?;
    let mut v = hrp_expand(hrp);
    v.extend(&vals);
    if polymod(&v) != 1 {
        return None;
    }
    Some((hrp.to_string(), from5(&vals[..vals.len() - 6])?))
}

const B64: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn b64(data: &[u8]) -> String {
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..c.len() + 1 {
            s.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    s
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    if s.contains('=') || s.len() % 4 == 1 {
        return None; // age uses unpadded base64 only
    }
    let vals: Vec<u32> = s.bytes().map(|c| B64.iter().position(|x| *x == c).map(|p| p as u32)).collect::<Option<_>>()?;
    let mut out = Vec::new();
    for c in vals.chunks(4) {
        let n = c.iter().enumerate().fold(0u32, |a, (i, v)| a | v << (18 - 6 * i));
        for i in 0..c.len() - 1 {
            out.push((n >> (16 - 8 * i)) as u8);
        }
    }
    // Canonical: re-encoding must give the same text.
    (b64(&out) == s).then_some(out)
}

// ---------------------------------------------------------------- keys

fn random32() -> [u8; 32] {
    use chacha20poly1305::aead::rand_core::RngCore;
    let mut k = [0u8; 32];
    chacha20poly1305::aead::OsRng.fill_bytes(&mut k);
    k
}

/// A new identity: (`AGE-SECRET-KEY-1…`, `age1…`).
pub fn new_identity() -> (String, String) {
    let sk = random32();
    identity_strings(&sk)
}

fn identity_strings(sk: &[u8; 32]) -> (String, String) {
    let pk = x25519(sk, &BASEPOINT);
    (bech32_encode("age-secret-key-", sk).to_uppercase(), bech32_encode("age", &pk))
}

/// The `age1…` recipient an `AGE-SECRET-KEY-1…` identity answers to.
pub fn recipient_of(identity: &str) -> Result<String, String> {
    Ok(identity_strings(&secret_key(identity)?).1)
}

fn secret_key(identity: &str) -> Result<[u8; 32], String> {
    let (hrp, data) = bech32_decode(identity.trim()).ok_or("that isn't an age secret key")?;
    if hrp != "age-secret-key-" || data.len() != 32 {
        return Err("that isn't an age secret key".into());
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&data);
    Ok(k)
}

fn public_key(recipient: &str) -> Result<[u8; 32], String> {
    let (hrp, data) = bech32_decode(recipient.trim()).ok_or_else(|| format!("\"{}\" isn't an age1… key", recipient.trim()))?;
    if hrp != "age" || data.len() != 32 {
        return Err(format!("\"{}\" isn't an age1… key", recipient.trim()));
    }
    let mut k = [0u8; 32];
    k.copy_from_slice(&data);
    Ok(k)
}

// ---------------------------------------------------------------- the file

const INTRO: &str = "age-encryption.org/v1";
const CHUNK: usize = 64 * 1024;

fn aead(key: &[u8], nonce: &[u8; 12], data: &[u8], seal: bool) -> Result<Vec<u8>, String> {
    let c = ChaCha20Poly1305::new(Key::from_slice(key));
    let n = Nonce::from_slice(nonce);
    let p = Payload { msg: data, aad: &[] };
    if seal { c.encrypt(n, p) } else { c.decrypt(n, p) }.map_err(|_| "it didn't authenticate".to_string())
}

fn wrap_lines(s: &str) -> String {
    // Stanza bodies: 64 columns, and always a final line shorter than 64.
    let mut out = String::new();
    let b = s.as_bytes();
    let mut i = 0;
    loop {
        let end = (i + 64).min(b.len());
        out.push_str(&s[i..end]);
        out.push('\n');
        if end - i < 64 {
            break;
        }
        i = end;
    }
    out
}

/// Seal `plain` so each of `recipients` (`age1…`) can open it.
pub fn seal(plain: &[u8], recipients: &[String]) -> Result<Vec<u8>, String> {
    if recipients.is_empty() {
        return Err("seal it to whom? — an age1… key".into());
    }
    let file_key: [u8; 16] = random32()[..16].try_into().unwrap_or([0; 16]);
    let mut header = format!("{INTRO}\n");
    for r in recipients {
        let pk = public_key(r)?;
        let e = random32();
        let epk = x25519(&e, &BASEPOINT);
        let shared = x25519(&e, &pk);
        if shared.iter().all(|b| *b == 0) {
            return Err(format!("\"{r}\" is a key no one can open"));
        }
        let mut salt = epk.to_vec();
        salt.extend_from_slice(&pk);
        let wrap = hkdf(&salt, &shared, b"age-encryption.org/v1/X25519", 32);
        let body = aead(&wrap, &[0u8; 12], &file_key, true)?;
        header.push_str(&format!("-> X25519 {}\n", b64(&epk)));
        header.push_str(&wrap_lines(&b64(&body)));
    }
    header.push_str("---");
    let mac = hmac(&hkdf(&[], &file_key, b"header", 32), header.as_bytes());
    header.push_str(&format!(" {}\n", b64(&mac)));
    let nonce: [u8; 16] = random32()[..16].try_into().unwrap_or([0; 16]);
    let pkey = hkdf(&nonce, &file_key, b"payload", 32);
    let mut out = header.into_bytes();
    out.extend_from_slice(&nonce);
    let chunks: Vec<&[u8]> = if plain.is_empty() { vec![&[][..]] } else { plain.chunks(CHUNK).collect() };
    for (i, c) in chunks.iter().enumerate() {
        let mut n = [0u8; 12];
        n[3..11].copy_from_slice(&(i as u64).to_be_bytes());
        n[11] = (i + 1 == chunks.len()) as u8;
        out.extend(aead(&pkey, &n, c, true)?);
    }
    Ok(out)
}

/// Open an age file with an `AGE-SECRET-KEY-1…` identity.
pub fn open(sealed: &[u8], identity: &str) -> Result<Vec<u8>, String> {
    let sk = secret_key(identity)?;
    let my_pk = x25519(&sk, &BASEPOINT);
    // The header is text up to and including the "--- <mac>\n" line.
    let end = sealed.windows(4).position(|w| w == b"\n---").ok_or("not an age file")? + 1;
    let mac_line_end = sealed[end..].iter().position(|b| *b == b'\n').ok_or("not an age file")? + end;
    let head = std::str::from_utf8(&sealed[..end]).map_err(|_| "not an age file")?;
    let mac_line = std::str::from_utf8(&sealed[end..mac_line_end]).map_err(|_| "not an age file")?;
    let mut lines = head.lines();
    if lines.next() != Some(INTRO) {
        return Err("not an age v1 file".into());
    }
    let mut file_key: Option<[u8; 16]> = None;
    let mut stanza: Option<Vec<String>> = None;
    let mut bodies: Vec<(Vec<String>, String)> = Vec::new();
    let mut body = String::new();
    for l in lines {
        if let Some(rest) = l.strip_prefix("-> ") {
            if let Some(s) = stanza.take() {
                bodies.push((s, std::mem::take(&mut body)));
            }
            stanza = Some(rest.split(' ').map(String::from).collect());
        } else {
            body.push_str(l);
        }
    }
    if let Some(s) = stanza.take() {
        bodies.push((s, body));
    }
    for (args, body) in &bodies {
        if args.first().map(String::as_str) != Some("X25519") || args.len() != 2 {
            continue; // someone else's kind of recipient
        }
        let epk: [u8; 32] = unb64(&args[1]).and_then(|v| v.try_into().ok()).ok_or("a damaged recipient line")?;
        let shared = x25519(&sk, &epk);
        let mut salt = epk.to_vec();
        salt.extend_from_slice(&my_pk);
        let wrap = hkdf(&salt, &shared, b"age-encryption.org/v1/X25519", 32);
        let wrapped = unb64(body).ok_or("a damaged recipient line")?;
        if let Ok(k) = aead(&wrap, &[0u8; 12], &wrapped, false) {
            file_key = k.try_into().ok();
            break;
        }
    }
    let file_key = file_key.ok_or("it isn't sealed to this key")?;
    let mac = unb64(mac_line.strip_prefix("--- ").ok_or("not an age file")?).ok_or("a damaged header")?;
    let want = hmac(&hkdf(&[], &file_key, b"header", 32), &sealed[..end + 3]);
    if mac != want {
        return Err("the header was changed after it was sealed".into());
    }
    let rest = &sealed[mac_line_end + 1..];
    if rest.len() < 16 {
        return Err("the file is cut short".into());
    }
    let pkey = hkdf(&rest[..16], &file_key, b"payload", 32);
    let data = &rest[16..];
    let mut out = Vec::new();
    let pieces: Vec<&[u8]> = data.chunks(CHUNK + 16).collect();
    for (i, c) in pieces.iter().enumerate() {
        let mut n = [0u8; 12];
        n[3..11].copy_from_slice(&(i as u64).to_be_bytes());
        n[11] = (i + 1 == pieces.len()) as u8;
        out.extend(aead(&pkey, &n, c, false).map_err(|_| "the contents were changed or cut short".to_string())?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex32(s: &str) -> [u8; 32] {
        let v: Vec<u8> = (0..32).map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap()).collect();
        v.try_into().unwrap()
    }

    #[test]
    fn agefile_x25519_matches_rfc_7748() {
        // §5.2 test vectors.
        let k = hex32("a546e36bf0527c9d3b16154b82465edd62144c0ac1fc5a18506a2244ba449ac4");
        let u = hex32("e6db6867583030db3594c1a424b15f7c726624ec26b3353b10a903a6d0ab1c4c");
        assert_eq!(x25519(&k, &u), hex32("c3da55379de9c6908e94ea4df28d084f32eccf03491c71f754b4075577a28552"));
        let k = hex32("4b66e9d4d1b4673c5ad22691957d6af5c11b6421e0ea01d42ca4169e7918ba0d");
        let u = hex32("e5210f12786811d3f4b7959d0538ae2c31dbe7106fc03c3efc4cd549c715a493");
        assert_eq!(x25519(&k, &u), hex32("95cbde9476e8907d7aade45cb4b873f88b595a68799fa152e6f8f7647aac7957"));
        // §6.1 Diffie–Hellman: both sides reach the same secret.
        let a = hex32("77076d0a7318a57d3c16c17251b26645df4c2f87ebc0992ab177fba51db92c2a");
        let b = hex32("5dab087e624a8a4b79e17f8b83800ee66f3bb1292618b6fd1c2f8b27ff88e0eb");
        let mut nine = [0u8; 32];
        nine[0] = 9;
        let (pa, pb) = (x25519(&a, &nine), x25519(&b, &nine));
        assert_eq!(pa, hex32("8520f0098930a754748b7ddcb43ef75a0dbf3a0d26381af4eba4a98eaa9b4e6a"));
        assert_eq!(pb, hex32("de9edb7d7b7dc1b4d35b61c2ece435373f8343c85b78674dadfc7e146f882b4f"));
        let shared = hex32("4a5d9d5ba4ce2de1728e3bf480350f25e07e21c947d19e3376f09b3c1e161742");
        assert_eq!(x25519(&a, &pb), shared);
        assert_eq!(x25519(&b, &pa), shared);
        // §5.2, iterated 1,000 times.
        let (mut k, mut u) = (nine, nine);
        for _ in 0..1000 {
            let r = x25519(&k, &u);
            u = k;
            k = r;
        }
        assert_eq!(k, hex32("684cf59ba83309552800ef566f2f4d3c1c3887c49360e3875f2eb94d99532c51"));
    }
}
