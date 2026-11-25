//! Household key, HMAC-SHA256 and hex helpers.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

pub const KEY_LEN: usize = 32;

/// A fresh random household key.
pub fn new_key() -> [u8; KEY_LEN] {
    let mut k = [0u8; KEY_LEN];
    getrandom::getrandom(&mut k).expect("no system randomness");
    k
}

/// A random device id (16 hex characters).
pub fn new_device_id() -> String {
    let mut b = [0u8; 8];
    getrandom::getrandom(&mut b).expect("no system randomness");
    hex(&b)
}

pub fn mac(key: &[u8], data: &[u8]) -> [u8; 32] {
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC takes any key length");
    m.update(data);
    m.finalize().into_bytes().into()
}

/// Constant-time check of a MAC.
pub fn verify(key: &[u8], data: &[u8], tag: &[u8]) -> bool {
    let mut m = HmacSha256::new_from_slice(key).expect("HMAC takes any key length");
    m.update(data);
    m.verify_slice(tag).is_ok()
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    Sha256::digest(data).into()
}

pub fn hex(b: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(D[(x >> 4) as usize] as char);
        s.push(D[(x & 15) as usize] as char);
    }
    s
}

pub fn unhex(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Parses a pairing code into a household key.
pub fn parse_key(s: &str) -> Option<Vec<u8>> {
    let k = unhex(&s.replace(['-', ' '], ""))?;
    (k.len() == KEY_LEN).then_some(k)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trip() {
        let k = new_key();
        assert_eq!(unhex(&hex(&k)).unwrap(), k);
        assert_eq!(parse_key(&hex(&k)).unwrap(), k);
        assert!(parse_key("abcd").is_none());
        assert!(unhex("zz").is_none());
    }

    #[test]
    fn mac_verifies_and_rejects() {
        let k = new_key();
        let t = mac(&k, b"hello");
        assert!(verify(&k, b"hello", &t));
        assert!(!verify(&k, b"hellp", &t));
        assert!(!verify(&new_key(), b"hello", &t));
    }
}

/// URL-safe base64 without padding (Android's provisioning checksums use it).
pub fn base64url(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..=c.len() {
            out.push(A[(n >> (18 - 6 * i)) as usize & 63] as char);
        }
    }
    out
}

#[cfg(test)]
mod b64 {
    #[test]
    fn base64url_matches_rfc4648() {
        for (i, o) in [("", ""), ("f", "Zg"), ("fo", "Zm8"), ("foo", "Zm9v"), ("foob", "Zm9vYg"), ("fooba", "Zm9vYmE"), ("foobar", "Zm9vYmFy")] {
            assert_eq!(super::base64url(i.as_bytes()), o);
        }
        assert_eq!(super::base64url(&[0xfb, 0xff]), "-_8");
    }
}
