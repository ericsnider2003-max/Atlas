//! Minimal base64 encoder. Screenshots go to vision models as base64; pulling
//! a crate in for 30 lines of table lookup isn't worth the dependency.

const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let b = [c[0], *c.get(1).unwrap_or(&0), *c.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(A[(n >> 18 & 63) as usize] as char);
        out.push(A[(n >> 12 & 63) as usize] as char);
        out.push(if c.len() > 1 { A[(n >> 6 & 63) as usize] as char } else { '=' });
        out.push(if c.len() > 2 { A[(n & 63) as usize] as char } else { '=' });
    }
    out
}

/// The inverse of `encode`. Needed for exactly one thing so far: proving
/// in a test that `msoauth::xoauth2_string` built what it claims to,
/// without trusting the encoder to check its own work.
pub fn decode(s: &str) -> Result<Vec<u8>, &'static str> {
    fn val(c: u8) -> Option<u8> {
        A.iter().position(|&a| a == c).map(|p| p as u8)
    }
    let s = s.trim_end_matches('=');
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let bytes = s.as_bytes();
    for chunk in bytes.chunks(4) {
        let mut vals = [0u8; 4];
        for (i, &c) in chunk.iter().enumerate() {
            vals[i] = val(c).ok_or("invalid base64 character")?;
        }
        let n = (vals[0] as u32) << 18 | (vals[1] as u32) << 12 | (vals[2] as u32) << 6 | vals[3] as u32;
        out.push((n >> 16 & 0xff) as u8);
        if chunk.len() > 2 {
            out.push((n >> 8 & 0xff) as u8);
        }
        if chunk.len() > 3 {
            out.push((n & 0xff) as u8);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_is_the_real_inverse_of_encode_for_arbitrary_bytes() {
        for sample in [b"".as_slice(), b"a", b"ab", b"abc", b"hello, world!", &[0, 1, 2, 255, 254]] {
            let encoded = encode(sample);
            let decoded = decode(&encoded).unwrap();
            assert_eq!(decoded, sample, "round-trip failed for {sample:?}");
        }
    }

    #[test]
    fn decode_rejects_an_invalid_character_rather_than_producing_garbage() {
        assert!(decode("not valid base64!!").is_err());
    }
}
