//! Text forms of `Bytes`: hex, base64 (padded) and base64url (unpadded), shared by the
//! interpreter, JSON and native values. The C runtime section `bytes` in `std_rt.c` implements
//! the same rules for native code.

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}

/// How a `Bytes` value shows: `0x` and its hex digits.
pub fn show(b: &[u8]) -> String {
    format!("0x{}", hex(b))
}

fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

/// Upper or lower case hex digits, an even number of them.
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    let s = s.as_bytes();
    if s.len() % 2 != 0 {
        return None;
    }
    s.chunks(2).map(|p| Some(nibble(p[0])? << 4 | nibble(p[1])?)).collect()
}

/// Standard base64 with `=` padding, or base64url without it.
pub fn base64(b: &[u8], url: bool) -> String {
    let a = if url { URL } else { STD };
    let mut s = String::with_capacity(b.len().div_ceil(3) * 4);
    for c in b.chunks(3) {
        let n = (u32::from(c[0]) << 16) | (u32::from(*c.get(1).unwrap_or(&0)) << 8) | u32::from(*c.get(2).unwrap_or(&0));
        let k = c.len() + 1;
        for i in 0..4 {
            if i < k {
                s.push(a[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else if !url {
                s.push('=');
            }
        }
    }
    s
}

/// The inverse of `base64`: padding is optional, but when present it must be complete, and
/// the unused low bits of the last character must be zero, so each value has one text form.
pub fn from_base64(s: &str, url: bool) -> Option<Vec<u8>> {
    let a = if url { URL } else { STD };
    let mut t = s.as_bytes();
    if t.ends_with(b"=") {
        if t.len() % 4 != 0 {
            return None;
        }
        t = t.strip_suffix(b"==").or_else(|| t.strip_suffix(b"=")).unwrap_or(t);
    }
    if t.len() % 4 == 1 {
        return None;
    }
    let mut out = Vec::with_capacity(t.len() * 3 / 4);
    for c in t.chunks(4) {
        let mut n = 0u32;
        for (i, &ch) in c.iter().enumerate() {
            let v = a.iter().position(|&x| x == ch)? as u32;
            n |= v << (18 - 6 * i);
        }
        let k = c.len() - 1;
        let mask = match k {
            1 => 0xffff,
            2 => 0xff,
            _ => 0,
        };
        if n & mask != 0 {
            return None;
        }
        out.extend_from_slice(&n.to_be_bytes()[1..=k]);
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4648_vectors_round_trip() {
        for (plain, b64) in [("", ""), ("f", "Zg=="), ("fo", "Zm8="), ("foo", "Zm9v"), ("foob", "Zm9vYg=="), ("fooba", "Zm9vYmE="), ("foobar", "Zm9vYmFy")] {
            assert_eq!(base64(plain.as_bytes(), false), b64);
            assert_eq!(from_base64(b64, false).unwrap(), plain.as_bytes());
            assert_eq!(from_base64(b64.trim_end_matches('='), false).unwrap(), plain.as_bytes());
            assert_eq!(base64(plain.as_bytes(), true), b64.trim_end_matches('='));
        }
        assert_eq!(base64(&[0xfb, 0xff], false), "+/8=");
        assert_eq!(base64(&[0xfb, 0xff], true), "-_8");
        assert_eq!(from_base64("-_8", true).unwrap(), vec![0xfb, 0xff]);
        for bad in ["Zg=", "Z", "Zh==", "Zm9", "Zm9v=", "Zg==Zg==", "Zm 9v", "+/8"] {
            assert!(from_base64(bad, bad == "+/8").is_none(), "{bad}");
        }
        assert_eq!(hex(&[0, 0xab, 0x10]), "00ab10");
        assert_eq!(from_hex("00AB10").unwrap(), vec![0, 0xab, 0x10]);
        assert!(from_hex("abc").is_none() && from_hex("zz").is_none());
    }
}
